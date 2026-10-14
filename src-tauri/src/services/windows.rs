//! 窗口与托盘（services::windows）。
//!
//! 四个 Tauri 窗口（label 见 exe 权限表）：
//!   main            index.html               主界面
//!   filter-overlay  index.html#/overlay/filter  暖色蒙层（点击穿透、置顶、全屏）
//!   animal-overlay  index.html#/overlay/animal  休息宠物
//!   desktop-ear     index.html#/overlay/ear     桌面小云豆
//!
//! 日志字面量：`[windows] pet overlay created at <x>,<y> <w>x<h> peek=<bool>`
//!   `[windows] creating the desktop ear`、`[windows] filter overlay failed: …`
//!   `[windows] show main failed: …`、`[windows] monitor layout changed, re-placing filter overlay`
//! 托盘菜单文案：打开主界面、立即休息、退出（右键菜单；左键单击 = 打开主界面）

use crate::services::timer;
use crate::state::{lock, Shared};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager, WebviewUrl, WebviewWindowBuilder,
};

const FILTER_W: f64 = 640.0;
const FILTER_H: f64 = 400.0;
const EAR_W: f64 = 150.0;
const EAR_H: f64 = 170.0;

/// peek 小窗距主屏右/下边缘的距离。
/// 由原包日志反推（两组独立数据，扣除 DWM 不可见边框 8px 后均为 右10 / 下56）：
///   `pet overlay created at 1262,624 640x400`  -> 1920-1262-640=18 -> 可见 10
///   `pet overlay created at 1390,696 640x400`  -> 2048-1390-640=18 -> 可见 10
const PEEK_GAP_RIGHT: f64 = 10.0;
const PEEK_GAP_BOTTOM: f64 = 56.0;



static ANIMAL_ALIVE: AtomicBool = AtomicBool::new(false);

/// 程序主动关闭 overlay 时置位：销毁事件到达时不把它当作“用户跳过休息”
pub static SUPPRESS_OVERLAY_CLOSE_EVENT: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------- 窗口工具



fn overlay_window(
    app: &tauri::AppHandle,
    label: &str,
    url: &str,
    w: f64,
    h: f64,
    transparent: bool,
) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(win) = app.get_webview_window(label) {
        return Ok(win);
    }
    let win = WebviewWindowBuilder::new(app, label, WebviewUrl::App(url.into()))
        .title(label)
        .inner_size(w, h)
        .transparent(transparent)
        .decorations(false)
        .resizable(false)
        .focused(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(true)
        .build()?;
    if transparent {
        // 关键：窗口层与 webview 层都得透明，否则会出现“透明矩形框”
        let _ = win.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
        // 关掉 DWM 阴影：透明窗口四周会因此出现一圈圆角矩形边界（即用户看到的“透明的背景”）。
        // 不要用“去掉 WS_THICKFRAME”那招 —— 去掉后 DWM 会画一条硬白边，更明显。
        let _ = win.set_shadow(false);
    }
    Ok(win)
}

/// Windows 会给无边框窗口保留一圈不可见边框（DWM 扩展框架，实测左/右/下各 8px）。
/// `set_position` 定的是窗口矩形，而肉眼看到的是客户区 → 用 outer/inner 位置差反向补偿。
fn compensate_invisible_frame(win: &tauri::WebviewWindow) {
    let (outer, inner) = match (win.outer_position(), win.inner_position()) {
        (Ok(o), Ok(i)) => (o, i),
        _ => return,
    };
    let dx = inner.x - outer.x;
    let dy = inner.y - outer.y;
    if dx == 0 && dy == 0 {
        return;
    }
    log::debug!("[windows] compensate invisible frame dx={dx} dy={dy}");
    let _ = win.set_position(tauri::PhysicalPosition::new(outer.x - dx, outer.y - dy));
}

/// 当前主显示器尺寸（逻辑坐标），拿不到时回落 1920x1080
/// 主显示器的逻辑区域 `(x, y, w, h)`。
/// ⚠ 两个坑：
///   1. `monitor.size()` 是**物理**像素，`inner_size()`/`LogicalPosition` 要**逻辑**像素 → 需除以 scale；
///   2. 主屏不一定在 (0,0)（本机就是向右偏移），overlay 必须摆到 `monitor.position()`，不能写死 0,0。
fn primary_monitor_logical(app: &tauri::AppHandle) -> (f64, f64, f64, f64) {
    let mon = app
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| app.available_monitors().ok().and_then(|ms| ms.into_iter().next()));
    match mon {
        Some(m) => {
            let s = m.size();
            let p = m.position();
            let scale = if m.scale_factor() > 0.0 { m.scale_factor() } else { 1.0 };
            (
                p.x as f64 / scale,
                p.y as f64 / scale,
                s.width as f64 / scale,
                s.height as f64 / scale,
            )
        }
        None => (0.0, 0.0, 1920.0, 1080.0),
    }
}

/// 滤镜蒙层：铺满主显示器，点击穿透；alpha=0 时应当隐藏而不是常驻
pub fn ensure_filter_overlay(app: &tauri::AppHandle) -> tauri::Result<()> {
    let (mx, my, w, h) = primary_monitor_logical(app);
    let win = match overlay_window(app, "filter-overlay", "index.html#/overlay/filter", w, h, true)
    {
        Ok(w) => w,
        Err(e) => {
            log::warn!("[windows] filter overlay failed: {e}");
            return Err(e);
        }
    };
    let _ = win.set_ignore_cursor_events(true);
    let _ = win.set_always_on_top(true);
    // 摆到主屏原点（主屏可能不在 0,0）
    let _ = win.set_position(tauri::LogicalPosition::new(mx, my));
    compensate_invisible_frame(&win);
    // 关键：让窗口/webview 层都透明，否则会出现“遮罩常驻 + 拖动残影”
    let _ = win.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
    Ok(())
}

/// 蒙层显隐：alpha=0（关掉滤镜/蒙层为 0）时隐藏窗口，避免全屏置顶窗常驻导致残影
pub fn set_filter_overlay_visible(app: &tauri::AppHandle, visible: bool) {
    if let Some(win) = app.get_webview_window("filter-overlay") {
        if visible {
            let _ = win.show();
        } else {
            let _ = win.hide();
        }
    }
}

/// 显示器拓扑变化后重放蒙层（`monitor layout changed`）
pub fn replace_filter_overlay(app: &tauri::AppHandle) {
    log::info!("[windows] monitor layout changed, re-placing filter overlay");
    if let Some(win) = app.get_webview_window("filter-overlay") {
        let _ = win.close();
    }
    let _ = ensure_filter_overlay(app);
}

/// 休息宠物窗口：peek=右下角 640x400，其余=铺满屏幕
/// query 值只保留 URL 安全字符（id 本身就是 [a-z0-9_-]，这里只是防御）
fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

pub fn show_animal_overlay(
    app: &tauri::AppHandle,
    animal_id: &str,
    presence: &str,
    preview: bool,
) -> tauri::Result<()> {
    // 关键：先销毁上一个窗口。
    // 复用旧窗会把旧 URL（preview/presence/animal）和旧尺寸一并带过来，
    // 表现就是“探头->陪你休息”时宠物还在右下角那个小窗里。
    if app.get_webview_window("animal-overlay").is_some() {
        ANIMAL_ALIVE.store(false, Ordering::SeqCst);
        destroy_overlay_quietly(app, "animal-overlay");
        // 等销毁落地，避免 label 未释放导致新建失败
        std::thread::sleep(std::time::Duration::from_millis(150));
    }

    let peek = presence == "peek";
    // accompany / focus：铺满主屏（否则压暗层盖不住全屏，会露出窗口矩形边）
    // 原包日志写死 `1920x1080`，在 >1080p 的屏幕上就是这个缺陷的来源
    let (mx, my, sw, sh) = primary_monitor_logical(app);
    let (w, h) = if peek { (FILTER_W, FILTER_H) } else { (sw, sh) };
    // 预览模式：把后端已经解析好的 animal/presence 放进 query，
    // 否则窗口里的 `get_timer().animalId` 为空（没有真休息在跑），取不到宠物
    let url = if preview {
        format!(
            "index.html#/overlay/animal?preview=1&animal={}&presence={}",
            sanitize_id(animal_id),
            sanitize_id(presence)
        )
    } else {
        "index.html#/overlay/animal".to_string()
    };
    let win = overlay_window(app, "animal-overlay", &url, w, h, true)?;
    // 即便窗口被复用（如显示器变化），尺寸/位置也要跟随当前模式
    let _ = win.set_size(tauri::LogicalSize::new(w, h));

    let position = if peek {
        let x = mx + (sw - w).max(0.0) - PEEK_GAP_RIGHT;
        let y = my + (sh - h).max(0.0) - PEEK_GAP_BOTTOM;
        (x, y)
    } else {
        (mx, my) // 铺满主屏：非 peek 模式要盖住整块屏，否则压暗层会露出矩形边
    };
    let _ = win.set_position(tauri::LogicalPosition::new(position.0, position.1));
    compensate_invisible_frame(&win);
    let _ = win.set_ignore_cursor_events(false);
    let _ = win.show();

    ANIMAL_ALIVE.store(true, Ordering::SeqCst);
    log::info!(
        "[windows] pet overlay created at {},{} {}x{} peek={peek}",
        position.0 as i32,
        position.1 as i32,
        w as u32,
        h as u32,
    );
    let _ = animal_id;
    Ok(())
}

pub fn animal_overlay_alive() -> bool {
    ANIMAL_ALIVE.load(Ordering::SeqCst)
}

/// 关闭 overlay 且不让 `Destroyed` 事件被当成“用户跳过休息”
pub fn close_overlay_quietly(app: &tauri::AppHandle, label: &str) {
    SUPPRESS_OVERLAY_CLOSE_EVENT.store(true, Ordering::SeqCst);
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.close();
    }
}

/// 同上，但用 `destroy()` —— 立即释放 label，才能马上用同名重建。
/// （`close()` 要经 CloseRequested 异步流程，马上重建会拿到旧窗口 → 旧 URL/旧尺寸被复用）
pub fn destroy_overlay_quietly(app: &tauri::AppHandle, label: &str) {
    SUPPRESS_OVERLAY_CLOSE_EVENT.store(true, Ordering::SeqCst);
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.destroy();
    }
}

pub fn close_animal_overlay(app: &tauri::AppHandle) -> tauri::Result<()> {
    ANIMAL_ALIVE.store(false, Ordering::SeqCst);
    // 程序自己关窗，不应被下一次休息当成“用户关闭”
    SUPPRESS_OVERLAY_CLOSE_EVENT.store(true, Ordering::SeqCst);
    if let Some(win) = app.get_webview_window("animal-overlay") {
        let _ = win.close();
    }
    Ok(())
}

/// 桌面小云豆：贴左/右边缘，垂直位置 desktopEarY%
pub fn ensure_desktop_ear(app: &tauri::AppHandle, shared: &Shared) -> tauri::Result<()> {
    let (show, side, y_percent) = {
        let guard = lock(shared);
        (
            guard.config.show_desktop_ear,
            guard.config.desktop_ear_side.clone(),
            guard.config.desktop_ear_y,
        )
    };
    if let Some(win) = app.get_webview_window("desktop-ear") {
        if !show {
            let _ = win.close();
        }
        return Ok(());
    }
    if !show {
        return Ok(());
    }

    log::info!("[windows] creating the desktop ear");
    let win = match overlay_window(app, "desktop-ear", "index.html#/overlay/ear", EAR_W, EAR_H, true)
    {
        Ok(w) => w,
        Err(e) => {
            log::warn!("[windows] desktop ear failed: {e}");
            return Err(e);
        }
    };

    let (mx, my, sw, sh) = primary_monitor_logical(app);
    let x = mx + if side == "left" { 0.0 } else { (sw - EAR_W).max(0.0) };
    let y = my + sh * (y_percent.min(100) as f64) / 100.0;
    let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    compensate_invisible_frame(&win);
    let _ = win.set_ignore_cursor_events(false);
    Ok(())
}

// ---------------------------------------------------------------- 主窗口

pub fn show_main(app: &tauri::AppHandle) -> tauri::Result<()> {
    match app.get_webview_window("main") {
        Some(win) => {
            let _ = win.show();
            let _ = win.unminimize();
            let _ = win.set_focus();
            Ok(())
        }
        None => {
            log::warn!("[windows] show main failed: no main window");
            Err(tauri::Error::WindowNotFound)
        }
    }
}

pub fn hide_main(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
}

// ---------------------------------------------------------------- 系统状态

/// 前台窗口是否覆盖整个显示器（用于休息降级为“角落探头”）
pub fn fullscreen_active() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowRect, IsZoomed,
        };
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() || IsZoomed(hwnd).as_bool() {
            // 最大化的窗口多数情况即全屏播放器/游戏；再比对尺寸
            let mut rect = windows::Win32::Foundation::RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                let w = (rect.right - rect.left) as i64;
                let h = (rect.bottom - rect.top) as i64;
                return w >= 1920 && h >= 1000;
            }
            return false;
        }
    }
    false
}

/// 系统是否正忙（不建议打断），对应 `SHQueryUserNotificationState`
pub fn notification_busy() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Shell::{
            SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE,
            QUNS_RUNNING_D3D_FULL_SCREEN,
        };
        // windows 0.58：无参，返回 Result<QUERY_USER_NOTIFICATION_STATE>
        if let Ok(state) = SHQueryUserNotificationState() {
            return state == QUNS_BUSY
                || state == QUNS_RUNNING_D3D_FULL_SCREEN
                || state == QUNS_PRESENTATION_MODE;
        }
    }
    false
}

// ---------------------------------------------------------------- 托盘

pub fn build_tray(app: &tauri::AppHandle, shared: &Shared) -> tauri::Result<()> {
    if app.tray_by_id("main-tray").is_some() {
        return Ok(()); // 已创建
    }

    let rest = MenuItem::with_id(app, "rest", "立即休息", true, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "打开主界面", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        // 打开主界面 → 立即休息 → ─ → 退出
        &[&show, &rest, &sep, &quit],
    )?;

    let shared_for_tray = shared.clone();
    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("护眼宝")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            match id {
                "rest" => timer::start_now(app, &shared_for_tray),
                "show" => {
                    let _ = show_main(app);
                }
                "quit" => {
                    timer::shutdown();
                    crate::request_exit();
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            // 左键单击 -> 打开主界面（与菜单“打开主界面”一致）
            if let tauri::tray::TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, .. } = event {
                let app = tray.app_handle();
                let _ = show_main(app);
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

/// 托盘标题（预提醒用；部分平台忽略）
pub fn set_tray_title(app: &tauri::AppHandle, title: &str) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        if title.is_empty() {
            let _ = tray.set_tooltip(Some("护眼宝"));
        } else {
            let _ = tray.set_tooltip(Some(title));
        }
    }
}
