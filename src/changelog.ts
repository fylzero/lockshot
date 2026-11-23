export interface ChangelogRelease {
  version: string;
  date: string;
  notes: string[];
}

/* Newest first. Add an entry with every version bump. */
export const CHANGELOG: ChangelogRelease[] = [
  {
    version: "1.4.1",
    date: "November 23, 2026",
    notes: [
      "On macOS the window's close, minimize and zoom buttons sit in the tab bar, so there's no separate title bar strip above the tabs. Drag the window by any empty part of the tab bar, or double-click it to zoom.",
    ],
  },
  {
    version: "1.4.0",
    date: "November 23, 2026",
    notes: [
      "Drag with the Numbered step tool to add a cone pointing at something. Drag the dot at its tip to re-aim it; hold Shift for 45° angles.",
      "The editor opens on the desktop (Space) and screen you captured on. To always use one screen, pick it in Settings › General › Open the editor on.",
      "The window sizes itself to each capture so it shows at 1:1, and fits captures too big for the screen. New settings in General turn this off or always open at 1:1.",
      "Scrolling Capture can pick a window: hover to highlight it, then click the part that scrolls.",
      "Scrolling Capture keeps other windows and the mouse pointer out of the pages.",
      "Area capture with more than one display no longer shows one screen's image squished onto another.",
    ],
  },
  {
    version: "1.3.5",
    date: "November 22, 2026",
    notes: [
      "While Scrolling Capture reads the text (OCR) after it stops, the Saving notice has a Skip button. Skip stops the reading right away; the pages and the long image are still saved, along with the text read so far.",
      "In the light theme, the logo on the home screen sits on its dark rounded tile, so it no longer shows rough dark edges and pale lettering on the light background.",
    ],
  },
  {
    version: "1.3.4",
    date: "November 21, 2026",
    notes: [
      "Dragging the file icon in the preview's corner onto the Desktop or a Finder folder now drops the file there. Before, Finder took the drop but wrote nothing. Dropping on a browser, a chat or another app works as before, even with a big capture dropped the instant it is grabbed.",
      "The file icon picks up a drag the moment you move, even with a quick flick, and every grab works. Before, a fast drag often ended before it began, and the grab after each drag did nothing.",
      "Start a Scrolling Capture from the buttons at the top right of the window, between Capture window and Capture screen, as on the home screen.",
    ],
  },
  {
    version: "1.3.3",
    date: "November 21, 2026",
    notes: [
      "Scrolling Capture no longer repeats the bottom of the page in the long image. When a page reached its end on a list of look-alike rows, like a GitHub or email list, a last scroll that moved nothing could be taken for a small one and part of the page was stitched on again.",
      "Scrolling Capture no longer pauses on a page while the text is read. The first capture after installing or updating could sit on one page for 15 seconds or more while macOS loaded its text-reading models; now scrolling carries on and the text catches up while it saves.",
    ],
  },
  {
    version: "1.3.2",
    date: "November 21, 2026",
    notes: [
      "Scrolling Capture no longer stops after one page when the middle of the selected area is over something that does not scroll, like a still window or a header. Lockshot now tries other spots in the area until one scrolls, and keeps scrolling there. If nothing in the area scrolls, it says so.",
      "Saving text from the first Scrolling Capture after installing or updating is quicker. macOS loads its text-reading models the first time a new version reads text, which could take a while, so Lockshot now does that in the background when it starts.",
      "When Scrolling Capture finishes, the Saving and Saved notices stay where the progress notice was, instead of jumping to another corner of the screen.",
      "The Saving notice says what Scrolling Capture is doing: stitching the long image, then reading the text (OCR), with how many parts are read so far on a long capture.",
    ],
  },
  {
    version: "1.3.1",
    date: "November 21, 2026",
    notes: [
      "Clearer setup steps for Screen Recording and Accessibility on macOS. Allow access opens System Settings right on the list you need, so the steps now just say to turn on the switch next to Lockshot instead of pointing you through Privacy & Security menus.",
    ],
  },
  {
    version: "1.3.0",
    date: "November 21, 2026",
    notes: [
      "Scrolling Capture selects an area instead of a window. Drag over the part of the screen to scroll, like a chat thread or a web page, or click for the whole screen. Lockshot scrolls the middle of it and saves just that area as pages, a long image or text, as before. A white border frames the area while it runs, and the progress notice keeps out of it, so neither lands in the pages.",
      "On macOS and Windows the mouse pointer is locked in place while Scrolling Capture runs, so bumping the mouse no longer ends it early. Press Esc or the scrolling hotkey again to stop.",
      "Selected text in a text box keeps its color while you edit it, with a light highlight behind it, instead of turning white and blending into a white outline.",
      "Selected text now shows a resize dot on its lower-right corner, so you can make it bigger or smaller without opening it for editing first.",
      "Home has an Open screenshot folder link under Open image… that opens the folder your captures go to in Finder, File Explorer or your file manager. The folder settings have an Open button too.",
    ],
  },
  {
    version: "1.2.0",
    date: "November 20, 2026",
    notes: [
      "Hold Shift with the Highlighter to draw a straight horizontal highlight.",
      "Hold Shift with the Pen to draw a straight line, horizontal or vertical depending on which way you drag. Let go of Shift mid-stroke to go back to freehand.",
      "Select a rectangle, ellipse, line or arrow to get a resize dot, like the one on Redact boxes. Drag the dot on a rectangle or ellipse's lower-right corner to change its size, or the dot on a line or arrow's lower-right end to make it longer or shorter. The stroke stays the same thickness. Hold Shift to keep a rectangle or ellipse square, or a line at 45° steps.",
    ],
  },
  {
    version: "1.1.0",
    date: "November 20, 2026",
    notes: [
      "Capture Window (⌥⇧5): click a window to capture just that window.",
      "Scrolling Capture (⌥⇧6): click inside a window and Lockshot scrolls it a page at a time, saving every page to a new folder until it reaches the end. Made for backing up long Slack, Discord and Teams threads. Scroll down, or up to save a thread's history. Esc, Stop or moving the mouse ends it early. Options are in Settings › General; on macOS it asks for Accessibility permission the first time.",
      "Scrolling Capture also joins the pages into one long image as it goes, with no overlap and the window's header and message box kept once. Choose pages and a long image, just the image, or just the pages in Settings › General.",
      "On macOS, Scrolling Capture can save the text of the thread too: turn on Save the text and the messages are read off the pages into Text.txt, on your Mac.",
      "Home shows all four capture buttons in one row.",
      "Save Without Editing: turn it on from the menu bar icon or Settings and every capture is saved straight to a file in your default format, without opening the editor. A small notice in the corner shows each one, with Edit to open it for markup. Pick a separate folder for these in Settings › General.",
      "Annotations are the same size on every screenshot. A size looks the same at 1:1 zoom whether the capture is a small area or a full Retina screen, instead of growing with the image.",
      "The back end of an arrow is rounded, like the Line tool.",
      "The editor remembers your tool, shape, color, size and Redact style after you quit and reopen Lockshot.",
      "Select a Redact box to get a dot on its corner, and drag the dot to resize the box. Hold Shift to keep it square.",
      "Settings no longer has an About tab. The version and GitHub link are in the status bar at the bottom of the window.",
      "Toast messages appear in the bottom-left corner, just above the status bar, and their close button is a plain × that lights up on hover, matching Recon and Shipyard.",
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
