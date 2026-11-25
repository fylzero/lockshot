export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";
export type ExportFormat = "png" | "jpg" | "heic" | "tiff" | "gif" | "bmp" | "pdf";
export type HotkeyAction = "area" | "window" | "scroll" | "screen" | "editor";
export type CaptureMode = "area" | "window" | "scroll" | "screen";
export type ScrollDirection = "down" | "up";
export type ScrollOutput = "both" | "image" | "pages";
export type OsName = "macos" | "windows" | "linux";

export interface Hotkeys {
  area: string;
  window: string;
  scroll: string;
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
  /** Save each capture straight to a file instead of opening the editor. */
  quickSave: boolean;
  /** Where those files go; empty means the save folder. */
  quickSaveDir: string;
  /** Scrolling Capture: towards the end, or towards the top (older messages). */
  scrollDirection: ScrollDirection;
  /** Wait after each scroll for the app to draw, in ms (100–5000). */
  scrollDelayMs: number;
  /** Stop after this many pages (2–1000). */
  scrollMaxPages: number;
  /** Save the pages and a long image of them, just the image, or just the pages. */
  scrollOutput: ScrollOutput;
  /** Also read the text off the pages into Text.txt (macOS). */
  scrollText: boolean;
  launchAtLogin: boolean;
  /** macOS: Cmd+Q hides the window and leaves Lockshot in the menu bar. */
  keepRunningInBackground: boolean;
  /** Look for a new release at launch and every few hours, and say when one is ready. */
  autoCheckUpdates: boolean;
  /** The screen the editor opens on; empty means the one you captured on. */
  editorScreen: string;
  /** Size the window to each capture that opens, up to the screen's size. */
  resizeWindowToCapture: boolean;
  /** Open every capture at 1:1, even one too big for the window. */
  alwaysActualSize: boolean;
  theme: ThemePreference;
  /** How solid the background behind the image is, 0.05–1. */
  opacity: number;
  /** Blur radius behind that see-through background; 0 is off. */
  blur: number;
  /** The background behind the image as "#rrggbb"; empty is the theme's own. */
  backgroundColor: string;
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
  /** "scroll" picks a window by the part that scrolls, or selects an area like "area". */
  mode: "area" | "window" | "scroll";
  /** Front to back; empty in area mode. */
  windows: WindowTarget[];
}

/** The save notice shown after a capture with Save Without Editing on. */
export interface SaveToast {
  id: string;
  capture: CaptureReady | null;
  path: string | null;
  fileName: string | null;
  thumbnail: { width: number; height: number; data: number[] } | null;
  copied: boolean;
  error: string | null;
  /** Scrolling Capture: pages saved so far, or in all once done. */
  pages: number | null;
  /** Scrolling Capture is still going. */
  running: boolean;
  /** The pointer is held still while Scrolling Capture runs, so only Esc stops it. */
  pointerLocked: boolean;
  /** Scrolling Capture has stopped and is writing the long image and text. */
  saving: boolean;
  /** What it is doing while saving, such as "Stitching the long image…". */
  stage: string | null;
  /** That can be skipped (reading the text): the toast offers Skip. */
  skippable: boolean;
  /** Why a Scrolling Capture ended. */
  note: string | null;
  /** What a finished Scrolling Capture saved, such as "Saved 12 pages and a long image". */
  title: string | null;
}

export interface PermissionStatus {
  screen: "granted" | "denied" | "not-required";
  os: OsName;
  wayland: boolean;
  /** Scrolling Capture's control of the mouse (macOS: Accessibility). */
  input: "granted" | "denied" | "unsupported" | "not-required";
}
