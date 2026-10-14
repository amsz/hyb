//! 静默更新（services::updater）。
//!
//! 证据：
//!   检查 `https://api.huyanbao.cc/api/v1/app/update/check`
//!   响应字段 status/currentVersion/latestVersion/message/updateAvailable/reason/
//!            size/notes/pubDate/url/sha256/downloadedAt/installerArgs/ok
//!   下载 `huyanbao-update.exe`（临时 `.part`），校验 sha256/size，写 `pending_update.json`
//!   安装：计划任务 `Update HuyanBao - Skip UAC`
//!          （schtasks /Create /XML /TN /F，/SC ONCE /ST 00:00 /SD 01/01/2099 /RL HIGHEST）
//!   文案：当前已是最新版本。/ 发现新版本并已下载，将在下次启动时安装。/
//!         检查更新失败，请稍后再试。
//!   日志：`[updater] silent check version=… endpoint=…`、`[updater] check failed: …`

use crate::error::{AppError, AppResult};
use crate::state::{data_dir, Shared};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const CHECK_ENDPOINT: &str = "https://api.huyanbao.cc/api/v1/app/update/check";
const TASK_NAME: &str = if crate::state::IS_DEV {
    "Update HuyanBao Dev - Skip UAC"
} else {
    "Update HuyanBao - Skip UAC"
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResponse {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub current_version: String,
    #[serde(default)]
    pub latest_version: Option<String>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub update_available: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub pub_date: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub downloaded_at: Option<String>,
    #[serde(default)]
    pub installer_args: Option<Vec<String>>,
    /// 前端关于页读取：downloading / done / error / none
    #[serde(default)]
    pub download_status: String,
    /// 0..=100
    #[serde(default)]
    pub progress_percent: f64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingUpdate {
    version: String,
    url: String,
    sha256: String,
    downloaded_at: String,
    installer_args: Vec<String>,
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn update_log() -> PathBuf {
    data_dir().join("update.log")
}
fn pending_path() -> PathBuf {
    data_dir().join("pending_update.json")
}

/// 检查更新（前端 `check_for_updates` 调用；启动时也做一次 silent check）
pub async fn check(
    _app: &tauri::AppHandle,
    shared: &Shared,
    download: bool,
) -> AppResult<UpdateCheckResponse> {
    let (current, allow_auto) = {
        let guard = crate::state::lock(shared);
        (crate::APP_VERSION.to_string(), guard.config.allow_auto_update)
    };
    log::info!("[updater] silent check version={current} endpoint={CHECK_ENDPOINT}");

    let client = reqwest::Client::builder().build()?;
    let resp = client
        .post(CHECK_ENDPOINT)
        .json(&serde_json::json!({ "version": current, "platform": "windows" }))
        .send()
        .await?
        .error_for_status()?;
    let body: UpdateCheckResponse = resp.json().await.map_err(|e| {
        log::warn!("[updater] parse update api response: {e}");
        AppError::msg(format!("parse update api response: {e}"))
    })?;

    if !body.ok && body.latest_version.is_none() {
        return Ok(UpdateCheckResponse {
            ok: false,
            message: "检查更新失败，请稍后再试。".into(),
            ..Default::default()
        });
    }

    let Some(url) = body.url.clone() else {
        return Ok(UpdateCheckResponse {
            ok: true,
            message: "当前已是最新版本。".into(),
            download_status: "none".into(),
            progress_percent: 0.0,
            ..body
        });
    };

    if download {
        match download_installer(&url, &body).await {
            Ok(()) => {
                schedule_update_task(&body)?;
                return Ok(UpdateCheckResponse {
                    ok: true,
                    message: "发现新版本并已下载，将在下次启动时安装。".into(),
                    download_status: "done".into(),
                    progress_percent: 100.0,
                    ..body
                });
            }
            Err(e) => {
                log::warn!("[updater] check failed: {e}");
                return Ok(UpdateCheckResponse {
                    ok: false,
                    message: "检查更新失败，请稍后再试。".into(),
                    download_status: "error".into(),
                    progress_percent: 0.0,
                    ..body
                });
            }
        }
    }
    let _ = allow_auto;
    Ok(body)
}

async fn download_installer(url: &str, info: &UpdateCheckResponse) -> AppResult<()> {
    let target = exe_dir().join("huyanbao-update.exe");
    let part = exe_dir().join("huyanbao-update.exe.part");

    let resp = reqwest::get(url).await?.error_for_status()?;
    let bytes = resp.bytes().await?;
    if bytes.is_empty() {
        return Err(AppError::msg("downloaded installer was empty"));
    }
    if let Some(size) = info.size {
        if bytes.len() as u64 != size {
            return Err(AppError::msg(format!(
                "size mismatch: got {}, expected {size}",
                bytes.len()
            )));
        }
    }
    if let Some(want) = &info.sha256 {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(&bytes);
        let got = format!("{:x}", h.finalize());
        if !got.eq_ignore_ascii_case(want) {
            return Err(AppError::msg("pending installer integrity check failed"));
        }
    }

    std::fs::write(&part, &bytes)?;
    std::fs::rename(&part, &target)?;

    let pending = PendingUpdate {
        version: info.latest_version.clone().unwrap_or_default(),
        url: url.to_string(),
        sha256: info.sha256.clone().unwrap_or_default(),
        downloaded_at: chrono::Local::now().to_rfc3339(),
        installer_args: info.installer_args.clone().unwrap_or_default(),
    };
    std::fs::write(pending_path(), serde_json::to_string_pretty(&pending)?)?;
    let _ = std::fs::write(
        update_log(),
        format!(
            "{} installer={}, args={:?}\n",
            chrono::Local::now().to_rfc3339(),
            target.display(),
            pending.installer_args
        ),
    );
    log::info!("[updater] started installer candidate staged; keeping pending marker until the new version launches");
    Ok(())
}

/// 生成 update-task.xml 并用 schtasks 注册“下次启动提权安装”
fn schedule_update_task(info: &UpdateCheckResponse) -> AppResult<()> {
    let target = exe_dir().join("huyanbao-update.exe");
    let target_s = target.display().to_string();
    let args = info.installer_args.clone().unwrap_or_default().join(" ");
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <URI>\{TASK_NAME}</URI>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT1H</ExecutionTimeLimit>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{target_s}</Command>
      <Arguments>{args}</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    );

    let xml_path = data_dir().join("update-task.xml");
    std::fs::write(&xml_path, &xml)?;
    log::info!("[updater] schedule pending updates");
    // 1) XML 方式（保留 /RL HIGHEST）
    let xml_out = std::process::Command::new("schtasks.exe")
        .args(["/Create", "/XML", &xml_path.to_string_lossy(), "/TN", TASK_NAME, "/F"])
        .output();
    if !matches!(xml_out, Ok(o) if o.status.success()) {
        log::warn!("[updater] create update task via xml failed; trying cli");
        // 2) 兜底：/SC ONCE /ST 00:00 /SD 01/01/2099 /RL HIGHEST
        let _ = std::process::Command::new("schtasks.exe")
            .args([
                "/Create",
                "/TN",
                TASK_NAME,
                "/TR",
                &format!("\"{}\" {}", target.display(), args),
                "/SC",
                "ONCE",
                "/ST",
                "00:00",
                "/SD",
                "01/01/2099",
                "/RL",
                "HIGHEST",
                "/F",
            ])
            .output();
    }
    Ok(())
}

/// 启动时若存在 pending_update.json，则运行安装器
pub fn run_pending_if_any() {
    let Ok(text) = std::fs::read_to_string(pending_path()) else { return };
    let Ok(pending) = serde_json::from_str::<PendingUpdate>(&text) else { return };
    let target = exe_dir().join("huyanbao-update.exe");
    if !target.exists() {
        return;
    }
    log::info!("[updater] run update task");
    match std::process::Command::new(&target)
        .args(&pending.installer_args)
        .spawn()
    {
        Ok(_) => log::info!("[updater] started installer"),
        Err(e) => log::error!("[updater] failed to start installer: {e}"),
    }
}
