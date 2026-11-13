#!/bin/sh
# Builds Reflux-<version>-<arch>.AppImage from a release build, with GStreamer and its plugins
# bundled (linuxdeploy + linuxdeploy-plugin-gstreamer). `fonts/` sits beside the executable
# (`theme::font_dir`). Usage: packaging/linux/appimage.sh <version>
set -eu
cd "$(git rev-parse --show-toplevel)"
version=$1
arch=$(uname -m)
tools=$(mktemp -d)
app=$(mktemp -d)/AppDir
trap 'rm -rf "$tools"' EXIT
curl -fsSL -o "$tools/linuxdeploy" "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$arch.AppImage"
curl -fsSL -o "$tools/linuxdeploy-plugin-gstreamer.sh" https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gstreamer/master/linuxdeploy-plugin-gstreamer.sh
chmod +x "$tools"/*
mkdir -p "$app/usr/bin"
cp -R assets/fonts "$app/usr/bin/fonts"
cp assets/media/images/app-icon.png "$tools/reflux.png"
export PATH="$tools:$PATH" APPIMAGE_EXTRACT_AND_RUN=1 LDAI_OUTPUT="$PWD/Reflux-$version-$arch.AppImage"
export GSTREAMER_PLUGINS_DIR="${GSTREAMER_PLUGINS_DIR:-$(pkg-config --variable=pluginsdir gstreamer-1.0)}"
export GSTREAMER_HELPERS_DIR="${GSTREAMER_HELPERS_DIR:-$(pkg-config --variable=pluginscannerdir gstreamer-1.0)}"
linuxdeploy --appdir "$app" --executable target/release/reflux --desktop-file packaging/reflux.desktop --icon-file "$tools/reflux.png" --plugin gstreamer --output appimage
