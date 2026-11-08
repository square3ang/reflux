//! Controls the channel, forum and thread settings share (`SlowmodeControl`,
//! `MatureContentSection`, `ThreadDefaultsSection`) and the forum tag editor
//! (`ForumTagSettings`, `ForumTagEditModal`).

use reflux_model::{Channel, Snowflake};
use reqwest::Method;
use rune::{El, col, icon, row, text};
use serde_json::{Map, Value, json};

use super::settings::{small, title};
use super::{Then, field, textarea};
use crate::i18n::{t, tp};
use crate::icons;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Modal, RadioGroup, Select, Slider, Switch, popup_access};

/// `SLOWMODE_STOP_SECONDS`: the slider moves between these.
pub const SLOWMODE_STOPS: [u32; 14] = [0, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 21600];

/// `formatSlowmodeDuration`: "Off", or hours, minutes and seconds.
pub fn slowmode_label(secs: u32) -> String {
    if secs == 0 {
        return t("Off").into();
    }
    let parts = [
        (secs / 3600, "{count, plural, one {# hour} other {# hours}}"),
        (secs % 3600 / 60, "{count, plural, one {# minute} other {# minutes}}"),
        (secs % 60, "{count, plural, one {# second} other {# seconds}}"),
    ];
    parts.iter().filter(|(n, _)| *n > 0).map(|(n, f)| tp(f, *n as i64, &[])).collect::<Vec<_>>().join(", ")
}

/// The stop at or below `secs`.
fn stop_of(secs: u32) -> usize {
    SLOWMODE_STOPS.iter().rposition(|&s| s <= secs).unwrap_or(0)
}

/// `SlowmodeControl`: a stepped slider kept in `form.data[key]` (seconds); a change goes into
/// `patch[api]`.
#[allow(clippy::too_many_arguments)]
pub fn slowmode<S: 'static>(st: &ShellState, acc: ShellAccess<S>, key: &'static str, api: &str, label: &str, cur: u32, patch: &mut Map<String, Value>, p: &Palette) -> El {
    let secs = st.form.data.get(key).and_then(Value::as_u64).map_or(cur, |v| v as u32);
    if secs != cur {
        patch.insert(api.into(), json!(secs));
    }
    slowmode_control(&format!("slowmode-{key}"), label, secs, p, move |s: &mut S, v| {
        acc(s).form.data.insert(key.into(), json!(v));
    })
}

/// The slider itself, for forms that don't keep their draft in `form.data`.
pub fn slowmode_control<S: 'static>(el_key: &str, label: &str, secs: u32, p: &Palette, set: impl Fn(&mut S, u32) + 'static) -> El {
    let slider = Slider::new(el_key, stop_of(secs) as f32, 0.0, (SLOWMODE_STOPS.len() - 1) as f32).on_change(move |s: &mut S, v: f32| set(s, SLOWMODE_STOPS[(v.round() as usize).min(SLOWMODE_STOPS.len() - 1)]));
    col()
        .w_full()
        .gap(8.0)
        .child(row().w_full().justify_between().child(title(label, p)).child(small(slowmode_label(secs), p)))
        .child(slider.el(p))
        .child(small(crate::i18n::tf("Wait between messages. Members with \"{bypassSlowmodePermissionLabel}\" can bypass this.", &[("bypassSlowmodePermissionLabel", t("Bypass slowmode"))]), p).wrap())
}

/// `CONTENT_WARNING_TEXT_MAX_LENGTH`.
const WARNING_MAX: usize = 200;

/// `MatureContentSection`: the mature-content override and a custom content warning.
pub fn mature<S: 'static>(st: &ShellState, ch: &Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>, patch: &mut Map<String, Value>) -> El {
    let f = &st.form;
    let key = |v: Option<bool>| match v {
        None => "inherit",
        Some(true) => "on",
        Some(false) => "off",
    };
    let cur = key(ch.nsfw_override);
    let value = f.data.get("nsfw_override").and_then(Value::as_str).unwrap_or(cur).to_string();
    let opts = [("inherit", t("Inherit")), ("on", t("On")), ("off", t("Off"))];
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let select = Select::new("nsfw_override", &opts, Some(&value))
        .width(160.0)
        .on_choose(move |s: &mut S, v| {
            acc(s).form.data.insert("nsfw_override".into(), json!(v));
        })
        .el(&st.popups, pops, p, layer);
    if value != cur {
        patch.insert("nsfw_override".into(), match value.as_str() {
            "on" => json!(true),
            "off" => json!(false),
            _ => Value::Null,
        });
    }
    let warn = f.data.get("cw_level").and_then(Value::as_u64).map_or(ch.content_warning_level, |v| v as u8);
    let mut out = col()
        .w_full()
        .gap(16.0)
        .child(row().w_full().gap(16.0).center_y().child(title(t("Mature content"), p).grow(1.0)).child(select))
        .child(Switch::new(warn == 1).label(t("Show a content warning in this channel")).on_toggle(move |s: &mut S| {
            acc(s).form.data.insert("cw_level".into(), json!(if warn == 1 { 0 } else { 1 }));
        }).el(p));
    if warn != ch.content_warning_level {
        patch.insert("content_warning_level".into(), json!(warn));
    }
    if warn == 1 {
        let cur_text = ch.content_warning_text.clone().unwrap_or_default();
        let typed = f.value("cw_text", &cur_text);
        out = out.child(textarea(st, acc, "cw_text", &cur_text, t("Custom warning text"), t("This contains sensitive content."), 64.0, p));
        if typed.trim() != cur_text && typed.chars().count() <= WARNING_MAX {
            patch.insert("content_warning_text".into(), if typed.trim().is_empty() { Value::Null } else { json!(typed.trim()) });
        }
    }
    out
}

/// `getAutoArchiveOptions` (minutes).
pub const AUTO_ARCHIVE: [(&str, &str, Option<&str>); 4] = [("60", "1 hour", None), ("1440", "24 hours", None), ("4320", "3 days", None), ("10080", "1 week", None)];

/// `ThreadDefaultsSection`: what new threads start with.
pub fn thread_defaults<S: 'static>(st: &ShellState, ch: &Channel, p: &Palette, acc: ShellAccess<S>, patch: &mut Map<String, Value>) -> El {
    let cur = ch.default_auto_archive_duration.unwrap_or(4320).to_string();
    let value = st.form.data.get("auto_archive").and_then(Value::as_str).map_or(cur.clone(), str::to_string);
    if value != cur {
        patch.insert("default_auto_archive_duration".into(), json!(value.parse::<u32>().unwrap_or(4320)));
    }
    col()
        .w_full()
        .gap(20.0)
        .child(
            col()
                .w_full()
                .gap(8.0)
                .child(title(t("Default hide after inactivity"), p))
                .child(small(t("New threads will stop showing in the channel list after this period of inactivity."), p).wrap())
                .child(RadioGroup::new(&AUTO_ARCHIVE, Some(&value)).on_change(move |s: &mut S, v| {
                    acc(s).form.data.insert("auto_archive".into(), json!(v));
                }).el(p)),
        )
        .child(slowmode(st, acc, "thread_slowmode", "default_thread_rate_limit_per_user", t("Default thread slowmode"), ch.default_thread_rate_limit_per_user, patch, p))
}

// ---- Forum tags ------------------------------------------------------------------------------

/// `MAX_FORUM_TAGS_PER_CHANNEL`, `FORUM_TAG_NAME_MAX_LENGTH`.
const MAX_TAGS: usize = 20;
const TAG_NAME_MAX: usize = 50;

/// Opens the tag editor for `tag` (`None` creates one).
fn edit_tag(st: &mut ShellState, ch: &Channel, tag: Option<Snowflake>) {
    let f = &mut st.form;
    f.error = None;
    f.text.remove("tag-name");
    let t = tag.and_then(|id| ch.available_tags.iter().find(|t| t.id == id));
    f.data.insert("tag-edit".into(), json!(tag.map_or(String::new(), |t| t.to_string())));
    f.data.insert("tag-moderated".into(), json!(t.is_some_and(|t| t.moderated)));
    f.data.insert("tag-emoji".into(), json!({ "id": t.and_then(|t| t.emoji_id).map(|i| i.to_string()), "name": t.and_then(|t| t.emoji_name.clone()) }));
    f.text.insert("tag-name".into(), rune::TextEdit::new(t.map_or(String::new(), |t| t.name.clone())));
}

fn close_tag(st: &mut ShellState) {
    st.form.data.retain(|k, _| !k.starts_with("tag-"));
    st.form.text.remove("tag-name");
}

/// `ForumTagSettings`: the tags with an edit button each, and Add tag.
pub fn tags<S: 'static>(st: &ShellState, ch: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let list = if ch.available_tags.is_empty() {
        small(t("This forum has no tags yet."), p).key("empty")
    } else {
        row().key("pills").w_full().flex_wrap().gaps(8.0, 8.0).children(ch.available_tags.iter().map(|tag| {
            let id = tag.id;
            let c2 = ch.clone();
            let mut pill = row().key(format!("t-{id}")).gap(4.0).center_y().child(crate::shell::forum::tag_pill(st, tag, None, false, p));
            if tag.moderated {
                pill = pill.child(icon(&icons::LOCK_FILL, 14.0).color(p.text_primary_muted));
            }
            pill.child(
                row()
                    .size(24.0, 24.0)
                    .center()
                    .radius(4.0)
                    .hover(p.background_modifier_hover)
                    .cursor(rune::Cursor::Pointer)
                    .role("button")
                    .label(t("Edit tag"))
                    .key(format!("edit-tag-{id}"))
                    .on_press(move |s: &mut S| edit_tag(acc(s), &c2, Some(id)))
                    .child(icon(&icons::PENCIL_SIMPLE_FILL, 14.0).color(p.text_primary_muted)),
            )
        }))
    };
    let c2 = ch.clone();
    let add = Button::new(t("Add tag"))
        .variant(ButtonVariant::Secondary)
        .size(ButtonSize::Small)
        .disabled(ch.available_tags.len() >= MAX_TAGS)
        .key("add-tag")
        .on_press(move |s: &mut S| edit_tag(acc(s), &c2, None))
        .el(p);
    col()
        .w_full()
        .gap(8.0)
        .child(title(t("Tags"), p))
        .child(small(crate::i18n::tf("Tags help people find posts. You can add up to {count} tags.", &[("count", &MAX_TAGS.to_string())]), p).wrap())
        .child(list)
        .child(row().child(add))
}

/// `ForumTagEditModal` over the channel settings, while `form.data["tag-edit"]` is set.
pub fn tag_modal<S: 'static>(st: &ShellState, ch: &Channel, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let f = &st.form;
    let editing = f.data.get("tag-edit")?.as_str()?.parse::<Snowflake>().ok();
    let forum = ch.id;
    let close = move |s: &mut S| close_tag(acc(s));
    let name = f.value("tag-name", "");
    let moderated = f.data.get("tag-moderated").and_then(Value::as_bool).unwrap_or(false);
    let emoji = f.data.get("tag-emoji").cloned().unwrap_or(Value::Null);
    let emoji_id = emoji.get("id").and_then(Value::as_str).and_then(|i| i.parse::<Snowflake>().ok());
    let emoji_name = emoji.get("name").and_then(Value::as_str).map(str::to_string);
    let has_emoji = emoji_id.is_some() || emoji_name.is_some();
    let glyph = if has_emoji {
        crate::shell::forum::forum_emoji(st, emoji_id, emoji_name.as_deref(), 24.0).unwrap_or_else(rune::el)
    } else {
        icon(&icons::SMILEY_FILL, 20.0).color(p.text_primary_muted)
    };
    let picker = row()
        .size(40.0, 40.0)
        .center()
        .radius(8.0)
        .fill(p.background_tertiary)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(t("Pick an emoji"))
        .key("pick-tag-emoji")
        .on_press(move |s: &mut S| crate::shell::expr::open_for_form(acc(s), forum, "tag-emoji"))
        .child(glyph);
    let mut emoji_row = row().gap(8.0).center_y().child(picker);
    if has_emoji {
        emoji_row = emoji_row.child(Button::new(t("Remove")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).on_press(move |s: &mut S| {
            acc(s).form.data.insert("tag-emoji".into(), json!({ "id": null, "name": null }));
        }).el(p));
    }
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(field(st, acc, "tag-name", "", t("Tag name"), "", p))
        .child(col().gap(8.0).child(text(t("Emoji")).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary)).child(emoji_row))
        .child(Switch::new(moderated).label(t("Only moderators can use this tag")).description(t("Members who can manage posts can add or remove it.")).on_toggle(move |s: &mut S| {
            acc(s).form.data.insert("tag-moderated".into(), json!(!moderated));
        }).el(p));
    if let Some(e) = super::error(st, p) {
        body = body.child(e);
    }
    let trimmed = name.trim().chars().take(TAG_NAME_MAX).collect::<String>();
    let payload = json!({ "name": trimmed, "moderated": moderated, "emoji_id": emoji_id.map(|i| i.to_string()), "emoji_name": emoji_name });
    let save = move |s: &mut S| {
        let sh = acc(s);
        match editing {
            Some(tag) => sh.api(Method::PUT, format!("/channels/{forum}/tags/{tag}"), Some(payload.clone()), Then::Nothing),
            None => sh.api(Method::POST, format!("/channels/{forum}/tags"), Some(payload.clone()), Then::Nothing),
        }
        close_tag(sh);
    };
    let mut footer = row().gap(8.0);
    if let Some(tag) = editing {
        footer = footer.child(Button::new(t("Delete tag")).variant(ButtonVariant::Danger).key("delete-tag").on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.api(Method::DELETE, format!("/channels/{forum}/tags/{tag}"), None, Then::Nothing);
            close_tag(sh);
        }).el(p));
    }
    footer = footer
        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
        .child(Button::new(t("Save")).key("save-tag").disabled(trimmed.is_empty()).on_press(save).el(p));
    Some(Modal::new(if editing.is_some() { t("Edit tag") } else { t("Create tag") }, close).body(body).footer(footer).el(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slowmode_stops_and_labels() {
        assert_eq!(stop_of(0), 0);
        assert_eq!(stop_of(300), 7);
        assert_eq!(stop_of(301), 7);
        assert_eq!(slowmode_label(0), "Off");
        assert_eq!(slowmode_label(5), "5 seconds");
        assert_eq!(slowmode_label(3600), "1 hour");
        assert_eq!(slowmode_label(5430), "1 hour, 30 minutes, 30 seconds");
    }
}
