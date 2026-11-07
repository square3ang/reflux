//! The header's pinned messages popout (`ChannelPinsPopout`, `ChannelPinsContent`,
//! `MessagePreview.module.css`): newest pin first, Jump and Unpin on hover, more on scroll.
//! Pins load once per channel and follow the gateway like `ChannelPins.ts`. The popout resizes
//! like `useResizablePane` (default 480×640, min 320×200, 16px from the window, size kept).
// shortcut: only the left and bottom edges resize (the ones facing away from the anchor; Rune
// has no diagonal resize cursor for corners and the anchored edges would need re-placement).

use reflux_model::{Channel, ChannelType, Message, Permissions, Snowflake};
use reflux_store::Store;
use rune::{El, ScrollSpec, col, el, icon, row, text};

use super::markdown::Md;
use super::{Intent, ShellAccess, ShellState};
use crate::icons;
use crate::theme::{Palette, font};
use crate::ui::overlay::popout_shadow;
use crate::ui::overlay::anchor;
use crate::ui::{Button, ButtonVariant, Modal, Placement, over, scroller, spinner};
use crate::i18n::t;

pub const POPOUT: &str = "header-pins";

/// A channel's loaded pins.
#[derive(Clone, Debug, Default)]
pub struct Pins {
    /// (message, pinned_at), newest first.
    pub items: Vec<(Message, String)>,
    pub has_more: bool,
    pub loading: bool,
    pub failed: bool,
}

/// The pin button: opens the popout and loads the pins unless they're loaded.
pub fn open(st: &mut ShellState, channel: Snowflake) {
    st.popups.toggle(POPOUT);
    if st.popups.is_open(POPOUT) && !st.pins.contains_key(&channel) {
        st.pins.entry(channel).or_default().loading = true;
        st.intents.push(Intent::LoadPins { channel, more: false });
    }
}

/// `ChannelPins` gateway handlers: READY forgets every list, CHANNEL_PINS_UPDATE one (an open
/// popout loads it again), edits and deletes follow the messages.
pub fn on_dispatch(st: &mut ShellState, d: &reflux_model::DispatchEvent, now_iso: impl FnOnce() -> String) {
    use reflux_model::DispatchEvent as D;
    match d {
        D::Ready(_) => st.pins.clear(),
        D::ChannelPinsUpdate(u) => {
            st.pins.remove(&u.channel_id);
            if st.popups.is_open(POPOUT) && st.channel == Some(u.channel_id) {
                st.pins.entry(u.channel_id).or_default().loading = true;
                st.intents.push(Intent::LoadPins { channel: u.channel_id, more: false });
            }
        }
        D::MessageUpdate(m) => {
            let Some(p) = st.pins.get_mut(&m.channel_id) else { return };
            match p.items.iter().position(|(x, _)| x.id == m.id) {
                Some(i) if !m.pinned => drop(p.items.remove(i)),
                Some(i) => p.items[i].0 = (**m).clone(),
                None if m.pinned => p.items.insert(0, ((**m).clone(), now_iso())),
                None => {}
            }
        }
        D::MessageDelete(m) => {
            if let Some(p) = st.pins.get_mut(&m.channel_id) {
                p.items.retain(|(x, _)| x.id != m.id);
            }
        }
        _ => {}
    }
}

const DEFAULT_SIZE: (f32, f32) = (480.0, 640.0);
const MIN_SIZE: (f32, f32) = (320.0, 200.0);
const VIEWPORT_PADDING: f32 = 16.0;

/// The kept size, fitted to the window.
fn size(st: &ShellState) -> (f32, f32) {
    let (w, h) = st.prefs.pins_popout_size.unwrap_or(DEFAULT_SIZE);
    let [vw, vh] = rune::memo::layout_size(super::BODY_KEY).unwrap_or([1280.0, 800.0]);
    let top = rune::memo::layout(POPOUT).map_or(56.0, |r| r[1] + r[3] + 8.0);
    (w.clamp(MIN_SIZE.0, (vw - 2.0 * VIEWPORT_PADDING).max(MIN_SIZE.0)), h.clamp(MIN_SIZE.1, (vh - top - VIEWPORT_PADDING).max(MIN_SIZE.1)))
}

/// A resize strip: `horizontal` drags the left edge, else the bottom one.
fn handle<S: 'static>(horizontal: bool, acc: ShellAccess<S>) -> El {
    let strip = if horizontal { el().pin(Some(0.0), Some(-4.0), None, Some(0.0)).w(8.0).cursor(rune::Cursor::ResizeColumn) } else { el().pin(None, Some(0.0), Some(-4.0), Some(0.0)).h(8.0).cursor(rune::Cursor::ResizeRow) };
    let label = if horizontal { "Resize pinned messages from the left edge" } else { "Resize pinned messages from the bottom edge" };
    strip.role("separator").label(t(label)).key(format!("pins-resize-{}", if horizontal { "left" } else { "bottom" })).on_drag(move |s: &mut S, d: &mut rune::Drag, _| {
        let st = acc(s);
        let (w, h) = size(st);
        if d.phase == rune::Phase::Down {
            d.data = if horizontal { w } else { h };
        }
        st.prefs.pins_popout_size = Some(if horizontal { ((d.data - d.dx).round(), h) } else { (w, (d.data + d.dy).round()) });
        let fitted = size(st);
        st.prefs.pins_popout_size = Some(fitted);
        if d.phase == rune::Phase::Up {
            let prefs = st.prefs.clone();
            st.intents.push(Intent::Prefs(Box::new(prefs)));
        }
    })
}

/// Unpins and drops it from the list at once (the gateway confirms).
fn unpin(st: &mut ShellState, channel: Snowflake, message: Snowflake) {
    if let Some(p) = st.pins.get_mut(&channel) {
        p.items.retain(|(m, _)| m.id != message);
    }
    st.intents.push(Intent::Pin { channel, message, pin: false });
}

pub fn popout<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if let Some((ch, id)) = st.confirm_unpin
        && let Some(m) = st.pins.get(&ch).and_then(|x| x.items.iter().find(|(m, _)| m.id == id)).map(|x| &x.0)
    {
        layer.push(confirm(store, st, m, p, acc));
    }
    if !st.popups.is_open(POPOUT) {
        return;
    }
    let bg = p.form_surface_background;
    let pins = st.pins.get(&c.id).cloned().unwrap_or_default();
    let dm = matches!(c.kind, ChannelType::DM | ChannelType::GROUP_DM);
    let can_unpin = dm || store.permissions(c.id).contains(Permissions::MANAGE_MESSAGES);
    let header = row()
        .w_full()
        .min_h(68.0)
        .padding(16.0)
        .gap(4.0)
        .center_y()
        .child(icon(&icons::PUSH_PIN, 24.0).color(p.text_primary_muted))
        .child(text(t("Pinned messages")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary));
    let mut list = col().w_full().pt(8.0).px(12.0).pb(12.0);
    if pins.items.is_empty() && pins.loading {
        list = list.child(row().w_full().h(400.0).center().child(spinner(p.text_primary_muted)));
    } else if pins.items.is_empty() {
        let hint = if dm { "Pin messages here for everyone to see.".to_string() } else { "Members with \"Pin Messages\" can pin messages for everyone.".to_string() };
        list = list.child(state(&icons::SPARKLE, 80.0, if pins.failed { "Couldn't load pins" } else { t("No pinned messages") }, &hint, 400.0, p));
    } else {
        for (m, _) in &pins.items {
            list = list.child(card(store, st, c, m, can_unpin, p, acc));
        }
        if pins.loading {
            list = list.child(row().w_full().py(16.0).center().child(spinner(p.text_primary_muted)));
        } else if pins.has_more {
            let ch = c.id;
            list = list.child(
                row().w_full().py(8.0).center().child(
                    Button::new(t("Load more"))
                        .variant(ButtonVariant::Secondary)
                        .key("pins-more")
                        .on_press(move |s: &mut S| {
                            let st = acc(s);
                            st.pins.entry(ch).or_default().loading = true;
                            st.intents.push(Intent::LoadPins { channel: ch, more: true });
                        })
                        .el(p),
                ),
            );
        } else {
            list = list.child(state(&icons::FLAG_CHECKERED, 48.0, t("You're at the end"), t("Pinned messages show up here."), 200.0, p));
        }
    }
    let body = scroller(col().w_full().h_full().scroll(ScrollSpec { stick_bottom: false }).key("pins-list").child(list), false, p);
    let (w, h) = size(st);
    let surface = col()
        .size(w, h)
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .clip_rounded(8.0)
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .role("dialog")
        .label(t("Pinned messages"))
        .child(header)
        .child(col().w_full().grow(1.0).min_h(0.0).child(body))
        .child(handle(true, acc))
        .child(handle(false, acc))
        .modal(true, move |s: &mut S| acc(s).popups.close());
    layer.push(anchor(surface, POPOUT, Placement::BottomEnd, 8.0));
}

/// Empty and end states: a big icon, a title and a line.
fn state(glyph: &'static rune::Icon, size: f32, title: &str, line: &str, min_h: f32, p: &Palette) -> El {
    col()
        .w_full()
        .min_h(min_h)
        .center()
        .gap(8.0)
        .child(icon(glyph, size).color(p.text_primary_muted))
        .child(text(title.to_string()).font(font(20.0, 600.0).lh(28.0)).color(p.text_primary))
        .child(text(line.to_string()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap().align(rune::Align::Center))
}

/// `.previewCard`: the message as a group head, with Jump (and Unpin) on hover.
fn card<S: 'static>(store: &Store, st: &ShellState, c: &Channel, m: &Message, can_unpin: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let md = Md::new(store, &st.media, c.guild_id, p, format!("pin-m{}", m.id));
    let (ch, id, guild) = (c.id, m.id, c.guild_id);
    let jump = row()
        .h(20.0)
        .px(8.0)
        .center()
        .radius(4.0)
        .fill(p.background_primary)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .hover_group()
        .child(text(t("Jump")).font(font(11.0, 600.0).lh(20.0)).color(p.text_primary_muted).group_color(p.text_primary))
        .key(format!("pin-jump-{id}"))
        .on_press(move |s: &mut S| {
            let st = acc(s);
            st.popups.close();
            super::jump(st, guild, ch, id);
        });
    let mut actions = row().gap(4.0).child(jump);
    if can_unpin {
        actions = actions.child(
            row()
                .size(24.0, 24.0)
                .center()
                .radius(4.0)
                .fill(p.background_primary)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(t("Unpin message"))
                .hover_group()
                .child(icon(&icons::X_BOLD, 16.0).color(p.text_primary_muted).group_color(p.text_primary))
                .key(format!("pin-unpin-{id}"))
                .on_press_tx(move |s: &mut S, tx| {
                    let st = acc(s);
                    if tx.mods().shift {
                        unpin(st, ch, id);
                    } else {
                        st.confirm_unpin = Some((ch, id));
                    }
                }),
        );
    }
    col()
        .w_full()
        .mb(8.0)
        .py(8.0)
        .radius(8.0)
        .clip_rounded(8.0)
        .border(1.0, p.background_header_secondary)
        .hover_group()
        .child(col().w_full().inert().child(super::chat::message_preview_in(store, st, m, &md, p, acc, size(st).0 - 24.0)))
        .child(actions.pin(None, Some(12.0), Some(12.0), None).group_opacity(0.0, 1.0))
}

/// "Unpin message": "Send this pin back in time?" with the message.
fn confirm<S: 'static>(store: &Store, st: &ShellState, m: &Message, p: &Palette, acc: ShellAccess<S>) -> El {
    let close = move |s: &mut S| acc(s).confirm_unpin = None;
    let md = Md::new(store, &st.media, store.channels.get(&m.channel_id).and_then(|c| c.guild_id), p, format!("unpin-m{}", m.id));
    let (ch, id) = (m.channel_id, m.id);
    let body = col()
        .w_full()
        .gap(16.0)
        .child(text(t("Send this pin back in time?")).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap())
        .child(
            col()
                .w_full()
                .max_h(220.0)
                .clip_rounded(6.0)
                .py(8.0)
                .radius(6.0)
                .fill(p.background_secondary)
                .border(1.0, over(p.background_header_secondary, p.background_secondary))
                .inert()
                .child(super::chat::message_preview(store, st, m, &md, p, acc)),
        );
    Modal::new(t("Unpin message"), close)
        .body(body)
        .footer(
            row()
                .gap(8.0)
                .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("unpin-cancel").on_press(close).el(p))
                .child(
                    Button::new(t("Unpin it"))
                        .variant(ButtonVariant::Danger)
                        .key("unpin-confirm")
                        .on_press(move |s: &mut S| {
                            let st = acc(s);
                            st.confirm_unpin = None;
                            unpin(st, ch, id);
                        })
                        .el(p),
                ),
        )
        .el(p)
}

#[cfg(test)]
mod tests {
    use reflux_model::DispatchEvent as D;

    use super::*;

    #[test]
    fn follows_the_gateway_like_channel_pins() {
        let ch = Snowflake(7);
        let m = |id: u64, pinned: bool| Message { id: Snowflake(id), channel_id: ch, pinned, ..Default::default() };
        let mut st = ShellState { channel: Some(ch), ..Default::default() };
        st.pins.insert(ch, Pins { items: vec![(m(1, true), "a".into())], ..Default::default() });
        on_dispatch(&mut st, &D::MessageUpdate(Box::new(m(2, true))), || "now".into());
        assert_eq!(st.pins[&ch].items.iter().map(|x| (x.0.id.0, x.1.as_str())).collect::<Vec<_>>(), [(2, "now"), (1, "a")], "a new pin goes first");
        on_dispatch(&mut st, &D::MessageUpdate(Box::new(m(1, false))), String::new);
        assert_eq!(st.pins[&ch].items.len(), 1, "unpinned");
        on_dispatch(&mut st, &D::MessageDelete(Box::new(reflux_model::MessageDelete { id: Snowflake(2), channel_id: ch, ..Default::default() })), String::new);
        assert!(st.pins[&ch].items.is_empty());
        open(&mut st, ch);
        assert!(st.intents.is_empty(), "loaded once");
        let update = reflux_model::ChannelPinsUpdate { channel_id: ch, ..Default::default() };
        on_dispatch(&mut st, &D::ChannelPinsUpdate(update), String::new);
        assert_eq!(st.intents, [Intent::LoadPins { channel: ch, more: false }], "the open popout reloads");
    }
}
