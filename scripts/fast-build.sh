#!/bin/sh
# Build the app for UI work without libwebrtc (the slowest dependency): the `voice` feature is
# off, so reflux-voice builds its stub and calls / screen share / camera are unavailable.
# Usage: scripts/fast-build.sh [extra cargo args]
# shortcut: a second, hand-maintained no-WebRTC implementation of reflux-voice's API; delete it
# when libwebrtc builds fast enough that the split no longer pays for itself.
set -eu
cd "$(git rev-parse --show-toplevel)"
start=$(date +%s)
cargo build -p reflux --no-default-features --features gst "$@"
echo "fast build: $(($(date +%s) - start))s (no libwebrtc; voice calls, screen share and camera are disabled)"
