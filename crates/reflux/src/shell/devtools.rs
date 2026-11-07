//! The staff / developer-mode "Developer tools" header menu (`DeveloperToolsContextMenu` and
//! `channel_header_components/developer_tools/*`): the override summary, general toggles,
//! account & premium overrides, mocked product flows, nagbar overrides and runtime utilities.
//! Overrides are local (`desktop::Prefs::dev`, like Fluxer's persisted `DeveloperOptions`);
//! only the items Fluxer itself sends to the server (backend premium override, premium perks,
//! mention suppression, forget authorized IPs, clear trusted domains, reset premium state,
//! open system DM, force AFK move) touch the account. Items with no Reflux counterpart are
//! listed in docs/WORKAROUNDS.md.

use std::borrow::Cow;
use std::collections::BTreeMap;

use reflux_model::{PremiumType, Snowflake, User};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, Icon, icon, row, text};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Intent, ShellAccess, ShellState};
use crate::guild::Then;
use crate::i18n::{t, tf, tp};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::{Button, ButtonVariant, Menu, MenuEntry, MenuItem, Modal, Placement, menu_slider};

pub const MENU: &str = "dev-tools";
/// `SudoPrompt.requestVerification({path: '<developer-tools>'})`: the prompt closes once proven.
pub const SUDO_PATH: &str = "<developer-tools>";
const DAY: i64 = 86_400_000;

/// `FluxerStaffIcon` (512 viewBox).
pub static STAFF_ICON: Icon = Icon {
    name: "fluxer-staff",
    view: 512.0,
    body: r#"<path d="M55.4407 82.8799C20.3377 117.576 14.1307 156.77 22.2127 200.717C28.5027 234.919 44.2917 271.524 63.1047 308.484C80.8427 281.37 104.222 251.66 131.781 229.967C111.081 212.803 93.5197 194.076 80.4137 173.52C63.1237 146.4 53.9097 115.91 55.4407 82.8799ZM462.559 82.8799C464.09 115.91 454.876 146.4 437.586 173.52C424.48 194.076 406.919 212.803 386.219 229.967C413.778 251.66 437.157 281.37 454.895 308.484C473.708 271.524 489.498 234.919 495.787 200.717C503.869 156.77 497.662 117.575 462.559 82.8799ZM259 208C250.298 208 241.939 210.757 236 215.316V237.696C242.7 235.048 250.535 233.68 259 233.68C267.465 233.68 275.3 235.048 282 237.695V215.316C276.061 210.757 267.702 208 259 208ZM218 238.053C187.515 249.63 157.957 272.713 133.834 300.857C101.718 338.326 79.7837 384.501 76.4817 420H218V238.053ZM300 238.053V420H441.518C438.217 384.501 416.282 338.326 384.166 300.857C360.043 272.714 330.485 249.63 300 238.053ZM146.498 241.543C117.401 263.718 91.3087 297.755 72.7657 327.049C75.886 332.918 79.035 338.772 82.2127 344.611C92.3747 325.385 105.301 306.485 120.166 289.143C132.149 275.163 145.455 262.178 159.723 250.988C155.253 247.926 150.844 244.777 146.498 241.543ZM371.502 241.543C367.155 244.777 362.746 247.926 358.277 250.988C372.545 262.178 385.851 275.163 397.834 289.143C412.699 306.485 425.624 325.385 435.787 344.611C438.966 338.761 442.126 332.906 445.234 327.049C426.691 297.755 400.599 263.719 371.502 241.543ZM259 251.68C251.38 251.68 244.551 253.34 240.398 255.416C237.136 257.047 236.295 258.389 236.08 258.68C236.295 258.97 237.136 260.312 240.398 261.943C244.551 264.02 251.379 265.68 259 265.68C266.62 265.68 273.449 264.02 277.602 261.943C280.864 260.313 281.705 258.971 281.92 258.68C281.705 258.389 280.864 257.047 277.602 255.416C273.449 253.339 266.621 251.68 259 251.68ZM281.92 258.68C281.979 258.76 282 258.775 282 258.68C282 258.584 281.979 258.6 281.92 258.68ZM236.08 258.68C236.021 258.6 236 258.584 236 258.68C236 258.775 236.021 258.76 236.08 258.68ZM236 279.664V328.016C242.7 325.368 250.535 324 259 324C267.465 324 275.3 325.368 282 328.016V279.664C275.3 282.312 267.465 283.68 259 283.68C250.535 283.68 242.7 282.312 236 279.664ZM259 342C251.38 342 244.551 343.66 240.398 345.736C237.136 347.368 236.295 348.709 236.08 349C236.295 349.291 237.136 350.632 240.398 352.264C244.551 354.34 251.38 356 259 356C266.62 356 273.449 354.34 277.602 352.264C280.864 350.632 281.705 349.291 281.92 349C281.705 348.709 280.864 347.368 277.602 345.736C273.449 343.66 266.62 342 259 342ZM281.92 349C281.979 349.08 282 349.096 282 349C282 348.904 281.979 348.92 281.92 349ZM236.08 349C236.021 348.92 236 348.904 236 349C236 349.096 236.021 349.08 236.08 349ZM236 369.984V415.854C242.7 413.205 250.535 411.838 259 411.838C267.465 411.838 275.3 413.205 282 415.854V369.984C275.3 372.632 267.465 374 259 374C250.535 374 242.7 372.632 236 369.984ZM259 429.838C251.38 429.838 244.551 431.498 240.398 433.574C237.136 435.205 236.295 436.547 236.08 436.838C236.295 437.128 237.136 438.47 240.398 440.102C244.551 442.178 251.379 443.838 259 443.838C266.62 443.838 273.449 442.178 277.602 440.102C280.864 438.47 281.705 437.129 281.92 436.838C281.705 436.547 280.864 435.205 277.602 433.574C273.449 431.498 266.621 429.838 259 429.838ZM281.92 436.838C281.979 436.918 282 436.933 282 436.838C282 436.742 281.979 436.758 281.92 436.838ZM236.08 436.838C236.021 436.758 236 436.742 236 436.838C236 436.933 236.021 436.918 236.08 436.838ZM75.9997 438V468H94.4547C91.6747 463.578 89.9997 458.48 89.9997 453C89.9997 447.52 91.6757 442.422 94.4547 438H75.9997ZM131 438C123.987 438 117.806 440.204 113.773 443.229C109.74 446.253 108 449.615 108 453C108 456.385 109.74 459.747 113.773 462.771C117.806 465.796 123.987 468 131 468C138.013 468 144.194 465.796 148.227 462.771C152.26 459.747 154 456.385 154 453C154 449.615 152.26 446.253 148.227 443.229C144.194 440.204 138.013 438 131 438ZM167.545 438C170.325 442.422 172 447.52 172 453C172 458.48 170.324 463.578 167.545 468H218V438H167.545ZM300 438V468H350.455C347.675 463.578 346 458.48 346 453C346 447.52 347.676 442.422 350.455 438H300ZM387 438C379.987 438 373.806 440.204 369.773 443.229C365.74 446.253 364 449.615 364 453C364 456.385 365.74 459.747 369.773 462.771C373.806 465.796 379.987 468 387 468C394.013 468 400.194 465.796 404.227 462.771C408.26 459.747 410 456.385 410 453C410 449.615 408.26 446.253 404.227 443.229C400.194 440.204 394.013 438 387 438ZM423.545 438C426.325 442.422 428 447.52 428 453C428 458.48 426.324 463.578 423.545 468H442V438H423.545ZM236 457.822V474.275L259 508.775L282 474.275V457.822C275.3 460.47 267.465 461.838 259 461.838C250.535 461.838 242.7 460.47 236 457.822Z"/>"#,
};

// ---- Options ------------------------------------------------------------------------------------

/// The ported `DeveloperOptionsState` keys (Fluxer's names in snake case), kept in session.json.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub bypass_loading_skeleton: bool,
    pub force_loading_skeleton: bool,
    pub show_myself_typing: bool,
    pub self_hosted_mode_override: bool,
    pub slow_message_load: bool,
    pub slow_message_send: bool,
    pub slow_profile_load: bool,
    pub force_profile_skeletons: bool,
    pub force_profile_data_warning: bool,
    pub force_fail_message_sends: bool,
    pub force_fail_message_loads: bool,
    pub force_unknown_message_type: bool,
    pub no_op_in_app_reports: bool,
    pub premium_scenario_override: Option<String>,
    pub premium_type_override: Option<u8>,
    pub premium_lifetime_sequence_override: Option<i64>,
    /// Unix ms.
    pub premium_since_override: Option<i64>,
    pub premium_until_override: Option<i64>,
    pub premium_billing_cycle_override: Option<String>,
    pub premium_will_cancel_override: Option<bool>,
    pub has_ever_purchased_override: Option<bool>,
    pub has_unread_gift_inventory_override: Option<bool>,
    pub unread_gift_inventory_count_override: Option<u32>,
    pub email_verified_override: Option<bool>,
    pub unclaimed_account_override: Option<bool>,
    /// `none`, `geo_restricted`, `mature_content_check_required`, `consent_required`.
    pub mock_mature_content_gate_reason: String,
    /// `none`, `geo_restricted`, `mature_content_check_required`.
    pub mock_mature_media_gate_reason: String,
    pub force_mature_media: bool,
    #[serde(rename = "mock_in_uk")]
    pub mock_in_uk: bool,
    /// `none`, `unclaimed_account`, `unverified_email`, `account_too_new`, `not_member_long`,
    /// `account_limited`, `send_message_disabled`.
    pub mock_verification_barrier: String,
    /// ms; `None` = the barrier's default.
    pub mock_barrier_time_remaining: Option<u64>,
    pub force_no_send_messages: bool,
    pub force_no_attach_files: bool,
    pub mock_slowmode_active: bool,
    /// ms.
    pub mock_slowmode_remaining: u64,
    pub mock_gift_inventory: Option<bool>,
    pub mock_gift_duration_months: u32,
    pub mock_gift_redeemed: Option<bool>,
    /// `Nagbar` force flags by control key: `true` = force show, `false` = force hide.
    pub nagbars: BTreeMap<String, bool>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            bypass_loading_skeleton: false,
            force_loading_skeleton: false,
            show_myself_typing: false,
            self_hosted_mode_override: false,
            slow_message_load: false,
            slow_message_send: false,
            slow_profile_load: false,
            force_profile_skeletons: false,
            force_profile_data_warning: false,
            force_fail_message_sends: false,
            force_fail_message_loads: false,
            force_unknown_message_type: false,
            no_op_in_app_reports: false,
            premium_scenario_override: None,
            premium_type_override: None,
            premium_lifetime_sequence_override: None,
            premium_since_override: None,
            premium_until_override: None,
            premium_billing_cycle_override: None,
            premium_will_cancel_override: None,
            has_ever_purchased_override: None,
            has_unread_gift_inventory_override: None,
            unread_gift_inventory_count_override: None,
            email_verified_override: None,
            unclaimed_account_override: None,
            mock_mature_content_gate_reason: "none".into(),
            mock_mature_media_gate_reason: "none".into(),
            force_mature_media: false,
            mock_in_uk: false,
            mock_verification_barrier: "none".into(),
            mock_barrier_time_remaining: None,
            force_no_send_messages: false,
            force_no_attach_files: false,
            mock_slowmode_active: false,
            mock_slowmode_remaining: 10_000,
            mock_gift_inventory: None,
            mock_gift_duration_months: 12,
            mock_gift_redeemed: None,
            nagbars: BTreeMap::new(),
        }
    }
}

/// `PremiumScenarioOptions`: (value, label, (type, ever purchased, since days, until days, cycle, will cancel)).
type Scenario = (&'static str, &'static str, (u8, bool, Option<i64>, Option<i64>, Option<&'static str>, bool));
const SCENARIOS: &[Scenario] = &[
    ("free_no_purchases", "Free user (no purchases)", (0, false, None, None, None, false)),
    ("free_with_history", "Free user (with purchase history)", (0, true, None, None, None, false)),
    ("active_monthly", "Active monthly subscriber", (1, true, Some(-15), Some(15), Some("monthly"), false)),
    ("active_monthly_cancelled", "Active monthly subscriber (cancellation scheduled)", (1, true, Some(-20), Some(10), Some("monthly"), true)),
    ("active_yearly", "Active yearly subscriber", (1, true, Some(-180), Some(185), Some("yearly"), false)),
    ("active_yearly_cancelled", "Active yearly subscriber (cancellation scheduled)", (1, true, Some(-250), Some(60), Some("yearly"), true)),
    ("gift_active_no_history", "Gift: active (no purchase history)", (1, false, Some(-10), Some(20), None, false)),
    ("gift_active_with_history", "Gift: active (has purchase history)", (1, true, Some(-5), Some(60), None, false)),
    ("gift_expiring_soon", "Gift: expiring soon", (1, false, Some(-25), Some(1), None, false)),
    ("gift_grace_period_active", "Gift: grace period (still have access)", (1, false, Some(-31), Some(-1), None, false)),
    ("gift_expired_recent", "Gift: expired (within 30 days)", (0, false, Some(-35), Some(-5), None, false)),
    ("grace_period_active", "Grace period (still have access)", (1, true, Some(-31), Some(-1), Some("yearly"), false)),
    ("expired_recent", "Expired (within 30 days)", (0, true, Some(-35), Some(-5), Some("yearly"), false)),
    ("expired_old", "Expired (over 30 days ago)", (0, true, Some(-65), Some(-35), Some("yearly"), false)),
    ("visionary", "Visionary ({visionaryLifetimeBadgeLabel})", (2, true, Some(-365), None, None, false)),
];

impl Options {
    /// `updateOption` for a premium state key: a hand-set value drops the applied scenario.
    fn touch_premium(&mut self) {
        self.premium_scenario_override = None;
    }

    /// `applyPremiumScenarioOption`.
    pub fn apply_scenario(&mut self, value: &str, now_ms: i64) {
        let Some((_, _, (kind, ever, since, until, cycle, cancel))) = SCENARIOS.iter().find(|s| s.0 == value) else { return };
        self.premium_type_override = Some(*kind);
        self.has_ever_purchased_override = Some(*ever);
        self.premium_since_override = since.map(|d| now_ms + d * DAY);
        self.premium_until_override = until.map(|d| now_ms + d * DAY);
        self.premium_billing_cycle_override = cycle.map(str::to_owned);
        self.premium_will_cancel_override = Some(*cancel);
        self.premium_scenario_override = Some(value.to_owned());
    }

    /// `resetPremiumStateOverrides`.
    pub fn reset_premium(&mut self) {
        let d = Self::default();
        self.premium_scenario_override = None;
        self.premium_type_override = None;
        self.has_ever_purchased_override = None;
        self.premium_since_override = None;
        self.premium_until_override = None;
        self.premium_billing_cycle_override = None;
        self.premium_will_cancel_override = None;
        self.has_unread_gift_inventory_override = d.has_unread_gift_inventory_override;
        self.unread_gift_inventory_count_override = d.unread_gift_inventory_count_override;
    }

    /// Whether any override reshapes the current user (`User` model getters).
    fn touches_user(&self) -> bool {
        let d = Self::default();
        (self.premium_type_override, self.premium_lifetime_sequence_override, self.premium_since_override, self.premium_until_override) != (None, None, None, None)
            || self.premium_billing_cycle_override.is_some()
            || (self.premium_will_cancel_override, self.has_ever_purchased_override, self.has_unread_gift_inventory_override, self.email_verified_override, self.unclaimed_account_override)
                != (d.premium_will_cancel_override, d.has_ever_purchased_override, d.has_unread_gift_inventory_override, d.email_verified_override, d.unclaimed_account_override)
            || self.unread_gift_inventory_count_override.is_some()
    }

    /// The user as Fluxer's `User` getters see it under these overrides.
    pub fn apply(&self, me: &User) -> User {
        let mut u = me.clone();
        let ts = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        if self.unclaimed_account_override == Some(true) {
            u.account_tier = Some("unclaimed".into());
            u.claimed = Some(false);
        }
        if let Some(v) = self.email_verified_override {
            u.verified = v;
            if self.unclaimed_account_override != Some(true) {
                u.account_tier = Some(if v { "verified" } else { "unverified" }.into());
            }
        }
        if let Some(k) = self.premium_type_override {
            u.premium_type = Some(PremiumType(k));
        }
        // A scenario sets "no date" explicitly; a hand-set type keeps the real dates.
        let scenario = self.premium_scenario_override.is_some();
        if scenario || self.premium_since_override.is_some() {
            u.premium_since = self.premium_since_override.and_then(ts);
        }
        if scenario || self.premium_until_override.is_some() {
            u.premium_until = self.premium_until_override.and_then(ts);
        }
        if scenario || self.premium_billing_cycle_override.is_some() {
            u.premium_billing_cycle = self.premium_billing_cycle_override.clone();
        }
        if let Some(v) = self.premium_lifetime_sequence_override {
            u.premium_lifetime_sequence = Some(v);
        }
        if let Some(v) = self.premium_will_cancel_override {
            u.premium_will_cancel = v;
        }
        if let Some(v) = self.has_ever_purchased_override {
            u.has_ever_purchased = v;
        }
        if let Some(v) = self.has_unread_gift_inventory_override {
            u.has_unread_gift_inventory = v;
        }
        if let Some(v) = self.unread_gift_inventory_count_override {
            u.unread_gift_inventory_count = v;
        }
        u
    }

    /// `getActiveDeveloperOptionEntries`: (key, label, value) of every non-default option but
    /// the scenario and the nagbars.
    pub fn active(&self) -> Vec<(String, String, String)> {
        let (now, def) = (serde_json::to_value(self).unwrap_or_default(), serde_json::to_value(Self::default()).unwrap_or_default());
        let (Value::Object(now), Value::Object(def)) = (now, def) else { return Vec::new() };
        now.into_iter()
            .filter(|(k, v)| k != "premium_scenario_override" && k != "nagbars" && def.get(k) != Some(v))
            .map(|(k, v)| {
                let label = label(&k);
                let value = value(&k, &v);
                (k, label, value)
            })
            .collect()
    }

    /// `resetDeveloperOption`.
    pub fn reset(&mut self, key: &str) {
        let (Ok(Value::Object(mut now)), Ok(Value::Object(def))) = (serde_json::to_value(&*self), serde_json::to_value(Self::default())) else { return };
        if let Some(v) = def.get(key) {
            now.insert(key.into(), v.clone());
        }
        if let Ok(o) = serde_json::from_value(Value::Object(now)) {
            *self = o;
        }
        if matches!(key, "premium_type_override" | "premium_since_override" | "premium_until_override" | "premium_billing_cycle_override" | "premium_will_cancel_override" | "has_ever_purchased_override") {
            self.touch_premium();
        }
    }

    /// `resetAllDeveloperOptions` (the nagbars are reset separately, as `resetAllNagbars`).
    pub fn reset_all(&mut self) {
        *self = Self { nagbars: std::mem::take(&mut self.nagbars), ..Self::default() };
    }

    /// The mature channel gate a mock forces (`GuildMatureContentAgree.getGateReason`).
    pub fn mature_gate(&self) -> Option<super::attachments::Gate> {
        use super::attachments::Gate;
        match self.mock_mature_content_gate_reason.as_str() {
            "geo_restricted" => Some(Gate::Geo),
            "mature_content_check_required" => Some(Gate::Check),
            "consent_required" => Some(Gate::Consent),
            _ => None,
        }
    }

    /// `isEffectiveMatureMedia`'s developer half: every media item counts as mature.
    pub fn mature_media(&self) -> bool {
        self.force_mature_media || self.mock_mature_media_gate_reason != "none"
    }

    /// `MockingMenu`'s gift: `GiftCommands.mockGiftMetadata`.
    pub fn mock_gifts(&self, me: Value, now_ms: i64) -> Value {
        let months = self.mock_gift_duration_months;
        let (kind, qty) = if months != 0 && months.is_multiple_of(12) { ("years", months / 12) } else { ("months", months) };
        let at = |d: i64| chrono::DateTime::from_timestamp_millis(now_ms - d * DAY).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        let redeemed = self.mock_gift_redeemed == Some(true);
        json!([{
            "code": "MOCK-GIFT-TEST-1234",
            "duration_type": kind,
            "duration_quantity": qty,
            "created_at": at(7),
            "created_by": me,
            "redeemed_at": if redeemed { json!(at(2)) } else { Value::Null },
            "redeemed_by": if redeemed { me } else { Value::Null },
        }])
    }
}

/// `getDeveloperOptionLabel`.
fn label(key: &str) -> String {
    let l = match key {
        "bypass_loading_skeleton" => "Bypass loading skeleton",
        "force_loading_skeleton" => "Force loading skeleton",
        "force_profile_skeletons" => "Profile skeletons",
        "mock_verification_barrier" => "Verification barrier",
        "mock_barrier_time_remaining" => "Barrier countdown",
        "force_no_send_messages" => return tf("No {sendMessagesPermission} permission", &[("sendMessagesPermission", "SEND_MESSAGES")]),
        "force_no_attach_files" => return tf("No {attachFilesPermission} permission", &[("attachFilesPermission", "ATTACH_FILES")]),
        "show_myself_typing" => "Show myself typing",
        "self_hosted_mode_override" => "Self-hosted mode",
        "slow_message_load" => "Slow message load",
        "slow_message_send" => "Slow message send",
        "slow_profile_load" => "Slow profile load",
        "force_profile_data_warning" => "Profile data warning",
        "force_fail_message_sends" => "Fail message sends",
        "force_fail_message_loads" => "Fail message loads",
        "force_unknown_message_type" => "Unknown message type",
        "premium_type_override" => "Premium type",
        "premium_lifetime_sequence_override" => "Visionary badge ID",
        "premium_since_override" => "Premium since date",
        "premium_until_override" => "Premium until date",
        "premium_billing_cycle_override" => "Billing cycle",
        "premium_will_cancel_override" => "Will cancel",
        "has_ever_purchased_override" => "Has ever purchased",
        "has_unread_gift_inventory_override" => "Unread gift inventory",
        "unread_gift_inventory_count_override" => "Unread gift count",
        "email_verified_override" => "Email verified",
        "unclaimed_account_override" => "Unclaimed account",
        "mock_mature_content_gate_reason" => "Mature content channel gate",
        "mock_mature_media_gate_reason" => "Mature content media gate",
        "force_mature_media" => "Force mature media",
        "mock_in_uk" => "UK geo",
        "mock_slowmode_active" => "Slowmode active",
        "mock_slowmode_remaining" => "Slowmode remaining",
        "mock_gift_inventory" => "Gift inventory",
        "mock_gift_duration_months" => "Gift duration",
        "mock_gift_redeemed" => "Gift redeemed",
        // `humanizeDeveloperStateKey`, e.g. "No op in app reports".
        _ => return humanize(key),
    };
    t(l).to_string()
}

fn humanize(key: &str) -> String {
    let s = key.replace('_', " ");
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// `formatDurationMs`.
fn duration(ms: u64) -> String {
    if ms == 0 {
        return t("No timer").into();
    }
    let secs = (ms as f64 / 1000.0).round() as i64;
    if secs < 60 {
        return tf("{seconds, plural, one {# second} other {# seconds}}", &[("seconds", &secs.to_string())]);
    }
    let mins = (secs as f64 / 60.0).round() as i64;
    tf("{minutes, plural, one {# minute} other {# minutes}}", &[("minutes", &mins.to_string())])
}

fn premium_type_label(v: Option<u8>) -> &'static str {
    match v {
        Some(0) => "None",
        Some(1) => "Subscription",
        Some(2) => "Lifetime",
        _ => "Use actual value",
    }
}

/// `formatDeveloperOptionValue`.
fn value(key: &str, v: &Value) -> String {
    match key {
        "premium_type_override" => return t(premium_type_label(v.as_u64().map(|n| n as u8))).into(),
        "premium_lifetime_sequence_override" if !v.is_null() => return format!("#{v}"),
        "mock_slowmode_remaining" => return duration(v.as_u64().unwrap_or(0)),
        "mock_barrier_time_remaining" => return v.as_u64().map_or_else(|| t("Use default").to_string(), duration),
        "premium_since_override" | "premium_until_override" if !v.is_null() => {
            return chrono::DateTime::from_timestamp_millis(v.as_i64().unwrap_or(0))
                .map(|d| crate::desktop::time::short_date(&d.with_timezone(&chrono::Local), crate::i18n::locale()))
                .unwrap_or_default();
        }
        "unread_gift_inventory_count_override" if !v.is_null() => return tp("{count, plural, one {# gift} other {# gifts}}", v.as_i64().unwrap_or(0), &[]),
        "mock_gift_duration_months" => {
            let m = v.as_i64().unwrap_or(0);
            return if m == 0 { t("Lifetime").into() } else { tf("{months, plural, one {# month} other {# months}}", &[("months", &m.to_string())]) };
        }
        _ => {}
    }
    match v {
        Value::Bool(true) => t("Enabled").into(),
        Value::Bool(false) => t("Forced off").into(),
        Value::Null => t("Use actual value").into(),
        Value::String(s) if s.trim().is_empty() => t("Custom value").into(),
        Value::String(s) => s.replace('_', " "),
        other => other.to_string(),
    }
}

/// `MessageCommands` / `MessageQueue` / profile developer hooks, before the request: fails with
/// `fail` when forced, else waits 3 s (`DEV_MESSAGE_DELAY`, `PROFILE_LOAD_DELAY_MS`) when slow.
pub async fn gate(slow: bool, fail: Option<&'static str>) -> reflux_net::Result<()> {
    if let Some(e) = fail {
        return Err(reflux_net::Error::Upload(e.into()));
    }
    if slow {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
    Ok(())
}

/// [`gate`]'s arguments for message loads.
pub fn load_gate(o: &Options) -> (bool, Option<&'static str>) {
    (o.slow_message_load, o.force_fail_message_loads.then_some("Forced message load failure"))
}

/// The current user under the developer overrides (`Users.currentUser`).
pub fn me<'a>(store: &'a Store, st: &ShellState) -> Option<Cow<'a, User>> {
    let me = store.me.as_ref()?;
    let o = &st.prefs.dev;
    Some(if o.touches_user() { Cow::Owned(o.apply(me)) } else { Cow::Borrowed(me) })
}

/// `DeveloperMode.isDeveloper`: staff, or the account's developer mode (Reflux has no build-tap
/// unlock; the synced `developer_mode` setting stands in for it).
pub fn visible(store: &Store) -> bool {
    store.settings.developer_mode || store.me.as_ref().is_some_and(|m| m.is_staff)
}

/// `RollingTypingStore`'s typers, with me among them under "Show myself typing".
pub fn typing(store: &Store, st: &ShellState, channel: Snowflake) -> Vec<Snowflake> {
    if !st.prefs.dev.show_myself_typing {
        return store.typing_users(channel, st.now_ms);
    }
    store.typing.get(&channel).into_iter().flatten().filter(|&&(_, exp)| exp > st.now_ms).map(|&(u, _)| u).collect()
}

/// `forceUnknownMessageType`: my messages draw as `UnknownMessage`.
pub fn unknown(store: &Store, st: &ShellState, m: &reflux_model::Message) -> bool {
    st.prefs.dev.force_unknown_message_type && m.author.id == store.my_id()
}

/// `UnknownMessage`'s `.unknownMessageWarning`: a filled warning circle and "Update Fluxer to
/// view this message." in `--message-unknown-warning-color`, gap 4.
pub fn unknown_warning(p: &Palette) -> El {
    let c = p.message_unknown_warning_color;
    row()
        .gap(4.0)
        .center_y()
        .child(icon(&icons::WARNING_CIRCLE_FILL, 16.0).color(c))
        .child(text(tf("Update {productName} to view this message.", &[("productName", "Fluxer")])).font(font(16.0, 400.0).lh(dim::MESSAGE_LINE_HEIGHT)).color(c).wrap())
}

// ---- Nagbars ------------------------------------------------------------------------------------

/// `getNagbarControls`, the bars Reflux has: (key, label, "using actual" status, the
/// `desktop::Prefs::dismissed_nagbars` entry its "Use actual" clears).
const NAGBARS: &[(&str, &str, &str, Option<&str>)] = &[
    ("forceUnclaimedAccount", "Unclaimed account nagbar", "Using actual account state", None),
    ("forceEmailVerification", "Email verification nagbar", "Using actual verification state", None),
    ("forceAccountLimited", "Messaging paused", "Using actual account state", None),
    ("forceDesktopNotification", "Desktop notification nagbar", "Currently showing", Some("desktop-notification")),
    ("forcePremiumGracePeriod", "Premium grace period nagbar", "Using actual premium state", None),
    ("forcePremiumExpired", "Premium expired nagbar", "Using actual premium state", None),
    ("forcePremiumOnboarding", "Premium onboarding nagbar", "Using actual premium state", Some("premium-onboarding")),
    ("forceGiftInventory", "Gift inventory nagbar", "Using actual gift inventory state", None),
    ("forceVisionaryMfa", "Visionary 2FA nagbar", "Using actual state", Some("visionary-mfa")),
    ("forceTermsAcceptance", "Terms acceptance nagbar", "Using actual state", None),
    ("forceGuildMembershipCta", "Community membership CTA nagbar", "Using actual state", Some("guild-membership-cta")),
    ("forceScheduledMaintenance", "Scheduled maintenance nagbar", "Using actual state", None),
    ("forceIncident", "Service incident nagbar", "Using actual state", None),
    ("forceVoiceSessionRestore", "Voice session restore nagbar", "Using actual state", None),
    ("forceInvitesDisabled", "Invites disabled nagbar", "Using actual state", None),
    ("forceGuildMfaRequirement", "Community MFA requirement nagbar", "Using actual state", None),
];

/// A nagbar's override: `Some(true)` force shown, `Some(false)` force hidden.
pub fn nagbar(st: &ShellState, key: &str) -> Option<bool> {
    st.prefs.dev.nagbars.get(key).copied()
}

fn dismissed(st: &ShellState, key: Option<&str>) -> bool {
    key.is_some_and(|k| st.prefs.dismissed_nagbars.iter().any(|d| d == k))
}

/// `createNagbarControl().status`.
fn nagbar_status(st: &ShellState, n: &(&str, &str, &str, Option<&str>)) -> &'static str {
    match nagbar(st, n.0) {
        Some(true) => "Force enabled",
        Some(false) => "Force disabled",
        None if dismissed(st, n.3) => "Currently dismissed",
        None => match n.2 {
            "Using actual verification state" => "Using actual verification state",
            "Using actual premium state" => "Using actual premium state",
            "Using actual gift inventory state" => "Using actual gift inventory state",
            "Using actual account state" => "Using actual account state",
            "Currently showing" => "Currently showing",
            _ => "Using actual state",
        },
    }
}

/// `setNagbarUseActual`.
fn use_actual(o: &mut Options, dismissed: &mut Vec<String>, key: &str) {
    o.nagbars.remove(key);
    if let Some(d) = NAGBARS.iter().find(|n| n.0 == key).and_then(|n| n.3) {
        dismissed.retain(|x| x != d);
    }
}

// ---- State --------------------------------------------------------------------------------------

/// Confirmations and mocks the menu opens.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ui {
    /// `openDangerConfirmation`.
    pub confirm: Option<Danger>,
    /// `MockIncomingCall`: `true` = the group DM variant.
    pub mock_call: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Danger {
    ForgetAuthorizedIps,
    ClearTrustedDomains,
    ResetPremiumState,
}

/// Changes the options (and the prefs that hold them), then saves.
fn edit<S: 'static>(acc: ShellAccess<S>, f: impl Fn(&mut Options, &mut Vec<String>) + 'static) -> impl Fn(&mut S) + 'static {
    move |s: &mut S| {
        let sh = acc(s);
        f(&mut sh.prefs.dev, &mut sh.prefs.dismissed_nagbars);
        sh.intents.push(Intent::Prefs(Box::new(sh.prefs.clone())));
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// ---- Menu ---------------------------------------------------------------------------------------

fn check<S: 'static>(id: &str, label: &str, on: bool, acc: ShellAccess<S>, f: impl Fn(&mut Options, bool) + 'static) -> MenuEntry<S> {
    MenuEntry::Item(MenuItem::new(id.to_string(), t(label).to_string()).checkbox(on).keep_open().on_select(edit(acc, move |o, _| f(o, !on))))
}

fn check_label<S: 'static>(id: &str, label: String, on: bool, acc: ShellAccess<S>, f: impl Fn(&mut Options, bool) + 'static) -> MenuEntry<S> {
    MenuEntry::Item(MenuItem::new(id.to_string(), label).checkbox(on).keep_open().on_select(edit(acc, move |o, _| f(o, !on))))
}

fn radio<S: 'static>(id: String, label: String, on: bool, acc: ShellAccess<S>, f: impl Fn(&mut Options) + 'static) -> MenuEntry<S> {
    MenuEntry::Item(MenuItem::new(id, label).radio(on).keep_open().on_select(edit(acc, move |o, _| f(o))))
}

fn sub<S: 'static>(id: &str, label: &str, entries: Vec<MenuEntry<S>>) -> MenuEntry<S> {
    MenuEntry::Submenu(id.to_string(), t(label).to_string(), entries)
}

/// `DeveloperStateSummaryMenu`.
fn summary<S: 'static>(me: &User, st: &ShellState, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let o = &st.prefs.dev;
    let entries = o.active();
    let bars: Vec<_> = NAGBARS.iter().filter(|n| nagbar(st, n.0).is_some() || dismissed(st, n.3)).collect();
    let (backend, perks) = (me.premium_enabled_override, me.premium_perks_disabled);
    let count = entries.len() + bars.len() + usize::from(backend) + usize::from(perks);
    let hint = if count == 0 { t("No active overrides").to_string() } else { tp("{count, plural, one {# active override} other {# active overrides}}", count as i64, &[]) };
    let mut review = Vec::new();
    if !entries.is_empty() {
        review.push(MenuEntry::Separator);
        for (k, l, v) in entries {
            review.push(MenuEntry::Item(MenuItem::new(format!("active-{k}"), l).hint(v).icon(&icons::TRASH_BOLD).keep_open().on_select(edit(acc, move |o, _| o.reset(&k)))));
        }
    }
    if !bars.is_empty() {
        review.push(MenuEntry::Separator);
        for n in bars {
            let key = n.0;
            review.push(MenuEntry::Item(MenuItem::new(format!("active-{key}"), t(n.1).to_string()).hint(t(nagbar_status(st, n))).icon(&icons::BELL_RINGING_FILL).keep_open().on_select(edit(acc, move |o, d| use_actual(o, d, key)))));
        }
    }
    if backend {
        review.push(MenuEntry::Separator);
        review.push(MenuEntry::Item(MenuItem::new("active-backend-premium", t("Backend premium override")).hint(t("Backend premium override enabled")).icon(&icons::ATOM_FILL).keep_open().on_select(move |s: &mut S| backend_premium(acc(s), false))));
    }
    if perks {
        review.push(MenuEntry::Separator);
        review.push(MenuEntry::Item(MenuItem::new("active-premium-perks", t("Backend premium perks disabled")).hint(t("Premium perks disabled")).icon(&icons::ATOM_FILL).keep_open().on_select(move |s: &mut S| premium_perks(acc(s), false))));
    }
    vec![
        MenuEntry::Item(MenuItem::new("state", t("Developer state")).icon(&icons::WARNING_CIRCLE_FILL).hint(hint.clone()).disabled(true)),
        MenuEntry::SubmenuHint { id: "review".into(), label: t("Review active overrides").into(), hint: Some(hint), disabled: count == 0, entries: review },
        MenuEntry::Item(MenuItem::new("clear-all", t("Clear all active overrides")).icon(&icons::TRASH_BOLD).danger().disabled(count == 0).keep_open().on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.prefs.dev.reset_all();
            sh.prefs.dev.nagbars.clear();
            sh.intents.push(Intent::Prefs(Box::new(sh.prefs.clone())));
            if backend {
                backend_premium(sh, false);
            }
            if perks {
                premium_perks(sh, false);
            }
        })),
    ]
}

/// `UserCommands.update({premium_enabled_override})`.
fn backend_premium(sh: &mut ShellState, on: bool) {
    sh.api(Method::PATCH, "/users/@me", Some(json!({ "premium_enabled_override": on })), Then::Nothing);
}

/// `PremiumCommands.setPremiumPerksDisabled`.
fn premium_perks(sh: &mut ShellState, disabled: bool) {
    // shortcut: a failure is a toast, not Fluxer's "Failed to update premium perk state." modal; add the modal if staff want it.
    sh.api(Method::PATCH, "/premium/perks-disabled", Some(json!({ "disabled": disabled })), Then::Toast("", "Failed to update premium perk state."));
}

/// `GeneralDeveloperOptionsMenu`: the toggle groups Reflux has a counterpart for, then the
/// staff-only mention controls.
fn general<S: 'static>(store: &Store, me: &User, o: &Options, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let mut out = vec![
        sub("app-state", "App state", vec![
            check("bypass-skeleton", "Bypass loading skeleton", o.bypass_loading_skeleton, acc, |o, v| o.bypass_loading_skeleton = v),
            check("force-skeleton", "Force loading skeleton", o.force_loading_skeleton, acc, |o, v| o.force_loading_skeleton = v),
            check("show-myself-typing", "Show myself typing", o.show_myself_typing, acc, |o, v| o.show_myself_typing = v),
            check("self-hosted", "Self-hosted mode override", o.self_hosted_mode_override, acc, |o, v| o.self_hosted_mode_override = v),
        ]),
        sub("networking", "Networking and performance", vec![
            check("slow-load", "Slow message load", o.slow_message_load, acc, |o, v| o.slow_message_load = v),
            check("slow-send", "Slow message send", o.slow_message_send, acc, |o, v| o.slow_message_send = v),
            check("slow-profile", "Slow profile load", o.slow_profile_load, acc, |o, v| o.slow_profile_load = v),
            check("profile-skeletons", "Force profile skeletons", o.force_profile_skeletons, acc, |o, v| o.force_profile_skeletons = v),
            check("profile-warning", "Force profile data warning", o.force_profile_data_warning, acc, |o, v| o.force_profile_data_warning = v),
            check("fail-sends", "Force failed message sends", o.force_fail_message_sends, acc, |o, v| o.force_fail_message_sends = v),
            check("fail-loads", "Force failed message loads", o.force_fail_message_loads, acc, |o, v| o.force_fail_message_loads = v),
        ]),
        sub("features", "Features", vec![
            check("unknown-type", "Force unknown message type", o.force_unknown_message_type, acc, |o, v| o.force_unknown_message_type = v),
            check("noop-reports", "No-op in-app reports", o.no_op_in_app_reports, acc, |o, v| o.no_op_in_app_reports = v),
        ]),
    ];
    if me.is_staff {
        let on = store.settings.suppress_unprivileged_self_mentions;
        out.push(sub("mentions", "Mention controls", vec![MenuEntry::Item(
            MenuItem::new("suppress-self-mentions", t("Suppress unprivileged self-mentions"))
                .checkbox(on)
                .keep_open()
                .on_select(move |s: &mut S| acc(s).intents.push(Intent::PatchSettings(json!({ "suppress_unprivileged_self_mentions": !on })))),
        )]));
    }
    out
}

/// `AccountPremiumMenu`.
fn account_premium<S: 'static>(me: &User, o: &Options, p: &Palette, acc: ShellAccess<S>) -> MenuEntry<S> {
    let mut account = vec![
        check("email-verified", "Email verified override", o.email_verified_override == Some(true), acc, |o, v| o.email_verified_override = v.then_some(true)),
        check("unclaimed", "Unclaimed account override", o.unclaimed_account_override == Some(true), acc, |o, v| o.unclaimed_account_override = v.then_some(true)),
        check("unread-gifts", "Unread gift inventory override", o.has_unread_gift_inventory_override == Some(true), acc, |o, v| {
            o.has_unread_gift_inventory_override = v.then_some(true);
            if !v {
                o.unread_gift_inventory_count_override = None;
            }
        }),
    ];
    if o.has_unread_gift_inventory_override == Some(true) {
        let n = o.unread_gift_inventory_count_override.unwrap_or(1);
        account.push(menu_slider("dev-gift-count".into(), t("Unread gift count"), n as f32, 0.0, 99.0, tp("{count, plural, one {# gift} other {# gifts}}", n as i64, &[]), p, move |s: &mut S, v| {
            edit(acc, move |o, _| o.unread_gift_inventory_count_override = Some(v.round() as u32))(s)
        }));
    }
    let mut kind: Vec<MenuEntry<S>> = [(None, "Use actual premium type"), (Some(0u8), "None"), (Some(1), "Subscription"), (Some(2), "Lifetime")]
        .into_iter()
        .map(|(v, l)| radio(format!("premium-type-{v:?}"), t(l).into(), o.premium_type_override == v, acc, move |o| {
            o.premium_type_override = v;
            o.touch_premium();
        }))
        .collect();
    // `user.isClaimed()`.
    if me.account_tier.as_deref() != Some("unclaimed") {
        let (backend, perks) = (me.premium_enabled_override, me.premium_perks_disabled);
        kind.push(MenuEntry::Item(MenuItem::new("backend-premium", t("Backend premium override")).checkbox(backend).keep_open().on_select(move |s: &mut S| backend_premium(acc(s), !backend))));
        kind.push(MenuEntry::Item(MenuItem::new("premium-perks", t("Disable backend premium perks")).checkbox(perks).keep_open().on_select(move |s: &mut S| premium_perks(acc(s), !perks))));
    }
    kind.push(check("ever-purchased", "Has ever purchased override", o.has_ever_purchased_override == Some(true), acc, |o, v| {
        o.has_ever_purchased_override = v.then_some(true);
        o.touch_premium();
    }));
    let seq = o.premium_lifetime_sequence_override;
    let visionary = vec![
        radio("visionary-actual".into(), t("Use actual value").into(), seq.is_none(), acc, |o| o.premium_lifetime_sequence_override = None),
        menu_slider("dev-visionary-id".into(), t("Visionary ID number"), seq.unwrap_or(42) as f32, 1.0, 999.0, format!("#{}", seq.unwrap_or(42)), p, move |s: &mut S, v| {
            edit(acc, move |o, _| o.premium_lifetime_sequence_override = Some(v.round() as i64))(s)
        }),
    ];
    let mut scenarios: Vec<MenuEntry<S>> = SCENARIOS
        .iter()
        .map(|(v, l, _)| {
            let on = o.premium_scenario_override.as_deref() == Some(*v);
            let label = if *v == "visionary" { tf(l, &[("visionaryLifetimeBadgeLabel", "Visionary")]) } else { t(l).to_string() };
            MenuEntry::Item(MenuItem::new(format!("scenario-{v}"), label).checkbox(on).keep_open().on_select(edit(acc, move |o, _| if on { o.reset_premium() } else { o.apply_scenario(v, now_ms()) })))
        })
        .collect();
    scenarios.push(MenuEntry::Item(MenuItem::new("scenario-reset", t("Reset to actual values")).keep_open().on_select(edit(acc, |o, _| o.reset_premium()))));
    sub("account-premium", "Account & premium overrides", vec![
        sub("account-state", "Account state", account),
        sub("premium-type", "Premium type", kind),
        sub("visionary", "Visionary badge", visionary),
        sub("scenarios", "Subscription scenarios", scenarios),
    ])
}

/// `MockingMenu`.
fn mocking<S: 'static>(o: &Options, p: &Palette, acc: ShellAccess<S>) -> MenuEntry<S> {
    let call = |id: &str, label: &str, i: &'static Icon, group: bool| {
        MenuEntry::Item(MenuItem::new(id.to_string(), t(label).to_string()).icon(i).on_select(move |s: &mut S| acc(s).devtools.mock_call = Some(group)))
    };
    let mut slow = vec![check("slowmode", "Force slowmode active", o.mock_slowmode_active, acc, |o, v| o.mock_slowmode_active = v)];
    if o.mock_slowmode_active {
        let ms = o.mock_slowmode_remaining;
        let secs = ms / 1000;
        slow.push(menu_slider("dev-slowmode".into(), t("Slowmode time remaining"), ms as f32, 0.0, 60_000.0, tp("{count, plural, one {# second} other {# seconds}}", secs as i64, &[]), p, move |s: &mut S, v| {
            edit(acc, move |o, _| o.mock_slowmode_remaining = ((v / 1000.0).round() * 1000.0) as u64)(s)
        }));
    }
    let gate_radios = |key: &'static str, cur: &str, opts: &'static [(&'static str, &'static str)], f: fn(&mut Options, &'static str)| -> Vec<MenuEntry<S>> {
        opts.iter().map(|(v, l)| radio(format!("{key}-{v}"), t(l).into(), cur == *v, acc, move |o| f(o, v))).collect()
    };
    const CHANNEL_OPTS: &[(&str, &str)] = &[("none", "None (normal behavior)"), ("geo_restricted", "Geo restricted"), ("mature_content_check_required", "Mature content"), ("consent_required", "Consent required")];
    let channel_opts = CHANNEL_OPTS;
    let safety = vec![
        sub("channel-gate", "Mock mature content channel gate reason", gate_radios("channel-gate", &o.mock_mature_content_gate_reason, channel_opts, |o, v| o.mock_mature_content_gate_reason = v.into())),
        sub("media-gate", "Mock mature content media gate reason", gate_radios("media-gate", &o.mock_mature_media_gate_reason, &CHANNEL_OPTS[..3], |o, v| o.mock_mature_media_gate_reason = v.into())),
        check("force-mature", "Force mature media", o.force_mature_media, acc, |o, v| o.force_mature_media = v),
        check("mock-uk", "Mock UK geo", o.mock_in_uk, acc, |o, v| o.mock_in_uk = v),
    ];
    let mut gifts = vec![check("mock-gifts", "Mock gift inventory", o.mock_gift_inventory == Some(true), acc, |o, v| {
        o.mock_gift_inventory = v.then_some(true);
        if !v {
            (o.mock_gift_duration_months, o.mock_gift_redeemed) = (12, None);
        }
    })];
    if o.mock_gift_inventory == Some(true) {
        let cur = o.mock_gift_duration_months;
        let durations = [(1u32, "1 month"), (3, "3 months"), (6, "6 months"), (12, "12 months (1 year)"), (0, "Lifetime")];
        gifts.push(sub("gift-duration", "Gift duration", durations.into_iter().map(|(v, l)| radio(format!("gift-months-{v}"), t(l).into(), cur == v, acc, move |o| o.mock_gift_duration_months = v)).collect()));
        gifts.push(check("gift-redeemed", "Mark as redeemed", o.mock_gift_redeemed == Some(true), acc, |o, v| o.mock_gift_redeemed = v.then_some(true)));
    }
    const BARRIERS: &[(&str, &str)] = &[
        ("none", "None (normal behavior)"),
        ("unclaimed_account", "Unclaimed account"),
        ("unverified_email", "Unverified email"),
        ("account_too_new", "Account too new"),
        ("not_member_long", "Not a member long enough"),
        ("account_limited", "Messaging paused"),
        ("send_message_disabled", "Send messages disabled"),
    ];
    let mut barriers = gate_radios("barrier", &o.mock_verification_barrier, BARRIERS, |o, v| o.mock_verification_barrier = v.into());
    if matches!(o.mock_verification_barrier.as_str(), "account_too_new" | "not_member_long") {
        // `getCountdownTimerOptions`.
        let cur = o.mock_barrier_time_remaining.unwrap_or(300_000);
        let timers = [0u64, 10_000, 30_000, 60_000, 120_000, 300_000, 600_000]
            .into_iter()
            .map(|ms| radio(format!("barrier-timer-{ms}"), duration(ms), cur == ms, acc, move |o| o.mock_barrier_time_remaining = Some(ms)))
            .collect();
        barriers.push(sub("barrier-timer", "Countdown timer", timers));
    }
    let perms = vec![
        check_label("no-send", tf("Force no {permissionName} permission", &[("permissionName", "SEND_MESSAGES")]), o.force_no_send_messages, acc, |o, v| o.force_no_send_messages = v),
        check_label("no-attach", tf("Force no {permissionName} permission", &[("permissionName", "ATTACH_FILES")]), o.force_no_attach_files, acc, |o, v| o.force_no_attach_files = v),
    ];
    sub("mocking", "Mocked product flows", vec![
        sub("barriers", "Verification & membership barriers", barriers),
        sub("channel-perms", "Channel permission overrides", perms),
        sub("incoming-call", "Mock incoming call", vec![call("call-dm", "Direct message", &icons::PHONE_FILL, false), call("call-group", "Group DM", &icons::USERS_FILL, true)]),
        sub("slowmode", "Slowmode simulation", slow),
        sub("safety", "Content safety gates", safety),
        sub("gift-flow", "Gift inventory flow", gifts),
    ])
}

/// `NagbarsMenu`.
fn nagbars<S: 'static>(st: &ShellState, acc: ShellAccess<S>) -> MenuEntry<S> {
    let mut out: Vec<MenuEntry<S>> = NAGBARS
        .iter()
        .map(|n| {
            let key = n.0;
            let force = nagbar(st, key);
            let actual_off = force.is_none() && !dismissed(st, n.3);
            let item = |id: &str, label: &str, disabled: bool, f: fn(&mut Options, &mut Vec<String>, &'static str)| {
                MenuEntry::Item(MenuItem::new(format!("{key}-{id}"), t(label).to_string()).disabled(disabled).keep_open().on_select(edit(acc, move |o, d| f(o, d, key))))
            };
            MenuEntry::SubmenuHint {
                id: key.into(),
                label: t(n.1).into(),
                hint: Some(t(nagbar_status(st, n)).into()),
                disabled: false,
                entries: vec![
                    item("use-actual", "Use actual", actual_off, use_actual),
                    item("force-show", "Force show", force == Some(true), |o, _, k| { o.nagbars.insert(k.into(), true); }),
                    item("force-hide", "Force hide", force == Some(false), |o, _, k| { o.nagbars.insert(k.into(), false); }),
                ],
            }
        })
        .collect();
    out.push(MenuEntry::Separator);
    out.push(MenuEntry::Item(MenuItem::new("reset-nagbars", t("Reset all nagbars")).icon(&icons::TRASH_BOLD).keep_open().on_select(edit(acc, |o, d| {
        for n in NAGBARS {
            use_actual(o, d, n.0);
        }
    }))));
    sub("nagbars", "Nagbar overrides", out)
}

/// `ToolsMenu` ("Runtime utilities").
fn tools<S: 'static>(store: &Store, st: &ShellState, acc: ShellAccess<S>) -> MenuEntry<S> {
    let trusted = &st.media_ui.trusted;
    let all = trusted.iter().any(|d| d == "*");
    let trusted_label = if all {
        t("Clear trust all setting").to_string()
    } else if trusted.is_empty() {
        t("No trusted domains").to_string()
    } else {
        tf("Clear trusted domains ({trustedDomainsCount})", &[("trustedDomainsCount", &trusted.len().to_string())])
    };
    let confirm = |d: Danger| move |s: &mut S| acc(s).devtools.confirm = Some(d);
    let afk = st.voice.channel.and_then(|(g, c)| {
        let g = g?;
        let afk = store.guilds.get(&g)?.info.as_ref()?.afk_channel_id?;
        (afk != c).then_some((g, afk))
    });
    let mfa = store.me.as_ref().is_some_and(|m| m.mfa_enabled);
    sub("tools", "Runtime utilities", vec![
        MenuEntry::Item(MenuItem::new("reset-socket", t("Reset socket")).icon(&icons::PLUG_BOLD).on_select(move |s: &mut S| acc(s).intents.push(Intent::DevSocket { reset: true }))),
        MenuEntry::Item(MenuItem::new("disconnect-socket", t("Disconnect socket")).icon(&icons::PLUG_BOLD).on_select(move |s: &mut S| acc(s).intents.push(Intent::DevSocket { reset: false }))),
        // `MediaEngine.moveToAfkChannel`: only from a guild voice channel that isn't the AFK one.
        MenuEntry::Item(MenuItem::new("afk", t("Force move to AFK channel")).icon(&icons::PHONE_FILL).on_select(move |s: &mut S| {
            if let Some((g, c)) = afk {
                acc(s).intents.push(Intent::JoinVoice { guild: Some(g), channel: c });
            }
        })),
        MenuEntry::Item(MenuItem::new("new-device", t("Show new device modal")).on_select(move |s: &mut S| {
            // `NewDeviceMonitoring.showTestModal`.
            acc(s).voice.modal = Some(super::voice::Modal::NewDevice(super::voice::NewDevice { name: "Test Audio Device".into(), input: Some("test-device-id".into()), output: None }));
        })),
        MenuEntry::Item(MenuItem::new("keyboard-intro", t("Show keyboard mode intro")).icon(&icons::KEYBOARD_FILL).on_select(move |s: &mut S| acc(s).nav.keyboard_intro = true)),
        MenuEntry::Item(MenuItem::new("claim-account", t("Open claim account modal")).on_select(move |s: &mut S| acc(s).account_gate.open = Some(None))),
        MenuEntry::Item(MenuItem::new("sudo", t("Open sudo verification modal")).on_select(move |s: &mut S| {
            let sh = acc(s);
            if sh.account.sudo.is_none() {
                let call = crate::guild::Call { method: Method::POST, path: SUDO_PATH.into(), body: None, then: Then::Nothing };
                crate::settings::sudo::require(sh, call, &json!({ "has_mfa": mfa, "methods": { "totp": mfa, "password": !mfa } }));
            }
        })),
        MenuEntry::Item(MenuItem::new("system-dm", t("Open system DM")).icon(&icons::ROBOT_FILL).on_select(move |s: &mut S| acc(s).intents.push(Intent::OpenDm(super::client_system::FLUXERBOT)))),
        MenuEntry::Item(MenuItem::new("forget-ips", t("Forget authorized IPs")).danger().on_select(confirm(Danger::ForgetAuthorizedIps))),
        MenuEntry::Item(MenuItem::new("clear-trusted", trusted_label).danger().disabled(trusted.is_empty()).on_select(confirm(Danger::ClearTrustedDomains))),
        MenuEntry::Item(MenuItem::new("reset-premium", t("Reset premium state")).danger().on_select(confirm(Danger::ResetPremiumState))),
        MenuEntry::Item(MenuItem::new("trigger-crash", t("Trigger React crash")).icon(&icons::BUG_FILL).danger().on_select(|_: &mut S| crate::crash::show("Error: Triggered React crash from Developer Tools menu".into()))),
    ])
}

/// The header button (`StaffToolsButton`) and, while open, its menu.
pub fn button<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> Option<El> {
    if !visible(store) {
        return None;
    }
    let open = st.popups.is_open(MENU);
    let b = row()
        .size(dim::CHANNEL_HEADER_ACTION_SIZE, dim::CHANNEL_HEADER_ACTION_SIZE)
        .center()
        .radius(dim::RADIUS_FULL)
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .focusable(true)
        .focus_ring(4.0, 0.0, p.focus_primary)
        .label(t("Developer tools"))
        .expanded(open)
        .key(MENU)
        .child(icon(&STAFF_ICON, dim::CHANNEL_HEADER_ACTION_ICON_SIZE).color(if open { p.text_primary } else { p.text_primary_muted }).group_color(p.text_primary))
        .on_press(move |s: &mut S| acc(s).popups.toggle(MENU))
        .on_secondary_press(move |s: &mut S, _, _| acc(s).popups.toggle(MENU));
    if open {
        menu(store, st, p, acc).el(&st.popups, crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups), p, layer);
    }
    Some(b)
}

/// `DeveloperToolsContextMenu`.
fn menu<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Menu<S> {
    let m = Menu::new(MENU).placement(Placement::BottomEnd);
    let Some(me) = me(store, st) else {
        return m.item(MenuItem::new("unavailable", t("Developer tools unavailable")).disabled(true));
    };
    // The backend flags are the server's, not the overridden view's.
    let real = store.me.as_ref().unwrap_or(&me);
    let o = &st.prefs.dev;
    let mut m = m;
    for e in summary(real, st, acc) {
        m = m.entry(e);
    }
    m = m.separator();
    for e in general(store, real, o, acc) {
        m = m.entry(e);
    }
    m.entry(account_premium(&me, o, p, acc)).entry(mocking(o, p, acc)).separator().entry(nagbars(st, acc)).entry(tools(store, st, acc))
}

/// `openDangerConfirmation`'s `ConfirmModal`.
pub fn modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let d = st.devtools.confirm?;
    let (title, body) = match d {
        Danger::ForgetAuthorizedIps => ("Forget authorized IPs", "This removes all previously authorized IP addresses on your account. The next login from any IP will require email verification."),
        Danger::ClearTrustedDomains => ("Clear trusted domains", "This removes all trusted domains. You will see the external link warning for all domains again."),
        Danger::ResetPremiumState => ("Reset premium state", "This clears your backend premium state entirely, including premium type, billing metadata, and override flags."),
    };
    let close = move |s: &mut S| acc(s).devtools.confirm = None;
    let go = move |s: &mut S| {
        let sh = acc(s);
        sh.devtools.confirm = None;
        // shortcut: no pending label ("Forgetting authorized IPs...") and failures are toasts, not error modals; track the request if staff need them.
        match d {
            Danger::ForgetAuthorizedIps => sh.api(Method::DELETE, "/users/@me/authorized-ips", Some(json!({})), Then::Toast("Authorized IPs cleared. New logins will require email verification.", "Failed to forget authorized IPs. Try again.")),
            Danger::ClearTrustedDomains => {
                // `TrustedDomain.clearAllTrustedDomains`: the synced `trusted_domains` setting.
                sh.media_ui.trusted.clear();
                sh.intents.push(Intent::PatchSettings(json!({ "trusted_domains": [] })));
                crate::desktop::toast(&mut sh.desktop, t("Trusted domains cleared."), true);
            }
            Danger::ResetPremiumState => {
                sh.prefs.dev.reset_premium();
                sh.intents.push(Intent::Prefs(Box::new(sh.prefs.clone())));
                sh.api(Method::POST, "/users/@me/premium/reset", None, Then::Toast("Premium state reset on the backend.", "Failed to reset premium state."));
            }
        }
    };
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
        .child(Button::new(t(title)).variant(ButtonVariant::Danger).key("dev-confirm").on_press(go).el(p));
    Some(Modal::new(t(title), close).body(text(t(body)).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap()).footer(footer).el(p))
}

/// `MockIncomingCall`'s caller: me in a DM, "Riley Park" in a group DM of four.
pub fn mock_call(store: &Store, st: &ShellState) -> Option<(Snowflake, String)> {
    let group = st.devtools.mock_call?;
    Some(if group {
        (Snowflake(1_100_000_000_000_000_001), "Riley Park, Jordan Lee, Sasha Mori".into())
    } else {
        let me = store.my_id();
        (me, store.display_name(None, me))
    })
}

/// The `/users/@me/gifts` answer while "Mock gift inventory" is on.
pub fn gifts(sh: &ShellState) -> Option<Value> {
    let o = &sh.prefs.dev;
    (o.mock_gift_inventory == Some(true)).then(|| {
        let me = sh.account.current.as_ref().map(|a| json!({ "id": a.user_id, "username": a.username, "global_name": a.display_name, "discriminator": "0" })).unwrap_or(Value::Null);
        o.mock_gifts(me, now_ms())
    })
}

/// `--shell-demo --dev=menu|review|nagbars|unknown|call|confirm`.
pub fn demo(sh: &mut ShellState, store: &mut Store, scene: &str) {
    store.settings.developer_mode = true;
    match scene {
        "menu" => {
            sh.popups.toggle(MENU);
            sh.popups.sub = Some("account-premium/premium-type".into());
            sh.prefs.dev.premium_type_override = Some(2);
        }
        "review" => {
            sh.popups.toggle(MENU);
            sh.popups.sub = Some("review".into());
            sh.prefs.dev.show_myself_typing = true;
            sh.prefs.dev.mock_slowmode_active = true;
            sh.prefs.dev.apply_scenario("active_monthly", now_ms());
            sh.prefs.dev.nagbars.insert("forceScheduledMaintenance".into(), true);
            sh.prefs.dev.nagbars.insert("forceEmailVerification".into(), true);
            sh.prefs.dev.nagbars.insert("forcePremiumOnboarding".into(), true);
        }
        "nagbars" => {
            sh.popups.toggle(MENU);
            sh.popups.sub = Some("nagbars/forceGiftInventory".into());
            for k in ["forceEmailVerification", "forceScheduledMaintenance", "forceIncident", "forcePremiumGracePeriod", "forceGiftInventory"] {
                sh.prefs.dev.nagbars.insert(k.into(), true);
            }
        }
        "unknown" => {
            sh.prefs.dev.force_unknown_message_type = true;
            sh.prefs.dev.mock_slowmode_active = true;
            // It only reshapes my own messages: make the last one in each channel mine.
            let me = store.me.clone().unwrap_or_default();
            for w in store.messages.values_mut() {
                if let Some(m) = w.messages.last_mut() {
                    (m.author.id, m.author.username, m.author.global_name, m.author.avatar) = (me.id, me.username.clone(), me.global_name.clone(), me.avatar.clone());
                }
            }
        }
        "call" => sh.devtools.mock_call = Some(true),
        "confirm" => sh.devtools.confirm = Some(Danger::ForgetAuthorizedIps),
        "claim" => super::account_gate::open_claim(store, sh, None),
        "claim-username" => {
            sh.tiers.username = true;
            super::account_gate::open_claim(store, sh, None);
        }
        "claim-code" => {
            super::account_gate::open_claim(store, sh, None);
            if let Some(c) = sh.account_gate.claim.as_mut() {
                (c.verify, c.email, c.resend_at) = (true, rune::TextEdit::new("ana@example.com"), sh.now_ms + 24_000);
            }
        }
        "help" => sh.account_gate.help = Some(rune::TextEdit::new("South Korea")),
        "notifications-confirm" => sh.account_gate.bar_confirm = Some(crate::desktop::top_bars::Confirm::DesktopNotifications),
        "invites-confirm" => sh.account_gate.bar_confirm = sh.guild.map(crate::desktop::top_bars::Confirm::EnableInvites),
        "skeleton" => sh.prefs.dev.force_loading_skeleton = true,
        "profile-skeleton" => {
            sh.prefs.dev.force_profile_skeletons = true;
            sh.open_profile(Snowflake(1001), sh.guild, String::new(), "member-1001".into(), crate::ui::Placement::LeftStart);
        }
        "messages-skeleton" => drop(sh.channel.and_then(|c| store.messages.remove(&c))),
        "no-send" => sh.prefs.dev.force_no_send_messages = true,
        "no-attach" => {
            sh.prefs.dev.force_no_attach_files = true;
            sh.popups.toggle("composer-plus");
        }
        "media-details-geo" => sh.media_ui.mature_details = Some(super::attachments::Gate::Geo),
        "media-details-check" => sh.media_ui.mature_details = Some(super::attachments::Gate::Check),
        // `--dev=media-gate:geo_restricted`, `--dev=barrier:account_too_new`,
        // `--dev=force:forceUnclaimedAccount,forceAccountLimited`.
        s if s.starts_with("media-gate:") => sh.prefs.dev.mock_mature_media_gate_reason = s["media-gate:".len()..].into(),
        s if s.starts_with("barrier:") => {
            sh.prefs.dev.mock_verification_barrier = s["barrier:".len()..].into();
            (sh.account_gate.mock_key, sh.account_gate.mock_since) = (sh.prefs.dev.mock_verification_barrier.clone(), sh.now_ms);
        }
        s if s.starts_with("force:") => {
            for k in s["force:".len()..].split(',') {
                sh.prefs.dev.nagbars.insert(k.into(), true);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_reshape_the_user_like_fluxer_getters() {
        let me = User { verified: true, premium_type: Some(PremiumType::NONE), premium_until: Some("2020-01-01T00:00:00.000Z".into()), ..Default::default() };
        let mut o = Options::default();
        assert!(!o.touches_user());
        assert_eq!(o.apply(&me), me);
        o.email_verified_override = Some(false);
        o.unclaimed_account_override = Some(true);
        o.premium_type_override = Some(2);
        let u = o.apply(&me);
        assert!(!u.verified);
        assert_eq!(u.account_tier.as_deref(), Some("unclaimed"), "unclaimed wins over the verified tier");
        assert_eq!(u.premium_type, Some(PremiumType::LIFETIME));
        assert_eq!(u.premium_until, me.premium_until, "a hand-set type keeps the real dates");
        // A scenario sets every premium field, dates relative to now; editing a key drops it.
        let now = 1_700_000_000_000;
        o.apply_scenario("active_yearly_cancelled", now);
        assert_eq!(o.premium_scenario_override.as_deref(), Some("active_yearly_cancelled"));
        let u = o.apply(&me);
        assert_eq!(u.premium_type, Some(PremiumType::SUBSCRIPTION));
        assert_eq!(u.premium_until, chrono::DateTime::from_timestamp_millis(now + 60 * DAY).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)));
        assert_eq!((u.premium_billing_cycle.as_deref(), u.premium_will_cancel, u.has_ever_purchased), (Some("yearly"), true, true));
        o.apply_scenario("visionary", now);
        assert_eq!((o.apply(&me).premium_until, o.premium_billing_cycle_override.clone()), (None, None), "visionary has no end date");
        o.reset("premium_type_override");
        assert_eq!((o.premium_type_override, o.premium_scenario_override.clone()), (None, None));
        o.reset_premium();
        assert_eq!((o.premium_since_override, o.has_ever_purchased_override), (None, None));
    }

    #[test]
    fn active_entries_list_non_defaults_and_reset_one_by_one() {
        let mut o = Options::default();
        assert!(o.active().is_empty());
        o.show_myself_typing = true;
        o.mock_slowmode_remaining = 5000;
        o.mock_mature_media_gate_reason = "geo_restricted".into();
        o.premium_lifetime_sequence_override = Some(7);
        o.mock_gift_duration_months = 0;
        o.premium_scenario_override = Some("expired_old".into());
        o.nagbars.insert("forceIncident".into(), true);
        let a = o.active();
        let get = |k: &str| a.iter().find(|e| e.0 == k).map(|e| (e.1.clone(), e.2.clone()));
        assert_eq!(a.len(), 5, "the scenario and nagbars aren't developer entries: {a:?}");
        assert_eq!(get("show_myself_typing"), Some(("Show myself typing".into(), "Enabled".into())));
        assert_eq!(get("mock_slowmode_remaining").unwrap().1, "5 seconds");
        assert_eq!(get("mock_mature_media_gate_reason").unwrap().1, "geo restricted");
        assert_eq!(get("premium_lifetime_sequence_override").unwrap().1, "#7");
        assert_eq!(get("mock_gift_duration_months").unwrap().1, "Lifetime");
        o.reset("mock_slowmode_remaining");
        assert_eq!(o.mock_slowmode_remaining, 10_000);
        o.reset_all();
        assert_eq!(o.active(), Vec::new());
        assert_eq!(o.nagbars.get("forceIncident"), Some(&true), "nagbars reset on their own");
        assert_eq!(duration(0), "No timer");
        assert_eq!(duration(120_000), "2 minutes");
    }

    #[test]
    fn nagbar_use_actual_clears_force_and_dismissal() {
        let mut o = Options::default();
        let mut dismissed = vec!["premium-onboarding".to_string(), "maintenance:x".into()];
        o.nagbars.insert("forcePremiumOnboarding".into(), false);
        use_actual(&mut o, &mut dismissed, "forcePremiumOnboarding");
        assert!(o.nagbars.is_empty());
        assert_eq!(dismissed, ["maintenance:x"]);
    }

    #[test]
    fn mock_gifts_like_gift_commands() {
        let o = Options { mock_gift_inventory: Some(true), mock_gift_duration_months: 24, mock_gift_redeemed: Some(true), ..Default::default() };
        let g = o.mock_gifts(json!({"id": "1"}), 1_700_000_000_000);
        assert_eq!((g[0]["duration_type"].as_str(), g[0]["duration_quantity"].as_u64()), (Some("years"), Some(2)));
        assert!(g[0]["redeemed_at"].is_string() && g[0]["redeemed_by"]["id"] == "1");
        let g = Options { mock_gift_duration_months: 0, ..o }.mock_gifts(Value::Null, 0);
        assert_eq!(g[0]["duration_type"], "months");
    }
}
