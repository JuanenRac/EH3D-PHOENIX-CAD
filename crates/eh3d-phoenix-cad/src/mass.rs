//! Volume and mass of an enclosure, in integers.
//!
//! Volumes are whole cubic nanometres, masses whole micrograms. The only irrational number is pi,
//! used as the ratio [`PI_NUM`] / [`PI_DEN`] (12 decimals, a relative error below 10^-12), so the
//! same model gives the same figure on every platform. A calculation that does not fit in 128
//! bits is refused instead of wrapping.

use crate::enclosure::{Enclosure, Face, Hole, HoleStyle, Treatment};
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

/// The volume of a hole of the given diameter and style that crosses material of the given
/// thickness, in cubic nanometres: the straight part plus the countersink or the counterbore, or
/// the rectangle or the slot.
pub fn hole_volume_nm3(
    diameter: Length,
    style: HoleStyle,
    thickness: Length,
) -> Result<i128, CadError> {
    let overflow = || CadError::Overflow("volume");
    let (d, t) = (i128::from(diameter.get()), i128::from(thickness.get()));
    match style {
        HoleStyle::Plain => cylinder_volume_nm3(diameter, thickness),
        // the cone of 90 degrees over the cylinder: pi (D - d)^2 (D + 2 d) / 24
        HoleStyle::Countersunk { head_diameter } => {
            let h = i128::from(head_diameter.get());
            let n = (h - d)
                .checked_mul(h - d)
                .and_then(|x| x.checked_mul(h + 2 * d))
                .and_then(|x| x.checked_mul(PI_NUM))
                .ok_or_else(overflow)?;
            cylinder_volume_nm3(diameter, thickness)?
                .checked_add(div_round_half_away(n, 24 * PI_DEN)?)
                .ok_or_else(overflow)
        }
        // the ring between the two circles over the depth: pi (D^2 - d^2) k / 4
        HoleStyle::Counterbored {
            head_diameter,
            depth,
        } => {
            let h = i128::from(head_diameter.get());
            let n = (h * h - d * d)
                .checked_mul(i128::from(depth.get()))
                .and_then(|x| x.checked_mul(PI_NUM))
                .ok_or_else(overflow)?;
            cylinder_volume_nm3(diameter, thickness)?
                .checked_add(div_round_half_away(n, 4 * PI_DEN)?)
                .ok_or_else(overflow)
        }
        HoleStyle::Rectangular { height } => d
            .checked_mul(i128::from(height.get()))
            .and_then(|x| x.checked_mul(t))
            .ok_or_else(overflow),
        // a rectangle between the two half circles plus the circle they make together
        HoleStyle::Oval { length, .. } => {
            let straight = (i128::from(length.get()) - d)
                .checked_mul(d)
                .and_then(|x| x.checked_mul(t))
                .ok_or_else(overflow)?;
            straight
                .checked_add(cylinder_volume_nm3(diameter, thickness)?)
                .ok_or_else(overflow)
        }
    }
}

/// The mass in whole micrograms of a volume of a material of the given density (kilograms per
/// cubic metre), rounded half away from zero. One cubic millimetre of water weighs 1000 micrograms.
pub fn mass_micrograms(volume_nm3: i128, density_kg_m3: u32) -> Result<i128, CadError> {
    let n = volume_nm3
        .checked_mul(i128::from(density_kg_m3))
        .ok_or(CadError::Overflow("mass"))?;
    div_round_half_away(n, 1_000_000_000_000_000_000)
}

/// The volume taken from the four vertical edges by a treatment, for a box `z` high.
fn vertical_edges_volume(t: Treatment, z: i128) -> Result<i128, CadError> {
    let overflow = CadError::Overflow("volume");
    match t {
        Treatment::Sharp => Ok(0),
        // four triangles of s^2 / 2 along the height
        Treatment::Chamfer(s) => {
            let s = i128::from(s.get());
            s.checked_mul(s)
                .and_then(|x| x.checked_mul(z))
                .and_then(|x| x.checked_mul(2))
                .ok_or(overflow)
        }
        // four corners of s^2 (1 - pi / 4) along the height
        Treatment::Round(s) => {
            let s = i128::from(s.get());
            let n = s
                .checked_mul(s)
                .and_then(|x| x.checked_mul(z))
                .and_then(|x| x.checked_mul(4 * PI_DEN - PI_NUM))
                .ok_or(overflow)?;
            div_round_half_away(n, PI_DEN)
        }
    }
}

/// The volume taken from the four edges around the lid (or the floor) by a treatment, for a box
/// whose length and width add up to `xy`. The strips meet at the corners in a mitre.
fn rim_edges_volume(t: Treatment, xy: i128) -> Result<i128, CadError> {
    let overflow = CadError::Overflow("volume");
    match t {
        Treatment::Sharp => Ok(0),
        // c^2 (X + Y) - 4 c^3 / 3
        Treatment::Chamfer(c) => {
            let c = i128::from(c.get());
            let first = c
                .checked_mul(c)
                .and_then(|x| x.checked_mul(xy))
                .and_then(|x| x.checked_mul(3))
                .ok_or(CadError::Overflow("volume"))?;
            let second = c
                .checked_mul(c)
                .and_then(|x| x.checked_mul(c))
                .and_then(|x| x.checked_mul(4))
                .ok_or(overflow)?;
            div_round_half_away(first - second, 3)
        }
        // (X + Y) r^2 (4 - pi) / 2 - 20 r^3 / 3 + 2 pi r^3
        Treatment::Round(r) => {
            let r = i128::from(r.get());
            let r2 = r.checked_mul(r).ok_or(CadError::Overflow("volume"))?;
            let r3 = r2.checked_mul(r).ok_or(CadError::Overflow("volume"))?;
            let first = r2
                .checked_mul(xy)
                .and_then(|x| x.checked_mul(3 * (4 * PI_DEN - PI_NUM)))
                .ok_or(CadError::Overflow("volume"))?;
            let second = r3.checked_mul(12 * PI_NUM - 40 * PI_DEN).ok_or(overflow)?;
            div_round_half_away(first + second, 6 * PI_DEN)
        }
    }
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
        let (su, sv) = hole.style.footprint(hole.diameter);
        let (u, v) = (i128::from(hole.u.get()), i128::from(hole.v.get()));
        // 2 * centre -/+ size against 2 * bounds, so a half nanometre never matters
        let within =
            |c: i128, lo: i128, hi: i128, e: i128| 2 * c - e >= 2 * lo && 2 * c + e <= 2 * hi;
        let (x, y) = (i128::from(x.get()), i128::from(y.get()));
        match hole.face {
            Face::Front | Face::Back => {
                within(u, wall, x - wall, su) && within(v, floor, floor + inner_h, sv)
            }
            Face::Left | Face::Right => {
                within(u, wall, y - wall, su) && within(v, floor, floor + inner_h, sv)
            }
            Face::Top | Face::Bottom => {
                within(u, wall, x - wall, su) && within(v, wall, y - wall, sv)
            }
        }
    }

    /// The volume of the holes: each is as long as the thin part of its face is thick, with its
    /// countersink or counterbore when it has one.
    pub fn holes_volume(&self) -> Result<Volume, CadError> {
        let mut total: i128 = 0;
        let mut exact = true;
        for hole in self.holes().values() {
            let v = hole_volume_nm3(hole.diameter, hole.style, self.thin_depth(hole.face))?;
            total = total.checked_add(v).ok_or(CadError::Overflow("volume"))?;
            exact &= self.hole_in_thin_part(hole);
        }
        Ok(Volume { nm3: total, exact })
    }

    /// The volume that rounded and chamfered edges take from the outer box. It is exact when the
    /// vertical edges are left sharp or when the edges of the lid and of the floor are; when both
    /// kinds are treated the strips overlap at the corners, the overlap is counted twice and the
    /// figure says it is not exact.
    pub fn edging_volume(&self) -> Result<Volume, CadError> {
        let edging = self.edging();
        let (x, y, z) = self.params().outer();
        let xy = i128::from(x.get()) + i128::from(y.get());
        let vertical = vertical_edges_volume(edging.vertical, i128::from(z.get()))?;
        let top = rim_edges_volume(edging.top, xy)?;
        let bottom = rim_edges_volume(edging.bottom, xy)?;
        let total = vertical
            .checked_add(top)
            .and_then(|v| v.checked_add(bottom))
            .ok_or(CadError::Overflow("volume"))?;
        let overlap =
            !edging.vertical.is_sharp() && (!edging.top.is_sharp() || !edging.bottom.is_sharp());
        Ok(Volume {
            nm3: total,
            exact: !overlap,
        })
    }

    /// The volume of material: the outer box, minus the inner space, minus the treatment of the
    /// edges, minus the holes.
    pub fn material_volume_with_holes(&self) -> Result<Volume, CadError> {
        let holes = self.holes_volume()?;
        let edges = self.edging_volume()?;
        Ok(Volume {
            nm3: self.material_volume_nm3() - edges.nm3 - holes.nm3,
            exact: holes.exact && edges.exact,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::{Edging, EnclosureParams, Operation, Treatment};
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
                style: crate::enclosure::HoleStyle::Plain,
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

    fn edged(edging: Edging) -> Enclosure {
        let mut e = Enclosure::new(params(3)).unwrap();
        e.apply(Operation::SetEdging(edging)).unwrap();
        e
    }

    const MM3: i128 = 1_000_000_000_000_000_000;

    #[test]
    fn chamfered_and_rounded_vertical_edges_take_the_volume_of_their_corners() {
        let chamfer = edged(Edging {
            vertical: Treatment::Chamfer(mm(3)),
            ..Edging::default()
        });
        // four triangles of 4.5 square millimetres along 35 millimetres
        let v = chamfer.edging_volume().unwrap();
        assert_eq!((v.nm3, v.exact), (630 * MM3, true));
        let round = edged(Edging {
            vertical: Treatment::Round(mm(5)),
            ..Edging::default()
        });
        // four times 25 (1 - pi / 4) square millimetres along 35 millimetres
        assert_eq!(
            round.edging_volume().unwrap().nm3,
            751_106_428_108_750_000_000
        );
        let plain = Enclosure::new(params(3)).unwrap();
        assert_eq!(plain.edging_volume().unwrap().nm3, 0);
        assert_eq!(
            round.material_volume_with_holes().unwrap().nm3,
            round.material_volume_nm3() - 751_106_428_108_750_000_000
        );
    }

    #[test]
    fn the_edges_of_the_lid_are_mitred_at_the_corners() {
        let chamfer = edged(Edging {
            top: Treatment::Chamfer(mm(2)),
            ..Edging::default()
        });
        assert_eq!(
            chamfer.edging_volume().unwrap().nm3,
            661_333_333_333_333_333_333
        );
        let round = edged(Edging {
            top: Treatment::Round(mm(2)),
            bottom: Treatment::Sharp,
            ..Edging::default()
        });
        assert_eq!(
            round.edging_volume().unwrap().nm3,
            285_357_017_517_866_666_667
        );
        // the floor has the same rim, so treating both takes twice as much
        let both = edged(Edging {
            top: Treatment::Round(mm(2)),
            bottom: Treatment::Round(mm(2)),
            ..Edging::default()
        });
        assert_eq!(
            both.edging_volume().unwrap().nm3,
            2 * 285_357_017_517_866_666_667
        );
    }

    #[test]
    fn treating_vertical_edges_and_the_rim_together_is_marked_as_not_exact() {
        let both = edged(Edging {
            vertical: Treatment::Round(mm(5)),
            top: Treatment::Chamfer(mm(2)),
            bottom: Treatment::Sharp,
        });
        let v = both.edging_volume().unwrap();
        assert!(!v.exact);
        assert_eq!(
            v.nm3,
            751_106_428_108_750_000_000 + 661_333_333_333_333_333_333
        );
        assert!(!both.material_volume_with_holes().unwrap().exact);
    }

    fn styled(style: HoleStyle, diameter: i64, face: Face) -> Enclosure {
        let mut e = Enclosure::new(params(3)).unwrap();
        e.apply(Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, 1, 1).unwrap(),
            hole: Hole {
                name: "s".into(),
                face,
                u: mm(50),
                v: mm(17),
                diameter: Length::new(diameter).unwrap(),
                style,
            },
        })
        .unwrap();
        e
    }

    #[test]
    fn a_countersink_adds_its_cone_to_the_cylinder() {
        let e = styled(
            HoleStyle::Countersunk {
                head_diameter: Length::new(6_300_000).unwrap(),
            },
            3_400_000,
            Face::Front,
        );
        let v = e.holes_volume().unwrap();
        assert_eq!(v.nm3, 32_579_755_714_361_195_417);
        assert!(v.exact);
    }

    #[test]
    fn a_counterbore_adds_its_ring_to_the_cylinder() {
        let e = styled(
            HoleStyle::Counterbored {
                head_diameter: mm(6),
                depth: mm(1),
            },
            3_400_000,
            Face::Front,
        );
        assert_eq!(e.holes_volume().unwrap().nm3, 37_353_536_651_185_100_000);
    }

    #[test]
    fn a_rectangular_hole_and_a_slot_have_their_own_volume() {
        let rect = styled(
            HoleStyle::Rectangular { height: mm(4) },
            10_000_000,
            Face::Front,
        );
        assert_eq!(rect.holes_volume().unwrap().nm3, 80 * MM3);
        let slot = styled(
            HoleStyle::Oval {
                length: mm(10),
                vertical: false,
            },
            4_000_000,
            Face::Front,
        );
        assert_eq!(slot.holes_volume().unwrap().nm3, 73_132_741_228_720_000_000);
        // the slot may run along v as well, with the same volume
        let upright = styled(
            HoleStyle::Oval {
                length: mm(10),
                vertical: true,
            },
            4_000_000,
            Face::Front,
        );
        assert_eq!(
            upright.holes_volume().unwrap(),
            slot.holes_volume().unwrap()
        );
    }

    #[test]
    fn a_wide_head_that_reaches_the_floor_is_marked_as_not_exact() {
        // the countersink is 6.5 mm wide and 4 mm above the bottom, so the opening reaches the floor
        let mut e = Enclosure::new(params(0)).unwrap();
        e.apply(Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, 1, 1).unwrap(),
            hole: Hole {
                name: "s".into(),
                face: Face::Front,
                u: mm(50),
                v: mm(4),
                diameter: mm(3),
                style: HoleStyle::Countersunk {
                    head_diameter: Length::new(6_500_000).unwrap(),
                },
            },
        })
        .unwrap();
        assert!(!e.holes_volume().unwrap().exact);
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
