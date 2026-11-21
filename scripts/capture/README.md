# Screenshot and Demo Capture Automation

These scripts generate the screenshots and demo GIF in Lockshot's README.

## Overview

The capture runs on macOS against a real Lockshot.app. It renders a made-up dashboard to a PNG, launches Lockshot with an isolated HOME, opens the PNG in the editor, marks it up with real mouse drags, and captures the window with `screencapture`.

Output:

- `docs/screenshots/01-home.png`: Home with the capture cards
- `docs/screenshots/02-editor.png`: the editor with a box, numbered steps, an arrow, a label, a highlight and pixelated email addresses
- `docs/screenshots/03-export.png`: the format picker in the export bar
- `docs/screenshots/04-hotkeys.png`: Settings › Hotkeys
- `docs/demo.gif`: the markup drawn live, then the format switched to JPG

## Prerequisites

- **macOS** on a screen of at least 1464×960 points
- **A Lockshot.app**. The script uses, in order: `LOCKSHOT_APP` if set, the fresh build at `src-tauri/target/release/bundle/macos/Lockshot.app`, then `/Applications/Lockshot.app`
- **Google Chrome**, to render `demo-page.html`
- **ffmpeg**, for the GIF: `brew install ffmpeg`
- **Accessibility** and **Screen Recording** permission for the app that runs the script (Terminal, iTerm, Cursor, Claude, ...), in *System Settings › Privacy & Security*

## Usage

Build the app from source, then run the capture:

```bash
bun run tauri build --bundles app
```

```bash
bash scripts/capture/capture.sh
```

The build ends with an error about `TAURI_SIGNING_PRIVATE_KEY` unless the updater key is set. That step only signs the update archive; `Lockshot.app` is already built by then.

Other options:

```bash
bash scripts/capture/capture.sh --dry-run          # render the demo data only
bash scripts/capture/capture.sh --only 02-editor   # write just one screenshot (and no GIF)
```

Don't touch the mouse or keyboard while it runs, about a minute. Review the images, then commit them.

## How it works

1. **Demo image.** Headless Chrome renders `demo-page.html` at 1200×660 and 2× scale. The page is an invented analytics dashboard: every name, number and `.example` address in it is made up. At 2400×1320 pixels it fits the editor at 100%, so its position on screen is known exactly.
2. **Isolated HOME.** The app runs with `HOME=/tmp/lockshot-capture/home`, so it reads a seeded `settings.json` (setup finished, dark theme, no launch at login) instead of yours.
3. **Running alongside your Lockshot.** Lockshot is single-instance: a second launch hands its arguments to the running copy through `/tmp/com_lockshot_app_si.sock` and exits. If that socket exists, the script moves it aside while the capture copy runs and puts it back afterwards. Your copy, the installed app or `bun run tauri dev`, keeps running and is never quit.
4. **localStorage.** The webview keeps the theme and the editor's last tool and color in `~/Library/WebKit/com.lockshot.app`, which HOME doesn't redirect. `localstorage.py` saves every row before the run, clears the editor style so the capture starts from the defaults, and restores the rows exactly afterwards.
5. **Driving the UI.** Coordinates are CSS pixels in the webview, measured in a 1440×872 viewport with `bun run dev` in a browser. The window is 1440×900 with a 32 point title bar (macOS 26). Clicks and drags go through `mouse.js`, which posts CoreGraphics mouse events so the editor's canvas sees real pointer drags. Tools are picked with their keyboard shortcuts.
6. **GIF.** `screencapture -v` records the window region while the markup is drawn, and ffmpeg turns it into a 960 pixel wide, 12 fps GIF with a generated palette.

## Troubleshooting

- **Clicks land in the wrong place.** If the title bar isn't 32 points on your macOS, set `LOCKSHOT_TITLE_BAR` to the gap above the tab bar in `01-home.png`, in points. After a layout change, measure the new positions in a browser at a 1440×872 viewport (`bun run dev`, then read `getBoundingClientRect()` of the element) and update `capture.sh`.
- **An annotation is missing.** In the editor, any click on an existing annotation selects it instead of drawing, so each new annotation must start clear of the earlier ones.
- **"Operation not permitted" or blank captures.** Grant Accessibility and Screen Recording to the app running the script and restart it.
- **The open panel doesn't take the path.** The script presses ⌘⇧G in the panel and types the path; make sure nothing else grabs focus while it runs.

## Files

```
scripts/capture/
├── capture.sh        # main script
├── demo-page.html    # the invented dashboard that gets marked up
├── mouse.js          # CoreGraphics clicks and drags (JXA)
├── localstorage.py   # saves and restores the webview's localStorage
└── README.md
```
