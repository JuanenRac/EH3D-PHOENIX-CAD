// SPDX-License-Identifier: GPL-3.0-or-later
//! The drawing primitives of a sheet: lines, circles, arcs and texts on layers, in micrometres
//! with the origin at the lower left corner of the sheet and `y` up. The SVG, PDF and DXF writers
//! and the preview of the window all read the same list.

/// Where a text sits on its point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// The text starts at the point.
    Start,
    /// The text is centred on the point.
    Middle,
}

/// A shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// A straight line.
    Line {
        /// One end.
        a: (i64, i64),
        /// The other end.
        b: (i64, i64),
    },
    /// A circle.
    Circle {
        /// The centre.
        c: (i64, i64),
        /// The radius.
        r: i64,
    },
    /// An arc, counter-clockwise from `a0` to `a1` degrees.
    Arc {
        /// The centre.
        c: (i64, i64),
        /// The radius.
        r: i64,
        /// The start angle in degrees.
        a0: f64,
        /// The end angle in degrees.
        a1: f64,
    },
    /// A text with its baseline at the point, `h` tall, turned `turn` degrees counter-clockwise.
    Text {
        /// The point.
        at: (i64, i64),
        /// The height.
        h: i64,
        /// The text.
        text: String,
        /// Where the text sits on the point.
        anchor: Anchor,
        /// The turn in degrees.
        turn: f64,
    },
}

/// A shape on a layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Prim {
    /// The name of the layer.
    pub layer: String,
    /// The shape.
    pub shape: Shape,
}

impl Prim {
    /// A line on a layer.
    pub fn line(layer: &str, a: (i64, i64), b: (i64, i64)) -> Prim {
        Prim {
            layer: layer.to_string(),
            shape: Shape::Line { a, b },
        }
    }

    /// A text on a layer, not turned.
    pub fn text(layer: &str, at: (i64, i64), h: i64, text: &str, anchor: Anchor) -> Prim {
        Prim::turned(layer, at, h, text, anchor, 0.0)
    }

    /// A text on a layer, turned.
    pub fn turned(
        layer: &str,
        at: (i64, i64),
        h: i64,
        text: &str,
        anchor: Anchor,
        turn: f64,
    ) -> Prim {
        Prim {
            layer: layer.to_string(),
            shape: Shape::Text {
                at,
                h,
                text: text.to_string(),
                anchor,
                turn,
            },
        }
    }
}

/// A point rounded to the micrometre.
pub fn pt(x: f64, y: f64) -> (i64, i64) {
    (x.round() as i64, y.round() as i64)
}

/// The two lines of an arrow whose tip is at `tip` and whose body goes the way of `dir` (a
/// direction, any length), `len` long and `width` across at its open end.
pub fn arrow(layer: &str, tip: (i64, i64), dir: (f64, f64), len: i64, width: i64) -> Vec<Prim> {
    let norm = (dir.0 * dir.0 + dir.1 * dir.1).sqrt();
    if norm == 0.0 {
        return Vec::new();
    }
    let (dx, dy) = (dir.0 / norm, dir.1 / norm);
    let (bx, by) = (
        tip.0 as f64 + dx * len as f64,
        tip.1 as f64 + dy * len as f64,
    );
    let (px, py) = (-dy * width as f64 / 2.0, dx * width as f64 / 2.0);
    vec![
        Prim::line(layer, tip, pt(bx + px, by + py)),
        Prim::line(layer, tip, pt(bx - px, by - py)),
    ]
}

/// A length in micrometres as text in millimetres with three decimals and no trailing zeros
/// beyond what is needed (`12.5`, `0.35`, `-3`).
pub fn mm_text(um: i64) -> String {
    let sign = if um < 0 { "-" } else { "" };
    let a = um.unsigned_abs();
    let (whole, frac) = (a / 1000, a % 1000);
    if frac == 0 {
        format!("{sign}{whole}")
    } else {
        let f = format!("{frac:03}");
        format!("{sign}{whole}.{}", f.trim_end_matches('0'))
    }
}

/// A length in nanometres as text in millimetres with `decimals` decimals (0 to 6), rounded half
/// away from zero.
pub fn nm_text(nm: i64, decimals: u32) -> String {
    let unit = 10i128.pow(6 - decimals.min(6));
    let a = i128::from(nm).abs();
    let r = (a + unit / 2) / unit;
    let sign = if nm < 0 && r != 0 { "-" } else { "" };
    if decimals == 0 {
        return format!("{sign}{r}");
    }
    let p = 10i128.pow(decimals);
    format!(
        "{sign}{}.{:0width$}",
        r / p,
        r % p,
        width = decimals as usize
    )
}

/// An angle in thousandths of a degree as text with `decimals` decimals (0 to 3).
pub fn angle_text(milli_deg: i64, decimals: u32) -> String {
    let d = decimals.min(3);
    let unit = 10i64.pow(3 - d);
    let r = (milli_deg.abs() + unit / 2) / unit;
    if d == 0 {
        return format!("{r}");
    }
    let p = 10i64.pow(d);
    format!("{}.{:0width$}", r / p, r % p, width = d as usize)
}
