//! Slash-command option menus (`useLexicalAutocomplete` slot menus, `AutocompleteCommandChoice`,
//! `AutocompleteCommandOptionalAdd`) for a plain-text composer: after `/name ` the option at
//! the caret offers members (`user`), choices (`choice`) or the optional arguments left.

use reflux_model::{Channel, Permissions};
use reflux_store::Store;

use super::autocomplete::{Completion, Glyph, Trigger, completions};
use crate::i18n::t;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Str,
    User,
    Choice,
}

struct Opt {
    name: &'static str,
    desc: &'static str,
    kind: Kind,
    required: bool,
}

const MEMBER: &str = "The member to target.";
const REASON: &str = "Reason (optional).";
const MESSAGE: &str = "The message to send.";
const QUERY: &str = "What to search for.";

const fn o(name: &'static str, desc: &'static str, kind: Kind, required: bool) -> Opt {
    Opt { name, desc, kind, required }
}

static MSG1: [Opt; 1] = [o("message", MESSAGE, Kind::Str, true)];
static QUERY1: [Opt; 1] = [o("query", QUERY, Kind::Str, true)];
static NICK: [Opt; 1] = [o("nickname", "Your new nickname, or leave blank to reset it.", Kind::Str, true)];
static KICK: [Opt; 2] = [o("user", MEMBER, Kind::User, true), o("reason", REASON, Kind::Str, false)];
static BAN: [Opt; 3] = [
    o("user", MEMBER, Kind::User, true),
    o("delete_messages", "How much of the member's recent message history to delete.", Kind::Choice, true),
    o("reason", REASON, Kind::Str, false),
];
static MSG: [Opt; 2] = [o("user", MEMBER, Kind::User, true), o("message", MESSAGE, Kind::Str, true)];

/// `useCommands` options per action command.
fn options(cmd: &str) -> &'static [Opt] {
    match cmd {
        "me" | "spoiler" | "tts" => &MSG1,
        "saved" | "sticker" | "gif" => &QUERY1,
        "nick" => &NICK,
        "kick" => &KICK,
        "ban" => &BAN,
        "msg" => &MSG,
        _ => &[],
    }
}

/// The menu heading's sigils (see `autocomplete::heading`).
pub const CHOICES: char = '\u{1}';
pub const OPTIONAL: char = '\u{2}';

/// The option menu for the command at the start of `text` with the caret at its end.
pub fn hints(store: &Store, st: &super::ShellState, c: &Channel, text: &str, caret: usize) -> Option<(Trigger, Vec<Completion>)> {
    if caret != text.len() {
        return None;
    }
    let (name, args) = text.strip_prefix('/')?.split_once(char::is_whitespace)?;
    let (_, _, perm, _) = crate::compose::COMMANDS.iter().find(|x| x.0 == name)?;
    if !crate::compose::offered(*perm, c.guild_id.is_some(), store.permissions(c.id)) {
        return None;
    }
    let opts = options(name);
    let toks: Vec<&str> = args.split_whitespace().collect();
    let mid = !args.is_empty() && !args.ends_with(char::is_whitespace);
    let idx = if mid { toks.len() - 1 } else { toks.len() };
    let partial = if mid { toks[idx] } else { "" };
    // A string option takes the rest of the line.
    if idx >= opts.len() || opts[..idx].iter().any(|o| o.kind == Kind::Str) {
        return None;
    }
    let start = caret - partial.len();
    let opt = &opts[idx];
    let (sigil, items) = match opt.kind {
        Kind::Choice => {
            let q = partial.to_lowercase();
            let items = crate::social::moderation::BAN_DELETE
                .iter()
                .filter(|(v, label, _)| v.starts_with(&q) || t(label).to_lowercase().contains(&q))
                .map(|(v, label, _)| Completion { glyph: Glyph::Plain, label: t(label).into(), detail: String::new(), insert: format!("{v} ") })
                .collect();
            (CHOICES, items)
        }
        Kind::User => {
            let q = partial.trim_start_matches('@');
            let me = store.my_id();
            let perm = match name {
                "kick" => Some(Permissions::KICK_MEMBERS),
                "ban" => Some(Permissions::BAN_MEMBERS),
                _ => None,
            };
            // `buildCommandArgOptions`: DMs offer recipients to /msg only; kick/ban only members we
            // may act on.
            if c.guild_id.is_none() && name != "msg" {
                return None;
            }
            let tr = Trigger { start, end: caret, sigil: '@', query: q.into() };
            let items = completions(store, st, c, &tr)
                .into_iter()
                .filter(|x| match (x.glyph.clone(), c.guild_id, perm) {
                    (Glyph::User(u), Some(g), Some(p)) => crate::social::moderation::can_moderate(store, g, u, p),
                    (Glyph::User(u), _, _) => u != me || c.guild_id.is_some(),
                    _ => false,
                })
                .collect();
            ('@', items)
        }
        Kind::Str if !opt.required && partial.is_empty() => {
            let items = opts[idx..]
                .iter()
                .filter(|o| !o.required)
                .map(|o| Completion { glyph: Glyph::Plain, label: o.name.into(), detail: t(o.desc).into(), insert: String::new() })
                .collect();
            (OPTIONAL, items)
        }
        Kind::Str => return None,
    };
    Some((Trigger { start, end: caret, sigil, query: partial.into() }, items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_follow_the_caret() {
        let d = crate::shell::demo::Demo::new();
        let c = &d.store.channels[&d.shell.channel.unwrap()];
        let kinds = |s: &str| hints(&d.store, &d.shell, c, s, s.len()).map(|(t, i)| (t.sigil, i.len()));
        assert_eq!(kinds("/me"), None, "no space yet");
        assert_eq!(kinds("/me "), None, "a required string has no menu");
        assert_eq!(kinds("/me hi there"), None);
        let at = hints(&d.store, &d.shell, c, "/msg ", 5).unwrap();
        assert_eq!(at.0.sigil, '@');
        assert!(at.1.iter().all(|x| matches!(x.glyph, Glyph::User(_))));
        let u = d.store.users.keys().next().copied().unwrap();
        let after = format!("/msg <@{u}> ");
        assert_eq!(kinds(&after), None, "the message is a required string");
        let ban = format!("/ban <@{u}> ");
        let (tr, items) = hints(&d.store, &d.shell, c, &ban, ban.len()).unwrap();
        assert_eq!((tr.sigil, items.len()), (CHOICES, 7));
        assert_eq!(items[1].insert, "3600 ");
        let ban3 = format!("/ban <@{u}> 36");
        let (tr, items) = hints(&d.store, &d.shell, c, &ban3, ban3.len()).unwrap();
        assert_eq!((tr.start, items.len()), (ban3.len() - 2, 1));
        let kick = format!("/kick <@{u}> ");
        let (tr, items) = hints(&d.store, &d.shell, c, &kick, kick.len()).unwrap();
        assert_eq!((tr.sigil, items[0].label.as_str(), items[0].insert.as_str()), (OPTIONAL, "reason", ""));
        assert_eq!(kinds(&format!("/kick <@{u}> spam")), None);
    }
}
