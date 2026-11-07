#!/bin/sh
# `cargo check -p reflux` for Windows and macOS from Linux, with zig as the C compiler
# (mingw headers / no SDK). Check only: webrtc-sys's MSVC-flavoured C++ and Objective-C sources
# get empty objects, and a fake `xcrun` satisfies webrtc-sys's macOS build script, so nothing
# here links. Usage: scripts/cross-check.sh [windows|macos] (default: both). Needs `zig` and the
# rustup targets x86_64-pc-windows-gnu / aarch64-apple-darwin.
set -eu
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
for pair in win:x86_64-windows-gnu mac:aarch64-macos; do
  n=${pair%%:*}; z=${pair#*:}
  for k in cc c++; do
    cat > "$dir/$k-$n" <<EOF
#!/usr/bin/env python3
import os, sys
a = [x for x in sys.argv[1:] if not x.startswith('--target=')]
if any(x in ('/EHsc', '/std:c++20') or 'webrtc-sys' in x or 'livekit_webrtc' in x or x.endswith(('.m', '.mm')) for x in a):
    if '-o' in a:
        open(a[a.index('-o') + 1], 'wb').close()
    sys.exit(0)
os.execvp('zig', ['zig', '$k', '-target', '$z'] + a)
EOF
  done
done
printf '#!/bin/sh\nexec zig ar "$@"\n' > "$dir/ar"
printf '#!/bin/sh\necho /nonexistent-sdk\n' > "$dir/xcrun"
chmod +x "$dir"/*
run() {
  t=$1; n=$2; v=$(echo "$t" | tr - _)
  echo "== $t"
  env PATH="$dir:$PATH" "CC_$v=$dir/cc-$n" "CXX_$v=$dir/c++-$n" "AR_$v=$dir/ar" \
    cargo check -p reflux --target "$t" --no-default-features --message-format=short
}
case "${1:-both}" in
  windows) run x86_64-pc-windows-gnu win ;;
  macos) run aarch64-apple-darwin mac ;;
  *) run x86_64-pc-windows-gnu win; run aarch64-apple-darwin mac ;;
esac
