export const isMac =
  typeof navigator === "undefined" || /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

export const isWindows = typeof navigator !== "undefined" && /Win/.test(navigator.platform || navigator.userAgent);

export const fileManagerName = isMac ? "Finder" : isWindows ? "File Explorer" : "File Manager";

export const modKey = isMac ? "⌘" : "Ctrl";
