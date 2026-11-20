export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";
export type ExportFormat = "png" | "jpg" | "heic" | "tiff" | "gif" | "bmp" | "pdf";
export type HotkeyAction = "area" | "window" | "screen" | "editor";
export type CaptureMode = "area" | "window" | "screen";
export type OsName = "macos" | "windows" | "linux";

export interface Hotkeys {
  area: string;
  window: string;
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
  launchAtLogin: boolean;
  /** macOS: Cmd+Q hides the window and leaves Lockshot in the menu bar. */
  keepRunningInBackground: boolean;
  theme: ThemePreference;
  onboarded: boolean;
  /** "permission", "hotkeys" or "tray" while setup is in progress. */
  setupStep: string;
}

/** One of the OS's own screenshot shortcuts (macOS only for now). */
export interface SystemShortcut {
  /** macOS's id for it, used to turn it off or back on. */
  id: string;
  /** As System Settings › Keyboard Shortcuts › Screenshots names it. */
  label: string;
  accelerator: string;
  enabled: boolean;
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

/** A window Capture Window can pick, in fractions of the overlay's display. */
export interface WindowTarget {
  id: number;
  x: number;
  y: number;
  width: number;
  height: number;
  label: string;
}

/** Tells a capture overlay which frozen display to show, and what a click does. */
export interface OverlayStart {
  session: string;
  index: number;
  width: number;
  height: number;
  mode: "area" | "window";
  /** Front to back; empty for an area capture. */
  windows: WindowTarget[];
}

export interface PermissionStatus {
  screen: "granted" | "denied" | "not-required";
  os: OsName;
  wayland: boolean;
}
