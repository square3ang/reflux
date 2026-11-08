//! Forum channel settings (`forum/components/settings/ForumOverviewTab`, `ForumTagSettings`):
//! the overview fields a forum or media channel has instead of a text channel's.

use reflux_model::{Channel, ChannelFlags, ChannelType};
use rune::{El, col, icon, row};
use serde_json::{Map, Value, json};

use super::channel_opts;
use super::settings::{small, title};
use super::textarea;
use crate::icons;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::Palette;
use crate::ui::{Button, ButtonSize, ButtonVariant, RadioGroup, Switch};
use crate::i18n::t;

const SORTS: [(&str, &str, Option<&str>); 2] = [("0", "Recently active", None), ("1", "Date posted", None)];
const LAYOUTS: [(&str, &str, Option<&str>); 2] = [("1", "List", None), ("2", "Gallery", None)];

/// `ForumOverviewForm`'s fields after the name, appended to `content`; changes go into `patch`.
#[allow(clippy::too_many_arguments)]
pub(super) fn fields<S: 'static>(st: &ShellState, ch: &Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>, mut content: El, patch: &mut Map<String, Value>) -> El {
    let f = &st.form;
    let data = |k: &str| f.data.get(k).cloned();
    let media = ch.kind == ChannelType::GUILD_MEDIA;

    let cur_topic = ch.topic.clone().unwrap_or_default();
    content = content.child(textarea(st, acc, "topic", &cur_topic, t("Post guidelines"), t("Let people know how to post in this forum"), 88.0, p));
    let topic = f.value("topic", &cur_topic);
    if topic.trim() != cur_topic {
        patch.insert("topic".into(), if topic.trim().is_empty() { Value::Null } else { json!(topic) });
    }

    // Tags and the flags.
    content = content.child(channel_opts::tags(st, ch, p, acc));
    let require = data("require_tag").and_then(|v| v.as_bool()).unwrap_or(ch.flags.contains(ChannelFlags::REQUIRE_TAG));
    content = content.child(Switch::new(require).label(t("Require people to select tags when posting")).on_toggle(move |s: &mut S| {
        acc(s).form.data.insert("require_tag".into(), json!(!require));
    }).el(p));
    let hide_dl = data("hide_media_download").and_then(|v| v.as_bool()).unwrap_or(ch.flags.contains(ChannelFlags::HIDE_MEDIA_DOWNLOAD_OPTIONS));
    if media {
        content = content.child(Switch::new(hide_dl).label(t("Hide media download options")).on_toggle(move |s: &mut S| {
            acc(s).form.data.insert("hide_media_download".into(), json!(!hide_dl));
        }).el(p));
    }
    let mut flags = ch.flags;
    flags.set(ChannelFlags::REQUIRE_TAG, require);
    if media {
        flags.set(ChannelFlags::HIDE_MEDIA_DOWNLOAD_OPTIONS, hide_dl);
    }
    if flags != ch.flags {
        patch.insert("flags".into(), json!(flags.bits()));
    }

    // Default reaction: picked into `form.data["reaction"]` (`{"id", "name"}` or null).
    let cur = ch.default_reaction_emoji.as_ref().map(|e| json!({ "id": e.emoji_id.map(|i| i.to_string()), "name": e.emoji_name }));
    let picked = data("reaction");
    let reaction = picked.clone().or(cur.clone()).filter(|v| !v.is_null());
    let emoji = reaction.as_ref().and_then(|v| {
        let id = v.get("id").and_then(Value::as_str).and_then(|i| i.parse().ok());
        crate::shell::forum::forum_emoji(st, id, v.get("name").and_then(Value::as_str), 24.0)
    });
    let forum = ch.id;
    let picker = row()
        .size(40.0, 40.0)
        .center()
        .radius(8.0)
        .fill(p.background_tertiary)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(t("Pick an emoji"))
        .key("pick-reaction")
        .on_press(move |s: &mut S| crate::shell::expr::open_for_form(acc(s), forum, "reaction"))
        .child(emoji.unwrap_or_else(|| icon(&icons::SMILEY_FILL, 20.0).color(p.text_primary_muted)));
    let mut r = row().gap(12.0).center_y().child(picker);
    if reaction.is_some() {
        r = r.child(Button::new(t("Remove")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).on_press(move |s: &mut S| {
            acc(s).form.data.insert("reaction".into(), Value::Null);
        }).el(p));
    }
    content = content.child(col().w_full().gap(8.0).child(title(t("Default reaction"), p)).child(small(t("Shown on every post so people can react with one click."), p).wrap()).child(r));
    if let Some(v) = picked.filter(|v| Some(v) != cur.as_ref() && !(v.is_null() && cur.is_none())) {
        patch.insert("default_reaction_emoji".into(), if v.is_null() { Value::Null } else { json!({ "emoji_id": v["id"], "emoji_name": v["name"] }) });
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
    if !media {
        let cur_layout = if ch.default_forum_layout == 2 { "2" } else { "1" }.to_string();
        let layout = data("layout").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| cur_layout.clone());
        content = content.child(col().w_full().gap(8.0).child(title(t("Default layout"), p)).child(RadioGroup::new(&LAYOUTS, Some(&layout)).on_change(move |s: &mut S, v| {
            acc(s).form.data.insert("layout".into(), json!(v));
        }).el(p)));
        if layout != cur_layout {
            patch.insert("default_forum_layout".into(), json!(layout.parse::<u8>().unwrap_or(1)));
        }
    }

    content
        .child(channel_opts::slowmode(st, acc, "slowmode", "rate_limit_per_user", t("Slowmode for creating posts"), ch.rate_limit_per_user, patch, p))
        .child(channel_opts::thread_defaults(st, ch, p, acc, patch))
        .child(channel_opts::mature(st, ch, p, acc, layer, patch))
}
