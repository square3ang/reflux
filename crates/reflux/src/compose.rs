//! The rich composer: mention, channel and emoji pills (`lexical/nodes`), the selection
//! formatting toolbar (`lexical/composer/SelectionFormattingToolbar`), the edit groups of
//! `TextareaContextMenu`, and the slash commands that act instead of sending
//! (`devtools/utils/CommandUtils`).

use reflux_model::{Permissions, Snowflake};
use rune::{El, Icon, Shadow, TextEdit, Tx, icon, row};
use crate::ui::menu::{Menu, MenuItem};
use serde_json::json;

use crate::icons;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::Palette;

// ---- pills ----

/// The pill an autocomplete insert becomes: `<@id> `, `<#id> ` or `<a:name:id> ` and the
/// completion's label give `(token, label)`; anything else stays text.
// shortcut: a custom emoji pill shows `:name:`, not the emoji image.
pub fn pill_for(insert: &str, label: &str) -> Option<(String, String)> {
    let token = insert.trim_end();
    if !(token.starts_with('<') && token.ends_with('>')) {
        return None;
    }
    let label = match token.as_bytes().get(1) {
        Some(b'@') => format!("@{label}"),
        Some(b'#') => label.to_string(),
        _ => label.to_string(),
    };
    Some((token.to_string(), label))
}

/// Replaces `range` with a pill and a space after it, as one undo step.
pub fn put_pill(edit: &mut TextEdit, range: std::ops::Range<usize>, token: String, label: &str) {
    edit.undo_group(|e| {
        e.replace_with_pill(range, token, label);
        e.insert(" ");
    });
}

/// The text to send: every pill's label swapped back for its token.
pub fn serialize(edit: &TextEdit) -> String {
    let mut out = String::with_capacity(edit.text.len());
    let mut at = 0;
    for (r, id) in edit.pills() {
        out.push_str(&edit.text[at..r.start]);
        out.push_str(id);
        at = r.end;
    }
    out.push_str(&edit.text[at..]);
    out
}

/// A channel pill's leading glyph.
pub fn pill_icon(id: &str) -> Option<&'static Icon> {
    id.starts_with("<#").then_some(&icons::HASH)
}

/// Spelling underlines outside pills.
pub fn unpilled(spans: Vec<rune::Span>, edit: &TextEdit) -> Vec<rune::Span> {
    let pills: Vec<_> = edit.pills().map(|(r, _)| r).collect();
    spans.into_iter().filter(|s| !pills.iter().any(|r| (s.start as usize) < r.end && r.start < s.end as usize)).collect()
}

// ---- selection formatting toolbar ----

/// `FORMATS`: label, icon, Markdown wrapper.
pub const FORMATS: [(&str, &Icon, &str); 6] = [
    ("Bold", &icons::TEXT_B, "**"),
    ("Italic", &icons::TEXT_ITALIC, "*"),
    ("Underline", &icons::TEXT_UNDERLINE, "__"),
    ("Strikethrough", &icons::TEXT_STRIKETHROUGH, "~~"),
    ("Inline code", &icons::CODE, "`"),
    ("Spoiler", &icons::EYE_SLASH, "||"),
];

/// How many `c` sit just before `a` and just after `b`.
fn runs(text: &str, a: usize, b: usize, c: char) -> (usize, usize) {
    (text[..a].chars().rev().take_while(|x| *x == c).count(), text[b..].chars().take_while(|x| *x == c).count())
}

/// Whether `w` wraps the selection from outside (`**|sel|**`). `*` only counts on an odd
/// run, so bold isn't italic too.
fn wrapped_outside(text: &str, a: usize, b: usize, w: &str) -> bool {
    let c = w.chars().next().unwrap_or('*');
    let (l, r) = runs(text, a, b, c);
    let n = w.chars().count();
    if w == "*" { l % 2 == 1 && r % 2 == 1 } else if c == '*' { l >= n && r >= n && l != 1 && r != 1 } else { l >= n && r >= n }
}

/// Whether `w` is active on the selection (outside, or the selected text itself is wrapped).
pub fn active(edit: &TextEdit, w: &str) -> bool {
    let (a, b) = edit.range();
    let sel = &edit.text[a..b];
    wrapped_outside(&edit.text, a, b, w) || (sel.len() >= 2 * w.len() && sel.starts_with(w) && sel.ends_with(w) && (w != "*" || !sel.starts_with("**")))
}

/// Toggles `w` around the selection and keeps the same text selected (`$wrapComposerSelection`).
/// Edits go in at the ends, so pills inside survive.
pub fn toggle(edit: &mut TextEdit, w: &str) {
    let (a, b) = edit.range();
    if a == b {
        return;
    }
    let n = w.len();
    let sel = edit.text[a..b].to_string();
    edit.undo_group(|e| {
        if wrapped_outside(&e.text, a, b, w) {
            e.set_selection(b, b + n);
            e.insert("");
            e.set_selection(a - n, a);
            e.insert("");
            e.set_selection(a - n, b - n);
        } else if sel.len() >= 2 * n && sel.starts_with(w) && sel.ends_with(w) {
            e.set_selection(b - n, b);
            e.insert("");
            e.set_selection(a, a + n);
            e.insert("");
            e.set_selection(a, b - 2 * n);
        } else {
            e.set_caret(b);
            e.insert(w);
            e.set_caret(a);
            e.insert(w);
            e.set_selection(a + n, b + n);
        }
    });
}

thread_local! {
    /// The composer's caret line `[x, y, w, h]` (window points) while it has a selection, read
    /// from `Cx::caret` by the root view; the format toolbar floats above it.
    pub static CARET: std::cell::Cell<Option<[f32; 4]>> = const { std::cell::Cell::new(None) };
}

/// The format toolbar over the composer's selection (`FormattingToolbar`), in the root layer.
pub fn floating_toolbar<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some(c) = CARET.get() else { return };
    let [x, y] = crate::desktop::window::unzoom([c[0], c[1]]);
    layer.push(rune::el().key("format-caret").absolute(x, y).size(1.0, 1.0));
    layer.push(toolbar(&st.composer, p, acc).anchored("format-caret", rune::Side::Top, rune::Align::Center, 8.0));
}

/// `.toolbar`: 3 padding, 2 gap, radius 8, 1px border, 0 4 12 shadow; 28px buttons with an
/// 18px icon, radius 5. Pressing keeps the composer focused.
// shortcut: Rune exposes the caret's rect, not the selection's, so the bar centres over the
// caret end of the selection; no Alt+F10 roving.
pub fn toolbar<S: 'static>(edit: &TextEdit, p: &Palette, acc: ShellAccess<S>) -> El {
    let mut bar = row()
        .key("format-toolbar")
        .gap(2.0)
        .px(3.0).py(3.0)
        .radius(8.0)
        .fill(p.background_secondary)
        .border(1.0, p.background_modifier_accent)
        .box_shadow(Shadow { x: 0.0, y: 4.0, blur: 12.0, spread: 0.0, color: crate::ui::rgba(0, 0, 0, 0.24), inset: false })
        .preserve_focus_on_press(true)
        .role("toolbar");
    for (label, ic, w) in FORMATS {
        let on = active(edit, w);
        bar = bar.child(
            row()
                .size(28.0, 28.0)
                .center()
                .radius(5.0)
                .fill(if on { p.background_modifier_selected } else { rune::Color::TRANSPARENT })
                .hover(p.background_modifier_hover)
                .hover_group()
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(label.to_string())
                .child(icon(ic, 18.0).color(if on { p.text_primary } else { p.text_primary_muted }).group_color(p.text_primary))
                .on_press(move |s: &mut S| toggle(&mut acc(s).composer, w)),
        );
    }
    bar
}

// ---- composer context menu ----

/// `TextareaContextMenu`'s Undo/Redo and Cut/Copy/Paste/Select all groups, after the spelling
/// items.
// shortcut: Undo and Redo are always enabled (Rune doesn't expose the history depth).
pub fn edit_items<S: 'static>(st: &ShellState, mut m: Menu<S>, after_spelling: bool, acc: ShellAccess<S>) -> Menu<S> {
    let close = move |s: &mut S| {
        let sh = acc(s);
        sh.desktop.spell_menu = None;
        sh.popups.close();
    };
    let has_sel = st.composer.range().0 != st.composer.range().1;
    if after_spelling {
        m = m.separator();
    }
    m.item(MenuItem::new("undo", "Undo").icon(&icons::ARROW_COUNTER_CLOCKWISE).on_select(move |s: &mut S| {
        acc(s).composer.undo();
        close(s);
    }))
    .item(MenuItem::new("redo", "Redo").icon(&icons::ARROW_CLOCKWISE).on_select(move |s: &mut S| {
        acc(s).composer.redo();
        close(s);
    }))
    .separator()
    .item(MenuItem::new("cut", "Cut").icon(&icons::SCISSORS).disabled(!has_sel).on_select_tx(move |s: &mut S, tx: &mut Tx| {
        let c = acc(s);
        tx.copy(c.composer.selected().to_string());
        c.composer.insert("");
        close(s);
    }))
    .item(MenuItem::new("copy", "Copy").icon(&icons::COPY).disabled(!has_sel).on_select_tx(move |s: &mut S, tx: &mut Tx| {
        tx.copy(acc(s).composer.selected().to_string());
        close(s);
    }))
    .item(MenuItem::new("paste", "Paste").icon(&icons::CLIPBOARD_TEXT).on_select_tx(move |s: &mut S, tx: &mut Tx| {
        tx.read_clipboard(move |s: &mut S, content: &rune::ClipboardContent, _| acc(s).composer.insert(content.text()));
        close(s);
    }))
    .item(MenuItem::new("select-all", "Select all").icon(&icons::SELECTION).disabled(st.composer.text.is_empty()).on_select(move |s: &mut S| {
        let c = &mut acc(s).composer;
        c.set_selection(0, c.text.len());
        close(s);
    }))
}

// ---- slash commands ----

/// `useCommands`: name, description, permission needed (in a community), what a simple one
/// inserts (`None`: run at send).
pub const COMMANDS: [(&str, &str, Option<Permissions>, Option<&str>); 13] = [
    ("shrug", "Appends ¯\\_(ツ)_/¯ to your message.", None, Some("¯\\_(ツ)_/¯")),
    ("tableflip", "Appends (╯°□°)╯︵ ┻━┻ to your message.", None, Some("(╯°□°)╯︵ ┻━┻")),
    ("unflip", "Appends ┬─┬ ノ( ゜-゜ノ) to your message.", None, Some("┬─┬ ノ( ゜-゜ノ)")),
    ("me", "Send an action message (wraps in italics).", None, None),
    ("spoiler", "Send a spoiler message (wraps in spoiler tags).", None, None),
    ("tts", "Send a text-to-speech message.", Some(Permissions::SEND_TTS_MESSAGES), None),
    ("nick", "Change your nickname in this community.", Some(Permissions::CHANGE_NICKNAME), None),
    ("kick", "Kick a member from this community.", Some(Permissions::KICK_MEMBERS), None),
    ("ban", "Ban a member from this community.", Some(Permissions::BAN_MEMBERS), None),
    ("msg", "Send a direct message to a user.", None, None),
    ("saved", "Send a saved media item.", None, None),
    ("sticker", "Send a sticker.", None, None),
    ("gif", "Search for and send a GIF.", None, None),
];

/// Whether a command is offered: guild-only ones need a community and the permission.
pub fn offered(perm: Option<Permissions>, guild: bool, have: Permissions) -> bool {
    match perm {
        None => true,
        Some(p) if p == Permissions::SEND_TTS_MESSAGES => !guild || have.contains(p),
        Some(p) => guild && have.contains(p),
    }
}

/// `BAN_DELETE_MESSAGE_SECONDS_CHOICE_VALUES`.
const BAN_DELETE: [&str; 7] = ["0", "3600", "21600", "43200", "86400", "259200", "604800"];

#[derive(Debug, PartialEq)]
pub enum Command {
    Nick(String),
    Kick { user: Snowflake, reason: Option<String> },
    Ban { user: Snowflake, delete: u64, reason: Option<String> },
    Msg { user: Snowflake, message: String },
    Tts(String),
    Media(&'static str, String),
    Unknown,
}

/// `parseCommand` for the commands that don't send as text; `None` sends the text.
pub fn parse(content: &str) -> Option<Command> {
    let t = content.trim();
    let after = |p: &str| t.strip_prefix(p).filter(|r| r.is_empty() || r.starts_with(char::is_whitespace)).map(str::trim);
    let mention = |r: &str| -> Option<(Snowflake, String)> {
        let rest = r.strip_prefix("<@")?.trim_start_matches('!');
        let end = rest.find('>')?;
        Some((rest[..end].parse().ok()?, rest[end + 1..].trim().to_string()))
    };
    let some = |s: String| Some(s).filter(|s| !s.is_empty());
    if let Some(r) = after("/nick") {
        return Some(Command::Nick(r.to_string()));
    }
    if let Some(r) = after("/kick").filter(|r| !r.is_empty()) {
        return Some(mention(r).map_or(Command::Unknown, |(user, rest)| Command::Kick { user, reason: some(rest) }));
    }
    if let Some(r) = after("/ban").filter(|r| !r.is_empty()) {
        let Some((user, rest)) = mention(r) else { return Some(Command::Unknown) };
        let first = rest.split_whitespace().next().unwrap_or("");
        let (delete, reason) = if BAN_DELETE.contains(&first) {
            (first.parse().unwrap_or(86400), rest[first.len()..].trim().to_string())
        } else if !first.is_empty() && first.bytes().all(|b| b.is_ascii_digit()) {
            return Some(Command::Unknown);
        } else {
            (86400, rest)
        };
        return Some(Command::Ban { user, delete, reason: some(reason) });
    }
    if let Some(r) = after("/msg").filter(|r| !r.is_empty()) {
        return Some(match mention(r) {
            Some((user, message)) if !message.is_empty() => Command::Msg { user, message },
            _ => Command::Unknown,
        });
    }
    if let Some(r) = after("/tts").filter(|r| !r.is_empty()) {
        return Some(Command::Tts(r.to_string()));
    }
    for kind in ["saved", "sticker", "gif"] {
        if let Some(r) = after(&format!("/{kind}")) {
            return Some(Command::Media(kind, r.to_string()));
        }
    }
    None
}

/// Runs a parsed command from the composer of `channel` (in `guild`), clearing it. `false`
/// when the text should be sent as a message instead.
// shortcut: no "nickname changed" system message; a failed /msg DM only logs.
pub fn run(st: &mut ShellState, channel: Snowflake, guild: Option<Snowflake>, content: &str) -> bool {
    let Some(cmd) = parse(content) else { return false };
    let req = |method, path: String, body, reason: Option<String>| Intent::Social(crate::social::Req { method, path, body, reason, done: crate::social::Done::Nothing });
    use reqwest::Method;
    match (cmd, guild) {
        (Command::Nick(n), Some(g)) => st.intents.push(req(Method::PATCH, format!("/guilds/{g}/members/@me"), Some(json!({ "nick": Some(n).filter(|n| !n.is_empty()) })), None)),
        (Command::Kick { user, reason }, Some(g)) => st.intents.push(req(Method::DELETE, format!("/guilds/{g}/members/{user}"), None, reason)),
        (Command::Ban { user, delete, reason }, Some(g)) => {
            let body = json!({ "delete_message_seconds": delete, "ban_duration_seconds": 0 });
            st.intents.push(req(Method::PUT, format!("/guilds/{g}/bans/{user}"), Some(body), reason));
        }
        (Command::Msg { user, message }, _) => {
            st.nav.pending_dm = Some((user, message));
            st.intents.push(Intent::OpenDm(user));
        }
        (Command::Tts(text), _) => {
            let nonce = st.now_ms.to_string();
            st.intents.push(Intent::Fetch { key: String::new(), method: Method::POST, path: format!("/channels/{channel}/messages"), body: Some(json!({ "content": text, "tts": true, "nonce": nonce })) });
        }
        (Command::Media(kind, q), _) => {
            let tab = match kind {
                "gif" => crate::shell::expr::Tab::Gifs,
                "sticker" => crate::shell::expr::Tab::Stickers,
                _ => crate::shell::expr::Tab::Memes,
            };
            crate::shell::expr::toggle(st, channel, tab);
            st.expr.query = TextEdit::new(q);
            return finish(st);
        }
        // Unknown or out of place: the text stays for the user to fix.
        _ => return true,
    }
    finish(st)
}

fn finish(st: &mut ShellState) -> bool {
    st.composer.clear();
    st.ac_dismissed = None;
    true
}

/// Sends a `/msg` once its DM channel exists.
pub fn poll(store: &reflux_store::Store, st: &mut ShellState) -> bool {
    let Some((user, _)) = &st.nav.pending_dm else { return false };
    let me = store.my_id();
    let dm = store.private_channels().into_iter().find(|c| c.kind == reflux_model::ChannelType::DM && c.recipients.iter().any(|u| u.id == *user) && *user != me).map(|c| c.id);
    let Some(channel) = dm else { return false };
    let (_, content) = st.nav.pending_dm.take().unwrap_or_default();
    st.intents.push(Intent::SendMessage { channel, content, reply: None, attachments: Vec::new() });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pills_serialize_to_tokens() {
        let mut e = TextEdit::new("hi @ad");
        let (token, label) = pill_for("<@42> ", "Ada").unwrap();
        put_pill(&mut e, 3..6, token, &label);
        e.insert("there");
        assert_eq!(e.text, "hi @Ada there");
        assert_eq!(serialize(&e), "hi <@42> there");
        assert_eq!(pill_for("🎉 ", ":tada:"), None);
    }

    #[test]
    fn toolbar_toggles_wrappers() {
        let mut e = TextEdit::new("say hi now");
        e.set_selection(4, 6);
        toggle(&mut e, "**");
        assert_eq!(e.text, "say **hi** now");
        assert_eq!(e.selected(), "hi");
        assert!(active(&e, "**") && !active(&e, "*"));
        toggle(&mut e, "**");
        assert_eq!(e.text, "say hi now");
        e.set_selection(4, 6);
        toggle(&mut e, "||");
        e.set_selection(4, 10);
        assert!(active(&e, "||"));
        toggle(&mut e, "||");
        assert_eq!(e.text, "say hi now");
    }

    #[test]
    fn parses_commands_like_fluxer() {
        assert_eq!(parse("/nick"), Some(Command::Nick(String::new())));
        assert_eq!(parse("/kick <@5> spam"), Some(Command::Kick { user: Snowflake(5), reason: Some("spam".into()) }));
        assert_eq!(parse("/ban <@5> 3600 rude"), Some(Command::Ban { user: Snowflake(5), delete: 3600, reason: Some("rude".into()) }));
        assert_eq!(parse("/ban <@5> 12 rude"), Some(Command::Unknown));
        assert_eq!(parse("/ban <@5>"), Some(Command::Ban { user: Snowflake(5), delete: 86400, reason: None }));
        assert_eq!(parse("/msg <@5>"), Some(Command::Unknown));
        assert_eq!(parse("/gif cats"), Some(Command::Media("gif", "cats".into())));
        assert_eq!(parse("/me waves"), None);
        assert_eq!(parse("/nickname"), None);
    }

    #[test]
    fn run_clears_and_queues() {
        let mut st = ShellState { composer: TextEdit::new("/kick <@5>"), ..Default::default() };
        assert!(run(&mut st, Snowflake(1), Some(Snowflake(2)), "/kick <@5>"));
        assert!(st.composer.text.is_empty());
        assert!(matches!(&st.intents[..], [Intent::Social(r)] if r.path == "/guilds/2/members/5"));
        assert!(!run(&mut st, Snowflake(1), None, "hello"));
        st.composer = TextEdit::new("/kick <@5>");
        assert!(run(&mut st, Snowflake(1), None, "/kick <@5>"));
        assert_eq!(st.composer.text, "/kick <@5>");
    }
}
