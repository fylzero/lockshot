import { createRouter, createWebHashHistory } from "vue-router";
import HomeView from "./views/HomeView.vue";
import { captures } from "./store";

/**
 * Editor tabs are drawn by App.vue, one kept-alive pane per capture, so
 * switching tabs never loses annotations; the editor route itself renders
 * nothing and only says which capture is showing.
 */
const EditorRoute = { render: () => null };

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "home", component: HomeView },
    {
      path: "/editor/:id",
      name: "editor",
      component: EditorRoute,
      /* A capture that was closed, or never opened, goes back Home. */
      beforeEnter: (to) => (captures.value.some((c) => c.id === to.params.id) ? true : "/"),
    },
    {
      path: "/editor",
      redirect: () => {
        const last = captures.value[captures.value.length - 1];
        return last ? `/editor/${last.id}` : "/";
      },
    },
    { path: "/settings/:section?", name: "settings", component: () => import("./views/SettingsView.vue") },
    { path: "/changelog", name: "changelog", component: () => import("./views/ChangelogView.vue") },
    { path: "/welcome", redirect: "/" },
  ],
});
