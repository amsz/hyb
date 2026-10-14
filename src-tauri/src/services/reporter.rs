//! 遥测上报（services::reporter）——**二次开发建议默认关闭或移除**。
//!
//! 证据（exe 字符串 `src\services\reporter.rs` + 本机数据目录）：
//!   端点 `https://api.huyanbao.cc`
//!     /api/v1/statistics/newInstall   -> install_reported.flag
//!     /api/v1/statistics/dau          -> dau_%Y-%m-%d.flag
//!     /api/v1/statistics/dailyReport  -> daily_%Y-%m-%d.flag
//!     /api/v1/statistics/screenshot   -> screenshot_install.flag / screenshot_dau_*.flag
//!   队列 report_queue.json / screenshot_queue.json，图片落在 pending_screenshots\
//!   错误状态写 screenshot_last_error.txt（本机值为 `queued`）
//!   设备指纹（PowerShell 内联执行）：
//!     MAC: Get-CimInstance Win32_NetworkAdapterConfiguration … $_.MACAddress
//!     机型: (Get-CimInstance Win32_ComputerSystem).Manufacturer + '|' + .Model
//!   注释字符串：`publicIp and geo should be enriched by the API server from the request source`
//!   `capture skipped: no screenshot on this platform`
//!
//! 所有上报均受 `config.allowUploadStats` 控制（[INFERRED] 原包未确认开关归属）。

use crate::state::{data_dir, lock, Shared};
use serde_json::json;
use std::path::PathBuf;

pub const BASE: &str = "https://api.huyanbao.cc";

fn flag(name: &str) -> PathBuf {
    data_dir().join(name)
}
fn today(suffix: &str) -> String {
    let d = chrono::Local::now().format("%Y-%m-%d");
    format!("{suffix}_{d}.flag")
}
fn mark_done(path: &PathBuf) {
    let _ = std::fs::write(path, b"1");
}
fn already(path: &PathBuf) -> bool {
    path.exists()
}

async fn post_json(path: &str, payload: serde_json::Value) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{BASE}{path}"))
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("status {}", resp.status()));
    }
    Ok(())
}

fn allow(shared: &Shared) -> bool {
    lock(shared).config.allow_upload_stats
}

/// 设备指纹（仅在 allowUploadStats 打开时采集）
fn device_meta() -> serde_json::Value {
    let run = |script: &str| -> String {
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    let macs = run(
        "Get-CimInstance Win32_NetworkAdapterConfiguration | \
         Where-Object { $_.MACAddress -and $_.IPEnabled } | \
         ForEach-Object { $_.MACAddress }",
    );
    let model = run(
        "(Get-CimInstance Win32_ComputerSystem | Select-Object -First 1).Manufacturer + '|' + \
         (Get-CimInstance Win32_ComputerSystem | Select-Object -First 1).Model",
    );
    json!({ "macs": macs, "model": model })
}

fn base_payload(_shared: &Shared) -> serde_json::Value {
    json!({
        "installId": crate::state::install_id(),
        "version": crate::APP_VERSION,
        "ts": chrono::Local::now().to_rfc3339(),
        // publicIp / geo 由服务端按请求来源补齐
    })
}

// ---------------------------------------------------------------- 上报项

/// 新装上报（一次性）
pub async fn report_new_install(app: &tauri::AppHandle, shared: &Shared) {
    let path = flag("install_reported.flag");
    if already(&path) || !allow(shared) {
        return;
    }
    let mut payload = base_payload(shared);
    payload["newInstall"] = json!(true);
    payload["meta"] = device_meta();
    match post_json("/api/v1/statistics/newInstall", payload).await {
        Ok(()) => {
            mark_done(&path);
            let _ = mark_done(&flag("screenshot_install.flag"));
        }
        Err(e) => enqueue(app, "newInstall", e),
    }
}

/// 日活上报（每日一次）
pub async fn report_dau(app: &tauri::AppHandle, shared: &Shared) {
    let path = flag(&today("dau"));
    if already(&path) || !allow(shared) {
        return;
    }
    let mut payload = base_payload(shared);
    payload["dau"] = json!(true);
    match post_json("/api/v1/statistics/dau", payload).await {
        Ok(()) => mark_done(&path),
        Err(e) => enqueue(app, "dau", e),
    }
}

/// 日报上报（每天 00:00 后第一次运行）
pub async fn report_daily(app: &tauri::AppHandle, shared: &Shared) {
    let path = flag(&today("daily"));
    if already(&path) || !allow(shared) {
        return;
    }
    let stats = { lock(shared).today_stats() };
    let mut payload = base_payload(shared);
    payload["stats"] = serde_json::to_value(&stats).unwrap_or_default();
    payload["eyeFilterSeconds"] = json!(stats.eye_filter_seconds);
    match post_json("/api/v1/statistics/dailyReport", payload).await {
        Ok(()) => mark_done(&path),
        Err(e) => enqueue(app, "dailyReport", e),
    }
}

/// 每日一屏截图（DAU 活跃凭证）
pub async fn report_screenshot(app: &tauri::AppHandle, shared: &Shared) {
    let path = flag(&today("screenshot_dau"));
    if already(&path) || !allow(shared) {
        return;
    }

    let Some(path_file) = capture_screen() else {
        log::info!("capture skipped: no screenshot on this platform");
        let _ = std::fs::write(data_dir().join("screenshot_last_error.txt"), "skipped");
        return;
    };

    let mut payload = base_payload(shared);
    payload["path"] = json!(path_file);
    payload["batchId"] = json!(uuid::Uuid::new_v4().to_string());
    payload["displayIndex"] = json!(0);
    match post_json("/api/v1/statistics/screenshot", payload).await {
        Ok(()) => {
            mark_done(&path);
            let _ = std::fs::write(data_dir().join("screenshot_last_error.txt"), "queued");
            let _ = std::fs::remove_file(&path_file);
        }
        Err(e) => {
            let _ = std::fs::write(data_dir().join("screenshot_last_error.txt"), &e);
            enqueue(app, "screenshot", e);
        }
    }
}

/// 抓取主显示器 -> `pending_screenshots\<ts>.jpg`
#[cfg(windows)]
fn capture_screen() -> Option<String> {
    use screenshots::Screen;
    let screens = Screen::all().ok()?;
    let screen = screens.first()?;
    let image = screen.capture().ok()?;
    let dir = data_dir().join("pending_screenshots");
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join(format!("{}.jpg", chrono::Local::now().format("%Y%m%d%H%M%S")));
    image.save(&file).ok()?;
    Some(file.to_string_lossy().to_string())
}

#[cfg(not(windows))]
fn capture_screen() -> Option<String> {
    None
}

// ---------------------------------------------------------------- 队列

/// 失败的上报项落到 report_queue.json，下次启动重试
fn enqueue(_app: &tauri::AppHandle, kind: &str, error: String) {
    let path = data_dir().join("report_queue.json");
    let mut queue: Vec<serde_json::Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    queue.push(json!({ "kind": kind, "error": error, "ts": chrono::Local::now().to_rfc3339() }));
    let _ = std::fs::write(&path, serde_json::to_string(&queue).unwrap_or_default());
}

/// 启动时统一执行（受 allowUploadStats 控制）
pub async fn run(app: tauri::AppHandle, shared: Shared) {
    if !allow(&shared) {
        log::info!("[reporter] upload disabled by config.allowUploadStats");
        return;
    }
    report_new_install(&app, &shared).await;
    report_dau(&app, &shared).await;
    report_daily(&app, &shared).await;
    report_screenshot(&app, &shared).await;
    let _ = app;
}
