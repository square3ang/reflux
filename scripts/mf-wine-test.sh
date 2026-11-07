#!/bin/sh
# Runs the Media Foundation player (crates/reflux/src/mf.rs) for real under wine: builds a tiny
# crate holding only that module for x86_64-pc-windows-gnu (zig as linker; the whole app can't
# link here because of webrtc-sys) and plays clips GStreamer encodes here: an H.264 (4:2:0 main
# profile: like Windows', wine's decoder has no 4:4:4) / AAC mp4
# and an mp3. Needs `wine`, `zig`, `gst-launch-1.0` (x264, avenc_aac, lame) and the rustup
# target x86_64-pc-windows-gnu. Usage: scripts/mf-wine-test.sh
# shortcut: wine's MF (winegstreamer) stands in for Windows' own decoders; run the same on a
# real Windows box when one is around.
set -eu
repo=$(cd "$(dirname "$0")/.." && pwd)
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
gst-launch-1.0 -q mp4mux name=m ! filesink location="$dir/av.mp4" \
  videotestsrc num-buffers=60 ! video/x-raw,width=320,height=240,framerate=30/1,format=I420 ! x264enc ! video/x-h264,profile=main ! h264parse ! m. \
  audiotestsrc num-buffers=94 samplesperbuffer=1024 ! audio/x-raw,rate=48000,channels=2 ! audioconvert ! avenc_aac ! aacparse ! m.
gst-launch-1.0 -q audiotestsrc num-buffers=50 ! audio/x-raw,rate=44100 ! audioconvert ! lamemp3enc ! filesink location="$dir/a.mp3"

mkdir -p "$dir/src" "$dir/lib"
cp "$repo/Cargo.lock" "$dir/"
cat > "$dir/Cargo.toml" <<EOF
[package]
name = "mf-wine"
version = "0.0.0"
edition = "2024"
[dependencies]
$(grep -E '^(rodio|windows) = ' "$repo/crates/reflux/Cargo.toml")
EOF
sed "s|MF_RS|$repo/crates/reflux/src/mf.rs|" "$repo/scripts/mf-wine-test.rs" > "$dir/src/main.rs"
# Rust's windows-gnu target asks for libgcc's pieces; zig's mingw has what they provide.
for l in gcc gcc_eh; do zig ar rc "$dir/lib/lib$l.a"; done
# zig links the UCRT and its own threads, so rustc's msvcrt/winpthreads asks are dropped.
cat > "$dir/win-cc" <<EOF
#!/bin/sh
for a; do shift; case "\$a" in -lmsvcrt|-l:libpthread.a) ;; *) set -- "\$@" "\$a" ;; esac; done
exec zig cc -target x86_64-windows-gnu -L$dir/lib "\$@"
EOF
# The `windows` crate's raw-dylib imports need a mingw dlltool; zig has one.
printf '#!/bin/sh\nexec zig dlltool "$@"\n' > "$dir/x86_64-w64-mingw32-dlltool"
chmod +x "$dir/win-cc" "$dir/x86_64-w64-mingw32-dlltool"
export PATH="$dir:$PATH" WINEDEBUG=${WINEDEBUG:--all}
export MF_CLIP_AV="$(winepath -w "$dir/av.mp4" 2>/dev/null)" MF_CLIP_MP3="$(winepath -w "$dir/a.mp3" 2>/dev/null)"
cd "$dir"
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$dir/win-cc" CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine \
  cargo test --quiet --target x86_64-pc-windows-gnu -- --test-threads=1 --nocapture
