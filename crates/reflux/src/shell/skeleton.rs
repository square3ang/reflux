//! Loading skeletons: the message list's `ScrollFillerSkeleton` (with `PlaceholderSpecs`'
//! seeded groups), the startup `AppSkeleton` and the profile popout's
//! `UserProfileLoadingSkeleton`.

use reflux_model::Snowflake;
use rune::{Color, El, col, el, row};

use crate::theme::{Palette, dim};

/// `SkeletonEmphasis`: the `--skeleton-opacity` of a block.
#[derive(Clone, Copy)]
enum Emphasis {
    Strong,
    Default,
    Muted,
}

fn opacity(e: Emphasis) -> f32 {
    match e {
        Emphasis::Strong => 0.55,
        Emphasis::Default => 0.45,
        Emphasis::Muted => 0.35,
    }
}

fn tone(p: &Palette, e: Emphasis) -> Color {
    let c = p.background_modifier_accent;
    Color { a: c.a * opacity(e), ..c }
}

/// `.skeleton`: the accent fill at the emphasis' opacity with the 3.2 s `skeleton-shimmer`:
/// a 105° band of 7% `--text-primary`, twice the block's width, swept from `-50%` to `150%`
/// (right to left). Reduced motion drops the band, as Fluxer does.
fn block(b: El, radius: f32, e: Emphasis, p: &Palette) -> El {
    let b = b.radius(radius).fill(tone(p, e));
    if crate::look::reduced_motion() {
        return b;
    }
    let peak = Color { a: 0.07 * opacity(e), ..p.text_primary };
    let clear = Color { a: 0.0, ..peak };
    let band = el()
        .pin(Some(0.0), Some(0.0), None, Some(0.0))
        .w_frac(2.0)
        .linear_gradient(105f32.to_radians(), [(0.0, clear), (0.5, peak), (1.0, clear)])
        // `background-position` -50% -> 150% of (block - image) moves the 2x-wide image's left
        // edge from +1/2 to -3/2 block widths: +1/4 to -3/4 of its own width.
        .looping(rune::LoopSpec::slide(3.2, [0.25, 0.0], [-0.75, 0.0]).easing(rune::Easing::LINEAR))
        .inert();
    b.clip_rounded(radius).child(band)
}

/// `SkeletonLine`: a pill `frac` of its row wide.
fn line(frac: f32, h: f32, e: Emphasis, p: &Palette) -> El {
    block(el().w_frac(frac / 100.0).h(h), h / 2.0, e, p)
}

/// A fixed-size pill (`SkeletonLine` / `SkeletonCircle`) for other skeletons.
pub(crate) fn pill(w: f32, h: f32, muted: bool, p: &Palette) -> El {
    block(el().size(w, h), h / 2.0, if muted { Emphasis::Muted } else { Emphasis::Default }, p)
}

fn circle(size: f32, p: &Palette) -> El {
    block(el().size(size, size), size / 2.0, Emphasis::Default, p)
}

/// `createSkeletonRandomFromKey`: FNV-1a over the key seeding a xorshift32.
pub struct Rng(u32);

impl Rng {
    pub fn from_key(key: &str) -> Self {
        let mut h: u32 = 2_166_136_261;
        for u in key.encode_utf16() {
            h ^= u as u32;
            h = h.wrapping_mul(16_777_619);
        }
        Rng(if h == 0 { 0x9e37_79b9 } else { h })
    }

    pub fn next(&mut self) -> f64 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        s as f64 / 4_294_967_296.0
    }

    fn range(&mut self, min: u32, max: u32) -> u32 {
        (self.next() * (max - min + 1) as f64).floor() as u32 + min
    }
}

/// `PlaceholderMessageGroup`.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub lines: Vec<f32>,
    pub username: f32,
    pub timestamp: f32,
    pub attachment: Option<(u32, u32)>,
}

const LINE: f32 = dim::MESSAGE_LINE_HEIGHT;
const ROW_PAD: f32 = dim::MESSAGE_SPACING_Y * 2.0;

/// `generatePlaceholderSpecs` for a viewport `viewport` tall.
pub fn specs(seed: &str, compact: bool, group_spacing: f32, viewport: f32) -> Vec<Group> {
    let mean_lines = (1.0 + 4.0) / 2.0;
    let avg = if compact { (LINE + ROW_PAD) * mean_lines } else { 2.0 * LINE + ROW_PAD + (LINE + ROW_PAD) * (mean_lines - 1.0) };
    let rows = (viewport.max(400.0) / avg).ceil();
    let clamped = (1.05 * rows).ceil().clamp(10.0, 30.0);
    let n = if compact { clamped } else { (0.87 * clamped).round() } as usize;
    let attachments = n.min(((n as f32 / 30.0) * 8.0).round().max(1.0) as usize);
    let density = if compact { "1" } else { "0" };
    let mut r = Rng::from_key(&format!("{seed}|{density}|{group_spacing}"));
    let mut groups: Vec<Group> = (0..n)
        .map(|_| {
            let count = (r.next() * 4.0).floor() as usize + 1;
            let lines = (0..count).map(|_| (75.0 + r.next() * 18.0) as f32).collect();
            Group { lines, username: (48.0 + r.next() * 36.0) as f32, timestamp: (8.0 + r.next() * 12.0) as f32, attachment: None }
        })
        .collect();
    let mut free: Vec<usize> = (0..groups.len()).collect();
    for _ in 0..attachments {
        if free.is_empty() {
            break;
        }
        let i = free.remove((r.next() * free.len() as f64).floor() as usize);
        groups[i].attachment = Some((r.range(140, 400), r.range(100, 320)));
    }
    groups
}

/// `ScrollFillerSkeleton`: placeholder message groups while a channel's messages load.
pub fn messages(channel: Snowflake, compact: bool, p: &Palette) -> El {
    let compact_avatars = crate::look::with(|l| l.compact_avatars);
    let spacing = crate::look::with(|l| if compact { l.compact_group_spacing } else { l.group_spacing });
    let groups = specs(&channel.to_string(), compact, spacing, 800.0);
    let n = groups.len();
    let pad_x = if compact { dim::MESSAGE_COMPACT_HORIZONTAL_PADDING } else { dim::CHAT_HORIZONTAL_PADDING_DEFAULT };
    let attachment = |a: Option<(u32, u32)>, indent: f32| a.map(|(w, h)| row().pt(8.0).pl(indent).child(block(el().size(w as f32, h as f32), 8.0, Emphasis::Muted, p)));
    let rows = groups.into_iter().enumerate().map(|(i, g)| {
        let mut group = col().w_full();
        if compact {
            let stamp = dim::MESSAGE_COMPACT_INDENT - dim::MESSAGE_COMPACT_GAP;
            for (k, w) in g.lines.iter().enumerate() {
                let mut body = row().grow(1.0).min_w(0.0).gap(4.0).center_y();
                if k == 0 {
                    if compact_avatars {
                        body = body.child(circle(dim::MESSAGE_AVATAR_SIZE_COMPACT, p).mr(dim::MESSAGE_COMPACT_GAP));
                    }
                    body = body.child(row().w_frac(g.username.min(32.0 / 0.45) * 0.45 / 100.0).min_w(48.0).max_w(128.0).child(line(100.0, 12.0, Emphasis::Strong, p)));
                }
                body = body.child(row().grow(1.0).min_w(0.0).child(line(*w, 12.0, Emphasis::Default, p)));
                group = group.child(
                    row()
                        .w_full()
                        .min_h(LINE + ROW_PAD)
                        .center_y()
                        .gap(dim::MESSAGE_COMPACT_GAP)
                        .child(row().w(stamp).child(line((55.0 + g.timestamp).min(78.0), 10.0, Emphasis::Muted, p)))
                        .child(body),
                );
            }
            group = group.children(attachment(g.attachment, dim::MESSAGE_COMPACT_INDENT));
        } else {
            let head = row()
                .w_full()
                .gap(dim::MESSAGE_GUTTER)
                .items_start()
                .py(dim::MESSAGE_SPACING_Y)
                .child(row().mt(dim::MESSAGE_SPACING_Y + dim::MESSAGE_AVATAR_ALIGN_OFFSET).child(circle(dim::MESSAGE_AVATAR_SIZE, p)))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .child(row().w_full().h(LINE).gap(12.0).center_y().child(line(g.username, 12.0, Emphasis::Strong, p)).child(line(g.timestamp, 10.0, Emphasis::Muted, p)))
                        .child(row().w_full().h(LINE).center_y().child(line(g.lines[0], 12.0, Emphasis::Default, p))),
                );
            group = group.child(head);
            for w in &g.lines[1..] {
                group = group.child(row().w_full().min_h(LINE + ROW_PAD).center_y().pl(dim::MESSAGE_AVATAR_SIZE + dim::MESSAGE_GUTTER).child(line(*w, 12.0, Emphasis::Default, p)));
            }
            group = group.children(attachment(g.attachment, dim::MESSAGE_AVATAR_SIZE + dim::MESSAGE_GUTTER));
        }
        if i + 1 < n { group.mb(spacing) } else { group }
    });
    col().w_full().pt(16.0).pb(12.0).px(pad_x).key("msgs-skeleton").role("status").label(crate::i18n::t("Loading messages")).children(rows)
}

/// `forceLoadingSkeleton` (and not `bypassLoadingSkeleton`, which wins in `isClientBooting`).
pub fn forced(o: &super::devtools::Options) -> bool {
    o.force_loading_skeleton && !o.bypass_loading_skeleton
}

/// The booting screen: only the skeleton (Fluxer doesn't mount the app behind it), which
/// fades out over the app when it goes (`.containerExiting`, `useLoadingSkeletonRetention`).
pub fn screen(p: &Palette) -> El {
    rune_kit::root(el().w_full().h_full().fill(p.background_tertiary), vec![app(p).pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).transition(rune::Transition::asymmetric(rune::Transition::none(), rune::Transition::opacity()))])
}

/// `AppSkeleton`: the rail, a channel sidebar, the chat column and the user area, shown
/// until the first READY.
// shortcut: fixed rail / sidebar / member shapes; no `SkeletonLayoutMemory` of the last layout. Port it if the jump on READY matters.
pub fn app(p: &Palette) -> El {
    let rail = col()
        .w(dim::LAYOUT_GUILD_LIST_WIDTH)
        .h_full()
        .pt(12.0)
        .gap(dim::GUILD_LIST_ITEM_GAP + 4.0)
        .items_center()
        .child(block(el().size(dim::GUILD_LIST_ITEM_BOX_SIZE, dim::GUILD_LIST_ITEM_BOX_SIZE), 16.0, Emphasis::Strong, p))
        .child(block(el().w(32.0).h(2.0), 1.0, Emphasis::Muted, p))
        .children((0..7).map(|_| circle(dim::GUILD_LIST_ITEM_BOX_SIZE, p)));
    let mut r = Rng::from_key("app-skeleton");
    let channels = (0..14).map(|i| {
        let w = 40.0 + r.next() as f32 * 40.0;
        if i % 6 == 0 {
            row().w_full().h(28.0).pt(12.0).px(8.0).center_y().child(line(w * 0.6, 10.0, Emphasis::Muted, p))
        } else {
            row().w_full().h(34.0).px(8.0).gap(8.0).center_y().child(block(el().size(20.0, 20.0), 4.0, Emphasis::Muted, p)).child(line(w, 12.0, Emphasis::Default, p))
        }
    });
    let user_area = row()
        .w_full()
        .h(dim::LAYOUT_USER_AREA_HEIGHT)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .fill(p.background_secondary_alt)
        .child(circle(32.0, p))
        .child(col().grow(1.0).gap(6.0).child(line(60.0, 12.0, Emphasis::Strong, p)).child(line(40.0, 10.0, Emphasis::Muted, p)));
    let sidebar = col()
        .w(dim::LAYOUT_SIDEBAR_WIDTH)
        .h_full()
        .fill(p.background_secondary)
        .child(row().w_full().h(dim::LAYOUT_HEADER_HEIGHT).px(16.0).center_y().child(line(55.0, 14.0, Emphasis::Strong, p)))
        .child(el().w_full().h(1.0).fill(p.skeleton_chrome_border_color))
        .child(col().w_full().grow(1.0).min_h(0.0).px(8.0).pt(8.0).clip_rounded(0.0).children(channels))
        .child(user_area);
    let header = row().w_full().h(dim::LAYOUT_HEADER_HEIGHT).px(16.0).gap(8.0).center_y().child(block(el().size(24.0, 24.0), 6.0, Emphasis::Muted, p)).child(line(18.0, 14.0, Emphasis::Strong, p));
    let composer = col().w_full().px(dim::FOOTER_BOX_INSET).pb(dim::COMPOSER_BOX_INSET).child(el().w_full().h(dim::FOOTER_BOX_HEIGHT).radius(dim::FOOTER_BOX_RADIUS).fill(p.background_textarea));
    let chat = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .fill(p.background_secondary_lighter)
        .child(header)
        .child(el().w_full().h(1.0).fill(p.skeleton_chrome_border_color))
        .child(col().w_full().grow(1.0).min_h(0.0).justify_end().clip_rounded(0.0).child(messages(Snowflake(0), false, p)))
        .child(composer);
    let members = col().w(dim::LAYOUT_MEMBER_LIST_WIDTH).h_full().fill(p.background_secondary).clip_rounded(0.0).child(crate::nav::skeleton::member_list(p));
    row().w_full().h_full().fill(p.background_tertiary).key("app-skeleton").role("status").label(crate::i18n::t("Loading")).child(rail).child(sidebar).child(chat).child(members)
}

/// `UserProfileLoadingSkeleton` (popout): banner with the avatar slot, name / username /
/// status lines, an activity block, two sections, role pills, connection tiles and the button.
pub fn profile_popout(border: Color, p: &Palette) -> El {
    let fill = p.background_secondary_alt;
    let strong = p.background_tertiary;
    let bar = |frac: f32, h: f32| el().w_frac(frac / 100.0).h(h).radius(h / 2.0).fill(fill);
    let section = |lines: &[f32]| col().w_full().gap(6.0).child(bar(34.0, 14.0)).children(lines.iter().map(|w| bar(*w, 12.0)));
    let pill = |w: f32| el().w(w).h(22.0).radius(11.0).fill(fill);
    let header = col()
        .w_full()
        .h(140.0)
        .child(el().w_full().h(105.0).fill(strong))
        .child(el().pin(Some(10.0), Some(55.0), None, None).size(80.0, 80.0).radius(44.0).border(3.2, p.background_primary).fill(strong))
        .child(el().pin(Some(10.0 + 80.0 - 18.0 + 3.2), Some(55.0 + 80.0 - 18.0 + 3.2), None, None).size(24.0, 24.0).radius(12.0).border(4.0, p.background_primary).fill(strong));
    let content = col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(10.0)
        .px(16.0)
        .py(8.0)
        .clip_rounded(0.0)
        .child(bar(72.0, 22.0).mt(2.0))
        .child(bar(48.0, 14.0))
        .child(bar(78.0, 16.0))
        .child(el().w_full().h(48.0).radius(6.0).fill(fill))
        .child(section(&[92.0, 74.0, 54.0]))
        .child(section(&[74.0, 54.0]))
        .child(row().gap(6.0).child(pill(68.0)).child(pill(58.0)).child(pill(72.0)))
        .child(row().gap(6.0).children((0..3).map(|_| el().size(30.0, 30.0).radius(6.0).fill(fill))));
    let footer = row().w_full().pt(8.0).px(12.0).pb(12.0).child(el().w_full().h(32.0).radius(6.0).fill(fill));
    col()
        .w(300.0)
        .h(520.0)
        .radius(6.0)
        .border(2.5, border)
        .fill(p.background_primary)
        .clip_rounded(6.0)
        .role("status")
        .label(crate::i18n::t("Loading profile"))
        .child(col().w_full().grow(1.0).min_h(0.0).cycle(PULSE).child(header).child(content).child(footer))
}

/// The popout's pulse: opacity 1 → 0.65 → 1 over 1.4 s, ease-in-out each way (sampled at
/// eighths); reduced motion holds it at 1.
const PULSE: rune::Cycle = rune::Cycle {
    period: 1.4,
    phase: 0.0,
    stops: &[(0.0, 1.0), (0.125, 0.955), (0.25, 0.825), (0.375, 0.695), (0.5, 0.65), (0.625, 0.695), (0.75, 0.825), (0.875, 0.955), (1.0, 1.0)],
    still: 1.0,
    fps: 30.0,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_like_fluxer() {
        // `hashString("a")` is FNV-1a: 0xe40c292c.
        assert_eq!(Rng::from_key("a").0, 0xe40c_292c);
        let a = specs("123", false, 16.0, 800.0);
        assert_eq!(a, specs("123", false, 16.0, 800.0), "the same channel draws the same placeholder");
        assert_ne!(a, specs("124", false, 16.0, 800.0));
        // Cozy: ceil(800 / avg) rows, ×1.05, clamped to 10..30, ×0.87.
        assert_eq!(a.len(), 10);
        assert_eq!(specs("1", true, 0.0, 800.0).len(), 14);
        assert!(a.iter().all(|g| (1..=4).contains(&g.lines.len()) && g.lines.iter().all(|w| (75.0..93.0).contains(w))));
        assert_eq!(a.iter().filter(|g| g.attachment.is_some()).count(), 3);
    }
}
