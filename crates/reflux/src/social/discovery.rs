//! Explore (`DiscoveryPage`, `DiscoveryGuildCard`), the guild preview nagbar shown while
//! visiting a community, and `InviteAcceptModal`.

use crate::ui::SmoothWheel;
use reflux_model::Snowflake;
use reflux_store::Store;
use reqwest::Method;
use rune::{El, ScrollSpec, col, el, icon, row, text};
use serde_json::{Value, json};

use super::{Done, body, close, desc, error};
use crate::icons;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Input, Modal as Dialog, scroller, spinner};
use crate::i18n::t;

const CATEGORIES: [&str; 9] = ["Gaming", "Music", "Entertainment", "Education", "Science & Technology", "Content Creator", "Anime & Manga", "Movies & TV", "Other"];

/// `DiscoverySupportedLanguages`: (code, English name).
const LANGUAGES: [(&str, &str); 33] = [
    ("ar", "Arabic"),
    ("bg", "Bulgarian"),
    ("cs", "Czech"),
    ("da", "Danish"),
    ("de", "German"),
    ("el", "Greek"),
    ("en-US", "English"),
    ("es-ES", "Spanish (Spain)"),
    ("es-419", "Spanish (Latin America)"),
    ("fi", "Finnish"),
    ("fr", "French"),
    ("he", "Hebrew"),
    ("hi", "Hindi"),
    ("hr", "Croatian"),
    ("hu", "Hungarian"),
    ("id", "Indonesian"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("lt", "Lithuanian"),
    ("nl", "Dutch"),
    ("no", "Norwegian"),
    ("pl", "Polish"),
    ("pt-BR", "Portuguese (Brazil)"),
    ("ro", "Romanian"),
    ("ru", "Russian"),
    ("sv-SE", "Swedish"),
    ("th", "Thai"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
    ("vi", "Vietnamese"),
    ("zh-CN", "Chinese (Simplified)"),
    ("zh-TW", "Chinese (Traditional)"),
];

/// The search request for the current query, category and language, from `offset`.
pub fn search_path_from(query: &str, category: Option<u8>, language: Option<&str>, offset: usize) -> String {
    let mut p = "/discovery/guilds?limit=24".to_string();
    if offset > 0 {
        p.push_str(&format!("&offset={offset}"));
    }
    let q = query.trim();
    if !q.is_empty() {
        p.push_str("&sort_by=relevance&query=");
        for b in q.bytes() {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                p.push(b as char);
            } else {
                p.push_str(&format!("%{b:02X}"));
            }
        }
    }
    if let Some(c) = category {
        p.push_str(&format!("&category={c}"));
    }
    if let Some(l) = language {
        p.push_str(&format!("&language={l}"));
    }
    p
}

/// The first page's request.
pub fn search_path(query: &str, category: Option<u8>, language: Option<&str>) -> String {
    search_path_from(query, category, language, 0)
}

/// Opens Explore and runs the search.
pub fn open(st: &mut ShellState) {
    st.social.discovery = true;
    st.channel = None;
    search(st);
}

fn search(st: &mut ShellState) {
    st.social.data.remove("discovery");
    let path = search_path(&st.social.discovery_query.text, st.social.discovery_category, st.social.discovery_language);
    st.social(Method::GET, path, None, Done::Data("discovery".into()));
}

/// `DiscoveryPage`'s "load more": the next page after what the grid shows.
fn load_more(st: &mut ShellState) {
    let Some(v) = st.social.data.get("discovery") else { return };
    let offset = v["guilds"].as_array().map_or(0, |g| g.len());
    let path = search_path_from(&st.social.discovery_query.text, st.social.discovery_category, st.social.discovery_language, offset);
    st.social(Method::GET, path, None, Done::AppendGuilds("discovery".into(), offset));
}

fn compact(n: u64) -> String {
    match n {
        0..1000 => n.to_string(),
        1000..1_000_000 => format!("{:.1}K", n as f64 / 1000.0).replace(".0K", "K"),
        _ => format!("{:.1}M", n as f64 / 1e6).replace(".0M", "M"),
    }
}

pub fn discovery_page<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let search_box = Input::new("discovery-search", &st.social.discovery_query, move |s: &mut S| -> &mut rune::TextEdit { &mut acc(s).social.discovery_query })
        .placeholder(t("Search communities"))
        .el(p)
        .w(360.0)
        .on_key(move |s: &mut S, k: &rune::KeyEvent, _: &mut rune::Tx| {
            if k.key == rune::Key::Enter {
                search(acc(s));
                return true;
            }
            false
        });
    let head = row()
        .w_full()
        .h(48.0)
        .px(16.0)
        .gap(8.0)
        .center_y()
        .child(rune::el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.background_header_secondary))
        .child(icon(&icons::COMPASS_FILL, 20.0).color(p.text_primary_muted))
        .child(text(t("Explore")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).grow(1.0))
        .child(language(st, p, acc, layer))
        .child(search_box);
    let pill = |label: &str, cat: Option<u8>| {
        let on = st.social.discovery_category == cat;
        row()
            .h(32.0)
            .px(12.0)
            .center()
            .radius(16.0)
            .fill(if on { p.brand_primary } else { p.background_secondary })
            .hover(if on { p.brand_primary } else { p.background_modifier_hover })
            .cursor(rune::Cursor::Pointer)
            .role("tab")
            .key(format!("discovery-cat-{}", cat.map_or(-1, |c| c as i32)))
            .child(text(label.to_string()).font(font(14.0, 500.0).lh(20.0)).color(if on { p.brand_primary_fill } else { p.text_primary }))
            .on_press(move |s: &mut S| {
                let st = acc(s);
                st.social.discovery_category = cat;
                search(st);
            })
    };
    let mut cats = row().w_full().gap(8.0).flex_wrap().child(pill(t("All"), None));
    for (i, c) in CATEGORIES.iter().enumerate() {
        cats = cats.child(pill(c, Some(i as u8)));
    }
    let results = match st.social.data.get("discovery") {
        None => row().w_full().h(300.0).center().child(spinner(p.text_primary_muted)),
        Some(v) => {
            let guilds = v["guilds"].as_array().cloned().unwrap_or_default();
            if guilds.is_empty() {
                let searching = !st.social.discovery_query.text.trim().is_empty() || st.social.discovery_category.is_some();
                col()
                    .w_full()
                    .min_h(300.0)
                    .center()
                    .gap(8.0)
                    .child(icon(&icons::COMPASS_FILL, 64.0).color(p.text_primary_muted))
                    .child(text(if searching { t("No communities match.") } else { t("Nothing to explore yet") }).font(font(20.0, 600.0).lh(28.0)).color(p.text_primary))
                    .child(
                        text(if searching { t("Try another search term, or pick a different category or language.") } else { t("No communities are listed right now. Check back a little later.") })
                            .font(font(14.0, 400.0).lh(20.0))
                            .color(p.text_primary_muted),
                    )
            } else {
                let total = v["total"].as_u64().unwrap_or(guilds.len() as u64);
                let mut grid = col().w_full().gap(16.0).child(text(if total == 1 { "1 community found".to_string() } else { format!("{total} communities found") }).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary_muted));
                for chunk in guilds.chunks(3) {
                    let mut r = row().w_full().gap(16.0).items_stretch();
                    for g in chunk {
                        r = r.child(card(st, g, p, acc));
                    }
                    for _ in chunk.len()..3 {
                        r = r.child(el().grow(1.0).w(0.0));
                    }
                    grid = grid.child(r);
                }
                if (guilds.len() as u64) < total {
                    grid = grid.child(
                        row().w_full().h(40.0).center().child(
                            Button::new(t("Load more"))
                                .key("discovery-more")
                                .variant(ButtonVariant::Secondary)
                                .size(ButtonSize::Medium)
                                .on_press(move |s: &mut S| load_more(acc(s)))
                                .el(p),
                        ),
                    );
                }
                grid
            }
        }
    };
    let content = col().w_full().padding(24.0).gap(24.0).child(cats).children(st.social.data.get("discovery_error").map(|e| text(e.as_str().unwrap_or_default().to_string()).color(p.status_danger))).child(results);
    col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .fill(p.background_secondary_lighter)
        .child(head)
        .child(col().w_full().grow(1.0).min_h(0.0).child(scroller(col().w_full().h_full().scroll(ScrollSpec { stick_bottom: false }).smooth_wheel::<S>("discovery-list").key("discovery-list").child(content), false, p)))
}

/// `DiscoveryLanguageDropdown`: "All languages" and the supported ones by name.
fn language<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let mut opts: Vec<(&str, &str)> = LANGUAGES.iter().map(|&(c, n)| (c, t(n))).collect();
    opts.sort_by_key(|o| o.1.to_lowercase());
    opts.insert(0, ("", t("All languages")));
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    crate::ui::Select::new("discovery-language", &opts, Some(st.social.discovery_language.unwrap_or("")))
        .width(200.0)
        .on_choose(move |s: &mut S, v: &str| {
            let st = acc(s);
            st.social.discovery_language = LANGUAGES.iter().find(|l| l.0 == v).map(|l| l.0);
            search(st);
        })
        .el(&st.popups, pops, p, layer)
}

/// `DiscoveryGuildCard`: banner strip, icon (or initials), name, description and counts; opens
/// a preview.
fn card<S: 'static>(st: &ShellState, g: &Value, p: &Palette, acc: ShellAccess<S>) -> El {
    let id: Snowflake = g["id"].as_str().and_then(|s| s.parse().ok()).unwrap_or_default();
    let name = g["name"].as_str().unwrap_or_default().to_string();
    let description = g["description"].as_str().filter(|d| !d.is_empty()).unwrap_or("No description.").to_string();
    let online = g["online_count"].as_u64().unwrap_or(0);
    let members = g["member_count"].as_u64().unwrap_or(0);
    let dot = |c| el().size(8.0, 8.0).radius(4.0).fill(c);
    let image = |hash: Option<&str>, url: &dyn Fn(&str, &str, f32) -> String| hash.map(|h| st.media.asset(|b, s| url(b, h, s)));
    let icon_px = image(g["icon"].as_str(), &|b, h, s| crate::media::guild_icon_url(b, id, h, 56.0, s));
    let banner_px = image(g["banner"].as_str(), &|b, h, s| crate::media::guild_banner_url(b, id, h, s));
    let face = row().size(56.0, 56.0).center().radius(16.0).clip_rounded(16.0).fill(p.background_primary).border(3.0, p.background_secondary);
    let face = match icon_px {
        Some(crate::media::Load::Ready(px)) => face.child(el().size(50.0, 50.0).radius(13.0).pixels_fit(&px, rune::SurfaceFit::Fill)),
        _ => face.child(text(crate::shell::acronym(&name)).font(font(18.0, 600.0)).color(p.text_primary)),
    };
    let banner = match banner_px {
        Some(crate::media::Load::Ready(px)) => el().w_full().h(120.0).pixels_fit(&px, rune::SurfaceFit::Fill),
        _ => el().w_full().h(120.0).fill(p.background_tertiary),
    };
    let visits = g["visits_enabled"].as_bool().unwrap_or(true);
    col()
        .grow(1.0)
        .w(0.0)
        .min_w(0.0)
        .radius(12.0)
        .clip_rounded(12.0)
        .fill(p.background_secondary)
        .hover(p.background_secondary_alt)
        .border(1.0, p.background_modifier_accent)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(crate::i18n::tf("Preview {communityName}", &[("communityName", &name.to_string())]))
        .key(format!("discovery-guild-{id}"))
        .on_press(move |s: &mut S| {
            let st = acc(s);
            if visits {
                st.social(Method::POST, format!("/discovery/guilds/{id}/visit"), None, Done::EnterGuild(id));
            } else {
                st.social(Method::POST, format!("/discovery/guilds/{id}/join"), None, Done::EnterGuild(id));
            }
        })
        .child(banner)
        .child(face.pin(Some(16.0), Some(92.0), None, None))
        .child(
            col()
                .w_full()
                .pt(40.0)
                .px(16.0)
                .pb(16.0)
                .gap(8.0)
                .grow(1.0)
                .child(text(name.clone()).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).truncate())
                .child(text(description).font(font(14.0, 400.0).lh(21.0)).color(p.text_secondary).wrap().max_lines(4).min_h(84.0))
                .child(
                    row()
                        .gap(16.0)
                        .center_y()
                        .child(row().gap(6.0).center_y().child(dot(p.status_online)).child(text(crate::i18n::tf("{onlineCount} online", &[("onlineCount", &(compact(online)).to_string())])).font(font(12.0, 500.0).lh(16.0)).color(p.text_primary_muted)))
                        .child(row().gap(6.0).center_y().child(dot(p.text_tertiary)).child(text(if members == 1 { "1 member".to_string() } else { format!("{} members", compact(members)) }).font(font(12.0, 500.0).lh(16.0)).color(p.text_primary_muted))),
                ),
        )
}

/// While visiting a community: "Join" or "Stop previewing".
pub fn preview_nagbar<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let g = st.guild?;
    let gs = store.guilds.get(&g).filter(|gs| gs.visiting)?;
    let name = gs.info.as_ref().map_or(String::new(), |i| i.name.clone());
    let join = Button::new(t("Join"))
        .variant(ButtonVariant::Inverted)
        .size(ButtonSize::SuperCompact)
        .submitting(st.form.busy && st.dialog.is_none())
        .key("nagbar-visit-join")
        .on_press(move |s: &mut S| acc(s).social(Method::POST, format!("/users/@me/guild-visits/{g}/join"), None, Done::Nothing))
        .el(p);
    let stop = Button::new(t("Stop previewing"))
        .variant(ButtonVariant::Secondary)
        .size(ButtonSize::SuperCompact)
        .key("nagbar-visit-stop")
        .on_press(move |s: &mut S| {
            let st = acc(s);
            st.social(Method::DELETE, format!("/users/@me/guild-visits/{g}"), None, Done::Nothing);
            st.select_guild(None, None);
        })
        .el(p);
    Some(
        row()
            .w_full()
            .min_h(36.0)
            .py(4.0)
            .px(8.0)
            .gap(12.0)
            .center()
            .fill(p.brand_primary)
            .role("status")
            .child(text(format!("You're previewing {name}. Join to chat and get notifications.")).font(font(14.0, 600.0).lh(20.0)).color(rune::Color::WHITE).wrap())
            .child(join)
            .child(stop),
    )
}

/// `InviteAcceptModal`: the invite preview (`form.data["invite"]`), then Join.
pub fn invite_accept<S: 'static>(_store: &Store, st: &ShellState, code: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let inv = st.form.data.get("invite").cloned();
    let Some(inv) = inv else {
        let mut items = vec![if st.form.error.is_some() { desc("This invite may be expired, or you might not have permission to join.", p) } else { row().w_full().h(80.0).center().child(spinner(p.text_primary_muted)) }];
        items.extend(error(st, p));
        return Dialog::new(t("Join community"), close(acc)).body(body(items)).el(p);
    };
    let name = inv["guild"]["name"].as_str().or(inv["channel"]["name"].as_str()).unwrap_or("this community").to_string();
    let by = inv["inviter"]["global_name"].as_str().or(inv["inviter"]["username"].as_str()).map(str::to_string);
    let members = inv["member_count"].as_u64();
    let online = inv["presence_count"].as_u64();
    let mut items = vec![col()
        .w_full()
        .gap(12.0)
        .items_center()
        .child(row().size(80.0, 80.0).center().radius(24.0).fill(p.background_tertiary).child(text(crate::shell::acronym(&name)).font(font(24.0, 600.0)).color(p.text_primary)))
        .children(by.map(|b| text(format!("{b} invited you to join")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted)))
        .child(text(name.clone()).font(font(24.0, 600.0).lh(30.0)).color(p.text_primary))
        .child(
            row()
                .gap(16.0)
                .children(online.map(|o| text(crate::i18n::tf("{onlineCount} online", &[("onlineCount", &(compact(o)).to_string())])).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary_muted)))
                .children(members.map(|m| text(format!("{} members", compact(m))).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary_muted))),
        )];
    items.extend(error(st, p));
    let code = code.to_string();
    let join = Button::new(t("Join community")).submitting(st.form.busy).key("invite-accept").on_press(move |s: &mut S| acc(s).social(Method::POST, format!("/invites/{code}"), Some(json!({})), Done::Enter)).el(p);
    Dialog::new(t("Join community"), close(acc)).body(body(items)).footer(row().w_full().child(join.grow(1.0))).el(p)
}

/// The code of an invite link on this instance (`{invite}/code` or `{webapp}/invite/code`).
pub fn invite_code(invite_base: &str, web_base: &str, url: &str) -> Option<String> {
    link_code(invite_base, web_base, "/invite", url)
}

/// The code of a gift link on this instance (`{gift}/code` or `{webapp}/gift/code`).
pub fn gift_code(gift_base: &str, web_base: &str, url: &str) -> Option<String> {
    link_code(gift_base, web_base, "/gift", url)
}

fn link_code(base: &str, web_base: &str, web_path: &str, url: &str) -> Option<String> {
    let strip = |base: &str, extra: &str| {
        let b = base.trim_end_matches('/');
        (!b.is_empty()).then(|| url.strip_prefix(&format!("{b}{extra}/"))).flatten()
    };
    let rest = strip(base, "").or_else(|| strip(web_base, web_path))?;
    let code = rest.split(['?', '#', '/']).next()?;
    (!code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then(|| code.to_string())
}

/// Opens the invite modal and loads its preview.
pub fn open_invite(st: &mut ShellState, code: String) {
    st.open_social(super::Modal::InviteAccept(code.clone()));
    st.social(Method::GET, format!("/invites/{code}?with_counts=true"), None, Done::Form("invite".into()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_paths_encode_the_query() {
        assert_eq!(search_path("", None, None), "/discovery/guilds?limit=24");
        assert_eq!(search_path(" rust & go ", Some(4), Some("de")), "/discovery/guilds?limit=24&sort_by=relevance&query=rust%20%26%20go&category=4&language=de");
        assert_eq!(search_path_from("rust", None, None, 24), "/discovery/guilds?limit=24&offset=24&sort_by=relevance&query=rust");
        assert_eq!(search_path_from("", Some(2), None, 0), "/discovery/guilds?limit=24&category=2");
        assert_eq!(compact(1500), "1.5K");
        assert_eq!(compact(2000), "2K");
        assert_eq!(invite_code("https://fluxer.gg", "https://web.fluxer.app", "https://fluxer.gg/AbC-1"), Some("AbC-1".into()));
        assert_eq!(invite_code("https://fluxer.gg", "https://web.fluxer.app", "https://web.fluxer.app/invite/xyz?x=1"), Some("xyz".into()));
        assert_eq!(invite_code("https://fluxer.gg", "https://web.fluxer.app", "https://example.com/xyz"), None);
        assert_eq!(invite_code("", "", "https://example.com/xyz"), None);
        assert_eq!(gift_code("https://fluxer.gift", "https://web.fluxer.app", "https://web.fluxer.app/gift/G1ft"), Some("G1ft".into()));
    }
}
