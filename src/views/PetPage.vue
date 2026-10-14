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
    if (c) draft.value = { ...c, enabledSpecies: [...c.enabledSpecies] };
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
.row{display:flex;align-items:center;gap:8px;margin-bottom:8px;font-size:12px}
.row.between{justify-content:space-between}
.preview{position:relative;width:min(640px,82vw);height:min(480px,70vh);display:grid;place-items:center;border-radius:20px;background:#05100feb;overflow:hidden}
.preview video{width:100%;height:100%;object-fit:contain}
.preview-close{position:absolute;top:12px;right:12px;width:30px;height:30px;border:0;border-radius:50%;color:#fff;background:#00000073;cursor:pointer}
</style>
