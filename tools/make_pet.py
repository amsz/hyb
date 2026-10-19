#!/usr/bin/env python3
"""把一条 mp4 制作成护眼宝宠物资源。

产出 <out>/<id>/ 四件套（对应 animals.rs 的 load_animal + petstore.rs 的 PET_FILES）：

    meta.json      元数据（species 必须在白名单内）
    enter.webm     出场动画，带 alpha（VP9 + Matroska BlockAdditional）
    idle.webm      待机循环；素材不够长时退回复用 enter（前端会自动 loop）
    thumbnail.png  卡片缩略图（72x58 显示区，输出 144 宽 = 2x）

抠像：mp4（H.264/HEVC）没有 alpha 通道，只有当背景是一块**近似纯色**时，
用 colorkey 抠成透明；背景不均匀则只能带矩形背景显示（会很难看），除非
你显式给 --key。若输入本身带 alpha（mov/prores 4444 等）则直接转封。

用法示例：
    python tools/make_pet.py clip.mp4 --id goose2 --name 鹅二号 --species goose
    python tools/make_pet.py clip.mp4 --id fox1 --name 狐狸 --key 0x00FF00 --enter 0:3 --idle 3:0 --pingpong
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# animals.rs: SPECIES_ALLOWLIST
ALLOWLIST = ["cat", "dog", "panda", "duck", "chicken", "fish", "rabbit", "fox", "bird", "goose"]
# petstore.rs: is_plausible_id
ID_RE = re.compile(r"^[a-z0-9_-]{1,63}$")
# 官方宠物规格（tabbycat 等）：1280x720
DEFAULT_MAX_WIDTH = 1280


def die(msg: str) -> "sys.NoReturn":
    print(f"[make_pet] 错误: {msg}", file=sys.stderr)
    sys.exit(1)


def run(cmd: list[str], quiet: bool = True) -> subprocess.CompletedProcess:
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if p.returncode != 0:
        tail = "\n".join((p.stderr or "").strip().splitlines()[-8:])
        die(f"命令失败: {' '.join(cmd[:6])} …\n{tail}")
    return p


def probe(path: Path) -> dict:
    p = run([
        "ffprobe", "-v", "error", "-show_streams", "-show_format",
        "-of", "json", str(path),
    ])
    data = json.loads(p.stdout)
    v = next((s for s in data.get("streams", []) if s.get("codec_type") == "video"), None)
    if not v:
        die(f"{path} 里没有视频流")
    return {
        "pix_fmt": v.get("pix_fmt", ""),
        "width": int(v.get("width", 0)),
        "height": int(v.get("height", 0)),
        "duration": float(data.get("format", {}).get("duration") or v.get("duration") or 0),
        "fps": eval_fps(v.get("r_frame_rate", "30/1")),
    }


def eval_fps(s: str) -> float:
    try:
        n, d = s.split("/")
        return float(n) / float(d) or 30.0
    except Exception:
        return 30.0


def first_frame_rgb(path: Path, width: int = 128) -> tuple[bytes, int, int]:
    """取第一帧缩成小图的 rgb24 原始数据，用于背景纯度检测（必须二进制读）。"""
    proc = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", str(path), "-vframes", "1",
         "-vf", f"scale={width}:-2", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
        capture_output=True)
    if proc.returncode != 0:
        die("读取首帧失败")
    raw = proc.stdout
    w = width
    h = len(raw) // (w * 3)
    return raw, w, h


def guess_key_color(path: Path) -> str | None:
    """四角取样：每块色差小且彼此接近 → 判定为纯色背景，返回 0xRRGGBB。"""
    raw, w, h = first_frame_rgb(path)
    if h < 8:
        return None
    p = max(2, w // 16)
    patches = []
    for x0, y0 in [(0, 0), (w - p, 0), (0, h - p), (w - p, h - p)]:
        px = []
        for y in range(y0, y0 + p):
            for x in range(x0, x0 + p):
                i = (y * w + x) * 3
                px.append(raw[i:i + 3])
        spread = max(max(c) - min(c) for c in zip(*px))
        mean = tuple(sum(c) for c in zip(*px))
        mean = tuple(m // len(px) for m in mean)
        patches.append((spread, mean))
    if any(spread > 16 for spread, _ in patches):
        return None
    if max(max(abs(a - b) for a, b in zip(m1, m2)) for _, m1 in patches for _, m2 in patches) > 24:
        return None
    r = sum(m[0] for _, m in patches) // 4
    g = sum(m[1] for _, m in patches) // 4
    b = sum(m[2] for _, m in patches) // 4
    return f"0x{r:02X}{g:02X}{b:02X}"


def parse_time(s: str, default: float) -> float:
    if not s:
        return default
    if ":" in s:
        m, sec = s.rsplit(":", 1)
        return float(m) * 60 + float(sec)
    return float(s)


def vf_chain(key: str | None, similarity: float, blend: float, target_w: int, fps: float | None) -> str:
    parts = []
    if key:
        parts.append(f"colorkey={key}:{similarity}:{blend}")
    parts.append(f"scale={target_w}:-2")
    if fps:
        parts.append(f"fps={fps}")
    parts.append("format=yuva420p" if key else "format=yuv420p")
    return ",".join(parts)


def encode_vp9(src: Path, dst: Path, start: float, dur: float | None,
               vf: str, crf: int, pingpong: bool) -> None:
    common_out = [
        "-an", "-c:v", "libvpx-vp9", "-b:v", "0", f"-crf", str(crf),
        "-row-mt", "1", "-deadline", "good", "-cpu-used", "2",
    ]
    if pingpong:
        trim = f"trim=start={start}" + (f":end={start + dur}" if dur else "")
        fc = (
            f"[0:v]{trim},setpts=PTS-STARTPTS,{vf}[f];"
            f"[f]split[a][b];[b]reverse[r];[a][r]concat=n=2:v=1[o]"
        )
        run(["ffmpeg", "-y", "-v", "error", "-i", str(src),
             "-filter_complex", fc, "-map", "[o]", *common_out, str(dst)])
    else:
        cmd = ["ffmpeg", "-y", "-v", "error"]
        if start > 0:
            cmd += ["-ss", str(start)]
        cmd += ["-i", str(src)]
        if dur:
            cmd += ["-t", str(dur)]
        cmd += ["-vf", vf, *common_out, str(dst)]
        run(cmd)


def make_thumbnail(src: Path, dst: Path, at: float, key: str | None,
                   similarity: float, blend: float, width: int = 144) -> None:
    vf = (f"colorkey={key}:{similarity}:{blend}," if key else "") + \
        f"scale={width}:-2,format=rgba"
    run(["ffmpeg", "-y", "-v", "error", "-ss", str(at), "-i", str(src),
         "-vf", vf, "-vframes", "1", str(dst)])


def has_alpha(path: Path) -> bool:
    p = run(["ffprobe", "-v", "error", "-select_streams", "v", "-show_packets",
             "-show_entries", "packet=side_data_list", "-of", "compact", str(path)])
    return "matroska_blockadditional" in p.stdout


def parse_args() -> argparse.Namespace:
    ap = argparse.ArgumentParser(
        description="把 mp4 制作成护眼宝宠物（meta.json + enter.webm + idle.webm + thumbnail.png）",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter)
    ap.add_argument("input", type=Path, help="输入 mp4（或其他 ffmpeg 能读的视频）")
    ap.add_argument("--id", required=True, help="宠物 id，仅 [a-z0-9_-]，<64 字符")
    ap.add_argument("--name", required=True, help="显示名")
    ap.add_argument("--species", required=True, choices=ALLOWLIST, help="物种（白名单内）")
    ap.add_argument("--description", default="", help="卡片描述")
    ap.add_argument("--rarity", default="common", help="稀有度（纯展示文本）")
    ap.add_argument("--personality", default="calm", help="性格（纯展示文本）")
    ap.add_argument("--bg-color", default="#0B0E0D", help="卡片缩略图底色")
    ap.add_argument("--anchor-x", type=float, default=0.5, help="屏幕水平位置 0~1")
    ap.add_argument("--scale", type=float, default=0.78, help="显示大小系数")
    ap.add_argument("--key", default="auto",
                    help="抠像背景色 auto|none|0xRRGGBB；auto=检测四角纯色")
    ap.add_argument("--key-similarity", type=float, default=0.30, help="colorkey 相似度")
    ap.add_argument("--key-blend", type=float, default=0.12, help="colorkey 边缘混合")
    ap.add_argument("--enter", default="", metavar="START:DUR",
                    help="出场段，如 0:3（默认 0 起 3 秒，不足则取整段）")
    ap.add_argument("--idle", default="", metavar="START:DUR",
                    help="待机段（默认接在出场段后面到结尾）")
    ap.add_argument("--pingpong", action="store_true",
                    help="待机段正放+倒放拼接，循环接缝更自然")
    ap.add_argument("--crf", type=int, default=32, help="VP9 质量（15~40，越小越好）")
    ap.add_argument("--fps", type=float, default=None, help="输出帧率（默认跟随源）")
    ap.add_argument("--max-width", type=int, default=DEFAULT_MAX_WIDTH, help="输出宽度上限")
    ap.add_argument("--out", type=Path, default=None,
                    help="输出根目录（默认 %%APPDATA%%\\top.huyanbao.dev\\pets）")
    return ap.parse_args()


def main() -> None:
    # Windows 控制台默认 cp936，管道捕获时中文会乱码；统一走 UTF-8
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except Exception:
            pass
    if not shutil.which("ffmpeg") or not shutil.which("ffprobe"):
        die("找不到 ffmpeg/ffprobe，请先安装并加入 PATH")
    a = parse_args()

    if not ID_RE.match(a.id):
        die(f"--id {a.id!r} 不合法：只允许 [a-z0-9_-] 且 <64 字符（petstore.rs: is_plausible_id）")
    if not a.input.is_file():
        die(f"输入文件不存在: {a.input}")

    info = probe(a.input)
    dur = info["duration"]
    print(f"[make_pet] 源: {info['width']}x{info['height']} {info['fps']:.5g}fps "
          f"{dur:.2f}s pix_fmt={info['pix_fmt']}")

    # --- 抠像策略 -------------------------------------------------------
    src_alpha = "a" in info["pix_fmt"]  # ya??/rgba 等自带 alpha
    if a.key.lower() == "none":
        key = None
    elif src_alpha:
        key = None
        print("[make_pet] 源自带 alpha，跳过抠像")
    elif a.key.lower() == "auto":
        key = guess_key_color(a.input)
        if key:
            print(f"[make_pet] 检测到纯色背景，抠像色 {key}")
        else:
            print("[make_pet] 警告: 背景不均匀，无法自动抠像 → 宠物会带矩形背景。\n"
                  "           用 --key 0xRRGGBB 强制指定背景色，或选背景纯色的素材。")
    else:
        key = a.key
    if key and not re.match(r"^0x[0-9A-Fa-f]{6}$", key):
        die(f"--key {key!r} 应形如 0x00FF00 或 none/auto")

    # --- 切段 -----------------------------------------------------------
    # --enter / --idle 形如 START:DUR；DUR 省略或为 0 → 一直到素材结尾
    def segment(spec: str, default_start: float) -> tuple[float, float | None]:
        if not spec:
            return default_start, None
        start_s, _, tail = spec.partition(":")
        start = parse_time(start_s, default_start)
        seg_dur = parse_time(tail, 0.0) if tail else 0.0
        return start, (seg_dur if seg_dur > 0 else None)

    enter_dur_default = min(3.0, dur) if dur > 3.5 else 0.0
    enter_start, enter_dur = segment(a.enter, 0.0)
    if not a.enter:
        enter_dur = enter_dur_default or None
    idle_default = enter_start + (enter_dur if enter_dur is not None else 0.0)
    if a.idle:
        idle_start, idle_dur = segment(a.idle, idle_default)
        has_idle = dur - idle_start > 0.3  # 剩余 0.3s 以内就不做独立待机段
    else:
        idle_start, idle_dur = idle_default, None
        # 出场段占满整条素材 → 没有剩余时间做待机，idle 复用 enter
        has_idle = enter_dur is not None and dur - idle_start > 0.3
    if enter_dur and enter_start + enter_dur > dur + 0.05:
        die(f"出场段超出素材长度 {dur:.2f}s")

    target_w = min(info["width"], a.max_width)
    target_w -= target_w % 2
    vf = vf_chain(key, a.key_similarity, a.key_blend, target_w, a.fps)

    with tempfile.TemporaryDirectory(prefix="make_pet_") as td:
        tmp = Path(td)
        enter_webm = tmp / "enter.webm"
        idle_webm = tmp / "idle.webm"
        thumb = tmp / "thumbnail.png"

        print(f"[make_pet] 编码 enter.webm: start={enter_start}s "
              f"dur={enter_dur or '到结尾'}")
        encode_vp9(a.input, enter_webm, enter_start, enter_dur, vf, a.crf, pingpong=False)

        reuse_enter = False
        if has_idle:
            print(f"[make_pet] 编码 idle.webm: start={idle_start}s "
                  f"dur={idle_dur or '到结尾'}"
                  + (" (pingpong)" if a.pingpong else ""))
            encode_vp9(a.input, idle_webm, idle_start, idle_dur, vf, a.crf, pingpong=a.pingpong)
        else:
            reuse_enter = True
            print("[make_pet] 素材没有可用的待机段 → idle 复用 enter（前端会自动循环出场动画）")
            shutil.copyfile(enter_webm, idle_webm)

        print("[make_pet] 生成 thumbnail.png")
        make_thumbnail(a.input, thumb, idle_start if has_idle else enter_start,
                       key, a.key_similarity, a.key_blend)

        # --- 校验 -------------------------------------------------------
        explicit_opaque = a.key.lower() == "none"  # 用户明确接受矩形背景
        for fname, fpath, optional in [("enter", enter_webm, False),
                                       ("idle", idle_webm, reuse_enter)]:
            if optional or has_alpha(fpath):
                continue
            if explicit_opaque:
                print(f"[make_pet] 警告: {fname}.webm 是不透明的（--key none），"
                      "overlay 上会显示矩形背景")
            else:
                die(f"{fname}.webm 没有 alpha 通道（packet 无 BlockAdditional）→ "
                    "产物会是不透明矩形。背景不均匀时用 --key 0xRRGGBB 指定抠像色，"
                    "或显式 --key none 接受矩形背景")
        pi = probe(enter_webm)

        # --- 落盘 -------------------------------------------------------
        out_root = a.out or (Path(__import__("os").environ.get("APPDATA", ".")) / "top.huyanbao.dev" / "pets")
        dest = out_root / a.id
        dest.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(enter_webm, dest / "enter.webm")
        shutil.copyfile(idle_webm, dest / "idle.webm")
        shutil.copyfile(thumb, dest / "thumbnail.png")

        meta = {
            "id": a.id,
            "name": a.name,
            "description": a.description,
            "species": a.species,
            "rarity": a.rarity,
            "personality": a.personality,
            "bgColor": a.bg_color,
            "anchorX": a.anchor_x,
            "scale": a.scale,
        }
        (dest / "meta.json").write_text(
            json.dumps(meta, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    size = sum(f.stat().st_size for f in dest.iterdir())
    print(f"[make_pet] 完成 → {dest}")
    print(f"          enter.webm {pi['width']}x{pi['height']} "
          f"{'带 alpha' if has_alpha(dest / 'enter.webm') else '不透明'} · "
          f"idle {'复用 enter' if reuse_enter else '独立'} · 共 {size/1024/1024:.1f} MB")
    print("          重启护眼宝即可在「宠物」页看到；要发布到镜像还需 "
          "index.json + pets/<id>/manifest.json（petstore.rs）")
    if not key and not src_alpha:
        print("          注意: 本次未抠像，overlay 上会显示视频矩形背景")


if __name__ == "__main__":
    main()
