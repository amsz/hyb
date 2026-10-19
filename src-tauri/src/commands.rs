//! IPC 命令层（huyanbao_lib::commands）。
//!
//! 命令名与 exe 内 invoke 处理器表逐一对齐，见报告 §4。
//! 事件：config:changed / eye:changed / shortcuts:conflict / config:warnings

use crate::services::{animals, bluelight, petstore, timer, updater, windows};
use crate::error::AppResult;
use crate::state::{lock, AppConfig, DailyStats, EyeState, Shared, TimerState};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};

// ---------------------------------------------------------------- 配置

#[tauri::command]
pub fn get_config(shared: tauri::State<'_, Shared>) -> AppConfig {
    lock(&shared).config.clone()
}

/// 保存配置：落盘 + 广播 + 应用副作用（自启/热键/小云豆/色温）
///
/// ⚠ 必须 async：副作用里 "ear" 会 `ensure_desktop_ear` → `WebviewWindowBuilder::build()`
///   创建 WebView2。同步 command 跑在主线程上，主线程在事件循环泵消息时创建
///   WebView2 会在初始化中途死锁：窗口壳建出来（白底、停在系统默认级联位置）
///   但页面永远停在 about:blank，主线程卡死后所有 IPC 超时、托盘退出也无法执行。
///   async command 跑在 async runtime 线程池上，窗口创建走 run_on_main_thread
///   往返，主循环空闲 → 正常（与 `ensure_filter_overlay`/`preview_rest_presence` 同理）。
#[tauri::command]
pub async fn save_config(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
    config: AppConfig,
) -> AppResult<AppConfig> {
    let changed_keys = {
        let mut guard = lock(&shared);
        let old = guard.config.clone();
        guard.config = config;
        let cfg = guard.config.clone();
        let _ = crate::state::save_config(&cfg);

        let mut changed = Vec::new();
        if old.shortcuts_enabled != cfg.shortcuts_enabled
            || old.shortcut_pause != cfg.shortcut_pause
            || old.shortcut_rest != cfg.shortcut_rest
            || old.shortcut_show != cfg.shortcut_show
            || old.shortcut_quit != cfg.shortcut_quit
            || old.shortcut_filter_up != cfg.shortcut_filter_up
            || old.shortcut_filter_down != cfg.shortcut_filter_down
            || old.shortcut_eye_toggle != cfg.shortcut_eye_toggle
        {
            changed.push("shortcuts");
        }
        if old.autostart != cfg.autostart {
            changed.push("autostart");
        }
        if old.show_desktop_ear != cfg.show_desktop_ear
            || old.desktop_ear_side != cfg.desktop_ear_side
            || old.desktop_ear_y != cfg.desktop_ear_y
        {
            changed.push("ear");
        }
        if old.input_stats_enabled != cfg.input_stats_enabled {
            changed.push("activity");
        }
        if old.eye_enabled != cfg.eye_enabled
            || old.filter_strength != cfg.filter_strength
            || old.overlay_strength != cfg.overlay_strength
            || old.night_mode != cfg.night_mode
            || old.color_preset != cfg.color_preset
        {
            changed.push("eye");
        }
        changed
    };

    apply_side_effects(&app, &shared, &changed_keys);
    let _ = app.emit("config:changed", lock(&shared).config.clone());
    Ok(lock(&shared).config.clone())
}

fn apply_side_effects(app: &tauri::AppHandle, shared: &Shared, changed: &[&str]) {
    for key in changed {
        match *key {
            "shortcuts" => {
                let conflicts = {
                    crate::engine()
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .reregister(shared)
                };
                *crate::conflicts().lock().unwrap_or_else(|e| e.into_inner()) =
                    conflicts.clone();
                let _ = app.emit("shortcuts:conflict", conflicts);
            }
            "autostart" => {
                if let Err(e) = set_autostart(lock(&shared).config.autostart) {
                    log::warn!("[autostart] {e}");
                    let _ = app.emit(
                        "config:warnings",
                        vec![format!("开机自启设置未能生效：{e}。其他设置已保存。")],
                    );
                }
            }
            "ear" => {
                let _ = windows::ensure_desktop_ear(app, shared);
            }
            "activity" => {
                // 输入钩子需重启应用生效（前端已提示）
            }
            "eye" => {
                bluelight::refresh(app, shared);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- 护眼

#[tauri::command]
pub fn get_eye_state(shared: tauri::State<'_, Shared>) -> EyeState {
    lock(&shared).eye.clone()
}

pub fn internal_set_eye_enabled(app: &tauri::AppHandle, shared: &Shared, enabled: bool) {
    {
        let mut guard = lock(shared);
        guard.config.eye_enabled = enabled;
        let _ = crate::state::save_config(&guard.config);
    }
    bluelight::refresh(app, shared);
    let _ = app.emit("config:changed", lock(shared).config.clone());
}

#[tauri::command]
pub fn set_eye_enabled(app: tauri::AppHandle, shared: tauri::State<'_, Shared>, enabled: bool) {
    internal_set_eye_enabled(&app, &shared, enabled);
}

#[tauri::command]
pub fn set_filter_strength(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
    strength: u32,
) {
    {
        let mut guard = lock(&shared);
        guard.config.filter_strength = strength.min(100);
        let _ = crate::state::save_config(&guard.config);
    }
    bluelight::refresh(&app, &shared);
}

#[tauri::command]
pub fn set_overlay_strength(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
    strength: u32,
) {
    {
        let mut guard = lock(&shared);
        guard.config.overlay_strength = strength.min(100);
        let _ = crate::state::save_config(&guard.config);
    }
    bluelight::refresh(&app, &shared);
}

pub fn internal_nudge(app: &tauri::AppHandle, shared: &Shared, delta: i32) {
    {
        let mut guard = lock(shared);
        let next = guard.config.filter_strength as i32 + delta;
        guard.config.filter_strength = next.clamp(0, 100) as u32;
        let _ = crate::state::save_config(&guard.config);
    }
    bluelight::refresh(app, shared);
}

#[tauri::command]
pub fn nudge_filter_strength(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
    delta: i32,
) {
    internal_nudge(&app, &shared, delta);
}

#[tauri::command]
pub fn set_night_mode(app: tauri::AppHandle, shared: tauri::State<'_, Shared>, enabled: bool) {
    {
        let mut guard = lock(&shared);
        guard.config.night_mode = enabled;
        let _ = crate::state::save_config(&guard.config);
    }
    bluelight::refresh(&app, &shared);
}

#[tauri::command]
pub fn set_color_preset(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
    preset_id: String,
) {
    {
        let mut guard = lock(&shared);
        guard.config.color_preset = preset_id;
        let _ = crate::state::save_config(&guard.config);
    }
    bluelight::refresh(&app, &shared);
}

#[tauri::command]
pub fn list_color_presets() -> Vec<Value> {
    bluelight::PRESETS
        .iter()
        .map(|p| {
            json!({ "id": p.id, "name": p.name, "color": p.color, "description": p.description })
        })
        .collect()
}

// ---------------------------------------------------------------- 计时

#[tauri::command]
pub fn get_timer(shared: tauri::State<'_, Shared>) -> TimerState {
    lock(&shared).timer.clone()
}

#[tauri::command]
pub fn toggle_timer_pause(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
) -> TimerState {
    {
        let mut guard = lock(&shared);
        guard.config.timer_paused = !guard.config.timer_paused;
        let _ = crate::state::save_config(&guard.config);
    }
    let _ = app.emit("config:changed", lock(&shared).config.clone());
    lock(&shared).timer.clone()
}

/// ⚠ 必须 async：`start_rest` → `show_animal_overlay` → `WebviewWindowBuilder::build()`，
///   同步 command（主线程）里创建 WebView2 会死锁（同 save_config 注释）。
#[tauri::command]
pub async fn start_rest_now(app: tauri::AppHandle, shared: tauri::State<'_, Shared>) -> AppResult<()> {
    timer::start_now(&app, &shared);
    Ok(())
}

#[tauri::command]
pub fn skip_rest(app: tauri::AppHandle, shared: tauri::State<'_, Shared>, with_pet: Option<bool>) {
    // withPet = 前端从宠物窗口调用时传 true（原包参数，本实现两种情况都计为跳过）
    let _ = with_pet;
    timer::skip(&app, &shared);
}

// ---------------------------------------------------------------- 统计

#[tauri::command]
pub fn get_daily_stats(shared: tauri::State<'_, Shared>) -> DailyStats {
    lock(&shared).today_stats()
}

#[tauri::command]
pub fn get_stats_history(shared: tauri::State<'_, Shared>) -> Vec<DailyStats> {
    lock(&shared).history.clone()
}

// ---------------------------------------------------------------- 宠物

#[tauri::command]
pub fn list_animals() -> Vec<animals::Animal> {
    animals::list()
}

#[tauri::command]
pub fn list_species() -> Vec<String> {
    animals::list()
        .into_iter()
        .map(|a| a.species)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[tauri::command]
pub fn get_animal(animal_id: String) -> Option<animals::Animal> {
    animals::get(&animal_id)
}

#[tauri::command]
pub fn close_animal_overlay(app: tauri::AppHandle) {
    // 前端关闭 -> 计时层据此结束本次休息
    crate::services::timer::OVERLAY_CLOSED
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = windows::close_animal_overlay(&app);
}

/// 在设置页点“出现方式”时试看：不计入休息/统计，到时自动收起。
/// 10s 是为了能把进场动画（实测 8~15s）完整看一遍。
pub const PREVIEW_SECONDS: u64 = 10;

/// 预览代次：切换预览时旧的 10s 定时器不得杀掉新预览
static PREVIEW_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[tauri::command]
pub async fn preview_rest_presence(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
    presence: String,
    animal_id: Option<String>,
) -> AppResult<()> {
    log::info!(
        "[windows] preview_rest_presence invoked presence={presence} animal_id={animal_id:?}"
    );
    // 正在真实休息时不打扰
    if lock(&shared).rest_running {
        log::info!("[windows] preview skipped: a real rest is running");
        return Ok(());
    }
    let id = animal_id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| lock(&shared).config.selected_animal_id.clone());
    if id.is_empty() || animals::get(&id).is_none() {
        if let Some(a) = animals::list().into_iter().next() {
            // 没指定就用库里第一只
            log::info!("[windows] preview fallback animal={}", a.id);
        }
    }
    let id = if id.is_empty() {
        animals::list().first().map(|a| a.id.clone()).unwrap_or_default()
    } else {
        id
    };
    if id.is_empty() {
        log::warn!("[windows] preview skipped: pet library is empty");
        return Ok(());
    }
    if let Err(e) = windows::show_animal_overlay(&app, &id, &presence, true) {
        log::warn!("[windows] preview overlay failed: {e}");
        return Ok(());
    }
    log::info!("[windows] preview {presence} with animal={id}, auto close in {PREVIEW_SECONDS}s");
    let seq = PREVIEW_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(PREVIEW_SECONDS));
        if PREVIEW_SEQ.load(std::sync::atomic::Ordering::SeqCst) != seq {
            log::info!("[windows] preview superseded by a newer one, skip closing");
            return;
        }
        // 预览的 10s 定时器不能关掉真休息的窗口：用户可能在这 10s 内点了“立即休息”，
        // 此时 label=animal-overlay 已被真休息重建；误关会让休息全程看不到宠物
        // （而且 Destroyed 被 SUPPRESS 抑制，休息不会结束 → 卡在 2 分钟倒计时）。
        if lock(&app.state::<Shared>()).rest_running {
            log::info!("[windows] preview timeout skipped: a real rest took over the overlay");
            return;
        }
        log::info!("[windows] preview timeout, closing overlay");
        windows::close_overlay_quietly(&app, "animal-overlay");
    });
    Ok(())
}

/// 预览时手动关闭（不计入跳过休息）
#[tauri::command]
pub async fn cancel_preview_overlay(app: tauri::AppHandle) -> AppResult<()> {
    log::info!("[windows] preview cancelled from frontend");
    // 作废尚未到期的超时定时器
    PREVIEW_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    windows::close_overlay_quietly(&app, "animal-overlay");
    Ok(())
}

// ---------------------------------------------------------------- 窗口

#[tauri::command]
pub fn show_main_window(app: tauri::AppHandle) {
    let _ = windows::show_main(&app);
}

#[tauri::command]
pub fn hide_main_to_tray(app: tauri::AppHandle) {
    windows::hide_main(&app);
}

#[tauri::command]
pub async fn ensure_filter_overlay(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
) -> AppResult<()> {
    // ⚠ 必须 async：内部会调 outer_position/inner_position（要等主循环回话），
    //   同步 command 跑在主线程上会死锁。
    let _ = windows::ensure_filter_overlay(&app);
    // 创建后按当前 alpha 决定显隐（alpha=0 隐藏，避免全屏置顶窗常驻）
    bluelight::refresh(&app, &shared);
    Ok(())
}

/// 显式创建/销毁蒙层窗口（原包 Rust 侧存在同名命令，前端未调用；此处保持对齐）
#[tauri::command]
pub async fn create_filter_overlay(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
) -> AppResult<()> {
    let _ = windows::ensure_filter_overlay(&app);
    bluelight::refresh(&app, &shared);
    Ok(())
}

#[tauri::command]
pub fn destroy_filter_overlay(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("filter-overlay") {
        let _ = win.close();
    }
}

#[tauri::command]
pub fn destroy_ear_overlay(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("desktop-ear") {
        let _ = win.close();
    }
}

/// 小云豆在拖拽/菜单展开时临时接管鼠标，否则鼠标穿透
#[tauri::command]
pub fn set_ear_ignoring_cursor_events(app: tauri::AppHandle, ignoring: bool) {
    if let Some(win) = app.get_webview_window("desktop-ear") {
        let _ = win.set_ignore_cursor_events(ignoring);
    }
}

/// 主窗口当前位置（物理坐标），供桌面挂件类功能定位。
/// async：`outer_position()` 要等主循环回话，同步 command 会死锁。
#[tauri::command]
pub async fn get_global_position(app: tauri::AppHandle) -> AppResult<Value> {
    match app.get_webview_window("main").and_then(|w| w.outer_position().ok()) {
        Some(p) => Ok(json!({ "x": p.x, "y": p.y })),
        None => Ok(json!({ "x": 0, "y": 0 })),
    }
}

/// 原包存在此命令（前端未调用）；本实现不做窗口状态持久化，恒返回 false
#[tauri::command]
pub fn restore_state_from_storage() -> bool {
    false
}

// ---------------------------------------------------------------- 桌面小云豆

#[tauri::command]
pub fn set_desktop_ear_pose(pose: String) {
    log::debug!("[windows] desktop ear pose -> {pose}");
}

/// 开始拖拽（前端超过阈值时调一次）。async：内部 outer_position 要等主循环回话。
#[tauri::command]
pub async fn begin_desktop_ear_drag(app: tauri::AppHandle) -> AppResult<()> {
    windows::begin_ear_drag(&app);
    Ok(())
}

/// 结束拖拽：停拖拽线程 + 夹紧可见 + 把位置写进 config 持久化。
#[tauri::command]
pub async fn end_desktop_ear_drag(app: tauri::AppHandle, shared: tauri::State<'_, Shared>) -> AppResult<()> {
    windows::end_ear_drag(&app, &shared);
    Ok(())
}

// ---------------------------------------------------------------- 快捷键

#[tauri::command]
pub fn get_shortcut_conflicts() -> Vec<String> {
    crate::conflicts()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// 热键触发的分发（services::shortcuts 调用）
pub fn on_shortcut(app: &tauri::AppHandle, shared: &Shared, field: &str) {
    match field {
        "shortcutEyeToggle" => {
            let enabled = { !lock(shared).config.eye_enabled };
            internal_set_eye_enabled(app, shared, enabled);
            log::info!("[eye] toggled by shortcut -> enabled={enabled}");
        }
        "shortcutPause" => {
            {
                let mut guard = lock(shared);
                guard.config.timer_paused = !guard.config.timer_paused;
                let _ = crate::state::save_config(&guard.config);
            }
            let _ = app.emit("config:changed", lock(shared).config.clone());
        }
        "shortcutRest" => timer::start_now(app, shared),
        "shortcutShow" => {
            let _ = windows::show_main(app);
        }
        "shortcutQuit" => {
            timer::shutdown();
            crate::request_exit();
            app.exit(0);
        }
        "shortcutFilterUp" => internal_nudge(app, shared, 5),
        "shortcutFilterDown" => internal_nudge(app, shared, -5),
        _ => {}
    }
}

// ---------------------------------------------------------------- 更新 / 其他

#[tauri::command]
pub async fn check_for_updates(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
) -> AppResult<Value> {
    // 离线策略：不请求 api.huyanbao.cc
    if !crate::state::ALLOW_HUYANBAO_NET {
        return Ok(json!({
            "ok": false,
            "message": "当前为离线构建，已关闭更新检查。",
            "latestVersion": null,
            "downloadStatus": "none",
            "progressPercent": 0,
        }));
    }
    let resp = updater::check(&app, &shared, true).await?;
    Ok(serde_json::to_value(resp)?)
}

/// 返回 `https://api.huyanbao.cc/tongji.js` 的源码，由前端注入（overlay 路由不注入）
#[tauri::command]
pub async fn load_tongji_script() -> AppResult<String> {
    // 离线策略：不请求 api.huyanbao.cc，直接返回空串（前端会跳过注入）
    if !crate::state::ALLOW_HUYANBAO_NET {
        log::info!("[tongji] offline policy: analytics script not loaded");
        return Ok(String::new());
    }
    let url = format!("{}/tongji.js", crate::services::reporter::BASE);
    Ok(reqwest::get(&url).await?.error_for_status()?.text().await?)
}

#[tauri::command]
pub fn get_install_id() -> String {
    crate::state::install_id()
}

/// 以 PowerShell 执行一条系统命令（超时受 timeoutSecs 控制）。
///
/// ⚠ 安全提示：该命令等价于“管理员权限 + 绕过执行策略执行任意命令”，
/// 二次开发建议移除或换成白名单实现；原包前端并未调用它。
#[tauri::command]
pub fn run_system_command(
    command: String,
    cwd: Option<String>,
    timeout_secs: Option<u64>,
) -> AppResult<Value> {
    let timeout = timeout_secs.unwrap_or(30).min(300);
    let mut child = {
        let mut cmd = std::process::Command::new("powershell");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &command]);
        if let Some(dir) = &cwd {
            cmd.current_dir(dir);
        }
        cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?
    };

    let start = std::time::Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) => {
                use std::io::Read;
                let mut out = String::new();
                let mut err = String::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = s.read_to_string(&mut out);
                }
                if let Some(mut s) = child.stderr.take() {
                    let _ = s.read_to_string(&mut err);
                }
                return Ok(json!({
                    "status": status.code().unwrap_or(-1),
                    "stdout": out,
                    "stderr": err,
                }));
            }
            None => {
                if start.elapsed().as_secs() > timeout {
                    let _ = child.kill();
                    return Ok(json!({
                        "status": -1,
                        "stdout": "",
                        "stderr": format!("Command timed out after {timeout} seconds"),
                    }));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
}

// ---------------------------------------------------------------- 自启

/// 开机自启 = 计划任务（原包 `HuyanBao Auto Start`，ONLOGON /DELAY0000:20，参数 `--autostart`）
fn set_autostart(enable: bool) -> Result<(), String> {
    const TASK: &str = crate::state::AUTOSTART_TASK;
    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .to_string();

    if enable {
        let out = std::process::Command::new("schtasks.exe")
            .args([
                "/Create",
                "/TN",
                TASK,
                "/TR",
                &format!("\"{exe}\" --autostart"),
                "/SC",
                "ONLOGON",
                "/DELAY",
                "0000:20",
                "/F",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
    } else {
        let out = std::process::Command::new("schtasks.exe")
            .args(["/Delete", "/TN", TASK, "/F"])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- 启动期

/// 启动时同步宠物库（阻塞一小段，失败不影响启动）
#[tauri::command]
pub async fn sync_pets(app: tauri::AppHandle) -> AppResult<usize> {
    petstore::sync(app).await
}

