import { createApp } from "vue";
import "@fontsource/jetbrains-mono/latin-400.css";
import "@fontsource/jetbrains-mono/latin-700.css";
import "./styles.css";

if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  (await import("./devMock")).installMock();
}
const { getCurrentWindow } = await import("@tauri-apps/api/window");

/* Capture overlays, the save notice and the scrolling border share the bundle but mount pages of their own. */
const label = getCurrentWindow().label;
if (label.startsWith("frame-")) {
  /* A strip of the border around a Scrolling Capture's area: just colour. */
  document.documentElement.style.background = "#fff";
  document.body.style.cssText = "margin:0;height:100vh;background:#fff;box-shadow:inset 0 0 0 1px rgb(0 0 0 / 0.45)";
} else if (label.startsWith("overlay-")) {
  const { default: Overlay } = await import("./Overlay.vue");
  createApp(Overlay).mount("#app");
} else if (label === "toast") {
  await import("./theme");
  const { default: SaveToast } = await import("./SaveToast.vue");
  createApp(SaveToast).mount("#app");
} else {
  /* The main window is transparent so the background behind the image can be see-through. */
  document.documentElement.classList.add("main-window");
  await import("./theme");
  const [{ default: App }, { router }] = await Promise.all([import("./App.vue"), import("./router")]);
  createApp(App).use(router).mount("#app");
}
