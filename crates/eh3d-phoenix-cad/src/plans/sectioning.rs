// SPDX-License-Identifier: GPL-3.0-or-later
//! Section views and detail views of the solids of aligned prisms.
//!
//! A **section** cuts the solid by a plane across the axis the view looks along, keeps the half
//! that lies away from the viewer, and shows it from the side of the cut: the edges of the half
//! are the edges of the view, and the faces the plane leaves open (the cut faces) are hatched.
//! The line of the cut is drawn in another view with a thick mark at each end, an arrow that shows
//! where it is looked at from, and its letters. A **detail** is the part of another view inside a
//! circle or a rectangle, drawn larger; the circle or the rectangle is marked in the view it was
//! taken from. Everything here is integer nanometres or micrometres except the hatching and the
//! clipping against a circle, which round to the unit of the output.

use std::collections::BTreeMap;

use super::measure::view_axes;
use super::model::{Axis, DetailShape, ViewKind};
use super::prim::{arrow, pt, Anchor, Prim};
use super::settings::{HatchStyle, SectionStyle};
use crate::prism::{Limits, Prism, Solid};
use crate::{CadError, Length};

/// A rectangle `(low corner, high corner)`.
pub type Rect = ((i64, i64), (i64, i64));

/// Whether the half that stays has the coordinates above the cut (the front view looks from the
/// lowest `y`, so it keeps `y` above the cut) or below it (the top looks from above and the right
/// from the highest `x`).
pub fn keeps_high(kind: ViewKind) -> bool {
    kind == ViewKind::Front
}

fn axis_of(kind: ViewKind) -> Option<usize> {
    kind.depth_axis().map(Axis::index)
}

/// The two axes of a picture of a kind, across and up.
fn picture_axes(kind: ViewKind) -> Option<(usize, usize)> {
    view_axes(kind).map(|(u, v)| (u.index(), v.index()))
}

/// The half of a solid that a section keeps. A prism that the plane cuts is cut; one that lies
/// wholly on the other side is left out.
pub fn clip_solid(
    solid: &Solid,
    kind: ViewKind,
    at_nm: i64,
    limits: &Limits,
) -> Result<Solid, CadError> {
    let a = axis_of(kind).ok_or_else(|| {
        CadError::PlanSection("a section is made for the front, top or right view".into())
    })?;
    let high = keeps_high(kind);
    let mut kept = Vec::new();
    for p in solid.prisms() {
        let (mut lo, mut hi) = (p.min(), p.max());
        if high {
            lo[a] = lo[a].max(at_nm);
        } else {
            hi[a] = hi[a].min(at_nm);
        }
        if lo[a] >= hi[a] {
            continue;
        }
        let corner = [
            Length::new(lo[0])?,
            Length::new(lo[1])?,
            Length::new(lo[2])?,
        ];
        let size = [
            Length::new(hi[0] - lo[0])?,
            Length::new(hi[1] - lo[1])?,
            Length::new(hi[2] - lo[2])?,
        ];
        kept.push(Prism::new(corner, size)?);
    }
    Solid::from_prisms(&kept, limits)
}

/// The faces that the plane leaves open: the rectangles, in the picture of the view, of the
/// prisms of the kept half that reach the plane.
pub fn cut_face(kept: &Solid, kind: ViewKind, at_nm: i64) -> Vec<Rect> {
    let (Some(a), Some((u, v))) = (axis_of(kind), picture_axes(kind)) else {
        return Vec::new();
    };
    let high = keeps_high(kind);
    let mut out: Vec<Rect> = kept
        .prisms()
        .iter()
        .filter(|p| {
            let p: &Prism = p;
            if high {
                p.min()[a] == at_nm
            } else {
                p.max()[a] == at_nm
            }
        })
        .map(|p| ((p.min()[u], p.min()[v]), (p.max()[u], p.max()[v])))
        .collect();
    out.sort_unstable();
    out
}

/// The unit vector of an angle in degrees, exact for the right angles.
fn direction(angle_deg: u32) -> (f64, f64) {
    match angle_deg % 180 {
        0 => (1.0, 0.0),
        90 => (0.0, 1.0),
        a => {
            let r = f64::from(a).to_radians();
            (r.cos(), r.sin())
        }
    }
}

/// The hatching of rectangles of the sheet (micrometres): parallel lines at an angle from the
/// first axis, `spacing` apart, drawn as they cross the rectangles; the pieces of one line that
/// join (rectangles side by side) are one line. Lines are numbered from the origin of the sheet
/// so the hatching of two views of the same part is the same. The error is the number of lines
/// the hatching would need when it is above `max_lines`.
pub fn hatch_lines(
    rects: &[Rect],
    style: HatchStyle,
    max_lines: usize,
) -> Result<Vec<(Prim2, Prim2)>, usize> {
    let (dx, dy) = direction(style.angle_deg);
    let (nx, ny) = (-dy, dx);
    let s = style.spacing_um as f64;
    let mut by_k: BTreeMap<i64, Vec<(f64, f64)>> = BTreeMap::new();
    let mut count = 0usize;
    for ((x0, y0), (x1, y1)) in rects {
        let (x0, y0, x1, y1) = (*x0 as f64, *y0 as f64, *x1 as f64, *y1 as f64);
        let proj = [
            x0 * nx + y0 * ny,
            x1 * nx + y0 * ny,
            x0 * nx + y1 * ny,
            x1 * nx + y1 * ny,
        ];
        let pmin = proj.iter().copied().fold(f64::INFINITY, f64::min);
        let pmax = proj.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let k0 = (pmin / s).ceil() as i64;
        let k1 = (pmax / s).floor() as i64;
        for k in k0..=k1 {
            count += 1;
            if count > max_lines.saturating_mul(4) {
                return Err(count);
            }
            let c = k as f64 * s;
            let (bx, by) = (c * nx, c * ny);
            let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
            let mut inside = true;
            for (base, d, lo, hi) in [(bx, dx, x0, x1), (by, dy, y0, y1)] {
                if d.abs() < 1e-12 {
                    if base < lo || base > hi {
                        inside = false;
                    }
                } else {
                    let (a, b) = ((lo - base) / d, (hi - base) / d);
                    t0 = t0.max(a.min(b));
                    t1 = t1.min(a.max(b));
                }
            }
            if inside && t1 - t0 > 0.001 {
                by_k.entry(k).or_default().push((t0, t1));
            }
        }
    }
    let mut out = Vec::new();
    for (k, mut spans) in by_k {
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (a, b) in spans {
            match merged.last_mut() {
                Some(last) if a <= last.1 + 0.001 => last.1 = last.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        let c = k as f64 * s;
        for (a, b) in merged {
            out.push((
                pt(c * nx + a * dx, c * ny + a * dy),
                pt(c * nx + b * dx, c * ny + b * dy),
            ));
            if out.len() > max_lines {
                return Err(out.len());
            }
        }
    }
    Ok(out)
}

/// A point of the sheet in micrometres.
pub type Prim2 = (i64, i64);

/// The marks of the line of a cut in the view it is drawn in: the chain line over the whole view
/// and a little past it, a thick mark and a stem with an arrow at each end, and the letters. The
/// `frame` is the placement of that view; `None` is returned when the view does not show the
/// axis of the cut (a view along the axis, an isometric or a sketch view).
#[allow(clippy::too_many_arguments)]
pub fn cut_marks(
    parent: &super::measure::Frame,
    cut_kind: ViewKind,
    at_nm: i64,
    label: &str,
    style: &SectionStyle,
    arrow_len: i64,
    arrow_width: i64,
    layers: &CutLayers,
) -> Option<Vec<Prim>> {
    let a = axis_of(cut_kind)?;
    let (u, v) = view_axes(parent.kind)?;
    let high = keeps_high(cut_kind);
    let sign: i64 = if high { 1 } else { -1 };
    let size = parent.size();
    let (x0, y0) = parent.origin;
    let (x1, y1) = (x0 + size.0, y0 + size.1);
    let over = style.overshoot_um;
    // the line is across the axis of the cut: vertical when that axis runs across the picture
    let (p0, p1, dir): ((i64, i64), (i64, i64), (i64, i64)) = if a == u.index() {
        let x = parent.map((at_nm, 0)).0;
        ((x, y0 - over), (x, y1 + over), (sign, 0))
    } else if a == v.index() {
        let y = parent.map((0, at_nm)).1;
        ((x0 - over, y), (x1 + over, y), (0, sign))
    } else {
        return None;
    };
    let mut out = vec![Prim::line(&layers.line, p0, p1)];
    let along = ((p1.0 - p0.0).signum(), (p1.1 - p0.1).signum());
    let h = style.text_height_um;
    for (end, inward) in [(p0, along), (p1, (-along.0, -along.1))] {
        let mark = (
            end.0 + inward.0 * style.end_length_um,
            end.1 + inward.1 * style.end_length_um,
        );
        out.push(Prim::line(&layers.mark, end, mark));
        let tip = (
            end.0 + dir.0 * style.arrow_length_um,
            end.1 + dir.1 * style.arrow_length_um,
        );
        out.push(Prim::line(&layers.mark, end, tip));
        out.extend(arrow(
            &layers.mark,
            tip,
            (-(dir.0 as f64), -(dir.1 as f64)),
            arrow_len,
            arrow_width,
        ));
        let centre = (tip.0 + dir.0 * h, tip.1 + dir.1 * h);
        out.push(Prim::text(
            &layers.text,
            (centre.0, centre.1 - h / 3),
            h,
            label,
            Anchor::Middle,
        ));
    }
    Some(out)
}

/// The names of the layers that the marks of a cut are drawn on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutLayers {
    /// The chain line.
    pub line: String,
    /// The thick marks and the stems with the arrows.
    pub mark: String,
    /// The letters.
    pub text: String,
}

/// The part of a segment inside a circle (centre and radius in the same unit as the segment).
pub fn clip_to_circle(
    a: (i64, i64),
    b: (i64, i64),
    c: (i64, i64),
    r: i64,
) -> Option<((i64, i64), (i64, i64))> {
    let (ax, ay) = ((a.0 - c.0) as f64, (a.1 - c.1) as f64);
    let (dx, dy) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
    let qa = dx * dx + dy * dy;
    if qa == 0.0 {
        return None;
    }
    let qb = 2.0 * (ax * dx + ay * dy);
    let qc = ax * ax + ay * ay - (r as f64) * (r as f64);
    let disc = qb * qb - 4.0 * qa * qc;
    if disc <= 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let t0 = ((-qb - root) / (2.0 * qa)).max(0.0);
    let t1 = ((-qb + root) / (2.0 * qa)).min(1.0);
    if t1 <= t0 {
        return None;
    }
    let at = |t: f64| -> (i64, i64) {
        if t <= 0.0 {
            a
        } else if t >= 1.0 {
            b
        } else {
            (
                (a.0 as f64 + t * dx).round() as i64,
                (a.1 as f64 + t * dy).round() as i64,
            )
        }
    };
    let (p, q) = (at(t0), at(t1));
    (p != q).then_some((p, q))
}

/// The part of a segment inside a rectangle.
pub fn clip_to_rect(a: (i64, i64), b: (i64, i64), rect: Rect) -> Option<((i64, i64), (i64, i64))> {
    let ((x0, y0), (x1, y1)) = rect;
    let (dx, dy) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-dx, (a.0 - x0) as f64),
        (dx, (x1 - a.0) as f64),
        (-dy, (a.1 - y0) as f64),
        (dy, (y1 - a.1) as f64),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    if t1 <= t0 {
        return None;
    }
    let at = |t: f64| -> (i64, i64) {
        if t <= 0.0 {
            a
        } else if t >= 1.0 {
            b
        } else {
            (
                (a.0 as f64 + t * dx).round() as i64,
                (a.1 as f64 + t * dy).round() as i64,
            )
        }
    };
    let (p, q) = (at(t0), at(t1));
    (p != q).then_some((p, q))
}

/// The box of the part of a view that a detail takes, and its clipping of a segment.
pub fn detail_box(centre: (i64, i64), shape: DetailShape) -> Rect {
    match shape {
        DetailShape::Circle { radius_nm } => (
            (centre.0 - radius_nm, centre.1 - radius_nm),
            (centre.0 + radius_nm, centre.1 + radius_nm),
        ),
        DetailShape::Rect {
            width_nm,
            height_nm,
        } => (
            (centre.0 - width_nm / 2, centre.1 - height_nm / 2),
            (
                centre.0 - width_nm / 2 + width_nm,
                centre.1 - height_nm / 2 + height_nm,
            ),
        ),
    }
}

/// A segment clipped to a detail shape.
pub fn clip_detail(
    a: (i64, i64),
    b: (i64, i64),
    centre: (i64, i64),
    shape: DetailShape,
) -> Option<((i64, i64), (i64, i64))> {
    match shape {
        DetailShape::Circle { radius_nm } => clip_to_circle(a, b, centre, radius_nm),
        DetailShape::Rect { .. } => clip_to_rect(a, b, detail_box(centre, shape)),
    }
}
