//! The desktop shell around the app, as `fluxer_desktop` provides it: message notifications and
//! sounds, the tray icon, the dock badge / urgency hint, close-to-tray, `fluxer://` links and
//! single-instance forwarding.

pub mod link;
pub mod notify;
pub mod sound;
mod tray;

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
}

impl Default for Prefs {
    /// `DesktopConfig.ts` defaults.
    fn default() -> Self {
        Self { notifications: true, sounds: true, tray: true, close_to_tray: true }
    }
}

/// Runtime state of the desktop integration.
#[derive(Default)]
pub struct Desktop {
    /// The window has keyboard focus (`on_focus`).
    pub focused: bool,
    tray: Option<rune::tray::Tray>,
    tray_look: Option<tray::Look>,
    /// Last (mention count, unread) sent to the dock.
    badge: Option<(u32, bool)>,
    /// Last seen self mute / deaf, for their sounds.
    voice_self: Option<(bool, bool)>,
    /// Others in our voice channel last frame, for join / leave sounds.
    peers: Option<(Snowflake, HashSet<Snowflake>)>,
    /// Messages already alerted (`notifiedMessageIds`, an LRU of 500).
    notified: VecDeque<Snowflake>,
    /// A launch link waiting for READY.
    pending_link: Option<link::Link>,
}

pub fn play(s: &State, sound: Sound) {
    if s.app.saved.desktop.sounds {
        sound::play(sound);
    }
}

/// Startup in a windowed run: forwards to a running instance (then exits) or starts listening
/// for later launches, and remembers a launch link for after sign-in.
pub fn start(ui: &rune::AppHandle<State>) {
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

/// A `fluxer://` link from a launch: kept until the shell is up.
pub fn open_link(s: &mut State, url: &str) {
    let Some(l) = link::parse(url) else { return eprintln!("reflux: ignoring link {url:?}") };
    s.desktop.pending_link = Some(l);
    if s.app.route == Route::Main {
        ready(s);
    }
}

/// The shell is up (READY): follow a link that arrived before it.
pub fn ready(s: &mut State) {
    let Some(l) = s.desktop.pending_link.take() else { return };
    let sh = &mut s.app.shell;
    match l {
        link::Link::Invite(code) => {
            sh.form = Default::default();
            sh.form.text.insert("invite".into(), rune::TextEdit::new(code));
            sh.dialog = Some(crate::guild::Dialog::Add(crate::guild::AddView::Join));
        }
        link::Link::Channel { guild, channel } => sh.select_guild(guild, Some(channel)),
    }
}

/// After a dispatch was applied: alerts for new messages and calls.
pub fn on_dispatch(s: &mut State, d: &DispatchEvent) {
    match d {
        DispatchEvent::MessageCreate(m) => on_message(s, m),
        DispatchEvent::CallCreate(c) | DispatchEvent::CallUpdate(c) if c.ringing.contains(&s.app.store.my_id()) => {
            play(s, Sound::IncomingRing)
        }
        _ => {}
    }
}

/// `Notification.handleMessageCreate`.
fn on_message(s: &mut State, m: &Message) {
    let store = &s.app.store;
    let viewing = s.app.shell.channel == Some(m.channel_id) && s.desktop.focused;
    if viewing || s.desktop.notified.contains(&m.id) || !store.should_notify(m) {
        return; // the same-channel sound is off by default in Fluxer
    }
    s.desktop.notified.push_back(m.id);
    if s.desktop.notified.len() > 500 {
        s.desktop.notified.pop_front();
    }
    play(s, Sound::Message);
    attention(s);
    if !s.app.saved.desktop.notifications || rune::is_headless() {
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

/// Every frame: dock badge, tray icon and menu, and the voice sounds that follow state changes.
pub fn tick(s: &mut State) {
    let main = s.app.route == Route::Main;
    let store = &s.app.store;
    // `UnreadBadge`: total mentions across guilds and DMs, plus unread DMs.
    let mentions: u32 = if main {
        store
            .channels
            .values()
            .filter(|c| c.kind != ChannelType::GUILD_CATEGORY)
            .map(|c| store.mention_count(c.id))
            .sum()
    } else {
        0
    };
    let badge = (mentions, mentions > 0 && !s.desktop.focused);
    if s.desktop.badge != Some(badge) {
        s.desktop.badge = Some(badge);
        set_badge(badge.0, badge.1);
    }
    sync_tray(s, mentions > 0);
    voice_sounds(s);
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
            "mute" => app::update(|s| s.app.shell.toggle_mute()),
            "deafen" => app::update(|s| s.app.shell.toggle_deaf()),
            "disconnect" => app::update(|s| s.app.shell.leave_voice()),
            "quit" => app::quit(),
            _ => {}
        },
        _ => {}
    }
}

/// `App::on_close`: hide to the tray when it shows and close-to-tray is on, else quit.
pub fn on_close(s: &mut State, tx: &mut rune::Tx) {
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

/// Unity LauncherEntry: the count on the dock icon (KDE, Plank, GNOME's Dash to Dock) and its
/// urgency, as Electron's `app.setBadgeCount` does on Linux.
#[cfg(target_os = "linux")]
fn set_badge(count: u32, urgent: bool) {
    if rune::is_headless() {
        return;
    }
    std::thread::spawn(move || {
        use std::collections::HashMap;
        use zbus::zvariant::Value;
        let r = (|| -> zbus::Result<()> {
            let conn = zbus::blocking::Connection::session()?;
            let props: HashMap<&str, Value> =
                HashMap::from([("count", Value::from(count as i64)), ("count-visible", Value::from(count > 0)), ("urgent", Value::from(urgent))]);
            let uri = format!("application://{}.desktop", notify::DESKTOP_ID);
            conn.emit_signal(None::<&str>, "/com/canonical/unity/launcherentry/reflux", "com.canonical.Unity.LauncherEntry", "Update", &(uri, props))
        })();
        if let Err(e) = r {
            eprintln!("reflux: dock badge: {e}");
        }
    });
}

// shortcut: no taskbar overlay (Windows) or Dock badge (macOS) yet.
#[cfg(not(target_os = "linux"))]
fn set_badge(_count: u32, _urgent: bool) {}
