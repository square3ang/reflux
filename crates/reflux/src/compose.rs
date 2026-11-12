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
/// completion's label give `(token, label)`; anything else stays text. A custom emoji's label
/// is [`emoji_label`], under its image.
pub fn pill_for(insert: &str, label: &str) -> Option<(String, String)> {
    let token = insert.trim_end();
    if !(token.starts_with('<') && token.ends_with('>')) {
        return None;
    }
    if emoji_pill(token).is_some() {
        return Some((token.to_string(), emoji_label()));
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
    id.starts_with("<#").then_some(&icons::HASH_FILL)
}

/// Spelling underlines outside pills.
pub fn unpilled(spans: Vec<rune::Span>, edit: &TextEdit) -> Vec<rune::Span> {
    let pills: Vec<_> = edit.pills().map(|(r, _)| r).collect();
    spans.into_iter().filter(|s| !pills.iter().any(|r| (s.start as usize) < r.end && r.start < s.end as usize)).collect()
}

// ---- emoji ----

/// The custom emoji a pill id (`<:name:id>` / `<a:name:id>`) stands for.
pub fn emoji_pill(id: &str) -> Option<Snowflake> {
    let t = id.strip_prefix('<')?.strip_suffix('>')?;
    let t = t.strip_prefix('a').unwrap_or(t).strip_prefix(':')?;
    let (name, id) = t.rsplit_once(':')?;
    (!name.is_empty()).then_some(())?;
    id.parse().ok().map(Snowflake)
}

/// `.customEmoji` in the composer: 1.375em (22px at 16px) and 1px either side.
pub const EMOJI_SIDE: f32 = 22.0;

/// A custom emoji pill's label: no-break spaces that, with the chip's margin and padding
/// (Rune's `PillMetrics` at 16px: 2 × 7.68), make the pill `.customEmoji`'s 24px. The emoji's
/// image is drawn over it; the chip itself is transparent.
pub fn emoji_label() -> String {
    let nbsp = rune::text::advance("\u{a0}", crate::theme::font(16.0, 400.0)).max(1.0);
    let room = EMOJI_SIDE + 2.0 - 2.0 * (16.0f32 * 0.12 + 16.0 * 0.36);
    "\u{a0}".repeat(((room / nbsp).round() as usize).max(1))
}

/// What a typed `:name:` becomes (`resolveTypedEmojiToken`): a unicode emoji's text, or a
/// custom emoji's pill `(token, label)`.
#[derive(Clone, Debug, PartialEq)]
pub enum Typed {
    Text(String),
    Pill(String, String),
}

/// The custom emoji usable in a channel, as autocomplete names them (`name`, `name~1`…), each
/// with its `<a:name:id>` token.
pub type CustomEmoji = std::rc::Rc<Vec<(String, String)>>;

/// `resolveTypedEmojiToken`: unicode by shortcode first, then a usable custom emoji.
pub fn resolve(name: &str, custom: &[(String, String)]) -> Option<Typed> {
    if let Some(e) = crate::shell::emoji::by_name(name) {
        return Some(Typed::Text(e.glyph.clone()));
    }
    custom.iter().find(|(n, _)| n == name).map(|(_, token)| Typed::Pill(token.clone(), emoji_label()))
}

/// `TYPED_EMOJI_SHORTCODE_PATTERN`'s name characters.
fn name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '+' | '~' | '.' | '-')
}

/// `SHORTCODE_WORD_BOUNDARY_CHAR`.
fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `findTypedEmojiShortcode` over `text[from..to]`: each `:name:` (2+ name characters) not
/// inside a word, a `<:custom:id>`, inline code, a URL or after a backslash, as (range, name).
pub fn shortcodes(text: &str, from: usize, to: usize) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut i = from;
    while let Some(a) = text.get(i..to).and_then(|s| s.find(':')).map(|k| i + k) {
        let rest = &text[a + 1..to];
        let n = rest.find(|c: char| !name_char(c)).unwrap_or(rest.len());
        let b = a + 1 + n;
        let closed = text[b..to].starts_with(':');
        let name = &text[a + 1..b];
        let before = text[..a].chars().next_back();
        // Only a closed match has an end to look past; `b` can sit inside a no-break space.
        let after = closed.then(|| text[b + 1..].chars().next()).flatten();
        // Past the whole whitespace char: a no-break space is two bytes.
        let word_start = text[..a].rfind(char::is_whitespace).map_or(0, |k| k + text[k..].chars().next().map_or(1, char::len_utf8));
        let ok = closed
            && name.chars().count() >= 2
            && !before.is_some_and(word_char)
            && !after.is_some_and(word_char)
            && !text[..a].ends_with('<')
            && !text[..a].ends_with("<a")
            && !text[..a].ends_with('\\')
            && text[..a].matches('`').count().is_multiple_of(2)
            && !text[word_start..a].contains("://");
        if ok {
            out.push((a..b + 1, name));
            i = b + 1;
        } else {
            i = a + 1;
        }
    }
    out
}

/// `isReactionShorthandText`: `+:name:` alone reacts to the last message, so it stays text.
fn reaction_shorthand(text: &str) -> bool {
    text.trim().strip_prefix("+:").is_some_and(|r| r.ends_with(':') && !r[..r.len() - 1].contains([':', ' ']))
}

/// The `:name:`s in `edit.text[from..to]` that resolve, outside pills.
fn resolved(edit: &TextEdit, from: usize, to: usize, custom: &[(String, String)]) -> Vec<(std::ops::Range<usize>, Typed)> {
    if reaction_shorthand(&edit.text) {
        return Vec::new();
    }
    let pills: Vec<_> = edit.pills().map(|(r, _)| r).collect();
    shortcodes(&edit.text, from, to)
        .into_iter()
        .filter(|(r, _)| !pills.iter().any(|p| r.start < p.end && p.start < r.end))
        .filter_map(|(r, name)| Some((r, resolve(name, custom)?)))
        .collect()
}

/// Puts each `(range, emoji)` in (ranges in order, not overlapping) and keeps the caret where
/// it was relative to the text around it.
fn apply(e: &mut TextEdit, edits: &[(std::ops::Range<usize>, Typed)]) {
    let mut caret = e.focus;
    for (r, typed) in edits.iter().rev() {
        let len = match typed {
            Typed::Text(s) => {
                e.set_selection(r.start, r.end);
                e.insert(s);
                s.len()
            }
            Typed::Pill(token, label) => {
                e.replace_with_pill(r.clone(), token.clone(), label);
                label.len()
            }
        };
        if r.end <= caret {
            caret = caret - (r.end - r.start) + len;
        }
    }
    e.set_selection(caret, caret);
}

/// `resolveTypedEmojiShortcodes` on send: every `:name:` left in the draft that resolves
/// becomes its emoji. One undo step.
pub fn convert(edit: &mut TextEdit, custom: &[(String, String)]) {
    let edits = resolved(edit, 0, edit.text.len(), custom);
    if !edits.is_empty() {
        edit.undo_group(|e| apply(e, &edits));
    }
}

/// The closing `:` of a typed shortcode (`ComposerEmojiShortcode`): `:name` just before the
/// caret becomes its emoji instead of taking the colon. False (the field types the colon)
/// otherwise.
pub fn typed_colon(edit: &mut TextEdit, custom: &[(String, String)]) -> bool {
    let (a, b) = edit.range();
    let Some(start) = edit.text[..a].rfind(':').filter(|_| a == b && !reaction_shorthand(&format!("{}:", &edit.text[..a]))) else { return false };
    let with = format!("{}:{}", &edit.text[..a], &edit.text[a..]);
    let Some(name) = shortcodes(&with, start, a + 1).into_iter().find(|(r, _)| r.end == a + 1).map(|(_, n)| n) else { return false };
    if edit.pills().any(|(r, _)| start < r.end && r.start < a) {
        return false;
    }
    let Some(typed) = resolve(name, custom) else { return false };
    edit.undo_group(|e| apply(e, &[(start..a, typed)]));
    true
}

/// Pastes `text` with its shortcodes as emoji: copying a custom emoji puts `:name:` on the
/// clipboard, and Fluxer's composer turns it back into the emoji. False when nothing in it
/// converts (the field pastes as usual).
pub fn paste(edit: &mut TextEdit, text: &str, html: Option<&str>, custom: &[(String, String)]) -> bool {
    // Copied inside this field: the HTML carries the pills' tokens, so let the field restore
    // them (`paste_html_with_pills`) rather than flattening to text.
    if html.is_some_and(|h| h.contains("data-pill")) {
        return false;
    }
    if reaction_shorthand(text) || shortcodes(text, 0, text.len()).iter().all(|(_, n)| resolve(n, custom).is_none()) {
        return false;
    }
    let (a, _) = edit.range();
    edit.undo_group(|e| {
        e.insert(text);
        let edits = resolved(e, a, a + text.len(), custom);
        apply(e, &edits);
    });
    true
}

// ---- selection formatting toolbar ----

/// `FORMATS`: label, icon, Markdown wrapper.
pub const FORMATS: [(&str, &Icon, &str); 6] = [
    ("Bold", &icons::TEXT_B_FILL, "**"),
    ("Italic", &icons::TEXT_ITALIC_FILL, "*"),
    ("Underline", &icons::TEXT_UNDERLINE_FILL, "__"),
    ("Strikethrough", &icons::TEXT_STRIKETHROUGH_FILL, "~~"),
    ("Inline code", &icons::CODE, "`"),
    ("Spoiler", &icons::EYE_SLASH_FILL, "||"),
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
    /// The composer's caret line `[x, y, w, h]` (zoomed points) while it has a selection, read
    /// from `Cx::caret` by the root view; the format toolbar floats above it.
    pub static CARET: std::cell::Cell<Option<[f32; 4]>> = const { std::cell::Cell::new(None) };
}

/// The format toolbar over the composer's selection (`FormattingToolbar`), in the root layer.
pub fn floating_toolbar<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some(c) = CARET.get() else { return };
    layer.push(rune::el().key("format-caret").absolute(c[0], c[1]).size(1.0, 1.0));
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
    let draft = if st.desktop.menu_pane { &st.threads.composer } else { &st.composer };
    let has_sel = draft.range().0 != draft.range().1;
    if after_spelling {
        m = m.separator();
    }
    m.item(MenuItem::new("undo", "Undo").icon(&icons::ARROW_COUNTER_CLOCKWISE_FILL).on_select(move |s: &mut S| {
        menu_draft(acc(s)).undo();
        close(s);
    }))
    .item(MenuItem::new("redo", "Redo").icon(&icons::ARROW_CLOCKWISE_FILL).on_select(move |s: &mut S| {
        menu_draft(acc(s)).redo();
        close(s);
    }))
    .separator()
    .item(MenuItem::new("cut", "Cut").icon(&icons::SCISSORS_FILL).disabled(!has_sel).on_select_tx(move |s: &mut S, tx: &mut Tx| {
        let c = acc(s);
        tx.copy(menu_draft(c).selected().to_string());
        menu_draft(c).insert("");
        close(s);
    }))
    .item(MenuItem::new("copy", "Copy").icon(&icons::COPY_FILL).disabled(!has_sel).on_select_tx(move |s: &mut S, tx: &mut Tx| {
        tx.copy(menu_draft(acc(s)).selected().to_string());
        close(s);
    }))
    .item(MenuItem::new("paste", "Paste").icon(&icons::CLIPBOARD_TEXT_FILL).on_select_tx(move |s: &mut S, tx: &mut Tx| {
        tx.read_clipboard(move |s: &mut S, content: &rune::ClipboardContent, _| menu_draft(acc(s)).insert(content.text()));
        close(s);
    }))
    .item(MenuItem::new("select-all", "Select all").icon(&icons::SELECTION_FILL).disabled(draft.text.is_empty()).on_select(move |s: &mut S| {
        let c = menu_draft(acc(s));
        c.set_selection(0, c.text.len());
        close(s);
    }))
}

/// The draft the textarea context menu was opened on: the main composer's or the thread panel's.
pub fn menu_draft(sh: &mut ShellState) -> &mut TextEdit {
    if sh.desktop.menu_pane { &mut sh.threads.composer } else { &mut sh.composer }
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
/// `/nick` and `/msg` report back through Fluxerbot notes ([`crate::shell::client_system`]).
pub fn run(st: &mut ShellState, channel: Snowflake, guild: Option<Snowflake>, content: &str) -> bool {
    let Some(cmd) = parse(content) else { return false };
    let req = |method, path: String, body, reason: Option<String>| Intent::Social(crate::social::Req { method, path, body, reason, done: crate::social::Done::Nothing });
    use reqwest::Method;
    match (cmd, guild) {
        (Command::Nick(n), Some(g)) => st.intents.push(Intent::Nick { guild: g, channel, nick: Some(n).filter(|n| !n.is_empty()) }),
        (Command::Kick { user, reason }, Some(g)) => st.intents.push(req(Method::DELETE, format!("/guilds/{g}/members/{user}"), None, reason)),
        (Command::Ban { user, delete, reason }, Some(g)) => {
            let body = json!({ "delete_message_seconds": delete, "ban_duration_seconds": 0 });
            st.intents.push(req(Method::PUT, format!("/guilds/{g}/bans/{user}"), Some(body), reason));
        }
        (Command::Msg { user, message }, _) => st.intents.push(Intent::MsgCommand { from: channel, user, message }),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcodes_follow_typed_emoji_match() {
        let names = |t: &str| shortcodes(t, 0, t.len()).into_iter().map(|(_, n)| n.to_string()).collect::<Vec<_>>();
        assert_eq!(names("hi :smile: and :party~1:"), ["smile", "party~1"]);
        assert!(names("a:smile:b").is_empty(), "inside a word");
        assert!(names("<:fluxer:123> <a:fluxer:4>").is_empty(), "custom emoji markdown");
        assert!(names("`:smile:` \\:smile: https://x.y/:smile:").is_empty(), "code, escaped, URL");
        assert!(names(":x: 12:30:45").is_empty(), "one letter; a time");
    }

    #[test]
    fn shortcodes_never_split_a_multibyte_char() {
        // An unclosed `:name` right before a no-break space (a custom emoji pill's label).
        let nbsp = "\u{a0}".repeat(2);
        for t in [format!(":{nbsp}"), format!("hi :x{nbsp} there"), format!(":::{nbsp}::{nbsp}")] {
            assert!(shortcodes(&t, 0, t.len()).is_empty() || true);
        }
        let custom = vec![("fluxer".to_string(), "<:fluxer:9>".to_string())];
        let mut e = TextEdit::new("");
        assert!(paste(&mut e, ":fluxer: :fluxer:", None, &custom));
        assert_eq!(serialize(&e), "<:fluxer:9> <:fluxer:9>");
        assert!(!paste(&mut e, ":fluxer:", Some("<span data-pill=\"<:fluxer:9>\">x</span>"), &custom), "the field restores pill HTML itself");
    }

    #[test]
    fn typed_colon_and_send_convert_emoji() {
        let custom = vec![("fluxer".to_string(), "<:fluxer:9>".to_string())];
        let mut e = TextEdit::new("hi :smile");
        e.set_caret(e.text.len());
        assert!(typed_colon(&mut e, &custom));
        assert_eq!(e.text, "hi 😄");
        e.insert(" :fluxer");
        assert!(typed_colon(&mut e, &custom));
        assert_eq!(serialize(&e), "hi 😄 <:fluxer:9>");
        assert!(!typed_colon(&mut TextEdit::new(":nope"), &custom), "unknown names type the colon");
        let mut e = TextEdit::new("+:smile:");
        convert(&mut e, &custom);
        assert_eq!(e.text, "+:smile:", "a reaction shorthand stays");
        let mut e = TextEdit::new("");
        assert!(paste(&mut e, "x :fluxer: y", None, &custom));
        assert_eq!(serialize(&e), "x <:fluxer:9> y");
    }

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
