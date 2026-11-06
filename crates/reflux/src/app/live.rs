//! `--live[=SCENE]` (with `--snap`): a headless snapshot of the real signed-in UI. Read-only mode
//! is forced. Rune's `setup` blocks (real time, bounded) applying the runtime's updates itself
//! until READY and the scene's data are in, and the media loader fetches images synchronously so
//! the first frame has them. Scenes: `guild` (default), `members`, `dm`, `settings`, `home`;
//! `REFLUX_LIVE_GUILD=N` picks the rail's Nth guild, `REFLUX_LIVE_TAB` the settings tab.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use rune::{Image, ImageCache};

use super::{Route, poll};
use crate::State;

type Update = Box<dyn FnOnce(&mut State) + Send>;

const TIMEOUT: Duration = Duration::from_secs(30);

static CAPTURE: Mutex<Option<(Sender<Update>, Option<Receiver<Update>>)>> = Mutex::new(None);

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
    while start.elapsed() < TIMEOUT {
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
        let t = *target.get_or_insert_with(|| target::pick(s, scene));
        if target::ready(s, scene, t) {
            break;
        }
    }
    *CAPTURE.lock().unwrap() = None;
    while let Ok(f) = rx.try_recv() {
        f(s);
    }
    poll(s, &mut rune::Tx::default());
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
            sh.settings = Some(crate::settings::SettingsUi::new(tab));
        }
        Some(t)
    }

    pub fn ready(s: &State, scene: &str, t: Option<Target>) -> bool {
        let Some(t) = t else { return true };
        let messages = s.app.store.messages.contains_key(&t.channel);
        let members = scene != "members"
            || t.guild.is_some_and(|g| s.app.store.member_lists.get(&(g, t.channel.to_string())).is_some_and(|l| l.rows.iter().any(Option::is_some)));
        messages && members
    }
}
