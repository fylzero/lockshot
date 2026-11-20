export interface ChangelogRelease {
  version: string;
  date: string;
  notes: string[];
}

/* Newest first. Add an entry with every version bump. */
export const CHANGELOG: ChangelogRelease[] = [
  {
    version: "0.1.1",
    date: "November 19, 2026",
    notes: [
      "On macOS 26 and later, the app icon fills its rounded square on a dark olive background instead of sitting on a plain grey tile in the Dock, Finder, and Launchpad.",
    ],
  },
  {
    version: "0.1.0",
    date: "November 19, 2026",
    notes: [
      "Lockshot lives in the menu bar on macOS and the system tray on Windows and Linux, with Capture Area, Capture Screen, Hotkeys and Settings in its menu. Closing the window keeps it running.",
      "Capture an area by dragging over a frozen image of your screens, so menus and hover states stay put. Click without dragging to take the whole screen, or press Esc to cancel.",
      "Global hotkeys default to ⌥⇧4 for an area and ⌥⇧3 for the whole screen (Alt+Shift on Windows and Linux). Change them in Settings by pressing the new keys, or switch to the system screenshot keys with step-by-step instructions for freeing them.",
      "On macOS, Lockshot checks for Screen Recording permission before each capture and walks you through allowing it in System Settings.",
      "Mark up captures with arrows, lines, rectangles, ellipses, a pen, a highlighter, text, numbered steps, pixelate for hiding sensitive details, and crop. Shapes can be selected, moved, restyled and undone.",
      "The export bar shows the file type, an editable file name (Screenshot-date-time by default) and the save folder. Save as PNG, JPG, HEIC, TIFF, GIF, BMP or PDF, or copy the image to the clipboard.",
      "The Home tab has quick buttons to capture an area or the screen, and a short setup guide for permissions, hotkeys and the menu bar icon. An Editor tab opens beside it whenever you take a capture or open an image, and closes when you're done.",
      "Open an existing PNG, JPG, GIF, BMP, TIFF or HEIC image to mark it up, from Open image… on Home or by dropping the file on the window.",
      "On Linux under Wayland, bind lockshot --area and lockshot --screen to keys in your desktop's settings.",
    ],
  },
];
