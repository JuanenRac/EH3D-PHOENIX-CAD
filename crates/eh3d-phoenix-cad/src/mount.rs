//! Posts that rise from the floor of the box, and a lid that is screwed onto them.
//!
//! A [`Post`] is a round pillar on the floor with a bore in its top for a threaded insert or for a
//! self-tapping screw, and an optional wider foot that reinforces its base. Its position is the
//! centre in the inner space, measured from the inner face of the left wall (`x`) and of the front
//! wall (`y`). A [`Lid`] describes how the lid is fixed: through holes in the lid, aligned with the
//! posts (or with the four corners when the box has no posts), with a head recess for the screw,
//! and an optional lip on the underside of the lid that centres it in the opening. The holes of the
//! lid are not stored: they are derived from the posts and the lid every time, so they follow the
//! posts when a post moves.
//!
//! The functions at the end make posts from the mounting holes of a board ([`posts_for_board`]):
//! the sizes come from the tables of the settings file, and the answer is a list of operations
//! that is checked by applying it to a copy of the enclosure, so nothing is returned that the
//! enclosure would refuse.

use std::collections::BTreeMap;

use eh3d_container::ObjectId;

use crate::config::{Head, Settings};
use crate::enclosure::{Enclosure, Face, Hole, HoleStyle, Id, Operation};
use crate::scad::mm_text;
use crate::{CadError, Length};

/// What the bore in the top of a post is made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoreKind {
    /// A hole for a threaded insert that is pressed or melted into the post.
    Insert,
    /// A pilot hole for a self-tapping screw.
    SelfTapping,
}

impl BoreKind {
    /// A stable lower-case name.
    pub fn name(self) -> &'static str {
        match self {
            BoreKind::Insert => "insert",
            BoreKind::SelfTapping => "self-tapping",
        }
    }

    /// The kind with a name given by [`BoreKind::name`], or `None`.
    pub fn from_name(name: &str) -> Option<BoreKind> {
        [BoreKind::Insert, BoreKind::SelfTapping]
            .into_iter()
            .find(|k| k.name() == name)
    }
}

/// The wider foot of a post: a cone from this diameter at the floor to the diameter of the post.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Base {
    /// The diameter at the floor.
    pub diameter: Length,
    /// The height of the cone.
    pub height: Length,
}

/// A post on the floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    /// The name shown to people; an attribute, it never identifies the post.
    pub name: String,
    /// The mounting hole of the board that this post was made for, when there is one.
    pub board_hole: Option<Id>,
    /// The position of the centre along X, from the inner face of the left wall.
    pub x: Length,
    /// The position of the centre along Y, from the inner face of the front wall.
    pub y: Length,
    /// The outer diameter.
    pub outer_diameter: Length,
    /// The height above the floor.
    pub height: Length,
    /// What the bore is for.
    pub kind: BoreKind,
    /// The diameter of the bore.
    pub bore_diameter: Length,
    /// The depth of the bore from the top of the post.
    pub bore_depth: Length,
    /// The reinforced foot, if any.
    pub base: Option<Base>,
}

impl Post {
    /// The diameter of the circle that holds the whole post on the floor: the foot when it is
    /// wider than the post.
    pub fn footprint_diameter(&self) -> Length {
        match self.base {
            Some(base) if base.diameter > self.outer_diameter => base.diameter,
            _ => self.outer_diameter,
        }
    }

    /// Checks the shape of the post on its own, apart from the box.
    pub fn check_shape(&self) -> Result<(), CadError> {
        for (what, v) in [
            ("post diameter", self.outer_diameter),
            ("post height", self.height),
            ("post bore diameter", self.bore_diameter),
            ("post bore depth", self.bore_depth),
        ] {
            if v.get() <= 0 {
                return Err(CadError::NotPositive(what));
            }
        }
        if self.bore_diameter >= self.outer_diameter {
            return Err(CadError::PostShape(format!(
                "post {}: the bore is not narrower than the post",
                self.name
            )));
        }
        if self.bore_depth > self.height {
            return Err(CadError::PostShape(format!(
                "post {}: the bore is deeper than the post is high",
                self.name
            )));
        }
        if let Some(base) = self.base {
            if base.height.get() <= 0 {
                return Err(CadError::NotPositive("post base height"));
            }
            if base.diameter <= self.outer_diameter {
                return Err(CadError::PostShape(format!(
                    "post {}: the base is not wider than the post",
                    self.name
                )));
            }
            if base.height >= self.height {
                return Err(CadError::PostShape(format!(
                    "post {}: the base is as high as the post",
                    self.name
                )));
            }
        }
        Ok(())
    }
}

/// Where the screws of the lid go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LidFixing {
    /// Over each post.
    Posts,
    /// At the four corners of the inner space, this far from each of its two walls.
    Corners {
        /// The distance from the centre of the screw to both walls of its corner.
        inset: Length,
    },
    /// Over each post, or at the four corners when the box has no posts.
    Auto {
        /// The distance used at the corners.
        inset: Length,
    },
}

/// A lip on the underside of the lid that centres it in the opening of the box: a rectangular
/// ring that fits inside the walls with the clearance of the printer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lip {
    /// The thickness of the ring.
    pub thickness: Length,
    /// How far the ring hangs below the lid.
    pub depth: Length,
}

/// The fixing of the lid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lid {
    /// Where the screws go.
    pub fixing: LidFixing,
    /// The screw of the table of the settings, kept for the people who read the recipe.
    pub screw: String,
    /// The diameter of the hole the screw passes through.
    pub hole_diameter: Length,
    /// The recess for the head: a plain hole, a countersink or a counterbore.
    pub style: HoleStyle,
    /// The gap between the lip and the walls (the clearance of the printer profile).
    pub clearance: Length,
    /// The centring lip, if any.
    pub lip: Option<Lip>,
}

/// The holes that the lid has over the posts or at the corners, in the order of the posts (by id)
/// or of the corners (front left, front right, back left, back right). They are holes of the top
/// face, placed from the lower left corner of the face as seen from outside.
pub fn lid_holes_of(
    wall: Length,
    inner_length: Length,
    inner_width: Length,
    posts: &BTreeMap<Id, Post>,
    lid: &Lid,
) -> Result<Vec<Hole>, CadError> {
    let corners = match lid.fixing {
        LidFixing::Posts => None,
        LidFixing::Corners { inset } => Some(inset),
        LidFixing::Auto { inset } => {
            if posts.is_empty() {
                Some(inset)
            } else {
                None
            }
        }
    };
    let make = |name: String, x: i64, y: i64| -> Result<Hole, CadError> {
        Ok(Hole {
            name,
            face: Face::Top,
            u: Length::new(wall.get() + x)?,
            v: Length::new(wall.get() + y)?,
            diameter: lid.hole_diameter,
            style: lid.style,
        })
    };
    let mut out = Vec::new();
    match corners {
        Some(inset) => {
            let a = inset.get();
            if a <= 0 {
                return Err(CadError::NotPositive("lid corner inset"));
            }
            let (il, iw) = (inner_length.get(), inner_width.get());
            if 2 * a >= il || 2 * a >= iw {
                return Err(CadError::LidFixing(
                    "the corner inset leaves no room between the screws".into(),
                ));
            }
            let ys = [("front", a), ("back", iw - a)];
            let xs = [("left", a), ("right", il - a)];
            for (row, y) in ys {
                for (column, x) in xs {
                    out.push(make(format!("lid screw {row} {column}"), x, y)?);
                }
            }
        }
        None => {
            for post in posts.values() {
                out.push(make(
                    format!("lid screw {}", post.name),
                    post.x.get(),
                    post.y.get(),
                )?);
            }
        }
    }
    Ok(out)
}

/// An id made from another one: the same time and counter, with the random part changed, so that
/// the same board hole always gives the same post. The result differs from the original.
pub fn derived_id(id: Id, salt: u8) -> Result<Id, CadError> {
    let mut bytes = *id.as_bytes();
    bytes[15] ^= if salt == 0 { 0xA5 } else { salt };
    ObjectId::from_bytes(bytes).map_err(|_| CadError::ValueOutOfRange("id"))
}

/// A mounting hole of a board: its centre in the frame of the board (nanometres, `y` upwards as
/// seen from the component side) and its diameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardHole {
    /// The identity of the hole in the board tool.
    pub id: Id,
    /// The name shown to people.
    pub name: String,
    /// The position of the centre along X.
    pub x: Length,
    /// The position of the centre along Y.
    pub y: Length,
    /// The diameter of the hole.
    pub diameter: Length,
}

/// What the board tool tells the mechanical side: the outline of the board, its mounting holes and
/// the sizes that the posts have to respect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardInterface {
    /// The corners of the outline, in order; the last one joins the first.
    pub outline: Vec<(Length, Length)>,
    /// The mounting holes.
    pub holes: Vec<BoardHole>,
    /// The thickness of the board.
    pub thickness: Length,
    /// The height of the tallest part above the top face of the board, when it is known.
    pub tallest_component: Option<Length>,
    /// Where the lower left corner of the rectangle that holds the outline lies in the inner space
    /// of the box; when it is not given the board is centred.
    pub placement: Option<(Length, Length)>,
}

impl BoardInterface {
    /// The smallest and the largest corner of the rectangle that holds the outline, in
    /// nanometres, or `None` when the outline has no points.
    pub fn bounds(&self) -> Option<((i64, i64), (i64, i64))> {
        let first = self.outline.first()?;
        let mut min = (first.0.get(), first.1.get());
        let mut max = min;
        for (x, y) in &self.outline {
            min = (min.0.min(x.get()), min.1.min(y.get()));
            max = (max.0.max(x.get()), max.1.max(y.get()));
        }
        Some((min, max))
    }
}

/// What the posts made from a board have in common.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostRequest<'a> {
    /// The screw of the table of the settings.
    pub screw: &'a str,
    /// An insert or a self-tapping screw.
    pub kind: BoreKind,
    /// The height of the posts: from the floor to the underside of the board.
    pub height: Length,
    /// Whether the posts have the reinforced foot of the settings.
    pub base: bool,
}

/// One post for a screw of the table, with its centre at `at` in the inner space.
pub fn post_for(
    settings: &Settings,
    req: &PostRequest<'_>,
    name: &str,
    board_hole: Option<Id>,
    at: (Length, Length),
) -> Result<Post, CadError> {
    let fixing = settings.fixing(req.screw)?;
    let (bore_diameter, bore_depth) = match req.kind {
        BoreKind::Insert => (fixing.insert_diameter, fixing.insert_depth),
        BoreKind::SelfTapping => (fixing.pilot_diameter, fixing.pilot_depth),
    };
    if fixing.post_diameter.get() - bore_diameter.get() < 2 * settings.posts.min_wall.get() {
        return Err(CadError::PostShape(format!(
            "post {name}: the wall around the bore is thinner than the least wall of the settings"
        )));
    }
    let base = if req.base {
        Some(Base {
            diameter: fixing
                .post_diameter
                .checked_add(settings.posts.base_extra)?,
            height: settings.posts.base_height,
        })
    } else {
        None
    };
    Ok(Post {
        name: name.to_string(),
        board_hole,
        x: at.0,
        y: at.1,
        outer_diameter: fixing.post_diameter,
        height: req.height,
        kind: req.kind,
        bore_diameter,
        bore_depth,
        base,
    })
}

/// The operations that add one post under each mounting hole of a board.
///
/// The board is placed as `board.placement` says (centred when it does not), the post of a hole
/// stands under the centre of the hole, and its id is [`derived_id`] of the id of the hole. The
/// board must lie inside the inner space; the posts, the board and its tallest part must fit in
/// the inner height; a hole of the board must be at least as wide as the thread of the screw.
/// The list is applied to a copy of the enclosure first, so whatever the enclosure would refuse
/// (a post that meets a wall, another post or a hole in the floor) is the error returned, and
/// nothing is returned that cannot be applied.
pub fn posts_for_board(
    settings: &Settings,
    enclosure: &Enclosure,
    board: &BoardInterface,
    req: &PostRequest<'_>,
) -> Result<Vec<Operation>, CadError> {
    let screw = settings.screw(req.screw)?;
    let ((min_x, min_y), (max_x, max_y)) = board
        .bounds()
        .ok_or(CadError::ValueOutOfRange("board outline"))?;
    let p = enclosure.params();
    let (il, iw, ih) = (
        p.inner_length.get(),
        p.inner_width.get(),
        p.inner_height.get(),
    );
    let (bw, bh) = (max_x - min_x, max_y - min_y);
    let (px, py) = match board.placement {
        Some((x, y)) => (x.get(), y.get()),
        None => ((il - bw) / 2, (iw - bh) / 2),
    };
    if px < 0 || py < 0 || px + bw > il || py + bh > iw {
        return Err(CadError::PostPlace(
            "the board does not lie inside the inner space of the box".into(),
        ));
    }
    let tallest = board.tallest_component.map_or(0, Length::get);
    let needed = req.height.get() + board.thickness.get() + tallest;
    if needed > ih {
        return Err(CadError::PostHeight(format!(
            "the posts, the board and its tallest part need {} mm and the inner height is {} mm",
            mm_text(needed),
            mm_text(ih)
        )));
    }
    let mut ops = Vec::with_capacity(board.holes.len());
    for hole in &board.holes {
        if hole.diameter < screw.nominal {
            return Err(CadError::PostShape(format!(
                "board hole {}: narrower than the thread of {}",
                hole.name, req.screw
            )));
        }
        let at = (
            Length::new(px + hole.x.get() - min_x)?,
            Length::new(py + hole.y.get() - min_y)?,
        );
        let post = post_for(
            settings,
            req,
            &format!("post {}", hole.name),
            Some(hole.id),
            at,
        )?;
        ops.push(Operation::AddPost {
            id: derived_id(hole.id, 0xA5)?,
            post,
        });
    }
    let mut trial = enclosure.clone();
    trial.apply_all(ops.clone())?;
    Ok(ops)
}

/// The fixing of the lid for a screw of the table: the through hole is the clearance of the screw,
/// the recess is the one that `head` asks for, and the gap of the lip is the clearance of the
/// printer profile. The lip has the size of the settings.
pub fn lid_for(
    settings: &Settings,
    screw: &str,
    head: Head,
    fixing: LidFixing,
    lip: bool,
) -> Result<Lid, CadError> {
    let hole = settings.screw_hole(
        screw,
        head,
        "lid screw",
        Face::Top,
        (Length::ZERO, Length::ZERO),
    )?;
    Ok(Lid {
        fixing,
        screw: screw.to_string(),
        hole_diameter: hole.diameter,
        style: hole.style,
        clearance: settings.printer.fit_clearance,
        lip: if lip {
            Some(Lip {
                thickness: settings.lid.lip_thickness,
                depth: settings.lid.lip_depth,
            })
        } else {
            None
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::EnclosureParams;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn um(v: i64) -> Length {
        Length::from_um(v).unwrap()
    }

    fn nm(v: i64) -> Length {
        Length::new(v).unwrap()
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

    fn id(n: u64) -> Id {
        ObjectId::from_parts(1_700_000_000_000, n as u16, n).unwrap()
    }

    /// A post of 8 mm across and 6 mm high with a bore of 4.2 mm and 5 mm deep.
    fn post(name: &str, x: Length, y: Length) -> Post {
        Post {
            name: name.to_string(),
            board_hole: None,
            x,
            y,
            outer_diameter: mm(8),
            height: mm(6),
            kind: BoreKind::Insert,
            bore_diameter: um(4_200),
            bore_depth: mm(5),
            base: None,
        }
    }

    fn add(e: &mut Enclosure, n: u64, p: Post) -> Result<Operation, CadError> {
        e.apply(Operation::AddPost { id: id(n), post: p })
    }

    /// The fixing of the usual tests: a countersunk M3 screw, six millimetres from the corners.
    fn lid(lip: Option<Lip>) -> Lid {
        Lid {
            fixing: LidFixing::Auto { inset: mm(6) },
            screw: "M3".to_string(),
            hole_diameter: um(3_400),
            style: HoleStyle::Countersunk {
                head_diameter: um(6_300),
            },
            clearance: um(200),
            lip,
        }
    }

    fn lip() -> Lip {
        Lip {
            thickness: um(1_200),
            depth: mm(3),
        }
    }

    fn code(r: Result<Operation, CadError>) -> &'static str {
        r.unwrap_err().code()
    }

    #[test]
    fn the_kinds_of_bore_have_stable_names() {
        assert_eq!(BoreKind::Insert.name(), "insert");
        assert_eq!(BoreKind::SelfTapping.name(), "self-tapping");
        for kind in [BoreKind::Insert, BoreKind::SelfTapping] {
            assert_eq!(BoreKind::from_name(kind.name()), Some(kind));
        }
        assert_eq!(BoreKind::from_name("tapped"), None);
    }

    #[test]
    fn a_post_takes_its_sizes_from_the_table_of_the_settings() {
        let s = Settings::defaults();
        let req = PostRequest {
            screw: "M3",
            kind: BoreKind::Insert,
            height: mm(6),
            base: false,
        };
        let at = (mm(25), mm(15));
        let p = post_for(&s, &req, "p", None, at).unwrap();
        assert_eq!(p.outer_diameter.get(), 8_000_000);
        assert_eq!(p.bore_diameter.get(), 4_200_000);
        assert_eq!(p.bore_depth.get(), 5_000_000);
        assert_eq!(p.height.get(), 6_000_000);
        assert_eq!((p.x, p.y), at);
        assert_eq!(p.base, None);
        // a self-tapping screw has a narrower and deeper pilot hole
        let tap = PostRequest {
            kind: BoreKind::SelfTapping,
            height: mm(10),
            ..req
        };
        let p = post_for(&s, &tap, "p", None, at).unwrap();
        assert_eq!(p.bore_diameter.get(), 2_500_000);
        assert_eq!(p.bore_depth.get(), 8_000_000);
        // the foot is as much wider as the settings say: 8 + 4 mm, 2 mm high
        let foot = PostRequest { base: true, ..req };
        let p = post_for(&s, &foot, "p", None, at).unwrap();
        assert_eq!(
            p.base,
            Some(Base {
                diameter: nm(12_000_000),
                height: nm(2_000_000)
            })
        );
    }

    #[test]
    fn a_screw_without_a_post_entry_is_refused_with_the_screw_code() {
        let own = "[screw.M3.5]\nnominal = 3.5 mm\nclearance = 3.9 mm\nhead_diameter = 6 mm\nhead_height = 3.5 mm\ncountersink_diameter = 7.3 mm\n";
        let s = Settings::with_user_config(own).unwrap();
        let mut req = PostRequest {
            screw: "M3.5",
            kind: BoreKind::Insert,
            height: mm(6),
            base: false,
        };
        let at = (mm(25), mm(15));
        assert_eq!(
            post_for(&s, &req, "p", None, at).unwrap_err().code(),
            "PHOENIX-CAD-021"
        );
        req.screw = "M99";
        assert_eq!(
            post_for(&s, &req, "p", None, at).unwrap_err(),
            CadError::Screw("M99".into())
        );
    }

    #[test]
    fn the_wall_around_the_bore_may_not_be_thinner_than_the_least_wall() {
        let req = PostRequest {
            screw: "M3",
            kind: BoreKind::Insert,
            height: mm(6),
            base: false,
        };
        let at = (mm(25), mm(15));
        // 8 mm across and a bore of 4.2 mm leave 3.8 mm for two walls, 1.9 mm each
        let thin = Settings::with_user_config("[posts]\nmin_wall = 2 mm\n").unwrap();
        let err = post_for(&thin, &req, "p", None, at).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-027");
        let exact = Settings::with_user_config("[posts]\nmin_wall = 1.9 mm\n").unwrap();
        assert!(post_for(&exact, &req, "p", None, at).is_ok());
    }

    #[test]
    fn a_post_may_touch_a_wall_but_not_cross_it() {
        // the post is 8 mm across, so its centre must be 4 mm from each wall of the inner space
        for (x, y, ok) in [
            (4_000_000, 30_000_000, true),
            (3_999_999, 30_000_000, false),
            (96_000_000, 30_000_000, true),
            (96_000_001, 30_000_000, false),
            (50_000_000, 4_000_000, true),
            (50_000_000, 3_999_999, false),
            (50_000_000, 56_000_000, true),
            (50_000_000, 56_000_001, false),
        ] {
            let mut e = enclosure();
            let before = e.clone();
            let result = add(&mut e, 1, post("p", nm(x), nm(y)));
            if ok {
                assert!(result.is_ok(), "{x} {y}");
            } else {
                assert_eq!(code(result), "PHOENIX-CAD-028", "{x} {y}");
                assert_eq!(e, before);
            }
        }
    }

    #[test]
    fn a_post_may_touch_another_post_but_not_overlap_it() {
        let mut e = enclosure();
        add(&mut e, 1, post("a", mm(20), mm(30))).unwrap();
        // two posts of 8 mm whose centres are 8 mm apart touch each other
        assert!(add(&mut e, 2, post("b", mm(28), mm(30))).is_ok());
        let before = e.clone();
        // one nanometre nearer than touching, on the other side of a
        let err = add(&mut e, 3, post("c", nm(12_000_001), mm(30)));
        assert_eq!(code(err), "PHOENIX-CAD-028");
        assert_eq!(e, before);
        // on the diagonal the distance counts, not each axis: 6 and 6 mm are 8.49 mm, 5 and 5 are 7.07
        let mut f = enclosure();
        add(&mut f, 1, post("a", mm(20), mm(30))).unwrap();
        assert!(add(&mut f, 2, post("d", mm(26), mm(36))).is_ok());
        let mut g = enclosure();
        add(&mut g, 1, post("a", mm(20), mm(30))).unwrap();
        assert_eq!(
            code(add(&mut g, 2, post("e", mm(25), mm(35)))),
            "PHOENIX-CAD-028"
        );
    }

    #[test]
    fn the_foot_of_a_post_counts_when_it_is_wider_than_the_post() {
        let mut footed = post("p", mm(6), mm(30));
        footed.base = Some(Base {
            diameter: mm(12),
            height: mm(2),
        });
        let mut e = enclosure();
        // 12 mm across: the centre must be 6 mm from the wall
        assert!(add(&mut e, 1, footed.clone()).is_ok());
        let mut e = enclosure();
        footed.x = nm(5_999_999);
        assert_eq!(code(add(&mut e, 1, footed)), "PHOENIX-CAD-028");
        assert_eq!(post("p", mm(6), mm(30)).footprint_diameter(), mm(8));
    }

    #[test]
    fn the_shape_of_a_post_is_checked_on_its_own() {
        let at = (mm(50), mm(30));
        let try_post = |edit: &dyn Fn(&mut Post)| {
            let mut p = post("p", at.0, at.1);
            edit(&mut p);
            add(&mut enclosure(), 1, p)
        };
        // the bore must be narrower than the post, and no deeper than the post is high
        assert!(try_post(&|p| p.bore_diameter = nm(7_999_999)).is_ok());
        assert_eq!(
            code(try_post(&|p| p.bore_diameter = mm(8))),
            "PHOENIX-CAD-027"
        );
        assert!(try_post(&|p| p.bore_depth = mm(6)).is_ok());
        assert_eq!(
            code(try_post(&|p| p.bore_depth = nm(6_000_001))),
            "PHOENIX-CAD-027"
        );
        assert_eq!(
            code(try_post(&|p| p.outer_diameter = Length::ZERO)),
            "PHOENIX-CAD-002"
        );
        assert_eq!(
            code(try_post(&|p| p.height = Length::ZERO)),
            "PHOENIX-CAD-002"
        );
        // the foot must be wider than the post, and lower than it
        let foot = |d: i64, h: i64| {
            move |p: &mut Post| {
                p.base = Some(Base {
                    diameter: nm(d),
                    height: nm(h),
                })
            }
        };
        assert!(try_post(&foot(8_000_001, 5_999_999)).is_ok());
        assert_eq!(
            code(try_post(&foot(8_000_000, 2_000_000))),
            "PHOENIX-CAD-027"
        );
        assert_eq!(
            code(try_post(&foot(12_000_000, 6_000_000))),
            "PHOENIX-CAD-027"
        );
        assert_eq!(code(try_post(&foot(12_000_000, 0))), "PHOENIX-CAD-002");
    }

    #[test]
    fn a_post_must_be_lower_than_the_inner_height() {
        let tall = |h: i64| {
            let mut p = post("p", mm(50), mm(30));
            p.height = nm(h);
            add(&mut enclosure(), 1, p)
        };
        assert!(tall(29_999_999).is_ok());
        assert_eq!(code(tall(30_000_000)), "PHOENIX-CAD-029");
        assert_eq!(code(tall(31_000_000)), "PHOENIX-CAD-029");
    }

    #[test]
    fn a_post_may_not_meet_a_hole_of_the_floor_whichever_comes_first() {
        // the post stands at (25, 15) in the inner space, (27, 17) in the outer box, so a hole of
        // the floor at u = 27 mm and v = 64 - 17 = 47 mm is right under it
        let floor_hole = |u: i64| Hole {
            name: "drain".into(),
            face: Face::Bottom,
            u: nm(u),
            v: mm(47),
            diameter: mm(6),
            style: HoleStyle::Plain,
        };
        // 8 mm and 6 mm across: the centres must be 7 mm apart, and they are at 25 and 32
        let hole_id = id(50);
        let mut e = enclosure();
        add(&mut e, 1, post("p", mm(25), mm(15))).unwrap();
        assert!(e
            .apply(Operation::AddHole {
                id: hole_id,
                hole: floor_hole(34_000_000)
            })
            .is_ok());
        let mut e = enclosure();
        add(&mut e, 1, post("p", mm(25), mm(15))).unwrap();
        let before = e.clone();
        let err = e.apply(Operation::AddHole {
            id: hole_id,
            hole: floor_hole(33_999_999),
        });
        assert_eq!(code(err), "PHOENIX-CAD-028");
        assert_eq!(e, before);
        // the hole first, the post after
        let mut e = enclosure();
        e.apply(Operation::AddHole {
            id: hole_id,
            hole: floor_hole(27_000_000),
        })
        .unwrap();
        let err = add(&mut e, 1, post("p", mm(25), mm(15)));
        assert_eq!(code(err), "PHOENIX-CAD-028");
        assert!(e.posts().is_empty());
    }

    #[test]
    fn the_operation_that_adds_a_post_returns_the_one_that_removes_it() {
        let mut e = enclosure();
        let original = e.clone();
        let p = post("p", mm(25), mm(15));
        let undo = add(&mut e, 1, p.clone()).unwrap();
        assert_eq!(undo, Operation::RemovePost { id: id(1) });
        assert_eq!(e.posts().get(&id(1)), Some(&p));
        // the same id twice, and an id that is not there
        assert_eq!(
            code(add(&mut e, 1, post("q", mm(60), mm(30)))),
            "PHOENIX-CAD-005"
        );
        assert_eq!(
            code(e.apply(Operation::RemovePost { id: id(9) })),
            "PHOENIX-CAD-005"
        );
        assert_eq!(
            code(e.apply(Operation::ReplacePost {
                id: id(9),
                post: p.clone()
            })),
            "PHOENIX-CAD-005"
        );
        let inverse = e.apply(undo).unwrap();
        assert_eq!(inverse, Operation::AddPost { id: id(1), post: p });
        assert_eq!(e, original);
    }

    #[test]
    fn replacing_a_post_gives_back_the_old_one() {
        let mut e = enclosure();
        add(&mut e, 1, post("a", mm(25), mm(15))).unwrap();
        let moved = post("a", mm(30), mm(15));
        let undo = e
            .apply(Operation::ReplacePost {
                id: id(1),
                post: moved.clone(),
            })
            .unwrap();
        assert_eq!(e.posts().get(&id(1)), Some(&moved));
        e.apply(undo).unwrap();
        assert_eq!(e.posts().get(&id(1)).unwrap().x, mm(25));
    }

    #[test]
    fn without_posts_the_lid_is_screwed_at_the_four_corners() {
        let mut e = enclosure();
        e.apply(Operation::SetLid(Some(lid(None)))).unwrap();
        let holes = e.lid_holes();
        // 2 mm of wall and 6 mm from the corner; the other side is 2 + 100 - 6 and 2 + 60 - 6
        let got: Vec<(String, i64, i64)> = holes
            .iter()
            .map(|h| (h.name.clone(), h.u.get(), h.v.get()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("lid screw front left".to_string(), 8_000_000, 8_000_000),
                ("lid screw front right".to_string(), 96_000_000, 8_000_000),
                ("lid screw back left".to_string(), 8_000_000, 56_000_000),
                ("lid screw back right".to_string(), 96_000_000, 56_000_000),
            ]
        );
        for h in &holes {
            assert_eq!(h.face, Face::Top);
            assert_eq!(h.diameter, um(3_400));
            assert_eq!(
                h.style,
                HoleStyle::Countersunk {
                    head_diameter: um(6_300)
                }
            );
        }
        // the holes are not stored: they are not among the holes of the faces
        assert!(e.holes().is_empty());
        assert_eq!(e.all_holes(), holes);
    }

    #[test]
    fn with_posts_the_holes_of_the_lid_are_over_the_posts_and_follow_them() {
        let mut e = enclosure();
        add(&mut e, 2, post("b", mm(75), mm(45))).unwrap();
        add(&mut e, 1, post("a", mm(25), mm(15))).unwrap();
        e.apply(Operation::SetLid(Some(lid(None)))).unwrap();
        let got: Vec<(String, i64, i64)> = e
            .lid_holes()
            .iter()
            .map(|h| (h.name.clone(), h.u.get(), h.v.get()))
            .collect();
        // in the order of the ids of the posts, not of the order they were added; 2 mm of wall
        assert_eq!(
            got,
            vec![
                ("lid screw a".to_string(), 27_000_000, 17_000_000),
                ("lid screw b".to_string(), 77_000_000, 47_000_000),
            ]
        );
        // moving the post moves its hole
        e.apply(Operation::ReplacePost {
            id: id(1),
            post: post("a", mm(30), mm(15)),
        })
        .unwrap();
        assert_eq!(e.lid_holes()[0].u, mm(32));
        // a lid that is fixed to the posts only has no hole at all without posts
        let mut posts_only = lid(None);
        posts_only.fixing = LidFixing::Posts;
        let mut f = enclosure();
        f.apply(Operation::SetLid(Some(posts_only))).unwrap();
        assert!(f.lid_holes().is_empty());
        // taking the fixing away
        let undo = e.apply(Operation::SetLid(None)).unwrap();
        assert!(e.lid().is_none() && e.lid_holes().is_empty());
        e.apply(undo).unwrap();
        assert_eq!(e.lid_holes().len(), 2);
    }

    #[test]
    fn a_hole_of_the_lid_is_checked_like_any_other_hole_whichever_comes_first() {
        let top_hole = Hole {
            name: "vent".into(),
            face: Face::Top,
            u: mm(8),
            v: mm(8),
            diameter: mm(4),
            style: HoleStyle::Plain,
        };
        // a hole of the lid over the place of the corner screw
        let mut e = enclosure();
        e.apply(Operation::SetLid(Some(lid(None)))).unwrap();
        let before = e.clone();
        let err = e.apply(Operation::AddHole {
            id: id(60),
            hole: top_hole.clone(),
        });
        assert_eq!(code(err), "PHOENIX-CAD-004");
        assert_eq!(e, before);
        // the hole first, then the fixing
        let mut e = enclosure();
        e.apply(Operation::AddHole {
            id: id(60),
            hole: top_hole,
        })
        .unwrap();
        let err = e.apply(Operation::SetLid(Some(lid(None))));
        assert_eq!(code(err), "PHOENIX-CAD-004");
        assert!(e.lid().is_none());
        // 4 mm from the corner of the box, so a countersink of 6.3 mm is too near the edge
        let mut near = lid(None);
        near.fixing = LidFixing::Corners { inset: mm(2) };
        let err = enclosure().apply(Operation::SetLid(Some(near)));
        assert_eq!(code(err), "PHOENIX-CAD-003");
    }

    #[test]
    fn the_head_of_a_lid_screw_is_round_and_the_corners_need_room() {
        let mut slot = lid(None);
        slot.style = HoleStyle::Rectangular { height: mm(4) };
        assert_eq!(
            code(enclosure().apply(Operation::SetLid(Some(slot)))),
            "PHOENIX-CAD-030"
        );
        // the inner space is 60 mm wide: two screws 30 mm from the walls would be on top of each other
        let mut corners = lid(None);
        corners.fixing = LidFixing::Corners { inset: mm(30) };
        assert_eq!(
            code(enclosure().apply(Operation::SetLid(Some(corners.clone())))),
            "PHOENIX-CAD-030"
        );
        corners.fixing = LidFixing::Corners {
            inset: Length::ZERO,
        };
        assert_eq!(
            code(enclosure().apply(Operation::SetLid(Some(corners)))),
            "PHOENIX-CAD-002"
        );
        let mut thin = lid(None);
        thin.hole_diameter = Length::ZERO;
        assert_eq!(
            code(enclosure().apply(Operation::SetLid(Some(thin)))),
            "PHOENIX-CAD-002"
        );
    }

    #[test]
    fn the_centring_lip_has_its_limits() {
        let with = |thickness: i64, depth: i64, clearance: i64| {
            let mut l = lid(Some(Lip {
                thickness: nm(thickness),
                depth: nm(depth),
            }));
            l.clearance = nm(clearance);
            enclosure().apply(Operation::SetLid(Some(l)))
        };
        assert!(with(1_200_000, 3_000_000, 200_000).is_ok());
        // as deep as the inner space is refused, one nanometre less is not
        assert_eq!(
            code(with(1_200_000, 30_000_000, 200_000)),
            "PHOENIX-CAD-030"
        );
        assert!(with(1_200_000, 29_999_999, 200_000).is_ok());
        // the ring starts 0.2 mm inside the wall and the inner space is 60 mm across at its
        // narrowest: 2 * (0.2 + 29.8) mm = 60 mm leaves no opening, a nanometre less does
        assert_eq!(
            code(with(29_800_000, 3_000_000, 200_000)),
            "PHOENIX-CAD-030"
        );
        assert!(with(29_799_999, 3_000_000, 200_000).is_ok());
        assert_eq!(code(with(0, 3_000_000, 200_000)), "PHOENIX-CAD-002");
        assert_eq!(code(with(1_200_000, 0, 200_000)), "PHOENIX-CAD-002");
        assert_eq!(code(with(1_200_000, 3_000_000, -1)), "PHOENIX-CAD-002");
    }

    #[test]
    fn a_post_that_reaches_the_lip_must_stay_inside_its_ring() {
        // the lip hangs 3 mm from the lid, so it starts 27 mm above the floor; the ring is
        // 0.2 + 1.2 = 1.4 mm wide, so a post of 8 mm needs its centre 4 + 1.4 = 5.4 mm from the wall
        let tall = |h: i64, x: i64| {
            let mut p = post("p", nm(x), mm(30));
            p.height = nm(h);
            p
        };
        let try_lid = |p: Post| {
            let mut e = enclosure();
            add(&mut e, 1, p).unwrap();
            e.apply(Operation::SetLid(Some(lid(Some(lip())))))
        };
        assert!(
            try_lid(tall(27_000_000, 5_000_000)).is_ok(),
            "does not reach"
        );
        assert!(try_lid(tall(27_000_001, 5_400_000)).is_ok());
        assert_eq!(
            code(try_lid(tall(27_000_001, 5_399_999))),
            "PHOENIX-CAD-030"
        );
        assert_eq!(
            code(try_lid(tall(29_000_000, 5_000_000))),
            "PHOENIX-CAD-030"
        );
    }

    #[test]
    fn the_lid_for_a_screw_comes_from_the_table_and_the_profile() {
        let s = Settings::defaults();
        let fixing = LidFixing::Auto { inset: mm(6) };
        let l = lid_for(&s, "M3", Head::Countersunk, fixing, true).unwrap();
        assert_eq!(l.hole_diameter.get(), 3_400_000);
        assert_eq!(
            l.style,
            HoleStyle::Countersunk {
                head_diameter: nm(6_300_000)
            }
        );
        assert_eq!(l.clearance.get(), 200_000);
        assert_eq!(
            l.lip,
            Some(Lip {
                thickness: nm(1_200_000),
                depth: nm(3_000_000)
            })
        );
        assert_eq!(l.screw, "M3");
        let plain = lid_for(&s, "M3", Head::Clearance, fixing, false).unwrap();
        assert_eq!(plain.style, HoleStyle::Plain);
        assert_eq!(plain.lip, None);
        assert_eq!(
            lid_for(&s, "M99", Head::Clearance, fixing, false)
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-021"
        );
    }

    /// A board of 60 by 40 mm with a mounting hole of 3.2 mm 5 mm from each corner.
    fn board() -> BoardInterface {
        let hole = |n: u64, x: i64, y: i64| BoardHole {
            id: id(n),
            name: format!("H{n}"),
            x: mm(x),
            y: mm(y),
            diameter: um(3_200),
        };
        BoardInterface {
            outline: vec![
                (mm(0), mm(0)),
                (mm(60), mm(0)),
                (mm(60), mm(40)),
                (mm(0), mm(40)),
            ],
            holes: vec![
                hole(1, 5, 5),
                hole(2, 55, 5),
                hole(3, 5, 35),
                hole(4, 55, 35),
            ],
            thickness: um(1_600),
            tallest_component: None,
            placement: None,
        }
    }

    fn request() -> PostRequest<'static> {
        PostRequest {
            screw: "M3",
            kind: BoreKind::Insert,
            height: mm(6),
            base: false,
        }
    }

    fn positions(e: &Enclosure) -> Vec<(i64, i64)> {
        let mut v: Vec<(i64, i64)> = e.posts().values().map(|p| (p.x.get(), p.y.get())).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn the_bounds_of_the_outline_are_its_smallest_and_largest_corner() {
        let b = board();
        assert_eq!(b.bounds(), Some(((0, 0), (60_000_000, 40_000_000))));
        let empty = BoardInterface {
            outline: Vec::new(),
            ..board()
        };
        assert_eq!(empty.bounds(), None);
    }

    #[test]
    fn a_board_is_centred_in_the_box_and_gets_a_post_under_each_mounting_hole() {
        let s = Settings::defaults();
        let mut e = enclosure();
        let ops = posts_for_board(&s, &e, &board(), &request()).unwrap();
        assert_eq!(ops.len(), 4);
        e.apply_all(ops).unwrap();
        // the inner space is 100 by 60 mm and the board 60 by 40: it is 20 mm and 10 mm from the
        // walls, so the hole at (5, 5) is at (25, 15) and the one at (55, 35) at (75, 45)
        assert_eq!(
            positions(&e),
            vec![
                (25_000_000, 15_000_000),
                (25_000_000, 45_000_000),
                (75_000_000, 15_000_000),
                (75_000_000, 45_000_000),
            ]
        );
        for (post_id, post) in e.posts() {
            let hole = board()
                .holes
                .into_iter()
                .find(|h| Some(h.id) == post.board_hole)
                .unwrap();
            assert_eq!(*post_id, derived_id(hole.id, 0xA5).unwrap());
            assert_eq!(post.name, format!("post {}", hole.name));
        }
    }

    #[test]
    fn the_placement_of_the_board_moves_the_posts_with_it() {
        let s = Settings::defaults();
        let mut b = board();
        b.placement = Some((mm(10), mm(5)));
        let mut e = enclosure();
        e.apply_all(posts_for_board(&s, &e, &b, &request()).unwrap())
            .unwrap();
        assert_eq!(
            positions(&e),
            vec![
                (15_000_000, 10_000_000),
                (15_000_000, 40_000_000),
                (65_000_000, 10_000_000),
                (65_000_000, 40_000_000),
            ]
        );
    }

    #[test]
    fn a_board_that_cannot_be_mounted_is_refused_for_the_reason() {
        let s = Settings::defaults();
        let e = enclosure();
        // the posts (25 mm), the board (1.6 mm) and the tallest part (5 mm) need 31.6 mm of 30 mm
        let mut b = board();
        b.tallest_component = Some(mm(5));
        let mut tall = request();
        tall.height = mm(25);
        let err = posts_for_board(&s, &e, &b, &tall).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-029");
        let text = err.to_string();
        assert!(text.contains("31.6 mm") && text.contains("30 mm"), "{text}");
        b.tallest_component = None;
        assert!(posts_for_board(&s, &e, &b, &tall).is_ok());
        // a board of 120 mm does not lie in an inner space of 100 mm
        let mut wide = board();
        wide.outline = vec![
            (mm(0), mm(0)),
            (mm(120), mm(0)),
            (mm(120), mm(40)),
            (mm(0), mm(40)),
        ];
        let err = posts_for_board(&s, &e, &wide, &request()).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-028");
        // a hole of 2.5 mm is narrower than the thread of an M3 screw
        let mut narrow = board();
        narrow.holes[0].diameter = um(2_500);
        let err = posts_for_board(&s, &e, &narrow, &request()).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-027");
        // a post 2 mm from the wall, and two posts 6 mm apart
        let mut near_wall = board();
        near_wall.placement = Some((Length::ZERO, Length::ZERO));
        near_wall.holes[0].x = mm(2);
        let err = posts_for_board(&s, &e, &near_wall, &request()).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-028");
        let mut close = board();
        close.holes[1].x = mm(11);
        let err = posts_for_board(&s, &e, &close, &request()).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-028");
        // an outline without points, and a screw that is not in the table
        let mut empty = board();
        empty.outline.clear();
        assert_eq!(
            posts_for_board(&s, &e, &empty, &request())
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-007"
        );
        let mut unknown = request();
        unknown.screw = "M99";
        assert_eq!(
            posts_for_board(&s, &e, &board(), &unknown)
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-021"
        );
    }

    #[test]
    fn a_hole_of_the_floor_under_a_mounting_hole_stops_the_posts() {
        let s = Settings::defaults();
        let mut e = enclosure();
        e.apply(Operation::AddHole {
            id: id(70),
            hole: Hole {
                name: "drain".into(),
                face: Face::Bottom,
                u: mm(27),
                v: mm(47),
                diameter: mm(6),
                style: HoleStyle::Plain,
            },
        })
        .unwrap();
        let err = posts_for_board(&s, &e, &board(), &request()).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-028");
        assert!(err.to_string().contains("drain"));
    }

    #[test]
    fn the_id_of_a_post_is_made_from_the_id_of_its_hole_and_back() {
        let hole = id(7);
        let post_id = derived_id(hole, 0xA5).unwrap();
        assert_ne!(post_id, hole);
        assert_eq!(derived_id(post_id, 0xA5).unwrap(), hole);
        // a salt of zero would change nothing, so it is taken as the usual one
        assert_eq!(derived_id(hole, 0).unwrap(), post_id);
        // the same time and counter, only the random part differs
        assert_eq!(post_id.counter(), hole.counter());
        assert_eq!(post_id.as_bytes()[..8], hole.as_bytes()[..8]);
    }

    struct Rng(u64);

    impl Rng {
        fn draw(&mut self, below: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % below
        }
    }

    /// Whatever sequence of operations is tried, a refused one changes nothing, and undoing the
    /// accepted ones in the reverse order gives back the box that was started with.
    #[test]
    fn random_operations_are_atomic_and_undo_back_to_the_original() {
        let mut total = 0;
        for seed in 1..=40u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let mut e = enclosure();
            let original = e.clone();
            let mut undo: Vec<Operation> = Vec::new();
            for _ in 0..40 {
                let n = rng.draw(5) + 1;
                let x = mm(3 + rng.draw(95) as i64);
                let y = mm(3 + rng.draw(55) as i64);
                let op = match rng.draw(6) {
                    0 | 1 => Operation::AddPost {
                        id: id(n),
                        post: post(&format!("p{n}"), x, y),
                    },
                    2 => Operation::ReplacePost {
                        id: id(n),
                        post: post(&format!("p{n}"), x, y),
                    },
                    3 => Operation::RemovePost { id: id(n) },
                    4 => {
                        let lip = if rng.draw(2) == 0 { None } else { Some(lip()) };
                        Operation::SetLid(if rng.draw(3) == 0 {
                            None
                        } else {
                            Some(lid(lip))
                        })
                    }
                    _ => {
                        let faces = [Face::Top, Face::Front, Face::Bottom];
                        Operation::AddHole {
                            id: id(100 + n),
                            hole: Hole {
                                name: format!("h{n}"),
                                face: faces[rng.draw(3) as usize],
                                u: mm(10 + rng.draw(80) as i64),
                                v: mm(10 + rng.draw(20) as i64),
                                diameter: mm(4),
                                style: HoleStyle::Plain,
                            },
                        }
                    }
                };
                let before = e.clone();
                match e.apply(op) {
                    Ok(inverse) => {
                        undo.push(inverse);
                        total += 1;
                    }
                    Err(_) => assert_eq!(e, before, "seed {seed}"),
                }
            }
            while let Some(inverse) = undo.pop() {
                e.apply(inverse).unwrap();
            }
            assert_eq!(e, original, "seed {seed}");
        }
        assert!(total > 40, "{total} operations were accepted");
    }
}
