# Workarounds

Where reflux does something differently from Fluxer, or in a roundabout way, because of a limit
in Rune, a platform, the server or the build. Each entry: what we do, why, where, and what would
let us remove it. Feature gaps live in [FEATURES.md](FEATURES.md); this file is about *how* we got
around something.

Rules:
- Every workaround in code gets a `// shortcut:` comment at the spot. The appendix at the bottom is
  generated from those comments: `scripts/workarounds.sh` rewrites it.
- A structural workaround (one that shapes a module, the build, or behaviour across files) also
  gets a hand-written entry in the sections below.
- When a workaround is removed, delete its comment and its entry.

## Rune (pinned rev, not forked or patched)

| Workaround | Why | Where | Remove when |
|---|---|---|---|
| System titlebar instead of Fluxer's custom one | Rune has no frameless window / custom titlebar on Linux and Windows | `main.rs` window setup | Rune gets frameless windows with hit-testing |
| Inline custom emoji: a no-break-space gap in the text plus an image overlay placed with `rune::memo::layout(key)`; copying maps back through `El::selection_copy` | Rune text runs can't hold inline images | `shell/markdown.rs` | Rune supports inline boxes in text |
| Custom emoji pills in the composer show `:name:` | pill chips take only a vector `Icon` and no pill rect is exposed | `compose.rs` | Rune exposes pill rects or image pills |
| Format toolbar centres on the caret end | Rune exposes the caret's rect, not the selection's | `compose.rs` | a selection rect API |
| Compact layout: text wraps under its own start, not 32px under the name | no text-indent; placeholder spaces would leak into copies | `shell/compact.rs` | Rune gets text-indent |
| Pop-out windows are a fixed pool of 4, the oldest gives way | Rune can add windows only before the app runs | `popout.rs` | runtime window creation |
| Settings-search highlight covers the shared helpers and the tabs' `text()`, not `rich()` runs (texts with an inline link) | Rune keeps the element tree private | `ui/mark.rs` | a tree walk / text query API |
| Search tokens are coloured spans: no 1px border, both halves round all corners, padding doesn't widen the text | a Rune span draws a padded rounded fill only | `shell/search.rs` | span boxes with borders and per-corner radius |
| Quick CSS applies only `:root` colour/font variables | Rune has no CSS engine | `settings/look_tabs.rs`, `quick_css.rs` | never (by design) |
| Spellcheck uses our own dictionary; the caret stands in for the pointer position | no OS text services; no pointer→text-offset API | `desktop/spell.rs` | Rune exposes hit-testing into text |
| Undo/Redo always enabled | Rune doesn't expose the edit history depth | `compose.rs` | history API |
| Inline video/audio and gifv on Windows/Linux don't use Rune's `Player`: our `VideoPlayer` (GStreamer `playbin` on Linux, Media Foundation `IMFSourceReader` on Windows) draws RGBA frames into a `Pixels` with our own control bar | `Player` has only a macOS (AVFoundation) backend | `gst.rs`, `mf.rs`, `shell/{clip,gifv}.rs` | `Player` gains Windows/Linux backends |
| RTL: bidi text runs only, layout isn't mirrored | Rune has no layout direction | `i18n.rs` | layout direction in Rune |
| No minimize-to-tray, window size only (no position/maximized) — **WONTFIX** (user decision) | no minimize event; position impossible on Wayland | `desktop/window.rs` | — |
| Keyboard-mode intro shows only for a Tab no field takes | the app-level `on_key` only sees keys the focused control passes on, and a field's Tab moves focus itself | `nav/keys.rs` | a pre-dispatch key hook |
| The thread composer's long placeholder is cut near the field width | Rune wraps a placeholder even in a single-line field | `shell/threads.rs` | single-line placeholders clip |
| Spinner dots only fade (no 0.8 scale) | Rune's looping motion is opacity, rotation or slide; the scale-capable `Sequence` runs once | `ui/misc.rs` | looping scale keyframes |
| Scrollbars keep rune-kit's 2pt edge gap and 24pt minimum thumb (Fluxer's regular bar: 4 in, 40 minimum) | `ScrollBarStyle` has no gap / minimum knobs | `ui/misc.rs` | rune-kit exposes them |
| Scaled markdown spans (headings) keep even half-leading, so their baseline differs a little from Fluxer's | Rune lays out a scaled span with its own line height | `shell/markdown.rs` | per-span baseline control |
| Jumbo emoji can't be selected or copied in place ("Copy text" has them); one-line previews keep emoji as text | the emoji are image overlays, not text; truncated one-line text has no overlay slots | `shell/markdown.rs` | inline images in text runs |
| Recovery kit "Print" opens the PDF in the system viewer | Rune can't print | `settings/recovery_kit.rs` | a print API |
| Pinned-messages popout resizes from the left and bottom edges only | Rune's `Cursor` has no diagonal resize; the anchored edges would need re-placement | `shell/pins.rs` | diagonal cursors |

## Platform

| Workaround | Why | Where | Remove when |
|---|---|---|---|
| Global shortcuts: Wayland portal; X11 RECORD hook, Windows/macOS listen-only keyboard hooks; in-window press of a global combo is skipped | no single cross-platform API; the portal only exists on Wayland desktops | `desktop/hooks.rs`, `desktop/shortcuts*` | — |
| Global shortcuts are keyboard-only | the hooks don't capture mouse buttons, and the push-to-talk recorder can't record them either | `desktop/hooks.rs` | mouse hooks plus a recorder that takes buttons |
| Windows single instance: a per-user named pipe forwards deep links to the running app | Rune has no single-instance API | `desktop/` deep links | — |
| macOS deep links only as launch arguments | an open-URL event isn't exposed by Rune and needs the bundle's URL scheme | `desktop/` deep links | Rune forwards `application:openURLs:` |
| Flatpak autostart through the XDG Background portal, result kept in a state file | no `~/.config/autostart` access in the sandbox | `desktop/autostart*` | — |
| Multi-file picking through the XDG portal (`multiple: true`), else Rune's one-file dialog; Windows/macOS one file per dialog | Rune's dialog picks one file | `file_picker.rs` | Rune multi-select dialog |
| Wayland: no in-app screen-share thumbnails; the Applications / Entire screen tabs steer the portal dialog | the portal can't enumerate sources | `shell/call.rs`, `reflux-voice` `ScreenSource::portal` | — |
| The picker's source list refreshes every 2 s (Fluxer: 1 s), re-capturing every thumbnail | libwebrtc's capturer has no incremental thumbnail update | `app/voice.rs` `tick`, `Op::LoadScreenSources` | the SDK exposes a thumbnail stream |
| Screen-share Devices tab lists capture devices by name, with no live preview | a preview needs camera permission and a second capture per device | `shell/call.rs` | device previews through the camera capture |
| Wayland ignores always-on-top for PiP pop-outs | winit can't request it | `popout.rs` | compositor protocol support |
| Theme folders are rescanned every 2 s | same as Fluxer's polling fallback; no cross-platform file watcher used | `settings/theme_folder.rs` | add a notify-based watcher |
| TTS through the platform's CLI voice (`spd-say`, `say`, PowerShell) | no speech API in Rune | `desktop/tts.rs` | — |
| Passkeys: Windows via webauthn.dll; Linux/macOS USB keys only | no Linux platform authenticator; macOS iCloud passkeys need an entitlement only fluxer.app has | `passkey.rs` | — |
| App-audio capture on Linux: `pw-dump` to find the node + `pipewiresrc`; appsink with `sync=false` (with sync it dropped ~70% of the audio) | no PipeWire API in libwebrtc | `gst.rs`, `reflux-voice` | — |
| Recovery kit PDF: text outside Windows-1252 (CJK, Cyrillic…) is written in English | the PDF uses the built-in Helvetica; Fluxer rasterises the page | `settings/recovery_pdf.rs` | embed a font |
| Animated WebP avatars/banners crop to their first frame | the `image` crate has no animated WebP encoder | `settings/crop.rs` | an encoder |
| Camera and inline video on Linux go through GStreamer (`gst` feature, default) | libwebrtc's capture isn't exposed by the Rust SDK; Rune has no Linux player | `gst.rs` | — |
| Camera capture is ours per OS behind one `camera::{available, cameras, CameraCapture}` interface: GStreamer on Linux, Media Foundation `IMFSourceReader` over the device source (native mode picked by `camera::pick::mode`, RGB32 through the reader's video processor) on Windows, `AVCaptureSession` + `AVCaptureVideoDataOutput` (BGRA, session preset by height) on macOS; all pace frames to the fps setting with `camera::pick::Pacer` instead of asking the device for the rate | libwebrtc's own capture modules aren't exposed by the LiveKit Rust SDK; asking GStreamer/PipeWire for a rate the camera lacks fails to negotiate (15 fps on a 10/20/30/60 fps device) | `camera/`, `gst.rs` | the SDK exposes libwebrtc's `VideoCaptureModule` |
| Windows playback pump: one decode thread per clip reads the source reader synchronously and paces samples against a wall clock (pause/seek/rate move the clock); sound goes to rodio (`Player` per clip, rebuilt on seek, ~0.5 s queued ahead, rate = rodio speed so the pitch shifts), frames RGB32 → RGBA | Media Foundation's own renderers (EVR/SAR, Media Engine) draw into a window, not into Rune's `Pixels`; rodio is already the app's sound output | `mf.rs` | Rune `Player` on Windows |
| Media Foundation readers open/close one at a time and an `E_UNEXPECTED` open is retried | under wine concurrent opens deadlocked and an open right after a close failed; not seen on Windows (untested there) | `mf.rs` | a real Windows run shows it's not needed |

## Voice / LiveKit Rust SDK

| Workaround | Why | Where | Remove when |
|---|---|---|---|
| Mic and remote audio go through our own mixer and mic chain (WebRTC APM echo cancel + AGC, RNNoise), not LiveKit's playout | device selection, volumes, noise suppression and voice-activity need it | `reflux-voice/src/audio.rs` | — |
| Linux device lists and call audio use libpulse (`libpulse-binding` + `libpulse-simple-binding`); cpal stays the fallback when no PulseAudio server answers | cpal's only Linux host is ALSA, whose "devices" are raw plugin/PCM names (default, pipewire, every HDA profile), so PipeWire's real sinks/sources never showed up in the pickers | `reflux-voice/src/audio.rs`, `audio/pulse.rs` | cpal gets a PipeWire/PulseAudio host, or we talk to PipeWire directly |
| Call playback asks the Pulse server for an 80 ms queue with a 40 ms prefill (10 ms request size), not the ALSA plugin's small default buffer | the old cpal stream underran when a mixer lock or scheduler hiccup delayed a callback; `pw-top` now shows ERR 0 for the stream | `reflux-voice/src/audio/pulse.rs` | cpal/ALSA exposes a reliable larger buffer, or the mixer runs lock-free |
| Device selection is persisted by the Pulse description label (duplicates get `" (name)"`), not by an immutable device id | the shared picker/session stores one plain string per side (`Saved::voice_input`/`voice_output`) | `reflux-voice/src/audio/pulse.rs`, `session.rs` | the device lists carry `(id, label)` pairs |
| H.264 profile `42e01f` for NVENC | the hardware factory rejects other profiles | `reflux-voice` | — |
| Screen-share "content hint" maps to libwebrtc's degradation preference | the Rust SDK has no content-hint setter | `reflux-voice`, `settings/advanced.rs` | SDK exposes `contentHint` |
| Choosing an SVC mode publishes VP9 with the software encoder; Automatic uses H.264 + NVENC/VA-API | libwebrtc only offers scalability for AV1/VP9, and the hardware factories here encode H.264 only | `reflux-voice` `ScreenQuality::publish_options`, `settings/advanced.rs` | hardware AV1/VP9 encoders are exposed |
| A live quality change and "Change stream" stop the share, then publish a new track from the saved source | the Rust SDK doesn't expose a published sender's encoding parameters, and native capture has no in-place source switch | `app/voice.rs` `toggle_screen`, `Op::Settings`/`Op::ChangeScreen` | the SDK exposes `RtpSender` parameters and source switching |
| "No hardware encoder was found" never shows on the encoder setting | webrtc-sys doesn't expose its encoder factory probe | `settings/advanced.rs` | the SDK exposes the probe |
| P2P mesh is our own implementation of `VoiceMeshEngine` (op 13, STUN only) | no JS engine to reuse | `reflux-voice/src/mesh.rs`, `app/voice.rs` | — |

## Server / instance

| Workaround | Why | Where | Remove when |
|---|---|---|---|
| REST base from `api_transport`, then `api_public`… instead of `web.fluxer.app/api` | the web origin returns 403 `INVALID_API_ORIGIN` to other clients | `reflux-net/src/rest.rs` | — |
| Media proxy bodies are capped (images 64 MiB, text 4 MiB, downloads 512 MiB) | remote input; don't buffer unbounded responses | `media.rs` `get_capped` | — |
| YouTube embeds open the browser | the embed carries only an iframe URL, no stream | `shell/embeds.rs` | (optional) a stream resolver like `yt-dlp` |
| P2P rollout polls every `poll_interval_seconds` without ETag / 304 and doesn't slow down ×4 while hidden | Rune reports no window visibility; the REST client has no conditional requests | `app/voice.rs` | visibility events + `If-None-Match` |

| No price-announcement / legacy-price nagbars | they need the account's Stripe billing state, which the client API doesn't return | `desktop/notices.rs` | the API returns it |
| No in-app Plutonium page, checkout, plan changes, gift redeem form or composer "Send gift" | purchases run through Stripe checkout in the web app; Reflux opens it (and the customer portal) in the browser | `settings/premium.rs`, `shell/upload.rs` | — |
| The updater is off unless `REFLUX_UPDATE_FEED` names a feed | Reflux publishes no releases yet | `desktop/updater.rs` | releases with a feed |

## Not ported yet (no external limit)

| What | Why it's still open | Where | Remove when |
|---|---|---|---|
| Quick switcher: other accounts show a letter and no unread/channel rows | Reflux keeps one account connected | `shell/switcher_extra.rs` | per-account connections |
| Quick CSS editor: syntax colours and variable completions (typed or Ctrl+Space) only (no selector/property/snippet completions, hover, lint) | those target the DOM; only `:root` variables apply here | `quick_css_syntax.rs` | — |
| UI strings: literals go through `t`; text only Reflux has stays English | it has no Fluxer msgid to translate from | `i18n.rs` | — |

## Developer tools menu (`shell/devtools.rs`)

The staff/developer-mode header menu (`DeveloperToolsContextMenu`) is ported. Overrides are local and saved in `session.json`. The items that write to the server are the same ones Fluxer's do: backend premium override, premium perks disabled, forget authorized IPs, reset premium state, clear trusted domains, the self-mention setting, the Fluxerbot DM and joining the AFK channel. These items are left out or simplified (Fluxer has no phone or membership-screening gate: `getEffectiveGuildVerificationLevel` caps the level at HIGH):

| Item | Why | Remove when |
|---|---|---|
| Force vanity URL disclaimer | the Vanity tab (`social/tabs.rs`) has no disclaimer to force | the disclaimer is ported |
| Disable translation DOM guard | it patches the DOM against browser translators; there is no DOM | never |
| Clear attachment mocks | no attachment mock items exist to clear | — |
| Titlebar platform override | Reflux uses the system titlebar (see Rune) | a custom titlebar |
| Nagbars: desktop download, corrupted installation, domain moved | web-only bars | never |
| Loading skeletons from fixed sizes | the app skeleton doesn't remember the last layout (`SkeletonLayoutMemory`) | layout memory is ported |
| Desktop notification nagbar: forced only | Fluxer never shows it in its desktop app (`isDesktop()`); Enable turns on the `notifications` pref | — |
| Staff tools button in the voice call header | not ported; the header menu covers the same items | — |
| Nested submenus open to the left | Rune doesn't report which way the parent flipped; only this menu nests, and it sits at the right edge | Rune exposes flip state |

## Session / tokens

| Workaround | Why | Where | Remove when |
|---|---|---|---|
| Tokens live in the OS keyring (service `reflux`, user `<config dir>#session` for the active one, `#account:<instance>:<user>` for others), else in the 0600 file | like Fluxer's `DesktopAppStorage` | `session.rs` | — |
| Importing from the Fluxer desktop app guesses its userData as `<config dir>/fluxer`; logout of an imported session forgets the token locally only | the imported session belongs to the Fluxer app | `session.rs` | — |

## Build / tooling

| Workaround | Why | Where | Remove when |
|---|---|---|---|
| Shared `CARGO_TARGET_DIR` across worktrees; stale cross-worktree errors fixed by `touch`ing the crate's `lib.rs`/`main.rs` | disk is tight; libwebrtc builds are huge | HANDOFF.md | more disk |
| `scripts/fast-build.sh` (`--no-default-features --features gst`) builds a stub `reflux-voice` instead of LiveKit/libwebrtc; voice calls, screen share and camera are unavailable there | libwebrtc is by far the slowest dependency and UI work doesn't need it | `crates/reflux-voice/src/{lib,stub}.rs`, `crates/reflux/Cargo.toml`, `scripts/fast-build.sh` | libwebrtc builds fast enough to stop splitting |
| NVENC needs the gitignored `.cuda` symlink before the first build | webrtc-sys looks for CUDA headers there | HANDOFF.md | — |
| `scripts/cross-check.sh`: Windows/macOS `cargo check` with zig; webrtc-sys's MSVC C++ and the Objective-C sources get empty objects, a fake `xcrun` satisfies the macOS build script — nothing links | no mingw/MSVC toolchain or Mac here | `scripts/cross-check.sh` | real Windows/macOS CI |
| `scripts/mf-wine-test.sh`: builds only `mf.rs` and `camera/{pick,mf}.rs` (via `#[path]`) for windows-gnu with zig as linker (empty libgcc stubs, msvcrt/winpthreads args dropped, `zig dlltool` for raw-dylib) and runs its tests under wine on GStreamer-encoded clips; the camera test only sees an empty device list (wine has no MF video capture source) | the whole app can't link here (webrtc-sys); wine's MF (winegstreamer) stands in for Windows' decoders | `scripts/mf-wine-test.{sh,rs}` | real Windows CI |
| `winapi` with the `std` feature on Windows | `authenticator` 0.5's Windows HID transport doesn't compile otherwise | `crates/reflux/Cargo.toml` | authenticator fix |
| The live probe reads the token from the keyring off the async runtime (`spawn_blocking`) | the Secret Service client blocks and panics inside tokio | `reflux-net/examples/live_probe.rs` | — |

<!-- BEGIN GENERATED: scripts/workarounds.sh -->

## Appendix: every `shortcut:` comment in the code

- `crates/reflux-voice/src/audio/pulse.rs:59` the picker persists this label, not the immutable pulse name; a renamed sink reads as unplugged and falls back to the server default. Upgrade when the device lists carry (id, label) pairs.
- `crates/reflux-voice/src/lib.rs:16` a second, hand-maintained implementation of the same API (`stub.rs`); delete it when
- `crates/reflux/src/app/voice.rs:1054` no ETag / 304 and no ×4 slower polling while the window is hidden.
- `crates/reflux/src/app/voice.rs:1514` 2 s, not Fluxer's 1 s — each refresh re-captures every source's thumbnail.
- `crates/reflux/src/compose.rs:19` a custom emoji pill shows `:name:`, not the emoji image.
- `crates/reflux/src/compose.rs:146` Rune exposes the caret's rect, not the selection's, so the bar centres over the
- `crates/reflux/src/compose.rs:183` Undo and Redo are always enabled (Rune doesn't expose the history depth).
- `crates/reflux/src/desktop/hooks.rs:4` keyboard only; Fluxer's hooks also see mouse buttons, which our push-to-talk
- `crates/reflux/src/desktop/notices.rs:7` no price-announcement / legacy-price nagbars (they need Stripe's billing state).
- `crates/reflux/src/desktop/spell.rs:8` no OS spellchecker engine.
- `crates/reflux/src/desktop/spell.rs:339` Rune gives no text offset for a pointer position, so the caret stands in for it.
- `crates/reflux/src/desktop/tts.rs:6` speech goes through the platform's command-line voice (speech-dispatcher's `spd-say`,
- `crates/reflux/src/desktop/updater.rs:12` Reflux has no release feed yet, so without `REFLUX_UPDATE_FEED` the updater stays
- `crates/reflux/src/desktop/window.rs:4` only the size is remembered. Rune reports no window position or maximized state
- `crates/reflux/src/desktop/window.rs:6` no minimize-to-tray; Rune sends no minimize event to hide on.
- `crates/reflux/src/file_picker.rs:4` macOS and Windows (and desktops without the portal) pick one file per dialog; the
- `crates/reflux/src/i18n.rs:5` UI literals go through `t` where they're drawn (option tables via `RadioGroup`, `Select`,
- `crates/reflux/src/i18n.rs:11` Arabic and Hebrew get right-to-left text runs (parley's bidi) but the layout isn't
- `crates/reflux/src/mf.rs:100` rodio's speed shifts the pitch (no `preservesPitch` like playbin's scaletempo);
- `crates/reflux/src/mf.rs:157` readers open and close one at a time, and an E_UNEXPECTED open is retried: under
- `crates/reflux/src/nav/keys.rs:81` a Tab inside a field moves focus without showing the intro.
- `crates/reflux/src/passkey.rs:14` no hybrid (phone) flow off Windows.
- `crates/reflux/src/popout.rs:6` on Wayland the compositor ignores always-on-top and placement (winit can't ask for
- `crates/reflux/src/popout.rs:36` at most this many pop-outs at once (Fluxer has no limit); the oldest gives way.
- `crates/reflux/src/quick_css_syntax.rs:4` only the variable completions; selector, `data-flx`, property and snippet completions,
- `crates/reflux/src/session.rs:216` userData guessed as `<config dir>/fluxer` (Linux ~/.config, Windows %APPDATA%, macOS
- `crates/reflux/src/settings/advanced.rs:348` Fluxer's "No hardware encoder was found" description needs webrtc-sys's factory
- `crates/reflux/src/settings/crop.rs:6` animated WebP crops to its first frame (no WebP animation encoder in `image`).
- `crates/reflux/src/settings/look_tabs.rs:708` sizes, spacing and other non-colour, non-font variables can't be overridden
- `crates/reflux/src/settings/premium.rs:4` no in-app Plutonium page, checkout, plan changes or gift code redeem form (the web
- `crates/reflux/src/settings/recovery_kit.rs:6` Print opens the PDF in the system viewer (Rune can't print).
- `crates/reflux/src/settings/recovery_pdf.rs:4` text outside Windows-1252 (CJK, Cyrillic…) is written in English instead of being
- `crates/reflux/src/shell/call.rs:934` device cards are names only (Fluxer shows live previews, which needs
- `crates/reflux/src/shell/compact.rs:5` the text wraps under its own start, not 32px under the name
- `crates/reflux/src/shell/devtools.rs:634` a failure is a toast, not Fluxer's "Failed to update premium perk state." modal; add the modal if staff want it.
- `crates/reflux/src/shell/devtools.rs:942` no pending label ("Forgetting authorized IPs...") and failures are toasts, not error modals; track the request if staff need them.
- `crates/reflux/src/shell/embeds.rs:191` play opens the video in the browser; Fluxer swaps in the YouTube iframe.
- `crates/reflux/src/shell/markdown.rs:91` a scaled span keeps its half-leading even, so the line's baseline lands where that
- `crates/reflux/src/shell/markdown.rs:364` jumbo emoji can't be selected or copied (no text); "Copy text" has them.
- `crates/reflux/src/shell/markdown.rs:395` emoji in one-line previews stay text (Fluxer draws 1.25em images).
- `crates/reflux/src/shell/pins.rs:5` only the left and bottom edges resize (the ones facing away from the anchor; Rune
- `crates/reflux/src/shell/search.rs:73` Rune spans draw a padded rounded fill only, so the chip has no 1px border, the key and
- `crates/reflux/src/shell/skeleton.rs:203` fixed rail / sidebar / member shapes; no `SkeletonLayoutMemory` of the last layout. Port it if the jump on READY matters.
- `crates/reflux/src/shell/switcher_extra.rs:102` Reflux keeps no other account connected, so there are no unread/mention sections or
- `crates/reflux/src/shell/threads.rs:687` Rune wraps a long placeholder even in a single-line field; cut it near the
- `crates/reflux/src/shell/upload.rs:241` no "Send gift" (needs the instance's premium pricing state; gifts are bought in the
- `crates/reflux/src/ui/mark.rs:4` Rune keeps the element tree private, so text is marked where it's built: the shared
- `crates/reflux/src/ui/menu.rs:195` rune can't tell us which way the parent flipped, so a nested submenu opens to the
- `crates/reflux/src/ui/misc.rs:67` the keyframes also scale each dot to .8; Rune's loop here is opacity only.
- `crates/reflux/src/ui/misc.rs:76` rune-kit's bar keeps its own 2pt edge gap and 24pt minimum thumb; Fluxer's regular
- `scripts/fast-build.sh:5` a second, hand-maintained no-WebRTC implementation of reflux-voice's API; delete it
- `scripts/mf-wine-test.sh:8` wine's MF (winegstreamer) stands in for Windows' own decoders; run the same on a

<!-- END GENERATED -->
