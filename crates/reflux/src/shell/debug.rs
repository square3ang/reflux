//! Developer-mode debug modals (`devtools/components/debug/{DebugModal,RecordDebugModal,
//! MessageDebugModal}`): a record as pretty JSON behind "Debug user / channel / community /
//! member / message" menu items, and a client inspector (session and store counts, user
//! settings, the `/experiments` assignments) in place of the desktop app's Chromium devtools.

use reflux_model::Snowflake;
use reflux_store::Store;
use rune::{El, col, row, text};
use serde_json::{Value, json};

use super::{Intent, ShellAccess, ShellState};
use crate::i18n::t;
use crate::theme::{Palette, font};
use crate::ui::{MenuItem, Modal, ModalSize, scroller};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Subject {
    User(Snowflake),
    Channel(Snowflake),
    Guild(Snowflake),
    Member(Snowflake, Snowflake),
    Message(Snowflake, Snowflake),
    Client,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Debug {
    pub subject: Subject,
    pub tab: usize,
}

const EXPERIMENTS: &str = "debug-experiments";

impl ShellState {
    pub fn open_debug(&mut self, subject: Subject) {
        self.popups.close();
        self.debug = Some(Debug { subject, tab: 0 });
        if subject == Subject::Client {
            self.intents.push(Intent::Fetch { key: EXPERIMENTS.into(), method: reqwest::Method::GET, path: "/experiments".into(), body: None });
        }
    }
}

/// A developer-mode menu item that opens `subject`'s debug modal.
pub fn item<S: 'static>(id: &str, label: &str, subject: Subject, acc: ShellAccess<S>) -> MenuItem<S> {
    MenuItem::new(id.to_string(), label.to_string()).icon(&crate::icons::BUG).on_select(move |s: &mut S| acc(s).open_debug(subject))
}

fn json_of(v: impl serde::Serialize) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// `MessageDebugModal`'s AST tab: the content's and each embed's title, description and fields,
/// parsed, with their parse times; and the total in ms.
fn message_ast(m: &reflux_model::Message) -> (Value, f64) {
    let opts = reflux_markdown::Options { flags: reflux_markdown::presets::STANDARD, emoji: Default::default() };
    let mut total = 0.0;
    let mut parse = |s: &str| {
        let t0 = std::time::Instant::now();
        let ast = reflux_markdown::parse(s, &opts);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        total += ms;
        json!({ "content": s, "ast": ast, "parseTime": ms })
    };
    let mut out = serde_json::Map::new();
    if !m.content.is_empty() {
        out.insert("message_content".into(), parse(&m.content));
    }
    if !m.embeds.is_empty() {
        let embeds: Vec<Value> = m.embeds.iter().enumerate().map(|(i, e)| {
            let mut r = json!({ "embed_index": i, "embed_type": e.kind });
            for (k, v) in [("title", &e.title), ("description", &e.description)] {
                if let Some(v) = v.as_deref().filter(|v| !v.is_empty()) {
                    r[k] = parse(v);
                }
            }
            if !e.fields.is_empty() {
                r["fields"] = e.fields.iter().enumerate().map(|(j, f)| {
                    let mut fr = json!({ "field_index": j, "inline": f.inline });
                    if !f.name.is_empty() { fr["name"] = parse(&f.name); }
                    if !f.value.is_empty() { fr["value"] = parse(&f.value); }
                    fr
                }).collect();
            }
            r
        }).collect();
        out.insert("embeds".into(), embeds.into());
    }
    (if out.is_empty() { Value::Null } else { out.into() }, total)
}

/// (tab label, content, summary (label, value)).
type Tab = (String, String, Option<(String, String)>);

/// (title, tabs).
fn tabs(store: &Store, st: &ShellState, subject: Subject) -> (String, Vec<Tab>) {
    let pretty = |v: Value| if v.is_null() { "No data available".to_string() } else { serde_json::to_string_pretty(&v).unwrap_or_default() };
    let one = |title: &str, label: &str, v: Value| (title.to_string(), vec![(label.to_string(), pretty(v), None)]);
    match subject {
        Subject::User(u) => {
            let me = store.me.as_ref().filter(|m| m.id == u).map(json_of);
            one(t("User debug"), t("User record"), me.or_else(|| store.users.get(&u).map(json_of)).unwrap_or_default())
        }
        Subject::Channel(c) => one(t("Channel debug"), t("Channel record"), store.channels.get(&c).map(json_of).unwrap_or_default()),
        Subject::Guild(g) => {
            let v = store.guilds.get(&g).map(|gs| {
                json!({ "id": gs.id, "properties": gs.info, "unavailable": gs.unavailable, "visiting": gs.visiting, "joined_at": gs.joined_at,
                        "roles": gs.roles, "emojis": gs.emojis.len(), "stickers": gs.stickers.len(), "member_count": gs.member_count,
                        "online_count": gs.online_count, "members_cached": gs.members.len() })
            });
            one(t("Community debug"), t("Community record"), v.unwrap_or_default())
        }
        Subject::Member(g, u) => one(t("Community member debug"), t("Community member record"), store.member(g, u).map(json_of).unwrap_or_default()),
        Subject::Message(c, m) => {
            let msg = store.messages(c).iter().find(|x| x.id == m);
            let (ast, total) = msg.map_or((Value::Null, 0.0), message_ast);
            let summary = (!ast.is_null()).then(|| (t("Total parsing time:").to_string(), format!("{total:.2} ms")));
            (t("Message debug").to_string(), vec![(t("Message record").to_string(), pretty(msg.map(json_of).unwrap_or_default()), None), (t("Message AST").to_string(), pretty(ast), summary)])
        }
        Subject::Client => {
            let windows: usize = store.messages.values().map(|w| w.messages.len()).sum();
            let state = json!({ "session_id": store.session_id, "user_id": store.my_id(), "guilds": store.guilds.len(), "channels": store.channels.len(),
                "users": store.users.len(), "presences": store.presences.len(), "voice_states": store.voice_states.len(),
                "read_states": store.read_states.len(), "message_windows": store.messages.len(), "messages_loaded": windows,
                "relationships": store.relationships.len(), "guild" : st.guild, "channel": st.channel });
            let experiments = match st.expr.fetched.get(EXPERIMENTS) {
                Some(super::expr::Fetched::Done(v)) => pretty(v.clone()),
                Some(super::expr::Fetched::Failed(e)) => e.clone(),
                _ => t("Loading…").to_string(),
            };
            ("Developer tools".to_string(), vec![("State".into(), pretty(state), None), (t("User settings").to_string(), pretty(json_of(&store.settings)), None), ("Experiments".into(), experiments, None)])
        }
    }
}

pub fn modal<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let d = st.debug?;
    let (title, tabs) = tabs(store, st, d.subject);
    let active = d.tab.min(tabs.len().saturating_sub(1));
    let mut body = col().w_full().gap(12.0);
    if tabs.len() > 1 {
        let mut bar = row().gap(16.0);
        for (i, (label, ..)) in tabs.iter().enumerate() {
            let on = i == active;
            bar = bar.child(
                col()
                    .gap(6.0)
                    .cursor(rune::Cursor::Pointer)
                    .role("tab")
                    .checked(on)
                    .key(format!("debug-tab-{i}"))
                    .on_press(move |s: &mut S| {
                        if let Some(d) = &mut acc(s).debug {
                            d.tab = i;
                        }
                    })
                    .child(text(label.clone()).font(font(14.0, 600.0).lh(20.0)).color(if on { p.text_primary } else { p.text_primary_muted }))
                    .child(rune::el().h(2.0).w_full().fill(if on { p.brand_primary } else { rune::Color::TRANSPARENT })),
            );
        }
        body = body.child(bar);
    }
    // `.summary`: a card with an uppercase "Summary" heading and label / mono value rows.
    if let Some((label, value)) = tabs.get(active).and_then(|tab| tab.2.clone()) {
        body = body.child(
            col()
                .w_full()
                .padding(16.0)
                .radius(8.0)
                .fill(p.background_secondary)
                .border(1.0, p.background_modifier_accent)
                .child(text(t("Summary").to_uppercase()).font(font(14.0, 600.0).lh(20.0)).color(p.text_secondary).mb(8.0))
                .child(row().w_full().gap(12.0).child(text(label).font(font(14.0, 500.0).lh(20.0)).color(p.text_secondary)).child(rune::el().grow(1.0)).child(text(value).font(font(14.0, 400.0).lh(20.0).mono()).color(p.text_primary))),
        );
    }
    let code = tabs.get(active).map(|tab| tab.1.clone()).unwrap_or_default();
    let spans = if code.starts_with(['{', '[']) { super::highlight::spans("json", &code, crate::ui::is_light(p)) } else { Vec::new() };
    let block = col()
        .w_full()
        .padding(12.0)
        .radius(6.0)
        .fill(p.background_secondary_alt)
        .child(rune::rich(code, spans).font(font(13.0, 400.0).lh(18.0).mono()).color(p.text_primary).wrap().selectable(true));
    body = body.child(col().w_full().h(420.0).child(scroller(col().w_full().h_full().scroll(rune::ScrollSpec { stick_bottom: false }).child(block), false, p)));
    Some(Modal::new(title, move |s: &mut S| acc(s).debug = None).size(ModalSize::Large).body(body).el(p))
}

#[cfg(test)]
mod tests {
    #[test]
    fn message_ast_like_fluxer() {
        let mut m = reflux_model::Message { content: "**hi**".into(), ..Default::default() };
        m.embeds.push(reflux_model::Embed { kind: "rich".into(), title: Some("T".into()), fields: vec![reflux_model::EmbedField { name: "n".into(), value: String::new(), inline: true }], ..Default::default() });
        let (v, total) = super::message_ast(&m);
        assert_eq!(v["message_content"]["content"], "**hi**");
        assert!(v["message_content"]["ast"].is_array());
        assert_eq!(v["embeds"][0]["embed_type"], "rich");
        assert_eq!(v["embeds"][0]["title"]["content"], "T");
        assert!(v["embeds"][0].get("description").is_none());
        assert_eq!(v["embeds"][0]["fields"][0]["inline"], true);
        assert!(v["embeds"][0]["fields"][0].get("value").is_none());
        assert!(total >= 0.0);
        assert!(super::message_ast(&reflux_model::Message::default()).0.is_null());
    }
}
