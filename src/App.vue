<script setup>
import { computed } from "vue";
import { darkTheme } from "naive-ui";
import { useAppStore } from "./store/app";

const store = useAppStore();
// 深色模式只改变操作界面，不影响护眼滤镜（原包 uiDarkMode 语义）
const theme = computed(() => (store.config?.uiDarkMode ? darkTheme : null));

const isMac =
  navigator.userAgent.includes("Macintosh") || navigator.platform.toLowerCase().includes("mac");
if (isMac) document.documentElement.classList.add("is-mac");
</script>

<template>
  <n-config-provider :theme="theme" class="app-root">
    <router-view />
  </n-config-provider>
</template>
