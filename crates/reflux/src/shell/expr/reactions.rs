//! Who reacted: the pill's `ReactionTooltip`, `MessageReactionsModal`, its removal buttons,
//! the "Remove all reactions" confirmation and the message menu's reaction items.

use crate::ui::SmoothWheel;
use reflux_model::{Message, Permissions, ReactionEmoji, Snowflake};
use reflux_store::Store;
use rune::{El, Tx, col, el, row, text};
use serde_json::Value;

use super::{Fetched, ShellAccess, ShellState, enc, send};
use crate::icons;
use crate::media::Load;
use crate::theme::{Palette, font};
use crate::ui::overlay::{anchor, popout_shadow};
use crate::ui::{Avatar, Button, ButtonVariant, Menu, MenuEntry, MenuItem, Modal, ModalSize, Placement, over, scroller};
use crate::i18n::t;

/// `useHoverFloatingTooltip(500)`, and how long it outlives the pointer.
const OPEN_MS: u64 = 500;
const LINGER_MS: u64 = 300;

/// A reaction's identity in keys: the custom emoji's ID, or the unicode surrogate.
pub fn reactor_tip_key(e: &ReactionEmoji) -> String {
    e.id.map_or_else(|| e.name.clone(), |id| id.to_string())
}

/// The `{emoji}` path segment: `name:id`, or the surrogate.
fn segment(e: &ReactionEmoji) -> String {
    enc(&e.id.map_or_else(|| e.name.clone(), |id| format!("{}:{id}", e.name)))
}

fn reactors_key(message: Snowflake, key: &str, limit: u32) -> String {
    format!("reactors:{message}:{key}:{limit}")
}

fn find(store: &Store, channel: Snowflake, message: Snowflake) -> Option<&Message> {
    let ms = store.messages(channel);
    ms.binary_search_by_key(&message, |m| m.id).ok().map(|i| &ms[i])
}

fn reaction<'a>(m: &'a Message, key: &str) -> Option<&'a reflux_model::Reaction> {
    m.reactions.iter().find(|r| reactor_tip_key(&r.emoji) == key)
}

fn fetch_reactors(st: &mut ShellState, channel: Snowflake, message: Snowflake, e: &ReactionEmoji, limit: u32) {
    let path = format!("/channels/{channel}/messages/{message}/reactions/{}?limit={limit}", segment(e));
    super::fetch(st, &reactors_key(message, &reactor_tip_key(e), limit), &path);
}

/// Hover tracking for a reaction pill keyed `pill-{message}-{key}`.
pub fn pill_hover<S: 'static>(pill: El, channel: Snowflake, message: Snowflake, key: String, acc: ShellAccess<S>) -> El {
    pill.on_hover_change(move |s: &mut S, inside, tx: &mut Tx| hover(acc(s), (channel, message, key.clone()), inside, tx))
}

fn hover(st: &mut ShellState, at: (Snowflake, Snowflake, String), inside: bool, tx: &mut Tx) {
    let x = &mut st.expr;
    let now = crate::app::now_ms();
    if inside {
        if x.tip.as_ref() != Some(&at) {
            (x.tip, x.tip_since) = (Some(at), now);
        }
        x.tip_left = None;
        tx.wake_after(std::time::Duration::from_millis(OPEN_MS));
    } else if x.tip.as_ref() == Some(&at) {
        x.tip_left = Some(now);
        tx.wake_after(std::time::Duration::from_millis(LINGER_MS));
    }
}

/// Drops a tooltip the pointer left, and loads the first reactors of one that's showing.
pub(super) fn poll(store: &Store, st: &mut ShellState) -> bool {
    let now = crate::app::now_ms();
    if st.expr.tip_left.is_some_and(|t| now >= t + LINGER_MS) {
        (st.expr.tip, st.expr.tip_left) = (None, None);
        return true;
    }
    if let Some((c, m, k)) = st.expr.tip.clone()
        && now >= st.expr.tip_since + OPEN_MS
        && let Some(r) = find(store, c, m).and_then(|msg| reaction(msg, &k))
    {
        let e = r.emoji.clone();
        fetch_reactors(st, c, m, &e, 3);
    }
    false
}

/// Opens `MessageReactionsModal` on `e`.
fn open(st: &mut ShellState, channel: Snowflake, message: Snowflake, e: &ReactionEmoji) {
    st.expr.reactions = Some((channel, message, reactor_tip_key(e)));
    (st.expr.tip, st.expr.tip_left) = (None, None);
    st.popups.close();
    fetch_reactors(st, channel, message, e, 100);
}

/// A page's reactors: `ReactionUsersPageResponse.items`, or a bare array.
fn items(v: &Value) -> Option<&Vec<Value>> {
    v.as_array().or_else(|| v["items"].as_array())
}

fn users<'a>(st: &'a ShellState, message: Snowflake, key: &str, limit: u32) -> Option<&'a Vec<Value>> {
    match st.expr.fetched.get(&reactors_key(message, key, limit)) {
        Some(Fetched::Done(v)) => items(v),
        _ => None,
    }
}

/// Every loaded page of the modal's reactors (100 each, chained by `after`), and the cursor
/// of the next page while `has_more`.
fn all_users(st: &ShellState, message: Snowflake, key: &str) -> Option<(Vec<Value>, Option<String>)> {
    let base = reactors_key(message, key, 100);
    let mut k = base.clone();
    let mut out = Vec::new();
    loop {
        let v = match st.expr.fetched.get(&k) {
            Some(Fetched::Done(v)) => v,
            _ if out.is_empty() => return None,
            _ => return Some((out, None)),
        };
        let page = items(v).cloned().unwrap_or_default();
        let more = v["has_more"].as_bool().unwrap_or(page.len() >= 100);
        let next = v["next_after"].as_str().map(str::to_string).or_else(|| page.last().and_then(|u| u["id"].as_str()).map(str::to_string));
        out.extend(page);
        match next.filter(|_| more) {
            Some(n) => {
                k = format!("{base}:after:{n}");
                if !st.expr.fetched.contains_key(&k) {
                    return Some((out, Some(n)));
                }
            }
            None => return Some((out, None)),
        }
    }
}

fn name(u: &Value) -> String {
    u["global_name"].as_str().or(u["username"].as_str()).unwrap_or("Unknown").to_string()
}

/// `formatList` (conjunction, long) of up to three names plus "N others".
fn reacted_by(e: &ReactionEmoji, names: &[String], count: u32) -> String {
    let mut parts = names.to_vec();
    let others = (count as usize).saturating_sub(names.len());
    if others > 0 {
        parts.push(crate::i18n::tp("{count, plural, one {# other} other {# others}}", others as i64, &[]));
    }
    let list = crate::i18n::list_and(&parts);
    let label = if e.id.is_some() { format!(":{}:", e.name) } else { crate::shell::emoji::name_of(&e.name).map_or(e.name.clone(), |n| format!(":{n}:")) };
    let count = names.len().max(count as usize).to_string();
    crate::i18n::tf("{reactorCount, plural, one {{emojiName} reacted by {reactors}} other {{emojiName} reacted by {reactors}}}", &[("reactorCount", &count), ("emojiName", &label), ("reactors", &list)])
}

/// A reaction's emoji at `size`.
fn emoji(st: &ShellState, e: &ReactionEmoji, size: f32) -> El {
    let load = match e.id {
        Some(id) => st.media.asset(|b, _| crate::media::emoji_url(b, &id.to_string())),
        None => crate::shell::emoji::sprite_index(&e.name).map_or(Load::Failed, |i| st.media.sprite(i)),
    };
    match load {
        Load::Ready(px) => el().size(size, size).pixels_fit(&px, rune::SurfaceFit::Fit),
        _ => text(e.name.clone()).font(font(size * 0.8, 400.0).lh(size)),
    }
}

fn user_avatar(st: &ShellState, u: &Value, size: f32, p: &Palette) -> El {
    let id: Snowflake = u["id"].as_str().and_then(|s| s.parse().ok()).unwrap_or_default();
    let px = match u["avatar"].as_str().map(|h| st.media.asset(|b, s| crate::media::avatar_url(b, id, h, size, s))) {
        Some(Load::Ready(px)) => Some(px),
        _ => st.default_avatar(id).map(|i| i.pixels().clone()),
    };
    Avatar::new(size).pixels(px.as_ref()).el(p)
}

/// The tooltip, the modal and the confirmation.
pub fn layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if let Some((c, m, k)) = &st.expr.tip
        && st.expr.reactions.is_none()
        && crate::app::now_ms() >= st.expr.tip_since + OPEN_MS
        && let Some(msg) = find(store, *c, *m)
        && let Some(r) = reaction(msg, k)
    {
        layer.push(anchor(tooltip(st, *c, *m, r, p, acc), &format!("pill-{m}-{k}"), Placement::Top, 8.0));
    }
    if let Some((c, m, k)) = &st.expr.reactions
        && let Some(msg) = find(store, *c, *m)
    {
        layer.push(modal(store, st, msg, k, p, acc));
    }
    if let Some((c, m)) = st.expr.confirm_clear {
        layer.push(confirm(c, m, p, acc));
    }
}

/// `EmojiTooltipContent`: a 32px emoji, who reacted and "Click to view all reactions".
fn tooltip<S: 'static>(st: &ShellState, c: Snowflake, m: Snowflake, r: &reflux_model::Reaction, p: &Palette, acc: ShellAccess<S>) -> El {
    let key = reactor_tip_key(&r.emoji);
    let bg = p.form_surface_background;
    let line = users(st, m, &key, 3).map(|u| reacted_by(&r.emoji, &u.iter().take(3).map(name).collect::<Vec<_>>(), r.count));
    let (tip, e) = ((c, m, key.clone()), r.emoji.clone());
    let lines = col()
        .ml(12.0)
        .min_w(160.0)
        .min_h(56.0)
        .gap(4.0)
        .justify_center()
        .shrink()
        .child(match line {
            Some(l) => text(l).font(font(13.0, 500.0).lh(16.25)).color(p.text_primary).wrap(),
            None => text(t("Loading…")).font(font(13.0, 500.0).lh(16.25)).color(p.text_primary_muted),
        })
        .child(text(t("Click to view all reactions")).font(font(12.0, 400.0).lh(15.0)).color(p.text_primary_muted));
    row()
        .max_w(288.0)
        .padding(12.0)
        .center_y()
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .box_shadow(popout_shadow(p, 0.22, 0.12))
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .key(format!("reaction-tip-{m}"))
        .child(emoji(st, &r.emoji, 32.0))
        .child(lines)
        .on_hover_change(move |s: &mut S, inside, tx: &mut Tx| hover(acc(s), tip.clone(), inside, tx))
        .on_press(move |s: &mut S| open(acc(s), c, m, &e))
}

/// `MessageReactionsModal`: a 96px filter column, then who reacted with the selected one.
fn modal<S: 'static>(store: &Store, st: &ShellState, m: &Message, sel: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let (c, mid) = (m.channel_id, m.id);
    let manage = store.permissions(c).contains(Permissions::MANAGE_MESSAGES);
    let me = store.my_id();
    let mut filters = col().w_full().gap(4.0).padding(8.0);
    for r in &m.reactions {
        let key = reactor_tip_key(&r.emoji);
        let on = key == sel;
        let e = r.emoji.clone();
        let mut b = row()
            .w_full()
            .h(32.0)
            .px(8.0)
            .gap(6.0)
            .center_y()
            .radius(6.0)
            .cursor(rune::Cursor::Pointer)
            .role("tab")
            .label(e.name.clone())
            .key(format!("filter-{key}"))
            .child(emoji(st, &r.emoji, 20.0))
            .child(text(r.count.to_string()).font(font(14.0, 600.0).lh(16.0)).color(if on { p.text_primary } else { p.text_primary_muted }))
            .on_press(move |s: &mut S| open(acc(s), c, mid, &e));
        b = if on { b.fill(p.background_modifier_selected) } else { b.hover(p.background_modifier_hover) };
        filters = filters.child(b);
    }
    let selected = reaction(m, sel).map(|r| r.emoji.clone());
    let list = match (all_users(st, mid, sel), selected) {
        (Some((us, next)), Some(e)) => {
            let mut rows = Vec::new();
            for u in &us {
                let uid: Snowflake = u["id"].as_str().and_then(|s| s.parse().ok()).unwrap_or_default();
                let mut r = row()
                    .w_full()
                    .h(44.0)
                    .px(8.0)
                    .gap(12.0)
                    .center_y()
                    .radius(6.0)
                    .hover(p.background_modifier_hover)
                    .hover_group()
                    .key(format!("reactor-{uid}"))
                    .child(user_avatar(st, u, 24.0, p))
                    .child(
                        row()
                            .grow(1.0)
                            .min_w(0.0)
                            .gap(6.0)
                            .center_y()
                            .child(text(name(u)).font(font(16.0, 500.0).lh(20.0)).color(p.text_primary).truncate().shrink())
                            .child(text(u["username"].as_str().unwrap_or_default().to_string()).font(font(14.0, 400.0).lh(18.0)).color(p.text_primary_muted).truncate().shrink()),
                    );
                if manage || uid == me {
                    let (e, sel) = (e.clone(), sel.to_string());
                    r = r.child(
                        row()
                            .size(24.0, 24.0)
                            .center()
                            .radius(4.0)
                            .cursor(rune::Cursor::Pointer)
                            .role("button")
                            .label(t("Remove reaction"))
                            .group_opacity(0.0, 1.0)
                            .child(rune::icon(&icons::X_BOLD, 16.0).color(p.text_primary_muted))
                            .on_press(move |s: &mut S| remove(acc(s), c, mid, &e, &sel, uid, me)),
                    );
                }
                rows.push(r);
            }
            let n = rows.len();
            if next.is_some() {
                rows.push(row().w_full().h(44.0).center().key(format!("reactors-more-{mid}")).child(text(t("Loading reactions")).font(font(14.0, 400.0)).color(p.text_primary_muted)));
            }
            // `MessageReactionsReactorsList` loads the next page when its end scrolls into view.
            let sel = sel.to_string();
            let l = col().w_full().gap(2.0).growth(rune::GrowthSpec::default()).virtualize(46.0).children(rows).on_visible_rows(move |s: &mut S, r| {
                if let Some(after) = next.as_ref().filter(|_| r.end > n) {
                    let path = format!("/channels/{c}/messages/{mid}/reactions/{}?limit=100&after={after}", segment(&e));
                    super::fetch(acc(s), &format!("{}:after:{after}", reactors_key(mid, &sel, 100)), &path);
                }
                false
            });
            scroller(col().w_full().h_full().scroll(rune::ScrollSpec { stick_bottom: false }).smooth_wheel::<S>("reaction-list").key("reaction-list").child(l), false, p).key("list")
        }
        _ => col().w_full().h_full().center().key("loading").child(text(t("Loading reactions")).font(font(14.0, 400.0)).color(p.text_primary_muted)),
    };
    let body = row()
        .w_full()
        .h(360.0)
        .radius(8.0)
        .clip_rounded(8.0)
        .fill(p.background_primary)
        .child(col().w(96.0).h_full().fill(p.background_secondary).child(scroller(col().w_full().h_full().scroll(rune::ScrollSpec { stick_bottom: false }).smooth_wheel::<S>("reaction-filters").key("reaction-filters").child(filters), false, p)))
        .child(col().grow(1.0).min_w(0.0).h_full().padding(8.0).child(list));
    Modal::new(t("Reactions"), move |s: &mut S| acc(s).expr.reactions = None).size(ModalSize::Medium).body(body).el(p)
}

/// Removes one reactor: `@me` for your own, the user's ID with Manage Messages.
fn remove(st: &mut ShellState, c: Snowflake, m: Snowflake, e: &ReactionEmoji, key: &str, user: Snowflake, me: Snowflake) {
    let who = if user == me { "@me".to_string() } else { user.to_string() };
    send(st, reqwest::Method::DELETE, format!("/channels/{c}/messages/{m}/reactions/{}/{who}", segment(e)), None);
    let base = reactors_key(m, key, 100);
    for (_, f) in st.expr.fetched.iter_mut().filter(|(k, _)| k.starts_with(&base)) {
        if let Fetched::Done(v) = f
            && let Some(us) = if v.is_array() { v.as_array_mut() } else { v["items"].as_array_mut() }
        {
            us.retain(|u| u["id"].as_str() != Some(&user.to_string()));
        }
    }
}

fn confirm<S: 'static>(c: Snowflake, m: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let close = move |s: &mut S| acc(s).expr.confirm_clear = None;
    Modal::new(t("Remove all reactions"), close)
        .body(text("Are you sure you want to remove all reactions from this message?").font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap())
        .footer(
            row()
                .gap(8.0)
                .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
                .child(
                    Button::new(t("Remove all"))
                        .variant(ButtonVariant::Danger)
                        .key("clear-reactions-confirm")
                        .on_press(move |s: &mut S| {
                            let st = acc(s);
                            st.expr.confirm_clear = None;
                            send(st, reqwest::Method::DELETE, format!("/channels/{c}/messages/{m}/reactions"), None);
                        })
                        .el(p),
                ),
        )
        .el(p)
}

/// "View reactions", and with Manage Messages "Remove reactions" (one emoji) and
/// "Remove all reactions" (`MessageContextMenu`'s reaction group).
pub fn message_menu_items<S: 'static>(mut menu: Menu<S>, store: &Store, m: &Message, acc: ShellAccess<S>) -> Menu<S> {
    if m.reactions.is_empty() {
        return menu;
    }
    let (c, mid) = (m.channel_id, m.id);
    let first = m.reactions[0].emoji.clone();
    menu = menu.item(MenuItem::new("view-reactions", t("View reactions")).icon(&icons::SMILEY_FILL).on_select(move |s: &mut S| open(acc(s), c, mid, &first)));
    if !store.permissions(c).contains(Permissions::MANAGE_MESSAGES) {
        return menu;
    }
    let one: Vec<MenuEntry<S>> = m
        .reactions
        .iter()
        .map(|r| {
            let e = r.emoji.clone();
            let label = if e.id.is_some() { format!(":{}:", e.name) } else { e.name.clone() };
            MenuEntry::Item(MenuItem::new(format!("remove-reaction-{}", reactor_tip_key(&e)), label).danger().on_select(move |s: &mut S| {
                send(acc(s), reqwest::Method::DELETE, format!("/channels/{c}/messages/{mid}/reactions/{}", segment(&e)), None)
            }))
        })
        .collect();
    menu.submenu(format!("remove-reactions-{mid}"), t("Remove reactions"), one).item(
        MenuItem::new("remove-all-reactions", t("Remove all reactions")).icon(&icons::TRASH_FILL).danger().on_select(move |s: &mut S| acc(s).expr.confirm_clear = Some((c, mid))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_line_names_three_then_others() {
        let e = ReactionEmoji { name: "👍".into(), ..Default::default() };
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(reacted_by(&e, &n(&["A"]), 1), ":thumbsup: reacted by A");
        assert_eq!(reacted_by(&e, &n(&["A", "B", "C"]), 5), ":thumbsup: reacted by A, B, C, and 2 others");
        let c = ReactionEmoji { id: Some(Snowflake(7)), name: "party".into(), animated: None };
        assert_eq!(reacted_by(&c, &n(&["A", "B"]), 2), ":party: reacted by A and B");
        assert_eq!(segment(&c), "party%3A7");
    }

    #[test]
    fn modal_pages_chain_by_after() {
        let mut st = ShellState::default();
        let m = Snowflake(1);
        let page = |a: u64, b: u64| (a..b).map(|i| serde_json::json!({ "id": i.to_string() })).collect::<Vec<_>>();
        st.expr.fetched.insert(reactors_key(m, "x", 100), Fetched::Done(serde_json::json!({ "items": page(0, 100), "has_more": true, "next_after": "99" })));
        let (us, next) = all_users(&st, m, "x").unwrap();
        assert_eq!((us.len(), next.as_deref()), (100, Some("99")));
        st.expr.fetched.insert(format!("{}:after:99", reactors_key(m, "x", 100)), Fetched::Done(Value::Array(page(100, 130))));
        let (us, next) = all_users(&st, m, "x").unwrap();
        assert_eq!((us.len(), next), (130, None));
    }
}
