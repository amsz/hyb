import { createRouter, createWebHashHistory } from "vue-router";

/**
 * 路由结构来自 exe 内嵌 bundle 的路由表：
 *   /            -> redirect /eye
 *   /eye /health /pet /settings /about
 *   /overlay/filter  滤镜蒙层窗口（独立 Tauri 窗口 filter-overlay）
 *   /overlay/animal  休息宠物窗口（animal-overlay）
 *   /overlay/ear     桌面小云豆（desktop-ear）
 */
const routes = [
  {
    path: "/",
    component: () => import("./components/MainLayout.vue"),
    children: [
      { path: "", redirect: "/eye" },
      { path: "eye", component: () => import("./views/EyePage.vue") },
      { path: "health", component: () => import("./views/HealthPage.vue") },
      { path: "pet", component: () => import("./views/PetPage.vue") },
      { path: "animal", redirect: "/pet" },
      { path: "settings", component: () => import("./views/SettingsPage.vue") },
      { path: "about", component: () => import("./views/AboutPage.vue") },
    ],
  },
  { path: "/overlay/filter", component: () => import("./overlays/FilterOverlay.vue") },
  { path: "/overlay/animal", component: () => import("./overlays/AnimalOverlay.vue") },
  { path: "/overlay/ear", component: () => import("./overlays/EarOverlay.vue") },
];

export default createRouter({
  history: createWebHashHistory(),
  routes,
});
