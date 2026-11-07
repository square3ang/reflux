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
| Settings-search highlight covers only text built by the shared helpers | Rune keeps the element tree private | `ui/mark.rs` | a tree walk / text query API |
| Quick CSS applies only `:root` colour/font variables | Rune has no CSS engine | `settings/look_tabs.rs`, `quick_css.rs` | never (by design) |
| Spellcheck uses our own dictionary; the caret stands in for the pointer position | no OS text services; no pointer→text-offset API | `desktop/spell.rs` | Rune exposes hit-testing into text |
| Undo/Redo always enabled | Rune doesn't expose the edit history depth | `compose.rs` | history API |
| Inline video on Windows/Linux doesn't use Rune's `Player` | `Player` has only a macOS (AVFoundation) backend | `gst.rs` (Linux), Windows backend in progress | `Player` gains Windows/Linux backends |
| RTL: bidi text runs only, layout isn't mirrored | Rune has no layout direction | `i18n.rs` | layout direction in Rune |
| No minimize-to-tray, window size only (no position/maximized) — **WONTFIX** (user decision) | no minimize event; position impossible on Wayland | `desktop/window.rs` | — |
| Icons can't be mirrored (missed-call phone) | no icon transform | `shell/system_msg.rs` | icon flip/transform |
| Pinned-messages popout resizes from the left and bottom edges only | Rune's `Cursor` has no diagonal resize; the anchored edges would need re-placement | `shell/pins.rs` | diagonal cursors |

## Platform

| Workaround | Why | Where | Remove when |
|---|---|---|---|
| Global shortcuts: Wayland portal; X11 RECORD hook, Windows/macOS listen-only keyboard hooks; in-window press of a global combo is skipped | no single cross-platform API; the portal only exists on Wayland desktops | `desktop/hooks.rs`, `desktop/shortcuts*` | — |
| Global shortcuts are keyboard-only; X11 keymap read once | hooks don't capture mouse buttons; layout changes aren't watched | `desktop/hooks.rs` | watch `MappingNotify`, add mouse hooks |
| Windows single instance: a per-user named pipe forwards deep links to the running app | Rune has no single-instance API | `desktop/` deep links | — |
| macOS deep links only as launch arguments | an open-URL event isn't exposed by Rune and needs the bundle's URL scheme | `desktop/` deep links | Rune forwards `application:openURLs:` |
| Flatpak autostart through the XDG Background portal, result kept in a state file | no `~/.config/autostart` access in the sandbox | `desktop/autostart*` | — |
| Multi-file picking through the XDG portal (`multiple: true`), else Rune's one-file dialog; Windows/macOS one file per dialog | Rune's dialog picks one file | `file_picker.rs` | Rune multi-select dialog |
| Wayland: no in-app screen-share thumbnails; Applications / Entire screen buttons steer the portal dialog | the portal can't enumerate sources | `shell/call.rs`, `reflux-voice` `ScreenSource::portal` | — |
| Wayland ignores always-on-top for PiP pop-outs | winit can't request it | `popout.rs` | compositor protocol support |
| Theme folders are rescanned every 2 s | same as Fluxer's polling fallback; no cross-platform file watcher used | `settings/theme_folder.rs` | add a notify-based watcher |
| TTS through the platform's CLI voice (`spd-say`, `say`, PowerShell) | no speech API in Rune | `desktop/tts.rs` | — |
| Passkeys: Windows via webauthn.dll; Linux/macOS USB keys only | no Linux platform authenticator; macOS iCloud passkeys need an entitlement only fluxer.app has | `passkey.rs` | — |
| App-audio capture on Linux: `pw-dump` to find the node + `pipewiresrc`; appsink with `sync=false` (with sync it dropped ~70% of the audio) | no PipeWire API in libwebrtc | `gst.rs`, `reflux-voice` | — |
| Camera and inline video on Linux go through GStreamer (`gst` feature, default) | libwebrtc's capture isn't exposed by the Rust SDK; Rune has no Linux player | `gst.rs` | — |

## Voice / LiveKit Rust SDK

| Workaround | Why | Where | Remove when |
|---|---|---|---|
| Mic and remote audio go through our own mixer and mic chain (WebRTC APM echo cancel + AGC, RNNoise), not LiveKit's playout | device selection, volumes, noise suppression and voice-activity need it | `reflux-voice/src/audio.rs` | — |
| H.264 profile `42e01f` for NVENC | the hardware factory rejects other profiles | `reflux-voice` | — |
| Screen-share "content hint" maps to libwebrtc's degradation preference | the Rust SDK has no content-hint setter | `reflux-voice`, `settings/advanced.rs` | SDK exposes `contentHint` |
| Mixer resampling is linear interpolation | simple; aliases a little | `reflux-voice/src/audio.rs` | use windowed-sinc if music matters |
| P2P mesh is our own implementation of `VoiceMeshEngine` (op 13, STUN only) | no JS engine to reuse | `reflux-voice/src/mesh.rs`, `app/voice.rs` | — |

## Server / instance

| Workaround | Why | Where | Remove when |
|---|---|---|---|
| REST base from `api_transport`, then `api_public`… instead of `web.fluxer.app/api` | the web origin returns 403 `INVALID_API_ORIGIN` to other clients | `reflux-net/src/rest.rs` | — |
| Media proxy bodies are capped (images 64 MiB, text 4 MiB, downloads 512 MiB) | remote input; don't buffer unbounded responses | `media.rs` `get_capped` | — |
| YouTube embeds open the browser | the embed carries only an iframe URL, no stream | `shell/embeds.rs` | (optional) a stream resolver like `yt-dlp` |
| Synced preferences have no base-hash / 409 retry | the favorites splice has no per-field dirty tracking, so a refetch after a 409 would drop the local edit instead of merging it | `synced.rs` | port `flushSyncedPreferences` |
| P2P rollout polls every `poll_interval_seconds` without ETag / 304 and doesn't slow down ×4 while hidden | Rune reports no window visibility; the REST client has no conditional requests | `app/voice.rs` | visibility events + `If-None-Match` |
| Quick switcher orders mentions by newest message, not by when the mention arrived | the store keeps no mention arrival time | `shell/switcher.rs` | record arrival times in the store |

## Session / tokens

| Workaround | Why | Where | Remove when |
|---|---|---|---|
| Tokens live in the OS keyring (service `reflux`, user `<config dir>#session` for the active one, `#account:<instance>:<user>` for others), else in the 0600 file | like Fluxer's `DesktopAppStorage` | `session.rs` | — |
| Importing from the Fluxer desktop app guesses its userData as `<config dir>/fluxer`; logout of an imported session forgets the token locally only | the imported session belongs to the Fluxer app | `session.rs` | — |

## Build / tooling

| Workaround | Why | Where | Remove when |
|---|---|---|---|
| Shared `CARGO_TARGET_DIR` across worktrees; stale cross-worktree errors fixed by `touch`ing the crate's `lib.rs`/`main.rs` | disk is tight; libwebrtc builds are huge | HANDOFF.md | more disk |
| NVENC needs the gitignored `.cuda` symlink before the first build | webrtc-sys looks for CUDA headers there | HANDOFF.md | — |
| `scripts/cross-check.sh`: Windows/macOS `cargo check` with zig; webrtc-sys's MSVC C++ and the Objective-C sources get empty objects, a fake `xcrun` satisfies the macOS build script — nothing links | no mingw/MSVC toolchain or Mac here | `scripts/cross-check.sh` | real Windows/macOS CI |
| `winapi` with the `std` feature on Windows | `authenticator` 0.5's Windows HID transport doesn't compile otherwise | `crates/reflux/Cargo.toml` | authenticator fix |
| The live probe reads the token from the keyring off the async runtime (`spawn_blocking`) | the Secret Service client blocks and panics inside tokio | `reflux-net/examples/live_probe.rs` | — |

<!-- BEGIN GENERATED: scripts/workarounds.sh -->

## Appendix: every `shortcut:` comment in the code

- `crates/reflux-markdown/src/lib.rs:68` clones the emoji table per call (upstream API takes it by value); share an Arc if it gets large.
- `crates/reflux-store/src/lib.rs:656` forum "new post" levels aren't modelled (unjoined posts notify on mentions).
- `crates/reflux-store/src/lib.rs:926` typed payloads can't tell an omitted `reactions` from an empty one; keep the
- `crates/reflux-voice/src/audio.rs:132` linear interpolation aliases a little; use a windowed-sinc resampler if music
- `crates/reflux/src/app/voice.rs:994` no ETag / 304 and no ×4 slower polling while the window is hidden.
- `crates/reflux/src/compose.rs:19` a custom emoji pill shows `:name:`, not the emoji image.
- `crates/reflux/src/compose.rs:146` Rune exposes the caret's rect, not the selection's, so the bar centres over the
- `crates/reflux/src/compose.rs:183` Undo and Redo are always enabled (Rune doesn't expose the history depth).
- `crates/reflux/src/compose.rs:314` no "nickname changed" system message; a failed /msg DM only logs.
- `crates/reflux/src/desktop/hooks.rs:4` keyboard only; Fluxer's hooks also see mouse buttons, which our push-to-talk
- `crates/reflux/src/desktop/hooks.rs:44` the first keysym per keycode, read once; a layout switched later isn't seen.
- `crates/reflux/src/desktop/notices.rs:7` no price-announcement / legacy-price nagbars (they need Stripe's billing state).
- `crates/reflux/src/desktop/notices.rs:229` "Manage subscription" / "Reactivate" open the web Plutonium page instead of
- `crates/reflux/src/desktop/spell.rs:8` no OS spellchecker engine.
- `crates/reflux/src/desktop/spell.rs:339` Rune gives no text offset for a pointer position, so the caret stands in for it.
- `crates/reflux/src/desktop/tts.rs:6` speech goes through the platform's command-line voice (speech-dispatcher's `spd-say`,
- `crates/reflux/src/desktop/updater.rs:12` Reflux has no release feed yet, so without `REFLUX_UPDATE_FEED` the updater stays
- `crates/reflux/src/desktop/window.rs:4` only the size is remembered. Rune reports no window position or maximized state
- `crates/reflux/src/desktop/window.rs:6` no minimize-to-tray; Rune sends no minimize event to hide on.
- `crates/reflux/src/file_picker.rs:4` macOS and Windows (and desktops without the portal) pick one file per dialog; the
- `crates/reflux/src/guild/audit/mod.rs:4` users in sentences are bold names (Fluxer's `ClickableUser` opens a profile), and
- `crates/reflux/src/guild/modals.rs:82` "Import template" (a third-party platform's template API) isn't offered.
- `crates/reflux/src/guild/modals.rs:109` a picked icon shows as a picture glyph, not a preview of the image.
- `crates/reflux/src/guild/modals.rs:490` unread badges and the reply mention preference aren't here.
- `crates/reflux/src/guild/settings.rs:172` parses the kept JSON every frame; keep typed lists if they get large.
- `crates/reflux/src/i18n.rs:5` UI literals go through `t` where they're drawn (option tables via `RadioGroup`, `Select`,
- `crates/reflux/src/i18n.rs:11` Arabic and Hebrew get right-to-left text runs (parley's bidi) but the layout isn't
- `crates/reflux/src/keybinds.rs:114` plain and Alt-only character combos aren't recordable (they'd type).
- `crates/reflux/src/look.rs:4` emoticons are matched as whole space-separated words from a short list.
- `crates/reflux/src/look.rs:5` Fluxer also syncs some of these through `synced_preferences` (a protobuf in the
- `crates/reflux/src/nav/favorites.rs:4` categories and nicknames are kept in the snapshot but not shown; no drag reordering.
- `crates/reflux/src/nav/keys.rs:81` a Tab inside a field moves focus without showing the intro.
- `crates/reflux/src/nav/keys.rs:279` records in place instead of in a popout.
- `crates/reflux/src/nav/mod.rs:64` kept until the DM shows up; a DM that can't open leaves it pending.
- `crates/reflux/src/passkey.rs:14` no hybrid (phone) flow off Windows.
- `crates/reflux/src/popout.rs:6` on Wayland the compositor ignores always-on-top and placement (winit can't ask for
- `crates/reflux/src/popout.rs:36` at most this many pop-outs at once (Fluxer has no limit); the oldest gives way.
- `crates/reflux/src/quick_css.rs:4` no CSS completions or syntax colours (Fluxer's CodeMirror workspace); a plain mono
- `crates/reflux/src/session.rs:216` userData guessed as `<config dir>/fluxer` (Linux ~/.config, Windows %APPDATA%, macOS
- `crates/reflux/src/settings/advanced.rs:4` no "New settings" section, no in-tab search field (the settings search filters this
- `crates/reflux/src/settings/advanced.rs:226` Fluxer's "No hardware encoder was found" description needs webrtc-sys's factory
- `crates/reflux/src/settings/crop.rs:6` animated WebP crops to its first frame (no WebP animation encoder in `image`); no "Skip"
- `crates/reflux/src/settings/dev.rs:3` the bot banner preview is the profile card's (Fluxer shows an `ImagePreviewField`).
- `crates/reflux/src/settings/look_tabs.rs:599` "Sync theme across devices" is always on (the theme is saved to the account).
- `crates/reflux/src/settings/look_tabs.rs:684` sizes, spacing and other non-colour, non-font variables can't be overridden
- `crates/reflux/src/settings/mod.rs:259` restores the snapshot from before this patch, dropping any later
- `crates/reflux/src/settings/mod.rs:591` Fluxer also lists What's new, Submit feedback and Donate here; they open the
- `crates/reflux/src/settings/premium.rs:4` no in-app Plutonium page, checkout, plan changes or gift code redeem form (the web
- `crates/reflux/src/settings/recovery_kit.rs:6` Print opens the PDF in the system viewer (Rune can't print).
- `crates/reflux/src/settings/recovery_pdf.rs:4` text outside Windows-1252 (CJK, Cyrillic…) is written in English instead of being
- `crates/reflux/src/settings/theme_folder.rs:5` "Stop watching" confirms inline (like "Reset data") instead of a ConfirmModal.
- `crates/reflux/src/settings/theme_library.rs:4` each row has a delete button instead of Fluxer's options menu (edit details/CSS, move
- `crates/reflux/src/shell/attachments.rs:136` the region shows as its ISO code, and the age-check texts have no "Learn more" link.
- `crates/reflux/src/shell/autocomplete.rs:3` no command option hints; custom emoji come from the open guild only.
- `crates/reflux/src/shell/call.rs:347` the explanation is the accessible label, not a hover tooltip (no overlay layer here).
- `crates/reflux/src/shell/call.rs:369` Shift-click doesn't skip the confirmation.
- `crates/reflux/src/shell/chat.rs:774` no media items (copy/save image, open link) when the right-click lands on an
- `crates/reflux/src/shell/chat.rs:1024` the whole name line is the click target, not just the name.
- `crates/reflux/src/shell/chat.rs:1434` the context menu edits the main composer only.
- `crates/reflux/src/shell/chat.rs:1478` the thread panels have no upload tray; files go through the full view.
- `crates/reflux/src/shell/chat.rs:1536` in the thread panels the list is keyboard-only (its clicks edit the main draft).
- `crates/reflux/src/shell/compact.rs:5` the text wraps under its own start, not 32px under the name
- `crates/reflux/src/shell/compact.rs:105` lives here (not in chat.rs) to keep the shared file's diff small.
- `crates/reflux/src/shell/debug.rs:5` the JSON is plain monospace text (Fluxer runs it through its code-block highlighter).
- `crates/reflux/src/shell/embeds.rs:190` play opens the video in the browser; Fluxer swaps in the YouTube iframe.
- `crates/reflux/src/shell/guilds.rs:242` skips the canvas width fit that shrinks wide acronyms further.
- `crates/reflux/src/shell/invite_embed.rs:252` the instance's custom guild badges (`badge_ids`) and built-in replacement images
- `crates/reflux/src/shell/links.rs:116` the title "External link warning" is for screen readers only in Fluxer; ours shows.
- `crates/reflux/src/shell/markdown.rs:91` a scaled span keeps its half-leading even, so the line's baseline lands where that
- `crates/reflux/src/shell/markdown.rs:331` skin-tone variants and emoji missing from the base sheet fall back to the font
- `crates/reflux/src/shell/markdown.rs:354` jumbo emoji can't be selected or copied (no text); "Copy text" has them.
- `crates/reflux/src/shell/markdown.rs:385` emoji in one-line previews stay text (Fluxer draws 1.25em images).
- `crates/reflux/src/shell/menus.rs:179` privacy, the community profile, hide muted channels and delete-my-messages are only
- `crates/reflux/src/shell/mod.rs:800` group DM icons aren't loaded; the recipient cluster always shows.
- `crates/reflux/src/shell/pins.rs:5` only the left and bottom edges resize (the ones facing away from the anchor; Rune
- `crates/reflux/src/shell/pins.rs:245` media wider than the card is cropped, not scaled down to fit.
- `crates/reflux/src/shell/queue.rs:87` Fluxer posts these as a local "Only you can see this" system message; here
- `crates/reflux/src/shell/search.rs:6` tokens whose value doesn't resolve aren't marked "unapplied" (wavy warning).
- `crates/reflux/src/shell/search.rs:594` media wider than the card is cropped, not scaled down to fit.
- `crates/reflux/src/shell/search_query.rs:4` no search chips or confusable-skeleton matching; unresolvable values are dropped as in
- `crates/reflux/src/shell/sidebar.rs:30` the banner doesn't collapse or frost as the channel list scrolls.
- `crates/reflux/src/shell/switcher.rs:3` no message links, other accounts or member search over the network.
- `crates/reflux/src/shell/switcher.rs:69` mentions are ordered by newest message, not by when the mention arrived.
- `crates/reflux/src/shell/system_msg.rs:191` Fluxer mirrors the missed-call phone (`scaleX(-1)`); Rune icons can't flip.
- `crates/reflux/src/shell/threads.rs:392` content as typed (no markdown stripping); attachment-only messages say so.
- `crates/reflux/src/shell/threads.rs:405` the punctuation around the links is approximated.
- `crates/reflux/src/shell/threads.rs:429` the whole line opens the thread (or the browser when it isn't loaded), rather
- `crates/reflux/src/shell/threads.rs:690` Rune wraps a long placeholder even in a single-line field; cut it near the
- `crates/reflux/src/shell/upload.rs:102` rules filtered by guild features aren't applied (user-scope rules only).
- `crates/reflux/src/shell/upload.rs:196` no "upload as file" or gift items.
- `crates/reflux/src/social/inbox.rs:493` a single-line field with a counter (Fluxer's is a 3–8 row textarea).
- `crates/reflux/src/synced.rs:4` no `synced_preferences_base_hash` / 409 retry like `UserSettings.flushSyncedPreferences`;
- `crates/reflux/src/ui/button.rs:182` hover/press replace the fill only; CSS also shifts the border colour, which
- `crates/reflux/src/ui/controls.rs:251` the grabber's hover border (brand 62%) is skipped; Rune's hover paints an overlay, not a border.
- `crates/reflux/src/ui/mark.rs:4` Rune keeps the element tree private, so only text built by the shared helpers
- `crates/reflux/src/ui/misc.rs:67` the keyframes also scale each dot to .8; Rune's loop here is opacity only.
- `crates/reflux/src/ui/misc.rs:76` rune-kit's bar keeps its own 2pt edge gap and 24pt minimum thumb; Fluxer's regular

<!-- END GENERATED -->
