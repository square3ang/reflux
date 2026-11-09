//! `ExpressionInfoCard` in its popout: an emoji's or sticker's preview, what it is, the
//! community it's from (go there, or join it when it's discoverable), a favorite toggle and
//! "Clone emoji to…" (`CloneExpressionMenuItem`) for communities you manage.
//! Fluxer keeps favorite and clone in the expression's context menu; here they sit on the card.

use reflux_markdown::EmojiKind;
use reflux_model::{Permissions, Snowflake, StickerItem};
use reflux_store::Store;
use rune::{El, col, el, row, text};

use super::{Fetched, ShellAccess, ShellState, guild_icon};
use crate::icons;
use crate::theme::{Palette, font};
use crate::ui::overlay::{anchor, popout_shadow};
use crate::ui::{Placement, mix, over};
use crate::i18n::t;

#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub sticker: bool,
    /// Custom emoji and stickers; unicode emoji have none.
    pub id: Option<Snowflake>,
    pub name: String,
    pub animated: bool,
    /// A unicode emoji's surrogate.
    pub glyph: Option<String>,
    /// The element it opens over.
    pub anchor: String,
    /// "Clone … to" is expanded.
    pub cloning: bool,
}

fn meta_key(i: &Info) -> String {
    format!("meta:{}:{}", if i.sticker { "sticker" } else { "emoji" }, i.id.unwrap_or_default())
}

fn source_key(i: &Info) -> String {
    format!("source:{}:{}", if i.sticker { "sticker" } else { "emoji" }, i.id.unwrap_or_default())
}

fn open(st: &mut ShellState, info: Info) {
    if let Some(id) = info.id {
        let kind = if info.sticker { "stickers" } else { "emojis" };
        super::fetch(st, &meta_key(&info), &format!("/{kind}/{id}/metadata"));
        // `ExpressionSource`: the owning guild when it's discoverable or you're in it.
        super::fetch(st, &source_key(&info), &format!("/{kind}/{id}/source"));
    }
    st.popups.close();
    st.expr.info = Some(info);
}

pub fn open_emoji_info(st: &mut ShellState, kind: &EmojiKind, anchor: String) {
    let info = match kind {
        EmojiKind::Custom { name, id, animated } => Info { sticker: false, id: id.parse().ok(), name: name.clone(), animated: *animated, glyph: None, anchor, cloning: false },
        EmojiKind::Standard { raw, name, .. } => Info { sticker: false, id: None, name: name.clone(), animated: false, glyph: Some(raw.clone()), anchor, cloning: false },
    };
    open(st, info);
}

pub fn open_sticker_info(st: &mut ShellState, s: &StickerItem, anchor: String) {
    open(st, Info { sticker: true, id: Some(s.id), name: s.name.clone(), animated: s.animated, glyph: None, anchor, cloning: false });
}

/// The guild it belongs to: one you're in, else the metadata's.
fn source(store: &Store, st: &ShellState, i: &Info) -> (Option<Snowflake>, bool) {
    let id = i.id;
    let mine = store.guilds.iter().find(|(_, g)| {
        if i.sticker { g.stickers.iter().any(|s| Some(s.id) == id) } else { g.emojis.iter().any(|e| Some(e.id) == id) }
    });
    if let Some((g, _)) = mine {
        return (Some(*g), true);
    }
    let meta = match st.expr.fetched.get(&meta_key(i)) {
        Some(Fetched::Done(v)) => v["guild_id"].as_str().and_then(|g| g.parse().ok()),
        _ => None,
    };
    (meta, false)
}

fn allow_cloning(st: &ShellState, i: &Info) -> bool {
    matches!(st.expr.fetched.get(&meta_key(i)), Some(Fetched::Done(v)) if v["allow_cloning"].as_bool() == Some(true))
}

pub fn layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if let Some(j) = &st.expr.join {
        layer.push(join_confirm(st, j, p, acc));
    }
    let Some(i) = &st.expr.info else { return };
    let close = move |s: &mut S| acc(s).expr.info = None;
    layer.push(anchor(card(store, st, i, p, acc).modal(true, close), &i.anchor, Placement::Top, 8.0));
}

/// `ExpressionJoinConfirmation` in its `ConfirmModal`.
fn join_confirm<S: 'static>(st: &ShellState, j: &(Snowflake, String, Option<String>, String, bool), p: &Palette, acc: ShellAccess<S>) -> El {
    let (g, name, icon, expr, sticker) = j.clone();
    let close = move |s: &mut S| acc(s).expr.join = None;
    let reason = if sticker {
        crate::i18n::tf("{expressionName} is a custom sticker from this community. Members can use it here.", &[("expressionName", &expr)])
    } else {
        crate::i18n::tf("{expressionName} is a custom emoji from this community. Members can use it here.", &[("expressionName", &expr)])
    };
    let body = col()
        .w_full()
        .gap(16.0)
        .items_center()
        .child(super::guild_face(st, g, &name, icon.as_deref(), 80.0, p))
        .child(col().items_center().gap(2.0).child(text(name.clone()).font(font(18.0, 700.0).lh(22.0)).color(p.text_primary)).child(text(t("Discoverable community")).font(font(13.0, 400.0).lh(16.0)).color(p.text_primary_muted)))
        .child(text(reason).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap().align(rune::Align::Center))
        .child(text(crate::i18n::tf("Do you want to join {communityName}?", &[("communityName", &name)])).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary).wrap().align(rune::Align::Center));
    let footer = row()
        .gap(8.0)
        .child(crate::ui::Button::new(t("Cancel")).variant(crate::ui::ButtonVariant::Secondary).on_press(close).el(p))
        .child(crate::ui::Button::new(t("Join community")).key("expr-join").on_press(move |s: &mut S| {
            let st = acc(s);
            st.expr.join = None;
            st.social(reqwest::Method::POST, format!("/discovery/guilds/{g}/join"), None, crate::social::Done::EnterGuild(g));
        }).el(p));
    crate::ui::Modal::new(t("Join community"), close).body(body).footer(footer).el(p)
}

/// `slotsFull`: the target has no room for another emoji or sticker.
pub(super) fn slots_full(store: &Store, st: &ShellState, g: Snowflake, sticker: bool) -> bool {
    let Some(x) = store.guilds.get(&g) else { return false };
    let features = x.info.as_ref().map(|i| i.features.as_slice()).unwrap_or_default();
    let (emojis, stickers) = crate::shell::upload::guild_limits(st, features);
    let (unlimited, count, max) = if sticker { ("UNLIMITED_STICKERS", x.stickers.len(), stickers) } else { ("UNLIMITED_EMOJI", x.emojis.len(), emojis) };
    !features.iter().any(|f| f == unlimited) && count as u64 >= max
}

fn card<S: 'static>(store: &Store, st: &ShellState, i: &Info, p: &Palette, acc: ShellAccess<S>) -> El {
    let bg = p.form_surface_background;
    let border = over(p.background_modifier_accent, bg);
    let what = if i.sticker { "sticker" } else { "emoji" };
    let (guild, member) = source(store, st, i);
    let preview = match (i.sticker, i.id) {
        (true, Some(id)) => super::sticker::image(&st.media, id, 48.0),
        (false, Some(id)) => super::emoji::glyph(&st.media, &super::Item::Emoji { guild: guild.unwrap_or_default(), id, name: i.name.clone(), animated: i.animated }, 0, 48.0),
        _ => match i.glyph.as_deref().and_then(crate::shell::emoji::sprite_index).map(|x| st.media.sprite(x)) {
            Some(crate::media::Load::Ready(px)) => el().size(48.0, 48.0).pixels_fit(&px, rune::SurfaceFit::Fit),
            _ => text(i.glyph.clone().unwrap_or_default()).font(font(40.0, 400.0).lh(48.0)),
        },
    };
    let description = match (i.id, guild, member) {
        (None, ..) => "A default emoji. You can use it anywhere on Fluxer.".to_string(),
        (_, Some(_), true) if st.limits.global_expressions => crate::i18n::tf(if i.sticker { "A custom sticker from this community. You can use it anywhere on {productName}." } else { "A custom emoji from this community. You can use it anywhere on {productName}." }, &[("productName", "Fluxer")]),
        (_, Some(_), true) => t(if i.sticker { "A custom sticker from this community. You can use it in this community." } else { "A custom emoji from this community. You can use it in this community." }).to_string(),
        _ => t(if i.sticker { "This is a custom sticker from a community." } else { "This is a custom emoji from a community." }).to_string(),
    };
    let name = if i.sticker || i.id.is_none() && i.glyph.is_none() { i.name.clone() } else { format!(":{}:", i.name) };
    let summary = row()
        .w_full()
        .padding(14.0)
        .gap(12.0)
        .items_start()
        .child(preview)
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(text(name).font(font(15.0, 600.0).lh(18.75)).color(p.text_primary).wrap())
                .child(text(description).font(font(13.0, 400.0).lh(18.2)).color(p.text_secondary).wrap()),
        );
    let band = mix(p.text_primary, rune::Color::TRANSPARENT, 4.0);
    let section = |label: String, body: El| {
        col()
            .w_full()
            .py(12.0)
            .px(14.0)
            .gap(8.0)
            .fill(band)
            .box_shadow(rune::Shadow { x: 0.0, y: 1.0, blur: 0.0, spread: 0.0, color: border, inset: true })
            .child(text(label.to_uppercase()).font(font(11.0, 600.0).lh(13.75).tracking(0.5)).color(p.text_primary_muted))
            .child(body)
    };
    let mut c = col()
        .w(320.0)
        .radius(12.0)
        .clip_rounded(12.0)
        .fill(bg)
        .border(1.0, border)
        .box_shadow(popout_shadow(p, 0.22, 0.12))
        .role("dialog")
        .label(format!("{what} info"))
        .key("expr-info")
        .child(summary);
    if i.id.is_some() {
        let from = t(if i.sticker { "This sticker is from" } else { "This emoji is from" }).to_string();
        let gname = guild.and_then(|g| store.guilds.get(&g)).and_then(|g| g.info.as_ref()).map(|x| x.name.clone());
        let body = match (guild, gname) {
            (Some(g), Some(n)) if member => row()
                .w_full()
                .padding(4.0)
                .mx(-4.0)
                .gap(10.0)
                .center_y()
                .radius(6.0)
                .hover(mix(p.text_primary, rune::Color::TRANSPARENT, 8.0))
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(crate::i18n::tf("Go to {communityName}", &[("communityName", &n)]))
                .key("member")
                .child(guild_icon(store, st, g, 40.0, p))
                .child(text(n).font(font(14.0, 600.0).lh(17.5)).color(p.text_primary).truncate().shrink().min_w(0.0))
                .on_press(move |s: &mut S| {
                    let st = acc(s);
                    st.expr.info = None;
                    st.select_guild(Some(g), None);
                }),
            _ => match st.expr.fetched.get(&source_key(i)) {
                // `ExpressionSourceGuildRow` for a guild you aren't in.
                Some(Fetched::Done(v)) if v["id"].as_str().is_some() => {
                    let sg: Snowflake = v["id"].as_str().and_then(|x| x.parse().ok()).unwrap_or_default();
                    let n = v["name"].as_str().unwrap_or_default().to_string();
                    let icon = v["icon"].as_str().map(str::to_owned);
                    let discoverable = v["features"].as_array().is_some_and(|f| f.iter().any(|x| x == "DISCOVERABLE"));
                    let sub = if discoverable { t("Discoverable community") } else { t("Invite-only community") };
                    let mut r = row()
                        .w_full()
                        .padding(4.0)
                        .mx(-4.0)
                        .gap(10.0)
                        .center_y()
                        .radius(6.0)
                        .key("source")
                        .child(super::guild_face(st, sg, &n, icon.as_deref(), 40.0, p))
                        .child(col().gap(2.0).shrink().min_w(0.0).child(text(n.clone()).font(font(14.0, 600.0).lh(17.5)).color(p.text_primary).truncate()).child(text(sub).font(font(12.0, 400.0).lh(15.0)).color(p.text_primary_muted)));
                    if discoverable {
                        let expr = if i.sticker { i.name.clone() } else { format!(":{}:", i.name) };
                        let sticker = i.sticker;
                        r = r
                            .hover(mix(p.text_primary, rune::Color::TRANSPARENT, 8.0))
                            .cursor(rune::Cursor::Pointer)
                            .role("button")
                            .label(crate::i18n::tf("Join {communityName}", &[("communityName", &n)]))
                            .key("expr-info-join")
                            .on_press(move |s: &mut S| {
                                let st = acc(s);
                                st.expr.info = None;
                                st.expr.join = Some((sg, n.clone(), icon.clone(), expr.clone(), sticker));
                            });
                    }
                    r
                }
                Some(Fetched::Loading) => row().w_full().h(48.0).key("loading"),
                _ => row().w_full().min_h(40.0).center_y().key("unavailable").child(text(t("A community that is either invite-only or unavailable.")).font(font(12.0, 400.0).lh(15.0)).color(p.text_primary_muted).wrap()),
            },
        };
        c = c.child(section(from, body));
    }
    if let (Some(id), Some(g)) = (i.id, guild) {
        let key = if i.sticker {
            format!("sticker:{g}:{id}")
        } else {
            format!("custom:{g}:{id}")
        };
        let fav = st.expr.is_favorite(&key);
        let mut actions = col().w_full().gap(2.0).child(action(
            if fav { &icons::STAR_FILL } else { &icons::STAR },
            if fav { t("Remove from favorites") } else { t("Add to favorites") },
            p,
            move |s: &mut S| acc(s).expr.toggle_favorite(&key),
        ));
        // `CloneExpressionMenuItem`: communities you manage expressions in, other than its own,
        // full ones disabled.
        // Only with Advanced's "Emoji and sticker cloning shortcuts" on.
        if crate::look::with(|l| l.clone_shortcuts) && (allow_cloning(st, i) || member) {
            let mut targets: Vec<(Snowflake, String)> = store
                .guilds
                .iter()
                .filter(|(gid, x)| **gid != g && store.guild_permissions(**gid).contains(Permissions::MANAGE_EXPRESSIONS) && x.info.is_some())
                .map(|(gid, x)| (*gid, x.info.as_ref().map(|i| i.name.clone()).unwrap_or_default()))
                .collect();
            targets.sort_by(|a, b| a.1.cmp(&b.1));
            if !targets.is_empty() {
                let open = i.cloning;
                let label = if i.sticker { t("Clone sticker to…") } else { t("Clone emoji to...") };
                actions = actions.child(action(if open { &icons::CARET_DOWN_BOLD } else { &icons::CARET_RIGHT_BOLD }, label, p, move |s: &mut S| {
                    if let Some(i) = acc(s).expr.info.as_mut() {
                        i.cloning = !i.cloning;
                    }
                }));
            }
            for (tg, n) in targets.into_iter().filter(|_| i.cloning) {
                let full = slots_full(store, st, tg, i.sticker);
                let (kind, field) = if i.sticker { ("stickers", "source_sticker_id") } else { ("emojis", "source_emoji_id") };
                let body = serde_json::json!({ field: id.to_string() });
                let done = if i.sticker {
                    crate::i18n::tf("Cloned sticker {stickerName} to {targetName}", &[("stickerName", &i.name), ("targetName", &n)])
                } else {
                    crate::i18n::tf("Cloned :{sourceName}: to {targetName}", &[("sourceName", &i.name), ("targetName", &n)])
                };
                let mut item = row()
                    .w_full()
                    .h(32.0)
                    .pl(34.0)
                    .pr(8.0)
                    .gap(8.0)
                    .center_y()
                    .radius(6.0)
                    .role("button")
                    .label(n.clone())
                    .child(guild_icon(store, st, tg, 16.0, p))
                    .child(text(n).font(font(14.0, 500.0).lh(18.0)).color(p.text_primary).truncate().grow(1.0).min_w(0.0));
                if full {
                    let hint = if i.sticker { t("Sticker slots full") } else { t("Emoji slots full") };
                    item = item.opacity(0.5).child(text(hint).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary));
                } else {
                    item = item.hover(p.background_modifier_hover).cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| {
                        let st = acc(s);
                        st.expr.info = None;
                        st.social(reqwest::Method::POST, format!("/guilds/{tg}/{kind}/clone"), Some(body.clone()), crate::social::Done::Toast(done.clone()));
                    });
                }
                actions = actions.child(item);
            }
        }
        c = c.child(col().w_full().padding(6.0).box_shadow(rune::Shadow { x: 0.0, y: 1.0, blur: 0.0, spread: 0.0, color: border, inset: true }).child(actions));
    }
    c
}

fn action<S: 'static>(glyph: &'static rune::Icon, label: &str, p: &Palette, f: impl Fn(&mut S) + 'static) -> El {
    row()
        .w_full()
        .h(32.0)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .radius(6.0)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.to_string())
        .child(crate::ui::icon(glyph, 18.0).color(p.text_primary_muted))
        .child(text(label.to_string()).font(font(14.0, 500.0).lh(18.0)).color(p.text_primary).truncate().shrink().min_w(0.0))
        .on_press(f)
}
