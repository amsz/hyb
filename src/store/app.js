import { reactive, computed } from "vue";
import * as ipc from "../lib/ipc";

/**
 * 全局应用状态（对应原包中的 `Gl` store）。
 *
 * 数据形态由以下证据确定：
 *  - config: 本机 config.json 全字段（configVersion=3）
 *  - eye:    FilterOverlay 使用 enabled/alpha/color；
 *            EyePage 使用 nightMode/presetId/gammaWorking/strength
 *  - timer:  MainLayout 使用 state/restRemainingSecs/nextRestInSecs；
 *            exe 内 serde 字段表 state/workElapsedSecs/workTargetSecs/restRemainingSecs/
 *            restTotalSecs/isLongRest/nextRestInSecs/animalId/presence
 *  - stats:  history.json 每日字段
 *  - presets:list_color_presets -> { id, name, color, description }
 */
const state = reactive({
  config: null,
  eye: null,
  timer: null,
  stats: null,
  animals: [],
  petSync: "done",
  presets: [],
  ready: false,
});

async function init() {
  const [config, eye, timer, stats, animals, presets] = await Promise.all([
    ipc.getConfig(),
    ipc.getEyeState(),
    ipc.getTimer(),
    ipc.getDailyStats(),
    ipc.listAnimals(),
    ipc.listColorPresets(),
  ]);
  Object.assign(state, { config, eye, timer, stats, animals, presets, ready: true });

  // 滤镜蒙层窗口常驻，保证 overlayStrength 有承载窗口
  await ipc.ensureFilterOverlay();

  await ipc.listen("timer:tick", (e) => (state.timer = e.payload));
  await ipc.listen("eye:changed", (e) => (state.eye = e.payload));
  await ipc.listen("config:changed", (e) => (state.config = e.payload));
  await ipc.listen("rest:end", () => refreshStats());
  await ipc.listen("pets:updated", async () => (state.animals = await ipc.listAnimals()));
  await ipc.listen("pets:sync", (e) => (state.petSync = e.payload));
}

async function refreshStats() {
  state.stats = await ipc.getDailyStats();
}

async function refreshEye() {
  state.eye = await ipc.getEyeState();
  state.config = await ipc.getConfig();
}

async function setStrength(strength) {
  await ipc.setFilterStrength(Math.round(strength));
  await refreshEye();
}

async function setOverlayStrength(strength) {
  await ipc.setOverlayStrength(Math.round(strength));
  await refreshEye();
}

async function setEnabled(enabled) {
  await ipc.setEyeEnabled(enabled);
  await refreshEye();
}

async function setNight(enabled) {
  await ipc.setNightMode(enabled);
  await refreshEye();
}

async function setPreset(presetId) {
  await ipc.setColorPreset(presetId);
  await refreshEye();
}

async function saveConfig(config) {
  state.config = await ipc.saveConfig(config);
  state.eye = await ipc.getEyeState();
}

/** 局部更新：合并 -> 后端落盘 -> 广播 config:changed */
async function patchConfig(patch) {
  if (!state.config) return;
  await saveConfig({ ...state.config, ...patch });
}

async function togglePause() {
  state.timer = await ipc.toggleTimerPause();
  state.config = await ipc.getConfig();
}

async function startRest() {
  await ipc.startRestNow();
}

export function useAppStore() {
  return {
    get config() {
      return state.config;
    },
    get eye() {
      return state.eye;
    },
    get timer() {
      return state.timer;
    },
    get stats() {
      return state.stats;
    },
    get animals() {
      return state.animals;
    },
    get petSync() {
      return state.petSync;
    },
    get presets() {
      return state.presets;
    },
    get ready() {
      return state.ready;
    },

    /** 物种去重（宠物页筛选） */
    get species() {
      return [...new Set(state.animals.map((a) => a.species))].sort();
    },

    /** 滤镜强度百分比 */
    get filterPercent() {
      return state.eye?.strength ?? 0;
    },

    /**
     * 状态文案：已关闭 / 时段外已暂停 / 夜间助眠 / 护眼中
     * （对应原包表达式：config.eyeEnabled ? (schedule&&!eye.enabled ? 时段外 : night ? 夜间 : 护眼中) : 已关闭）
     */
    get eyeStatusText() {
      const config = state.config;
      const eye = state.eye;
      if (!config?.eyeEnabled) return "已关闭";
      if (config.eyeScheduleEnabled && !eye?.enabled) return "时段外已暂停";
      return eye?.nightMode ? "夜间助眠" : "护眼中";
    },

    init,
    refreshStats,
    refreshEye,
    setStrength,
    setOverlayStrength,
    setEnabled,
    setNight,
    setPreset,
    saveConfig,
    patchConfig,
    togglePause,
    startRest,
  };
}
