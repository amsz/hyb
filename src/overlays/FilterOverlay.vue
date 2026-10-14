<script setup>
import { computed, onMounted, ref } from "vue";
import { getEyeState, listen } from "../lib/ipc";

/**
 * 滤镜蒙层窗口（label=filter-overlay，路由 #/overlay/filter）。
 * 由 services::windows 常驻创建；gamma 不可用时作为兜底的暖色叠加层。
 * 颜色与 alpha 来自 get_eye_state / eye:changed（alpha 上限 0.68，见原包 KZ）。
 */
const MAX_ALPHA = 0.68;
const eye = ref(null);

const style = computed(() => {
  if (!eye.value?.enabled) return { backgroundColor: "transparent" };
  const alpha = Math.min(Math.max(eye.value.alpha, 0), MAX_ALPHA);
  return { backgroundColor: `rgba(${eye.value.color}, ${alpha})` };
});

onMounted(async () => {
  eye.value = await getEyeState();
  await listen("eye:changed", (e) => (eye.value = e.payload));
});
</script>

<template>
  <div class="filter" :style="style" />
</template>
<style scoped>
/* 还原自 huyanbao.exe 内嵌 bundle 的 scoped 样式（原作用域 data-v-6799d199） */
.filter{position:fixed;top:0;right:0;bottom:0;left:0;pointer-events:none;background-color:transparent;transition:background-color .18s linear}
</style>
