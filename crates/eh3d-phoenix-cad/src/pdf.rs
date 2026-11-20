// SPDX-License-Identifier: GPL-3.0-or-later
//! A small PDF writer for the sheets of drawings: one page of vector lines, circles, arcs and text, written by hand with no dependency.
//!
//! The page is a [`Canvas`] in user units with the origin at the top left and `y` growing downwards (the way the SVG sheet is laid out), turned into
//! points (1/72 inch) by one factor. The text is set in Helvetica, one of the 14 fonts every reader has, with the encoding WinAnsi: a letter outside it is
//! written as a question mark (the cells of the title block in Japanese and Chinese have no glyph in that font). The file has a catalogue, one page tree,
//! one page, its content stream (not compressed, so it can be read in a text editor), the font and an information dictionary with the title; there is no
//! date and no random identifier, so the same sheet always gives the same bytes. The table of cross references holds the exact offset of every object.

use std::fmt::Write as _;

/// Points in one millimetre.
pub const POINTS_PER_MM: f64 = 72.0 / 25.4;

/// The widths of the characters 32 to 126 of Helvetica, in thousandths of the size of the text.
const WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// The byte of the encoding WinAnsi for a character, or a question mark when the encoding has none.
fn win_ansi(c: char) -> u8 {
    match c {
        ' '..='~' => c as u8,
        // the diameter sign has no place in the encoding: the capital O with a stroke is the nearest letter
        '\u{2300}' => 0xd8,
        '\u{2212}' => b'-',
        '\u{2013}' => 0x96,
        '\u{2014}' => 0x97,
        '\u{a0}'..='\u{ff}' => c as u8,
        _ => b'?',
    }
}

/// The width of a text in thousandths of its size.
fn width_milli(text: &str) -> u32 {
    text.chars()
        .map(|c| match win_ansi(c) {
            b @ 32..=126 => u32::from(WIDTHS[usize::from(b) - 32]),
            0xd8 => 778,
            _ => 556,
        })
        .sum()
}

/// A text as the string of a PDF content stream: between parentheses, the three characters that need it escaped and every byte past ASCII as an octal code.
fn pdf_string(text: &str) -> String {
    let mut out = String::from("(");
    for c in text.chars() {
        match win_ansi(c) {
            b @ (b'(' | b')' | b'\\') => {
                out.push('\\');
                out.push(b as char);
            }
            b @ 32..=126 => out.push(b as char),
            b => {
                let _ = write!(out, "\\{b:03o}");
            }
        }
    }
    out.push(')');
    out
}

/// A number as short text: up to three decimals, no trailing zeros and never `-0`.
fn num(v: f64) -> String {
    let t = format!("{v:.3}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" || t.is_empty() {
        "0".to_owned()
    } else {
        t.to_owned()
    }
}

/// Where a text sits on its point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// The text starts at the point.
    Start,
    /// The text is centred on the point.
    Middle,
}

/// The page being drawn.
#[derive(Debug, Clone)]
pub struct Canvas {
    width: f64,
    height: f64,
    factor: f64,
    ops: String,
}

impl Canvas {
    /// A page of `width` by `height` user units, `factor` points to each unit.
    pub fn new(width: f64, height: f64, factor: f64) -> Canvas {
        Canvas {
            width,
            height,
            factor,
            ops: String::new(),
        }
    }

    /// The x of a user point in points.
    fn px(&self, x: f64) -> f64 {
        x * self.factor
    }

    /// The y of a user point in points (the page grows upwards in PDF).
    fn py(&self, y: f64) -> f64 {
        (self.height - y) * self.factor
    }

    /// The colour of the lines, from `#rrggbb`; a text that is not that is black.
    pub fn stroke(&mut self, hex: &str) {
        let (r, g, b) = rgb(hex);
        let _ = writeln!(self.ops, "{} {} {} RG", num(r), num(g), num(b));
    }

    /// The colour of the text, from `#rrggbb`.
    pub fn fill(&mut self, hex: &str) {
        let (r, g, b) = rgb(hex);
        let _ = writeln!(self.ops, "{} {} {} rg", num(r), num(g), num(b));
    }

    /// The width of the lines, in user units.
    pub fn width(&mut self, w: f64) {
        let _ = writeln!(self.ops, "{} w", num(w * self.factor));
    }

    /// A dashed line: `on` drawn and `off` blank, in user units; `None` is a continuous line.
    pub fn dash(&mut self, pattern: Option<(f64, f64)>) {
        match pattern {
            Some((on, off)) => {
                let _ = writeln!(
                    self.ops,
                    "[{} {}] 0 d",
                    num(on * self.factor),
                    num(off * self.factor)
                );
            }
            None => self.ops.push_str("[] 0 d\n"),
        }
    }

    /// A straight line.
    pub fn line(&mut self, a: (f64, f64), b: (f64, f64)) {
        let _ = writeln!(
            self.ops,
            "{} {} m {} {} l S",
            num(self.px(a.0)),
            num(self.py(a.1)),
            num(self.px(b.0)),
            num(self.py(b.1))
        );
    }

    /// The Bézier curves of an arc round `c` (a point of the page, `y` down) from `a0` to `a1` degrees, the angles counted counter-clockwise as the drawing
    /// has them, starting with the move to the first point. At most a quarter of a turn to each curve.
    fn arc_path(&mut self, c: (f64, f64), r: f64, a0: f64, a1: f64) {
        let span = a1 - a0;
        let parts = ((span.abs() / 90.0).ceil() as usize).clamp(1, 16);
        let delta = (span / parts as f64).to_radians();
        let k = 4.0 / 3.0 * (delta / 4.0).tan();
        // a point and its tangent at an angle; the page grows downwards, so the sine changes its sign
        let at = |t: f64| {
            (
                (c.0 + r * t.cos(), c.1 - r * t.sin()),
                (-r * t.sin(), -r * t.cos()),
            )
        };
        let start = a0.to_radians();
        let (p0, _) = at(start);
        let _ = write!(self.ops, "{} {} m ", num(self.px(p0.0)), num(self.py(p0.1)));
        for i in 0..parts {
            let t0 = start + delta * i as f64;
            let t1 = t0 + delta;
            let ((x0, y0), (dx0, dy0)) = at(t0);
            let ((x1, y1), (dx1, dy1)) = at(t1);
            let _ = write!(
                self.ops,
                "{} {} {} {} {} {} c ",
                num(self.px(x0 + k * dx0)),
                num(self.py(y0 + k * dy0)),
                num(self.px(x1 - k * dx1)),
                num(self.py(y1 - k * dy1)),
                num(self.px(x1)),
                num(self.py(y1))
            );
        }
    }

    /// A circle.
    pub fn circle(&mut self, c: (f64, f64), r: f64) {
        self.arc_path(c, r, 0.0, 360.0);
        self.ops.push_str("h S\n");
    }

    /// An arc, counter-clockwise from `a0` to `a1` degrees.
    pub fn arc(&mut self, c: (f64, f64), r: f64, a0: f64, a1: f64) {
        self.arc_path(c, r, a0, a1);
        self.ops.push_str("S\n");
    }

    /// The width a text takes at a size, in user units.
    pub fn text_width(text: &str, size: f64) -> f64 {
        f64::from(width_milli(text)) * size / 1000.0
    }

    /// A text with its baseline at the point, `size` user units tall, turned `turn` degrees counter-clockwise on the page.
    pub fn text(&mut self, at: (f64, f64), size: f64, text: &str, anchor: Anchor, turn: f64) {
        let shift = match anchor {
            Anchor::Start => 0.0,
            Anchor::Middle => -Canvas::text_width(text, size) / 2.0,
        };
        let (s, c) = turn.to_radians().sin_cos();
        // the start of the text lies `shift` along the direction of the text
        let (x, y) = (at.0 + shift * c, at.1 - shift * s);
        let _ = writeln!(
            self.ops,
            "BT /F1 {} Tf {} {} {} {} {} {} Tm {} Tj ET",
            num(size * self.factor),
            num(c),
            num(s),
            num(-s),
            num(c),
            num(self.px(x)),
            num(self.py(y)),
            pdf_string(text)
        );
    }

    /// The file: the page with what was drawn on it and the title of the document (ASCII only, anything else becomes a question mark).
    pub fn finish(&self, title: &str) -> Vec<u8> {
        let content = self.ops.as_bytes();
        let mut title_text = String::from("(");
        for c in title.chars() {
            match c {
                '(' | ')' | '\\' => {
                    title_text.push('\\');
                    title_text.push(c);
                }
                ' '..='~' => title_text.push(c),
                _ => title_text.push('?'),
            }
        }
        title_text.push(')');
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
                num(self.px(self.width)),
                num(self.py(0.0))
            )
            .into_bytes(),
            {
                let mut o = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
                o.extend_from_slice(content);
                o.extend_from_slice(b"endstream");
                o
            },
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
                .to_vec(),
            format!("<< /Title {title_text} /Producer (EH3D-PHOENIX-CAD) >>").into_bytes(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for off in offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }
}

/// The three channels (0 to 1) of a colour written `#rrggbb`; black for anything else.
fn rgb(hex: &str) -> (f64, f64, f64) {
    let h = hex.strip_prefix('#').unwrap_or(hex);
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return (0.0, 0.0, 0.0);
    }
    let ch = |i: usize| f64::from(u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0)) / 255.0;
    (ch(0), ch(2), ch(4))
}

/// The page size in points and the number of objects of a file written by [`Canvas::finish`], read from the file itself, and whether the table of cross
/// references points at every object: what a test needs to say that the file is well formed. `None` when the file is not one of ours.
pub fn check(file: &[u8]) -> Option<(f64, f64, usize)> {
    let text = String::from_utf8(file.to_vec()).ok()?;
    if !text.starts_with("%PDF-1.4\n") || !text.ends_with("%%EOF\n") {
        return None;
    }
    let start = text.rfind("startxref\n")?;
    let xref_at: usize = text[start + 10..].lines().next()?.parse().ok()?;
    let table = text.get(xref_at..)?;
    let mut lines = table.lines();
    if lines.next()? != "xref" {
        return None;
    }
    let count: usize = lines.next()?.strip_prefix("0 ")?.parse().ok()?;
    if lines.next()? != "0000000000 65535 f " {
        return None;
    }
    for n in 1..count {
        let entry = lines.next()?;
        let off: usize = entry.get(..10)?.parse().ok()?;
        if !text.get(off..)?.starts_with(&format!("{n} 0 obj\n")) {
            return None;
        }
    }
    let media = text.find("/MediaBox [0 0 ")?;
    let mut sizes = text[media + 15..].split(']').next()?.split(' ');
    let w: f64 = sizes.next()?.parse().ok()?;
    let h: f64 = sizes.next()?.parse().ok()?;
    // the length of the content stream is the one it says
    let len_at = text.find("/Length ")?;
    let len: usize = text[len_at + 8..].split(' ').next()?.parse().ok()?;
    let body = text.find("stream\n")? + 7;
    if !text.get(body + len..)?.starts_with("endstream") {
        return None;
    }
    Some((w, h, count - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_with_every_kind_of_mark_is_a_well_formed_file() {
        let mut c = Canvas::new(210.0, 148.0, POINTS_PER_MM);
        c.stroke("#1f4fd8");
        c.width(0.3);
        c.dash(Some((6.0, 1.5)));
        c.line((10.0, 10.0), (100.0, 10.0));
        c.dash(None);
        c.circle((50.0, 50.0), 12.5);
        c.arc((100.0, 80.0), 20.0, 0.0, 180.0);
        c.fill("#111111");
        c.text((10.0, 140.0), 3.2, "1:2", Anchor::Start, 0.0);
        let file = c.finish("box");
        let (w, h, objects) = check(&file).expect("a well formed file");
        assert!(
            (w - 595.276).abs() < 0.01 && (h - 419.528).abs() < 0.01,
            "{w} {h}"
        );
        assert_eq!(objects, 6);
        let text = String::from_utf8_lossy(&file);
        assert!(text.contains("/BaseFont /Helvetica") && text.contains("/Title (box)"));
        assert!(text.contains("[17.008 4.252] 0 d") && text.contains("[] 0 d"));
        // the same page gives the same bytes
        assert_eq!(file, c.finish("box"));
    }

    #[test]
    fn the_text_is_written_in_win_ansi_with_its_marks_escaped() {
        assert_eq!(pdf_string("a (b) \\ c"), "(a \\(b\\) \\\\ c)");
        assert_eq!(
            pdf_string("\u{2300}8 \u{d7} 5\u{b0}"),
            "(\\3308 \\327 5\\260)"
        );
        // a letter the font has no glyph for is a question mark
        assert_eq!(pdf_string("\u{65e5}\u{672c}"), "(??)");
        assert_eq!(pdf_string("tab\tend"), "(tab?end)");
        for c in ['\u{0}', '\u{1f}', '\u{7f}', '\u{80}', '\u{9f}'] {
            assert_eq!(win_ansi(c), b'?', "{c:?}");
        }
    }

    #[test]
    fn a_text_is_centred_or_turned_by_its_width() {
        assert_eq!(width_milli("iii"), 666);
        assert_eq!(width_milli("WWW"), 2832);
        assert!((Canvas::text_width("10", 3.0) - 3.336).abs() < 1e-9);
        let mut c = Canvas::new(100.0, 100.0, 1.0);
        c.text((50.0, 50.0), 10.0, "10", Anchor::Middle, 0.0);
        c.text((50.0, 50.0), 10.0, "10", Anchor::Middle, 90.0);
        let ops: Vec<&str> = c.ops.lines().collect();
        // a horizontal text starts half its width to the left of the point
        assert!(ops[0].contains("1 0 0 1 44.44 50 Tm"), "{}", ops[0]);
        // a text turned a quarter turn starts half its width below the point on the page (44.44 up from the bottom of the file)
        assert!(ops[1].contains("0 1 -1 0 50 44.44 Tm"), "{}", ops[1]);
    }

    #[test]
    fn an_arc_of_a_full_turn_is_four_curves_and_a_quarter_one_curve() {
        let mut c = Canvas::new(100.0, 100.0, 1.0);
        c.circle((50.0, 50.0), 10.0);
        assert_eq!(c.ops.matches(" c ").count(), 4);
        assert!(c.ops.starts_with("60 50 m "), "{}", c.ops);
        let mut q = Canvas::new(100.0, 100.0, 1.0);
        q.arc((50.0, 50.0), 10.0, 0.0, 90.0);
        assert_eq!(q.ops.matches(" c ").count(), 1);
        // it ends on the top of the circle: x 50, y 60 of the file (the page is 100 tall)
        assert!(q.ops.contains("50 60 c"), "{}", q.ops);
        // a span of 100 degrees needs two curves
        let mut two = Canvas::new(100.0, 100.0, 1.0);
        two.arc((50.0, 50.0), 10.0, 10.0, 110.0);
        assert_eq!(two.ops.matches(" c ").count(), 2);
    }

    #[test]
    fn colours_are_read_from_hex_and_anything_else_is_black() {
        assert_eq!(rgb("#ff0000"), (1.0, 0.0, 0.0));
        assert_eq!(rgb("#000"), (0.0, 0.0, 0.0));
        assert_eq!(rgb("#gg0000"), (0.0, 0.0, 0.0));
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(2.0), "2");
    }

    #[test]
    fn the_checker_refuses_a_damaged_file() {
        let mut c = Canvas::new(100.0, 100.0, 1.0);
        c.line((0.0, 0.0), (1.0, 1.0));
        let good = c.finish("t");
        assert!(check(&good).is_some());
        assert!(check(b"").is_none());
        assert!(check(b"%PDF-1.4\n%%EOF\n").is_none());
        // an offset that points elsewhere
        let text =
            String::from_utf8_lossy(&good).replace("0000000009 00000 n", "0000000010 00000 n");
        assert!(check(text.as_bytes()).is_none());
        // a missing end
        assert!(check(&good[..good.len() - 7]).is_none());
        // a length that is not the one of the stream
        let longer = String::from_utf8_lossy(&good).replacen("/Length ", "/Length 1", 1);
        assert!(check(longer.as_bytes()).is_none());
    }

    /// A small generator with a fixed seed (xorshift), so that a failure can be repeated.
    struct Rng(u64);

    impl Rng {
        fn draw(&mut self, below: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % below
        }
    }

    #[test]
    fn pages_of_random_marks_and_texts_are_always_well_formed_files_with_a_fixed_seed() {
        let alphabet: Vec<char> =
            "ab Z09().\\/-:\u{2300}\u{d7}\u{b0}\u{e9}\u{fc}\u{65e5}\u{20ac}\t\u{1f600}"
                .chars()
                .collect();
        for seed in 1..=60u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let size = |rng: &mut Rng| 1.0 + rng.draw(400) as f64 / 2.0;
            let mut c = Canvas::new(
                size(&mut rng),
                size(&mut rng),
                0.01 + rng.draw(900) as f64 / 100.0,
            );
            let mut marks = 0;
            for _ in 0..rng.draw(40) + 1 {
                let p = (rng.draw(500) as f64 - 50.0, rng.draw(500) as f64 - 50.0);
                match rng.draw(6) {
                    0 => c.line(p, (p.0 + 7.0, p.1 - 3.0)),
                    1 => c.circle(p, 0.1 + rng.draw(80) as f64),
                    2 => c.arc(
                        p,
                        1.0 + rng.draw(80) as f64,
                        rng.draw(360) as f64,
                        360.0 + rng.draw(360) as f64,
                    ),
                    3 => c.dash(if rng.draw(2) == 0 {
                        None
                    } else {
                        Some((1.0 + rng.draw(9) as f64, 0.5))
                    }),
                    4 => c.stroke(["#112233", "#zzzzzz", "", "#FFFFFF"][rng.draw(4) as usize]),
                    _ => {
                        let text: String = (0..rng.draw(12))
                            .map(|_| alphabet[rng.draw(alphabet.len() as u64) as usize])
                            .collect();
                        let anchor = if rng.draw(2) == 0 {
                            Anchor::Start
                        } else {
                            Anchor::Middle
                        };
                        c.text(
                            p,
                            0.5 + rng.draw(20) as f64,
                            &text,
                            anchor,
                            rng.draw(360) as f64,
                        );
                        marks += 1;
                    }
                }
            }
            let title: String = (0..rng.draw(10))
                .map(|_| alphabet[rng.draw(alphabet.len() as u64) as usize])
                .collect();
            let file = c.finish(&title);
            // nothing past ASCII reaches the file, whatever the texts held, and it is well formed
            assert!(file.is_ascii(), "seed {seed}");
            let (_, _, objects) =
                check(&file).unwrap_or_else(|| panic!("seed {seed}: not well formed"));
            assert_eq!(objects, 6, "seed {seed}");
            // one text operator for each text, and the same page gives the same bytes
            assert_eq!(
                String::from_utf8_lossy(&file).matches(" Tj ET").count(),
                marks,
                "seed {seed}"
            );
            assert_eq!(file, c.finish(&title), "seed {seed}");
        }
    }
}
