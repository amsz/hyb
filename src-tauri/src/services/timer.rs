//! 工作-休息计时（services::timer）。
//!
//! 两种计时口径：
//!   * 有效工作时长（`work_elapsed_secs` / `today.work_seconds`）
//!     —— 键鼠门控：最近一次输入距今 > `activeIdleSecs`（默认 30s）就不累加；
//!        到点才触发休息。
//!   * 在座时长（`session_elapsed_secs` / `today.session_seconds`）
//!     —— 墙钟：段内、未手动暂停即累加，仅当键鼠空闲超过 `idlePauseMinutes` 才停。
//! 两者都用真实时间差 dt 累加（非每秒写死 +1）。
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
use crate::state::{in_work_schedule, lock, Shared, TimerState};
use chrono::{Datelike, Timelike};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::Emitter;

const TICK: Duration = Duration::from_secs(1);

/// 单次 tick 最多按多少秒补账（线程被极端调度延迟时的兜底）
const MAX_DT: u64 = 60;

/// 最近一次算出来的键鼠活跃度（仅用于状态翻转时打日志）
static LAST_ACTIVE: AtomicBool = AtomicBool::new(true);

/// 距上次把 history.json 落盘累积的秒数（每 60s 存一次，防进程被杀丢当天数据）
static SAVE_ACC: AtomicU64 = AtomicU64::new(0);
const SAVE_EVERY: u64 = 60;

/// overlay 被前端关闭（点“好啦”/Esc）时置位，由 windows 层回调
pub static OVERLAY_CLOSED: AtomicBool = AtomicBool::new(false);

/// 托盘提示四态（`空闲中` 为本工程新增）：
///   `护眼宝 - 休息中 {mm:ss}` / `护眼宝 - 计时已暂停` / `护眼宝 - 距离休息还有 {n} 秒`
///   / `护眼宝 - 空闲中`（键鼠超过 `activeIdleSecs` 无动作，有效工时已停算）
static LAST_TRAY_TITLE: Mutex<String> = Mutex::new(String::new());

fn set_tray_title_once(app: &tauri::AppHandle, title: &str) {
    let mut last = LAST_TRAY_TITLE.lock().unwrap_or_else(|e| e.into_inner());
    if *last == title {
        return;
    }
    *last = title.to_string();
    windows::set_tray_title(app, title);
}

/// 由“最近一次键鼠输入距今秒数”算出两个判定：
///   * `active`  → 是否计入**有效工作时长**（超过 `active_limit` 秒无输入就停）
///   * `seated`  → 是否计入**在座时长**（超过 `away_limit` 秒无输入才停）
/// `limit == 0` 表示该门控关闭。
///
/// 边界：`active` 用 `<=`（到 30s 整那一秒仍算在内），`seated` 用 `<`
///（与原包 `idle >= idle_pause_minutes*60 → 暂停` 的语义保持一致）。
fn activity_verdict(idle: u64, active_limit: u64, away_limit: u64) -> (bool, bool) {
    let active = active_limit == 0 || idle <= active_limit;
    let seated = away_limit == 0 || idle < away_limit;
    (active, seated)
}

pub fn spawn(app: tauri::AppHandle, shared: Shared) {
    std::thread::spawn(move || {
        let mut prev = monotonic_secs();
        loop {
            std::thread::sleep(TICK);
            let now = monotonic_secs();
            // 用真实时间差累加（而不是每秒写死 +1）：tick 被锁争用/调度延迟拖慢时不会少记。
            // GetTickCount64 不累计睡眠时间，合盖/休眠回来 dt 仍是 1，不会凭空补账。
            let dt = now.saturating_sub(prev).min(MAX_DT);
            prev = now;
            if dt == 0 {
                continue;
            }
            if let Err(e) = tick(&app, &shared, dt) {
                log::error!("[timer] tick failed: {e}");
            }
        }
    });
}

/// 单调秒。Windows 用 GetTickCount64（与 GetLastInputInfo.dwTime 同一时基，均不含睡眠）
fn monotonic_secs() -> u64 {
    #[cfg(windows)]
    unsafe {
        use ::windows::Win32::System::SystemInformation::GetTickCount64;
        return GetTickCount64() / 1000;
    }
    #[cfg(not(windows))]
    {
        use std::sync::OnceLock;
        use std::time::Instant;
        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        ORIGIN.get_or_init(Instant::now).elapsed().as_secs()
    }
}

/// 最近一次键鼠输入距今秒数（GetLastInputInfo，用于“键鼠门控”与“离开判定”）
///
/// `dwTime` 是 32 位 tick，这里取 `GetTickCount64()` 的低 32 位做同域 wrapping 差，
/// 避免 49.7 天回绕和 32/64 位混算。
fn idle_seconds() -> u64 {
    #[cfg(windows)]
    unsafe {
        // 注意：本模块已 `use crate::services::windows`，需用 `::windows` 显式指向 extern crate
        use ::windows::Win32::System::SystemInformation::GetTickCount64;
        use ::windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info).as_bool() {
            let now32 = GetTickCount64() & 0xffff_ffff;
            return now32.wrapping_sub(u64::from(info.dwTime)) / 1000;
        }
    }
    0
}

/// 按 strategy 选一只宠物（random | dailyRotate | fixed | byTimeSlot）。
///
/// `enabledAnimalIds`（宠物页复选框）只作用于随机类策略：
/// 勾了若干只 → 在这几只里随机；一只都没勾 → 全部宠物参与。
pub(crate) fn pick_animal(shared: &Shared) -> String {
    let (strategy, wanted, pool) = {
        let guard = lock(shared);
        (
            guard.config.animal_strategy.clone(),
            guard.config.selected_animal_id.clone(),
            guard.config.enabled_animal_ids.clone(),
        )
    };
    let all = animals::list();
    if all.is_empty() {
        return String::new();
    }
    // 池子里只留还存在的宠物；勾选的都不在了就当没勾（fail-open，别让休息没宠物看）
    let picked: Vec<animals::Animal> = if pool.is_empty() {
        Vec::new()
    } else {
        all.iter().filter(|a| pool.iter().any(|id| id == &a.id)).cloned().collect()
    };
    let list: &[animals::Animal] = if picked.is_empty() { &all } else { &picked };

    match strategy.as_str() {
        "fixed" if all.iter().any(|a| a.id == wanted) => wanted,
        "dailyRotate" => {
            let day = chrono::Local::now().date_naive().num_days_from_ce() as usize;
            list[day % list.len()].id.clone()
        }
        "byTimeSlot" => {
            let hour = chrono::Local::now().hour() as usize;
            list[(hour / 4) % list.len()].id.clone()
        }
        _ => {
            let idx = pseudo_random_index(list.len());
            list[idx].id.clone()
        }
    }
}

/// 无 rand 依赖时的随机下标：纳秒时间戳 → 混淆后再取模，避免固定毫秒造成的偏向。
fn pseudo_random_index(len: usize) -> usize {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let spiced = nanos
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .rotate_left(17)
        ^ std::process::id() as u64;
    (spiced >> 8) as usize % len
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
            activity: "active".into(),
            session_elapsed_secs: 0,
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
            activity: "active".into(),
            session_elapsed_secs: 0,
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

fn tick(app: &tauri::AppHandle, shared: &Shared, dt: u64) -> Result<(), crate::error::AppError> {
    // 跨天结算 + 护眼时长按秒累计
    {
        let mut guard = lock(shared);
        guard.roll_day();
        if guard.config.eye_enabled {
            if let Some(today) = guard.history.last_mut() {
                today.eye_filter_seconds += dt;
            }
        }
    }

    maybe_flush_history(shared, dt);

    // ---------- 休息阶段 ----------
    if lock(shared).rest_running {
        if OVERLAY_CLOSED.swap(false, Ordering::SeqCst) {
            log::warn!("[rest] overlay closed while the rest was still running; ending it");
            end_rest(app, shared);
            return Ok(());
        }
        let done = {
            let mut guard = lock(shared);
            guard.timer.rest_remaining_secs = guard.timer.rest_remaining_secs.saturating_sub(dt);
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
    let (paused, away_limit, active_limit, pre_remind, pre_secs) = {
        let guard = lock(shared);
        (
            guard.config.timer_paused,
            guard.config.idle_pause_minutes as u64 * 60,
            u64::from(guard.config.active_idle_secs),
            guard.config.pre_remind,
            guard.config.pre_remind_seconds,
        )
    };

    // 工作时间段（休息计划只在段内执行；段外工作计时清零）
    let in_schedule = {
        let guard = lock(shared);
        in_work_schedule(&guard.config, &chrono::Local::now())
    };

    let idle = idle_seconds();
    // 键鼠门控：最近一次键鼠输入距今超过 active_limit 秒 → 算“没在工作”，不计有效工时。
    // 注意不回补：恢复输入后从当前秒继续累加，离开期间的时间永远不会被追认。
    let (active, seated) = activity_verdict(idle, active_limit, away_limit);

    if LAST_ACTIVE.swap(active, Ordering::SeqCst) != active {
        log::info!(
            "[activity] verdict={} idle={}s gate={}s",
            if active { "active" } else { "idle" },
            idle,
            active_limit
        );
    }

    let (finished, remaining) = {
        let mut guard = lock(shared);
        guard.timer.activity = if active { "active" } else { "idle" }.into();

        if paused {
            // 手动暂停：保留已累计的工作时间，回到段内仍处于暂停态
            guard.timer.state = "paused".into();
        } else if !in_schedule {
            // 段外：倒计时清零，不累计工作时长（休息不可能在段外自动触发）
            guard.timer.state = "outsideSchedule".into();
            guard.timer.work_elapsed_secs = 0;
            guard.timer.session_elapsed_secs = 0;
        } else {
            guard.timer.state = "working".into();
            if active {
                guard.timer.work_elapsed_secs += dt;
            }
            if seated {
                guard.timer.session_elapsed_secs += dt;
            }
            if active || seated {
                guard.ensure_today();
                if let Some(today) = guard.history.last_mut() {
                    if active {
                        today.work_seconds += dt;
                    }
                    if seated {
                        today.session_seconds += dt;
                    }
                }
            }
        }
        guard.timer.work_target_secs = u64::from(guard.config.work_minutes) * 60;
        guard.timer.next_rest_in_secs =
            guard.timer.work_target_secs.saturating_sub(guard.timer.work_elapsed_secs);
        // 到点只在“人真的在”的时候触发；空闲冻结期间即便正好跨过目标也等恢复输入再休息
        let finished = !paused
            && in_schedule
            && active
            && guard.timer.work_elapsed_secs >= guard.timer.work_target_secs;
        (finished, guard.timer.next_rest_in_secs)
    };

    // 托盘提示：手动暂停 / 段外 / 空闲 / 预提醒（preRemind，仅后端，无设置项 UI）/ 恢复默认
    let title = if paused {
        "护眼宝 - 计时已暂停".to_string()
    } else if !in_schedule {
        "护眼宝 - 时段外，计划已暂停".to_string()
    } else if !active {
        "护眼宝 - 空闲中".to_string()
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

/// 把当日统计落盘：退出时调一次，tick 里每 60s 调一次。
/// 原来只在“休息结束 / 跨天”写盘，进程被关掉就丢了当天还没结算的有效工时与在座时长。
pub fn flush_history(shared: &Shared) {
    let mut guard = lock(shared);
    guard.ensure_today();
    if let Err(e) = crate::state::save_history(&guard.history) {
        log::warn!("[stats] save failed: {e}");
    }
}

fn maybe_flush_history(shared: &Shared, dt: u64) {
    if SAVE_ACC.fetch_add(dt, Ordering::Relaxed) + dt < SAVE_EVERY {
        return;
    }
    SAVE_ACC.store(0, Ordering::Relaxed);
    flush_history(shared);
}

#[cfg(test)]
mod tests {
    use super::activity_verdict;

    /// 默认参数：键鼠门控 30s、离开判定 5min
    const GATE: u64 = 30;
    const AWAY: u64 = 300;

    #[test]
    fn active_while_typing() {
        assert_eq!(activity_verdict(0, GATE, AWAY), (true, true));
        assert_eq!(activity_verdict(1, GATE, AWAY), (true, true));
    }

    #[test]
    fn gate_closes_exactly_after_limit() {
        // 第 30 秒仍算工作，第 31 秒开始停算有效工时，但仍算在座
        assert_eq!(activity_verdict(30, GATE, AWAY), (true, true));
        assert_eq!(activity_verdict(31, GATE, AWAY), (false, true));
    }

    #[test]
    fn seated_survives_longer_than_active() {
        assert_eq!(activity_verdict(299, GATE, AWAY), (false, true));
        // 到 5 分钟整就不再见座（与原包 idle_pause_minutes 语义一致）
        assert_eq!(activity_verdict(300, GATE, AWAY), (false, false));
        assert_eq!(activity_verdict(3600, GATE, AWAY), (false, false));
    }

    #[test]
    fn zero_disables_the_gate() {
        assert_eq!(activity_verdict(10_000, 0, 0), (true, true));
        // 只关工时门控、留着离开判定
        assert_eq!(activity_verdict(10_000, 0, AWAY), (true, false));
    }
}
