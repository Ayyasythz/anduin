#!/bin/bash
# Builds Anduin.app and installs it. Usage: macos/build-app.sh [dest-dir]
set -euo pipefail
cd "$(dirname "$0")/.."

DEST="${1:-/Applications}"
APP="target/Anduin.app"
ICONSET="target/AppIcon.iconset"

cargo build --release
swift build -c release --package-path macos/app

rm -rf "$APP" "$ICONSET"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$ICONSET"
cp macos/Info.plist "$APP/Contents/"
cp macos/app/.build/release/Anduin "$APP/Contents/MacOS/Anduin"
cp target/release/anduin "$APP/Contents/MacOS/anduin-cli"
chmod +x "$APP/Contents/MacOS/"*

swift macos/icon.swift target/icon-1024.png
for s in 16 32 128 256 512; do
    sips -z $s $s target/icon-1024.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
    sips -z $((s * 2)) $((s * 2)) target/icon-1024.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

codesign --force --deep --sign - "$APP"

mkdir -p "$DEST"
rm -rf "$DEST/Anduin.app"
cp -R "$APP" "$DEST/"
# Refresh Launchpad/Spotlight's view of the bundle and its icon.
touch "$DEST/Anduin.app"
echo "installed $DEST/Anduin.app"
