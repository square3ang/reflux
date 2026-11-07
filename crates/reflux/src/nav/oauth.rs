//! `OAuthAuthorizeModal` from a `fluxer://oauth2/authorize?…` link: the app, the scopes it
//! asks for, a community to add its bot to, then `POST /oauth2/authorize/consent`.
// shortcut: the scopes can't be unticked, the bot's permissions aren't listed, and group DMs
// aren't offered as a bot destination.

use reflux_model::{Permissions, Snowflake};
use reflux_store::Store;
use rune::{El, col, icon, row, text};
use serde_json::{Value, json};

use crate::i18n::{t, tf};
use crate::icons;
use crate::shell::expr::Fetched;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Modal, Select, popup_access};

const APP: &str = "oauth-app";
const CONSENT: &str = "oauth-consent";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OAuth {
    /// The link's query, decoded.
    pub params: Vec<(String, String)>,
    pub guild: Option<Snowflake>,
    /// Added the bot without a redirect: the community's name.
    pub done: Option<String>,
    pub error: Option<String>,
}

impl OAuth {
    fn get(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
    }
    fn scopes(&self) -> Vec<&str> {
        self.get("scope").map_or(Vec::new(), |s| s.split([' ', '+']).filter(|s| !s.is_empty()).collect())
    }
}

/// `oauth2/authorize?query` (the part after `fluxer://`): its decoded parameters.
pub fn parse(rest: &str) -> Option<Vec<(String, String)>> {
    let (path, query) = rest.split_once('?')?;
    if path.trim_matches('/') != "oauth2/authorize" {
        return None;
    }
    let dec = |s: &str| {
        let b = s.replace('+', " ").into_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%'
                && i + 2 < b.len()
                && let Ok(x) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(x);
                i += 3;
                continue;
            }
            out.push(b[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    };
    let params: Vec<_> = query.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (dec(k), dec(v))).collect();
    params.iter().any(|(k, v)| k == "client_id" && !v.is_empty()).then_some(params)
}

/// Opens the modal and asks for the app's public profile.
pub fn open(st: &mut ShellState, params: Vec<(String, String)>) {
    let o = OAuth { params, ..Default::default() };
    if let Some(id) = o.get("client_id") {
        st.expr.fetched.remove(CONSENT);
        st.expr.fetched.insert(APP.into(), Fetched::Loading);
        st.intents.push(Intent::Fetch { key: APP.into(), method: reqwest::Method::GET, path: format!("/oauth2/applications/{id}/public"), body: None });
    }
    st.nav.oauth = Some(o);
}

/// The consent's answer: follow its redirect, or show where the bot went.
pub fn poll(store: &Store, st: &mut ShellState) -> bool {
    let Some(o) = st.nav.oauth.as_mut() else { return false };
    match st.expr.fetched.remove(CONSENT) {
        Some(Fetched::Done(v)) => match v["redirect_to"].as_str() {
            Some(url) => {
                crate::settings::privacy::open_url(url);
                st.nav.oauth = None;
            }
            None => o.done = Some(o.guild.and_then(|g| store.guilds.get(&g)?.info.as_ref().map(|i| i.name.clone())).unwrap_or_default()),
        },
        Some(Fetched::Failed(e)) => o.error = Some(e),
        Some(Fetched::Loading) => {
            st.expr.fetched.insert(CONSENT.into(), Fetched::Loading);
            return false;
        }
        None => return false,
    }
    true
}

fn authorize(st: &mut ShellState) {
    let Some(o) = &st.nav.oauth else { return };
    let mut body = json!({ "response_type": o.get("response_type").unwrap_or("code"), "client_id": o.get("client_id"), "scope": o.scopes().join(" ") });
    for k in ["redirect_uri", "state", "code_challenge", "code_challenge_method"] {
        if let Some(v) = o.get(k) {
            body[k] = json!(v);
        }
    }
    if o.scopes().contains(&"bot") {
        let Some(g) = o.guild else { return };
        body["guild_id"] = json!(g.to_string());
        if let Some(p) = o.get("permissions") {
            body["permissions"] = json!(p);
        }
    }
    st.expr.fetched.insert(CONSENT.into(), Fetched::Loading);
    st.intents.push(Intent::Fetch { key: CONSENT.into(), method: reqwest::Method::POST, path: "/oauth2/authorize/consent".into(), body: Some(body) });
}

/// `getOAuth2ScopeDescription`.
fn scope_text(s: &str) -> String {
    match s {
        "identify" => t("Access your basic profile information (username, avatar, etc.)").into(),
        "email" => t("View your email address").into(),
        "guilds" => t("View the communities you are a member of").into(),
        "connections" => t("View your connected accounts").into(),
        "bot" => t("Add a bot to a community with requested permissions").into(),
        other => other.to_string(),
    }
}

pub fn layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some(o) = &st.nav.oauth else { return };
    let close = move |s: &mut S| acc(s).nav.oauth = None;
    let app = match st.expr.fetched.get(APP) {
        Some(Fetched::Done(v)) => Some(v.clone()),
        _ => None,
    };
    let name = app.as_ref().and_then(|a| a["name"].as_str()).unwrap_or("…").to_string();
    let line = |s: String, c| text(s).font(font(14.0, 400.0).lh(20.0)).color(c).wrap();
    let start = layer.len();
    let mut body = col().w_full().gap(16.0);
    if let Some(dest) = &o.done {
        body = body.child(line(tf("{name} was added to {destination}.", &[("name", &name), ("destination", dest)]), p.text_primary));
    } else if let Some(Fetched::Failed(e)) = st.expr.fetched.get(APP) {
        body = body.child(text(t("Invalid authorization request")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary)).child(line(e.clone(), p.text_primary_muted));
    } else {
        body = body
            .child(text(name.clone()).font(font(20.0, 700.0).lh(24.0)).color(p.text_primary))
            .child(line(tf("{name} wants to:", &[("name", &name)]), p.text_primary_muted))
            .child(col().w_full().gap(8.0).children(o.scopes().into_iter().map(|s| {
                row().w_full().gap(8.0).center_y().child(icon(&icons::CHECK_CIRCLE_FILL, 18.0).color(p.status_online)).child(line(scope_text(s), p.text_primary).grow(1.0).min_w(0.0))
            })));
        if o.scopes().contains(&"bot") {
            let guilds: Vec<(String, String)> = store
                .guild_list()
                .into_iter()
                .flat_map(|i| match i {
                    reflux_store::GuildListItem::Guild(g) => vec![g],
                    reflux_store::GuildListItem::Folder { guilds, .. } => guilds,
                })
                .filter(|g| store.guild_permissions(*g).contains(Permissions::MANAGE_GUILD))
                .filter_map(|g| Some((g.to_string(), store.guilds.get(&g)?.info.as_ref()?.name.clone())))
                .collect();
            let opts: Vec<(&str, &str)> = guilds.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            let value = o.guild.map(|g| g.to_string());
            body = body.child(text(t("Add bot to a destination")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary)).child(
                Select::new("oauth-guild", &opts, value.as_deref())
                    .placeholder(t("Choose a destination"))
                    .width(408.0)
                    .on_choose(move |s: &mut S, v: &str| {
                        if let Some(o) = acc(s).nav.oauth.as_mut() {
                            o.guild = v.parse().ok().map(Snowflake);
                        }
                    })
                    .el(&st.popups, popup_access(move |s: &mut S| &mut acc(s).popups), p, layer),
            );
        }
        if let Some(host) = o.get("redirect_uri").and_then(|u| u.split('/').nth(2)) {
            body = body.child(line(tf("You will be taken to {redirectHostname} after authorizing.", &[("redirectHostname", host)]), p.text_primary_muted));
        }
        if let Some(e) = &o.error {
            body = body.child(line(e.clone(), p.status_danger));
        }
    }
    let busy = matches!(st.expr.fetched.get(CONSENT), Some(Fetched::Loading));
    let can = app.is_some() && o.done.is_none() && (!o.scopes().contains(&"bot") || o.guild.is_some());
    let mut footer = row().gap(8.0).child(Button::new(if o.done.is_some() { t("Close") } else { t("Cancel") }).variant(ButtonVariant::Secondary).on_press(close).el(p));
    if o.done.is_none() {
        footer = footer.child(Button::new(t("Authorize")).key("oauth-authorize").disabled(!can).submitting(busy).on_press(move |s: &mut S| authorize(acc(s))).el(p));
    }
    let modal = Modal::new(t("Authorize application"), close).body(body).footer(footer).el(p);
    layer.insert(start, modal);
}

/// Demo: a bot invite asking for `identify` and `bot`.
pub fn demo(st: &mut ShellState) {
    st.nav.oauth = Some(OAuth { params: parse("oauth2/authorize?client_id=1&scope=identify+bot&permissions=8&redirect_uri=https%3A%2F%2Fexample.com%2Fcb").unwrap_or_default(), ..Default::default() });
    st.expr.fetched.insert(APP.into(), Fetched::Done(json!({ "id": "1", "name": "Fluxbot", "bot": { "id": "1" } }) as Value));
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_authorize_links() {
        let p = super::parse("oauth2/authorize?client_id=42&scope=identify%20bot&redirect_uri=https%3A%2F%2Fa.b%2Fc").unwrap();
        assert!(p.contains(&("scope".into(), "identify bot".into())));
        assert!(p.contains(&("redirect_uri".into(), "https://a.b/c".into())));
        assert_eq!(super::parse("oauth2/authorize?scope=bot"), None, "needs a client");
        assert_eq!(super::parse("invite/abc?x=1"), None);
    }
}
