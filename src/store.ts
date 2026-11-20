import { reactive, ref, watch } from "vue";
import { api, errorMessage } from "./api";
import { themePreference } from "./theme";
import type { CaptureReady, ExportFormat, OsName, Settings } from "./types";
import { formatFilename } from "./filename";

export const settings = ref<Settings | null>(null);
export const env = reactive({
  defaultSaveDir: "",
  settingsPath: "",
  formats: [] as ExportFormat[],
  os: "macos" as OsName,
  hotkeyErrors: [] as string[],
});

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
  toastTimer = setTimeout(() => (toast.value = null), kind === "error" ? 7000 : 4000);
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
