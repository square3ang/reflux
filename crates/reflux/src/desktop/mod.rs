//! The desktop shell around the app, as `fluxer_desktop` provides it: message notifications and
//! sounds, the tray icon, the dock badge / urgency hint, close-to-tray, `fluxer://` links and
//! single-instance forwarding; launch at login, window size and zoom, spellcheck, global
//! shortcuts, the updater, nagbars and toasts, and streaming privacy.

pub mod badge;
mod hooks;
pub mod autostart;
pub mod link;
pub mod menu;
pub mod nagbar;
pub mod nagbar_sync;
pub mod notices;
pub mod notify;
pub mod os_idle;
pub mod shortcuts;
pub mod sound;
pub mod spell;
pub mod streamer;
pub mod time;
pub mod top_bars;
pub mod tts;
mod tray;
pub mod updater;
pub mod window;
#[cfg(target_os = "linux")]
mod wl_drop;

use std::collections::{HashSet, VecDeque};

use reflux_model::{ChannelType, DispatchEvent, Message, Snowflake};
use rune::tray::TrayEvent;
use serde::{Deserialize, Serialize};

pub use sound::Sound;

use crate::State;
use crate::app::{self, Route};

/// Local desktop preferences (Fluxer keeps these per device too, outside user settings).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Desktop notifications for messages.
    pub notifications: bool,
    /// UI sounds (messages, mute, voice joins, ringing).
    pub sounds: bool,
    /// `showTrayIcon`.
    pub tray: bool,
    /// `closeToTray`: closing the window hides it while the tray icon shows.
    pub close_to_tray: bool,
    /// Launch at login (`Autostart.ts`); the entry itself is the source of truth at startup.
    pub autostart: bool,
    /// `startMinimized`: a launch at login stays in the tray.
    pub start_minimized: bool,
    /// `rememberWindowState`, and the size last seen (logical points).
    pub remember_window: bool,
    pub window: Option<[f32; 2]>,
    /// `Accessibility:zoomLevel` (0.5..=2).
    pub zoom: f32,
    /// Spellcheck in the composer, its dictionaries (`dictionary-*` package names; empty = the
    /// interface language's) and the user's own words.
    pub spellcheck: bool,
    /// "Detect while typing" (`Spellcheck.autoDetect`); off, `spell_langs` are used.
    pub spell_auto: bool,
    pub spell_langs: Vec<String>,
    pub words: Vec<String>,
    /// System-wide mute / deafen / push-to-talk shortcuts.
    pub global_shortcuts: bool,
    /// The guild (`None` = home) and channel open last, reopened on start.
    pub last: Option<(Option<Snowflake>, Snowflake)>,
    pub streamer: streamer::Prefs,
    /// Appearance, accessibility, chat input, sounds and quick CSS.
    pub look: crate::look::Prefs,
    /// The update version whose bar was dismissed, and "Disable automatic updates" (no
    /// background check).
    pub skipped_update: Option<String>,
    pub no_auto_update: bool,
    /// Nagbars closed for good: `notice:{id}`, `maintenance:{id}:{status}`, `premium-*`.
    pub dismissed_nagbars: Vec<String>,
    /// Per-account preferences (favorites, keybinds), by user ID.
    pub accounts: std::collections::HashMap<String, crate::nav::AccountPrefs>,
    /// Channels opened, newest first (`SelectedChannel.recentVisits`, the quick switcher's
    /// "Previous channels").
    pub recent: Vec<Snowflake>,
    /// Message searches by scope (guild, or the DM channel), newest first (`SearchHistory`).
    pub search_history: std::collections::HashMap<Snowflake, Vec<String>>,
    /// `ThreadPanelWidth` (None = the default 450).
    pub thread_panel_width: Option<f32>,
    /// `UI_CHANNEL_PINS_POPOUT_SIZE` (None = the default 480×640).
    pub pins_popout_size: Option<(f32, f32)>,
    /// Users who chose "Not now" on the recovery kit reminder (`RecoveryKitReminderDismissed:<id>`).
    pub recovery_reminder_dismissed: std::collections::HashSet<Snowflake>,
    /// Voice message playback speed (`fluxer:media:playbackRates`, voice kind).
    pub voice_message_rate: f32,
    /// Media player volume (0..1) and mute (`fluxer:media:volume` / `:muted`), shared by players.
    pub media_volume: f32,
    pub media_muted: bool,
    /// `ReverseImageSearch.engines` (kept on this device like Fluxer's): the enabled engine ids,
    /// and the custom engines as (id, name, URL template with `{url}`). The default is synced
    /// (`shell::expr::image_search`).
    pub ris_enabled: Vec<String>,
    pub ris_custom: Vec<(String, String, String)>,
    /// `SearchEngine.engines` and `Translation.engines` (`shell::search_providers`): enabled ids
    /// and custom (id, name, URL template with `{query}`); their defaults are synced.
    pub search_enabled: Vec<String>,
    pub search_custom: Vec<(String, String, String)>,
    pub translators_enabled: Vec<String>,
    pub translators_custom: Vec<(String, String, String)>,
    /// Developer tools overrides (Fluxer's persisted `DeveloperOptions`, kept on this device).
    pub dev: crate::shell::devtools::Options,
    /// `VoiceSessionRestore`'s snapshot, and `UnclaimedNagbarSnooze` by user (unix ms).
    pub voice_restore: Option<top_bars::VoiceSnapshot>,
    pub unclaimed_snoozed_until: std::collections::HashMap<Snowflake, u64>,
    /// Users offered a passkey after claiming (`fluxer.authenticator_offer_dismissed:<id>`).
    pub authenticator_offered: std::collections::HashSet<Snowflake>,
    /// `CompactVoiceCallHeight`: remembered compact call heights per channel (`heightsByKey`).
    pub compact_call_heights: std::collections::HashMap<Snowflake, f32>,
    /// `VoicePrompts.skipHideOwnCameraConfirm`.
    pub skip_hide_own_camera_confirm: bool,
    /// `incoming_call_overlay_position`: where the incoming call card was dragged to.
    pub incoming_call_pos: Option<(f32, f32)>,
    /// The screen share PiP's corner: bit 1 right, bit 2 bottom (`None` = bottom-right).
    pub pip_corner: Option<u8>,
    /// The PiP overlay's width in points (240 .. 70% of the window); `None` = 320.
    pub pip_width: Option<f32>,
}

impl Default for Prefs {
    /// `DesktopConfig.ts` defaults.
    fn default() -> Self {
        Self {
            notifications: true,
            sounds: true,
            tray: true,
            close_to_tray: true,
            autostart: false,
            start_minimized: false,
            remember_window: true,
            window: None,
            zoom: 1.0,
            spellcheck: true,
            spell_auto: true,
            spell_langs: Vec::new(),
            words: Vec::new(),
            global_shortcuts: true,
            last: None,
            streamer: Default::default(),
            look: Default::default(),
            skipped_update: None,
            no_auto_update: false,
            dismissed_nagbars: Vec::new(),
            accounts: Default::default(),
            recent: Vec::new(),
            search_history: Default::default(),
            thread_panel_width: None,
            pins_popout_size: None,
            recovery_reminder_dismissed: Default::default(),
            voice_message_rate: 1.0,
            media_volume: 1.0,
            media_muted: false,
            ris_enabled: vec!["google_lens".into()],
            ris_custom: Vec::new(),
            search_enabled: vec!["google".into()],
            search_custom: Vec::new(),
            translators_enabled: vec!["google_translate".into()],
            translators_custom: Vec::new(),
            dev: Default::default(),
            voice_restore: None,
            unclaimed_snoozed_until: Default::default(),
            authenticator_offered: Default::default(),
            compact_call_heights: Default::default(),
            skip_hide_own_camera_confirm: false,
            incoming_call_pos: None,
            pip_corner: None,
            pip_width: None,
        }
    }
}

/// What the shell's views show of the desktop side, mirrored into `ShellState` each frame;
/// toasts and the spelling menu live here because shell handlers push them.
#[derive(Clone, Debug, Default)]
pub struct ShellView {
    /// The window lost keyboard focus (pauses our own stream preview).
    pub unfocused: bool,
    /// The gateway is reconnecting.
    pub offline: bool,
    pub update: Option<updater::Available>,
    /// Streaming privacy is on (manually or detected).
    pub streamer: bool,
    pub toasts: Vec<nagbar::Toast>,
    /// Discovery's instance notice, and the status page's maintenance.
    pub notice: Option<reflux_model::InstanceNotice>,
    pub maintenance: Option<notices::Maintenance>,
    /// The misspelling the composer's context menu is about: (byte range, word).
    pub spell_menu: Option<(std::ops::Range<usize>, String)>,
    /// The textarea menu belongs to the thread panel's composer, not the main one.
    pub menu_pane: bool,
    /// The window's width (0 before the first layout).
    pub width: f32,
}

/// Desktop requests from shell handlers (`Intent::Desktop`).
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Download and install the update (AppImage), or open its page.
    Update,
    /// Restart into the installed update.
    Restart,
    OpenUrl(String),
}

/// `--shell-demo --desktop=bars|spell|toast|streamer`: nagbars, the spelling menu over a
/// composer draft (with a tiny inline dictionary), a toast, masked names.
pub fn demo(sh: &mut crate::shell::ShellState, store: &mut reflux_store::Store, scene: &str) {
    match scene {
        "bars" => {
            sh.desktop.offline = true;
            sh.desktop.update = Some(updater::Available { version: "0.2.0".into(), page: String::new(), appimage: Some(Default::default()), stage: updater::Stage::Available });
            sh.desktop.streamer = true;
        }
        "spell" => {
            let dict = spellbook::Dictionary::new("SET UTF-8\n", "6\nthis\nis\na\ntest\nof\nspelling\n").expect("demo dictionary");
            spell::insert("dictionary-en", dict);
            sh.composer = rune::TextEdit::new("this is a tset of speling ");
            sh.composer.set_caret(12);
            nagbar::open_spell_menu(sh, [560.0, 640.0], false);
        }
        "toast" => toast(&mut sh.desktop, "Added \"Fluxer\" to dictionary", true),
        "streamer" => {
            sh.desktop.streamer = true;
            store.mask_names = true;
        }
        _ => {}
    }
}

/// Pushes a toast (`ToastCommands.createToast`).
pub fn toast(view: &mut ShellView, text: impl Into<String>, success: bool) {
    view.toasts.push(nagbar::Toast { text: text.into(), success, at: std::time::Instant::now() });
}

/// Runtime state of the desktop integration.
#[derive(Default)]
pub struct Desktop {
    /// The window has keyboard focus (`on_focus`).
    pub focused: bool,
    tray: Option<rune::tray::Tray>,
    tray_look: Option<tray::Look>,
    /// Last (badge, urgent) sent to the dock / taskbar.
    badge: Option<(i32, bool)>,
    /// Last seen self mute / deaf, for their sounds.
    voice_self: Option<(bool, bool)>,
    /// Others in our voice channel last frame, for join / leave sounds.
    peers: Option<(Snowflake, HashSet<Snowflake>)>,
    /// Messages already alerted (`notifiedMessageIds`, an LRU of 500).
    notified: VecDeque<Snowflake>,
    /// A launch link waiting for READY.
    pending_link: Option<link::Link>,
    /// The window's size (root layout), and when it last changed while not yet saved.
    pub size: [f32; 2],
    size_changed: Option<std::time::Instant>,
    title: String,
    pub(crate) update: Option<updater::Available>,
    /// OBS / XSplit is running (`StreamerModeProcessDetection.ts`).
    pub capture_app: bool,
    streamer_polled: Option<std::time::Instant>,
    pub update_checked: Option<std::time::Instant>,
    pub maintenance: Option<notices::Maintenance>,
    /// The status page poll: when it's next due, failures in a row, and a page that answered 4xx.
    pub status_next: Option<std::time::Instant>,
    pub status_failures: u32,
    pub status_unsupported: Option<String>,
}

/// `Sound.playSound`: unless all sounds, this sound or streaming privacy mutes it; the user's
/// custom file and master volume apply.
pub fn play(s: &State, sound: Sound) {
    let p = &s.app.saved.desktop;
    if p.sounds && p.look.sound_on(sound.id()) && !streamer::mute_sounds(&p.streamer, s.desktop.capture_app) {
        sound::play(sound, p.look.sound_gain(sound.id()), p.look.custom_sounds.get(sound.id()).map(String::as_str));
    }
}

/// After the saved session loads: zoom, spellcheck, and the autostart
/// preference following the entry on disk (the user may have removed it).
pub fn boot(s: &mut State) {
    let p = &mut s.app.saved.desktop;
    window::set_zoom(p.zoom);
    crate::look::set(&p.look);
    spell::configure(p);
    if !rune::is_headless() && p.autostart != autostart::is_enabled() {
        p.autostart = !p.autostart;
        crate::session::save(&s.app.saved);
    }
}

/// Startup in a windowed run. macOS: forwards to a running instance (then exits) or starts
/// listening for later launches, and remembers a launch link for after sign-in. Windows and
/// Linux do this through Rune (`rune::single_instance`, `on_open_url`, `on_reopen`).
pub fn start(ui: &rune::AppHandle<State>) {
    link::register_protocol();
    // shortcut: macOS keeps our socket; drop it once a bundle (`CFBundleURLTypes`) hands links to `on_open_url`.
    #[cfg(target_os = "macos")]
    {
        let link = link::from_args();
        if link::forward(link.as_deref()) {
            std::process::exit(0);
        }
        let handle = ui.clone();
        link::listen(move |token, link| {
            match token {
                Some(t) => handle.activate(t),
                None => handle.open(),
            }
            if let Some(l) = link {
                handle.update(move |s, _| open_link(s, &l));
            }
        });
        if let Some(l) = link {
            ui.update(move |s, _| open_link(s, &l));
        }
    }
    #[cfg(target_os = "linux")]
    wl_drop::set_app(ui.clone());
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = ui;
}

/// `App::on_reopen`, a later launch without a link: Rune shows a hidden window; one that's
/// already showing is brought forward (Fluxer's `second-instance` focuses it). Windows' default
/// raises it itself.
pub fn on_reopen(_: &mut State, has_visible: bool, _: &mut rune::Tx) -> bool {
    if cfg!(target_os = "linux") && has_visible {
        app::show_window();
        return true;
    }
    false
}

/// A `fluxer://` link from a launch: kept until the shell is up.
pub fn open_link(s: &mut State, url: &str) {
    let Some(l) = link::parse(url) else { return eprintln!("reflux: ignoring link {url:?}") };
    if let link::Link::Web(path) = &l {
        // Signed in or not, on the saved instance's web app.
        let base = s.app.instance.as_ref().map_or_else(|| format!("https://{}", s.app.saved.instance), |i| i.discovery.endpoints.webapp.clone());
        return crate::settings::open_url(&format!("{}{path}", base.trim_end_matches('/')));
    }
    s.desktop.pending_link = Some(l);
    if s.app.route == Route::Main {
        ready(s);
    }
}

/// The shell is up (READY): follow a link that arrived before it, else reopen the last channel.
pub fn ready(s: &mut State) {
    let Some(l) = s.desktop.pending_link.take() else { return reopen(s) };
    let sh = &mut s.app.shell;
    match l {
        link::Link::Invite(code) => {
            sh.form = Default::default();
            sh.form.text.insert("invite".into(), rune::TextEdit::new(code));
            sh.dialog = Some(crate::guild::Dialog::Add(crate::guild::AddView::Join));
        }
        link::Link::Channel { guild, channel } => sh.select_guild(guild, Some(channel)),
        link::Link::OAuth2(params) => crate::nav::oauth::open(sh, params),
        link::Link::Gift(code) => crate::settings::open_gift(sh, code),
        link::Link::Web(_) => {}
    }
}

/// After a dispatch was applied: alerts for new messages and calls.
pub fn on_dispatch(s: &mut State, d: &DispatchEvent) {
    match d {
        DispatchEvent::MessageCreate(m) => {
            tts::on_message(s, m);
            on_message(s, m)
        }
        DispatchEvent::CallCreate(c) | DispatchEvent::CallUpdate(c) if c.ringing.contains(&s.app.store.my_id()) => {
            play(s, Sound::IncomingRing)
        }
        DispatchEvent::MessageDelete(d) => tts::on_delete(d.channel_id, d.id),
        _ => {}
    }
}

/// `Notification.handleMessageCreate`.
fn on_message(s: &mut State, m: &Message) {
    let store = &s.app.store;
    let viewing = s.app.shell.channel == Some(m.channel_id) && s.desktop.focused;
    if s.desktop.notified.contains(&m.id) || !store.should_notify(m) {
        return;
    }
    if viewing {
        // Off by default in Fluxer; on, it plays for messages in the open, focused channel.
        if m.author.id != store.my_id() {
            play(s, Sound::SameChannelMessage);
        }
        return;
    }
    s.desktop.notified.push_back(m.id);
    if s.desktop.notified.len() > 500 {
        s.desktop.notified.pop_front();
    }
    let dm = store.channels.get(&m.channel_id).is_none_or(|c| c.guild_id.is_none());
    play(s, if dm { Sound::DirectMessage } else { Sound::Message });
    attention(s);
    if !s.app.saved.desktop.notifications || streamer::pause_notifications(&s.app.saved.desktop.streamer, s.desktop.capture_app) || rune::is_headless() {
        return;
    }
    let (title, body) = (notify::title(store, m), notify::body(store, m));
    let (channel, guild) = (m.channel_id, store.channels.get(&m.channel_id).and_then(|c| c.guild_id));
    let on_click = move || {
        app::show_window();
        app::update(move |s| s.app.shell.select_guild(guild, Some(channel)));
    };
    let base = s.app.shell.media.base();
    let avatar = m.author.avatar.clone().filter(|_| !base.is_empty());
    let user = m.author.id;
    app::spawn(async move {
        let image = match avatar {
            Some(hash) => {
                let url = crate::media::avatar_url(&base, user, &hash, 128.0, 1.0);
                notify::avatar_file(reflux_net::http_client(), url, user, hash).await
            }
            None => None,
        };
        notify::show(title, body, image.or_else(app_icon), on_click);
    });
}

/// Fluxer's app icon as a file, the notification image when the author has no avatar.
fn app_icon() -> Option<std::path::PathBuf> {
    let path = dirs::cache_dir()?.join("reflux/fluxer.png");
    if !path.exists() {
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, include_bytes!("../../../../assets/media/images/app-icon.png")).ok()?;
    }
    Some(path)
}

/// Urgency while unfocused (`flashFrame`): Wayland marks the window urgent on a tokenless focus
/// request. X11 would steal focus that way, so it relies on the dock badge's `urgent` instead.
fn attention(s: &State) {
    if !s.desktop.focused && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        app::request_attention();
    }
}

/// `LocationState`: the channel open last, if it still exists.
pub(crate) fn reopen(s: &mut State) {
    let Some((guild, channel)) = s.app.saved.desktop.last else { return };
    let sh = &mut s.app.shell;
    if sh.channel.is_none() && s.app.store.channels.contains_key(&channel) && guild.is_none_or(|g| s.app.store.guilds.contains_key(&g)) {
        sh.select_guild(guild, Some(channel));
    }
}

/// Every frame: dock badge, tray icon and menu, and the voice sounds that follow state changes.
pub fn tick(s: &mut State, tx: &mut rune::Tx) {
    // `SkeletonLayoutMemory`: save the shapes this account is drawing (only when they change).
    crate::shell::skeleton_memory::capture(
        &s.app.store,
        s.app.saved.current.as_ref().map(|a| a.user_id.as_str()),
        s.app.shell.guild,
        s.app.shell.channel,
    );
    window::tick(s, tx);
    streamer::tick(s, tx);
    updater::tick(s);
    tts::on_channel(s.app.shell.channel);
    notices::tick(s);
    // The portal pre-fills the user's own keys, which are kept per account: it starts once the
    // account is known (at boot they were Fluxer's defaults, and KDE keeps what it was first given).
    if !shortcuts::started() && !rune::is_headless() && s.app.shell.account.me.0 != 0 {
        shortcuts::start(s.app.saved.desktop.global_shortcuts, shortcuts::preferred_triggers(&s.app.shell));
    }
    shortcuts::sync(shortcuts::bindings(&s.app.shell));
    let prefs = &mut s.app.saved.desktop;
    let here = s.app.shell.channel.map(|c| (s.app.shell.guild, c));
    if s.app.route == Route::Main && here.is_some() && prefs.last != here && !rune::is_headless() {
        prefs.last = here;
        if let Some((_, c)) = here {
            prefs.recent.retain(|&x| x != c);
            prefs.recent.insert(0, c);
            prefs.recent.truncate(8);
        }
        crate::session::save(&s.app.saved);
    }
    time::set_format(s.app.store.settings.time_format);
    let v = &mut s.app.shell.desktop;
    v.offline = s.app.route == Route::Main && matches!(s.app.connection, Some(reflux_net::ConnectionState::Reconnecting(_) | reflux_net::ConnectionState::Connecting));
    v.update = s.desktop.update.clone().filter(|u| s.app.saved.desktop.skipped_update.as_ref() != Some(&u.version));
    v.width = s.desktop.size[0];
    v.notice = s.app.instance.as_ref().and_then(|i| i.discovery.notice.clone());
    v.maintenance = s.desktop.maintenance.clone();
    v.streamer = streamer::enabled(&s.app.saved.desktop.streamer, s.desktop.capture_app);
    v.toasts.retain(|t| t.at.elapsed() < nagbar::TOAST_TIME);
    if !v.toasts.is_empty() {
        tx.wake_after(std::time::Duration::from_millis(250));
    }
    let main = s.app.route == Route::Main;
    let store = &s.app.store;
    // `UnreadBadge`: total mentions across guilds and DMs, plus unread DMs, plus what the other
    // signed-in accounts' background engines report (`AppBadgeCount.ts` + `otherAccounts*`).
    let mut mentions: u32 = if main {
        store
            .channels
            .values()
            .filter(|c| c.kind != ChannelType::GUILD_CATEGORY)
            .map(|c| store.mention_count(c.id))
            .sum()
    } else {
        0
    };
    let mut unread = main && store.channels.values().any(|c| store.has_unread(c.id) && !store.is_muted(c));
    mentions = mentions.saturating_add(s.app.accounts.mentions());
    unread |= s.app.accounts.any_unread();
    let requests = if main { store.relationships.values().filter(|r| r.kind == reflux_model::RelationshipType::INCOMING_REQUEST).count() as u32 } else { 0 };
    let badge = (badge::compute(mentions, requests, unread, s.app.saved.desktop.look.unread_badge), mentions > 0 && !s.desktop.focused);
    if s.desktop.badge != Some(badge) {
        s.desktop.badge = Some(badge);
        badge::set(badge.0, badge.1);
    }
    sync_tray(s, mentions > 0);
    voice_sounds(s);
    let title = if s.app.saved.desktop.look.unread_badge { window::title(&s.app.store, &s.app.shell, mentions, unread) } else { window::title(&s.app.store, &s.app.shell, 0, false) };
    if s.desktop.title != title {
        tx.set_title(title.clone());
        s.desktop.title = title;
    }
}

/// A changed preference that acts outside the app (`Intent::Prefs`).
pub fn prefs_changed(old: &Prefs, new: &Prefs) {
    if old.autostart != new.autostart
        && let Err(e) = autostart::set(new.autostart)
    {
        eprintln!("reflux: autostart: {e}");
    }
    if old.global_shortcuts != new.global_shortcuts {
        shortcuts::set_enabled(new.global_shortcuts);
    }
    if old.spell_langs != new.spell_langs || old.words != new.words || old.spellcheck != new.spellcheck {
        spell::configure(new);
    }
    window::set_zoom(new.zoom);
    crate::look::set(&new.look);
}

/// Runs an [`Intent::Desktop`](crate::shell::Intent::Desktop).
pub fn run(s: &mut State, r: Request, tx: &mut rune::Tx) {
    match r {
        Request::Update => updater::install(s),
        Request::Restart => updater::restart(),
        Request::OpenUrl(u) => {
            if let Err(e) = tx.open_url(&u) {
                eprintln!("reflux: open {u}: {e:?}");
            }
        }
    }
}

fn sync_tray(s: &mut State, dot: bool) {
    if rune::is_headless() {
        return;
    }
    let v = &s.app.shell.voice;
    let look = tray::Look { dot, visible: app::window_visible(), in_voice: v.channel.is_some(), mute: v.self_mute, deaf: v.self_deaf };
    if !s.app.saved.desktop.tray {
        (s.desktop.tray, s.desktop.tray_look) = (None, None);
        return;
    }
    let old = s.desktop.tray_look.replace(look.clone());
    match (&s.desktop.tray, old) {
        // The icon can't change in place, so a new dot makes a new tray item.
        (Some(t), Some(o)) if o.dot == look.dot => {
            if o != look {
                t.set_menu(tray::menu(&look));
            }
        }
        _ => {
            s.desktop.tray = None; // drop the old item first
            s.desktop.tray = tray::create(&look, on_tray);
        }
    }
}

/// Runs on the tray's thread.
fn on_tray(e: TrayEvent) {
    match e {
        TrayEvent::Activate { token } => app::show_window_with(token),
        TrayEvent::Menu(id) => match id.as_str() {
            "toggle" if app::window_visible() => app::hide_window(),
            "toggle" => app::show_window(),
            "settings" => {
                app::show_window();
                app::update(|s| s.app.shell.open_settings(None));
            }
            // `update` applies at once but a frame still has to run to send the voice state
            // and rebuild this menu; unfocused, none comes on its own, so ask for one.
            "mute" => app::update_tx(|s, tx| {
                s.app.shell.toggle_mute();
                tx.wake_after(std::time::Duration::ZERO);
            }),
            "deafen" => app::update_tx(|s, tx| {
                s.app.shell.toggle_deaf();
                tx.wake_after(std::time::Duration::ZERO);
            }),
            "disconnect" => app::update_tx(|s, tx| {
                s.app.shell.leave_voice();
                tx.wake_after(std::time::Duration::ZERO);
            }),
            "quit" => app::quit(),
            _ => {}
        },
        _ => {}
    }
}

/// `App::on_quit`: quitting ends the process, as Electron's `app.quit()` does. The call is hung
/// up first, so a teardown that hangs can't leave it playing in a windowless process.
// shortcut: a deadline (`_exit`, skipping the atexit handlers and C++ destructors that hung), not
// a fix of whichever call teardown hangs; Rune logs where the quit was stuck.
pub fn on_quit(s: &mut State, _: &mut rune::Tx) {
    app::stop_for_crash(s);
    rune::exit_deadline(std::time::Duration::from_secs(3));
}

/// `App::on_close`: hide to the tray when it shows and close-to-tray is on, else quit.
pub fn on_close(s: &mut State, tx: &mut rune::Tx) {
    window::save_size(s);
    if !(s.app.saved.desktop.close_to_tray && s.desktop.tray.is_some()) {
        tx.quit();
    }
}

fn voice_sounds(s: &mut State) {
    let v = &s.app.shell.voice;
    let now = (v.self_mute, v.self_deaf);
    let joined = v.channel.filter(|_| v.connected).map(|(_, c)| c);
    if let Some((mute, deaf)) = s.desktop.voice_self.replace(now) {
        let sound = match (deaf != now.1, mute != now.0) {
            (true, _) => Some(if now.1 { Sound::Deaf } else { Sound::Undeaf }),
            (false, true) => Some(if now.0 { Sound::Mute } else { Sound::Unmute }),
            _ => None,
        };
        if let Some(snd) = sound {
            play(s, snd);
        }
    }
    let me = s.app.store.my_id();
    let peers = joined.map(|c| {
        let users = s.app.store.voice_states.iter().filter(|(_, vs)| vs.channel_id == Some(c)).map(|((_, u), _)| *u).filter(|&u| u != me);
        (c, users.collect::<HashSet<_>>())
    });
    let old = std::mem::replace(&mut s.desktop.peers, peers.clone());
    let snd = match (old, &peers) {
        (None, Some(_)) => Some(Sound::UserJoin), // we joined
        (Some(_), None) => Some(Sound::VoiceDisconnect),
        (Some((oc, o)), Some((nc, n))) if oc == *nc => {
            if n.difference(&o).next().is_some() {
                Some(Sound::UserJoin)
            } else if o.difference(n).next().is_some() {
                Some(Sound::UserLeave)
            } else {
                None
            }
        }
        (Some(_), Some(_)) => Some(Sound::UserJoin), // moved channels
        (None, None) => None,
    };
    if let Some(snd) = snd {
        play(s, snd);
    }
}
