//! Instance and account nagbars: `InstanceNoticeNagbar` (discovery's `notice`),
//! `ScheduledMaintenanceNagbar` (the Instatus status page), `PremiumStatusNagbar` (grace period /
//! expired), `PremiumOnboardingNagbar` and `GiftInventoryNagbar`.
//!
//! `DesktopNotificationNagbar` never shows on desktop (`isDesktop()`), and neither do the price
//! nagbars on a self-hosted instance; on fluxer.app they need the billing `PremiumState`.
// shortcut: no price-announcement / legacy-price nagbars (they need Stripe's billing state).

use std::time::{Duration, Instant};

use reflux_model::{InstanceNotice, PremiumType};
use reflux_store::Store;
use rune::El;
use serde_json::{Value, json};

use crate::State;
use crate::i18n::{t, tf};
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::Palette;
use crate::ui::rgb;

/// `STATUS_PAGE_POLL_INTERVAL_MS`.
// shortcut: a flat 60 s poll, without Fluxer's jitter, start-time wakeups and failure backoff.
const POLL: Duration = Duration::from_secs(60);
const PRODUCT: &str = "Plutonium";
const PRODUCT_FULL: &str = "Fluxer Plutonium";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Scheduled,
    InProgress,
    Completed,
}

/// `StatusPageMaintenance`.
#[derive(Clone, Debug, PartialEq)]
pub struct Maintenance {
    pub id: String,
    pub status: Status,
    pub start: String,
    pub minutes: u64,
    pub url: String,
}

fn status_of(s: &str) -> Option<Status> {
    Some(match s {
        "NOTSTARTEDYET" => Status::Scheduled,
        "INPROGRESS" => Status::InProgress,
        "COMPLETED" => Status::Completed,
        _ => return None,
    })
}

/// `selectActiveStatusPageMaintenance`: in progress before scheduled; completed ones skipped.
fn select(list: &[Value]) -> Option<Maintenance> {
    let mut best: Option<Maintenance> = None;
    for m in list {
        let Some(status) = m["status"].as_str().and_then(status_of).filter(|s| *s != Status::Completed) else { continue };
        let minutes = m["duration"].as_u64().or_else(|| m["duration"].as_str().and_then(|d| d.parse().ok())).filter(|d| *d > 0);
        let (Some(minutes), Some(start)) = (minutes, m["start"].as_str()) else { continue };
        if best.as_ref().is_none_or(|b| b.status == Status::Scheduled && status == Status::InProgress) {
            best = Some(Maintenance { id: m["id"].as_str().unwrap_or_default().into(), status, start: start.into(), minutes, url: m["url"].as_str().unwrap_or_default().into() });
        }
    }
    best
}

async fn check(base: &str) -> reqwest::Result<Option<Maintenance>> {
    let http = reflux_net::http_client();
    let summary: Value = http.get(format!("{base}/summary.json")).send().await?.error_for_status()?.json().await?;
    let found = select(summary["activeMaintenances"].as_array().map_or(&[][..], Vec::as_slice));
    if found.is_some() || summary["page"]["status"].as_str() != Some("UNDERMAINTENANCE") {
        return Ok(found);
    }
    let comps: Value = http.get(format!("{base}/components.json")).send().await?.error_for_status()?.json().await?;
    let list = comps.as_array().or_else(|| comps["components"].as_array()).map_or(&[][..], Vec::as_slice);
    let all: Vec<Value> = list.iter().flat_map(|c| c["activeMaintenances"].as_array().cloned().unwrap_or_default()).collect();
    Ok(select(&all))
}

/// Polls the instance's Instatus page (`StatusPage.startPolling`).
pub fn tick(s: &mut State) {
    if rune::is_headless() || s.desktop.status_polled.is_some_and(|t| t.elapsed() < POLL) {
        return;
    }
    let Some(base) = s.app.instance.as_ref().map(|i| &i.discovery.app_public.branding).filter(|b| b.status_page_instatus).and_then(|b| b.status_page_url.clone()) else { return };
    s.desktop.status_polled = Some(Instant::now());
    let base = base.trim_end_matches('/').to_string();
    crate::app::spawn(async move {
        match check(&base).await {
            Ok(m) => crate::app::update(move |s| s.desktop.maintenance = m),
            Err(e) => eprintln!("reflux: status page: {e}"),
        }
    });
}

/// `isInstanceNoticeLive`.
pub fn live(n: &InstanceNotice, now: chrono::DateTime<chrono::Utc>) -> bool {
    let at = |v: &Option<String>| v.as_deref().and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok());
    at(&n.starts_at).is_none_or(|s| s <= now) && at(&n.ends_at).is_none_or(|e| now < e)
}

/// `formatMaintenanceDurationLabel`: whole days, else whole hours, else minutes.
fn duration(minutes: u64) -> String {
    let m = minutes.max(1);
    let (n, unit) = if m.is_multiple_of(1440) { (m / 1440, "day") } else if m.is_multiple_of(60) { (m / 60, "hour") } else { (m, "minute") };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

fn when(ts: &str) -> String {
    let Ok(d) = chrono::DateTime::parse_from_rfc3339(ts) else { return ts.to_string() };
    let d = d.with_timezone(&chrono::Local);
    let locale = crate::i18n::locale();
    format!("{}, {}", super::time::short_date(&d, locale), super::time::time(&d, locale))
}

fn maintenance_message(m: &Maintenance) -> String {
    match m.status {
        Status::InProgress => tf("Maintenance is in progress. Expected duration: {duration}.", &[("duration", &duration(m.minutes))]),
        Status::Completed => t("Maintenance is complete.").into(),
        Status::Scheduled => tf("Maintenance is scheduled for {localizedTime}. Expected duration: {duration}.", &[("localizedTime", &when(&m.start)), ("duration", &duration(m.minutes))]),
    }
}

fn dismissed(st: &ShellState, key: &str) -> bool {
    st.prefs.dismissed_nagbars.iter().any(|k| k == key)
}

fn dismiss<S: 'static>(acc: ShellAccess<S>, key: String) -> Box<dyn Fn(&mut S)> {
    Box::new(move |s: &mut S| {
        let sh = acc(s);
        let mut p = sh.prefs.clone();
        p.dismissed_nagbars.push(key.clone());
        sh.intents.push(Intent::Prefs(Box::new(p)));
    })
}

fn learn_more<S: 'static>(url: String, p: &Palette, acc: ShellAccess<S>) -> El {
    super::nagbar::button("nagbar-learn-more", t("Learn more"), p, move |s: &mut S| acc(s).media_ui.link_warning = Some((url.clone(), false)))
}

/// Maintenance, then the instance notice (they sit under Connection and Terms).
pub fn instance<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Vec<El> {
    let mut out = Vec::new();
    if let Some(m) = &st.desktop.maintenance {
        let key = format!("maintenance:{}:{:?}", m.id, m.status);
        if !dismissed(st, &key) {
            let bg = match m.status {
                Status::Scheduled => rgb(0x1d, 0x4e, 0xd8),
                Status::InProgress => rgb(0x9a, 0x34, 0x12),
                Status::Completed => rgb(0x16, 0x65, 0x34),
            };
            out.push(super::nagbar::bar("nagbar-maintenance", bg, maintenance_message(m), vec![learn_more(m.url.clone(), p, acc)], Some(dismiss(acc, key))));
        }
    }
    if let Some(n) = &st.desktop.notice {
        let key = format!("notice:{}", n.id);
        if !dismissed(st, &key) && live(n, chrono::Utc::now()) {
            let actions = n.url.clone().map(|u| learn_more(u, p, acc)).into_iter().collect();
            out.push(super::nagbar::bar("nagbar-notice", rgb(0x42, 0x85, 0xf4), n.message.clone(), actions, Some(dismiss(acc, key))));
        }
    }
    out
}

fn ms(ts: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(ts).ok().map(|d| d.timestamp_millis())
}

/// `PremiumGracePeriodNagbar`, `PremiumExpiredNagbar`, `PremiumOnboardingNagbar` and
/// `GiftInventoryNagbar`, in that order (under the email verification bar).
pub fn premium<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Vec<El> {
    let mut out = Vec::new();
    let Some(me) = store.me.as_ref().filter(|_| st.premium) else { return out };
    let now = chrono::Utc::now().timestamp_millis();
    let kind = me.premium_type.unwrap_or_default();
    let until = me.premium_until.as_deref().and_then(ms);
    // `getPremiumGraceEndDate`: the server's date, else 3 days past expiry.
    let grace_end = me.premium_grace_ends_at.as_deref().and_then(ms).or(until.map(|u| u + 3 * 86_400_000));
    let web = move |path: &'static str| {
        move |s: &mut S| {
            let sh = acc(s);
            let url = format!("{}{path}", sh.web_base.trim_end_matches('/'));
            sh.intents.push(Intent::Desktop(super::Request::OpenUrl(url)));
        }
    };
    // shortcut: "Manage subscription" / "Reactivate" open the web Plutonium page instead of
    // Stripe's customer portal.
    if let (Some(u), Some(g)) = (until, grace_end)
        && kind != PremiumType::LIFETIME
    {
        let grace = me.premium_grace_ends_at.is_some() && me.premium_billing_cycle.is_some() && now > u && now <= g;
        let expired = !me.premium_will_cancel && now > g && now <= g + 30 * 86_400_000;
        if grace && !dismissed(st, "premium-grace") {
            let date = chrono::DateTime::from_timestamp_millis(g).unwrap_or_default().with_timezone(&chrono::Local);
            let msg = tf(
                "Your {premiumProductName} subscription failed to renew. You still have access to all {premiumProductName} features until {formattedGraceDate}. Update your payment method before then to keep your access.",
                &[("premiumProductName", PRODUCT), ("formattedGraceDate", &super::time::long_date(&date, crate::i18n::locale()))],
            );
            out.push(super::nagbar::bar("nagbar-premium-grace", rgb(0xf9, 0x73, 0x16), msg, vec![super::nagbar::button("nagbar-premium-manage", t("Manage subscription"), p, web("/channels/@premium"))], Some(dismiss(acc, "premium-grace".into()))));
        }
        if expired && !dismissed(st, "premium-expired") {
            let msg = tf(
                "Your {premiumProductName} subscription has expired, and you no longer have access to {premiumProductName} features. Reactivate your subscription to get access again.",
                &[("premiumProductName", PRODUCT)],
            );
            out.push(super::nagbar::bar("nagbar-premium-expired", p.status_danger, msg, vec![super::nagbar::button("nagbar-premium-reactivate", t("Reactivate"), p, web("/channels/@premium"))], Some(dismiss(acc, "premium-expired".into()))));
        }
    }
    let premium_now = kind == PremiumType::LIFETIME || (kind != PremiumType::default() && until.is_none_or(|u| u > now));
    if premium_now && !me.has_dismissed_premium_onboarding && !dismissed(st, "premium-onboarding") {
        let close = move |s: &mut S| {
            let sh = acc(s);
            let mut p = sh.prefs.clone();
            p.dismissed_nagbars.push("premium-onboarding".into());
            sh.intents.push(Intent::Prefs(Box::new(p)));
            sh.api(reqwest::Method::PATCH, "/users/@me", Some(json!({ "has_dismissed_premium_onboarding": true })), crate::guild::Then::Nothing);
        };
        let view = move |s: &mut S| {
            close(s);
            web("/channels/@premium")(s);
        };
        let label = tf("View {premiumProductName} features", &[("premiumProductName", PRODUCT)]);
        out.push(super::nagbar::bar("nagbar-premium-onboarding", p.brand_primary, onboarding(me, now), vec![super::nagbar::button("nagbar-premium-view", &label, p, view)], Some(Box::new(close))));
    }
    if me.has_unread_gift_inventory {
        let n = me.unread_gift_inventory_count.max(1);
        let msg = if n == 1 { t("You have a new gift code waiting in your gift inventory.").to_string() } else { tf("You have {count} new gift codes waiting in your gift inventory.", &[("count", &n.to_string())]) };
        let open = move |s: &mut S| acc(s).open_settings(Some("gift_inventory"));
        let close = move |s: &mut S| acc(s).api(reqwest::Method::PATCH, "/users/@me", Some(json!({ "has_unread_gift_inventory": false })), crate::guild::Then::Nothing);
        out.push(super::nagbar::bar("nagbar-gifts", p.brand_primary, msg, vec![super::nagbar::button("nagbar-gifts-view", t("View gift inventory"), p, open)], Some(Box::new(close))));
    }
    out
}

/// `useOnboardingMessage`, from the user's plan (Fluxer prefers the store subscription's).
fn onboarding(me: &reflux_model::User, now: i64) -> String {
    let names = [("premiumProductFullName", PRODUCT_FULL), ("premiumProductName", PRODUCT)];
    let with_date = |pat: &str, ts: &str| {
        let date = chrono::DateTime::parse_from_rfc3339(ts).map(|d| super::time::long_date(&d.with_timezone(&chrono::Local), crate::i18n::locale())).unwrap_or_default();
        tf(pat, &[names[0], names[1], ("formattedDate", &date)])
    };
    if me.premium_type == Some(PremiumType::LIFETIME) {
        return tf("Welcome to {premiumProductFullName}. You have lifetime access. See all {premiumProductName} features.", &names);
    }
    let until = me.premium_until.as_deref().filter(|u| ms(u).is_some_and(|u| u > now));
    match (until, me.premium_will_cancel, me.premium_billing_cycle.as_deref()) {
        (Some(u), true, _) => with_date("Welcome to {premiumProductFullName}. Your subscription is active until {formattedDate}. See all {premiumProductName} features and manage your subscription.", u),
        (Some(u), _, Some("monthly")) => with_date("Welcome to {premiumProductFullName}. Your monthly subscription renews on {formattedDate}. See all {premiumProductName} features and manage your subscription.", u),
        (Some(u), _, Some(_)) => with_date("Welcome to {premiumProductFullName}. Your yearly subscription renews on {formattedDate}. See all {premiumProductName} features and manage your subscription.", u),
        (Some(u), _, None) => with_date("Welcome to {premiumProductFullName}. Your gift subscription runs until {formattedDate}. See all {premiumProductName} features.", u),
        _ => tf("Welcome to {premiumProductFullName}. See all {premiumProductName} features and manage your subscription.", &names),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_in_progress_maintenance_and_formats_duration() {
        let list = json!([
            {"id": "a", "status": "NOTSTARTEDYET", "start": "2026-01-01T00:00:00Z", "duration": "60", "url": "u"},
            {"id": "b", "status": "INPROGRESS", "start": "2026-01-01T00:00:00Z", "duration": 90, "url": "u"},
            {"id": "c", "status": "COMPLETED", "start": "2026-01-01T00:00:00Z", "duration": 5, "url": "u"},
        ]);
        let m = select(list.as_array().unwrap()).unwrap();
        assert_eq!((m.id.as_str(), m.status, m.minutes), ("b", Status::InProgress, 90));
        assert_eq!((duration(60), duration(2880), duration(90), duration(0)), ("1 hour".into(), "2 days".into(), "90 minutes".into(), "1 minute".into()));
    }

    #[test]
    fn notice_window() {
        let n = InstanceNotice { starts_at: Some("2026-01-02T00:00:00Z".into()), ends_at: Some("2026-01-03T00:00:00Z".into()), ..Default::default() };
        let at = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&chrono::Utc);
        assert!(!live(&n, at("2026-01-01T12:00:00Z")));
        assert!(live(&n, at("2026-01-02T12:00:00Z")));
        assert!(!live(&n, at("2026-01-03T00:00:00Z")));
    }
}
