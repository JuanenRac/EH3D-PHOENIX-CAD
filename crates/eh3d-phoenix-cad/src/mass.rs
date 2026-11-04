//! Volume and mass of an enclosure, in integers.
//!
//! Volumes are whole cubic nanometres, masses whole micrograms. The only irrational number is pi,
//! used as the ratio [`PI_NUM`] / [`PI_DEN`] (12 decimals, a relative error below 10^-12), so the
//! same model gives the same figure on every platform. A calculation that does not fit in 128
//! bits is refused instead of wrapping.

use crate::enclosure::{Enclosure, Face, Hole};
use crate::units::div_round_half_away;
use crate::{CadError, Length};

/// The numerator of the ratio used for pi.
pub const PI_NUM: i128 = 3_141_592_653_590;

/// The denominator of the ratio used for pi.
pub const PI_DEN: i128 = 1_000_000_000_000;

/// A volume with the statement of how far it can be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume {
    /// Cubic nanometres.
    pub nm3: i128,
    /// True when every hole lies entirely in the thin part of its face, so that the figure is the
    /// whole volume of the holes; false when some hole reaches a corner or a thicker part, and the
    /// figure of the holes is then a lower bound (the box material is an upper bound).
    pub exact: bool,
}

/// The volume of a round cylinder in cubic nanometres, rounded half away from zero.
pub fn cylinder_volume_nm3(diameter: Length, height: Length) -> Result<i128, CadError> {
    let d = i128::from(diameter.get());
    let h = i128::from(height.get());
    let n = d
        .checked_mul(d)
        .and_then(|x| x.checked_mul(h))
        .and_then(|x| x.checked_mul(PI_NUM))
        .ok_or(CadError::Overflow("volume"))?;
    div_round_half_away(n, 4 * PI_DEN)
}

/// The mass in whole micrograms of a volume of a material of the given density (kilograms per
/// cubic metre), rounded half away from zero. One cubic millimetre of water weighs 1000 micrograms.
pub fn mass_micrograms(volume_nm3: i128, density_kg_m3: u32) -> Result<i128, CadError> {
    let n = volume_nm3
        .checked_mul(i128::from(density_kg_m3))
        .ok_or(CadError::Overflow("mass"))?;
    div_round_half_away(n, 1_000_000_000_000_000_000)
}

impl Enclosure {
    /// The volume of the outer box, without removing the inner space or the holes.
    pub fn outer_volume_nm3(&self) -> i128 {
        let (x, y, z) = self.params().outer();
        i128::from(x.get()) * i128::from(y.get()) * i128::from(z.get())
    }

    /// The thickness of the material that a hole of this face crosses where the face is thin: the
    /// wall for the four sides, the lid for the top and the floor for the bottom.
    fn thin_depth(&self, face: Face) -> Length {
        match face {
            Face::Front | Face::Back | Face::Left | Face::Right => self.params().wall,
            Face::Top => self.params().lid,
            Face::Bottom => self.params().floor,
        }
    }

    /// Whether the circle of a hole lies in the thin part of its face, away from the corners
    /// where the walls meet and from the floor and the lid.
    fn hole_in_thin_part(&self, hole: &Hole) -> bool {
        let p = self.params();
        let (x, y, _) = p.outer();
        let (wall, floor) = (i128::from(p.wall.get()), i128::from(p.floor.get()));
        let inner_h = i128::from(p.inner_height.get());
        let d = i128::from(hole.diameter.get());
        let (u, v) = (i128::from(hole.u.get()), i128::from(hole.v.get()));
        // 2 * centre -/+ diameter against 2 * bounds, so a half nanometre never matters
        let within = |c: i128, lo: i128, hi: i128| 2 * c - d >= 2 * lo && 2 * c + d <= 2 * hi;
        let (x, y) = (i128::from(x.get()), i128::from(y.get()));
        match hole.face {
            Face::Front | Face::Back => {
                within(u, wall, x - wall) && within(v, floor, floor + inner_h)
            }
            Face::Left | Face::Right => {
                within(u, wall, y - wall) && within(v, floor, floor + inner_h)
            }
            Face::Top | Face::Bottom => within(u, wall, x - wall) && within(v, wall, y - wall),
        }
    }

    /// The volume of the holes: each is a cylinder as long as the thin part of its face is thick.
    pub fn holes_volume(&self) -> Result<Volume, CadError> {
        let mut total: i128 = 0;
        let mut exact = true;
        for hole in self.holes().values() {
            let v = cylinder_volume_nm3(hole.diameter, self.thin_depth(hole.face))?;
            total = total.checked_add(v).ok_or(CadError::Overflow("volume"))?;
            exact &= self.hole_in_thin_part(hole);
        }
        Ok(Volume { nm3: total, exact })
    }

    /// The volume of material: the outer box, minus the inner space, minus the holes.
    pub fn material_volume_with_holes(&self) -> Result<Volume, CadError> {
        let holes = self.holes_volume()?;
        Ok(Volume {
            nm3: self.material_volume_nm3() - holes.nm3,
            exact: holes.exact,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::{EnclosureParams, Operation};
    use eh3d_container::ObjectId;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn params(margin: i64) -> EnclosureParams {
        EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(margin),
            min_web: mm(2),
        }
    }

    fn with_hole(margin: i64, face: Face, u: i64, v: i64, d: i64) -> Enclosure {
        let mut e = Enclosure::new(params(margin)).unwrap();
        e.apply(Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, 1, 1).unwrap(),
            hole: Hole {
                name: "h".into(),
                face,
                u: mm(u),
                v: mm(v),
                diameter: mm(d),
            },
        })
        .unwrap();
        e
    }

    #[test]
    fn a_cylinder_has_the_volume_of_pi_r_squared_h() {
        // pi * 5^2 * 2 = 157.0796... cubic millimetres
        assert_eq!(
            cylinder_volume_nm3(mm(10), mm(2)).unwrap(),
            157_079_632_679_500_000_000
        );
        assert_eq!(cylinder_volume_nm3(mm(10), Length::ZERO).unwrap(), 0);
    }

    #[test]
    fn a_cylinder_too_large_for_the_integers_is_refused() {
        let big = Length::new((1 << 40) - 1).unwrap();
        assert_eq!(
            cylinder_volume_nm3(big, big).unwrap_err(),
            CadError::Overflow("volume")
        );
    }

    #[test]
    fn mass_follows_from_the_volume_and_the_density() {
        // 1 cubic millimetre of water is 1 mg = 1000 micrograms
        assert_eq!(
            mass_micrograms(1_000_000_000_000_000_000, 1000).unwrap(),
            1000
        );
        // 1 cubic centimetre of a plastic of 1240 kg/m3 is 1.24 g
        assert_eq!(
            mass_micrograms(1_000_000_000_000_000_000_000, 1240).unwrap(),
            1_240_000
        );
        assert_eq!(mass_micrograms(0, 1240).unwrap(), 0);
        assert_eq!(
            mass_micrograms(i128::MAX, 2).unwrap_err(),
            CadError::Overflow("mass")
        );
    }

    #[test]
    fn the_empty_box_has_the_outer_volume_minus_the_inner_space() {
        let e = Enclosure::new(params(3)).unwrap();
        let outer = 104i128 * 64 * 35;
        assert_eq!(e.outer_volume_nm3(), outer * 1_000_000_000_000_000_000);
        let v = e.material_volume_with_holes().unwrap();
        assert_eq!(v.nm3, e.material_volume_nm3());
        assert!(v.exact);
    }

    #[test]
    fn a_hole_in_a_side_removes_a_cylinder_as_long_as_the_wall_is_thick() {
        let e = with_hole(3, Face::Front, 50, 17, 10);
        let holes = e.holes_volume().unwrap();
        assert_eq!(holes.nm3, cylinder_volume_nm3(mm(10), mm(2)).unwrap());
        assert!(holes.exact);
        let material = e.material_volume_with_holes().unwrap();
        assert_eq!(material.nm3, e.material_volume_nm3() - holes.nm3);
    }

    #[test]
    fn holes_of_the_lid_and_the_floor_use_their_own_thickness() {
        let top = with_hole(3, Face::Top, 52, 32, 10);
        assert_eq!(
            top.holes_volume().unwrap().nm3,
            cylinder_volume_nm3(mm(10), mm(3)).unwrap()
        );
        let bottom = with_hole(3, Face::Bottom, 52, 32, 10);
        assert_eq!(
            bottom.holes_volume().unwrap().nm3,
            cylinder_volume_nm3(mm(10), mm(2)).unwrap()
        );
        assert!(top.holes_volume().unwrap().exact);
    }

    #[test]
    fn a_hole_that_reaches_the_floor_or_a_corner_is_marked_as_not_exact() {
        // no margin: a 6 mm hole centred 3 mm above the bottom reaches the floor
        let low = with_hole(0, Face::Front, 50, 3, 6);
        assert!(!low.holes_volume().unwrap().exact);
        // a hole of the front face that reaches the corner where the left wall is
        let corner = with_hole(0, Face::Front, 4, 17, 6);
        assert!(!corner.holes_volume().unwrap().exact);
        // a hole of the top that reaches the wall column
        let edge = with_hole(0, Face::Top, 4, 32, 6);
        assert!(!edge.holes_volume().unwrap().exact);
    }
}
