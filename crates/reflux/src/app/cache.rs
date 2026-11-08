//! Offline account cache: the last READY and each open channel's newest messages, per account,
//! as plain owner-only JSON under `<config dir>/cache/` (Fluxer keeps the same unencrypted, in
//! its sqlite store). A saved session paints from it at once and swaps to live data on READY;
//! the cache is deleted when its account signs out.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use reflux_model::{DispatchEvent, Message, Ready, Snowflake};
use serde::{Deserialize, Serialize};

use super::{Route, now_ms, session};
use crate::State;

/// Messages kept per channel, and channels kept (most recently viewed first).
const MESSAGES: usize = 50;
const CHANNELS: usize = 30;
const EVERY: Duration = Duration::from_secs(60);

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct File {
    ready: Option<Box<Ready>>,
    messages: HashMap<Snowflake, Vec<Message>>,
}

#[derive(Default)]
pub struct Cache {
    ready: Option<Box<Ready>>,
    saved: Option<Instant>,
}

fn path(a: &session::Account) -> Option<PathBuf> {
    let host: String = a.instance.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '_' }).collect();
    Some(session::dir()?.join("cache").join(format!("{host}-{}.json", a.user_id)))
}

/// Paints the saved account from its cache while the gateway connects.
pub fn restore(s: &mut State) {
    // `--live` waits for the real READY; `REFLUX_LIVE_CACHE=1` keeps the restore to test the
    // cache → READY handover.
    if super::live::scene().is_some() && std::env::var_os("REFLUX_LIVE_CACHE").is_none() {
        return;
    }
    let Some(file) = s.app.saved.current.as_ref().and_then(path).and_then(|p| std::fs::read(p).ok()) else { return };
    let Ok(File { ready: Some(ready), messages }) = serde_json::from_slice::<File>(&file) else { return };
    s.app.store.apply(&DispatchEvent::Ready(ready), now_ms());
    for (c, page) in messages {
        s.app.store.set_latest_messages(c, page, MESSAGES);
    }
    s.app.route = Route::Main;
    crate::desktop::reopen(s);
}

pub fn ready(s: &mut State, r: &Ready) {
    s.app.cache = Cache { ready: Some(Box::new(r.clone())), saved: None };
}

/// Writes the cache once a minute (the first time right after READY).
pub fn tick(s: &mut State) {
    let c = &s.app.cache;
    if c.ready.is_none() || c.saved.is_some_and(|t| t.elapsed() < EVERY) {
        return;
    }
    s.app.cache.saved = Some(Instant::now());
    let Some(p) = s.app.saved.current.as_ref().and_then(path) else { return };
    let store = &s.app.store;
    let mut order: Vec<Snowflake> = s.app.saved.desktop.recent.clone();
    order.extend(store.messages.keys().filter(|c| !order.contains(c)).copied().collect::<Vec<_>>());
    let messages = order
        .into_iter()
        .filter_map(|c| {
            let m = store.messages(c);
            (!m.is_empty()).then(|| (c, m[m.len().saturating_sub(MESSAGES)..].to_vec()))
        })
        .take(CHANNELS)
        .collect();
    let file = File { ready: s.app.cache.ready.clone(), messages };
    if let Err(e) = write(&p, &file) {
        eprintln!("reflux: saving {}: {e}", p.display());
    }
}

fn write(p: &std::path::Path, file: &File) -> std::io::Result<()> {
    std::fs::create_dir_all(p.parent().unwrap())?;
    let tmp = p.with_extension("json.tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    opts.open(&tmp)?.write_all(&serde_json::to_vec(file)?)?;
    std::fs::rename(&tmp, p)
}

/// Signing out forgets what the account had loaded.
pub fn forget(a: &session::Account) {
    if let Some(p) = path(a) {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_paths_are_per_account() {
        let a = session::Account { instance: "https://fluxer.com/".into(), user_id: "42".into(), ..Default::default() };
        assert!(path(&a).unwrap().ends_with("cache/https___fluxer.com_-42.json"));
        let f = File { ready: Some(Box::new(Ready { session_id: "s".into(), ..Default::default() })), messages: HashMap::from([(Snowflake(7), vec![Message::default()])]) };
        let back: File = serde_json::from_slice(&serde_json::to_vec(&f).unwrap()).unwrap();
        assert_eq!(back.ready.unwrap().session_id, "s");
        assert_eq!(back.messages[&Snowflake(7)].len(), 1);
    }
}
