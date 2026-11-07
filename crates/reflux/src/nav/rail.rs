//! Guild rail drag and drop (`app/components/layout/dnd/GuildReorderStateMachine.ts` and
//! `GuildsLayout.tsx` `applyGuildFolderDrop`): where a drag would land, and the `guild_folders`
//! setting a drop writes.
//!
//! Rune hands a drag only to the element it started on, so the rail lays its rows out here
//! ([`rows`]) and the source maps the pointer onto them.

use std::collections::HashSet;

use reflux_model::{GuildFolder, Snowflake};
use reflux_store::{GuildListItem, Store};

use crate::theme::dim;

/// `.guildListReorderTarget`: box size plus the item gap.
pub const ROW: f32 = dim::GUILD_LIST_ITEM_TARGET_SIZE;
/// `.expandedGuilds`: 1px between a folder's guilds and half a gap under the last.
pub const FOLDER_GAP: f32 = 1.0;
pub const FOLDER_PAD: f32 = dim::GUILD_LIST_ITEM_GAP / 2.0;

/// What a rail row is (a guild inside an open folder is a `Guild` too).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Guild(Snowflake),
    Folder(Option<i64>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drop {
    Before(Slot),
    After(Slot),
    /// Into a folder (its end).
    Inside(Option<i64>),
    /// Onto a top-level guild: both become a new folder.
    Combine(Snowflake),
}

/// A row's place in the rail's guild list (y from the first guild row).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub top: f32,
    pub h: f32,
    pub slot: Slot,
    /// The open folder it's in.
    pub folder: Option<Option<i64>>,
    /// The last guild of its open folder.
    pub last: bool,
}

/// The rail's draggable rows, in the order and sizes `shell::guilds::rail` draws them.
pub fn rows(store: &Store, open: &HashSet<Option<i64>>) -> Vec<Row> {
    let mut out = Vec::new();
    let mut y = 0.0;
    for item in store.guild_list() {
        match item {
            GuildListItem::Guild(id) => {
                out.push(Row { top: y, h: ROW, slot: Slot::Guild(id), folder: None, last: false });
                y += ROW;
            }
            GuildListItem::Folder { folder, guilds } => {
                out.push(Row { top: y, h: ROW, slot: Slot::Folder(folder.id), folder: None, last: false });
                y += ROW;
                if open.contains(&folder.id) {
                    let n = guilds.len();
                    for (i, g) in guilds.into_iter().enumerate() {
                        out.push(Row { top: y, h: ROW, slot: Slot::Guild(g), folder: Some(folder.id), last: i + 1 == n });
                        y += ROW + if i + 1 == n { FOLDER_PAD } else { FOLDER_GAP };
                    }
                }
            }
        }
    }
    out
}

/// Where a drag of `src` with the pointer at list-y `y` would drop (`selectIntent`, plus the
/// bottom drop zone under the last row).
pub fn resolve(rows: &[Row], src: Slot, y: f32) -> Option<Drop> {
    let src_is_folder = matches!(src, Slot::Folder(_));
    let Some(r) = rows.iter().find(|r| y >= r.top && y < r.top + r.h) else {
        // `BottomDropZone`: after the last top-level item.
        let last = rows.iter().rev().find(|r| r.folder.is_none())?;
        let bottom = rows.last().map_or(0.0, |r| r.top + r.h);
        return (y >= bottom && last.slot != src).then_some(Drop::After(last.slot));
    };
    if r.slot == src {
        return None;
    }
    let upper = y < r.top + r.h * 0.25;
    match (r.slot, r.folder) {
        (Slot::Folder(f), _) => Some(if src_is_folder || upper { Drop::Before(r.slot) } else { Drop::Inside(f) }),
        (Slot::Guild(_), Some(_)) if src_is_folder => None,
        (Slot::Guild(_), Some(_)) => Some(if r.last && y >= r.top + r.h / 2.0 { Drop::After(r.slot) } else { Drop::Before(r.slot) }),
        (Slot::Guild(g), None) => Some(if src_is_folder || upper { Drop::Before(r.slot) } else { Drop::Combine(g) }),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Top {
    Guild(Snowflake),
    Folder(GuildFolder),
}

/// The new `guild_folders` after dropping `src` (`None`: nothing changes).
pub fn apply(store: &Store, src: Slot, drop: Drop) -> Option<Vec<GuildFolder>> {
    let mut tops: Vec<Top> = store
        .guild_list()
        .into_iter()
        .map(|i| match i {
            GuildListItem::Guild(g) => Top::Guild(g),
            GuildListItem::Folder { folder, .. } => Top::Folder(folder.clone()),
        })
        .collect();
    let next_id = store.settings.guild_folders.iter().filter_map(|f| f.id).max().unwrap_or(0).max(0) + 1;
    apply_tops(&mut tops, src, drop, next_id)?;
    Some(to_folders(tops))
}

fn apply_tops(tops: &mut Vec<Top>, src: Slot, drop: Drop, next_id: i64) -> Option<()> {
    let target = match drop {
        Drop::Before(t) | Drop::After(t) => t,
        Drop::Inside(f) => Slot::Folder(f),
        Drop::Combine(g) => Slot::Guild(g),
    };
    if target == src {
        return None;
    }
    // Take the source out.
    let moved = match src {
        Slot::Folder(f) => {
            let i = tops.iter().position(|t| matches!(t, Top::Folder(x) if x.id == f))?;
            tops.remove(i)
        }
        Slot::Guild(g) => {
            if let Some(i) = tops.iter().position(|t| *t == Top::Guild(g)) {
                tops.remove(i);
            } else {
                let f = tops.iter_mut().find_map(|t| match t {
                    Top::Folder(f) if f.guild_ids.contains(&g) => Some(f),
                    _ => None,
                })?;
                f.guild_ids.retain(|&x| x != g);
            }
            Top::Guild(g)
        }
    };
    let top_index = |tops: &[Top], s: Slot| {
        tops.iter().position(|t| match (t, s) {
            (Top::Guild(a), Slot::Guild(b)) => *a == b,
            (Top::Folder(a), Slot::Folder(b)) => a.id == b,
            _ => false,
        })
    };
    match drop {
        Drop::Inside(f) => {
            let Top::Guild(g) = moved else { return None };
            let Some(Top::Folder(folder)) = tops.iter_mut().find(|t| matches!(t, Top::Folder(x) if x.id == f)) else { return None };
            folder.guild_ids.push(g);
        }
        Drop::Combine(t) => {
            let Top::Guild(g) = moved else { return None };
            let i = top_index(tops, Slot::Guild(t))?;
            tops[i] = Top::Folder(GuildFolder { id: Some(next_id), icon: "folder".into(), guild_ids: vec![t, g], ..Default::default() });
        }
        Drop::Before(t) | Drop::After(t) => {
            let after = matches!(drop, Drop::After(_));
            if let Some(i) = top_index(tops, t) {
                tops.insert(i + after as usize, moved);
            } else {
                // A guild inside a folder.
                let (Top::Guild(g), Slot::Guild(t)) = (moved, t) else { return None };
                let folder = tops.iter_mut().find_map(|x| match x {
                    Top::Folder(f) if f.guild_ids.contains(&t) => Some(f),
                    _ => None,
                })?;
                let j = folder.guild_ids.iter().position(|&x| x == t)?;
                folder.guild_ids.insert(j + after as usize, g);
            }
        }
    }
    Some(())
}

/// `buildGuildFoldersFromTopLevelItems`: runs of loose guilds become uncategorized (`-1`)
/// folders; empty folders go.
fn to_folders(tops: Vec<Top>) -> Vec<GuildFolder> {
    let mut out: Vec<GuildFolder> = Vec::new();
    let mut loose: Vec<Snowflake> = Vec::new();
    let flush = |out: &mut Vec<GuildFolder>, loose: &mut Vec<Snowflake>| {
        if !loose.is_empty() {
            out.push(GuildFolder { id: Some(-1), icon: "folder".into(), guild_ids: std::mem::take(loose), ..Default::default() });
        }
    };
    for t in tops {
        match t {
            Top::Guild(g) => loose.push(g),
            Top::Folder(f) => {
                flush(&mut out, &mut loose);
                if !f.guild_ids.is_empty() {
                    out.push(f);
                }
            }
        }
    }
    flush(&mut out, &mut loose);
    out
}

/// Folder settings "Delete folder": its guilds become uncategorized where it was.
pub fn ungroup(folders: &[GuildFolder], id: Option<i64>) -> Vec<GuildFolder> {
    folders
        .iter()
        .map(|f| if f.id == id { GuildFolder { id: Some(-1), icon: "folder".into(), guild_ids: f.guild_ids.clone(), ..Default::default() } } else { f.clone() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(n: u64) -> Snowflake {
        Snowflake(n)
    }

    fn folder(id: i64, ids: &[u64]) -> Top {
        Top::Folder(GuildFolder { id: Some(id), guild_ids: ids.iter().map(|&n| g(n)).collect(), ..Default::default() })
    }

    fn shape(tops: &[Top]) -> String {
        tops.iter()
            .map(|t| match t {
                Top::Guild(x) => x.0.to_string(),
                Top::Folder(f) => format!("f{}{:?}", f.id.unwrap_or(0), f.guild_ids.iter().map(|x| x.0).collect::<Vec<_>>()),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn run(src: Slot, drop: Drop) -> String {
        let mut t = vec![Top::Guild(g(1)), Top::Guild(g(2)), folder(7, &[5, 6]), Top::Guild(g(3))];
        match apply_tops(&mut t, src, drop, 8) {
            Some(()) => shape(&t),
            None => "none".into(),
        }
    }

    #[test]
    fn drops_follow_fluxer() {
        assert_eq!(run(Slot::Guild(g(1)), Drop::Before(Slot::Guild(g(3)))), "2 f7[5, 6] 1 3");
        assert_eq!(run(Slot::Guild(g(3)), Drop::Before(Slot::Guild(g(1)))), "3 1 2 f7[5, 6]");
        assert_eq!(run(Slot::Guild(g(1)), Drop::Inside(Some(7))), "2 f7[5, 6, 1] 3");
        assert_eq!(run(Slot::Guild(g(2)), Drop::Combine(g(1))), "f8[1, 2] f7[5, 6] 3");
        assert_eq!(run(Slot::Guild(g(5)), Drop::After(Slot::Guild(g(3)))), "1 2 f7[6] 3 5");
        assert_eq!(run(Slot::Guild(g(1)), Drop::Before(Slot::Guild(g(6)))), "2 f7[5, 1, 6] 3");
        assert_eq!(run(Slot::Guild(g(6)), Drop::Before(Slot::Guild(g(5)))), "1 2 f7[6, 5] 3");
        assert_eq!(run(Slot::Folder(Some(7)), Drop::Before(Slot::Guild(g(1)))), "f7[5, 6] 1 2 3");
        assert_eq!(run(Slot::Folder(Some(7)), Drop::Before(Slot::Guild(g(5)))), "none");
        assert_eq!(run(Slot::Guild(g(1)), Drop::Combine(g(1))), "none");
        // The folder's last guild leaving empties it.
        let mut t = vec![folder(7, &[5]), Top::Guild(g(1))];
        apply_tops(&mut t, Slot::Guild(g(5)), Drop::After(Slot::Guild(g(1))), 8).unwrap();
        let f = to_folders(t);
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].id, f[0].guild_ids.clone()), (Some(-1), vec![g(1), g(5)]));
    }

    #[test]
    fn pointer_maps_to_intents() {
        let r = |top: f32, slot, folder, last| Row { top, h: ROW, slot, folder, last };
        let rows = [
            r(0.0, Slot::Guild(g(1)), None, false),
            r(ROW, Slot::Folder(Some(7)), None, false),
            r(2.0 * ROW, Slot::Guild(g(5)), Some(Some(7)), true),
        ];
        let src = Slot::Guild(g(9));
        assert_eq!(resolve(&rows, src, 5.0), Some(Drop::Before(Slot::Guild(g(1)))));
        assert_eq!(resolve(&rows, src, 30.0), Some(Drop::Combine(g(1))));
        assert_eq!(resolve(&rows, src, ROW + 40.0), Some(Drop::Inside(Some(7))));
        assert_eq!(resolve(&rows, src, 2.0 * ROW + 40.0), Some(Drop::After(Slot::Guild(g(5)))));
        assert_eq!(resolve(&rows, src, 9.0 * ROW), Some(Drop::After(Slot::Folder(Some(7)))));
        assert_eq!(resolve(&rows, Slot::Folder(Some(7)), 30.0), Some(Drop::Before(Slot::Guild(g(1)))));
        assert_eq!(resolve(&rows, Slot::Folder(Some(7)), 2.0 * ROW + 5.0), None);
        assert_eq!(resolve(&rows, Slot::Guild(g(1)), 10.0), None);
    }
}
