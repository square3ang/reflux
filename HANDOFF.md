# Reflux handoff

Goal: rewrite the Fluxer desktop client (https://github.com/fluxerapp/fluxer) in Rust with
Rune (https://github.com/anomalyco/rune, pinned rev `781e2b6d87cd5f32d6f4365956fad4baaf24ee50`,
**do not fork Rune**). Pixel-identical design to the official app, all features, third-party
instances, Linux NVENC + VA-API screen share. License AGPL-3.0-or-later (assets are copied from Fluxer).

## Reference material (local clones, not in the repo)
- `/tmp/opencode/fluxer` — Fluxer monorepo. UI: `fluxer_app/src` (React, ~260k LOC, Phosphor icons,
  CSS modules). Desktop shell: `fluxer_desktop`. API/Gateway docs: `fluxer_docs/src/content/docs`
  (very complete: http-api/*.mdx, gateway/*.md, media-proxy/*, topics/captcha.md = ALTCHA PoW, so
  no webview is needed for captcha).
- `/tmp/opencode/rune` — Rune at the pinned rev. Read `crates/rune/README.md` (2.7k lines) and
  `crates/rune-kit/README.md`. Headless check: `cargo run -p reflux -- --snap out.png --size 1280x800 --scale 1`.
- `/tmp/opencode/rust-sdks` — LiveKit Rust SDK. `webrtc-sys` already has Linux NVENC
  (`src/nvidia`, compiled only when `$CUDA_HOME/include/cuda.h` exists; libcuda is dlopened) and
  VA-API (`src/vaapi`, compiled when pkg-config finds libva) encoder factories, chosen with
  `LIVEKIT_PREFERRED_HW_ENCODER=nvenc|vaapi`. On this machine a cuda.h is at
  `~/.local/lib/python3.14/site-packages/nvidia/cu13/include/cuda.h` (pip nvidia-cuda-runtime).
  GPU: RTX 3060, libva 2.24 with nvidia-vaapi-driver.
- `/tmp/opencode/phosphor` — @phosphor-icons/core SVGs.
- `/tmp/opencode/sink/*.json` + `assets/theme/*.json` — CSS variables computed from the live web
  app (web.fluxer.app) for dark/light/coal, and `login-layout.json` (element rects + computed
  styles of the login page). Technique: in-app browser `evaluate` dumps rects/styles, then POST to
  a tiny local sink server (`/tmp/opencode/sink.py`, port 18765) to save to disk.
  Logged-in screens need a test account (not available yet) — ask the user for one, or self-host
  (docker is installed but the daemon was not running).

## Repo state (`/home/square3ang/Documents/reflux`)
See `git log` for details. Crates:
- `reflux-model` — serde types for API/gateway, `DispatchEvent::parse` (81 events + `Unknown`),
  permissions calc (`compute_permissions`, `thread_permissions`). 29 tests.
- `reflux-net` — discovery (`/.well-known/fluxer`), REST (`Client`, per-route + global rate limits,
  ALTCHA v2 = PBKDF2 PoW auto-solve + retry), gateway (zstd-stream both directions, heartbeat,
  resume, backoff). `Client::from_discovery` prefers `api_transport`/`api_public` because
  `web.fluxer.app/api` rejects non-browser Origins (403 INVALID_API_ORIGIN). Default instance
  `fluxer.com`.
- `reflux-store` — `Store::apply(&DispatchEvent, now_ms)`, message windows (200), optimistic sends
  by nonce, unread/mention rules, sorted guild/channel lists. Gaps are marked `shortcut:`.
- `reflux-voice` — livekit (git rev 052f0891…) skeleton (`Call::connect/publish_mic/publish_screen`)
  + `examples/encode_probe.rs`. NVENC works only with H.264 profile `42e01f` (default `42001f`
  silently falls back to OpenH264). VA-API encode unavailable on this NVIDIA box (decode-only
  driver). `CUDA_HOME` via `.cargo/config.toml` → gitignored `.cuda` symlink. Voice token comes
  from gateway op 4 → `VOICE_SERVER_UPDATE` (no HTTP route). Not yet wired into the app.
- `reflux-markdown` — Fluxer's own markdown parser.
- `reflux` (bin) — `app.rs` (tokio runtime thread + Rune `AppHandle`; `Action`/`act`; `poll`
  drains `shell::Intent`s each frame), `session.rs` (~/.config/reflux/session.json, 0600),
  `login.rs` (instance picker, email/password, TOTP, IP-authorization polling), `shell/` (guild
  rail, sidebar, chat, composer, members; `--shell-demo` fixture), `ui/` kit (button, input,
  avatar, tooltip, modal, menu, controls, select, misc; `--gallery`), `theme/`, `icons.rs`.
  Only one real API call was made (wrong-credential login → proper error). Everything after
  login is untested against a live server (no test account).
- Done since: message actions (reply/edit/delete/reactions/typing/acks), user settings + theme
  switching, media-proxy images (WebP/GIF decoded by `image` → `ImageCache::rgba`), inline emoji
  (no-break-space gap + `rune::memo::layout(key)` overlay; copy mapped via `El::selection_copy`),
  friends/DMs/profile popout/lazy member list/context menus, voice (op 4 → LiveKit, panel,
  mute/deafen, speaking rings, screen share, device picker), register/forgot password/USB FIDO2
  passkeys (`authenticator` crate; origin = instance web origin), community/channel settings,
  invites, roles, notification settings. All verified only with `--shell-demo` / login scenes.
- Desktop (`desktop/`, `keybinds.rs`, `i18n.rs`): message notifications per Fluxer's rules
  (`Store::should_notify`: levels/overrides/mutes/DND/suppress flag; focus + viewed channel in
  `desktop::on_message`) via notify-rust with the author's avatar (cached PNG) and click → focus +
  open channel (Linux only); sounds via rodio (message, mute/deafen, voice join/leave/disconnect,
  ringing); Rune tray (rebuilt to toggle the red mention dot; show/hide, settings, mute, deafen,
  disconnect, quit) + close-to-tray (`keep_running` + `on_close`); Unity LauncherEntry dock badge;
  Wayland urgency via tokenless focus; `fluxer://invite|channels` links + single instance over
  `$XDG_RUNTIME_DIR/reflux-<uid>.sock` (`packaging/reflux.desktop` has the MimeType); Fluxer's
  default keybinds (window `on_key`) + Keybinds/Notifications/Language/Linux app settings tabs.
  i18n: English text is the key into Fluxer's catalogs; `scripts/i18n-extract.py` copies the used
  entries to `assets/i18n/*.json` (re-run after adding strings). Only login + settings chrome are
  converted; the remaining files are listed in `i18n.rs`. Headless snapshots: `REFLUX_LOCALE=ko`.
- Media (`shell/{attachments,embeds,lightbox,links,highlight,invite_embed}.rs`, `media.rs`):
  mosaic, spoiler/mature blur, inline video/audio (Rune `Player`; it only plays on macOS, so
  Linux/Windows show "Open in browser"), file cards + text previews, rich/YouTube/gifv embeds,
  invite cards (`Intent::FetchInvite` → `app/invites.rs`), lightbox (zoom/pan/arrows/Esc,
  download, copy image/link), external link warning + trusted domains, syntect highlighting,
  GIF/animated-WebP frames on a shared clock (`Media::take_next_frame` → `wake_after`).
  Scenes: `--shell-demo --msg-scene=media-{mosaic,files,embeds,invite,viewer,link,nsfw,blur}`.
  Shared target dir caveat: other worktrees overwrite `target/debug/reflux`; snapshot with the
  `target/debug/deps/reflux-<hash>` your build produced.
- Account (`settings/{profile,account,privacy,sudo}.rs`, `shell/{status,profile}.rs`,
  `app/account.rs`): profile editor (global + per-community, auto centre-crop), Account tab
  (email/password/username, TOTP with QR, backup codes, passkey list, disable/delete), Privacy,
  Devices, Authorized apps, Connections (domains), Blocked users; every write is an
  `Intent::Api` and `SUDO_MODE_REQUIRED` opens the sudo modal, then retries with the proof. User
  area popout (status, custom status, switch accounts), full profile modal, change nickname.
  Tokens live in the OS keyring (`keyring` crate; `REFLUX_NO_KEYRING=1` disables), several
  accounts in `Saved.accounts`. Demo: `--settings=<tab>`, `--account-ui=menu|status|accounts|nickname|sudo|profile`.
  None of the writes were exercised against a live server.
- Desktop shell, part 2 (`desktop/{autostart,window,spell,shortcuts,updater,streamer,time,nagbar}.rs`,
  prefs in `desktop::Prefs` inside session.json): launch at login (XDG autostart entry / HKCU Run /
  LaunchAgent, `--autostart` + "Start minimized" → `App::windowless`); remembered window size
  (Rune exposes no position/maximized or minimize event, so no minimize-to-tray); app zoom
  (Ctrl+=/−/0, Appearance slider) by laying the root out at window/zoom and `El::scale`-ing it,
  with `Popups::open_at` dividing pointer points by the zoom; composer spellcheck with Fluxer's
  CDN Hunspell dictionaries (sha256-checked, cached in `~/.cache/reflux/spellcheck`) read by
  `spellbook`, red underlines via `text_input().decorations`, right-click menu (suggestions, add to
  dictionary) for the caret's misspelling; global shortcuts via the xdg-desktop-portal
  GlobalShortcuts portal (ashpd, zbus 6) and Windows `RegisterHotKey` — voice can read
  `desktop::shortcuts::ptt_held()`; updater against `REFLUX_UPDATE_FEED` (GitHub latest-release
  JSON; no feed exists yet) with AppImage self-replace; locale-aware 12/24h times and dates
  (`desktop::time`, chrono `unstable-locales`, user setting `time_format`); window title badge;
  reopen last channel; nagbars (connection, update, streaming privacy) and toasts
  (`desktop::toast`); streaming privacy (`Store::mask_names` shortens names; account gate, invite
  link, sounds, notifications; OBS/XSplit detection). Snapshots: `--shell-demo
  --desktop=bars|spell|toast|streamer`, `REFLUX_ZOOM=1.25`. Windows/macOS code was compile-checked
  in an isolated crate only (no mingw toolchain for ring/zstd here).
- Voice & video (`app/voice.rs`, `shell/{voice,call,clip,voice_message}.rs`, `settings/voice_tabs.rs`,
  `gst.rs`, `voice_input.rs`, `reflux-voice`): camera (GStreamer `DeviceMonitor` → RGBA appsink →
  `Camera::push_rgba` → H.264 track, `CameraPreviewModal`), `VoiceCallView` (click the joined
  voice channel; grid / focused tile, camera + stream tiles, speaking border, mute icons, control
  bar), DM calls (header buttons, `/call` eligibility → join → `/call/ring`, `IncomingCallUI`
  from `Store::calls`, DM call strip, ring / stop-ringing in the tile menu, `CallMessage`
  wording), Voice & video settings (`Saved.voice_settings`; profiles, PTT key + release delay,
  VAD threshold/auto, mic test meter, camera, screen share quality/fps/bitrate, encoder →
  `LIVEKIT_PREFERRED_HW_ENCODER` / software), local mute, voice details popout (ping graph),
  E2EE (`e2ee_key` → shared-key AES-GCM frame encryption + lock badge), moderator move follow,
  reconnecting state, join-confirm / channel-full / refused modals, watch-stream full window +
  spectators, video/audio attachment playback through GStreamer (`gst` feature, default; Linux
  only, `--no-default-features` drops it), voice messages (composer "+" → record WAV via cpal,
  Fluxer waveform, `VOICE_MESSAGE` flag; waveform player). The mic gate enables/disables the
  local track (no mute signalling). Scenes: `--shell-demo --voice=call|focus|details|tile-menu|
  camera|share|full|refused|incoming|dm|voice-message|meter`, `--msg-scene=media-files`.
  Shortcuts (in code): no per-user/stream/device volume (SDK lacks it), EC/NS/AGC toggles only the
  hardware processors, P2P calls refused, no portal source thumbnails, no Linux stream audio, no
  entrance sound, mic test/VAD/recording use the default input. PTT = the in-window key OR
  `desktop::shortcuts::ptt_held()` (portal global shortcut; its key is set in the desktop). Nothing here was exercised in a real call (no test channel).
- Workflow that worked: each subagent in its own `git worktree` with shared
  `CARGO_TARGET_DIR=<repo>/target`, then rebase + `merge --ff-only` into master.
- Next: composer uploads, pins, search, threads/forums, quick switcher, converting the remaining
  UI strings to `t()`; and live testing (needs an account or a self-hosted instance).

## Design-fidelity status
- Reference: the live login page (`web.fluxer.app/login`, dark theme, 1020×893 viewport). The
  card is 800×448, centred, `background-secondary` (rgb 26,24,30), radius 8, 1px border
  rgba(255,255,255,.08). Left brand panel is 288px wide with a 1px `background-modifier-accent` right
  border, 128px logo (`assets/media/images/fluxer-logo-color.svg`) + 32px-tall wordmark, gap 24.
  Form side has 40px padding; title "Welcome back" is 20px/600, line-height 30; buttons are 430×36,
  radius 8, 13px/600, gap 10 (primary = `brand-primary` with a 1px border mixing in 18% white;
  secondary = `form-surface-background` + `background-modifier-accent` border), 16px icons
  (envelope, key, user-plus), icon gap 6. Page background is a 145° linear gradient
  rgb(5,4,13) → rgb(8,6,22) 46% → rgb(13,10,28), with a star field (`SpaceBackground`). Full numbers
  are in `assets/reference/login-layout.json`.
  The red "installation is corrupted" nagbar on that page is web-only; don't reproduce it.
- Ours so far: `/tmp/opencode/snaps/hello.png` = only the title text on `background-primary`.
  Next concrete step: write `ui/button.rs`, wire `mod ui; mod icons;` into `main.rs`, and build the
  login screen to match the numbers above, then compare snapshots side by side at the same size.

## Environment notes
- The in-app browser tab ID used earlier will be gone. Open a new tab, and restart the sink with
  `setsid python3 /tmp/opencode/sink.py &` (port 18765; 8765 is taken by something else).
- `/home` is 95% full (~100 GB free). The target dir grows quickly with Rune + libwebrtc.
- The Docker CLI is installed but the daemon is not running (needed only for self-hosting a test
  instance).
- `/tmp/opencode/*` clones are outside the repo and may be wiped on reboot. Re-clone if missing:
  fluxerapp/fluxer, anomalyco/rune (checkout the pinned rev), livekit/rust-sdks, and the
  @phosphor-icons/core npm tarball.

## User decisions so far
- Plan first, then implement in iterations until the whole chat app is rebuilt; subagents allowed.
- Linux, Windows and macOS all in scope; Rune is used as-is (no fork).
- Media: livekit-rust + hardware encoders (originally planned with GStreamer, but webrtc-sys's
  built-in NVENC/VAAPI factories make GStreamer unnecessary for encoding; confirm with the user if
  GStreamer turns out to be needed for capture).
- License: AGPL-3.0-or-later.

## Commands
- Build: `cargo build -p reflux` (first build ~6 min). Tests: `cargo test --workspace`.
- Snapshot: `cargo run -p reflux -- --snap /tmp/opencode/snaps/x.png --size 1280x800 --scale 1`.

## Live testing (real account, read-only)
- `reflux --import-fluxer-session` (or the login screen's "Import session from Fluxer desktop")
  copies the official desktop app's session (`<config>/fluxer/desktop-app-store.sqlite3`) into
  `~/.config/reflux/session.json`. Logging out of an imported session only forgets it locally.
- `REFLUX_READONLY=1` (forced by `--live` and the probe): non-GET REST (except
  `POST /search/messages`) and mutating gateway ops are refused in `reflux-net`.
- `cargo run -p reflux-net --example live_probe` (`REFLUX_PROBE_SECS`): parse failures with JSON
  paths, unknown events, rate limits, store invariants, dropped fields, pins/search/history pages.
- `reflux --live[=guild|members|dm|home|settings|pins|search|jump] --snap /tmp/opencode/snaps/live/x.png
  --size 1280x800 --scale 1 [--at 1]` renders the real UI headless (`REFLUX_LIVE_GUILD=N`,
  `REFLUX_LIVE_TAB`, `REFLUX_LIVE_QUERY`, `REFLUX_LIVE_JUMP_FRAMES`). Snapshots hold private
  messages: never commit them.
- Writes, in one channel only: `REFLUX_WRITE_CHANNEL=<channel id> reflux --live=write --snap ...
  --at 1` (send, edit, react, reply, typing, ack, pin, uploads both ways, search, jump; each step
  waits for its echo, then reads back from the server), then `--live=cleanup` (deletes the
  `[reflux test` messages and pin notices, then checks none remain). The read-only guard lets
  only `/channels/<that id>/...` writes and their upload PUTs through.
- Shared target dir: other worktrees overwrite `target/debug/reflux`; copy the binary right after
  building (cargo's `--message-format=json` `executable`) before running it.
