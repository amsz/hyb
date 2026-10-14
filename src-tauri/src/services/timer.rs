//! 工作-休息计时（services::timer）。
//!
//! 日志字面量来自 exe：
//!   `[rest] starting: animal=<id> presence=<P> long=<bool> minutes=<n> library=<n>`
//!   `[rest] a fullscreen app is up, using the corner pet instead of …`
//!   `[rest] deferred <n>s: the OS says this is a bad moment to interrupt`
//!   `[rest] due, but the pet reminder is off`
//!   `[rest] the pet library is empty; the overlay will have nothing to play`
//!   `[rest] overlay closed while the rest was still running; ending it`
//!   `[stats] filing <昨天> and starting <今天>`
//! 事件：`timer:tick` `rest:start` `rest:end`

use crate::services::{animals, bluelight, windows};
use crate::state::{lock, Shared, TimerState};
use chrono::{Datelike, Timelike};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::Emitter;

const TICK: Duration = Duration::from_secs(1);

/// overlay 被前端关闭（点“好啦”/Esc）时置位，由 windows 层回调
pub static OVERLAY_CLOSED: AtomicBool = AtomicBool::new(false);

/// 托盘提示三态（原包 rodata 中的字面量）：
///   `护眼宝 - 休息中 {mm:ss}` / `护眼宝 - 计时已暂停` / `护眼宝 - 距离休息还有 {n} 秒`
static LAST_TRAY_TITLE: Mutex<String> = Mutex::new(String::new());

fn set_tray_title_once(app: &tauri::AppHandle, title: &str) {
    let mut last = LAST_TRAY_TITLE.lock().unwrap_or_else(|e| e.into_inner());
    if *last == title {
        return;
    }
    *last = title.to_string();
    windows::set_tray_title(app, title);
}

pub fn spawn(app: tauri::AppHandle, shared: Shared) {
    std::thread::spawn(move || loop {
        if let Err(e) = tick(&app, &shared) {
            log::error!("[timer] tick failed: {e}");
        }
        std::thread::sleep(TICK);
    });
}

/// 最近一次键鼠输入距今秒数（GetLastInputInfo，用于“离开暂停”）
fn idle_seconds() -> u64 {
    #[cfg(windows)]
    unsafe {
        // 注意：本模块已 `use crate::services::windows`，需用 `::windows` 显式指向 extern crate
        use ::windows::Win32::System::SystemInformation::GetTickCount;
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info).as_bool() {
            return (GetTickCount().wrapping_sub(info.dwTime) / 1000) as u64;
        }
    }
    0
}

/// 按 strategy 选一只宠物（random | dailyRotate | fixed | byTimeSlot）
fn pick_animal(shared: &Shared) -> String {
    let (strategy, wanted) = {
        let guard = lock(shared);
        (guard.config.animal_strategy.clone(), guard.config.selected_animal_id.clone())
    };
    let list = animals::list();
    if list.is_empty() {
        return String::new();
    }
    match strategy.as_str() {
        "fixed" if list.iter().any(|a| a.id == wanted) => wanted,
        "dailyRotate" => {
            let day = chrono::Local::now().date_naive().num_days_from_ce() as usize;
            list[day % list.len()].id.clone()
        }
        "byTimeSlot" => {
            let hour = chrono::Local::now().hour() as usize;
            list[(hour / 4) % list.len()].id.clone()
        }
        _ => {
            let idx = chrono::Local::now().timestamp_subsec_millis() as usize % list.len();
            list[idx].id.clone()
        }
    }
}

fn emit_tick(app: &tauri::AppHandle, shared: &Shared) {
    let state = lock(shared).timer.clone();
    let _ = app.emit("timer:tick", &state);
}

fn start_rest(app: &tauri::AppHandle, shared: &Shared) {
    let (pet_enabled, presence_cfg) = {
        let mut guard = lock(shared);
        guard.ensure_today();
        (guard.config.rest_pet_enabled, guard.config.rest_presence.clone())
    };

    if !pet_enabled {
        log::info!("[rest] due, but the pet reminder is off");
        let mut guard = lock(shared);
        guard.timer.work_elapsed_secs = 0;
        guard.timer.state = "working".into();
        guard.timer.next_rest_in_secs = u64::from(guard.config.work_minutes) * 60;
        return;
    }

    let library = animals::list().len();
    if library == 0 {
        log::warn!("[rest] the pet library is empty; the overlay will have nothing to play");
    }

    let animal_id = pick_animal(shared);

    // 长休：以“已完成 + 跳过”的休息次数为周期（longRestEvery）
    let (is_long, minutes) = {
        let guard = lock(shared);
        let today = guard.today_stats();
        let cycle = today.rest_completed + today.rest_skipped;
        let every = guard.config.long_rest_every;
        let is_long = every > 0 && cycle > 0 && cycle % every as u64 == 0;
        let minutes = if is_long { guard.config.long_rest_minutes } else { guard.config.short_rest_minutes };
        (is_long, minutes)
    };

    log::info!(
        "[rest] starting: animal={animal_id} presence={presence_cfg} long={is_long} \
         minutes={minutes} library={library}"
    );

    // 全屏应用在前台 -> 降级为角落探头；系统正忙 -> 推迟打断
    let presence = if windows::fullscreen_active() {
        log::info!("[rest] a fullscreen app is up, using the corner pet instead of full screen");
        "peek".to_string()
    } else {
        presence_cfg
    };
    if windows::notification_busy() {
        let defer = 30u64;
        log::info!("[rest] deferred {defer}s: the OS says this is a bad moment to interrupt");
        std::thread::sleep(Duration::from_secs(defer));
    }

    {
        let mut guard = lock(shared);
        guard.rest_running = true;
        guard.skipped_this_rest = false;
        guard.timer = TimerState {
            state: "animalShowing".into(),
            work_elapsed_secs: 0,
            work_target_secs: u64::from(guard.config.work_minutes) * 60,
            rest_remaining_secs: u64::from(minutes) * 60,
            rest_total_secs: u64::from(minutes) * 60,
            is_long_rest: is_long,
            next_rest_in_secs: 0,
            animal_id: animal_id.clone(),
            presence: presence.clone(),
        };
    }

    if library > 0 {
        if let Err(e) = windows::show_animal_overlay(app, &animal_id, &presence, false) {
            log::warn!("[windows] pet overlay failed: {e}");
        }
    } else {
        // 没有宠物可播时仍然进入休息倒计时（只是不弹窗）
        let _ = windows::close_animal_overlay(app);
    }

    let state = lock(shared).timer.clone();
    let _ = app.emit("rest:start", &state);
    let _ = app.emit("timer:tick", &state);
}

fn end_rest(app: &tauri::AppHandle, shared: &Shared) {
    {
        let mut guard = lock(shared);
        guard.rest_running = false;
        guard.ensure_today();
        let skipped = guard.skipped_this_rest;
        if let Some(today) = guard.history.last_mut() {
            if skipped {
                today.rest_skipped += 1;
            } else {
                today.rest_completed += 1;
            }
        }
        let work_target = u64::from(guard.config.work_minutes) * 60;
        let presence = guard.config.rest_presence.clone();
        guard.timer = TimerState {
            state: "working".into(),
            work_elapsed_secs: 0,
            work_target_secs: work_target,
            rest_remaining_secs: 0,
            rest_total_secs: 0,
            is_long_rest: false,
            next_rest_in_secs: work_target,
            animal_id: String::new(),
            presence,
        };
        let _ = crate::state::save_history(&guard.history);
        log::info!(
            "[rest] ended: {} completed / {} skipped today, next work target {}s",
            guard
                .history
                .last()
                .map(|s| s.rest_completed)
                .unwrap_or(0),
            guard
                .history
                .last()
                .map(|s| s.rest_skipped)
                .unwrap_or(0),
            work_target
        );
    }
    let _ = windows::close_animal_overlay(app);
    set_tray_title_once(app, "");
    let state = lock(shared).timer.clone();
    let _ = app.emit("rest:end", &state);
    let _ = app.emit("timer:tick", &state);
}

fn tick(app: &tauri::AppHandle, shared: &Shared) -> Result<(), crate::error::AppError> {
    // 跨天结算 + 护眼时长按秒累计
    {
        let mut guard = lock(shared);
        guard.roll_day();
        if guard.config.eye_enabled {
            if let Some(today) = guard.history.last_mut() {
                today.eye_filter_seconds += 1;
            }
        }
    }

    // ---------- 休息阶段 ----------
    if lock(shared).rest_running {
        if OVERLAY_CLOSED.swap(false, Ordering::SeqCst) {
            log::warn!("[rest] overlay closed while the rest was still running; ending it");
            end_rest(app, shared);
            return Ok(());
        }
        let done = {
            let mut guard = lock(shared);
            if guard.timer.rest_remaining_secs > 0 {
                guard.timer.rest_remaining_secs -= 1;
            }
            guard.timer.rest_remaining_secs == 0
        };
        emit_tick(app, shared);
        // 休息期间的托盘提示：护眼宝 - 休息中 mm:ss
        let remaining = lock(shared).timer.rest_remaining_secs;
        set_tray_title_once(
            app,
            &format!(
                "护眼宝 - 休息中 {:02}:{:02}",
                remaining / 60,
                remaining % 60
            ),
        );
        if done {
            end_rest(app, shared);
        }
        return Ok(());
    }

    // ---------- 工作阶段 ----------
    let (paused, idle_limit, pre_remind, pre_secs) = {
        let guard = lock(shared);
        (
            guard.config.timer_paused,
            guard.config.idle_pause_minutes as u64 * 60,
            guard.config.pre_remind,
            guard.config.pre_remind_seconds,
        )
    };

    let idle = idle_seconds();
    let idle_paused = idle_limit > 0 && idle >= idle_limit;

    let (finished, remaining) = {
        let mut guard = lock(shared);
        if paused || idle_paused {
            guard.timer.state = "paused".into();
        } else {
            guard.timer.state = "working".into();
            guard.timer.work_elapsed_secs += 1;
            guard.ensure_today();
            if let Some(today) = guard.history.last_mut() {
                today.work_seconds += 1;
            }
        }
        guard.timer.work_target_secs = u64::from(guard.config.work_minutes) * 60;
        guard.timer.next_rest_in_secs =
            guard.timer.work_target_secs.saturating_sub(guard.timer.work_elapsed_secs);
        let finished =
            !paused && !idle_paused && guard.timer.work_elapsed_secs >= guard.timer.work_target_secs;
        (finished, guard.timer.next_rest_in_secs)
    };

    // 托盘提示：暂停 / 预提醒（preRemind，仅后端，无设置项 UI）/ 恢复默认
    let title = if paused {
        "护眼宝 - 计时已暂停".to_string()
    } else if pre_remind && remaining > 0 && remaining <= u64::from(pre_secs) {
        format!("护眼宝 - 距离休息还有 {remaining} 秒")
    } else {
        String::new()
    };
    set_tray_title_once(app, &title);

    emit_tick(app, shared);
    if finished {
        let s = lock(shared).timer.clone();
        log::info!(
            "[rest] work phase finished: elapsed={}s target={}s",
            s.work_elapsed_secs,
            s.work_target_secs
        );
    }
    if finished {
        start_rest(app, shared);
    }
    Ok(())
}

/// 命令层入口：立即开始休息
pub fn start_now(app: &tauri::AppHandle, shared: &Shared) {
    if lock(shared).rest_running {
        return;
    }
    start_rest(app, shared);
}

/// 命令层入口：跳过本次休息（计入 restSkipped）
pub fn skip(app: &tauri::AppHandle, shared: &Shared) {
    if !{ lock(shared).rest_running } {
        return;
    }
    {
        let mut guard = lock(shared);
        guard.skipped_this_rest = true;
    }
    end_rest(app, shared);
}

/// 退出时恢复系统色温
pub fn shutdown() {
    bluelight::reset();
}
