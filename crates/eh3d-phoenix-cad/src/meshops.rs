// SPDX-License-Identifier: GPL-3.0-or-later
//! Operations on surfaces of triangles: moving, scaling, turning and mirroring a solid, linear and
//! circular patterns of copies, the cross-section a plane makes and the cut of a solid by a plane
//! or by a box, with the cut closed by a cap so that the part that is left is a solid again.
//!
//! All places are in nanometres, as in [`crate::mesh::Mesh`]. A mirror, or a scale with an odd
//! number of negative factors, turns the triangles the other way round so that the solid stays
//! outside in. A cut follows one rule for the points that lie exactly on the plane: they count as
//! being above it, so two triangles that share such a point always agree and the cut is closed.
//! The sines and cosines are the ones of the circles of the program (sums and products only), so
//! every computer gives the same bits.

use std::collections::HashMap;

use crate::mesh::{sin_cos, Mesh, P3};
use crate::triangulate::{triangulate, Pt};

/// The nanometres of a millimetre.
const NM_PER_MM: f64 = 1.0e6;

/// The farthest a place may be from the origin, in nanometres (a thousand kilometres).
const FAR_NM: f64 = 1.0e15;

/// The most copies of a pattern.
pub const MAX_COPIES: u32 = 100_000;

/// An axis of the coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// The axis x.
    X,
    /// The axis y.
    Y,
    /// The axis z.
    Z,
}

impl Axis {
    /// The axis named `x`, `y` or `z` (in either case).
    pub fn parse(word: &str) -> Option<Axis> {
        match word.to_ascii_lowercase().as_str() {
            "x" => Some(Axis::X),
            "y" => Some(Axis::Y),
            "z" => Some(Axis::Z),
            _ => None,
        }
    }

    /// The letter of the axis.
    pub fn letter(self) -> char {
        match self {
            Axis::X => 'x',
            Axis::Y => 'y',
            Axis::Z => 'z',
        }
    }

    fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }

    /// The two axes of the plane across this one, in an order that makes (first, second, this) a
    /// right-handed set.
    fn across(self) -> (usize, usize) {
        match self {
            Axis::X => (1, 2),
            Axis::Y => (2, 0),
            Axis::Z => (0, 1),
        }
    }
}

fn check_far(mesh: &Mesh) -> Result<(), String> {
    if mesh
        .vertices
        .iter()
        .any(|v| v.iter().any(|c| !c.is_finite() || c.abs() > FAR_NM))
    {
        return Err(
            "the result goes farther than a thousand kilometres from the origin".to_owned(),
        );
    }
    Ok(())
}

fn flipped_triangles(mesh: &mut Mesh) {
    for t in &mut mesh.triangles {
        t.swap(1, 2);
    }
}

/// The solid moved by `by`.
pub fn translate(mesh: &Mesh, by: P3) -> Result<Mesh, String> {
    let out = mesh.mapped(|v| [v[0] + by[0], v[1] + by[1], v[2] + by[2]]);
    check_far(&out)?;
    Ok(out)
}

/// The solid scaled by a factor along each axis about a point. A factor of zero is refused; with
/// an odd number of negative factors the triangles are turned so that the solid stays outside in.
pub fn scale(mesh: &Mesh, factors: [f64; 3], about: P3) -> Result<Mesh, String> {
    if factors
        .iter()
        .any(|f| !f.is_finite() || *f == 0.0 || f.abs() > 1.0e6)
    {
        return Err(
            "a scale factor must be a number that is not zero and at most a million".to_owned(),
        );
    }
    let mut out = mesh.mapped(|v| {
        [
            about[0] + (v[0] - about[0]) * factors[0],
            about[1] + (v[1] - about[1]) * factors[1],
            about[2] + (v[2] - about[2]) * factors[2],
        ]
    });
    if factors.iter().filter(|f| **f < 0.0).count() % 2 == 1 {
        flipped_triangles(&mut out);
    }
    check_far(&out)?;
    Ok(out)
}

/// The solid turned by an angle in degrees about the line through `about` along `axis` (a positive
/// angle goes from the first axis across to the second, counter-clockwise seen from the end of the
/// axis); the places are rounded to the nanometre.
pub fn rotate(mesh: &Mesh, axis: Axis, degrees: f64, about: P3) -> Result<Mesh, String> {
    if !degrees.is_finite() {
        return Err("the angle must be a number".to_owned());
    }
    let (s, c) = sin_cos(degrees.rem_euclid(360.0) * std::f64::consts::PI / 180.0);
    let (u, v) = axis.across();
    let out = mesh.mapped(|p| {
        let (du, dv) = (p[u] - about[u], p[v] - about[v]);
        let mut q = p;
        q[u] = (about[u] + du * c - dv * s).round();
        q[v] = (about[v] + du * s + dv * c).round();
        q
    });
    check_far(&out)?;
    Ok(out)
}

/// The solid mirrored in the plane across `axis` at the place `at`, with the triangles turned so
/// that it stays outside in.
pub fn mirror(mesh: &Mesh, axis: Axis, at: f64) -> Result<Mesh, String> {
    let k = axis.index();
    let mut out = mesh.mapped(|mut p| {
        p[k] = 2.0 * at - p[k];
        p
    });
    flipped_triangles(&mut out);
    check_far(&out)?;
    Ok(out)
}

fn append(into: &mut Mesh, copy: &Mesh) {
    let base = into.vertices.len() as u32;
    into.vertices.extend_from_slice(&copy.vertices);
    into.triangles.extend(
        copy.triangles
            .iter()
            .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
    );
}

fn check_copies(mesh: &Mesh, count: u32, max_triangles: usize) -> Result<(), String> {
    if !(1..=MAX_COPIES).contains(&count) {
        return Err(format!(
            "the number of copies must be from 1 to {MAX_COPIES}"
        ));
    }
    if mesh.triangles.len().saturating_mul(count as usize) > max_triangles {
        return Err(format!(
            "{count} copies would hold more than {max_triangles} triangles"
        ));
    }
    Ok(())
}

/// `count` copies of the solid (the first is the solid itself), each moved by `step` more than the
/// one before, in one surface.
pub fn linear_pattern(
    mesh: &Mesh,
    count: u32,
    step: P3,
    max_triangles: usize,
) -> Result<Mesh, String> {
    check_copies(mesh, count, max_triangles)?;
    let mut out = Mesh::default();
    for i in 0..count {
        let n = f64::from(i);
        append(
            &mut out,
            &translate(mesh, [step[0] * n, step[1] * n, step[2] * n])?,
        );
    }
    Ok(out)
}

/// `count` copies of the solid turned about the line through `center` along `axis`, spread over
/// `total_degrees`: a full turn (360) puts them evenly round the circle, any other angle puts the
/// first at 0 and the last at `total_degrees`. The first copy is the solid itself.
pub fn circular_pattern(
    mesh: &Mesh,
    count: u32,
    axis: Axis,
    center: P3,
    total_degrees: f64,
    max_triangles: usize,
) -> Result<Mesh, String> {
    check_copies(mesh, count, max_triangles)?;
    if !total_degrees.is_finite() || total_degrees == 0.0 || total_degrees.abs() > 360.0 {
        return Err(
            "the angle of a circular pattern is from -360 to 360 degrees, not zero".to_owned(),
        );
    }
    let full = total_degrees.abs() == 360.0;
    let divisor = if full || count == 1 { count } else { count - 1 };
    let mut out = Mesh::default();
    for i in 0..count {
        let angle = total_degrees * f64::from(i) / f64::from(divisor);
        if i == 0 {
            append(&mut out, mesh);
        } else {
            append(&mut out, &rotate(mesh, axis, angle, center)?);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// The cross-section
// ---------------------------------------------------------------------------------------------

/// The cross-section a plane makes in a solid.
#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    /// The closed outlines in millimetres, in the two axes across the cut (see [`Axis`]); an outer
    /// outline goes counter-clockwise and a hole clockwise.
    pub loops: Vec<Vec<[f64; 2]>>,
    /// The area of the section in square millimetres (the holes taken out).
    pub area_mm2: f64,
    /// The length of all the outlines in millimetres.
    pub perimeter_mm: f64,
    /// How many outer outlines.
    pub outer: usize,
    /// How many holes.
    pub holes: usize,
    /// How many segments did not close into an outline (a surface that is not closed).
    pub open_segments: usize,
}

fn bits(p: P3) -> [u64; 3] {
    [
        (p[0] + 0.0).to_bits(),
        (p[1] + 0.0).to_bits(),
        (p[2] + 0.0).to_bits(),
    ]
}

/// Where the edge between two vertices crosses the plane; the same bits for the two triangles that
/// share the edge.
fn crossing(mesh: &Mesh, a: u32, b: u32, k: usize, at: f64) -> P3 {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    let (p, q) = (mesh.vertices[lo as usize], mesh.vertices[hi as usize]);
    // a vertex on the plane is the crossing itself, to the bit
    if p[k] == at {
        return p;
    }
    if q[k] == at {
        return q;
    }
    let t = (at - p[k]) / (q[k] - p[k]);
    let mut out = [
        p[0] + t * (q[0] - p[0]),
        p[1] + t * (q[1] - p[1]),
        p[2] + t * (q[2] - p[2]),
    ];
    out[k] = at;
    out
}

/// The directed segments the plane makes in the triangles, each going from the point where the
/// triangle's boundary falls through the plane to the one where it rises (so that the inside of an
/// outside-in solid is on the left seen from the end of the axis).
fn segments(mesh: &Mesh, k: usize, at: f64) -> Vec<(P3, P3)> {
    let mut out = Vec::new();
    for t in &mesh.triangles {
        let above = t.map(|i| mesh.vertices[i as usize][k] >= at);
        if above.iter().all(|a| *a) || above.iter().all(|a| !*a) {
            continue;
        }
        let (mut falling, mut rising) = (None, None);
        for j in 0..3 {
            let (a, b) = (j, (j + 1) % 3);
            if above[a] != above[b] {
                let p = crossing(mesh, t[a], t[b], k, at);
                if above[b] {
                    rising = Some(p);
                } else {
                    falling = Some(p);
                }
            }
        }
        if let (Some(f), Some(r)) = (falling, rising) {
            if bits(f) != bits(r) {
                out.push((f, r));
            }
        }
    }
    out
}

/// Chains directed segments into closed outlines; the segments that do not close are counted.
fn chain(segments: &[(P3, P3)]) -> (Vec<Vec<P3>>, usize) {
    let mut starts: HashMap<[u64; 3], Vec<usize>> = HashMap::new();
    for (i, (a, _)) in segments.iter().enumerate() {
        starts.entry(bits(*a)).or_default().push(i);
    }
    let mut used = vec![false; segments.len()];
    let mut loops = Vec::new();
    let mut open = 0;
    for first in 0..segments.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let origin = bits(segments[first].0);
        let mut points = vec![segments[first].0];
        let mut taken = vec![first];
        let mut end = segments[first].1;
        let closed = loop {
            if bits(end) == origin {
                break true;
            }
            points.push(end);
            let next = starts
                .get(&bits(end))
                .and_then(|list| list.iter().copied().find(|i| !used[*i]));
            match next {
                Some(i) => {
                    used[i] = true;
                    taken.push(i);
                    end = segments[i].1;
                }
                None => break false,
            }
        };
        if closed && points.len() >= 3 {
            loops.push(points);
        } else {
            open += taken.len();
        }
    }
    (loops, open)
}

fn area2d(points: &[[f64; 2]]) -> f64 {
    let mut sum = 0.0;
    for (i, p) in points.iter().enumerate() {
        let q = points[(i + 1) % points.len()];
        sum += p[0] * q[1] - q[0] * p[1];
    }
    sum / 2.0
}

/// The cross-section of the solid at the place `at_nm` across `axis`.
pub fn slice(mesh: &Mesh, axis: Axis, at_nm: f64) -> Slice {
    let k = axis.index();
    let (u, v) = axis.across();
    let (loops3, open_segments) = chain(&segments(mesh, k, at_nm));
    let mut loops = Vec::new();
    let (mut area, mut perimeter, mut outer, mut holes) = (0.0, 0.0, 0, 0);
    for points in loops3 {
        let flat: Vec<[f64; 2]> = points
            .iter()
            .map(|p| [p[u] / NM_PER_MM, p[v] / NM_PER_MM])
            .collect();
        let a = area2d(&flat);
        if a >= 0.0 {
            outer += 1;
        } else {
            holes += 1;
        }
        area += a;
        for (i, p) in flat.iter().enumerate() {
            let q = flat[(i + 1) % flat.len()];
            perimeter += ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt();
        }
        loops.push(flat);
    }
    Slice {
        loops,
        area_mm2: area,
        perimeter_mm: perimeter,
        outer,
        holes,
        open_segments,
    }
}

/// The section as an SVG drawing (millimetres, the first axis to the right and the second up),
/// the holes cut out with the even-odd rule.
pub fn slice_svg(section: &Slice) -> String {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in section.loops.iter().flatten() {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    if section.loops.is_empty() {
        lo = [0.0; 2];
        hi = [1.0; 2];
    }
    let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
    let mut d = String::new();
    for l in &section.loops {
        for (i, p) in l.iter().enumerate() {
            d.push_str(&format!(
                "{}{:.6} {:.6} ",
                if i == 0 { "M" } else { "L" },
                p[0] - lo[0],
                hi[1] - p[1]
            ));
        }
        d.push_str("Z ");
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.3}mm\" height=\"{h:.3}mm\" viewBox=\"0 0 {w:.6} {h:.6}\">\n<path d=\"{}\" fill=\"#cccccc\" fill-rule=\"evenodd\" stroke=\"#000000\" stroke-width=\"{:.4}\"/>\n</svg>\n",
        d.trim_end(),
        (w.max(h) / 400.0).max(1e-6)
    )
}

// ---------------------------------------------------------------------------------------------
// The cut by a plane
// ---------------------------------------------------------------------------------------------

/// A solid cut by a plane.
#[derive(Debug, Clone, PartialEq)]
pub struct Clipped {
    /// The part that is left.
    pub mesh: Mesh,
    /// How many triangles close the cut.
    pub cap_triangles: usize,
    /// Whether the cut could be closed (it cannot when the outlines of the cut touch or cross, or
    /// when the surface was not closed to begin with).
    pub capped: bool,
}

/// The part of the solid on one side of the plane across `axis` at `at_nm` (`keep_above` keeps the
/// side towards the end of the axis), with the cut closed by a cap.
pub fn clip_plane(
    mesh: &Mesh,
    axis: Axis,
    at_nm: f64,
    keep_above: bool,
) -> Result<Clipped, String> {
    let k = axis.index();
    let (u, v) = axis.across();
    let mut out = Mesh::default();
    let mut moved: HashMap<u32, u32> = HashMap::new();
    let mut on_plane: HashMap<[u64; 3], u32> = HashMap::new();
    let mut cut: Vec<(u32, u32)> = Vec::new();
    let plane_vertex = |out: &mut Mesh, on_plane: &mut HashMap<[u64; 3], u32>, p: P3| -> u32 {
        *on_plane.entry(bits(p)).or_insert_with(|| {
            out.vertices.push(p);
            (out.vertices.len() - 1) as u32
        })
    };
    for t in &mesh.triangles {
        let kept = t.map(|i| (mesh.vertices[i as usize][k] >= at_nm) == keep_above);
        if kept.iter().all(|x| !*x) {
            continue;
        }
        let mut polygon: Vec<u32> = Vec::with_capacity(4);
        let mut exit: Option<u32> = None;
        // the walk starts at a kept vertex so that the cut edge is a pair of consecutive points
        let start = (0..3)
            .find(|j| kept[*j] && !kept[(*j + 2) % 3])
            .unwrap_or(0);
        for step in 0..3 {
            let j = (start + step) % 3;
            let n = (j + 1) % 3;
            if kept[j] {
                let id = t[j];
                let p = mesh.vertices[id as usize];
                let mapped = if p[k] == at_nm {
                    plane_vertex(&mut out, &mut on_plane, p)
                } else {
                    *moved.entry(id).or_insert_with(|| {
                        out.vertices.push(p);
                        (out.vertices.len() - 1) as u32
                    })
                };
                polygon.push(mapped);
            }
            if kept[j] != kept[n] {
                let point = crossing(mesh, t[j], t[n], k, at_nm);
                let id = plane_vertex(&mut out, &mut on_plane, point);
                polygon.push(id);
                if kept[j] {
                    exit = Some(id);
                } else if let Some(from) = exit.take() {
                    if from != id {
                        cut.push((from, id));
                    }
                }
            }
        }
        polygon.dedup();
        while polygon.len() > 1 && polygon.first() == polygon.last() {
            polygon.pop();
        }
        for w in 1..polygon.len().saturating_sub(1) {
            let tri = [polygon[0], polygon[w], polygon[w + 1]];
            if tri[0] != tri[1] && tri[1] != tri[2] && tri[0] != tri[2] {
                out.triangles.push(tri);
            }
        }
    }
    // the cut edges that were left open by the order of the walk: the walk starts at a kept vertex
    // whose predecessor is not kept, so an exit is always followed by its entry
    let mut edges: HashMap<u32, Vec<u32>> = HashMap::new();
    for (a, b) in &cut {
        // the cap goes the other way along the cut
        edges.entry(*b).or_default().push(*a);
    }
    let mut loops: Vec<Vec<u32>> = Vec::new();
    let mut open = false;
    let mut starts: Vec<u32> = edges.keys().copied().collect();
    starts.sort_unstable();
    for s in starts {
        while let Some(first) = edges.get_mut(&s).and_then(Vec::pop) {
            let mut ring = vec![s];
            let mut at = first;
            while at != s {
                ring.push(at);
                match edges.get_mut(&at).and_then(Vec::pop) {
                    Some(next) => at = next,
                    None => {
                        open = true;
                        break;
                    }
                }
                if ring.len() > cut.len() + 1 {
                    open = true;
                    break;
                }
            }
            if at == s && ring.len() >= 3 {
                loops.push(ring);
            } else {
                open = true;
            }
        }
    }
    let sign = if keep_above { -1.0 } else { 1.0 };
    let project = |id: u32| -> Pt {
        let p = out.vertices[id as usize];
        (id, p[u] / NM_PER_MM, p[v] / NM_PER_MM)
    };
    let mut outers: Vec<Vec<Pt>> = Vec::new();
    let mut holes: Vec<Vec<Pt>> = Vec::new();
    for ring in &loops {
        let pts: Vec<Pt> = ring.iter().map(|id| project(*id)).collect();
        if crate::triangulate::area2(&pts) * sign >= 0.0 {
            outers.push(pts);
        } else {
            holes.push(pts);
        }
    }
    let before = out.triangles.len();
    let mut capped = !open;
    for outline in &outers {
        // a hole belongs to the outline that contains its first point
        let mine: Vec<Vec<Pt>> = holes
            .iter()
            .filter(|h| inside(outline, (h[0].1, h[0].2)))
            .cloned()
            .collect();
        match triangulate(outline, &mine) {
            Some(tris) => {
                for t in tris {
                    // the triangles come out counter-clockwise across the cut; the cap of the part
                    // above the plane faces down
                    out.triangles
                        .push(if keep_above { [t[0], t[2], t[1]] } else { t });
                }
            }
            None => capped = false,
        }
    }
    if outers.is_empty() && !holes.is_empty() {
        capped = false;
    }
    let cap_triangles = out.triangles.len() - before;
    check_far(&out)?;
    Ok(Clipped {
        mesh: out,
        cap_triangles,
        capped,
    })
}

/// Whether a point is inside an outline (even-odd rule).
fn inside(outline: &[Pt], p: (f64, f64)) -> bool {
    let mut yes = false;
    for i in 0..outline.len() {
        let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
        if (a.2 > p.1) != (b.2 > p.1) && p.0 < (b.1 - a.1) * (p.1 - a.2) / (b.2 - a.2) + a.1 {
            yes = !yes;
        }
    }
    yes
}

/// The part of the solid inside a box (the intersection of the solid with the box), made by six
/// cuts, each closed by a cap.
pub fn clip_box(mesh: &Mesh, low: P3, high: P3) -> Result<Clipped, String> {
    if (0..3).any(|k| low[k].partial_cmp(&high[k]) != Some(std::cmp::Ordering::Less)) {
        return Err("the box must have a size along every axis".to_owned());
    }
    let mut current = mesh.clone();
    let (mut caps, mut capped) = (0, true);
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        let k = axis.index();
        for (at, above) in [(low[k], true), (high[k], false)] {
            let step = clip_plane(&current, axis, at, above)?;
            caps += step.cap_triangles;
            capped &= step.capped;
            current = step.mesh;
        }
    }
    Ok(Clipped {
        mesh: current,
        cap_triangles: caps,
        capped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box of the given size with its low corner at `origin`, millimetres, wound outside in.
    fn cuboid(origin: [f64; 3], size: [f64; 3]) -> Mesh {
        let m = NM_PER_MM;
        let mut vertices = Vec::new();
        for z in [0.0, size[2]] {
            for y in [0.0, size[1]] {
                for x in [0.0, size[0]] {
                    vertices.push([
                        (origin[0] + x) * m,
                        (origin[1] + y) * m,
                        (origin[2] + z) * m,
                    ]);
                }
            }
        }
        let triangles = vec![
            [0, 2, 1],
            [1, 2, 3],
            [4, 5, 6],
            [5, 7, 6],
            [0, 1, 4],
            [1, 5, 4],
            [2, 6, 3],
            [3, 6, 7],
            [0, 4, 2],
            [2, 4, 6],
            [1, 3, 5],
            [3, 7, 5],
        ];
        Mesh {
            vertices,
            triangles,
        }
    }

    fn mm3(mesh: &Mesh) -> f64 {
        mesh.volume_nm3() / 1.0e18
    }

    #[test]
    fn a_move_a_scale_and_a_turn_change_the_places_and_not_the_shape() {
        let c = cuboid([0.0; 3], [10.0, 20.0, 30.0]);
        let moved = translate(&c, [5.0e6, 0.0, -1.0e6]).unwrap();
        assert_eq!(moved.vertices[0], [5.0e6, 0.0, -1.0e6]);
        assert_eq!(moved.triangles, c.triangles);
        let big = scale(&c, [2.0, 2.0, 2.0], [0.0; 3]).unwrap();
        assert!((mm3(&big) - 8.0 * 6000.0).abs() < 1e-6);
        let flat = scale(&c, [1.0, 1.0, -1.0], [0.0; 3]).unwrap();
        assert!(
            mm3(&flat) > 0.0,
            "a mirroring scale keeps the solid outside in"
        );
        assert!(scale(&c, [0.0, 1.0, 1.0], [0.0; 3]).is_err());
        assert!(scale(&c, [1.0, f64::NAN, 1.0], [0.0; 3]).is_err());
        let turned = rotate(&c, Axis::Z, 90.0, [0.0; 3]).unwrap();
        // (10, 0, 0) goes to (0, 10, 0) and (0, 20, 0) to (-20, 0, 0)
        assert!(turned.vertices.contains(&[0.0, 10.0e6, 0.0]));
        assert!(turned.vertices.contains(&[-20.0e6, 0.0, 0.0]));
        assert!((mm3(&turned) - 6000.0).abs() < 1e-6);
        assert!(rotate(&c, Axis::Z, f64::NAN, [0.0; 3]).is_err());
        let far = translate(&c, [2.0e15, 0.0, 0.0]);
        assert!(far.unwrap_err().contains("thousand kilometres"));
    }

    #[test]
    fn a_mirror_keeps_the_volume_positive_and_the_surface_closed() {
        let c = cuboid([5.0, 0.0, 0.0], [10.0, 20.0, 30.0]);
        let m = mirror(&c, Axis::X, 0.0).unwrap();
        assert!(m.vertices.contains(&[-5.0e6, 0.0, 0.0]));
        assert!(m.vertices.contains(&[-15.0e6, 20.0e6, 30.0e6]));
        assert!(mm3(&m) > 0.0);
        assert!((mm3(&m) - 6000.0).abs() < 1e-6);
        assert!(m.check_closed().is_ok());
        // mirroring twice gives back the places
        let again = mirror(&m, Axis::X, 0.0).unwrap();
        assert_eq!(again.vertices, c.vertices);
    }

    #[test]
    fn a_linear_pattern_puts_copies_at_equal_steps_and_a_circular_one_round_the_axis() {
        let c = cuboid([0.0; 3], [1.0, 1.0, 1.0]);
        let line = linear_pattern(&c, 4, [3.0e6, 0.0, 0.0], 1000).unwrap();
        assert_eq!(line.triangles.len(), 48);
        assert!(line.vertices.contains(&[9.0e6, 0.0, 0.0]));
        assert!((mm3(&line) - 4.0).abs() < 1e-9);
        assert!(line.check_closed().is_ok());
        let ring = circular_pattern(&c, 4, Axis::Z, [0.0; 3], 360.0, 1000).unwrap();
        assert_eq!(ring.triangles.len(), 48);
        // the copy turned a quarter turn has its corner (1, 0, 0) at (0, 1, 0)
        assert!(ring.vertices.contains(&[0.0, 1.0e6, 0.0]));
        assert!(ring.vertices.contains(&[-1.0e6, 0.0, 0.0]));
        // an arc puts the last copy at the end angle
        let arc = circular_pattern(&c, 3, Axis::Z, [0.0; 3], 90.0, 1000).unwrap();
        assert!(arc.vertices.contains(&[0.0, 1.0e6, 0.0]));
        assert!((mm3(&arc) - 3.0).abs() < 1e-3);
    }

    #[test]
    fn the_limits_of_a_pattern_are_told() {
        let c = cuboid([0.0; 3], [1.0, 1.0, 1.0]);
        assert!(linear_pattern(&c, 0, [1.0; 3], 100).is_err());
        assert!(linear_pattern(&c, MAX_COPIES + 1, [1.0; 3], usize::MAX)
            .unwrap_err()
            .contains("from 1 to"));
        assert!(linear_pattern(&c, 10, [1.0; 3], 100)
            .unwrap_err()
            .contains("more than 100"));
        assert!(circular_pattern(&c, 3, Axis::X, [0.0; 3], 0.0, 100).is_err());
        assert!(circular_pattern(&c, 3, Axis::X, [0.0; 3], 400.0, 100).is_err());
        assert!(Axis::parse("Y") == Some(Axis::Y) && Axis::parse("w").is_none());
    }

    #[test]
    fn the_section_of_a_box_is_a_rectangle_and_that_of_a_box_with_a_hole_has_a_hole() {
        let c = cuboid([0.0; 3], [10.0, 20.0, 30.0]);
        let s = slice(&c, Axis::Z, 15.0e6);
        assert_eq!((s.outer, s.holes, s.open_segments), (1, 0, 0));
        assert!((s.area_mm2 - 200.0).abs() < 1e-9);
        assert!((s.perimeter_mm - 60.0).abs() < 1e-9);
        let s = slice(&c, Axis::X, 3.0e6);
        assert!((s.area_mm2 - 600.0).abs() < 1e-9);
        // outside the solid there is nothing
        assert!(slice(&c, Axis::Z, 31.0e6).loops.is_empty());
        // the points on the plane count as above: a plane through the bottom face meets nothing,
        // and one through the top face gives the outline of that face
        assert!(slice(&c, Axis::Z, 0.0).loops.is_empty());
        let on_top = slice(&c, Axis::Z, 30.0e6);
        assert!((on_top.area_mm2 - 200.0).abs() < 1e-9);
        // a tube: a box with a box taken out, built as two closed shells (the inner turned inside out)
        let outer = cuboid([0.0; 3], [10.0, 10.0, 10.0]);
        let mut inner = cuboid([3.0, 3.0, -1.0], [4.0, 4.0, 12.0]);
        for t in &mut inner.triangles {
            t.swap(1, 2);
        }
        let mut both = outer.clone();
        append(&mut both, &inner);
        let s = slice(&both, Axis::Z, 5.0e6);
        assert_eq!((s.outer, s.holes), (1, 1));
        assert!((s.area_mm2 - (100.0 - 16.0)).abs() < 1e-9);
    }

    #[test]
    fn a_section_that_does_not_close_counts_its_open_segments() {
        let mut c = cuboid([0.0; 3], [10.0, 10.0, 10.0]);
        // take the two triangles of a side wall away: the section cannot close
        c.triangles.retain(|t| *t != [0, 1, 4] && *t != [1, 5, 4]);
        let s = slice(&c, Axis::Z, 5.0e6);
        assert!(s.open_segments > 0);
        assert_eq!(s.outer + s.holes, 0);
    }

    #[test]
    fn the_section_is_told_as_an_svg_with_the_holes_cut_out() {
        let outer = cuboid([0.0; 3], [10.0, 10.0, 10.0]);
        let mut inner = cuboid([3.0, 3.0, -1.0], [4.0, 4.0, 12.0]);
        for t in &mut inner.triangles {
            t.swap(1, 2);
        }
        let mut both = outer;
        append(&mut both, &inner);
        let svg = slice_svg(&slice(&both, Axis::Z, 5.0e6));
        assert!(svg.starts_with("<svg xmlns="));
        assert!(svg.contains("fill-rule=\"evenodd\""));
        assert_eq!(svg.matches('Z').count(), 2);
        assert!(svg.contains("width=\"10.000mm\""));
        assert!(slice_svg(&slice(&both, Axis::Z, 50.0e6)).contains("<path d=\"\""));
    }

    #[test]
    fn a_box_cut_in_half_keeps_a_closed_solid_of_half_the_volume_on_each_side() {
        let c = cuboid([0.0; 3], [10.0, 20.0, 30.0]);
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let size = match axis {
                Axis::X => 10.0,
                Axis::Y => 20.0,
                Axis::Z => 30.0,
            };
            let at = 0.4 * size * NM_PER_MM;
            let above = clip_plane(&c, axis, at, true).unwrap();
            let below = clip_plane(&c, axis, at, false).unwrap();
            assert!(above.capped && below.capped);
            assert!(above.mesh.check_closed().is_ok(), "{axis:?} above");
            assert!(below.mesh.check_closed().is_ok(), "{axis:?} below");
            assert!(
                (mm3(&above.mesh) - 6000.0 * 0.6).abs() < 1e-6,
                "{}",
                mm3(&above.mesh)
            );
            assert!((mm3(&below.mesh) - 6000.0 * 0.4).abs() < 1e-6);
            assert!(above.cap_triangles >= 2 && below.cap_triangles >= 2);
        }
    }

    #[test]
    fn a_cut_through_a_vertex_and_a_cut_that_misses_the_solid_are_handled() {
        let c = cuboid([0.0; 3], [10.0, 10.0, 10.0]);
        // exactly on the top face: the face counts as above, so the part below is the whole box
        let below = clip_plane(&c, Axis::Z, 10.0e6, false).unwrap();
        assert!((mm3(&below.mesh) - 1000.0).abs() < 1e-9);
        assert!(below.mesh.check_closed().is_ok());
        // exactly on the bottom face: everything is above and nothing is below
        let above = clip_plane(&c, Axis::Z, 0.0, true).unwrap();
        assert!((mm3(&above.mesh) - 1000.0).abs() < 1e-9);
        let none = clip_plane(&c, Axis::Z, 0.0, false).unwrap();
        assert!(none.mesh.triangles.is_empty());
        let none = clip_plane(&c, Axis::Z, 99.0e6, true).unwrap();
        assert!(none.mesh.triangles.is_empty());
    }

    #[test]
    fn a_solid_with_a_hole_is_cut_and_closed_with_a_hole_in_the_cap() {
        let outer = cuboid([0.0; 3], [10.0, 10.0, 10.0]);
        let mut inner = cuboid([3.0, 3.0, 2.0], [4.0, 4.0, 4.0]);
        for t in &mut inner.triangles {
            t.swap(1, 2);
        }
        let mut both = outer;
        append(&mut both, &inner);
        let top = clip_plane(&both, Axis::Z, 4.0e6, true).unwrap();
        assert!(top.capped, "{top:?}");
        // 10x10x6 less the cavity from z = 4 to 6 (4x4x2)
        assert!(
            (mm3(&top.mesh) - (600.0 - 32.0)).abs() < 1e-6,
            "{}",
            mm3(&top.mesh)
        );
        assert!(top.mesh.check_closed().is_ok());
    }

    #[test]
    fn the_part_inside_a_box_is_the_intersection() {
        let c = cuboid([0.0; 3], [10.0, 10.0, 10.0]);
        let part = clip_box(&c, [2.0e6, 3.0e6, 4.0e6], [12.0e6, 8.0e6, 7.0e6]).unwrap();
        assert!(part.capped);
        // x from 2 to 10, y from 3 to 8, z from 4 to 7
        assert!((mm3(&part.mesh) - 8.0 * 5.0 * 3.0).abs() < 1e-6);
        assert!(part.mesh.check_closed().is_ok());
        let none = clip_box(&c, [20.0e6, 0.0, 0.0], [30.0e6, 1.0e6, 1.0e6]).unwrap();
        assert!(none.mesh.triangles.is_empty());
        assert!(clip_box(&c, [1.0; 3], [1.0, 2.0, 2.0])
            .unwrap_err()
            .contains("size"));
    }

    #[test]
    fn a_tilted_solid_is_cut_and_closed_and_the_two_parts_add_up() {
        let c = cuboid([0.0; 3], [10.0, 10.0, 10.0]);
        let tilted = rotate(&c, Axis::Y, 30.0, [5.0e6, 5.0e6, 5.0e6]).unwrap();
        let total = mm3(&tilted);
        let a = clip_plane(&tilted, Axis::Z, 4.3e6, true).unwrap();
        let b = clip_plane(&tilted, Axis::Z, 4.3e6, false).unwrap();
        assert!(a.capped && b.capped);
        assert!(a.mesh.check_closed().is_ok() && b.mesh.check_closed().is_ok());
        assert!((mm3(&a.mesh) + mm3(&b.mesh) - total).abs() < 1e-3);
    }

    #[test]
    fn random_cuts_with_a_fixed_seed_keep_the_volume() {
        let mut seed = 4242u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f64::from(seed >> 8) / f64::from(1u32 << 24)
        };
        for _ in 0..40 {
            let size = [
                2.0 + 20.0 * next(),
                2.0 + 20.0 * next(),
                2.0 + 20.0 * next(),
            ];
            let c = cuboid([0.0; 3], size);
            let turned = rotate(&c, Axis::X, 360.0 * next(), [0.0; 3]).unwrap();
            let turned = rotate(&turned, Axis::Z, 360.0 * next(), [1.0e6, 2.0e6, 0.0]).unwrap();
            let total = mm3(&turned);
            let axis = [Axis::X, Axis::Y, Axis::Z][(next() * 3.0) as usize % 3];
            let (lo, hi) = turned.bounds().unwrap();
            let k = axis.index();
            let at = (lo[k] + (hi[k] - lo[k]) * (0.1 + 0.8 * next())).round();
            let a = clip_plane(&turned, axis, at, true).unwrap();
            let b = clip_plane(&turned, axis, at, false).unwrap();
            assert!(a.capped && b.capped);
            assert!(
                (mm3(&a.mesh) + mm3(&b.mesh) - total).abs() < 1e-3 * total.abs().max(1.0),
                "{} + {} against {total}",
                mm3(&a.mesh),
                mm3(&b.mesh)
            );
            assert!(a.mesh.check_closed().is_ok() && b.mesh.check_closed().is_ok());
            // the section of the whole at that place has the area of the cap
            let section = slice(&turned, axis, at);
            let cap: f64 = a
                .mesh
                .triangles
                .iter()
                .rev()
                .take(a.cap_triangles)
                .map(|t| a.mesh.area_nm2(t))
                .sum::<f64>()
                / 1.0e12;
            assert!((cap - section.area_mm2).abs() < 1e-3 * section.area_mm2.max(1.0));
        }
    }
}
