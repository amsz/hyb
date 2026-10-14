/**
 * Tauri IPC 封装。
 *
 * 命令名来自 exe 内 invoke 处理器表（见报告 §4），事件名来自前端 bundle 的 listen 调用。
 * 新增命令时保持命名风格 `verb_noun`。
 */
import { invoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";

// ---------- 配置 / 护眼状态 ----------
export const getConfig = () => invoke("get_config");
export const saveConfig = (config) => invoke("save_config", { config });
export const getEyeState = () => invoke("get_eye_state");
export const setEyeEnabled = (enabled) => invoke("set_eye_enabled", { enabled });
export const setFilterStrength = (strength) => invoke("set_filter_strength", { strength });
export const setOverlayStrength = (strength) => invoke("set_overlay_strength", { strength });
export const nudgeFilterStrength = (delta) => invoke("nudge_filter_strength", { delta });
export const setNightMode = (enabled) => invoke("set_night_mode", { enabled });
export const setColorPreset = (presetId) => invoke("set_color_preset", { presetId });
export const listColorPresets = () => invoke("list_color_presets");

// ---------- 计时 / 休息 ----------
export const getTimer = () => invoke("get_timer");
export const toggleTimerPause = () => invoke("toggle_timer_pause");
export const startRestNow = () => invoke("start_rest_now");
export const skipRest = (withPet = false) => invoke("skip_rest", { withPet });

// ---------- 统计 ----------
export const getDailyStats = () => invoke("get_daily_stats");
export const getStatsHistory = () => invoke("get_stats_history");

// ---------- 宠物 ----------
export const listAnimals = () => invoke("list_animals");
export const listSpecies = () => invoke("list_species");
export const getAnimal = (animalId) => invoke("get_animal", { animalId });
export const closeAnimalOverlay = () => invoke("close_animal_overlay");
/** 设置页点“出现方式”时试看（不计入休息/统计） */
export const previewRestPresence = (presence, animalId) =>
  invoke("preview_rest_presence", { presence, animalId });
export const cancelPreviewOverlay = () => invoke("cancel_preview_overlay");

// ---------- 窗口 ----------
export const showMainWindow = () => invoke("show_main_window");
export const hideMainToTray = () => invoke("hide_main_to_tray");
export const ensureFilterOverlay = () => invoke("ensure_filter_overlay");
// 窗口生命周期（原包 Rust 侧存在同名命令）
export const createFilterOverlay = () => invoke("create_filter_overlay");
export const destroyFilterOverlay = () => invoke("destroy_filter_overlay");
export const destroyEarOverlay = () => invoke("destroy_ear_overlay");
export const setEarIgnoringCursorEvents = (ignoring) =>
  invoke("set_ear_ignoring_cursor_events", { ignoring });
export const getGlobalPosition = () => invoke("get_global_position");
export const restoreStateFromStorage = () => invoke("restore_state_from_storage");

// ---------- 桌面小云豆 ----------
export const setDesktopEarPose = (pose) => invoke("set_desktop_ear_pose", { pose });
export const beginDesktopEarDrag = () => invoke("begin_desktop_ear_drag");
export const endDesktopEarDrag = () => invoke("end_desktop_ear_drag");

// ---------- 快捷键 / 更新 / 其他 ----------
export const getShortcutConflicts = () => invoke("get_shortcut_conflicts");
export const checkForUpdates = () => invoke("check_for_updates");
export const loadTongjiScript = () => invoke("load_tongji_script");
export const getInstallId = () => invoke("get_install_id");
export const runSystemCommand = (command, cwd, timeoutSecs) =>
  invoke("run_system_command", { command, cwd, timeoutSecs });

/** 外部链接（opener 插件，原包通过 plugin:opener|open_url 打开反馈/官网） */
export const openUrl = (url) => invoke("plugin:opener|open_url", { url });
/** 读取应用版本（plugin:app|version） */
export const getAppVersion = () => invoke("plugin:app|version");

/** 事件监听（返回 unlisten） */
export const listen = (event, handler) => tauriListen(event, handler);
