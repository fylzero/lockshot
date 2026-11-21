#!/bin/bash
# Automated screenshot and demo GIF capture for the Lockshot README.
#
# Runs a Lockshot.app against an isolated HOME, opens a made-up dashboard
# (demo-page.html) in the editor, marks it up with real mouse drags, and writes:
#   docs/screenshots/01-home.png ... 04-hotkeys.png
#   docs/demo.gif
#
# Requirements (macOS, Apple Silicon):
#   - A Lockshot.app: the fresh build in src-tauri/target/release (bun run
#     tauri build --bundles app), else /Applications/Lockshot.app, or set
#     LOCKSHOT_APP to another bundle
#   - Google Chrome (renders the demo page), ffmpeg (brew install ffmpeg)
#   - Accessibility and Screen Recording permission for the app running this
#     script (Terminal, Cursor, ...). See scripts/capture/README.md.
#
# Usage:
#   scripts/capture/capture.sh [--dry-run] [--only <name>]
#   names: 01-home 02-editor 03-export 04-hotkeys
#
# Safety:
#   - A Lockshot that is already running (the installed app or `tauri dev`)
#     keeps running and is never quit. Its single-instance socket is set aside
#     while the capture copy runs, then put back.
#   - Your real settings are untouched. HOME is redirected for the app's data
#     directory, and the webview's localStorage is saved first and restored
#     afterwards (see localstorage.py).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCREENSHOTS_DIR="$PROJECT_ROOT/docs/screenshots"
DEMO_GIF_PATH="$PROJECT_ROOT/docs/demo.gif"

WORK_DIR="/tmp/lockshot-capture"
ISOLATED_HOME="$WORK_DIR/home"
DEMO_IMAGE="$WORK_DIR/Q4 sign-ups.png"
STORAGE_BACKUP="$WORK_DIR/localstorage.json"
SOCKET="/tmp/com_lockshot_app_si.sock"   # tauri-plugin-single-instance
SOCKET_ASIDE="$SOCKET.capture-aside"

WINDOW_WIDTH=1440
WINDOW_HEIGHT=900
WINDOW_Y=40
# Coordinates below are CSS pixels in the webview, measured with a 1440x872
# viewport (bun run dev in a browser). Vertically centered content shifts by
# half of any difference in the real title bar height, handled by vy().
REF_VIEW_HEIGHT=872
# macOS 26's title bar, in points. Override if a different macOS draws it
# taller or shorter (it is the gap above the tab bar in a capture).
TITLE_BAR="${LOCKSHOT_TITLE_BAR:-32}"
# The demo image is 2400x1320 pixels, 1200x660 points: it fits the editor at
# 100%, centered on the stage, so its top left sits here in the viewport.
IMAGE_X=120
IMAGE_Y=135

DRY_RUN=false
ONLY_SHOT=""
APP_PID=""
RECORD_PID=""

# ---------------------------------------------------------------- output ----
BLUE=$'\033[34m'; GREEN=$'\033[32m'; RED=$'\033[31m'; YELLOW=$'\033[33m'; NC=$'\033[0m'
info()    { echo "${BLUE}▸${NC} $*"; }
success() { echo "${GREEN}✓${NC} $*"; }
warn()    { echo "${YELLOW}!${NC} $*" >&2; }
die()     { echo "${RED}✗${NC} $*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN=true; shift ;;
    --only) ONLY_SHOT="${2:-}"; shift 2 ;;
    --only=*) ONLY_SHOT="${1#*=}"; shift ;;
    -h|--help) sed -n '2,29p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) die "Unknown option: $1" ;;
  esac
done

# ------------------------------------------------------------- the app ------
if [[ -n "${LOCKSHOT_APP:-}" ]]; then
  APP_BUNDLE="$LOCKSHOT_APP"
elif [[ -d "$PROJECT_ROOT/src-tauri/target/release/bundle/macos/Lockshot.app" ]]; then
  APP_BUNDLE="$PROJECT_ROOT/src-tauri/target/release/bundle/macos/Lockshot.app"
else
  APP_BUNDLE="/Applications/Lockshot.app"
fi
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

[[ -x "$APP_BUNDLE/Contents/MacOS/lockshot" ]] || die "No Lockshot.app at $APP_BUNDLE (build one with: bun run tauri build --bundles app)"
[[ -x "$CHROME" ]] || die "Google Chrome is required to render the demo page"
command -v ffmpeg >/dev/null 2>&1 || die "ffmpeg is required: brew install ffmpeg"
command -v python3 >/dev/null 2>&1 || die "python3 is required"

# --------------------------------------------------------------- cleanup ----
STORAGE_SAVED=false
cleanup() {
  local code=$?
  [[ -n "$RECORD_PID" ]] && kill -INT "$RECORD_PID" 2>/dev/null || true
  # Quit only the instance this script started, and wait so its webview has
  # written localStorage before the saved rows go back.
  if [[ -n "$APP_PID" ]] && kill -0 "$APP_PID" 2>/dev/null; then
    kill "$APP_PID" 2>/dev/null || true
    for _ in $(seq 1 20); do kill -0 "$APP_PID" 2>/dev/null || break; sleep 0.25; done
  fi
  [[ "$STORAGE_SAVED" == true ]] && python3 "$SCRIPT_DIR/localstorage.py" restore "$STORAGE_BACKUP" || true
  if [[ -S "$SOCKET_ASIDE" ]]; then
    rm -f "$SOCKET"
    mv "$SOCKET_ASIDE" "$SOCKET"
  fi
  rm -rf "$WORK_DIR" 2>/dev/null || true
  exit $code
}
trap cleanup EXIT

# -------------------------------------------------------------- demo data ---
rm -rf "$WORK_DIR"
mkdir -p "$WORK_DIR/chrome" "$ISOLATED_HOME/Library/Application Support/com.lockshot.app" "$ISOLATED_HOME/Pictures"

info "Rendering the demo page"
"$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
  --window-size=1200,660 --user-data-dir="$WORK_DIR/chrome" \
  --screenshot="$DEMO_IMAGE" "file://$SCRIPT_DIR/demo-page.html" >/dev/null 2>&1
[[ -s "$DEMO_IMAGE" ]] || die "Chrome did not render the demo page"

# Setup on Home is already done, the theme is dark, and nothing is registered
# to launch at login for this throwaway HOME.
cat > "$ISOLATED_HOME/Library/Application Support/com.lockshot.app/settings.json" <<'JSON'
{
  "onboarded": true,
  "setupStep": "done",
  "theme": "dark",
  "launchAtLogin": false
}
JSON
success "Demo data ready"

if [[ "$DRY_RUN" == true ]]; then
  warn "Dry run: not launching the app or capturing anything"
  exit 0
fi

# Screen size decides where the window can sit.
read -r SCREEN_W SCREEN_H < <(osascript -l JavaScript -e 'ObjC.import("AppKit"); var f=$.NSScreen.mainScreen.frame; Math.round(f.size.width)+" "+Math.round(f.size.height)')
(( SCREEN_W >= WINDOW_WIDTH + 24 && SCREEN_H >= WINDOW_HEIGHT + WINDOW_Y + 20 )) \
  || die "Screen is ${SCREEN_W}x${SCREEN_H}; need at least $((WINDOW_WIDTH + 24))x$((WINDOW_HEIGHT + WINDOW_Y + 20)) for a ${WINDOW_WIDTH}x${WINDOW_HEIGHT} window"
WINDOW_X=$(( (SCREEN_W - WINDOW_WIDTH) / 2 ))

# A running Lockshot owns the single-instance socket, and a second launch would
# just hand its arguments over and exit. Set the socket aside (the running copy
# keeps listening on it) and put it back in cleanup.
if [[ -S "$SOCKET" ]]; then
  warn "Lockshot is already running. It will not be touched; only the copy started here is controlled."
  mv "$SOCKET" "$SOCKET_ASIDE"
fi

python3 "$SCRIPT_DIR/localstorage.py" backup "$STORAGE_BACKUP"
STORAGE_SAVED=true

info "Launching $(basename "$APP_BUNDLE") with an isolated HOME"
HOME="$ISOLATED_HOME" "$APP_BUNDLE/Contents/MacOS/lockshot" >/dev/null 2>&1 &
APP_PID=$!
sleep 2
# Onboarded, it starts in the menu bar; a second launch with --open shows it.
HOME="$ISOLATED_HOME" "$APP_BUNDLE/Contents/MacOS/lockshot" --open >/dev/null 2>&1 || true

# ------------------------------------------------------------- helpers ------
MOUSE=(osascript -l JavaScript "$SCRIPT_DIR/mouse.js")

window_id() {
  osascript -l JavaScript -e "ObjC.import('CoreGraphics');
    var l=ObjC.deepUnwrap(ObjC.castRefToObject(\$.CGWindowListCopyWindowInfo(\$.kCGWindowListOptionOnScreenOnly,0)));
    l.filter(w=>w.kCGWindowOwnerPID==$APP_PID&&w.kCGWindowLayer==0&&w.kCGWindowBounds.Width>500).map(w=>w.kCGWindowNumber).join('')" 2>/dev/null
}

app_sys_events() {  # run an AppleScript snippet "inside" our process
  osascript -e "tell application \"System Events\" to tell (first process whose unix id is $APP_PID) to $1" 2>/dev/null
}

bring_to_front() {
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $APP_PID) to true" 2>/dev/null || true
    [[ "$(osascript -e 'tell application "System Events" to get unix id of first process whose frontmost is true' 2>/dev/null)" == "$APP_PID" ]] && return 0
    sleep 0.3
  done
  die "Could not bring Lockshot to the front"
}

# Viewport (CSS pixel) coordinates to screen points. vy_c is for content
# centered vertically in the window, which moves by half the height change.
sx()   { echo $(( WINDOW_X + $1 )); }
sy()   { echo $(( CONTENT_Y + $1 )); }
sy_c() { echo $(( CONTENT_Y + $1 + (VIEW_HEIGHT - REF_VIEW_HEIGHT) / 2 )); }
# Demo image coordinates (points from its top left) to screen points.
ix()   { echo $(( WINDOW_X + IMAGE_X + $1 )); }
iy()   { echo $(( CONTENT_Y + IMAGE_Y + $1 + (VIEW_HEIGHT - REF_VIEW_HEIGHT) / 2 )); }

click()    { bring_to_front; "${MOUSE[@]}" click "$(sx "$1")" "$(sy "$2")"; sleep "${3:-0.8}"; }
click_c()  { bring_to_front; "${MOUSE[@]}" click "$(sx "$1")" "$(sy_c "$2")"; sleep "${3:-0.8}"; }
img_click(){ "${MOUSE[@]}" click "$(ix "$1")" "$(iy "$2")"; sleep "${3:-0.5}"; }
img_drag() { "${MOUSE[@]}" drag "$(ix "$1")" "$(iy "$2")" "$(ix "$3")" "$(iy "$4")" 0 "${5:-0.6}"; sleep 0.4; }
key() {
  local using=""
  [[ -n "${2:-}" ]] && using=" using {$2}"
  osascript -e "tell application \"System Events\" to keystroke \"$1\"$using"
  sleep "${3:-0.3}"
}
key_code() { osascript -e "tell application \"System Events\" to key code $1"; sleep "${2:-0.3}"; }
type_text(){ osascript -e "tell application \"System Events\" to keystroke \"$1\""; sleep 0.3; }
press_return() { key_code 36 "$@"; }
press_escape() { key_code 53 "$@"; }

# Park the pointer over empty status bar so nothing shows a hover state.
park_pointer() { "${MOUSE[@]}" move "$(sx 720)" "$(sy $(( VIEW_HEIGHT - 4 )))"; sleep 0.4; }

capture() {
  local name="$1"
  [[ -n "$ONLY_SHOT" && "$ONLY_SHOT" != "$name" ]] && return 0
  mkdir -p "$SCREENSHOTS_DIR"
  local wid; wid="$(window_id)"
  [[ -n "$wid" ]] || die "No Lockshot window found for pid $APP_PID"
  screencapture -l "$wid" -o "$SCREENSHOTS_DIR/$name.png" \
    || die "screencapture failed (is Screen Recording allowed for this app?)"
  success "Captured $name.png"
}

# --------------------------------------------------------------- window -----
for _ in $(seq 1 40); do [[ -n "$(window_id)" ]] && break; sleep 0.5; done
[[ -n "$(window_id)" ]] || die "Lockshot window did not appear"
sleep 1.5
app_sys_events "tell window 1 to set position to {$WINDOW_X, $WINDOW_Y}"
app_sys_events "tell window 1 to set size to {$WINDOW_WIDTH, $WINDOW_HEIGHT}"
sleep 1.5
bring_to_front

# The web content's height is the window's minus the title bar.
VIEW_HEIGHT=$(( WINDOW_HEIGHT - TITLE_BAR ))
CONTENT_Y=$(( WINDOW_Y + TITLE_BAR ))
success "Window ready (${WINDOW_WIDTH}x${WINDOW_HEIGHT} at $WINDOW_X,$WINDOW_Y, content ${VIEW_HEIGHT} tall)"

# The first click after launch can be swallowed while the window activates.
park_pointer
click 720 $(( VIEW_HEIGHT - 4 )) 0.5

# ----------------------------------------------------------- the editor -----
open_demo_image() {
  click_c 720 620 1.5                     # Open image… on Home
  key "g" "command down, shift down" 0.8  # Go to folder in the open panel
  type_text "$DEMO_IMAGE"
  sleep 0.6
  press_return 1
  press_return 2
}

# Mark the dashboard up. Points are in the demo image, 1200x660.
annotate() {
  local pace="${1:-0.6}"
  # A click on an existing annotation grabs it, so nothing starts on one.
  key "r"; img_drag 230 142 462 252 "$pace"                 # box the headline number
  key "n"; img_click 404 197 0.4                           # step 1 inside the box, clear of its edge
  key "a"; img_drag 662 302 562 316 "$pace"                 # arrow to the launch week bar
  key "2"; key "t"; img_click 592 270 0.4                  # a small label above the arrow
  type_text "Launch week"; press_return 0.3; press_escape 0.3; key "3"
  key "n"; img_click 548 304 0.4                           # step 2 on the bar
  key "h"; img_drag 398 125 534 125 "$pace"                 # highlight "updated 4 minutes ago"
  key "b"; img_drag 882 298 1066 524 "$pace"                # pixelate the email addresses
  press_escape 0.3
}

# ----------------------------------------------------------- screenshots ----
# 1. Home with the capture cards
park_pointer
capture 01-home

# 2. The editor with a marked-up capture
open_demo_image
[[ -n "$ONLY_SHOT" && "$ONLY_SHOT" == 01-home ]] || annotate 0.4
park_pointer
capture 02-editor

# 3. The format picker in the export bar
click 1277 58 0.8
park_pointer
capture 03-export
press_escape 0.5

# 4. Settings › Hotkeys
click 1424 19 1.5      # Settings
click 84 130 1         # Hotkeys
park_pointer
capture 04-hotkeys

# ----------------------------------------------------------------- GIF ------
if [[ -z "$ONLY_SHOT" ]]; then
  info "Recording the demo GIF"
  # Close Settings and the marked-up tab, back to Home.
  key "w" "command down" 0.8
  key "w" "command down" 0.8
  click 21 19 1
  open_demo_image
  park_pointer

  TEMP_VIDEO="$WORK_DIR/demo.mov"
  PALETTE="$WORK_DIR/palette.png"
  screencapture -v -R"$WINDOW_X,$WINDOW_Y,$WINDOW_WIDTH,$WINDOW_HEIGHT" "$TEMP_VIDEO" 2>/dev/null &
  RECORD_PID=$!
  sleep 1.5

  annotate 0.7
  park_pointer
  sleep 1
  click 1277 58 1.2      # format picker
  click 1104 147 1       # JPG
  park_pointer
  sleep 1.5

  kill -INT "$RECORD_PID" 2>/dev/null || true
  wait "$RECORD_PID" 2>/dev/null || true
  RECORD_PID=""
  sleep 1
  [[ -s "$TEMP_VIDEO" ]] || die "Recording failed (is Screen Recording allowed for this app?)"

  ffmpeg -loglevel error -y -i "$TEMP_VIDEO" -vf "fps=12,scale=960:-1:flags=lanczos,palettegen=stats_mode=diff" "$PALETTE"
  ffmpeg -loglevel error -y -i "$TEMP_VIDEO" -i "$PALETTE" \
    -lavfi "fps=12,scale=960:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5" "$DEMO_GIF_PATH"

  size_bytes=$(stat -f%z "$DEMO_GIF_PATH")
  success "Demo GIF created: docs/demo.gif ($((size_bytes / 1024)) KB)"
  (( size_bytes > 8388608 )) && warn "GIF is over 8 MB; shorten the flow or lower the fps"
fi

success "Done. Review docs/screenshots/*.png and docs/demo.gif, then commit them."
