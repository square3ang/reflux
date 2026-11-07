//! `buildRecoveryKitPdf` (`auth/utils/RecoveryKitSheet.ts`): the recovery kit as an A4 PDF with
//! the same layout, written by hand with the standard Helvetica / Courier fonts and the QR code as
//! vector squares.
// shortcut: text outside Windows-1252 (CJK, Cyrillic…) is written in English instead of being
// rasterised like Fluxer does; upgrade with an embedded font if translated PDFs are needed.

/// What the sheet shows (`RecoveryKitSheet`).
#[derive(Clone)]
pub struct Sheet {
    pub product: String,
    pub title: String,
    pub intro: String,
    pub details: Vec<(String, String)>,
    pub key_label: String,
    pub key: String,
    pub recover_url: String,
    pub qr_caption: String,
    pub steps_title: String,
    pub steps: Vec<String>,
    /// (title, note, codes).
    pub backup: Option<(String, String, Vec<String>)>,
}

const PAGE_W: f32 = 595.28;
const PAGE_H: f32 = 841.89;
const MARGIN: f32 = 56.0;
const CONTENT_W: f32 = PAGE_W - MARGIN * 2.0;
const QR: f32 = 112.0;
const QR_GAP: f32 = 24.0;
const KEY_SIZE: f32 = 17.0;
const KEY_BOX: f32 = 44.0;
const LH: f32 = 1.35;

#[derive(Clone, Copy, PartialEq)]
enum Font {
    Regular,
    Bold,
    Mono,
}

/// Helvetica and Helvetica-Bold advance widths for ASCII 32..=126 (AFM, per 1000).
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELV_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// The Windows-1252 byte for `c`, if the standard fonts can draw it.
fn win(c: char) -> Option<u8> {
    match c {
        ' '..='~' | '\u{a0}'..='\u{ff}' => Some(c as u8),
        '\u{2019}' => Some(0x92),
        '\u{2018}' => Some(0x91),
        '\u{201c}' => Some(0x93),
        '\u{201d}' => Some(0x94),
        '\u{2013}' => Some(0x96),
        '\u{2014}' => Some(0x97),
        '\u{2026}' => Some(0x85),
        '\u{2022}' => Some(0x95),
        _ => None,
    }
}

/// `canEncode`.
pub fn can_encode(s: &str) -> bool {
    s.chars().all(|c| win(c).is_some())
}

fn width(s: &str, f: Font, size: f32) -> f32 {
    let units: u32 = s
        .chars()
        .map(|c| match f {
            Font::Mono => 600,
            _ => {
                let table = if f == Font::Bold { &HELV_BOLD } else { &HELV };
                if (' '..='~').contains(&c) { u32::from(table[c as usize - 32]) } else { 556 }
            }
        })
        .sum();
    units as f32 * size / 1000.0
}

/// `wrapText`: words onto lines no wider than `max`; an over-long word breaks by character.
fn wrap(s: &str, max: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
    let (mut lines, mut cur) = (Vec::new(), String::new());
    for word in s.split_whitespace() {
        let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if measure(&cand) <= max {
            cur = cand;
            continue;
        }
        if !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        for c in word.chars() {
            let next = format!("{cur}{c}");
            if !cur.is_empty() && measure(&next) > max {
                lines.push(std::mem::replace(&mut cur, c.to_string()));
            } else {
                cur = next;
            }
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

fn literal(s: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for b in s.chars().filter_map(win) {
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

struct Writer {
    pages: Vec<Vec<u8>>,
    y: f32,
}

impl Writer {
    fn page(&mut self) -> &mut Vec<u8> {
        self.pages.last_mut().expect("a page")
    }

    fn ensure(&mut self, h: f32) {
        if self.y - h < MARGIN {
            self.pages.push(Vec::new());
            self.y = PAGE_H - MARGIN;
        }
    }

    fn draw(&mut self, s: &str, x: f32, y: f32, size: f32, f: Font, rgb: [f32; 3]) {
        let name = match f {
            Font::Regular => "F1",
            Font::Bold => "F2",
            Font::Mono => "F3",
        };
        let mut op = format!("BT {} {} {} rg /{name} {size} Tf {x:.2} {y:.2} Td ", rgb[0], rgb[1], rgb[2]).into_bytes();
        op.extend(literal(s));
        op.extend(b" Tj ET\n");
        self.page().extend(op);
    }

    /// `SheetWriter.text`.
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, s: &str, size: f32, f: Font, rgb: [f32; 3], x: f32, w: f32, after: f32) {
        for line in wrap(s, w, |v| width(v, f, size)) {
            self.ensure(size * LH);
            self.draw(&line, x, self.y - size, size, f, rgb);
            self.y -= size * LH;
        }
        self.y -= after;
    }
}

const TEXT: [f32; 3] = [0.07, 0.09, 0.15];
const MUTED: [f32; 3] = [0.36, 0.39, 0.45];

/// The PDF bytes.
pub fn build(s: &Sheet) -> Vec<u8> {
    let mut w = Writer { pages: vec![Vec::new()], y: PAGE_H - MARGIN };
    w.text(&s.title, 22.0, Font::Bold, TEXT, MARGIN, CONTENT_W, 8.0);
    w.text(&s.intro, 11.0, Font::Regular, MUTED, MARGIN, CONTENT_W, 20.0);

    // The QR code (error correction M, no margin) on the right, its caption under it.
    let top = w.y;
    let qx = PAGE_W - MARGIN - QR;
    if let Ok(code) = qrcode::QrCode::with_error_correction_level(s.recover_url.as_bytes(), qrcode::EcLevel::M) {
        let n = code.width();
        let m = QR / n as f32;
        let mut ops = String::from("0 0 0 rg\n");
        for (i, c) in code.to_colors().into_iter().enumerate() {
            if c == qrcode::Color::Dark {
                let (cx, cy) = ((i % n) as f32, (i / n) as f32);
                ops.push_str(&format!("{:.3} {:.3} {m:.3} {m:.3} re\n", qx + cx * m, top - (cy + 1.0) * m));
            }
        }
        ops.push_str("f\n");
        w.page().extend(ops.into_bytes());
    }
    w.y = top - QR - 6.0;
    w.text(&s.qr_caption, 8.0, Font::Regular, MUTED, qx, QR, 0.0);
    let qr_bottom = w.y;
    w.y = top;
    let details_w = CONTENT_W - QR - QR_GAP;
    for (label, value) in &s.details {
        w.text(label, 9.0, Font::Bold, MUTED, MARGIN, details_w, 2.0);
        w.text(value, 12.0, Font::Regular, TEXT, MARGIN, details_w, 10.0);
    }
    w.y = w.y.min(qr_bottom) - 14.0;

    w.text(&s.key_label, 11.0, Font::Bold, TEXT, MARGIN, CONTENT_W, 6.0);
    w.ensure(KEY_BOX);
    let y = w.y;
    w.page().extend(format!("0.95 0.96 0.97 rg 0.82 0.84 0.87 RG 1 w {MARGIN} {:.2} {CONTENT_W:.2} {KEY_BOX} re B\n", y - KEY_BOX).into_bytes());
    let kw = width(&s.key, Font::Mono, KEY_SIZE);
    w.draw(&s.key, MARGIN + (CONTENT_W - kw) / 2.0, y - KEY_BOX / 2.0 - KEY_SIZE * 0.3, KEY_SIZE, Font::Mono, TEXT);
    w.y -= KEY_BOX + 24.0;

    w.text(&s.steps_title, 13.0, Font::Bold, TEXT, MARGIN, CONTENT_W, 6.0);
    for (i, step) in s.steps.iter().enumerate() {
        w.ensure(11.0 * LH);
        w.draw(&format!("{}.", i + 1), MARGIN, w.y - 11.0, 11.0, Font::Bold, TEXT);
        w.text(step, 11.0, Font::Regular, TEXT, MARGIN + 18.0, CONTENT_W - 18.0, 4.0);
    }
    if let Some((title, note, codes)) = s.backup.as_ref().filter(|b| !b.2.is_empty()) {
        w.y -= 16.0;
        w.text(title, 13.0, Font::Bold, TEXT, MARGIN, CONTENT_W, 4.0);
        w.text(note, 10.0, Font::Regular, MUTED, MARGIN, CONTENT_W, 8.0);
        for pair in codes.chunks(2) {
            w.ensure(18.0);
            for (col, code) in pair.iter().enumerate() {
                w.draw(code, MARGIN + col as f32 * CONTENT_W / 2.0, w.y - 12.0, 12.0, Font::Mono, TEXT);
            }
            w.y -= 18.0;
        }
    }
    assemble(&w.pages, &s.title, &s.product)
}

/// The document around the page content streams.
fn assemble(pages: &[Vec<u8>], title: &str, product: &str) -> Vec<u8> {
    let n = pages.len();
    // 1 catalog, 2 pages, 3-5 fonts, 6 info, then a page and its stream per page.
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 7 + 2 * i)).collect();
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier-Bold /Encoding /WinAnsiEncoding >>".to_vec(),
        [b"<< /Title ".to_vec(), literal(title), b" /Creator ".to_vec(), literal(product), b" /Producer ".to_vec(), literal(product), b" >>".to_vec()].concat(),
    ];
    for (i, content) in pages.iter().enumerate() {
        objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> >> /Contents {} 0 R >>", 8 + 2 * i).into_bytes());
        objs.push([format!("<< /Length {} >>\nstream\n", content.len()).into_bytes(), content.clone(), b"\nendstream".to_vec()].concat());
    }
    let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).into_bytes());
        out.extend(o);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).into_bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").into_bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).into_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_and_builds_a_wellformed_pdf() {
        assert_eq!(wrap("aaa bbb ccc", width("aaa bbb", Font::Regular, 10.0), |v| width(v, Font::Regular, 10.0)), ["aaa bbb", "ccc"]);
        assert_eq!(wrap("abcdef", width("abc", Font::Mono, 10.0), |v| width(v, Font::Mono, 10.0)), ["abc", "def"]);
        assert!(can_encode("Don’t — café") && !can_encode("복구 키"));
        let s = Sheet {
            product: "Fluxer".into(),
            title: "Fluxer recovery kit".into(),
            intro: "Keep (this) safe.".into(),
            details: vec![("Instance".into(), "https://web.fluxer.app".into())],
            key_label: "Recovery key".into(),
            key: "ABCD-EFGH-IJKL-MNOP".into(),
            recover_url: "https://web.fluxer.app/recover#key=x".into(),
            qr_caption: "Scan".into(),
            steps_title: "How".into(),
            steps: vec!["One".into()],
            backup: Some(("Codes".into(), "Once".into(), vec!["a".into(); 60])),
        };
        let pdf = build(&s);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4") && text.ends_with("%%EOF\n"));
        assert!(text.contains("(Keep \\(this\\) safe.) Tj"));
        // The xref offset points at the table.
        let at: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert_eq!(&pdf[at..at + 4], b"xref");
        assert!(text.contains("/Count 2"), "60 codes overflow onto a second page");
    }
}
