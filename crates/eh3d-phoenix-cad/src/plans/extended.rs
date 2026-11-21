// SPDX-License-Identifier: GPL-3.0-or-later
//! Chain and ordinate dimensions, the placing of dimension lines so that they do not lie on one
//! another, and the logic of dragging a dimension line. Nothing here has a window in it: the
//! panel hands over a point of the sheet and gets back a distance.
//!
//! * a **chain** is the distance from each face to the next one, on one line;
//! * an **ordinate** dimension is the coordinate of each face from a common origin (the first
//!   reference), written at the end of its extension line, with no arrows;
//! * the **parallel** dimensions (all from the datum, stacked) are the kind `cumulative`.
//!
//! A dimension with no offset of its own is put by the sheet: the dimensions of one view that
//! share a side are taken in the order of the file and each goes on the first line, counted
//! outward from the view, where it does not meet another one along the line (with the room of
//! `sheet.dim.clearance` between them).

use std::collections::BTreeMap;

use super::measure::{face_place, view_axes, Measured, Resolved};
use super::model::{Dim, DimKind};
use super::prim::{arrow, Anchor, Prim};
use super::settings::DimStyle;
use crate::enclosure::Id;

type Fail = (&'static str, String);

const INVALID: &str = "PHOENIX-CAD-056";

/// Draws a chain or an ordinate dimension that has been measured.
pub fn draw(
    dim: &Dim,
    m: &Measured,
    frame: &super::measure::Frame,
    style: &DimStyle,
) -> Result<Vec<Prim>, Fail> {
    let (u_axis, v_axis) = view_axes(frame.kind).ok_or((
        INVALID,
        format!(
            "{}: lengths are drawn in the front, top and right views",
            dim.name
        ),
    ))?;
    let Some(Resolved::Face { axis, .. }) = m.refs.first() else {
        return Err((INVALID, format!("{} measures faces", dim.name)));
    };
    let horizontal = *axis == u_axis;
    if !horizontal && *axis != v_axis {
        return Err((
            INVALID,
            format!(
                "{}: the axis {} is not in a view of this kind",
                dim.name,
                axis.name()
            ),
        ));
    }
    let layer = dim.layer.as_str();
    let h = style.text_height_um;
    let level = dim.offset_um.unwrap_or(style.offset_um);
    let line_at = if horizontal {
        frame.origin.1 - level
    } else {
        frame.origin.0 - level
    };
    let xy = |along: i64, across: i64| {
        if horizontal {
            (along, across)
        } else {
            (across, along)
        }
    };
    let places: Vec<(i64, i64)> = m
        .refs
        .iter()
        .map(|r| face_place(r, frame, horizontal, u_axis, v_axis))
        .collect();
    let mut out = Vec::new();
    let overshoot = if dim.kind == DimKind::Ordinate {
        0
    } else {
        style.extension_overshoot_um
    };
    for (along, across) in &places {
        out.push(Prim::line(
            layer,
            xy(*along, across - style.extension_gap_um),
            xy(*along, line_at - overshoot),
        ));
    }
    let lo = places.iter().map(|p| p.0).min().unwrap_or(0);
    let hi = places.iter().map(|p| p.0).max().unwrap_or(0);
    out.push(Prim::line(layer, xy(lo, line_at), xy(hi, line_at)));
    let text = |along: i64, across: i64, s: &str| -> Prim {
        if horizontal {
            Prim::text(layer, (along, across), h, s, Anchor::Middle)
        } else {
            Prim::turned(layer, (across, along), h, s, Anchor::Middle, 90.0)
        }
    };
    let dirs = |a: i64, b: i64| -> ((f64, f64), (f64, f64)) {
        let s = if b >= a { 1.0 } else { -1.0 };
        if horizontal {
            ((s, 0.0), (-s, 0.0))
        } else {
            ((0.0, s), (0.0, -s))
        }
    };
    if dim.kind == DimKind::Chain {
        for (k, w) in places.windows(2).enumerate() {
            let (a, b) = (w[0].0, w[1].0);
            let (da, db) = dirs(a, b);
            out.extend(arrow(
                layer,
                xy(a, line_at),
                da,
                style.arrow_length_um,
                style.arrow_width_um,
            ));
            out.extend(arrow(
                layer,
                xy(b, line_at),
                db,
                style.arrow_length_um,
                style.arrow_width_um,
            ));
            let across = if horizontal {
                line_at + h * 3 / 10
            } else {
                line_at - h * 3 / 10
            };
            out.push(text((a + b) / 2, across, &m.texts[k]));
        }
    } else {
        let across = if horizontal {
            line_at - h * 12 / 10
        } else {
            line_at - h * 3 / 10
        };
        out.push(text(places[0].0, across, "0"));
        for (k, p) in places.iter().enumerate().skip(1) {
            out.push(text(p.0, across, &m.texts[k - 1]));
        }
    }
    Ok(out)
}

/// A dimension line that wants a place: the dimensions of one view on one side, the stretch of
/// the sheet it covers along its line, and how many lines it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// The dimension.
    pub id: Id,
    /// The view it is drawn in.
    pub view: Id,
    /// Whether its lines run across the sheet (below the view) or up it (left of the view).
    pub horizontal: bool,
    /// Where it starts along its line, in micrometres.
    pub lo: i64,
    /// Where it ends.
    pub hi: i64,
    /// The distance between its lines.
    pub step: i64,
    /// How many lines it takes.
    pub lines: i64,
}

/// How far from the first line each dimension goes, in micrometres: `0` for the first line. A
/// dimension is put on the nearest line where it does not meet one already placed (along its
/// line, with `clearance` between), so that dimensions that overlap are stacked and dimensions
/// that do not share the line.
pub fn place(slots: &[Slot], clearance: i64) -> BTreeMap<Id, i64> {
    let mut placed: Vec<(Slot, i64)> = Vec::new();
    let mut out = BTreeMap::new();
    for s in slots {
        let height = s.lines.max(1) * s.step;
        let meets = |p: &(Slot, i64)| -> bool {
            p.0.view == s.view
                && p.0.horizontal == s.horizontal
                && p.0.lo < s.hi + clearance
                && s.lo < p.0.hi + clearance
        };
        let mut candidates = vec![0];
        candidates.extend(
            placed
                .iter()
                .filter(|p| meets(p))
                .map(|p| p.1 + p.0.lines.max(1) * p.0.step),
        );
        candidates.sort_unstable();
        let at = candidates
            .into_iter()
            .find(|b| {
                placed
                    .iter()
                    .filter(|p| meets(p))
                    .all(|p| *b >= p.1 + p.0.lines.max(1) * p.0.step || *b + height <= p.1)
            })
            .unwrap_or(0);
        placed.push((*s, at));
        out.insert(s.id, at);
    }
    out
}

/// Where a dimension line is on the sheet, for picking it and dragging it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DimLine {
    /// Whether the line runs across the sheet.
    pub horizontal: bool,
    /// The lower left corner of the view it is drawn in.
    pub view_origin: (i64, i64),
    /// The distance of its first line from the view.
    pub level_um: i64,
    /// Where it starts along its line.
    pub lo: i64,
    /// Where it ends.
    pub hi: i64,
}

impl DimLine {
    /// The coordinate of the first line across the direction of the line.
    pub fn line_at(&self) -> i64 {
        if self.horizontal {
            self.view_origin.1 - self.level_um
        } else {
            self.view_origin.0 - self.level_um
        }
    }

    /// Whether a point of the sheet is on the line, within `tolerance` micrometres.
    pub fn hit(&self, pointer: (i64, i64), tolerance: i64) -> bool {
        let (along, across) = if self.horizontal {
            (pointer.0, pointer.1)
        } else {
            (pointer.1, pointer.0)
        };
        along >= self.lo - tolerance
            && along <= self.hi + tolerance
            && (across - self.line_at()).abs() <= tolerance
    }

    /// The distance from the view that puts the line at the pointer, rounded to a multiple of
    /// `snap` micrometres and kept within a metre.
    pub fn offset_for(&self, pointer: (i64, i64), snap: i64) -> i64 {
        let raw = if self.horizontal {
            self.view_origin.1 - pointer.1
        } else {
            self.view_origin.0 - pointer.0
        };
        let snap = snap.max(1);
        let q = (raw as f64 / snap as f64).round() as i64;
        (q * snap).clamp(-1_000_000, 1_000_000)
    }
}
