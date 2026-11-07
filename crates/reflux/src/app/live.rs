//! `--live[=SCENE]` (with `--snap`): a headless snapshot of the real signed-in UI. Read-only mode
//! is forced. Rune's `setup` blocks (real time, bounded) applying the runtime's updates itself
//! until READY and the scene's data are in, and the media loader fetches images synchronously so
//! the first frame has them. Scenes: `guild` (default), `members`, `dm`, `settings`, `home`,
//! `pins`, `search` (`REFLUX_LIVE_QUERY`, default `has:image`) and `jump` (to a message ~100
//! back, loaded around it, then scrolled to on the first frame, or `REFLUX_LIVE_JUMP_FRAMES` later;
//! snap with `--at 1`).
//! `write` and `cleanup` test writes in `REFLUX_WRITE_CHANNEL` only (see [`write`]).
//! `REFLUX_LIVE_GUILD=N` picks the rail's Nth guild, `REFLUX_LIVE_TAB` the settings tab.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use rune::{Image, ImageCache};

use super::{Route, poll};

mod voice;
mod write;
use crate::State;

type Update = Box<dyn FnOnce(&mut State) + Send>;

const TIMEOUT: Duration = Duration::from_secs(30);
/// For `write` and `cleanup`, whose steps each time out on their own.
const WRITE_TIMEOUT: Duration = Duration::from_secs(300);

/// The sender for `app::update`, and the receiver until [`wait`] takes it.
type Capture = (Sender<Update>, Option<Receiver<Update>>);

static CAPTURE: Mutex<Option<Capture>> = Mutex::new(None);

/// A scroll request held for a number of polls (frames), as if the user clicked after mount.
static DELAYED: Mutex<Option<(String, u32)>> = Mutex::new(None);

/// From `app::poll`: hands a held scroll request to the shell once its frame comes.
pub(super) fn on_poll(s: &mut State, tx: &mut rune::Tx) {
    let mut d = DELAYED.lock().unwrap();
    match d.as_mut() {
        Some((_, n)) if *n > 0 => {
            *n -= 1;
            tx.wake_after(Duration::ZERO);
        }
        Some(_) => s.app.shell.scroll_to = d.take().map(|(k, _)| k),
        None => {}
    }
}

pub fn scene() -> Option<&'static str> {
    rune::extra_args().iter().find_map(|a| match a.as_str() {
        "--live" => Some("guild"),
        a => a.strip_prefix("--live="),
    })
}

/// Before `app::start`: route runtime updates to [`wait`] instead of the app handle.
pub fn capture() {
    let (tx, rx) = channel();
    *CAPTURE.lock().unwrap() = Some((tx, Some(rx)));
}

/// `app::update` while capturing; gives `f` back otherwise.
pub(super) fn send(f: Update) -> Result<(), Update> {
    match CAPTURE.lock().unwrap().as_ref() {
        Some((tx, _)) => tx.send(f).map_err(|e| e.0),
        None => Err(f),
    }
}

/// The media loader's fixture in live runs: a blocking fetch on the network runtime.
pub fn fetch_image(cache: &ImageCache, url: &str) -> Option<Image> {
    static HTTP: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    let http = HTTP.get_or_init(reflux_net::http_client);
    let bridge = super::BRIDGE.get()?;
    let bytes = bridge.rt.block_on(async { http.get(url).send().await?.error_for_status()?.bytes().await });
    let img = crate::media::decode(cache, bytes.ok()?.to_vec()).ok()?;
    let _ = img.wait();
    Some(img)
}

/// From `setup`: applies updates until the scene is ready (or [`TIMEOUT`]), then hands updates
/// back to the app handle.
pub fn wait(s: &mut State) {
    let Some(scene) = scene() else { return };
    let Some(rx) = CAPTURE.lock().unwrap().as_mut().and_then(|c| c.1.take()) else { return };
    let start = Instant::now();
    let mut target: Option<Option<target::Target>> = None;
    let mut started = false;
    // `poll` here scrolls a throwaway `Tx`; [`on_poll`] hands this to the real frames.
    let mut scroll = None;
    let mut run = write::Run::new(scene);
    let mut voice_run = voice::Run::new(scene);
    let timeout = if run.is_some() { WRITE_TIMEOUT } else if voice_run.is_some() { voice::TIMEOUT + Duration::from_secs(20) } else { TIMEOUT };
    while start.elapsed() < timeout {
        if let Ok(f) = rx.recv_timeout(Duration::from_millis(50)) {
            f(s);
        }
        while let Ok(f) = rx.try_recv() {
            f(s);
        }
        poll(s, &mut rune::Tx::default());
        if s.app.route != Route::Main {
            continue;
        }
        if let Some(r) = &mut voice_run {
            if start.elapsed() > voice::TIMEOUT {
                r.abort(s);
            }
            if r.step(s) {
                break;
            }
            continue;
        }
        if let Some(r) = &mut run {
            match r.step(s) {
                write::Next::Wait => continue,
                write::Next::Done(key) => {
                    scroll = key;
                    break;
                }
            }
        }
        let t = *target.get_or_insert_with(|| target::pick(s, scene));
        if !target::ready(s, scene, t) {
            continue;
        }
        let Some(t) = t else { break };
        let first = !std::mem::replace(&mut started, true);
        let sh = &mut s.app.shell;
        match scene {
            "pins" => {
                if first {
                    crate::shell::pins::open(sh, t.channel);
                }
                if sh.pins.get(&t.channel).is_some_and(|p| !p.loading) {
                    break;
                }
            }
            "search" => {
                if first {
                    let q = std::env::var("REFLUX_LIVE_QUERY").unwrap_or_else(|_| "has:image".into());
                    sh.search.query = rune::TextEdit::new(q);
                    sh.search_requested = true;
                }
                if sh.search.results.as_ref().is_some_and(|r| !matches!(r, crate::shell::search::Results::Loading)) {
                    break;
                }
            }
            "jump" => {
                if first {
                    target::jump_back(s, t.channel);
                }
                let sh = &mut s.app.shell;
                if let Some(h) = sh.highlight
                    && s.app.store.messages(t.channel).iter().any(|m| m.id == h)
                    && sh.intents.is_empty()
                {
                    scroll = Some(format!("msg-{h}"));
                    let at = s.app.store.messages(t.channel).iter().position(|m| m.id == h).unwrap_or(0);
                    eprintln!("reflux --live=jump: message {at} of {} in the window", s.app.store.messages(t.channel).len());
                    break;
                }
            }
            _ => break,
        }
    }
    *CAPTURE.lock().unwrap() = None;
    while let Ok(f) = rx.try_recv() {
        f(s);
    }
    poll(s, &mut rune::Tx::default());
    let frames = std::env::var("REFLUX_LIVE_JUMP_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    *DELAYED.lock().unwrap() = scroll.map(|k| (k, frames));

    let ok = s.app.route == Route::Main;
    eprintln!("reflux --live={scene}: {} after {:.1}s", if ok { "ready" } else { "TIMED OUT" }, start.elapsed().as_secs_f32());
}

mod target {
    use reflux_model::Snowflake;
    use reflux_store::GuildListItem;

    use crate::State;

    #[derive(Clone, Copy)]
    pub struct Target {
        pub guild: Option<Snowflake>,
        pub channel: Snowflake,
    }

    /// Selects what the scene shows, like a user's clicks would.
    pub fn pick(s: &mut State, scene: &str) -> Option<Target> {
        let store = &s.app.store;
        let t = if scene == "dm" {
            let c = store.private_channels().into_iter().find(|c| c.last_message_id.is_some())?;
            Target { guild: None, channel: c.id }
        } else if scene == "home" {
            return None;
        } else {
            let n: usize = std::env::var("REFLUX_LIVE_GUILD").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
            let guilds: Vec<Snowflake> = store
                .guild_list()
                .into_iter()
                .flat_map(|i| match i {
                    GuildListItem::Guild(g) => vec![g],
                    GuildListItem::Folder { guilds, .. } => guilds,
                })
                .collect();
            let g = *guilds.get(n).or(guilds.first())?;
            Target { guild: Some(g), channel: crate::shell::first_channel(store, g)? }
        };
        let sh = &mut s.app.shell;
        sh.select_guild(t.guild, Some(t.channel));
        sh.member_list = true;
        if scene == "settings" {
            let tab = std::env::var("REFLUX_LIVE_TAB").ok().map(|t| &*t.leak());
            sh.open_settings(tab);
        }
        Some(t)
    }

    /// Jumps to the oldest message of the page before the loaded ones (or the oldest loaded).
    pub fn jump_back(s: &State, channel: Snowflake) {
        let (Some(inst), Some(oldest)) = (s.app.instance.clone(), s.app.store.messages(channel).first().map(|m| m.id)) else { return };
        let guild = s.app.store.channels.get(&channel).and_then(|c| c.guild_id);
        crate::app::spawn(async move {
            let page = inst.rest.messages(channel, reflux_net::MessagePage::Before(oldest), 100).await;
            // Newest first: the last is the oldest.
            let id = page.ok().and_then(|p| p.last().map(|m| m.id)).unwrap_or(oldest);
            crate::app::update(move |s| crate::shell::jump(&mut s.app.shell, guild, channel, id));
        });
    }

    pub fn ready(s: &State, scene: &str, t: Option<Target>) -> bool {
        let Some(t) = t else { return true };
        let messages = s.app.store.messages.contains_key(&t.channel);
        let members = scene != "members"
            || t.guild.is_some_and(|g| s.app.store.member_lists.get(&(g, t.channel.to_string())).is_some_and(|l| l.rows.iter().any(Option::is_some)));
        messages && members
    }
}
