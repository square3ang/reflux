//! Forum channel settings (`forum/components/settings/ForumOverviewTab`, `ForumTagSettings`):
//! the overview fields a forum or media channel has instead of a text channel's.

use reflux_model::{Channel, ChannelFlags, ChannelType};
use rune::{El, col, icon, row};
use serde_json::{Map, Value, json};

use super::field;
use super::settings::{small, title};
use crate::icons;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::Palette;
use crate::ui::{Button, ButtonSize, ButtonVariant, RadioGroup, Select, Switch, popup_access};
use crate::i18n::t;

const SORTS: [(&str, &str, Option<&str>); 2] = [("0", "Recently active", None), ("1", "Date posted", None)];
const LAYOUTS: [(&str, &str, Option<&str>); 2] = [("1", "List", None), ("2", "Gallery", None)];

/// `ForumOverviewForm`'s fields after the name, appended to `content`; changes go into `patch`.
// shortcut: guidelines are a single-line field (Fluxer's is a 3-row textarea); tags are listed
// but not added or edited (no `ForumTagEditModal`); the default reaction can be removed but
// not picked; thread defaults and mature-content sections aren't shown.
#[allow(clippy::too_many_arguments)]
pub(super) fn fields<S: 'static>(st: &ShellState, ch: &Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>, mut content: El, patch: &mut Map<String, Value>) -> El {
    let f = &st.form;
    let data = |k: &str| f.data.get(k).cloned();

    let cur_topic = ch.topic.clone().unwrap_or_default();
    content = content.child(field(st, acc, "topic", &cur_topic, t("Post guidelines"), t("Let people know how to post in this forum"), p));
    let topic = f.value("topic", &cur_topic);
    if topic.trim() != cur_topic {
        patch.insert("topic".into(), if topic.trim().is_empty() { Value::Null } else { json!(topic.trim()) });
    }

    // Tags.
    let tags = if ch.available_tags.is_empty() {
        small(t("This forum has no tags yet."), p)
    } else {
        row().w_full().flex_wrap().gaps(8.0, 8.0).children(ch.available_tags.iter().map(|t| {
            let mut pill = row().gap(4.0).center_y().child(crate::shell::forum::tag_pill(st, t, None, false, p));
            if t.moderated {
                pill = pill.child(icon(&icons::LOCK, 14.0).color(p.text_primary_muted));
            }
            pill
        }))
    };
    content = content.child(col().w_full().gap(8.0).child(title(t("Tags"), p)).child(small("Tags help people find posts. You can add up to 20 tags.", p).wrap()).child(tags));
    let require = data("require_tag").and_then(|v| v.as_bool()).unwrap_or(ch.flags.contains(ChannelFlags::REQUIRE_TAG));
    content = content.child(Switch::new(require).label(t("Require people to select tags when posting")).on_toggle(move |s: &mut S| {
        acc(s).form.data.insert("require_tag".into(), json!(!require));
    }).el(p));
    if require != ch.flags.contains(ChannelFlags::REQUIRE_TAG) {
        let mut flags = ch.flags;
        flags.set(ChannelFlags::REQUIRE_TAG, require);
        patch.insert("flags".into(), json!(flags.bits()));
    }

    // Default reaction.
    let removed = data("reaction_removed").is_some();
    let reaction = ch.default_reaction_emoji.as_ref().filter(|_| !removed);
    let mut r = col().w_full().gap(8.0).child(title(t("Default reaction"), p)).child(small(t("Shown on every post so people can react with one click."), p).wrap());
    if let Some(e) = reaction {
        let emoji = crate::shell::forum::forum_emoji(st, e.emoji_id, e.emoji_name.as_deref(), 24.0);
        r = r.child(
            row().gap(12.0).center_y().children(emoji).child(
                Button::new(t("Remove")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).on_press(move |s: &mut S| {
                    acc(s).form.data.insert("reaction_removed".into(), json!(true));
                }).el(p),
            ),
        );
    }
    content = content.child(r);
    if removed && ch.default_reaction_emoji.is_some() {
        patch.insert("default_reaction_emoji".into(), Value::Null);
    }

    // Sort order and layout.
    let cur_sort = ch.default_sort_order.unwrap_or(0).to_string();
    let sort = data("sort").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| cur_sort.clone());
    content = content.child(col().w_full().gap(8.0).child(title(t("Default sort order"), p)).child(RadioGroup::new(&SORTS, Some(&sort)).on_change(move |s: &mut S, v| {
        acc(s).form.data.insert("sort".into(), json!(v));
    }).el(p)));
    if sort != cur_sort {
        patch.insert("default_sort_order".into(), json!(sort.parse::<u8>().unwrap_or(0)));
    }
    if ch.kind != ChannelType::GUILD_MEDIA {
        let cur_layout = if ch.default_forum_layout == 2 { "2" } else { "1" }.to_string();
        let layout = data("layout").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| cur_layout.clone());
        content = content.child(col().w_full().gap(8.0).child(title(t("Default layout"), p)).child(RadioGroup::new(&LAYOUTS, Some(&layout)).on_change(move |s: &mut S, v| {
            acc(s).form.data.insert("layout".into(), json!(v));
        }).el(p)));
        if layout != cur_layout {
            patch.insert("default_forum_layout".into(), json!(layout.parse::<u8>().unwrap_or(1)));
        }
    }

    // Post slowmode.
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let slow = data("slowmode").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| ch.rate_limit_per_user.to_string());
    content = content.child(
        Select::new("slowmode", &super::channel::SLOWMODE, Some(&slow))
            .label(t("Slowmode for creating posts"))
            .width(340.0)
            .on_choose(move |s: &mut S, v| {
                acc(s).form.data.insert("slowmode".into(), json!(v));
            })
            .el(&st.popups, pops, p, layer),
    );
    if slow != ch.rate_limit_per_user.to_string() {
        patch.insert("rate_limit_per_user".into(), json!(slow.parse::<u32>().unwrap_or(0)));
    }
    content
}
