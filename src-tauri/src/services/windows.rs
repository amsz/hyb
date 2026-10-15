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
    Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};

const FILTER_W: f64 = 640.0;
const FILTER_H: f64 = 400.0;
const EAR_W: f64 = 150.0;
const EAR_H: f64 = 170.0;
/// 豆脸在 peek 姿势下伸出窗口边缘的 CSS 像素（前端 CSS `right/left:-12px`）。
/// 单屏时这部分被屏幕边缘裁掉是设计（“只探出一点小脸”）；但双屏相邻时会画到副屏上，
/// 所以定位时要把窗口往屏内收这么多，保证豆脸完整落在目标屏内。
const EAR_PEEK_OVERHANG_CSS: f64 = 12.0;

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
        // ⚠ 必须先隐藏创建：如果 visible(true)，窗口会先出现在 Windows 默认级联位置
        //   （屏幕左上区域），调用方 set_position 后才挪走 → 用户看到“左上角闪一个小窗”。
        //   统一改为：隐藏创建 → 调用方定好位 → 再 show()。
        .visible(false)
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
    // 窗口现在是隐藏重建的；按当前 alpha 恢复显隐（不重跑 gamma，只管窗口）
    let state = app.state::<Shared>();
    let alpha = lock(&state).eye.alpha;
    set_filter_overlay_visible(app, alpha > 0.0);
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

/// 主显示器的**物理**矩形 `(x, y, w, h)` + scale factor；拿不到时 None。
/// 定位小云豆必须用物理坐标：`set_position(LogicalPosition)` 会按窗口当前所在屏的
/// scale 换算，双屏不同 DPI 时会摆错位置（实测主屏 1.25）。
fn primary_monitor_physical(app: &tauri::AppHandle) -> Option<(i32, i32, i32, i32, f64)> {
    let mon = app
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| app.available_monitors().ok().and_then(|ms| ms.into_iter().next()))?;
    let p = mon.position();
    let s = mon.size();
    let scale = if mon.scale_factor() > 0.0 { mon.scale_factor() } else { 1.0 };
    Some((p.x, p.y, s.width as i32, s.height as i32, scale))
}

/// (x, y, w, h) 是否至少有 (40, 40) 落在任意一个显示器内（物理坐标）
fn window_visible_enough(app: &tauri::AppHandle, x: i32, y: i32, w: i32, h: i32) -> bool {
    let Ok(mons) = app.available_monitors() else { return false };
    mons.iter().any(|m| {
        let p = m.position();
        let s = m.size();
        let (mx, my, mw, mh) = (p.x, p.y, s.width as i32, s.height as i32);
        ((x + w).min(mx + mw) - x.max(mx) > 40) && ((y + h).min(my + mh) - y.max(my) > 40)
    })
}

/// 把 (x,y) 夹紧到与它重叠最多的那个显示器内（物理坐标，按窗口尺寸）
fn clamp_to_visible_monitor(
    app: &tauri::AppHandle,
    x: i32,
    y: i32,
    size: tauri::PhysicalSize<u32>,
) -> (i32, i32) {
    let (w, h) = (size.width as i32, size.height as i32);
    let Ok(mons) = app.available_monitors() else { return (x, y) };
    let mut best: Option<(i32, i32, i32, i32)> = None;
    let mut best_ov = i32::MIN;
    for m in &mons {
        let p = m.position();
        let s = m.size();
        let (mx, my, mw, mh) = (p.x, p.y, s.width as i32, s.height as i32);
        let ox = ((x + w).min(mx + mw) - x.max(mx)).max(0);
        let oy = ((y + h).min(my + mh) - y.max(my)).max(0);
        if ox * oy > best_ov {
            best_ov = ox * oy;
            best = Some((mx, my, mw, mh));
        }
    }
    match best {
        Some((mx, my, mw, mh)) => (
            x.clamp(mx, (mx + mw - w).max(mx)),
            y.clamp(my, (my + mh - h).max(my)),
        ),
        None => (x, y),
    }
}

/// 默认摆位：窗口外框贴主屏边缘，并为豆脸的 peek 伸出留出余量，
/// 保证窗口和豆脸都完整落在目标屏内（双屏相邻也不越界）。
fn place_ear_at_edge(
    app: &tauri::AppHandle,
    win: &tauri::WebviewWindow,
    side: &str,
    y_percent: u32,
) {
    let Some((mx, my, mw, mh, scale)) = primary_monitor_physical(app) else {
        log::warn!("[windows] no monitor for desktop ear placement");
        return;
    };
    let overhang = (EAR_PEEK_OVERHANG_CSS * scale).round() as i32;
    let Ok(size) = win.outer_size() else { return };
    let (w, h) = (size.width as i32, size.height as i32);
    let x = if side == "left" {
        mx + overhang
    } else {
        (mx + mw - overhang - w).max(mx)
    };
    let y = (my + (mh as f64 * y_percent.min(100) as f64 / 100.0) as i32).min(my + mh - h);
    log::info!("[windows] desktop ear placed at {x},{y} ({side}, y={y_percent}% of {mw}x{mh}@{scale})");
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
}

/// 桌面小云豆：优先用上次拖拽记住的位置，否则贴主屏边缘。
/// 坐标全部用**物理**像素：LogicalPosition 按窗口当前所在屏的 scale 换算，双屏异 DPI 会摆错。
pub fn ensure_desktop_ear(app: &tauri::AppHandle, shared: &Shared) -> tauri::Result<()> {
    let (show, side, y_percent, saved) = {
        let guard = lock(shared);
        (
            guard.config.show_desktop_ear,
            guard.config.desktop_ear_side.clone(),
            guard.config.desktop_ear_y,
            (guard.config.desktop_ear_px, guard.config.desktop_ear_py),
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

    // 记住的位置：只要还有足够的部分在某个屏幕内就用它（换屏/改分辨率后自动回退默认摆位）
    if let (Some(px), Some(py)) = saved {
        if let Ok(size) = win.outer_size() {
            if window_visible_enough(app, px, py, size.width as i32, size.height as i32) {
                log::info!("[windows] desktop ear restored at {px},{py} (dragged position)");
                let _ = win.set_position(tauri::PhysicalPosition::new(px, py));
                let _ = win.set_ignore_cursor_events(false);
                let _ = win.show();
                return Ok(());
            }
            log::warn!(
                "[windows] saved desktop ear position {px},{py} is off-screen; using edge placement"
            );
        }
    }
    place_ear_at_edge(app, &win, &side, y_percent);
    let _ = win.set_ignore_cursor_events(false);
    let _ = win.show();
    Ok(())
}

// ---------------------------------------------------------------- 小云豆拖拽
//
// 后端用一个后台线程轮询光标位置来拖动窗口：
// 前端只负责在超过阈值时调 begin、松手时调 end（pointer capture 保证窗移出后仍能收到 pointerup），
// 不逐帧发 IPC。end 时把最终外框位置（物理坐标）写进 config 持久化。

static EAR_DRAG_STOP: AtomicBool = AtomicBool::new(false);
static EAR_DRAG_RUNNING: AtomicBool = AtomicBool::new(false);
/// 当前抓取偏移（光标 - 窗口外框），线程每帧读取；begin 时重算，
/// 这样即使上一次拖拽线程因异常未退干净，重新 begin 也能拿到正确的偏移。
static EAR_DRAG_OFFSET: std::sync::Mutex<(i32, i32)> = std::sync::Mutex::new((0, 0));

fn cursor_pos_physical() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p).ok()?; }
    Some((p.x, p.y))
}

/// 开始拖拽：记录抓取偏移，后台线程每 ~8ms 把窗口外框贴到光标上。
pub fn begin_ear_drag(app: &tauri::AppHandle) {
    let Some(win) = app.get_webview_window("desktop-ear") else { return };
    let Some((cx, cy)) = cursor_pos_physical() else { return };
    let Ok(origin) = win.outer_position() else { return };
    let (dx, dy) = (cx - origin.x, cy - origin.y);
    if let Ok(mut off) = EAR_DRAG_OFFSET.lock() {
        *off = (dx, dy);
    }
    log::debug!("[windows] desktop ear drag begin at {cx},{cy} grab-offset {dx},{dy}");
    if EAR_DRAG_RUNNING.swap(true, Ordering::SeqCst) {
        return; // 线程已在跑，只需更新偏移
    }
    EAR_DRAG_STOP.store(false, Ordering::SeqCst);
    let app_handle = app.clone();
    std::thread::spawn(move || {
        while !EAR_DRAG_STOP.load(Ordering::SeqCst) {
            if app_handle.get_webview_window("desktop-ear").is_none() {
                break; // 窗口在拖拽中被销毁（如关掉小云豆）→ 线程自退
            }
            if let (Some((cx, cy)), Ok(off)) = (cursor_pos_physical(), EAR_DRAG_OFFSET.lock()) {
                let _ = win.set_position(tauri::PhysicalPosition::new(cx - off.0, cy - off.1));
            }
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        EAR_DRAG_RUNNING.store(false, Ordering::SeqCst);
    });
}

/// 结束拖拽：停线程 → 夹紧到可见屏 → 写进 config 持久化。
pub fn end_ear_drag(app: &tauri::AppHandle, shared: &Shared) {
    EAR_DRAG_STOP.store(true, Ordering::SeqCst);
    // 等拖拽线程退出（每 8ms 检查一次，最多 ~100ms）
    for _ in 0..20 {
        if !EAR_DRAG_RUNNING.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let Some(win) = app.get_webview_window("desktop-ear") else { return };
    let Ok(pos) = win.outer_position() else { return };
    let (x, y) = match win.outer_size() {
        Ok(size) => clamp_to_visible_monitor(app, pos.x, pos.y, size),
        Err(_) => (pos.x, pos.y),
    };
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
    {
        let mut guard = lock(shared);
        guard.config.desktop_ear_px = Some(x);
        guard.config.desktop_ear_py = Some(y);
        let _ = crate::state::save_config(&guard.config);
    }
    let _ = app.emit("config:changed", lock(shared).config.clone());
    log::info!("[windows] desktop ear dragged to {x},{y}, position saved");
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

    let shared_for_rest = shared.clone();
    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("护眼宝")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            match id {
                // ⚠ 托盘菜单事件在主线程回调；start_now → show_animal_overlay →
                //   WebviewWindowBuilder::build() 在主线程创建 WebView2 会死锁
                //   （窗口壳变白底、页面 about:blank、主线程卡死、退出也失效）。
                //   丢到后台线程里跑，窗口创建会正常往返主循环。
                "rest" => {
                    let app = app.clone();
                    let shared = shared_for_rest.clone();
                    std::thread::spawn(move || timer::start_now(&app, &shared));
                }
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
