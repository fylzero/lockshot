import { createApp } from "vue";
import "@fontsource/jetbrains-mono/latin-400.css";
import "@fontsource/jetbrains-mono/latin-700.css";
import "./styles.css";

if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  (await import("./devMock")).installMock();
}
const { getCurrentWindow } = await import("@tauri-apps/api/window");

/* Capture overlays share the bundle but mount a page of their own. */
if (getCurrentWindow().label.startsWith("overlay-")) {
  const { default: Overlay } = await import("./Overlay.vue");
  createApp(Overlay).mount("#app");
} else {
  await import("./theme");
  const [{ default: App }, { router }] = await Promise.all([import("./App.vue"), import("./router")]);
  createApp(App).use(router).mount("#app");
}
