//! The rest of the community Overview tab (`guild_overview_tab`): the detached banner switch,
//! invite / embed splashes and card alignment, Previewing, the Advanced section and the
//! message history threshold modal.

use reflux_model::{Guild, Permissions as P, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, col, el, icon, row, text};
use serde_json::{Map, Value, json};

use super::settings::{image_field, small, title};
use super::{Then, error, field};
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
    // `CardAlignmentControls`: left / centre / right.
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
    let alignment = col()
        .gap(8.0)
        .items_start()
        .child(title(t("Card alignment"), p))
        .child(row().gap(4.0).px(4.0).py(4.0).radius(8.0).fill(p.background_tertiary).child(seg(1, &icons::TEXT_ALIGN_LEFT, "Left")).child(seg(0, &icons::TEXT_ALIGN_CENTER, "Center")).child(seg(2, &icons::TEXT_ALIGN_RIGHT, "Right")))
        .child(small(t("Only applies on wide screens."), p));
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
        .child(row().size(28.0, 28.0).center().radius(6.0).hover(p.background_modifier_hover).child(icon(if open { &icons::CARET_UP } else { &icons::CARET_DOWN }, 18.0).color(p.text_primary_muted)));
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

/// A `YYYY-MM-DD` threshold as RFC 3339, or why it's refused (before the community or future).
pub fn threshold(input: &str, created_ms: u64, now_ms: u64) -> Result<String, &'static str> {
    let d = chrono::NaiveDate::parse_from_str(input.trim(), "%Y-%m-%d").map_err(|_| "Enter a date as YYYY-MM-DD.")?;
    let at = d.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc();
    let ms = at.timestamp_millis().max(0) as u64;
    if ms + 86_400_000 <= created_ms {
        return Err("Threshold date cannot be before community creation");
    }
    if ms > now_ms {
        return Err("Threshold date cannot be in the future");
    }
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
                    f.text.remove("mht-date");
                    f.error = None;
                })
                .el(p),
        )
    })
}

/// `MessageHistoryThresholdModal`.
pub fn threshold_modal<S: 'static>(st: &ShellState, g: Snowflake, info: &Guild, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    st.form.data.get("mht")?;
    let cur = info.message_history_cutoff.clone();
    let on = st.form.data.get("mht-on").and_then(Value::as_bool).unwrap_or(cur.is_some());
    let initial = cur.as_deref().and_then(|c| c.get(..10)).unwrap_or("").to_string();
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
    if on {
        // shortcut: a typed date; reflux has no date picker.
        body = body.child(field(st, acc, "mht-date", &initial, t("Threshold date"), "YYYY-MM-DD", p)).child(small(t("Members without the Read message history permission can view messages sent after this date."), p).wrap());
    }
    if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    let created = g.timestamp_ms();
    let save = move |s: &mut S| {
        let sh = acc(s);
        let cutoff = if on {
            match threshold(&sh.form.value("mht-date", &initial), created, crate::app::now_ms()) {
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
    Some(Modal::new(t("Message history threshold"), close).body(body).footer(footer).el(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_dates_stay_in_range() {
        let created = 1_704_067_200_000; // 2024-01-01
        let now = 1_767_225_600_000; // 2026-01-01
        assert_eq!(threshold("2025-06-01", created, now).unwrap(), "2025-06-01T00:00:00.000Z");
        assert_eq!(threshold("2024-01-01", created, now).unwrap(), "2024-01-01T00:00:00.000Z");
        assert!(threshold("2023-12-30", created, now).is_err());
        assert!(threshold("2027-01-01", created, now).is_err());
        assert!(threshold("June", created, now).is_err());
    }
}
