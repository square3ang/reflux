//! The header's pinned messages popout (`ChannelPinsPopout`, `ChannelPinsContent`,
//! `MessagePreview.module.css`): newest pin first, Jump and Unpin on hover, more on scroll.
// shortcut: fixed 480×640, not resizable; pins are refetched each time the popout opens.

use reflux_model::{Channel, ChannelType, Message, Permissions, Snowflake};
use reflux_store::Store;
use rune::{El, ScrollSpec, col, icon, row, text};

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

/// The pin button: opens the popout and (re)loads the pins.
pub fn open(st: &mut ShellState, channel: Snowflake) {
    st.popups.toggle(POPOUT);
    if st.popups.is_open(POPOUT) {
        st.pins.entry(channel).or_default().loading = true;
        st.intents.push(Intent::LoadPins { channel, more: false });
    }
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
    let body = scroller(col().w_full().h_full().scroll(ScrollSpec { stick_bottom: false }).child(list), false, p);
    let surface = col()
        .size(480.0, 640.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .clip_rounded(8.0)
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .role("dialog")
        .label(t("Pinned messages"))
        .child(header)
        .child(col().w_full().grow(1.0).min_h(0.0).child(body))
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
        // shortcut: media wider than the card is cropped, not scaled down to fit.
        .clip_rounded(8.0)
        .border(1.0, p.background_header_secondary)
        .hover_group()
        .child(col().w_full().inert().child(super::chat::message_preview(store, st, m, &md, p, acc)))
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
