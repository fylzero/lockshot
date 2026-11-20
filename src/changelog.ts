export interface ChangelogRelease {
  version: string;
  date: string;
  notes: string[];
}

/* Newest first. Add an entry with every version bump. */
export const CHANGELOG: ChangelogRelease[] = [
  {
    version: "0.3.0",
    date: "November 20, 2026",
    notes: [
      "Drag the file icon in the export bar out of the window to drop the image as a real file, named and formatted as set in the export bar. Drop it on the Desktop, in a folder, on a browser upload, in Jira, a chat, or any app that takes files.",
      "The Home tab shows the same Lockshot icon as the menu bar instead of a house, and is a little wider so it's easier to click.",
      "Setup's Screen Recording step is a single, roomier card. Allow access and Relaunch Lockshot sit side by side, and Open System Settings is a link in the steps.",
      "Settings has a JSON page for editing settings.json directly, with line numbers and highlighting. Save checks everything first and points to the line with a problem. Import loads a settings file to look over before saving, Export writes your settings to a file, and Reveal shows where the file lives.",
      "The editor has no separate Select tool: click any annotation to select it, and drag it to move it, whichever tool you're using. With the Text tool, clicking text edits it.",
      "Arrow, Line, Rectangle, Ellipse, Pen, Highlighter and Numbered step share one toolbar button showing the tool you picked; click it to choose another. Color and size are single buttons too, each opening a small menu.",
      "Drag the dot on the corner of a text box up or down to resize the text. A new Text outline menu picks a black, white or no outline, or Automatic as before.",
      "Crop has an Apply button (or press Enter). Once applied, the editor shows just the cropped image; pick Crop again to adjust it, or Cancel (Esc) to leave it as it was.",
      "Annotations start out thicker: the old Large size is now the default, Medium. The sizes are Extra small, Small, Medium, Large and a new, thicker Extra large (keys 1 to 5), so the old Small is now Extra small.",
      "A new set of annotation colors: pink (the new default), red, orange, yellow, green, teal, purple, white and black. The custom color picker is still at the end.",
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
