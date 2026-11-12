//! The invite modal's friend list (`RecipientList` / `useRecipientItems`): friends (DM'd ones
//! first) then group DMs, each with an Invite button that sends the link there.

use crate::ui::ChromiumWheel;
use reflux_model::{ChannelType, RelationshipType, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, ScrollSpec, col, row, text};
use serde_json::{Value, json};

use super::Then;
use crate::i18n::t;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant};

const SENT: &str = "invite-sent";

/// A recipient: a friend (DM'd) or a group DM.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum To {
    Friend(Snowflake),
    Group(Snowflake),
}

/// Friends, those with a DM first by latest message then by name, then group DMs.
pub fn recipients(store: &Store) -> Vec<(To, String)> {
    let last_dm = |u: Snowflake| store.channels.values().filter(|c| c.kind == ChannelType::DM && c.recipients.iter().any(|r| r.id == u)).filter_map(|c| c.last_message_id).max();
    let mut v: Vec<(Option<Snowflake>, Snowflake, String)> =
        store.relationships.values().filter(|r| r.kind == RelationshipType::FRIEND).map(|r| (last_dm(r.id), r.id, store.display_name(None, r.id))).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.to_lowercase().cmp(&b.2.to_lowercase())));
    let mut out: Vec<(To, String)> = v.into_iter().map(|(_, u, n)| (To::Friend(u), n)).collect();
    let mut groups: Vec<_> = store.channels.values().filter(|c| c.kind == ChannelType::GROUP_DM).collect();
    groups.sort_by_key(|c| std::cmp::Reverse(c.last_message_id));
    out.extend(groups.into_iter().map(|c| (To::Group(c.id), crate::shell::channel_title(store, c))));
    out
}

/// The search box and the list; `None` without friends.
pub fn view<S: 'static>(store: &Store, st: &ShellState, link: Option<String>, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let all = recipients(store);
    if all.is_empty() {
        return None;
    }
    let q = st.form.value("friend-search", "").trim().to_lowercase();
    let sent: Vec<String> = st.form.data.get(SENT).and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
    let rows: Vec<El> = all
        .into_iter()
        .filter(|(_, n)| q.is_empty() || n.to_lowercase().contains(&q))
        .map(|(to, name)| {
            let (To::Friend(u) | To::Group(u)) = to;
            let done = sent.contains(&u.to_string());
            let link = link.clone();
            let button = Button::new(if done { t("Sent") } else { t("Invite") })
                .variant(if done { ButtonVariant::Secondary } else { ButtonVariant::Primary })
                .size(ButtonSize::Compact)
                .disabled(done || link.is_none())
                .on_press(move |s: &mut S| {
                    let Some(l) = link.clone() else { return };
                    let then = Then::SendDm { content: l, user: u, mark: SENT.into() };
                    match to {
                        To::Friend(_) => acc(s).api(Method::POST, "/users/@me/channels", Some(json!({ "recipient_id": u.to_string() })), then),
                        // The group DM itself, to send into.
                        To::Group(_) => acc(s).api(Method::GET, format!("/channels/{u}"), None, then),
                    }
                });
            row()
                .key(format!("f-{u}"))
                .w_full()
                .py(6.0)
                .gap(12.0)
                .center_y()
                .child(match to {
                    To::Friend(_) => crate::shell::avatar(store, st, u, 32.0, None, p.background_secondary, p),
                    To::Group(_) => row().size(32.0, 32.0).center().radius(16.0).fill(p.brand_primary).child(rune::icon(&crate::icons::USERS_BOLD, 18.0).color(p.brand_primary_fill)),
                })
                .child(text(name).font(font(15.0, 500.0).lh(20.0)).color(p.text_primary).truncate().grow(1.0).shrink())
                .child(button.el(p))
        })
        .collect();
    let list = if rows.is_empty() {
        text(t("No friends found")).key("none").font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted)
    } else {
        col().key("list").w_full().max_h(240.0).scroll(ScrollSpec { stick_bottom: false }).chromium_wheel::<S>("invite-friends-list").child(col().w_full().children(rows))
    };
    Some(col().w_full().gap(8.0).child(super::field(st, acc, "friend-search", "", "", t("Search friends"), p)).child(list))
}
