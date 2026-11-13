#!/bin/sh
# Builds Reflux.app and its disk image from a release build (macOS only: sips, iconutil, hdiutil).
# Usage: packaging/macos/bundle.sh <version> <out.dmg>   (after cargo build --release -p reflux)
# The executable sits in Contents/MacOS with `fonts/` beside it (`theme::font_dir`).
set -eu
cd "$(git rev-parse --show-toplevel)"
version=$1
out=$2
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app=$work/dmg/Reflux.app
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/reflux "$app/Contents/MacOS/reflux"
cp -R assets/fonts "$app/Contents/MacOS/fonts"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"

set=$work/reflux.iconset
mkdir "$set"
for s in 16 32 64 128 256 512; do
  sips -z $s $s assets/media/images/app-icon.png --out "$set/icon_${s}x${s}.png" >/dev/null
done
for s in 16 32 128 256; do
  cp "$set/icon_$((s * 2))x$((s * 2)).png" "$set/icon_${s}x${s}@2x.png"
done
rm "$set/icon_64x64.png"
iconutil -c icns "$set" -o "$app/Contents/Resources/reflux.icns"

# shortcut: ad-hoc signed, not notarized: the first launch needs right-click > Open (or
# `xattr -dr com.apple.quarantine`); sign and notarize once there's a Developer ID.
codesign --force --deep --sign - "$app"
ln -s /Applications "$work/dmg/Applications"
hdiutil create -volname Reflux -srcfolder "$work/dmg" -ov -format UDZO "$out"
