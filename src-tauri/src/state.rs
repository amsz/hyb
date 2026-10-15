//! 配置与运行时状态持久化。
//!
//! 证据来源：
//!  - `%APPDATA%\top.huyanbao.desktop\config.json`（本机 configVersion=3，44 个字段）
//!  - `history.json`（每日统计）、`install_id.txt`（UUID）
//!  - exe 字符串：`[config] migrated from version N to M`、
//!    `json.corrupt`、`falling back to defaults, bad copy kept at …`
//!
//! ⚠ 当前为开发模式：目录名与 tauri.conf.json 的 identifier 都改成了 `top.huyanbao.dev`，
//!   以免与已安装的护眼宝（`top.huyanbao.desktop`）共用 config/热键/计划任务。
//!   发布时两者都要改回 `top.huyanbao.desktop`。
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 与 tauri.conf.json 的 identifier 保持一致
pub const APP_DIR_NAME: &str = "top.huyanbao.dev";

/// 是否允许与 huyanbao.cc / api.huyanbao.cc / pets.huyanbao.workers.dev 通讯。
/// 当前为 **false（离线）**：不拉宠物库、不检查更新、不上报、不注入 tongji.js。
/// 发布时改回 `true` 即可恢复原包的联网行为。
pub const ALLOW_HUYANBAO_NET: bool = false;

/// 开发构建开关。**必须与 `APP_DIR_NAME` / tauri.conf.json 的 identifier 同步修改**：
/// 正式发布时三者一起改回原包值（true -> false）。
pub const IS_DEV: bool = true;

/// 计划任务名（原包为 `HuyanBao Auto Start`，dev 加后缀避免互相覆盖）
pub const AUTOSTART_TASK: &str = if IS_DEV { "HuyanBao Dev Auto Start" } else { "HuyanBao Auto Start" };

pub const CONFIG_VERSION: u32 = 4;

// ---------------------------------------------------------------- 配置结构

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimeRange {
    pub start: String,
    pub end: String,
}

impl Default for TimeRange {
    fn default() -> Self {
        Self { start: "09:00".into(), end: "21:00".into() }
    }
}

/// 休息计划默认不启用工作时间段（保持原包行为）
fn default_work_schedule_enabled() -> bool {
    false
}

/// 休息计划默认工作时间段 09:00-18:00
fn default_work_ranges() -> Vec<TimeRange> {
    vec![TimeRange { start: "09:00".into(), end: "18:00".into() }]
}

/// 段外/段内判定：左闭右开 `[start, end)`；`start >= end` 的段视为无效（不支持跨午夜）。
/// `workScheduleEnabled = false` 时恒为段内；没有有效段时 fail-open 视为段内，
/// 避免配置异常导致休息计划永远不会触发。
pub fn in_work_schedule(cfg: &AppConfig, now: &chrono::DateTime<chrono::Local>) -> bool {
    if !cfg.work_schedule_enabled {
        return true;
    }
    // "HH:MM" 定长零填充，字符串比较即时间比较
    let now_hm = now.format("%H:%M").to_string();
    let mut any_valid = false;
    let inside = cfg.work_ranges.iter().any(|r| {
        if r.start.is_empty() || r.end.is_empty() || r.start >= r.end {
            return false; // 无效段（含跨午夜写法）直接跳过
        }
        any_valid = true;
        r.start <= now_hm && now_hm < r.end
    });
    inside || !any_valid
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn cfg(enabled: bool, ranges: &[(&str, &str)]) -> AppConfig {
        AppConfig {
            work_schedule_enabled: enabled,
            work_ranges: ranges
                .iter()
                .copied()
                .map(|(s, e)| TimeRange { start: s.into(), end: e.into() })
                .collect(),
            ..AppConfig::default()
        }
    }

    fn at(hm: &str) -> chrono::DateTime<chrono::Local> {
        let (h, m) = hm.split_once(':').unwrap();
        chrono::Local
            .with_ymd_and_hms(2024, 5, 1, h.parse().unwrap(), m.parse().unwrap(), 0)
            .unwrap()
    }

    #[test]
    fn disabled_means_always_in_schedule() {
        assert!(in_work_schedule(&cfg(false, &[]), &at("23:59")));
    }

    #[test]
    fn range_is_left_closed_right_open() {
        let c = cfg(true, &[("09:00", "12:00")]);
        assert!(!in_work_schedule(&c, &at("08:59")));
        assert!(in_work_schedule(&c, &at("09:00")));
        assert!(in_work_schedule(&c, &at("11:59")));
        assert!(!in_work_schedule(&c, &at("12:00")));
    }

    #[test]
    fn any_range_contains_now() {
        let c = cfg(true, &[("09:00", "12:00"), ("13:00", "18:00")]);
        assert!(!in_work_schedule(&c, &at("12:30"))); // 午休段外
        assert!(in_work_schedule(&c, &at("13:00")));
        assert!(in_work_schedule(&c, &at("17:59")));
        assert!(!in_work_schedule(&c, &at("18:00")));
        assert!(!in_work_schedule(&c, &at("05:00")));
    }

    #[test]
    fn invalid_or_empty_ranges_fail_open() {
        // 跨午夜写法（start >= end）视为无效
        let c = cfg(true, &[("22:00", "02:00")]);
        assert!(in_work_schedule(&c, &at("23:00")));
        assert!(in_work_schedule(&c, &at("05:00")));
        // 空列表同样 fail-open
        assert!(in_work_schedule(&cfg(true, &[]), &at("05:00")));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    // ---- 护眼 ----
    pub eye_enabled: bool,
    pub filter_strength: u32,
    pub overlay_strength: u32,
    pub night_mode: bool,
    pub color_preset: String,
    pub eye_schedule_enabled: bool,
    pub eye_schedule_start: String,
    pub eye_schedule_end: String,
    pub eye_schedule_ranges: Vec<TimeRange>,

    // ---- 休息计划 ----
    pub work_minutes: u32,
    pub short_rest_minutes: u32,
    pub long_rest_every: u32,
    pub long_rest_minutes: u32,
    pub idle_pause_minutes: u32,
    /// 只在工作时间段内执行休息计划（段外工作计时清零）
    #[serde(default = "default_work_schedule_enabled")]
    pub work_schedule_enabled: bool,
    /// 工作时间段列表（与 eye_schedule_ranges 独立，互不联动）
    #[serde(default = "default_work_ranges")]
    pub work_ranges: Vec<TimeRange>,
    pub pre_remind: bool,
    pub pre_remind_seconds: u32,
    pub sound_enabled: bool,
    pub sound_volume: u32,
    pub timer_paused: bool,

    // ---- 宠物 ----
    pub selected_animal_id: String,
    pub animal_strategy: String, // random | dailyRotate | fixed | byTimeSlot
    pub enabled_species: Vec<String>,
    pub dim_opacity: u32,
    pub rest_presence: String, // peek | accompany | focus
    pub rest_pet_enabled: bool,

    // ---- 快捷键 ----
    pub shortcut_pause: String,
    pub shortcut_rest: String,
    pub shortcut_show: String,
    pub shortcut_quit: String,
    pub shortcut_filter_up: String,
    pub shortcut_filter_down: String,
    pub shortcut_eye_toggle: String,
    pub shortcuts_enabled: bool,

    // ---- 桌面小云豆 ----
    pub show_desktop_ear: bool,
    pub desktop_ear_side: String, // left | right
    pub desktop_ear_y: u32,
    /// 拖拽后记住的绝对位置：虚拟屏**物理**像素、窗口外框左上角。
    /// None = 未拖过，按 side/y 百分比在主屏边缘默认摆位。
    #[serde(default)]
    pub desktop_ear_px: Option<i32>,
    #[serde(default)]
    pub desktop_ear_py: Option<i32>,

    // ---- 常规 / 隐私 ----
    pub autostart: bool,
    pub allow_upload_stats: bool,
    pub allow_auto_update: bool,
    pub input_stats_enabled: bool,
    pub ui_dark_mode: bool,
    pub config_version: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            eye_enabled: false,
            filter_strength: 55,
            overlay_strength: 0,
            night_mode: false,
            color_preset: "warm_yellow".into(),
            eye_schedule_enabled: false,
            eye_schedule_start: "09:00".into(),
            eye_schedule_end: "21:00".into(),
            eye_schedule_ranges: vec![TimeRange::default()],
            work_minutes: 50,
            short_rest_minutes: 2,
            long_rest_every: 2,
            long_rest_minutes: 10,
            idle_pause_minutes: 5,
            work_schedule_enabled: false,
            work_ranges: default_work_ranges(),
            pre_remind: false,
            pre_remind_seconds: 30,
            sound_enabled: true,
            sound_volume: 70,
            timer_paused: false,
            selected_animal_id: String::new(),
            animal_strategy: "random".into(),
            enabled_species: vec![],
            dim_opacity: 28,
            rest_presence: "peek".into(),
            rest_pet_enabled: true,
            shortcut_pause: "CommandOrControl+Alt+P".into(),
            shortcut_rest: "CommandOrControl+Alt+R".into(),
            shortcut_show: "CommandOrControl+Alt+S".into(),
            shortcut_quit: "CommandOrControl+Alt+Q".into(),
            shortcut_filter_up: "CommandOrControl+Alt+Shift+Up".into(),
            shortcut_filter_down: "CommandOrControl+Alt+Shift+Down".into(),
            shortcut_eye_toggle: "CommandOrControl+Alt+E".into(),
            shortcuts_enabled: false,
            show_desktop_ear: false,
            desktop_ear_side: "right".into(),
            desktop_ear_y: 38,
            desktop_ear_px: None,
            desktop_ear_py: None,
            autostart: true,
            allow_upload_stats: true,
            allow_auto_update: true,
            input_stats_enabled: false,
            ui_dark_mode: false,
            config_version: CONFIG_VERSION,
        }
    }
}

impl AppConfig {
    /// 按字段名读取快捷键组合（供 services::shortcuts 使用）
    pub fn config_value(&self, field: &str) -> &str {
        match field {
            "shortcutEyeToggle" => &self.shortcut_eye_toggle,
            "shortcutPause" => &self.shortcut_pause,
            "shortcutRest" => &self.shortcut_rest,
            "shortcutShow" => &self.shortcut_show,
            "shortcutQuit" => &self.shortcut_quit,
            "shortcutFilterUp" => &self.shortcut_filter_up,
            "shortcutFilterDown" => &self.shortcut_filter_down,
            _ => "",
        }
    }

    /// 旧版本配置迁移（原包：`[config] migrated from version N to M`）
    fn migrated(mut cfg: AppConfig) -> AppConfig {
        if cfg.config_version < CONFIG_VERSION {
            log::info!(
                "[config] migrated from version {} to {}",
                cfg.config_version,
                CONFIG_VERSION
            );
            cfg.config_version = CONFIG_VERSION;
        }
        // v4：工作时间段（旧配置缺字段时由 serde default 兜底，这里再保证非空）
        if cfg.work_ranges.is_empty() {
            cfg.work_ranges = default_work_ranges();
        }
        cfg
    }
}

// ---------------------------------------------------------------- 每日统计

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DailyStats {
    pub date: String,
    pub work_seconds: u64,
    pub rest_completed: u64,
    pub rest_skipped: u64,
    pub keystrokes: u64,
    pub mouse_clicks: u64,
    pub eye_filter_seconds: u64,
    pub short_rest_count_cycle: u64,
}

// ---------------------------------------------------------------- 目录

/// `%APPDATA%\top.huyanbao.desktop`
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
}

/// `%LOCALAPPDATA%\top.huyanbao.desktop\logs`
pub fn log_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
        .join("logs")
}

fn config_path() -> PathBuf {
    data_dir().join("config.json")
}
fn history_path() -> PathBuf {
    data_dir().join("history.json")
}

// ---------------------------------------------------------------- 读写

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> AppResult<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)?;
    match serde_json::from_str(&text) {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            // 损坏的配置备份为 .corrupt 后回退默认值
            let bak = path.with_extension("json.corrupt");
            log::warn!(
                "[state] {} unreadable ({e}); falling back to defaults, bad copy kept at {}",
                path.display(),
                bak.display()
            );
            let _ = std::fs::copy(path, &bak);
            Ok(None)
        }
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(value)?;
    std::fs::write(&tmp, text)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        log::error!("[state] cannot replace {}: {e}", path.display());
        return Err(e.into());
    }
    Ok(())
}

pub fn load_config() -> AppConfig {
    match read_json(&config_path()) {
        Ok(Some(raw)) => AppConfig::migrated(raw),
        Ok(None) => AppConfig::default(),
        Err(e) => {
            log::error!("[state] cannot read config: {e}");
            AppConfig::default()
        }
    }
}

pub fn save_config(cfg: &AppConfig) -> AppResult<()> {
    write_json(&config_path(), cfg).map_err(|e| {
        log::error!("[state] cannot write config: {e}");
        e
    })
}

pub fn load_history() -> Vec<DailyStats> {
    read_json(&history_path()).ok().flatten().unwrap_or_default()
}

pub fn save_history(history: &[DailyStats]) -> AppResult<()> {
    write_json(&history_path(), &history.to_vec())
}

/// `install_id.txt` —— 遥测用的匿名安装标识
pub fn install_id() -> String {
    let path = data_dir().join("install_id.txt");
    if let Ok(id) = std::fs::read_to_string(&path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(&path, &id);
    id
}

// ---------------------------------------------------------------- 运行态

/// 前端可见的护眼状态（对应 `get_eye_state` 返回值）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EyeState {
    pub enabled: bool,
    pub strength: u32,
    /// 蒙层透明度（0..=0.68）
    pub alpha: f32,
    /// 蒙层颜色 `"r, g, b"`
    pub color: String,
    pub night_mode: bool,
    pub preset_id: String,
    /// Windows 是否接受了当前色温
    pub gamma_working: bool,
}

impl Default for EyeState {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 55,
            alpha: 0.0,
            color: "255, 168, 36".into(),
            night_mode: false,
            preset_id: "warm_yellow".into(),
            gamma_working: true,
        }
    }
}

/// 计时器状态（serde 字段名取自 exe 内字段表）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerState {
    /// working | paused | animalShowing
    pub state: String,
    pub work_elapsed_secs: u64,
    pub work_target_secs: u64,
    pub rest_remaining_secs: u64,
    pub rest_total_secs: u64,
    pub is_long_rest: bool,
    pub next_rest_in_secs: u64,
    pub animal_id: String,
    /// peek | accompany | focus
    pub presence: String,
}

impl Default for TimerState {
    fn default() -> Self {
        Self {
            state: "working".into(),
            work_elapsed_secs: 0,
            work_target_secs: 50 * 60,
            rest_remaining_secs: 0,
            rest_total_secs: 0,
            is_long_rest: false,
            next_rest_in_secs: 50 * 60,
            animal_id: String::new(),
            presence: "peek".into(),
        }
    }
}

/// 进程内共享状态
#[derive(Debug)]
pub struct AppData {
    pub config: AppConfig,
    pub eye: EyeState,
    pub timer: TimerState,
    pub history: Vec<DailyStats>,
    /// 本次休息中被跳过的次数（结算时写入 rest_skipped）
    pub skipped_this_rest: bool,
    pub rest_running: bool,
}

impl AppData {
    pub fn load() -> Self {
        Self {
            config: load_config(),
            eye: EyeState::default(),
            timer: TimerState::default(),
            history: load_history(),
            skipped_this_rest: false,
            rest_running: false,
        }
    }

    /// 今日统计（不存在则返回默认）
    pub fn today_stats(&self) -> DailyStats {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.history
            .iter()
            .find(|s| s.date == today)
            .cloned()
            .unwrap_or(DailyStats { date: today, ..Default::default() })
    }

    /// 当天记录不存在则创建
    pub fn ensure_today(&mut self) {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        if !self.history.iter().any(|s| s.date == today) {
            self.history.push(DailyStats { date: today, ..Default::default() });
        }
    }

    /// 跨天结算（`[stats] filing <昨天> and starting <今天>`）
    pub fn roll_day(&mut self) {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let last = self.history.last().map(|s| s.date.clone());
        if last.as_deref() != Some(today.as_str()) {
            log::info!(
                "[stats] filing {} and starting {}",
                last.clone().unwrap_or_default(),
                today
            );
            self.ensure_today();
            let _ = save_history(&self.history);
        }
    }
}

pub type Shared = Arc<Mutex<AppData>>;

pub fn shared() -> Shared {
    Arc::new(Mutex::new(AppData::load()))
}

/// 锁助手：中毒后仍可读取
pub fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, AppData> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn persist(shared: &Shared) {
    let guard = lock(shared);
    if let Err(e) = save_config(&guard.config) {
        log::error!("[state] cannot serialise config: {e}");
    }
}

pub fn map_err(e: AppError) -> AppError {
    e
}
