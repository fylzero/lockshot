export interface ChangelogRelease {
  version: string;
  date: string;
  notes: string[];
}

/* Newest first. Add an entry with every version bump. */
export const CHANGELOG: ChangelogRelease[] = [
  {
    version: "1.1.0",
    date: "November 20, 2026",
    notes: [
      "Annotations are the same size on every screenshot. A size looks the same at 1:1 zoom whether the capture is a small area or a full Retina screen, instead of growing with the image.",
      "The back end of an arrow is rounded, like the Line tool.",
      "The editor remembers your tool, shape, color, size and Redact style after you quit and reopen Lockshot.",
      "New setting on macOS: Keep running in the background. With it on (the default), ⌘Q closes the window and leaves Lockshot in the menu bar so your hotkeys keep working. Quit Lockshot from the menu bar icon to stop it.",
    ],
  },
  {
    version: "1.0.0",
    date: "November 20, 2026",
    notes: [
      "Redact (B) replaces Pixelate, with three styles: Pixelate, solid white or solid black.",
      "Pixelate is cleaner and adapts to the box: fine blocks on small areas, bigger blocks on large ones.",
      "The file name, format and Save buttons moved into the toolbar, and the bottom bar is gone.",
      "Drag the file icon in the preview's corner out of the window to drop the image as a file anywhere.",
      "Zoom controls and the image size sit in the preview's lower-left corner.",
      "Arrow has its own button; the other drawing tools and the Redact styles are split buttons.",
      "Click any annotation to select or move it. There's no separate Select tool.",
      "Drag the corner dot on a text box to resize it, and pick its outline from the Text outline menu.",
      "Crop has Apply (Enter) and Cancel (Esc).",
      "Thicker default annotation sizes, a new Extra large size, and a new color set with pink as default.",
      "Settings has a JSON page for editing, importing and exporting settings.json.",
      "If macOS's own screenshot shortcuts still own a Lockshot hotkey, Lockshot says so and can turn them off for you, with Undo.",
      "On a Mac with a PC keyboard, PrtSc can be your capture key: click Use PrtSc (PC keyboard) in Settings → Hotkeys for PrtSc (area) and Shift+PrtSc (screen).",
      "On Windows 11, if the Snipping Tool still owns PrtSc, Lockshot says so and opens the setting to turn it off.",
      "Lockshot starts at login. Turn it off in Settings → General.",
      "Setup's Screen Recording step and the Home tab icon are tidied up.",
    ],
  },
  {
    version: "0.2.0",
    date: "November 19, 2026",
    notes: [
      "Each screenshot, and each image you open, gets its own Editor tab named after its file. Every tab keeps its own annotations, undo history, crop and file name while you switch between them, and closing a tab frees its memory.",
      "⌘W (Ctrl+W on Windows and Linux) closes the tab you're on, working back to Home, and only then closes the window. Lockshot keeps running in the menu bar.",
      "On macOS, Lockshot has a full menu bar, with Settings (⌘,) and Check for Updates in the Lockshot menu.",
      "Home walks through setup one step at a time: allow Screen Recording, pick your hotkeys, then find Lockshot in the menu bar. The capture buttons appear once setup is done, and the Screen Recording card comes back if access is ever turned off.",
      "The Home tab is a home icon in the corner, and stays put while lots of screenshot tabs scroll past it. The tab bar no longer shows a scrollbar; scroll it with the wheel or a swipe, and the tab you're on is always in view.",
      "Capture Area no longer dims the screen or draws guide lines that trailed the mouse. You get the normal crosshair pointer, and the selection is a solid white outline.",
      "The editor sits on a plain neutral grey instead of a checkerboard, and its toolbar no longer changes height when Reset or Clear crop appears.",
      "The export bar is a single row, with the file name, format, and buttons all the same height. Choose where Save puts files in Settings.",
      "The hotkeys shown on Home's capture buttons are easier to read.",
    ],
  },
  {
    version: "0.1.2",
    date: "November 19, 2026",
    notes: [
      "Capture Area no longer quits Lockshot on macOS.",
      "Captures show up in the editor again, and Capture Area's selection overlay appears, instead of an empty editor or nothing at all.",
      "Capture Area opens almost instantly instead of after a few seconds. The overlay covers the whole display right away, and your first drag starts the selection.",
      "The Drag to capture hint sits at the bottom of the screen, clear of the notch and menu bar.",
    ],
  },
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
