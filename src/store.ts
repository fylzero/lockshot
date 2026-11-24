import { reactive, ref, watch } from "vue";
import { api, errorMessage } from "./api";
import { themePreference } from "./theme";
import type { CaptureReady, ExportFormat, OsName, Settings, SystemShortcut } from "./types";
import { formatFilename } from "./filename";
import { systemClashes } from "./hotkeys";

export const settings = ref<Settings | null>(null);
export const env = reactive({
  defaultSaveDir: "",
  settingsPath: "",
  formats: [] as ExportFormat[],
  os: "macos" as OsName,
  hotkeyErrors: [] as string[],
});

/**
 * The OS's own screenshot shortcuts. Refreshed whenever the window comes
 * back to the front, so a warning about one clears as soon as the user has
 * turned it off in System Settings.
 */
export const systemShortcuts = ref<SystemShortcut[]>([]);

/**
 * Whether captures can run (false only on macOS without Screen Recording).
 * The capture buttons grey out while it's false. The permission cards keep
 * it current as they poll.
 */
export const screenAllowed = ref(true);

export async function refreshScreenAllowed() {
  try {
    screenAllowed.value = (await api.permissionStatus()).screen !== "denied";
  } catch {
    /* keep the last answer */
  }
}

/**
 * Re-read the OS's screenshot shortcuts. `keysFreed` says the caller just
 * freed some itself, so register the hotkeys again even though the list it
 * already stored shows no change.
 */
export async function refreshSystemShortcuts(keysFreed = false) {
  const before = clashKeys();
  try {
    systemShortcuts.value = await api.systemScreenshotShortcuts();
  } catch {
    systemShortcuts.value = [];
  }
  /* macOS just gave a key back: register again so it reaches Lockshot now. */
  const after = clashKeys();
  if (keysFreed || before.some((k) => !after.includes(k))) {
    try {
      env.hotkeyErrors = await api.reregisterHotkeys();
    } catch {
      /* The warning has cleared either way; a failure shows up as usual. */
    }
  }
}

function clashKeys() {
  return systemClashes(settings.value?.hotkeys, systemShortcuts.value).map((c) => c.accelerator);
}

if (typeof window !== "undefined") {
  window.addEventListener("focus", () => void refreshSystemShortcuts());
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") void refreshSystemShortcuts();
  });
}

/** A capture open in an editor tab, with the file name it was given. */
export type CaptureDoc = CaptureReady & { name: string; takenAt: Date };

/** Every open capture, in tab order. Each gets its own Editor tab. */
export const captures = ref<CaptureDoc[]>([]);

export const toast = ref<{
  id: number;
  message: string;
  kind: "info" | "error";
  action?: { label: string; run: () => void };
} | null>(null);
let toastTimer: ReturnType<typeof setTimeout> | undefined;

export function showToast(
  message: string,
  kind: "info" | "error" = "info",
  action?: { label: string; run: () => void },
) {
  clearTimeout(toastTimer);
  toast.value = { id: Date.now(), message, kind, action };
  /* Errors and toasts with a button (like Undo) stay long enough to act on. */
  const ms = action ? 10000 : kind === "error" ? 7000 : 4000;
  toastTimer = setTimeout(() => (toast.value = null), ms);
}

export function dismissToast() {
  clearTimeout(toastTimer);
  toast.value = null;
}

export async function loadSettings() {
  const boot = await api.bootstrap();
  env.defaultSaveDir = boot.defaultSaveDir;
  env.settingsPath = boot.settingsPath;
  env.formats = boot.formats;
  env.os = boot.os;
  env.hotkeyErrors = boot.hotkeyErrors;
  settings.value = boot.settings;
  themePreference.value = boot.settings.theme;
  void refreshSystemShortcuts();
  void refreshScreenAllowed();
}

let saveTimer: ReturnType<typeof setTimeout> | undefined;

/** Persist everything but hotkeys a moment after the last edit. */
export function saveSettingsSoon() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    if (!settings.value) return;
    try {
      const saved = await api.updateSettings(settings.value);
      settings.value = { ...saved, hotkeys: settings.value.hotkeys };
    } catch (err) {
      showToast(errorMessage(err), "error");
    }
  }, 300);
}

watch(
  () => settings.value?.theme,
  (theme) => {
    if (theme) themePreference.value = theme;
  },
);

/** Add a capture as a new editor tab (or return the one already open). */
export function openCapture(ready: CaptureReady): CaptureDoc {
  const existing = captures.value.find((c) => c.id === ready.id);
  if (existing) return existing;
  const takenAt = new Date();
  const doc: CaptureDoc = {
    ...ready,
    takenAt,
    name: ready.name || formatFilename(settings.value?.filenameTemplate ?? "", takenAt, ready),
  };
  captures.value.push(doc);
  return doc;
}

/** Close a capture's tab and let Rust free its pixels. */
export function closeCapture(id: string) {
  captures.value = captures.value.filter((c) => c.id !== id);
  void api.releaseCapture(id).catch(() => undefined);
}

export const OPENABLE_EXTENSIONS = ["png", "jpg", "jpeg", "gif", "bmp", "tiff", "tif", "heic", "heif"];

/** Pick an image file and open it in the editor. */
export async function openImageFile() {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    title: "Open image",
    multiple: false,
    filters: [{ name: "Images", extensions: OPENABLE_EXTENSIONS }],
  });
  if (typeof picked !== "string") return;
  try {
    await api.openImage(picked);
  } catch (err) {
    showToast(errorMessage(err), "error");
  }
}
