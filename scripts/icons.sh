#!/bin/sh
# Regenerate every icon from app-icon.png (the illustrated logo) and the tray
# SVGs. Falls back to the placeholder mark while app-icon.png is missing.
# The macOS icon (Dock and Command-Tab) comes from app-icon-macos.png, the
# squircle tile; scripts/icons/fit-squircle.py made it from the artwork.
set -e
cd "$(dirname "$0")/.."
SRC=app-icon.png
[ -f "$SRC" ] || SRC=scripts/icons/placeholder-app-icon.svg
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

bunx tauri icon "$SRC" -o src-tauri/icons
rm -rf src-tauri/icons/android src-tauri/icons/ios
if [ -f app-icon.png ]; then cp app-icon.png public/app-icon.png; else cp src-tauri/icons/icon.png public/app-icon.png; fi
if [ -f app-icon-macos.png ]; then
  bunx tauri icon app-icon-macos.png -o "$TMP/macos" >/dev/null
  cp "$TMP/macos/icon.icns" src-tauri/icons/icon.icns
fi

bunx tauri icon scripts/icons/tray.svg -o "$TMP/tray" >/dev/null
cp "$TMP/tray/64x64.png" src-tauri/icons/tray-template@2x.png
bunx tauri icon scripts/icons/tray-color.svg -o "$TMP/color" >/dev/null
cp "$TMP/color/64x64.png" src-tauri/icons/tray-color.png
echo "Icons written to src-tauri/icons from $SRC"
