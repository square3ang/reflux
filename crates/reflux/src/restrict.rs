//! `AccountRestrictionScreen` (`auth/flow`): a sign-in refused with `ACCOUNT_SUSPENDED_*`
//! (an enforcement notice) or `GLOBAL_IP_*BANNED` (a network block) shows what happened and how
//! to appeal instead of a form error.

use rune::text::Span;
use rune::{Align, El, col, text};
use serde_json::{Map, Value};

use crate::State;
use crate::i18n::{t, tf};
use crate::theme::{Palette, font};
use crate::ui::Button;

/// `.summary`, `.text`, `.fineText` / `.reference`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Style {
    Summary,
    Text,
    Fine,
}

/// A paragraph: runs of text, some of them links.
#[derive(Clone, Debug, PartialEq)]
pub struct Para {
    pub style: Style,
    pub parts: Vec<(String, Option<String>)>,
}

impl Para {
    fn plain(style: Style, s: impl Into<String>) -> Self {
        Self { style, parts: vec![(s.into(), None)] }
    }

    /// `LineText`: `[label](https://…)` markdown links become links.
    fn markdown(style: Style, line: &str) -> Self {
        let mut parts = Vec::new();
        let mut rest = line;
        while let Some(open) = rest.find('[') {
            let link = rest[open + 1..].split_once("](").and_then(|(label, tail)| {
                let end = tail.find(|c: char| c == ')' || c.is_whitespace())?;
                let url = &tail[..end];
                (tail[end..].starts_with(')') && !label.contains(']') && (url.starts_with("https://") || url.starts_with("http://"))).then_some((label, url, tail[end + 1..].len()))
            });
            let Some((label, url, after)) = link else {
                parts.push((rest[..=open].to_string(), None));
                rest = &rest[open + 1..];
                continue;
            };
            parts.push((rest[..open].to_string(), None));
            parts.push((label.to_string(), Some(url.to_string())));
            rest = &rest[rest.len() - after..];
        }
        parts.push((rest.to_string(), None));
        parts.retain(|(s, _)| !s.is_empty());
        Self { style, parts }
    }

    /// A translated `<Trans>` message whose `<0>…</0>` is a link to `url`.
    fn linked(style: Style, msg: String, url: Option<&str>) -> Self {
        let Some((before, rest)) = msg.split_once("<0>") else { return Self::plain(style, msg) };
        let (label, after) = rest.split_once("</0>").unwrap_or((rest, ""));
        let parts = [(before, None), (label, url.map(str::to_owned)), (after, None)];
        Self { style, parts: parts.into_iter().filter(|(s, _)| !s.is_empty()).map(|(s, u)| (s.to_string(), u)).collect() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Restriction {
    pub title: String,
    /// (heading, paragraphs); a `None` heading is unheaded text.
    pub sections: Vec<(Option<String>, Vec<Para>)>,
    /// A network block: the footer is "Try again" instead of "Back to sign in".
    pub network: bool,
}

/// `RuleLine`.
fn rule_line(rule: &Value) -> Option<Para> {
    let title = rule["title"].as_str()?;
    Some(match rule["url"].as_str() {
        Some(url) => Para::linked(Style::Text, tf("Rule: <0>{title}</0>", &[("title", title)]), Some(url)),
        None => Para::plain(Style::Text, tf("Rule: {title}", &[("title", title)])),
    })
}

/// `accountRestrictionFromBody`.
pub fn from_error(code: &str, body: &Map<String, Value>) -> Option<Restriction> {
    let list = |v: &Value| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect::<Vec<_>>()).unwrap_or_default();
    match code {
        "ACCOUNT_SUSPENDED_PERMANENTLY" | "ACCOUNT_SUSPENDED_TEMPORARILY" => {
            let n = body.get("notice")?;
            let title = match n["kind"].as_str()? {
                "closure" => t("Account closed"),
                _ if n["permanent"].as_bool() == Some(true) => t("Account banned"),
                _ => t("Account suspended"),
            };
            let mut happened = vec![Para::markdown(Style::Summary, n["summary"].as_str()?)];
            happened.extend(list(&n["effects"]).iter().map(|l| Para::markdown(Style::Text, l)));
            let mut sections = vec![(Some(t("What happened").to_string()), happened)];
            let mut why: Vec<Para> = rule_line(&n["rule"]).into_iter().collect();
            why.extend(list(&n["facts"]).iter().map(|l| Para::markdown(Style::Text, l)));
            why.extend(n["statement"].as_str().map(|st| Para::plain(Style::Text, tf("Note from our team: {statement}", &[("statement", st)]))));
            if !why.is_empty() {
                sections.push((Some(t("Why").into()), why));
            }
            let decided = list(&n["how_we_decided"]);
            if !decided.is_empty() {
                sections.push((Some(t("How we decided").into()), decided.iter().map(|l| Para::markdown(Style::Text, l)).collect()));
            }
            if let Some(email) = n["appeal"]["email"].as_str() {
                let mail = format!("mailto:{email}");
                let deadline = n["appeal"]["deadline"].as_str().map(|d| d.trim_end_matches('.'));
                let msg = match deadline {
                    None => tf("If you think we got this wrong, email <0>{email}</0> and include your username.", &[("email", email)]),
                    Some(d) if n["appeal"]["account_has_email"].as_bool() != Some(true) => tf(
                        "If you think we got this wrong, email <0>{email}</0> by {deadline}. Include the decision ID and your username. Appeals are free, and a person reviews each one.",
                        &[("email", email), ("deadline", d)],
                    ),
                    Some(d) => tf(
                        "If you think we got this wrong, email <0>{email}</0> by {deadline}. Write from the email address on your account if you can, and include the decision ID. Appeals are free, and a person reviews each one.",
                        &[("email", email), ("deadline", d)],
                    ),
                };
                let mut appeal = vec![Para::linked(Style::Text, msg, Some(&mail))];
                appeal.extend(list(&n["redress"]).iter().map(|l| Para::markdown(Style::Fine, l)));
                sections.push((Some(t("How to appeal").into()), appeal));
            }
            if let Some(id) = n["decision_id"].as_str() {
                sections.push((None, vec![Para::plain(Style::Fine, tf("Decision ID: {decisionId}", &[("decisionId", id)]))]));
            }
            Some(Restriction { title: title.into(), sections, network: false })
        }
        "GLOBAL_IP_BANNED" | "GLOBAL_IP_TEMPORARILY_BANNED" => {
            let product = body.get("product_name").and_then(Value::as_str).unwrap_or("Fluxer");
            let reason = body.get("reason").filter(|r| r.is_object());
            let mut lines = vec![Para::plain(
                Style::Text,
                match reason {
                    Some(_) => tf("{productName} blocked access from this network because of activity that broke its rules.", &[("productName", product)]),
                    None => tf("{productName} blocked access from this network.", &[("productName", product)]),
                },
            )];
            lines.extend(reason.and_then(rule_line));
            if let Some(d) = body.get("expires_at").and_then(Value::as_str) {
                // `dateStyle: medium, timeStyle: short` in UTC.
                let date = chrono::DateTime::parse_from_rfc3339(d).map(|d| d.with_timezone(&chrono::Utc).format("%b %-d, %Y, %-I:%M %p UTC").to_string()).unwrap_or_else(|_| d.to_string());
                lines.push(Para::plain(Style::Text, tf("The block ends on {date}.", &[("date", &date)])));
            }
            lines.push(Para::plain(Style::Text, t("If you share this network, for example at school, at work, on public Wi-Fi, through your mobile carrier or a VPN, this block may have nothing to do with you. The quickest fix is to switch networks, for example to mobile data instead of Wi-Fi, or to turn your VPN on or off.")));
            if let (Some(e), Some(ip)) = (body.get("appeal_email").and_then(Value::as_str), body.get("ip_address").and_then(Value::as_str)) {
                let msg = tf("If you think this block is a mistake, email <0>{email}</0> and include your IP address, {ip}.", &[("email", e), ("ip", ip)]);
                lines.push(Para::linked(Style::Text, msg, Some(&format!("mailto:{e}"))));
            }
            Some(Restriction { title: t("Network blocked").into(), sections: vec![(None, lines)], network: true })
        }
        _ => None,
    }
}

fn para(pa: &Para, p: &Palette) -> El {
    let (size, weight, color) = match pa.style {
        Style::Summary => (15.0, 600.0, p.text_primary),
        Style::Text => (14.0, 400.0, p.text_secondary),
        Style::Fine => (13.0, 400.0, p.text_tertiary),
    };
    let (mut body, mut spans, mut links) = (String::new(), Vec::new(), Vec::new());
    for (s, url) in &pa.parts {
        let start = body.len();
        body.push_str(s);
        if let Some(url) = url {
            spans.push(Span::new(start, body.len()).color(p.text_link));
            links.push(rune::Link::new(start..body.len(), url.clone()));
        }
    }
    let mut e = rune::rich(body, spans).w_full().font(font(size, weight).lh(size * 1.5)).color(color).wrap().selectable(true);
    for l in links {
        e = e.link(l);
    }
    e.on_link(|_: &mut State, url: &str, tx: &mut rune::Tx| {
        let _ = tx.open_url(url);
    })
}

/// `.container` (gap 20): title, sections (heading 15/600, gap 8), then the full-width button.
pub fn view(r: &Restriction, p: &Palette) -> El {
    let title = text(r.title.clone()).w_full().align(Align::Center).font(font(18.0, 600.0).lh(27.0)).color(p.text_primary);
    let mut out = col().w_full().gap(20.0).py(4.0).child(title);
    for (h, paras) in &r.sections {
        let mut sec = col().w_full().gap(8.0);
        if let Some(h) = h {
            sec = sec.child(text(h.clone()).font(font(15.0, 600.0).lh(20.0)).color(p.text_primary));
        }
        out = out.child(sec.children(paras.iter().map(|pa| para(pa, p))));
    }
    // "Try again" clears the block screen too; the next sign-in shows it again if it still holds.
    let label = if r.network { t("Try again") } else { t("Back to sign in") };
    let back = Button::new(label).fit_container().key("restricted-back").on_press(|s: &mut State| s.login.step = crate::login::Step::Method).el(p);
    out.child(col().w_full().mt(4.0).py(12.0).child(back))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Para, Style};

    #[test]
    fn notices_and_blocks_become_screens() {
        let body = json!({"notice": {"kind": "suspension", "permanent": false, "summary": "We suspended your account.", "effects": ["You can't sign in."], "facts": ["Spam in [#general](https://x.test/c)."], "rule": {"title": "No spam", "url": "https://x.test/rules"}, "statement": null, "how_we_decided": [], "appeal": {"email": "appeals@x", "deadline": "May 1, 2026.", "account_has_email": true}, "redress": [], "decision_id": "9"}});
        let r = super::from_error("ACCOUNT_SUSPENDED_TEMPORARILY", body.as_object().unwrap()).unwrap();
        assert_eq!(r.title, "Account suspended");
        let section = |h: &str| r.sections.iter().find(|(x, _)| x.as_deref() == Some(h)).map(|(_, p)| p.clone()).unwrap();
        let appeal = &section("How to appeal")[0];
        assert_eq!(appeal.parts[1], ("appeals@x".into(), Some("mailto:appeals@x".into())));
        assert!(appeal.parts[2].0.starts_with(" by May 1, 2026. Write"));
        let why = section("Why");
        assert_eq!(why[0].parts, vec![("Rule: ".into(), None), ("No spam".into(), Some("https://x.test/rules".into()))]);
        assert_eq!(why[1].parts[1], ("#general".into(), Some("https://x.test/c".into())), "facts sit under Why, with their links");
        assert!(r.sections.iter().all(|(h, _)| h.as_deref() != Some("How we decided")), "empty sections are left out");
        let ip = json!({"ip_address": "1.2.3.4", "appeal_email": "abuse@x", "appeals_supported": true, "ban_kind": "temporary_24h", "expires_at": "2026-11-08T09:30:00Z", "reason": null, "product_name": "Fluxer"});
        let n = super::from_error("GLOBAL_IP_BANNED", ip.as_object().unwrap()).unwrap();
        assert!(n.network && n.title == "Network blocked");
        assert!(n.sections[0].1.iter().any(|p| p.parts[0].0 == "The block ends on Nov 8, 2026, 9:30 AM UTC."));
        assert!(super::from_error("OTHER", ip.as_object().unwrap()).is_none());
    }

    #[test]
    fn markdown_links_only_for_http() {
        let p = Para::markdown(Style::Text, "See [rules](https://a.b/r) and [x](javascript:alert(1)) [y]");
        assert_eq!(p.parts.iter().filter(|(_, u)| u.is_some()).count(), 1);
        assert_eq!(p.parts.iter().map(|(s, _)| s.as_str()).collect::<String>(), "See rules and [x](javascript:alert(1)) [y]");
    }
}
