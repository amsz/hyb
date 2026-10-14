<script setup>
import { computed, onMounted, onUnmounted, ref } from "vue";
import { listen } from "../lib/ipc";
import { useAppStore } from "../store/app";

const store = useAppStore();

// 环形进度：r=52 的周长
const R = 52;
const circumference = 2 * Math.PI * R;
const dashOffset = computed(() => circumference * (1 - store.filterPercent / 100));

/** gamma 被系统拒绝（Windows 校验色温过严）时给出降级提示 */
const gammaRejected = computed(
  () => !!store.config?.eyeEnabled && store.eye?.gammaWorking === false,
);

/** 预设描述（夜间模式下改用夜间文案） */
const presetDesc = computed(() => {
  if (store.eye?.nightMode) return "夜间强力暖色，适合睡前使用";
  return store.presets.find((p) => p.id === store.eye?.presetId)?.description ?? "";
});

// ---- 保存失败/告警（后端 config:warnings 事件） ----
const warnings = ref([]);
let offWarnings = null;
const patchError = ref("");
onMounted(async () => {
  offWarnings = await listen("config:warnings", (e) => {
    warnings.value = e.payload ?? [];
  });
});
onUnmounted(() => offWarnings?.());

async function patch(patch) {
  patchError.value = "";
  try {
    await store.patchConfig(patch);
  } catch (err) {
    patchError.value = err instanceof Error ? err.message : String(err);
  }
}

// ---- 按时段开关 ----
const ranges = computed(() => store.config?.eyeScheduleRanges ?? []);
function setRanges(next) {
  patch({ eyeScheduleRanges: next });
}
function setRangeStart(i, e) {
  const v = e.target.value;
  if (v) setRanges(ranges.value.map((r, j) => (j === i ? { ...r, start: v } : r)));
}
function setRangeEnd(i, e) {
  const v = e.target.value;
  if (v) setRanges(ranges.value.map((r, j) => (j === i ? { ...r, end: v } : r)));
}
function addRange() {
  setRanges([...ranges.value, { start: "14:00", end: "18:00" }]);
}
function dropRange(i) {
  if (ranges.value.length <= 1) return;
  setRanges(ranges.value.filter((_, j) => j !== i));
}
</script>

<template>
  <div v-if="store.eye" class="page">
    <div class="ring-wrap">
      <svg viewBox="0 0 120 120" class="ring">
        <circle cx="60" cy="60" r="52" class="track" />
        <circle
          cx="60"
          cy="60"
          r="52"
          class="progress"
          :style="{ strokeDashoffset: dashOffset }"
        />
      </svg>
      <div class="ring-center">
        <div class="percent">{{ store.filterPercent }}%</div>
        <div class="status">{{ store.eyeStatusText }}</div>
      </div>
    </div>

    <div class="block">
      <div class="label-row">
        <div>
          <div>蓝光过滤强度</div>
          <div class="desc">直接调整屏幕色温，全屏幕生效</div>
        </div>
        <n-switch
          :value="!!store.config?.eyeEnabled"
          @update:value="store.setEnabled"
        />
      </div>
      <n-slider
        :value="store.filterPercent"
        :min="0"
        :max="100"
        :disabled="!store.config?.eyeEnabled"
        @update:value="store.setStrength"
      />
      <div v-if="gammaRejected" class="warn">
        当前强度未能生效，系统拒绝了这档色温。调低强度，或改用下面的界面蒙层。
      </div>
    </div>

    <div class="block">
      <div class="label-row">
        <div>
          <div>界面蒙层</div>
          <div class="desc">额外叠一层暖色，调高可能让文字发虚</div>
        </div>
        <div class="overlay-value">{{ store.config?.overlayStrength ?? 0 }}%</div>
      </div>
      <n-slider
        :value="store.config?.overlayStrength ?? 0"
        :min="0"
        :max="100"
        :disabled="!store.config?.eyeEnabled"
        @update:value="store.setOverlayStrength"
      />
    </div>

    <div v-if="warnings.length" class="notice">{{ warnings.join(" ") }}</div>
    <div v-if="patchError" class="notice">{{ patchError }}</div>

    <div class="block">
      <div class="label-row">
        <div>
          <div>按时段开关</div>
          <div class="desc">到点自动开启，离开时段自动关闭</div>
        </div>
        <n-switch
          :value="!!store.config?.eyeScheduleEnabled"
          :disabled="!store.config?.eyeEnabled"
          @update:value="(v) => patch({ eyeScheduleEnabled: v })"
        />
      </div>

      <template v-if="store.config?.eyeScheduleEnabled">
        <div v-for="(r, i) in ranges" :key="i" class="time-row">
          <label>
            开始
            <input type="time" :value="r.start" @change="setRangeStart(i, $event)" />
          </label>
          <span>至</span>
          <label>
            结束
            <input type="time" :value="r.end" @change="setRangeEnd(i, $event)" />
          </label>
          <button
            class="range-drop"
            type="button"
            title="删除这一段"
            :disabled="ranges.length <= 1"
            @click="dropRange(i)"
          >
            ×
          </button>
        </div>
        <button class="range-add" type="button" @click="addRange">+ 再加一段</button>
        <div class="desc">可分上下午两段。结束早于开始表示跨午夜。</div>
      </template>
    </div>

    <div class="block night">
      <div class="label-row">
        <div>
          <div class="title">夜间助眠</div>
          <div class="desc">切换到强力暖色模式</div>
        </div>
        <n-switch
          :value="!!store.eye?.nightMode"
          :disabled="!store.config?.eyeEnabled"
          @update:value="store.setNight"
        />
      </div>
    </div>

    <div class="block">
      <div class="label-row">
        <div>
          <div class="title">色温预设</div>
        </div>
      </div>
      <div class="presets">
        <button
          v-for="p in store.presets"
          :key="p.id"
          class="preset"
          :class="{ active: store.eye?.presetId === p.id && !store.eye?.nightMode }"
          :style="{ background: `rgba(${p.color}, 0.9)` }"
          :title="p.name"
          @click="store.setPreset(p.id)"
        />
      </div>
      <div class="preset-desc">{{ presetDesc }}</div>
    </div>
  </div>
</template>
<style scoped>
/* 还原自 huyanbao.exe 内嵌 bundle 的 scoped 样式（原作用域 data-v-506469f1） */
.page{display:flex;flex-direction:column;gap:16px}
.ring-wrap{position:relative;width:160px;height:160px;margin:8px auto 4px}
.ring{width:100%;height:100%;transform:rotate(-90deg)}
.track{fill:none;stroke:#b39ddb26;stroke-width:10}
.progress{fill:none;stroke:url(#none);stroke:#7e57c2;stroke-width:10;stroke-linecap:round;stroke-dasharray:326.73;transition:stroke-dashoffset .25s ease}
.ring-center{position:absolute;top:0;right:0;bottom:0;left:0;display:grid;place-content:center;text-align:center}
.percent{font-size:32px;font-weight:700;color:var(--lav-700);line-height:1}
.status{margin-top:6px;font-size:13px;color:var(--muted)}
.block{background:var(--card);border:1px solid var(--border);border-radius:16px;padding:14px}
.label-row{display:flex;justify-content:space-between;align-items:center;gap:12px;margin-bottom:8px;font-size:13px;font-weight:600}
.label-row>div:first-child{flex:1;min-width:0}
.label-row>:not(:first-child){flex:none}
.night{display:flex;justify-content:space-between;align-items:center}
.title{font-size:13px;font-weight:600}
.desc,.notice{margin-bottom:10px;padding:8px 10px;border-radius:10px;background:#ffa82424;color:var(--text);font-size:12px;line-height:1.6}
.warn{margin-top:8px;padding:8px 10px;border-radius:8px;background:#ffa8241f;color:var(--text);font-size:11px;font-weight:400;line-height:1.6}
.range-drop{width:24px;height:24px;padding:0;border:1px solid var(--border);border-radius:8px;background:var(--card);color:var(--muted);cursor:pointer;line-height:1}
.range-drop:disabled{opacity:.4;cursor:default}
.range-add{margin:2px 0 6px;padding:4px 10px;border:1px dashed var(--border);border-radius:8px;background:transparent;color:var(--muted);font-size:12px;cursor:pointer}
.overlay-value{font-size:12px;color:var(--muted);font-variant-numeric:tabular-nums}
.preset-desc{margin-top:4px;font-size:12px;color:var(--muted)}
.time-row{display:flex;align-items:center;gap:10px;margin-top:10px;font-size:12px;color:var(--muted)}
.time-row input{margin-left:6px;padding:4px 8px;border:1px solid var(--border);border-radius:8px;background:var(--key-bg);color:var(--ink)}
.presets{display:flex;gap:10px;margin-top:12px;flex-wrap:wrap}
.preset{width:28px;height:28px;border-radius:50%;border:2px solid transparent;cursor:pointer;box-shadow:inset 0 0 0 1px #00000014}
.preset.active{border-color:var(--lav-700);box-shadow:0 0 0 3px #7e57c240}
</style>
