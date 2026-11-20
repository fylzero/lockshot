import { createRouter, createWebHashHistory } from "vue-router";
import HomeView from "./views/HomeView.vue";
import { current } from "./store";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "home", component: HomeView },
    {
      path: "/editor",
      name: "editor",
      component: () => import("./views/EditorView.vue"),
      /* No capture, no editor: the tab only exists while one is open. */
      beforeEnter: () => (current.value ? true : "/"),
    },
    { path: "/editor/:id", redirect: "/editor" },
    { path: "/settings/:section?", name: "settings", component: () => import("./views/SettingsView.vue") },
    { path: "/changelog", name: "changelog", component: () => import("./views/ChangelogView.vue") },
    { path: "/welcome", redirect: "/" },
  ],
});
