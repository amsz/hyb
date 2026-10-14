<script setup>
import { onMounted, ref } from "vue";
import { checkForUpdates, getAppVersion, openUrl } from "../lib/ipc";
import logo from "../assets/logo.svg";

const SHARE_URL = "https://huyanbao.cc";
const FEEDBACK_URL = "https://tally.so/r/Y5ljDB"; // 原包 VZ 常量

const copied = ref(false);
const checking = ref(false);
const version = ref("0.1.2");
const updateText = ref("当前版本 0.1.2");

const isMac =
  navigator.userAgent.includes("Macintosh") || navigator.platform.toLowerCase().includes("mac");

onMounted(async () => {
  try {
    version.value = await getAppVersion();
    updateText.value = `当前版本 ${version.value}`;
  } catch {
    /* 版本读取失败时保留占位 */
  }
});

async function copyShare() {
  await navigator.clipboard.writeText(SHARE_URL);
  copied.value = true;
  window.setTimeout(() => (copied.value = false), 1400);
}

function openFeedback() {
  openUrl(FEEDBACK_URL);
}

async function checkUpdate() {
  checking.value = true;
  updateText.value = "正在检查更新…";
  try {
    const d = await checkForUpdates();
    updateText.value = d.latestVersion
      ? `${d.message} 最新版本：${d.latestVersion}`
      : d.message;
  } catch {
    updateText.value = "检查更新失败，请稍后再试。";
  } finally {
    checking.value = false;
  }
}
</script>

<template>
  <div class="page">
    <section class="hero">
      <img class="hero-logo" :src="logo" alt="护眼宝" />
      <div>
        <h2>护眼宝</h2>
        <p>一个轻松可爱的桌面护眼小助手，陪你调好屏幕光线，也提醒你适时离开屏幕休息一下。</p>
      </div>
    </section>

    <section class="block">
      <div class="title">它会做什么</div>
      <div class="feature">柔和调节屏幕色温，让长时间看屏幕更舒服</div>
      <div class="feature">按你的节奏安排休息，工作时专注，到点就放松</div>
      <div class="feature">用可爱的宠物动画提醒你眨眨眼、动一动</div>
    </section>

    <section v-if="!isMac" class="block compact">
      <div>
        <div class="title">分享给朋友</div>
        <div class="muted">{{ SHARE_URL }}</div>
      </div>
      <n-button type="primary" secondary @click="copyShare">
        {{ copied ? "已复制" : "复制链接" }}
      </n-button>
    </section>

    <section class="block compact">
      <div>
        <div class="title">反馈问题</div>
        <div class="muted">建议、Bug、宠物需求都可以发给我们。</div>
      </div>
      <n-button type="primary" @click="openFeedback">打开反馈</n-button>
    </section>

    <section class="block compact">
      <div>
        <div class="title">版本更新</div>
        <div class="muted">{{ updateText }}</div>
      </div>
      <n-button type="primary" secondary :loading="checking" @click="checkUpdate">
        检查更新
      </n-button>
    </section>

    <div class="version">Version {{ version }}</div>
  </div>
</template>
<style scoped>
/* 还原自 huyanbao.exe 内嵌 bundle 的 scoped 样式（原作用域 data-v-17b6c252） */
.hero{display:grid;grid-template-columns:56px 1fr;gap:14px;align-items:center;margin-bottom:14px}
.hero-logo{width:56px;height:56px;border-radius:18px;box-shadow:var(--shadow)}
h2{margin:0;font-size:20px}
p{margin:6px 0 0;color:var(--muted);font-size:12px;line-height:1.55}
.block{margin-bottom:12px;padding:14px;border:1px solid var(--border);border-radius:16px;background:var(--card)}
.block.compact{display:flex;justify-content:space-between;align-items:center;gap:12px}
.title{margin-bottom:8px;font-size:13px;font-weight:700}
.feature{padding:7px 0;color:var(--ink);font-size:12px;border-top:1px solid rgba(38,166,154,.1)}
.feature:first-of-type{border-top:0}
.muted{color:var(--muted);font-size:12px;word-break:break-all}
.version{margin-top:16px;color:var(--muted);font-size:11px;text-align:center}
</style>
