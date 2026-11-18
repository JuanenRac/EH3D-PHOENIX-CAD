// SPDX-License-Identifier: GPL-3.0-or-later
//! The solid made by turning a sketch about one of its axes: its volume, its surface and its mass.
//!
//! A profile (the closed outlines of a sketch, see [`crate::profile`]) turned about the first
//! axis of its plane (`x`) or the second one (`y`) by an angle up to a full turn sweeps a solid
//! of revolution: a spacer, a knob, a washer. The figures come from the theorems of Pappus: the
//! volume is the angle times the first moment of the area about the axis, and the surface is the
//! angle times the first moment of the boundary (the outer outlines and the holes) about the axis,
//! plus the two flat ends when the turn is not a full one. The moments of the lines, arcs and
//! circles are worked out exactly in floating point (the circular segment of an arc has its own
//! area and centroid), so the figures are rounded to a cubic or square nanometre and are not exact
//! integers.
//!
//! The profile must lie on one side of the axis, touching it at most: a profile that crosses the
//! axis would sweep through itself and is refused with `PHOENIX-CAD-036`. [`mesh`] makes the
//! triangles of the solid for an STL, OBJ or 3MF file; there is no STEP file of it.

use std::f64::consts::{PI, TAU};

use crate::lang::{fill, Lang, Msg};
use crate::mass::mass_micrograms;
use crate::mesh::{sin_cos, Builder, Circle, Loop, Mesh, Role};
use crate::profile::{Outline, Piece, Profile, Pt};
use crate::units::{div_round_half_away, Angle};
use crate::CadError;

/// The axis of the sketch that the profile turns about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// The first axis (`x`): the distance from the axis is the second coordinate.
    X,
    /// The second axis (`y`): the distance from the axis is the first coordinate.
    Y,
}

impl Axis {
    /// The word of the command line.
    pub fn name(self) -> &'static str {
        match self {
            Axis::X => "x",
            Axis::Y => "y",
        }
    }

    /// The axis of a word of the command line.
    pub fn from_name(name: &str) -> Option<Axis> {
        match name {
            "x" => Some(Axis::X),
            "y" => Some(Axis::Y),
            _ => None,
        }
    }

    /// The index of the coordinate that is the distance from the axis.
    fn radial(self) -> usize {
        match self {
            Axis::X => 1,
            Axis::Y => 0,
        }
    }
}

/// The figures of a solid of revolution.
#[derive(Debug, Clone, PartialEq)]
pub struct Revolved {
    /// The turn, in degrees.
    pub degrees: f64,
    /// The area of the profile in square nanometres.
    pub profile_nm2: f64,
    /// The volume in cubic nanometres.
    pub volume_nm3: i128,
    /// The surface in square nanometres: what the boundary sweeps and, when the turn is not a full
    /// one, the two flat ends.
    pub surface_nm2: i128,
    /// The smallest and the largest coordinate along the axis, in nanometres.
    pub axial: (i64, i64),
    /// The smallest and the largest distance from the axis, in nanometres.
    pub radial: (i64, i64),
}

/// The area and the first moment about the axis of the region an outline bounds.
pub(crate) fn outline_moments(o: &Outline, k: usize) -> (f64, f64) {
    if let Some((c, r)) = o.circle_of() {
        let area = PI * (r as f64) * (r as f64);
        let centre = if k == 0 { c.0 } else { c.1 };
        return (area, area * centre as f64);
    }
    let ends: Vec<Pt> = o.pieces().iter().map(Piece::start).collect();
    let (mut area2, mut green) = (0.0f64, 0.0f64);
    for (i, p) in ends.iter().enumerate() {
        let q = ends[(i + 1) % ends.len()];
        let cross = (p.0 as f64) * (q.1 as f64) - (q.0 as f64) * (p.1 as f64);
        let (cp, cq) = if k == 0 { (p.0, q.0) } else { (p.1, q.1) };
        area2 += cross;
        green += (cp as f64 + cq as f64) * cross;
    }
    let mut area = area2 / 2.0;
    let mut first = green / 6.0;
    for piece in o.pieces() {
        if let Piece::Arc {
            center,
            from,
            to,
            ccw,
        } = *piece
        {
            let (r, theta, a0) = arc_figures(center, from, to, ccw);
            let tail = theta - theta.sin();
            if tail <= 1e-12 {
                continue;
            }
            let segment = 0.5 * r * r * tail;
            let d = 4.0 * r * (theta / 2.0).sin().powi(3) / (3.0 * tail);
            let mid = if ccw {
                a0 + theta / 2.0
            } else {
                a0 - theta / 2.0
            };
            let centre = if k == 0 { center.0 } else { center.1 } as f64;
            let at = centre + d * if k == 0 { mid.cos() } else { mid.sin() };
            let sign = if ccw { 1.0 } else { -1.0 };
            area += sign * segment;
            first += sign * segment * at;
        }
    }
    // the orientation of the outline decides the sign of both figures
    let sign = if area < 0.0 { -1.0 } else { 1.0 };
    (area.abs(), first * sign)
}

/// The radius, the angle walked (0 to a full turn) and the angle of the start of an arc.
pub(crate) fn arc_figures(center: Pt, from: Pt, to: Pt, ccw: bool) -> (f64, f64, f64) {
    let (fx, fy) = ((from.0 - center.0) as f64, (from.1 - center.1) as f64);
    let (tx, ty) = ((to.0 - center.0) as f64, (to.1 - center.1) as f64);
    let r = fx.hypot(fy);
    let a0 = fy.atan2(fx);
    let a1 = ty.atan2(tx);
    let raw = if ccw { a1 - a0 } else { a0 - a1 };
    let mut theta = raw.rem_euclid(TAU);
    if theta <= 0.0 {
        theta = TAU;
    }
    (r, theta, a0)
}

/// The smallest and the largest corner of the profile with the circles and the arcs themselves
/// (the profile keeps the straight lines that stand for them, which sag a little inside).
pub(crate) fn exact_bounds(profile: &Profile) -> (Pt, Pt) {
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    let mut take = |x: f64, y: f64| {
        lo = (lo.0.min(x), lo.1.min(y));
        hi = (hi.0.max(x), hi.1.max(y));
    };
    for o in profile.outlines() {
        if let Some((c, r)) = o.circle_of() {
            take(c.0 as f64 - r as f64, c.1 as f64 - r as f64);
            take(c.0 as f64 + r as f64, c.1 as f64 + r as f64);
            continue;
        }
        for piece in o.pieces() {
            match *piece {
                Piece::Line { from, to } => {
                    take(from.0 as f64, from.1 as f64);
                    take(to.0 as f64, to.1 as f64);
                }
                Piece::Arc {
                    center,
                    from,
                    to,
                    ccw,
                } => {
                    take(from.0 as f64, from.1 as f64);
                    take(to.0 as f64, to.1 as f64);
                    let (r, theta, a0) = arc_figures(center, from, to, ccw);
                    for quarter in 0..4 {
                        let dir = f64::from(quarter) * PI / 2.0;
                        let rel = if ccw { dir - a0 } else { a0 - dir };
                        if rel.rem_euclid(TAU) <= theta {
                            take(
                                center.0 as f64 + r * dir.cos(),
                                center.1 as f64 + r * dir.sin(),
                            );
                        }
                    }
                }
            }
        }
    }
    (
        (lo.0.round() as i64, lo.1.round() as i64),
        (hi.0.round() as i64, hi.1.round() as i64),
    )
}

/// The integral of the distance from the axis along the boundary of an outline.
pub(crate) fn boundary_moment(o: &Outline, k: usize) -> f64 {
    if let Some((c, r)) = o.circle_of() {
        let centre = if k == 0 { c.0 } else { c.1 };
        return TAU * r as f64 * centre as f64;
    }
    let mut total = 0.0f64;
    for piece in o.pieces() {
        match *piece {
            Piece::Line { from, to } => {
                let length = ((to.0 - from.0) as f64).hypot((to.1 - from.1) as f64);
                let (a, b) = if k == 0 {
                    (from.0, to.0)
                } else {
                    (from.1, to.1)
                };
                total += length * (a as f64 + b as f64) / 2.0;
            }
            Piece::Arc {
                center,
                from,
                to,
                ccw,
            } => {
                let (r, theta, a0) = arc_figures(center, from, to, ccw);
                let start = if ccw { a0 } else { a0 - theta };
                let end = start + theta;
                let around = if k == 0 {
                    end.sin() - start.sin()
                } else {
                    start.cos() - end.cos()
                };
                let centre = if k == 0 { center.0 } else { center.1 } as f64;
                total += r * (centre * theta + r * around);
            }
        }
    }
    total
}

/// The figures of the profile turned about an axis by an angle. Refused with `PHOENIX-CAD-007`
/// when the angle is not above zero and up to a full turn, and with `PHOENIX-CAD-036` when the
/// profile crosses the axis.
pub fn revolve(profile: &Profile, axis: Axis, angle: Angle) -> Result<Revolved, CadError> {
    let ndeg = angle.get();
    if ndeg <= 0 || ndeg > 360_000_000_000 {
        return Err(CadError::ValueOutOfRange("angle"));
    }
    let k = axis.radial();
    let (lo, hi) = exact_bounds(profile);
    let (lo_r, hi_r) = if k == 0 { (lo.0, hi.0) } else { (lo.1, hi.1) };
    if lo_r < 0 && hi_r > 0 {
        return Err(CadError::Profile(
            "the profile crosses the axis it would be turned about".into(),
        ));
    }
    let side = if hi_r <= 0 { -1.0 } else { 1.0 };
    let radians = ndeg as f64 / 1e9 * PI / 180.0;
    let (mut area, mut first, mut boundary) = (0.0f64, 0.0f64, 0.0f64);
    for region in &profile.regions {
        let (a, m) = outline_moments(&region.outer, k);
        area += a;
        first += m;
        boundary += boundary_moment(&region.outer, k);
        for hole in &region.holes {
            let (a, m) = outline_moments(hole, k);
            area -= a;
            first -= m;
            boundary += boundary_moment(hole, k);
        }
    }
    let first = first * side;
    let boundary = boundary * side;
    let full = ndeg == 360_000_000_000;
    let surface = radians * boundary + if full { 0.0 } else { 2.0 * area };
    let (axial, radial) = if k == 0 {
        ((lo.1, hi.1), (lo_r, hi_r))
    } else {
        ((lo.0, hi.0), (lo_r, hi_r))
    };
    let radial = if side < 0.0 {
        (-radial.1, -radial.0)
    } else {
        radial
    };
    Ok(Revolved {
        degrees: ndeg as f64 / 1e9,
        profile_nm2: area,
        volume_nm3: (radians * first).round() as i128,
        surface_nm2: surface.round() as i128,
        axial,
        radial,
    })
}

/// A volume or a surface in a unit that is `unit` nanometres to the power of the dimension, with
/// three decimals.
pub(crate) fn scaled(value: i128, unit: i128) -> String {
    let milli = div_round_half_away(value, unit / 1000).unwrap_or(0);
    let sign = if milli < 0 { "-" } else { "" };
    let abs = milli.unsigned_abs();
    format!("{sign}{}.{:03}", abs / 1000, abs % 1000)
}

/// A length in millimetres, without decimals that are zeros.
pub(crate) fn length_text(nm: i64) -> String {
    crate::scad::mm_text(nm)
}

/// The figures as lines of text in a language. With a material (its name and its density in
/// kilograms per cubic metre) the mass is given too.
pub fn revolve_text(
    r: &Revolved,
    material: Option<(&str, u32)>,
    lang: Lang,
) -> Result<String, CadError> {
    let degrees = format!("{}", (r.degrees * 1000.0).round() / 1000.0);
    let mut lines = vec![
        fill(Msg::RevAngle.tr(lang), &[&degrees]),
        fill(
            Msg::RevArea.tr(lang),
            &[&scaled(r.profile_nm2.round() as i128, 1_000_000_000_000)],
        ),
        fill(
            Msg::RevVolume.tr(lang),
            &[&scaled(r.volume_nm3, 1_000_000_000_000_000_000)],
        ),
        fill(
            Msg::RevSurface.tr(lang),
            &[&scaled(r.surface_nm2, 1_000_000_000_000)],
        ),
        fill(
            Msg::RevAxial.tr(lang),
            &[&length_text(r.axial.0), &length_text(r.axial.1)],
        ),
        fill(
            Msg::RevRadius.tr(lang),
            &[&length_text(r.radial.0), &length_text(r.radial.1)],
        ),
    ];
    if let Some((name, density)) = material {
        let grams = crate::catalog::mass_g_text(mass_micrograms(r.volume_nm3, density)?);
        lines.push(fill(Msg::RevMass.tr(lang), &[name, &grams]));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    Ok(out)
}

/// The boundary of an outline as points in the direction it is walked: a circle by the sides of
/// the table, an arc by whole steps of one side or less with its two ends exact.
fn sample_outline(o: &Outline, table: &Circle) -> Vec<(f64, f64)> {
    let n = table.sides();
    let mut out: Vec<(f64, f64)> = Vec::new();
    if let Some((c, r)) = o.circle_of() {
        for k in 0..n {
            let (cos, sin) = table.at(k);
            out.push((c.0 as f64 + r as f64 * cos, c.1 as f64 + r as f64 * sin));
        }
        return out;
    }
    for piece in o.pieces() {
        match *piece {
            Piece::Line { from, .. } => out.push((from.0 as f64, from.1 as f64)),
            Piece::Arc {
                center,
                from,
                to,
                ccw,
            } => {
                let (r, theta, a0) = arc_figures(center, from, to, ccw);
                let steps = ((theta / (TAU / n as f64)).ceil() as usize).max(1);
                out.push((from.0 as f64, from.1 as f64));
                for k in 1..steps {
                    let part = theta * k as f64 / steps as f64;
                    let angle = if ccw { a0 + part } else { a0 - part };
                    let (sin, cos) = sin_cos(angle);
                    out.push((center.0 as f64 + r * cos, center.1 as f64 + r * sin));
                }
            }
        }
    }
    out.dedup();
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// Twice the signed area of a list of points (positive when they run counter-clockwise).
fn signed_area2(points: &[(f64, f64)]) -> f64 {
    let mut total = 0.0;
    for (i, p) in points.iter().enumerate() {
        let q = points[(i + 1) % points.len()];
        total += p.0 * q.1 - q.0 * p.1;
    }
    total
}

/// The regions of a profile as lists of points: for each region its outer outline (counter-clockwise)
/// and then its holes (clockwise). The first coordinate is that of the sketch and so is the second.
pub(crate) fn sampled_regions(profile: &Profile, table: &Circle) -> Vec<Vec<Vec<(f64, f64)>>> {
    let oriented = |o: &Outline, ccw: bool| {
        let mut points = sample_outline(o, table);
        if (signed_area2(&points) > 0.0) != ccw {
            points.reverse();
        }
        points
    };
    profile
        .regions
        .iter()
        .map(|region| {
            let mut loops = vec![oriented(&region.outer, true)];
            loops.extend(region.holes.iter().map(|h| oriented(h, false)));
            loops
        })
        .collect()
}

/// The vertices of an outline at the start and at the end of a turn or of a path, and whether it is
/// a hole: what the flat face at each end is made of.
type EndLoops = (Vec<u32>, Vec<u32>, bool);

/// The triangles of the solid made by turning a profile about an axis by an angle (see
/// [`revolve`] for what is refused). A full turn is `sides` steps (a multiple of four, from the
/// settings of the mesh) and a part of a turn the part of them that its angle needs, closed by a
/// flat face at each end. The triangles face away from the material and the surface is closed;
/// `limit` is the most triangles that are made.
pub fn mesh(
    profile: &Profile,
    axis: Axis,
    angle: Angle,
    sides: u32,
    limit: usize,
) -> Result<Mesh, CadError> {
    revolve(profile, axis, angle)?;
    let (_, hi) = exact_bounds(profile);
    let hi_r = if axis.radial() == 0 { hi.0 } else { hi.1 };
    let side = if hi_r <= 0 { -1.0 } else { 1.0 };
    let table = Circle::new(sides);
    let n = table.sides();
    let ndeg = angle.get();
    let full = ndeg == 360_000_000_000;
    let steps = if full {
        n
    } else {
        ((n as f64 * ndeg as f64 / 360e9).ceil() as usize).max(1)
    };
    let radians = ndeg as f64 / 1e9 * PI / 180.0;
    let rings = if full { steps } else { steps + 1 };
    let turn: Vec<(f64, f64)> = (0..rings)
        .map(|j| {
            if full {
                table.at(j)
            } else {
                let (s, c) = sin_cos(radians * j as f64 / steps as f64);
                (c, s)
            }
        })
        .collect();
    // the point (axial, radial) of the profile turned to the ring j
    let place = |a: f64, r: f64, j: usize| -> [f64; 3] {
        let (c, s) = turn[j];
        match axis {
            Axis::X => [a, r * c, r * s],
            Axis::Y => [r * c, a, -(r * s)],
        }
    };
    // swapping the two coordinates or negating the radial one turns the outlines the other way
    let mirrored = (axis == Axis::Y) != (side < 0.0);
    let mut b = Builder::new(limit);
    let mut cap_loops: Vec<Vec<EndLoops>> = Vec::new();
    for region in sampled_regions(profile, &table) {
        let mut caps = Vec::new();
        for (index, points) in region.iter().enumerate() {
            let mut loop_points: Vec<(f64, f64)> = points
                .iter()
                .map(|&(x, y)| match axis {
                    Axis::X => (x, side * y),
                    Axis::Y => (y, side * x),
                })
                .collect();
            if mirrored {
                loop_points.reverse();
            }
            let ids: Vec<Vec<u32>> = (0..rings)
                .map(|j| {
                    loop_points
                        .iter()
                        .map(|&(a, r)| b.vertex(place(a, r, j)))
                        .collect()
                })
                .collect();
            let m = loop_points.len();
            for j in 0..steps {
                let j1 = (j + 1) % rings;
                for i in 0..m {
                    let q = (i + 1) % m;
                    let (pa, pb, pc, pd) = (ids[j][i], ids[j][q], ids[j1][q], ids[j1][i]);
                    if pa != pb && pb != pc && pa != pc {
                        b.triangle(pa, pb, pc)?;
                    }
                    if pa != pc && pc != pd && pa != pd {
                        b.triangle(pa, pc, pd)?;
                    }
                }
            }
            if !full {
                caps.push((ids[0].clone(), ids[steps].clone(), index > 0));
            }
        }
        if !full {
            cap_loops.push(caps);
        }
    }
    if !full {
        let (c1, s1) = turn[steps];
        let flow = |s: f64, c: f64| -> [f64; 3] {
            match axis {
                Axis::X => [0.0, -s, c],
                Axis::Y => [-s, 0.0, -c],
            }
        };
        let start = flow(0.0, 1.0);
        let end = flow(s1, c1);
        for caps in &cap_loops {
            let first: Vec<Loop> = caps
                .iter()
                .map(|(a, _, hole)| {
                    Loop::new(
                        a.clone(),
                        if *hole { Role::Cut } else { Role::Boundary },
                        "the start of the turn",
                    )
                })
                .collect();
            let last: Vec<Loop> = caps
                .iter()
                .map(|(_, z, hole)| {
                    Loop::new(
                        z.clone(),
                        if *hole { Role::Cut } else { Role::Boundary },
                        "the end of the turn",
                    )
                })
                .collect();
            b.plane(&first, [-start[0], -start[1], -start[2]])?;
            b.plane(&last, end)?;
        }
    }
    Ok(b.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::Face;
    use crate::profile::ProfileLimits;
    use crate::sketch::{Sketch, SketchPlane};
    use crate::Length;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn new() -> Sketch {
        Sketch::new("part", SketchPlane::on_face(Face::Front)).unwrap()
    }

    fn rectangle(x0: i64, y0: i64, x1: i64, y1: i64) -> Sketch {
        let mut s = new();
        let ids: Vec<_> = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
            .iter()
            .map(|(x, y)| s.add_point(mm(*x), mm(*y)).unwrap())
            .collect();
        for i in 0..4 {
            s.add_line(ids[i], ids[(i + 1) % 4]).unwrap();
        }
        s
    }

    fn profile(s: &Sketch) -> Profile {
        Profile::from_sketch(s, &ProfileLimits::shipped()).unwrap()
    }

    fn deg(v: i64) -> Angle {
        Angle::from_deg(v).unwrap()
    }

    fn text(s: &Sketch, axis: Axis, angle: i64) -> String {
        let r = revolve(&profile(s), axis, deg(angle)).unwrap();
        revolve_text(&r, None, Lang::En).unwrap()
    }

    #[test]
    fn a_rectangle_turned_about_the_first_axis_is_a_tube_by_the_theorem_of_pappus() {
        // 10 mm long, from 5 to 15 mm of the axis: pi (15^2 - 5^2) 10 = 2000 pi = 6283.185 mm3
        // and a surface of 2 pi (15 + 5) 10 + 2 pi (15^2 - 5^2) = 800 pi = 2513.274 mm2
        let s = rectangle(0, 5, 10, 15);
        let r = revolve(&profile(&s), Axis::X, deg(360)).unwrap();
        assert_eq!(r.axial, (0, 10_000_000));
        assert_eq!(r.radial, (5_000_000, 15_000_000));
        assert!((r.profile_nm2 - 100e12).abs() < 1.0);
        assert_eq!(
            text(&s, Axis::X, 360),
            "turn: 360 degrees\nprofile area: 100.000 mm2\nvolume: 6283.185 mm3\nsurface: 2513.274 mm2\nalong the axis: 0 to 10 mm\ndistance from the axis: 5 to 15 mm\n"
        );
        // turned about the second axis a rectangle that is the same one gives the same figures
        let t = rectangle(5, 0, 15, 10);
        let ty = revolve(&profile(&t), Axis::Y, deg(360)).unwrap();
        assert_eq!(ty.volume_nm3, r.volume_nm3);
        assert_eq!(ty.surface_nm2, r.surface_nm2);
        assert_eq!(ty.axial, (0, 10_000_000));
    }

    #[test]
    fn a_half_turn_has_half_the_volume_and_two_flat_ends() {
        // 1000 pi = 3141.593 mm3; 400 pi + 2 x 100 = 1456.637 mm2
        let s = rectangle(0, 5, 10, 15);
        let t = text(&s, Axis::X, 180);
        assert!(t.contains("volume: 3141.593 mm3"), "{t}");
        assert!(t.contains("surface: 1456.637 mm2"), "{t}");
        assert!(t.starts_with("turn: 180 degrees\n"));
        // a quarter turn
        assert!(text(&s, Axis::X, 90).contains("volume: 1570.796 mm3"));
    }

    #[test]
    fn a_profile_on_the_other_side_of_the_axis_gives_the_same_solid() {
        let above = revolve(&profile(&rectangle(0, 5, 10, 15)), Axis::X, deg(360)).unwrap();
        let below = revolve(&profile(&rectangle(0, -15, 10, -5)), Axis::X, deg(360)).unwrap();
        assert_eq!(above.volume_nm3, below.volume_nm3);
        assert_eq!(above.surface_nm2, below.surface_nm2);
        assert_eq!(below.radial, (5_000_000, 15_000_000));
    }

    #[test]
    fn a_profile_that_crosses_the_axis_or_a_bad_angle_is_refused() {
        let across = profile(&rectangle(0, -5, 10, 5));
        let e = revolve(&across, Axis::X, deg(360)).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-036");
        // touching the axis is fine
        assert!(revolve(&profile(&rectangle(0, 0, 10, 5)), Axis::X, deg(360)).is_ok());
        let p = profile(&rectangle(0, 5, 10, 15));
        for bad in [0, -90, 361, 720] {
            let e = revolve(&p, Axis::X, deg(bad)).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-007", "{bad}");
        }
        assert!(revolve(&p, Axis::X, Angle::new(360_000_000_000).unwrap()).is_ok());
    }

    #[test]
    fn a_circle_turned_about_an_axis_is_a_torus() {
        // radius 2 mm, centre 10 mm from the axis: V = 2 pi^2 R r^2 = 80 pi^2 = 789.568 mm3 and
        // S = 4 pi^2 R r = 80 pi^2 = 789.568 mm2
        let mut s = new();
        let c = s.add_point(mm(0), mm(10)).unwrap();
        s.add_circle(c, mm(2)).unwrap();
        let t = text(&s, Axis::X, 360);
        assert!(t.contains("volume: 789.568 mm3"), "{t}");
        assert!(t.contains("surface: 789.568 mm2"), "{t}");
        assert!(t.contains("distance from the axis: 8 to 12 mm"), "{t}");
        assert!(t.contains("along the axis: -2 to 2 mm"), "{t}");
    }

    #[test]
    fn a_half_disc_with_its_diameter_on_the_axis_is_a_sphere() {
        // radius 3 mm: V = 4/3 pi 27 = 36 pi = 113.097 mm3 and S = 4 pi 9 = 36 pi = 113.097 mm2
        let mut s = new();
        let a = s.add_point(mm(-3), mm(0)).unwrap();
        let b = s.add_point(mm(3), mm(0)).unwrap();
        let o = s.add_point(mm(0), mm(0)).unwrap();
        s.add_line(a, b).unwrap();
        s.add_arc(o, b, a).unwrap();
        let t = text(&s, Axis::X, 360);
        assert!(t.contains("volume: 113.097 mm3"), "{t}");
        assert!(t.contains("surface: 113.097 mm2"), "{t}");
        assert!(t.contains("profile area: 14.137 mm2"), "{t}");
        // the arc reaches 3 mm from the axis (the top of the half disc) and the axis itself
        assert!(t.contains("distance from the axis: 0 to 3 mm"), "{t}");
        assert!(t.contains("along the axis: -3 to 3 mm"), "{t}");
    }

    #[test]
    fn a_hole_in_the_profile_takes_its_ring_away() {
        // 20 x 20 mm from 10 to 30 mm of the axis with a hole of 2 mm radius at 20 mm:
        // 2 pi (400 x 20 - 4 pi x 20) = 16000 pi - 160 pi^2 = 48686.346 mm3
        let mut s = rectangle(0, 10, 20, 30);
        let c = s.add_point(mm(10), mm(20)).unwrap();
        s.add_circle(c, mm(2)).unwrap();
        let t = text(&s, Axis::X, 360);
        assert!(t.contains("volume: 48686.346 mm3"), "{t}");
        // the surface counts the wall of the hole too: the boundary integral is 20 x 10 + 20 x 30
        // + 2 x 20 x 20 for the square and 2 pi x 2 x 20 = 80 pi for the hole, times 2 pi
        let s2 = 2.0 * PI * (1600.0 + 80.0 * PI);
        assert!(t.contains(&format!("surface: {s2:.3} mm2")), "{t}");
    }

    #[test]
    fn an_arc_walked_clockwise_takes_its_notch_away() {
        // 20 x 20 mm from 10 to 30 mm of the axis with a half disc of 5 mm radius cut from the
        // top edge (centred on it at 10 mm): the arc is walked clockwise in the outline.
        // V = 16000 pi - (750 pi^2 - 500 pi / 3) = 43386.878 mm3 and the boundary integral is
        // 200 + 400 + 400 + 300 + 5 (30 pi - 10), so S = 2 pi that = 10814.863 mm2
        let mut s = new();
        let a = s.add_point(mm(0), mm(10)).unwrap();
        let b = s.add_point(mm(20), mm(10)).unwrap();
        let c = s.add_point(mm(20), mm(30)).unwrap();
        let d = s.add_point(mm(15), mm(30)).unwrap();
        let e = s.add_point(mm(5), mm(30)).unwrap();
        let f = s.add_point(mm(0), mm(30)).unwrap();
        let o = s.add_point(mm(10), mm(30)).unwrap();
        for (p, q) in [(a, b), (b, c), (c, d), (e, f), (f, a)] {
            s.add_line(p, q).unwrap();
        }
        // from the right end of the notch round the bottom to the left end: clockwise
        s.add_arc(o, e, d).unwrap();
        let t = text(&s, Axis::X, 360);
        assert!(t.contains("volume: 43386.878 mm3"), "{t}");
        assert!(t.contains("surface: 10814.863 mm2"), "{t}");
    }

    #[test]
    fn the_mass_comes_from_the_density_and_the_words_from_the_language() {
        let s = rectangle(0, 5, 10, 15);
        let r = revolve(&profile(&s), Axis::X, deg(360)).unwrap();
        // 6283.185 mm3 of aluminium at 2700 kg/m3 is 16.965 g
        let es = revolve_text(&r, Some(("aluminium", 2700)), Lang::Es).unwrap();
        assert!(es.starts_with("giro: 360 grados\n"), "{es}");
        assert!(es.contains("volumen: 6283.185 mm3\n"), "{es}");
        assert!(es.ends_with("masa de aluminium: 16.965 g\n"), "{es}");
        for l in Lang::ALL {
            let t = revolve_text(&r, Some(("steel", 7850)), l).unwrap();
            assert_eq!(t.lines().count(), 7, "{}", l.code());
            assert!(t.contains("6283.185"), "{}", l.code());
        }
        assert_eq!(Axis::from_name("x"), Some(Axis::X));
        assert_eq!(Axis::from_name("z"), None);
        assert_eq!(Axis::Y.name(), "y");
    }

    fn solid(s: &Sketch, axis: Axis, degrees: i64) -> Mesh {
        mesh(&profile(s), axis, deg(degrees), 128, 1_000_000).unwrap()
    }

    /// Whether the volume of the triangles is within `ppm` parts per million of `mm3`.
    fn volume_is(m: &Mesh, mm3: f64, ppm: f64) -> bool {
        let v = m.volume_nm3() / 1e18;
        (v - mm3).abs() <= mm3 * ppm / 1e6
    }

    #[test]
    fn a_tube_is_a_closed_surface_with_the_volume_of_the_theorem() {
        // pi (15^2 - 5^2) 10 = 6283.185 mm3; 128 sides take 0.04 % of it
        let tube = solid(&rectangle(0, 5, 10, 15), Axis::X, 360);
        tube.check_closed().unwrap();
        assert!(volume_is(&tube, 6283.185, 1000.0), "{}", tube.volume_nm3());
        // 4 corners, 128 steps: a quad of two triangles for each edge and step
        assert_eq!(tube.triangles.len(), 4 * 128 * 2);
        // the same tube about the second axis, and below the axis
        let y = solid(&rectangle(5, 0, 15, 10), Axis::Y, 360);
        y.check_closed().unwrap();
        assert!(volume_is(&y, 6283.185, 1000.0));
        let below = solid(&rectangle(0, -15, 10, -5), Axis::X, 360);
        below.check_closed().unwrap();
        assert!(volume_is(&below, 6283.185, 1000.0));
        // the tube lies on the first axis, from 0 to 10 mm, and 15 mm round it
        let (lo, hi) = tube.bounds().unwrap();
        assert!((lo[0]).abs() < 1.0 && (hi[0] - 10e6).abs() < 1.0);
        assert!((hi[1] - 15e6).abs() < 1.0 && (lo[2] + 15e6).abs() < 1.0);
    }

    #[test]
    fn a_part_of_a_turn_is_closed_by_a_flat_face_at_each_end() {
        let s = rectangle(0, 5, 10, 15);
        for (degrees, volume) in [(180, 3141.593), (90, 1570.796), (45, 785.398)] {
            let half = solid(&s, Axis::X, degrees);
            half.check_closed().unwrap();
            assert!(
                volume_is(&half, volume, 1500.0),
                "{degrees}: {}",
                half.volume_nm3()
            );
        }
        let y = solid(&rectangle(5, 0, 15, 10), Axis::Y, 90);
        y.check_closed().unwrap();
        assert!(volume_is(&y, 1570.796, 1500.0));
        // 360 degrees less a little still has its two ends
        let almost = solid(&s, Axis::X, 359);
        almost.check_closed().unwrap();
    }

    #[test]
    fn holes_and_the_axis_are_part_of_the_surface() {
        // the block with a round hole of the volume test: 48686.346 mm3, and the hole is a wall
        let mut s = rectangle(0, 10, 20, 30);
        let c = s.add_point(mm(10), mm(20)).unwrap();
        s.add_circle(c, mm(2)).unwrap();
        let block = solid(&s, Axis::X, 360);
        block.check_closed().unwrap();
        assert!(
            volume_is(&block, 48686.346, 3000.0),
            "{}",
            block.volume_nm3()
        );
        let cut = solid(&s, Axis::X, 120);
        cut.check_closed().unwrap();
        // a half disc on the axis is a ball: the points on the axis are single vertices
        let mut d = new();
        let a = d.add_point(mm(-3), mm(0)).unwrap();
        let b = d.add_point(mm(3), mm(0)).unwrap();
        let o = d.add_point(mm(0), mm(0)).unwrap();
        d.add_line(a, b).unwrap();
        d.add_arc(o, b, a).unwrap();
        let ball = solid(&d, Axis::X, 360);
        ball.check_closed().unwrap();
        assert!(volume_is(&ball, 113.097, 3000.0), "{}", ball.volume_nm3());
        let quarter = solid(&d, Axis::X, 90);
        quarter.check_closed().unwrap();
        assert!(volume_is(&quarter, 28.274, 3000.0));
    }

    #[test]
    fn a_circle_turned_is_a_closed_torus_and_the_limits_and_refusals_are_kept() {
        let mut s = new();
        let c = s.add_point(mm(0), mm(10)).unwrap();
        s.add_circle(c, mm(2)).unwrap();
        let ring = solid(&s, Axis::X, 360);
        ring.check_closed().unwrap();
        assert!(volume_is(&ring, 789.568, 3000.0), "{}", ring.volume_nm3());
        // a limit of triangles
        let e = mesh(&profile(&s), Axis::X, deg(360), 128, 100).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-050");
        // what revolve refuses, mesh refuses
        let across = profile(&rectangle(0, -5, 10, 5));
        assert_eq!(
            mesh(&across, Axis::X, deg(360), 128, 1_000_000)
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-036"
        );
        assert_eq!(
            mesh(&profile(&s), Axis::X, deg(0), 128, 1_000_000)
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-007"
        );
        // fewer sides, fewer triangles; the same input gives the same bytes
        let coarse = mesh(&profile(&s), Axis::X, deg(360), 16, 1_000_000).unwrap();
        assert!(coarse.triangles.len() < ring.triangles.len());
        let again = mesh(&profile(&s), Axis::X, deg(360), 16, 1_000_000).unwrap();
        assert_eq!(coarse, again);
    }
}
