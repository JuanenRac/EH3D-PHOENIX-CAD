// SPDX-License-Identifier: GPL-3.0-or-later
//! What the pointer can take on the sheet and drag, with no window in it: the line of a cut, a
//! balloon (the circle or the end of its leader) and a note. Every function takes points of the
//! sheet in micrometres and gives the new place of the thing; the panel applies it to the sheet
//! (`PlanState`) and the window only passes the pointer on.

use super::model::{Balloon, Note};
use crate::enclosure::Id;

/// The line of the cut of a section view, as it is drawn in the view it is cut from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutLine {
    /// The section view.
    pub view: Id,
    /// Whether the line is vertical on the sheet (the plane of the cut is across the axis that
    /// runs across the picture of the view it is drawn in).
    pub vertical: bool,
    /// Where the line is on the sheet across itself (`x` for a vertical line), in micrometres.
    pub at_um: i64,
    /// Where the line starts and ends on the sheet along itself, in micrometres.
    pub span: (i64, i64),
    /// The position of the plane, in nanometres.
    pub at_nm: i64,
    /// The lowest coordinate of the picture along the axis of the cut, in nanometres.
    pub lo_nm: i64,
    /// Where that coordinate is on the sheet, in micrometres.
    pub origin_um: i64,
    /// The scale `n:d` of the view the line is drawn in.
    pub scale: (u32, u32),
    /// Whether the line can be dragged: not when the view it is drawn in is a broken one, where
    /// the sheet does not say where the plane is in the model.
    pub draggable: bool,
}

impl CutLine {
    /// Whether the point is on the line, within `tolerance`.
    pub fn hit(&self, p: (i64, i64), tolerance: i64) -> bool {
        let (across, along) = if self.vertical {
            (p.0, p.1)
        } else {
            (p.1, p.0)
        };
        (across - self.at_um).abs() <= tolerance
            && along >= self.span.0 - tolerance
            && along <= self.span.1 + tolerance
    }

    /// How far the point is from the line across it.
    pub fn distance(&self, p: (i64, i64)) -> i64 {
        let across = if self.vertical { p.0 } else { p.1 };
        (across - self.at_um).abs()
    }

    /// The position of the plane, in nanometres, that puts the line at the point. It lands on a
    /// multiple of `snap_um` of the sheet (so, in the model, of `snap_um` times the scale).
    pub fn at_nm_for(&self, p: (i64, i64), snap_um: i64) -> i64 {
        let across = if self.vertical { p.0 } else { p.1 };
        let (n, d) = (i128::from(self.scale.0), i128::from(self.scale.1));
        let raw = i128::from(self.lo_nm) + i128::from(across - self.origin_um) * 1000 * d / n;
        let step = (i128::from(snap_um.max(1)) * 1000 * d / n).max(1);
        let snapped = (raw + step / 2).div_euclid(step) * step;
        snapped.clamp(-1_000_000_000_000, 1_000_000_000_000) as i64
    }
}

/// The part of a balloon that is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalloonPart {
    /// The circle with its number (the leader keeps its end).
    Circle,
    /// The end of the leader (the circle stays).
    Tip,
}

/// The balloon, and the part of it, under a point of the sheet. The end of the leader wins over the
/// circle when both are under the point (a short leader), and the nearest balloon wins.
pub fn pick_balloon(
    balloons: &[Balloon],
    radius_um: i64,
    p: (i64, i64),
    tolerance: i64,
) -> Option<(Id, BalloonPart)> {
    let dist = |a: (i64, i64)| -> i64 {
        let (dx, dy) = ((a.0 - p.0) as f64, (a.1 - p.1) as f64);
        dx.hypot(dy) as i64
    };
    balloons
        .iter()
        .flat_map(|b| {
            [
                (b.id, BalloonPart::Tip, dist(b.to), tolerance),
                (b.id, BalloonPart::Circle, dist(b.at), radius_um + tolerance),
            ]
        })
        .filter(|(_, _, d, reach)| d <= reach)
        .min_by_key(|(_, part, d, _)| (*d, i64::from(*part == BalloonPart::Circle)))
        .map(|(id, part, _, _)| (id, part))
}

/// Where a point lands when it is dragged on a grid of `snap_um`.
pub fn snap(p: (i64, i64), snap_um: i64) -> (i64, i64) {
    let step = snap_um.max(1);
    let one = |v: i64| (v + step / 2).div_euclid(step) * step;
    (one(p.0), one(p.1))
}

/// The note whose text starts near a point of the sheet: within `tolerance` of its start, or of
/// the box of its text (`height_um` high and as wide as its characters at 60 % of the height).
pub fn pick_note(notes: &[Note], height_um: i64, p: (i64, i64), tolerance: i64) -> Option<Id> {
    notes
        .iter()
        .filter(|n| {
            let width = n.text.chars().count() as i64 * height_um * 6 / 10;
            p.0 >= n.at.0 - tolerance
                && p.0 <= n.at.0 + width + tolerance
                && p.1 >= n.at.1 - tolerance
                && p.1 <= n.at.1 + height_um + tolerance
        })
        .min_by_key(|n| {
            let (dx, dy) = ((n.at.0 - p.0) as f64, (n.at.1 - p.1) as f64);
            dx.hypot(dy) as i64
        })
        .map(|n| n.id)
}
