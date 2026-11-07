//! `OAuthAuthorizeModal` from a `fluxer://oauth2/authorize?…` link (`useAuthorizeFlow`): the
//! scopes it asks for (each can be turned off but `bot`), then for a bot a destination (a
//! community you can grant its permissions in, or a group DM) and the permissions to grant,
//! then `POST /oauth2/authorize/consent`.

use std::collections::HashSet;

use reflux_model::{ChannelType, Permissions, Snowflake};
use reflux_store::Store;
use rune::{El, col, row, text};
use serde_json::{Value, json};

use crate::i18n::{t, tf};
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
    pub step: Step,
    /// Scopes turned off (`useScopeSelection`; `bot` stays on).
    pub off_scopes: HashSet<String>,
    /// Requested bot permissions turned off (`usePermissionSelection`).
    pub off_perms: Permissions,
    /// The chosen destination (`None`: the first one, or the link's `guild_id`/`channel_id`).
    pub dest: Option<Dest>,
    /// Authorize was pressed (sent from [`poll`], which has the store).
    pub submit: bool,
    /// Added the bot without a redirect: the community's name.
    pub done: Option<String>,
    pub error: Option<String>,
}

/// `ReviewStep` (the account step is the web page's).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Step {
    #[default]
    Scopes,
    Community,
    Permissions,
}

/// `BotInviteDestinationOption`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dest {
    Guild(Snowflake),
    GroupDm(Snowflake),
}

impl OAuth {
    fn get(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
    }
    fn scopes(&self) -> Vec<&str> {
        self.get("scope").map_or(Vec::new(), |s| s.split([' ', '+']).filter(|s| !s.is_empty()).collect())
    }
    fn selected_scopes(&self) -> Vec<&str> {
        self.scopes().into_iter().filter(|s| *s == "bot" || !self.off_scopes.contains(*s)).collect()
    }
    fn has_bot(&self) -> bool {
        self.scopes().contains(&"bot")
    }
    /// `parseRequestedBotPermissions`, limited to the listed ones (`requestedKeys`).
    fn requested(&self) -> Permissions {
        let bits = self.get("permissions").and_then(|p| p.parse::<u64>().ok()).unwrap_or(0);
        Permissions::from_bits_retain(bits) & listed()
    }
    /// `reviewSteps`.
    fn steps(&self, dest: Option<Dest>) -> Vec<Step> {
        let mut v = vec![Step::Scopes];
        if self.has_bot() {
            v.push(Step::Community);
            if !matches!(dest, Some(Dest::GroupDm(_))) && !self.requested().is_empty() {
                v.push(Step::Permissions);
            }
        }
        v
    }
    fn link_dest(&self) -> Option<Dest> {
        let id = |k| self.get(k)?.parse().ok().map(Snowflake);
        id("channel_id").map(Dest::GroupDm).or_else(|| id("guild_id").map(Dest::Guild))
    }
}

/// Every permission `getAllBotPermissions` lists.
fn listed() -> Permissions {
    crate::guild::perms::GUILD.iter().flat_map(|(_, e)| e.iter()).fold(Permissions::empty(), |a, (b, _, _)| a | *b)
}

/// `useBotInviteDestinations`: communities where you can grant the requested permissions
/// (`canAuthorizeBotInvite`), then your group DMs; (destination, label, is a group DM).
fn destinations(store: &Store, requested: Permissions) -> Vec<(Dest, String)> {
    let mut out: Vec<(Dest, String)> = store
        .guild_list()
        .into_iter()
        .flat_map(|i| match i {
            reflux_store::GuildListItem::Guild(g) => vec![g],
            reflux_store::GuildListItem::Folder { guilds, .. } => guilds,
        })
        .filter(|g| can_invite(store.guild_permissions(*g), requested))
        .filter_map(|g| Some((Dest::Guild(g), store.guilds.get(&g)?.info.as_ref()?.name.clone())))
        .collect();
    let mut dms: Vec<_> = store.channels.values().filter(|c| c.kind == ChannelType::GROUP_DM).collect();
    dms.sort_by_key(|c| std::cmp::Reverse(c.last_message_id));
    out.extend(dms.into_iter().map(|c| (Dest::GroupDm(c.id), crate::shell::channel_title(store, c))));
    out
}

/// `canAuthorizeBotInvite`.
fn can_invite(mine: Permissions, requested: Permissions) -> bool {
    let admin = mine.contains(Permissions::ADMINISTRATOR);
    (admin || mine.contains(Permissions::MANAGE_GUILD)) && (requested.is_empty() || admin || mine.contains(requested))
}

/// The destination in effect: the chosen one if still offered, else the link's, else the first.
fn current_dest(o: &OAuth, opts: &[(Dest, String)]) -> Option<Dest> {
    let has = |d: Dest| opts.iter().any(|(x, _)| *x == d);
    if let Some(d) = o.dest.filter(|d| has(*d)) {
        return Some(d);
    }
    match o.link_dest() {
        Some(d) => has(d).then_some(d),
        None => opts.first().map(|(d, _)| *d),
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
    if std::mem::take(&mut o.submit) {
        o.error = None;
        authorize(store, st);
        return true;
    }
    match st.expr.fetched.remove(CONSENT) {
        Some(Fetched::Done(v)) => match v["redirect_to"].as_str() {
            Some(url) => {
                crate::settings::privacy::open_url(url);
                st.nav.oauth = None;
            }
            None => {
                let opts = destinations(store, o.requested());
                let dest = current_dest(o, &opts);
                o.done = Some(opts.into_iter().find(|(d, _)| Some(*d) == dest).map(|(_, n)| n).unwrap_or_default());
            }
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

fn authorize(store: &Store, st: &mut ShellState) {
    let Some(o) = &st.nav.oauth else { return };
    let scopes = o.selected_scopes();
    let mut body = json!({ "response_type": o.get("response_type").unwrap_or("code"), "client_id": o.get("client_id"), "scope": scopes.join(" ") });
    for k in ["redirect_uri", "state", "code_challenge", "code_challenge_method"] {
        if let Some(v) = o.get(k) {
            body[k] = json!(v);
        }
    }
    if scopes.contains(&"bot") {
        match current_dest(o, &destinations(store, o.requested())) {
            Some(Dest::Guild(g)) => {
                body["guild_id"] = json!(g.to_string());
                let bits = o.requested() - o.off_perms;
                if !bits.is_empty() {
                    body["permissions"] = json!(bits.bits().to_string());
                }
            }
            Some(Dest::GroupDm(c)) => body["channel_id"] = json!(c.to_string()),
            None => return,
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
    let heading = |s: String| text(s).font(font(20.0, 700.0).lh(24.0)).color(p.text_primary).wrap();
    let note = |s: String, c| row().w_full().padding(12.0).radius(8.0).fill(p.background_tertiary).child(text(s).font(font(13.0, 400.0).lh(18.0)).color(c).wrap().grow(1.0).min_w(0.0));
    let requested = o.requested();
    let opts = destinations(store, requested);
    let dest = current_dest(o, &opts);
    let steps = o.steps(dest);
    let at = steps.iter().position(|s| *s == o.step).unwrap_or(0);
    let start = layer.len();
    let mut body = col().w_full().gap(16.0);
    let scopes = o.scopes();
    let selected = o.selected_scopes();
    if let Some(dest) = &o.done {
        body = body.child(line(tf("{name} was added to {destination}.", &[("name", &name), ("destination", dest)]), p.text_primary));
    } else if let Some(Fetched::Failed(e)) = st.expr.fetched.get(APP) {
        body = body.child(text(t("Invalid authorization request")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary)).child(line(e.clone(), p.text_primary_muted));
    } else {
        match steps[at] {
            Step::Scopes => {
                let about = app.as_ref().and_then(|a| a["description"].as_str()).filter(|d| !d.is_empty()).map_or_else(|| t("Review what this app is asking for before you continue.").to_string(), str::to_owned);
                body = body.child(heading(tf("{clientLabel} wants to connect", &[("clientLabel", &name)]))).child(line(about, p.text_primary_muted));
                if o.has_bot() {
                    let label = if o.get("redirect_uri").is_none() { t("Bot invite (no external redirect)").to_string() } else { o.get("channel_id").or(o.get("guild_id")).map_or_else(|| t("Bot scope requested").to_string(), str::to_owned) };
                    body = body.child(text(label).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary));
                }
                body = body
                    .child(col().w_full().gap(4.0).child(text(t("Requested scopes")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary)).child(line(t("Turn off anything you're not comfortable with. Some features may stop working.").into(), p.text_primary_muted)));
                if scopes.is_empty() {
                    body = body.child(line(t("No specific scopes requested.").into(), p.text_primary_muted));
                }
                for sc in &scopes {
                    let locked = *sc == "bot";
                    let label = if locked { format!("{sc} · {}", t("Required")) } else { sc.to_string() };
                    let key = sc.to_string();
                    body = body.child(
                        crate::ui::Switch::new(selected.contains(sc))
                            .label(label)
                            .description(scope_text(sc))
                            .disabled(locked)
                            .on_toggle(move |s: &mut S| {
                                if let Some(o) = acc(s).nav.oauth.as_mut()
                                    && !o.off_scopes.remove(&key)
                                {
                                    o.off_scopes.insert(key.clone());
                                }
                            })
                            .el(p),
                    );
                }
                if !scopes.is_empty() && selected.is_empty() {
                    body = body.child(note(t("Turn on at least one scope to authorize this app.").into(), p.status_danger));
                } else if selected.len() < scopes.len() {
                    body = body.child(note(t("Turning off scopes may prevent the app from working correctly.").into(), p.text_primary_muted));
                }
            }
            Step::Community => {
                let help = if requested.is_empty() {
                    tf("Select a community where you have {permissionName} permission, or a group DM you belong to.", &[("permissionName", t("Manage community"))])
                } else {
                    t("Select a community where you can grant the requested permissions, or a group DM you belong to.").into()
                };
                body = body.child(heading(t("Add bot to a destination").into())).child(line(help, p.text_primary_muted));
                if opts.is_empty() {
                    let empty = if requested.is_empty() { t("No communities or group DMs available for this bot.") } else { t("No group DMs or communities where you can grant the requested permissions are available.") };
                    body = body.child(line(empty.into(), p.text_primary_muted));
                } else {
                    let keyed: Vec<(String, String)> = opts
                        .iter()
                        .map(|(d, n)| match d {
                            Dest::Guild(g) => (format!("guild:{g}"), format!("{n} · {}", t("Community"))),
                            Dest::GroupDm(c) => (format!("group_dm:{c}"), format!("{n} · {}", t("Group DM"))),
                        })
                        .collect();
                    let choices: Vec<(&str, &str)> = keyed.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
                    let value = dest.map(|d| match d {
                        Dest::Guild(g) => format!("guild:{g}"),
                        Dest::GroupDm(c) => format!("group_dm:{c}"),
                    });
                    body = body.child(
                        Select::new("oauth-guild", &choices, value.as_deref())
                            .placeholder(t("Choose a destination"))
                            .width(408.0)
                            .on_choose(move |s: &mut S, v: &str| {
                                let parsed = v.split_once(':').and_then(|(k, id)| {
                                    let id = Snowflake(id.parse().ok()?);
                                    Some(if k == "group_dm" { Dest::GroupDm(id) } else { Dest::Guild(id) })
                                });
                                if let Some(o) = acc(s).nav.oauth.as_mut() {
                                    o.dest = parsed;
                                }
                            })
                            .el(&st.popups, popup_access(move |s: &mut S| &mut acc(s).popups), p, layer),
                    );
                }
            }
            Step::Permissions => {
                body = body.child(heading(t("Configure bot permissions").into())).child(line(tf("Choose what {clientLabel} can do in your community. Uncheck any permissions you don't want to grant.", &[("clientLabel", &name)]), p.text_primary_muted));
                let mut list = col().w_full().gap(8.0);
                for (bit, title, _) in crate::guild::perms::GUILD.iter().flat_map(|(_, e)| e.iter()).filter(|(b, _, _)| requested.contains(*b)) {
                    let bit = *bit;
                    list = list.child(
                        crate::ui::Checkbox::new(!o.off_perms.contains(bit))
                            .label(t(title))
                            .on_toggle(move |s: &mut S| {
                                if let Some(o) = acc(s).nav.oauth.as_mut() {
                                    o.off_perms.toggle(bit);
                                }
                            })
                            .el(p),
                    );
                }
                body = body.child(crate::ui::scroller(col().w_full().max_h(280.0).scroll(rune::ScrollSpec { stick_bottom: false }).key("oauth-scopes").child(list), false, p));
                if requested.contains(Permissions::ADMINISTRATOR) {
                    body = body.child(note(t("This bot requests the administrator permission. Only grant it if you fully trust the developer. Ask them to request fewer permissions instead. Close this page if unsure.").into(), p.status_warning));
                }
                if !o.off_perms.is_empty() {
                    body = body.child(note(t("Removing permissions could limit the bot's features.").into(), p.text_primary_muted));
                }
            }
        }
        let last = at + 1 == steps.len();
        if last && let Some(host) = o.get("redirect_uri").and_then(|u| u.split('/').nth(2)) {
            body = body.child(line(tf("You will be taken to {redirectHostname} after authorizing.", &[("redirectHostname", host)]), p.text_primary_muted));
        }
        if let Some(e) = &o.error {
            body = body.child(line(e.clone(), p.status_danger));
        }
    }
    let busy = matches!(st.expr.fetched.get(CONSENT), Some(Fetched::Loading));
    let cannot = selected.is_empty() || (o.has_bot() && dest.is_none());
    let go = move |step: Step| move |s: &mut S| {
        if let Some(o) = acc(s).nav.oauth.as_mut() {
            o.step = step;
        }
    };
    let mut footer = row().gap(8.0);
    if o.done.is_some() || app.is_none() {
        footer = footer.child(Button::new(if o.done.is_some() { t("Close") } else { t("Cancel") }).variant(ButtonVariant::Secondary).on_press(close).el(p));
    } else {
        footer = footer.child(match at.checked_sub(1).map(|i| steps[i]) {
            Some(prev) => Button::new(t("Back")).variant(ButtonVariant::Secondary).key("oauth-back").disabled(busy).on_press(go(prev)).el(p),
            None => Button::new(t("Cancel")).variant(ButtonVariant::Secondary).disabled(busy).on_press(close).el(p),
        });
        footer = footer.child(match steps.get(at + 1) {
            Some(next) => Button::new(t("Next")).key("oauth-next").disabled(cannot || busy).on_press(go(*next)).el(p).default_button(),
            None => Button::new(t("Authorize")).key("oauth-authorize").disabled(cannot).submitting(busy).on_press(move |s: &mut S| {
                if let Some(o) = acc(s).nav.oauth.as_mut() {
                    o.submit = true;
                }
            }).el(p).default_button(),
        });
    }
    let modal = Modal::new(t("Authorize application"), close).body(body).footer(footer).el(p);
    layer.insert(start, modal);
}

/// Demo: a bot invite asking for `identify`, `email` (turned off) and `bot` with a few
/// permissions; `oauth-community` / `oauth-perms` open the later steps.
pub fn demo(st: &mut ShellState, scene: &str) {
    let params = parse("oauth2/authorize?client_id=1&scope=identify+email+bot&permissions=3080&redirect_uri=https%3A%2F%2Fexample.com%2Fcb").unwrap_or_default();
    let step = match scene {
        "oauth-community" => Step::Community,
        "oauth-perms" => Step::Permissions,
        _ => Step::Scopes,
    };
    st.nav.oauth = Some(OAuth { params, step, off_scopes: HashSet::from(["email".to_string()]), off_perms: Permissions::SEND_MESSAGES, ..Default::default() });
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

    #[test]
    fn bot_invite_steps_and_destinations() {
        use super::*;
        let mut o = OAuth { params: parse("oauth2/authorize?client_id=1&scope=identify+bot&permissions=2056").unwrap(), ..Default::default() };
        o.off_scopes.extend(["identify".to_string(), "bot".to_string()]);
        assert_eq!(o.selected_scopes(), ["bot"], "bot can't be turned off");
        assert_eq!(o.steps(None), [Step::Scopes, Step::Community, Step::Permissions]);
        assert_eq!(o.steps(Some(Dest::GroupDm(Snowflake(5)))), [Step::Scopes, Step::Community], "group DMs take no permissions");
        let want = Permissions::ADMINISTRATOR | Permissions::SEND_MESSAGES;
        assert!(!can_invite(Permissions::SEND_MESSAGES, Permissions::empty()), "needs Manage community");
        assert!(can_invite(Permissions::MANAGE_GUILD, Permissions::empty()));
        assert!(!can_invite(Permissions::MANAGE_GUILD | Permissions::SEND_MESSAGES, want));
        assert!(can_invite(Permissions::ADMINISTRATOR, want));
        let opts = [(Dest::Guild(Snowflake(1)), String::new()), (Dest::GroupDm(Snowflake(2)), String::new())];
        assert_eq!(current_dest(&o, &opts), Some(Dest::Guild(Snowflake(1))));
        o.params.push(("channel_id".into(), "2".into()));
        assert_eq!(current_dest(&o, &opts), Some(Dest::GroupDm(Snowflake(2))), "the link's destination");
    }
}
