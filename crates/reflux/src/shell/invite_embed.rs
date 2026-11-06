//! Invite links in a message as cards (`channel/components/InviteEmbed.tsx` on `EmbedCard`).

use reflux_model::{Invite, Message};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, col, el, icon, row, text};

use super::{Intent, ShellAccess, ShellState};
use crate::icons;
use crate::media::{self, Load};
use crate::theme::{Palette, font};
use crate::ui::{Button, over, rgb};

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
        .collect()
}

/// `EmbedCard`: icon slot and header, then the full-width button under a divider.
fn card(icon_el: El, header: El, footer: Option<El>, p: &Palette) -> El {
    let border = over(p.background_modifier_accent, p.background_secondary);
    let grid = row().w_full().padding(13.6).gap(14.4).center_y().child(row().w(56.0).center().child(icon_el)).child(col().grow(1.0).min_w(0.0).gap(2.4).child(header));
    let mut c = col().w_full().max_w(360.0).radius(12.0).clip_rounded(12.0).fill(p.background_secondary).border(1.0, border).child(grid);
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
    card(skeleton(48.0, 48.0, 24.0, p), col().gap(6.4).child(skeleton(160.0, 18.0, 4.0, p)).child(stats), Some(skeleton(330.0, 40.0, 8.0, p).w_full()), p)
}

/// `InviteNotFoundError`.
fn unknown(p: &Palette) -> El {
    let face = row().size(48.0, 48.0).center().radius(24.0).fill(p.background_tertiary).child(icon(&icons::QUESTION, 28.0).color(p.text_tertiary));
    let header = col().gap(2.4).child(title("Unknown invite", p.status_danger)).child(text("Try asking for a new invite.").font(font(12.8, 400.0).lh(15.36)).color(p.text_tertiary));
    let button = Button::<()>::new("Invite unavailable").fit_container().disabled(true).el(p);
    card(face, header, Some(button), p)
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
    let (face, name, stats, label, joined) = match &inv.guild {
        Some(g) => {
            let load = g.icon.as_deref().map(|h| st.media.asset(|b, s| media::guild_icon_url(b, g.id, h, 48.0, s)));
            let face = match load {
                Some(Load::Ready(px)) => el().size(48.0, 48.0).radius(24.0).pixels_fit(&px, rune::SurfaceFit::Fill),
                Some(Load::Pending) => el().size(48.0, 48.0).radius(24.0).fill(p.background_tertiary),
                _ => super::guilds::acronym_face(&g.name, 48.0, false, p),
            };
            let members = inv.member_count.unwrap_or(0);
            let stats = row()
                .gap(9.6)
                .child(stat(Some(p.status_online), format!("{} online", count(inv.presence_count.unwrap_or(0))), p))
                .child(stat(Some(p.text_tertiary_secondary), format!("{} {}", count(members), if members == 1 { "member" } else { "members" }), p));
            let joined = store.guilds.contains_key(&g.id);
            (face, g.name.clone(), stats, if joined { "Go to community" } else { "Join community" }, joined)
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
            let stats = row().child(stat(Some(p.text_tertiary_secondary), format!("{} {}", count(members), if members == 1 { "Member" } else { "Members" }), p));
            let joined = inv.channel_id.or(ch.map(|c| c.id)).is_some_and(|c| store.channels.contains_key(&c));
            (face, name, stats, if joined { "Already joined" } else { "Join group" }, joined)
        }
    };
    let (code, guild, channel) = (inv.code.clone(), inv.guild.as_ref().map(|g| g.id), inv.channel.as_ref().map(|c| c.id));
    let go = move |s: &mut S| {
        let sh = acc(s);
        match guild {
            Some(g) if joined => sh.select_guild(Some(g), channel),
            _ => sh.api(Method::POST, format!("/invites/{code}"), None, crate::guild::Then::Enter),
        }
    };
    let button = Button::new(label).fit_container().disabled(joined && guild.is_none()).key(format!("invite-{}", inv.code)).on_press(go).el(p);
    let header = col().gap(4.0).child(title(&name, p.text_primary)).child(stats);
    card(face, header, Some(button), p)
}

/// Fetches the invites linked in the open channel's loaded messages once each.
pub fn request(store: &Store, st: &mut ShellState) -> bool {
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
