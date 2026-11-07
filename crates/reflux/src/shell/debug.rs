//! Developer-mode debug modals (`devtools/components/debug/{DebugModal,RecordDebugModal,
//! MessageDebugModal}`): a record as pretty JSON behind "Debug user / channel / community /
//! member / message" menu items, and a client inspector (session and store counts, user
//! settings, the `/experiments` assignments) in place of the desktop app's Chromium devtools.
// shortcut: the JSON is plain monospace text (Fluxer runs it through its code-block highlighter).

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

/// (title, [(tab label, content)]).
fn tabs(store: &Store, st: &ShellState, subject: Subject) -> (String, Vec<(String, String)>) {
    let pretty = |v: Value| if v.is_null() { "No data available".to_string() } else { serde_json::to_string_pretty(&v).unwrap_or_default() };
    let one = |title: &str, label: &str, v: Value| (title.to_string(), vec![(label.to_string(), pretty(v))]);
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
            let ast = msg.map_or_else(String::new, |m| {
                let opts = reflux_markdown::Options { flags: reflux_markdown::presets::STANDARD, emoji: Default::default() };
                format!("{:#?}", reflux_markdown::parse(&m.content, &opts))
            });
            (t("Message debug").to_string(), vec![(t("Message record").to_string(), pretty(msg.map(json_of).unwrap_or_default())), (t("Message AST").to_string(), ast)])
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
            ("Developer tools".to_string(), vec![("State".into(), pretty(state)), (t("User settings").to_string(), pretty(json_of(&store.settings))), ("Experiments".into(), experiments)])
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
        for (i, (label, _)) in tabs.iter().enumerate() {
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
    let code = tabs.get(active).map(|(_, c)| c.clone()).unwrap_or_default();
    let block = col()
        .w_full()
        .padding(12.0)
        .radius(6.0)
        .fill(p.background_secondary_alt)
        .child(text(code).font(font(13.0, 400.0).lh(18.0).mono()).color(p.text_primary).wrap().selectable(true));
    body = body.child(col().w_full().h(420.0).child(scroller(col().w_full().h_full().scroll(rune::ScrollSpec { stick_bottom: false }).child(block), false, p)));
    Some(Modal::new(title, move |s: &mut S| acc(s).debug = None).size(ModalSize::Large).body(body).el(p))
}
