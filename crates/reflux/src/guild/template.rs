//! "Import Discord template" in Add a community (`add_guild_modal/TemplateImportForm.tsx`):
//! a template link → the template looked up directly (the desktop app's
//! `OtherPlatformTemplateIpc`), or its JSON pasted from the browser → name and icon →
//! `POST /guilds` with the serialized guild as `template`.

use reqwest::Method;
use rune::{El, col, icon, row, text};
use serde_json::{Map, Value, json};

use super::{AddView, Dialog, Then, error, field, textarea};
use crate::i18n::{t, tf};
use crate::icons;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Modal, over};

/// `THE_OTHER_PLATFORM`.
pub const PLATFORM: &str = "Discord";

/// `TemplateImportStep`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Url,
    Json,
    Create,
}

/// `parseTemplateCode`: a `discord.new/…`, API or `discord.com/template/…` link, or a bare code.
pub fn parse_code(input: &str) -> Option<String> {
    let s = input.trim();
    let code_after = |marker: &str| {
        let rest = &s[s.find(marker)? + marker.len()..];
        let code: String = rest.chars().take_while(char::is_ascii_alphanumeric).collect();
        (!code.is_empty()).then_some(code)
    };
    code_after("discord.new/")
        .or_else(|| code_after("discord.com/api/guilds/templates/"))
        .or_else(|| code_after("discord.com/template/"))
        .or_else(|| (!s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric())).then(|| s.to_string()))
}

fn entity_id(v: &Value) -> bool {
    v.as_u64().is_some() || v.as_str().is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

fn bitfield(v: &Value) -> bool {
    v.is_string() || v.as_u64().is_some()
}

/// A key, its type check, and whether null is accepted (`v.nullish`).
type Field<'a> = (&'a str, fn(&Value) -> bool, bool);

/// Copies `keys` of `src` that are present (valibot's `v.object` drops the rest); `None` when a
/// present key fails `ok`. `nullish` keys accept null.
fn pick(src: &Map<String, Value>, keys: &[Field]) -> Option<Map<String, Value>> {
    let mut out = Map::new();
    for &(k, ok, nullish) in keys {
        match src.get(k) {
            None => {}
            Some(Value::Null) if nullish => {
                out.insert(k.into(), Value::Null);
            }
            Some(v) if ok(v) => {
                out.insert(k.into(), v.clone());
            }
            Some(_) => return None,
        }
    }
    Some(out)
}

const fn any(_: &Value) -> bool {
    true
}

/// `GuildTemplateResponseSchema`: the template's name and its `serialized_source_guild` as
/// valibot outputs it, or `None` when the response doesn't have that shape.
pub fn parse_response(v: &Value) -> Option<(String, Value)> {
    v["code"].as_str()?;
    let name = v["name"].as_str()?.to_string();
    if !(v["description"].is_null() || v["description"].is_string()) {
        return None;
    }
    let g = v["serialized_source_guild"].as_object()?;
    let num = |v: &Value| v.is_number();
    let s = |v: &Value| v.is_string();
    let mut out = pick(g, &[
        ("name", s, false),
        ("description", s, true),
        ("verification_level", num, false),
        ("default_message_notifications", num, false),
        ("explicit_content_filter", num, false),
        ("system_channel_id", entity_id, true),
        ("afk_timeout", num, false),
        ("system_channel_flags", num, false),
    ])?;
    out.get("name")?;
    let roles = g.get("roles")?.as_array()?.iter().map(|r| {
        let r = r.as_object()?;
        if !entity_id(r.get("id")?) {
            return None;
        }
        let mut o = pick(r, &[
            ("id", any, false),
            ("name", s, true),
            ("permissions", bitfield, false),
            ("permissions_new", bitfield, false),
            ("color", num, false),
            ("hoist", Value::is_boolean, false),
            ("mentionable", Value::is_boolean, false),
            ("unicode_emoji", s, true),
        ])?;
        // `v.nullish(v.string(), '')`.
        if o.get("name").is_none_or(Value::is_null) {
            o.insert("name".into(), json!(""));
        }
        Some(Value::Object(o))
    });
    let roles: Vec<Value> = roles.collect::<Option<_>>()?;
    let channels = g.get("channels")?.as_array()?.iter().map(|c| {
        let mut c = c.as_object()?.clone();
        if !entity_id(c.get("id")?) || !c.get("type")?.is_number() || !c.get("position")?.is_number() {
            return None;
        }
        if c.get("name").is_none_or(Value::is_null) {
            c.insert("name".into(), json!(""));
        }
        let nullish = |c: &Map<String, Value>, k: &str, ok: fn(&Value) -> bool| c.get(k).is_none_or(|v| v.is_null() || ok(v));
        let ok = nullish(&c, "topic", s)
            && nullish(&c, "parent_id", entity_id)
            && ["bitrate", "user_limit", "voice_connection_limit", "rate_limit_per_user"].iter().all(|k| nullish(&c, k, num))
            && nullish(&c, "nsfw", Value::is_boolean);
        if !ok {
            return None;
        }
        if let Some(ows) = c.get("permission_overwrites").and_then(Value::as_array) {
            let ows = ows.iter().map(|o| {
                if !(entity_id(&o["id"]) && bitfield(&o["allow"]) && bitfield(&o["deny"])) {
                    return None;
                }
                // `OverwriteType`: "role" → 0, "member" → 1, other strings as numbers.
                let kind = match &o["type"] {
                    Value::Number(n) => Value::Number(n.clone()),
                    Value::String(s) if s == "role" => json!(0),
                    Value::String(s) if s == "member" => json!(1),
                    Value::String(s) => s.parse::<f64>().ok().map_or(Value::Null, |n| json!(n)),
                    _ => return None,
                };
                Some(json!({"id": o["id"], "type": kind, "allow": o["allow"], "deny": o["deny"]}))
            });
            c.insert("permission_overwrites".into(), Value::Array(ows.collect::<Option<_>>()?));
        } else if c.get("permission_overwrites").is_some_and(|v| !v.is_null()) {
            return None;
        }
        Some(Value::Object(c))
    });
    let channels: Vec<Value> = channels.collect::<Option<_>>()?;
    out.insert("roles".into(), Value::Array(roles));
    out.insert("channels".into(), Value::Array(channels));
    Some((name, Value::Object(out)))
}

/// `mapTemplateChannelTypeToFluxer`: stage channels become voice; unknown types are dropped.
fn fluxer_type(t: i64) -> Option<i64> {
    match t {
        0 | 2 | 4 | 5 | 15 | 16 => Some(t),
        13 => Some(2),
        _ => None,
    }
}

/// `templateStats`: text, forum, voice channels, categories and roles (without @everyone).
pub fn stats(g: &Value) -> [usize; 5] {
    let types: Vec<i64> = g["channels"].as_array().into_iter().flatten().filter_map(|c| fluxer_type(c["type"].as_i64()?)).collect();
    let count = |ts: &[i64]| types.iter().filter(|t| ts.contains(t)).count();
    let everyone = |r: &Value| r["name"] == "@everyone" || r["id"].as_str() == Some("0") || r["id"].as_u64() == Some(0);
    let roles = g["roles"].as_array().into_iter().flatten().filter(|r| !everyone(r)).count();
    [count(&[0, 5]), count(&[15, 16]), count(&[2]), count(&[4]), roles]
}

/// `TEMPLATE_STATS_DESCRIPTOR` (`…_WITH_FORUMS_…` when there are forums).
fn stats_text(g: &Value) -> String {
    let [text_n, forum, voice, cat, roles] = stats(g).map(|n| n.to_string());
    let args = [("textChannelCount", text_n.as_str()), ("forumChannelCount", forum.as_str()), ("voiceChannelCount", voice.as_str()), ("categoryCount", cat.as_str()), ("roleCount", roles.as_str())];
    if forum != "0" {
        tf("{textChannelCount, plural, one {# text channel} other {# text channels}}, {forumChannelCount, plural, one {# forum channel} other {# forum channels}}, {voiceChannelCount, plural, one {# voice channel} other {# voice channels}}, {categoryCount, plural, one {# category} other {# categories}}, {roleCount, plural, one {# role} other {# roles}}", &args)
    } else {
        tf("{textChannelCount, plural, one {# text channel} other {# text channels}}, {voiceChannelCount, plural, one {# voice channel} other {# voice channels}}, {categoryCount, plural, one {# category} other {# categories}}, {roleCount, plural, one {# role} other {# roles}}", &args)
    }
}

/// The template API link the paste step opens.
fn api_url(code: &str) -> String {
    format!("https://discord.com/api/guilds/templates/{code}")
}

/// Keeps a parsed template and moves on to naming the community.
pub(crate) fn apply(sh: &mut ShellState, name: String, guild: Value, direct: bool) {
    let gname = guild["name"].as_str().unwrap_or_default().to_string();
    sh.form.data.insert("tpl".into(), guild);
    sh.form.data.insert("tpl-name".into(), json!(name));
    sh.form.data.insert("tpl-direct".into(), json!(direct));
    sh.form.text.insert("name".into(), rune::TextEdit::new(gname));
    sh.form.error = None;
    sh.dialog = Some(Dialog::Add(AddView::Import(Step::Create)));
}

fn to_paste(sh: &mut ShellState, code: &str, attempted: bool) {
    sh.form.data.insert("tpl-api".into(), json!(api_url(code)));
    sh.form.data.insert("tpl-failed".into(), json!(attempted));
    sh.form.data.insert("tpl-direct".into(), json!(false));
    sh.form.error = None;
    sh.dialog = Some(Dialog::Add(AddView::Import(Step::Json)));
}

/// `OtherPlatformTemplateFetchFailure`.
#[derive(Debug, PartialEq)]
enum Failure {
    NotFound,
    RateLimited,
    /// Network, unexpected status or an unreadable body: fall back to pasting.
    Other,
}

/// `fetchOtherPlatformTemplate`: GET the public template endpoint (15 s, 4 MiB).
async fn fetch(code: String) -> Result<Value, Failure> {
    let http = reflux_net::http_client();
    let send = http.get(format!("https://discord.com/api/v10/guilds/templates/{code}")).header("Accept", "application/json").timeout(std::time::Duration::from_secs(15)).send();
    let r = send.await.map_err(|_| Failure::Other)?;
    match r.status().as_u16() {
        404 => return Err(Failure::NotFound),
        429 => return Err(Failure::RateLimited),
        s if !(200..300).contains(&s) => return Err(Failure::Other),
        _ => {}
    }
    if r.content_length().is_some_and(|n| n > 4 << 20) {
        return Err(Failure::Other);
    }
    let body = r.bytes().await.map_err(|_| Failure::Other)?;
    if body.len() > 4 << 20 {
        return Err(Failure::Other);
    }
    serde_json::from_slice::<Value>(&body).ok().filter(Value::is_object).ok_or(Failure::Other)
}

fn invalid_code() -> String {
    tf("Enter a valid {theOtherPlatform} template URL or code.", &[("theOtherPlatform", PLATFORM)])
}

/// "Next" on the link step.
fn resolve(sh: &mut ShellState) {
    let Some(code) = parse_code(&sh.form.value("tpl-url", "")) else {
        sh.form.error = Some(invalid_code());
        return;
    };
    if sh.form.busy {
        return;
    }
    sh.form.busy = true;
    sh.form.error = None;
    crate::app::spawn(async move {
        let r = fetch(code.clone()).await;
        crate::app::update(move |s| {
            let sh = &mut s.app.shell;
            sh.form.busy = false;
            if sh.dialog != Some(Dialog::Add(AddView::Import(Step::Url))) {
                return;
            }
            let p = [("theOtherPlatform", PLATFORM)];
            match r {
                Ok(v) => match parse_response(&v) {
                    Some((name, g)) => apply(sh, name, g, true),
                    None => sh.form.error = Some(tf("This {theOtherPlatform} template couldn't be read. It may use a format Fluxer doesn't support yet.", &p)),
                },
                Err(Failure::NotFound) => sh.form.error = Some(tf("We couldn't find that {theOtherPlatform} template. Check the link and try again.", &p)),
                Err(Failure::RateLimited) => sh.form.error = Some(tf("{theOtherPlatform} is limiting template lookups right now. Try again in a moment.", &p)),
                Err(Failure::Other) => to_paste(sh, &code, true),
            }
        });
    });
}

/// "Next" on the paste step.
fn parse_pasted(sh: &mut ShellState) {
    let raw = sh.form.value("tpl-json", "");
    match serde_json::from_str::<Value>(&raw) {
        Err(_) => sh.form.error = Some(t("Invalid JSON. Paste the full response from the link above.").into()),
        Ok(v) => match parse_response(&v) {
            Some((name, g)) => apply(sh, name, g, false),
            None => sh.form.error = Some(t("This doesn't look like a valid template response. Make sure you copied the entire JSON document.").into()),
        },
    }
}

/// The modal for `step`.
pub(super) fn view<S: 'static>(st: &ShellState, step: Step, p: &Palette, acc: ShellAccess<S>) -> El {
    let go = move |v: AddView| move |s: &mut S| {
        let sh = acc(s);
        sh.form.error = None;
        sh.dialog = Some(Dialog::Add(v));
    };
    let back = |to: AddView| Button::new(t("Back")).variant(ButtonVariant::Secondary).on_press(go(to)).el(p);
    let desc = |s: String| text(s).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap();
    let mut body = col().w_full().gap(12.0);
    let footer = match step {
        Step::Url => {
            body = body
                .child(desc(tf("Paste a {theOtherPlatform} template URL to import its structure into a new community.", &[("theOtherPlatform", PLATFORM)])))
                .child(field(st, acc, "tpl-url", "", t("Template URL"), "https://discord.new/abcd1234", p));
            let empty = st.form.value("tpl-url", "").trim().is_empty();
            let next = Button::new(t("Next")).disabled(empty).submitting(st.form.busy).key("tpl-url-next").on_press(move |s: &mut S| resolve(acc(s)));
            vec![back(AddView::Landing), next.el(p)]
        }
        Step::Json => {
            let failed = st.form.data.get("tpl-failed").and_then(Value::as_bool).unwrap_or(false);
            let api = st.form.data.get("tpl-api").and_then(Value::as_str).unwrap_or_default().to_string();
            let open = Button::new(t("Open template JSON"))
                .variant(ButtonVariant::Secondary)
                .icon(&icons::ARROW_SQUARE_OUT)
                .on_press_tx(move |s: &mut S, tx| crate::shell::links::open(acc(s), &api, tx));
            body = body
                .child(desc(if failed {
                    t("Couldn't load the template automatically. Open the link below in your browser, then copy the entire JSON response and paste it into the box.").into()
                } else {
                    t("Open the link below in your browser, then copy the entire JSON response and paste it into the box.").into()
                }))
                .child(row().child(open.el(p)))
                .child(textarea(st, acc, "tpl-json", "", t("Template JSON"), t("Paste the JSON response here..."), 6.0 * 20.0 + 16.0, p));
            let empty = st.form.value("tpl-json", "").trim().is_empty();
            let next = Button::new(t("Next")).disabled(empty).key("tpl-json-next").on_press(move |s: &mut S| parse_pasted(acc(s)));
            vec![back(AddView::Import(Step::Url)), next.el(p)]
        }
        Step::Create => {
            if let (Some(name), Some(g)) = (st.form.data.get("tpl-name").and_then(Value::as_str), st.form.data.get("tpl")) {
                // `.templatePreview`.
                body = body.child(
                    col()
                        .w_full()
                        .gap(8.0)
                        .padding(12.0)
                        .radius(8.0)
                        .fill(p.background_secondary)
                        .border(1.0, over(p.background_modifier_accent, p.background_secondary))
                        .child(
                            col()
                                .gap(2.0)
                                .child(text(t("Template").to_uppercase()).font(font(11.0, 600.0).lh(14.0).tracking(0.22)).color(p.text_primary_muted))
                                .child(text(name.to_string()).font(font(14.0, 600.0).lh(18.0)).color(p.text_primary).wrap()),
                        )
                        .child(text(stats_text(g)).font(font(13.0, 400.0).lh(17.0)).color(p.text_primary_muted).wrap()),
                );
            }
            body = body.child(super::modals::create_fields(st, p, acc));
            let direct = st.form.data.get("tpl-direct").and_then(Value::as_bool).unwrap_or(false);
            let empty = st.form.value("name", "").trim().is_empty();
            let create = move |s: &mut S| {
                let sh = acc(s);
                let Some(g) = sh.form.data.get("tpl").cloned() else { return };
                let mut b = json!({ "name": sh.form.value("name", "").trim(), "template": g });
                if let Some(i) = sh.form.data.get("icon") {
                    b["icon"] = i.clone();
                }
                sh.api(Method::POST, "/guilds", Some(b), Then::Enter);
            };
            let to = if direct { Step::Url } else { Step::Json };
            vec![back(AddView::Import(to)), Button::new(t("Create community")).disabled(empty).submitting(st.form.busy).key("tpl-create").on_press(create).el(p)]
        }
    };
    if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    let close = move |s: &mut S| acc(s).dialog = None;
    Modal::new(tf("Import {theOtherPlatform} template", &[("theOtherPlatform", PLATFORM)]), close).body(body).footer(row().gap(8.0).children(footer)).el(p)
}

/// The landing view's wide "Import Discord template" tile.
pub(super) fn landing_tile<S: 'static>(p: &Palette, acc: ShellAccess<S>) -> El {
    let label = tf("Import {theOtherPlatform} template", &[("theOtherPlatform", PLATFORM)]);
    row()
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .center()
        .radius(8.0)
        .fill(p.background_secondary)
        .border(1.0, over(p.background_modifier_accent, p.background_secondary))
        .hover(p.background_secondary_alt)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.clone())
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.form.error = None;
            sh.dialog = Some(Dialog::Add(AddView::Import(Step::Url)));
        })
        .child(row().size(48.0, 48.0).center().radius(24.0).fill(p.brand_primary).child(icon(&icons::DOWNLOAD_SIMPLE, 24.0).color(p.brand_primary_fill)))
        .child(text(label).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_codes_parse_like_fluxer() {
        assert_eq!(parse_code(" https://discord.new/abcD1234 ").as_deref(), Some("abcD1234"));
        assert_eq!(parse_code("https://discord.com/api/guilds/templates/XyZ9").as_deref(), Some("XyZ9"));
        assert_eq!(parse_code("https://discord.com/template/q1w2?x=1").as_deref(), Some("q1w2"));
        assert_eq!(parse_code("abc123").as_deref(), Some("abc123"));
        assert_eq!(parse_code("not a code"), None);
        assert_eq!(parse_code(""), None);
    }

    fn sample() -> Value {
        json!({
            "code": "abc", "name": "Gaming", "description": null, "usage_count": 3,
            "serialized_source_guild": {
                "name": "Gaming Hub", "description": null, "region": "us", "verification_level": 1,
                "roles": [{"id": 0, "name": "@everyone", "permissions": "104324673", "icon": null}, {"id": 1, "name": null, "color": 0, "hoist": true}],
                "channels": [
                    {"id": 2, "type": 4, "name": "Text", "position": 0, "parent_id": null},
                    {"id": 3, "type": 0, "name": "general", "position": 0, "parent_id": 2, "permission_overwrites": [{"id": 0, "type": "role", "allow": "0", "deny": "2048"}]},
                    {"id": 4, "type": 13, "name": "stage", "position": 1, "parent_id": null},
                    {"id": 5, "type": 15, "name": "forum", "position": 2, "parent_id": null},
                    {"id": 6, "type": 14, "name": "directory", "position": 3}
                ]
            }
        })
    }

    #[test]
    fn responses_are_validated_and_stripped_like_valibot() {
        let (name, g) = parse_response(&sample()).expect("valid");
        assert_eq!(name, "Gaming");
        assert!(g.get("region").is_none(), "unknown guild keys dropped");
        assert!(g["roles"][0].get("icon").is_none(), "unknown role keys dropped");
        assert_eq!(g["roles"][1]["name"], "", "null names default to empty");
        assert_eq!(g["channels"][1]["permission_overwrites"][0]["type"], 0, "role overwrite type mapped");
        assert_eq!(stats(&g), [1, 1, 1, 1, 1]);
        assert_eq!(stats_text(&g), "1 text channel, 1 forum channel, 1 voice channel, 1 category, 1 role");
        let mut bad = sample();
        bad["serialized_source_guild"]["channels"][0]["position"] = json!("x");
        assert!(parse_response(&bad).is_none());
        assert!(parse_response(&json!({"name": "x"})).is_none());
    }
}
