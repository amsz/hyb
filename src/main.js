import { createApp } from "vue";
import App from "./App.vue";
import router from "./router";
import naive from "naive-ui";
import "./styles/app.css";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { showMainWindow } from "./lib/ipc";

const app = createApp(App);
app.use(router);
app.use(naive);
app.mount("#app");

// 主窗口隐藏启动（tauri.conf visible:false）消灭启动白屏/透明窗闪现。
//
// show 时机的权衡：
//   - 立刻 show：DOM 已 styled（模块脚本 defer，CSS 先加载完），但浏览器可能还没画第一帧 →
//     极小概率闪 1 帧透明/背景
//   - 等双 rAF（首帧画完）再 show：绝无闪帧，但隐藏窗口下 rAF 会被合成器节流（实测拖慢 ~500ms）
// 折中：双 rAF 优先（正常路径 = 首帧画完才 show），200ms 超时兜底（rAF 被节流时不至于太慢）。
// ⚠ 不要加 opacity 淡入：窗口 transparent:true，show 时内容 opacity<1 就会被看成"透明窗口"。
// lib.rs 里还有 3s 兜底：前端挂了（JS 报错）时强制 show，窗口不会永远不出现。
// overlay 窗口（filter/animal/ear）由 Rust 定位后自己 show，不走这里。
if (getCurrentWindow().label === "main") {
  let shown = false;
  const go = () => {
    if (shown) return;
    shown = true;
    showMainWindow().catch(() => {});
  };
  requestAnimationFrame(() => requestAnimationFrame(go));
  setTimeout(go, 200);
}
