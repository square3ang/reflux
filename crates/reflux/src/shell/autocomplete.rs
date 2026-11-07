//! Composer autocomplete (`channel/components/Autocomplete*`): `@` members, `#` channels,
//! `:` emoji and `/` commands (`devtools/hooks/useCommands.ts`, the text-only ones).
//! Command options after `/name ` come from [`super::command_hints`]. Custom emoji come from
//! every community, the open one first, if usable here (`Emoji.getAllEmojis`,
//! `filterEmojisForAutocomplete`); a repeated name gets `~1`, `~2`… as in `buildAllEmojis`.

use reflux_model::{Channel, ChannelType, Permissions, Snowflake};
use reflux_store::Store;
use rune::{El, col, el, icon, row, text};

use super::{ShellAccess, ShellState, avatar, channel_icon, emoji};
use crate::theme::{Palette, font};
use crate::ui::over;

/// The word being completed: `text[start..end]`, `end` is the caret.
#[derive(Clone, Debug, PartialEq)]
pub struct Trigger {
    pub start: usize,
    pub end: usize,
    pub sigil: char,
    pub query: String,
}

/// The `@`, `#`, `:` (2+ letters) or leading `/` word ending at the caret.
pub fn trigger(text: &str, caret: usize) -> Option<Trigger> {
    let before = text.get(..caret)?;
    let start = before.rfind(char::is_whitespace).map_or(0, |i| i + before[i..].chars().next().map_or(1, char::len_utf8));
    let word = &before[start..];
    let sigil = word.chars().next()?;
    let query = word[sigil.len_utf8()..].to_string();
    let ok = match sigil {
        '@' | '#' => !query.contains(['@', '#']),
        ':' => query.chars().count() >= 2 && !query.contains(':'),
        '/' => start == 0 && !query.contains('/'),
        _ => false,
    };
    ok.then_some(Trigger { start, end: caret, sigil, query })
}

#[derive(Clone, Debug, PartialEq)]
pub enum Glyph {
    User(Snowflake),
    Channel(ChannelType),
    Emoji(String),
    CustomEmoji(Snowflake),
    Text(&'static str),
    /// No glyph (command choices and optional arguments).
    Plain,
    /// A role mention: no glyph, the `@name` label in the role colour (0 = none).
    Role(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Completion {
    pub glyph: Glyph,
    pub label: String,
    pub detail: String,
    /// Replaces the trigger word.
    pub insert: String,
}


/// `/me` and `/spoiler` at send (`useTextareaSubmit`).
pub fn apply_command(content: &str) -> String {
    let arg = |name: &str| content.strip_prefix(name).filter(|r| r.starts_with(char::is_whitespace)).map(str::trim).filter(|a| !a.is_empty());
    if let Some(a) = arg("/me") {
        format!("_{a}_")
    } else if let Some(a) = arg("/spoiler") {
        format!("||{a}||")
    } else {
        content.to_string()
    }
}

const MAX: usize = 10;

/// What the word at the caret could become, best first.
pub fn completions(store: &Store, st: &ShellState, c: &Channel, t: &Trigger) -> Vec<Completion> {
    let q = t.query.to_lowercase();
    let rank = |name: &str| {
        let n = name.to_lowercase();
        if n.starts_with(&q) { Some(0) } else if n.contains(&q) { Some(1) } else { None }
    };
    let mut out: Vec<(u8, Completion)> = Vec::new();
    match t.sigil {
        '@' => {
            let me = store.my_id();
            let users: Vec<Snowflake> = match c.guild_id.and_then(|g| store.guilds.get(&g)) {
                Some(g) => g.members.keys().copied().collect(),
                None => c.recipients.iter().map(|u| u.id).chain([me]).collect(),
            };
            for u in users {
                let shown = store.display_name(c.guild_id, u);
                let username = store.users.get(&u).map(|x| x.username.clone()).or_else(|| store.me.as_ref().filter(|m| m.id == u).map(|m| m.base.username.clone())).unwrap_or_default();
                if let Some(r) = rank(&shown).into_iter().chain(rank(&username)).min() {
                    out.push((r, Completion { glyph: Glyph::User(u), label: shown, detail: username, insert: format!("<@{u}> ") }));
                }
            }
            // Members first, then @everyone / @here, then roles (`useLexicalAutocomplete`).
            sort(&mut out);
            out.truncate(MAX);
            let everyone = store.permissions(c.id).contains(Permissions::MENTION_EVERYONE);
            if c.guild_id.is_some() && everyone {
                for (name, detail) in [("everyone", "Notify everyone who has permission to view this channel."), ("here", "Notify everyone online who has permission to view this channel.")] {
                    if name.contains(&q) {
                        out.push((0, Completion { glyph: Glyph::Text("@"), label: format!("@{name}"), detail: crate::i18n::t(detail).into(), insert: format!("@{name} ") }));
                    }
                }
            }
            // `filterDisplayRoles` (not @everyone), mentionable unless we may mention everyone,
            // highest first.
            if let Some(g) = c.guild_id.and_then(|g| store.guilds.get(&g)) {
                let mut roles: Vec<_> = g.roles.iter().filter(|r| r.id != g.id && (everyone || r.mentionable) && r.name.to_lowercase().contains(&q)).collect();
                roles.sort_by_key(|r| std::cmp::Reverse(r.position));
                for r in roles.into_iter().take(MAX) {
                    let detail = crate::i18n::t("Notify users with this role who have permission to view this channel.").into();
                    out.push((0, Completion { glyph: Glyph::Role(r.color), label: r.name.clone(), detail, insert: format!("<@&{}> ", r.id) }));
                }
            }
            return out.into_iter().map(|x| x.1).collect();
        }
        '#' => {
            if let Some(g) = c.guild_id {
                for grp in store.channel_groups(g) {
                    for ch in grp.text.iter().chain(grp.voice.iter()).filter(|ch| ch.kind != ChannelType::GUILD_LINK) {
                        let name = ch.name.clone().unwrap_or_default();
                        let category = grp.category.and_then(|x| x.name.clone()).unwrap_or_default();
                        if let Some(r) = rank(&name) {
                            out.push((r, Completion { glyph: Glyph::Channel(ch.kind), label: name, detail: category, insert: format!("<#{}> ", ch.id) }));
                        }
                    }
                }
            }
        }
        ':' => {
            let (custom_ok, emoji_ok) = crate::look::with(|l| (l.autocomplete_custom_emojis, l.autocomplete_emojis));
            let mut order = super::expr::guild_order(store, st);
            if let Some(i) = c.guild_id.and_then(|g| order.iter().position(|x| *x == g)) {
                let g = order.remove(i);
                order.insert(0, g);
            }
            let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for gid in order {
                if !custom_ok {
                    break;
                }
                let Some(g) = store.guilds.get(&gid) else { continue };
                for e in &g.emojis {
                    let n = seen.entry(e.name.clone()).or_insert_with(|| usize::from(emoji::by_name(&e.name).is_some()));
                    let name = if *n == 0 { e.name.clone() } else { format!("{}~{n}", e.name) };
                    *n += 1;
                    if !super::expr::can_use(store, st, Some(c), gid, Permissions::USE_EXTERNAL_EMOJIS) {
                        continue;
                    }
                    if let Some(r) = rank(&name) {
                        let (a, raw, id) = (if e.animated { "a" } else { "" }, &e.name, e.id);
                        out.push((r, Completion { glyph: Glyph::CustomEmoji(id), label: format!(":{name}:"), detail: String::new(), insert: format!("<{a}:{raw}:{id}> ") }));
                    }
                }
            }
            if emoji_ok {
                for e in emoji::search(&q).into_iter().take(MAX) {
                    out.push((rank(&e.name).unwrap_or(1), Completion { glyph: Glyph::Emoji(e.glyph.clone()), label: format!(":{}:", e.name), detail: String::new(), insert: format!("{} ", e.glyph) }));
                }
            }
        }
        '/' => {
            let have = store.permissions(c.id);
            for (name, detail, perm, simple) in crate::compose::COMMANDS {
                if let Some(r) = rank(name).filter(|_| crate::compose::offered(perm, c.guild_id.is_some(), have)) {
                    let insert = format!("{} ", simple.map_or_else(|| format!("/{name}"), str::to_string));
                    out.push((r, Completion { glyph: Glyph::Text("/"), label: format!("/{name}"), detail: detail.into(), insert }));
                }
            }
        }
        _ => {}
    }
    sort(&mut out);
    out.into_iter().take(MAX).map(|x| x.1).collect()
}

fn sort(out: &mut [(u8, Completion)]) {
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.label.to_lowercase().cmp(&b.1.label.to_lowercase())));
}

/// Puts a completion in place of the trigger word.
pub fn accept(st: &mut ShellState, t: &Trigger, c: &Completion) {
    // An optional-argument row: the argument is typed next, so the menu just closes.
    if c.insert.is_empty() {
        st.ac_dismissed = Some((t.start, t.query.clone()));
        return;
    }
    match crate::compose::pill_for(&c.insert, &c.label) {
        Some((token, label)) => crate::compose::put_pill(&mut st.composer, t.start..t.end, token, &label),
        None => {
            st.composer.set_selection(t.start, t.end);
            st.composer.insert(&c.insert);
        }
    }
    st.ac_index = 0;
}

fn heading(sigil: char, q: &str) -> String {
    match sigil {
        '@' => "MEMBERS".into(),
        '#' => "TEXT CHANNELS".into(),
        ':' => format!("EMOJI MATCHING :{}", q.to_uppercase()),
        super::command_hints::CHOICES => crate::i18n::t("Choices").to_uppercase(),
        super::command_hints::OPTIONAL => crate::i18n::t("Optional arguments").to_uppercase(),
        _ => "COMMANDS".into(),
    }
}

/// `.containerAttached`: the list above the composer box.
/// `slot`: the composer it completes for (a press there edits that draft).
pub(super) fn view<S: 'static>(store: &Store, st: &ShellState, t: &Trigger, items: &[Completion], p: &Palette, acc: ShellAccess<S>, slot: super::chat::Slot) -> El {
    let selected = st.ac_index.min(items.len().saturating_sub(1));
    let mut list = col().w_full().py(8.0).gap(4.0).child(text(heading(t.sigil, &t.query)).font(font(12.0, 600.0)).color(p.text_primary_muted).px(12.0).py(4.0));
    for (i, c) in items.iter().enumerate() {
        let glyph = match &c.glyph {
            Glyph::User(u) => avatar(store, st, *u, 24.0, None, p.background_primary, p),
            Glyph::Channel(k) => icon(channel_icon(*k), 20.0).color(p.text_primary_muted),
            Glyph::Emoji(g) => match emoji::sprite_index(g).map(|i| st.media.sprite(i)) {
                Some(crate::media::Load::Ready(px)) => el().size(20.0, 20.0).pixels_fit(&px, rune::SurfaceFit::Fit),
                _ => text(g.clone()).font(font(18.0, 400.0).lh(20.0)),
            },
            Glyph::CustomEmoji(id) => match st.media.asset(|b, _| crate::media::emoji_url(b, &id.to_string())) {
                crate::media::Load::Ready(px) => el().size(20.0, 20.0).pixels_fit(&px, rune::SurfaceFit::Fit),
                _ => el().size(20.0, 20.0),
            },
            Glyph::Text(s) => row().size(24.0, 24.0).center().child(text(*s).font(font(16.0, 600.0)).color(p.text_primary_muted)),
            Glyph::Role(_) | Glyph::Plain => el(),
        };
        let (label, color) = match c.glyph {
            Glyph::Role(rgb) => (format!("@{}", c.label), if rgb == 0 { p.text_primary } else { super::role_color(rgb) }),
            _ => (c.label.clone(), p.text_primary),
        };
        let (t2, c2) = (t.clone(), c.clone());
        let mut r = row()
            .w_full()
            .min_h(40.0)
            .padding(8.0)
            .center_y()
            .radius(8.0)
            .cursor(rune::Cursor::Pointer)
            .role("option")
            .key(format!("ac-{i}"))
            .hover(p.background_modifier_hover)
            .children((!matches!(c.glyph, Glyph::Role(_) | Glyph::Plain)).then(|| row().mr(8.0).child(glyph)))
            .child(text(label).font(font(16.0, 400.0).lh(20.0)).color(color).truncate().shrink().min_w(0.0))
            .child(el().grow(1.0))
            .child(text(c.detail.clone()).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted).truncate().ml(16.0).shrink())
            .on_press(move |s: &mut S| super::chat::in_slot(acc(s), slot, |st| accept(st, &t2, &c2)));
        if i == selected {
            r = r.fill(p.background_modifier_hover);
        }
        list = list.child(row().w_full().px(8.0).child(r));
    }
    let bg = p.background_primary;
    col()
        .w_full()
        .max_h(490.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .box_shadow(rune::Shadow { x: 0.0, y: 8.0, blur: 16.0, spread: 0.0, color: crate::ui::rgba(0, 0, 0, 0.24), inset: false })
        .clip_rounded(8.0)
        .child(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_at_the_caret() {
        assert_eq!(trigger("hi @ad", 6).map(|t| (t.start, t.sigil, t.query)), Some((3, '@', "ad".into())));
        assert_eq!(trigger("#gen", 4).map(|t| t.query), Some("gen".into()));
        assert!(trigger("a :s", 4).is_none(), "emoji needs two letters");
        assert_eq!(trigger("a :sm", 5).map(|t| t.sigil), Some(':'));
        assert!(trigger("x /shrug", 8).is_none(), "commands only lead the message");
        assert!(trigger("hi @ad there", 12).is_none(), "the caret must end the word");
        assert_eq!(trigger("/sh", 3).map(|t| t.sigil), Some('/'));
    }

    #[test]
    fn commands_rewrite_the_message() {
        assert_eq!(apply_command("/mess"), "/mess");
        assert_eq!(apply_command("/me waves"), "_waves_");
        assert_eq!(apply_command("/spoiler boo"), "||boo||");
        assert_eq!(apply_command("/unknown x"), "/unknown x");
        assert_eq!(apply_command("plain"), "plain");
    }

    #[test]
    fn completes_from_the_demo() {
        let d = crate::shell::demo::Demo::new();
        let c = &d.store.channels[&d.shell.channel.unwrap()];
        let at = completions(&d.store, &d.shell, c, &trigger("@ada", 4).unwrap());
        assert_eq!(at[0].insert, "<@1001> ");
        let g = &d.store.guilds[&c.guild_id.unwrap()];
        let role = g.roles.iter().filter(|r| r.id != g.id).max_by_key(|r| r.position).unwrap();
        let q = format!("@{}", &role.name[..3]);
        let roles = completions(&d.store, &d.shell, c, &trigger(&q, q.len()).unwrap());
        assert!(roles.iter().any(|x| x.insert == format!("<@&{}> ", role.id)), "{roles:?}");
        let ch = completions(&d.store, &d.shell, c, &trigger("#off", 4).unwrap());
        assert_eq!(ch[0].insert, "<#2203> ");
        let em = completions(&d.store, &d.shell, c, &trigger(":smile", 6).unwrap());
        assert!(em.iter().any(|e| e.label == ":smile:"), "{em:?}");
        let cmd = completions(&d.store, &d.shell, c, &trigger("/sh", 3).unwrap());
        assert_eq!((cmd[0].label.as_str(), cmd[0].insert.as_str()), ("/shrug", "¯\\_(ツ)_/¯ "));
    }
}
