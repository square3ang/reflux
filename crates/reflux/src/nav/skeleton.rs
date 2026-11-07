//! Member list skeletons (`channel/components/MemberListSkeleton.tsx`, `skeleton/Skeleton.module.css`):
//! `background-modifier-accent` pills at 45% (35% muted), with the shared shimmer.

use rune::{El, col, row};

use crate::theme::{Palette, dim};

/// A skeleton block (`SkeletonLine` is pill-shaped, `SkeletonCircle` round).
fn block(w: f32, h: f32, muted: bool, p: &Palette) -> El {
    crate::shell::skeleton::pill(w, h, muted, p)
}

/// `createSkeletonRandomFromKey`'s role: a stable 0..1 per row.
fn rand(i: usize, salt: u64) -> f32 {
    let mut x = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt;
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 32;
    (x % 10_000) as f32 / 10_000.0
}

/// `MemberListSkeletonRow` (guild variant): a 32px circle, then a name (40–80%) and a muted
/// status (30–80%) line, in the member row's box.
pub fn member_row(i: usize, p: &Palette) -> El {
    // The info column's width: the list minus padding, avatar and gap.
    let info_w = dim::LAYOUT_MEMBER_LIST_WIDTH - 2.0 * dim::MEMBER_LIST_SCROLLER_PADDING_INLINE - 2.0 * dim::MEMBER_LIST_ITEM_PADDING_INLINE - 32.0 - dim::MEMBER_LIST_ITEM_CONTENT_GAP;
    let name = (40.0 + rand(i, 1) * 40.0) / 100.0 * info_w;
    let status = (30.0 + rand(i, 2) * 50.0) / 100.0 * info_w;
    row()
        .w_full()
        .h(dim::MEMBER_LIST_ITEM_CONTENT_HEIGHT + 2.0 * dim::MEMBER_LIST_ITEM_PADDING_BLOCK)
        .py(dim::MEMBER_LIST_ITEM_PADDING_BLOCK)
        .px(dim::MEMBER_LIST_ITEM_PADDING_INLINE)
        .gap(dim::MEMBER_LIST_ITEM_CONTENT_GAP)
        .center_y()
        .child(block(32.0, 32.0, false, p))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .child(row().h(dim::MEMBER_LIST_ITEM_NAME_LINE_HEIGHT).center_y().child(block(name, 10.0, false, p)))
                .child(row().h(dim::MEMBER_LIST_ITEM_STATUS_LINE_HEIGHT).center_y().child(block(status, 8.0, true, p))),
        )
        .inert()
}

/// `MemberListSkeleton` before the list's first sync: three groups of 7, 9 and 8 rows.
pub fn member_list(p: &Palette) -> El {
    let mut out = col().w_full();
    let mut i = 0;
    for (heading, rows) in [(80.0, 7), (100.0, 9), (72.0, 8)] {
        out = out.child(row().w_full().h(32.0).pt(12.0).px(dim::MEMBER_LIST_ITEM_PADDING_INLINE).child(block(heading, 10.0, true, p)));
        for _ in 0..rows {
            out = out.child(member_row(i, p));
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn widths_are_stable_and_in_range() {
        for i in 0..50 {
            let r = super::rand(i, 1);
            assert!((0.0..1.0).contains(&r));
            assert_eq!(r, super::rand(i, 1));
        }
        assert_ne!(super::rand(3, 1), super::rand(4, 1));
    }
}
