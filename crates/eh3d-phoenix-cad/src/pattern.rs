//! Patterns of holes (a row, a grid, a circle) and batches of operations that apply all or nothing.
//!
//! A pattern only computes positions; [`holes_of_pattern`] turns them into `AddHole` operations
//! with ids that the caller provides, and [`Enclosure::apply_all`] applies a list of operations
//! so that either every one is applied or the enclosure stays exactly as it was. The positions of
//! a circle use the integer sine and cosine of [`crate::units::Angle`], so they are the same on
//! every platform.

use crate::enclosure::{Enclosure, Face, Hole, Id, Operation};
use crate::units::{scale, Angle, Count, COUNT_LIMIT};
use crate::{CadError, Length};

/// A position on a face: `u` to the right and `v` upwards from the lower left corner.
pub type Position = (Length, Length);

/// A repeated arrangement of positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    /// `count` positions in a line, from `start`, each `step` after the previous.
    Linear {
        /// The first position.
        start: Position,
        /// The offset from one position to the next.
        step: Position,
        /// How many positions.
        count: Count,
    },
    /// `columns` by `rows` positions, rows after rows, from `start`.
    Grid {
        /// The first position (the lower left one).
        start: Position,
        /// The distance between columns (u) and between rows (v).
        pitch: Position,
        /// How many positions in a row.
        columns: Count,
        /// How many rows.
        rows: Count,
    },
    /// `count` positions on a circle, the first at `start` and each `step` further round.
    Polar {
        /// The centre of the circle.
        center: Position,
        /// The radius.
        radius: Length,
        /// The angle of the first position, counted from the u direction towards the v direction.
        start: Angle,
        /// The angle between two positions.
        step: Angle,
        /// How many positions.
        count: Count,
    },
}

fn offset(start: Length, step: Length, index: u32) -> Result<Length, CadError> {
    let delta = step
        .get()
        .checked_mul(i64::from(index))
        .ok_or(CadError::Overflow("pattern"))?;
    start.checked_add(Length::new(delta)?)
}

impl Pattern {
    /// The positions in order. A pattern with no position, or with more than the limit of a
    /// count, is refused.
    pub fn positions(&self) -> Result<Vec<Position>, CadError> {
        let mut out = Vec::new();
        match *self {
            Pattern::Linear { start, step, count } => {
                if count.get() == 0 {
                    return Err(CadError::ValueOutOfRange("pattern count"));
                }
                for i in 0..count.get() {
                    out.push((offset(start.0, step.0, i)?, offset(start.1, step.1, i)?));
                }
            }
            Pattern::Grid {
                start,
                pitch,
                columns,
                rows,
            } => {
                let total = u64::from(columns.get()) * u64::from(rows.get());
                if total == 0 || total > u64::from(COUNT_LIMIT) {
                    return Err(CadError::ValueOutOfRange("pattern count"));
                }
                for r in 0..rows.get() {
                    for c in 0..columns.get() {
                        out.push((offset(start.0, pitch.0, c)?, offset(start.1, pitch.1, r)?));
                    }
                }
            }
            Pattern::Polar {
                center,
                radius,
                start,
                step,
                count,
            } => {
                if count.get() == 0 {
                    return Err(CadError::ValueOutOfRange("pattern count"));
                }
                for i in 0..count.get() {
                    let turn = step
                        .get()
                        .checked_mul(i64::from(i))
                        .ok_or(CadError::Overflow("pattern"))?;
                    let angle = start.checked_add(Angle::new(turn)?)?;
                    let du = scale(radius, angle.cos(), 1_000_000_000)?;
                    let dv = scale(radius, angle.sin(), 1_000_000_000)?;
                    out.push((center.0.checked_add(du)?, center.1.checked_add(dv)?));
                }
            }
        }
        Ok(out)
    }
}

/// The operations that add a round hole of the given diameter at each position of a pattern, on
/// one face. The holes are named `"<name> 1"`, `"<name> 2"`, ... and take their ids from
/// `new_id`, called once per hole.
pub fn holes_of_pattern(
    pattern: &Pattern,
    face: Face,
    diameter: Length,
    name: &str,
    mut new_id: impl FnMut() -> Id,
) -> Result<Vec<Operation>, CadError> {
    let positions = pattern.positions()?;
    Ok(positions
        .into_iter()
        .enumerate()
        .map(|(i, (u, v))| Operation::AddHole {
            id: new_id(),
            hole: Hole {
                name: format!("{name} {}", i + 1),
                face,
                u,
                v,
                diameter,
            },
        })
        .collect())
}

impl Enclosure {
    /// Applies the operations one after the other, all or nothing: when one is refused, the ones
    /// before it are undone and the enclosure is exactly as it was. On success it returns the
    /// operations that undo the batch, to be applied in the order given.
    pub fn apply_all(&mut self, ops: Vec<Operation>) -> Result<Vec<Operation>, CadError> {
        let mut undo: Vec<Operation> = Vec::with_capacity(ops.len());
        for op in ops {
            match self.apply(op) {
                Ok(inverse) => undo.push(inverse),
                Err(e) => {
                    for inverse in undo.into_iter().rev() {
                        self.apply(inverse)
                            .expect("the inverse of an operation just applied is valid");
                    }
                    return Err(e);
                }
            }
        }
        undo.reverse();
        Ok(undo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::EnclosureParams;
    use eh3d_container::ObjectId;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn count(n: i64) -> Count {
        Count::new(n).unwrap()
    }

    fn enclosure() -> Enclosure {
        Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap()
    }

    fn ids() -> impl FnMut() -> Id {
        let mut n = 0u64;
        move || {
            n += 1;
            ObjectId::from_parts(1_700_000_000_000, (n & 0x0fff) as u16, n).unwrap()
        }
    }

    #[test]
    fn a_line_has_equally_spaced_positions() {
        let p = Pattern::Linear {
            start: (mm(10), mm(17)),
            step: (mm(15), Length::ZERO),
            count: count(4),
        };
        let pos = p.positions().unwrap();
        let us: Vec<i64> = pos.iter().map(|(u, _)| u.get() / 1_000_000).collect();
        assert_eq!(us, vec![10, 25, 40, 55]);
        assert!(pos.iter().all(|(_, v)| *v == mm(17)));
    }

    #[test]
    fn a_grid_goes_along_the_columns_row_after_row() {
        let p = Pattern::Grid {
            start: (mm(30), mm(20)),
            pitch: (mm(8), mm(8)),
            columns: count(3),
            rows: count(2),
        };
        let pos: Vec<(i64, i64)> = p
            .positions()
            .unwrap()
            .iter()
            .map(|(u, v)| (u.get() / 1_000_000, v.get() / 1_000_000))
            .collect();
        assert_eq!(
            pos,
            vec![(30, 20), (38, 20), (46, 20), (30, 28), (38, 28), (46, 28)]
        );
    }

    #[test]
    fn a_circle_of_four_has_exact_positions_at_the_quarter_turns() {
        let p = Pattern::Polar {
            center: (mm(50), mm(30)),
            radius: mm(10),
            start: Angle::ZERO,
            step: Angle::from_deg(90).unwrap(),
            count: count(4),
        };
        let pos: Vec<(i64, i64)> = p
            .positions()
            .unwrap()
            .iter()
            .map(|(u, v)| (u.get(), v.get()))
            .collect();
        assert_eq!(
            pos,
            vec![
                (60_000_000, 30_000_000),
                (50_000_000, 40_000_000),
                (40_000_000, 30_000_000),
                (50_000_000, 20_000_000)
            ]
        );
    }

    #[test]
    fn a_circle_of_six_closes_on_itself_within_a_nanometre() {
        let p = Pattern::Polar {
            center: (mm(50), mm(30)),
            radius: mm(10),
            start: Angle::from_deg(30).unwrap(),
            step: Angle::from_deg(60).unwrap(),
            count: count(6),
        };
        let pos = p.positions().unwrap();
        // the first at 30 degrees: 10 * cos(30) = 8.660254 mm
        assert_eq!(pos[0].0.get(), 50_000_000 + 8_660_254);
        assert_eq!(pos[0].1.get(), 30_000_000 + 5_000_000);
        // the position 180 degrees round is the mirror image about the centre
        assert_eq!(
            pos[0].0.get() + pos[3].0.get(),
            2 * 50_000_000,
            "opposite positions are symmetric"
        );
        assert_eq!(pos[0].1.get() + pos[3].1.get(), 2 * 30_000_000);
    }

    #[test]
    fn a_pattern_with_nothing_in_it_or_too_much_is_refused() {
        let empty = Pattern::Linear {
            start: (mm(10), mm(10)),
            step: (mm(1), mm(0)),
            count: count(0),
        };
        assert_eq!(empty.positions().unwrap_err().code(), "PHOENIX-CAD-007");
        let huge = Pattern::Grid {
            start: (mm(1), mm(1)),
            pitch: (mm(1), mm(1)),
            columns: count(2_000),
            rows: count(2_000),
        };
        assert_eq!(huge.positions().unwrap_err().code(), "PHOENIX-CAD-007");
        let far = Pattern::Linear {
            start: (mm(1), mm(1)),
            step: (mm(500_000), mm(0)),
            count: count(10),
        };
        assert_eq!(far.positions().unwrap_err().code(), "PHOENIX-CAD-001");
    }

    #[test]
    fn a_vent_grid_on_the_lid_is_added_with_distinct_ids_and_names() {
        let p = Pattern::Grid {
            start: (mm(30), mm(20)),
            pitch: (mm(8), mm(8)),
            columns: count(5),
            rows: count(3),
        };
        let ops = holes_of_pattern(&p, Face::Top, mm(4), "vent", ids()).unwrap();
        assert_eq!(ops.len(), 15);
        let mut e = enclosure();
        let undo = e.apply_all(ops).unwrap();
        assert_eq!(e.holes().len(), 15);
        let names: Vec<&str> = e.holes().values().map(|h| h.name.as_str()).collect();
        assert!(names.contains(&"vent 1") && names.contains(&"vent 15"));
        // the undo list takes the batch back out
        for op in undo {
            e.apply(op).unwrap();
        }
        assert_eq!(e, enclosure());
    }

    #[test]
    fn a_batch_with_one_refused_operation_leaves_the_enclosure_as_it_was() {
        // 90, 100 and 110 mm: the second centre is already too near the edge of a 104 mm face
        let p = Pattern::Linear {
            start: (mm(90), mm(17)),
            step: (mm(10), Length::ZERO),
            count: count(3),
        };
        let ops = holes_of_pattern(&p, Face::Front, mm(6), "row", ids()).unwrap();
        let mut e = enclosure();
        let before = e.clone();
        let err = e.apply_all(ops).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-003");
        assert_eq!(e, before);
    }
}
