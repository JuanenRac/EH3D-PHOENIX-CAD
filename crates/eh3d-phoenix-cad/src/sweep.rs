// SPDX-License-Identifier: GPL-3.0-or-later
//! The solid made by sweeping a profile along a path of a sketch: its volume and its surface.
//!
//! The profile (the closed outlines of a sketch, see [`crate::profile`]) is kept at right angles to
//! the path, with its origin on the path and its first axis pointing to the left of the direction
//! the path is walked; the second axis points out of the plane of the path. The path is a chain of
//! lines and arcs of another sketch (or one circle, a closed path) walked from the free end of its
//! first element, and the plane of the path is the plane of that sketch.
//!
//! Along a straight piece the solid is a prism of length `L`. Along an arc of radius `R` and angle
//! `a` a point of the profile at `x` to the left sweeps the radius `R - x` (turning left) or
//! `R + x` (turning right). At a corner between two lines the pieces are cut by the plane that
//! halves the corner (a mitre), so a point at `x` loses `x tan(f / 2)` on each piece, `f` being the
//! turn of the corner. With the area `A` and the first moment `Mx` of the profile, its perimeter
//! `P` and the first moment `Mb` of its boundary (the holes included), and the signed turn
//! `T = (sum of the angles of the arcs) + (sum of 2 tan(f / 2) at the corners)`, left turns being
//! positive, the volume is `A L - Mx T` and the lateral surface is `P L - Mb T`; an open path adds
//! the two flat ends, `A` each. The figures are worked out in floating point and rounded to a cubic
//! or square nanometre.
//!
//! Refused with `PHOENIX-CAD-036`: a path that branches, has several pieces, or turns back on
//! itself; an arc that meets its neighbour at a corner (make them tangent); a profile that reaches
//! the centre of a bend; and a piece too short for the mitres next to it with this profile. Only
//! the figures are made; no mesh and no file of the swept solid.

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::lang::{fill, Lang, Msg};
use crate::mass::mass_micrograms;
use crate::profile::{Outline, Piece, Profile, Pt};
use crate::revolve::{arc_figures, boundary_moment, exact_bounds, outline_moments, scaled};
use crate::sketch::{Element, ElementId, Sketch};
use crate::CadError;

/// Two directions closer than this (radians) are the same: the pieces are tangent.
const TANGENT: f64 = 1e-4;

/// A corner this close to a half turn is a path that turns back on itself.
const REVERSAL: f64 = 1e-3;

fn bad(why: impl Into<String>) -> CadError {
    CadError::Profile(why.into())
}

/// The figures of a swept solid.
#[derive(Debug, Clone, PartialEq)]
pub struct Swept {
    /// The length of the path in nanometres.
    pub path_nm: f64,
    /// The number of pieces (lines and arcs) of the path.
    pub pieces: usize,
    /// Whether the path closes on itself (then the solid has no flat ends).
    pub closed: bool,
    /// The area of the profile in square nanometres.
    pub profile_nm2: f64,
    /// The perimeter of the profile, the holes included, in nanometres.
    pub perimeter_nm: f64,
    /// The volume in cubic nanometres.
    pub volume_nm3: i128,
    /// The surface in square nanometres: the sides and, for an open path, the two flat ends.
    pub surface_nm2: i128,
}

/// A path of a sketch in the order it is walked.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    /// The pieces, each walked from its start to its end.
    pub pieces: Vec<Piece>,
    /// Whether the last piece ends where the first starts.
    pub closed: bool,
    /// Set when the path is one circle (then there are no pieces).
    pub circle: Option<(Pt, i64)>,
}

fn point_of(sketch: &Sketch, id: ElementId) -> Result<Pt, CadError> {
    let (x, y) = sketch.point(id)?;
    Ok((x.get(), y.get()))
}

/// The path a sketch draws: its lines and arcs (construction geometry left out) joined end to end,
/// or one circle. The walk starts at the free end of the first element that has one (the start of
/// the first element when the path is closed).
pub fn path_of(sketch: &Sketch) -> Result<Path, CadError> {
    let mut pieces: Vec<Piece> = Vec::new();
    let mut circle: Option<(Pt, i64)> = None;
    let mut circles = 0usize;
    for entry in sketch.elements() {
        if entry.construction {
            continue;
        }
        match entry.element {
            Element::Point { .. } => {}
            Element::Line { a, b } => pieces.push(Piece::Line {
                from: point_of(sketch, a)?,
                to: point_of(sketch, b)?,
            }),
            Element::Arc { center, start, end } => pieces.push(Piece::Arc {
                center: point_of(sketch, center)?,
                from: point_of(sketch, start)?,
                to: point_of(sketch, end)?,
                ccw: true,
            }),
            Element::Circle { center, radius } => {
                circles += 1;
                circle = Some((point_of(sketch, center)?, radius.get()));
            }
        }
    }
    if circles > 0 {
        if circles > 1 || !pieces.is_empty() {
            return Err(bad("a circle cannot be joined to other pieces of a path"));
        }
        return Ok(Path {
            pieces,
            closed: true,
            circle,
        });
    }
    if pieces.is_empty() {
        return Err(bad("the sketch of the path has no line or arc"));
    }
    let ends = |p: &Piece| match *p {
        Piece::Line { from, to } | Piece::Arc { from, to, .. } => (from, to),
    };
    let mut at: BTreeMap<Pt, usize> = BTreeMap::new();
    for p in &pieces {
        let (a, b) = ends(p);
        *at.entry(a).or_insert(0) += 1;
        *at.entry(b).or_insert(0) += 1;
    }
    if at.values().any(|n| *n > 2) {
        return Err(bad("three pieces of the path meet at one point"));
    }
    let free: Vec<Pt> = at
        .iter()
        .filter(|(_, n)| **n == 1)
        .map(|(p, _)| *p)
        .collect();
    if !free.is_empty() && free.len() != 2 {
        return Err(bad("the path is not one chain of pieces"));
    }
    let closed = free.is_empty();
    let start = if closed {
        ends(&pieces[0]).0
    } else {
        let mut found = None;
        for p in &pieces {
            let (a, b) = ends(p);
            if free.contains(&a) {
                found = Some(a);
                break;
            }
            if free.contains(&b) {
                found = Some(b);
                break;
            }
        }
        found.ok_or_else(|| bad("the path has no free end"))?
    };
    let mut used = vec![false; pieces.len()];
    let mut walked: Vec<Piece> = Vec::with_capacity(pieces.len());
    let mut here = start;
    while let Some(i) = (0..pieces.len()).find(|i| {
        let (a, b) = ends(&pieces[*i]);
        !used[*i] && (a == here || b == here)
    }) {
        used[i] = true;
        let piece = pieces[i];
        let oriented = if ends(&piece).0 == here {
            piece
        } else {
            reversed(&piece)
        };
        here = ends(&oriented).1;
        walked.push(oriented);
    }
    if walked.len() != pieces.len() {
        return Err(bad("the path is in more than one piece"));
    }
    Ok(Path {
        pieces: walked,
        closed,
        circle: None,
    })
}

fn reversed(piece: &Piece) -> Piece {
    match *piece {
        Piece::Line { from, to } => Piece::Line { from: to, to: from },
        Piece::Arc {
            center,
            from,
            to,
            ccw,
        } => Piece::Arc {
            center,
            from: to,
            to: from,
            ccw: !ccw,
        },
    }
}

/// One piece of the path with what the sweep needs of it.
struct Leg {
    len: f64,
    /// The direction of the path where the piece starts and where it ends, in radians.
    a_in: f64,
    a_out: f64,
    /// The radius and the side of an arc (true when it turns left), none for a line.
    bend: Option<(f64, bool)>,
    /// The signed angle the piece turns through.
    turn: f64,
}

fn leg_of(piece: &Piece) -> Leg {
    match *piece {
        Piece::Line { from, to } => {
            let (dx, dy) = ((to.0 - from.0) as f64, (to.1 - from.1) as f64);
            let a = dy.atan2(dx);
            Leg {
                len: dx.hypot(dy),
                a_in: a,
                a_out: a,
                bend: None,
                turn: 0.0,
            }
        }
        Piece::Arc {
            center,
            from,
            to,
            ccw,
        } => {
            let (r, theta, a0) = arc_figures(center, from, to, ccw);
            let a1 = ((to.1 - center.1) as f64).atan2((to.0 - center.0) as f64);
            let side = if ccw { FRAC_PI_2 } else { -FRAC_PI_2 };
            Leg {
                len: r * theta,
                a_in: a0 + side,
                a_out: a1 + side,
                bend: Some((r, ccw)),
                turn: if ccw { theta } else { -theta },
            }
        }
    }
}

/// An angle brought into the range above minus a half turn up to a half turn.
fn wrap(a: f64) -> f64 {
    let r = a.rem_euclid(TAU);
    if r > PI {
        r - TAU
    } else {
        r
    }
}

/// The length of the boundary of an outline.
fn outline_length(o: &Outline) -> f64 {
    if let Some((_, r)) = o.circle_of() {
        return TAU * r as f64;
    }
    o.pieces()
        .iter()
        .map(|p| match *p {
            Piece::Line { from, to } => ((to.0 - from.0) as f64).hypot((to.1 - from.1) as f64),
            Piece::Arc {
                center,
                from,
                to,
                ccw,
            } => {
                let (r, theta, _) = arc_figures(center, from, to, ccw);
                r * theta
            }
        })
        .sum()
}

/// The figures of a profile swept along a path. Refused with `PHOENIX-CAD-036` for what the module
/// documentation lists.
pub fn sweep(profile: &Profile, path: &Path) -> Result<Swept, CadError> {
    let legs: Vec<Leg> = match path.circle {
        Some((_, r)) => vec![Leg {
            len: TAU * r as f64,
            a_in: 0.0,
            a_out: 0.0,
            bend: Some((r as f64, true)),
            turn: TAU,
        }],
        None => path.pieces.iter().map(leg_of).collect(),
    };
    let (lo, hi) = exact_bounds(profile);
    let (xmin, xmax) = (lo.0 as f64, hi.0 as f64);
    let reach_left = xmax.max(0.0);
    let reach_right = (-xmin).max(0.0);

    // the corners: the turn of each joint, none where the pieces are tangent
    let n = legs.len();
    let joints = if path.closed { n } else { n.saturating_sub(1) };
    let mut corner = vec![0.0f64; n + 1];
    let mut turn: f64 = legs.iter().map(|l| l.turn).sum();
    if path.circle.is_none() {
        for j in 0..joints {
            let (before, after) = (&legs[j], &legs[(j + 1) % n]);
            let phi = wrap(after.a_in - before.a_out);
            if phi.abs() < TANGENT {
                continue;
            }
            if before.bend.is_some() || after.bend.is_some() {
                return Err(bad(
                    "an arc of the path meets the next piece at a corner: make them tangent",
                ));
            }
            if phi.abs() > PI - REVERSAL {
                return Err(bad("the path turns back on itself"));
            }
            corner[j] = phi;
            turn += 2.0 * (phi / 2.0).tan();
        }
    }
    // a bend cannot be smaller than the reach of the profile towards its centre
    for l in &legs {
        if let Some((r, left)) = l.bend {
            let reach = if left { reach_left } else { reach_right };
            if reach >= r {
                return Err(bad("the profile reaches the centre of a bend of the path"));
            }
        }
    }
    // the side of a corner that is inside loses length on both pieces; a piece cannot lose more
    // than it has
    for (i, l) in legs.iter().enumerate() {
        if l.bend.is_some() {
            continue;
        }
        let before = if i > 0 {
            corner[i - 1]
        } else if path.closed {
            corner[n - 1]
        } else {
            0.0
        };
        let after = if i + 1 < n || path.closed {
            corner[i]
        } else {
            0.0
        };
        let lost = |phi: f64| {
            let inner = if phi > 0.0 { reach_left } else { reach_right };
            inner * (phi.abs() / 2.0).tan()
        };
        if lost(before) + lost(after) > l.len * (1.0 + 1e-12) {
            return Err(bad(
                "a piece of the path is too short for the corners next to it with this profile",
            ));
        }
    }

    let length: f64 = legs.iter().map(|l| l.len).sum();
    let (mut area, mut mx, mut perimeter, mut mb) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for region in &profile.regions {
        let (a, m) = outline_moments(&region.outer, 0);
        area += a;
        mx += m;
        perimeter += outline_length(&region.outer);
        mb += boundary_moment(&region.outer, 0);
        for hole in &region.holes {
            let (a, m) = outline_moments(hole, 0);
            area -= a;
            mx -= m;
            perimeter += outline_length(hole);
            mb += boundary_moment(hole, 0);
        }
    }
    let volume = area * length - mx * turn;
    let sides = perimeter * length - mb * turn;
    let surface = sides + if path.closed { 0.0 } else { 2.0 * area };
    Ok(Swept {
        path_nm: length,
        pieces: if path.circle.is_some() { 1 } else { n },
        closed: path.closed,
        profile_nm2: area,
        perimeter_nm: perimeter,
        volume_nm3: volume.round() as i128,
        surface_nm2: surface.round() as i128,
    })
}

/// The figures as lines of text in a language. With a material (its name and its density in
/// kilograms per cubic metre) the mass is given too.
pub fn sweep_text(
    s: &Swept,
    material: Option<(&str, u32)>,
    lang: Lang,
) -> Result<String, CadError> {
    let mut lines = vec![
        fill(
            Msg::SwpPath.tr(lang),
            &[
                &scaled(s.path_nm.round() as i128, 1_000_000),
                &s.pieces.to_string(),
            ],
        ),
        fill(
            Msg::RevArea.tr(lang),
            &[&scaled(s.profile_nm2.round() as i128, 1_000_000_000_000)],
        ),
        fill(
            Msg::SwpPerimeter.tr(lang),
            &[&length_text_3(s.perimeter_nm)],
        ),
        fill(
            Msg::RevVolume.tr(lang),
            &[&scaled(s.volume_nm3, 1_000_000_000_000_000_000)],
        ),
        fill(
            Msg::RevSurface.tr(lang),
            &[&scaled(s.surface_nm2, 1_000_000_000_000)],
        ),
    ];
    if let Some((name, density)) = material {
        let grams = crate::catalog::mass_g_text(mass_micrograms(s.volume_nm3, density)?);
        lines.push(fill(Msg::RevMass.tr(lang), &[name, &grams]));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    Ok(out)
}

/// A length in nanometres as millimetres with three decimals.
fn length_text_3(nm: f64) -> String {
    scaled(nm.round() as i128, 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::Face;
    use crate::profile::ProfileLimits;
    use crate::sketch::SketchPlane;
    use crate::Length;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn new(name: &str) -> Sketch {
        Sketch::new(name, SketchPlane::on_face(Face::Front)).unwrap()
    }

    fn profile(s: &Sketch) -> Profile {
        Profile::from_sketch(s, &ProfileLimits::shipped()).unwrap()
    }

    fn rectangle(x0: i64, y0: i64, x1: i64, y1: i64) -> Sketch {
        let mut s = new("profile");
        s.add_rectangle((mm(x0), mm(y0)), (mm(x1), mm(y1))).unwrap();
        s
    }

    fn circle(x: i64, r: i64) -> Sketch {
        let mut s = new("profile");
        let c = s.add_point(mm(x), mm(0)).unwrap();
        s.add_circle(c, mm(r)).unwrap();
        s
    }

    fn polyline(points: &[(i64, i64)], closed: bool) -> Sketch {
        let mut s = new("path");
        let pts: Vec<_> = points.iter().map(|(x, y)| (mm(*x), mm(*y))).collect();
        s.add_polyline(&pts, closed).unwrap();
        s
    }

    fn figures(profile: &Sketch, path: &Sketch) -> Swept {
        sweep(&self::profile(profile), &path_of(path).unwrap()).unwrap()
    }

    fn text(profile: &Sketch, path: &Sketch) -> String {
        sweep_text(&figures(profile, path), None, Lang::En).unwrap()
    }

    fn refused(profile: &Sketch, path: &Sketch) -> String {
        let e = path_of(path)
            .and_then(|p| sweep(&self::profile(profile), &p))
            .unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-036");
        e.to_string()
    }

    #[test]
    fn a_straight_tube_is_the_ring_times_the_length() {
        // a ring of 5 and 3 mm radius, 100 mm long: V = pi (25 - 9) 100 = 1600 pi = 5026.548 mm3,
        // the sides are (10 pi + 6 pi) 100 = 5026.548 mm2 and the two ends are 2 x 16 pi
        let mut ring = circle(0, 5);
        let c = ring.add_point(mm(0), mm(0)).unwrap();
        ring.add_circle(c, mm(3)).unwrap();
        let path = polyline(&[(0, 0), (0, 100)], false);
        let t = text(&ring, &path);
        assert_eq!(
            t,
            "path: 100.000 mm, pieces: 1\nprofile area: 50.265 mm2\nperimeter of the profile: 50.265 mm\nvolume: 5026.548 mm3\nsurface: 5127.079 mm2\n"
        );
    }

    #[test]
    fn a_circle_swept_along_a_circle_is_a_torus() {
        // R = 20, r = 3: V = 2 pi^2 R r^2 = 360 pi^2 = 3553.058 mm3 and
        // S = 4 pi^2 R r = 240 pi^2 = 2368.705 mm2; a closed path has no ends
        let path = circle(0, 20);
        let s = figures(&circle(0, 3), &path);
        assert!(s.closed);
        assert_eq!(s.pieces, 1);
        let t = sweep_text(&s, None, Lang::En).unwrap();
        assert!(t.contains("volume: 3553.058 mm3\n"), "{t}");
        assert!(t.contains("surface: 2368.705 mm2\n"), "{t}");
        assert!(t.starts_with("path: 125.664 mm, pieces: 1\n"), "{t}");
    }

    #[test]
    fn a_bend_uses_the_radius_of_the_point_of_the_profile() {
        // a quarter of a circle of 20 mm radius, from (20, 0) to (0, 20), counter-clockwise: the
        // profile of 3 mm radius on the path gives V = 9 pi x 10 pi = 90 pi^2 = 888.264 mm3
        let mut bend = new("path");
        let o = bend.add_point(mm(0), mm(0)).unwrap();
        let a = bend.add_point(mm(20), mm(0)).unwrap();
        let b = bend.add_point(mm(0), mm(20)).unwrap();
        bend.add_arc(o, a, b).unwrap();
        let t = text(&circle(0, 3), &bend);
        assert!(t.contains("volume: 888.264 mm3\n"), "{t}");
        assert!(t.starts_with("path: 31.416 mm, pieces: 1\n"), "{t}");
        // the same profile 2 mm to the left, towards the centre of the bend: the radius of its
        // centre is 18 mm, V = (pi / 2)(20 x 9 pi - 9 pi x 2) = 81 pi^2 = 799.438 mm3 and the
        // sides are 60 pi^2 - 12 pi x pi / 2 = 54 pi^2 plus the ends 18 pi = 589.507 mm2
        let t = text(&circle(2, 3), &bend);
        assert!(t.contains("volume: 799.438 mm3\n"), "{t}");
        assert!(t.contains("surface: 589.507 mm2\n"), "{t}");
        // 2 mm to the right it is the radius of 22 mm: (pi / 2)(20 x 9 pi + 9 pi x 2) = 99 pi^2
        let t = text(&circle(-2, 3), &bend);
        assert!(t.contains("volume: 977.091 mm3\n"), "{t}");
    }

    #[test]
    fn a_mitred_corner_costs_the_inside_what_the_outside_gains() {
        // two lines of 50 mm with a left turn of 90 degrees
        let path = polyline(&[(0, 0), (50, 0), (50, 50)], false);
        // a rectangle centred on the path loses nothing: V = 8 x 100, S = 12 x 100 + 2 x 8
        let t = text(&rectangle(-2, -1, 2, 1), &path);
        assert!(t.contains("volume: 800.000 mm3\n"), "{t}");
        assert!(t.contains("surface: 1216.000 mm2\n"), "{t}");
        // the same rectangle from the path to 4 mm to the left, on the inside of the corner:
        // the length at the offset x is 100 - 2 x, so V = 100 x 8 - 2 x 16 = 768 mm3 and the sides
        // are 12 x 100 - 2 x 24 = 1152 plus the ends 2 x 8
        let t = text(&rectangle(0, -1, 4, 1), &path);
        assert!(t.contains("volume: 768.000 mm3\n"), "{t}");
        assert!(t.contains("surface: 1168.000 mm2\n"), "{t}");
        // on the outside of the corner the length grows the same way
        let t = text(&rectangle(-4, -1, 0, 1), &path);
        assert!(t.contains("volume: 832.000 mm3\n"), "{t}");
        // the path walked in the other direction has the corner to the right
        let back = polyline(&[(50, 50), (50, 0), (0, 0)], false);
        let t = text(&rectangle(0, -1, 4, 1), &back);
        assert!(t.contains("volume: 832.000 mm3\n"), "{t}");
    }

    #[test]
    fn a_closed_path_of_lines_has_no_ends() {
        // a square of 100 mm turning left four times: the length at the offset x is 400 - 8 x,
        // so V = 8 x 400 - 8 x 16 = 3072 mm3 and the sides are 12 x 400 - 8 x 24 = 4608 mm2
        let square = polyline(&[(0, 0), (100, 0), (100, 100), (0, 100)], true);
        let s = figures(&rectangle(0, -1, 4, 1), &square);
        assert!(s.closed);
        assert_eq!(s.pieces, 4);
        let t = sweep_text(&s, None, Lang::En).unwrap();
        assert!(t.contains("volume: 3072.000 mm3\n"), "{t}");
        assert!(t.contains("surface: 4608.000 mm2\n"), "{t}");
    }

    #[test]
    fn lines_and_arcs_that_are_tangent_join_without_a_corner() {
        // a line of 10 mm, then a quarter of a circle of 10 mm radius that leaves it tangent
        let mut path = new("path");
        let a = path.add_point(mm(0), mm(0)).unwrap();
        let b = path.add_point(mm(10), mm(0)).unwrap();
        let o = path.add_point(mm(10), mm(10)).unwrap();
        let c = path.add_point(mm(20), mm(10)).unwrap();
        path.add_line(a, b).unwrap();
        // from (10, 0) to (20, 10) about (10, 10), counter-clockwise from below the centre
        path.add_arc(o, b, c).unwrap();
        let s = figures(&circle(0, 1), &path);
        assert_eq!(s.pieces, 2);
        let t = sweep_text(&s, None, Lang::En).unwrap();
        // the profile has the area pi and the path is 10 + 5 pi mm long: V = pi (10 + 5 pi)
        assert!(t.contains("volume: 80.764 mm3\n"), "{t}");
        // the walk starts at the free end of the first element even when it is drawn backwards
        let mut other = new("path");
        let a = other.add_point(mm(10), mm(0)).unwrap();
        let b = other.add_point(mm(0), mm(0)).unwrap();
        other.add_line(a, b).unwrap();
        let p = path_of(&other).unwrap();
        assert_eq!(
            p.pieces,
            vec![Piece::Line {
                from: (10_000_000, 0),
                to: (0, 0)
            }]
        );
    }

    #[test]
    fn a_path_or_a_profile_that_cannot_be_swept_is_refused() {
        let line = polyline(&[(0, 0), (0, 100)], false);
        // a profile that reaches the centre of a bend
        let mut bend = new("path");
        let o = bend.add_point(mm(0), mm(0)).unwrap();
        let a = bend.add_point(mm(20), mm(0)).unwrap();
        let b = bend.add_point(mm(0), mm(20)).unwrap();
        bend.add_arc(o, a, b).unwrap();
        assert!(refused(&circle(0, 25), &bend).contains("centre of a bend"));
        // a piece too short for the corner next to it
        let short = polyline(&[(0, 0), (3, 0), (3, 3)], false);
        assert!(refused(&rectangle(0, -1, 4, 1), &short).contains("too short"));
        // a corner at the end of an arc
        let mut corner = new("path");
        let o = corner.add_point(mm(0), mm(0)).unwrap();
        let a = corner.add_point(mm(10), mm(0)).unwrap();
        let b = corner.add_point(mm(0), mm(10)).unwrap();
        let c = corner.add_point(mm(0), mm(20)).unwrap();
        corner.add_arc(o, a, b).unwrap();
        corner.add_line(b, c).unwrap();
        assert!(refused(&circle(0, 1), &corner).contains("at a corner"));
        // a path that turns back on itself
        let back = polyline(&[(0, 0), (50, 0), (0, 0)], false);
        assert!(refused(&circle(0, 1), &back).contains("turns back"));
        // three pieces at one point, two chains, no pieces, a circle with a line
        let mut fork = new("path");
        let c = fork.add_point(mm(0), mm(0)).unwrap();
        for (x, y) in [(10, 0), (0, 10), (-10, 0)] {
            let p = fork.add_point(mm(x), mm(y)).unwrap();
            fork.add_line(c, p).unwrap();
        }
        assert!(refused(&circle(0, 1), &fork).contains("meet at one point"));
        let mut two = polyline(&[(0, 0), (10, 0)], false);
        let p = two.add_point(mm(0), mm(20)).unwrap();
        let q = two.add_point(mm(10), mm(20)).unwrap();
        two.add_line(p, q).unwrap();
        assert!(refused(&circle(0, 1), &two).contains("not one chain"));
        assert!(refused(&circle(0, 1), &new("path")).contains("no line or arc"));
        let mut mixed = circle(0, 5);
        let p = mixed.add_point(mm(0), mm(20)).unwrap();
        let q = mixed.add_point(mm(10), mm(20)).unwrap();
        mixed.add_line(p, q).unwrap();
        assert!(refused(&circle(0, 1), &mixed).contains("cannot be joined"));
        // and the straight line itself is fine
        assert!(path_of(&line).is_ok());
    }

    #[test]
    fn the_mass_and_the_words_follow_the_material_and_the_language() {
        // the square tube of 10 mm side, 50 mm long, 7850 kg/m3: 5000 mm3 is 39.250 g
        let s = figures(
            &rectangle(-5, -5, 5, 5),
            &polyline(&[(0, 0), (50, 0)], false),
        );
        let es = sweep_text(&s, Some(("steel", 7850)), Lang::Es).unwrap();
        assert!(es.starts_with("camino: 50.000 mm, tramos: 1\n"), "{es}");
        assert!(es.ends_with("masa de steel: 39.250 g\n"), "{es}");
        for l in Lang::ALL {
            let t = sweep_text(&s, Some(("steel", 7850)), l).unwrap();
            assert_eq!(t.lines().count(), 6, "{}", l.code());
            assert!(t.contains("5000.000"), "{}", l.code());
        }
    }
}
