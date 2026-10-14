<script setup>
import { onMounted, onUnmounted, ref, watch } from "vue";
import { getShortcutConflicts, listen } from "../lib/ipc";
import { useAppStore } from "../store/app";

const store = useAppStore();
const draft = ref(null);
const capturing = ref(null); // 正在录制的配置键名
const conflictKeys = ref([]);
const warnings = ref([]);
const formError = ref("");

const isMac =
  navigator.userAgent.includes("Macintosh") || navigator.platform.toLowerCase().includes("mac");

/** 快捷键行（键名 -> config 字段，标签来自原包 a 表） */
const ROWS = [
  { key: "shortcutEyeToggle", label: "开关护眼" },
  { key: "shortcutPause", label: "暂停 / 恢复计时" },
  { key: "shortcutRest", label: "立即休息" },
  { key: "shortcutShow", label: "打开主界面" },
  { key: "shortcutQuit", label: "退出" },
  { key: "shortcutFilterUp", label: "增强滤镜 5%" },
  { key: "shortcutFilterDown", label: "减弱滤镜 5%" },
];

let offConflict = null;
let offWarnings = null;
onMounted(async () => {
  conflictKeys.value = await getShortcutConflicts();
  offConflict = await listen("shortcuts:conflict", (e) => (conflictKeys.value = e.payload ?? []));
  offWarnings = await listen("config:warnings", (e) => (warnings.value = e.payload ?? []));
});
onUnmounted(() => {
  offConflict?.();
  offWarnings?.();
  window.removeEventListener("keydown", onKeyDown, true);
});

watch(
  () => store.config,
  (c) => {
    if (c) draft.value = { ...c, enabledSpecies: [...c.enabledSpecies] };
  },
  { immediate: true },
);

watch(capturing, (v) => {
  if (v) window.addEventListener("keydown", onKeyDown, true);
  else window.removeEventListener("keydown", onKeyDown, true);
});

function beginCapture(key) {
  formError.value = "";
  capturing.value = capturing.value === key ? null : key;
}
function clearRow(key) {
  capturing.value = null;
  patch({ [key]: "" });
}
function toggleMaster(v) {
  capturing.value = null;
  if (!v) conflictKeys.value = [];
  patch({ shortcutsEnabled: v });
}

/** keydown -> 组合串；必须含 Ctrl/⌘，可叠加 Alt/Shift */
function onKeyDown(e) {
  if (!capturing.value || !draft.value) return;
  if (e.key === "Escape") {
    capturing.value = null;
    e.preventDefault();
    e.stopPropagation();
    return;
  }
  if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
  e.preventDefault();
  e.stopPropagation();

  const combo = buildCombo(e);
  if (!combo) {
    formError.value = `请使用 ${isMac ? "⌘" : "Ctrl"} 加上字母、数字或方向键`;
    return;
  }
  const target = capturing.value;
  const duplicated = ROWS.some(
    (r) =>
      r.key !== target &&
      draft.value[r.key] &&
      draft.value[r.key] === combo,
  );
  if (duplicated) {
    formError.value = "这个快捷键已经被占用";
    capturing.value = null;
    return;
  }
  capturing.value = null;
  patch({ [target]: combo });
}

function buildCombo(e) {
  if (!e.ctrlKey && !e.metaKey) return null;
  const parts = ["CommandOrControl"];
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  const key = keyFromCode(e.code);
  if (!key) return null;
  parts.push(key);
  return parts.join("+");
}

function keyFromCode(code) {
  if (code.startsWith("Key") && code.length === 4) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (/^F([1-9]|1[0-2])$/.test(code)) return code;
  if (code === "ArrowUp") return "Up";
  if (code === "ArrowDown") return "Down";
  if (code === "ArrowLeft") return "Left";
  if (code === "ArrowRight") return "Right";
  if (code === "Equal" || code === "NumpadAdd") return "Equal";
  if (code === "Minus" || code === "NumpadSubtract") return "Minus";
  return null;
}

/** 展示用：CommandOrControl -> Ctrl/⌘，方向键 -> 箭头 */
function display(combo) {
  if (!combo) return "未设置";
  const ctrl = isMac ? "⌘" : "Ctrl";
  return combo
    .replace(/CommandOrControl/gi, ctrl)
    .replace(/Command/gi, "⌘")
    .replace(/Control/gi, "Ctrl")
    .replace(/\bUp\b/g, "↑")
    .replace(/\bDown\b/g, "↓")
    .replace(/\bLeft\b/g, "←")
    .replace(/\bRight\b/g, "→")
    .replace(/\+/g, " + ");
}

async function patch(p) {
  if (!draft.value) return;
  formError.value = "";
  try {
    await store.patchConfig(p);
  } catch (err) {
    formError.value = err instanceof Error ? err.message : String(err);
  }
}
</script>

<template>
  <div v-if="draft" class="page">
    <h2>设置</h2>

    <section class="block">
      <div class="title">常规</div>

      <div class="row between">
        <div>
          <div>开机自启</div>
          <div class="hint">{{ isMac ? "登录后自动启动，也可以随时关掉" : "默认开启，也可以随时关掉" }}</div>
        </div>
        <n-switch :value="draft.autostart" @update:value="(v) => patch({ autostart: v })" />
      </div>
      <div v-if="isMac" class="hint" style="margin: -2px 0 10px">
        第一次开启时，若系统提示允许后台运行，请点允许。辅助功能只用于统计击键和点击，
        休息提醒不需要这项权限；授权后请重启一次才会开始计数。
      </div>

      <div class="row between">
        <div>
          <div>深色模式</div>
          <div class="hint">仅改变操作界面，不影响护眼滤镜</div>
        </div>
        <n-switch :value="draft.uiDarkMode" @update:value="(v) => patch({ uiDarkMode: v })" />
      </div>

      <div class="row between">
        <div>
          <div>桌面小云豆</div>
          <div class="hint">默认关闭。平时只从屏幕边探出一点小脸，点一下才展开，用完自己躲回去</div>
        </div>
        <n-switch
          :value="draft.showDesktopEar"
          @update:value="(v) => patch({ showDesktopEar: v })"
        />
      </div>

      <div class="row between">
        <div>
          <div>统计击键和点击</div>
          <div class="hint">
            默认关闭。开启后健康页会显示击键数和鼠标点击次数。它需要一个全局输入钩子，
            部分杀软会对此报警，重启应用后生效。
          </div>
        </div>
        <n-switch
          :value="draft.inputStatsEnabled"
          @update:value="(v) => patch({ inputStatsEnabled: v })"
        />
      </div>
    </section>

    <section class="block">
      <div class="title">快捷键</div>

      <div class="row between">
        <div>
          <div>启用全局快捷键</div>
          <div class="hint">关掉后本页所有快捷键立即失效，不影响其他功能</div>
        </div>
        <n-switch :value="draft.shortcutsEnabled" @update:value="toggleMaster" />
      </div>

      <div class="hint" style="margin: 2px 0 10px">
        点击右侧按键，再按下新的组合。需要包含 {{ isMac ? "⌘" : "Ctrl" }}。
        <template v-if="!isMac">默认已避开 Ctrl + Shift，那是系统输入法切换键。</template>
        清除某一行即可单独停用它。
      </div>

      <div
        v-for="row in ROWS"
        :key="row.key"
        class="row between"
        :class="{ dimmed: !draft.shortcutsEnabled }"
      >
        <span>{{ row.label }}</span>
        <span class="key-cell">
          <button
            class="key-btn"
            :class="{ capturing: capturing === row.key }"
            :disabled="!draft.shortcutsEnabled"
            @click="beginCapture(row.key)"
          >
            {{ capturing === row.key ? "按下新快捷键…" : display(draft[row.key]) }}
          </button>
          <button
            class="clear-btn"
            type="button"
            :disabled="!draft.shortcutsEnabled || !draft[row.key]"
            :title="draft[row.key] ? '清除，停用这一项' : '已停用'"
            @click="clearRow(row.key)"
          >
            ×
          </button>
        </span>
      </div>

      <div v-if="conflictKeys.length" class="warn">
        以下快捷键没能注册，多半已被别的程序占用，换个组合或清除即可：{{ conflictKeys.join("、") }}
      </div>
    </section>

    <div v-for="(w, i) in warnings" :key="i" class="warn">{{ w }}</div>
    <div v-if="formError" class="err">{{ formError }}</div>
  </div>
</template>
<style scoped>
/* 还原自 huyanbao.exe 内嵌 bundle 的 scoped 样式（原作用域 data-v-ee0fcb06） */
.page h2{margin:0 0 12px;font-size:18px}
.block{margin-bottom:10px;padding:12px;border-radius:14px;border:1px solid var(--border);background:var(--card)}
.title{margin-bottom:10px;font-size:13px;font-weight:700}
.hint{font-size:11px;color:var(--muted)}
.row{display:flex;align-items:center;gap:8px;margin-bottom:8px;font-size:12px}
.row.between{justify-content:space-between}
.key-btn{min-width:168px;padding:6px 10px;border:1px solid var(--border);border-radius:8px;background:var(--key-bg);color:var(--mint-700);cursor:pointer;font-size:12px}
.key-btn.capturing{border-color:var(--mint-600);background:#26a69a1f}
.key-cell{display:inline-flex;align-items:center;gap:6px}
.clear-btn{width:24px;height:24px;padding:0;border:1px solid var(--border);border-radius:8px;background:var(--key-bg);color:var(--muted);cursor:pointer;font-size:13px;line-height:1}
.clear-btn:disabled,.key-btn:disabled{opacity:.45;cursor:default}
.row.dimmed>span:first-child{opacity:.55}
.warn{margin-top:8px;font-size:11px;color:var(--danger);line-height:1.5}
.err{margin-top:8px;color:var(--danger);font-size:12px}
</style>
