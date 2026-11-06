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
Commits: `fe8a272` skeleton/theme/fonts, `9dc92f3` markdown parser + icons.
- `Cargo.toml` — workspace, `members = ["crates/*"]`, shared deps (tokio, reqwest rustls,
  tokio-tungstenite, zstd, serde, rune, rune-kit).
- `crates/reflux` (bin) — `main.rs` is a placeholder "Welcome back" screen (builds; verified with
  `--snap`). `theme/` = generated `palette.rs` (215 colours × dark/light/coal as typed `Palette`
  fields, plus `dim::*` CSS dimensions in px) + Fluxer Sans/Mono fonts. `icons.rs` = 252 Phosphor
  icons used by fluxer_app (regular/`_BOLD`/`_FILL`). `ui/mod.rs` (uncommitted, not yet wired into
  main.rs) = `mix()` (CSS color-mix in sRGB), `rgb()`; references a `ui/button.rs` that is NOT
  written yet (spec: `fluxer_app/src/features/ui/button/Button.module.css`; hover = `.hover(color)`
  overlay drawn over fill).
- `crates/reflux-markdown` — done: Fluxer's own Rust markdown parser (upstream
  `packages/markdown_parser`) copied in; `parse(input, &Options)`, presets. Tests pass. Gaps: the
  app-side `cleanNode` link post-processing and unicode emoji context are not ported.
- `crates/reflux-model` — **currently does NOT compile** (subagent cancelled mid-way, uncommitted).
  `lib.rs` has the helper macros (`model!` = serde structs with `#[serde(default)]`, `int_enum!` =
  open integer enums, `int_flags!` = bitflags as JSON ints, `GuildScoped<T>`, `double_option`) and
  declares modules `auth channel gateway guild instance message permissions user`, but only
  `snowflake.rs` exists (`cargo check -p reflux-model` → E0583 file not found). Because `reflux`
  depends on it, **`cargo build -p reflux` is broken too** until those files exist (or the `mod`
  lines are commented out). Still needed: serde types for every API/gateway object (docs-driven),
  permission bitflags + calculation (with tests), typed `DispatchEvent` with an `Unknown` fallback.
  `bitflags = "2"` was added to its Cargo.toml.
- `crates/reflux-voice` — not created (subagent cancelled before writing files). Plan: LiveKit
  `livekit` crate (git, pinned) + cpal; set `CUDA_HOME` (e.g. `.cargo/config.toml` `[env]`) so
  webrtc-sys compiles the NVENC factory; encoder preference via `LIVEKIT_PREFERRED_HW_ENCODER`;
  prove with an encode probe example. Screen capture: libwebrtc desktop_capturer (PipeWire portal).
- `crates/reflux-net`, `crates/reflux-store` — empty stubs (Cargo.toml only).
- `assets/` — fonts (TTF converted from woff2), theme JSON, media (logo SVGs, default avatars,
  emoji data/spritesheet, sounds).
- `tools/gen_theme.py`, `tools/gen_icons.py` — regenerate palette/icons.

## Planned architecture / next steps
1. Finish `reflux-model`; then `reflux-net`: instance discovery (`/.well-known/fluxer`), REST client
   with auth + rate limits + ALTCHA, gateway (`?v=1&encoding=json&compress=zstd-stream&stream=1`,
   one zstd decompressor per connection, heartbeat/resume per gateway/overview.md).
2. `reflux-store`: cache applying dispatches (guilds, channels, members, messages, read states,
   presences, voice states), permission calc.
3. App: tokio runtime on a background thread; `rune::App` + `AppHandle::update` to push events
   into UI state; images fetched with reqwest and decoded to RGBA (`ImageCache::rgba`), media proxy
   supports `format=png`.
4. UI kit in `crates/reflux/src/ui` matching Fluxer CSS (button, input, avatar+status, modal,
   tooltip, in-app context menu — Rune's native popup menu is macOS-only, popover, switch, slider,
   scroller), then screens: auth (login/register/MFA/instance picker, layout in
   `login-layout.json`), guild list, channel sidebar, chat (virtual rows), composer, member list,
   user area, settings, voice UI.
5. Known Rune limitation: no custom (frameless) titlebar on Windows/Linux — keep system titlebar.

## Uncommitted work (as of handoff)
`git status`: modified `crates/reflux-model/{Cargo.toml,src/lib.rs}`; untracked
`crates/reflux-model/src/snowflake.rs`, `crates/reflux/src/ui/mod.rs`, `assets/media/`, `HANDOFF.md`.
Commit `assets/media` + `HANDOFF.md` as-is; commit the model/ui work once it builds.

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
