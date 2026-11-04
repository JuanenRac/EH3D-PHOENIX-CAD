//! A parametric enclosure and the operations that change it.
//!
//! The box has an inner space of `inner_length` (X) by `inner_width` (Y) by `inner_height` (Z),
//! walls of `wall`, a floor of `floor` below and a lid of `lid` above, so the outer box is
//! `inner_length + 2 wall` by `inner_width + 2 wall` by `inner_height + floor + lid`.
//!
//! Holes are round, in one face, placed by the position of their centre measured from the lower
//! left corner of the face as seen from outside (`u` to the right, `v` up). A hole must lie inside
//! its face at least `edge_margin` from every edge and at least `min_web` from the other holes of
//! the face. A refused operation leaves the model exactly as it was.

use std::collections::BTreeMap;

use eh3d_container::ObjectId;

use crate::{CadError, Length};

/// The identity of a hole: the object identity (UUID v7) that the whole suite uses.
pub type Id = ObjectId;

/// The six faces of the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Face {
    /// The face at the smallest Y.
    Front,
    /// The face at the largest Y.
    Back,
    /// The face at the smallest X.
    Left,
    /// The face at the largest X.
    Right,
    /// The face at the largest Z (the lid).
    Top,
    /// The face at the smallest Z (the floor).
    Bottom,
}

impl Face {
    /// The six faces in their fixed order.
    pub const ALL: [Face; 6] = [
        Face::Front,
        Face::Back,
        Face::Left,
        Face::Right,
        Face::Top,
        Face::Bottom,
    ];

    /// The face on the other side of the box.
    pub fn opposite(self) -> Face {
        match self {
            Face::Front => Face::Back,
            Face::Back => Face::Front,
            Face::Left => Face::Right,
            Face::Right => Face::Left,
            Face::Top => Face::Bottom,
            Face::Bottom => Face::Top,
        }
    }

    /// The face with a name given by [`Face::name`], or `None`.
    pub fn from_name(name: &str) -> Option<Face> {
        Face::ALL.into_iter().find(|f| f.name() == name)
    }

    /// A stable lower-case name.
    pub fn name(self) -> &'static str {
        match self {
            Face::Front => "front",
            Face::Back => "back",
            Face::Left => "left",
            Face::Right => "right",
            Face::Top => "top",
            Face::Bottom => "bottom",
        }
    }
}

/// The parameters of an enclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnclosureParams {
    /// Inner size along X.
    pub inner_length: Length,
    /// Inner size along Y.
    pub inner_width: Length,
    /// Inner size along Z.
    pub inner_height: Length,
    /// Thickness of the four walls.
    pub wall: Length,
    /// Thickness of the floor.
    pub floor: Length,
    /// Thickness of the lid.
    pub lid: Length,
    /// The least distance from a hole to the edge of its face.
    pub edge_margin: Length,
    /// The least material between two holes of a face.
    pub min_web: Length,
}

impl EnclosureParams {
    fn check(&self) -> Result<(), CadError> {
        for (name, v) in [
            ("inner_length", self.inner_length),
            ("inner_width", self.inner_width),
            ("inner_height", self.inner_height),
            ("wall", self.wall),
            ("floor", self.floor),
            ("lid", self.lid),
        ] {
            if v.get() <= 0 {
                return Err(CadError::NotPositive(name));
            }
        }
        for (name, v) in [("edge_margin", self.edge_margin), ("min_web", self.min_web)] {
            if v.get() < 0 {
                return Err(CadError::NotPositive(name));
            }
        }
        // the outer sizes must stay in range
        Length::new(self.inner_length.get() + 2 * self.wall.get())?;
        Length::new(self.inner_width.get() + 2 * self.wall.get())?;
        Length::new(self.inner_height.get() + self.floor.get() + self.lid.get())?;
        Ok(())
    }

    /// The outer size along X, Y and Z.
    pub fn outer(&self) -> (Length, Length, Length) {
        let len = |v: i64| Length::new(v).expect("checked when the enclosure was made");
        (
            len(self.inner_length.get() + 2 * self.wall.get()),
            len(self.inner_width.get() + 2 * self.wall.get()),
            len(self.inner_height.get() + self.floor.get() + self.lid.get()),
        )
    }
}

/// A round hole in a face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hole {
    /// The name shown to people; an attribute, it never identifies the hole.
    pub name: String,
    /// The face.
    pub face: Face,
    /// The position of the centre to the right, from the lower left corner of the face.
    pub u: Length,
    /// The position of the centre upwards, from the lower left corner of the face.
    pub v: Length,
    /// The diameter.
    pub diameter: Length,
}

/// A structured change to an enclosure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    /// Adds a hole under an id that is not used yet.
    AddHole {
        /// The id of the new hole.
        id: Id,
        /// The hole.
        hole: Hole,
    },
    /// Removes a hole.
    RemoveHole {
        /// The id of the hole.
        id: Id,
    },
    /// Replaces a hole by another (same id).
    ReplaceHole {
        /// The id of the hole.
        id: Id,
        /// The new hole.
        hole: Hole,
    },
    /// Replaces the parameters of the enclosure. Every hole must still be valid.
    SetParams(EnclosureParams),
}

/// The enclosure: its parameters and its holes by id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enclosure {
    params: EnclosureParams,
    holes: BTreeMap<Id, Hole>,
}

impl Enclosure {
    /// A new enclosure without holes.
    pub fn new(params: EnclosureParams) -> Result<Enclosure, CadError> {
        params.check()?;
        Ok(Enclosure {
            params,
            holes: BTreeMap::new(),
        })
    }

    /// The parameters.
    pub fn params(&self) -> &EnclosureParams {
        &self.params
    }

    /// The holes by id (ordered by id, so every listing is the same).
    pub fn holes(&self) -> &BTreeMap<Id, Hole> {
        &self.holes
    }

    /// The size of a face: its width (along u) and its height (along v).
    pub fn face_size(&self, face: Face) -> (Length, Length) {
        let (x, y, z) = self.params.outer();
        match face {
            Face::Front | Face::Back => (x, z),
            Face::Left | Face::Right => (y, z),
            Face::Top | Face::Bottom => (x, y),
        }
    }

    /// The volume of material of the empty box in cubic nanometres: the outer box minus the inner
    /// space. Holes are not subtracted.
    pub fn material_volume_nm3(&self) -> i128 {
        let (x, y, z) = self.params.outer();
        let outer = i128::from(x.get()) * i128::from(y.get()) * i128::from(z.get());
        let inner = i128::from(self.params.inner_length.get())
            * i128::from(self.params.inner_width.get())
            * i128::from(self.params.inner_height.get());
        outer - inner
    }

    fn check_hole(&self, hole: &Hole, ignoring: Option<Id>) -> Result<(), CadError> {
        self.check_hole_in(&self.params, &self.holes, hole, ignoring)
    }

    fn check_hole_in(
        &self,
        params: &EnclosureParams,
        holes: &BTreeMap<Id, Hole>,
        hole: &Hole,
        ignoring: Option<Id>,
    ) -> Result<(), CadError> {
        if hole.diameter.get() <= 0 {
            return Err(CadError::NotPositive("hole diameter"));
        }
        let probe = Enclosure {
            params: *params,
            holes: BTreeMap::new(),
        };
        let (w, h) = probe.face_size(hole.face);
        let r = i128::from(hole.diameter.get());
        let m = i128::from(params.edge_margin.get());
        let (u, v) = (i128::from(hole.u.get()), i128::from(hole.v.get()));
        // 2 * (distance from the centre to an edge) >= diameter + 2 * margin
        let inside = |c: i128, size: i128| 2 * c >= r + 2 * m && 2 * (size - c) >= r + 2 * m;
        if !inside(u, i128::from(w.get())) || !inside(v, i128::from(h.get())) {
            return Err(CadError::HoleOutsideFace(hole.name.clone()));
        }
        let web = i128::from(params.min_web.get());
        for (other_id, other) in holes {
            if other.face != hole.face || Some(*other_id) == ignoring {
                continue;
            }
            let (du, dv) = (u - i128::from(other.u.get()), v - i128::from(other.v.get()));
            let need = r + i128::from(other.diameter.get()) + 2 * web;
            // 4 * distance^2 < need^2
            if 4 * (du * du + dv * dv) < need * need {
                return Err(CadError::HolesOverlap(
                    hole.name.clone(),
                    other.name.clone(),
                ));
            }
        }
        Ok(())
    }

    /// Applies an operation and returns the operation that undoes it. When the operation is
    /// refused the enclosure is exactly as it was.
    pub fn apply(&mut self, op: Operation) -> Result<Operation, CadError> {
        match op {
            Operation::AddHole { id, hole } => {
                if self.holes.contains_key(&id) {
                    return Err(CadError::HoleId(id));
                }
                self.check_hole(&hole, None)?;
                self.holes.insert(id, hole);
                Ok(Operation::RemoveHole { id })
            }
            Operation::RemoveHole { id } => match self.holes.remove(&id) {
                Some(hole) => Ok(Operation::AddHole { id, hole }),
                None => Err(CadError::HoleId(id)),
            },
            Operation::ReplaceHole { id, hole } => {
                let old = self.holes.get(&id).ok_or(CadError::HoleId(id))?.clone();
                self.check_hole(&hole, Some(id))?;
                self.holes.insert(id, hole);
                Ok(Operation::ReplaceHole { id, hole: old })
            }
            Operation::SetParams(params) => {
                params.check()?;
                // every hole must still be valid against the new parameters, one after the other
                let mut accepted: BTreeMap<Id, Hole> = BTreeMap::new();
                for (id, hole) in &self.holes {
                    self.check_hole_in(&params, &accepted, hole, None)?;
                    accepted.insert(*id, hole.clone());
                }
                let old = self.params;
                self.params = params;
                Ok(Operation::SetParams(old))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn params() -> EnclosureParams {
        EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        }
    }

    /// A stable id made from a short name, so the tests can name their holes.
    fn id_of(name: &str) -> Id {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for b in name.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        ObjectId::from_parts(1_700_000_000_000, (h & 0x0fff) as u16, h).unwrap()
    }

    fn hole(face: Face, u: i64, v: i64, d: i64) -> Hole {
        Hole {
            name: String::new(),
            face,
            u: mm(u),
            v: mm(v),
            diameter: mm(d),
        }
    }

    fn add(name: &str, mut h: Hole) -> Operation {
        h.name = name.into();
        Operation::AddHole {
            id: id_of(name),
            hole: h,
        }
    }

    #[test]
    fn the_outer_size_and_the_material_follow_from_the_parameters() {
        let e = Enclosure::new(params()).unwrap();
        let (x, y, z) = e.params().outer();
        assert_eq!(
            (x.get(), y.get(), z.get()),
            (104_000_000, 64_000_000, 35_000_000)
        );
        // 104*64*35 - 100*60*30 cubic millimetres, and one cubic millimetre is 10^18 cubic nanometres
        let mm3 = 104i128 * 64 * 35 - 100 * 60 * 30;
        assert_eq!(e.material_volume_nm3(), mm3 * 1_000_000_000_000_000_000);
        assert_eq!(e.face_size(Face::Front), (x, z));
        assert_eq!(e.face_size(Face::Left), (y, z));
        assert_eq!(e.face_size(Face::Top), (x, y));
    }

    #[test]
    fn bad_parameters_are_refused_with_the_name_of_the_parameter() {
        let mut p = params();
        p.wall = Length::ZERO;
        assert_eq!(
            Enclosure::new(p).unwrap_err(),
            CadError::NotPositive("wall")
        );
        let mut p = params();
        p.min_web = Length::new(-1).unwrap();
        assert_eq!(
            Enclosure::new(p).unwrap_err(),
            CadError::NotPositive("min_web")
        );
        let mut p = params();
        p.inner_length = Length::new((1 << 40) - 10).unwrap();
        assert!(matches!(
            Enclosure::new(p),
            Err(CadError::LengthOutOfRange(_))
        ));
    }

    #[test]
    fn holes_must_lie_inside_their_face_with_the_margin() {
        let mut e = Enclosure::new(params()).unwrap();
        // the front face is 104 x 35 mm; a 6 mm hole needs its centre 6 mm from every edge
        assert!(e.apply(add("a", hole(Face::Front, 6, 6, 6))).is_ok());
        let before = e.clone();
        assert_eq!(
            e.apply(add("b", hole(Face::Front, 5, 20, 6))).unwrap_err(),
            CadError::HoleOutsideFace("b".into())
        );
        assert_eq!(
            e.apply(add("c", hole(Face::Front, 50, 30, 6))).unwrap_err(),
            CadError::HoleOutsideFace("c".into())
        );
        assert_eq!(e, before, "a refused operation leaves the model unchanged");
        assert_eq!(
            e.apply(add("d", hole(Face::Top, 50, 20, 0))).unwrap_err(),
            CadError::NotPositive("hole diameter")
        );
    }

    #[test]
    fn holes_of_one_face_keep_their_web_and_other_faces_do_not_matter() {
        let mut e = Enclosure::new(params()).unwrap();
        e.apply(add("a", hole(Face::Front, 20, 17, 6))).unwrap();
        // centres 8 mm apart, diameters 6 + 6 and a web of 2: the least distance is 8 mm
        assert!(e.apply(add("b", hole(Face::Front, 28, 17, 6))).is_ok());
        assert_eq!(
            e.apply(add("c", hole(Face::Front, 20, 24, 6))).unwrap_err(),
            CadError::HolesOverlap("c".into(), "a".into())
        );
        // the same place on the back face is free
        assert!(e.apply(add("c", hole(Face::Back, 20, 24, 6))).is_ok());
    }

    #[test]
    fn names_are_unique_and_must_exist_to_be_removed_or_replaced() {
        let mut e = Enclosure::new(params()).unwrap();
        e.apply(add("a", hole(Face::Front, 20, 17, 6))).unwrap();
        assert_eq!(
            e.apply(add("a", hole(Face::Back, 20, 17, 6))).unwrap_err(),
            CadError::HoleId(id_of("a"))
        );
        assert_eq!(
            e.apply(Operation::RemoveHole { id: id_of("z") })
                .unwrap_err(),
            CadError::HoleId(id_of("z"))
        );
        assert_eq!(
            e.apply(Operation::ReplaceHole {
                id: id_of("z"),
                hole: hole(Face::Front, 20, 17, 6)
            })
            .unwrap_err(),
            CadError::HoleId(id_of("z"))
        );
    }

    #[test]
    fn every_operation_has_an_exact_inverse() {
        let mut e = Enclosure::new(params()).unwrap();
        let start = e.clone();
        let mut undo = Vec::new();
        undo.push(e.apply(add("a", hole(Face::Front, 20, 17, 6))).unwrap());
        undo.push(e.apply(add("b", hole(Face::Left, 30, 17, 8))).unwrap());
        // a hole may replace itself with a bigger one at the same place
        undo.push(
            e.apply(Operation::ReplaceHole {
                id: id_of("a"),
                hole: Hole {
                    name: "a".into(),
                    ..hole(Face::Front, 20, 17, 10)
                },
            })
            .unwrap(),
        );
        let mut wider = params();
        wider.inner_length = mm(120);
        undo.push(e.apply(Operation::SetParams(wider)).unwrap());
        undo.push(e.apply(Operation::RemoveHole { id: id_of("b") }).unwrap());
        assert_ne!(e, start);
        while let Some(op) = undo.pop() {
            e.apply(op).unwrap();
        }
        assert_eq!(e, start);
    }

    #[test]
    fn changing_the_parameters_cannot_leave_a_hole_outside() {
        let mut e = Enclosure::new(params()).unwrap();
        e.apply(add("a", hole(Face::Front, 98, 17, 6))).unwrap();
        let before = e.clone();
        let mut narrower = params();
        narrower.inner_length = mm(80);
        assert_eq!(
            e.apply(Operation::SetParams(narrower)).unwrap_err(),
            CadError::HoleOutsideFace("a".into())
        );
        assert_eq!(e, before);
        let mut bad = params();
        bad.lid = Length::ZERO;
        assert_eq!(
            e.apply(Operation::SetParams(bad)).unwrap_err(),
            CadError::NotPositive("lid")
        );
        assert_eq!(e, before);
    }
}
