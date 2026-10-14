<script setup>
import { onMounted, onBeforeUnmount, ref } from "vue";
import {
  beginDesktopEarDrag,
  endDesktopEarDrag,
  getConfig,
  getEyeState,
  listen,
  nudgeFilterStrength,
  setDesktopEarPose,
  setEyeEnabled,
} from "../lib/ipc";

/**
 * 桌面小云豆（label=desktop-ear，路由 #/overlay/ear）。
 *
 * 状态机取自原包 EarOverlay：
 *   pose:  peek（贴边收起） -> awake（点开） -> menu（悬停展开按钮扇区）
 *   anim:  idle / look / sleep / spark / smile / hi
 *   常量:  HI_INTERVAL=30s（打招呼节流）、HI_DURATION=1600ms、
 *          SLEEP_AFTER=45s、DRAG_THRESHOLD=5px
 */
const HI_INTERVAL_MS = 30_000;
const HI_DURATION_MS = 1600;
const SLEEP_AFTER_MS = 45_000;
const DRAG_THRESHOLD = 5;

const pose = ref("peek");
const anim = ref("idle");
const side = ref("right");
const eyeOn = ref(true);
const strength = ref(55);
const showHi = ref(false);
const expanded = ref(false);
const dragging = ref(false);

let lastInteraction = Date.now();
let lastHiAt = 0;
let sleepAt = 0;
let collapseTimer = null;
let hiTimer = null;
let animTimer = null;
let sleepInterval = null;
let dragStart = null;
const unlisteners = [];

const isMac =
  navigator.userAgent.includes("Macintosh") || navigator.platform.toLowerCase().includes("mac");

function touch() {
  lastInteraction = Date.now();
  if (anim.value === "sleep") anim.value = expanded.value ? "look" : "idle";
}
function armSleepCheck(minMs = 400) {
  sleepAt = Date.now() + minMs;
}
function clearCollapse() {
  if (collapseTimer) window.clearTimeout(collapseTimer);
}
function scheduleCollapse(delay = 450) {
  if (dragging.value) return;
  clearCollapse();
  collapseTimer = window.setTimeout(() => {
    if (dragging.value || expanded.value) return;
    showHi.value = false;
    setPose("peek");
  }, delay);
}

async function setPose(next) {
  if (pose.value === next || dragging.value) return;
  const categoryChanged = category(pose.value) !== category(next);
  armSleepCheck();
  pose.value = next;
  if (categoryChanged) {
    await setDesktopEarPose({ pose: next === "menu" ? "menu" : "peek" });
    armSleepCheck();
  }
  if (next === "peek" && !expanded.value) anim.value = "idle";
}
function category(p) {
  return p === "menu" ? "menu" : "idle";
}

/** 招手“嗨”：30s 节流 */
function maybeWave() {
  const now = Date.now();
  if (now - lastHiAt < HI_INTERVAL_MS) return;
  lastHiAt = now;
  showHi.value = true;
  if (hiTimer) window.clearTimeout(hiTimer);
  hiTimer = window.setTimeout(() => (showHi.value = false), HI_DURATION_MS);
}

function transient(next, revert, ms) {
  anim.value = next;
  if (animTimer) window.clearTimeout(animTimer);
  animTimer = window.setTimeout(() => {
    anim.value = expanded.value ? revert : "idle";
  }, ms);
}

// ---- 悬停展开 / 收起 ----
function onEnter() {
  if (dragging.value) return;
  expanded.value = true;
  touch();
  clearCollapse();
  anim.value = pose.value === "menu" ? "spark" : "look";
  if (pose.value === "peek") {
    setPose("awake").then(() => {
      if (expanded.value) maybeWave();
    });
  } else if (pose.value === "awake") {
    maybeWave();
  }
}
function onLeave() {
  if (dragging.value) return;
  expanded.value = false;
  showHi.value = false;
  anim.value = "idle";
  const remain = Math.max(0, sleepAt - Date.now());
  scheduleCollapse((pose.value === "menu" ? 500 : 220) + remain);
}

// ---- 拖拽 / 点击 ----
function onPointerDown(e) {
  if (e.button !== 0) return;
  if (e.target.closest(".dot")) return; // 按钮不参与拖拽
  dragStart = { x: e.screenX, y: e.screenY, id: e.pointerId };
  dragging.value = false;
  touch();
  clearCollapse();
  armSleepCheck();
  e.currentTarget.setPointerCapture(e.pointerId);
}
async function onPointerMove(e) {
  if (!dragStart || dragging.value) return;
  const dx = e.screenX - dragStart.x;
  const dy = e.screenY - dragStart.y;
  if (dx * dx + dy * dy < DRAG_THRESHOLD * DRAG_THRESHOLD) return;
  dragging.value = true;
  dragStart = null;
  showHi.value = false;
  clearCollapse();
  await beginDesktopEarDrag();
}
async function onPointerUp() {
  if (dragging.value) {
    dragging.value = false;
    await endDesktopEarDrag();
    pose.value = "peek";
    anim.value = "idle";
    expanded.value = false;
    armSleepCheck(200);
    return;
  }
  if (dragStart) {
    dragStart = null;
    transient("spark", "look", 220);
    if (pose.value === "menu") setPose("peek");
    else {
      expanded.value = true;
      clearCollapse();
      setPose("menu");
    }
  }
}

// ---- 菜单按钮 ----
async function nudge(delta) {
  touch();
  expanded.value = true;
  clearCollapse();
  armSleepCheck();
  strength.value = Math.max(0, Math.min(100, strength.value + delta));
  transient("smile", "look", 900);
  await nudgeFilterStrength({ delta });
}
async function toggleEye() {
  touch();
  expanded.value = true;
  clearCollapse();
  armSleepCheck();
  eyeOn.value = !eyeOn.value;
  transient(eyeOn.value ? "smile" : "sleep", "look", 900);
  await setEyeEnabled({ enabled: eyeOn.value });
}

function applyConfig(cfg) {
  side.value = cfg.desktopEarSide === "left" ? "left" : "right";
  document.documentElement.classList.toggle("theme-dark", !!cfg.uiDarkMode);
}
function applyEye(eye) {
  strength.value = Math.round(eye.strength);
  eyeOn.value = eye.enabled;
}

onMounted(async () => {
  document.documentElement.classList.add("is-overlay");
  document.body.style.background = "transparent";

  const [cfg, eye] = await Promise.all([getConfig(), getEyeState()]);
  applyConfig(cfg);
  applyEye(eye);

  unlisteners.push(
    await listen("eye:changed", (e) => applyEye(e.payload)),
    await listen("config:changed", (e) => applyConfig(e.payload)),
    await listen("ear:collapse", () => {
      if (dragging.value) return;
      pose.value = "peek";
      showHi.value = false;
      clearCollapse();
    }),
  );

  sleepInterval = window.setInterval(() => {
    if (expanded.value || dragging.value || pose.value !== "peek") return;
    if (Date.now() - lastInteraction > SLEEP_AFTER_MS) anim.value = "sleep";
  }, 2000);
});

onBeforeUnmount(() => {
  clearCollapse();
  if (hiTimer) window.clearTimeout(hiTimer);
  if (animTimer) window.clearTimeout(animTimer);
  if (sleepInterval) window.clearInterval(sleepInterval);
  if (dragging.value) endDesktopEarDrag().catch(() => {});
  unlisteners.forEach((u) => u?.());
  document.documentElement.classList.remove("is-overlay");
});
</script>

<template>
  <div
    class="stage"
    :class="[side, `pose-${pose}`, { on: eyeOn, dragging, mac: isMac }]"
    @pointerenter="onEnter"
    @pointerleave="onLeave"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointercancel="onPointerUp"
  >
    <div v-if="showHi" class="hi">嗨</div>

    <div v-if="pose === 'menu'" class="fan">
      <button class="dot minus" type="button" title="弱一点" @pointerdown.stop @click="nudge(-5)">
        −
      </button>
      <button
        class="dot power"
        :class="{ off: !eyeOn }"
        type="button"
        :title="eyeOn ? '关掉滤镜' : '打开滤镜'"
        @pointerdown.stop
        @click="toggleEye"
      >
        <span class="spark" />
      </button>
      <button class="dot plus" type="button" title="暖一点" @pointerdown.stop @click="nudge(5)">
        +
      </button>
    </div>

    <div class="bean" :class="anim">
      <span class="bean-inner">
        <svg viewBox="0 0 48 48" aria-hidden="true">
          <defs>
            <linearGradient id="beanFill" x1="12" y1="6" x2="38" y2="42" gradientUnits="userSpaceOnUse">
              <stop offset="0" stop-color="#E8FFF8" />
              <stop offset="0.55" stop-color="#9EE6D8" />
              <stop offset="1" stop-color="#5EC4B4" />
            </linearGradient>
          </defs>
          <ellipse cx="17" cy="18" rx="8.5" ry="7.5" fill="url(#beanFill)" />
          <ellipse cx="31" cy="17" rx="9.5" ry="8" fill="url(#beanFill)" />
          <ellipse cx="24" cy="28" rx="16.5" ry="14.5" fill="url(#beanFill)" />
          <ellipse class="shine" cx="18" cy="22" rx="6" ry="3.4" fill="url(#beanFill)" />
          <g class="face">
            <rect class="eye left" x="16.2" y="25.2" width="4.2" height="5.2" rx="2.1" />
            <rect class="eye right" x="27.4" y="25.2" width="4.2" height="5.2" rx="2.1" />
            <path class="mouth" d="M21.4 32.4 C23.2 34.1 24.8 34.1 26.6 32.4" />
          </g>
        </svg>
      </span>
    </div>
  </div>
</template>
<style scoped>
/* 还原自 huyanbao.exe 内嵌 bundle 的 scoped 样式（原作用域 data-v-6f9b6efb） */
.stage{position:relative;width:100%;height:100%;overflow:hidden;background:transparent;cursor:grab;touch-action:none}
.stage.dragging{cursor:grabbing}
.bean{position:absolute;top:50%;width:44px;height:44px;pointer-events:none;transform:translateY(-50%)}
.bean-inner{display:block;width:44px;height:44px;filter:drop-shadow(0 3px 6px rgba(0,90,82,.18));transform-origin:70% 50%;animation:rest-6f9b6efb 3.2s ease-in-out infinite}
.stage.left .bean-inner{transform-origin:30% 50%}
.pose-menu .bean-inner,.dragging .bean-inner{animation:none}
svg{display:block;width:44px;height:44px;overflow:visible}
.shine{fill:#ffffff6b}
.eye{fill:#1d4f49;transition:transform .18s ease}
.mouth{fill:none;stroke:#1d4f49;stroke-width:1.35;stroke-linecap:round;opacity:0}
.stage:not(.on) .bean-inner{filter:drop-shadow(0 3px 6px rgba(0,90,82,.12)) grayscale(.35) brightness(1.05)}
.pose-peek.right .bean{right:-12px}
.pose-peek.left .bean{left:-12px}
.pose-peek.right .face{transform:translate(-4px)}
.pose-peek.left .face{transform:translate(4px)}
.pose-awake.right .bean,.pose-menu.right .bean{right:2px}
.pose-awake.left .bean,.pose-menu.left .bean{left:2px}
.sleep .eye{transform:scaleY(.22);transform-box:fill-box;transform-origin:center}
.look .eye,.spark .eye{transform:scaleY(1.12);transform-box:fill-box;transform-origin:center}
.smile .mouth{opacity:1}
.hi{position:absolute;top:6px;z-index:2;padding:3px 7px;border-radius:999px;background:#ffffffe6;color:#2b6b63;font-size:10px;line-height:1.2;box-shadow:0 4px 10px #005a521f;pointer-events:none;animation:pop-6f9b6efb 1.6s ease both}
.right .hi{left:4px}
.left .hi{right:4px}
.fan{position:absolute;top:0;right:0;bottom:0;left:0;pointer-events:none}
.dot{position:absolute;display:grid;place-items:center;width:28px;height:28px;border:0;border-radius:999px;color:#0f4d46;background:#ffffffe6;box-shadow:0 4px 10px #005a5224;font-size:15px;font-weight:700;line-height:1;pointer-events:auto;cursor:pointer}
.dot.power{width:30px;height:30px;background:#26a69a}
.dot.power.off{background:#ffffffdb}
.spark{width:8px;height:8px;border-radius:999px;background:#fff8e7;box-shadow:0 0 8px #fff4d6e6}
.dot.power.off .spark{background:#9bb8b3;box-shadow:none}
.right .minus{left:28px;top:86px}
.right .plus{left:28px;top:18px}
.right .power{left:10px;top:51px}
.left .minus{right:28px;top:86px}
.left .plus{right:28px;top:18px}
.left .power{right:10px;top:51px}
</style>
