/**
 * Dev-only stand-in for the Tauri backend, so the UI can be worked on in a
 * plain browser at http://localhost:1440 (`bun run dev`). Never bundled into
 * the app: main.ts only imports it when the Tauri runtime is missing.
 */
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";

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

/** #toast, #toast-error, #toast-scrolling (in progress) or #toast-scrolled (done). */
function sampleToast(kind: string) {
  const none = { pages: null, running: false, note: null };
  if (kind.includes("error")) {
    return { id: "t1", capture: null, path: null, fileName: null, thumbnail: null, copied: false,
      error: "Could not write the image: Permission denied (os error 13)", ...none };
  }
  const width = 160, height = 100;
  const c = document.createElement("canvas");
  c.width = width;
  c.height = height;
  const g = c.getContext("2d")!;
  const grad = g.createLinearGradient(0, 0, width, height);
  grad.addColorStop(0, "#3a4229");
  grad.addColorStop(1, "#1b2a3a");
  g.fillStyle = grad;
  g.fillRect(0, 0, width, height);
  g.fillStyle = "#f5f6f8";
  g.fillRect(20, 14, 120, 72);
  const thumbnail = { width, height, data: Array.from(g.getImageData(0, 0, width, height).data) };
  if (kind.includes("scrolling")) {
    return { id: "t1", capture: null, path: null, fileName: null, thumbnail, copied: false, error: null,
      pages: 12, running: true, note: null };
  }
  if (kind.includes("scrolled")) {
    return { id: "t1", capture: null, path: "/Users/demo/Pictures/Screenshot-2026-11-20-18.04.29 Scrolling",
      fileName: "Screenshot-2026-11-20-18.04.29 Scrolling", thumbnail, copied: false, error: null,
      pages: 37, running: false, note: "Reached the end" };
  }
  return {
    ...none,
    id: "t1",
    capture: { id: "sample", width: 1600, height: 1000, name: "Screenshot-2026-11-20-18.04.29" },
    path: "/Users/demo/Pictures/Screenshot-2026-11-20-18.04.29.png",
    fileName: "Screenshot-2026-11-20-18.04.29.png",
    thumbnail,
    copied: true,
    error: null,
  };
}

export function installMock() {
  /* macOS's screenshot shortcuts; add #sysfree to the URL to start with them off. */
  const on = !location.hash.includes("sysfree");
  let mockShortcuts = [
    { id: "28", label: "Save picture of screen as a file", accelerator: "Shift+Cmd+3", enabled: on },
    { id: "29", label: "Copy picture of screen to the clipboard", accelerator: "Ctrl+Shift+Cmd+3", enabled: true },
    { id: "30", label: "Save picture of selected area as a file", accelerator: "Shift+Cmd+4", enabled: on },
    { id: "31", label: "Copy picture of selected area to the clipboard", accelerator: "Ctrl+Shift+Cmd+4", enabled: true },
    { id: "184", label: "Screenshot and recording options", accelerator: "Shift+Cmd+5", enabled: false },
  ];
  const sample = sampleImage();
  let settings = {
    hotkeys: { area: "Alt+Shift+4", window: "Alt+Shift+5", scroll: "Alt+Shift+6", screen: "Alt+Shift+3", editor: "Alt+Shift+L" },
    saveDir: "",
    format: "png",
    filenameTemplate: "Screenshot-{date}-{time}",
    jpegQuality: 90,
    copyOnCapture: false,
    quickSave: false,
    quickSaveDir: "",
    scrollDirection: "down",
    scrollDelayMs: 500,
    scrollMaxPages: 200,
    launchAtLogin: true,
    keepRunningInBackground: true,
    theme: "system",
    onboarded: !location.hash.includes("setup"),
    setupStep: "",
  };
  /* #toast shows the save notice window; #toast-error its failure state. */
  const toastWindow = location.hash.includes("toast");
  mockWindows(toastWindow ? "toast" : "main");
  let opened = 0;
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
      case "settings_json":
      case "import_settings_json":
        return JSON.stringify(settings, null, 2) + "\n";
      case "apply_settings_json":
        try {
          settings = { ...settings, ...JSON.parse(a.raw) };
        } catch (err) {
          throw `Line 1, column 1: ${err}`;
        }
        return settings;
      case "update_settings":
        settings = a.next;
        return settings;
      case "set_hotkey":
        settings = { ...settings, hotkeys: { ...settings.hotkeys, [a.action]: a.accelerator } };
        return settings;
      case "latest_capture":
        return location.hash.includes("empty") ? null : { id: "sample", width: 1600, height: 1000 };
      case "reregister_hotkeys":
        return [];
      case "system_screenshot_shortcuts":
        return mockShortcuts;
      case "set_system_screenshot_shortcuts":
        mockShortcuts = mockShortcuts.map((s) => (a.ids.includes(s.id) ? { ...s, enabled: a.enabled } : s));
        return mockShortcuts;
      case "permission_status":
      case "request_screen_permission":
      case "request_input_permission":
        return {
          screen: location.hash.includes("denied") ? "denied" : "granted",
          os: "macos",
          wayland: false,
          input: location.hash.includes("denied") ? "denied" : "granted",
        };
      case "save_image":
        return "/Users/demo/Pictures/Screenshot.png";
      case "drag_image":
        return null;
      case "plugin:dialog|open":
        return "/Users/demo/Pictures/Sample.png";
      case "open_image":
        /* Like a real capture: Rust announces it and the app opens a tab. */
        opened += 1;
        setTimeout(() => void emit("capture-ready", { id: `opened-${opened}`, width: 1600, height: 1000, name: `Sample-${opened}` }), 50);
        return null;
      case "take_pending_route":
        return null;
      case "toast_pending":
        return toastWindow ? sampleToast(location.hash) : null;
      case "plugin:app|version":
        return "0.1.0";
      default:
        return null;
    }
  }, { shouldMockEvents: true });
  (window as any).__TAURI_INTERNALS__.convertFileSrc = () => sample;
}
