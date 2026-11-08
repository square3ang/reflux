//! Invite links in a message as cards (`channel/components/InviteEmbed.tsx` on `EmbedCard`).

use reflux_model::{Invite, Message, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, col, el, icon, row, text};

use super::{Intent, ShellAccess, ShellState};
use crate::icons;
use crate::media::{self, Load};
use crate::theme::{Palette, font};
use crate::ui::{Button, over, rgb};
use crate::i18n::t;

/// An invite's preview while it loads, once loaded, or when it doesn't resolve.
pub enum Preview {
    Loading,
    Ready(Box<Invite>),
    Unknown,
}

/// `OFFICIAL_INVITE_URL_BASES`, host + path.
const OFFICIAL: [&str; 8] =
    ["fluxer.app/invite", "canary.fluxer.app/invite", "web.fluxer.app/invite", "web.canary.fluxer.app/invite", "fluxer.com/invite", "canary.fluxer.com/invite", "fluxer.gg", "fluxer.gg/invite"];

fn strip_scheme(base: &str) -> &str {
    let b = base.trim().trim_end_matches('/');
    b.strip_prefix("https://").or_else(|| b.strip_prefix("http://")).unwrap_or(b)
}

/// `findInvites`: up to ten distinct codes after an invite base (`host[/#]/path/CODE`), skipping
/// links wrapped in `<…>`.
pub fn find_codes(content: &str, invite_base: &str, web_base: &str) -> Vec<String> {
    let web = format!("{}/invite", strip_scheme(web_base));
    let mut bases: Vec<String> = [strip_scheme(invite_base).to_owned(), web].into_iter().filter(|b| !b.is_empty() && b != "/invite").collect();
    bases.extend(OFFICIAL.iter().map(|b| b.to_string()));
    // Longest first, as the regex alternation orders them.
    bases.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
    bases.dedup();
    let lower = content.to_ascii_lowercase();
    let code_char = |c: u8| c.is_ascii_alphanumeric() || c == b'-';
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lower.len() && out.len() < 10 {
        let rest = &lower[i..];
        let hit = bases.iter().find_map(|b| {
            let (host, path) = b.split_once('/').map_or((b.as_str(), ""), |(h, p)| (h, p));
            let mut r = rest.strip_prefix(host)?;
            r = r.strip_prefix("/#").unwrap_or(r);
            if !path.is_empty() {
                r = r.strip_prefix('/')?.strip_prefix(path)?;
            }
            let r = r.strip_prefix('/')?;
            let n = r.bytes().take_while(|&c| code_char(c)).count();
            (2..=32).contains(&n).then_some((rest.len() - r.len(), n))
        });
        let Some((off, n)) = hit else {
            i += rest.chars().next().map_or(1, char::len_utf8);
            continue;
        };
        // The match may start at an `https://` just before.
        let start = ["https://", "http://"].iter().find(|s| lower[..i].ends_with(*s)).map_or(i, |s| i - s.len());
        let end = i + off + n;
        let wrapped = content[..start].ends_with('<') && content[end..].starts_with('>');
        let code = &content[i + off..end];
        if !wrapped && !out.iter().any(|c| c == code) {
            out.push(code.to_owned());
        }
        i = end;
    }
    out
}

/// The cards for the invites in `m` ([`request`] fetches them).
pub fn embeds<S: 'static>(store: &Store, st: &ShellState, m: &Message, p: &Palette, acc: ShellAccess<S>) -> Vec<El> {
    find_codes(&m.content, &st.invite_base, &st.web_base)
        .into_iter()
        .map(|code| match st.media_ui.invites.get(&code) {
            None | Some(Preview::Loading) => loading(p),
            Some(Preview::Unknown) => unknown(p),
            Some(Preview::Ready(inv)) => ready(store, st, inv, p, acc),
        })
        .map(|card| menu_on(card, m.id, find_ids(st, &m.content), acc))
        .collect()
}

/// The (guild, channel) of the first loaded invite in `content`, for the card's menu.
fn find_ids(st: &ShellState, content: &str) -> (Option<Snowflake>, Option<Snowflake>) {
    find_codes(content, &st.invite_base, &st.web_base)
        .iter()
        .find_map(|c| match st.media_ui.invites.get(c) {
            Some(Preview::Ready(inv)) => Some((inv.guild.as_ref().map(|g| g.id), inv.channel.as_ref().map(|c| c.id).or(inv.channel_id))),
            _ => None,
        })
        .unwrap_or_default()
}

/// `InviteEmbedContextMenu`: the message's menu plus "Copy community ID" / "Copy channel ID".
fn menu_on<S: 'static>(card: El, msg: Snowflake, ids: (Option<Snowflake>, Option<Snowflake>), acc: ShellAccess<S>) -> El {
    card.on_secondary_press(move |s: &mut S, at, _| {
        let st = acc(s);
        st.popups.open_at(&format!("msg-menu-{msg}"), [at.x, at.y]);
        st.active_msg = Some(msg);
        st.media_ui.invite_menu = ids;
    })
}

/// The invite items of the message menu opened on a card.
pub fn menu_items<S: 'static>(st: &ShellState) -> Vec<crate::ui::MenuEntry<S>> {
    let copy = |key: &str, label: &str, id: Snowflake| crate::ui::MenuEntry::Item(crate::ui::MenuItem::new(key.to_string(), label).icon(&icons::SNOWFLAKE).on_select_tx(move |_: &mut S, tx: &mut rune::Tx| tx.copy(id.to_string())));
    let (g, c) = st.media_ui.invite_menu;
    g.map(|g| copy("invite-copy-guild", t("Copy community ID"), g)).into_iter().chain(c.map(|c| copy("invite-copy-channel", t("Copy channel ID"), c))).collect()
}

/// `EmbedCard`: an optional splash (60 tall, or the image's clamped 16:9…16:18 ratio), icon slot
/// and header, then the full-width button under a divider.
fn card(splash: Option<El>, icon_el: El, header: El, footer: Option<El>, p: &Palette) -> El {
    let border = over(p.background_modifier_accent, p.background_secondary);
    let grid = row().w_full().padding(13.6).gap(14.4).center_y().child(row().w(56.0).center().child(icon_el)).child(col().grow(1.0).min_w(0.0).gap(2.4).child(header));
    let mut c = col().w_full().max_w(360.0).radius(12.0).clip_rounded(12.0).fill(p.background_secondary).border(1.0, border).children(splash).child(grid);
    if let Some(f) = footer {
        c = c.child(el().w_full().h(1.0).fill(border)).child(el().w_full().padding(13.6).child(f));
    }
    c.role("group")
}

fn title(s: &str, color: rune::Color) -> El {
    text(s.to_owned()).font(font(16.0, 600.0).lh(20.0)).color(color).truncate()
}

/// `.statText`: 0.82em, `text-tertiary`.
fn stat(dot: Option<rune::Color>, s: String, p: &Palette) -> El {
    let t = text(s).font(font(13.12, 400.0).lh(15.74)).color(p.text_tertiary);
    match dot {
        Some(c) => row().center_y().child(el().size(8.0, 8.0).radius(4.0).fill(c).mr(4.8)).child(t),
        None => t,
    }
}

fn skeleton(w: f32, h: f32, r: f32, p: &Palette) -> El {
    el().size(w, h).radius(r).fill(if crate::ui::is_light(p) { rgb(209, 213, 219) } else { rgb(75, 85, 99) })
}

/// `InviteLoadingState`.
fn loading(p: &Palette) -> El {
    let stats = row().gap(9.6).child(row().gap(4.8).center_y().child(skeleton(8.0, 8.0, 4.0, p)).child(skeleton(64.0, 12.0, 4.0, p))).child(row().gap(4.8).center_y().child(skeleton(8.0, 8.0, 4.0, p)).child(skeleton(96.0, 12.0, 4.0, p)));
    card(None, skeleton(48.0, 48.0, 24.0, p), col().gap(6.4).child(skeleton(160.0, 18.0, 4.0, p)).child(stats), Some(skeleton(330.0, 40.0, 8.0, p).w_full()), p)
}

/// `InviteNotFoundError`.
fn unknown(p: &Palette) -> El {
    let face = row().size(48.0, 48.0).center().radius(24.0).fill(p.background_tertiary).child(icon(&icons::QUESTION_FILL, 28.0).color(p.text_tertiary));
    let header = col().gap(2.4).child(title(t("Unknown invite"), p.status_danger)).child(text(t("Try asking for a new invite.")).font(font(12.8, 400.0).lh(15.36)).color(p.text_tertiary));
    let button = Button::<()>::new(t("Invite unavailable")).fit_container().disabled(true).el(p);
    card(None, face, header, Some(button), p)
}

fn ready<S: 'static>(store: &Store, st: &ShellState, inv: &Invite, p: &Palette, acc: ShellAccess<S>) -> El {
    // `formatNumber` (en-US grouping).
    let count = |n: u32| {
        let d = n.to_string();
        d.chars().enumerate().fold(String::new(), |mut o, (i, c)| {
            if i > 0 && (d.len() - i).is_multiple_of(3) {
                o.push(',');
            }
            o.push(c);
            o
        })
    };
    let mut splash = None;
    let mut badge = None;
    let mut paused = None;
    let (face, name, stats, label, disabled) = match &inv.guild {
        Some(g) => {
            let load = g.icon.as_deref().map(|h| st.media.asset(|b, s| media::guild_icon_url(b, g.id, h, 48.0, s)));
            let face = match load {
                Some(Load::Ready(px)) => el().size(48.0, 48.0).radius(24.0).pixels_fit(&px, rune::SurfaceFit::Fill),
                Some(Load::Pending) => el().size(48.0, 48.0).radius(24.0).fill(p.background_tertiary),
                _ => super::guilds::acronym_face(&g.name, 48.0, false, p),
            };
            if let Some(h) = g.embed_splash.as_deref() {
                // `clampWideAssetAspectRatio`: 16:9 between half and full height.
                let ratio = match (g.embed_splash_width, g.embed_splash_height) {
                    (Some(w), Some(h)) if w > 0 && h > 0 => Some((w as f32 / h as f32).clamp(16.0 / 9.0, 32.0 / 9.0)),
                    _ => None,
                };
                let height = ratio.map_or(60.0, |r| (358.0 / r).max(60.0));
                let s = el().w_full().h(height).fill(p.background_tertiary);
                splash = Some(match st.media.asset(|b, sc| media::guild_wide_url(b, "embed-splashes", g.id, h, sc)) {
                    Load::Ready(px) => s.pixels_fit(&px, rune::SurfaceFit::Fill),
                    _ => s,
                });
            }
            badge = guild_badge(&g.features, &g.badge_ids, st, p, acc);
            let members = inv.member_count.unwrap_or(0);
            let stats = row()
                .gap(9.6)
                .child(stat(Some(p.status_online), crate::i18n::tf("{onlineCount} online", &[("onlineCount", &(count(inv.presence_count.unwrap_or(0))).to_string())]), p))
                .child(stat(Some(p.text_tertiary_secondary), format!("{} {}", count(members), if members == 1 { "member" } else { "members" }), p));
            // `getGuildInvitePrimaryAction`.
            let member = store.guilds.get(&g.id).is_some_and(|gs| !gs.visiting);
            let has = |f: &str| g.features.iter().any(|x| x == f);
            let label = if has("INVITES_DISABLED") && !member {
                paused = Some(if has("RAID_DETECTED") { crate::i18n::tf("{productName} detected a potential raid, so new users can't join right now.", &[("productName", "Fluxer")]) } else { t("Invites are paused for this community.").to_string() });
                t("Invites disabled")
            } else if member {
                t("Go to community")
            } else if g.visits_enabled == Some(true) {
                t("Preview community")
            } else {
                t("Join community")
            };
            (face, g.name.clone(), stats, label, paused.is_some())
        }
        None => {
            // A group DM invite: the inviter's face and the group's name.
            let ch = inv.channel.as_ref();
            let name = ch.and_then(|c| c.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| {
                let names: Vec<&str> = ch.map(|c| c.recipients.iter().map(|r| r.username.as_str()).collect()).unwrap_or_default();
                if names.is_empty() { "Unnamed group".into() } else { names.join(", ") }
            });
            let face = match &inv.inviter {
                Some(u) => super::avatar(store, st, u.id, 48.0, None, p.background_secondary, p),
                None => el().size(48.0, 48.0).radius(24.0).fill(p.background_tertiary),
            };
            let members = inv.member_count.unwrap_or(0);
            let stats = row().child(stat(Some(p.text_tertiary_secondary), format!("{} {}", count(members), if members == 1 { "Member" } else { t("Members") }), p));
            let joined = inv.channel_id.or(ch.map(|c| c.id)).is_some_and(|c| store.channels.contains_key(&c));
            (face, name, stats, if joined { t("Already joined") } else { t("Join group") }, joined)
        }
    };
    let (code, guild, channel) = (inv.code.clone(), inv.guild.as_ref().map(|g| g.id), inv.channel.as_ref().map(|c| c.id));
    let member = guild.and_then(|g| store.guilds.get(&g)).is_some_and(|gs| !gs.visiting);
    let go = move |s: &mut S| {
        let sh = acc(s);
        match guild {
            Some(g) if member => sh.select_guild(Some(g), channel),
            _ => sh.api(Method::POST, format!("/invites/{code}"), None, crate::guild::Then::Enter),
        }
    };
    let button = Button::new(label).fit_container().disabled(disabled).key(format!("invite-{}", inv.code)).on_press(go).el(p);
    let mut stats = col().gap(4.0).child(stats);
    if let Some(msg) = paused {
        stats = stats.child(text(msg).font(font(13.12, 400.0).lh(16.0)).color(p.text_tertiary).wrap());
    }
    let header = col().gap(4.0).child(row().gap(4.0).center_y().child(title(&name, p.text_primary).shrink()).children(badge)).child(stats);
    card(splash, face, header, Some(button), p)
}

/// `DEFAULT_VISIBLE_CUSTOM_BADGES`.
const VISIBLE_CUSTOM: usize = 3;

/// `customBadgeImageUrl`.
fn badge_image(st: &ShellState, asset_url: &str) -> Option<rune::Pixels> {
    match st.media.get(&super::badges::custom_url(asset_url)) {
        Load::Ready(px) => Some(px),
        _ => None,
    }
}

/// `GuildBadgeDefinitions.resolve`: the custom badges among `ids`, from discovery's set or a
/// lookup ([`request`]), by `position` and then discovery order; `None` until all are known.
fn custom_badges(st: &ShellState, ids: &[String]) -> Option<Vec<reflux_model::GuildBadge>> {
    let mu = &st.media_ui;
    let fetched: Vec<reflux_model::GuildBadge> = ids_key(st, ids)
        .and_then(|k| match st.expr.fetched.get(&k) {
            Some(super::expr::Fetched::Done(v)) => serde_json::from_value(v["badges"].clone()).ok(),
            _ => None,
        })
        .unwrap_or_default();
    let mut found = Vec::new();
    let mut settled = true;
    for id in ids {
        match mu.guild_badges.iter().chain(&fetched).find(|b| b.id.to_string() == *id) {
            Some(b) if !found.contains(b) => found.push(b.clone()),
            Some(_) => {}
            None => settled &= mu.badges_version.is_none() || ids_key(st, ids).is_some_and(|k| st.expr.fetched.get(&k).is_some_and(|f| !matches!(f, super::expr::Fetched::Loading))),
        }
    }
    let index = |b: &reflux_model::GuildBadge| mu.guild_badges.iter().position(|x| x.id == b.id).unwrap_or(usize::MAX);
    let pos = |b: &reflux_model::GuildBadge| b.position.as_deref().and_then(|p| p.parse::<i64>().ok());
    found.sort_by(|a, b| match (pos(a), pos(b)) {
        (Some(x), Some(y)) => x.cmp(&y).then(a.id.cmp(&b.id)),
        _ => index(a).cmp(&index(b)).then(a.id.cmp(&b.id)),
    });
    settled.then_some(found)
}

/// The lookup key for the ids discovery doesn't list, if any (and the instance versions badges).
fn ids_key(st: &ShellState, ids: &[String]) -> Option<String> {
    st.media_ui.badges_version.as_ref()?;
    let unknown: Vec<&str> = ids.iter().filter(|id| !st.media_ui.guild_badges.iter().any(|b| b.id.to_string() == **id)).map(String::as_str).collect();
    (!unknown.is_empty()).then(|| format!("badges:{}", unknown.join(",")))
}

/// `GuildBadge` (`large`, `linkable`, `onLightSurface`): the built-in badge (partnered over
/// verified over discoverable, or the instance's replacement image), then up to three custom
/// badges and a `+N` pill opening `GuildBadgesModal`; each has its tooltip.
pub fn guild_badge<S: 'static>(features: &[String], badge_ids: &[String], st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    guild_badge_sized(features, badge_ids, 24.0, VISIBLE_CUSTOM, true, st, p, acc)
}

/// `GuildBadge` at `size` (24 `large`, else 20) showing `visible` custom badges;
/// `light_surface` = `onLightSurface` (the light theme swaps seal and glyph colours).
#[allow(clippy::too_many_arguments)]
pub fn guild_badge_sized<S: 'static>(features: &[String], badge_ids: &[String], size: f32, visible: usize, light_surface: bool, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let has = |f: &str| features.iter().any(|x| x == f) && !st.media_ui.hidden_badges.iter().any(|h| h == &f.to_ascii_lowercase());
    let (verified, partnered) = (has("VERIFIED"), has("PARTNERED"));
    let built = if partnered {
        Some((&PARTNERED, if verified { t("Verified & partnered community") } else { t("Partnered community") }, &st.media_ui.badge_images.partnered))
    } else if verified {
        Some((&VERIFIED, t("Verified community"), &st.media_ui.badge_images.verified))
    } else if has("DISCOVERABLE") {
        Some((&DISCOVERABLE, t("Listed in Explore"), &st.media_ui.badge_images.discoverable))
    } else {
        None
    };
    let tip = move |key: String, content: El, trigger: El| super::badges::tip(key, content, 190.0, trigger, acc);
    // `.badgeOnLightSurface`: the light theme swaps seal and glyph colours.
    let (seal, glyph_c) = if light_surface && crate::ui::is_light(p) { (p.brand_primary, p.brand_primary_fill) } else { (p.brand_primary_fill, p.brand_primary) };
    let mut items = Vec::new();
    if let Some((glyph, label, replacement)) = built {
        let face = match replacement.as_deref() {
            Some(url) => match badge_image(st, url) {
                Some(px) => el().size(size, size).pixels_fit(&px, rune::SurfaceFit::Fit),
                None => el().size(size, size),
            },
            None => el().size(size, size).child(icon(&SEAL, size).color(seal)).child(icon(glyph, size).color(glyph_c).absolute(0.0, 0.0)),
        };
        items.push(tip(format!("guild-badge-{}-builtin", badge_ids.join("-")), super::badges::label_tip(label, p), face.label(label)));
    }
    let custom = custom_badges(st, badge_ids).unwrap_or_default();
    for b in custom.iter().take(visible) {
        let face = match badge_image(st, &b.asset_url) {
            Some(px) => el().size(size, size).pixels_fit(&px, rune::SurfaceFit::Fit),
            None => el().size(size, size),
        };
        let mut face = face.label(&b.name);
        if let Some(href) = b.href.clone() {
            face = face.cursor(rune::Cursor::Pointer).on_press_tx(move |s: &mut S, tx| super::links::open(acc(s), &href, tx));
        }
        // `.customBadgeTooltip`: the name in 600, the description under it.
        let line = |s: &str, w: f32| text(s.to_string()).font(font(13.0, w).lh(18.0)).color(p.text_primary).align(rune::Align::Center).wrap();
        let content = match &b.description {
            Some(d) => col().max_w(256.0).gap(2.0).py(7.0).px(10.0).child(line(&b.name, 600.0)).child(line(d, 500.0)),
            None => super::badges::label_tip(&b.name, p),
        };
        items.push(tip(format!("guild-badge-{}", b.id), content, face));
    }
    if custom.len() > visible {
        let all = custom.iter().map(super::badges::from_guild).collect();
        let pill = super::badges::more_pill(custom.len() - visible, 20.0, 20.0, 10.0, all, p, acc);
        items.push(tip(format!("guild-badge-{}-more", badge_ids.join("-")), super::badges::label_tip(t("Show all badges"), p), pill));
    }
    match items.len() {
        0 => None,
        1 => items.pop(),
        _ => Some(row().gap(4.0).center_y().children(items)),
    }
}

static SEAL: rune::Icon = rune::Icon { name: "guild-badge-seal", view: 256.0, body: r#"<path d="M240 128C240 138.44 232.49 146.27 225.86 153.18C222.09 157.12 218.19 161.18 216.72 164.75C215.36 168.02 215.28 173.44 215.2 178.69C215.05 188.45 214.89 199.51 207.2 207.2C199.51 214.89 188.45 215.05 178.69 215.2C173.44 215.28 168.02 215.36 164.75 216.72C161.18 218.19 157.12 222.09 153.18 225.86C146.27 232.49 138.44 240 128 240C117.56 240 109.73 232.49 102.82 225.86C98.88 222.09 94.82 218.19 91.25 216.72C87.98 215.36 82.56 215.28 77.31 215.2C67.55 215.05 56.49 214.89 48.8 207.2C41.11 199.51 40.95 188.45 40.8 178.69C40.72 173.44 40.64 168.02 39.28 164.75C37.81 161.18 33.91 157.12 30.14 153.18C23.51 146.27 16 138.44 16 128C16 117.56 23.51 109.73 30.14 102.82C33.91 98.88 37.81 94.82 39.28 91.25C40.64 87.98 40.72 82.56 40.8 77.31C40.95 67.55 41.11 56.49 48.8 48.8C56.49 41.11 67.55 40.95 77.31 40.8C82.56 40.72 87.98 40.64 91.25 39.28C94.82 37.81 98.88 33.91 102.82 30.14C109.73 23.51 117.56 16 128 16C138.44 16 146.27 23.51 153.18 30.14C157.12 33.91 161.18 37.81 164.75 39.28C168.02 40.64 173.44 40.72 178.69 40.8C188.45 40.95 199.51 41.11 207.2 48.8C214.89 56.49 215.05 67.55 215.2 77.31C215.28 82.56 215.36 87.98 216.72 91.25C218.19 94.82 222.09 98.88 225.86 102.82C232.49 109.73 240 117.56 240 128Z"/>"# };
static VERIFIED: rune::Icon = rune::Icon { name: "guild-badge-verified", view: 256.0, body: r#"<path d="M185.655 109.455L121.455 173.655C119.955 175.154 117.921 175.996 115.8 175.996C113.679 175.996 111.645 175.154 110.145 173.655L74.3349 137.655C72.8373 136.155 71.9961 134.122 71.9961 132.003C71.9961 129.883 72.8373 127.85 74.3349 126.35L84.3349 116.35C85.8342 114.857 87.864 114.019 89.9799 114.019C92.0958 114.019 94.1256 114.857 95.6249 116.35L116 136.11L164.38 88.325C165.879 86.8304 167.91 85.9912 170.027 85.9912C172.145 85.9912 174.175 86.8304 175.675 88.325L185.65 98.095C186.4 98.8384 186.996 99.7229 187.402 100.698C187.809 101.672 188.018 102.718 188.019 103.774C188.019 104.83 187.811 105.876 187.405 106.851C186.999 107.826 186.404 108.711 185.655 109.455Z"/>"# };
static PARTNERED: rune::Icon = rune::Icon { name: "guild-badge-partnered", view: 256.0, body: r#"<path d="M112.732 144.785L110.823 146.945C106.987 150.791 102.1 153.41 96.7796 154.471C91.4591 155.533 85.9441 154.988 80.9322 152.907C75.9203 150.826 71.6365 147.301 68.6226 142.778C65.6087 138.256 64 132.939 64 127.5C64 122.061 65.6087 116.744 68.6226 112.222C71.6365 107.699 75.9203 104.175 80.9322 102.093C85.9441 100.012 91.4591 99.4673 96.7796 100.529C102.1 101.59 106.987 104.209 110.823 108.055L145.177 146.945C149.013 150.791 153.9 153.41 159.22 154.471C164.541 155.533 170.056 154.988 175.068 152.907C180.08 150.826 184.363 147.301 187.377 142.778C190.391 138.256 192 132.939 192 127.5C192 122.061 190.391 116.744 187.377 112.222C184.363 107.699 180.08 104.175 175.068 102.093C170.056 100.012 164.541 99.4673 159.22 100.529C153.9 101.59 149.013 104.209 145.177 108.055L143.268 110.215" fill="none" stroke="currentColor" stroke-width="16" stroke-linecap="round" stroke-linejoin="round"/>"# };
static DISCOVERABLE: rune::Icon = rune::Icon { name: "guild-badge-discoverable", view: 256.0, body: r#"<path d="M184 168H176V132L177.17 133.17C177.922 133.921 178.941 134.342 180.004 134.341C181.066 134.34 182.084 133.917 182.835 133.165C183.586 132.413 184.007 131.394 184.006 130.332C184.005 129.269 183.582 128.251 182.83 127.5L133.655 78.3401C132.155 76.8409 130.121 75.9988 128 75.9988C125.879 75.9988 123.845 76.8409 122.345 78.3401L73.17 127.5C72.4201 128.251 71.9991 129.268 71.9995 130.329C72 131.39 72.4219 132.408 73.1725 133.158C73.9231 133.907 74.9408 134.328 76.0018 134.328C77.0628 134.328 78.0801 133.906 78.83 133.155L80 132V168H72C70.9391 168 69.9217 168.421 69.1716 169.172C68.4214 169.922 68 170.939 68 172C68 173.061 68.4214 174.078 69.1716 174.828C69.9217 175.579 70.9391 176 72 176H184C185.061 176 186.078 175.579 186.828 174.828C187.579 174.078 188 173.061 188 172C188 170.939 187.579 169.922 186.828 169.172C186.078 168.421 185.061 168 184 168ZM140 168H116V144C116 143.47 116.211 142.961 116.586 142.586C116.961 142.211 117.47 142 118 142H138C138.53 142 139.039 142.211 139.414 142.586C139.789 142.961 140 143.47 140 144V168Z"/>"# };

/// Fetches the invites linked in the open channel's loaded messages once each, and the custom
/// badges their communities carry that discovery doesn't list (`GuildBadgeDefinitions.request`).
pub fn request(store: &Store, st: &mut ShellState) -> bool {
    let lookups: Vec<(String, String)> = st
        .media_ui
        .invites
        .values()
        .filter_map(|v| match v {
            Preview::Ready(inv) => inv.guild.as_ref().map(|g| g.badge_ids.clone()),
            _ => None,
        })
        .chain(st.guild.and_then(|g| Some(store.guilds.get(&g)?.info.as_ref()?.badge_ids.clone())))
        .filter_map(|ids| {
            let key = ids_key(st, &ids)?;
            let version = st.media_ui.badges_version.clone()?;
            let path = format!("/instance/badges?ids={}&version={}", super::expr::enc(&key["badges:".len()..]), super::expr::enc(&version));
            Some((key, path))
        })
        .collect();
    for (key, path) in lookups {
        super::expr::fetch(st, &key, &path);
    }
    let Some(channel) = st.channel else { return false };
    let mut asked = false;
    for m in store.messages(channel) {
        for code in find_codes(&m.content, &st.invite_base, &st.web_base) {
            if !st.media_ui.invites.contains_key(&code) {
                st.media_ui.invites.insert(code.clone(), Preview::Loading);
                st.intents.push(Intent::FetchInvite(code));
                asked = true;
            }
        }
    }
    asked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_codes_follow_fluxer_bases() {
        let f = |s: &str| find_codes(s, "https://fluxer.gg", "https://web.fluxer.app");
        assert_eq!(f("join https://fluxer.gg/abc123 now"), ["abc123"]);
        assert_eq!(f("fluxer.gg/invite/Xy-z and web.fluxer.app/invite/Xy-z"), ["Xy-z"]);
        assert_eq!(f("<https://fluxer.gg/hidden>"), Vec::<String>::new());
        assert_eq!(f("https://example.com/abc https://fluxer.gg/a"), Vec::<String>::new());
        assert_eq!(f(&format!("fluxer.gg/{}", "a".repeat(33))), Vec::<String>::new());
    }
}
