<script setup>
import { computed, onMounted, watch } from "vue";
import { useRouter } from "vue-router";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { hideMainToTray } from "../lib/ipc";
import { useAppStore } from "../store/app";
import AppIcon from "./AppIcon.vue";
import logo from "../assets/logo.svg";

const store = useAppStore();
const router = useRouter();
const isMac =
  navigator.userAgent.includes("Macintosh") || navigator.platform.toLowerCase().includes("mac");

const nav = [
  { to: "/eye", label: "护眼", icon: "EyeOutline" },
  { to: "/health", label: "健康", icon: "HeartOutline" },
  { to: "/pet", label: "宠物", icon: "PawOutline" },
  { to: "/settings", label: "设置", icon: "SettingsOutline" },
  { to: "/about", label: "关于", icon: "InformationCircleOutline" },
];

const pad = (n) => String(n).padStart(2, "0");

/** 侧栏计时芯片：已暂停 / 休息 mm:ss / 休息倒计时 mm:ss */
const timerChip = computed(() => {
  const t = store.timer;
  if (!t) return "—";
  if (t.state === "paused") return "已暂停";
  if (t.state === "animalShowing") {
    return `休息 ${pad(Math.floor(t.restRemainingSecs / 60))}:${pad(t.restRemainingSecs % 60)}`;
  }
  return `休息倒计时 ${pad(Math.floor(t.nextRestInSecs / 60))}:${pad(t.nextRestInSecs % 60)}`;
});

onMounted(() => {
  store.init().catch(console.error);
});

watch(
  () => store.config?.uiDarkMode,
  (v) => document.documentElement.classList.toggle("theme-dark", !!v),
  { immediate: true },
);

function minimize() {
  getCurrentWindow().minimize();
}
function closeToTray() {
  hideMainToTray();
}
</script>

<template>
  <div v-if="store.ready" class="shell" :class="{ mac: isMac }">
    <header class="titlebar" data-tauri-drag-region>
      <div v-if="!isMac" class="window-title" data-tauri-drag-region>
        <img class="window-mark" :src="logo" alt="" />
        <span data-tauri-drag-region>护眼宝</span>
      </div>
      <div v-if="!isMac" class="window-actions">
        <button class="window-btn" type="button" title="最小化" @click="minimize">
          <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
            <rect x="1" y="5.25" width="10" height="1.5" rx="0.6" fill="currentColor" />
          </svg>
        </button>
        <button class="window-btn close" type="button" title="关闭" @click="closeToTray">
          <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
            <path
              d="M2.4 2.4 L9.6 9.6 M9.6 2.4 L2.4 9.6"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
            />
          </svg>
        </button>
      </div>
    </header>

    <aside class="sidebar">
      <div class="brand">
        <img class="logo" :src="logo" alt="护眼宝" />
        <div>
          <div class="name">护眼宝</div>
          <div class="sub">护眼 · 休息 · 陪伴</div>
        </div>
      </div>

      <nav>
        <router-link
          v-for="item in nav"
          :key="item.to"
          :to="item.to"
          class="nav-item"
          :class="{ active: router.currentRoute.value.path === item.to }"
        >
          <AppIcon :name="item.icon" :size="18" />
          <span>{{ item.label }}</span>
        </router-link>
      </nav>

      <div class="sidebar-foot">
        <div class="timer-chip" @click="store.togglePause()">{{ timerChip }}</div>
      </div>
    </aside>

    <main class="content">
      <router-view />
    </main>
  </div>
  <div v-else class="boot">正在启动…</div>
</template>
<style scoped>
/* 还原自 huyanbao.exe 内嵌 bundle 的 scoped 样式（原作用域 data-v-8620a25b） */
.shell{display:grid;grid-template-columns:128px minmax(0,1fr);grid-template-rows:38px minmax(0,1fr);width:100%;height:100%;min-height:0;background:var(--shell-bg)}
.titlebar{grid-column:1/-1;display:flex;align-items:center;justify-content:space-between;height:38px;padding-left:12px;background:var(--titlebar-bg);border-bottom:1px solid rgba(38,166,154,.12)}
.shell.mac .titlebar{padding-left:84px}
.window-title{display:flex;align-items:center;gap:8px;font-size:13px;font-weight:700;color:var(--ink)}
.window-mark{width:20px;height:20px;border-radius:7px}
.window-actions{display:flex;height:100%}
.window-btn{width:46px;height:100%;border:0;color:var(--muted);background:transparent;cursor:pointer;display:inline-flex;align-items:center;justify-content:center}
.window-btn:hover{background:#26a69a1a}
.window-btn.close:hover{color:#fff;background:#e57373}
.sidebar{display:flex;flex-direction:column;min-height:0;overflow:hidden;padding:14px 10px;border-right:1px solid var(--border);background:var(--sidebar-bg)}
.brand{display:flex;gap:8px;align-items:center;margin-bottom:18px;padding:4px}
.logo{width:34px;height:34px;border-radius:12px;box-shadow:var(--shadow)}
.name{font-size:13px;font-weight:700;letter-spacing:.02em}
.sub{font-size:10px;color:var(--muted);margin-top:2px}
nav{display:flex;flex-direction:column;gap:6px;flex:1}
.nav-item{display:flex;align-items:center;gap:8px;padding:10px;border-radius:12px;color:var(--muted);text-decoration:none;transition:.2s ease}
.nav-item:hover{background:#4db6ac1f;color:var(--mint-700)}
.nav-item.active{background:linear-gradient(135deg,#4db6ac3d,#26a69a2e);color:var(--mint-700);font-weight:600}
.sidebar-foot{margin-top:auto}
.timer-chip{font-size:11px;padding:8px;border-radius:10px;background:var(--card);border:1px solid var(--border);color:var(--mint-700);text-align:center;cursor:pointer}
.content{min-width:0;min-height:0;overflow:auto;padding:16px 16px 18px}
.boot{height:100%;display:grid;place-items:center;color:var(--mint-700);background:var(--mint-50)}
</style>
