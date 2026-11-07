// SPDX-License-Identifier: GPL-3.0-or-later
//! Simple assemblies: parts of the catalog that are placed relative to each other, and the check
//! of which of them interfere.
//!
//! A [`Mate`] places one part (`b`) relative to another (`a`) along **one** axis, so it takes away
//! one of the three coordinates of `b` and leaves the other two as they were placed. There are
//! three kinds:
//!
//! * `gap <direction> <distance>`: `b` lies on the side of `a` that the direction names (`+x`
//!   to `-z`), `distance` away from its face (0 touches it);
//! * `align <axis> <min|max|center>`: the lower faces of the two bounding boxes lie on one plane
//!   (`min`), the upper faces do (`max`) or the middles do (`center`; refused when the middle is
//!   half a nanometre off, because positions are whole nanometres);
//! * `stack`: `b` stands on `a`, the lower face of `b` on the upper face of `a` (`gap +z 0`).
//!
//! The mates are constraints, not moves: the position typed for a part stays in the model and
//! [`place`] gives every part its position with the mates applied, so taking a mate away puts the
//! part back where it was typed. A part can be placed by one mate along each axis (two mates for
//! the same part and axis are refused), a mate may depend on a part that is placed by another mate
//! (a chain is solved from its free end) and a cycle is refused.
//!
//! [`interference`] finds the pairs of parts that share volume. A pair is looked at in three steps:
//! the bounding boxes (a pair that only touches does not interfere), then the shapes made of whole
//! nanometres: a profile is a box, or four boxes for a hollow section; a bracket is two boxes
//! (one per leg); a screw is two cylinders (the head and the shank). A box meets a cylinder along
//! an axis, and two cylinders along one axis, exactly, with integers (the squares of the
//! distances, doubled so that a centre on a half nanometre is exact). Two cylinders along
//! different axes are taken as their bounding boxes. A head is a cylinder of the diameter of the
//! head for every kind of head (a hexagon is taken as its circle across the flats). Last, a screw
//! that goes through the hole of a bracket is allowed when its shank fits in the hole, with the
//! clearance of the settings: that pair is counted as a pass, not as a conflict.
//!
//! Everything is whole nanometres, and the shared volume that is reported is the one of the
//! bounding boxes of the two parts (an upper bound of the volume the shapes share).

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use eh3d_container::ObjectId;

use crate::catalog::{length_word, share_volume, Axis, PartInstance, Shape};
use crate::config::Config;
use crate::enclosure::Id;
use crate::{CadError, Length};

/// The most that the number of mates can be set to.
pub const MATES_CAP: u32 = 100_000;

/// The most that the number of pairs shown in a report can be set to.
pub const PAIRS_CAP: u32 = 100_000;

/// The most that the clearance of a hole can be set to, in nanometres (a metre).
pub const CLEARANCE_CAP_NM: i64 = 1_000_000_000;

fn mate_error(why: impl AsRef<str>) -> CadError {
    CadError::Mate(why.as_ref().to_string())
}

/// The limits of the assemblies, read from the settings file (`[assembly]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssemblyLimits {
    /// The most mates in one model.
    pub max_mates: usize,
    /// The room a shank needs in a hole, as the difference of the two diameters: a screw passes
    /// through a hole when the diameter of the hole is at least the diameter of the shank plus
    /// this.
    pub hole_clearance: Length,
    /// The most pairs a report lists.
    pub max_report_pairs: usize,
}

impl AssemblyLimits {
    /// The limits of the shipped settings.
    pub fn shipped() -> AssemblyLimits {
        crate::config::DesignLimits::shipped().assembly
    }

    /// Reads and checks the keys of `[assembly]`.
    pub fn from_config(c: &Config) -> Result<AssemblyLimits, CadError> {
        let bounded = |key: &str, cap: u32| -> Result<usize, CadError> {
            let v = c.number(key)?;
            if v == 0 || v > cap {
                return Err(CadError::Config(format!("{key} must be from 1 to {cap}")));
            }
            usize::try_from(v).map_err(|_| CadError::Config(format!("{key} is too large")))
        };
        let clearance = c.length("assembly.hole_clearance")?;
        if clearance.get() < 0 || clearance.get() > CLEARANCE_CAP_NM {
            return Err(CadError::Config(format!(
                "assembly.hole_clearance must be from 0 to {} mm",
                CLEARANCE_CAP_NM / 1_000_000
            )));
        }
        Ok(AssemblyLimits {
            max_mates: bounded("assembly.max_mates", MATES_CAP)?,
            hole_clearance: clearance,
            max_report_pairs: bounded("assembly.max_report_pairs", PAIRS_CAP)?,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Mates
// ---------------------------------------------------------------------------------------------

/// One of the three axes, without a direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dim {
    /// The `x` axis.
    X,
    /// The `y` axis.
    Y,
    /// The `z` axis.
    Z,
}

impl Dim {
    /// The three axes in a fixed order.
    pub const ALL: [Dim; 3] = [Dim::X, Dim::Y, Dim::Z];

    /// The word of the files.
    pub fn name(self) -> &'static str {
        match self {
            Dim::X => "x",
            Dim::Y => "y",
            Dim::Z => "z",
        }
    }

    /// The axis with a word of the files.
    pub fn from_name(name: &str) -> Option<Dim> {
        Dim::ALL.into_iter().find(|d| d.name() == name)
    }

    /// The place of the axis in a position: 0, 1 or 2.
    pub fn index(self) -> usize {
        match self {
            Dim::X => 0,
            Dim::Y => 1,
            Dim::Z => 2,
        }
    }
}

/// Which face of the bounding box an alignment puts on one plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// The lower faces.
    Min,
    /// The upper faces.
    Max,
    /// The middles.
    Center,
}

impl Edge {
    /// The three edges in a fixed order.
    pub const ALL: [Edge; 3] = [Edge::Min, Edge::Max, Edge::Center];

    /// The word of the files.
    pub fn name(self) -> &'static str {
        match self {
            Edge::Min => "min",
            Edge::Max => "max",
            Edge::Center => "center",
        }
    }

    /// The edge with a word of the files.
    pub fn from_name(name: &str) -> Option<Edge> {
        Edge::ALL.into_iter().find(|e| e.name() == name)
    }
}

/// What a mate asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MateKind {
    /// `b` is on the side of `a` that the direction names, `distance` from its face.
    Gap {
        /// The side of `a` that `b` lies on.
        direction: Axis,
        /// The air between the two faces, from zero up.
        distance: Length,
    },
    /// The same face of the two bounding boxes lies on one plane.
    Align {
        /// The axis across the plane.
        axis: Dim,
        /// The face that is aligned.
        edge: Edge,
    },
    /// `b` stands on `a`.
    Stack,
}

/// A part placed relative to another, along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mate {
    /// The part that stays (its position is the reference).
    pub a: Id,
    /// The part that is placed.
    pub b: Id,
    /// What is asked for.
    pub kind: MateKind,
}

/// The axis (0, 1 or 2) and the sign of a direction.
fn direction_of(axis: Axis) -> (usize, i8) {
    match axis {
        Axis::PlusX => (0, 1),
        Axis::MinusX => (0, -1),
        Axis::PlusY => (1, 1),
        Axis::MinusY => (1, -1),
        Axis::PlusZ => (2, 1),
        Axis::MinusZ => (2, -1),
    }
}

impl MateKind {
    /// The word of the files for the kind: `gap`, `align` or `stack`.
    pub fn name(self) -> &'static str {
        match self {
            MateKind::Gap { .. } => "gap",
            MateKind::Align { .. } => "align",
            MateKind::Stack => "stack",
        }
    }

    /// The axis (0 for `x`, 1 for `y`, 2 for `z`) along which the mate places `b`.
    pub fn axis_index(self) -> usize {
        match self {
            MateKind::Gap { direction, .. } => direction_of(direction).0,
            MateKind::Align { axis, .. } => axis.index(),
            MateKind::Stack => 2,
        }
    }
}

impl Mate {
    /// The words of the mate in a line of a file: the two ids and then the kind with its values.
    pub fn words(&self) -> Vec<String> {
        let mut out = vec![self.a.to_string(), self.b.to_string()];
        match self.kind {
            MateKind::Gap {
                direction,
                distance,
            } => {
                out.push("gap".to_string());
                out.push(direction.name().to_string());
                out.push(distance.get().to_string());
            }
            MateKind::Align { axis, edge } => {
                out.push("align".to_string());
                out.push(axis.name().to_string());
                out.push(edge.name().to_string());
            }
            MateKind::Stack => out.push("stack".to_string()),
        }
        out
    }

    /// The mate with the words [`Mate::words`] wrote. The words are checked for being canonical.
    pub fn from_words(words: &[&str]) -> Result<Mate, String> {
        let id = |w: &str| ObjectId::from_str(w).map_err(|_| "an id is not valid".to_string());
        let kind = match words {
            [_, _, "gap", direction, distance] => MateKind::Gap {
                direction: Axis::from_name(direction)
                    .ok_or("the direction of a gap is not known")?,
                distance: length_word(distance, "distance of a gap")?,
            },
            [_, _, "align", axis, edge] => MateKind::Align {
                axis: Dim::from_name(axis).ok_or("the axis of an alignment is not known")?,
                edge: Edge::from_name(edge).ok_or("the edge of an alignment is not known")?,
            },
            [_, _, "stack"] => MateKind::Stack,
            _ => return Err("a mate is not understood".to_string()),
        };
        Ok(Mate {
            a: id(words[0])?,
            b: id(words[1])?,
            kind,
        })
    }

    /// Whether the mate holds together by itself: it joins two different parts and a gap is not
    /// larger than the largest dimension of the settings.
    pub fn check(&self, largest: Length) -> Result<(), CadError> {
        if self.a == self.b {
            return Err(mate_error("a part cannot be placed relative to itself"));
        }
        if let MateKind::Gap { distance, .. } = self.kind {
            if distance.get() < 0 || distance.get() > largest.get() {
                return Err(mate_error(format!(
                    "the distance of a gap must be from 0 to {} nm",
                    largest.get()
                )));
            }
        }
        Ok(())
    }
}

/// The position along `axis` that `b` takes for a mate, from the lower face of `a` (`a_min`), the
/// size of `a` (`a_size`) and the size of `b` (`b_size`) along the axis.
fn mated_position(kind: MateKind, a_min: i64, a_size: i64, b_size: i64) -> Result<i64, CadError> {
    let (a_min, a_size, b_size) = (i128::from(a_min), i128::from(a_size), i128::from(b_size));
    let v = match kind {
        MateKind::Gap {
            direction,
            distance,
        } => {
            let d = i128::from(distance.get());
            if direction_of(direction).1 > 0 {
                a_min + a_size + d
            } else {
                a_min - d - b_size
            }
        }
        MateKind::Align { edge, .. } => match edge {
            Edge::Min => a_min,
            Edge::Max => a_min + a_size - b_size,
            Edge::Center => {
                let twice = 2 * a_min + a_size - b_size;
                if twice % 2 != 0 {
                    return Err(mate_error(
                        "the middles cannot be aligned: they differ by half a nanometre",
                    ));
                }
                twice / 2
            }
        },
        MateKind::Stack => a_min + a_size,
    };
    i64::try_from(v)
        .ok()
        .filter(|v| Length::new(*v).is_ok())
        .ok_or_else(|| mate_error("a part is placed outside the range of lengths"))
}

/// The parts with their mates applied: every part keeps the position typed for it along the axes
/// no mate places, and gets the position the mate gives along the others. Refused with
/// `PHOENIX-CAD-046` when a mate names a part that is not there, joins a part to itself, when a part
/// is placed twice along an axis, when mates place each other in a cycle, or when a part would lie
/// outside the range of lengths. The parts are only copied: the model is not changed.
pub fn place(
    parts: &BTreeMap<Id, PartInstance>,
    mates: &BTreeMap<Id, Mate>,
) -> Result<BTreeMap<Id, PartInstance>, CadError> {
    let mut placed = parts.clone();
    if mates.is_empty() {
        return Ok(placed);
    }
    // for each axis, the mate that places each part
    let mut by_target: [BTreeMap<Id, &Mate>; 3] =
        [BTreeMap::new(), BTreeMap::new(), BTreeMap::new()];
    for mate in mates.values() {
        let name = |id: &Id| parts.get(id).map(|p| p.name.clone());
        let (Some(a), Some(b)) = (name(&mate.a), name(&mate.b)) else {
            return Err(mate_error("a mate names a part that is not in the model"));
        };
        if mate.a == mate.b {
            return Err(mate_error(format!(
                "{a}: a part cannot be placed relative to itself"
            )));
        }
        let axis = mate.kind.axis_index();
        if by_target[axis].insert(mate.b, mate).is_some() {
            return Err(mate_error(format!(
                "{b} is placed by two mates along {}",
                Dim::ALL[axis].name()
            )));
        }
    }
    let sizes: BTreeMap<Id, [i64; 3]> = parts.iter().map(|(id, p)| (*id, p.extent().0)).collect();
    for (axis, targets) in by_target.iter().enumerate() {
        let mut done: BTreeSet<Id> = BTreeSet::new();
        for start in targets.keys() {
            // up the chain to a part that is free along this axis (or already solved), then down
            let mut chain: Vec<Id> = Vec::new();
            let mut seen: BTreeSet<Id> = BTreeSet::new();
            let mut current = *start;
            while let Some(mate) = targets.get(&current) {
                if done.contains(&current) {
                    break;
                }
                if !seen.insert(current) {
                    return Err(mate_error(format!(
                        "{}: the mates place each other in a cycle along {}",
                        parts[&current].name,
                        Dim::ALL[axis].name()
                    )));
                }
                chain.push(current);
                current = mate.a;
            }
            for b in chain.into_iter().rev() {
                let mate = targets[&b];
                let a_min = placed[&mate.a].at[axis].get();
                let value =
                    mated_position(mate.kind, a_min, sizes[&mate.a][axis], sizes[&b][axis])?;
                if let Some(p) = placed.get_mut(&b) {
                    p.at[axis] = Length::new(value)?;
                }
                done.insert(b);
            }
        }
    }
    Ok(placed)
}

// ---------------------------------------------------------------------------------------------
// The shapes of the parts, in whole nanometres
// ---------------------------------------------------------------------------------------------

/// A solid with faces on the axes, in the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Solid {
    /// A box: the smallest and the largest corner.
    Box { lo: [i64; 3], hi: [i64; 3] },
    /// A cylinder along an axis: the range along it, twice the centre across it (on the other two
    /// axes, in the order of the axes) and the diameter.
    Cylinder {
        axis: usize,
        lo: i64,
        hi: i64,
        c2: [i64; 2],
        d: i64,
    },
}

/// The hole through a box: the same description as a cylinder.
#[derive(Debug, Clone, Copy)]
struct HoleAt {
    axis: usize,
    c2: [i64; 2],
    d: i64,
}

/// A solid of a part, with its hole when it has one and a mark for the shank of a screw.
#[derive(Debug, Clone, Copy)]
struct Piece {
    solid: Solid,
    shank: bool,
    hole: Option<HoleAt>,
}

/// How the frame of a part is put in the model.
struct Frame {
    at: [i64; 3],
    cols: [[i8; 3]; 3],
    size: [i64; 3],
}

impl Frame {
    /// The axis of the model and the sign that a local axis is turned onto.
    fn target(&self, local: usize) -> (usize, i64) {
        for (i, c) in self.cols[local].iter().enumerate() {
            if *c != 0 {
                return (i, i64::from(*c));
            }
        }
        (local, 1)
    }

    /// A range along a local axis, as the axis of the model and the range there.
    fn interval(&self, local: usize, lo: i64, hi: i64) -> (usize, i64, i64) {
        let (i, sign) = self.target(local);
        if sign > 0 {
            (i, self.at[i] + lo, self.at[i] + hi)
        } else {
            (
                i,
                self.at[i] + self.size[local] - hi,
                self.at[i] + self.size[local] - lo,
            )
        }
    }

    /// A box of the local frame, in the model.
    fn boxed(&self, lo: [i64; 3], hi: [i64; 3]) -> Solid {
        let (mut glo, mut ghi) = ([0i64; 3], [0i64; 3]);
        for j in 0..3 {
            let (i, a, b) = self.interval(j, lo[j], hi[j]);
            glo[i] = a;
            ghi[i] = b;
        }
        Solid::Box { lo: glo, hi: ghi }
    }

    /// A cylinder of the local frame along the local axis `axial` between `lo` and `hi`, whose
    /// centre is given by twice its coordinate on the two other local axes.
    fn cylinder(&self, axial: usize, lo: i64, hi: i64, centre: [(usize, i64); 2], d: i64) -> Solid {
        let (axis, a, b) = self.interval(axial, lo, hi);
        let mut c2 = [0i64; 3];
        for (j, u2) in centre {
            let (i, sign) = self.target(j);
            c2[i] = if sign > 0 {
                2 * self.at[i] + u2
            } else {
                2 * self.at[i] + 2 * self.size[j] - u2
            };
        }
        let mut across = [0i64; 2];
        let mut k = 0;
        for (i, v) in c2.iter().enumerate() {
            if i != axis {
                across[k] = *v;
                k += 1;
            }
        }
        Solid::Cylinder {
            axis,
            lo: a,
            hi: b,
            c2: across,
            d,
        }
    }
}

/// The solids that make a placed part.
fn pieces(part: &PartInstance) -> Vec<Piece> {
    let frame = Frame {
        at: [part.at[0].get(), part.at[1].get(), part.at[2].get()],
        cols: part.orientation.columns(),
        size: part.part.size(part.cut),
    };
    let solid = |solid: Solid| Piece {
        solid,
        shank: false,
        hole: None,
    };
    let [w, h, l] = frame.size;
    match &part.part.shape {
        Shape::Profile { wall, .. } => {
            let t = wall.get();
            if t == 0 {
                vec![solid(frame.boxed([0, 0, 0], [w, h, l]))]
            } else {
                vec![
                    solid(frame.boxed([0, 0, 0], [w, t, l])),
                    solid(frame.boxed([0, h - t, 0], [w, h, l])),
                    solid(frame.boxed([0, t, 0], [t, h - t, l])),
                    solid(frame.boxed([w - t, t, 0], [w, h - t, l])),
                ]
            }
        }
        Shape::Screw {
            diameter,
            length,
            head_diameter,
            head_height,
            ..
        } => {
            // the head sits at the smallest local z and the shank runs up from it; both are
            // centred on the square of the bounding box, whose side is w
            let centre = [(0, w), (1, w)];
            vec![
                solid(frame.cylinder(2, 0, head_height.get(), centre, head_diameter.get())),
                Piece {
                    solid: frame.cylinder(
                        2,
                        head_height.get(),
                        head_height.get() + length.get(),
                        centre,
                        diameter.get(),
                    ),
                    shank: true,
                    hole: None,
                },
            ]
        }
        Shape::Bracket {
            leg_a,
            leg_b,
            width,
            thickness,
            hole_diameter,
        } => {
            let (a, b, wd, t, d) = (
                leg_a.get(),
                leg_b.get(),
                width.get(),
                thickness.get(),
                hole_diameter.get(),
            );
            let hole = |solid: Solid| {
                if d > 0 {
                    if let Solid::Cylinder { axis, c2, d, .. } = solid {
                        return Some(HoleAt { axis, c2, d });
                    }
                }
                None
            };
            // the hole of the first leg runs along the local z, in the middle of the free part of
            // the leg; the hole of the second leg runs along the local x
            let hole_a = frame.cylinder(2, 0, t, [(0, a + t), (1, wd)], d.max(1));
            let hole_b = frame.cylinder(0, 0, t, [(1, wd), (2, b + t)], d.max(1));
            vec![
                Piece {
                    solid: frame.boxed([0, 0, 0], [a, wd, t]),
                    shank: false,
                    hole: hole(hole_a),
                },
                Piece {
                    solid: frame.boxed([0, 0, 0], [t, wd, b]),
                    shank: false,
                    hole: hole(hole_b),
                },
            ]
        }
    }
}

/// The bounding box of a solid in twice the nanometres, so that the middle of a cylinder is whole.
fn box2(solid: &Solid) -> ([i128; 3], [i128; 3]) {
    match *solid {
        Solid::Box { lo, hi } => (lo.map(|v| 2 * i128::from(v)), hi.map(|v| 2 * i128::from(v))),
        Solid::Cylinder {
            axis,
            lo,
            hi,
            c2,
            d,
        } => {
            let (mut a, mut b) = ([0i128; 3], [0i128; 3]);
            let mut k = 0;
            for i in 0..3 {
                if i == axis {
                    a[i] = 2 * i128::from(lo);
                    b[i] = 2 * i128::from(hi);
                } else {
                    a[i] = i128::from(c2[k]) - i128::from(d);
                    b[i] = i128::from(c2[k]) + i128::from(d);
                    k += 1;
                }
            }
            (a, b)
        }
    }
}

/// The two axes other than `axis`, in order.
fn across(axis: usize) -> [usize; 2] {
    match axis {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    }
}

/// Whether two solids share volume (touching is not sharing).
fn solids_meet(a: &Solid, b: &Solid) -> bool {
    match (a, b) {
        (Solid::Box { lo: al, hi: ah }, Solid::Box { lo: bl, hi: bh }) => {
            (0..3).all(|i| al[i] < bh[i] && bl[i] < ah[i])
        }
        (
            Solid::Cylinder {
                axis,
                lo,
                hi,
                c2,
                d,
            },
            Solid::Box { lo: bl, hi: bh },
        )
        | (
            Solid::Box { lo: bl, hi: bh },
            Solid::Cylinder {
                axis,
                lo,
                hi,
                c2,
                d,
            },
        ) => {
            if !(*lo < bh[*axis] && bl[*axis] < *hi) {
                return false;
            }
            let mut sum = 0i128;
            for (k, i) in across(*axis).into_iter().enumerate() {
                let (low, high) = (2 * i128::from(bl[i]), 2 * i128::from(bh[i]));
                let c = i128::from(c2[k]);
                let outside = if c < low {
                    low - c
                } else if c > high {
                    c - high
                } else {
                    0
                };
                sum += outside * outside;
            }
            let r2 = i128::from(*d);
            sum < r2 * r2
        }
        (
            Solid::Cylinder {
                axis: aa,
                lo: al,
                hi: ah,
                c2: ac,
                d: ad,
            },
            Solid::Cylinder {
                axis: ba,
                lo: bl,
                hi: bh,
                c2: bc,
                d: bd,
            },
        ) if aa == ba => {
            if !(al < bh && bl < ah) {
                return false;
            }
            let dx = i128::from(ac[0]) - i128::from(bc[0]);
            let dy = i128::from(ac[1]) - i128::from(bc[1]);
            let reach = i128::from(*ad) + i128::from(*bd);
            dx * dx + dy * dy < reach * reach
        }
        _ => {
            let ((al, ah), (bl, bh)) = (box2(a), box2(b));
            (0..3).all(|i| al[i] < bh[i] && bl[i] < ah[i])
        }
    }
}

/// Whether the shank `s` goes through the hole of `p`: the same direction and the circle of the
/// shank inside the circle of the hole with the clearance of the settings.
fn passes_through(s: &Piece, p: &Piece, clearance: i128) -> bool {
    let (Solid::Cylinder { axis, c2, d, .. }, Some(hole)) = (&s.solid, &p.hole) else {
        return false;
    };
    if !s.shank || *axis != hole.axis {
        return false;
    }
    let room = i128::from(hole.d) - clearance - i128::from(*d);
    if room < 0 {
        return false;
    }
    let dx = i128::from(c2[0]) - i128::from(hole.c2[0]);
    let dy = i128::from(c2[1]) - i128::from(hole.c2[1]);
    dx * dx + dy * dy <= room * room
}

// ---------------------------------------------------------------------------------------------
// Interference
// ---------------------------------------------------------------------------------------------

/// A pair of parts that share volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The part with the smaller id.
    pub a: Id,
    /// The part with the larger id.
    pub b: Id,
    /// The volume the two bounding boxes share, in cubic nanometres (the shapes share at most
    /// this much).
    pub shared_nm3: i128,
}

/// What the check of the interferences found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// The pairs that share volume, by id.
    pub conflicts: Vec<Conflict>,
    /// How many pairs have bounding boxes that share volume.
    pub boxes_overlapping: usize,
    /// How many of those are screws through the holes of brackets (allowed).
    pub passes: usize,
    /// How many of those are not in conflict although the boxes overlap (the shapes are clear).
    pub clear: usize,
}

impl Report {
    /// The ids of the parts that are in at least one conflict.
    pub fn conflicting(&self) -> BTreeSet<Id> {
        self.conflicts.iter().flat_map(|c| [c.a, c.b]).collect()
    }

    /// The refusal `PHOENIX-CAD-047` that says which pairs are in conflict (up to `shown` of
    /// them), or `None` when there are none.
    pub fn refusal(&self, parts: &BTreeMap<Id, PartInstance>, shown: usize) -> Option<CadError> {
        if self.conflicts.is_empty() {
            return None;
        }
        let name = |id: &Id| {
            parts
                .get(id)
                .map_or_else(|| id.to_string(), |p| p.name.clone())
        };
        let list: Vec<String> = self
            .conflicts
            .iter()
            .take(shown)
            .map(|c| format!("{} / {}", name(&c.a), name(&c.b)))
            .collect();
        let more = self.conflicts.len().saturating_sub(shown);
        let tail = if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        };
        Some(CadError::Interference(format!(
            "{} pairs share volume: {}{tail}",
            self.conflicts.len(),
            list.join(", ")
        )))
    }
}

/// Finds the pairs of parts that share volume. The parts should be the ones that [`place`] gives.
pub fn interference(
    parts: &BTreeMap<Id, PartInstance>,
    limits: &AssemblyLimits,
) -> Result<Report, CadError> {
    let mut boxes = Vec::with_capacity(parts.len());
    for (id, part) in parts {
        boxes.push((*id, part.bounds()?));
    }
    // sorted by the smallest x, so that the pairs whose x ranges overlap are next to each other
    boxes.sort_by_key(|(id, (lo, _))| (lo[0], *id));
    let shapes: BTreeMap<Id, Vec<Piece>> = parts.iter().map(|(id, p)| (*id, pieces(p))).collect();
    let clearance = i128::from(limits.hole_clearance.get());
    let mut report = Report::default();
    for (i, (ia, ba)) in boxes.iter().enumerate() {
        for (ib, bb) in &boxes[i + 1..] {
            if bb.0[0] >= ba.1[0] {
                break;
            }
            if !share_volume(ba, bb) {
                continue;
            }
            report.boxes_overlapping += 1;
            let mut passed = false;
            let mut meet = false;
            'pair: for pa in &shapes[ia] {
                for pb in &shapes[ib] {
                    if !solids_meet(&pa.solid, &pb.solid) {
                        continue;
                    }
                    if passes_through(pa, pb, clearance) || passes_through(pb, pa, clearance) {
                        passed = true;
                    } else {
                        meet = true;
                        break 'pair;
                    }
                }
            }
            if meet {
                let shared: i128 = (0..3)
                    .map(|k| i128::from(ba.1[k].min(bb.1[k]) - ba.0[k].max(bb.0[k])))
                    .product();
                let (a, b) = if ia < ib { (*ia, *ib) } else { (*ib, *ia) };
                report.conflicts.push(Conflict {
                    a,
                    b,
                    shared_nm3: shared,
                });
            } else if passed {
                report.passes += 1;
            } else {
                report.clear += 1;
            }
        }
    }
    report.conflicts.sort_by_key(|c| (c.a, c.b));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Head, Orientation, Part};

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn um(v: i64) -> Length {
        Length::from_um(v).unwrap()
    }

    fn id(n: u64) -> Id {
        Id::from_parts(1_700_000_000_000, 1, n).unwrap()
    }

    fn limits() -> AssemblyLimits {
        AssemblyLimits::shipped()
    }

    fn profile(name: &str, w: Length, h: Length, wall: Length) -> Part {
        Part {
            name: name.into(),
            material: "aluminium".into(),
            shape: Shape::Profile {
                width: w,
                height: h,
                wall,
                default_cut: mm(100),
                max_cut: mm(2000),
            },
        }
    }

    fn at(
        name: &str,
        part: Part,
        pos: [Length; 3],
        orientation: Orientation,
        cut: Option<Length>,
    ) -> PartInstance {
        PartInstance {
            name: name.into(),
            library: "lib".into(),
            part,
            at: pos,
            orientation,
            cut,
        }
    }

    fn bar(name: &str, pos: [i64; 3], cut: i64) -> PartInstance {
        at(
            name,
            profile("bar-20x30", mm(20), mm(30), Length::ZERO),
            pos.map(mm),
            Orientation::default(),
            Some(mm(cut)),
        )
    }

    fn set(list: Vec<(u64, PartInstance)>) -> BTreeMap<Id, PartInstance> {
        list.into_iter().map(|(n, p)| (id(n), p)).collect()
    }

    fn mates(list: Vec<(u64, Mate)>) -> BTreeMap<Id, Mate> {
        list.into_iter().map(|(n, m)| (id(n), m)).collect()
    }

    fn gap(a: u64, b: u64, direction: Axis, distance: Length) -> Mate {
        Mate {
            a: id(a),
            b: id(b),
            kind: MateKind::Gap {
                direction,
                distance,
            },
        }
    }

    fn align(a: u64, b: u64, axis: Dim, edge: Edge) -> Mate {
        Mate {
            a: id(a),
            b: id(b),
            kind: MateKind::Align { axis, edge },
        }
    }

    fn pos(placed: &BTreeMap<Id, PartInstance>, n: u64) -> [i64; 3] {
        let p = &placed[&id(n)];
        [p.at[0].get(), p.at[1].get(), p.at[2].get()]
    }

    const MM: i64 = 1_000_000;

    #[test]
    fn a_gap_a_stack_and_an_alignment_place_a_part_along_one_axis() {
        // a: 20 x 30 x 100 at (5, 6, 7); b: 20 x 30 x 40 typed at (100, 100, 100)
        let parts = set(vec![
            (1, bar("a", [5, 6, 7], 100)),
            (2, bar("b", [100, 100, 100], 40)),
        ]);
        let run = |m: Mate| {
            let placed = place(&parts, &mates(vec![(9, m)])).unwrap();
            pos(&placed, 2)
        };
        // 100 mm of a start at z = 7, so its upper face is at 107 mm; the other two coordinates stay
        assert_eq!(
            run(gap(1, 2, Axis::PlusZ, mm(5))),
            [100 * MM, 100 * MM, 112 * MM]
        );
        assert_eq!(
            run(Mate {
                a: id(1),
                b: id(2),
                kind: MateKind::Stack
            }),
            [100 * MM, 100 * MM, 107 * MM]
        );
        // on the side of smaller x: b ends 3 mm before the face of a at x = 5, so it starts at 5 - 3 - 20
        assert_eq!(
            run(gap(1, 2, Axis::MinusX, mm(3))),
            [-18 * MM, 100 * MM, 100 * MM]
        );
        // on the side of larger y: the face of a is at 36 mm
        assert_eq!(
            run(gap(1, 2, Axis::PlusY, Length::ZERO)),
            [100 * MM, 36 * MM, 100 * MM]
        );
        // the lower faces on one plane, the upper faces (b is 40 against 100 long), the middles
        assert_eq!(
            run(align(1, 2, Dim::Z, Edge::Min)),
            [100 * MM, 100 * MM, 7 * MM]
        );
        assert_eq!(
            run(align(1, 2, Dim::Z, Edge::Max)),
            [100 * MM, 100 * MM, 67 * MM]
        );
        assert_eq!(
            run(align(1, 2, Dim::Z, Edge::Center)),
            [100 * MM, 100 * MM, 37 * MM]
        );
        assert_eq!(
            run(align(1, 2, Dim::X, Edge::Center)),
            [5 * MM, 100 * MM, 100 * MM]
        );
        // a part with no mate is left where it was typed
        let placed = place(
            &parts,
            &mates(vec![(9, gap(1, 2, Axis::PlusZ, Length::ZERO))]),
        )
        .unwrap();
        assert_eq!(placed[&id(1)], parts[&id(1)]);
    }

    #[test]
    fn two_mates_on_different_axes_place_a_part_in_two_coordinates() {
        let parts = set(vec![
            (1, bar("a", [0, 0, 0], 100)),
            (2, bar("b", [50, 50, 50], 100)),
        ]);
        let placed = place(
            &parts,
            &mates(vec![
                (1, gap(1, 2, Axis::PlusX, mm(10))),
                (2, align(1, 2, Dim::Y, Edge::Max)),
            ]),
        )
        .unwrap();
        // x: 20 + 10; y: the upper faces are both at 30 mm and the parts are equally wide; z as typed
        assert_eq!(pos(&placed, 2), [30 * MM, 0, 50 * MM]);
    }

    #[test]
    fn a_chain_is_solved_from_its_free_end_whatever_the_order_of_the_ids() {
        // c is placed on b, which is placed on a: the ids run the other way
        let parts = set(vec![
            (3, bar("a", [0, 0, 0], 100)),
            (2, bar("b", [9, 9, 9], 40)),
            (1, bar("c", [8, 8, 8], 10)),
        ]);
        let chain = mates(vec![
            (1, gap(2, 1, Axis::PlusZ, mm(1))),
            (2, gap(3, 2, Axis::PlusZ, mm(2))),
        ]);
        let placed = place(&parts, &chain).unwrap();
        // b: 100 + 2 = 102; c: 102 + 40 + 1 = 143
        assert_eq!(pos(&placed, 2)[2], 102 * MM);
        assert_eq!(pos(&placed, 1)[2], 143 * MM);
        // the typed positions are kept in the model
        assert_eq!(parts[&id(1)].at[2], mm(8));
        // taking the mate that places b away puts b back where it was typed, and c follows it
        let placed = place(&parts, &mates(vec![(1, gap(2, 1, Axis::PlusZ, mm(1)))])).unwrap();
        assert_eq!(pos(&placed, 2)[2], 9 * MM);
        assert_eq!(pos(&placed, 1)[2], 50 * MM);
    }

    #[test]
    fn the_middles_of_two_parts_that_differ_by_half_a_nanometre_are_refused() {
        let odd = at(
            "odd",
            profile(
                "odd",
                Length::new(20_000_001).unwrap(),
                mm(30),
                Length::ZERO,
            ),
            [Length::ZERO; 3],
            Orientation::default(),
            Some(mm(100)),
        );
        let parts = set(vec![(1, bar("a", [0, 0, 0], 100)), (2, odd)]);
        let e = place(&parts, &mates(vec![(1, align(1, 2, Dim::X, Edge::Center))])).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-046");
        assert!(e.to_string().contains("half a nanometre"), "{e}");
        // the lower faces are fine, and so is the middle along an axis where the sizes agree
        place(&parts, &mates(vec![(1, align(1, 2, Dim::X, Edge::Min))])).unwrap();
        place(&parts, &mates(vec![(1, align(1, 2, Dim::Y, Edge::Center))])).unwrap();
    }

    #[test]
    fn mates_that_cannot_be_solved_are_refused_with_the_code_of_the_assembly() {
        let parts = set(vec![
            (1, bar("a", [0, 0, 0], 100)),
            (2, bar("b", [0, 0, 0], 100)),
            (3, bar("c", [0, 0, 0], 100)),
        ]);
        let code = |m: Vec<(u64, Mate)>| place(&parts, &mates(m)).unwrap_err();
        // a part that is not there, and a part joined to itself
        let e = code(vec![(1, gap(1, 7, Axis::PlusX, Length::ZERO))]);
        assert_eq!(e.code(), "PHOENIX-CAD-046");
        assert!(e.to_string().contains("not in the model"), "{e}");
        assert!(code(vec![(1, gap(2, 2, Axis::PlusX, Length::ZERO))])
            .to_string()
            .contains("relative to itself"));
        // the same part placed twice along one axis, even by two kinds
        let e = code(vec![
            (1, gap(1, 3, Axis::PlusX, Length::ZERO)),
            (2, align(2, 3, Dim::X, Edge::Min)),
        ]);
        assert!(e.to_string().contains("two mates along x"), "{e}");
        // but along two axes it is fine
        place(
            &parts,
            &mates(vec![
                (1, gap(1, 3, Axis::PlusX, Length::ZERO)),
                (2, gap(2, 3, Axis::PlusY, Length::ZERO)),
            ]),
        )
        .unwrap();
        // a cycle of two and a cycle of three
        let e = code(vec![
            (1, gap(1, 2, Axis::PlusZ, Length::ZERO)),
            (2, gap(2, 1, Axis::PlusZ, Length::ZERO)),
        ]);
        assert!(e.to_string().contains("in a cycle along z"), "{e}");
        let e = code(vec![
            (1, gap(1, 2, Axis::PlusZ, Length::ZERO)),
            (2, gap(2, 3, Axis::PlusZ, Length::ZERO)),
            (3, gap(3, 1, Axis::MinusZ, Length::ZERO)),
        ]);
        assert!(e.to_string().contains("in a cycle"), "{e}");
        // a position outside the range of lengths
        let far = set(vec![
            (1, bar("a", [0, 0, 0], 100)),
            (2, bar("b", [0, 0, 0], 100)),
        ]);
        let mut huge = far.clone();
        huge.get_mut(&id(1)).unwrap().at[2] = Length::new((1 << 40) - 2 * MM).unwrap();
        let e = place(&huge, &mates(vec![(1, gap(1, 2, Axis::PlusZ, mm(1)))])).unwrap_err();
        assert!(e.to_string().contains("range of lengths"), "{e}");
    }

    #[test]
    fn a_mate_is_written_in_one_form_and_read_again() {
        let list = [
            (gap(1, 2, Axis::MinusY, mm(5)), "gap -y 5000000"),
            (gap(1, 2, Axis::PlusZ, Length::ZERO), "gap +z 0"),
            (align(1, 2, Dim::X, Edge::Center), "align x center"),
            (align(1, 2, Dim::Z, Edge::Max), "align z max"),
            (
                Mate {
                    a: id(1),
                    b: id(2),
                    kind: MateKind::Stack,
                },
                "stack",
            ),
        ];
        for (mate, tail) in list {
            let words = mate.words();
            assert_eq!(words.join(" "), format!("{} {} {tail}", id(1), id(2)));
            let refs: Vec<&str> = words.iter().map(String::as_str).collect();
            assert_eq!(Mate::from_words(&refs).unwrap(), mate);
            assert_eq!(mate.kind.name(), tail.split(' ').next().unwrap());
        }
        let (a, b) = (id(1).to_string(), id(2).to_string());
        for bad in [
            vec![a.as_str(), b.as_str()],
            vec![a.as_str(), b.as_str(), "glue"],
            vec![a.as_str(), b.as_str(), "stack", "now"],
            vec![a.as_str(), b.as_str(), "gap", "+q", "0"],
            vec![a.as_str(), b.as_str(), "gap", "+x"],
            vec![a.as_str(), b.as_str(), "gap", "+x", "-1"],
            vec![a.as_str(), b.as_str(), "gap", "+x", "05"],
            vec![a.as_str(), b.as_str(), "gap", "+x", "1.5"],
            vec![a.as_str(), b.as_str(), "gap", "x", "1"],
            vec![a.as_str(), b.as_str(), "align", "w", "min"],
            vec![a.as_str(), b.as_str(), "align", "x", "middle"],
            vec![a.as_str(), "nope", "stack"],
        ] {
            assert!(Mate::from_words(&bad).is_err(), "{bad:?}");
        }
        // a mate on its own: the two parts differ and a gap is within the largest dimension
        assert!(gap(1, 1, Axis::PlusX, Length::ZERO).check(mm(100)).is_err());
        assert!(gap(1, 2, Axis::PlusX, mm(100)).check(mm(100)).is_ok());
        assert!(gap(1, 2, Axis::PlusX, mm(101)).check(mm(100)).is_err());
    }

    #[test]
    fn the_axis_a_mate_places_follows_its_kind() {
        assert_eq!(gap(1, 2, Axis::MinusX, Length::ZERO).kind.axis_index(), 0);
        assert_eq!(gap(1, 2, Axis::PlusY, Length::ZERO).kind.axis_index(), 1);
        assert_eq!(gap(1, 2, Axis::MinusZ, Length::ZERO).kind.axis_index(), 2);
        assert_eq!(align(1, 2, Dim::Y, Edge::Min).kind.axis_index(), 1);
        assert_eq!(MateKind::Stack.axis_index(), 2);
        for dim in Dim::ALL {
            assert_eq!(Dim::from_name(dim.name()), Some(dim));
        }
        for edge in Edge::ALL {
            assert_eq!(Edge::from_name(edge.name()), Some(edge));
        }
    }

    #[test]
    fn the_limits_of_the_assembly_come_from_the_settings() {
        let l = limits();
        assert_eq!(l.max_mates, 1000);
        assert_eq!(l.hole_clearance, Length::ZERO);
        assert_eq!(l.max_report_pairs, 100);
        let s = crate::config::Settings::with_user_config(
            "[assembly]\nmax_mates = 5\nhole_clearance = 0.2 mm\nmax_report_pairs = 7\n",
        )
        .unwrap();
        assert_eq!(s.design.assembly.max_mates, 5);
        assert_eq!(s.design.assembly.hole_clearance, um(200));
        assert_eq!(s.design.assembly.max_report_pairs, 7);
        for text in [
            "[assembly]\nmax_mates = 0\n",
            "[assembly]\nmax_mates = 100001\n",
            "[assembly]\nmax_mates = many\n",
            "[assembly]\nmax_report_pairs = 0\n",
            "[assembly]\nmax_report_pairs = 100001\n",
            "[assembly]\nhole_clearance = -1 mm\n",
            "[assembly]\nhole_clearance = 1001 mm\n",
            "[assembly]\nhole_clearance = wide\n",
        ] {
            let e = crate::config::Settings::with_user_config(text).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-011", "{text:?}: {e}");
        }
        crate::config::Settings::with_user_config(
            "[assembly]\nmax_mates = 100000\nhole_clearance = 1000 mm\nmax_report_pairs = 100000\n",
        )
        .unwrap();
    }

    #[test]
    fn parts_that_only_touch_do_not_interfere_and_a_millimetre_of_overlap_does() {
        // 20 x 30 x 100 at the origin, and a second one that starts where the first ends along x
        let touching = set(vec![
            (1, bar("a", [0, 0, 0], 100)),
            (2, bar("b", [20, 0, 0], 100)),
            (3, bar("c", [0, 30, 0], 100)),
            (4, bar("d", [0, 0, 100], 100)),
            (5, bar("e", [20, 30, 100], 100)),
        ]);
        let r = interference(&touching, &limits()).unwrap();
        assert!(r.conflicts.is_empty());
        assert_eq!((r.boxes_overlapping, r.passes, r.clear), (0, 0, 0));
        assert!(r.refusal(&touching, 5).is_none());
        // one millimetre of overlap along x: 1 x 30 x 100 = 3000 mm3
        let over = set(vec![
            (1, bar("a", [0, 0, 0], 100)),
            (2, bar("b", [19, 0, 0], 100)),
        ]);
        let r = interference(&over, &limits()).unwrap();
        assert_eq!(r.conflicts.len(), 1);
        assert_eq!((r.conflicts[0].a, r.conflicts[0].b), (id(1), id(2)));
        assert_eq!(r.conflicts[0].shared_nm3, 3_000_000_000_000_000_000_000);
        assert_eq!(r.conflicts[0].shared_nm3 / 1_000_000_000_000_000_000, 3000);
        assert_eq!(r.boxes_overlapping, 1);
        assert_eq!(r.conflicting(), BTreeSet::from([id(1), id(2)]));
        // one nanometre of overlap counts, and the shared volume is exact
        let mut tiny = over.clone();
        tiny.get_mut(&id(2)).unwrap().at[0] = Length::new(20 * MM - 1).unwrap();
        let r = interference(&tiny, &limits()).unwrap();
        assert_eq!(
            r.conflicts[0].shared_nm3,
            30 * MM as i128 * 100 * MM as i128
        );
    }

    #[test]
    fn the_report_names_the_pairs_and_says_how_many_it_left_out() {
        let parts = set(vec![
            (1, bar("one", [0, 0, 0], 100)),
            (2, bar("two", [10, 0, 0], 100)),
            (3, bar("three", [5, 0, 0], 100)),
        ]);
        let r = interference(&parts, &limits()).unwrap();
        // every pair overlaps: (1,2), (1,3) and (2,3), in the order of the ids
        let pairs: Vec<(Id, Id)> = r.conflicts.iter().map(|c| (c.a, c.b)).collect();
        assert_eq!(pairs, vec![(id(1), id(2)), (id(1), id(3)), (id(2), id(3))]);
        let e = r.refusal(&parts, 2).unwrap();
        assert_eq!(e.code(), "PHOENIX-CAD-047");
        assert!(
            e.to_string()
                .ends_with(": 3 pairs share volume: one / two, one / three and 1 more"),
            "{e}"
        );
        let all = r.refusal(&parts, 3).unwrap().to_string();
        assert!(
            all.ends_with("one / two, one / three, two / three"),
            "{all}"
        );
    }

    #[test]
    fn a_hollow_section_has_room_inside_and_its_walls_count() {
        // a tube of 40 x 40 with a wall of 4 over 100 mm, at the origin; the cavity is x and y from 4 to 36
        let tube = at(
            "tube",
            profile("tube", mm(40), mm(40), mm(4)),
            [Length::ZERO; 3],
            Orientation::default(),
            Some(mm(100)),
        );
        // a bar of 10 x 10 inside the cavity, with a margin all round
        let inner = at(
            "inner",
            profile("small", mm(10), mm(10), Length::ZERO),
            [mm(15), mm(15), mm(10)],
            Orientation::default(),
            Some(mm(50)),
        );
        let parts = set(vec![(1, tube.clone()), (2, inner)]);
        let r = interference(&parts, &limits()).unwrap();
        assert!(r.conflicts.is_empty());
        assert_eq!((r.boxes_overlapping, r.clear), (1, 1));
        // the same bar moved so that it reaches one millimetre into the right wall (x from 27 to 37)
        let mut wall = parts.clone();
        wall.get_mut(&id(2)).unwrap().at[0] = mm(27);
        let r = interference(&wall, &limits()).unwrap();
        assert_eq!(r.conflicts.len(), 1);
        // the bar lies wholly inside the bounding box of the tube: 10 x 10 x 50 mm shared
        assert_eq!(
            r.conflicts[0].shared_nm3,
            10 * MM as i128 * 10 * MM as i128 * 50 * MM as i128
        );
        // the bar that lies exactly in the cavity, touching the wall, is clear
        let mut touching = parts.clone();
        touching.get_mut(&id(2)).unwrap().at[0] = mm(26);
        assert!(interference(&touching, &limits())
            .unwrap()
            .conflicts
            .is_empty());
        // and one nanometre further in is a conflict
        touching.get_mut(&id(2)).unwrap().at[0] = Length::new(26 * MM + 1).unwrap();
        assert_eq!(
            interference(&touching, &limits()).unwrap().conflicts.len(),
            1
        );
    }

    #[test]
    fn a_turned_profile_is_met_where_it_really_is() {
        // 20 x 30 section along +x (roll 0) fills x 0..100, y 0..20, z 0..30 for a cut of 100;
        // along +y it fills x 0..30, y 0..100, z 0..20 (checked in the tests of the catalog)
        let along_x = at(
            "x",
            profile("bar-20x30", mm(20), mm(30), Length::ZERO),
            [Length::ZERO; 3],
            Orientation {
                axis: Axis::PlusX,
                roll: 0,
            },
            Some(mm(100)),
        );
        let along_y = at(
            "y",
            profile("bar-20x30", mm(20), mm(30), Length::ZERO),
            [mm(50), Length::ZERO, Length::ZERO],
            Orientation {
                axis: Axis::PlusY,
                roll: 0,
            },
            Some(mm(100)),
        );
        // x: 0..100 meets 50..80; y: 0..20 meets 0..100; z: 0..30 meets 0..20 -> a conflict
        let parts = set(vec![(1, along_x.clone()), (2, along_y.clone())]);
        let r = interference(&parts, &limits()).unwrap();
        assert_eq!(r.conflicts.len(), 1);
        // 30 x 20 x 20 mm of boxes shared
        assert_eq!(
            r.conflicts[0].shared_nm3,
            30 * MM as i128 * 20 * MM as i128 * 20 * MM as i128
        );
        // the second one lifted by 30 mm stands on the first one: they touch
        let mut lifted = parts.clone();
        lifted.get_mut(&id(2)).unwrap().at[2] = mm(30);
        assert!(interference(&lifted, &limits())
            .unwrap()
            .conflicts
            .is_empty());
    }

    fn screw(name: &str, pos: [Length; 3], axis: Axis, shank: Length) -> PartInstance {
        at(
            name,
            Part {
                name: "bolt".into(),
                material: "steel".into(),
                shape: Shape::Screw {
                    metric: "M3".into(),
                    diameter: shank,
                    length: mm(10),
                    head: Head::Cylinder,
                    head_diameter: um(5500),
                    head_height: mm(3),
                },
            },
            pos,
            Orientation { axis, roll: 0 },
            None,
        )
    }

    fn bracket(hole: Length) -> PartInstance {
        at(
            "angle",
            Part {
                name: "angle-30".into(),
                material: "aluminium".into(),
                shape: Shape::Bracket {
                    leg_a: mm(30),
                    leg_b: mm(30),
                    width: mm(20),
                    thickness: mm(3),
                    hole_diameter: hole,
                },
            },
            [Length::ZERO; 3],
            Orientation::default(),
            None,
        )
    }

    /// A screw of 3 mm, 5.5 mm head, pointing down (`-z`) so that the head is on top, with its axis
    /// over the middle of the hole of the first leg (16.5 mm, 10 mm) and its head resting on the
    /// leg (the leg ends at z = 3 mm and the head is 3 mm tall, so the smallest z of the screw is -7 mm).
    fn screw_in_the_first_leg(dx: i64) -> PartInstance {
        screw(
            "screw",
            [
                Length::new(13_750_000 + dx).unwrap(),
                Length::new(7_250_000).unwrap(),
                mm(-7),
            ],
            Axis::MinusZ,
            mm(3),
        )
    }

    #[test]
    fn a_screw_through_the_hole_of_a_bracket_is_a_pass_and_not_a_conflict() {
        // the hole is 4.5 mm and the shank 3 mm, centred; the head rests on the leg
        let parts = set(vec![(1, bracket(um(4500))), (2, screw_in_the_first_leg(0))]);
        let r = interference(&parts, &limits()).unwrap();
        assert!(r.conflicts.is_empty());
        assert_eq!((r.boxes_overlapping, r.passes, r.clear), (1, 1, 0));
        // 0.75 mm off the middle still fits in the hole (0.75 + 1.5 = 2.25 = the radius of the hole)
        let off = set(vec![
            (1, bracket(um(4500))),
            (2, screw_in_the_first_leg(750_000)),
        ]);
        assert_eq!(interference(&off, &limits()).unwrap().passes, 1);
        // one nanometre more does not fit
        let off = set(vec![
            (1, bracket(um(4500))),
            (2, screw_in_the_first_leg(750_001)),
        ]);
        let r = interference(&off, &limits()).unwrap();
        assert_eq!(r.conflicts.len(), 1);
        assert_eq!(r.passes, 0);
        // a hole as wide as the shank fits it with no clearance, and 2.9 mm does not
        let tight = set(vec![(1, bracket(mm(3))), (2, screw_in_the_first_leg(0))]);
        assert_eq!(interference(&tight, &limits()).unwrap().passes, 1);
        let small = set(vec![(1, bracket(um(2900))), (2, screw_in_the_first_leg(0))]);
        assert_eq!(interference(&small, &limits()).unwrap().conflicts.len(), 1);
        // a bracket without a hole is in the way of the shank
        let none = set(vec![
            (1, bracket(Length::ZERO)),
            (2, screw_in_the_first_leg(0)),
        ]);
        assert_eq!(interference(&none, &limits()).unwrap().conflicts.len(), 1);
    }

    #[test]
    fn the_clearance_of_the_settings_makes_the_hole_smaller() {
        let parts = set(vec![(1, bracket(um(3100))), (2, screw_in_the_first_leg(0))]);
        // 3.1 against 3 + 0.1: fits
        let mut l = limits();
        l.hole_clearance = um(100);
        assert_eq!(interference(&parts, &l).unwrap().passes, 1);
        // 3.1 against 3 + 0.2: does not
        l.hole_clearance = um(200);
        assert_eq!(interference(&parts, &l).unwrap().conflicts.len(), 1);
    }

    #[test]
    fn a_screw_in_the_second_leg_and_a_screw_that_is_too_long_are_told_apart() {
        // the hole of the second leg runs along x at y = 10 mm and z = 16.5 mm; a screw along +x has
        // its head at the smallest x (-3 to 0 mm) and its shank at 0 to 10 mm, through the leg
        let side = screw(
            "side",
            [
                mm(-3),
                Length::new(7_250_000).unwrap(),
                Length::new(13_750_000).unwrap(),
            ],
            Axis::PlusX,
            mm(3),
        );
        let parts = set(vec![(1, bracket(um(4500))), (2, side.clone())]);
        let r = interference(&parts, &limits()).unwrap();
        assert!(r.conflicts.is_empty(), "{:?}", r.conflicts);
        assert_eq!(r.passes, 1);
        // moved down by 15 mm the shank meets the first leg (z 0 to 3) and no longer fits the hole
        let mut low = side;
        low.at[2] = Length::new(-1_250_000).unwrap();
        let parts = set(vec![(1, bracket(um(4500))), (2, low)]);
        let r = interference(&parts, &limits()).unwrap();
        assert_eq!(r.conflicts.len(), 1);
    }

    #[test]
    fn a_round_head_is_not_a_box_at_the_corner_of_a_profile() {
        // a bar of 20 x 30 x 100 mm at the origin and, above its end, a screw along z whose head
        // (radius 2.75 mm, z 90 to 93) lies in the height of the bar; the corner is at (20, 30)
        let bar = bar("bar", [0, 0, 0], 100);
        let screw_at = |x: i64, y: i64| {
            screw(
                "s",
                [
                    Length::new(x - 2_750_000).unwrap(),
                    Length::new(y - 2_750_000).unwrap(),
                    mm(90),
                ],
                Axis::PlusZ,
                mm(3),
            )
        };
        let run = |x: i64, y: i64| {
            interference(&set(vec![(1, bar.clone()), (2, screw_at(x, y))]), &limits()).unwrap()
        };
        // the axis at (21, 31): the corner is 1.414 mm away, inside the radius
        assert_eq!(run(21 * MM, 31 * MM).conflicts.len(), 1);
        // the axis at (22.2, 32.2): the corner is 3.111 mm away, outside the radius, although the
        // bounding boxes (the head reaches x = 19.45) overlap
        let r = run(22_200_000, 32_200_000);
        assert!(r.conflicts.is_empty());
        assert_eq!((r.boxes_overlapping, r.clear), (1, 1));
        // along the diagonal the head reaches the corner when the axis is 1.9445436 mm from it on
        // both axes (2.75 / sqrt 2 = 1.944544...): 1944543 nm meets, 1944544 nm only comes near
        assert_eq!(
            run(20 * MM + 1_944_543, 30 * MM + 1_944_543)
                .conflicts
                .len(),
            1
        );
        let r = run(20 * MM + 1_944_544, 30 * MM + 1_944_544);
        assert!(r.conflicts.is_empty());
        assert_eq!(r.clear, 1);
    }

    #[test]
    fn two_screws_along_the_same_axis_meet_when_their_circles_do() {
        // two 3 mm shanks (5.5 mm heads) side by side: the heads touch at 5.5 mm between the axes
        let a = screw("a", [Length::ZERO; 3], Axis::PlusZ, mm(3));
        let mut b = screw("b", [Length::ZERO; 3], Axis::PlusZ, mm(3));
        b.at[0] = um(5500);
        assert!(
            interference(&set(vec![(1, a.clone()), (2, b.clone())]), &limits())
                .unwrap()
                .conflicts
                .is_empty()
        );
        b.at[0] = Length::new(5_499_999).unwrap();
        let r = interference(&set(vec![(1, a.clone()), (2, b)]), &limits()).unwrap();
        assert_eq!(r.conflicts.len(), 1);
        // a screw across the other one (along x, through the height of the shank of the first) is
        // taken as its bounding box
        let mut across = screw("c", [Length::ZERO; 3], Axis::PlusX, mm(3));
        across.at = [mm(-5), mm(1), mm(8)];
        let r = interference(&set(vec![(1, a), (2, across)]), &limits()).unwrap();
        assert_eq!(r.conflicts.len(), 1);
    }

    #[test]
    fn the_sweep_finds_the_same_pairs_as_looking_at_all_of_them() {
        // forty bars of 20 x 30 x 50 mm at places given by a fixed sequence
        let mut state = 12_345u64;
        let mut next = |m: u64| -> i64 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 33) % m) as i64
        };
        let mut list = Vec::new();
        for n in 1..=40u64 {
            list.push((
                n,
                bar(&format!("p{n}"), [next(120), next(120), next(120)], 50),
            ));
        }
        let parts = set(list);
        let r = interference(&parts, &limits()).unwrap();
        let mut naive = Vec::new();
        for (ia, a) in &parts {
            for (ib, b) in &parts {
                if ia < ib && share_volume(&a.bounds().unwrap(), &b.bounds().unwrap()) {
                    naive.push((*ia, *ib));
                }
            }
        }
        let found: Vec<(Id, Id)> = r.conflicts.iter().map(|c| (c.a, c.b)).collect();
        assert_eq!(found, naive);
        assert_eq!(r.boxes_overlapping, naive.len());
        assert!(naive.len() > 5, "{}", naive.len());
    }

    #[test]
    fn the_parts_with_the_mates_applied_are_the_ones_that_are_checked() {
        // b is typed on top of a, and a mate puts it 5 mm to the side of a: no conflict
        let parts = set(vec![
            (1, bar("a", [0, 0, 0], 100)),
            (2, bar("b", [0, 0, 0], 100)),
        ]);
        assert_eq!(interference(&parts, &limits()).unwrap().conflicts.len(), 1);
        let placed = place(&parts, &mates(vec![(1, gap(1, 2, Axis::PlusX, mm(5)))])).unwrap();
        assert!(interference(&placed, &limits())
            .unwrap()
            .conflicts
            .is_empty());
        // a gap of -0 is a touch; the alignment of the middles of two bars of the same size puts
        // one over the other
        let over = place(&parts, &mates(vec![(1, align(1, 2, Dim::X, Edge::Center))])).unwrap();
        assert_eq!(interference(&over, &limits()).unwrap().conflicts.len(), 1);
    }
}
