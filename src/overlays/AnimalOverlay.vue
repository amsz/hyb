<script setup>
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useRoute } from "vue-router";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  cancelPreviewOverlay,
  closeAnimalOverlay,
  getAnimal,
  getConfig,
  getTimer,
  listen,
  skipRest,
} from "../lib/ipc";

/**
 * 休息宠物窗口（label=animal-overlay，路由 #/overlay/animal）。
 *
 * 关键常量取自原包 bundle：
 *   XZ = 1200ms  打开后多久内不响应“跳过”
 *   ZZ = 900ms   触发离开后到关闭窗口的延时
 *   Py = 420ms   双 video 交叉淡入淡出时长 / 快进判定阈值
 *   QZ = 1.42, JZ = 1.72  accompany / focus 的放大系数
 *   dim: peek=0.28, focus=0.90, accompany=dimOpacity/100
 */
const OPEN_GUARD_MS = 1200;
const CLOSE_DELAY_MS = 900;
const CROSSFADE_MS = 420;
const SCALE_ACCOMPANY = 1.42;
const SCALE_FOCUS = 1.72;

const route = useRoute();
void route; // 保留：窗口 URL 变化由后端重建驱动

/**
 * 直接从 location.hash 解析，不用 vue-router 的 route.query：
 * `#/overlay/animal?preview=1&animal=eee&presence=accompany`
 * （曾用 route.query，实际取不到值 -> isPreview=false -> timer:tick 立刻把它当真休息的
 *   残留窗口关掉，表现就是“预览 2 秒就没了”）
 */
function hashParams() {
  const q = window.location.hash.split("?")[1] ?? "";
  return new URLSearchParams(q);
}
const q0 = hashParams();
/** 设置页的“出现方式”试看：不计入休息，点击或 6 秒后自动收起 */
const isPreview = q0.get("preview") === "1";
const previewAnimalId = q0.get("animal") ?? "";
const previewPresence = q0.get("presence") ?? "";

const timer = ref(null);
const config = ref(null);
const animal = ref(null);
const presence = ref("accompany");
const phase = ref("enter");
const leaving = ref(false);
const videos = ref([
  { src: "", loop: false },
  { src: "", loop: false },
]);
const active = ref(0);
const refs = [ref(null), ref(null)];

const openedAt = performance.now();
let crossfading = false;
let offTick = null;
let closing = false;
let closeTimer = null;

const isMac =
  navigator.userAgent.includes("Macintosh") || navigator.platform.toLowerCase().includes("mac");
const isWindows =
  navigator.userAgent.includes("Windows") || navigator.platform.toLowerCase().startsWith("win");

/**
 * 背景压暗。
 *
 * 原包 bundle 里这一行是 `presence==="focus" ? "0.90" : ((timer.dimOpacity ?? 90) / 100)`——
 * 读的是 **timer** 上的 dimOpacity，而 `TimerState` 根本没有这个字段（原包 TimerState 的
 * 序列化字段只有 state/workElapsedSecs/…/animalId/presence，见 exe .rdata 字面量），
 * 所以 accompany 永远走 `?? 90` → **0.900**。
 *
 * 我们曾经写成 `animal.dimOpacity ?? config.dimOpacity ?? 90`，而 config.dimOpacity 默认是
 * 28 → accompany 的压暗只有 0.28，桌面几乎没被盖住，看起来就像“没有全屏”。
 */
const dim = computed(() => {
  if (presence.value === "peek") return "0.28";
  if (presence.value === "focus") return "0.90";
  return ((timer.value?.dimOpacity ?? 90) / 100).toFixed(3);
});

/** 只有 enter / idle 相同（或 loopEnter）才循环 */
const loopEnter = computed(() => {
  const a = animal.value;
  if (!a) return false;
  if (a.loopEnter) return true;
  const enter = a.assets?.enter ?? "";
  const idle = a.assets?.idle ?? "";
  return !idle || idle === enter;
});

const toUrl = (p) => (p ? convertFileSrc(p) : "");

/** video 布局：peek 铺满；mac 铺满 cover；Windows 按 anchorX/scale 定位 */
const videoStyle = computed(() => {
  if (presence.value === "peek") {
    return { width: "100%", height: "100%", top: "0", left: "0", transform: "none", objectFit: "contain" };
  }
  if (isMac) {
    return { width: "100%", height: "100%", top: "0", left: "0", transform: "none", objectFit: "cover" };
  }
  const scale = animal.value?.scale ?? 0.72;
  const factor = isWindows
    ? presence.value === "focus"
      ? SCALE_FOCUS
      : SCALE_ACCOMPANY
    : 1;
  return {
    height: `${scale * factor * 100}vh`,
    left: `${(animal.value?.anchorX ?? 0.5) * 100}vw`,
  };
});

async function play(index, src, loop) {
  videos.value[index] = { src, loop };
  await new Promise((r) => requestAnimationFrame(r));
  const el = refs[index].value;
  if (!el) {
    console.error("[pet] player element missing; nothing will play");
    return;
  }
  await el.play().catch((e) => console.warn("[pet] explicit play declined", e));
}

/** 双槽位交叉淡入淡出切片 */
async function crossfade(src, loop) {
  if (crossfading || !src) return;
  crossfading = true;
  const next = 1 - active.value;
  videos.value[next] = { src, loop };
  await new Promise((r) => requestAnimationFrame(r));
  const el = refs[next].value;
  if (!el) {
    crossfading = false;
    return;
  }
  try {
    el.currentTime = 0;
    if (el.readyState < 2) {
      await new Promise((resolve) => {
        const onCanPlay = () => {
          el.removeEventListener("canplay", onCanPlay);
          resolve();
        };
        el.addEventListener("canplay", onCanPlay);
        window.setTimeout(onCanPlay, 600);
      });
    }
    await el.play().catch((e) => console.error("[pet] crossfade play failed", e));
  } finally {
    active.value = next;
    window.setTimeout(() => (crossfading = false), CROSSFADE_MS);
  }
}

function onEnded(i) {
  if (i !== active.value) return;
  goIdle();
}
function onTimeUpdate(i) {
  if (i !== active.value || crossfading) return;
  const el = refs[i].value;
  if (!el || !Number.isFinite(el.duration) || el.duration <= 0) return;
  if (el.duration - el.currentTime > CROSSFADE_MS / 1000) return;
  goIdle();
}
/** enter 播完切到 idle（loopEnter 的宠物不切） */
function goIdle() {
  const assets = animal.value?.assets;
  if (!assets || leaving.value || loopEnter.value || phase.value !== "enter") return;
  phase.value = "idle";
  crossfade(toUrl(assets.idle), true);
}
function onError(i) {
  if (i !== active.value) return;
  const assets = animal.value?.assets;
  if (!videos.value[i].src) return;
  console.error("[pet] clip failed to load", videos.value[i].src);
  if (!assets) return;
  if (loopEnter.value || phase.value === "idle") {
    dismiss(false);
    return;
  }
  phase.value = "idle";
  crossfade(toUrl(assets.idle), true);
}

function onStagePointer() {
  if (presence.value !== "peek") dismiss(true);
}
function onCardPointer(e) {
  if (presence.value === "peek") {
    e.stopPropagation();
    dismiss(true);
  }
}
function onKey(e) {
  if (["Escape", "Space", "Enter"].includes(e.code)) dismiss(true);
}
/** 打开后 XZ 内不响应跳过，避免误触 */
function dismiss(skip) {
  if (leaving.value) return;
  // 预览模式：只关窗，不碰计时器/统计
  if (isPreview) {
    leaving.value = true;
    cancelPreviewOverlay().catch(console.error);
    return;
  }
  if (performance.now() - openedAt < OPEN_GUARD_MS) return;
  leaving.value = true;
  if (skip) skipRest().catch(console.error);
  closeTimer = window.setTimeout(close, CLOSE_DELAY_MS);
}
function close() {
  if (closing) return;
  closing = true;
  if (isPreview) {
    cancelPreviewOverlay().catch(console.error);
  } else {
    closeAnimalOverlay().catch(console.error);
  }
}

onMounted(async () => {
  if (isPreview) {
    console.info("[pet] preview mode", { previewAnimalId, previewPresence });
  }
  const [t, c] = await Promise.all([getTimer(), getConfig()]);
  timer.value = t;
  config.value = c;
  // 预览：URL query 里的 presence 必须优先。
  // 曾经写成 `t.presence || previewPresence`，而没有真休息在跑时 timer.presence 是
  // TimerState 的默认值 "peek"（恒非空）→ 试看 accompany/focus 时窗口虽然已铺满主屏，
  // 页面却按 peek 渲染：不压暗、宠物 object-fit:contain 居中 → 看起来“没有全屏”。
  presence.value = (isPreview && previewPresence) || t.presence || c.restPresence || "accompany";
  const wanted = (isPreview && previewAnimalId) || t.animalId || c.selectedAnimalId || "";
  animal.value = wanted ? await getAnimal(wanted) : null;
  const assets = animal.value?.assets;
  if (assets) await play(0, toUrl(assets.enter), loopEnter.value);

  offTick = await listen("timer:tick", (e) => {
    timer.value = e.payload;
    // 预览窗口不跟随真实计时器退出
    if (e.payload.state !== "animalShowing" && !isPreview) dismiss(false);
  });
  window.addEventListener("keydown", onKey);
});

onUnmounted(() => {
  offTick?.();
  window.removeEventListener("keydown", onKey);
  if (closeTimer) window.clearTimeout(closeTimer);
});
</script>

<template>
  <div
    class="stage"
    :class="[presence, { leaving, mac: isMac }]"
    :style="{ '--dim': dim }"
    @pointerdown="onStagePointer"
  >
    <div class="backdrop" />
    <div class="card" @pointerdown="onCardPointer">
      <video
        v-for="(slot, i) in videos"
        :key="i"
        :ref="(el) => (refs[i].value = el)"
        class="pet"
        :class="{ hidden: i !== active }"
        :src="slot.src || undefined"
        :loop="slot.loop"
        :style="videoStyle"
        autoplay
        muted
        playsinline
        @ended="onEnded(i)"
        @timeupdate="onTimeUpdate(i)"
        @error="onError(i)"
      />
      <button v-if="presence === 'peek'" class="ok" type="button" @click="dismiss(true)">
        好啦
      </button>
    </div>
  </div>
</template>
<style scoped>
/* 还原自 huyanbao.exe 内嵌 bundle 的 scoped 样式（原作用域 data-v-38071425） */
.stage{position:fixed;top:0;right:0;bottom:0;left:0;overflow:hidden;background:transparent;cursor:default}
.stage.peek{overflow:visible}
.backdrop{position:absolute;top:0;right:0;bottom:0;left:0;background:#0f0a1c;opacity:0;animation:fade-in-38071425 .9s ease forwards}
.stage.peek .backdrop{opacity:1;animation:none;background:radial-gradient(ellipse 58% 62% at 50% 55%,rgba(5,16,15,.32) 0%,rgba(5,16,15,.12) 42%,transparent 72%)}
.stage.focus .backdrop{background:#080411}
.stage.leaving .backdrop{animation:fade-out-38071425 .8s ease forwards}
.stage.leaving.peek .backdrop{animation:none;opacity:0;transition:opacity .8s ease}
.card{position:absolute;top:0;right:0;bottom:0;left:0}
.stage.peek .card{display:flex;flex-direction:column;align-items:center;justify-content:flex-end;background:transparent;cursor:pointer}
.pet{position:absolute;top:50%;transform:translate(-50%,-50%);width:auto;object-fit:contain;background:transparent;pointer-events:none;opacity:1;transition:opacity .42s linear}
.pet.hidden{opacity:0}
.stage.leaving .pet{transition:opacity .8s ease}
.stage.peek .pet{width:100%;height:100%;top:0;left:0;transform:none}
.stage.leaving .pet,.stage.leaving .ok{opacity:0}
.ok{position:relative;z-index:2;margin-bottom:12px;padding:5px 12px;border:0;border-radius:999px;color:#ffffffe0;background:#0f0a1c59;font-size:12px;cursor:pointer}
</style>
