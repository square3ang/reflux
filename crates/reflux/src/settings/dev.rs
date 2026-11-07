//! Developer tabs: `ApplicationsTab` (list, create, detail with secrets, bot profile and the
//! OAuth2 URL builder) and `EmbedDebuggerTab` (`POST /unfurl`, its preview and raw JSON).
// shortcut: the bot's avatar/banner and friendly-bot flags aren't editable, and a created app
// stays on the list instead of opening its detail; add them when someone ships bots from Reflux.

use reflux_model::Message;
use reflux_store::Store;
use reqwest::Method;
use rune::{El, col, el, icon, rich, row, text};
use serde_json::{Value, json};

use super::account::{card, card_row, muted, small_button};
use super::tabs::section;
use crate::guild::Then;
use crate::i18n::{t, tf};
use crate::icons;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Checkbox, Modal, ModalSize, Select, Switch, popup_access};

const LIST: &str = "/oauth2/applications/@me";
const SCOPES: [&str; 5] = ["identify", "email", "guilds", "connections", "bot"];

/// The Applications tab's view state (the loaded list is `form.data["dev.apps"]`).
#[derive(Default)]
pub struct Dev {
    /// The application whose detail shows.
    pub app: Option<String>,
    pub create: bool,
    /// Unsaved edits over the loaded application.
    pub public: Option<bool>,
    pub code_grant: Option<bool>,
    pub redirects: Option<Vec<String>>,
    /// The URL builder's picks.
    pub scopes: Vec<String>,
    pub redirect: Option<String>,
    pub perms: u64,
}

pub(super) fn load(sh: &mut ShellState) {
    sh.nav.dev = Dev::default();
    sh.load("dev.apps", LIST);
}

fn label(s: &str, p: &Palette) -> El {
    text(s.to_string()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary)
}

/// `ApplicationDetail`'s `builderUrl`: empty scopes, or a missing required redirect, give none.
pub fn authorize_url(web: &str, id: &str, scopes: &[String], perms: u64, redirect: Option<&str>, code_grant: bool) -> Option<String> {
    if scopes.is_empty() {
        return None;
    }
    let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    let bot_only = scopes.len() == 1 && scopes[0] == "bot";
    let redirect = redirect.map(str::trim).filter(|r| !r.is_empty());
    if (!bot_only || code_grant) && redirect.is_none() {
        return None;
    }
    let mut u = format!("{}/oauth2/authorize?client_id={id}&scope={}", web.trim_end_matches('/'), enc(&scopes.join(" ")));
    if scopes.iter().any(|s| s == "bot") && perms != 0 {
        u += &format!("&permissions={perms}");
    }
    if let Some(r) = redirect {
        u += &format!("&redirect_uri={}&response_type=code", enc(r));
    }
    Some(u)
}

pub(super) fn applications<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    if st.nav.dev.create {
        layer.push(create_modal(st, p, acc));
    }
    let list = match super::loaded::<Vec<Value>>(st, "dev.apps", p) {
        Ok(v) => v,
        Err(e) => return section(t("Applications"), e, true, p),
    };
    if let Some(app) = st.nav.dev.app.as_ref().and_then(|id| list.iter().find(|a| a["id"].as_str() == Some(id))) {
        return detail(st, app, p, acc, layer);
    }
    let docs = row()
        .gap(4.0)
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .on_press(|_: &mut S| super::privacy::open_url("https://docs.fluxer.app"))
        .child(icon(&icons::BOOK_OPEN_FILL, 18.0).color(p.text_link))
        .child(text(tf("Read the documentation ({domain})", &[("domain", "docs.fluxer.app")])).font(font(14.0, 600.0).lh(17.5)).color(p.text_link));
    let buttons = row().gap(12.0).center_y().child(Button::new(t("Create application")).key("dev-create").on_press(move |s: &mut S| acc(s).nav.dev.create = true).el(p)).child(docs);
    let body = if list.is_empty() {
        col()
            .w_full()
            .py(32.0)
            .gap(8.0)
            .items_center()
            .child(icon(&icons::APP_WINDOW, 48.0).color(p.text_tertiary))
            .child(text(t("No applications yet")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary))
            .child(muted(tf("Create your first application to get started with the {apiName}.", &[("apiName", "Fluxer API")]), p))
    } else {
        col().w_full().gap(8.0).children(list.iter().map(|a| item(a, p, acc)))
    };
    section(t("Applications"), col().w_full().gap(20.0).child(buttons).child(body), true, p)
}

/// `ApplicationsList` row: avatar (or initial), name, created date, chevron.
fn item<S: 'static>(a: &Value, p: &Palette, acc: ShellAccess<S>) -> El {
    let id = a["id"].as_str().unwrap_or_default().to_string();
    let name = a["name"].as_str().unwrap_or_default();
    let created = id.parse::<u64>().map(|n| reflux_model::Snowflake(n).timestamp_ms()).unwrap_or(0);
    let date = chrono::DateTime::from_timestamp_millis(created as i64).map(|d| d.format("%b %-d, %Y").to_string()).unwrap_or_default();
    let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let open = id.clone();
    row()
        .w_full()
        .min_h(72.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(12.0)
        .border(1.0, p.background_modifier_accent)
        .fill(p.background_textarea)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .key(format!("dev-app-{id}"))
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.nav.dev = Dev { app: Some(open.clone()), ..Dev::default() };
            sh.form.text.retain(|k, _| !k.starts_with("dev."));
            sh.form.data.retain(|k, _| k == "dev.apps");
        })
        .child(row().size(32.0, 32.0).center().radius(16.0).fill(p.brand_primary).child(text(initial).font(font(14.0, 600.0).lh(17.0)).color(p.brand_primary_fill)))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(name.to_string()).font(font(14.0, 500.0).lh(18.0)).color(p.text_primary).truncate())
                .child(text(tf("Created {createdAt}", &[("createdAt", &date)])).font(font(12.0, 400.0).lh(13.0)).color(p.text_primary_muted)),
        )
        .child(icon(&icons::CARET_RIGHT_BOLD, 24.0).color(p.text_tertiary))
}

/// `ApplicationCreateModal`.
fn create_modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let close = move |s: &mut S| acc(s).nav.dev.create = false;
    let name = st.form.value("dev.create", "");
    let create = move |s: &mut S| {
        let sh = acc(s);
        let name = sh.form.value("dev.create", "").trim().to_string();
        if name.is_empty() {
            return;
        }
        sh.nav.dev.create = false;
        sh.form.text.remove("dev.create");
        sh.api(Method::POST, "/oauth2/applications", Some(json!({ "name": name, "redirect_uris": [] })), Then::Reload("dev.apps".into(), LIST.into()));
    };
    Modal::new(t("Create application"), close)
        .size(ModalSize::Small)
        .body(super::input(st, acc, "dev.create", "", t("Application name"), t("My application"), false, p))
        .footer(
            row()
                .gap(8.0)
                .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
                .child(Button::new(t("Create")).key("dev-create-submit").disabled(name.trim().is_empty()).on_press(create).el(p)),
        )
        .el(p)
}

fn detail<S: 'static>(st: &ShellState, app: &Value, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let d = &st.nav.dev;
    let id = app["id"].as_str().unwrap_or_default().to_string();
    let bot = &app["bot"];
    let public = d.public.unwrap_or(app["bot_public"].as_bool().unwrap_or(false));
    let code_grant = d.code_grant.unwrap_or(app["bot_require_code_grant"].as_bool().unwrap_or(false));
    let redirects: Vec<String> = d.redirects.clone().unwrap_or_else(|| app["redirect_uris"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default());

    let copy_id = id.clone();
    let header = col()
        .w_full()
        .gap(12.0)
        .child(row().child(small_button(t("Back to list"), true, "dev-back", move |s: &mut S| acc(s).nav.dev = Dev::default(), p)))
        .child(text(app["name"].as_str().unwrap_or_default().to_string()).font(font(24.0, 700.0).lh(30.0)).color(p.text_primary))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .items_end()
                .child(col().grow(1.0).min_w(0.0).gap(8.0).child(label(t("Application ID"), p)).child(text(id.clone()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary).selectable(true)))
                .child(Button::new(t("Copy ID")).variant(ButtonVariant::Secondary).icon(&icons::COPY).key("dev-copy-id").on_press_tx(move |_: &mut S, tx: &mut rune::Tx| tx.copy(copy_id.clone())).el(p)),
        );

    // Application information.
    let mut redirect_rows = col().w_full().gap(8.0).child(label(t("Redirect URIs"), p));
    for (i, r) in redirects.iter().enumerate() {
        let all = redirects.clone();
        redirect_rows = redirect_rows.child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(text(r.clone()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary).grow(1.0).min_w(0.0).truncate())
                .child(
                    Button::new(String::new())
                        .icon(&icons::TRASH)
                        .square()
                        .variant(ButtonVariant::Secondary)
                        .key(format!("dev-redir-del-{i}"))
                        .on_press(move |s: &mut S| {
                            let mut v = all.clone();
                            v.remove(i);
                            acc(s).nav.dev.redirects = Some(v);
                        })
                        .el(p)
                        .label(t("Delete redirect URI")),
                ),
        );
    }
    let all = redirects.clone();
    redirect_rows = redirect_rows.child(
        row().w_full().gap(8.0).items_end().child(el().grow(1.0).min_w(0.0).child(super::input(st, acc, "dev.redirect", "", "", "https://example.com/callback", false, p))).child(small_button(
            t("Add redirect"),
            true,
            "dev-redir-add",
            move |s: &mut S| {
                let sh = acc(s);
                let r = sh.form.value("dev.redirect", "").trim().to_string();
                if !r.is_empty() {
                    let mut v = all.clone();
                    v.push(r);
                    sh.nav.dev.redirects = Some(v);
                    sh.form.text.remove("dev.redirect");
                }
            },
            p,
        )),
    );
    let info = card(
        Some((t("Application information"), Some(t("Basic settings and allowed redirect URIs.")), None)),
        vec![col()
            .w_full()
            .padding(16.0)
            .gap(16.0)
            .child(super::input(st, acc, "dev.name", app["name"].as_str().unwrap_or_default(), t("Application name"), "", false, p))
            .child(Switch::new(public).label(t("Public bot")).description(t("Allow anyone to invite this bot to their communities.")).on_toggle(move |s: &mut S| acc(s).nav.dev.public = Some(!public)).el(p))
            .child(
                Switch::new(code_grant)
                    .label(t("Require OAuth2 code grant"))
                    .description(t("Requires a redirect URI and an authorization code when inviting this bot."))
                    .on_toggle(move |s: &mut S| acc(s).nav.dev.code_grant = Some(!code_grant))
                    .el(p),
            )
            .child(redirect_rows)],
        p,
    );

    // Secrets & tokens: a reset shows the new value once.
    let shown = |key: &str, field: &str| st.form.data.get(key).and_then(|v| v[field].as_str()).map(String::from);
    let secret_row = |title: &str, key: &'static str, field: &'static str, path: String, p: &Palette| {
        let value = shown(key, field);
        let mut actions = Vec::new();
        if let Some(v) = value.clone() {
            actions.push(Button::new(t("Copy")).variant(ButtonVariant::Secondary).size(crate::ui::ButtonSize::Small).key(format!("{key}-copy")).on_press_tx(move |_: &mut S, tx: &mut rune::Tx| tx.copy(v.clone())).el(p));
        }
        actions.push(small_button(t("Regenerate"), false, &format!("{key}-reset"), move |s: &mut S| acc(s).api(Method::POST, path.clone(), Some(json!({})), Then::Keep(key.into())), p));
        card_row(title, Some(muted(value.unwrap_or_else(|| "••••••••••••••••••••".into()), p).selectable(true)), actions, p)
    };
    let mut secret_rows = vec![secret_row(t("Client secret"), "dev.secret", "client_secret", format!("/oauth2/applications/{id}/client-secret/reset"), p)];
    if !bot.is_null() {
        secret_rows.push(secret_row(t("Bot token"), "dev.token", "token", format!("/oauth2/applications/{id}/bot/reset-token"), p));
    }
    let secrets = card(Some((t("Secrets & tokens"), Some(t("Keep these safe. Regenerating will break existing integrations.")), None)), secret_rows, p);

    // Bot profile.
    let bot_card = (!bot.is_null()).then(|| {
        card(
            Some((t("Bot profile"), Some(t("Avatar, tag, and rich profile details for your bot.")), None)),
            vec![col()
                .w_full()
                .padding(16.0)
                .gap(16.0)
                .child(super::input(st, acc, "dev.bot_name", bot["username"].as_str().unwrap_or_default(), t("Username"), "", false, p))
                .child(super::input(st, acc, "dev.bot_bio", bot["bio"].as_str().unwrap_or_default(), t("Bot bio"), t("A helpful bot that does amazing things!"), false, p))],
            p,
        )
    });

    // OAuth2 URL builder.
    let scopes = col().w_full().gap(8.0).child(label(t("Scopes"), p)).children(SCOPES.iter().map(|sc| {
        let on = d.scopes.iter().any(|x| x == sc);
        let sc = sc.to_string();
        Checkbox::new(on).size(18.0).label(sc.clone()).on_toggle(move |s: &mut S| {
            let v = &mut acc(s).nav.dev.scopes;
            if on { v.retain(|x| *x != sc) } else { v.push(sc.clone()) }
        }).el(p)
    }));
    let mut builder = col().w_full().padding(16.0).gap(16.0).child(scopes);
    let opts: Vec<(&str, &str)> = redirects.iter().map(|r| (r.as_str(), r.as_str())).collect();
    builder = builder.child(col().w_full().gap(8.0).child(label(t("Redirect URI"), p)).child(
        Select::new("dev-redirect", &opts, d.redirect.as_deref())
            .placeholder(t("Select a redirect URI"))
            .on_choose(move |s: &mut S, v: &str| acc(s).nav.dev.redirect = Some(v.to_string()))
            .el(&st.popups, popup_access(move |s: &mut S| &mut acc(s).popups), p, layer),
    ));
    if d.scopes.iter().any(|s| s == "bot") {
        let perms = d.perms;
        let boxes: Vec<El> = crate::guild::perms::GUILD
            .iter()
            .flat_map(|(_, e)| e.iter())
            .map(|(bit, name, _)| {
                let b = bit.bits();
                Checkbox::new(perms & b != 0).size(18.0).label(t(name)).on_toggle(move |s: &mut S| acc(s).nav.dev.perms ^= b).el(p).w(0.0).grow(1.0)
            })
            .collect();
        let mut grid = col().w_full().gap(8.0);
        let mut it = boxes.into_iter().peekable();
        while it.peek().is_some() {
            let mut r = row().w_full().gap(8.0);
            for _ in 0..3 {
                r = r.child(it.next().unwrap_or_else(|| el().w(0.0).grow(1.0)));
            }
            grid = grid.child(r);
        }
        builder = builder.child(col().w_full().gap(8.0).child(label(t("Bot permissions"), p)).child(grid));
    }
    let url = authorize_url(&st.web_base, &id, &d.scopes, d.perms, d.redirect.as_deref(), code_grant);
    let shown_url = url.clone().unwrap_or_else(|| t("Select scopes (and redirect URI if required)").into());
    let copy = url.clone().unwrap_or_default();
    builder = builder.child(
        row()
            .w_full()
            .gap(8.0)
            .items_end()
            .child(col().grow(1.0).min_w(0.0).gap(8.0).child(label(t("Authorize URL"), p)).child(text(shown_url).font(font(14.0, 400.0).lh(20.0)).color(if url.is_some() { p.text_primary } else { p.text_primary_muted }).truncate()))
            .child(Button::new(t("Copy")).variant(ButtonVariant::Secondary).disabled(url.is_none()).key("dev-copy-url").on_press_tx(move |_: &mut S, tx: &mut rune::Tx| tx.copy(copy.clone())).el(p)),
    );
    let builder = card(Some((t("OAuth2 URL builder"), Some(t("Construct an authorization URL with scopes and permissions.")), None)), vec![builder], p);

    let (save_id, del_id, has_bot) = (id.clone(), id.clone(), !bot.is_null());
    let (init_name, init_bot, init_bio) = (app["name"].as_str().unwrap_or_default().to_string(), bot["username"].as_str().unwrap_or_default().to_string(), bot["bio"].as_str().unwrap_or_default().to_string());
    let save = move |s: &mut S| {
        let sh = acc(s);
        let mut patch = json!({ "name": sh.form.value("dev.name", &init_name).trim(), "bot_public": public, "bot_require_code_grant": code_grant });
        if let Some(r) = &sh.nav.dev.redirects {
            patch["redirect_uris"] = json!(r);
        }
        if has_bot {
            let bot = json!({ "username": sh.form.value("dev.bot_name", &init_bot).trim(), "bio": sh.form.value("dev.bot_bio", &init_bio) });
            sh.api(Method::PATCH, format!("/oauth2/applications/{save_id}/bot"), Some(bot), Then::Nothing);
        }
        sh.api(Method::PATCH, format!("/oauth2/applications/{save_id}"), Some(patch), Then::Saved { clear: "dev.".into(), reload: Some(("dev.apps".into(), LIST.into())) });
        (sh.nav.dev.public, sh.nav.dev.code_grant, sh.nav.dev.redirects) = (None, None, None);
    };
    let delete = move |s: &mut S| {
        let sh = acc(s);
        sh.nav.dev = Dev::default();
        sh.api(Method::DELETE, format!("/oauth2/applications/{del_id}"), Some(json!({})), Then::Reload("dev.apps".into(), LIST.into()));
    };
    let mut out = col().w_full().gap(24.0).child(header);
    if let Some(e) = &st.form.error {
        out = out.child(text(e.clone()).font(font(14.0, 400.0).lh(20.0)).color(p.status_danger).wrap());
    }
    out.child(info)
        .child(secrets)
        .children(bot_card)
        .child(builder)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .child(Button::new(t("Delete application")).variant(ButtonVariant::Danger).key("dev-delete").on_press(delete).el(p))
                .child(el().grow(1.0))
                .child(Button::new(t("Save changes")).key("dev-save").submitting(st.form.busy).on_press(save).el(p)),
        )
}

// ---- Embed debugger --------------------------------------------------------------------------

pub(super) fn embed_debugger<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let url = st.form.value("dev.url", "");
    let submit = move |s: &mut S| {
        let sh = acc(s);
        let url = sh.form.value("dev.url", "").trim().to_string();
        if url.is_empty() {
            return;
        }
        sh.form.data.remove("dev.unfurl");
        sh.form.error = None;
        sh.form.busy = true;
        sh.api(Method::POST, "/unfurl", Some(json!({ "url": url })), Then::Keep("dev.unfurl".into()));
    };
    let form = row()
        .w_full()
        .gap(8.0)
        .items_end()
        .child(el().grow(1.0).min_w(0.0).child(super::input(st, acc, "dev.url", "", t("URL"), "", false, p)))
        .child(Button::new(t("Unfurl")).key("dev-unfurl").disabled(url.trim().is_empty()).submitting(st.form.busy).on_press(submit).el(p).min_w(112.0));
    let mut first = col().w_full().gap(16.0).child(muted(t("Fetch a URL and inspect the embed payload the chat renderer receives."), p)).child(form);
    if let Some(e) = &st.form.error {
        first = first.child(text(e.clone()).font(font(14.0, 400.0).lh(20.0)).color(p.status_danger).wrap());
    }
    let mut out = col().w_full().child(section(t("Unfurl debugger"), first, true, p));
    let Some(embeds) = st.form.data.get("dev.unfurl") else { return out };
    let msg: Option<Message> = serde_json::from_value(json!({
        "id": "1", "channel_id": "0", "content": url.trim(), "timestamp": "2026-01-01T00:00:00Z", "embeds": embeds,
        "author": { "id": "0", "username": "Embed Debugger", "discriminator": "0000", "bot": true }
    }))
    .ok();
    let preview = match msg {
        Some(m) if !m.embeds.is_empty() => col().w_full().gap(8.0).items_start().children(crate::shell::embeds::embeds(store, st, &m, p, acc)),
        _ => muted(t("No embeds returned"), p),
    };
    let raw = serde_json::to_string_pretty(embeds).unwrap_or_default();
    let spans = crate::shell::highlight::spans("json", &raw, crate::ui::is_light(p));
    let code = col()
        .w_full()
        .padding(15.0)
        .radius(6.0)
        .fill(p.bg_code_block)
        .border(1.0, crate::ui::over(p.code_block_border, p.bg_code_block))
        .child(rich(raw, spans).font(font(12.0, 400.0).mono().lh(16.0)).color(p.text_primary).wrap().selectable(true));
    out = out.child(section(t("Preview"), preview, false, p)).child(section(t("Raw embed JSON"), code, false, p));
    out
}

/// Demo scenes: the list, the create modal, a bot's detail with a builder URL, an unfurl.
pub fn demo(sh: &mut ShellState, scene: &str) {
    sh.web_base = "https://web.fluxer.app".into();
    let tab = if scene == "embed-debugger" { "embed_debugger" } else { "applications" };
    sh.open_settings(Some(tab));
    let bot = json!({ "id": "1400000000000000001", "username": "Fluxbot", "discriminator": "0420", "bio": "Rolls dice." });
    sh.form.data.insert(
        "dev.apps".into(),
        json!([
            { "id": "1400000000000000001", "name": "Fluxbot", "redirect_uris": ["https://example.com/callback"], "bot_public": true, "bot": bot },
            { "id": "1450000000000000002", "name": "Weather widget", "redirect_uris": [] }
        ]),
    );
    match scene {
        "apps-create" => sh.nav.dev.create = true,
        "app-detail" => {
            sh.nav.dev = Dev { app: Some("1400000000000000001".into()), scopes: vec!["bot".into(), "identify".into()], redirect: Some("https://example.com/callback".into()), perms: 8 | 2048, ..Dev::default() };
            sh.form.data.insert("dev.secret".into(), json!({ "client_secret": "s3cr3t-Kq9vX2mN7pL4wR8tY1zB" }));
        }
        "embed-debugger" => {
            sh.form.text.insert("dev.url".into(), rune::TextEdit::new("https://fluxer.app"));
            sh.form.data.insert(
                "dev.unfurl".into(),
                json!([{ "type": "link", "url": "https://fluxer.app", "title": "Fluxer", "description": "A free and open source instant messaging and VoIP platform.", "color": 0x4641d9, "provider": { "name": "fluxer.app" } }]),
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::authorize_url;

    #[test]
    fn builds_authorize_urls() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(authorize_url("https://web.fluxer.app/", "42", &s(&["bot"]), 8, None, false).as_deref(), Some("https://web.fluxer.app/oauth2/authorize?client_id=42&scope=bot&permissions=8"));
        assert_eq!(authorize_url("w", "42", &s(&["bot"]), 0, None, true), None, "code grant needs a redirect");
        assert_eq!(authorize_url("w", "42", &s(&["identify"]), 8, None, false), None, "non-bot scopes need a redirect");
        assert_eq!(
            authorize_url("w", "1", &s(&["identify", "email"]), 8, Some("https://a.b/c"), false).as_deref(),
            Some("w/oauth2/authorize?client_id=1&scope=identify%20email&redirect_uri=https%3A%2F%2Fa.b%2Fc&response_type=code")
        );
        assert_eq!(authorize_url("w", "1", &[], 0, None, false), None);
    }
}
