# Reflux

A desktop client for [Fluxer](https://github.com/fluxerapp/fluxer), written in Rust on the
[Rune](https://github.com/anomalyco/rune) UI toolkit. It aims to look and behave like the
official Electron app, screen for screen, without a browser engine.

> Unofficial. Reflux is not made or endorsed by the Fluxer team. It talks to any Fluxer
> instance through the same API and gateway as the official client.

## Status

Daily-driver quality on Linux (KDE Plasma / Wayland is the main test bed); Windows and macOS
build and are checked, with less runtime testing.

- Chat: messages, replies, edits, reactions, pins, threads, forums, search, mentions, inbox,
  markdown, embeds, attachments, voice messages, emoji / stickers / GIFs.
- Communities: channels, roles, permissions, invites, settings, audit log, emoji and stickers.
- Voice and video: calls, camera, screen share with audio, stream viewer, compact and full
  call views, noise suppression, push-to-talk, global mute / deafen shortcuts.
- Desktop: tray, notifications, dock badge, autostart, `fluxer://` links, multiple accounts,
  single instance, offline cache, themes and Quick CSS variables, right-to-left languages.

What is done and what is still partial is tracked row by row in
[docs/FEATURES.md](docs/FEATURES.md). Where Reflux has to differ from the official app, and why,
is listed in [docs/WORKAROUNDS.md](docs/WORKAROUNDS.md).

Not supported: in-app YouTube playback (embeds open in the browser), billing and checkout
(opened in the browser), camera background effects.

## Building

Rust (stable, edition 2024) and a C/C++ toolchain.

Linux packages for the default features:

- GStreamer 1.x with its development headers (`gstreamer`, `gst-plugins-base`, plus
  `gst-plugins-good` / `gst-libav` for playback) for inline video and the camera.
- PipeWire / PulseAudio, D-Bus and a Secret Service (KWallet or GNOME Keyring) at runtime.

```sh
cargo build --release -p reflux
./target/release/reflux
```

The first build compiles libwebrtc through LiveKit and takes a while. For UI work without
voice, camera or screen share:

```sh
scripts/fast-build.sh            # --no-default-features --features gst
```

`--no-default-features` alone also drops GStreamer; attachments then open in the browser.

### Fonts

The Latin faces are built in. The CJK, Arabic, Hebrew, Devanagari and Thai faces of Fluxer Sans
are loaded from a `fonts` folder beside the binary (or `assets/fonts` in a checkout); without
them those scripts fall back to system fonts. Build them from Fluxer's font package:

```sh
python3 scripts/fonts-extract.py <fluxer>/packages/fonts/files
```

## Running

- Sign in with your instance's address, or import the official desktop app's session:
  `reflux --import-fluxer-session`.
- Linux desktop entry and `fluxer://` handler: copy `packaging/reflux.desktop` to
  `~/.local/share/applications/` and run
  `xdg-mime default reflux.desktop x-scheme-handler/fluxer`.
- Global shortcuts on Wayland go through the desktop's GlobalShortcuts portal; the desktop asks
  you to confirm the keys once. On KDE, changing a voice keybind in the app updates the system
  shortcut too.

Environment variables that help when debugging: `REFLUX_CONFIG_DIR` (separate profile),
`REFLUX_NO_KEYRING`, `REFLUX_DEBUG_SCROLL`, `REFLUX_DEBUG_WHEEL`.

## Development

```sh
cargo test -p reflux -p reflux-voice
cargo clippy -p reflux
scripts/cross-check.sh           # cargo check for Windows and macOS targets
```

Most screens have offline demo scenes, rendered to an image without a server:

```sh
reflux --shell-demo --voice=compact-crowd --snap out.png --size 1280x800
reflux --gallery                 # the component gallery
```

Crates:

| Crate | What |
|---|---|
| `reflux` | the app: UI, settings, desktop integration |
| `reflux-net` | REST and gateway client |
| `reflux-store` | client state built from gateway events |
| `reflux-model` | API types |
| `reflux-markdown` | Fluxer's markdown dialect |
| `reflux-voice` | LiveKit calls, camera, screen share (`stub.rs` without WebRTC) |

## License

[AGPL-3.0-or-later](LICENSE), like Fluxer. Bundled fonts are derived from IBM Plex
(SIL Open Font License, [assets/fonts/LICENSE-IBM-PLEX.txt](assets/fonts/LICENSE-IBM-PLEX.txt));
other third-party assets carry their own license files next to them.
