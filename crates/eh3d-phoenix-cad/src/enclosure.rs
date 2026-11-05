//! A parametric enclosure and the operations that change it.
//!
//! The box has an inner space of `inner_length` (X) by `inner_width` (Y) by `inner_height` (Z),
//! walls of `wall`, a floor of `floor` below and a lid of `lid` above, so the outer box is
//! `inner_length + 2 wall` by `inner_width + 2 wall` by `inner_height + floor + lid`.
//!
//! Holes are in one face, placed by the position of their centre measured from the lower
//! left corner of the face as seen from outside (`u` to the right, `v` up). A hole is round unless
//! its [`HoleStyle`] says otherwise (countersunk, counterbored, rectangular or oval). A hole must
//! lie inside its face at least `edge_margin` from every edge and at least `min_web` from the other
//! holes of the face. A refused operation leaves the model exactly as it was.

use std::collections::BTreeMap;

use eh3d_container::ObjectId;

use crate::mount::{lid_holes_of, Lid, Post};
use crate::units::isqrt_u128;
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

    /// The faces that meet this one at its four sides, in the order: the side at the smallest `u`,
    /// the side at the largest `u`, the side at the smallest `v` and the side at the largest `v`
    /// (`u` to the right and `v` up as seen from outside).
    pub fn neighbours(self) -> [Face; 4] {
        match self {
            Face::Front => [Face::Left, Face::Right, Face::Bottom, Face::Top],
            Face::Back => [Face::Right, Face::Left, Face::Bottom, Face::Top],
            Face::Left => [Face::Back, Face::Front, Face::Bottom, Face::Top],
            Face::Right => [Face::Front, Face::Back, Face::Bottom, Face::Top],
            Face::Top => [Face::Left, Face::Right, Face::Front, Face::Back],
            Face::Bottom => [Face::Left, Face::Right, Face::Back, Face::Front],
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

/// What is done to the edges of the box that form a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Treatment {
    /// The edge is left sharp.
    #[default]
    Sharp,
    /// The edge is rounded with a quarter of a circle of this radius.
    Round(Length),
    /// The edge is cut flat; the cut is this far from the edge along both faces.
    Chamfer(Length),
}

impl Treatment {
    /// The radius or the size of the cut; zero for a sharp edge.
    pub fn size(self) -> Length {
        match self {
            Treatment::Sharp => Length::ZERO,
            Treatment::Round(s) | Treatment::Chamfer(s) => s,
        }
    }

    /// Whether the edge is left sharp.
    pub fn is_sharp(self) -> bool {
        self == Treatment::Sharp
    }

    /// Whether a point of the inner space that is `a` and `b` away from the two faces of an edge
    /// stays inside the material once the edge is treated.
    fn keeps_material(self, a: i128, b: i128) -> bool {
        match self {
            Treatment::Sharp => true,
            Treatment::Chamfer(s) => a + b > i128::from(s.get()),
            Treatment::Round(s) => {
                let r = i128::from(s.get());
                a >= r || b >= r || (r - a) * (r - a) + (r - b) * (r - b) < r * r
            }
        }
    }
}

/// The treatment of the three groups of edges of the box: the four vertical edges, the four edges
/// around the lid and the four edges around the floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Edging {
    /// The four edges along Z (between two side faces).
    pub vertical: Treatment,
    /// The four edges between the lid and the side faces.
    pub top: Treatment,
    /// The four edges between the floor and the side faces.
    pub bottom: Treatment,
}

impl Edging {
    /// The treatment of the edge between two faces (which must not be opposite faces).
    pub fn treatment_of(&self, a: Face, b: Face) -> Treatment {
        if a == Face::Top || b == Face::Top {
            self.top
        } else if a == Face::Bottom || b == Face::Bottom {
            self.bottom
        } else {
            self.vertical
        }
    }

    /// Whether every edge is left sharp.
    pub fn is_sharp(&self) -> bool {
        self.vertical.is_sharp() && self.top.is_sharp() && self.bottom.is_sharp()
    }

    /// Checks the treatments against the box: every size is positive, no treatment is larger than
    /// half of the box, the treatments of the lid and of the floor fit in its height, and none of
    /// them reaches the inner space.
    pub fn check(&self, p: &EnclosureParams) -> Result<(), CadError> {
        let (x, y, z) = p.outer();
        let plan = i128::from(x.get().min(y.get()));
        let (wall, floor, lid) = (
            i128::from(p.wall.get()),
            i128::from(p.floor.get()),
            i128::from(p.lid.get()),
        );
        for (what, t, a, b) in [
            ("vertical edges", self.vertical, wall, wall),
            ("edges of the lid", self.top, wall, lid),
            ("edges of the floor", self.bottom, wall, floor),
        ] {
            if t.is_sharp() {
                continue;
            }
            if t.size().get() <= 0 {
                return Err(CadError::NotPositive("edge treatment"));
            }
            if 2 * i128::from(t.size().get()) > plan {
                return Err(CadError::Edging(format!(
                    "{what}: larger than half the box"
                )));
            }
            if !t.keeps_material(a, b) {
                return Err(CadError::Edging(format!("{what}: reach the inner space")));
            }
        }
        if i128::from(self.top.size().get()) + i128::from(self.bottom.size().get())
            > i128::from(z.get())
        {
            return Err(CadError::Edging(
                "lid and floor: higher than the box".into(),
            ));
        }
        Ok(())
    }
}

/// The size of a face of a box with these parameters: its width (along u) and its height (along v).
fn face_size_of(params: &EnclosureParams, face: Face) -> (Length, Length) {
    let (x, y, z) = params.outer();
    match face {
        Face::Front | Face::Back => (x, z),
        Face::Left | Face::Right => (y, z),
        Face::Top | Face::Bottom => (x, y),
    }
}

/// The shape of a hole. The size across of the hole is [`Hole::diameter`]: the diameter of a round
/// hole, the width (along `u`) of a rectangular hole and the width of an oval one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HoleStyle {
    /// A straight round hole.
    #[default]
    Plain,
    /// A round hole with a conical countersink of 90 degrees at the outer face for the head of a
    /// flat screw: the cone starts at this diameter on the face and ends at the diameter of the hole.
    Countersunk {
        /// The diameter of the cone on the outer face.
        head_diameter: Length,
    },
    /// A round hole with a flat-bottomed cylindrical recess at the outer face for a socket head.
    Counterbored {
        /// The diameter of the recess.
        head_diameter: Length,
        /// The depth of the recess, which must stay inside the material.
        depth: Length,
    },
    /// A rectangular hole whose width along `u` is the diameter of the hole.
    Rectangular {
        /// The size along `v`.
        height: Length,
    },
    /// A slot with round ends whose width is the diameter of the hole.
    Oval {
        /// The whole length of the slot, which is not less than its width.
        length: Length,
        /// Whether the length runs along `v` instead of `u`.
        vertical: bool,
    },
}

impl HoleStyle {
    /// Whether the opening on the outer face is a circle.
    pub fn is_round(&self) -> bool {
        matches!(
            self,
            HoleStyle::Plain | HoleStyle::Countersunk { .. } | HoleStyle::Counterbored { .. }
        )
    }

    /// The size along `u` and along `v` of the smallest rectangle that holds the opening on the
    /// outer face, for a hole of this diameter.
    pub fn footprint(&self, diameter: Length) -> (i128, i128) {
        let d = i128::from(diameter.get());
        match self {
            HoleStyle::Plain => (d, d),
            HoleStyle::Countersunk { head_diameter }
            | HoleStyle::Counterbored { head_diameter, .. } => {
                let h = i128::from(head_diameter.get());
                (h, h)
            }
            HoleStyle::Rectangular { height } => (d, i128::from(height.get())),
            HoleStyle::Oval { length, vertical } => {
                let l = i128::from(length.get());
                if *vertical {
                    (d, l)
                } else {
                    (l, d)
                }
            }
        }
    }

    /// Checks the style against the diameter of the hole and the thickness of the material that
    /// it crosses.
    pub fn check(&self, diameter: Length, thickness: Length) -> Result<(), CadError> {
        let (d, t) = (i128::from(diameter.get()), i128::from(thickness.get()));
        match self {
            HoleStyle::Plain => Ok(()),
            HoleStyle::Countersunk { head_diameter } => {
                let h = i128::from(head_diameter.get());
                if h <= d {
                    return Err(CadError::HoleStyle(
                        "the countersink is not wider than the hole".into(),
                    ));
                }
                // the cone is (h - d) / 2 deep and must leave material below it
                if h - d >= 2 * t {
                    return Err(CadError::HoleStyle(
                        "the countersink is deeper than the material".into(),
                    ));
                }
                Ok(())
            }
            HoleStyle::Counterbored {
                head_diameter,
                depth,
            } => {
                if depth.get() <= 0 {
                    return Err(CadError::NotPositive("counterbore depth"));
                }
                if head_diameter.get() <= diameter.get() {
                    return Err(CadError::HoleStyle(
                        "the counterbore is not wider than the hole".into(),
                    ));
                }
                if i128::from(depth.get()) >= t {
                    return Err(CadError::HoleStyle(
                        "the counterbore is as deep as the material".into(),
                    ));
                }
                Ok(())
            }
            HoleStyle::Rectangular { height } => {
                if height.get() <= 0 {
                    return Err(CadError::NotPositive("rectangular hole height"));
                }
                Ok(())
            }
            HoleStyle::Oval { length, .. } => {
                if length.get() < diameter.get() {
                    return Err(CadError::HoleStyle(
                        "the slot is shorter than it is wide".into(),
                    ));
                }
                Ok(())
            }
        }
    }

    /// The same style with every size across made larger by `by` (the compensation of a printer
    /// widens a hole in every direction); depths are not changed.
    pub fn grown(&self, by: Length) -> Result<HoleStyle, CadError> {
        Ok(match *self {
            HoleStyle::Plain => HoleStyle::Plain,
            HoleStyle::Countersunk { head_diameter } => HoleStyle::Countersunk {
                head_diameter: head_diameter.checked_add(by)?,
            },
            HoleStyle::Counterbored {
                head_diameter,
                depth,
            } => HoleStyle::Counterbored {
                head_diameter: head_diameter.checked_add(by)?,
                depth,
            },
            HoleStyle::Rectangular { height } => HoleStyle::Rectangular {
                height: height.checked_add(by)?,
            },
            HoleStyle::Oval { length, vertical } => HoleStyle::Oval {
                length: length.checked_add(by)?,
                vertical,
            },
        })
    }
}

/// The thickness of the material that a hole of this face crosses where the face is thin: the
/// wall for the four sides, the lid for the top and the floor for the bottom.
fn thickness_of(params: &EnclosureParams, face: Face) -> Length {
    match face {
        Face::Front | Face::Back | Face::Left | Face::Right => params.wall,
        Face::Top => params.lid,
        Face::Bottom => params.floor,
    }
}

/// A hole in a face: round unless its style says otherwise.
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
    /// The diameter; for a rectangular hole or a slot, its width.
    pub diameter: Length,
    /// The shape.
    pub style: HoleStyle,
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
    /// Replaces the treatment of the edges. It must fit the box and every hole must still be valid.
    SetEdging(Edging),
    /// Adds a post under an id that no post has yet.
    AddPost {
        /// The id of the new post.
        id: Id,
        /// The post.
        post: Post,
    },
    /// Removes a post.
    RemovePost {
        /// The id of the post.
        id: Id,
    },
    /// Replaces a post by another (same id).
    ReplacePost {
        /// The id of the post.
        id: Id,
        /// The new post.
        post: Post,
    },
    /// Sets how the lid is fixed, or takes the fixing away (`None`).
    SetLid(Option<Lid>),
}

/// The enclosure: its parameters, its holes by id, its posts by id and the fixing of its lid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enclosure {
    params: EnclosureParams,
    edging: Edging,
    holes: BTreeMap<Id, Hole>,
    posts: BTreeMap<Id, Post>,
    lid: Option<Lid>,
}

impl Enclosure {
    /// A new enclosure without holes.
    pub fn new(params: EnclosureParams) -> Result<Enclosure, CadError> {
        params.check()?;
        Ok(Enclosure {
            params,
            edging: Edging::default(),
            holes: BTreeMap::new(),
            posts: BTreeMap::new(),
            lid: None,
        })
    }

    /// The parameters.
    pub fn params(&self) -> &EnclosureParams {
        &self.params
    }

    /// The treatment of the edges.
    pub fn edging(&self) -> &Edging {
        &self.edging
    }

    /// The holes by id (ordered by id, so every listing is the same).
    pub fn holes(&self) -> &BTreeMap<Id, Hole> {
        &self.holes
    }

    /// The posts by id (ordered by id, so every listing is the same).
    pub fn posts(&self) -> &BTreeMap<Id, Post> {
        &self.posts
    }

    /// The fixing of the lid, if there is one.
    pub fn lid(&self) -> Option<&Lid> {
        self.lid.as_ref()
    }

    /// Whether the enclosure has posts or a fixing of the lid.
    pub fn has_mounting(&self) -> bool {
        !self.posts.is_empty() || self.lid.is_some()
    }

    /// The holes that the fixing of the lid has over the posts or at the corners, in the top face.
    /// They are derived every time and never stored.
    pub fn lid_holes(&self) -> Vec<Hole> {
        self.derived_lid_holes().unwrap_or_default()
    }

    /// The holes of the faces and then the holes of the lid fixing.
    pub fn all_holes(&self) -> Vec<Hole> {
        let mut all: Vec<Hole> = self.holes.values().cloned().collect();
        all.extend(self.lid_holes());
        all
    }

    fn derived_lid_holes(&self) -> Result<Vec<Hole>, CadError> {
        match &self.lid {
            Some(lid) => lid_holes_of(
                self.params.wall,
                self.params.inner_length,
                self.params.inner_width,
                &self.posts,
                lid,
            ),
            None => Ok(Vec::new()),
        }
    }

    /// The size of a face: its width (along u) and its height (along v).
    pub fn face_size(&self, face: Face) -> (Length, Length) {
        face_size_of(&self.params, face)
    }

    /// The volume of material of the empty box in cubic nanometres: the outer box minus the inner
    /// space. Holes and the treatment of the edges are not subtracted.
    pub fn material_volume_nm3(&self) -> i128 {
        let (x, y, z) = self.params.outer();
        let outer = i128::from(x.get()) * i128::from(y.get()) * i128::from(z.get());
        let inner = i128::from(self.params.inner_length.get())
            * i128::from(self.params.inner_width.get())
            * i128::from(self.params.inner_height.get());
        outer - inner
    }

    fn check_hole(&self, hole: &Hole, ignoring: Option<Id>) -> Result<(), CadError> {
        let derived = self.derived_lid_holes()?;
        Self::check_hole_in(
            &self.params,
            &self.edging,
            &self.holes,
            &derived,
            hole,
            ignoring,
        )
    }

    /// Everything against the given parameters and edging: the holes one after the other, then the
    /// holes of the lid fixing, the posts and the lid.
    fn check_all(&self, params: &EnclosureParams, edging: &Edging) -> Result<(), CadError> {
        Self::check_state(params, edging, &self.holes, &self.posts, self.lid.as_ref())
    }

    /// The whole state: every hole against the others, the holes that the lid fixing derives
    /// against the holes and each other, every post against the walls, the other posts and the
    /// holes of the floor, and the lid itself.
    fn check_state(
        params: &EnclosureParams,
        edging: &Edging,
        holes: &BTreeMap<Id, Hole>,
        posts: &BTreeMap<Id, Post>,
        lid: Option<&Lid>,
    ) -> Result<(), CadError> {
        let derived = match lid {
            Some(lid) => lid_holes_of(
                params.wall,
                params.inner_length,
                params.inner_width,
                posts,
                lid,
            )?,
            None => Vec::new(),
        };
        let mut accepted: BTreeMap<Id, Hole> = BTreeMap::new();
        for (id, hole) in holes {
            Self::check_hole_in(params, edging, &accepted, &derived, hole, None)?;
            accepted.insert(*id, hole.clone());
        }
        let mut earlier: Vec<Hole> = Vec::new();
        for hole in &derived {
            Self::check_hole_in(params, edging, holes, &earlier, hole, None)?;
            earlier.push(hole.clone());
        }
        Self::check_posts(params, holes, posts)?;
        if let Some(lid) = lid {
            Self::check_lid(params, posts, lid)?;
        }
        Ok(())
    }

    /// Every post against the box: its own shape, its height, the walls, the other posts and the
    /// holes of the floor. A post may touch a wall or another post; it may not overlap them.
    fn check_posts(
        params: &EnclosureParams,
        holes: &BTreeMap<Id, Hole>,
        posts: &BTreeMap<Id, Post>,
    ) -> Result<(), CadError> {
        let (il, iw, ih) = (
            i128::from(params.inner_length.get()),
            i128::from(params.inner_width.get()),
            i128::from(params.inner_height.get()),
        );
        let wall = i128::from(params.wall.get());
        let outer_y = i128::from(params.outer().1.get());
        let mut seen: Vec<&Post> = Vec::new();
        for post in posts.values() {
            post.check_shape()?;
            if i128::from(post.height.get()) >= ih {
                return Err(CadError::PostHeight(format!(
                    "post {}: not lower than the inner height",
                    post.name
                )));
            }
            let d = i128::from(post.footprint_diameter().get());
            let (x, y) = (i128::from(post.x.get()), i128::from(post.y.get()));
            // twice the distance from the centre to each wall is at least the footprint
            if 2 * x < d || 2 * (il - x) < d || 2 * y < d || 2 * (iw - y) < d {
                return Err(CadError::PostPlace(format!(
                    "post {}: meets a wall",
                    post.name
                )));
            }
            for other in &seen {
                let od = i128::from(other.footprint_diameter().get());
                let (dx, dy) = (x - i128::from(other.x.get()), y - i128::from(other.y.get()));
                if 4 * (dx * dx + dy * dy) < (d + od) * (d + od) {
                    return Err(CadError::PostPlace(format!(
                        "posts {} and {}: overlap",
                        post.name, other.name
                    )));
                }
            }
            for hole in holes.values().filter(|h| h.face == Face::Bottom) {
                let (su, sv) = hole.style.footprint(hole.diameter);
                let across = if hole.style.is_round() {
                    su
                } else {
                    let sq = u128::try_from(su * su + sv * sv).unwrap_or(0);
                    i128::try_from(isqrt_u128(sq) + 1).unwrap_or(i128::MAX)
                };
                // the hole of the floor in the frame of the inner space
                let hx = i128::from(hole.u.get()) - wall;
                let hy = outer_y - i128::from(hole.v.get()) - wall;
                let (dx, dy) = (x - hx, y - hy);
                if 4 * (dx * dx + dy * dy) < (d + across) * (d + across) {
                    return Err(CadError::PostPlace(format!(
                        "post {}: meets the hole {} of the floor",
                        post.name, hole.name
                    )));
                }
            }
            seen.push(post);
        }
        Ok(())
    }

    /// The fixing of the lid against the box: the screw hole and its recess, and the lip.
    fn check_lid(
        params: &EnclosureParams,
        posts: &BTreeMap<Id, Post>,
        lid: &Lid,
    ) -> Result<(), CadError> {
        if lid.hole_diameter.get() <= 0 {
            return Err(CadError::NotPositive("lid screw hole"));
        }
        if lid.clearance.get() < 0 {
            return Err(CadError::NotPositive("lid clearance"));
        }
        if !lid.style.is_round() {
            return Err(CadError::LidFixing(
                "the head of a lid screw is round: use a plain hole, a countersink or a counterbore"
                    .into(),
            ));
        }
        lid.style.check(lid.hole_diameter, params.lid)?;
        if let Some(lip) = &lid.lip {
            if lip.thickness.get() <= 0 {
                return Err(CadError::NotPositive("lip thickness"));
            }
            if lip.depth.get() <= 0 {
                return Err(CadError::NotPositive("lip depth"));
            }
            let (il, iw, ih) = (
                i128::from(params.inner_length.get()),
                i128::from(params.inner_width.get()),
                i128::from(params.inner_height.get()),
            );
            let depth = i128::from(lip.depth.get());
            if depth >= ih {
                return Err(CadError::LidFixing(
                    "the lip is as deep as the inner space".into(),
                ));
            }
            // the ring starts one clearance inside the walls and is one thickness wide
            let ring = i128::from(lid.clearance.get()) + i128::from(lip.thickness.get());
            if 2 * ring >= il.min(iw) {
                return Err(CadError::LidFixing("the lip leaves no opening".into()));
            }
            for post in posts.values() {
                // a post that reaches the height of the lip must stay inside the ring
                if i128::from(post.height.get()) > ih - depth {
                    let d = i128::from(post.outer_diameter.get());
                    let (x, y) = (i128::from(post.x.get()), i128::from(post.y.get()));
                    if 2 * x < d + 2 * ring
                        || 2 * (il - x) < d + 2 * ring
                        || 2 * y < d + 2 * ring
                        || 2 * (iw - y) < d + 2 * ring
                    {
                        return Err(CadError::LidFixing(format!(
                            "post {}: meets the centring lip",
                            post.name
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    fn check_hole_in(
        params: &EnclosureParams,
        edging: &Edging,
        holes: &BTreeMap<Id, Hole>,
        extra: &[Hole],
        hole: &Hole,
        ignoring: Option<Id>,
    ) -> Result<(), CadError> {
        if hole.diameter.get() <= 0 {
            return Err(CadError::NotPositive("hole diameter"));
        }
        hole.style
            .check(hole.diameter, thickness_of(params, hole.face))?;
        let (w, h) = face_size_of(params, hole.face);
        let (su, sv) = hole.style.footprint(hole.diameter);
        let (u, v) = (i128::from(hole.u.get()), i128::from(hole.v.get()));
        // the distance to an edge that is rounded or chamfered is at least the size of the
        // treatment, because the face is not flat there
        let [u_low, u_high, v_low, v_high] = hole.face.neighbours();
        let margin_to = |side: Face| {
            i128::from(
                params
                    .edge_margin
                    .get()
                    .max(edging.treatment_of(hole.face, side).size().get()),
            )
        };
        // 2 * (distance from the centre to an edge) >= size + 2 * margin
        let inside = |c: i128, size: i128, extent: i128, low: Face, high: Face| {
            2 * c >= extent + 2 * margin_to(low) && 2 * (size - c) >= extent + 2 * margin_to(high)
        };
        if !inside(u, i128::from(w.get()), su, u_low, u_high)
            || !inside(v, i128::from(h.get()), sv, v_low, v_high)
        {
            return Err(CadError::HoleOutsideFace(hole.name.clone()));
        }
        // the lid and the floor have the rounded or chamfered corners of the vertical edges; the
        // circle that holds the opening (and its margin) must stay inside the corner
        if matches!(hole.face, Face::Top | Face::Bottom) {
            let (wu, hv) = (i128::from(w.get()), i128::from(h.get()));
            let across = if hole.style.is_round() {
                su
            } else {
                let sq = u128::try_from(su * su + sv * sv).unwrap_or(0);
                i128::try_from(isqrt_u128(sq) + 1).unwrap_or(i128::MAX)
            };
            let r2 = across + 2 * i128::from(params.edge_margin.get());
            let s = i128::from(edging.vertical.size().get());
            for (cu, cv) in [(u, v), (wu - u, v), (u, hv - v), (wu - u, hv - v)] {
                let clear = match edging.vertical {
                    Treatment::Sharp => true,
                    // the circle of radius r2 / 2 inside the arc of radius s centred at (s, s)
                    Treatment::Round(_) => {
                        cu >= s
                            || cv >= s
                            || (2 * s >= r2
                                && 4 * ((s - cu) * (s - cu) + (s - cv) * (s - cv))
                                    <= (2 * s - r2) * (2 * s - r2))
                    }
                    // at least r2 / 2 from the line u + v = s
                    Treatment::Chamfer(_) => {
                        let d = cu + cv - s;
                        d > 0 && 2 * d * d >= r2 * r2
                    }
                };
                if !clear {
                    return Err(CadError::HoleOutsideFace(hole.name.clone()));
                }
            }
        }
        let web = i128::from(params.min_web.get());
        let apart = |other: &Hole| -> bool {
            let (du, dv) = (u - i128::from(other.u.get()), v - i128::from(other.v.get()));
            let (ou, ov) = other.style.footprint(other.diameter);
            if hole.style.is_round() && other.style.is_round() {
                // 4 * distance^2 >= (size + other size + 2 * web)^2
                let need = su + ou + 2 * web;
                4 * (du * du + dv * dv) >= need * need
            } else {
                // the rectangles that hold the openings do not come within the web of each other
                2 * du.abs() >= su + ou + 2 * web || 2 * dv.abs() >= sv + ov + 2 * web
            }
        };
        for (other_id, other) in holes {
            if other.face != hole.face || Some(*other_id) == ignoring {
                continue;
            }
            if !apart(other) {
                return Err(CadError::HolesOverlap(
                    hole.name.clone(),
                    other.name.clone(),
                ));
            }
        }
        for other in extra {
            if other.face == hole.face && !apart(other) {
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
                self.check_floor_hole_against_posts(id, &hole)?;
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
                self.check_floor_hole_against_posts(id, &hole)?;
                self.holes.insert(id, hole);
                Ok(Operation::ReplaceHole { id, hole: old })
            }
            Operation::SetParams(params) => {
                params.check()?;
                self.edging.check(&params)?;
                // every hole must still be valid against the new parameters, one after the other
                self.check_all(&params, &self.edging)?;
                let old = self.params;
                self.params = params;
                Ok(Operation::SetParams(old))
            }
            Operation::SetEdging(edging) => {
                edging.check(&self.params)?;
                self.check_all(&self.params, &edging)?;
                let old = self.edging;
                self.edging = edging;
                Ok(Operation::SetEdging(old))
            }
            Operation::AddPost { id, post } => {
                if self.posts.contains_key(&id) {
                    return Err(CadError::HoleId(id));
                }
                let mut posts = self.posts.clone();
                posts.insert(id, post);
                self.check_posts_and_lid(&posts, self.lid.as_ref())?;
                self.posts = posts;
                Ok(Operation::RemovePost { id })
            }
            Operation::RemovePost { id } => {
                let mut posts = self.posts.clone();
                let post = posts.remove(&id).ok_or(CadError::HoleId(id))?;
                self.check_posts_and_lid(&posts, self.lid.as_ref())?;
                self.posts = posts;
                Ok(Operation::AddPost { id, post })
            }
            Operation::ReplacePost { id, post } => {
                let old = self.posts.get(&id).ok_or(CadError::HoleId(id))?.clone();
                let mut posts = self.posts.clone();
                posts.insert(id, post);
                self.check_posts_and_lid(&posts, self.lid.as_ref())?;
                self.posts = posts;
                Ok(Operation::ReplacePost { id, post: old })
            }
            Operation::SetLid(lid) => {
                self.check_posts_and_lid(&self.posts, lid.as_ref())?;
                let old = std::mem::replace(&mut self.lid, lid);
                Ok(Operation::SetLid(old))
            }
        }
    }

    /// The whole state with other posts and another lid fixing.
    fn check_posts_and_lid(
        &self,
        posts: &BTreeMap<Id, Post>,
        lid: Option<&Lid>,
    ) -> Result<(), CadError> {
        Self::check_state(&self.params, &self.edging, &self.holes, posts, lid)
    }

    /// A hole of the floor, added or replaced under `id`, against the posts.
    fn check_floor_hole_against_posts(&self, id: Id, hole: &Hole) -> Result<(), CadError> {
        if hole.face != Face::Bottom || self.posts.is_empty() {
            return Ok(());
        }
        let mut holes = self.holes.clone();
        holes.insert(id, hole.clone());
        Self::check_posts(&self.params, &holes, &self.posts)
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
            style: HoleStyle::Plain,
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

    fn rounded(vertical: i64) -> Edging {
        Edging {
            vertical: Treatment::Round(mm(vertical)),
            ..Edging::default()
        }
    }

    #[test]
    fn an_edge_treatment_must_fit_the_box_and_stay_out_of_the_inner_space() {
        let mut e = Enclosure::new(params()).unwrap();
        let before = e.clone();
        // larger than half of the 64 mm width
        let err = e.apply(Operation::SetEdging(rounded(40))).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-019");
        // a chamfer of 5 mm cuts through a wall of 2 mm at the inner corner
        let cut = Edging {
            vertical: Treatment::Chamfer(mm(5)),
            ..Edging::default()
        };
        assert_eq!(
            e.apply(Operation::SetEdging(cut)).unwrap_err().code(),
            "PHOENIX-CAD-019"
        );
        // a size of zero is not a treatment
        let zero = Edging {
            top: Treatment::Chamfer(Length::ZERO),
            ..Edging::default()
        };
        assert_eq!(
            e.apply(Operation::SetEdging(zero)).unwrap_err(),
            CadError::NotPositive("edge treatment")
        );
        assert_eq!(e, before, "a refused operation leaves the model unchanged");
        // a rounding of 5 mm leaves the corner of the inner space inside the material
        assert!(e.apply(Operation::SetEdging(rounded(5))).is_ok());
        assert!(!e.edging().is_sharp());
    }

    #[test]
    fn the_treatments_of_the_lid_and_of_the_floor_must_fit_in_the_height() {
        let tall_wall = EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(10),
            wall: mm(20),
            floor: mm(2),
            lid: mm(2),
            edge_margin: mm(3),
            min_web: mm(2),
        };
        let mut e = Enclosure::new(tall_wall).unwrap();
        let both = Edging {
            top: Treatment::Chamfer(mm(10)),
            bottom: Treatment::Chamfer(mm(10)),
            ..Edging::default()
        };
        // each is valid alone, together they are higher than the 14 mm box
        let top_only = Edging {
            bottom: Treatment::Sharp,
            ..both
        };
        assert!(e.apply(Operation::SetEdging(top_only)).is_ok());
        assert_eq!(
            e.apply(Operation::SetEdging(both)).unwrap_err().code(),
            "PHOENIX-CAD-019"
        );
    }

    #[test]
    fn holes_keep_away_from_a_treated_edge_by_the_size_of_the_treatment() {
        let mut e = Enclosure::new(params()).unwrap();
        // 6 mm hole centred 7 mm from the left edge: fine with the margin of 3 mm
        e.apply(add("a", hole(Face::Front, 7, 17, 6))).unwrap();
        let before = e.clone();
        // once the vertical edges are rounded by 5 mm the hole would need 5 + 3 mm of room
        assert_eq!(
            e.apply(Operation::SetEdging(rounded(5))).unwrap_err(),
            CadError::HoleOutsideFace("a".into())
        );
        assert_eq!(e, before);
        let mut e = Enclosure::new(params()).unwrap();
        e.apply(Operation::SetEdging(rounded(5))).unwrap();
        assert!(e.apply(add("a", hole(Face::Front, 7, 17, 6))).is_err());
        assert!(e.apply(add("a", hole(Face::Front, 8, 17, 6))).is_ok());
    }

    #[test]
    fn a_hole_of_the_lid_stays_inside_the_rounded_or_chamfered_corner() {
        let mut p = params();
        p.edge_margin = mm(1);
        let at = |u: i64, v: i64| {
            let mut h = hole(Face::Top, 0, 0, 6);
            h.name = "t".into();
            h.u = Length::new(u).unwrap();
            h.v = Length::new(v).unwrap();
            Operation::AddHole {
                id: id_of("t"),
                hole: h,
            }
        };
        let mut e = Enclosure::new(p).unwrap();
        e.apply(Operation::SetEdging(rounded(6))).unwrap();
        // a hole of 6 mm with 1 mm of margin has a radius of 4 mm to keep clear of the arc
        // of radius 6 mm whose centre is at (6, 6): 4.5 mm is too near the corner, 5 mm fits
        assert!(e.apply(at(4_500_000, 4_500_000)).is_err());
        assert!(e.apply(at(5_000_000, 5_000_000)).is_ok());
        let mut c = Enclosure::new(p).unwrap();
        c.apply(Operation::SetEdging(Edging {
            vertical: Treatment::Chamfer(mm(3)),
            ..Edging::default()
        }))
        .unwrap();
        // the cut runs through (3, 0) and (0, 3); the hole needs 4 mm from it, so u + v >= 3 + 4 * sqrt(2)
        assert!(c.apply(at(4_000_000, 4_000_000)).is_err());
        assert!(c.apply(at(4_400_000, 4_400_000)).is_ok());
    }

    #[test]
    fn the_edging_has_an_exact_inverse_and_the_parameters_cannot_break_it() {
        let mut e = Enclosure::new(params()).unwrap();
        let start = e.clone();
        let undo = e.apply(Operation::SetEdging(rounded(5))).unwrap();
        assert_eq!(undo, Operation::SetEdging(Edging::default()));
        // a thinner wall would let the rounding reach the inner space
        let mut thin = params();
        thin.wall = mm(1);
        let before = e.clone();
        assert_eq!(
            e.apply(Operation::SetParams(thin)).unwrap_err().code(),
            "PHOENIX-CAD-019"
        );
        assert_eq!(e, before);
        e.apply(undo).unwrap();
        assert_eq!(e, start);
    }

    #[test]
    fn the_neighbours_of_a_face_are_four_other_faces_that_are_not_opposite_to_each_other() {
        for face in Face::ALL {
            let n = face.neighbours();
            for f in n {
                assert_ne!(f, face);
                assert_ne!(f, face.opposite());
            }
            assert_eq!(n[0].opposite(), n[1]);
            assert_eq!(n[2].opposite(), n[3]);
        }
    }

    fn styled(name: &str, u: i64, v: i64, d: i64, style: HoleStyle) -> Operation {
        let mut h = hole(Face::Front, u, v, d);
        h.style = style;
        add(name, h)
    }

    fn um(v: i64) -> Length {
        Length::from_um(v).unwrap()
    }

    #[test]
    fn a_shape_that_does_not_fit_its_hole_or_the_material_is_refused_with_its_own_code() {
        let mut e = Enclosure::new(params()).unwrap();
        let before = e.clone();
        let refused = [
            // the countersink is not wider than the hole
            HoleStyle::Countersunk {
                head_diameter: mm(3),
            },
            // the cone would be as deep as the wall of 2 mm: (7 - 3) / 2
            HoleStyle::Countersunk {
                head_diameter: mm(7),
            },
            // the recess is as deep as the wall
            HoleStyle::Counterbored {
                head_diameter: mm(6),
                depth: mm(2),
            },
            // the recess is not wider than the hole
            HoleStyle::Counterbored {
                head_diameter: mm(3),
                depth: mm(1),
            },
            // a slot shorter than it is wide
            HoleStyle::Oval {
                length: mm(2),
                vertical: false,
            },
        ];
        for style in refused {
            let err = e.apply(styled("s", 30, 17, 3, style)).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-020", "{style:?}");
        }
        assert_eq!(
            e.apply(styled(
                "s",
                30,
                17,
                3,
                HoleStyle::Counterbored {
                    head_diameter: mm(6),
                    depth: Length::ZERO
                }
            ))
            .unwrap_err(),
            CadError::NotPositive("counterbore depth")
        );
        assert_eq!(
            e.apply(styled(
                "s",
                30,
                17,
                3,
                HoleStyle::Rectangular {
                    height: Length::ZERO
                }
            ))
            .unwrap_err(),
            CadError::NotPositive("rectangular hole height")
        );
        assert_eq!(e, before);
        // a countersink just under the limit is accepted: (6.9 - 3) / 2 is less than 2
        assert!(e
            .apply(styled(
                "ok",
                30,
                17,
                3,
                HoleStyle::Countersunk {
                    head_diameter: um(6_900)
                }
            ))
            .is_ok());
    }

    #[test]
    fn the_opening_of_a_wide_head_or_a_slot_is_what_must_fit_the_face() {
        let mut e = Enclosure::new(params()).unwrap();
        let slot = |u: i64| {
            styled(
                "slot",
                u,
                17,
                4,
                HoleStyle::Oval {
                    length: mm(20),
                    vertical: false,
                },
            )
        };
        // 20 mm along u with 3 mm of margin needs the centre 13 mm from the edge
        assert_eq!(
            e.apply(slot(12)).unwrap_err(),
            CadError::HoleOutsideFace("slot".into())
        );
        assert!(e.apply(slot(13)).is_ok());
        // the same slot upright has 4 mm across and 20 mm along v, but the face is 35 mm high
        let mut up = Enclosure::new(params()).unwrap();
        let upright = |v: i64| {
            styled(
                "slot",
                30,
                v,
                4,
                HoleStyle::Oval {
                    length: mm(20),
                    vertical: true,
                },
            )
        };
        assert!(up.apply(upright(12)).is_err());
        assert!(up.apply(upright(13)).is_ok());
    }

    #[test]
    fn rectangles_and_slots_keep_the_web_with_the_rectangle_that_holds_them() {
        let mut e = Enclosure::new(params()).unwrap();
        // a rectangle 10 mm wide (the diameter) and 4 mm high
        e.apply(styled(
            "r",
            30,
            17,
            10,
            HoleStyle::Rectangular { height: mm(4) },
        ))
        .unwrap();
        // a round hole of 6 mm: 20 = 10 + 6 + 2 * 2 is the least distance of the centres along u
        assert!(e.apply(add("a", hole(Face::Front, 39, 17, 6))).is_err());
        assert!(e.apply(add("a", hole(Face::Front, 40, 17, 6))).is_ok());
        // above the rectangle only the heights count: 4 / 2 + 6 / 2 + 2 = 7 mm
        assert!(e.apply(add("b", hole(Face::Front, 30, 23, 6))).is_err());
        assert!(e.apply(add("b", hole(Face::Front, 30, 24, 6))).is_ok());
    }

    #[test]
    fn changing_the_shape_of_a_hole_has_an_exact_inverse() {
        let mut e = Enclosure::new(params()).unwrap();
        e.apply(add("a", hole(Face::Front, 30, 17, 6))).unwrap();
        let start = e.clone();
        let mut oval = hole(Face::Front, 30, 17, 6);
        oval.name = "a".into();
        oval.style = HoleStyle::Oval {
            length: mm(12),
            vertical: true,
        };
        let undo = e
            .apply(Operation::ReplaceHole {
                id: id_of("a"),
                hole: oval,
            })
            .unwrap();
        assert_ne!(e, start);
        e.apply(undo).unwrap();
        assert_eq!(e, start);
    }

    #[test]
    fn a_style_grows_in_every_direction_and_keeps_its_depth() {
        let by = um(150);
        assert_eq!(HoleStyle::Plain.grown(by).unwrap(), HoleStyle::Plain);
        assert_eq!(
            HoleStyle::Counterbored {
                head_diameter: mm(6),
                depth: mm(1)
            }
            .grown(by)
            .unwrap(),
            HoleStyle::Counterbored {
                head_diameter: um(6_150),
                depth: mm(1)
            }
        );
        assert_eq!(
            HoleStyle::Oval {
                length: mm(10),
                vertical: true
            }
            .grown(by)
            .unwrap(),
            HoleStyle::Oval {
                length: um(10_150),
                vertical: true
            }
        );
        assert!(HoleStyle::Rectangular {
            height: Length::new((1 << 40) - 1).unwrap()
        }
        .grown(by)
        .is_err());
    }
}
