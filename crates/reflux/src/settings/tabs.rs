//! The content of each settings tab.

use reflux_store::Store;
use rune::{El, col, el, icon, row};
use crate::ui::mark::marked as text;

use super::SettingsUi;
use crate::i18n::t;
use crate::icons;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};

/// Tab `tab`'s content (the open tab, or one shown inline in search results).
pub fn body<S: 'static>(store: &Store, st: &ShellState, ui: &SettingsUi, tab: &str, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    match tab {
        "my_profile" => super::profile::tab(store, st, ui, p, acc, layer),
        "account_security" | "linked_accounts" if st.desktop.streamer && st.prefs.streamer.hide_personal => super::desktop_tabs::streamer_gate(p, acc).key("streamer-gate"),
        "account_security" => super::account::tab(store, st, ui, p, acc),
        "privacy_safety" => super::privacy::tab(store, st, p, acc, layer),
        "devices" => super::privacy::devices(st, p, acc),
        "authorized_apps" => super::privacy::apps(st, p, acc),
        "linked_accounts" => super::privacy::connections(st, p, acc),
        "blocked_users" => section(crate::i18n::t("Blocked users"), super::account::blocked(store, st, p, acc), true, p),
        "appearance" => super::look_tabs::appearance(store, st, p, acc, layer),
        "themes" => super::look_tabs::themes(store, st, p, acc, layer),
        "chat_settings" => super::look_tabs::chat(store, st, p, acc),
        "accessibility" => super::look_tabs::accessibility(store, st, p, acc, layer),
        "advanced_settings" => super::advanced::tab(store, st, &ui.searched, p, acc, layer),
        "voice_video" => super::voice_tabs::tab(store, st, p, acc, layer),
        "notifications" => super::look_tabs::notifications(store, st, p, acc),
        "keybinds" => crate::nav::keys::tab(st, p, acc, layer),
        "language" => super::desktop_tabs::language(store, st, p, acc),
        "plutonium" => super::premium::plutonium(store, st, p, acc),
        "gift_inventory" => section(crate::i18n::t("Purchased gifts"), super::premium::gifts(st, p, acc), true, p),
        "desktop_settings" => super::desktop_tabs::desktop(st, p, acc),
        "applications" => super::dev::applications(st, p, acc, layer),
        "embed_debugger" => super::dev::embed_debugger(store, st, p, acc),
        "component_gallery" => {
            let (body, mut over) = crate::ui::gallery::parts(&st.nav.gallery, true);
            layer.append(&mut over);
            body
        }
        _ => not_available(p),
    }
}

thread_local! {
    /// The tab being rendered, for the section deep links (`buildUserSettingsDeepLink`).
    static TAB: std::cell::Cell<&'static str> = const { std::cell::Cell::new("") };
}

pub(crate) fn set_current_tab(tab: &'static str) {
    TAB.with(|t| t.set(tab));
    spy::begin(tab);
}

/// `useScrollSpy` and the sidebar's section list: the sections the open tab rendered, which of
/// them the content is scrolled to, and scrolling to one.
pub(crate) mod spy {
    use std::cell::RefCell;

    const EPSILON: f32 = 2.0;
    /// `SCROLL_OFFSET`: a section counts from this far above its top.
    const OFFSET: f32 = 24.0;
    /// The content viewport's key (`settings::content`).
    pub const VIEWPORT: &str = "settings-content";

    #[derive(Default)]
    struct Spy {
        tab: &'static str,
        /// Each section's title and its top in the content, when it has been laid out.
        sections: Vec<(String, Option<f32>)>,
        pos: Option<rune::ScrollPos>,
        /// What the sidebar last showed as active.
        shown: Option<usize>,
        /// `pinnedRef`: the section a click scrolled to stays active while the offset stays put.
        /// It holds from the click until the scroll it asked for has been seen (`.2`).
        pin: Option<(String, f32, bool)>,
        /// A rebuild was asked for since the last build (one per build: other settings views
        /// share the viewport and never rebuild this list).
        asked: bool,
    }

    thread_local! {
        static SPY: RefCell<Spy> = RefCell::default();
        /// The content's scroll offset, for the layout reads in [`add`].
        static OFFSET_Y: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
    }

    pub(super) fn begin(tab: &'static str) {
        SPY.with_borrow_mut(|s| {
            if s.tab != tab {
                (s.tab, s.pin, s.pos) = (tab, None, None);
                OFFSET_Y.set(0.0);
            }
            s.sections.clear();
            s.asked = false;
        });
    }

    /// A section was rendered: its top comes from the last frame's layout.
    pub(super) fn add(title: &str, key: &str) {
        // Read as the top in the content (the window y plus the offset), which scrolling leaves
        // alone: a plain window rect changes every scrolled frame and would rebuild the view.
        let y = rune::memo::window_layout_with(key, |r| (r[1] + OFFSET_Y.get()).round());
        let vp = rune::memo::window_layout_with(VIEWPORT, |r| r[1].round());
        SPY.with_borrow_mut(|s| {
            let top = match (y, vp, s.pos) {
                (Some(y), Some(vp), Some(_)) => Some(y - vp),
                _ => None,
            };
            s.sections.push((title.to_owned(), top));
        });
    }

    /// `buildThresholds`: the offset each section becomes active at. Sections whose top can't
    /// reach the viewport's top share the last stretch of the scroll range in proportion.
    pub(super) fn thresholds(tops: &[f32], max: f32, client_h: f32) -> Vec<f32> {
        let raw: Vec<f32> = tops.iter().map(|t| t - OFFSET).collect();
        if raw.is_empty() || max <= 0.0 {
            return raw;
        }
        let Some(first) = raw.iter().position(|t| *t > max - EPSILON) else { return raw };
        let span = raw[raw.len() - 1] - max;
        if span <= 0.0 {
            return raw.iter().enumerate().map(|(i, t)| if i < first { *t } else { max }).collect();
        }
        let floor = if first > 0 { raw[first - 1].clamp(0.0, max) } else { 0.0 };
        let zone = span.min((client_h - OFFSET).max(0.0)).min(max - floor);
        raw.iter().enumerate().map(|(i, t)| if i < first { *t } else { max - zone * (1.0 - ((t - max) / span).clamp(0.0, 1.0)) }).collect()
    }

    /// `applyActiveFromPosition`.
    fn active(s: &mut Spy) -> Option<usize> {
        let pos = s.pos?;
        let at = pos.offset[1];
        if let Some((title, expected, seen)) = &s.pin {
            if (!seen || (at - expected).abs() <= EPSILON)
                && let Some(i) = s.sections.iter().position(|(t, _)| t == title)
            {
                return Some(i);
            }
            s.pin = None;
        }
        let tops: Vec<f32> = s.sections.iter().map(|(_, t)| *t).collect::<Option<_>>()?;
        let th = thresholds(&tops, pos.max[1], pos.viewport[1]);
        Some(th.iter().take_while(|t| **t <= at + EPSILON).count().saturating_sub(1))
    }

    /// After the tab's body is built: its sections' titles and the active one's index.
    pub fn sections() -> (Vec<String>, usize) {
        SPY.with_borrow_mut(|s| {
            let a = if s.sections.is_empty() { None } else { active(s) };
            s.shown = a;
            (s.sections.iter().map(|(t, _)| t.clone()).collect(), a.unwrap_or(0))
        })
    }

    /// The content scrolled or resized: rebuild when the active section changed, or when sections
    /// were rendered before they had a layout to measure.
    pub fn scrolled(pos: rune::ScrollPos) {
        let stale = SPY.with_borrow_mut(|s| {
            s.pos = Some(pos);
            OFFSET_Y.set(pos.offset[1]);
            if let Some(pin) = s.pin.as_mut() {
                pin.2 = true;
            }
            let stale = !s.asked && s.sections.len() > 1 && (s.sections.iter().any(|(_, t)| t.is_none()) || active(s) != s.shown);
            s.asked |= stale;
            stale
        });
        if stale {
            crate::app::update(|_| {});
        }
    }

    /// `scrollToSection`: the section's top lands [`OFFSET`] below the viewport's, and it is
    /// pinned as the active one.
    pub fn scroll_to(title: &str, tx: &mut rune::Tx) {
        SPY.with_borrow_mut(|s| {
            let (Some(pos), Some(top)) = (s.pos, s.sections.iter().find(|(t, _)| t == title).and_then(|(_, t)| *t)) else { return };
            let target = (top - OFFSET).clamp(0.0, pos.max[1].max(0.0));
            s.pin = Some((title.to_owned(), target, false));
            tx.scroll_to_y(VIEWPORT, target);
        });
    }
}

/// `buildUserSettingsDeepLink`: `fluxer://settings/user?tab=…[&section=…]`.
pub(crate) fn deep_link(tab: &str, section: Option<&str>) -> String {
    match section {
        Some(s) => format!("fluxer://settings/user?tab={tab}&section={s}"),
        None => format!("fluxer://settings/user?tab={tab}"),
    }
}

/// `SettingsHeadingLinkButton`: a 28px link that copies `href`, turning into a check for two
/// seconds (`COPY_LINK_TO_PAGE` / `COPY_LINK_TO_SECTION`).
pub(crate) fn link_button(href: String, page: bool, p: &Palette) -> El {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static COPIED: Mutex<Option<(String, Instant)>> = Mutex::new(None);
    let copied = COPIED.lock().unwrap().as_ref().is_some_and(|(h, at)| *h == href && at.elapsed() < Duration::from_secs(2));
    let label = if copied { t("Copied") } else if page { t("Copy link to page") } else { t("Copy link to section") };
    let press = href.clone();
    row()
        .size(28.0, 28.0)
        .center()
        .radius(6.0)
        .role("button")
        .label(label.to_string())
        .cursor(rune::Cursor::Pointer)
        .hover_group()
        .on_press_tx(move |_: &mut crate::State, tx: &mut rune::Tx| {
            tx.copy(press.clone());
            *COPIED.lock().unwrap() = Some((press.clone(), Instant::now()));
            tx.wake_after(Duration::from_secs(2));
        })
        .child(icon(if copied { &icons::CHECK_BOLD } else { &icons::LINK_BOLD }, 15.0).color(p.text_tertiary).group_color(p.text_primary))
}

/// The section's deep-link id: Fluxer's ids are the lowercased title (`audio`, `video`, …).
fn section_id(title: &str) -> String {
    title.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect::<String>().trim_matches('_').to_string()
}

/// `SettingsSection`: title 18/600 with its deep-link button, content gap 20; later sections get a
/// top border and 24 padding.
pub(crate) fn section(title: &str, content: El, first: bool, p: &Palette) -> El {
    // Keyed by title: tabs add sections conditionally, which would shift the ones after them.
    let link = deep_link(TAB.with(|t| t.get()), Some(&section_id(crate::i18n::t(title))));
    let key = format!("sec-{title}");
    spy::add(crate::i18n::t(title), &key);
    let mut s = col()
        .key(key)
        .w_full()
        .gap(16.0)
        .pb(24.0)
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(crate::ui::mark::text(text(crate::i18n::t(title)).font(font(18.0, 600.0).lh(23.4)).color(p.text_primary), crate::i18n::t(title)))
                .child(link_button(link, false, p)),
        )
        .child(col().w_full().gap(20.0).child(content));
    if !first {
        s = s.pt(24.0).child(el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(1.0).fill(p.background_modifier_accent));
    }
    s
}

pub(crate) fn not_available(p: &Palette) -> El {
    col()
        .w_full()
        .py(48.0)
        .gap(8.0)
        .items_center()
        .child(text("Not available yet").font(font(16.0, 600.0).lh(20.0)).color(p.text_primary))
        .child(
            text("Reflux doesn't support this part of settings yet.")
                .font(font(14.0, 400.0).lh(20.0))
                .color(p.text_primary_muted),
        )
}


#[cfg(test)]
mod tests {
    /// `buildUserSettingsDeepLink`: the tab, and the section when one is named.
    #[test]
    fn section_deep_links_match_fluxer() {
        assert_eq!(super::deep_link("voice_video", None), "fluxer://settings/user?tab=voice_video");
        assert_eq!(super::deep_link("voice_video", Some("audio")), "fluxer://settings/user?tab=voice_video&section=audio");
        assert_eq!(super::section_id("Audio"), "audio");
        assert_eq!(super::section_id("Video"), "video");
    }

    /// `buildThresholds`: reachable sections switch at their top less 24; the ones the scroll
    /// range can't reach share its end.
    #[test]
    fn section_thresholds_match_fluxer() {
        use super::spy::thresholds;
        assert_eq!(thresholds(&[0.0, 300.0, 600.0], 1000.0, 500.0), vec![-24.0, 276.0, 576.0]);
        // max 400: 676 and 876 are out of reach; span 476, zone min(476, 476, 400 - 276) = 124.
        let t = thresholds(&[0.0, 300.0, 700.0, 900.0], 400.0, 500.0);
        assert_eq!(&t[..2], &[-24.0, 276.0]);
        assert!((t[2] - (400.0 - 124.0 * (1.0 - 276.0 / 476.0))).abs() < 0.01 && (t[3] - 400.0).abs() < 0.01, "{t:?}");
        assert_eq!(thresholds(&[0.0, 300.0], 0.0, 500.0), vec![-24.0, 276.0], "nothing to scroll");
    }
}
