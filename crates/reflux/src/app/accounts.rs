//! Background account engine (`fluxer_desktop/src/engine/*`, `AccountEngineHost`): every other
//! signed-in account keeps one gateway connection so its mentions, unread channels, notifications
//! and inbox activity reach this window without switching. The foreground account stays on
//! [`super::App::gateway`]; switching moves a gateway between the two lists (`demote`/`promote`)
//! instead of reconnecting, so it stays instant and the previous account keeps running.
//!
//! The engine never fetches history and never writes: it only applies dispatches to its own
//! [`Store`] (which keeps no message window unless one was loaded while that account was in the
//! foreground) and reports counts.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use reflux_model::{Channel, ChannelType, DispatchEvent, InitialPresence, Message, NotificationLevel, SessionFlags, Snowflake, Status};
use reflux_net::{Client, Command, ConnectionState, Gateway, GatewayConfig, GatewayEvent};
use reflux_store::Store;
use tokio::sync::mpsc::UnboundedSender;

use super::{Instance, Route, now_ms, spawn, update};
use crate::session;
use crate::{State, desktop};

/// One id per gateway connection (foreground or background), so events find their owner even
/// after accounts changed places. Separate from `App::generation`, which only invalidates REST.
static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

pub fn next_conn() -> u64 {
    NEXT_CONN.fetch_add(1, Ordering::Relaxed)
}

/// `OTHER_ACCOUNTS_INBOX_CHANNEL_LIMIT` / `_MENTION_LIMIT`.
pub const INBOX_CHANNELS: usize = 8;
pub const INBOX_MENTIONS: usize = 3;

/// `AccountEngineConnectionState`, as the switcher shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EngineState {
    #[default]
    Connecting,
    Live,
    Reconnecting,
    Offline,
}

/// One channel with unread messages or mentions on another account (`getActivity`).
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelActivity {
    pub channel: Snowflake,
    pub guild: Option<Snowflake>,
    pub kind: ChannelType,
    pub name: String,
    pub guild_name: Option<String>,
    pub mentions: u32,
    pub unread: bool,
    pub last_message: Option<Snowflake>,
}

/// A recent mention on another account, for the inbox.
#[derive(Clone, Debug, PartialEq)]
pub struct MentionActivity {
    pub message: Snowflake,
    pub channel: Snowflake,
    pub guild: Option<Snowflake>,
    pub channel_name: String,
    pub guild_name: Option<String>,
    pub author: String,
    pub preview: String,
    pub timestamp: u64,
}

/// What the rail/dock badge, the switcher and the inbox read (`AccountEngineAccountSummary` +
/// `getActivity`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub mentions: u32,
    pub unread: u32,
    pub last_activity: u64,
    /// Unread/mention channels, mentions first then newest, capped at [`INBOX_CHANNELS`].
    pub channels: Vec<ChannelActivity>,
    pub recent_mentions: Vec<MentionActivity>,
}

pub struct Engine {
    pub slot: String,
    pub account: session::Account,
    pub instance: Option<Arc<Instance>>,
    pub store: Store,
    pub conn: u64,
    pub gateway: Option<UnboundedSender<Command>>,
    pub state: EngineState,
    pub ready: bool,
    pub summary: Summary,
    /// Dropped with the engine (signed out or app teardown) so its connect task stops.
    alive: Arc<()>,
    notified: VecDeque<Snowflake>,
}

impl Engine {
    fn new(slot: String, account: session::Account, conn: u64) -> Self {
        Self {
            slot,
            account,
            instance: None,
            store: Store::default(),
            conn,
            gateway: None,
            state: EngineState::Connecting,
            ready: false,
            summary: Summary::default(),
            alive: Arc::new(()),
            notified: VecDeque::new(),
        }
    }
}

#[derive(Default)]
pub struct Engines {
    pub list: Vec<Engine>,
}

/// What the account switcher and the inbox show for one background account.
#[derive(Clone, Debug, PartialEq)]
pub struct AccountActivity {
    pub slot: String,
    pub state: EngineState,
    pub summary: Summary,
}

impl Engines {
    pub fn mentions(&self) -> u32 {
        self.list.iter().map(|e| e.summary.mentions).sum()
    }

    pub fn any_unread(&self) -> bool {
        self.list.iter().any(|e| e.summary.unread > 0)
    }

    pub fn activities(&self) -> Vec<AccountActivity> {
        self.list.iter().map(|e| AccountActivity { slot: e.slot.clone(), state: e.state, summary: e.summary.clone() }).collect()
    }
}

/// The engine's view of a store, matching Fluxer's `BackgroundReadStateReducer::totals` +
/// `channelActivity`: mentions on every known channel, unread channels that aren't muted and
/// aren't hidden by an unread-badge level, newest activity first.
pub fn summarize(store: &Store, recent_mentions: Vec<MentionActivity>) -> Summary {
    let mut summary = Summary { recent_mentions: recent_mentions.into_iter().take(INBOX_MENTIONS).collect(), ..Default::default() };
    for c in store.channels.values() {
        if c.kind == ChannelType::GUILD_CATEGORY {
            continue;
        }
        // Unjoined threads don't show in the rail either (`getThreadContribution`).
        if c.kind.is_thread() && c.member.is_none() {
            continue;
        }
        let mentions = store.mention_count(c.id);
        let level = crate::shell::unread_badges::level(store, c);
        let unread = !matches!(level, Some(NotificationLevel::NO_MESSAGES | NotificationLevel::ONLY_MENTIONS))
            && !store.is_muted(c)
            && store.has_unread(c.id);
        if let Some(ts) = c.last_message_id.map(|m| m.timestamp_ms()) {
            summary.last_activity = summary.last_activity.max(ts);
        }
        if mentions == 0 && !unread {
            continue;
        }
        summary.mentions = summary.mentions.saturating_add(mentions);
        if unread {
            summary.unread += 1;
        }
        summary.channels.push(channel_activity(store, c, mentions, unread));
    }
    summary.channels.sort_by(|a, b| {
        (b.mentions > 0)
            .cmp(&(a.mentions > 0))
            .then_with(|| b.last_message.cmp(&a.last_message))
    });
    summary.channels.truncate(INBOX_CHANNELS);
    summary
}

fn channel_activity(store: &Store, c: &Channel, mentions: u32, unread: bool) -> ChannelActivity {
    ChannelActivity {
        channel: c.id,
        guild: c.guild_id,
        kind: c.kind,
        name: crate::shell::chat::channel_title(store, c),
        guild_name: c.guild_id.and_then(|g| store.guilds.get(&g)?.info.as_ref()).map(|i| i.name.clone()),
        mentions,
        unread,
        last_message: c.last_message_id,
    }
}

fn mention_activity(store: &Store, m: &Message) -> MentionActivity {
    let channel = store.channels.get(&m.channel_id);
    let guild = channel.and_then(|c| c.guild_id);
    MentionActivity {
        message: m.id,
        channel: m.channel_id,
        guild,
        channel_name: channel.map_or_else(|| m.channel_id.to_string(), |c| crate::shell::chat::channel_title(store, c)),
        guild_name: guild.and_then(|g| store.guilds.get(&g)?.info.as_ref()).map(|i| i.name.clone()),
        author: store.display_name(guild, m.author.id),
        preview: desktop::notify::body(store, m),
        timestamp: m.id.timestamp_ms(),
    }
}

/// Starts a gateway for every stored account that isn't the foreground one (`start`).
pub fn start_all(s: &mut State) {
    for i in 0..s.app.saved.accounts.len() {
        start(s, i);
    }
}

/// Starts the background gateway of `saved.accounts[i]` (one per account).
pub fn start(s: &mut State, i: usize) {
    let Some(account) = s.app.saved.accounts.get(i).cloned() else { return };
    let Some(token) = account.token.clone() else { return };
    let slot = account.slot();
    if s.app.accounts.list.iter().any(|e| e.slot == slot) {
        return;
    }
    let conn = next_conn();
    let host = account.instance.clone();
    let installation = s.app.saved.installation_id.clone();
    let engine = Engine::new(slot, account, conn);
    let alive = Arc::downgrade(&engine.alive);
    s.app.accounts.list.push(engine);
    spawn(async move {
        let http = reflux_net::http_client();
        let discovery = loop {
            match reflux_net::discover(&http, &host).await {
                Ok(d) => break d,
                Err(e) => {
                    eprintln!("reflux: background account {host}: {e}");
                    update(move |s| set_state(s, conn, EngineState::Offline));
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    if alive.strong_count() == 0 {
                        return;
                    }
                }
            }
        };
        if alive.strong_count() == 0 {
            return;
        }
        let rest = Client::from_discovery(http, &discovery).with_installation_id(installation);
        rest.set_token(Some(token.clone()));
        let config = GatewayConfig {
            url: discovery.endpoints.gateway.clone(),
            token,
            properties: super::identify_properties(),
            // Fluxer identifies background accounts as invisible, so the other account doesn't
            // appear online twice (`AccountEngine`'s `INVISIBLE_PRESENCE`).
            flags: SessionFlags::DEBOUNCE_MESSAGE_REACTIONS | SessionFlags::CHANNEL_THREADS,
            presence: Some(InitialPresence { status: Status::Invisible, afk: false, mobile: false, custom_status: None }),
            compress: true,
        };
        let mut gw = match Gateway::connect(config) {
            Ok(gw) => gw,
            Err(e) => {
                eprintln!("reflux: background gateway {host}: {e}");
                return update(move |s| set_state(s, conn, EngineState::Offline));
            }
        };
        if alive.strong_count() == 0 {
            let _ = gw.commands.send(Command::Close);
            return;
        }
        let commands = gw.commands.clone();
        update(move |s| attach(s, conn, Arc::new(Instance { discovery, rest }), commands));
        while let Some(ev) = gw.events.recv().await {
            if alive.strong_count() == 0 {
                let _ = gw.commands.send(Command::Close);
                return;
            }
            update(move |s| on_event(s, conn, ev));
        }
    });
}

fn attach(s: &mut State, conn: u64, instance: Arc<Instance>, gateway: UnboundedSender<Command>) {
    if let Some(e) = s.app.accounts.list.iter_mut().find(|e| e.conn == conn) {
        e.instance = Some(instance);
        e.gateway = Some(gateway);
        e.state = EngineState::Connecting;
    }
}

fn set_state(s: &mut State, conn: u64, state: EngineState) {
    if let Some(e) = s.app.accounts.list.iter_mut().find(|e| e.conn == conn) {
        e.state = state;
    }
}

/// A background gateway event. Same `Store::apply` as the foreground, but no voice/UI side
/// effects; counts and notifications are all this account contributes.
pub fn on_event(s: &mut State, conn: u64, ev: GatewayEvent) {
    let Some(i) = s.app.accounts.list.iter().position(|e| e.conn == conn) else { return };
    match ev {
        GatewayEvent::State(c) => {
            let state = match c {
                ConnectionState::Connected => EngineState::Live,
                ConnectionState::Reconnecting(_) => EngineState::Reconnecting,
                ConnectionState::Connecting => EngineState::Connecting,
                ConnectionState::Disconnected(_) => EngineState::Offline,
            };
            s.app.accounts.list[i].state = state;
        }
        GatewayEvent::Dispatch(d) => {
            let before = match &d {
                DispatchEvent::MessageCreate(m) => s.app.accounts.list[i].store.mention_count(m.channel_id),
                _ => 0,
            };
            s.app.accounts.list[i].store.apply(&d, now_ms());
            match &d {
                DispatchEvent::Ready(r) => {
                    s.app.accounts.list[i].ready = true;
                    s.app.accounts.list[i].state = EngineState::Live;
                    remember(s, i, &r.user.base);
                }
                DispatchEvent::MessageCreate(m) => {
                    let mentioned = s.app.accounts.list[i].store.mention_count(m.channel_id) > before;
                    if mentioned && m.author.id != s.app.accounts.list[i].store.my_id() {
                        let a = mention_activity(&s.app.accounts.list[i].store, m);
                        let e = &mut s.app.accounts.list[i];
                        e.summary.recent_mentions.retain(|x| x.message != a.message);
                        e.summary.recent_mentions.insert(0, a);
                        e.summary.recent_mentions.truncate(INBOX_MENTIONS);
                    }
                    if s.app.accounts.list[i].store.should_notify(m) {
                        notify(s, i, m);
                    }
                }
                _ => {}
            }
            if affects_counts(&d) {
                let recent = s.app.accounts.list[i].summary.recent_mentions.clone();
                s.app.accounts.list[i].summary = summarize(&s.app.accounts.list[i].store, recent);
            }
        }
        GatewayEvent::Error(e) => eprintln!("reflux: background gateway: {e:?}"),
        GatewayEvent::Malformed { t, error, .. } => eprintln!("reflux: malformed {t} skipped: {error}"),
    }
}

/// Dispatches that can change unread or mention counts (everything else only touches state the
/// summary doesn't read, so a message edit won't rescan every channel).
fn affects_counts(d: &DispatchEvent) -> bool {
    use DispatchEvent as E;
    matches!(
        d,
        E::Ready(_)
            | E::MessageCreate(_)
            | E::MessageAck(_)
            | E::ChannelCreate(_)
            | E::ChannelUpdate(_)
            | E::ChannelUpdateBulk(_)
            | E::ChannelDelete(_)
            | E::ThreadCreate(_)
            | E::ThreadUpdate(_)
            | E::ThreadDelete(_)
            | E::ThreadListSync(_)
            | E::GuildCreate(_)
            | E::GuildSync(_)
            | E::GuildUpdate(_)
            | E::GuildDelete(_)
            | E::UserGuildSettingsUpdate(_)
            | E::RelationshipAdd(_)
            | E::RelationshipUpdate(_)
            | E::RelationshipRemove(_)
    )
}

/// Keeps the saved identity fresh for the switcher and the next start.
fn remember(s: &mut State, i: usize, me: &reflux_model::PartialUser) {
    let id = me.id.to_string();
    if s.app.accounts.list[i].account.user_id.is_empty() {
        s.app.accounts.list[i].account.user_id = id.clone();
    }
    let (username, display_name, avatar) = (me.username.clone(), me.global_name.clone().unwrap_or_else(|| me.username.clone()), me.avatar.clone());
    let e = &mut s.app.accounts.list[i];
    e.account.username = username.clone();
    e.account.display_name = display_name.clone();
    e.account.avatar = avatar.clone();
    if let Some(a) = s.app.saved.accounts.iter_mut().find(|a| a.slot() == e.slot) {
        a.user_id = if a.user_id.is_empty() { id } else { a.user_id.clone() };
        a.username = username;
        a.display_name = display_name;
        a.avatar = avatar;
    }
}

/// Moves the foreground gateway/store into the engine list, so switching away doesn't lose its
/// connection (Fluxer's foreground handoff). The caller clears the view afterwards.
pub fn demote(s: &mut State) {
    let (Some(instance), Some(token)) = (s.app.instance.clone(), s.app.saved.token.clone()) else { return };
    let Some(mut account) = s.app.saved.current.clone() else { return };
    account.instance = s.app.saved.instance.clone();
    account.token = Some(token);
    account.imported = s.app.saved.imported;
    let slot = account.slot();
    // Background accounts identify invisible; the foreground session identified with the saved
    // status, so tell the server this one is no longer the active window.
    if let Some(gw) = &s.app.gateway
        && !reflux_net::readonly()
    {
        let d = serde_json::json!({ "status": "invisible", "afk": false, "mobile": false });
        let _ = gw.send(Command::Send(reflux_model::GatewayPayload::command(reflux_model::OpCode::PRESENCE_UPDATE, d)));
    }
    if let Some(j) = s.app.accounts.list.iter().position(|e| e.slot == slot)
        && let Some(gw) = s.app.accounts.list.remove(j).gateway
    {
        let _ = gw.send(Command::Close);
    }
    let conn = s.app.gateway_id.unwrap_or_else(next_conn);
    let mut e = Engine::new(slot, account, conn);
    e.instance = Some(instance);
    e.gateway = s.app.gateway.take();
    e.store = std::mem::take(&mut s.app.store);
    e.ready = s.app.route == Route::Main && e.store.me.is_some();
    e.state = match s.app.connection {
        Some(ConnectionState::Connected) => EngineState::Live,
        Some(ConnectionState::Reconnecting(_)) => EngineState::Reconnecting,
        Some(ConnectionState::Disconnected(_)) => EngineState::Offline,
        _ => EngineState::Connecting,
    };
    let recent = e.summary.recent_mentions.clone();
    e.summary = summarize(&e.store, recent);
    s.app.accounts.list.push(e);
    s.app.gateway_id = None;
    s.app.connection = None;
}

/// Makes the stored account's engine the foreground: its gateway keeps running (no reconnect)
/// and its store becomes the UI's store. False when no started engine exists.
pub fn promote(s: &mut State) -> bool {
    let Some(account) = s.app.saved.current.clone() else { return false };
    let slot = account.slot();
    let Some(i) = s.app.accounts.list.iter().position(|e| e.slot == slot) else { return false };
    let e = s.app.accounts.list.remove(i);
    let Some(instance) = e.instance else { return false };
    s.app.instance = Some(instance);
    s.app.store = e.store;
    let connected = e.gateway.is_some();
    let state = e.state;
    s.app.gateway = e.gateway;
    s.app.gateway_id = connected.then_some(e.conn);
    s.app.connection = Some(match state {
        EngineState::Live => ConnectionState::Connected,
        EngineState::Reconnecting => ConnectionState::Reconnecting(std::time::Duration::from_secs(1)),
        EngineState::Connecting => ConnectionState::Connecting,
        EngineState::Offline => ConnectionState::Disconnected(None),
    });
    s.app.cache = Default::default();
    if connected && e.ready {
        s.app.route = Route::Main;
        super::account::remember_me(s);
        crate::app::idle::ready(s);
        if !s.app.store.settings.locale.is_empty() {
            crate::i18n::set_locale(&s.app.store.settings.locale);
            crate::desktop::spell::configure(&s.app.saved.desktop);
        }
        crate::desktop::ready(s);
        super::open_pending_channel(s);
        super::publish_presence(s);
    } else {
        s.app.route = Route::Connecting;
        if !connected || state == EngineState::Offline {
            super::connect(s);
        }
    }
    true
}

/// Closes one background account's gateway (signing it out).
pub fn stop(s: &mut State, slot: &str) {
    if let Some(i) = s.app.accounts.list.iter().position(|e| e.slot == slot)
        && let Some(gw) = s.app.accounts.list.remove(i).gateway
    {
        let _ = gw.send(Command::Close);
    }
}

pub fn stop_all(s: &mut State) {
    for e in s.app.accounts.list.drain(..) {
        if let Some(gw) = e.gateway {
            let _ = gw.send(Command::Close);
        }
    }
}

fn notify(s: &mut State, i: usize, m: &Message) {
    let e = &s.app.accounts.list[i];
    if e.notified.contains(&m.id) {
        return;
    }
    let dm = e.store.channels.get(&m.channel_id).is_none_or(|c| c.guild_id.is_none());
    let title = desktop::notify::title(&e.store, m);
    let body = desktop::notify::body(&e.store, m);
    let account = account_label(s, e);
    let title = if title.contains(&account) { title } else { format!("{title} · {account}") };
    let channel = m.channel_id;
    let guild = e.store.channels.get(&channel).and_then(|c| c.guild_id);
    let base = e.instance.as_ref().map(|i| i.discovery.endpoints.media.clone()).unwrap_or_default();
    let avatar = m.author.avatar.clone().filter(|_| !base.is_empty());
    let user = m.author.id;
    let slot = e.slot.clone();
    s.app.accounts.list[i].notified.push_back(m.id);
    if s.app.accounts.list[i].notified.len() > 500 {
        s.app.accounts.list[i].notified.pop_front();
    }
    desktop::play(s, if dm { desktop::sound::Sound::DirectMessage } else { desktop::sound::Sound::Message });
    if !s.desktop.focused && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        super::request_attention();
    }
    if !s.app.saved.desktop.notifications || desktop::streamer::pause_notifications(&s.app.saved.desktop.streamer, s.desktop.capture_app) || rune::is_headless() {
        return;
    }
    spawn(async move {
        let image = match avatar {
            Some(hash) => {
                let url = crate::media::avatar_url(&base, user, &hash, 128.0, 1.0);
                desktop::notify::avatar_file(reflux_net::http_client(), url, user, hash).await
            }
            None => None,
        };
        let on_click = move || {
            crate::app::show_window();
            crate::app::update(move |s| {
                if let Some(i) = s.app.saved.accounts.iter().position(|a| a.slot() == slot) {
                    s.app.pending_channel = Some((guild, channel));
                    s.app.shell.intents.push(crate::shell::Intent::Account(crate::settings::Op::SwitchAccount(i)));
                }
            });
        };
        desktop::notify::show(title, body, image, on_click);
    });
}

/// `"{accountName} on {host}"` when the saved accounts span several instances.
fn account_label(s: &State, e: &Engine) -> String {
    let name = if e.account.display_name.is_empty() { e.account.username.clone() } else { e.account.display_name.clone() };
    let mut hosts = s.app.saved.accounts.iter().map(|a| a.instance.as_str()).chain(s.app.saved.current.iter().map(|a| a.instance.as_str()));
    let first = hosts.next().unwrap_or_default();
    if hosts.all(|h| h == first) {
        return name;
    }
    let host = e.account.instance.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
    format!("{name} on {host}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflux_model::{ReadState, UserGuildSettings};

    fn channel(id: u64, guild: Option<u64>, kind: ChannelType, last: u64) -> Channel {
        Channel { id: Snowflake(id), guild_id: guild.map(Snowflake), kind, name: Some(format!("c{id}")), last_message_id: Some(Snowflake(last)), ..Default::default() }
    }

    fn store() -> Store {
        let mut s = Store::default();
        for (id, guild, last, ack, mentions) in [(10, Some(1), 5000, 4000, 2), (11, Some(1), 3000, 3000, 0), (12, None, 9000, 8000, 1), (13, Some(1), 2000, 1000, 0), (16, Some(1), 10000, 10000, 0)] {
            s.channels.insert(Snowflake(id), channel(id, guild, if guild.is_some() { ChannelType::GUILD_TEXT } else { ChannelType::DM }, last));
            s.read_states.insert(Snowflake(id), ReadState { id: Snowflake(id), mention_count: mentions, last_message_id: Some(Snowflake(ack)), ..Default::default() });
        }
        s
    }

    #[test]
    fn aggregates_mentions_and_unread_channels() {
        let s = store();
        let sum = summarize(&s, Vec::new());
        assert_eq!(sum.mentions, 3);
        assert_eq!(sum.unread, 3, "channels 10, 12 and 13 are past their ack");
        assert_eq!(sum.last_activity, Snowflake(10000).timestamp_ms(), "newest message even in a read channel");
        assert_eq!(sum.channels.iter().map(|c| c.channel).collect::<Vec<_>>(), vec![Snowflake(12), Snowflake(10), Snowflake(13)], "mentions first, then newest");
    }

    #[test]
    fn muted_channels_are_not_unread_but_keep_mentions() {
        let mut s = store();
        s.guild_settings.insert(
            Some(Snowflake(1)),
            UserGuildSettings { guild_id: Some(Snowflake(1)), muted: true, ..Default::default() },
        );
        let sum = summarize(&s, Vec::new());
        assert_eq!(sum.unread, 1, "only the DM is unread");
        assert_eq!(sum.mentions, 3, "muted mentions still count");
    }

    #[test]
    fn categories_and_acked_channels_do_not_count() {
        let mut s = store();
        s.channels.insert(Snowflake(14), channel(14, Some(1), ChannelType::GUILD_CATEGORY, 9000));
        s.read_states.insert(Snowflake(14), ReadState { id: Snowflake(14), mention_count: 5, last_message_id: Some(Snowflake(1)), ..Default::default() });
        s.channels.insert(Snowflake(15), channel(15, Some(1), ChannelType::GUILD_TEXT, 1000));
        let sum = summarize(&s, Vec::new());
        assert_eq!(sum.mentions, 3);
        assert_eq!(sum.unread, 3);
        assert!(!sum.channels.iter().any(|c| c.channel == Snowflake(14) || c.channel == Snowflake(15)));
    }

    #[test]
    fn activity_is_capped_and_mentions_first() {
        let mut s = Store::default();
        for i in 0..12u64 {
            let id = 100 + i;
            s.channels.insert(Snowflake(id), channel(id, None, ChannelType::DM, 1000 + i));
            s.read_states.insert(Snowflake(id), ReadState { id: Snowflake(id), mention_count: (i % 3) as u32, last_message_id: Some(Snowflake(1)), ..Default::default() });
        }
        let sum = summarize(&s, Vec::new());
        assert_eq!(sum.channels.len(), INBOX_CHANNELS);
        assert_eq!(sum.channels[0].mentions, 2);
        assert!(sum.channels.windows(2).all(|w| (w[0].mentions > 0) >= (w[1].mentions > 0)));
    }

    #[test]
    fn engines_sum_mentions_and_unread_for_the_badge() {
        let mut engines = Engines::default();
        let mut a = Engine::new("a".into(), session::Account::default(), 1);
        a.summary = Summary { mentions: 2, unread: 0, ..Default::default() };
        let mut b = Engine::new("b".into(), session::Account::default(), 2);
        b.summary = Summary { mentions: 5, unread: 1, ..Default::default() };
        engines.list.extend([a, b]);
        assert_eq!(engines.mentions(), 7);
        assert!(engines.any_unread());
        assert_eq!(engines.activities().len(), 2);
    }
}
