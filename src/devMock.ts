/**
 * Dev-only stand-in for the Tauri backend, so the UI can be worked on in a
 * plain browser at http://localhost:1440 (`bun run dev`). Never bundled into
 * the app: main.ts only imports it when the Tauri runtime is missing.
 */
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

function sampleImage() {
  const c = document.createElement("canvas");
  c.width = 1600;
  c.height = 1000;
  const g = c.getContext("2d")!;
  const grad = g.createLinearGradient(0, 0, 1600, 1000);
  grad.addColorStop(0, "#3a4229");
  grad.addColorStop(1, "#1b2a3a");
  g.fillStyle = grad;
  g.fillRect(0, 0, 1600, 1000);
  g.fillStyle = "#f5f6f8";
  g.beginPath();
  g.roundRect(200, 140, 1200, 720, 18);
  g.fill();
  g.fillStyle = "#e3e7ee";
  g.fillRect(200, 140, 1200, 56);
  ["#ff5f57", "#febc2e", "#28c840"].forEach((col, i) => {
    g.fillStyle = col;
    g.beginPath();
    g.arc(236 + i * 28, 168, 9, 0, Math.PI * 2);
    g.fill();
  });
  g.fillStyle = "#1c2330";
  g.font = "700 44px system-ui";
  g.fillText("Quarterly report", 260, 290);
  g.font = "26px system-ui";
  g.fillStyle = "#5f6b7d";
  for (let i = 0; i < 6; i++) g.fillRect(260, 340 + i * 60, 600 - (i % 3) * 120, 18);
  g.fillStyle = "#2f86d6";
  [180, 260, 140, 320, 230].forEach((h, i) => g.fillRect(940 + i * 80, 780 - h, 50, h));
  /* The app fetches raw RGBA bytes, like the capture:// protocol serves. */
  const raw = g.getImageData(0, 0, c.width, c.height).data;
  return URL.createObjectURL(new Blob([raw]));
}

export function installMock() {
  const sample = sampleImage();
  let settings = {
    hotkeys: { area: "Alt+Shift+4", screen: "Alt+Shift+3", editor: "Alt+Shift+L" },
    saveDir: "",
    format: "png",
    filenameTemplate: "Screenshot-{date}-{time}",
    jpegQuality: 90,
    copyOnCapture: false,
    theme: "system",
    onboarded: true,
  };
  mockWindows("main");
  mockIPC((cmd, args) => {
    const a = args as Record<string, any>;
    switch (cmd) {
      case "bootstrap":
        return {
          settings,
          defaultSaveDir: "/Users/demo/Pictures",
          settingsPath: "/Users/demo/Library/Application Support/com.lockshot.app/settings.json",
          formats: ["png", "jpg", "heic", "tiff", "gif", "bmp", "pdf"],
          os: "macos",
          hotkeyErrors: [],
        };
      case "update_settings":
        settings = a.next;
        return settings;
      case "set_hotkey":
        settings = { ...settings, hotkeys: { ...settings.hotkeys, [a.action]: a.accelerator } };
        return settings;
      case "latest_capture":
        return location.hash.includes("empty") ? null : { id: "sample", width: 1600, height: 1000 };
      case "permission_status":
      case "request_screen_permission":
        return { screen: location.hash.includes("denied") ? "denied" : "granted", os: "macos", wayland: false };
      case "save_image":
        return "/Users/demo/Pictures/Screenshot.png";
      case "take_pending_route":
        return null;
      case "plugin:app|version":
        return "0.1.0";
      default:
        return null;
    }
  });
  (window as any).__TAURI_INTERNALS__.convertFileSrc = () => sample;
}
