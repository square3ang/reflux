//! Profile local time (`my_profile_tab/TimezoneProfileSettings.tsx`): the Profile tab section and
//! its modal, a searchable time zone list (`getTimeZoneDisplayOptions`, the @vvo/tzdb records in
//! [`super::timezones`]) and who may see the offset. Choices go into the profile draft
//! (`form.data["p.timezone"]`, `["p.timezone_privacy_flags"]`) and save with it.
// shortcut: the profile preview doesn't show the local time.

use reflux_model::ProfileFieldPrivacyFlags as F;
use reflux_store::Store;
use rune::{El, col, row, text};
use serde_json::Value;

use super::account::muted;
use super::timezones::ZONES;
use crate::i18n::{t, tf};
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Input, Switch};

/// The draft's time zone and privacy flags, falling back to the saved ones.
pub(super) fn draft(store: &Store, st: &ShellState) -> (Option<String>, u64) {
    let me = store.me.as_ref();
    let tz = match st.form.data.get("p.timezone") {
        Some(v) => v.as_str().map(str::to_string),
        None => me.and_then(|m| m.timezone.clone()),
    };
    let flags = st.form.data.get("p.timezone_privacy_flags").and_then(Value::as_u64).unwrap_or_else(|| me.map_or(F::EVERYONE.bits(), |m| m.timezone_privacy_flags.bits()));
    (tz, flags)
}

/// `formatUtcOffset`.
fn utc(min: i32) -> String {
    let a = min.unsigned_abs();
    format!("UTC{}{:02}:{:02}", if min >= 0 { '+' } else { '-' }, a / 60, a % 60)
}

/// The offset `name` has now (the runtime's tz database), else its standard one.
fn offset(name: &str, raw: i32) -> i32 {
    jiff::tz::TimeZone::get(name).map_or(raw, |z| z.to_offset(jiff::Timestamp::now()).seconds() / 60)
}

/// `getTimeZoneDisplayOptions`: (value, label, search text) sorted by offset then label; UTC
/// joins as "UTC".
pub(super) fn options() -> Vec<(String, String, String)> {
    let mut v: Vec<(i32, String, String, String)> = ZONES
        .iter()
        .map(|&(name, alt, cities, search, raw)| {
            let o = offset(name, raw);
            let place = if cities.is_empty() { alt.to_string() } else { format!("{alt} - {cities}") };
            (o, name.to_string(), format!("{} {place}", utc(o)), search.to_string())
        })
        .collect();
    v.push((0, "UTC".into(), format!("{} UTC", utc(0)), "Etc/UTC UTC Universal Zulu GMT Greenwich Coordinated Universal Time (UTC)".into()));
    v.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));
    v.into_iter().map(|(_, n, l, s)| (n, l, s)).collect()
}

/// The Profile tab's "Profile local time" section (global profile only).
pub(super) fn section<S: 'static>(p: &Palette, acc: ShellAccess<S>) -> El {
    col()
        .w_full()
        .gap(8.0)
        .child(text(t("Profile local time")).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary))
        .child(muted(tf("Set your time zone once so {productName} can keep your UTC offset current when daylight saving time changes. Other people can only see your UTC offset, not your exact time zone identifier.", &[("productName", "Reflux")]), p))
        .child(row().child(Button::new(t("Edit profile local time")).key("profile-timezone").variant(ButtonVariant::Secondary).on_press(move |s: &mut S| acc(s).nav.timezone = true).el(p)))
}

/// `TimezoneProfileSettingsModal`.
pub(super) fn modal<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let (tz, flags) = draft(store, st);
    let q = st.nav.timezone_query.text.trim().to_lowercase();
    let pick = |value: Option<String>, label: String| {
        let on = value == tz;
        let had = tz.is_some();
        row()
            .key(format!("tz-{}", value.as_deref().unwrap_or("")))
            .w_full()
            .px(10.0)
            .py(6.0)
            .radius(4.0)
            .fill(if on { p.background_modifier_selected } else { rune::Color::TRANSPARENT })
            .hover(p.background_modifier_hover)
            .cursor(rune::Cursor::Pointer)
            .role("option")
            .child(text(label).font(font(14.0, if on { 600.0 } else { 400.0 }).lh(20.0)).color(p.text_primary).truncate())
            .on_press(move |s: &mut S| {
                let f = &mut acc(s).form;
                // A first time zone starts out shared with everyone.
                if !had && value.is_some() {
                    f.data.insert("p.timezone_privacy_flags".into(), Value::from(F::EVERYONE.bits()));
                }
                f.data.insert("p.timezone".into(), value.clone().map_or(Value::Null, Value::String));
            })
    };
    let opts = options();
    let mut list = col().w_full().max_h(220.0).scroll(rune::ScrollSpec { stick_bottom: false });
    let not_set = t("Not set").to_string();
    if q.is_empty() || not_set.to_lowercase().contains(&q) {
        list = list.child(pick(None, not_set));
    }
    for (value, label, search) in opts.iter().cloned() {
        if q.is_empty() || label.to_lowercase().contains(&q) || search.to_lowercase().contains(&q) {
            list = list.child(pick(Some(value), label));
        }
    }
    let current = tz.as_deref().map_or_else(|| t("Not set").to_string(), |z| opts.iter().find(|o| o.0 == z).map_or_else(|| z.to_string(), |o| o.1.clone()));
    let picker = col()
        .w_full()
        .gap(8.0)
        .child(text(t("Time zone")).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary_muted))
        .child(text(current).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary))
        .child(Input::new("tz-search", &st.nav.timezone_query, move |s: &mut S| &mut acc(s).nav.timezone_query).placeholder(t("Search time zones")).el(p))
        .child(col().w_full().radius(8.0).border(1.0, p.background_modifier_accent).fill(p.form_surface_background).padding(4.0).child(list))
        .child(muted(tf("Choose the time zone {productName} uses to calculate your UTC offset for profile local time.", &[("productName", "Reflux")]), p));
    let none = tz.is_none();
    let everyone = flags & F::EVERYONE.bits() != 0;
    let toggle = move |flag: u64, on: bool| {
        move |s: &mut S| {
            let f = &mut acc(s).form;
            let next = if on { flags & !flag } else { flags | flag };
            f.data.insert("p.timezone_privacy_flags".into(), Value::from(next));
        }
    };
    let switch = |label: &str, desc: &str, flag: u64, on: bool, disabled: bool| Switch::new(on).label(t(label)).description(t(desc)).disabled(disabled).on_toggle(toggle(flag, on)).el(p);
    let body = col()
        .w_full()
        .gap(16.0)
        .child(muted(
            tf(
                "Other people can only see your current UTC offset when you choose to share profile local time. They do not see your exact time zone identifier, such as {timezoneIdentifierExample}. {productName} stores that identifier only so the offset can update automatically when daylight saving time changes.",
                &[("timezoneIdentifierExample", "America/New_York"), ("productName", "Reflux")],
            ),
            p,
        ))
        .child(picker)
        .child(
            col()
                .w_full()
                .gap(12.0)
                .child(switch("Everyone", "Allow anyone who can view your full profile to see your local time", F::EVERYONE.bits(), !none && everyone, none))
                .child(switch("Friends", "Allow your friends to see your local time", F::FRIENDS.bits(), !none && (everyone || flags & F::FRIENDS.bits() != 0), none || everyone))
                .child(switch("Community members", "Allow members from communities you're in to see your local time", F::MUTUAL_GUILDS.bits(), !none && (everyone || flags & F::MUTUAL_GUILDS.bits() != 0), none || everyone)),
        );
    crate::ui::Modal::new(t("Profile local time"), move |s: &mut S| acc(s).nav.timezone = false).body(body).el(p)
}
