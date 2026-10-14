<script setup>
import { computed, onMounted, ref, watch } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { previewRestPresence } from "../lib/ipc";
import { useAppStore } from "../store/app";

const store = useAppStore();

const draft = ref(null); // 本地编辑副本，点“保存”统一提交
const previewOpen = ref(false);
const previewAnimal = ref(null);
const previewPhase = ref("enter");
const previewVideo = ref(null);

const strategy = computed(() => draft.value?.animalStrategy ?? "random");
const presence = computed(() => draft.value?.restPresence ?? "accompany");

// ---- 工作时间段（休息计划只在段内执行） ----
const rangeError = ref("");
function normalizeRanges(list) {
  const valid = (Array.isArray(list) ? list : [])
    .filter((r) => r?.start && r?.end && r.start < r.end)
    .map((r) => ({ ...r }));
  return valid.length ? valid : [{ start: "09:00", end: "18:00" }];
}
function setRangeStart(i, e) {
  const v = e.target.value;
  if (!v) return;
  const r = draft.value.workRanges[i];
  if (r.end && v >= r.end) {
    rangeError.value = "结束时间需晚于开始时间，暂不支持跨午夜";
    return;
  }
  rangeError.value = "";
  draft.value.workRanges = draft.value.workRanges.map((x, j) => (j === i ? { ...x, start: v } : x));
}
function setRangeEnd(i, e) {
  const v = e.target.value;
  if (!v) return;
  const r = draft.value.workRanges[i];
  if (r.start && v <= r.start) {
    rangeError.value = "结束时间需晚于开始时间，暂不支持跨午夜";
    return;
  }
  rangeError.value = "";
  draft.value.workRanges = draft.value.workRanges.map((x, j) => (j === i ? { ...x, end: v } : x));
}
function addRange() {
  if (rangeError.value) return; // 还有非法时间段时不允许继续加
  draft.value.workRanges = [...draft.value.workRanges, { start: "14:00", end: "18:00" }];
}
function dropRange(i) {
  if (draft.value.workRanges.length <= 1) return;
  draft.value.workRanges = draft.value.workRanges.filter((_, j) => j !== i);
}

const syncNotice = computed(() => {
  if (store.petSync === "fetching") return { title: "还有几只在路上", desc: "它们会自己过来，不用管" };
  if (store.petSync === "failed")
    return { title: "有几只还没接来", desc: "网络不太顺，过一会儿自动再试" };
  return null;
});

const PRESENCES = [
  { id: "peek", name: "探头看看", desc: "推荐。角落里探出来，工作区几乎不动" },
  { id: "accompany", name: "陪你休息", desc: "屏幕适度变暗，宠物居中出现" },
  { id: "focus", name: "认真歇一下", desc: "更暗一些，宠物占满眼前" },
];

watch(
  () => store.config,
  (c) => {
    if (c) {
      draft.value = {
        ...c,
        enabledSpecies: [...c.enabledSpecies],
        workScheduleEnabled: c.workScheduleEnabled ?? false,
        workRanges: normalizeRanges(c.workRanges),
      };
    }
  },
  { immediate: true },
);

function setRestPetEnabled(v) {
  if (!draft.value) return;
  draft.value.restPetEnabled = v;
  save();
}

/** 循环素材：只有 idle 或 enter/idle 相同才 loop（同原包 V3 判断） */
function isLoop(animal) {
  if (!animal) return false;
  if (animal.loopEnter) return true;
  const enter = animal.assets?.enter ?? "";
  const idle = animal.assets?.idle ?? "";
  return !idle || idle === enter;
}

const loopEnter = computed(() => isLoop(previewAnimal.value));
const previewSrc = computed(() => {
  const a = previewAnimal.value;
  if (!a) return "";
  const path =
    loopEnter.value || previewPhase.value === "enter" ? a.assets?.enter : a.assets?.idle;
  return path ? convertFileSrc(path) : "";
});

function assetUrl(path) {
  return path ? convertFileSrc(path) : "";
}
function initial(animal) {
  return animal.name.trim().slice(0, 1) || animal.id.slice(0, 1).toUpperCase();
}
function openPreview(animal) {
  previewAnimal.value = animal;
  previewPhase.value = "enter";
  previewOpen.value = true;
}
function onPreviewEnded() {
  if (loopEnter.value || previewPhase.value !== "enter") return;
  previewPhase.value = "idle";
  requestAnimationFrame(() => previewVideo.value?.play().catch(() => {}));
}
function isFixed(animalId) {
  return draft.value?.animalStrategy === "fixed" && draft.value.selectedAnimalId === animalId;
}
function pickFixed(animal) {
  if (!draft.value) return;
  draft.value.selectedAnimalId = animal.id;
  draft.value.animalStrategy = "fixed";
  save();
}
function setStrategy(v) {
  if (!draft.value) return;
  draft.value.animalStrategy = v;
  save();
}
function setPresence(v) {
  if (!draft.value) return;
  draft.value.restPresence = v;
  save();
  // 点了就直接看一下效果（不计入休息/统计，6 秒后自动收起）
  previewRestPresence(v).catch(console.error);
}
async function save() {
  if (draft.value) await store.saveConfig(draft.value);
}
</script>

<template>
  <div v-if="draft" class="page">
    <h2>宠物</h2>

    <section class="block">
      <div class="row between">
        <div>
          <div class="switch-title">休息时让宠物出现</div>
          <div class="hint">关掉后到点不再打断你，护眼滤镜和工作统计照常</div>
        </div>
        <n-switch :value="draft.restPetEnabled" @update:value="setRestPetEnabled" />
      </div>
    </section>

    <template v-if="draft.restPetEnabled">
      <div class="pet-grid">
        <div v-for="animal in store.animals" :key="animal.id" class="pet-card">
          <div class="pet-thumb" :style="{ background: animal.bgColor || '#e6f7f2' }">
            <img
              v-if="animal.assets?.thumbnail"
              :src="assetUrl(animal.assets.thumbnail)"
              :alt="`${animal.name} 缩略图`"
            />
            <span v-else>{{ initial(animal) }}</span>
          </div>
          <div>
            <div class="pet-name">{{ animal.name }}</div>
            <div class="pet-desc">{{ animal.description }}</div>
          </div>
          <div class="pet-actions">
            <n-button size="small" secondary @click="openPreview(animal)">预览</n-button>
            <n-button
              size="small"
              :type="isFixed(animal.id) ? 'primary' : 'default'"
              secondary
              @click="pickFixed(animal)"
            >
              {{ isFixed(animal.id) ? "已指定" : "指定" }}
            </n-button>
          </div>
        </div>

        <div v-if="syncNotice" class="pet-card waiting">
          <div class="pet-thumb waiting-thumb"><span>···</span></div>
          <div>
            <div class="pet-name">{{ syncNotice.title }}</div>
            <div class="pet-desc">{{ syncNotice.desc }}</div>
          </div>
        </div>
      </div>

      <section class="block">
        <div class="title">谁来陪你</div>
        <n-radio-group :value="strategy" @update:value="setStrategy">
          <n-space vertical :size="6">
            <n-radio value="random">随机宠物</n-radio>
            <n-radio value="fixed">指定宠物</n-radio>
          </n-space>
        </n-radio-group>
      </section>

      <section class="block">
        <div class="title">出现方式</div>
        <div class="hint">想让它轻一点，还是认真管你一下。随时可以换回来。</div>
        <div class="presence">
          <button
            v-for="p in PRESENCES"
            :key="p.id"
            class="presence-card"
            :class="{ active: presence === p.id }"
            type="button"
            @click="setPresence(p.id)"
          >
            <div class="presence-preview" :class="p.id">
              <span class="presence-screen" />
              <span class="presence-pet" />
            </div>
            <div class="presence-name">{{ p.name }}</div>
            <div class="presence-desc">{{ p.desc }}</div>
          </button>
        </div>
      </section>

      <section class="block">
        <div class="title">休息计划</div>
        <div class="row between schedule-switch">
          <div>
            <div class="switch-title">只在工作时间段提醒</div>
            <div class="hint">段内才计时，段外倒计时清零；到点的休息不打断，自然结束</div>
          </div>
          <n-switch
            :value="draft.workScheduleEnabled"
            @update:value="(v) => (draft.workScheduleEnabled = v)"
          />
        </div>
        <template v-if="draft.workScheduleEnabled">
          <div v-for="(r, i) in draft.workRanges" :key="i" class="time-row">
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
              :disabled="draft.workRanges.length <= 1"
              @click="dropRange(i)"
            >
              ×
            </button>
          </div>
          <button
            class="range-add"
            type="button"
            :disabled="!!rangeError"
            @click="addRange"
          >
            + 再加一段
          </button>
          <div v-if="rangeError" class="range-error">{{ rangeError }}</div>
          <div class="hint">可分上下午多段，例如 9:00-12:00、13:00-18:00。不支持跨午夜。</div>
        </template>
        <div class="row">
          <span class="row-label">工作间隔</span>
          <n-slider
            v-model:value="draft.workMinutes"
            :min="5"
            :max="180"
            :step="5"
            style="flex: 1"
          />
          <span class="row-value">{{ draft.workMinutes }} 分钟</span>
        </div>
        <div class="row">
          <span class="row-label">短休息</span>
          <n-slider
            v-model:value="draft.shortRestMinutes"
            :min="1"
            :max="30"
            :step="1"
            style="flex: 1"
          />
          <span class="row-value">{{ draft.shortRestMinutes }} 分钟</span>
        </div>
        <div class="row">
          <span class="row-label">长休触发</span>
          <n-slider
            v-model:value="draft.longRestEvery"
            :min="1"
            :max="10"
            :step="1"
            style="flex: 1"
          />
          <span class="row-value">{{ draft.longRestEvery }} 次后</span>
        </div>
        <div class="row">
          <span class="row-label">长休息</span>
          <n-slider
            v-model:value="draft.longRestMinutes"
            :min="5"
            :max="60"
            :step="5"
            style="flex: 1"
          />
          <span class="row-value">{{ draft.longRestMinutes }} 分钟</span>
        </div>
        <div class="row">
          <span class="row-label">离开暂停</span>
          <n-slider
            v-model:value="draft.idlePauseMinutes"
            :min="1"
            :max="30"
            :step="1"
            style="flex: 1"
          />
          <span class="row-value">{{ draft.idlePauseMinutes }} 分钟</span>
        </div>
      </section>

      <n-button type="primary" @click="save">保存</n-button>
    </template>

    <n-modal v-model:show="previewOpen">
      <div class="preview">
        <video
          v-if="previewAnimal"
          ref="previewVideo"
          :src="previewSrc"
          :loop="loopEnter || previewPhase === 'idle'"
          autoplay
          muted
          playsinline
          @ended="onPreviewEnded"
        />
        <button class="preview-close" @click="previewOpen = false">×</button>
      </div>
    </n-modal>
  </div>
</template>
<style scoped>
/* 还原自 huyanbao.exe 内嵌 bundle 的 scoped 样式（原作用域 data-v-12fc8f7d） */
.page h2{margin:0 0 12px;font-size:18px}
.pet-card{display:grid;grid-template-columns:72px 1fr auto;gap:10px;align-items:center;padding:10px;border:1px solid var(--border);border-radius:14px;background:var(--card)}
.pet-actions{display:flex;flex-direction:column;gap:6px}
.pet-thumb{width:72px;height:58px;display:grid;place-items:center;border-radius:14px;color:#fff;font-size:24px;font-weight:800;box-shadow:inset 0 0 0 1px #ffffff2e;overflow:hidden}
.waiting{border-style:dashed;background:transparent}
.waiting .pet-name,.waiting .pet-desc{color:var(--muted)}
.waiting-thumb{background:transparent;border:1px dashed var(--border);box-shadow:none;color:var(--muted);font-size:18px;letter-spacing:2px}
.pet-thumb img{max-width:100%;max-height:100%;object-fit:contain}
.pet-name{font-size:13px;font-weight:700}
.pet-desc{margin-top:3px;color:var(--muted);font-size:11px}
.block{margin:12px 0;padding:12px;border:1px solid var(--border);border-radius:14px;background:var(--card)}
.title{margin-bottom:10px;font-size:13px;font-weight:700}
.hint{margin:-4px 0 10px;font-size:11px;color:var(--muted)}
.presence{display:grid;grid-template-columns:1fr;gap:8px}
.presence-card{display:grid;grid-template-columns:72px 1fr;grid-template-rows:auto auto;column-gap:10px;padding:10px;border:1px solid var(--border);border-radius:14px;background:transparent;color:inherit;text-align:left;cursor:pointer}
.presence-card.active{border-color:var(--mint-600);background:var(--key-bg)}
.presence-preview{grid-row:1/span 2;position:relative;width:72px;height:52px;border-radius:10px;background:#eef7f6;overflow:hidden}
.presence-screen{position:absolute;top:6px;right:8px;bottom:6px;left:8px;border-radius:4px;background:#d7ece9}
.presence-pet{position:absolute;width:16px;height:16px;border-radius:999px;background:#26a69a}
.presence-preview.peek .presence-screen{background:#9aaeab}
.presence-preview.peek .presence-pet{right:8px;bottom:8px}
.presence-preview.accompany .presence-screen{background:#8aa8a4}
.presence-preview.accompany .presence-pet{left:50%;top:50%;width:22px;height:22px;transform:translate(-50%,-50%)}
.presence-preview.focus .presence-screen{background:#3d5552}
.presence-preview.focus .presence-pet{left:50%;top:52%;width:34px;height:34px;transform:translate(-50%,-50%)}
.presence-name{font-size:13px;font-weight:700}
.presence-desc{margin-top:2px;font-size:11px;color:var(--muted)}
.switch-title{font-size:13px;font-weight:700}
.schedule-switch{align-items:flex-start}
.time-row{display:flex;align-items:center;gap:10px;margin-bottom:8px;font-size:12px;color:var(--muted)}
.time-row input{margin-left:6px;padding:4px 8px;border:1px solid var(--border);border-radius:8px;background:var(--key-bg);color:var(--ink);font-size:12px}
.range-drop{width:24px;height:24px;padding:0;border:1px solid var(--border);border-radius:8px;background:var(--card);color:var(--muted);cursor:pointer;line-height:1}
.range-drop:disabled{opacity:.4;cursor:default}
.range-add{margin:0 0 8px;padding:4px 10px;border:1px dashed var(--border);border-radius:8px;background:transparent;color:var(--muted);font-size:12px;cursor:pointer}
.range-add:disabled{opacity:.5;cursor:default}
.range-error{margin:-4px 0 8px;font-size:11px;color:#e57373}
.row{display:flex;align-items:center;gap:8px;margin-bottom:8px;font-size:12px}
.row-label{width:58px;flex:none}
.row-value{width:58px;flex:none;text-align:right;color:var(--muted);font-variant-numeric:tabular-nums}
.row.between{justify-content:space-between}
.preview{position:relative;width:min(640px,82vw);height:min(480px,70vh);display:grid;place-items:center;border-radius:20px;background:#05100feb;overflow:hidden}
.preview video{width:100%;height:100%;object-fit:contain}
.preview-close{position:absolute;top:12px;right:12px;width:30px;height:30px;border:0;border-radius:50%;color:#fff;background:#00000073;cursor:pointer}
</style>
