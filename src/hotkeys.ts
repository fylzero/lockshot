import { isMac } from "./platform";
import type { HotkeyAction, Hotkeys, OsName, SystemShortcut } from "./types";

export const HOTKEY_ACTIONS: { id: HotkeyAction; label: string; hint: string }[] = [
  { id: "area", label: "Capture area", hint: "Drag to select part of a screen. Click without dragging for the whole screen." },
  { id: "window", label: "Capture window", hint: "Click a window to capture just that window, even if others overlap it." },
  { id: "scroll", label: "Scrolling capture", hint: "Click inside a window: Lockshot scrolls it and saves a page at a time into a new folder. Press again or Esc to stop." },
  { id: "screen", label: "Capture screen", hint: "The whole screen under the pointer, straight to the editor." },
  { id: "editor", label: "Open Lockshot", hint: "Bring the editor back, with your last capture." },
];

const MODIFIER_CODES = new Set([
  "ShiftLeft", "ShiftRight", "ControlLeft", "ControlRight", "AltLeft", "AltRight", "MetaLeft", "MetaRight", "OSLeft", "OSRight",
]);

/* Keys that make sense on their own, without a modifier. */
const STANDALONE = /^(F\d{1,2}|PrintScreen|Pause|ScrollLock|Insert)$/;

/** The key part of an accelerator, from KeyboardEvent.code. */
function keyFromCode(code: string) {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  return code;
}

export type RecordResult =
  | { kind: "pending"; preview: string }
  | { kind: "done"; accelerator: string }
  | { kind: "invalid"; message: string };

/**
 * Turn a key press into an accelerator the Rust side can register, such as
 * "Alt+Shift+4". Pressing only modifiers returns a live preview.
 */
export function acceleratorFromEvent(event: KeyboardEvent): RecordResult {
  const mods: string[] = [];
  if (event.ctrlKey) mods.push("Ctrl");
  if (event.altKey) mods.push("Alt");
  if (event.shiftKey) mods.push("Shift");
  if (event.metaKey) mods.push(isMac ? "Cmd" : "Super");

  if (MODIFIER_CODES.has(event.code) || !event.code) {
    return { kind: "pending", preview: mods.length ? formatAccelerator(mods.join("+")) : "" };
  }
  const key = keyFromCode(event.code);
  if (!mods.length && !STANDALONE.test(key)) {
    return { kind: "invalid", message: "Add a modifier like Shift, Alt or Ctrl — single keys would fire while you type." };
  }
  if (mods.length === 1 && mods[0] === "Shift" && !STANDALONE.test(key)) {
    return { kind: "invalid", message: "Shift alone would fire while you type capitals. Add Alt, Ctrl or Cmd." };
  }
  return { kind: "done", accelerator: [...mods, key].join("+") };
}

const MAC_SYMBOLS: Record<string, string> = {
  ctrl: "⌃", control: "⌃", alt: "⌥", option: "⌥", shift: "⇧", cmd: "⌘", command: "⌘", super: "⌘", meta: "⌘",
  commandorcontrol: "⌘", cmdorctrl: "⌘",
};
const KEY_NAMES: Record<string, string> = {
  PrintScreen: "PrtSc", Space: "Space", Enter: "↩", Backspace: "⌫", Escape: "Esc", Tab: "⇥",
  ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→", Minus: "-", Equal: "=",
  BracketLeft: "[", BracketRight: "]", Backslash: "\\", Semicolon: ";", Quote: "'", Comma: ",",
  Period: ".", Slash: "/", Backquote: "`",
};

/** The pieces of an accelerator as they should appear on key caps. */
export function acceleratorKeys(accelerator: string, os: OsName = isMac ? "macos" : "windows") {
  if (!accelerator) return [];
  const parts = accelerator.split("+").filter(Boolean);
  return parts.map((part, i) => {
    const lower = part.toLowerCase();
    const isModifier = i < parts.length - 1;
    if (isModifier && os === "macos") return MAC_SYMBOLS[lower] ?? part;
    if (isModifier) {
      if (lower === "super" || lower === "cmd" || lower === "meta" || lower === "command") return os === "windows" ? "Win" : "Super";
      if (lower === "commandorcontrol" || lower === "cmdorctrl") return "Ctrl";
      return part;
    }
    return KEY_NAMES[part] ?? part;
  });
}

export function formatAccelerator(accelerator: string, os?: OsName) {
  const keys = acceleratorKeys(accelerator, os);
  return (os ?? (isMac ? "macos" : "windows")) === "macos" ? keys.join("") : keys.join("+");
}

/** The system's own screenshot keys, offered as one-click presets. */
export function systemPreset(os: OsName): { label: string; hotkeys: Partial<Record<HotkeyAction, string>> } | null {
  if (os === "macos") {
    return {
      label: "Use ⌘",
      hotkeys: { area: "Cmd+Shift+4", window: "Cmd+Shift+5", scroll: "Cmd+Shift+6", screen: "Cmd+Shift+3", editor: "Cmd+Shift+L" },
    };
  }
  if (os === "windows") {
    return {
      label: "Use Print Screen",
      hotkeys: { area: "PrintScreen", window: "Alt+PrintScreen", scroll: "Shift+PrintScreen", screen: "Ctrl+PrintScreen" },
    };
  }
  return {
    label: "Use Print Screen",
    hotkeys: { area: "PrintScreen", window: "Alt+PrintScreen", scroll: "Ctrl+PrintScreen", screen: "Shift+PrintScreen" },
  };
}

/**
 * For a PC keyboard on a Mac. macOS has no Print Screen key and hands the PC
 * keyboard's PrtSc over as F13, so that's what Lockshot listens for.
 */
export function pcKeyboardPreset(os: OsName): { label: string; hotkeys: Partial<Record<HotkeyAction, string>> } | null {
  if (os !== "macos") return null;
  return { label: "Use Print Screen", hotkeys: { area: "F13", window: "Alt+F13", scroll: "Ctrl+F13", screen: "Shift+F13" } };
}

const MODIFIER_ALIASES: Record<string, string> = {
  control: "ctrl", ctrl: "ctrl", alt: "alt", option: "alt", shift: "shift",
  cmd: "cmd", command: "cmd", super: "cmd", meta: "cmd", commandorcontrol: "cmd", cmdorctrl: "cmd",
};

/** An accelerator in one canonical form, so "Shift+Cmd+4" equals "Cmd+Shift+4". */
function canonical(accelerator: string) {
  const parts = accelerator.split("+").filter(Boolean);
  let key = (parts.pop() ?? "").toLowerCase();
  if (key === "prtsc" || key === "print") key = "printscreen";
  const mods = parts.map((p) => MODIFIER_ALIASES[p.toLowerCase()] ?? p.toLowerCase()).sort();
  return [...mods, key].join("+");
}

export interface SystemClash {
  action: HotkeyAction;
  accelerator: string;
  system: SystemShortcut;
}

/**
 * Lockshot hotkeys that the OS's own screenshot shortcuts still own. macOS
 * lets Lockshot register them without complaint, but its own shortcut wins,
 * so these keys open the system screenshot tool instead of Lockshot.
 */
export function systemClashes(hotkeys: Hotkeys | undefined, system: SystemShortcut[]): SystemClash[] {
  if (!hotkeys) return [];
  const clashes: SystemClash[] = [];
  for (const { id } of HOTKEY_ACTIONS) {
    const accelerator = hotkeys[id];
    if (!accelerator) continue;
    const owner = system.find((s) => s.enabled && canonical(s.accelerator) === canonical(accelerator));
    if (owner) clashes.push({ action: id, accelerator, system: owner });
  }
  return clashes;
}
