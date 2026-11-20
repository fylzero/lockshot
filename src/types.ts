export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";
export type ExportFormat = "png" | "jpg" | "heic" | "tiff" | "gif" | "bmp" | "pdf";
export type HotkeyAction = "area" | "screen" | "editor";
export type OsName = "macos" | "windows" | "linux";

export interface Hotkeys {
  area: string;
  screen: string;
  editor: string;
}

export interface Settings {
  hotkeys: Hotkeys;
  saveDir: string;
  format: ExportFormat;
  filenameTemplate: string;
  jpegQuality: number;
  copyOnCapture: boolean;
  theme: ThemePreference;
  onboarded: boolean;
  /** "permission", "hotkeys" or "tray" while setup is in progress. */
  setupStep: string;
}

export interface Bootstrap {
  settings: Settings;
  defaultSaveDir: string;
  settingsPath: string;
  formats: ExportFormat[];
  os: OsName;
  hotkeyErrors: string[];
}

export interface CaptureReady {
  id: string;
  width: number;
  height: number;
  /** Set for an image opened from disk: its file name without extension. */
  name?: string | null;
}

export interface PermissionStatus {
  screen: "granted" | "denied" | "not-required";
  os: OsName;
  wayland: boolean;
}
