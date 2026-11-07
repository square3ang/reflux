//! Message markdown (`reflux_markdown` AST) to Rune elements, styled after
//! `theme/styles/Markup.module.css`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

use reflux_markdown::{EmojiKind, MentionKind, Node, Options, presets};
use reflux_model::Snowflake;
use reflux_store::Store;
use rune::text::{Span, TextHighlight, TextSystem};
use rune::{Color, El, Pixels, col, el, rich, row, text};

use crate::i18n::t;
use crate::media::Load;
use crate::theme::{Palette, font};
use crate::ui::mix;

/// What copying each selectable message text takes, by the text's key: its message and, for
/// each emoji placeholder, the bytes it covers and what the emoji copies as.
pub type CopyMap = HashMap<String, (Snowflake, Vec<(Range<usize>, String)>)>;

#[derive(Clone)]
pub struct Md<'a> {
    pub store: &'a Store,
    pub media: &'a crate::media::Media,
    pub guild: Option<Snowflake>,
    pub p: &'a Palette,
    /// Prefix of the keys of the texts and emoji drawn, unique per markdown body on screen.
    pub key: String,
    /// A message body: its texts are selectable and note what they copy as here.
    pub copy: Option<(Snowflake, &'a RefCell<CopyMap>)>,
    texts: Cell<u32>,
    /// Makes a text's links pressable (adds its `on_link`); links are reported by URL.
    pub on_link: Option<std::rc::Rc<dyn Fn(El) -> El>>,
    /// Makes an emoji image pressable: (image, emoji, the image's key).
    pub on_emoji: Option<EmojiHook>,
}

/// See [`Md::on_emoji`].
pub type EmojiHook = std::rc::Rc<dyn Fn(El, &EmojiKind, String) -> El>;

impl<'a> Md<'a> {
    pub fn new(store: &'a Store, media: &'a crate::media::Media, guild: Option<Snowflake>, p: &'a Palette, key: impl Into<String>) -> Self {
        Md { store, media, guild, p, key: key.into(), copy: None, texts: Cell::new(0), on_link: None, on_emoji: None }
    }

    /// The same markdown context for another body.
    pub fn body(&self, key: impl Into<String>, copy: Option<(Snowflake, &'a RefCell<CopyMap>)>) -> Self {
        Md { key: key.into(), copy, texts: Cell::new(0), ..self.clone() }
    }

    /// A key for the next text, and the text made selectable when this is a message body.
    fn text(&self, t: El, subs: Vec<(Range<usize>, String)>) -> (El, String) {
        let n = self.texts.get();
        self.texts.set(n + 1);
        let key = format!("{}-t{n}", self.key);
        let mut t = t.key(key.clone());
        if let Some((msg, map)) = self.copy {
            t = t.selectable(true).selection_color(self.p.text_selection);
            map.borrow_mut().insert(key.clone(), (msg, subs));
        }
        (t, key)
    }
}

/// Stands in for an inline emoji in the text: no-break spaces sized (see [`placeholder_spans`])
/// so they're as wide as the 1.5em image (`--emoji-size-emoji`) and make their line as tall as
/// the image makes it in Fluxer; the image is drawn over them.
const PLACEHOLDER: &str = "\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}";
const NBSP: usize = '\u{a0}'.len_utf8();

/// The width of a no-break space per point of font size.
fn nbsp_em() -> f32 {
    thread_local!(static EM: Cell<(u64, f32)> = const { Cell::new((u64::MAX, 0.0)) });
    let epoch = rune::text::font_epoch();
    EM.with(|c| match c.get() {
        (e, em) if e == epoch => em,
        _ => {
            let em = rune::text::advance(&"\u{a0}".repeat(10), font(100.0, 400.0)) / 1000.0;
            c.set((epoch, em));
            em
        }
    })
}

/// `span` (over a [`PLACEHOLDER`] in `size`/`lh` text) split so the placeholder is 1.5em wide
/// and its line `max(lh, 1.5em)` tall, as a 1.5em image with `vertical-align: -0.4em` makes it.
/// A span's line height scales with its size, so the first spaces take the size that gives that
/// line height, one more makes up the width, and the rest are too small to count.
// shortcut: a scaled span keeps its half-leading even, so the line's baseline lands where that
// gives (17.6 → 18 in message text, but 24.2 → 26.6 in an h1); the image follows the baseline.
fn placeholder_spans(span: &Span, size: f32, lh: f32) -> [Span; 3] {
    let (width, em) = (1.5 * size, nbsp_em().max(0.01));
    let tall = size * lh.max(width) / lh;
    let n = ((width / (em * tall)).floor() as usize).min(PLACEHOLDER.len() / NBSP - 1);
    let rest = (width - n as f32 * em * tall) / em;
    let at = |i: usize| span.start + (i * NBSP) as u32;
    let part = |a: u32, b: u32, size: f32| Span { start: a, end: b, size: Some(size.max(0.01)), ..span.clone() };
    [part(at(0), at(n), tall), part(at(n), at(n + 1), rest.min(tall)), part(at(n + 1), span.end, 0.01)]
}

/// The baseline of a line holding an emoji in `size`/`lh` text, from the line's top.
fn baseline(size: f32, lh: f32) -> f32 {
    thread_local!(static TS: RefCell<Option<TextSystem>> = const { RefCell::new(None) });
    TS.with_borrow_mut(|ts| {
        let ts = ts.get_or_insert_with(|| TextSystem::new(crate::theme::FONTS, crate::theme::FAMILIES));
        let text = format!("x{PLACEHOLDER}");
        let spans = placeholder_spans(&Span::new(1, text.len()), size, lh);
        let l = ts.layout_rich(&text, &spans, font(size, 400.0).lh(lh), 1.0, None);
        l.lines().next().map_or(lh * 0.75, |line| line.metrics().baseline)
    })
}

/// Where an inline emoji image goes in its text box: centred on its placeholder's box `ph`
/// (`[x, y, w, h]`, its line's top to bottom) and, as `vertical-align: -0.4em` puts it, its
/// bottom 0.4em under the line's `baseline`. Returns `[x, y, side]`.
fn emoji_rect(text: [f32; 4], ph: [f32; 4], size: f32, baseline: f32) -> [f32; 3] {
    let side = 1.5 * size;
    [ph[0] - text[0] + (ph[2] - side) / 2.0, ph[1] - text[1] + baseline + 0.4 * size - side, side]
}

/// Copy text for the selected parts of message texts (`MessageSelectionCopyUtils`): each
/// emoji placeholder any of whose bytes are selected becomes the emoji's text, texts of one
/// message join with a newline, and a message starting a group is set off by a blank line.
/// `texts` are (key, selected range, selected text) in order.
pub fn copy_selection<'s>(texts: impl IntoIterator<Item = (Option<&'s str>, Range<usize>, &'s str)>, map: &CopyMap, heads: &HashSet<Snowflake>) -> String {
    let mut out = String::new();
    let mut last: Option<Option<Snowflake>> = None;
    for (key, r, sel) in texts {
        let found = key.and_then(|k| map.get(k));
        let msg = found.map(|f| f.0);
        let piece = |a: usize, b: usize| sel.get(a.max(r.start) - r.start..b.min(r.end).max(r.start) - r.start).unwrap_or("");
        let mut body = String::new();
        let mut at = r.start;
        for (ph, copy) in found.map_or(&[][..], |f| &f.1[..]) {
            if ph.end <= r.start || ph.start >= r.end {
                continue;
            }
            body += piece(at, ph.start);
            body += copy;
            at = ph.end;
        }
        body += piece(at, r.end);
        if last.is_some() {
            out += if last == Some(msg) || !msg.is_some_and(|m| heads.contains(&m)) { "\n" } else { "\n\n" };
        }
        out += &body;
        last = Some(msg);
    }
    // `sanitiseCopiedText`.
    let out = out.replace('\u{a0}', " ");
    let lines: Vec<&str> = out.split('\n').map(|l| l.trim_end_matches([' ', '\t'])).collect();
    lines.join("\n").trim_end().to_owned()
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

/// An inline emoji in a [`Run`]: its placeholder's bytes and span, the image (none while
/// hidden in a spoiler or loading) and what it copies as.
struct InlineEmoji {
    range: Range<usize>,
    span: usize,
    px: Option<Pixels>,
    copy: String,
    kind: EmojiKind,
}

/// One paragraph being built: text plus non-overlapping spans.
#[derive(Default)]
struct Run {
    text: String,
    spans: Vec<Span>,
    strikes: Vec<(usize, usize)>,
    emoji: Vec<InlineEmoji>,
    /// One-line previews draw emoji as text.
    plain_emoji: bool,
    /// Link byte ranges and their URLs.
    links: Vec<(Range<usize>, String)>,
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

    /// An emoji's placeholder; the image goes over it once the text is laid out.
    fn push_emoji(&mut self, px: Option<Pixels>, kind: &EmojiKind, st: Style, md: &Md) {
        let copy = emoji_text(kind);
        let start = self.text.len();
        self.push(PLACEHOLDER, st, md);
        let span = self.spans.len() - 1;
        if !st.spoiler {
            self.spans[span].color = Some(Color::TRANSPARENT);
        }
        self.emoji.push(InlineEmoji { range: start..self.text.len(), span, px: px.filter(|_| !st.spoiler), copy, kind: kind.clone() });
    }

    fn el(mut self, size: f32, lh: f32, weight: f32, color: Color, md: &Md) -> Option<El> {
        if self.text.is_empty() {
            return None;
        }
        // Each placeholder's span becomes three; later spans shift by two per earlier emoji.
        for (k, e) in self.emoji.iter().enumerate() {
            let i = e.span + 2 * k;
            let parts = placeholder_spans(&self.spans[i], size, lh);
            self.spans.splice(i..=i, parts);
        }
        let mut t = rich(self.text, self.spans).font(font(size, weight).lh(lh)).color(color).w_full();
        // `--markup-strikethrough-color`: dimmed to 55% unless turned off.
        let strike = crate::look::with(|l| l.dim_strikethrough).then(|| mix(color, Color::TRANSPARENT, 55.0));
        for (a, b) in self.strikes {
            t = t.strike(a, b, strike);
        }
        if let Some(f) = md.on_link.as_ref().filter(|_| !self.links.is_empty()) {
            let p_link = md.p.text_link;
            for (r, url) in std::mem::take(&mut self.links) {
                t = t.link(rune::Link::new(r, url).hover(rune::LinkPaint::new().underline(p_link)));
            }
            t = f(t);
        }
        let subs = self.emoji.iter().map(|e| (e.range.clone(), e.copy.clone())).collect();
        if self.emoji.is_empty() {
            return Some(md.text(t, subs).0);
        }
        let (mut t, key) = md.text(t, subs);
        let hl = |i: usize| format!("{key}-e{i}");
        t = t.text_highlights(self.emoji.iter().enumerate().map(|(i, e)| TextHighlight::new(e.range.clone(), Color::TRANSPARENT).key(hl(i))));
        // Placeholders are found where the runtime laid them out last frame (so a new or
        // re-wrapped text shows its emoji a frame late).
        let at = rune::memo::layout(&key);
        let base = baseline(size, lh);
        let imgs = self.emoji.iter().enumerate().filter_map(|(i, e)| {
            let [x, y, side] = emoji_rect(at?, rune::memo::layout(&hl(i))?, size, base);
            let img = el().absolute(x, y).size(side, side).pixels_fit(e.px.as_ref()?, rune::SurfaceFit::Fit).key(format!("{}-img", hl(i)));
            Some(match &md.on_emoji {
                Some(f) => f(img, &e.kind, format!("{}-img", hl(i))),
                None => img,
            })
        });
        Some(col().w_full().child(t).children(imgs.collect::<Vec<_>>()))
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
    let nodes = reflux_markdown::parse(content, &Options { flags: presets::STANDARD, emoji: super::emoji::context(content) });
    jumbo(&nodes, md).unwrap_or_else(|| blocks(&nodes, md, md.p.text_chat))
}

/// An emoji's image: custom ones from the media proxy, unicode ones from the bundled
/// spritesheet (Fluxer draws twemoji for both). `Failed` when there is none.
// shortcut: skin-tone variants and emoji missing from the base sheet fall back to the font
// glyph; Fluxer loads their twemoji SVG from its static host.
fn emoji_image(kind: &EmojiKind, md: &Md) -> Load {
    match kind {
        // `AnimateEmoji`: animated emoji move unless the user turned it off.
        EmojiKind::Custom { id, animated, .. } if *animated && md.store.settings.animate_emoji && !crate::look::reduced_motion() => {
            md.media.asset(|b, _| crate::media::emoji_url_animated(b, id))
        }
        EmojiKind::Custom { id, .. } => md.media.asset(|b, _| crate::media::emoji_url(b, id)),
        EmojiKind::Standard { raw, .. } => super::emoji::sprite_index(raw).map_or(Load::Failed, |i| md.media.sprite(i)),
    }
}

/// What an emoji copies as, and draws as without an image.
fn emoji_text(kind: &EmojiKind) -> String {
    match kind {
        EmojiKind::Standard { raw, .. } => raw.clone(),
        EmojiKind::Custom { name, .. } => format!(":{name}:"),
    }
}

/// `shouldRenderAsJumboEmojis` (only emoji, at most 30): a row of 48px
/// (`--emoji-size-jumbo-emoji`) emoji.
// shortcut: jumbo emoji can't be selected or copied (no text); "Copy text" has them.
fn jumbo(nodes: &[Node], md: &Md) -> Option<El> {
    let mut emoji = Vec::new();
    for n in nodes {
        match n {
            Node::Emoji { kind } => emoji.push(kind),
            Node::Text { content } if content.trim().is_empty() => {}
            _ => return None,
        }
    }
    if emoji.is_empty() || emoji.len() > 30 {
        return None;
    }
    let one = |(i, k): (usize, &&EmojiKind)| {
        let e = match emoji_image(k, md) {
            Load::Ready(px) => el().size(48.0, 48.0).pixels_fit(&px, rune::SurfaceFit::Fit),
            Load::Pending => el().size(48.0, 48.0),
            Load::Failed => text(emoji_text(k)).font(font(48.0, 400.0).lh(48.0)),
        };
        let key = format!("{}-j{i}", md.key);
        match &md.on_emoji {
            Some(f) => f(e.key(key.clone()), k, key),
            None => e,
        }
    };
    Some(row().w_full().flex_wrap().gaps(4.0, 4.0).children(emoji.iter().enumerate().map(one)))
}

/// A one-line inline rendering (reply previews): block syntax flattened.
pub fn inline_text(content: &str, md: &Md, style: rune::tok::TextStyle, color: Color) -> El {
    let nodes = reflux_markdown::parse(content, &Options { flags: presets::RESTRICTED_INLINE_REPLY, emoji: super::emoji::context(content) });
    // shortcut: emoji in one-line previews stay text (Fluxer draws 1.25em images).
    let mut run = Run { plain_emoji: true, ..Run::default() };
    for n in &nodes {
        inline(n, Style::default(), &mut run, md);
    }
    let t = run.text.replace('\n', " ");
    rich(t, run.spans).font(style).color(color).truncate()
}

/// The body text size and line height: 16/22 scaled by the chat font size setting.
fn body_size() -> (f32, f32) {
    let k = crate::look::with(|l| l.text_scale());
    (16.0 * k, (22.0 * k).round())
}

fn blocks(nodes: &[Node], md: &Md, color: Color) -> El {
    let (size, lh) = body_size();
    let mut out = col().w_full();
    let mut run = Run::default();
    let n = nodes.len();
    for (i, node) in nodes.iter().enumerate() {
        if !is_block(node) {
            inline(node, Style::default(), &mut run, md);
            continue;
        }
        if let Some(e) = std::mem::take(&mut run).el(size, lh, 400.0, color, md) {
            out = out.child(e);
        }
        out = out.child(block(node, md, color, i == 0, i + 1 == n));
    }
    if let Some(e) = run.el(size, lh, 400.0, color, md) {
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
            let k = crate::look::with(|l| l.text_scale());
            let size = k * match level {
                1 => 22.0,
                2 => 20.0,
                3 => 18.0,
                _ => 16.0,
            };
            let e = run.el(size, (size * 1.2).round(), 600.0, p.text_primary, md).unwrap_or_else(el);
            e.mt(if first { 4.0 } else { 20.0 }).mb(if last { 0.0 } else { 12.0 })
        }
        Node::Subtext { children } => {
            let mut run = Run::default();
            children.iter().for_each(|c| inline(c, Style::default(), &mut run, md));
            run.el(13.0, 18.0, 400.0, p.text_tertiary, md).unwrap_or_else(el)
        }
        Node::CodeBlock { content, language } => {
            // `.codeContainer`: radius 6, 1px border, padding 14; `pre` 12px mono, line-height 1.33.
            let body = content.strip_suffix('\n').unwrap_or(content);
            let spans = super::highlight::spans(language.as_deref().unwrap_or_default(), body, crate::ui::is_light(p));
            col()
                .w_full()
                .padding(15.0)
                .radius(6.0)
                .fill(p.bg_code_block)
                .border(1.0, crate::ui::over(p.code_block_border, p.bg_code_block))
                .mb(if last { 0.0 } else { 12.0 })
                .child(md.text(rich(body.to_string(), spans).font(font(12.0, 400.0).mono().lh(16.0)).color(p.text_primary).wrap(), Vec::new()).0)
        }
        Node::Blockquote { children, .. } => row()
            .w_full()
            .my(4.0)
            .child(el().w(4.0).self_stretch().radius(4.0).fill(p.interactive_muted).mr(12.0))
            .child(col().grow(1.0).min_w(0.0).py(4.0).child(blocks(children, md, p.text_chat_muted))),
        Node::Alert { children, alert_type } => {
            // `AlertRenderer`: a 4px bar in the alert's colour, its icon and title, then the body.
            use reflux_markdown::AlertType as A;
            let (c, glyph, title) = match alert_type {
                A::Note => (p.alert_note_color, &crate::icons::INFO, t("Note")),
                A::Tip => (p.alert_tip_color, &crate::icons::LIGHTBULB_FILAMENT, t("Tip")),
                A::Important => (p.alert_important_color, &crate::icons::WARNING, t("Important")),
                A::Warning => (p.alert_warning_color, &crate::icons::WARNING_OCTAGON, t("Warning")),
                A::Caution => (p.alert_caution_color, &crate::icons::WARNING_CIRCLE, t("Caution")),
            };
            let (size, _) = body_size();
            let title = row().gap(4.0).center_y().mb(4.0).child(rune::icon(glyph, size * 0.9375).color(c)).child(text(title).font(font(size * 0.9375, 600.0)).color(c));
            row()
                .w_full()
                .mt(4.0)
                .mb(if last { 0.0 } else { 4.0 })
                .child(el().w(4.0).self_stretch().radius(4.0).fill(c).mr(12.0))
                .child(col().grow(1.0).min_w(0.0).py(6.0).pr(12.0).child(title).child(col().w_full().mt(4.0).child(blocks(children, md, color))))
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
                                .child(text(marker).font(font(body_size().0, 400.0).lh(body_size().1)).color(color)),
                        )
                        .child(col().grow(1.0).min_w(0.0).child(blocks(&item.children, md, color))),
                );
            }
            list
        }
        Node::Spoiler { children, .. } => {
            col().w_full().padding(4.0).radius(6.0).fill(mix(color, Color::TRANSPARENT, 16.0)).child(blocks(children, md, Color::TRANSPARENT))
        }
        Node::Table { header, rows, alignments } => {
            // `TableRenderer`: 14px cells padded 8×12, at least 80 wide, a 600-weight header on
            // `bg-table-header`, rows striped odd/even, inside a rounded border; too wide, it
            // scrolls sideways. Columns are stacks of cells, so every cell is one line tall (the
            // table is `max-content`, so cells don't wrap anyway).
            let cells = |r: &Node| match r {
                Node::TableRow { cells } => cells.clone(),
                _ => Vec::new(),
            };
            let all: Vec<Vec<Node>> = std::iter::once(cells(header)).chain(rows.iter().map(cells)).collect();
            let n = all.iter().map(Vec::len).max().unwrap_or(0);
            let mut grid = row().radius(8.0).clip_rounded(8.0).border(1.0, p.border_color);
            for ci in 0..n {
                let mut column = col().min_w(80.0);
                for (ri, r) in all.iter().enumerate() {
                    let (bg, weight, fg) = match ri {
                        0 => (p.bg_table_header, 600.0, p.text_primary),
                        i if i % 2 == 1 => (p.bg_table_row_odd, 400.0, color),
                        _ => (p.bg_table_row_even, 400.0, color),
                    };
                    let mut run = Run::default();
                    if let Some(c) = r.get(ci) {
                        inline(c, Style::default(), &mut run, md);
                    }
                    let align = match alignments.get(ci) {
                        Some(reflux_markdown::TableAlignment::Center) => rune::Align::Center,
                        Some(reflux_markdown::TableAlignment::Right) => rune::Align::End,
                        _ => rune::Align::Start,
                    };
                    let cell = row().w_full().h(34.6).px(12.0).center_y().fill(bg);
                    column = column.child(cell.children(run.el(14.0, 19.6, weight, fg, md).map(|t| t.align(align))));
                    if ri + 1 < all.len() {
                        column = column.child(el().w_full().h(1.0).fill(p.border_color));
                    }
                }
                grid = grid.child(column);
            }
            row().w_full().mb(if last { 0.0 } else { 12.0 }).scroll_x().child(row().child(grid))
        }
        other => {
            let mut run = Run::default();
            inline(other, Style::default(), &mut run, md);
            run.el(16.0, 22.0, 400.0, color, md).unwrap_or_else(el)
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
            run.push_span(content, |s| s.mono().size(13.6 * crate::look::with(|l| l.text_scale())).color(p.text_code).bg(p.bg_code, 2.0))
        }
        Node::CodeBlock { content, .. } => run.push_span(content, |s| s.mono().size(13.6 * crate::look::with(|l| l.text_scale())).color(p.text_code).bg(p.bg_code, 2.0)),
        Node::Link { text: label, url, .. } => {
            // `--link-decoration`: "Always underline links".
            let st = Style { color: Some(p.text_link), underline: st.underline || crate::look::with(|l| l.underline_links), ..st };
            let start = run.text.len();
            match label {
                Some(t) => inline(t, st, run, md),
                None => run.push(url, st, md),
            }
            run.links.push((start..run.text.len(), url.clone()));
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
        // Copied as `renderEmojiToPlaintext` writes them; drawn as text when there's no image.
        Node::Emoji { kind } if run.plain_emoji => run.push(&emoji_text(kind), st, md),
        Node::Emoji { kind } => match emoji_image(kind, md) {
            Load::Failed => run.push(&emoji_text(kind), st, md),
            Load::Pending => run.push_emoji(None, kind, st, md),
            Load::Ready(px) => run.push_emoji(Some(px), kind, st, md),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_as_wide_and_tall_as_fluxers_emoji() {
        let mut ts = TextSystem::new(crate::theme::FONTS, crate::theme::FAMILIES);
        // Message text, an h1 and subtext.
        for (size, lh) in [(16.0, 22.0), (22.0, 26.0), (13.0, 18.0)] {
            let plain = ts.layout_rich("ab", &[], font(size, 400.0).lh(lh), 1.0, None).full_width();
            let text = format!("a{PLACEHOLDER}b");
            let spans = placeholder_spans(&Span::new(1, 1 + PLACEHOLDER.len()), size, lh);
            let l = ts.layout_rich(&text, &spans, font(size, 400.0).lh(lh), 1.0, None);
            let w = l.full_width() - plain;
            assert!((w - 1.5 * size).abs() < 0.05, "{size}: placeholder {w} wide");
            let h = l.height();
            assert!((h - lh.max(1.5 * size)).abs() < 0.5, "{size}/{lh}: line {h} tall");
        }
        // In message text Fluxer's baseline sits 1.1em (17.6) under the top of the line the
        // image fills; ours is within a point of it.
        assert!((baseline(16.0, 22.0) - 17.6).abs() <= 1.0, "baseline {}", baseline(16.0, 22.0));
    }

    #[test]
    fn emoji_images_centre_on_their_placeholder_and_sit_on_the_baseline() {
        // Text box at (100, 200); placeholder on its second 24px line at x 40..64.
        let [x, y, side] = emoji_rect([100.0, 200.0, 600.0, 48.0], [140.0, 224.0, 24.0, 24.0], 16.0, 18.0);
        assert_eq!((x, side), (40.0, 24.0));
        // Bottom 0.4em (6.4) under the baseline 18 down the line.
        assert!((y + side - (24.0 + 18.0 + 6.4)).abs() < 1e-4, "{y}");
        // A placeholder a little wider than the image (bold) centres it.
        assert_eq!(emoji_rect([0.0; 4], [10.0, 0.0, 26.0, 24.0], 16.0, 18.0)[0], 11.0);
    }

    #[test]
    fn copying_maps_placeholders_back_to_emoji() {
        let ph = PLACEHOLDER;
        let drawn = format!("hi {ph} there {ph}!");
        let (a, b) = (3, 3 + ph.len());
        let (c, d) = (b + 7, b + 7 + ph.len());
        let mut map = CopyMap::new();
        map.insert("m1-t0".into(), (Snowflake(1), vec![(a..b, "😀".into()), (c..d, ":fluxer:".into())]));
        map.insert("m1-t1".into(), (Snowflake(1), vec![]));
        map.insert("m2-t0".into(), (Snowflake(2), vec![]));
        map.insert("m3-t0".into(), (Snowflake(3), vec![]));
        let heads = HashSet::from([Snowflake(1), Snowflake(3)]);
        let one = |r: Range<usize>| copy_selection([(Some("m1-t0"), r.clone(), &drawn[r])], &map, &heads);
        assert_eq!(one(0..drawn.len()), "hi 😀 there :fluxer:!");
        // A selection reaching into a placeholder takes the whole emoji, from either side.
        assert_eq!(one(0..a + 2), "hi 😀");
        assert_eq!(one(d - 2..drawn.len()), ":fluxer:!");
        assert_eq!(one(a + 2..a + 4), "😀");
        // Ending right at a placeholder's start leaves it out (and the trailing space).
        assert_eq!(one(0..a), "hi");
        // Across messages: a newline inside one, a blank line before a group's first message.
        let texts = [
            (Some("m1-t0"), b + 1..drawn.len(), &drawn[b + 1..]),
            (Some("m1-t1"), 0..4, "code"),
            (Some("m2-t0"), 0..3, "two"),
            (Some("m3-t0"), 0..5, "three"),
        ];
        assert_eq!(copy_selection(texts, &map, &heads), "there :fluxer:!\ncode\ntwo\n\nthree");
        // Texts the map doesn't know copy as drawn.
        assert_eq!(copy_selection([(None, 0..4, "a\u{a0}b")], &map, &heads), "a b");
    }
}
