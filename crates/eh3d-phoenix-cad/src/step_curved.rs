// SPDX-License-Identifier: GPL-3.0-or-later
//! The curved parts of a STEP file that are read: arcs of circles as edges, and faces on a
//! cylinder or a cone (ISO 10303-42, `CIRCLE`, `CYLINDRICAL_SURFACE` and `CONICAL_SURFACE`).
//!
//! An arc becomes straight pieces, so many for each whole turn; the two faces that share an edge
//! get the same points, so the surface of triangles has no crack. A face on a cylinder or a cone is
//! cut into strips between its lower and its upper outline, laid flat by the angle about the axis
//! and the height along it. The shapes that are read are the whole wall between two circles, a part
//! of it between two arcs and two straight sides, and the point of a cone; anything else is refused
//! and nothing is guessed.

use std::f64::consts::{PI, TAU};

use crate::mesh::P3;

/// How far from its circle, cylinder or cone a corner may be, in nanometres (ten microns).
pub(crate) const ON_NM: f64 = 10_000.0;

/// Angles closer than this are one angle.
const SAME: f64 = 1.0e-7;

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: P3) -> Option<P3> {
    let length = dot(a, a).sqrt();
    (length > 1.0e-9).then(|| a.map(|v| v / length))
}

/// The place and the directions of a circle, a cylinder or a cone (`AXIS2_PLACEMENT_3D`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    origin: P3,
    x: P3,
    y: P3,
    z: P3,
}

impl Frame {
    /// A frame at `origin` (nanometres) with the axis `z` and the direction of the angle zero near
    /// `x`; `None` when the two directions are the same one.
    pub(crate) fn new(origin: P3, z: Option<P3>, x: Option<P3>) -> Option<Frame> {
        let z = unit(z.unwrap_or([0.0, 0.0, 1.0]))?;
        let across = x.unwrap_or(if z[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        });
        let along = dot(across, z);
        let x = unit([0, 1, 2].map(|k| across[k] - along * z[k]))?;
        Some(Frame {
            origin,
            x,
            y: cross(z, x),
            z,
        })
    }

    /// The angle of a point about the axis, its distance from the axis and its height along it.
    pub(crate) fn coordinates(&self, p: P3) -> (f64, f64, f64) {
        let d = sub(p, self.origin);
        let (x, y) = (dot(d, self.x), dot(d, self.y));
        (y.atan2(x), x.hypot(y), dot(d, self.z))
    }

    fn at(&self, angle: f64, radius: f64) -> P3 {
        let (sine, cosine) = angle.sin_cos();
        [0, 1, 2].map(|k| {
            (self.origin[k] + radius * (cosine * self.x[k] + sine * self.y[k])).round() + 0.0
        })
    }
}

/// The corners of an arc from `a` to `b` on the circle of a frame: counter-clockwise about the
/// axis when `with_axis`, clockwise otherwise, and a whole turn when the two ends are one point.
/// The ends are the points given; the corners between them are `segments` for each whole turn.
pub(crate) fn arc_points(
    frame: &Frame,
    radius: f64,
    a: P3,
    b: P3,
    with_axis: bool,
    segments: u32,
) -> Result<Vec<P3>, String> {
    if !(radius.is_finite() && radius > ON_NM) {
        return Err("the circle has no radius".to_owned());
    }
    let angle_of = |p: P3| {
        let (angle, radial, height) = frame.coordinates(p);
        if (radial - radius).abs() > ON_NM || height.abs() > ON_NM {
            Err("an end of the edge is not on its circle".to_owned())
        } else {
            Ok(angle)
        }
    };
    let (from, to) = (angle_of(a)?, angle_of(b)?);
    let turned = if with_axis { to - from } else { from - to }.rem_euclid(TAU);
    let sweep = if a == b || turned < SAME || TAU - turned < SAME {
        TAU
    } else {
        turned
    };
    let segments = segments.max(3);
    let pieces = (sweep / TAU * f64::from(segments) - 1.0e-6).ceil().max(1.0) as u32;
    let sign = if with_axis { 1.0 } else { -1.0 };
    let mut out = Vec::with_capacity(pieces as usize + 1);
    out.push(a);
    for k in 1..pieces {
        let angle = from + sign * sweep * f64::from(k) / f64::from(pieces);
        out.push(frame.at(angle, radius));
    }
    out.push(b);
    Ok(out)
}

/// A corner of a bound of a face on a cylinder or a cone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Corner {
    /// The number of its vertex in the surface.
    pub id: u32,
    /// Its angle about the axis; none for a corner on the axis (the point of a cone).
    pub angle: Option<f64>,
    /// Its height along the axis.
    pub height: f64,
}

/// A corner laid flat.
#[derive(Debug, Clone, Copy)]
struct Flat {
    id: u32,
    u: f64,
    v: f64,
}

const SHAPE: &str = "has a shape that is not read (the wall between two circles, a part of it \
                     between two arcs and two straight sides, or the point of a cone)";

fn shape() -> String {
    SHAPE.to_owned()
}

/// The turn from one angle to the next, the short way.
fn turn(from: f64, to: f64) -> f64 {
    let d = (to - from).rem_euclid(TAU);
    if d > PI {
        d - TAU
    } else {
        d
    }
}

/// The corners of a bound with their angles made continuous from one to the next.
fn unwrap(corners: &[Corner]) -> Result<Vec<Flat>, String> {
    let mut out: Vec<Flat> = Vec::with_capacity(corners.len());
    let mut last: Option<(f64, f64)> = None;
    for c in corners {
        let angle = c.angle.ok_or_else(shape)?;
        let u = match last {
            None => angle,
            Some((before, u)) => u + turn(before, angle),
        };
        last = Some((angle, u));
        out.push(Flat {
            id: c.id,
            u,
            v: c.height,
        });
    }
    Ok(out)
}

/// How many whole turns a closed bound makes about the axis: -1, 0 or 1.
fn turns(flat: &[Flat]) -> i32 {
    match (flat.first(), flat.last()) {
        (Some(first), Some(last)) => {
            let closing = turn(last.u.rem_euclid(TAU), first.u.rem_euclid(TAU));
            ((last.u + closing - first.u) / TAU).round() as i32
        }
        _ => 0,
    }
}

/// A bound that goes once round the axis, in the order of the angle, with its first corner
/// repeated one turn later.
fn ring(mut flat: Vec<Flat>) -> Result<Vec<Flat>, String> {
    match turns(&flat) {
        1 => {}
        -1 => flat.reverse(),
        _ => return Err(shape()),
    }
    let first = *flat.first().ok_or_else(shape)?;
    flat.push(Flat {
        u: first.u + TAU,
        ..first
    });
    Ok(flat)
}

/// Twice the area of a closed outline laid flat, positive when it goes counter-clockwise.
fn area2(points: &[Flat]) -> f64 {
    let mut sum = 0.0;
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        sum += a.u * b.v - b.u * a.v;
    }
    sum
}

/// The triangles between a lower and an upper outline, both in the order of the angle: the one
/// whose next corner comes first goes on. They go counter-clockwise seen with the angle to the
/// right and the height up.
fn zip(lower: &[Flat], upper: &[Flat], widest: f64) -> Result<Vec<[u32; 3]>, String> {
    let rising = |chain: &[Flat]| chain.windows(2).all(|w| w[1].u >= w[0].u - SAME);
    if lower.is_empty() || upper.is_empty() || !rising(lower) || !rising(upper) {
        return Err(shape());
    }
    let outline: Vec<Flat> = lower.iter().chain(upper.iter().rev()).copied().collect();
    let whole = area2(&outline);
    // the area is told apart from nothing against the box of the outline
    let tallest = outline.iter().fold((f64::MAX, f64::MIN), |(low, high), p| {
        (low.min(p.v), high.max(p.v))
    });
    let tolerance = 1.0e-6 * (tallest.1 - tallest.0).abs().max(1.0);
    if whole <= tolerance {
        return Err(shape());
    }
    let (mut i, mut j) = (0, 0);
    let mut sum = 0.0;
    let mut out = Vec::with_capacity(lower.len() + upper.len());
    while i + 1 < lower.len() || j + 1 < upper.len() {
        let below = j + 1 >= upper.len()
            || (i + 1 < lower.len() && lower[i + 1].u <= upper[j + 1].u + SAME);
        let triangle = if below {
            i += 1;
            [lower[i - 1], lower[i], upper[j]]
        } else {
            j += 1;
            [lower[i], upper[j], upper[j - 1]]
        };
        let area = area2(&triangle);
        let (low, high) = triangle
            .iter()
            .fold((f64::MAX, f64::MIN), |(low, high), p| {
                (low.min(p.u), high.max(p.u))
            });
        if area < -tolerance || high - low > widest {
            return Err(shape());
        }
        sum += area;
        let [a, b, c] = triangle.map(|p| p.id);
        if a != b && b != c && a != c {
            out.push([a, b, c]);
        }
    }
    if (sum - whole).abs() > 1.0e-6 * whole.abs() + tolerance {
        return Err(shape());
    }
    Ok(out)
}

/// The two sides of one bound that does not go round the axis: the lower and the upper outline.
fn sides(mut flat: Vec<Flat>) -> Result<(Vec<Flat>, Vec<Flat>), String> {
    if area2(&flat) < 0.0 {
        flat.reverse();
    }
    let count = flat.len();
    let (left, right) = flat.iter().fold((f64::MAX, f64::MIN), |(low, high), p| {
        (low.min(p.u), high.max(p.u))
    });
    // the lowest or the highest corner of a side
    let corner = |u: f64, highest: bool| {
        (0..count)
            .filter(|&i| (flat[i].u - u).abs() < SAME)
            .reduce(|a, b| {
                if (flat[b].v > flat[a].v) == highest {
                    b
                } else {
                    a
                }
            })
            .ok_or_else(shape)
    };
    let (left_low, left_high) = (corner(left, false)?, corner(left, true)?);
    let (right_low, right_high) = (corner(right, false)?, corner(right, true)?);
    let steps = |from: usize, to: usize| (to + count - from) % count;
    // a side is one straight edge: a corner in its middle would be left out
    if steps(right_low, right_high) > 1 || steps(left_high, left_low) > 1 {
        return Err(shape());
    }
    let walk = |from: usize, to: usize| -> Vec<Flat> {
        (0..=steps(from, to))
            .map(|k| flat[(from + k) % count])
            .collect()
    };
    let lower = walk(left_low, right_low);
    let mut upper = walk(right_high, left_high);
    upper.reverse();
    Ok((lower, upper))
}

/// The triangles of a face on a cylinder or a cone given by its bounds, counter-clockwise seen
/// from away from the axis. No triangle spans more than `widest` radians about the axis.
pub(crate) fn band(bounds: &[Vec<Corner>], widest: f64) -> Result<Vec<[u32; 3]>, String> {
    if let [one] = bounds {
        if let Some(at) = one.iter().position(|c| c.angle.is_none()) {
            // the point of a cone: a fan from it to the rest of the bound
            let point = one[at];
            let rest: Vec<Corner> = (1..one.len()).map(|k| one[(at + k) % one.len()]).collect();
            let mut chain = unwrap(&rest)?;
            if chain
                .first()
                .zip(chain.last())
                .is_some_and(|(a, b)| b.u < a.u)
            {
                chain.reverse();
            }
            let (Some(first), Some(last)) = (chain.first().copied(), chain.last().copied()) else {
                return Err(shape());
            };
            let mean = chain.iter().map(|p| p.v).sum::<f64>() / chain.len() as f64;
            let tip = |u: f64| Flat {
                id: point.id,
                u,
                v: point.height,
            };
            let tips = [tip(first.u), tip(last.u)];
            // the point is one corner: it stands above or below every part of the chain
            let mut out = Vec::new();
            for w in chain.windows(2) {
                let (a, b) = (w[0], w[1]);
                if b.u < a.u - SAME || b.u - a.u > widest {
                    return Err(shape());
                }
                if a.id == b.id || a.id == point.id || b.id == point.id {
                    continue;
                }
                out.push(if tips[0].v > mean {
                    [a.id, b.id, point.id]
                } else {
                    [b.id, a.id, point.id]
                });
            }
            if out.is_empty() {
                return Err(shape());
            }
            return Ok(out);
        }
        let flat = unwrap(one)?;
        if turns(&flat) != 0 {
            return Err(shape());
        }
        let (lower, upper) = sides(flat)?;
        return zip(&lower, &upper, widest);
    }
    let [one, two] = bounds else {
        return Err(shape());
    };
    let (mut lower, mut upper) = (ring(unwrap(one)?)?, ring(unwrap(two)?)?);
    let mean = |c: &[Flat]| c.iter().map(|p| p.v).sum::<f64>() / c.len() as f64;
    if mean(&lower) > mean(&upper) {
        std::mem::swap(&mut lower, &mut upper);
    }
    // the upper ring starts at its corner nearest to where the lower one starts
    let start = lower[0].u;
    let count = upper.len() - 1;
    let from = (0..count)
        .min_by(|&i, &j| {
            turn(start, upper[i].u)
                .abs()
                .total_cmp(&turn(start, upper[j].u).abs())
        })
        .ok_or_else(shape)?;
    let mut turned: Vec<Flat> = Vec::with_capacity(count + 1);
    let mut u = start + turn(start, upper[from].u);
    for k in 0..=count {
        let at = (from + k) % count;
        if k > 0 {
            let before = (from + k - 1) % count;
            u += turn(upper[before].u, upper[at].u);
        }
        turned.push(Flat { u, ..upper[at] });
    }
    zip(&lower, &turned, widest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame::new([0.0, 0.0, 0.0], None, None).unwrap()
    }

    #[test]
    fn an_arc_goes_the_way_it_says_and_ends_on_the_corners_given() {
        let f = frame();
        let r = 10.0e6;
        let (a, b) = ([r, 0.0, 0.0], [0.0, r, 0.0]);
        let short = arc_points(&f, r, a, b, true, 8).unwrap();
        assert_eq!(short.len(), 3);
        assert_eq!((short[0], short[2]), (a, b));
        assert!(short[1][0] > 0.0 && short[1][1] > 0.0);
        // the other way round is three quarters of the circle
        let long = arc_points(&f, r, a, b, false, 8).unwrap();
        assert_eq!(long.len(), 7);
        assert!(long[1][1] < 0.0);
        let whole = arc_points(&f, r, a, a, true, 8).unwrap();
        assert_eq!(whole.len(), 9);
        assert!(arc_points(&f, r, [r + 1.0e6, 0.0, 0.0], b, true, 8)
            .unwrap_err()
            .contains("not on its circle"));
        assert!(arc_points(&f, 0.0, a, b, true, 8).is_err());
    }

    #[test]
    fn a_frame_needs_two_directions_that_differ() {
        assert!(Frame::new([0.0; 3], Some([0.0, 0.0, 1.0]), Some([0.0, 0.0, 2.0])).is_none());
        let f = Frame::new([0.0; 3], Some([1.0, 0.0, 0.0]), None).unwrap();
        let (angle, radial, height) = f.coordinates([5.0, 3.0, 4.0]);
        assert!((radial - 5.0).abs() < 1e-12 && (height - 5.0).abs() < 1e-12);
        assert!(angle.is_finite());
    }

    fn at(id: u32, degrees: f64, height: f64) -> Corner {
        Corner {
            id,
            angle: Some(degrees.to_radians()),
            height,
        }
    }

    #[test]
    fn the_wall_between_two_rings_is_closed_whatever_way_they_are_listed() {
        let low: Vec<Corner> = (0..8).map(|k| at(k, 45.0 * f64::from(k), 0.0)).collect();
        let mut high: Vec<Corner> = (0..8)
            .map(|k| at(8 + k, 45.0 * f64::from(k) + 90.0, 5.0))
            .collect();
        high.reverse();
        let widest = 100f64.to_radians();
        for bounds in [[low.clone(), high.clone()], [high, low]] {
            let triangles = band(&bounds, widest).unwrap();
            assert_eq!(triangles.len(), 16);
            // every side between the two rings is used by two triangles
            let mut sides = std::collections::BTreeMap::new();
            for t in &triangles {
                for k in 0..3 {
                    let (a, b) = (t[k], t[(k + 1) % 3]);
                    *sides.entry((a.min(b), a.max(b))).or_insert(0) += 1;
                }
            }
            assert_eq!(sides.values().filter(|&&n| n == 1).count(), 16);
            assert!(sides.values().all(|&n| n <= 2));
        }
    }

    #[test]
    fn a_shape_that_is_not_a_band_is_refused() {
        let widest = 100f64.to_radians();
        // a single ring bounds no wall
        let ring: Vec<Corner> = (0..8).map(|k| at(k, 45.0 * f64::from(k), 0.0)).collect();
        assert!(band(std::slice::from_ref(&ring), widest).is_err());
        assert!(band(&[], widest).is_err());
        // a part of a wall with a corner in the middle of a straight side
        let notched = vec![
            at(0, 0.0, 0.0),
            at(1, 45.0, 0.0),
            at(2, 45.0, 2.0),
            at(3, 45.0, 5.0),
            at(4, 0.0, 5.0),
        ];
        assert!(band(&[notched], widest).unwrap_err().contains("not read"));
        // pieces too wide to stand for a curve
        let wide = vec![
            at(0, 0.0, 0.0),
            at(1, 170.0, 0.0),
            at(2, 170.0, 5.0),
            at(3, 0.0, 5.0),
        ];
        assert!(band(&[wide], widest).is_err());
    }
}
