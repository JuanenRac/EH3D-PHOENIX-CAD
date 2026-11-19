// SPDX-License-Identifier: GPL-3.0-or-later
//! Patterns of parts: a part of the catalog placed in the model, repeated along a line, over a
//! grid or round a vertical axis. Like the patterns of holes (see [`crate::pattern`]) it only
//! works out positions; the copies become parts of their own, placed by one step each, so a
//! project written with them needs nothing new in the project file and every copy can be moved or
//! taken away afterwards.
//!
//! The positions follow from the position of the seed (the smallest corner of its box, which is
//! also the first position) and the layout. A circular pattern moves the seed round the axis
//! through a centre, in the plane of `x` and `y`, and does not turn the copies: a part can only be
//! turned by quarter turns, which the orientation of the seed already says. All the positions are
//! in whole nanometres with the integer sine and cosine of [`crate::units::Angle`], so they are
//! the same on every platform. A pattern with fewer than two positions, with more than the limit
//! of the settings (`pattern.max_parts`), with a step of nothing, round a centre that is the seed
//! itself or with two positions that are one is refused with `PHOENIX-CAD-042`, before any copy
//! is made.

use std::collections::BTreeSet;

use crate::catalog::PartInstance;
use crate::section::PreviewLine;
use crate::units::{scale, Angle, Count};
use crate::{CadError, Length};

/// How the copies of a part follow from the seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartLayout {
    /// `count` positions in a line, each `step` (along `x`, `y` and `z`) after the previous.
    Linear {
        /// The offset from one position to the next.
        step: [Length; 3],
        /// How many positions, the seed among them.
        count: Count,
    },
    /// `columns` by `rows` positions in the plane of the seed, from the seed: the columns along
    /// `x` and the rows along `y`.
    Grid {
        /// The distance between columns (`x`) and between rows (`y`).
        pitch: [Length; 2],
        /// How many positions in a row.
        columns: Count,
        /// How many rows.
        rows: Count,
    },
    /// `count` positions round the vertical axis through `center`, the seed at the first and each
    /// `step` further round, counter-clockwise seen from above.
    Circular {
        /// The axis, as a point in the plane of `x` and `y`.
        center: [Length; 2],
        /// The angle between two positions.
        step: Angle,
        /// How many positions, the seed among them.
        count: Count,
    },
}

fn pattern_error(why: impl Into<String>) -> CadError {
    CadError::Pattern(why.into())
}

fn shifted(start: Length, step: Length, index: u32) -> Result<Length, CadError> {
    let delta = step
        .get()
        .checked_mul(i64::from(index))
        .ok_or(CadError::Overflow("pattern"))?;
    start.checked_add(Length::new(delta)?)
}

impl PartLayout {
    /// How many positions the layout has, the seed among them.
    pub fn count(&self) -> u64 {
        match *self {
            PartLayout::Linear { count, .. } | PartLayout::Circular { count, .. } => {
                u64::from(count.get())
            }
            PartLayout::Grid { columns, rows, .. } => {
                u64::from(columns.get()) * u64::from(rows.get())
            }
        }
    }

    /// The positions of the smallest corner of the part, in order, the first being `seed`. `max`
    /// is the most positions the settings allow.
    pub fn positions(&self, seed: [Length; 3], max: u32) -> Result<Vec<[Length; 3]>, CadError> {
        let n = self.count();
        if n < 2 {
            return Err(pattern_error("a pattern needs at least two positions"));
        }
        if n > u64::from(max) {
            return Err(pattern_error(format!(
                "{n} positions are more than the {max} that the settings allow"
            )));
        }
        let mut out = Vec::new();
        match *self {
            PartLayout::Linear { step, count } => {
                if step.iter().all(|s| s.get() == 0) {
                    return Err(pattern_error("the step of a line of parts is nothing"));
                }
                for i in 0..count.get() {
                    out.push([
                        shifted(seed[0], step[0], i)?,
                        shifted(seed[1], step[1], i)?,
                        shifted(seed[2], step[2], i)?,
                    ]);
                }
            }
            PartLayout::Grid {
                pitch,
                columns,
                rows,
            } => {
                if (columns.get() > 1 && pitch[0].get() == 0)
                    || (rows.get() > 1 && pitch[1].get() == 0)
                {
                    return Err(pattern_error("the pitch of a grid of parts is nothing"));
                }
                for r in 0..rows.get() {
                    for c in 0..columns.get() {
                        out.push([
                            shifted(seed[0], pitch[0], c)?,
                            shifted(seed[1], pitch[1], r)?,
                            seed[2],
                        ]);
                    }
                }
            }
            PartLayout::Circular {
                center,
                step,
                count,
            } => {
                let (vx, vy) = (
                    Length::new(seed[0].get() - center[0].get())?,
                    Length::new(seed[1].get() - center[1].get())?,
                );
                if vx.get() == 0 && vy.get() == 0 {
                    return Err(pattern_error(
                        "the centre of a circle of parts is the seed itself",
                    ));
                }
                for i in 0..count.get() {
                    let turn = step
                        .get()
                        .checked_mul(i64::from(i))
                        .ok_or(CadError::Overflow("pattern"))?;
                    let angle = Angle::new(turn.rem_euclid(360_000_000_000))?;
                    let (cos, sin) = (angle.cos(), angle.sin());
                    let one = 1_000_000_000;
                    let x = scale(vx, cos, one)?.get() - scale(vy, sin, one)?.get();
                    let y = scale(vx, sin, one)?.get() + scale(vy, cos, one)?.get();
                    out.push([
                        center[0].checked_add(Length::new(x)?)?,
                        center[1].checked_add(Length::new(y)?)?,
                        seed[2],
                    ]);
                }
            }
        }
        let distinct: BTreeSet<[i64; 3]> = out.iter().map(|p| p.map(Length::get)).collect();
        if distinct.len() != out.len() {
            return Err(pattern_error("two positions of the pattern are the same"));
        }
        Ok(out)
    }
}

/// The copies of a part that a pattern adds: the part at each position but the first (which is
/// the seed itself), named `"<name> 2"`, `"<name> 3"` and so on.
pub fn copies(
    seed: &PartInstance,
    layout: &PartLayout,
    max: u32,
) -> Result<Vec<PartInstance>, CadError> {
    let positions = layout.positions(seed.at, max)?;
    Ok(positions
        .into_iter()
        .enumerate()
        .skip(1)
        .map(|(i, at)| PartInstance {
            name: format!("{} {}", seed.name, i + 1),
            at,
            ..seed.clone()
        })
        .collect())
}

/// The plan of a pattern as lines fitted in a pane of `width` x `height` pixels with a `margin`
/// all round: the outline of the box of the part seen from above at each position (the seed
/// first), scaled to fit with the same scale along both axes, centred, and turned so that `y`
/// points up the pane. The lines of the seed are marked `hole` so that it can be drawn in another
/// colour. A pane too small for the margin gives no line.
pub fn preview(
    seed: &PartInstance,
    positions: &[[Length; 3]],
    width: f64,
    height: f64,
    margin: f64,
) -> Vec<PreviewLine> {
    let (size, _) = seed.extent();
    let boxes: Vec<(f64, f64, f64, f64)> = positions
        .iter()
        .map(|p| {
            let (x, y) = (p[0].get() as f64, p[1].get() as f64);
            (x, y, x + size[0] as f64, y + size[1] as f64)
        })
        .collect();
    let Some(first) = boxes.first() else {
        return Vec::new();
    };
    let (mut lo, mut hi) = ((first.0, first.1), (first.2, first.3));
    for b in &boxes {
        lo = (lo.0.min(b.0), lo.1.min(b.1));
        hi = (hi.0.max(b.2), hi.1.max(b.3));
    }
    let (room_w, room_h) = (width - 2.0 * margin, height - 2.0 * margin);
    if !(room_w > 0.0 && room_h > 0.0) {
        return Vec::new();
    }
    let (dx, dy) = (hi.0 - lo.0, hi.1 - lo.1);
    let scale = match (dx > 0.0, dy > 0.0) {
        (true, true) => (room_w / dx).min(room_h / dy),
        (true, false) => room_w / dx,
        (false, true) => room_h / dy,
        (false, false) => 1.0,
    };
    let (left, top) = (
        margin + (room_w - dx * scale) / 2.0,
        margin + (room_h - dy * scale) / 2.0,
    );
    let to_pane = |x: f64, y: f64| (left + (x - lo.0) * scale, top + (hi.1 - y) * scale);
    let mut out = Vec::new();
    for (i, b) in boxes.iter().enumerate() {
        let corners = [
            to_pane(b.0, b.1),
            to_pane(b.2, b.1),
            to_pane(b.2, b.3),
            to_pane(b.0, b.3),
        ];
        for k in 0..4 {
            out.push(PreviewLine {
                a: corners[k],
                b: corners[(k + 1) % 4],
                hole: i == 0,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Orientation, Part, Shape};

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn count(n: i64) -> Count {
        Count::new(n).unwrap()
    }

    /// A bar of 20 x 10 mm in section and 100 mm long, standing along `z`.
    fn bar() -> PartInstance {
        PartInstance {
            name: "Bar".into(),
            library: "stock".into(),
            part: Part {
                name: "bar".into(),
                material: "aluminium".into(),
                shape: Shape::Profile {
                    width: mm(20),
                    height: mm(10),
                    wall: mm(0),
                    default_cut: mm(100),
                    max_cut: mm(6000),
                },
            },
            at: [mm(5), mm(7), mm(0)],
            orientation: Orientation::default(),
            cut: Some(mm(100)),
        }
    }

    fn mm_of(p: &[Length; 3]) -> [i64; 3] {
        p.map(|v| v.get() / 1_000_000)
    }

    #[test]
    fn a_line_of_parts_steps_along_all_three_axes() {
        let layout = PartLayout::Linear {
            step: [mm(30), mm(0), mm(-2)],
            count: count(4),
        };
        let at = layout.positions(bar().at, 64).unwrap();
        let listed: Vec<[i64; 3]> = at.iter().map(mm_of).collect();
        assert_eq!(
            listed,
            vec![[5, 7, 0], [35, 7, -2], [65, 7, -4], [95, 7, -6]]
        );
        // the copies are the parts at all positions but the first, with their own names
        let made = copies(&bar(), &layout, 64).unwrap();
        let names: Vec<&str> = made.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Bar 2", "Bar 3", "Bar 4"]);
        assert_eq!(mm_of(&made[2].at), [95, 7, -6]);
        assert!(made
            .iter()
            .all(|c| c.part == bar().part && c.cut == bar().cut));
    }

    #[test]
    fn a_grid_of_parts_goes_row_after_row_in_the_plane_of_the_seed() {
        let layout = PartLayout::Grid {
            pitch: [mm(25), mm(15)],
            columns: count(3),
            rows: count(2),
        };
        let listed: Vec<[i64; 3]> = layout
            .positions(bar().at, 64)
            .unwrap()
            .iter()
            .map(mm_of)
            .collect();
        assert_eq!(
            listed,
            vec![
                [5, 7, 0],
                [30, 7, 0],
                [55, 7, 0],
                [5, 22, 0],
                [30, 22, 0],
                [55, 22, 0]
            ]
        );
        assert_eq!(layout.count(), 6);
    }

    #[test]
    fn a_circle_of_parts_goes_round_the_axis_and_back_to_the_start_exactly() {
        // the seed is 40 mm from the centre along x: the quarter turns land on whole millimetres
        let mut seed = bar();
        seed.at = [mm(50), mm(10), mm(3)];
        let layout = PartLayout::Circular {
            center: [mm(10), mm(10)],
            step: Angle::from_deg(90).unwrap(),
            count: count(4),
        };
        let listed: Vec<[i64; 3]> = layout
            .positions(seed.at, 64)
            .unwrap()
            .iter()
            .map(mm_of)
            .collect();
        assert_eq!(
            listed,
            vec![[50, 10, 3], [10, 50, 3], [-30, 10, 3], [10, -30, 3]]
        );
        // a step of 45 degrees through eight positions gives the diagonal at 40 cos 45 degrees
        let eight = PartLayout::Circular {
            center: [mm(10), mm(10)],
            step: Angle::from_deg(45).unwrap(),
            count: count(8),
        };
        let at = eight.positions(seed.at, 64).unwrap();
        let diagonal = 10_000_000 + (40_000_000.0 * std::f64::consts::FRAC_1_SQRT_2) as i64;
        assert!((at[1][0].get() - diagonal).abs() <= 1, "{:?}", at[1]);
        assert!((at[1][1].get() - diagonal).abs() <= 1, "{:?}", at[1]);
        // a negative step goes the other way round
        let back = PartLayout::Circular {
            center: [mm(10), mm(10)],
            step: Angle::from_deg(-90).unwrap(),
            count: count(2),
        };
        assert_eq!(
            mm_of(&back.positions(seed.at, 64).unwrap()[1]),
            [10, -30, 3]
        );
    }

    #[test]
    fn a_pattern_that_cannot_be_made_is_refused_with_its_code() {
        let line = |step: [Length; 3], n: i64| PartLayout::Linear {
            step,
            count: count(n),
        };
        let ring = |center: [Length; 2], deg: i64, n: i64| PartLayout::Circular {
            center,
            step: Angle::from_deg(deg).unwrap(),
            count: count(n),
        };
        for (layout, why) in [
            (line([mm(10), mm(0), mm(0)], 1), "at least two"),
            (line([mm(10), mm(0), mm(0)], 65), "more than the 64"),
            (line([mm(0), mm(0), mm(0)], 3), "step of a line"),
            (
                PartLayout::Grid {
                    pitch: [mm(0), mm(10)],
                    columns: count(2),
                    rows: count(2),
                },
                "pitch of a grid",
            ),
            (
                PartLayout::Grid {
                    pitch: [mm(10), mm(10)],
                    columns: count(1),
                    rows: count(1),
                },
                "at least two",
            ),
            (ring([mm(5), mm(7)], 90, 4), "seed itself"),
            (ring([mm(0), mm(0)], 120, 4), "are the same"),
            (ring([mm(0), mm(0)], 0, 3), "are the same"),
        ] {
            let e = layout.positions(bar().at, 64).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-042", "{why}: {e}");
            assert!(e.to_string().contains(why), "{why}: {e}");
        }
        // a grid of one column needs no pitch along x
        let column = PartLayout::Grid {
            pitch: [mm(0), mm(10)],
            columns: count(1),
            rows: count(3),
        };
        assert_eq!(column.positions(bar().at, 64).unwrap().len(), 3);
        // the limit of the settings is the one used
        assert!(line([mm(10), mm(0), mm(0)], 64)
            .positions(bar().at, 64)
            .is_ok());
        assert!(line([mm(10), mm(0), mm(0)], 64)
            .positions(bar().at, 63)
            .is_err());
        // a position outside the range of lengths is refused too
        let far = line([Length::new((1 << 40) - 1).unwrap(), mm(0), mm(0)], 2);
        assert!(far.positions(bar().at, 64).is_err());
    }

    #[test]
    fn the_plan_shows_the_box_of_each_copy_and_marks_the_seed() {
        let layout = PartLayout::Linear {
            step: [mm(40), mm(0), mm(0)],
            count: count(3),
        };
        let at = layout.positions(bar().at, 64).unwrap();
        let lines = preview(&bar(), &at, 200.0, 100.0, 10.0);
        assert_eq!(lines.len(), 12);
        assert_eq!(lines.iter().filter(|l| l.hole).count(), 4);
        // the boxes are 20 x 10 mm and span 100 x 10 mm: the width fills the 180 pixels of room
        let xs: Vec<f64> = lines.iter().flat_map(|l| [l.a.0, l.b.0]).collect();
        let min = xs.iter().cloned().fold(f64::MAX, f64::min);
        let max = xs.iter().cloned().fold(f64::MIN, f64::max);
        assert!(
            (min - 10.0).abs() < 1e-6 && (max - 190.0).abs() < 1e-6,
            "{min} {max}"
        );
        // a pane too small and no position give nothing
        assert!(preview(&bar(), &at, 15.0, 100.0, 10.0).is_empty());
        assert!(preview(&bar(), &[], 200.0, 100.0, 10.0).is_empty());
        // y points up the pane: the seed of a grid is at the lower left
        let grid = PartLayout::Grid {
            pitch: [mm(30), mm(30)],
            columns: count(2),
            rows: count(2),
        };
        let at = grid.positions(bar().at, 64).unwrap();
        let lines = preview(&bar(), &at, 100.0, 100.0, 0.0);
        let seed_lowest = lines
            .iter()
            .filter(|l| l.hole)
            .flat_map(|l| [l.a.1, l.b.1])
            .fold(f64::MIN, f64::max);
        let all_lowest = lines
            .iter()
            .flat_map(|l| [l.a.1, l.b.1])
            .fold(f64::MIN, f64::max);
        assert!((seed_lowest - all_lowest).abs() < 1e-6);
    }
}
