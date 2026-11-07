//! `ProfileBadgeSettings` (`my_profile_tab/ProfileBadgeSettings.tsx`): every badge the account
//! holds with a switch that hides it from other people, plus the premium / Operator Pass
//! variants (purchase date hidden, Visionary masked, pass date and count hidden). The draft lives
//! in `form.data["p.badges"]` and saves with the profile; each change re-runs
//! `POST /users/@me/profile-badges/preview` so the preview card shows what others would see.

use reflux_model::User;
use reqwest::Method;
use rune::{El, col, el, row};
use crate::ui::mark::marked as text;
use serde_json::{Value, json};

use super::account::muted;
use crate::guild::Then;
use crate::i18n::{t, tf};
use crate::media::Load;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Input, Switch};

/// Where the preview response is kept.
pub(crate) const PREVIEW: &str = "profile_badges";
const DRAFT: &str = "p.badges";
const PATH: &str = "/users/@me/profile-badges/preview";
/// `FILTER_THRESHOLD`.
const FILTER_THRESHOLD: usize = 20;

/// The variant switches' fields, in `ProfileBadgeVariantField` order.
const FIELDS: [&str; 4] = ["premium_badge_timestamp_hidden", "premium_badge_masked", "operator_pass_badge_timestamp_hidden", "operator_pass_badge_count_hidden"];

/// Hidden badge keys and the variant flags.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Draft {
    hidden: Vec<String>,
    flags: [bool; 4],
}

impl Draft {
    pub(super) fn saved(me: &User) -> Draft {
        let mut hidden = me.hidden_profile_badges.clone();
        hidden.sort();
        Draft { hidden, flags: [me.premium_badge_timestamp_hidden, me.premium_badge_masked, me.operator_pass_badge_timestamp_hidden, me.operator_pass_badge_count_hidden] }
    }

    /// The edited draft, else `saved`.
    pub(super) fn current(st: &ShellState, saved: &Draft) -> Draft {
        st.form.data.get(DRAFT).and_then(|v| serde_json::from_value::<(Vec<String>, [bool; 4])>(v.clone()).ok()).map_or_else(|| saved.clone(), |(hidden, flags)| Draft { hidden, flags })
    }

    /// The request body (`ProfileBadgePreviewRequest`); also the PATCH fields.
    fn body(&self) -> serde_json::Map<String, Value> {
        let mut m = serde_json::Map::new();
        m.insert("hidden_profile_badges".into(), json!(self.hidden));
        for (k, v) in FIELDS.iter().zip(self.flags) {
            m.insert((*k).into(), Value::Bool(v));
        }
        m
    }

    /// The fields that differ from `saved`, for the profile PATCH.
    pub(super) fn changes(&self, saved: &Draft, out: &mut serde_json::Map<String, Value>) {
        let body = self.body();
        if self.hidden != saved.hidden {
            out.insert("hidden_profile_badges".into(), body["hidden_profile_badges"].clone());
        }
        for (i, k) in FIELDS.iter().enumerate() {
            if self.flags[i] != saved.flags[i] {
                out.insert((*k).into(), body[*k].clone());
            }
        }
    }
}

/// Loads the badges with the saved settings.
pub(super) fn load(sh: &mut ShellState) {
    sh.api(Method::POST, PATH, Some(json!({})), Then::Keep(PREVIEW.into()));
}

/// Stores `d` as the draft and previews it.
fn set(sh: &mut ShellState, d: &Draft) {
    sh.form.data.insert(DRAFT.into(), json!((d.hidden.clone(), d.flags)));
    sh.api(Method::POST, PATH, Some(Value::Object(d.body())), Then::Keep(PREVIEW.into()));
}

/// The labels the preview card shows (`profile_badges`), if previewed.
pub(super) fn shown(st: &ShellState) -> Vec<String> {
    let list = st.form.data.get(PREVIEW).and_then(|v| v["profile_badges"].as_array());
    list.map(|a| a.iter().filter_map(|b| b["label"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// `profileBadgeImageUrl`.
fn art_url(st: &ShellState, b: &Value) -> Option<String> {
    if let Some(a) = b["asset"].as_str() {
        return Some(format!("{}/badges/{a}.webp", st.static_base.trim_end_matches('/')));
    }
    b["asset_url"].as_str().map(str::to_string)
}

/// `BadgeArt` in its 40×24 slot.
fn art(st: &ShellState, b: &Value, p: &Palette) -> El {
    let slot = row().size(40.0, 24.0).center();
    match art_url(st, b) {
        Some(url) => {
            let w = b["aspect_ratio"].as_f64().map_or(24.0, |r| 24.0 * r as f32);
            let img = el().size(w.min(40.0), 24.0);
            match st.media.get(&url) {
                Load::Ready(px) => slot.child(img.pixels_fit(&px, rune::SurfaceFit::Fit)),
                _ => slot.child(img),
            }
        }
        None => slot.child(text(b["caption"].as_str().unwrap_or_default().to_string()).font(font(16.0, 700.0).lh(16.0)).color(p.brand_primary_light)),
    }
}

/// The section (global profile only).
pub(super) fn section<S: 'static>(st: &ShellState, saved: &Draft, busy: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let d = Draft::current(st, saved);
    let own = st.form.data.get(PREVIEW).and_then(|v| v["own_badges"].as_array());
    let body = match own {
        None if st.form.error.is_some() => row()
            .gap(12.0)
            .center_y()
            .child(muted(t("Your badges couldn't be loaded."), p))
            .child(Button::new(t("Try again")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).on_press(move |s: &mut S| {
                let sh = acc(s);
                sh.form.error = None;
                load(sh);
            }).el(p)),
        None => muted(t("Loading…"), p),
        Some(a) if a.is_empty() => muted(t("You don't have any badges yet."), p),
        Some(a) => list(st, a, &d, busy, p, acc),
    };
    col().w_full().gap(16.0).child(muted(t("Choose which badges other people see on your profile."), p)).child(body)
}

fn list<S: 'static>(st: &ShellState, own: &[Value], d: &Draft, busy: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let filter = st.form.text.get("badge-filter").map(|e| e.text.trim().to_lowercase()).unwrap_or_default();
    let mut out = col().w_full().gap(16.0);
    if own.len() > FILTER_THRESHOLD {
        let edit = st.form.text.get("badge-filter").cloned().unwrap_or_default();
        let field = Input::new("form-badge-filter", &edit, move |s: &mut S| acc(s).form.text.entry("badge-filter".into()).or_default()).placeholder(t("Filter badges")).el(p);
        out = out.child(field.label(t("Filter badges")));
    }
    let shown: Vec<&Value> = own.iter().filter(|b| filter.is_empty() || b["label"].as_str().unwrap_or_default().to_lowercase().contains(&filter)).collect();
    if shown.is_empty() {
        out = out.child(muted(t("No badges match."), p));
    }
    let premium = "Plutonium";
    for b in shown {
        let key = b["key"].as_str().unwrap_or_default().to_string();
        let label = b["label"].as_str().unwrap_or_default().to_string();
        let visible = !d.hidden.contains(&key);
        let number_moot = key == "visionary_number" && (d.hidden.iter().any(|k| k == "visionary") || d.flags[1]);
        let toggle = {
            let (d, key) = (d.clone(), key.clone());
            move |s: &mut S| {
                let mut d = d.clone();
                match d.hidden.iter().position(|k| *k == key) {
                    Some(i) => {
                        d.hidden.remove(i);
                    }
                    None => {
                        d.hidden.push(key.clone());
                        d.hidden.sort();
                    }
                }
                set(acc(s), &d);
            }
        };
        let head = row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(art(st, b, p))
            .child(text(label.clone()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).wrap().grow(1.0).shrink())
            .child(Switch::new(visible).disabled(busy || number_moot).on_toggle(toggle).el(p).label(tf("Show {badgeLabel} on your profile", &[("badgeLabel", &label)])));
        let mut item = col().w_full().gap(12.0).child(head);
        if number_moot {
            item = item.child(muted(t("Shown only when your Visionary badge is visible and not masked."), p).pl(52.0));
        }
        let product = if key == "visionary" { "Visionary" } else { premium };
        let date = (0usize, tf("Hide {premiumProductName} purchase date", &[("premiumProductName", product)]), Some(tf("Remove when you first bought {premiumProductName} from your badge", &[("premiumProductName", product)])));
        let variants: Vec<(usize, String, Option<String>)> = match key.as_str() {
            _ if !visible => vec![],
            "visionary" => vec![(1, t("Mask Visionary as subscription").into(), Some(t("Show your Visionary as a regular subscription instead").into())), date],
            "plutonium" | "premium" => vec![date],
            "operator_pass" => vec![(2, tf("Hide when you got your {operatorPassName}", &[("operatorPassName", "Operator Pass")]), None), (3, t("Hide how many Operator Passes you have").into(), None)],
            _ => vec![],
        };
        if !variants.is_empty() {
            let mut vs = col().w_full().gap(12.0).pl(52.0);
            for (i, l, desc) in variants {
                let d = d.clone();
                let mut sw = Switch::new(d.flags[i]).label(l).disabled(busy).on_toggle(move |s: &mut S| {
                    let mut d = d.clone();
                    d.flags[i] = !d.flags[i];
                    set(acc(s), &d);
                });
                if let Some(desc) = desc {
                    sw = sw.description(desc);
                }
                vs = vs.child(sw.el(p));
            }
            item = item.child(vs);
        }
        out = out.child(item);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_list_only_edited_fields() {
        let saved = Draft { hidden: vec!["staff".into()], flags: [false; 4] };
        let mut m = serde_json::Map::new();
        saved.changes(&saved, &mut m);
        assert!(m.is_empty());
        let edited = Draft { hidden: vec![], flags: [true, false, false, true] };
        edited.changes(&saved, &mut m);
        assert_eq!(Value::Object(m), json!({ "hidden_profile_badges": [], "premium_badge_timestamp_hidden": true, "operator_pass_badge_count_hidden": true }));
        let mut st = ShellState::default();
        st.form.data.insert(DRAFT.into(), json!((edited.hidden.clone(), edited.flags)));
        assert_eq!(Draft::current(&st, &saved), edited);
    }
}
