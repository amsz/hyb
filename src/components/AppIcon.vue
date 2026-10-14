<script setup>
import { icons } from "./icons";

const props = defineProps({
  /** icons.js 中的图标名，例如 "EyeOutline" */
  name: { type: String, required: true },
  size: { type: Number, default: 18 },
});
</script>

<template>
  <svg
    class="app-icon"
    :viewBox="icons[props.name]?.viewBox"
    :width="props.size"
    :height="props.size"
    aria-hidden="true"
    v-html="icons[props.name]?.inner"
  />
</template>
