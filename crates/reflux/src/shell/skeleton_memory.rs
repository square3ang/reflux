//! `SkeletonLayoutMemory`: the shapes the loading skeleton drew last time, per account, kept for
//! 24 h so the next launch's `AppSkeleton` looks like the app that is about to appear instead of a
//! generic placeholder. A reduced port: the rail, the channel list's categories/rows and the
//! member-list group sizes (Fluxer's memory also records DMs, friends, discovery and more).

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use reflux_store::Store;
use serde::{Deserialize, Serialize};

/// `STORAGE_VERSION` (Fluxer's is 9; this reduced shape starts at 1).
const VERSION: u32 = 1;
/// `SNAPSHOT_TTL_MS`.
const TTL_MS: u64 = 24 * 60 * 60 * 1000;
/// `FUTURE_TIMESTAMP_TOLERANCE_MS`.
const FUTURE_MS: u64 = 5 * 60 * 1000;
const FILE: &str = "skeleton-layouts.json";

/// One sidebar row: a category header, or a channel with its icon and a name line of `width`
/// percent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Row {
    Category { width: f32 },
    Channel { width: f32 },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub rail: usize,
    pub rows: Vec<Row>,
    /// Rows in each member-list group (online, offline, roles).
    pub member_groups: Vec<usize>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    at: u64,
    layout: Layout,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Memory {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: BTreeMap<String, Stored>,
}

fn fresh(at: u64, now: u64) -> bool {
    at <= now + FUTURE_MS && now.saturating_sub(at) <= TTL_MS
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn cache() -> &'static Mutex<Option<Memory>> {
    static CACHE: OnceLock<Mutex<Option<Memory>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn path() -> Option<std::path::PathBuf> {
    Some(crate::session::dir()?.join(FILE))
}

fn read() -> Memory {
    let m: Memory = path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if m.version < VERSION { Memory::default() } else { m }
}

fn load() -> Memory {
    cache().lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(read).clone()
}

fn save(m: &Memory) {
    let Some(p) = path() else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(b) = serde_json::to_vec(m) {
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, &b).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

/// The remembered layout for `account`, if it is fresh (`SNAPSHOT_TTL_MS`, and not from the
/// future beyond the tolerance).
pub fn get(account: Option<&str>) -> Option<Layout> {
    let a = account?;
    let m = load();
    let s = m.accounts.get(a)?;
    fresh(s.at, now_ms()).then(|| s.layout.clone())
}

/// Shapes the store draws now. Cheap enough for the once-a-second tick; only saved when changed.
pub fn capture(s: &Store, account: Option<&str>, guild: Option<reflux_model::Snowflake>, channel: Option<reflux_model::Snowflake>) {
    let Some(a) = account else { return };
    let mut layout = Layout { rail: s.guilds.values().filter(|g| !g.unavailable).count().max(1), ..Default::default() };
    if let Some(g) = guild {
        let mut rows = Vec::new();
        let mut push = |c: &reflux_model::Channel| {
            let width = (36.0 + c.name.as_deref().map_or(0.0, |n| n.chars().count() as f32 * 4.0)).clamp(24.0, 94.0);
            if c.kind == reflux_model::ChannelType::GUILD_CATEGORY {
                rows.push(Row::Category { width });
            } else if matches!(
                c.kind,
                reflux_model::ChannelType::GUILD_TEXT
                    | reflux_model::ChannelType::GUILD_VOICE
                    | reflux_model::ChannelType::GUILD_ANNOUNCEMENT
                    | reflux_model::ChannelType::GUILD_FORUM
                    | reflux_model::ChannelType::GUILD_MEDIA
            ) {
                rows.push(Row::Channel { width });
            }
        };
        let mut chans: Vec<_> = s.channels.values().filter(|c| c.guild_id == Some(g)).collect();
        chans.sort_by_key(|c| (c.parent_id.is_some(), c.position, c.id));
        for c in chans {
            push(c);
        }
        layout.rows = rows.into_iter().take(64).collect();
        if let Some(list) = channel.and_then(|ch| s.member_lists.get(&(g, ch.to_string()))) {
                let mut counts = Vec::new();
                let mut n = 0;
                for r in &list.rows {
                    match r {
                        Some(reflux_store::MemberRow::Group(_)) => {
                            if n > 0 {
                                counts.push(n);
                            }
                            n = 0;
                        }
                        Some(reflux_store::MemberRow::Member(_)) => n += 1,
                        None => {}
                    }
                }
                if n > 0 {
                    counts.push(n);
                }
            layout.member_groups = counts.into_iter().take(8).collect();
        }
    }
    if layout.rows.is_empty() && layout.member_groups.is_empty() {
        return;
    }
    let mut m = load();
    if m.accounts.get(a).is_some_and(|s| s.layout == layout) {
        return;
    }
    m.version = VERSION;
    m.accounts.insert(a.to_string(), Stored { at: now_ms(), layout });
    *cache().lock().unwrap_or_else(|e| e.into_inner()) = Some(m.clone());
    save(&m);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_and_future_stamps() {
        let now = now_ms();
        assert!(fresh(now, now));
        assert!(fresh(now - TTL_MS, now), "exactly 24 h old is still fresh");
        assert!(!fresh(now - TTL_MS - 1, now));
        assert!(fresh(now + FUTURE_MS, now), "a little clock skew is tolerated");
        assert!(!fresh(now + FUTURE_MS + 1, now));
    }
}
