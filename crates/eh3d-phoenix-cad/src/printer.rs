//! The profile of a 3D printer: the compensation of holes and the clearance of parts that fit.
//!
//! The model of the design keeps the diameters that the person asked for. The model for
//! fabrication is another enclosure, made from it, whose holes are wider by the values of the
//! profile (a printed hole comes out smaller than drawn, and small holes more so). The values
//! come from the settings file ([`crate::config`]); none is written here.

use crate::enclosure::{Enclosure, Operation};
use crate::{CadError, Length};

/// The compensation of a printer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrinterProfile {
    /// Added to the diameter of every hole.
    pub hole_compensation: Length,
    /// A hole narrower than this gets `small_hole_extra` as well.
    pub small_hole_limit: Length,
    /// The extra compensation of narrow holes.
    pub small_hole_extra: Length,
    /// The gap to leave between two parts that must fit, such as a lid and its box.
    pub fit_clearance: Length,
}

impl PrinterProfile {
    /// The diameter to print for a hole of the given diameter in the design.
    pub fn fabrication_diameter(&self, designed: Length) -> Result<Length, CadError> {
        let mut d = designed.checked_add(self.hole_compensation)?;
        if designed < self.small_hole_limit {
            d = d.checked_add(self.small_hole_extra)?;
        }
        Ok(d)
    }

    /// The enclosure for fabrication: the same parameters and the same holes, under the same ids,
    /// with the compensated diameters. A compensation that makes a hole leave its face or come
    /// too close to another is refused with the error of that hole; the design is not changed. A
    /// design with an extrusion or a pocket of a sketch is refused with `PHOENIX-CAD-022`.
    pub fn for_fabrication(&self, design: &Enclosure) -> Result<Enclosure, CadError> {
        if !design.features().is_empty() {
            return Err(CadError::NotExported(
                "the extrusions and pockets of sketches".into(),
            ));
        }
        let mut out = Enclosure::new(*design.params())?;
        out.apply(Operation::SetEdging(*design.edging()))?;
        for (id, hole) in design.holes() {
            let mut hole = hole.clone();
            let widened = self.fabrication_diameter(hole.diameter)?;
            // every size across of the hole grows by the same amount as its diameter
            hole.style = hole.style.grown(widened.checked_sub(hole.diameter)?)?;
            hole.diameter = widened;
            out.apply(Operation::AddHole { id: *id, hole })?;
        }
        // the posts keep the bores of the table (an insert or a screw sets the size, not the
        // printer); the holes of the lid are widened like any other hole
        for (id, post) in design.posts() {
            out.apply(Operation::AddPost {
                id: *id,
                post: post.clone(),
            })?;
        }
        if let Some(lid) = design.lid() {
            let mut lid = lid.clone();
            let widened = self.fabrication_diameter(lid.hole_diameter)?;
            lid.style = lid.style.grown(widened.checked_sub(lid.hole_diameter)?)?;
            lid.hole_diameter = widened;
            out.apply(Operation::SetLid(Some(lid)))?;
        }
        // the cooling fins are not holes: they are printed as designed, but they have to stay
        // clear of the holes once these are widened
        for (id, fins) in design.fins() {
            out.apply(Operation::AddFins {
                id: *id,
                fins: fins.clone(),
            })?;
        }
        // placed parts are not printed by this model: they stay where they are
        for (id, part) in design.parts() {
            out.apply(Operation::AddPart {
                id: *id,
                part: part.clone(),
            })?;
        }
        // the mates keep the parts relative to each other
        for (id, mate) in design.mates() {
            out.apply(Operation::AddMate {
                id: *id,
                mate: *mate,
            })?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::{EnclosureParams, Face, Hole};
    use eh3d_container::ObjectId;

    fn um(v: i64) -> Length {
        Length::from_um(v).unwrap()
    }

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn profile() -> PrinterProfile {
        PrinterProfile {
            hole_compensation: um(150),
            small_hole_limit: mm(3),
            small_hole_extra: um(100),
            fit_clearance: um(200),
        }
    }

    fn design() -> Enclosure {
        let mut e = Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap();
        for (n, u, d) in [(1u64, 20i64, 2i64), (2, 50, 6)] {
            e.apply(Operation::AddHole {
                id: ObjectId::from_parts(1_700_000_000_000, 1, n).unwrap(),
                hole: Hole {
                    name: format!("h{n}"),
                    face: Face::Front,
                    u: mm(u),
                    v: mm(17),
                    diameter: mm(d),
                    style: crate::enclosure::HoleStyle::Plain,
                },
            })
            .unwrap();
        }
        e
    }

    #[test]
    fn narrow_holes_get_the_extra_compensation_and_wide_ones_do_not() {
        let p = profile();
        assert_eq!(p.fabrication_diameter(mm(6)).unwrap(), um(6_150));
        assert_eq!(p.fabrication_diameter(mm(2)).unwrap(), um(2_250));
        // exactly at the limit is not narrow
        assert_eq!(p.fabrication_diameter(mm(3)).unwrap(), um(3_150));
    }

    #[test]
    fn the_model_for_fabrication_keeps_the_ids_and_widens_the_holes() {
        let d = design();
        let before = d.clone();
        let f = profile().for_fabrication(&d).unwrap();
        assert_eq!(d, before, "the design is not changed");
        assert_eq!(f.params(), d.params());
        assert_eq!(
            f.holes().keys().collect::<Vec<_>>(),
            d.holes().keys().collect::<Vec<_>>()
        );
        let widths: Vec<i64> = f.holes().values().map(|h| h.diameter.get()).collect();
        assert_eq!(widths, vec![2_250_000, 6_150_000]);
        let positions_equal = f
            .holes()
            .values()
            .zip(d.holes().values())
            .all(|(a, b)| (a.face, a.u, a.v) == (b.face, b.u, b.v));
        assert!(positions_equal);
    }

    #[test]
    fn a_compensation_that_breaks_a_rule_is_refused_with_the_error_of_the_hole() {
        let mut p = profile();
        // so wide that the two holes (30 mm apart) and the edges no longer keep their rules
        p.hole_compensation = mm(40);
        let err = p.for_fabrication(&design()).unwrap_err();
        assert!(matches!(err.code(), "PHOENIX-CAD-003" | "PHOENIX-CAD-004"));
    }

    #[test]
    fn the_whole_opening_of_a_slot_is_widened_by_the_same_amount_as_its_width() {
        use crate::enclosure::HoleStyle;
        let mut e = Enclosure::new(*design().params()).unwrap();
        e.apply(Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, 1, 9).unwrap(),
            hole: Hole {
                name: "slot".into(),
                face: Face::Front,
                u: mm(40),
                v: mm(17),
                diameter: mm(4),
                style: HoleStyle::Oval {
                    length: mm(12),
                    vertical: false,
                },
            },
        })
        .unwrap();
        let fab = profile().for_fabrication(&e).unwrap();
        let hole = fab.holes().values().next().unwrap();
        // a width of 4 mm is wide enough to get only the compensation of 0.15 mm
        assert_eq!(hole.diameter, um(4_150));
        assert_eq!(
            hole.style,
            HoleStyle::Oval {
                length: um(12_150),
                vertical: false
            }
        );
        // the design is not changed
        assert_eq!(e.holes().values().next().unwrap().diameter, mm(4));
    }

    #[test]
    fn the_edges_of_the_design_are_kept_in_the_model_for_fabrication() {
        use crate::enclosure::{Edging, Treatment};
        let mut e = design();
        let edging = Edging {
            vertical: Treatment::Round(mm(4)),
            ..Edging::default()
        };
        e.apply(Operation::SetEdging(edging)).unwrap();
        let fab = profile().for_fabrication(&e).unwrap();
        assert_eq!(fab.edging(), &edging);
    }

    #[test]
    fn the_posts_keep_their_bores_and_the_holes_of_the_lid_are_widened() {
        use crate::enclosure::HoleStyle;
        use crate::mount::{BoreKind, Lid, LidFixing, Post};
        let mut e = design();
        let post = Post {
            name: "p".into(),
            board_hole: None,
            x: mm(25),
            y: mm(45),
            outer_diameter: mm(8),
            height: mm(6),
            kind: BoreKind::Insert,
            bore_diameter: um(4_200),
            bore_depth: mm(5),
            base: None,
        };
        e.apply(Operation::AddPost {
            id: ObjectId::from_parts(1_700_000_000_000, 1, 90).unwrap(),
            post: post.clone(),
        })
        .unwrap();
        e.apply(Operation::SetLid(Some(Lid {
            fixing: LidFixing::Auto { inset: mm(6) },
            screw: "M3".into(),
            hole_diameter: um(3_400),
            style: HoleStyle::Countersunk {
                head_diameter: um(6_300),
            },
            clearance: um(200),
            lip: None,
        })))
        .unwrap();
        let before = e.clone();
        let f = profile().for_fabrication(&e).unwrap();
        assert_eq!(e, before, "the design is not changed");
        assert_eq!(f.posts().values().next(), Some(&post));
        // 3.4 mm is not narrower than the limit of 3 mm: it gets the compensation of 0.15 mm, and
        // the head of the countersink grows by the same amount
        let lid = f.lid().unwrap();
        assert_eq!(lid.hole_diameter, um(3_550));
        assert_eq!(
            lid.style,
            HoleStyle::Countersunk {
                head_diameter: um(6_450)
            }
        );
        // the holes it draws follow the widened fixing, over the post
        let holes = f.lid_holes();
        assert_eq!(holes.len(), 1);
        assert_eq!(holes[0].diameter, um(3_550));
        assert_eq!(holes[0].u, mm(27));
    }

    fn catalog_part(kind: &str) -> crate::catalog::Part {
        use crate::catalog::{Head, Part, Shape};
        let um = |v: i64| Length::from_um(v).unwrap();
        match kind {
            "tube" => Part {
                name: "tube-20x20".into(),
                material: "aluminium".into(),
                shape: Shape::Profile {
                    width: um(20_000),
                    height: um(20_000),
                    wall: um(1_500),
                    default_cut: um(300_000),
                    max_cut: um(2_000_000),
                },
            },
            "bolt" => Part {
                name: "bolt".into(),
                material: "steel".into(),
                shape: Shape::Screw {
                    metric: "M3".into(),
                    diameter: um(3_000),
                    length: um(10_000),
                    head: Head::Cylinder,
                    head_diameter: um(5_500),
                    head_height: um(3_000),
                },
            },
            "flat" => Part {
                name: "flat".into(),
                material: "steel".into(),
                shape: Shape::Screw {
                    metric: "M3".into(),
                    diameter: um(3_000),
                    length: um(8_000),
                    head: Head::Countersunk,
                    head_diameter: um(6_000),
                    head_height: um(1_700),
                },
            },
            "hex" => Part {
                name: "hex".into(),
                material: "stainless".into(),
                shape: Shape::Screw {
                    metric: "M4".into(),
                    diameter: um(4_000),
                    length: um(16_000),
                    head: Head::Hex,
                    head_diameter: um(7_000),
                    head_height: um(2_800),
                },
            },
            _ => Part {
                name: "angle-30".into(),
                material: "aluminium".into(),
                shape: Shape::Bracket {
                    leg_a: um(30_000),
                    leg_b: um(30_000),
                    width: um(20_000),
                    thickness: um(3_000),
                    hole_diameter: um(4_500),
                },
            },
        }
    }

    fn place(
        e: &mut Enclosure,
        n: u64,
        kind: &str,
        at: [i64; 3],
        axis: crate::catalog::Axis,
        roll: u8,
        cut: Option<i64>,
    ) {
        use crate::catalog::{Orientation, PartInstance};
        e.apply(Operation::AddPart {
            id: ObjectId::from_parts(1_700_000_000_000, n as u16, n).unwrap(),
            part: PartInstance {
                name: format!("piece {n}"),
                library: "lib".into(),
                part: catalog_part(kind),
                at: at.map(mm),
                orientation: Orientation { axis, roll },
                cut: cut.map(mm),
            },
        })
        .unwrap();
    }

    #[test]
    fn the_parts_stay_where_they_are_in_the_model_for_fabrication() {
        use crate::catalog::Axis;
        let mut e = design();
        place(&mut e, 1, "tube", [0, 0, 0], Axis::PlusZ, 0, Some(300));
        place(&mut e, 2, "bolt", [100, 100, 100], Axis::PlusZ, 0, None);
        let f = profile().for_fabrication(&e).unwrap();
        assert_eq!(f.parts(), e.parts());
    }
}
