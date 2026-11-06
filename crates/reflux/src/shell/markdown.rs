//! Message markdown (`reflux_markdown` AST) to Rune elements, styled after
//! `theme/styles/Markup.module.css`.

use reflux_markdown::{EmojiKind, MentionKind, Node, Options, presets};
use reflux_model::Snowflake;
use reflux_store::Store;
use rune::text::Span;
use rune::{Color, El, col, el, rich, row, text};

use crate::theme::{Palette, font};
use crate::ui::mix;

pub struct Md<'a> {
    pub store: &'a Store,
    pub guild: Option<Snowflake>,
    pub p: &'a Palette,
}

/// Inline style carried down the tree.
#[derive(Clone, Copy, Default)]
struct Style {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    spoiler: bool,
    color: Option<Color>,
}

/// One paragraph being built: text plus non-overlapping spans.
#[derive(Default)]
struct Run {
    text: String,
    spans: Vec<Span>,
    strikes: Vec<(usize, usize)>,
}

impl Run {
    fn push(&mut self, s: &str, st: Style, md: &Md) {
        let start = self.text.len();
        self.text.push_str(s);
        let end = self.text.len();
        let mut span = Span::new(start, end);
        if st.bold {
            span = span.bold();
        }
        if st.italic {
            span = span.italic();
        }
        if let Some(c) = st.color {
            span = span.color(c);
        }
        if st.underline {
            span = span.underline(st.color.unwrap_or(md.p.text_chat));
        }
        if st.spoiler {
            // Hidden spoiler: `--markup-spoiler-hidden-bg` over invisible text.
            span = span.color(Color::TRANSPARENT).bg(mix(md.p.text_chat, Color::TRANSPARENT, 16.0), 2.0);
        }
        if st.strike {
            self.strikes.push((start, end));
        }
        self.spans.push(span);
    }

    fn push_span(&mut self, s: &str, span: impl Fn(Span) -> Span) {
        let start = self.text.len();
        self.text.push_str(s);
        self.spans.push(span(Span::new(start, self.text.len())));
    }

    fn el(self, size: f32, lh: f32, weight: f32, color: Color) -> Option<El> {
        if self.text.is_empty() {
            return None;
        }
        let mut e = rich(self.text, self.spans).font(font(size, weight).lh(lh)).color(color).w_full();
        for (a, b) in self.strikes {
            e = e.strike(a, b, None);
        }
        Some(e)
    }
}

fn is_block(n: &Node) -> bool {
    matches!(
        n,
        Node::Heading { .. }
            | Node::List { .. }
            | Node::CodeBlock { .. }
            | Node::Blockquote { .. }
            | Node::Subtext { .. }
            | Node::Table { .. }
            | Node::Alert { .. }
            | Node::Spoiler { is_block: Some(true), .. }
    )
}

/// The message body: blocks stacked, inline runs as rich text (16/22, `text-chat`).
pub fn render(content: &str, md: &Md) -> El {
    let nodes = reflux_markdown::parse(content, &Options::new(presets::STANDARD));
    blocks(&nodes, md, md.p.text_chat)
}

/// A one-line inline rendering (reply previews): block syntax flattened.
pub fn inline_text(content: &str, md: &Md, style: rune::tok::TextStyle, color: Color) -> El {
    let nodes = reflux_markdown::parse(content, &Options::new(presets::RESTRICTED_INLINE_REPLY));
    let mut run = Run::default();
    for n in &nodes {
        inline(n, Style::default(), &mut run, md);
    }
    let t = run.text.replace('\n', " ");
    rich(t, run.spans).font(style).color(color).truncate()
}

fn blocks(nodes: &[Node], md: &Md, color: Color) -> El {
    let mut out = col().w_full();
    let mut run = Run::default();
    let n = nodes.len();
    for (i, node) in nodes.iter().enumerate() {
        if !is_block(node) {
            inline(node, Style::default(), &mut run, md);
            continue;
        }
        if let Some(e) = std::mem::take(&mut run).el(16.0, 22.0, 400.0, color) {
            out = out.child(e);
        }
        out = out.child(block(node, md, color, i == 0, i + 1 == n));
    }
    if let Some(e) = run.el(16.0, 22.0, 400.0, color) {
        out = out.child(e);
    }
    out
}

fn block(node: &Node, md: &Md, color: Color, first: bool, last: bool) -> El {
    let p = md.p;
    match node {
        Node::Heading { level, children } => {
            let mut run = Run::default();
            children.iter().for_each(|c| inline(c, Style::default(), &mut run, md));
            // h1 1.375em, h2 1.25em, h3 1.125em; line-height 1.2; margin 20 0 12 (4 on top when first).
            let size = match level {
                1 => 22.0,
                2 => 20.0,
                3 => 18.0,
                _ => 16.0,
            };
            let e = run.el(size, (size * 1.2).round(), 600.0, p.text_primary).unwrap_or_else(el);
            e.mt(if first { 4.0 } else { 20.0 }).mb(if last { 0.0 } else { 12.0 })
        }
        Node::Subtext { children } => {
            let mut run = Run::default();
            children.iter().for_each(|c| inline(c, Style::default(), &mut run, md));
            run.el(13.0, 18.0, 400.0, p.text_tertiary).unwrap_or_else(el)
        }
        Node::CodeBlock { content, .. } => {
            // `.codeContainer`: radius 6, 1px border, padding 14; `pre` 12px mono, line-height 1.33.
            let body = content.strip_suffix('\n').unwrap_or(content);
            col()
                .w_full()
                .padding(15.0)
                .radius(6.0)
                .fill(p.bg_code_block)
                .border(1.0, crate::ui::over(p.code_block_border, p.bg_code_block))
                .mb(if last { 0.0 } else { 12.0 })
                .child(text(body.to_string()).font(font(12.0, 400.0).mono().lh(16.0)).color(p.text_primary).wrap())
        }
        Node::Blockquote { children, .. } => row()
            .w_full()
            .my(4.0)
            .child(el().w(4.0).self_stretch().radius(4.0).fill(p.interactive_muted).mr(12.0))
            .child(col().grow(1.0).min_w(0.0).py(4.0).child(blocks(children, md, p.text_chat_muted))),
        Node::Alert { children, alert_type } => {
            // shortcut: alerts draw as a tinted blockquote without the title row and icon.
            let c = match alert_type {
                reflux_markdown::AlertType::Note => p.alert_note_color,
                reflux_markdown::AlertType::Tip => p.alert_tip_color,
                reflux_markdown::AlertType::Important => p.alert_important_color,
                reflux_markdown::AlertType::Warning => p.alert_warning_color,
                reflux_markdown::AlertType::Caution => p.alert_caution_color,
            };
            row()
                .w_full()
                .my(4.0)
                .child(el().w(4.0).self_stretch().radius(4.0).fill(c).mr(12.0))
                .child(col().grow(1.0).min_w(0.0).py(4.0).child(blocks(children, md, color)))
        }
        Node::List { ordered, items } => {
            // margin 4 0 0 20; items 4 apart; the marker hangs in the left margin.
            let mut list = col().w_full().mt(4.0).pl(20.0).gap(4.0);
            for (i, item) in items.iter().enumerate() {
                let marker = if *ordered { format!("{}.", item.ordinal.unwrap_or(i + 1)) } else { "•".into() };
                list = list.child(
                    row()
                        .w_full()
                        .items_start()
                        .child(
                            row()
                                .w(20.0)
                                .ml(-20.0)
                                .justify_end()
                                .pr(6.0)
                                .child(text(marker).font(font(16.0, 400.0).lh(22.0)).color(color)),
                        )
                        .child(col().grow(1.0).min_w(0.0).child(blocks(&item.children, md, color))),
                );
            }
            list
        }
        Node::Spoiler { children, .. } => {
            col().w_full().padding(4.0).radius(6.0).fill(mix(color, Color::TRANSPARENT, 16.0)).child(blocks(children, md, Color::TRANSPARENT))
        }
        Node::Table { header, rows, .. } => {
            // shortcut: tables render as one line per row, cells separated by " │ ".
            let line = |r: &Node| {
                let mut run = Run::default();
                if let Node::TableRow { cells } = r {
                    for (i, c) in cells.iter().enumerate() {
                        if i > 0 {
                            run.push_span(" │ ", |s| s.color(p.text_tertiary));
                        }
                        inline(c, Style::default(), &mut run, md);
                    }
                }
                run.el(14.0, 20.0, 400.0, color).unwrap_or_else(el)
            };
            col().w_full().my(4.0).child(line(header).font(font(14.0, 600.0).lh(20.0))).children(rows.iter().map(line))
        }
        other => {
            let mut run = Run::default();
            inline(other, Style::default(), &mut run, md);
            run.el(16.0, 22.0, 400.0, color).unwrap_or_else(el)
        }
    }
}

fn inline(node: &Node, st: Style, run: &mut Run, md: &Md) {
    let p = md.p;
    let kids = |children: &[Node], st: Style, run: &mut Run| children.iter().for_each(|c| inline(c, st, run, md));
    match node {
        Node::Text { content } => run.push(content, st, md),
        Node::Strong { children } => kids(children, Style { bold: true, ..st }, run),
        Node::Emphasis { children } => kids(children, Style { italic: true, ..st }, run),
        Node::Underline { children } => kids(children, Style { underline: true, ..st }, run),
        Node::Strikethrough { children } => kids(children, Style { strike: true, ..st }, run),
        Node::Spoiler { children, .. } => kids(children, Style { spoiler: true, ..st }, run),
        Node::Sequence { children } | Node::TableCell { children } => kids(children, st, run),
        Node::Heading { children, .. } | Node::Subtext { children } | Node::Blockquote { children, .. } | Node::Alert { children, .. } => {
            kids(children, st, run)
        }
        Node::InlineCode { content } => {
            // `code.inline`: mono at 85%, `text-code` on `bg-code`.
            run.push_span(content, |s| s.mono().size(13.6).color(p.text_code).bg(p.bg_code, 2.0))
        }
        Node::CodeBlock { content, .. } => run.push_span(content, |s| s.mono().size(13.6).color(p.text_code).bg(p.bg_code, 2.0)),
        Node::Link { text: label, url, .. } => {
            let st = Style { color: Some(p.text_link), ..st };
            match label {
                Some(t) => inline(t, st, run, md),
                None => run.push(url, st, md),
            }
        }
        Node::Mention { kind } => {
            let label = match kind {
                MentionKind::User { id } => {
                    let uid = id.parse().map(Snowflake).unwrap_or_default();
                    let name = md.store.display_name(md.guild, uid);
                    format!("@{}", if name.is_empty() { "unknown-user".into() } else { name })
                }
                MentionKind::Channel { id } => {
                    let name = id.parse().ok().and_then(|c| md.store.channels.get(&Snowflake(c))).and_then(|c| c.name.clone());
                    format!("#{}", name.unwrap_or_else(|| "unknown-channel".into()))
                }
                MentionKind::Role { id } => {
                    let rid: Snowflake = id.parse().map(Snowflake).unwrap_or_default();
                    let name = md
                        .guild
                        .and_then(|g| md.store.guilds.get(&g))
                        .and_then(|g| g.roles.iter().find(|r| r.id == rid))
                        .map(|r| r.name.clone());
                    format!("@{}", name.unwrap_or_else(|| "unknown-role".into()))
                }
                MentionKind::Command { name, .. } => format!("/{name}"),
                MentionKind::GuildNavigation { .. } => "#Browse Channels".into(),
                MentionKind::Everyone => "@everyone".into(),
                MentionKind::Here => "@here".into(),
            };
            let (fg, bg) = match kind {
                MentionKind::Everyone => (p.markup_everyone_text, p.markup_everyone_fill),
                MentionKind::Here => (p.markup_here_text, p.markup_here_fill),
                _ => (p.markup_mention_text, p.markup_mention_fill),
            };
            run.push_span(&label, |s| s.weight(500.0).color(fg).bg(bg, 2.0));
        }
        Node::Emoji { kind } => match kind {
            EmojiKind::Standard { raw, .. } => run.push(raw, st, md),
            // shortcut: custom emoji show as `:name:` until emoji images load.
            EmojiKind::Custom { name, .. } => run.push(&format!(":{name}:"), st, md),
        },
        Node::Timestamp { timestamp, .. } => {
            let s = super::chat::format_full_date_time(*timestamp * 1000);
            run.push_span(&s, |sp| sp.bg(p.bg_code, 2.0));
        }
        Node::List { items, .. } => {
            for it in items {
                kids(&it.children, st, run);
            }
        }
        Node::Table { .. } | Node::TableRow { .. } => {}
    }
}
