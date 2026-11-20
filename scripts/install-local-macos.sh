#!/usr/bin/env bash
# Build Lockshot, sign it with your Developer ID, and install it to
# /Applications, so you can test what needs a real app bundle (Screen
# Recording permission, the menu bar icon, launch behavior) without a release.
#
# macOS remembers Screen Recording by signer and bundle ID, so with a stable
# Developer ID signature the permission survives every rebuild. Notarization
# is only needed for downloaded apps, so it is skipped here.
#
# Usage: bun run install:mac            (release build, like users get)
#        bun run install:mac -- --debug (faster, debug build)
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "$(uname)" != "Darwin" ]]; then
  echo "This script is for macOS." >&2
  exit 1
fi

identity="${APPLE_SIGNING_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)}"
if [[ -z "$identity" ]]; then
  echo "No Developer ID Application certificate in your keychain. See docs/macos-releases.md." >&2
  exit 1
fi

key="$HOME/.tauri/lockshot.key"
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && ! -f "$key" ]]; then
  echo "Updater key not found at $key. Set TAURI_SIGNING_PRIVATE_KEY or restore the key." >&2
  exit 1
fi

export APPLE_SIGNING_IDENTITY="$identity"
export TAURI_SIGNING_PRIVATE_KEY="${TAURI_SIGNING_PRIVATE_KEY:-$(cat "$key")}"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
# Without these Tauri signs but does not notarize.
unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH

profile=release
for arg in "$@"; do
  [[ "$arg" == "--debug" ]] && profile=debug
done

echo "Signing with: $identity"
bun run tauri build --bundles app "$@"

app="src-tauri/target/$profile/bundle/macos/Lockshot.app"
codesign --verify --deep --strict "$app"

# Quit the running copy so the new one replaces it cleanly.
osascript -e 'tell application "Lockshot" to quit' >/dev/null 2>&1 || true
pkill -x lockshot >/dev/null 2>&1 || true
sleep 1

rm -rf /Applications/Lockshot.app
ditto "$app" /Applications/Lockshot.app
echo "Installed /Applications/Lockshot.app"
open /Applications/Lockshot.app
