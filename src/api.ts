import { invoke } from "@tauri-apps/api/core";
import type {
  Bootstrap,
  CaptureMode,
  CaptureReady,
  ExportFormat,
  HotkeyAction,
  OverlayStart,
  PermissionStatus,
  Settings,
  SaveToast,
  SystemShortcut,
} from "./types";

export const api = {
  bootstrap: () => invoke<Bootstrap>("bootstrap"),
  updateSettings: (next: Settings) => invoke<Settings>("update_settings", { next }),
  setWindowBlur: (radius: number) => invoke<void>("set_window_blur", { radius }),
  setHotkey: (action: HotkeyAction, accelerator: string) =>
    invoke<Settings>("set_hotkey", { action, accelerator }),
  resetHotkeys: () => invoke<Settings>("reset_hotkeys"),
  pauseHotkeys: (paused: boolean) => invoke<void>("pause_hotkeys", { paused }),
  validateHotkey: (accelerator: string) => invoke<void>("validate_hotkey", { accelerator }),
  capture: (mode: CaptureMode) => invoke<void>("capture", { mode }),
  openImage: (path: string) => invoke<void>("open_image", { path }),
  finishArea: (session: string, rect: { x: number; y: number; width: number; height: number } | null) =>
    invoke<void>("finish_area", { session, rect }),
  finishWindow: (session: string, windowId: number) => invoke<void>("finish_window", { session, windowId }),
  /** `spot`: a window was picked by clicking the part that scrolls, and `rect` is that window. */
  finishScroll: (
    session: string,
    rect: { x: number; y: number; width: number; height: number } | null,
    spot: { x: number; y: number } | null,
  ) => invoke<void>("finish_scroll", { session, rect, spot }),
  scrollStop: () => invoke<void>("scroll_stop"),
  scrollSkipText: () => invoke<void>("scroll_skip_text"),
  overlayPending: () => invoke<OverlayStart | null>("overlay_pending"),
  overlayRefit: () => invoke<void>("overlay_refit"),
  overlayShow: () => invoke<void>("overlay_show"),
  overlayReveal: () => invoke<void>("overlay_reveal"),
  cancelArea: () => invoke<void>("cancel_area"),
  latestCapture: () => invoke<CaptureReady | null>("latest_capture"),
  releaseCapture: (id: string) => invoke<void>("release_capture", { id }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  revealSettingsFile: () => invoke<void>("reveal_settings_file"),
  openFolder: (path: string) => invoke<void>("open_folder", { path }),
  settingsJson: () => invoke<string>("settings_json"),
  applySettingsJson: (raw: string) => invoke<Settings>("apply_settings_json", { raw }),
  importSettingsJson: (path: string) => invoke<string>("import_settings_json", { path }),
  exportSettingsJson: (path: string) => invoke<void>("export_settings_json", { path }),
  screens: () => invoke<string[]>("screens"),
  /** Size the window so its content area is width × height CSS pixels. */
  sizeMainWindow: (width: number, height: number) => invoke<void>("size_main_window", { width, height }),
  permissionStatus: () => invoke<PermissionStatus>("permission_status"),
  requestScreenPermission: () => invoke<PermissionStatus>("request_screen_permission"),
  requestInputPermission: () => invoke<PermissionStatus>("request_input_permission"),
  openSystemSettings: (target: "screen-recording" | "accessibility" | "keyboard-shortcuts") =>
    invoke<void>("open_system_settings", { target }),
  systemScreenshotShortcuts: () => invoke<SystemShortcut[]>("system_screenshot_shortcuts"),
  reregisterHotkeys: () => invoke<string[]>("reregister_hotkeys"),
  setSystemScreenshotShortcuts: (ids: string[], enabled: boolean) =>
    invoke<SystemShortcut[]>("set_system_screenshot_shortcuts", { ids, enabled }),
  takePendingRoute: () => invoke<string | null>("take_pending_route"),
  hideMain: () => invoke<void>("hide_main"),
  toastPending: () => invoke<SaveToast | null>("toast_pending"),
  toastDismiss: (id: string) => invoke<void>("toast_dismiss", { id }),
  toastEdit: (id: string) => invoke<void>("toast_edit", { id }),

  /** Write the final image to disk; resolves to the path written. */
  saveImage(png: Uint8Array, options: SaveOptions) {
    return invoke<string>("save_image", saveBody(png, options));
  },
  /**
   * Get a native drag of the image as a file named `name.format` going. Call
   * it the moment the file icon is pressed, and `dragFile` with the image
   * under the same `id`. On macOS it starts the drag itself when the mouse
   * moves and resolves to "started", or "released" for a plain click;
   * elsewhere it resolves to "ready" and `startDrag` begins the drag.
   */
  armDrag: (options: { id: string; format: ExportFormat; name: string }) =>
    invoke<"started" | "released" | "ready">("arm_drag", options),
  /** Write the image for the press `id` while its drag gets going. */
  dragFile(png: Uint8Array, options: { id: string; format: ExportFormat; name: string }) {
    return invoke<void>("drag_file", saveBody(png, options));
  },
  startDrag: () => invoke<void>("start_drag"),
  /** Where the file icon is (CSS pixels), so it can be grabbed in a background window; null when hidden. */
  setDragHandle: (rect: [number, number, number, number] | null) =>
    invoke<void>("set_drag_handle", { rect }),
};

type SaveOptions = { format: ExportFormat; path?: string; dir?: string; name?: string; id?: string };

/**
 * Hand the final PNG to Rust as raw bytes: a little-endian u32 length,
 * the JSON options, then the image.
 */
function saveBody(png: Uint8Array, options: SaveOptions) {
  const json = new TextEncoder().encode(JSON.stringify(options));
  const body = new Uint8Array(4 + json.length + png.length);
  new DataView(body.buffer).setUint32(0, json.length, true);
  body.set(json, 4);
  body.set(png, 4 + json.length);
  return body;
}

export function errorMessage(err: unknown) {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}
