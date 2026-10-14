//! 击键/点击计数（services::activity）。
//!
//! 只计数、不记录内容（`counters only`），由 `inputStatsEnabled` 控制：
//!   `[activity] installing input hook (counters only)`
//!   `[activity] input-stats hook off, not installing`
//!   `[activity] input hook failed, key/click counters disabled: …`
//!
//! 注意：全局低级钩子进程内必须持续跑消息泵；部分杀软会告警（前端文案已提示）。

use crate::state::Shared;
use std::sync::atomic::{AtomicBool, Ordering};

static RUNNING: AtomicBool = AtomicBool::new(false);

pub fn start(shared: Shared) {
    let enabled = { crate::state::lock(&shared).config.input_stats_enabled };
    if !enabled {
        log::info!("[activity] input-stats hook off, not installing");
        return;
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    log::info!("[activity] installing input hook (counters only)");

    std::thread::spawn(move || {
        if let Err(e) = hook_thread(shared) {
            log::warn!("[activity] input hook failed, key/click counters disabled: {e}");
        }
    });
}

#[cfg(windows)]
fn hook_thread(shared: Shared) -> Result<(), String> {
    use std::sync::atomic::AtomicIsize;
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, MSG, SetWindowsHookExW, WH_KEYBOARD_LL, WH_MOUSE_LL,
    };

    static SHARED_PTR: AtomicIsize = AtomicIsize::new(0);
    SHARED_PTR.store(
        Box::into_raw(Box::new(shared)) as isize,
        Ordering::SeqCst,
    );
    fn with_shared(f: impl FnOnce(&Shared)) {
        let ptr = SHARED_PTR.load(Ordering::SeqCst) as *const Shared;
        if !ptr.is_null() {
            unsafe { f(&*ptr) }
        }
    }

    static KEY_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    static CLICK_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    unsafe extern "system" fn keyboard_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code >= 0 {
            KEY_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            with_shared(|s| {
                let mut guard = crate::state::lock(s);
                guard.ensure_today();
                if let Some(today) = guard.history.last_mut() {
                    today.keystrokes += 1;
                }
            });
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            // WM_LBUTTONDOWN / WM_RBUTTONDOWN / WM_MBUTTONDOWN
            const DOWN: [u32; 3] = [0x0201, 0x0204, 0x0207];
            let kind = (wparam.0 & 0xffff) as u32;
            if DOWN.contains(&kind) {
                CLICK_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                with_shared(|s| {
                    let mut guard = crate::state::lock(s);
                    guard.ensure_today();
                    if let Some(today) = guard.history.last_mut() {
                        today.mouse_clicks += 1;
                    }
                });
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    unsafe {
        // 低级钩子的 hmod 可以传 None（在本线程回调）
        let kh = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0)
            .map_err(|e| format!("keyboard hook: {e}"))?;
        let mh = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0)
            .map_err(|e| format!("mouse hook: {e}"))?;

        let mut msg = std::mem::zeroed::<MSG>();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            // 消息泵必须一直跑，否则钩子不回调
        }
        let _ = kh;
        let _ = mh;
        let _ = HINSTANCE::default();
    }
    Ok(())
}

#[cfg(not(windows))]
fn hook_thread(_shared: Shared) -> Result<(), String> {
    Err("input hook is only implemented on Windows".into())
}
