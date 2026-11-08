//! The tray icon (`fluxer_desktop/src/main/DesktopTray.ts`): Fluxer's icon, with the red unread
//! dot of `TrayIconImage.ts` while mentions are pending, and a menu of show/hide, mute, deafen,
//! disconnect and quit.

use rune::tray::{Tray, TrayEvent, TrayIcon, TrayItem, TraySpec};

use crate::i18n::{t, tf};

/// What the menu shows; the tray is rebuilt when this changes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    pub dot: bool,
    pub visible: bool,
    pub in_voice: bool,
    pub mute: bool,
    pub deaf: bool,
}

pub fn menu(l: &Look) -> Vec<TrayItem> {
    let app = [("appName", "Fluxer")];
    let mut m = vec![
        TrayItem::action("toggle", if l.visible { tf("Hide {appName}", &app) } else { tf("Show {appName}", &app) }),
        TrayItem::action("settings", t("Open settings")),
        TrayItem::Separator,
        TrayItem::action("mute", t(if l.mute { "Unmute microphone" } else { "Mute microphone" })),
        TrayItem::action("deafen", t(if l.deaf { "Undeafen" } else { "Deafen" })),
    ];
    // shortcut: Fluxer only adds Disconnect while connected, but Rune's SNI menu numbers items
    // by position and KDE keeps an id's old type across a layout update (a separator stays one,
    // wearing the next label). Keeping the shape fixed avoids that; disabled outside a call.
    m.push(TrayItem::action("disconnect", t("Disconnect")).enabled(l.in_voice));
    m.push(TrayItem::Separator);
    m.push(TrayItem::action("quit", tf("Quit {appName}", &app)));
    m
}

fn icon(dot: bool) -> TrayIcon {
    let img = image::load_from_memory(include_bytes!("../../../../assets/media/images/tray.png")).expect("tray icon").to_rgba8();
    let (w, h) = img.dimensions();
    let mut px = img.into_raw();
    if dot {
        paint_dot(&mut px, w, h);
    }
    TrayIcon::rgba(w, h, px)
}

/// `paintTrayIndicatorDot`: a red dot in the top-right corner with a transparent gap around it.
fn paint_dot(px: &mut [u8], w: u32, h: u32) {
    let size = w.min(h) as f32;
    let r = size * 0.22;
    let cut = r + size * 0.0625;
    let (cx, cy) = (w as f32 - r, r);
    // Coverage of a circle over one pixel, 4×4 subsamples.
    let cover = |x: u32, y: u32, radius: f32| {
        let hits = (0..16)
            .filter(|i| {
                let dx = x as f32 + ((i % 4) as f32 + 0.5) / 4.0 - cx;
                let dy = y as f32 + ((i / 4) as f32 + 0.5) / 4.0 - cy;
                dx * dx + dy * dy <= radius * radius
            })
            .count();
        hits as f32 / 16.0
    };
    for y in 0..h {
        for x in 0..w {
            let c = cover(x, y, cut);
            if c == 0.0 {
                continue;
            }
            let d = cover(x, y, r);
            let o = ((y * w + x) * 4) as usize;
            for (ch, v) in [0xdd, 0, 0].into_iter().enumerate() {
                px[o + ch] = (px[o + ch] as f32 * (1.0 - c) + v as f32 * d).round() as u8;
            }
            px[o + 3] = (px[o + 3] as f32 * (1.0 - c) + 255.0 * d).round() as u8;
        }
    }
}

pub fn create(l: &Look, on_event: impl Fn(TrayEvent) + Send + Sync + 'static) -> Option<Tray> {
    let spec = TraySpec::new(super::notify::DESKTOP_ID, "Fluxer", icon(l.dot)).tooltip("Fluxer").menu(menu(l));
    Tray::new(spec, on_event).map_err(|e| eprintln!("reflux: tray: {e}")).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn dot_paints_the_corner_red_and_leaves_the_rest() {
        let mut px = vec![255u8; 64 * 64 * 4];
        super::paint_dot(&mut px, 64, 64);
        let at = |x: usize, y: usize| &px[(y * 64 + x) * 4..][..4];
        assert_eq!(at(50, 14), [0xdd, 0, 0, 255], "dot centre");
        assert_eq!(at(10, 50), [255; 4], "far corner untouched");
        assert_eq!(at(33, 14)[3], 0, "transparent gap around the dot");
        let _ = super::icon(true); // the bundled PNG decodes
    }
}
