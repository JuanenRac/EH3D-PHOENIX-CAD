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

// ---------------------------------------------------------------------------------------------
// Centre of mass and moments of inertia
// ---------------------------------------------------------------------------------------------

/// An unsigned 256-bit integer (four 64-bit limbs, least significant first). The moments of a box
/// of a hundred millimetres already need more than 128 bits when written in nanometres, and a
/// moment must not be rounded before the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct U256([u64; 4]);

impl U256 {
    const ZERO: U256 = U256([0; 4]);

    fn from_u128(v: u128) -> U256 {
        U256([v as u64, (v >> 64) as u64, 0, 0])
    }

    fn to_u128(self) -> Option<u128> {
        if self.0[2] != 0 || self.0[3] != 0 {
            None
        } else {
            Some(u128::from(self.0[0]) | (u128::from(self.0[1]) << 64))
        }
    }

    fn add(&self, other: &U256) -> Option<U256> {
        let mut out = [0u64; 4];
        let mut carry = false;
        for ((r, a), b) in out.iter_mut().zip(self.0.iter()).zip(other.0.iter()) {
            let (s1, c1) = a.overflowing_add(*b);
            let (s2, c2) = s1.overflowing_add(u64::from(carry));
            *r = s2;
            carry = c1 || c2;
        }
        if carry {
            None
        } else {
            Some(U256(out))
        }
    }

    /// The difference, or `None` when `other` is larger.
    fn sub(&self, other: &U256) -> Option<U256> {
        let mut out = [0u64; 4];
        let mut borrow = false;
        for ((r, a), b) in out.iter_mut().zip(self.0.iter()).zip(other.0.iter()) {
            let (s1, c1) = a.overflowing_sub(*b);
            let (s2, c2) = s1.overflowing_sub(u64::from(borrow));
            *r = s2;
            borrow = c1 || c2;
        }
        if borrow {
            None
        } else {
            Some(U256(out))
        }
    }

    fn mul_u64(&self, m: u64) -> Option<U256> {
        let mut out = [0u64; 4];
        let mut carry: u128 = 0;
        for (r, a) in out.iter_mut().zip(self.0.iter()) {
            let t = u128::from(*a) * u128::from(m) + carry;
            *r = t as u64;
            carry = t >> 64;
        }
        if carry != 0 {
            None
        } else {
            Some(U256(out))
        }
    }

    fn mul_u128(&self, m: u128) -> Option<U256> {
        let low = self.mul_u64(m as u64)?;
        let high_factor = (m >> 64) as u64;
        if high_factor == 0 {
            return Some(low);
        }
        let high = self.mul_u64(high_factor)?;
        if high.0[3] != 0 {
            return None;
        }
        let shifted = U256([0, high.0[0], high.0[1], high.0[2]]);
        low.add(&shifted)
    }

    fn bit(&self, i: usize) -> bool {
        (self.0[i / 64] >> (i % 64)) & 1 == 1
    }

    /// The quotient and the remainder of a division by a number below 2^126.
    fn div_rem_u128(&self, d: u128) -> Option<(U256, u128)> {
        if d == 0 || d >= 1 << 126 {
            return None;
        }
        let mut quotient = [0u64; 4];
        let mut rem: u128 = 0;
        for i in (0..256).rev() {
            rem = (rem << 1) | u128::from(self.bit(i));
            if rem >= d {
                rem -= d;
                quotient[i / 64] |= 1u64 << (i % 64);
            }
        }
        Some((U256(quotient), rem))
    }

    /// `self / d` rounded half up.
    fn round_div(&self, d: u128) -> Option<U256> {
        let doubled = self.mul_u128(2)?.add(&U256::from_u128(d))?;
        Some(doubled.div_rem_u128(d.checked_mul(2)?)?.0)
    }
}

/// A sum of terms of both signs, kept as the sum of the positive ones and the sum of the negative
/// ones so that nothing is subtracted before the end.
#[derive(Debug, Clone, Copy)]
struct Signed {
    pos: U256,
    neg: U256,
}

impl Signed {
    const ZERO: Signed = Signed {
        pos: U256::ZERO,
        neg: U256::ZERO,
    };

    fn add(&mut self, negative: bool, magnitude: U256) -> Option<()> {
        if negative {
            self.neg = self.neg.add(&magnitude)?;
        } else {
            self.pos = self.pos.add(&magnitude)?;
        }
        Some(())
    }

    /// Adds another sum, or takes it away when `negative`.
    fn merge(&mut self, other: &Signed, negative: bool) -> Option<()> {
        self.add(negative, other.pos)?;
        self.add(!negative, other.neg)
    }

    /// Whether the value is negative, and its size.
    fn value(&self) -> (bool, U256) {
        match self.pos.sub(&self.neg) {
            Some(d) => (false, d),
            None => (true, self.neg.sub(&self.pos).unwrap_or(U256::ZERO)),
        }
    }
}

/// One part of the solid: a box or a cylinder, added or taken away. Lengths are in half
/// nanometres so that the centre of every part is a whole number.
#[derive(Debug, Clone, Copy)]
struct Piece {
    /// Whether the part is taken away from the solid.
    remove: bool,
    /// The volume in cubic nanometres.
    volume: i128,
    /// The centre in half nanometres.
    centre: [i128; 3],
    /// 48 times the variance of the part along each axis, in square half nanometres.
    spread: [i128; 3],
}

/// Where a hole enters the box.
struct Place {
    /// The axis the hole runs along: 0 for X, 1 for Y, 2 for Z.
    axis: usize,
    /// Whether the way into the material goes towards larger values of the axis.
    inward_is_up: bool,
    /// The position of the face along the axis, in nanometres.
    face_at: i128,
    /// The two other axes with the position of the centre of the hole on each, in nanometres:
    /// first the direction of `u` of the face, then the direction of `v`.
    across: [(usize, i128); 2],
}

/// The sums over all the parts.
struct Sums {
    volume: i128,
    m1: [Signed; 3],
    second: [Signed; 3],
    products: [Signed; 3],
}

/// The pairs of axes of the three products.
const AXIS_PAIRS: [(usize, usize); 3] = [(0, 1), (0, 2), (1, 2)];

impl Sums {
    fn add_piece(&mut self, p: &Piece) -> Option<()> {
        let volume_h = p.volume.checked_mul(8)?;
        self.volume = if p.remove {
            self.volume.checked_sub(volume_h)?
        } else {
            self.volume.checked_add(volume_h)?
        };
        let v = U256::from_u128(u128::try_from(volume_h).ok()?);
        let centre = p.centre.map(|c| u128::try_from(c).ok());
        let spread = p.spread.map(|s| u128::try_from(s).ok());
        for (((m1, second), c), s) in self
            .m1
            .iter_mut()
            .zip(self.second.iter_mut())
            .zip(centre.iter())
            .zip(spread.iter())
        {
            let (c, s) = ((*c)?, (*s)?);
            m1.add(p.remove, v.mul_u128(c)?)?;
            let around_origin = 48u128.checked_mul(c.checked_mul(c)?)?.checked_add(s)?;
            second.add(p.remove, v.mul_u128(around_origin)?)?;
        }
        for ((a, b), product) in AXIS_PAIRS.iter().zip(self.products.iter_mut()) {
            let (ca, cb) = (centre[*a]?, centre[*b]?);
            let both = 48u128.checked_mul(ca.checked_mul(cb)?)?;
            product.add(p.remove, v.mul_u128(both)?)?;
        }
        Some(())
    }
}

/// The result of the sums: the centre in nanometres and the moments about the axes through it,
/// 48 times too large and in half nanometres to the fifth power.
struct Raw {
    volume_h: u128,
    centre_nm: [i64; 3],
    diagonal: [U256; 3],
    products: [(bool, U256); 3],
}

fn raw_moments(s: &Sums) -> Option<Raw> {
    let volume_h = u128::try_from(s.volume).ok()?;
    if volume_h == 0 {
        return None;
    }
    let v = U256::from_u128(volume_h);
    let mut m1 = [U256::ZERO; 3];
    let mut centre_nm = [0i64; 3];
    for ((mag, c), sum) in m1.iter_mut().zip(centre_nm.iter_mut()).zip(s.m1.iter()) {
        let (negative, magnitude) = sum.value();
        if negative {
            return None;
        }
        *mag = magnitude;
        // the centre in nanometres: moment / (2 * volume), rounded to a whole nanometre
        let q = magnitude.round_div(volume_h.checked_mul(2)?)?;
        *c = i64::try_from(q.to_u128()?).ok()?;
    }
    // the centre as used for the moments, in half nanometres
    let centre_h = centre_nm.map(|c| 2 * u128::try_from(c).unwrap_or(0));
    let mut diagonal = [U256::ZERO; 3];
    for (((out, second), m), c) in diagonal
        .iter_mut()
        .zip(s.second.iter())
        .zip(m1.iter())
        .zip(centre_h.iter())
    {
        let mut acc = Signed::ZERO;
        acc.merge(second, false)?;
        acc.add(true, m.mul_u128(96u128.checked_mul(*c)?)?)?;
        acc.add(false, v.mul_u128(48u128.checked_mul(c.checked_mul(*c)?)?)?)?;
        let (negative, magnitude) = acc.value();
        if negative {
            return None;
        }
        *out = magnitude;
    }
    let mut products = [(false, U256::ZERO); 3];
    for (i, (a, b)) in AXIS_PAIRS.iter().enumerate() {
        let (a, b) = (*a, *b);
        let mut acc = Signed::ZERO;
        acc.merge(&s.products[i], false)?;
        acc.add(true, m1[b].mul_u128(48u128.checked_mul(centre_h[a])?)?)?;
        acc.add(true, m1[a].mul_u128(48u128.checked_mul(centre_h[b])?)?)?;
        let both = 48u128.checked_mul(centre_h[a].checked_mul(centre_h[b])?)?;
        acc.add(false, v.mul_u128(both)?)?;
        products[i] = acc.value();
    }
    Some(Raw {
        volume_h,
        centre_nm,
        diagonal,
        products,
    })
}

/// A moment in micrograms times square millimetres: the size `mag` of a moment 48 times too large
/// in half nanometres to the fifth power, for a density in kilograms per cubic metre.
fn moment_ug_mm2(mag: &U256, density_kg_m3: u32) -> Option<i128> {
    // 48 for the factor, 32 for the half nanometres, 10^18 for the volume and 10^12 for the
    // square millimetres
    const DENOMINATOR: u128 = 48 * 32 * 1_000_000_000_000_000_000_000_000_000_000;
    let n = mag.mul_u128(u128::from(density_kg_m3))?;
    i128::try_from(n.round_div(DENOMINATOR)?.to_u128()?).ok()
}

/// The moments of inertia of the solid, in micrograms times square millimetres, about the axes
/// through the centre of mass that are parallel to X, Y and Z. They are the components of the
/// inertia tensor: the products of inertia have the sign of the usual tensor (the opposite of the
/// integral of the product of the coordinates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inertia {
    /// About the axis parallel to X.
    pub xx: i128,
    /// About the axis parallel to Y.
    pub yy: i128,
    /// About the axis parallel to Z.
    pub zz: i128,
    /// The product of inertia of X and Y.
    pub xy: i128,
    /// The product of inertia of X and Z.
    pub xz: i128,
    /// The product of inertia of Y and Z.
    pub yz: i128,
}

/// The mass properties of the enclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MassProperties {
    /// The volume of the solid in cubic nanometres.
    pub volume_nm3: i128,
    /// The mass in micrograms.
    pub mass_micrograms: i128,
    /// The centre of mass in nanometres from the corner of the outer box with the smallest X, Y
    /// and Z (the left, front and bottom sides), rounded to a nanometre.
    pub centre: (Length, Length, Length),
    /// The moments of inertia about the axes through that centre.
    pub inertia: Inertia,
    /// True when the figures describe the model as it is; false when something was left out or
    /// approximated (see [`Enclosure::mass_properties`]).
    pub exact: bool,
}

fn box_piece(remove: bool, lo: [i128; 3], size: [i128; 3]) -> Result<Piece, CadError> {
    let volume = size[0]
        .checked_mul(size[1])
        .and_then(|x| x.checked_mul(size[2]))
        .ok_or(CadError::Overflow("volume"))?;
    Ok(Piece {
        remove,
        volume,
        centre: [0, 1, 2].map(|k| 2 * lo[k] + size[k]),
        spread: size.map(|s| 16 * s * s),
    })
}

impl Enclosure {
    /// Where a hole enters the box, in nanometres.
    fn hole_place(&self, hole: &Hole) -> Place {
        let (x, y, z) = self.params().outer();
        let (x, y, z) = (
            i128::from(x.get()),
            i128::from(y.get()),
            i128::from(z.get()),
        );
        let (u, v) = (i128::from(hole.u.get()), i128::from(hole.v.get()));
        let (axis, inward_is_up, face_at, across) = match hole.face {
            Face::Front => (1, true, 0, [(0, u), (2, v)]),
            Face::Back => (1, false, y, [(0, x - u), (2, v)]),
            Face::Left => (0, true, 0, [(1, y - u), (2, v)]),
            Face::Right => (0, false, x, [(1, u), (2, v)]),
            Face::Top => (2, false, z, [(0, u), (1, v)]),
            Face::Bottom => (2, true, 0, [(0, u), (1, y - v)]),
        };
        Place {
            axis,
            inward_is_up,
            face_at,
            across,
        }
    }

    /// A part of a hole: a prism that starts on the face and goes `depth` into the material, with
    /// the given volume and the given size across along `u` and along `v`.
    fn hole_piece(
        place: &Place,
        remove: bool,
        depth: i128,
        volume: i128,
        sizes: (i128, i128),
        round: bool,
    ) -> Piece {
        let low = if place.inward_is_up {
            place.face_at
        } else {
            place.face_at - depth
        };
        let mut centre = [0i128; 3];
        let mut spread = [0i128; 3];
        centre[place.axis] = 2 * low + depth;
        spread[place.axis] = 16 * depth * depth;
        for ((axis, at), size) in place.across.iter().zip([sizes.0, sizes.1]) {
            centre[*axis] = 2 * at;
            // a circle has a variance of a sixteenth of the square of its diameter, a rectangle
            // of a twelfth of the square of its side
            spread[*axis] = if round {
                12 * size * size
            } else {
                16 * size * size
            };
        }
        Piece {
            remove,
            volume,
            centre,
            spread,
        }
    }

    /// The parts that make the solid, and whether the figures describe it exactly.
    fn mass_pieces(&self) -> Result<(Vec<Piece>, bool), CadError> {
        let p = self.params();
        let (x, y, z) = p.outer();
        let n = |l: Length| i128::from(l.get());
        let mut exact = self.edging().is_sharp();
        let mut pieces = vec![
            box_piece(false, [0, 0, 0], [n(x), n(y), n(z)])?,
            box_piece(
                true,
                [n(p.wall), n(p.wall), n(p.floor)],
                [n(p.inner_length), n(p.inner_width), n(p.inner_height)],
            )?,
        ];
        for hole in self.holes().values() {
            let place = self.hole_place(hole);
            let t = self.thin_depth(hole.face);
            let d = n(hole.diameter);
            let straight = cylinder_volume_nm3(hole.diameter, t)?;
            let whole = hole_volume_nm3(hole.diameter, hole.style, t)?;
            exact &= self.hole_in_thin_part(hole);
            match hole.style {
                HoleStyle::Plain => {
                    pieces.push(Self::hole_piece(&place, true, n(t), straight, (d, d), true));
                }
                HoleStyle::Counterbored {
                    head_diameter,
                    depth,
                } => {
                    let (head, k) = (n(head_diameter), n(depth));
                    pieces.push(Self::hole_piece(&place, true, n(t), straight, (d, d), true));
                    // the recess is wider than the hole: take away the wide cylinder and put back
                    // the part of it that the hole had already taken away
                    let wide = cylinder_volume_nm3(head_diameter, depth)?;
                    let narrow = cylinder_volume_nm3(hole.diameter, depth)?;
                    pieces.push(Self::hole_piece(&place, true, k, wide, (head, head), true));
                    pieces.push(Self::hole_piece(&place, false, k, narrow, (d, d), true));
                }
                HoleStyle::Countersunk { head_diameter } => {
                    // approximated: the cone is taken as a cylinder of the mean diameter
                    let head = n(head_diameter);
                    pieces.push(Self::hole_piece(&place, true, n(t), straight, (d, d), true));
                    let depth = (head - d + 1) / 2;
                    let mean = (head + d) / 2;
                    pieces.push(Self::hole_piece(
                        &place,
                        true,
                        depth,
                        whole - straight,
                        (mean, mean),
                        true,
                    ));
                    exact = false;
                }
                HoleStyle::Rectangular { .. } => {
                    let (su, sv) = hole.style.footprint(hole.diameter);
                    pieces.push(Self::hole_piece(&place, true, n(t), whole, (su, sv), false));
                }
                HoleStyle::Oval { .. } => {
                    // approximated: the slot is taken as the rectangle that holds it
                    let (su, sv) = hole.style.footprint(hole.diameter);
                    pieces.push(Self::hole_piece(&place, true, n(t), whole, (su, sv), false));
                    exact = false;
                }
            }
        }
        Ok((pieces, exact))
    }

    /// The volume, the mass, the centre of mass and the moments of inertia of the enclosure for a
    /// material of the given density (kilograms per cubic metre), in integers.
    ///
    /// The solid is the outer box, minus the inner space, minus each hole (a cylinder or a box,
    /// with the recess of a counterbore), so that every part is a box or a cylinder whose moments
    /// are exact. Some things are left out or approximated, and then `exact` is false: the
    /// treatment of the edges is not taken from the solid (the volume is the one without it), a
    /// countersink is taken as a cylinder of its mean diameter, a slot as the rectangle that holds
    /// it, and a hole that is not wholly in the thin part of its face is as long as that part.
    pub fn mass_properties(&self, density_kg_m3: u32) -> Result<MassProperties, CadError> {
        let fail = || CadError::Overflow("mass properties");
        let (pieces, exact) = self.mass_pieces()?;
        let mut sums = Sums {
            volume: 0,
            m1: [Signed::ZERO; 3],
            second: [Signed::ZERO; 3],
            products: [Signed::ZERO; 3],
        };
        for piece in &pieces {
            sums.add_piece(piece).ok_or_else(fail)?;
        }
        let raw = raw_moments(&sums).ok_or_else(fail)?;
        let scale = |mag: &U256| moment_ug_mm2(mag, density_kg_m3).ok_or_else(fail);
        let signed = |(negative, mag): &(bool, U256)| -> Result<i128, CadError> {
            // the product of inertia is the opposite of the integral of the product
            let v = scale(mag)?;
            Ok(if *negative { v } else { -v })
        };
        let [dx, dy, dz] = raw.diagonal;
        let sum = |a: &U256, b: &U256| a.add(b).ok_or_else(fail);
        let volume_nm3 = i128::try_from(raw.volume_h / 8).map_err(|_| fail())?;
        let [cx, cy, cz] = raw.centre_nm;
        Ok(MassProperties {
            volume_nm3,
            mass_micrograms: mass_micrograms(volume_nm3, density_kg_m3)?,
            centre: (Length::new(cx)?, Length::new(cy)?, Length::new(cz)?),
            inertia: Inertia {
                xx: scale(&sum(&dy, &dz)?)?,
                yy: scale(&sum(&dx, &dz)?)?,
                zz: scale(&sum(&dx, &dy)?)?,
                xy: signed(&raw.products[0])?,
                xz: signed(&raw.products[1])?,
                yz: signed(&raw.products[2])?,
            },
            exact,
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

    #[test]
    fn wide_integers_multiply_divide_and_refuse_to_overflow() {
        let x = U256::from_u128((1 << 100) + 12_345);
        let y: u128 = (1 << 120) + 999;
        let p = x.mul_u128(y).unwrap();
        assert_eq!(p.div_rem_u128(y).unwrap(), (x, 0));
        let more = p.add(&U256::from_u128(5)).unwrap();
        assert_eq!(more.div_rem_u128(y).unwrap(), (x, 5));
        assert_eq!(more.sub(&p).unwrap().to_u128(), Some(5));
        assert!(U256([u64::MAX; 4]).mul_u128(2).is_none());
        assert!(U256([u64::MAX; 4]).add(&U256::from_u128(1)).is_none());
        assert!(U256::from_u128(3).sub(&U256::from_u128(4)).is_none());
        assert!(U256::from_u128(3).div_rem_u128(0).is_none());
        // rounded half up
        assert_eq!(U256::from_u128(7).round_div(2).unwrap().to_u128(), Some(4));
        assert_eq!(U256::from_u128(5).round_div(2).unwrap().to_u128(), Some(3));
        assert_eq!(U256::from_u128(4).round_div(3).unwrap().to_u128(), Some(1));
        let mut s = Signed::ZERO;
        s.add(false, U256::from_u128(3)).unwrap();
        s.add(true, U256::from_u128(5)).unwrap();
        assert_eq!(s.value(), (true, U256::from_u128(2)));
    }

    fn props(e: &Enclosure) -> MassProperties {
        e.mass_properties(1240).unwrap()
    }

    fn tensor(m: &MassProperties) -> [i128; 6] {
        let i = m.inertia;
        [i.xx, i.yy, i.zz, i.xy, i.xz, i.yz]
    }

    fn nm(c: (Length, Length, Length)) -> [i64; 3] {
        [c.0.get(), c.1.get(), c.2.get()]
    }

    #[test]
    fn the_empty_box_has_its_centre_and_its_moments_of_inertia() {
        let m = props(&Enclosure::new(params(3)).unwrap());
        // 52960 cubic millimetres of a material of 1240 kg/m3
        assert_eq!(m.volume_nm3, 52_960 * MM3);
        assert_eq!(m.mass_micrograms, 65_670_400);
        // the box is symmetric in X and Y; the floor and the lid are not alike in Z
        assert_eq!(nm(m.centre), [52_000_000, 32_000_000, 19_199_396]);
        assert_eq!(
            tensor(&m),
            [44_144_497_299, 86_871_921_299, 106_009_617_067, 0, 0, 0]
        );
        assert!(m.exact);
    }

    #[test]
    fn a_hole_moves_the_centre_and_gives_products_of_inertia() {
        let m = props(&with_hole(3, Face::Front, 50, 17, 10));
        assert_eq!(m.volume_nm3, 52_802_920_367_320_500_000_000);
        assert_eq!(nm(m.centre), [52_005_950, 32_092_220, 19_205_939]);
        assert_eq!(
            tensor(&m),
            [
                43_954_530_782,
                86_867_760_117,
                105_819_814_132,
                12_112_207,
                859_340,
                13_319_768
            ]
        );
        assert!(m.exact);
        // the volume is the one of the holes of the other figures, to the nanometre
        let by_holes = props(&with_hole(3, Face::Front, 50, 17, 10))
            .volume_nm3
            .abs_diff(
                with_hole(3, Face::Front, 50, 17, 10)
                    .material_volume_with_holes()
                    .unwrap()
                    .nm3,
            );
        assert!(by_holes <= 1, "{by_holes}");
    }

    #[test]
    fn holes_on_other_faces_are_placed_where_the_script_places_them() {
        let mut e = with_hole(3, Face::Front, 50, 17, 10);
        e.apply(Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, 2, 2).unwrap(),
            hole: Hole {
                name: "back".into(),
                face: Face::Back,
                u: mm(30),
                v: mm(20),
                diameter: mm(8),
                style: HoleStyle::Plain,
            },
        })
        .unwrap();
        let m = props(&e);
        assert_eq!(m.volume_nm3, 52_702_389_402_405_620_000_000);
        assert_eq!(nm(m.centre), [51_963_996, 32_033_262, 19_204_424]);
        assert_eq!(
            tensor(&m),
            [
                43_834_599_657,
                86_806_267_037,
                105_639_544_696,
                97_015_045,
                3_040_605,
                16_385_055
            ]
        );
    }

    #[test]
    fn a_rectangular_hole_and_a_counterbore_are_boxes_and_cylinders_too() {
        let rect = styled(
            HoleStyle::Rectangular { height: mm(4) },
            10_000_000,
            Face::Left,
        );
        // the helper puts the centre at (50, 17), but the left face is 64 mm wide: use u = 20
        let mut e = Enclosure::new(params(3)).unwrap();
        let mut hole = rect.holes().values().next().unwrap().clone();
        hole.u = mm(20);
        e.apply(Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, 1, 1).unwrap(),
            hole,
        })
        .unwrap();
        let m = props(&e);
        assert_eq!(m.volume_nm3, 52_880 * MM3);
        assert_eq!(nm(m.centre), [52_077_156, 31_981_846, 19_202_723]);
        assert_eq!(
            tensor(&m),
            [
                44_128_751_364,
                86_612_865_828,
                105_736_041_376,
                -60_802_246,
                11_144_017,
                -2_622_122
            ]
        );
        assert!(m.exact);
        let mut top = Enclosure::new(params(3)).unwrap();
        top.apply(Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, 1, 1).unwrap(),
            hole: Hole {
                name: "bore".into(),
                face: Face::Top,
                u: mm(52),
                v: mm(32),
                diameter: Length::new(3_400_000).unwrap(),
                style: HoleStyle::Counterbored {
                    head_diameter: mm(6),
                    depth: mm(1),
                },
            },
        })
        .unwrap();
        let m = props(&top);
        assert_eq!(m.volume_nm3, 52_913_567_260_579_939_800_000);
        assert_eq!(nm(m.centre), [52_000_000, 32_000_000, 19_186_484]);
        assert_eq!(
            tensor(&m),
            [44_131_884_492, 86_859_308_492, 106_009_426_760, 0, 0, 0]
        );
        assert!(m.exact);
    }

    #[test]
    fn what_is_left_out_or_approximated_is_said_by_the_exact_flag() {
        let rounded = edged(Edging {
            vertical: Treatment::Round(mm(5)),
            ..Edging::default()
        });
        assert!(!props(&rounded).exact);
        let sunk = styled(
            HoleStyle::Countersunk {
                head_diameter: Length::new(6_300_000).unwrap(),
            },
            3_400_000,
            Face::Front,
        );
        assert!(!props(&sunk).exact);
        let slot = styled(
            HoleStyle::Oval {
                length: mm(10),
                vertical: false,
            },
            4_000_000,
            Face::Front,
        );
        let m = props(&slot);
        assert!(!m.exact);
        // the volume is still the one of the slot
        assert_eq!(m.volume_nm3, slot.material_volume_with_holes().unwrap().nm3);
        // a hole that reaches the floor is as long as the thin part, and the figure says so
        assert!(!props(&with_hole(0, Face::Front, 50, 3, 6)).exact);
    }

    #[test]
    fn the_mass_follows_the_density_and_the_moments_follow_the_mass() {
        let e = Enclosure::new(params(3)).unwrap();
        let light = e.mass_properties(1000).unwrap();
        let heavy = e.mass_properties(2000).unwrap();
        assert_eq!(heavy.mass_micrograms, 2 * light.mass_micrograms);
        assert!(heavy.inertia.zz.abs_diff(2 * light.inertia.zz) <= 1);
        assert_eq!(heavy.centre, light.centre);
        assert_eq!(
            light.mass_micrograms,
            mass_micrograms(light.volume_nm3, 1000).unwrap()
        );
    }
}
