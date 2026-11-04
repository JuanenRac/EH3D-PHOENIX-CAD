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
    /// too close to another is refused with the error of that hole; the design is not changed.
    pub fn for_fabrication(&self, design: &Enclosure) -> Result<Enclosure, CadError> {
        let mut out = Enclosure::new(*design.params())?;
        for (id, hole) in design.holes() {
            let mut hole = hole.clone();
            hole.diameter = self.fabrication_diameter(hole.diameter)?;
            out.apply(Operation::AddHole { id: *id, hole })?;
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
}
