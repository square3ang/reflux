//! `AccountRestrictionScreen` (`auth/flow`): a sign-in refused with `ACCOUNT_SUSPENDED_*`
//! (an enforcement notice) or `GLOBAL_IP_*BANNED` (a network block) shows what happened and how
//! to appeal instead of a form error.
// shortcut: rule links and the appeal address are plain text, not links; the age gate and the
// new audio device prompt aren't here.

use rune::{El, col, text};
use serde_json::{Map, Value};

use crate::i18n::{t, tf};
use crate::theme::{Palette, font};

#[derive(Clone, Debug, PartialEq)]
pub struct Restriction {
    pub title: String,
    /// (heading, paragraphs); a `None` heading is the lead.
    pub sections: Vec<(Option<String>, Vec<String>)>,
}

/// `accountRestrictionFromBody`.
pub fn from_error(code: &str, body: &Map<String, Value>) -> Option<Restriction> {
    let s = |v: &Value| v.as_str().map(str::to_owned);
    let list = |v: &Value| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect::<Vec<_>>()).unwrap_or_default();
    match code {
        "ACCOUNT_SUSPENDED_PERMANENTLY" | "ACCOUNT_SUSPENDED_TEMPORARILY" => {
            let n = body.get("notice")?;
            let title = match n["kind"].as_str()? {
                "closure" => t("Account closed"),
                _ if n["permanent"].as_bool() == Some(true) => t("Account banned"),
                _ => t("Account suspended"),
            };
            let mut lead = vec![s(&n["summary"])?];
            lead.extend(list(&n["effects"]));
            let mut sections = vec![(None, lead)];
            let mut add = |h: &str, v: Vec<String>| {
                if !v.is_empty() {
                    sections.push((Some(t(h).to_string()), v));
                }
            };
            add("What happened", list(&n["facts"]));
            let mut why: Vec<String> = n["rule"]["title"].as_str().map(|r| tf("Rule: {title}", &[("title", r)])).into_iter().collect();
            why.extend(n["statement"].as_str().map(|st| tf("Note from our team: {statement}", &[("statement", st)])));
            add("Why", why);
            add("How we decided", list(&n["how_we_decided"]));
            let mut appeal: Vec<String> = n["appeal"]["email"]
                .as_str()
                .map(|e| match n["appeal"]["deadline"].as_str().map(|d| d.trim_end_matches('.')) {
                    None => format!("If you think we got this wrong, email {e} and include your username."),
                    Some(d) if n["appeal"]["account_has_email"].as_bool() != Some(true) => {
                        format!("If you think we got this wrong, email {e} by {d}. Include the decision ID and your username. Appeals are free, and a person reviews each one.")
                    }
                    Some(d) => format!("If you think we got this wrong, email {e} by {d}. Write from the email address on your account if you can, and include the decision ID. Appeals are free, and a person reviews each one."),
                })
                .into_iter()
                .collect();
            appeal.extend(list(&n["redress"]));
            add("How to appeal", appeal);
            if let Some(id) = n["decision_id"].as_str() {
                sections.push((None, vec![tf("Decision ID: {decisionId}", &[("decisionId", id)])]));
            }
            Some(Restriction { title: title.into(), sections })
        }
        "GLOBAL_IP_BANNED" | "GLOBAL_IP_TEMPORARILY_BANNED" => {
            let product = body.get("product_name").and_then(Value::as_str).unwrap_or("Fluxer");
            let reason = body.get("reason").and_then(|r| r["title"].as_str());
            let mut lines = vec![match reason {
                Some(_) => tf("{productName} blocked access from this network because of activity that broke its rules.", &[("productName", product)]),
                None => tf("{productName} blocked access from this network.", &[("productName", product)]),
            }];
            lines.extend(reason.map(|r| tf("Rule: {title}", &[("title", r)])));
            lines.extend(body.get("expires_at").and_then(Value::as_str).map(|d| tf("The block ends on {date}.", &[("date", &d.replace('T', " ").chars().take(16).collect::<String>())])));
            lines.push(t("If you share this network, for example at school, at work, on public Wi-Fi, through your mobile carrier or a VPN, this block may have nothing to do with you. The quickest fix is to switch networks, for example to mobile data instead of Wi-Fi, or to turn your VPN on or off.").into());
            if let (Some(e), Some(ip)) = (body.get("appeal_email").and_then(Value::as_str), body.get("ip_address").and_then(Value::as_str)) {
                lines.push(format!("If you think this block is a mistake, email {e} and include your IP address, {ip}."));
            }
            Some(Restriction { title: t("Network blocked").into(), sections: vec![(None, lines)] })
        }
        _ => None,
    }
}

pub fn view(r: &Restriction, p: &Palette) -> El {
    let para = |s: &String| text(s.clone()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary).wrap().selectable(true);
    let mut out = col().w_full().gap(16.0).child(crate::login::title(&r.title, p));
    for (h, lines) in &r.sections {
        let mut sec = col().w_full().gap(6.0);
        if let Some(h) = h {
            sec = sec.child(text(h.clone()).font(font(12.0, 700.0).lh(16.0).tracking(0.2)).color(p.text_primary_muted));
        }
        out = out.child(sec.children(lines.iter().map(para)));
    }
    out.child(crate::login::footer(p, false, |s| s.login.step = crate::login::Step::Method, None))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn notices_and_blocks_become_screens() {
        let body = json!({"notice": {"kind": "suspension", "permanent": false, "summary": "We suspended your account.", "effects": ["You can't sign in."], "facts": ["Spam."], "rule": {"title": "No spam", "url": null}, "statement": null, "how_we_decided": [], "appeal": {"email": "appeals@x", "deadline": "May 1, 2026.", "account_has_email": true}, "redress": [], "decision_id": "9"}});
        let r = super::from_error("ACCOUNT_SUSPENDED_TEMPORARILY", body.as_object().unwrap()).unwrap();
        assert_eq!(r.title, "Account suspended");
        assert!(r.sections.iter().any(|(h, l)| h.as_deref() == Some("How to appeal") && l[0].contains("by May 1, 2026. Write")));
        let ip = json!({"ip_address": "1.2.3.4", "appeal_email": null, "appeals_supported": false, "ban_kind": "permanent", "expires_at": null, "reason": null, "product_name": "Fluxer"});
        assert_eq!(super::from_error("GLOBAL_IP_BANNED", ip.as_object().unwrap()).unwrap().title, "Network blocked");
        assert!(super::from_error("OTHER", ip.as_object().unwrap()).is_none());
    }
}
