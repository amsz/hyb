//! 宠物仓库同步（远端 -> 本地）。
//!
//! 证据（exe 字符串 + 本机日志）：
//!   主源   https://pets.huyanbao.workers.dev/index.json   （长期不可达）
//!   镜像   https://huyanbao.cc/pets  +  address.txt（逐个尝试）
//!   校验   `sha256 does not match the manifest` / `size … does not match the manifest's`
//!   暂存   `_up_`（下载完成后移入 pets/）
//!   事件   pets:sync = fetching | done | failed，随后 pets:updated
//!   日志   `[pets] using mirror <url>` `[pets] installed <id>`
//!          `[pets] index.json only lists ids: downloads cannot be verified …`

use crate::error::{AppError, AppResult};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tauri::Emitter;

pub const PRIMARY_INDEX: &str = "https://pets.huyanbao.workers.dev/index.json";
pub const MIRROR_ADDRESS: &str = "https://huyanbao.cc/pets/address.txt";
pub const PET_FILES: [&str; 4] = ["meta.json", "enter.webm", "idle.webm", "thumbnail.png"];

#[derive(Debug, Deserialize)]
struct RemoteFile {
    path: String,
    sha256: Option<String>,
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RemotePet {
    /// manifest 里的宠物 id；下载方也从 index.json 拿到同名 id，此处仅用于校验
    #[allow(dead_code)]
    id: String,
    #[serde(default)]
    files: Vec<RemoteFile>,
}

fn staging_root() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join("_up_")
}

fn installed_dir(id: &str) -> PathBuf {
    crate::state::data_dir().join("pets").join(id)
}

/// 从 address.txt 解析镜像地址列表（每行一个，跳过空行与 # 注释）
fn parse_addresses(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.trim_end_matches('/').to_string())
        .collect()
}

async fn fetch_text(url: &str) -> AppResult<String> {
    let resp = reqwest::get(url).await?.error_for_status()?;
    Ok(resp.text().await?)
}

/// 选择可用镜像：先试 address.txt 列表，再回落主源
async fn resolve_base() -> AppResult<String> {
    match fetch_text(MIRROR_ADDRESS).await {
        Ok(text) => {
            for base in parse_addresses(&text) {
                let probe = format!("{base}/index.json");
                if fetch_text(&probe).await.is_ok() {
                    log::info!("[pets] using mirror {base}");
                    return Ok(base);
                }
            }
            Err(AppError::msg("no mirror in the address list could be used"))
        }
        Err(e) => {
            log::warn!("[pets] could not read {MIRROR_ADDRESS}: {e}");
            match fetch_text(PRIMARY_INDEX).await {
                Ok(_) => Ok("https://pets.huyanbao.workers.dev".into()),
                Err(e2) => Err(e2),
            }
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

async fn download(base: &str, id: &str) -> AppResult<PathBuf> {
    let staging = staging_root().join("resources").join("pets").join(id);
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    std::fs::create_dir_all(&staging)?;

    // index.json 只列 id，逐个文件需要该宠物的 manifest
    let manifest_text = fetch_text(&format!("{base}/pets/{id}/manifest.json")).await?;
    let manifest: RemotePet = serde_json::from_str(&manifest_text)
        .map_err(|_| AppError::msg(format!("{id} did not answer with a manifest")))?;

    for file in &manifest.files {
        let url = format!("{base}/pets/{}/{}", id, file.path);
        let resp = reqwest::get(&url).await?.error_for_status()?;
        let bytes = bytes_of(resp).await?;
        if let Some(want) = file.size {
            if bytes.len() as u64 != want {
                return Err(AppError::msg(format!(
                    "size {} does not match the manifest's {want}",
                    bytes.len()
                )));
            }
        }
        if let Some(want) = &file.sha256 {
            let got = sha256_hex(&bytes);
            if !got.eq_ignore_ascii_case(want) {
                return Err(AppError::msg("sha256 does not match the manifest"));
            }
        }
        let safe_name = Path::new(&file.path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| file.path.clone());
        std::fs::write(staging.join(safe_name), &bytes)?;
    }

    if !staging.join("enter.webm").exists() {
        return Err(AppError::msg("no enter.webm after download"));
    }
    Ok(staging)
}

async fn bytes_of(resp: reqwest::Response) -> AppResult<Vec<u8>> {
    use futures_util::StreamExt;
    let mut out = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        out.extend_from_slice(&chunk?);
    }
    Ok(out)
}

fn install(staging: &Path, id: &str) -> AppResult<PathBuf> {
    // 清理上次残留
    let leftover = staging_root().join("resources").join("pets").join(format!("{id}.old"));
    if leftover.exists() {
        log::info!("[pets] clearing leftover {}", leftover.display());
        let _ = std::fs::remove_dir_all(&leftover);
    }
    let dest = installed_dir(id);
    std::fs::create_dir_all(dest.parent().unwrap_or(Path::new(".")))?;
    if dest.exists() {
        let _ = std::fs::rename(&dest, &leftover);
    }
    std::fs::rename(staging, &dest)?;
    Ok(dest)
}

/// 同步宠物库；进度通过 `pets:sync` / `pets:updated` 事件广播
pub async fn sync(app: tauri::AppHandle) -> AppResult<usize> {
    let emit = |payload: &str| {
        let _ = app.emit("pets:sync", payload);
    };
    emit("fetching");

    let base = match resolve_base().await {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[pets] sync failed: {e}");
            emit("failed");
            return Err(e);
        }
    };

    let index_text = fetch_text(&format!("{base}/index.json")).await?;
    // index.json 只有 id 列表（无 sha256 -> 单文件不可校验，只能靠 manifest）
    let ids: Vec<String> = serde_json::from_str(&index_text)
        .map_err(|_| AppError::msg("pets index is not a list of ids"))?;
    log::info!(
        "[pets] index.json only lists ids: downloads cannot be verified and re-uploads \
         will not reach installs that already have the pet"
    );

    let mut installed = 0usize;
    for id in ids {
        if !is_plausible_id(&id) {
            log::warn!("[pets] skipping suspicious id {id}");
            continue;
        }
        let dest = installed_dir(&id);
        if dest.join(".revision").exists() {
            continue; // 已安装
        }
        match download(&base, &id).await {
            Ok(staging) => match install(&staging, &id) {
                Ok(_) => {
                    log::info!("[pets] installed {id}");
                    installed += 1;
                }
                Err(e) => log::warn!("[pets] {id}: {e}"),
            },
            Err(e) => log::warn!("[pets] {id}: {e}"),
        }
    }

    let _ = app.emit("pets:updated", ());
    emit("done");
    Ok(installed)
}

/// id 只允许 `[a-z0-9_-]`，避免路径穿越
fn is_plausible_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() < 64
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}
