#!/bin/sh
# Create a DMG without Finder/AppleScript customization (works with a locked desktop).
set -eu
APP_PATH='target/debug/bundle/macos/Personal Command Center.app'
STAGE_PATH=$(mktemp -d "${TMPDIR:-/tmp}/pcc-dmg.XXXXXX")
trap 'rm -rf "$STAGE_PATH"' EXIT HUP INT TERM
if [ ! -d "$APP_PATH" ]; then
  echo 'Build the debug macOS app first: npm run tauri -w apps/desktop -- build --debug --bundles app' >&2
  exit 1
fi
# This script packages DEBUG artifacts only. Ad-hoc signing repairs the bundle
# resource seal; it does not provide Developer ID trust or notarization.
codesign --force --deep --sign - "$APP_PATH"
codesign --verify --deep --strict "$APP_PATH"
VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$APP_PATH/Contents/Info.plist")
OUTPUT_PATH="target/debug/bundle/dmg/Personal Command Center_${VERSION}_$(uname -m).dmg"
mkdir -p "$STAGE_PATH" 'target/debug/bundle/dmg'
ditto "$APP_PATH" "$STAGE_PATH/Personal Command Center.app"
if [ ! -e "$STAGE_PATH/Applications" ]; then ln -s /Applications "$STAGE_PATH/Applications"; fi
hdiutil create -volname 'Personal Command Center' -srcfolder "$STAGE_PATH" -ov -format UDZO "$OUTPUT_PATH"
