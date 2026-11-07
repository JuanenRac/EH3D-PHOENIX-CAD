//! Patterns of holes (a row, a grid, a circle) and batches of operations that apply all or nothing.
//!
//! A pattern only computes positions; [`holes_of_pattern`] turns them into `AddHole` operations
//! with ids that the caller provides, and [`Enclosure::apply_all`] applies a list of operations
//! so that either every one is applied or the enclosure stays exactly as it was. The positions of
//! a circle use the integer sine and cosine of [`crate::units::Angle`], so they are the same on
//! every platform.
//!
//! A [`PatternOp`] is the same idea as one step of the history (`Operation::AddPattern`): a hole
//! or a post (the seed, which is also the first position) repeated along a line or round a
//! centre. The positions are worked out from the seed and the layout, each one becomes a hole or
//! a post of its own with an id derived from the id of the pattern ([`instance_id`]), and each is
//! checked by the enclosure like a hole or a post added by hand: the margins to the edges, the
//! distance to the others (the earlier positions of the same pattern among them), the walls, the
//! lid and the rest. Either every position is accepted or the enclosure stays as it was. Editing
//! the seed or the layout and building the recipe again gives the new positions: the pattern is
//! regenerated, never edited position by position.

use crate::enclosure::{Enclosure, Face, Hole, Id, Operation};
use crate::mount::Post;
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
                style: crate::enclosure::HoleStyle::Plain,
            },
        })
        .collect())
}

/// What a pattern repeats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seed {
    /// A hole in a face. Its position is the first position of the pattern.
    Hole(Hole),
    /// A post on the floor. Its position is the first position of the pattern.
    Post(Post),
}

/// How the positions of a pattern follow from its seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// `count` positions in a line: each one `step` after the previous (along `u` and `v` of a
    /// face, along `x` and `y` on the floor).
    Linear {
        /// The offset from one position to the next.
        step: Position,
        /// How many positions, the seed among them.
        count: Count,
    },
    /// `count` positions round `center`: the seed turned about it by `step` more each time,
    /// counterclockwise as seen from outside the face (from above on the floor).
    Circular {
        /// The centre of the turn, in the same frame as the seed.
        center: Position,
        /// The angle between two positions.
        step: Angle,
        /// How many positions, the seed among them.
        count: Count,
    },
}

impl Layout {
    /// How many positions.
    pub fn count(&self) -> Count {
        match *self {
            Layout::Linear { count, .. } | Layout::Circular { count, .. } => count,
        }
    }
}

/// A pattern as one step of the history: a seed and the layout that repeats it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternOp {
    /// The hole or post that is repeated; its position is the first one.
    pub seed: Seed,
    /// How it is repeated.
    pub layout: Layout,
}

/// The id of the position `index` of the pattern with id `base`: the id of the pattern itself for
/// the first position and, for the others, an id with the same time and a counter and random part
/// that follow from `base` and `index` alone, so the same recipe gives the same ids on every
/// system and nothing has to be stored.
pub fn instance_id(base: Id, index: u32) -> Id {
    if index == 0 {
        return base;
    }
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in base.as_bytes().iter().copied().chain(index.to_be_bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let counter = ((u32::from(base.counter()) + index) & 0x0fff) as u16;
    // the time of a valid id fits in 48 bits, so building an id with it cannot fail
    Id::from_parts(base.unix_ms(), counter, h).unwrap_or(base)
}

impl PatternOp {
    /// The name of the seed.
    pub fn name(&self) -> &str {
        match &self.seed {
            Seed::Hole(h) => &h.name,
            Seed::Post(p) => &p.name,
        }
    }

    /// The position of the seed, which is the first position.
    pub fn seed_position(&self) -> Position {
        match &self.seed {
            Seed::Hole(h) => (h.u, h.v),
            Seed::Post(p) => (p.x, p.y),
        }
    }

    /// The positions in order, for a pattern that may have at most `max` of them. A pattern with
    /// fewer than two positions, with more than `max`, with a step of nothing or turned about
    /// its own seed is refused (`PHOENIX-CAD-042`) before any position is worked out.
    pub fn positions(&self, max: u32) -> Result<Vec<Position>, CadError> {
        let n = self.layout.count().get();
        if n < 2 {
            return Err(CadError::Pattern(
                "a pattern needs at least two positions".into(),
            ));
        }
        if n > max {
            return Err(CadError::Pattern(format!(
                "{n} positions are more than the {max} that the settings allow"
            )));
        }
        let start = self.seed_position();
        match self.layout {
            Layout::Linear { step, count } => {
                if step == (Length::ZERO, Length::ZERO) {
                    return Err(CadError::Pattern(
                        "the step between positions is nothing".into(),
                    ));
                }
                Pattern::Linear { start, step, count }.positions()
            }
            Layout::Circular {
                center,
                step,
                count,
            } => {
                if step == Angle::ZERO {
                    return Err(CadError::Pattern(
                        "the angle between positions is nothing".into(),
                    ));
                }
                if start == center {
                    return Err(CadError::Pattern(
                        "the seed is at the centre of the turn".into(),
                    ));
                }
                let mut out = Vec::with_capacity(count.get() as usize);
                for k in 0..count.get() {
                    let turn = step
                        .get()
                        .checked_mul(i64::from(k))
                        .ok_or(CadError::Overflow("pattern"))?;
                    out.push(rotated(start, center, Angle::new(turn)?)?);
                }
                Ok(out)
            }
        }
    }

    /// The operations that add the hole or the post of each position, named `"<name> 1"`,
    /// `"<name> 2"`, ... under the ids of [`instance_id`]. A post made for a hole of a board does
    /// not keep that link: the positions are not that hole.
    pub fn operations(&self, id: Id, max: u32) -> Result<Vec<Operation>, CadError> {
        let positions = self.positions(max)?;
        Ok(positions
            .into_iter()
            .enumerate()
            .map(|(i, (a, b))| {
                let index = u32::try_from(i).unwrap_or(u32::MAX);
                let name = format!("{} {}", self.name(), i + 1);
                match &self.seed {
                    Seed::Hole(seed) => Operation::AddHole {
                        id: instance_id(id, index),
                        hole: Hole {
                            name,
                            u: a,
                            v: b,
                            ..seed.clone()
                        },
                    },
                    Seed::Post(seed) => Operation::AddPost {
                        id: instance_id(id, index),
                        post: Post {
                            name,
                            board_hole: None,
                            x: a,
                            y: b,
                            ..seed.clone()
                        },
                    },
                }
            })
            .collect())
    }

    /// The ids of the positions of the pattern with id `id`.
    pub fn instance_ids(&self, id: Id) -> Vec<Id> {
        (0..self.layout.count().get().min(COUNT_LIMIT))
            .map(|k| instance_id(id, k))
            .collect()
    }
}

/// The point `seed` turned about `center` by `turn`, counterclockwise, with the integer sine and
/// cosine: each of the four products is rounded to the nanometre, half away from zero.
fn rotated(seed: Position, center: Position, turn: Angle) -> Result<Position, CadError> {
    let dx = seed.0.checked_sub(center.0)?;
    let dy = seed.1.checked_sub(center.1)?;
    let (c, s) = (turn.cos(), turn.sin());
    let du = scale(dx, c, 1_000_000_000)?.checked_sub(scale(dy, s, 1_000_000_000)?)?;
    let dv = scale(dx, s, 1_000_000_000)?.checked_add(scale(dy, c, 1_000_000_000)?)?;
    Ok((center.0.checked_add(du)?, center.1.checked_add(dv)?))
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

    // ---- the pattern as one step of the history ------------------------------------------

    fn seed_hole(u: i64, v: i64) -> Hole {
        Hole {
            name: "vent".into(),
            face: Face::Top,
            u: mm(u),
            v: mm(v),
            diameter: mm(4),
            style: crate::enclosure::HoleStyle::Plain,
        }
    }

    fn seed_post(x: i64, y: i64) -> Post {
        Post {
            name: "pillar".into(),
            board_hole: None,
            x: mm(x),
            y: mm(y),
            outer_diameter: mm(8),
            height: mm(6),
            kind: crate::mount::BoreKind::Insert,
            bore_diameter: Length::new(4_200_000).unwrap(),
            bore_depth: mm(5),
            base: None,
        }
    }

    fn row(count: i64) -> PatternOp {
        PatternOp {
            seed: Seed::Hole(seed_hole(20, 20)),
            layout: Layout::Linear {
                step: (mm(10), Length::ZERO),
                count: count_of(count),
            },
        }
    }

    fn count_of(n: i64) -> Count {
        Count::new(n).unwrap()
    }

    fn pid() -> Id {
        ObjectId::from_parts(1_700_000_000_000, 0x123, 0x5555).unwrap()
    }

    #[test]
    fn a_linear_pattern_starts_at_the_seed_and_steps_along() {
        let p = PatternOp {
            seed: Seed::Hole(seed_hole(20, 12)),
            layout: Layout::Linear {
                step: (mm(10), mm(5)),
                count: count_of(4),
            },
        };
        let pos: Vec<(i64, i64)> = p
            .positions(256)
            .unwrap()
            .iter()
            .map(|(u, v)| (u.get() / 1_000_000, v.get() / 1_000_000))
            .collect();
        assert_eq!(pos, vec![(20, 12), (30, 17), (40, 22), (50, 27)]);
    }

    #[test]
    fn a_circular_pattern_turns_the_seed_about_the_centre() {
        // centre (50, 30), seed (60, 30), 90 degrees: (60,30) (50,40) (40,30) (50,20)
        let p = PatternOp {
            seed: Seed::Hole(seed_hole(60, 30)),
            layout: Layout::Circular {
                center: (mm(50), mm(30)),
                step: Angle::from_deg(90).unwrap(),
                count: count_of(4),
            },
        };
        let pos: Vec<(i64, i64)> = p
            .positions(256)
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
    fn a_circular_pattern_keeps_the_radius_when_the_seed_is_not_on_an_axis() {
        // centre (50, 30), seed (56, 38): the offset (6, 8) has length 10; turned by 60 degrees
        // the offset is (6 * 0.5 - 8 * 0.866025404, 6 * 0.866025404 + 8 * 0.5) =
        // (-3.928203232, 9.196152424) mm, each product rounded to the nanometre
        let p = PatternOp {
            seed: Seed::Hole(seed_hole(56, 38)),
            layout: Layout::Circular {
                center: (mm(50), mm(30)),
                step: Angle::from_deg(60).unwrap(),
                count: count_of(3),
            },
        };
        let pos = p.positions(256).unwrap();
        assert_eq!(pos[0], (mm(56), mm(38)));
        assert_eq!(pos[1].0.get(), 50_000_000 - 3_928_203);
        assert_eq!(pos[1].1.get(), 30_000_000 + 9_196_152);
        // the offsets of the second and the third position are 10 mm long to within a nanometre
        for (u, v) in &pos {
            let (du, dv) = (
                i128::from(u.get() - 50_000_000),
                i128::from(v.get() - 30_000_000),
            );
            let r2 = du * du + dv * dv;
            assert!((r2 - 100_000_000_000_000).abs() < 100_000_000, "{r2}");
        }
    }

    #[test]
    fn the_ids_of_the_positions_follow_from_the_id_of_the_pattern() {
        let base = pid();
        assert_eq!(instance_id(base, 0), base);
        let ids: Vec<Id> = (0..64).map(|k| instance_id(base, k)).collect();
        let distinct: std::collections::BTreeSet<_> = ids.iter().collect();
        assert_eq!(distinct.len(), 64);
        assert_eq!(
            ids,
            (0..64).map(|k| instance_id(base, k)).collect::<Vec<_>>()
        );
        // the same time as the pattern, and valid ids that read back from their text
        for id in &ids {
            assert_eq!(id.unix_ms(), 1_700_000_000_000);
            assert_eq!(id.to_string().parse::<Id>().unwrap(), *id);
        }
        assert_ne!(
            instance_id(base, 1),
            instance_id(
                ObjectId::from_parts(1_700_000_000_000, 0x124, 0x5555).unwrap(),
                1
            )
        );
    }

    #[test]
    fn a_pattern_that_is_not_one_is_refused_before_anything_is_worked_out() {
        for (p, why) in [
            (row(1), "at least two"),
            (row(0), "at least two"),
            (row(257), "more than the 256"),
            (
                PatternOp {
                    seed: Seed::Hole(seed_hole(20, 20)),
                    layout: Layout::Linear {
                        step: (Length::ZERO, Length::ZERO),
                        count: count_of(3),
                    },
                },
                "step between positions is nothing",
            ),
            (
                PatternOp {
                    seed: Seed::Hole(seed_hole(20, 20)),
                    layout: Layout::Circular {
                        center: (mm(30), mm(20)),
                        step: Angle::ZERO,
                        count: count_of(3),
                    },
                },
                "angle between positions is nothing",
            ),
            (
                PatternOp {
                    seed: Seed::Hole(seed_hole(20, 20)),
                    layout: Layout::Circular {
                        center: (mm(20), mm(20)),
                        step: Angle::from_deg(90).unwrap(),
                        count: count_of(3),
                    },
                },
                "seed is at the centre",
            ),
        ] {
            let e = p.positions(256).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-042", "{e}");
            assert!(e.to_string().contains(why), "{e}");
        }
        // 256 positions are allowed with the limit of 256, 257 with 300
        assert_eq!(row(256).positions(256).unwrap().len(), 256);
        assert_eq!(row(257).positions(300).unwrap().len(), 257);
    }

    #[test]
    fn a_row_of_vents_is_one_operation_and_its_inverse_takes_all_of_them_out() {
        let mut e = enclosure();
        let before = e.clone();
        let p = PatternOp {
            seed: Seed::Hole(seed_hole(20, 30)),
            layout: Layout::Linear {
                step: (mm(10), Length::ZERO),
                count: count_of(6),
            },
        };
        let undo = e
            .apply(Operation::AddPattern {
                id: pid(),
                pattern: p.clone(),
            })
            .unwrap();
        assert_eq!(undo, Operation::RemovePattern { id: pid() });
        assert_eq!(e.holes().len(), 6);
        assert_eq!(e.patterns().len(), 1);
        let names: Vec<String> = e.holes().values().map(|h| h.name.clone()).collect();
        for k in 1..=6 {
            assert!(names.contains(&format!("vent {k}")), "{names:?}");
        }
        let redo = e.apply(undo).unwrap();
        assert_eq!(e, before);
        assert_eq!(
            redo,
            Operation::AddPattern {
                id: pid(),
                pattern: p
            }
        );
        // and the redo gives the same enclosure as the first time
        e.apply(redo).unwrap();
        assert_eq!(e.holes().len(), 6);
    }

    #[test]
    fn a_position_that_is_refused_stops_the_whole_pattern() {
        let mut e = enclosure();
        // the face is 104 mm wide: the sixth position, at 100 mm, is within the 3 mm margin
        let p = PatternOp {
            seed: Seed::Hole(seed_hole(50, 30)),
            layout: Layout::Linear {
                step: (mm(10), Length::ZERO),
                count: count_of(6),
            },
        };
        let before = e.clone();
        let err = e
            .apply(Operation::AddPattern {
                id: pid(),
                pattern: p,
            })
            .unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-003");
        assert!(err.to_string().contains("vent 6"), "{err}");
        assert_eq!(e, before);
        assert!(e.patterns().is_empty());
    }

    #[test]
    fn positions_too_close_to_each_other_are_refused_with_the_name_of_the_second() {
        let mut e = enclosure();
        // 4 mm holes 5 mm apart leave 1 mm of web, and the least is 2 mm
        let p = PatternOp {
            seed: Seed::Hole(seed_hole(20, 30)),
            layout: Layout::Linear {
                step: (mm(5), Length::ZERO),
                count: count_of(3),
            },
        };
        let err = e
            .apply(Operation::AddPattern {
                id: pid(),
                pattern: p.clone(),
            })
            .unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-004");
        assert!(e.holes().is_empty());
        // the least web is exactly met at 6 mm
        let ok = PatternOp {
            layout: Layout::Linear {
                step: (mm(6), Length::ZERO),
                count: count_of(3),
            },
            ..p
        };
        e.apply(Operation::AddPattern {
            id: pid(),
            pattern: ok,
        })
        .unwrap();
        assert_eq!(e.holes().len(), 3);
    }

    #[test]
    fn a_new_pattern_is_checked_against_the_holes_that_are_already_there() {
        let mut e = enclosure();
        e.apply(Operation::AddHole {
            id: ids()(),
            hole: Hole {
                name: "old".into(),
                ..seed_hole(40, 30)
            },
        })
        .unwrap();
        let before = e.clone();
        let err = e
            .apply(Operation::AddPattern {
                id: pid(),
                pattern: PatternOp {
                    seed: Seed::Hole(seed_hole(20, 30)),
                    layout: Layout::Linear {
                        step: (mm(10), Length::ZERO),
                        count: count_of(4),
                    },
                },
            })
            .unwrap_err();
        // the third position, at 40 mm, is on top of the old hole
        assert_eq!(err.code(), "PHOENIX-CAD-004");
        assert_eq!(e, before);
    }

    #[test]
    fn a_position_of_a_pattern_is_not_changed_alone() {
        let mut e = enclosure();
        e.apply(Operation::AddPattern {
            id: pid(),
            pattern: row(3),
        })
        .unwrap();
        let first = pid();
        let second = instance_id(pid(), 1);
        assert_eq!(e.pattern_of(&second), Some(first));
        let before = e.clone();
        for op in [
            Operation::RemoveHole { id: second },
            Operation::ReplaceHole {
                id: second,
                hole: seed_hole(40, 40),
            },
            Operation::RemoveHole { id: first },
        ] {
            let err = e.apply(op).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-042", "{err}");
        }
        assert_eq!(e, before);
        // the same id again is an id used twice
        assert_eq!(
            e.apply(Operation::AddPattern {
                id: pid(),
                pattern: row(3)
            })
            .unwrap_err()
            .code(),
            "PHOENIX-CAD-005"
        );
        // a hole of the same id as a position is refused too
        assert!(e
            .apply(Operation::AddHole {
                id: second,
                hole: seed_hole(60, 40)
            })
            .is_err());
        assert_eq!(e, before);
        assert_eq!(
            e.apply(Operation::RemovePattern { id: second })
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-005"
        );
    }

    #[test]
    fn a_circle_of_posts_is_a_pattern_too_and_each_post_is_checked_against_the_walls() {
        let mut e = enclosure();
        // the inner space is 100 by 60: posts of 8 mm at 12 mm from the centre (50, 30), four of them
        let ring = |radius: i64| PatternOp {
            seed: Seed::Post(seed_post(50 + radius, 30)),
            layout: Layout::Circular {
                center: (mm(50), mm(30)),
                step: Angle::from_deg(90).unwrap(),
                count: count_of(4),
            },
        };
        let undo = e
            .apply(Operation::AddPattern {
                id: pid(),
                pattern: ring(12),
            })
            .unwrap();
        assert_eq!(e.posts().len(), 4);
        let mut names: Vec<String> = e.posts().values().map(|p| p.name.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["pillar 1", "pillar 2", "pillar 3", "pillar 4"]);
        // the second post is 12 mm above the centre
        let second = e.posts().get(&instance_id(pid(), 1)).unwrap();
        assert_eq!((second.x, second.y), (mm(50), mm(42)));
        e.apply(undo).unwrap();
        assert!(e.posts().is_empty());
        // at 28 mm the post on the left would be 22 mm from the left side and the one above
        // 58 mm up, past the wall at 60 mm less half the post
        let before = e.clone();
        let err = e
            .apply(Operation::AddPattern {
                id: pid(),
                pattern: ring(28),
            })
            .unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-028");
        assert_eq!(e, before);
        // a ring so small that neighbours overlap: 8 mm posts 7.07 mm apart at a radius of 5 mm
        let err = e
            .apply(Operation::AddPattern {
                id: pid(),
                pattern: ring(5),
            })
            .unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-028");
        assert_eq!(e, before);
    }

    #[test]
    fn the_limit_of_the_settings_is_the_one_the_enclosure_uses() {
        let mut e = enclosure();
        e.set_design_limits(crate::config::DesignLimits {
            pattern_max: 4,
            ..crate::config::DesignLimits::shipped()
        });
        let err = e
            .apply(Operation::AddPattern {
                id: pid(),
                pattern: row(5),
            })
            .unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-042");
        assert!(err.to_string().contains("more than the 4"), "{err}");
        e.apply(Operation::AddPattern {
            id: pid(),
            pattern: row(4),
        })
        .unwrap();
        assert_eq!(e.holes().len(), 4);
    }

    #[test]
    fn a_pattern_follows_the_box_when_the_parameters_change() {
        let mut e = enclosure();
        e.apply(Operation::AddPattern {
            id: pid(),
            pattern: row(5),
        })
        .unwrap();
        // a face 8 mm narrower puts the last position (at 60 mm) past the margin: refused
        let mut narrow = *e.params();
        narrow.inner_length = mm(52);
        let before = e.clone();
        assert_eq!(
            e.apply(Operation::SetParams(narrow)).unwrap_err().code(),
            "PHOENIX-CAD-003"
        );
        assert_eq!(e, before);
        narrow.inner_length = mm(80);
        e.apply(Operation::SetParams(narrow)).unwrap();
    }
}
