//! Badges: `UserProfileBadges` (`user/components/popouts/UserProfileBadges.tsx`), the shared
//! `BadgeListModal` it and `GuildBadgesModal` open from their `+N` pill, and the hover tooltips
//! of badge rows (a profile's and [`super::invite_embed::guild_badge`]'s).

use crate::ui::SmoothWheel;
use rune::{Align, El, col, el, icon, row, text};
use serde_json::Value;

use super::{ShellAccess, ShellState};
use crate::i18n::{t, tf, tp};
use crate::media::Load;
use crate::theme::{Palette, font};
use crate::ui::mix;
use crate::ui::overlay::{Placement, anchor, tooltip_custom};

/// `BadgeListItem`.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub key: String,
    pub image: Option<String>,
    pub caption: Option<String>,
    pub title: String,
    pub lines: Vec<String>,
    pub quote: Option<String>,
    pub href: Option<String>,
}

/// `customBadgeImageUrl`.
pub fn custom_url(asset_url: &str) -> String {
    format!("{asset_url}?size=96")
}

/// `profileBadgeImageUrl`: a built-in asset on the static CDN, else the custom badge's image.
pub fn image_url(st: &ShellState, b: &Value) -> Option<String> {
    if let Some(a) = b["asset"].as_str() {
        return Some(format!("{}/badges/{a}.webp", st.static_base.trim_end_matches('/')));
    }
    b["asset_url"].as_str().filter(|u| !u.is_empty()).map(custom_url)
}

fn str_of(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

/// `openBadgeList`'s mapping of a `ProfileBadgeResponse`.
pub fn from_profile(st: &ShellState, b: &Value) -> Item {
    Item {
        key: b["key"].as_str().unwrap_or_default().to_string(),
        image: image_url(st, b),
        caption: str_of(&b["caption"]),
        title: str_of(&b["details"]["title"]).or_else(|| str_of(&b["label"])).unwrap_or_default(),
        lines: b["details"]["lines"].as_array().map(|a| a.iter().filter_map(str_of).collect()).unwrap_or_default(),
        quote: str_of(&b["quote"]).filter(|q| !q.is_empty()),
        href: str_of(&b["href"]).filter(|h| !h.is_empty()),
    }
}

/// `GuildBadgesModal`'s mapping of an instance guild badge.
pub fn from_guild(b: &reflux_model::GuildBadge) -> Item {
    Item {
        key: b.id.to_string(),
        image: Some(custom_url(&b.asset_url)),
        caption: None,
        title: b.name.clone(),
        lines: b.description.clone().into_iter().collect(),
        quote: None,
        href: b.href.clone(),
    }
}

thread_local! {
    /// Tooltip contents of the badges drawn this frame, by trigger key ([`tip_layer`] draws the
    /// open one): the content and the tooltip's max width.
    static TIPS: std::cell::RefCell<Vec<(String, El, f32)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Keys `trigger` and gives it the tooltip `content` (`Tooltip text={() => …}`).
pub fn tip<S: 'static>(key: String, content: El, max_w: f32, trigger: El, acc: ShellAccess<S>) -> El {
    TIPS.with_borrow_mut(|t| t.push((key.clone(), content, max_w)));
    let id = key.clone();
    trigger.key(key).on_hover_change(move |s: &mut S, inside, _| {
        let t = &mut acc(s).popups.tip;
        if inside {
            *t = Some(id.clone());
        } else if t.as_deref() == Some(id.as_str()) {
            *t = None;
        }
    })
}

/// `.tooltipContent` around a plain label.
pub fn label_tip(label: &str, p: &Palette) -> El {
    text(label.to_string()).font(font(13.0, 500.0).lh(18.0)).color(p.text_primary).align(Align::Center).wrap().py(7.0).px(10.0)
}

/// The open badge tooltip, over its badge.
pub fn tip_layer(st: &ShellState, p: &Palette, layer: &mut Vec<El>) {
    let tips = TIPS.with_borrow_mut(std::mem::take);
    if let Some((key, content, w)) = st.popups.tip.as_ref().and_then(|open| tips.into_iter().find(|(k, ..)| k == open)) {
        layer.push(anchor(tooltip_custom(content, w, Placement::Top, p), &key, Placement::Top, 8.0));
    }
}

/// `MAX_WIDTH_MAP.xl`.
pub const XL: f32 = 350.0;
/// `COMPACT_VISIBLE_BADGES`, `WIDE_VISIBLE_BADGES`.
pub const POPOUT_VISIBLE: usize = 7;
pub const MODAL_VISIBLE: usize = 15;

/// `"…"` (`ProfileBadgeQuote`).
fn quote(q: &str, p: &Palette) -> El {
    text(format!("\"{q}\"")).font(font(13.0, 500.0).lh(18.0)).color(p.text_primary).align(Align::Center).wrap()
}

/// A badge's tooltip content: `ProfileBadgeDetailsTooltip` (art, title, lines, quote),
/// `ProfileBadgeLabelTooltip` (label and quote) or the label.
fn profile_tip(st: &ShellState, b: &Value, p: &Palette) -> El {
    let label = b["label"].as_str().unwrap_or_default();
    let q = b["quote"].as_str().filter(|q| !q.is_empty());
    // `.details`: centred column, gap 4, padding 4/2, inside `.tooltipContent`'s 7/10.
    let details = || col().gap(4.0).py(4.0 + 7.0).px(2.0 + 10.0).items_center();
    if b["details"].is_object() {
        let mut c = details();
        if let Some(url) = image_url(st, b) {
            let w = b["aspect_ratio"].as_f64().map_or(48.0, |r| 48.0 * r as f32);
            let art = el().size(w, 48.0).mb(4.0);
            c = c.child(match st.media.get(&url) {
                Load::Ready(px) => art.pixels_fit(&px, rune::SurfaceFit::Fit),
                _ => art,
            });
        }
        c = c.child(text(b["details"]["title"].as_str().unwrap_or(label).to_string()).font(font(14.0, 700.0).lh(20.0)).color(p.text_primary).align(Align::Center).wrap());
        for l in b["details"]["lines"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            c = c.child(text(l.to_string()).font(font(13.0, 400.0).lh(18.0)).color(p.text_secondary).align(Align::Center).wrap());
        }
        c.children(q.map(|q| quote(q, p)))
    } else if let Some(q) = q {
        details().child(text(label.to_string()).font(font(13.0, 500.0).lh(18.0)).color(p.text_primary).align(Align::Center).wrap()).child(quote(q, p))
    } else {
        label_tip(label, p)
    }
}

/// `opensPremiumModal`.
fn opens_premium(b: &Value) -> bool {
    match b["kind"].as_str() {
        Some("premium") => b["href"].is_null(),
        Some("plutonium") => true,
        _ => false,
    }
}

/// `UserProfileBadges`: up to `limit` badges (24px art or the caption), each with its tooltip
/// and link, a `+N` pill opening [`list_modal`], and the pencil when `edit` (own profile).
/// `None` when there's nothing to show.
pub fn profile_row<S: 'static>(st: &ShellState, owner: &str, badges: &[Value], limit: usize, edit: bool, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if badges.is_empty() && !edit {
        return None;
    }
    let shown = &badges[..badges.len().min(limit)];
    let hidden = badges.len() - shown.len();
    let mut items = Vec::new();
    for (i, b) in shown.iter().enumerate() {
        let face = match image_url(st, b) {
            Some(url) => {
                let w = b["aspect_ratio"].as_f64().map_or(24.0, |r| 24.0 * r as f32);
                let img = el().size(w, 24.0);
                match st.media.get(&url) {
                    Load::Ready(px) => img.pixels_fit(&px, rune::SurfaceFit::Fit),
                    _ => img,
                }
            }
            None => row()
                .h(24.0)
                .center_y()
                .child(text(b["caption"].as_str().unwrap_or_default().to_string()).font(font(20.0, 700.0).lh(24.0)).color(if crate::ui::is_light(p) { p.brand_primary_foreground } else { p.brand_primary_light })),
        };
        let alt = b["accessible_label"].as_str().or(b["caption"].as_str()).or(b["label"].as_str()).unwrap_or_default();
        let mut face = face.label(alt);
        if opens_premium(b) {
            face = face.cursor(rune::Cursor::Pointer).role("button").on_press(move |s: &mut S| {
                let sh = acc(s);
                sh.popups.close();
                sh.open_settings(Some("plutonium"));
            });
        } else if let Some(href) = b["href"].as_str().filter(|h| !h.is_empty()).map(str::to_string) {
            face = face.cursor(rune::Cursor::Pointer).role("link").on_press_tx(move |s: &mut S, tx| super::links::open(acc(s), &href, tx));
        }
        items.push(tip(format!("{owner}-badge-{i}"), profile_tip(st, b, p), XL, face, acc));
    }
    if hidden > 0 {
        let all: Vec<Item> = badges.iter().map(|b| from_profile(st, b)).collect();
        items.push(tip(format!("{owner}-badge-more"), label_tip(t("Show all badges"), p), 190.0, more_pill(hidden, 24.0, 24.0, 6.0, all, p, acc), acc));
    }
    if edit {
        let pencil = row()
            .size(24.0, 24.0)
            .center()
            .radius(4.0)
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(t("Edit badges"))
            .on_press(move |s: &mut S| {
                let sh = acc(s);
                sh.popups.close();
                sh.account.profile_modal = None;
                sh.open_settings(Some("my_profile"));
            })
            .child(icon(&crate::icons::PENCIL_SIMPLE_BOLD, 16.0).color(p.text_primary_muted).group_color(p.text_primary));
        items.push(tip(format!("{owner}-badge-edit"), label_tip(t("Edit badges"), p), 190.0, pencil, acc));
    }
    Some(row().flex_wrap().gaps(6.0, 6.0).center_y().children(items))
}

/// `.moreButton` / `.morePill`: `+N`, opening the list.
pub fn more_pill<S: 'static>(hidden: usize, min_w: f32, h: f32, radius: f32, all: Vec<Item>, p: &Palette, acc: ShellAccess<S>) -> El {
    let n = hidden.to_string();
    row()
        .min_w(min_w)
        .h(h)
        .px(6.0)
        .center()
        .radius(radius)
        .fill(p.background_modifier_accent)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(tp("{count, plural, one {+# more badge, show all} other {+# more badges, show all}}", hidden as i64, &[]))
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.popups.tip = None;
            sh.media_ui.badge_list = Some(all.clone());
        })
        .child(text(tf("+{count}", &[("count", &n)])).font(font(12.0, 600.0).lh(12.0)).color(p.text_primary))
}

/// `BadgeListModal`: every badge with its art (or caption), title, lines and quote; linked rows
/// open their link.
pub fn list_modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let items = st.media_ui.badge_list.as_ref()?;
    let close = move |s: &mut S| acc(s).media_ui.badge_list = None;
    let list = col().w_full().gap(4.0).children(items.iter().map(|it| {
        let art = match &it.image {
            Some(url) => {
                let img = el().size(32.0, 32.0);
                match st.media.get(url) {
                    Load::Ready(px) => img.pixels_fit(&px, rune::SurfaceFit::Fit),
                    _ => img,
                }
            }
            None => row()
                .min_w(32.0)
                .h(32.0)
                .center()
                
                .child(text(it.caption.clone().unwrap_or_default()).font(font(20.0, 700.0).lh(32.0)).color(p.brand_primary_light)),
        };
        let mut words = col().min_w(0.0).shrink().gap(2.0).child(text(it.title.clone()).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary).wrap());
        for l in &it.lines {
            words = words.child(text(l.clone()).font(font(13.0, 400.0).lh(18.0)).color(p.text_secondary).wrap());
        }
        if let Some(q) = &it.quote {
            words = words.child(text(format!("\"{q}\"")).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary).wrap());
        }
        let r = row().w_full().items_start().gap(12.0).padding(8.0).radius(6.0).child(art).child(words);
        match it.href.clone() {
            Some(href) => r.hover(p.background_modifier_hover).cursor(rune::Cursor::Pointer).role("link").on_press_tx(move |s: &mut S, tx| super::links::open(acc(s), &href, tx)),
            None => r,
        }
    }));
    let body = col().w_full().max_h(560.0).scroll(rune::ScrollSpec { stick_bottom: false }).smooth_wheel::<S>("badges-modal").child(list);
    Some(crate::ui::modal::Modal::new(t("Badges"), close).body(body).el(p))
}

/// The popout's badge bar (`.containerPopout`): over the header's bottom-right corner, 145px
/// from the card's top, on an 80% `background-secondary` plate.
pub fn popout_plate(row_el: El, card_w: f32, p: &Palette) -> El {
    row_el.max_w(card_w - 112.0).padding(4.0).radius(6.0).fill(mix(p.background_secondary, rune::Color::TRANSPARENT, 80.0)).justify_end()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn profile_items_map_like_open_badge_list() {
        let st = ShellState { static_base: "https://static.example".into(), ..Default::default() };
        let built = json!({"key": "staff", "kind": "staff", "label": "Staff", "asset": "staff", "href": null, "details": {"title": "Fluxer Staff", "lines": ["Since 2024"]}, "quote": ""});
        let it = from_profile(&st, &built);
        assert_eq!(it.image.as_deref(), Some("https://static.example/badges/staff.webp"));
        assert_eq!((it.title.as_str(), it.lines.len(), it.quote, it.href), ("Fluxer Staff", 1, None, None));
        let custom = json!({"key": "custom:1", "label": "Helper", "asset": null, "asset_url": "https://m/x.png", "caption": null, "quote": "hi", "href": "https://e"});
        let it = from_profile(&st, &custom);
        assert_eq!((it.image.as_deref(), it.title.as_str(), it.quote.as_deref()), (Some("https://m/x.png?size=96"), "Helper", Some("hi")));
        let caption = json!({"key": "visionary_number", "label": "#12", "caption": "#12"});
        assert_eq!(from_profile(&st, &caption).image, None);
    }

    #[test]
    fn premium_badges_open_the_plutonium_page() {
        assert!(opens_premium(&json!({"kind": "premium", "href": null})));
        assert!(!opens_premium(&json!({"kind": "premium", "href": "https://x"})));
        assert!(opens_premium(&json!({"kind": "plutonium"})));
        assert!(!opens_premium(&json!({"kind": "staff"})));
    }
}
