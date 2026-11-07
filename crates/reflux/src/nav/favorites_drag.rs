//! Drag to reorder favorites (`FavoritesChannelListContent`'s `useDrag`/`useDrop`): a channel
//! lands before or after another (taking its category), at the end of a category dropped on its
//! header, or at the end of the uncategorized group below the list; a category lands before or
//! after another category.

use reflux_model::Snowflake;

use super::AccountPrefs;
use super::favorites::{move_category, move_channel};

/// A channel row (`pt 1` + 6 + 24 + 6) and a category header (`pt 4` + 6 + 20 + 6).
pub const CHANNEL_H: f32 = 37.0;
pub const CATEGORY_H: f32 = 36.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Slot {
    Channel(Snowflake),
    Category(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Drop {
    Before(Slot),
    After(Slot),
    /// A channel into a category's end (on its header), or the uncategorized group's (`None`).
    Into(Option<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub top: f32,
    pub h: f32,
    pub slot: Slot,
}

/// The live drag: what's held and where it would land.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub src: Slot,
    pub drop: Option<Drop>,
}

/// Where the pointer at `y` (list coordinates) would put `src`.
pub fn resolve(rows: &[Row], src: &Slot, y: f32) -> Option<Drop> {
    let Some(r) = rows.iter().find(|r| y >= r.top && y < r.top + r.h) else {
        // Below the list: a channel goes to the end of the uncategorized group.
        let bottom = rows.last().map_or(0.0, |r| r.top + r.h);
        return (matches!(src, Slot::Channel(_)) && y >= bottom).then_some(Drop::Into(None));
    };
    if r.slot == *src {
        return None;
    }
    let upper = y < r.top + r.h / 2.0;
    match (src, &r.slot) {
        (Slot::Channel(_), Slot::Category(id)) => Some(Drop::Into(Some(id.clone()))),
        (Slot::Category(_), Slot::Channel(_)) => None,
        _ => Some(if upper { Drop::Before(r.slot.clone()) } else { Drop::After(r.slot.clone()) }),
    }
}

/// Applies a drop to the favorites.
pub fn apply(a: &mut AccountPrefs, src: &Slot, drop: &Drop) {
    match (src, drop) {
        (Slot::Channel(c), Drop::Into(parent)) => {
            let n = a.favorites.iter().filter(|f| f.parent == *parent && f.channel != *c).count();
            move_channel(&mut a.favorites, *c, parent.clone(), n);
        }
        (Slot::Channel(c), Drop::Before(Slot::Channel(t)) | Drop::After(Slot::Channel(t))) => {
            let Some(parent) = a.favorites.iter().find(|f| f.channel == *t).map(|f| f.parent.clone()) else { return };
            let peers: Vec<Snowflake> = a.favorites.iter().filter(|f| f.parent == parent && f.channel != *c).map(|f| f.channel).collect();
            let Some(i) = peers.iter().position(|x| x == t) else { return };
            let i = if matches!(drop, Drop::After(_)) { i + 1 } else { i };
            move_channel(&mut a.favorites, *c, parent, i);
        }
        (Slot::Category(id), Drop::Before(Slot::Category(t)) | Drop::After(Slot::Category(t))) => {
            let Some(i) = a.favorite_categories.iter().position(|c| c.id == *t) else { return };
            move_category(&mut a.favorite_categories, id, if matches!(drop, Drop::After(_)) { i + 1 } else { i });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::favorites::{Category, Favorite};
    use super::*;

    fn prefs() -> AccountPrefs {
        let f = |c: u64, p: Option<&str>| Favorite { guild: Some(Snowflake(1)), channel: Snowflake(c), parent: p.map(Into::into), nickname: None };
        AccountPrefs {
            favorites: vec![f(1, None), f(2, None), f(3, Some("a")), f(4, Some("b"))],
            favorite_categories: vec![Category { id: "a".into(), name: "A".into() }, Category { id: "b".into(), name: "B".into() }],
            ..Default::default()
        }
    }

    fn order(a: &AccountPrefs) -> Vec<(u64, Option<String>)> {
        a.favorites.iter().map(|f| (f.channel.0, f.parent.clone())).collect()
    }

    #[test]
    fn drops_reorder_and_recategorize() {
        let ch = |c: u64| Slot::Channel(Snowflake(c));
        let rows = vec![
            Row { top: 0.0, h: CHANNEL_H, slot: ch(1) },
            Row { top: 37.0, h: CHANNEL_H, slot: ch(2) },
            Row { top: 74.0, h: CATEGORY_H, slot: Slot::Category("a".into()) },
            Row { top: 110.0, h: CHANNEL_H, slot: ch(3) },
        ];
        assert_eq!(resolve(&rows, &ch(1), 50.0), Some(Drop::Before(ch(2))));
        assert_eq!(resolve(&rows, &ch(1), 70.0), Some(Drop::After(ch(2))));
        assert_eq!(resolve(&rows, &ch(1), 80.0), Some(Drop::Into(Some("a".into()))));
        assert_eq!(resolve(&rows, &ch(1), 10.0), None, "onto itself");
        assert_eq!(resolve(&rows, &ch(3), 400.0), Some(Drop::Into(None)));
        assert_eq!(resolve(&rows, &Slot::Category("a".into()), 10.0), None, "a category doesn't go among channels");

        let mut a = prefs();
        apply(&mut a, &ch(1), &Drop::After(ch(2)));
        assert_eq!(order(&a)[..2], [(2, None), (1, None)]);
        apply(&mut a, &ch(1), &Drop::Before(ch(3)));
        assert_eq!(order(&a)[1..3], [(1, Some("a".into())), (3, Some("a".into()))]);
        apply(&mut a, &ch(4), &Drop::Into(None));
        assert_eq!(order(&a)[1], (4, None), "the end of the uncategorized group");
        apply(&mut a, &Slot::Category("b".into()), &Drop::Before(Slot::Category("a".into())));
        assert_eq!(a.favorite_categories.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
    }
}
