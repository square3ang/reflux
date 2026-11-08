//! The rest of the community Overview tab (`guild_overview_tab`): the detached banner switch,
//! invite / embed splashes and card alignment, Previewing, the Advanced section and the
//! message history threshold modal.

use reflux_model::{Guild, Permissions as P, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, col, el, icon, row, text};
use serde_json::{Map, Value, json};

use super::settings::{image_field, small, title};
use super::{Then, error};
use crate::i18n::t;
use crate::icons;
use crate::media::{self, Load};
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Modal, Switch};

fn has(info: &Guild, f: &str) -> bool {
    info.features.iter().any(|x| x == f)
}

/// A feature switch's value: the form's pick, else whether the guild has it.
fn flag(st: &ShellState, info: &Guild, key: &str, feature: &str) -> bool {
    st.form.data.get(key).and_then(Value::as_bool).unwrap_or_else(|| has(info, feature))
}

fn switch<S: 'static>(on: bool, key: &'static str, label: &str, desc: Option<&str>, p: &Palette, acc: ShellAccess<S>) -> El {
    let mut s = Switch::new(on).label(label).on_toggle(move |s: &mut S| {
        acc(s).form.data.insert(key.into(), json!(!on));
    });
    if let Some(d) = desc {
        s = s.description(d);
    }
    s.el(p)
}

/// The form-held feature switches: (form key, feature).
const FLAGS: [(&str, &str); 3] = [("detached", "DETACHED_BANNER"), ("flexible", "TEXT_CHANNEL_FLEXIBLE_NAMES"), ("hide_crown", "HIDE_OWNER_CROWN")];

/// What these sections add to the Overview PATCH.
pub fn patch(st: &ShellState, info: &Guild, out: &mut Map<String, Value>) {
    for key in ["splash", "embed_splash"] {
        if let Some(v) = st.form.data.get(key) {
            out.insert(key.into(), v.clone());
        }
    }
    if let Some(a) = st.form.data.get("splash_align").and_then(Value::as_u64).filter(|a| *a != u64::from(info.splash_card_alignment)) {
        out.insert("splash_card_alignment".into(), json!(a));
    }
    let mut features = info.features.clone();
    let mut changed = false;
    for (key, f) in FLAGS {
        let want = flag(st, info, key, f);
        if want != has(info, f) {
            changed = true;
            if want { features.push(f.into()) } else { features.retain(|x| x != f) }
        }
    }
    if changed {
        out.insert("features".into(), json!(features));
    }
}

/// `BrandingSection`'s detached-banner switch (under the name, when banners are on).
pub fn detached<S: 'static>(st: &ShellState, info: &Guild, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    has(info, "BANNER").then(|| switch(flag(st, info, "detached", "DETACHED_BANNER"), "detached", t("Detached banner"), Some(t("Shows the banner in its own section below the community header.")), p, acc))
}

/// The invite splash, its card alignment and the embed splash (`INVITE_SPLASH`).
pub fn splashes<S: 'static>(st: &ShellState, g: Snowflake, info: &Guild, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if !has(info, "INVITE_SPLASH") {
        return None;
    }
    let load = |kind: &'static str, hash: &Option<String>| hash.as_deref().map_or(Load::Failed, |h| st.media.asset(|b, s| media::guild_wide_url(b, kind, g, h, s)));
    let align = st.form.data.get("splash_align").and_then(Value::as_u64).unwrap_or(u64::from(info.splash_card_alignment));
    // `GuildInviteSplashSettingsField`: the previews beside the card alignment.
    let alignment = col().gap(8.0).items_start().child(title(t("Card alignment"), p)).child(alignment_controls(align, p, acc)).child(small(t("Only applies on wide screens."), p));
    let alignment = row().w_full().gap(24.0).items_start().child(super::invite_preview::buttons(p, acc).grow(1.0)).child(alignment.grow(1.0));
    Some(
        col()
            .w_full()
            .gap(20.0)
            .child(image_field(st, t("Splash"), "splash", load("splashes", &info.splash), 240.0, 135.0, 8.0, p, acc))
            .child(alignment)
            .child(image_field(st, t("Embed splash"), "embed_splash", load("embed-splashes", &info.embed_splash), 240.0, 135.0, 8.0, p, acc))
            .child(small(t("Shown in invite embeds in chat."), p)),
    )
}

/// `CardAlignmentControls`: left / centre / right (`splash_card_alignment` 1 / 0 / 2).
pub fn alignment_controls<S: 'static>(align: u64, p: &Palette, acc: ShellAccess<S>) -> El {
    let seg = |v: u64, glyph: &'static rune::Icon, label: &'static str| {
        let on = align == v;
        row()
            .size(36.0, 32.0)
            .center()
            .radius(6.0)
            .fill(if on { p.background_modifier_selected } else { rune::Color::TRANSPARENT })
            .hover(p.background_modifier_hover)
            .cursor(rune::Cursor::Pointer)
            .role("radio")
            .checked(on)
            .label(t(label))
            .on_press(move |s: &mut S| {
                acc(s).form.data.insert("splash_align".into(), json!(v));
            })
            .child(icon(glyph, 18.0).color(if on { p.text_primary } else { p.text_primary_muted }))
    };
    row().gap(4.0).px(4.0).py(4.0).radius(8.0).fill(p.background_tertiary).child(seg(1, &icons::TEXT_ALIGN_LEFT_FILL, "Left")).child(seg(0, &icons::TEXT_ALIGN_CENTER_FILL, "Center")).child(seg(2, &icons::TEXT_ALIGN_RIGHT_FILL, "Right"))
}

/// `GuildVisitsSection`: saved at once, like Fluxer's `toggleFeature`.
pub fn previewing<S: 'static>(store: &Store, g: Snowflake, info: &Guild, p: &Palette, acc: ShellAccess<S>) -> El {
    let on = has(info, "VISITS_ENABLED");
    let mut features = info.features.clone();
    if on { features.retain(|f| f != "VISITS_ENABLED") } else { features.push("VISITS_ENABLED".into()) }
    let can = store.guild_permissions(g).contains(P::MANAGE_GUILD);
    Switch::new(on)
        .label(t("Allow people to preview this community before joining"))
        .description(t("People with an invite link can read the channels @everyone can see before they join. They can't post, react or see the member list."))
        .disabled(!can)
        .on_toggle(move |s: &mut S| acc(s).api(Method::PATCH, format!("/guilds/{g}"), Some(json!({ "features": features })), Then::Toast("", "Couldn't update this setting. Try again.")))
        .el(p)
}

/// The collapsed-by-default Advanced section (`SettingsSection isAdvanced`): flexible channel
/// names and the owner crown.
pub fn advanced<S: 'static>(st: &ShellState, info: &Guild, p: &Palette, acc: ShellAccess<S>) -> El {
    let open = st.form.data.get("advanced").and_then(Value::as_bool).unwrap_or(false);
    let head = row()
        .w_full()
        .center_y()
        .justify_between()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(t("Advanced"))
        .on_press(move |s: &mut S| {
            acc(s).form.data.insert("advanced".into(), json!(!open));
        })
        .child(text(t("Advanced")).font(font(18.0, 600.0).lh(23.4)).color(p.text_primary))
        .child(row().size(28.0, 28.0).center().radius(6.0).hover(p.background_modifier_hover).child(icon(if open { &icons::CARET_UP_FILL } else { &icons::CARET_DOWN_FILL }, 18.0).color(p.text_primary_muted)));
    let mut c = col().w_full().gap(16.0).pt(24.0).pb(24.0).child(el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(1.0).fill(p.background_modifier_accent)).child(head);
    if open {
        c = c
            .child(col().w_full().gap(12.0).child(title(t("Text channel names"), p)).child(switch(
                flag(st, info, "flexible", "TEXT_CHANNEL_FLEXIBLE_NAMES"),
                "flexible",
                t("Allow flexible text channel names"),
                Some(t("Allow capital letters and spaces in text channel names. Off restricts names to lowercase with hyphens and underscores.")),
                p,
                acc,
            )))
            .child(col().w_full().gap(12.0).child(title(t("Community owner crown"), p)).child(switch(flag(st, info, "hide_crown", "HIDE_OWNER_CROWN"), "hide_crown", t("Hide community owner crown"), None, p, acc)));
    }
    c
}

// ---- Message history threshold ---------------------------------------------------------------

/// A threshold (ms) as RFC 3339, or why it's refused (before the community or in the future).
pub fn threshold(ms: i64, created_ms: u64, now_ms: u64) -> Result<String, &'static str> {
    if ms < created_ms as i64 {
        return Err("Threshold date cannot be before community creation");
    }
    if ms > now_ms as i64 {
        return Err("Threshold date cannot be in the future");
    }
    let at = chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default();
    Ok(at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// The sidebar-foot button (`DesktopEntitySettingsView`'s threshold footer).
pub fn threshold_button<S: 'static>(store: &Store, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    store.guild_permissions(g).contains(P::MANAGE_GUILD).then(|| {
        el().w_full().mt(16.0).child(
            Button::new(t("Message history threshold"))
                .variant(ButtonVariant::Secondary)
                .size(crate::ui::ButtonSize::Small)
                .on_press(move |s: &mut S| {
                    let f = &mut acc(s).form;
                    f.data.insert("mht".into(), json!(true));
                    f.data.remove("mht-on");
                    f.data.remove("mht-day");
                    f.text.remove("mht-time");
                    f.error = None;
                })
                .el(p),
        )
    })
}

/// `MessageHistoryThresholdModal`.
pub fn threshold_modal<S: 'static>(st: &ShellState, g: Snowflake, info: &Guild, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if !st.form.data.contains_key("mht") {
        return;
    }
    let cur = info.message_history_cutoff.clone();
    let on = st.form.data.get("mht-on").and_then(Value::as_bool).unwrap_or(cur.is_some());
    // The current cutoff, else the community's creation (as Fluxer fills it when switched on).
    let created = g.timestamp_ms();
    let at = cur.as_deref().and_then(|c| chrono::DateTime::parse_from_rfc3339(c).ok()).map_or(created as i64, |d| d.timestamp_millis());
    let at = chrono::DateTime::from_timestamp_millis(at).unwrap_or_default().with_timezone(&chrono::Local);
    let day = st.form.data.get("mht-day").and_then(Value::as_str).and_then(|d| d.parse().ok()).unwrap_or(at.date_naive());
    let initial = at.format("%H:%M").to_string();
    let time = st.form.text.get("mht-time").map_or(initial.clone(), |e| e.text.clone());
    let close = move |s: &mut S| {
        acc(s).form.data.remove("mht");
    };
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(small(t("When a custom threshold date is not set, members without the Read message history permission cannot view any historical messages. They'll only see messages in real time as they arrive, and they'll disappear when they restart their clients."), p).wrap())
        .child(
            Switch::new(on)
                .label(t("Enable message history threshold"))
                .description(t("Allow members without the Read message history permission to see messages sent after a specific date."))
                .on_toggle(move |s: &mut S| {
                    acc(s).form.data.insert("mht-on".into(), json!(!on));
                })
                .el(p),
        );
    let mut cal = Vec::new();
    if on {
        let edit = st.form.text.get("mht-time").cloned().unwrap_or_else(|| rune::TextEdit::new(initial.clone()));
        let today = chrono::Local::now().date_naive();
        let created_day = chrono::DateTime::from_timestamp_millis(created as i64).unwrap_or_default().with_timezone(&chrono::Local).date_naive();
        let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
        body = body.child(
            crate::ui::DatePicker::new("mht-date", Some(day))
                .label(t("Threshold date"))
                .description(t("Members without the Read message history permission can view messages sent after this date."))
                .min(Some(created_day))
                .max(Some(today))
                .time(&edit, move |s: &mut S| {
                    acc(s).form.text.entry("mht-time".into()).or_insert_with(|| rune::TextEdit::new(initial.clone()))
                })
                .on_pick(move |s: &mut S, d| {
                    acc(s).form.data.insert("mht-day".into(), json!(d.to_string()));
                })
                .el(&st.popups, pops, p, &mut cal),
        );
    }
    if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    let save = move |s: &mut S| {
        let sh = acc(s);
        let cutoff = if on {
            let local = crate::ui::datepicker::with_time(day, &time).and_local_timezone(chrono::Local).earliest();
            match threshold(local.map_or(0, |d| d.timestamp_millis()), created, crate::app::now_ms()) {
                Ok(c) => json!(c),
                Err(e) => return sh.form.error = Some(t(e).into()),
            }
        } else {
            Value::Null
        };
        sh.form.data.remove("mht");
        sh.api(Method::PATCH, format!("/guilds/{g}"), Some(json!({ "message_history_cutoff": cutoff })), Then::Toast("Message history threshold updated", "Failed to update message history threshold"));
    };
    let footer = row().gap(8.0).child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p)).child(Button::new(t("Save")).submitting(st.form.busy).on_press(save).el(p));
    layer.push(Modal::new(t("Message history threshold"), close).body(body).footer(footer).el(p));
    layer.extend(cal);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_dates_stay_in_range() {
        let created = 1_704_067_200_000; // 2024-01-01
        let now = 1_767_225_600_000; // 2026-01-01
        assert_eq!(threshold(1_748_736_000_000, created, now).unwrap(), "2025-06-01T00:00:00.000Z");
        assert_eq!(threshold(created as i64, created, now).unwrap(), "2024-01-01T00:00:00.000Z");
        assert!(threshold(created as i64 - 1, created, now).is_err());
        assert!(threshold(now as i64 + 1, created, now).is_err());
    }
}
