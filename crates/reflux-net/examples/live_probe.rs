//! Read-only live check against a real account: `cargo run -p reflux-net --example live_probe`.
//!
//! Uses Reflux's saved session (`$REFLUX_CONFIG_DIR` or `<config dir>/reflux/session.json`, e.g.
//! from `reflux --import-fluxer-session`). Forces read-only mode, so only GETs and Identify/Resume/
//! heartbeat/lazy member requests leave the process. Discovers the instance, connects the gateway,
//! feeds every event to `reflux-store` for `REFLUX_PROBE_SECS` (default 60), fetches messages of
//! a few channels and some avatars, then reports parse failures (event, JSON path, the shape of
//! the value with every string redacted), unknown events, rate limits, store invariant breaks and
//! counts. Prints no tokens, names, IDs or message content.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reflux_model::{
    ChannelType, DispatchEvent, GatewayPayload, GuildSubscription, IdentifyProperties, LazyRequest, OpCode, SessionFlags,
    Snowflake,
};
use reflux_net::{Client, Command, ConnectionState, Gateway, GatewayConfig, GatewayEvent, MessagePage};
use reflux_store::Store;
use serde_json::Value;

#[derive(Default)]
struct Report {
    events: BTreeMap<String, u32>,
    unknown: BTreeMap<String, u32>,
    failures: Vec<String>,
    states: Vec<String>,
    channel_types: BTreeMap<u16, u32>,
    message_types: BTreeMap<u16, u32>,
    dropped: BTreeSet<String>,
}

fn dropped(raw: &Value, ser: &Value, path: &str, out: &mut BTreeSet<String>) {
    match (raw, ser) {
        (Value::Object(r), Value::Object(s)) => {
            for (k, v) in r {
                let p = format!("{path}.{k}");
                match s.get(k) {
                    _ if v.is_null() => {}
                    None | Some(Value::Null) => {
                        out.insert(p);
                    }
                    Some(sv) => dropped(v, sv, &p, out),
                }
            }
        }
        (Value::Array(r), Value::Array(s)) => {
            for (v, sv) in r.iter().zip(s) {
                dropped(v, sv, &format!("{path}[]"), out);
            }
        }
        _ => {}
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The value with strings replaced by their length, arrays cut to 2 items, depth-limited.
fn shape(v: &Value, depth: usize) -> Value {
    match v {
        Value::String(s) => Value::String(format!("<str {}>", s.len())),
        Value::Array(a) if depth == 0 => Value::String(format!("<array {}>", a.len())),
        Value::Object(o) if depth == 0 => Value::String(format!("<object {} keys>", o.len())),
        Value::Array(a) => Value::Array(a.iter().take(2).map(|x| shape(x, depth - 1)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), shape(x, depth - 1))).collect()),
        other => other.clone(),
    }
}

/// Follows a serde_path_to_error path (`a.b[2].c: message`) as far as it goes; the parent of
/// the failing field is the most useful context.
fn at_path<'a>(v: &'a Value, error: &str) -> (&'a Value, String) {
    let path = error.split(": ").next().unwrap_or("");
    let segs: Vec<&str> = path.split(['.', '[', ']']).filter(|s| !s.is_empty()).collect();
    let mut cur = v;
    let mut walked = String::new();
    for seg in segs.iter().take(segs.len().saturating_sub(1)) {
        let next = match seg.parse::<usize>() {
            Ok(i) => cur.get(i),
            Err(_) => cur.get(*seg),
        };
        match next {
            Some(n) => {
                cur = n;
                walked.push('/');
                walked.push_str(seg);
            }
            None => break,
        }
    }
    (cur, walked)
}

fn session() -> Value {
    let dir = std::env::var_os("REFLUX_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs_config().join("reflux"));
    let bytes = std::fs::read(dir.join("session.json")).expect("no saved session; run `reflux --import-fluxer-session` first");
    serde_json::from_slice(&bytes).expect("session.json")
}

fn dirs_config() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".config"))
}

#[tokio::main]
async fn main() {
    reflux_net::force_readonly();
    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).with_target(false).init();
    let secs: u64 = std::env::var("REFLUX_PROBE_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
    let s = session();
    let (host, token) = (s["instance"].as_str().unwrap_or_default(), s["token"].as_str().expect("signed out").to_owned());
    let mut rep = Report::default();

    let http = reflux_net::http_client();
    let d = match reflux_net::discover(&http, host).await {
        Ok(d) => d,
        Err(e) => return println!("discovery FAILED: {e}"),
    };
    println!("discovered {host}: api_code_version {}, self_hosted {}", d.api_code_version, d.features.self_hosted);
    let rest = Client::from_discovery(http.clone(), &d);
    rest.set_token(Some(token.clone()));
    let rest = match s["installation_id"].as_str() {
        Some(id) => rest.with_installation_id(id.to_owned()),
        None => rest,
    };
    match rest.get::<Value>("/users/@me").await {
        Ok(raw) => match serde_json::from_value::<reflux_model::User>(raw.clone()) {
            Ok(u) => {
                println!("GET /users/@me ok");
                dropped(&raw, &serde_json::to_value(u).unwrap_or_default(), "@me", &mut rep.dropped);
            }
            Err(e) => rep.failures.push(format!("GET /users/@me: {e}")),
        },
        Err(e) => rep.failures.push(format!("GET /users/@me: {e}")),
    }

    let mut gw = Gateway::connect(GatewayConfig {
        url: d.endpoints.gateway.clone(),
        token,
        properties: IdentifyProperties { os: "Linux".into(), browser: "Reflux".into(), device: std::env::consts::ARCH.into(), ..Default::default() },
        flags: SessionFlags::CHANNEL_THREADS,
        presence: None,
        compress: true,
    })
    .expect("gateway");

    let mut store = Store::default();
    let start = Instant::now();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    let mut probed = false;
    loop {
        let ev = tokio::select! {
            ev = gw.events.recv() => ev,
            _ = tokio::time::sleep_until(deadline) => break,
        };
        let Some(ev) = ev else { break };
        match ev {
            GatewayEvent::State(c) => {
                rep.states.push(format!("{:.1}s {c:?}", start.elapsed().as_secs_f32()));
                if matches!(c, ConnectionState::Disconnected(_)) {
                    break;
                }
            }
            GatewayEvent::Dispatch(e) => {
                *rep.events.entry(e.name().to_owned()).or_default() += 1;
                if let DispatchEvent::Unknown { t, .. } = &e {
                    *rep.unknown.entry(t.clone()).or_default() += 1;
                }
                if let DispatchEvent::RateLimited(r) = &e {
                    rep.failures.push(format!("gateway RATE_LIMITED: {r:?}"));
                }
                store.apply(&e, now_ms());
                if matches!(e, DispatchEvent::Ready(_)) && !probed {
                    probed = true;
                    probe_rest(&rest, &http, &d.endpoints.media, &mut store, &gw, &mut rep).await;
                }
            }
            GatewayEvent::Malformed { t, d, error } => {
                let (v, walked) = at_path(&d, &error);
                rep.failures.push(format!("dispatch {t}: {error}\n    at {walked}: {}", shape(v, 3)));
            }
            GatewayEvent::Error(e) => rep.failures.push(format!("gateway op 12: {e:?}")),
        }
    }
    let _ = gw.commands.send(Command::Close);

    for c in store.channels.values() {
        *rep.channel_types.entry(c.kind.0).or_default() += 1;
    }
    for m in store.messages.values().flat_map(|w| &w.messages) {
        *rep.message_types.entry(m.kind.0).or_default() += 1;
    }
    println!("\n== connection\n{}", rep.states.join("\n"));
    println!("\n== events ({secs}s)\n{:?}", rep.events);
    println!("\n== unknown events\n{:?}", rep.unknown);
    println!("\n== counts");
    let members: usize = store.guilds.values().map(|g| g.members.len()).sum();
    println!(
        "guilds {} (unavailable {}), channels {}, private {}, users {}, members {members}, presences {}, relationships {}, read_states {}, \
         message windows {} ({} messages), member lists {}, voice states {}",
        store.guilds.len(),
        store.guilds.values().filter(|g| g.unavailable).count(),
        store.channels.len(),
        store.private_channels().len(),
        store.users.len(),
        store.presences.len(),
        store.relationships.len(),
        store.read_states.len(),
        store.messages.len(),
        store.messages.values().map(|w| w.messages.len()).sum::<usize>(),
        store.member_lists.len(),
        store.voice_states.len(),
    );
    println!("channel types {:?}\nmessage types {:?}", rep.channel_types, rep.message_types);
    let all = || store.messages.values().flat_map(|w| &w.messages);
    println!(
        "messages with attachments {}, embeds {}, reactions {}, stickers {}, replies {}, mentions {}",
        all().filter(|m| !m.attachments.is_empty()).count(),
        all().filter(|m| !m.embeds.is_empty()).count(),
        all().filter(|m| !m.reactions.is_empty()).count(),
        all().filter(|m| !m.stickers.is_empty()).count(),
        all().filter(|m| m.message_reference.is_some()).count(),
        all().filter(|m| !m.mentions.is_empty()).count(),
    );
    println!("\n== store invariants");
    let broken = invariants(&store);
    println!("{}", if broken.is_empty() { "ok".into() } else { broken.join("\n") });
    println!("\n== fields the model drops\n{:?}", rep.dropped);
    println!("\n== failures ({})", rep.failures.len());
    for f in &rep.failures {
        println!("- {f}");
    }
}

/// Messages of up to 3 guild channels and 2 DMs, a member list subscription, and avatars.
async fn probe_rest(rest: &Client, http: &reqwest::Client, media: &str, store: &mut Store, gw: &Gateway, rep: &mut Report) {
    let mut targets: Vec<Snowflake> = Vec::new();
    let mut guilds: Vec<_> = store.guilds.keys().copied().collect();
    guilds.sort();
    for g in guilds.iter().take(3) {
        let first = store.channel_groups(*g).into_iter().flat_map(|gr| gr.text).find(|c| c.kind == ChannelType::GUILD_TEXT).map(|c| c.id);
        if let Some(c) = first {
            targets.push(c);
            if targets.len() == 1 {
                let sub = GuildSubscription { active: Some(true), member_list_channels: [(c, vec![[0, 99]])].into(), ..Default::default() };
                let req = LazyRequest { subscriptions: [(*g, sub)].into() };
                let _ = gw.commands.send(Command::Send(GatewayPayload::command(OpCode::LAZY_REQUEST, req)));
            }
        }
    }
    targets.extend(store.private_channels().iter().take(2).map(|c| c.id));
    for (i, c) in targets.iter().enumerate() {
        match rest.messages(*c, MessagePage::Latest, 50).await {
            Ok(page) => {
                println!("messages #{i}: {} ok", page.len());
                store.set_latest_messages(*c, page, 50);
            }
            Err(e) => rep.failures.push(format!("GET messages #{i}: {e}")),
        }
        // Fields the model ignores: present (non-null) in the raw JSON, missing once re-serialized.
        if let Ok(raw) = rest.get::<Value>(&format!("/channels/{c}/messages?limit=50")).await
            && let Ok(parsed) = serde_json::from_value::<Vec<reflux_model::Message>>(raw.clone())
        {
            dropped(&raw, &serde_json::to_value(parsed).unwrap_or_default(), "messages", &mut rep.dropped);
        }
    }
    let mut avatars: Vec<(Snowflake, String)> =
        store.users.values().filter_map(|u| Some((u.id, u.avatar.clone()?))).collect();
    avatars.sort();
    let (mut ok, mut bad) = (0, BTreeSet::new());
    for (id, hash) in avatars.iter().take(8) {
        let url = format!("{}/avatars/{id}/{}.webp?size=128", media.trim_end_matches('/'), hash.trim_start_matches("a_"));
        match http.get(&url).send().await {
            Ok(r) if r.status().is_success() => match r.bytes().await {
                Ok(b) if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") => ok += 1,
                Ok(b) => {
                    bad.insert(format!("not WebP ({} bytes, starts {:?})", b.len(), &b[..b.len().min(4)]));
                }
                Err(e) => {
                    bad.insert(e.to_string());
                }
            },
            Ok(r) => {
                bad.insert(format!("HTTP {}", r.status()));
            }
            Err(e) => {
                bad.insert(e.to_string());
            }
        }
    }
    println!("avatars: {ok}/{} ok", avatars.len().min(8));
    rep.failures.extend(bad.into_iter().map(|b| format!("avatar: {b}")));
}

fn invariants(s: &Store) -> Vec<String> {
    let mut out = Vec::new();
    if s.me.is_none() {
        out.push("no current user".into());
    }
    let orphans = s.channels.values().filter(|c| c.guild_id.is_some_and(|g| !s.guilds.contains_key(&g))).count();
    if orphans > 0 {
        out.push(format!("{orphans} channels of unknown guilds"));
    }
    let keyed_wrong = s.channels.iter().filter(|(k, c)| **k != c.id).count();
    if keyed_wrong > 0 {
        out.push(format!("{keyed_wrong} channels under the wrong key"));
    }
    for (ch, w) in &s.messages {
        if !w.messages.windows(2).all(|p| p[0].id < p[1].id) {
            out.push("a message window is not strictly ascending".into());
        }
        if w.messages.iter().any(|m| m.channel_id != *ch) {
            out.push("a message window holds another channel's message".into());
        }
        if w.messages.len() > 200 {
            out.push(format!("a message window holds {} messages", w.messages.len()));
        }
    }
    for g in s.guilds.keys() {
        let grouped: usize = s.channel_groups(*g).iter().map(|gr| gr.text.len() + gr.voice.len() + usize::from(gr.category.is_some())).sum();
        let visible = s
            .channels
            .values()
            .filter(|c| c.guild_id == Some(*g) && !c.kind.is_thread() && s.can_view(c.id))
            .filter(|c| {
                // Uncategorized, or under a visible category: what the sidebar can show.
                c.parent_id.is_none_or(|p| s.channels.get(&p).is_some_and(|pc| pc.kind == ChannelType::GUILD_CATEGORY && s.can_view(p)))
            })
            .count();
        if grouped != visible {
            out.push(format!("a guild shows {grouped} of {visible} visible channels in the sidebar"));
        }
        if s.guilds[g].info.is_none() && !s.guilds[g].unavailable {
            out.push("an available guild has no info".into());
        }
    }
    let listed: usize = s
        .guild_list()
        .iter()
        .map(|i| match i {
            reflux_store::GuildListItem::Guild(_) => 1,
            reflux_store::GuildListItem::Folder { guilds, .. } => guilds.len(),
        })
        .sum();
    let usable = s.guilds.values().filter(|g| !g.visiting && !g.unavailable).count();
    if listed < usable {
        out.push(format!("the guild rail lists {listed} of {usable} guilds"));
    }
    for ((_, key), l) in &s.member_lists {
        if l.rows.len() as u32 > l.member_count + l.groups.len() as u32 + 1 {
            out.push(format!("member list {key:?}: {} rows for {} members + {} groups", l.rows.len(), l.member_count, l.groups.len()));
        }
    }
    out
}
