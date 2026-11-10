//! huyanbao_lib —— 护眼宝应用核心。
//!
//! 本仓库是依据 `huyanbao.exe`（0.1.4，SHA-256 ebd3ac79…bfa17ada）逆向重建的源码：
//! 模块划分、IPC 命令名、事件名、日志文案、配置字段均与原二进制中的字面量对齐。
//! 未在原包中出现的实现细节以 `[INFERRED]` 标注。

pub mod commands;
pub mod error;
pub mod services;
pub mod state;

use services::{activity, bluelight, petstore, reporter, shortcuts, timer, updater, windows};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{Emitter, Manager};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 托盘“退出”置位后才允许真正退出（否则点窗口关闭只收托盘）
pub static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);

pub fn request_exit() {
    EXIT_REQUESTED.store(true, Ordering::SeqCst);
}

static ENGINE: OnceLock<Arc<Mutex<shortcuts::ShortcutEngine>>> = OnceLock::new();
static CONFLICTS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

/// 全局热键注册表（命令层与监听线程共享同一把锁）
pub fn engine() -> &'static Arc<Mutex<shortcuts::ShortcutEngine>> {
    ENGINE.get_or_init(|| Arc::new(Mutex::new(shortcuts::ShortcutEngine::new())))
}

pub fn conflicts() -> &'static Mutex<Vec<String>> {
    CONFLICTS.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn run() {
    let shared = state::shared();

    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_http::init())
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            // 配置 / 护眼
            commands::get_config,
            commands::save_config,
            commands::get_eye_state,
            commands::set_eye_enabled,
            commands::set_filter_strength,
            commands::set_overlay_strength,
            commands::nudge_filter_strength,
            commands::set_night_mode,
            commands::set_color_preset,
            commands::list_color_presets,
            // 计时
            commands::get_timer,
            commands::toggle_timer_pause,
            commands::start_rest_now,
            commands::skip_rest,
            // 统计
            commands::get_daily_stats,
            commands::get_stats_history,
            // 宠物
            commands::list_animals,
            commands::list_species,
            commands::get_animal,
            commands::sync_pets,
            commands::close_animal_overlay,
            commands::preview_rest_presence,
            commands::cancel_preview_overlay,
            // 窗口
            commands::show_main_window,
            commands::hide_main_to_tray,
            commands::ensure_filter_overlay,
            commands::create_filter_overlay,
            commands::destroy_filter_overlay,
            commands::destroy_ear_overlay,
            commands::set_ear_ignoring_cursor_events,
            commands::get_global_position,
            commands::restore_state_from_storage,
            // 桌面小云豆
            commands::set_desktop_ear_pose,
            commands::begin_desktop_ear_drag,
            commands::end_desktop_ear_drag,
            // 快捷键 / 更新 / 其他
            commands::get_shortcut_conflicts,
            commands::check_for_updates,
            commands::load_tongji_script,
            commands::get_install_id,
            commands::run_system_command,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            log::info!(
                "[startup] version={APP_VERSION} os={} autostartLaunch={} dataDir={} \
                 logDir={} installId={}",
                std::env::consts::OS,
                std::env::args().any(|a| a == "--autostart"),
                state::data_dir().display(),
                state::log_dir().display(),
                state::install_id()
            );

            // 内嵌宠物素材解包（单文件发布包靠它才有宠物；已解过就是一次读文件判断）
            services::embedded_pets::install();

            // 色温 + 蒙层显隐：先建窗口再 refresh，否则 alpha=0 的隐藏会被后续创建覆盖
            let _ = windows::ensure_filter_overlay(&handle);
            bluelight::refresh(&handle, &shared);

            // 主窗口隐藏启动（消灭启动白屏），前端首帧渲染完后会调 show_main_window。
            // 兑底：若前端挂了（JS 报错/加载失败），3s 后强制显示，避免"点了图标没反应"。
            {
                let handle = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    if let Some(win) = handle.get_webview_window("main") {
                        if !win.is_visible().unwrap_or(false) {
                            log::warn!(
                                "[windows] main window still hidden after 3s (frontend may have failed), forcing show"
                            );
                            let _ = windows::show_main(&handle);
                        }
                    }
                });
            }

            // 托盘
            if let Err(e) = windows::build_tray(&handle, &shared) {
                log::warn!("[windows] tray build failed: {e}");
            }

            // 桌面小云豆
            if let Err(e) = windows::ensure_desktop_ear(&handle, &shared) {
                log::warn!("[windows] desktop ear failed: {e}");
            }

            // 全局快捷键
            {
                let list = engine()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .reregister(&shared);
                *conflicts().lock().unwrap_or_else(|e| e.into_inner()) = list.clone();
                let _ = handle.emit("shortcuts:conflict", list);
            }
            shortcuts::spawn_listener(handle.clone(), shared.clone(), engine().clone());

            // 击键/点击计数（默认关）
            activity::start(shared.clone());

            // 计时线程
            timer::spawn(handle.clone(), shared.clone());

            // 更新：上次下载好的安装器先跑起来，再做一次静默检查
            // 离线/开发策略：不碰 api.huyanbao.cc
            if state::IS_DEV || !state::ALLOW_HUYANBAO_NET {
                log::info!(
                    "[updater] disabled (dev={}, allow_net={})",
                    state::IS_DEV,
                    state::ALLOW_HUYANBAO_NET
                );
                log::info!("[reporter] disabled (telemetry off)");
            } else {
                updater::run_pending_if_any();
                let (h1, s1) = (handle.clone(), shared.clone());
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = updater::check(&h1, &s1, true).await {
                        log::warn!("[updater] startup check failed: {e}");
                    }
                });

                // 遥测（受 allowUploadStats 控制）
                let (h2, s2) = (handle.clone(), shared.clone());
                tauri::async_runtime::spawn(async move {
                    reporter::run(h2, s2).await;
                });
            }

            // 宠物库同步（后台，进度走 pets:sync / pets:updated）
            // 离线策略（ALLOW_HUYANBAO_NET=false）：不发起任何到 huyanbao.cc 系域名的请求
            if !state::ALLOW_HUYANBAO_NET {
                log::info!("[pets] offline policy: pet store sync disabled");
                let _ = handle.emit("pets:sync", "done");
            } else {
                let h3 = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = petstore::sync(h3).await {
                        log::warn!("[pets] sync failed: {e}");
                    }
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                // 主窗口点关闭 = 收到托盘，而不是退出
                tauri::WindowEvent::CloseRequested { api, .. }
                    if window.label() == "main" =>
                {
                    api.prevent_close();
                    windows::hide_main(window.app_handle());
                }
                // 休息窗口被销毁 -> 若非程序主动关闭，则视为“用户关闭，结束本次休息”
                tauri::WindowEvent::Destroyed if window.label() == "animal-overlay" => {
                    use std::sync::atomic::Ordering::SeqCst;
                    let programmatic =
                        windows::SUPPRESS_OVERLAY_CLOSE_EVENT.swap(false, SeqCst);
                    if !programmatic {
                        services::timer::OVERLAY_CLOSED.store(true, SeqCst);
                    }
                }
                // 显示器拓扑变化 -> 重放滤镜蒙层
                tauri::WindowEvent::ScaleFactorChanged { .. } => {
                    if window.label() == "main" {
                        windows::replace_filter_overlay(window.app_handle());
                    }
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { api, .. } => {
                // 托盘常驻：只有托盘“退出”菜单（request_exit）或 --force-exit 才真正退出
                if !EXIT_REQUESTED.load(Ordering::SeqCst)
                    && !std::env::args().any(|a| a == "--force-exit")
                {
                    api.prevent_exit();
                }
            }
            tauri::RunEvent::Exit => {
                // 退出前补存当日统计（否则当天最后一段有效工时/在座时长会丢）
                if let Some(shared) = app.try_state::<state::Shared>() {
                    timer::flush_history(&shared);
                }
                timer::shutdown()
            }
            _ => {}
        });
}
