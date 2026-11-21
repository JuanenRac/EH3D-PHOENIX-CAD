// SPDX-License-Identifier: GPL-3.0-or-later
//! The broken view: a long part drawn without the middle of its length. The range that is taken
//! out is given in the coordinates of the picture of the view (nanometres of the model, along the
//! axis across the picture or up it); what is left of the part on both sides is drawn closer, with
//! a gap of `sheet.break.gap` between them, and a break mark (a zigzag) at each side of the gap.
//! The dimensions are worked out from the model, so a dimension across the break still says the
//! real length; only the drawing of the part is shorter.

use super::geom::Seg;
use super::prim::Prim;
use super::settings::BreakStyle;

/// How the picture of a broken view is squeezed: the range `from` to `to` along one axis of the
/// picture is replaced by a gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Warp {
    /// Whether the range is along the axis across the picture (the first coordinate) or up it.
    pub along_u: bool,
    /// Where the range starts, in nanometres.
    pub from: i64,
    /// Where it ends, in nanometres.
    pub to: i64,
    /// The length that is left between the two sides, in nanometres.
    pub gap: i64,
}

impl Warp {
    /// A coordinate of the picture after the range is taken out: what lies before the range stays,
    /// what lies after it moves towards it, and what lies inside it goes onto the first mark.
    pub fn coord(&self, c: i64) -> i64 {
        if c <= self.from {
            c
        } else if c >= self.to {
            c - (self.to - self.from) + self.gap
        } else {
            self.from
        }
    }

    /// A point of the picture after the range is taken out.
    pub fn point(&self, p: (i64, i64)) -> (i64, i64) {
        if self.along_u {
            (self.coord(p.0), p.1)
        } else {
            (p.0, self.coord(p.1))
        }
    }

    /// The length of the part that is not drawn, in nanometres.
    pub fn removed(&self) -> i64 {
        self.to - self.from - self.gap
    }

    fn along(&self, p: (i64, i64)) -> i64 {
        if self.along_u {
            p.0
        } else {
            p.1
        }
    }
}

/// The point of a segment where the coordinate along the axis of the break is `c`.
fn at(w: &Warp, a: (i64, i64), b: (i64, i64), c: i64) -> (i64, i64) {
    let (ca, cb) = (w.along(a), w.along(b));
    if ca == cb {
        return a;
    }
    let num = i128::from(c - ca);
    let den = i128::from(cb - ca);
    let lerp = |x: i64, y: i64| -> i64 {
        (i128::from(x) + i128::from(y - x) * num / den)
            .clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
    };
    (lerp(a.0, b.0), lerp(a.1, b.1))
}

/// The edges of the picture without the range that is taken out: an edge that lies inside the
/// range is dropped, one that crosses the start or the end is cut there, and one that crosses the
/// whole range is two edges. An edge across the axis of the break that lies exactly at the start
/// or at the end stays.
pub fn clip(segs: &[Seg], w: &Warp) -> Vec<Seg> {
    let mut out = Vec::with_capacity(segs.len());
    for g in segs {
        let (ca, cb) = (w.along(g.a), w.along(g.b));
        let (lo, hi) = (ca.min(cb), ca.max(cb));
        if hi <= w.from || lo >= w.to {
            out.push(*g);
            continue;
        }
        let (p, q) = if ca <= cb { (g.a, g.b) } else { (g.b, g.a) };
        if lo < w.from {
            out.push(Seg {
                a: p,
                b: at(w, p, q, w.from),
                hidden: g.hidden,
            });
        }
        if hi > w.to {
            out.push(Seg {
                a: at(w, p, q, w.to),
                b: q,
                hidden: g.hidden,
            });
        }
    }
    out
}

/// The box of the edges in the picture after the range is taken out.
pub fn warped_bounds(segs: &[Seg], w: &Warp) -> Option<((i64, i64), (i64, i64))> {
    let moved: Vec<Seg> = segs
        .iter()
        .map(|g| Seg {
            a: w.point(g.a),
            b: w.point(g.b),
            hidden: g.hidden,
        })
        .collect();
    super::geom::bounds_of(&moved)
}

/// One break mark: a zigzag across the part at `pos` on the sheet, from `a` to `b` along the
/// other axis, in micrometres.
pub fn zigzag(
    layer: &str,
    along_u: bool,
    pos: i64,
    a: i64,
    b: i64,
    style: &BreakStyle,
) -> Vec<Prim> {
    let n = i64::from(style.teeth) * 2;
    let point = |k: i64| -> (i64, i64) {
        let across = a + (b - a) * k / n;
        let off = if k == 0 || k == n {
            0
        } else if k % 2 == 1 {
            style.amplitude_um
        } else {
            -style.amplitude_um
        };
        if along_u {
            (pos + off, across)
        } else {
            (across, pos + off)
        }
    };
    (0..n)
        .map(|k| Prim::line(layer, point(k), point(k + 1)))
        .collect()
}

/// The two break marks of a view: `lo` and `hi` are the corners of the box of its picture on the
/// sheet in micrometres, and `first` and `second` where the two sides of the gap are on the sheet
/// along the axis of the break.
pub fn marks(
    layer: &str,
    w: &Warp,
    first: i64,
    second: i64,
    lo: (i64, i64),
    hi: (i64, i64),
    style: &BreakStyle,
) -> Vec<Prim> {
    let (a, b) = if w.along_u {
        (lo.1 - style.overshoot_um, hi.1 + style.overshoot_um)
    } else {
        (lo.0 - style.overshoot_um, hi.0 + style.overshoot_um)
    };
    let mut out = zigzag(layer, w.along_u, first, a, b, style);
    out.extend(zigzag(layer, w.along_u, second, a, b, style));
    out
}
