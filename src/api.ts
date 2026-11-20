import { invoke } from "@tauri-apps/api/core";
import type {
  Bootstrap,
  CaptureReady,
  ExportFormat,
  HotkeyAction,
  PermissionStatus,
  Settings,
} from "./types";

export const api = {
  bootstrap: () => invoke<Bootstrap>("bootstrap"),
  updateSettings: (next: Settings) => invoke<Settings>("update_settings", { next }),
  setHotkey: (action: HotkeyAction, accelerator: string) =>
    invoke<Settings>("set_hotkey", { action, accelerator }),
  resetHotkeys: () => invoke<Settings>("reset_hotkeys"),
  pauseHotkeys: (paused: boolean) => invoke<void>("pause_hotkeys", { paused }),
  validateHotkey: (accelerator: string) => invoke<void>("validate_hotkey", { accelerator }),
  capture: (mode: "area" | "screen") => invoke<void>("capture", { mode }),
  openImage: (path: string) => invoke<void>("open_image", { path }),
  finishArea: (session: string, rect: { x: number; y: number; width: number; height: number } | null) =>
    invoke<void>("finish_area", { session, rect }),
  overlayPending: () =>
    invoke<{ session: string; index: number; width: number; height: number } | null>("overlay_pending"),
  cancelArea: () => invoke<void>("cancel_area"),
  latestCapture: () => invoke<CaptureReady | null>("latest_capture"),
  releaseCapture: (id: string) => invoke<void>("release_capture", { id }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  revealSettingsFile: () => invoke<void>("reveal_settings_file"),
  permissionStatus: () => invoke<PermissionStatus>("permission_status"),
  requestScreenPermission: () => invoke<PermissionStatus>("request_screen_permission"),
  openSystemSettings: (target: "screen-recording" | "keyboard-shortcuts") =>
    invoke<void>("open_system_settings", { target }),
  takePendingRoute: () => invoke<string | null>("take_pending_route"),
  hideMain: () => invoke<void>("hide_main"),

  /**
   * Hand the final PNG to Rust as raw bytes: a little-endian u32 length,
   * the JSON options, then the image.
   */
  saveImage(
    png: Uint8Array,
    options: { format: ExportFormat; path?: string; dir?: string; name?: string },
  ) {
    const json = new TextEncoder().encode(JSON.stringify(options));
    const body = new Uint8Array(4 + json.length + png.length);
    new DataView(body.buffer).setUint32(0, json.length, true);
    body.set(json, 4);
    body.set(png, 4 + json.length);
    return invoke<string>("save_image", body);
  },
};

export function errorMessage(err: unknown) {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}
