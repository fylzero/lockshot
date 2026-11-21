import { createApp } from "vue";
import "@fontsource/jetbrains-mono/latin-400.css";
import "@fontsource/jetbrains-mono/latin-700.css";
import "./styles.css";

if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  (await import("./devMock")).installMock();
}
const { getCurrentWindow } = await import("@tauri-apps/api/window");

/* Capture overlays and the save notice share the bundle but mount pages of their own. */
const label = getCurrentWindow().label;
if (label.startsWith("overlay-")) {
  const { default: Overlay } = await import("./Overlay.vue");
  createApp(Overlay).mount("#app");
} else if (label === "toast") {
  await import("./theme");
  const { default: SaveToast } = await import("./SaveToast.vue");
  createApp(SaveToast).mount("#app");
} else {
  await import("./theme");
  const [{ default: App }, { router }] = await Promise.all([import("./App.vue"), import("./router")]);
  createApp(App).use(router).mount("#app");
}
