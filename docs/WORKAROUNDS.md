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
| Dragging a voice participant onto a channel is the row's own `on_drag`, hit-tested by hand | Rune's drops carry only files. While the drag runs, the sidebar records each voice channel row's `window_layout`, and the dragged row's origin plus `Drag.x/y` gives the pointer to test against them | `shell/voice_list.rs` (`draggable`, `drop_target`), `shell/sidebar.rs` (`channel_row`) | Rune gains internal drop targets |
| System titlebar instead of Fluxer's custom one | Rune has no frameless window / custom titlebar on Linux and Windows | `main.rs` window setup | Rune gets frameless windows with hit-testing |
| Advanced hides eight direct controls instead of faking them: video seek thumbnails, smooth scrolling, middle-click autoscroll, stay interactive unfocused, pre-upload attachments, connection volume controls, native title bar and source maps | Rune's wheel/pointer input has no browser smooth-scroll, autoscroll or focus guard; the inline player has no frame-at-time snapshot API; uploads only start at send (`Intent::SendMessage`); the per-user volume menu lives in `shell/call.rs` (platform-audio work); Rune always uses the system titlebar; a Rust build has no JS source maps | `settings/advanced.rs` (`UNAVAILABLE`), `shell/chat.rs`, `shell/attachments.rs` | the Rune/platform pieces above exist |
| Neko draws only its idle and four look frames, without oneko's walk/scratch physics | the shell has no per-frame animation clock for a purely decorative overlay; a pointer-driven sprite is enough for the easter egg | `shell/neko.rs`, `media.rs` | a shell animation tick |
| Advanced's "Enable text selection" gates message bodies; IDs, keys and logs stay selectable | Rune sets selection per text, and those debug values are meant to be copied even with message selection off | `shell/markdown.rs`, `shell/chat.rs` | Rune gets a window-wide selection switch |
| Sequential file send is shown on and disabled: one message carries every attachment | Reflux has no per-file message sequence to reorder; the pick order is already the send order | `settings/advanced.rs` | file messages are sent one by one |
| "Pause my screen share preview in the background" and "Hide my stream preview thumbnail" are kept for parity; only the PiP switch is enforced | the preview thumbnail is drawn in `shell/call.rs`, which the platform-audio work owns; the `look` flags are ready for that view | `settings/advanced.rs`, `shell/call.rs` | the platform-audio work reads the two flags |
| Inline custom emoji: a no-break-space gap in the text plus an image overlay placed with `rune::memo::layout(key)`; copying maps back through `El::selection_copy` | Rune text runs can't hold inline images | `shell/markdown.rs` | Rune supports inline boxes in text |
| Custom emoji pills in the composer show `:name:` | pill chips take only a vector `Icon` and no pill rect is exposed | `compose.rs` | Rune exposes pill rects or image pills |
| Format toolbar centres on the caret end | Rune exposes the caret's rect, not the selection's | `compose.rs` | a selection rect API |
| Compact layout: text wraps under its own start, not 32px under the name | no text-indent; placeholder spaces would leak into copies | `shell/compact.rs` | Rune gets text-indent |
| Pulsing call rings (someone being rung, the incoming call avatar) fade a fixed ring at half the spread instead of growing a box shadow | Rune's `LoopSpec` loops opacity (and rotation/slides), not scale or shadow spread | `shell/call.rs` (`call_participants_row`, `incoming`) | a looping scale or shadow track |
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
| The smooth wheel moves its glide's target by the content's growth while it glides up (a frame late, `retarget_wheels`), and older message pages that arrive mid-glide wait for it to stop (`app::fetch_older`) | a virtual list moves the offset by what's measured or added above the view, but not a running glide's target, so the glide flew on by that much (rows measured from the 48pt estimate, a whole page) | `ui/misc.rs`, `app.rs`, `main.rs` | Rune shifts `glide_to` with the anchor |
| An older message page is added with `keep_on_screen` on the message that was first (`app::prepend_older`) | a virtual list holds the topmost row in view, which at the top of what's loaded is the head skeleton filler: the page went in under it, the filler stayed in view and loaded the next page, on to the channel's start | `app.rs` | Rune anchors on a row the app marks, or skips fillers |
| Scaled markdown spans (headings) keep even half-leading, so their baseline differs a little from Fluxer's | Rune lays out a scaled span with its own line height | `shell/markdown.rs` | per-span baseline control |
| Jumbo emoji can't be selected or copied in place ("Copy text" has them); one-line previews keep emoji as text | the emoji are image overlays, not text; truncated one-line text has no overlay slots | `shell/markdown.rs` | inline images in text runs |
| Recovery kit "Print" opens the PDF in the system viewer | Rune can't print | `settings/recovery_kit.rs` | a print API |
| Pinned-messages popout resizes from the left and bottom edges only | Rune's `Cursor` has no diagonal resize; the anchored edges would need re-placement | `shell/pins.rs` | diagonal cursors |
| Call "fullscreen" fills the app window instead of the OS display | Rune has no fullscreen request | `shell/mod.rs` `view`, `shell/call.rs` | Rune exposes fullscreen |

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
| System audio for a screen share is our own capture: WASAPI loopback of the default render device on Windows (shared-mode mix format, polling `IAudioCaptureClient`, converted and linearly resampled to 48 kHz stereo) and a `ScreenCaptureKit` `SCStream` audio output over the main display on macOS (`capturesAudio`, self excluded); both push into the same `NativeAudioSource` as the Linux PipeWire path and appear as the synthetic `system` picker entry | the LiveKit Rust SDK/libwebrtc expose no desktop/system-audio capture, so Fluxer's Electron native modules have no SDK equivalent | `audio_share/`, `app/voice.rs` | the SDK exposes a system loopback / SCK audio source |
| "No hardware encoder was found" never shows on the encoder setting | webrtc-sys doesn't expose its encoder factory probe | `settings/advanced.rs` | the SDK exposes the probe |
| P2P mesh is our own implementation of `VoiceMeshEngine` (op 13, STUN only) | no JS engine to reuse | `reflux-voice/src/mesh.rs`, `app/voice.rs` | — |
| Screen-share source-ended detection is a flag the capturer thread sets when it stops; only display/window shares report it | libwebrtc's `DesktopCapturer` has no track-ended callback, and `Devices` shares capture through the camera pipeline, which has no end signal | `reflux-voice/src/real.rs` `spawn_capture`, `app/voice.rs` `tick` | the SDK exposes a track `ended` event or a capture-ended callback |
| Compact-call and HUD chrome hides on pointer enter/leave plus a 2.5 s timer | Rune reports hover changes but not pointer movement, so Fluxer's move-to-reveal behaviour isn't available | `shell/call.rs` `view` | Rune exposes pointer-move events |
| The region-teleport overlay runs on a timer after the region PATCH | the shell has no engine event for "teleport started/finished"; the request is the only signal | `shell/call.rs` `more_menu`, `view` | the engine reports a region-change/reconnect event |
| Stream previews in the compact call reuse the Active Now thumbnail cache | the shell can't subscribe a preview track without taking over the watcher state; Fluxer previews a live low-res track | `shell/call.rs` `stream_preview` | a preview-only subscription exists |
| Connection-stats packet loss and bitrate show "—" until the engine reports them | `Voice` carries ping/history only; the shell has no WebRTC stats handle | `shell/call.rs` `details` | `app/voice.rs` forwards a stats snapshot |


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
- `crates/reflux-voice/src/lib.rs:18` a second, hand-maintained implementation of the same API (`stub.rs`); delete it when
- `crates/reflux-voice/src/lib.rs:26` Fluxer's Noise gate, Speex and GTCRN backends aren't offered (no Rust ports wired).
- `crates/reflux-voice/src/real.rs:225` no `forceStereo` (stereo capture at 64 kbps+): the mic source is mono.
- `crates/reflux/src/app.rs:881` Rune shifts the offset by what lands above the view but not a running
- `crates/reflux/src/app.rs:894` Rune's virtual list holds the topmost row in view, which at the top of what's
- `crates/reflux/src/app/share_preview.rs:88` Fluxer snapshots the live video element; the call tile's renderer tees one
- `crates/reflux/src/app/voice.rs:1120` no ETag / 304 and no ×4 slower polling while the window is hidden.
- `crates/reflux/src/app/voice.rs:1326` Fluxer's Automatic mode warns only when its GPU probe expected hardware; webrtc-sys
- `crates/reflux/src/app/voice.rs:1790` only display/window shares report this; camera-device shares can't tell.
- `crates/reflux/src/app/voice.rs:1799` 2 s, not Fluxer's 1 s — each refresh re-captures every source's thumbnail.
- `crates/reflux/src/audio_share/mod.rs:167` first two channels; downmix when a 5.1 user reports a missing centre channel.
- `crates/reflux/src/compose.rs:324` Rune exposes the caret's rect, not the selection's, so the bar centres over the
- `crates/reflux/src/compose.rs:361` Undo and Redo are always enabled (Rune doesn't expose the history depth).
- `crates/reflux/src/desktop/hooks.rs:4` keyboard only; Fluxer's hooks also see mouse buttons, which our push-to-talk
- `crates/reflux/src/desktop/notices.rs:7` no price-announcement / legacy-price nagbars (they need Stripe's billing state).
- `crates/reflux/src/desktop/spell.rs:8` no OS spellchecker engine.
- `crates/reflux/src/desktop/spell.rs:339` Rune gives no text offset for a pointer position, so the caret stands in for it.
- `crates/reflux/src/desktop/tray.rs:28` Fluxer only adds Disconnect while connected, but Rune's SNI menu numbers items
- `crates/reflux/src/desktop/tts.rs:6` speech goes through the platform's command-line voice (speech-dispatcher's `spd-say`,
- `crates/reflux/src/desktop/updater.rs:12` Reflux has no release feed yet, so without `REFLUX_UPDATE_FEED` the updater stays
- `crates/reflux/src/desktop/window.rs:4` only the size is remembered. Rune reports no window position or maximized state
- `crates/reflux/src/desktop/window.rs:6` no minimize-to-tray; Rune sends no minimize event to hide on.
- `crates/reflux/src/file_picker.rs:4` macOS and Windows (and desktops without the portal) pick one file per dialog; the
- `crates/reflux/src/gst.rs:386` Fluxer's virtmic links new streams as PipeWire announces them; this rescans
- `crates/reflux/src/i18n.rs:5` UI literals go through `t` where they're drawn (option tables via `RadioGroup`, `Select`,
- `crates/reflux/src/i18n.rs:11` Arabic and Hebrew get right-to-left text runs (parley's bidi) but the layout isn't
- `crates/reflux/src/mf.rs:100` rodio's speed shifts the pitch (no `preservesPitch` like playbin's scaletempo);
- `crates/reflux/src/mf.rs:157` readers open and close one at a time, and an E_UNEXPECTED open is retried: under
- `crates/reflux/src/nav/keys.rs:82` a Tab inside a field moves focus without showing the intro.
- `crates/reflux/src/passkey.rs:14` no hybrid (phone) flow off Windows.
- `crates/reflux/src/popout.rs:6` on Wayland the compositor ignores always-on-top and placement (winit can't ask for
- `crates/reflux/src/quick_css_syntax.rs:4` only the variable completions; selector, `data-flx`, property and snippet completions,
- `crates/reflux/src/session.rs:230` userData guessed as `<config dir>/fluxer` (Linux ~/.config, Windows %APPDATA%, macOS
- `crates/reflux/src/settings/advanced.rs:363` only default and custom emoji are in the composer's `:` autocomplete; the
- `crates/reflux/src/settings/advanced.rs:621` Fluxer's "No hardware encoder was found" description needs webrtc-sys's factory
- `crates/reflux/src/settings/crop.rs:6` animated WebP crops to its first frame (no WebP animation encoder in `image`).
- `crates/reflux/src/settings/look_tabs.rs:747` sizes, spacing and other non-colour, non-font variables can't be overridden
- `crates/reflux/src/settings/premium.rs:4` no in-app Plutonium page, checkout, plan changes or gift code redeem form (the web
- `crates/reflux/src/settings/recovery_kit.rs:6` Print opens the PDF in the system viewer (Rune can't print).
- `crates/reflux/src/settings/recovery_pdf.rs:4` text outside Windows-1252 (CJK, Cyrillic…) is written in English instead of being
- `crates/reflux/src/settings/voice_tabs.rs:340` pressing records in place ("Press a key…") instead of opening Fluxer's recorder
- `crates/reflux/src/settings/voice_tabs.rs:379` Fluxer's system-wide keybind hint and setup alert live in Reflux's Desktop tab
- `crates/reflux/src/settings/voice_tabs.rs:512` opens the Advanced tab at its top; Fluxer scrolls to its Media section.
- `crates/reflux/src/settings/voice_tabs.rs:695` Fluxer's bars are slanted (`clip-path` parallelograms); these are plain rounded bars.
- `crates/reflux/src/shell/attachments.rs:552` plain text; Fluxer links it to the attachment-expiry help article.
- `crates/reflux/src/shell/call.rs:128` kept for the session; Fluxer persists it. Persist with the other layout prefs.
- `crates/reflux/src/shell/call.rs:199` the store keeps a 200-message window and no total unread count, so a channel with
- `crates/reflux/src/shell/call.rs:493` only our own Speak permission is known here; others show red for a server mute only.
- `crates/reflux/src/shell/call.rs:575` no "Add stream" button; reflux watches one stream at a time. Add it with
- `crates/reflux/src/shell/call.rs:667` other users' accent colours only arrive with their profile, which the tile
- `crates/reflux/src/shell/call.rs:1287` reflux keeps favorites for community channels only, so a DM call has no star.
- `crates/reflux/src/shell/call.rs:1312` `MediaVerticalVolumeControl`'s slider isn't here; the button mutes the stream.
- `crates/reflux/src/shell/call.rs:1340` no region-teleport event from the engine; the overlay runs for a few seconds
- `crates/reflux/src/shell/call.rs:1607` checking it needs an app, which is picked in the share dialog; with none picked
- `crates/reflux/src/shell/call.rs:1717` `localeCompare` is approximated by a case-insensitive compare.
- `crates/reflux/src/shell/call.rs:1901` a hold that runs out re-sorts on the next rebuild, not on a timer of its own.
- `crates/reflux/src/shell/call.rs:2004` Rune loops opacity only, so the ring fades in and out at half its spread
- `crates/reflux/src/shell/call.rs:2045` Button draws every icon at 16px; Reject's X and Ignore's bell are 18px in Fluxer.
- `crates/reflux/src/shell/call.rs:2445` Rune has no brightness filter or pressed offset, so hover only swaps the colour
- `crates/reflux/src/shell/call.rs:2472` Rune loops opacity only, so the ring fades in and out at half its spread.
- `crates/reflux/src/shell/call.rs:2562` no backdrop blur behind the card.
- `crates/reflux/src/shell/call.rs:2614` no edge resizing (240px .. 70% of the window) or fling physics; it keeps 320px and
- `crates/reflux/src/shell/call.rs:2682` Rune has no diagonal resize cursor, so it shows the row one.
- `crates/reflux/src/shell/call.rs:3167` device cards are names only (Fluxer shows live previews, which needs
- `crates/reflux/src/shell/call.rs:3450` the shell has no WebRTC stats handle; values stay dashes until the engine
- `crates/reflux/src/shell/call.rs:3477` no "Copy stats JSON" or "Screen share privacy" (Reflux has neither).
- `crates/reflux/src/shell/call.rs:3535` no "Pop out user", "Disable video locally" or "Change friend nickname" (no
- `crates/reflux/src/shell/compact.rs:5` the text wraps under its own start, not 32px under the name
- `crates/reflux/src/shell/devtools.rs:634` a failure is a toast, not Fluxer's "Failed to update premium perk state." modal; add the modal if staff want it.
- `crates/reflux/src/shell/devtools.rs:942` no pending label ("Forgetting authorized IPs...") and failures are toasts, not error modals; track the request if staff need them.
- `crates/reflux/src/shell/embeds.rs:191` play opens the video in the browser; Fluxer swaps in the YouTube iframe.
- `crates/reflux/src/shell/markdown.rs:62` Reflux's chrome text isn't selectable, so the setting changes nothing yet.
- `crates/reflux/src/shell/markdown.rs:94` a scaled span keeps its half-leading even, so the line's baseline lands where that
- `crates/reflux/src/shell/markdown.rs:393` emoji in one-line previews stay text (Fluxer draws 1.25em images).
- `crates/reflux/src/shell/mod.rs:558` Rune hovers an element by its own rect, not its children's (DOM keeps the row
- `crates/reflux/src/shell/mod.rs:807` Rune can't request OS fullscreen; filling the app window is the desktop equivalent.
- `crates/reflux/src/shell/neko.rs:3` walk/scratch/sleep animations and fling physics are left out; add them with a
- `crates/reflux/src/shell/pins.rs:5` only the left and bottom edges resize (the ones facing away from the anchor; Rune
- `crates/reflux/src/shell/search.rs:77` Rune spans draw a padded rounded fill only, so the chip has no 1px border, the key and
- `crates/reflux/src/shell/sidebar.rs:74` the `banner` variant's drop shadow isn't drawn.
- `crates/reflux/src/shell/status.rs:92` Fluxer's hover submenu (`UserAreaAccountSwitchMenu`) isn't built; the row's
- `crates/reflux/src/shell/status.rs:129` a set custom status shows read-only (`CustomStatusDisplay isEditable` edits it on
- `crates/reflux/src/shell/status.rs:144` the per-status expiry submenus ("For 15 minutes" …) aren't offered; a status set
- `crates/reflux/src/shell/switcher_extra.rs:102` Reflux keeps no other account connected, so there are no unread/mention sections or
- `crates/reflux/src/shell/threads.rs:689` Rune wraps a long placeholder even in a single-line field; cut it near the
- `crates/reflux/src/shell/upload.rs:241` no "Send gift" (needs the instance's premium pricing state; gifts are bought in the
- `crates/reflux/src/shell/voice.rs:1199` Rune's press has no pointer position, so a click anchors the menu to the
- `crates/reflux/src/shell/voice.rs:1617` the CSS steps each dot's opacity on its own keyframes; Rune's loop fades them
- `crates/reflux/src/shell/voice.rs:1712` it opens below the button (Reflux's stream controls sit in the header, Fluxer's in
- `crates/reflux/src/shell/voice_list.rs:87` no accent folding or ICU punctuation order, upgrade if names with accents sort wrong.
- `crates/reflux/src/shell/voice_list.rs:136` LiveKit speakers are tracked per user, so all of a user's connections speak; upgrade when speaking is per connection.
- `crates/reflux/src/shell/voice_list.rs:374` Rune rounds the ring from the row's radius, not `.channelItemFocusRing`'s 8px; upgrade when rings take a radius.
- `crates/reflux/src/shell/voice_list.rs:434` Rune's drops are file-only, so the row's own drag hit-tests the pointer against the
- `crates/reflux/src/shell/voice_list.rs:603` the shell doesn't know this client's connection ID, our latest voice state's stands in; upgrade when `ShellState` mirrors it.
crates/reflux/src/theme/mod.rs:100:/// shortcut: the rest of `--font-fallback-sans` and `system-ui` are left to Rune's own fallbacks.
- `crates/reflux/src/ui/controls.rs:52` a cross-fade, not a slide; Rune's `instant_changes` has no per-element opt-out.
- `crates/reflux/src/ui/controls.rs:314` it shows while the whole control is hovered (Rune's hover group), which also
- `crates/reflux/src/ui/mark.rs:4` Rune keeps the element tree private, so text is marked where it's built: the shared
- `crates/reflux/src/ui/menu.rs:227` rune can't tell us which way the parent flipped, so a nested submenu opens to the
- `crates/reflux/src/ui/misc.rs:67` the keyframes also scale each dot to .8; Rune's loop here is opacity only.
- `crates/reflux/src/ui/misc.rs:76` rune-kit's bar keeps its own 2pt edge gap and 24pt minimum thumb; Fluxer's regular
- `crates/reflux/src/ui/misc.rs:169` a virtual list keeps what's on screen when rows above it are measured
- `scripts/fast-build.sh:5` a second, hand-maintained no-WebRTC implementation of reflux-voice's API; delete it
- `scripts/mf-wine-test.sh:8` wine's MF (winegstreamer) stands in for Windows' own decoders; run the same on a

<!-- END GENERATED -->
