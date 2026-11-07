//! The profile of a sketch: the closed outlines that its lines, arcs and circles draw, ready to be
//! extruded or cut.
//!
//! Only what is not construction geometry counts. Lines and arcs are joined end to end into
//! chains (two ends within `profile_join` of each other are one corner); every chain must close,
//! and a corner where three ends meet is refused. A circle is an outline of its own. An outline
//! that crosses or touches itself or another one is refused, and so is one smaller than
//! `profile_min_area`. The outlines are then sorted by containment: the outermost ones bound
//! regions, the ones inside them are holes, the ones inside the holes are islands that bound
//! regions again, and so on. Outer outlines are turned counter-clockwise and holes clockwise, so
//! the material is always on the left.
//!
//! The checks run on the corners, exact in integer nanometres (128-bit products). An arc or a
//! circle stands in them for the straight lines that sag from it by at most `profile_chord`; the
//! area uses the arc itself (a floating point figure, so a volume made from it is rounded to a
//! cubic nanometre and is not the exact integer that the box volume is).

use std::f64::consts::{PI, TAU};

use crate::sketch::settings::SketchSettings;
use crate::sketch::{Element, Sketch};
use crate::CadError;

/// A point of the sketch in nanometres.
pub type Pt = (i64, i64);

/// The most straight lines that stand for one arc or circle.
const MAX_CHORDS: usize = 4096;

/// The limits of a profile, from the settings of the sketch, in integer nanometres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileLimits {
    /// Two ends this near on each axis are the same corner.
    pub join_nm: i64,
    /// The least area of an outline in square nanometres.
    pub min_area_nm2: i128,
    /// The most a curve sags from the straight lines that stand for it.
    pub chord_nm: i64,
}

impl ProfileLimits {
    /// The limits of some settings.
    pub fn from_settings(s: &SketchSettings) -> ProfileLimits {
        let join: f64 = (s.profile_join * 1e6).round();
        let chord: f64 = (s.profile_chord * 1e6).round();
        let area: f64 = (s.profile_min_area * 1e12).round();
        ProfileLimits {
            join_nm: (join as i64).max(1),
            min_area_nm2: (area as i128).max(1),
            chord_nm: (chord as i64).max(1),
        }
    }

    /// The limits of the shipped settings.
    pub fn shipped() -> ProfileLimits {
        ProfileLimits::from_settings(&SketchSettings::defaults())
    }
}

/// One piece of an outline, in the direction it is walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    /// A straight line.
    Line {
        /// Where it starts.
        from: Pt,
        /// Where it ends.
        to: Pt,
    },
    /// An arc about a centre.
    Arc {
        /// The centre.
        center: Pt,
        /// Where it starts.
        from: Pt,
        /// Where it ends.
        to: Pt,
        /// Whether it is walked counter-clockwise about its centre.
        ccw: bool,
    },
}

impl Piece {
    fn from(&self) -> Pt {
        match *self {
            Piece::Line { from, .. } | Piece::Arc { from, .. } => from,
        }
    }

    fn reversed(&self) -> Piece {
        match *self {
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
}

/// A closed outline: a chain of lines and arcs, or a circle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    pieces: Vec<Piece>,
    circle: Option<(Pt, i64)>,
    polygon: Vec<Pt>,
}

/// An outer outline and the holes in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// The outer outline, counter-clockwise.
    pub outer: Outline,
    /// The holes, clockwise.
    pub holes: Vec<Outline>,
}

/// The regions of a sketch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// The regions, in order of their lowest left corner.
    pub regions: Vec<Region>,
}

fn bad(why: impl Into<String>) -> CadError {
    CadError::Profile(why.into())
}

fn mm_text(nm: i64) -> String {
    let sign = if nm < 0 { "-" } else { "" };
    let a = nm.unsigned_abs();
    let whole = a / 1_000_000;
    let frac = a % 1_000_000;
    if frac == 0 {
        format!("{sign}{whole}")
    } else {
        let text = format!("{frac:06}");
        format!("{sign}{whole}.{}", text.trim_end_matches('0'))
    }
}

fn at_text(p: Pt) -> String {
    format!("({}, {}) mm", mm_text(p.0), mm_text(p.1))
}

// ---------------------------------------------------------------------------------------------
// Exact geometry on the corners
// ---------------------------------------------------------------------------------------------

fn cross(o: Pt, a: Pt, b: Pt) -> i128 {
    (i128::from(a.0) - i128::from(o.0)) * (i128::from(b.1) - i128::from(o.1))
        - (i128::from(a.1) - i128::from(o.1)) * (i128::from(b.0) - i128::from(o.0))
}

fn dot(o: Pt, a: Pt, b: Pt) -> i128 {
    (i128::from(a.0) - i128::from(o.0)) * (i128::from(b.0) - i128::from(o.0))
        + (i128::from(a.1) - i128::from(o.1)) * (i128::from(b.1) - i128::from(o.1))
}

fn sign(v: i128) -> i32 {
    match v {
        0 => 0,
        v if v > 0 => 1,
        _ => -1,
    }
}

fn within(a: i64, b: i64, c: i64) -> bool {
    a.min(b) <= c && c <= a.max(b)
}

/// Whether the segments `a b` and `c d` share a point (touching counts).
fn segments_touch(a: Pt, b: Pt, c: Pt, d: Pt) -> bool {
    let d1 = sign(cross(a, b, c));
    let d2 = sign(cross(a, b, d));
    let d3 = sign(cross(c, d, a));
    let d4 = sign(cross(c, d, b));
    if d1 * d2 < 0 && d3 * d4 < 0 {
        return true;
    }
    let on = |p: Pt, q: Pt, r: Pt| within(p.0, q.0, r.0) && within(p.1, q.1, r.1);
    (d1 == 0 && on(a, b, c))
        || (d2 == 0 && on(a, b, d))
        || (d3 == 0 && on(c, d, a))
        || (d4 == 0 && on(c, d, b))
}

/// Twice the signed area of a polygon, positive when it runs counter-clockwise.
fn area2(poly: &[Pt]) -> i128 {
    let mut sum: i128 = 0;
    for (i, p) in poly.iter().enumerate() {
        let q = poly[(i + 1) % poly.len()];
        sum += i128::from(p.0) * i128::from(q.1) - i128::from(q.0) * i128::from(p.1);
    }
    sum
}

/// Whether a point lies inside a polygon (the point is never on its boundary here).
fn inside(poly: &[Pt], p: Pt) -> bool {
    let mut odd = false;
    for (i, a) in poly.iter().enumerate() {
        let b = poly[(i + 1) % poly.len()];
        let dy = i128::from(b.1) - i128::from(a.1);
        if (a.1 > p.1) != (b.1 > p.1) {
            let t = (i128::from(b.0) - i128::from(a.0)) * (i128::from(p.1) - i128::from(a.1))
                - (i128::from(p.0) - i128::from(a.0)) * dy;
            if (dy > 0 && t > 0) || (dy < 0 && t < 0) {
                odd = !odd;
            }
        }
    }
    odd
}

fn bounds_of(poly: &[Pt]) -> (Pt, Pt) {
    let mut lo = poly[0];
    let mut hi = poly[0];
    for p in poly {
        lo = (lo.0.min(p.0), lo.1.min(p.1));
        hi = (hi.0.max(p.0), hi.1.max(p.1));
    }
    (lo, hi)
}

// ---------------------------------------------------------------------------------------------
// Curves
// ---------------------------------------------------------------------------------------------

fn angle_of(center: Pt, p: Pt) -> f64 {
    let dy: f64 = (p.1 - center.1) as f64;
    let dx: f64 = (p.0 - center.0) as f64;
    dy.atan2(dx)
}

fn radius_of(center: Pt, p: Pt) -> f64 {
    let dx: f64 = (p.0 - center.0) as f64;
    let dy: f64 = (p.1 - center.1) as f64;
    dx.hypot(dy)
}

/// The angle walked by an arc, in (0, 2 pi].
fn sweep_of(center: Pt, from: Pt, to: Pt, ccw: bool) -> f64 {
    let a0: f64 = angle_of(center, from);
    let a1: f64 = angle_of(center, to);
    let raw: f64 = if ccw { a1 - a0 } else { a0 - a1 };
    let mut s: f64 = raw.rem_euclid(TAU);
    if s <= 0.0 {
        s = TAU;
    }
    s
}

/// How many straight lines stand for an angle of a circle of radius `r` so that none sags more
/// than `chord` from it.
fn chords_for(r: f64, sweep: f64, chord: i64) -> usize {
    let s: f64 = chord as f64;
    if r <= s {
        return ((sweep / (PI / 2.0)).ceil() as usize).clamp(1, MAX_CHORDS);
    }
    let step: f64 = 2.0 * (1.0 - s / r).acos();
    if step <= 0.0 {
        return MAX_CHORDS;
    }
    ((sweep / step).ceil() as usize).clamp(1, MAX_CHORDS)
}

fn polar(center: Pt, r: f64, angle: f64) -> Pt {
    let (s, c) = angle.sin_cos();
    (
        center.0 + (r * c).round() as i64,
        center.1 + (r * s).round() as i64,
    )
}

/// The corners that stand for a chain of pieces: the start of each piece and the points on its
/// arc, without two equal corners in a row.
fn flatten(pieces: &[Piece], chord: i64) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::new();
    let mut push = |p: Pt| {
        if out.last() != Some(&p) {
            out.push(p);
        }
    };
    for piece in pieces {
        match *piece {
            Piece::Line { from, .. } => push(from),
            Piece::Arc {
                center,
                from,
                to,
                ccw,
            } => {
                push(from);
                let r = radius_of(center, from);
                let sweep = sweep_of(center, from, to, ccw);
                let n = chords_for(r, sweep, chord);
                let a0 = angle_of(center, from);
                for k in 1..n {
                    let t: f64 = sweep * (k as f64) / (n as f64);
                    let a: f64 = if ccw { a0 + t } else { a0 - t };
                    push(polar(center, r, a));
                }
            }
        }
    }
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

fn circle_polygon(center: Pt, radius: i64, chord: i64) -> Vec<Pt> {
    let r = radius as f64;
    let n = chords_for(r, TAU, chord).max(8);
    let mut out: Vec<Pt> = Vec::with_capacity(n);
    for k in 0..n {
        let a: f64 = TAU * (k as f64) / (n as f64);
        let p = polar(center, r, a);
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

impl Outline {
    fn chain(pieces: Vec<Piece>, chord: i64) -> Outline {
        let polygon = flatten(&pieces, chord);
        Outline {
            pieces,
            circle: None,
            polygon,
        }
    }

    fn circle(center: Pt, radius: i64, chord: i64) -> Outline {
        Outline {
            pieces: Vec::new(),
            circle: Some((center, radius)),
            polygon: circle_polygon(center, radius, chord),
        }
    }

    /// The pieces of a chain, in the direction they are walked; none for a circle.
    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// The centre and the radius of a circle, or `None` for a chain.
    pub fn circle_of(&self) -> Option<(Pt, i64)> {
        self.circle
    }

    /// The straight lines that stand for the outline, as corners in the direction walked.
    pub fn polygon(&self) -> &[Pt] {
        &self.polygon
    }

    /// The area in square nanometres (positive). A circle is pi r squared; the arcs of a chain
    /// add or take away their circular segments to the area of the polygon of the ends.
    pub fn area_nm2(&self) -> f64 {
        if let Some((_, r)) = self.circle {
            let r: f64 = r as f64;
            return PI * r * r;
        }
        let ends: Vec<Pt> = self.pieces.iter().map(Piece::from).collect();
        let mut area: f64 = area2(&ends) as f64 / 2.0;
        for piece in &self.pieces {
            if let Piece::Arc {
                center,
                from,
                to,
                ccw,
            } = *piece
            {
                let r: f64 = radius_of(center, from);
                let theta: f64 = sweep_of(center, from, to, ccw);
                let segment: f64 = 0.5 * r * r * (theta - theta.sin());
                area += if ccw { segment } else { -segment };
            }
        }
        area.abs()
    }

    fn reversed(&self) -> Outline {
        let mut polygon = self.polygon.clone();
        polygon.reverse();
        Outline {
            pieces: self.pieces.iter().rev().map(Piece::reversed).collect(),
            circle: self.circle,
            polygon,
        }
    }

    fn signed2(&self) -> i128 {
        area2(&self.polygon)
    }
}

// ---------------------------------------------------------------------------------------------
// From a sketch
// ---------------------------------------------------------------------------------------------

struct Seg {
    /// The centre when it is an arc (counter-clockwise from `p` to `q`).
    center: Option<Pt>,
    p: Pt,
    q: Pt,
}

fn node_of(nodes: &mut Vec<Pt>, p: Pt, join: i64) -> usize {
    for (i, n) in nodes.iter().enumerate() {
        if (n.0 - p.0).abs() <= join && (n.1 - p.1).abs() <= join {
            return i;
        }
    }
    nodes.push(p);
    nodes.len() - 1
}

fn point_of(sketch: &Sketch, id: crate::sketch::ElementId) -> Result<Pt, CadError> {
    let (x, y) = sketch.point(id)?;
    Ok((x.get(), y.get()))
}

fn chains(segs: &[Seg], limits: &ProfileLimits) -> Result<Vec<Vec<Piece>>, CadError> {
    let mut nodes: Vec<Pt> = Vec::new();
    let mut ends: Vec<(usize, usize)> = Vec::with_capacity(segs.len());
    for s in segs {
        let a = node_of(&mut nodes, s.p, limits.join_nm);
        let b = node_of(&mut nodes, s.q, limits.join_nm);
        if a == b {
            return Err(bad(format!(
                "a line or an arc ends where it starts, at {}",
                at_text(s.p)
            )));
        }
        ends.push((a, b));
    }
    // the ends that meet at each corner: (segment, 0 for its start or 1 for its end)
    let mut meet: Vec<Vec<(usize, usize)>> = vec![Vec::new(); nodes.len()];
    for (i, (a, b)) in ends.iter().enumerate() {
        meet[*a].push((i, 0));
        meet[*b].push((i, 1));
    }
    for (n, list) in meet.iter().enumerate() {
        if list.len() < 2 {
            return Err(bad(format!(
                "the outline is not closed at {}",
                at_text(nodes[n])
            )));
        }
        if list.len() > 2 {
            return Err(bad(format!(
                "three or more lines or arcs meet at {}",
                at_text(nodes[n])
            )));
        }
    }
    let mut used = vec![false; segs.len()];
    let mut out: Vec<Vec<Piece>> = Vec::new();
    for first in 0..segs.len() {
        if used[first] {
            continue;
        }
        let mut pieces: Vec<Piece> = Vec::new();
        let mut index = first;
        let mut forward = true;
        loop {
            used[index] = true;
            let s = &segs[index];
            let (a, b) = ends[index];
            let (from_node, to_node) = if forward { (a, b) } else { (b, a) };
            let (from, to) = (nodes[from_node], nodes[to_node]);
            pieces.push(match (s.center, forward) {
                (None, _) => Piece::Line { from, to },
                (Some(center), true) => Piece::Arc {
                    center,
                    from,
                    to,
                    ccw: true,
                },
                (Some(center), false) => Piece::Arc {
                    center,
                    from,
                    to,
                    ccw: false,
                },
            });
            // the other end that meets at the corner where this piece stops
            let next = meet[to_node]
                .iter()
                .find(|(j, _)| *j != index)
                .copied()
                .ok_or_else(|| bad("the outline is not closed"))?;
            if next.0 == first {
                break;
            }
            index = next.0;
            forward = next.1 == 0;
        }
        out.push(pieces);
    }
    Ok(out)
}

fn check_simple(poly: &[Pt], what: &str) -> Result<(), CadError> {
    let n = poly.len();
    if n < 3 {
        return Err(bad(format!("{what} has fewer than three corners")));
    }
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        for j in (i + 1)..n {
            let (c, d) = (poly[j], poly[(j + 1) % n]);
            let adjacent = j == i + 1 || (i == 0 && j == n - 1);
            if adjacent {
                // the shared corner is allowed; folding back over the line is not
                let (shared, p, q) = if j == i + 1 { (b, a, d) } else { (a, b, c) };
                if cross(shared, p, q) == 0 && dot(shared, p, q) > 0 {
                    return Err(bad(format!(
                        "{what} folds back on itself at {}",
                        at_text(shared)
                    )));
                }
            } else if segments_touch(a, b, c, d) {
                return Err(bad(format!(
                    "{what} crosses or touches itself near {}",
                    at_text(a)
                )));
            }
        }
    }
    Ok(())
}

/// The distance from a point to a segment.
fn distance_to_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let (px, py): (f64, f64) = (p.0 as f64, p.1 as f64);
    let (ax, ay): (f64, f64) = (a.0 as f64, a.1 as f64);
    let (dx, dy): (f64, f64) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
    let len2: f64 = dx * dx + dy * dy;
    let t: f64 = if len2 == 0.0 {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
    };
    (px - (ax + t * dx)).hypot(py - (ay + t * dy))
}

/// Whether a circle meets another outline: crossing it or touching it, within the join of the
/// limits. The straight lines that stand for a circle sag a little inside it, so a circle that
/// only touches a line is found from its centre and radius and not from them.
fn circle_meets(center: Pt, radius: i64, other: &Outline, join: i64) -> bool {
    let (r, j): (f64, f64) = (radius as f64, join as f64);
    if let Some((c2, r2)) = other.circle {
        let d: f64 = radius_of(center, c2);
        let r2: f64 = r2 as f64;
        let apart = d > r + r2 + j;
        let nested = d + r2 < r - j || d + r < r2 - j;
        return !(apart || nested);
    }
    let n = other.polygon.len();
    let mut nearest: f64 = f64::MAX;
    let mut farthest: f64 = 0.0;
    for i in 0..n {
        let (p, q) = (other.polygon[i], other.polygon[(i + 1) % n]);
        nearest = nearest.min(distance_to_segment(center, p, q));
        farthest = farthest.max(radius_of(center, p));
    }
    !(nearest > r + j || farthest < r - j)
}

fn outlines_touch(a: &Outline, b: &Outline, join: i64) -> bool {
    if let Some((c, r)) = a.circle {
        return circle_meets(c, r, b, join);
    }
    if let Some((c, r)) = b.circle {
        return circle_meets(c, r, a, join);
    }
    let (alo, ahi) = bounds_of(&a.polygon);
    let (blo, bhi) = bounds_of(&b.polygon);
    if ahi.0 < blo.0 || bhi.0 < alo.0 || ahi.1 < blo.1 || bhi.1 < alo.1 {
        return false;
    }
    let (n, m) = (a.polygon.len(), b.polygon.len());
    for i in 0..n {
        let (p, q) = (a.polygon[i], a.polygon[(i + 1) % n]);
        for j in 0..m {
            let (r, s) = (b.polygon[j], b.polygon[(j + 1) % m]);
            if segments_touch(p, q, r, s) {
                return true;
            }
        }
    }
    false
}

impl Profile {
    /// The profile of the sketch: the outlines of everything that is not construction geometry.
    /// Points alone are ignored.
    pub fn from_sketch(sketch: &Sketch, limits: &ProfileLimits) -> Result<Profile, CadError> {
        let mut segs: Vec<Seg> = Vec::new();
        let mut outlines: Vec<Outline> = Vec::new();
        for entry in sketch.elements() {
            if entry.construction {
                continue;
            }
            match entry.element {
                Element::Point { .. } => {}
                Element::Line { a, b } => segs.push(Seg {
                    center: None,
                    p: point_of(sketch, a)?,
                    q: point_of(sketch, b)?,
                }),
                Element::Arc { center, start, end } => segs.push(Seg {
                    center: Some(point_of(sketch, center)?),
                    p: point_of(sketch, start)?,
                    q: point_of(sketch, end)?,
                }),
                Element::Circle { center, radius } => {
                    let r = radius.get();
                    if r <= 0 {
                        return Err(bad("a circle has no radius"));
                    }
                    outlines.push(Outline::circle(
                        point_of(sketch, center)?,
                        r,
                        limits.chord_nm,
                    ));
                }
            }
        }
        for pieces in chains(&segs, limits)? {
            outlines.push(Outline::chain(pieces, limits.chord_nm));
        }
        if outlines.is_empty() {
            return Err(bad(
                "the sketch has no line, arc or circle that is not construction",
            ));
        }
        Profile::from_outlines(outlines, limits)
    }

    /// Sorts closed outlines into regions and holes, after checking each one.
    pub fn from_outlines(
        outlines: Vec<Outline>,
        limits: &ProfileLimits,
    ) -> Result<Profile, CadError> {
        for (i, o) in outlines.iter().enumerate() {
            let what = format!("outline {}", i + 1);
            check_simple(&o.polygon, &what)?;
            let corner = o.polygon[0];
            if o.area_nm2() < limits.min_area_nm2 as f64 {
                return Err(bad(format!(
                    "{what} near {} has less area than the least allowed",
                    at_text(corner)
                )));
            }
        }
        for i in 0..outlines.len() {
            for j in (i + 1)..outlines.len() {
                if outlines_touch(&outlines[i], &outlines[j], limits.join_nm) {
                    return Err(bad(format!(
                        "outlines {} and {} cross or touch near {}",
                        i + 1,
                        j + 1,
                        at_text(outlines[i].polygon[0])
                    )));
                }
            }
        }
        let n = outlines.len();
        // containers[i]: the outlines that hold outline i
        let mut containers: Vec<Vec<usize>> = vec![Vec::new(); n];
        for i in 0..n {
            for j in 0..n {
                if i != j && inside(&outlines[j].polygon, outlines[i].polygon[0]) {
                    containers[i].push(j);
                }
            }
        }
        let mut oriented: Vec<Outline> = Vec::with_capacity(n);
        for (i, o) in outlines.iter().enumerate() {
            let hole = containers[i].len() % 2 == 1;
            let ccw = o.signed2() > 0;
            oriented.push(if hole == ccw { o.reversed() } else { o.clone() });
        }
        let mut order: Vec<usize> = (0..n).filter(|i| containers[*i].len() % 2 == 0).collect();
        order.sort_by_key(|i| lowest_left(&oriented[*i].polygon));
        let mut regions: Vec<Region> = Vec::new();
        for i in &order {
            let mut holes: Vec<usize> = (0..n)
                .filter(|h| {
                    containers[*h].len() == containers[*i].len() + 1 && containers[*h].contains(i)
                })
                .collect();
            holes.sort_by_key(|h| lowest_left(&oriented[*h].polygon));
            regions.push(Region {
                outer: oriented[*i].clone(),
                holes: holes.into_iter().map(|h| oriented[h].clone()).collect(),
            });
        }
        Ok(Profile { regions })
    }

    /// The area of the material in square nanometres: each outer outline minus its holes.
    pub fn area_nm2(&self) -> f64 {
        let mut total: f64 = 0.0;
        for r in &self.regions {
            total += r.outer.area_nm2();
            for h in &r.holes {
                total -= h.area_nm2();
            }
        }
        total
    }

    /// The smallest and the largest corner of all the outlines.
    pub fn bounds(&self) -> (Pt, Pt) {
        let mut lo = self.regions[0].outer.polygon[0];
        let mut hi = lo;
        for r in &self.regions {
            for o in std::iter::once(&r.outer).chain(r.holes.iter()) {
                let (a, b) = bounds_of(&o.polygon);
                lo = (lo.0.min(a.0), lo.1.min(a.1));
                hi = (hi.0.max(b.0), hi.1.max(b.1));
            }
        }
        (lo, hi)
    }

    /// Every outline, the outer ones and their holes in turn.
    pub fn outlines(&self) -> Vec<&Outline> {
        let mut out: Vec<&Outline> = Vec::new();
        for r in &self.regions {
            out.push(&r.outer);
            out.extend(r.holes.iter());
        }
        out
    }

    /// The volume of a prism of this profile that is `height_nm` high, in cubic nanometres,
    /// rounded.
    pub fn volume_nm3(&self, height_nm: i64) -> i128 {
        let v: f64 = self.area_nm2() * (height_nm as f64);
        v.round() as i128
    }
}

fn lowest_left(poly: &[Pt]) -> Pt {
    let mut best = poly[0];
    for p in poly {
        if (p.0, p.1) < (best.0, best.1) {
            best = *p;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::Face;
    use crate::sketch::SketchPlane;
    use crate::Length;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn um(v: i64) -> Length {
        Length::from_um(v).unwrap()
    }

    fn limits() -> ProfileLimits {
        ProfileLimits::shipped()
    }

    fn new() -> Sketch {
        Sketch::new("plate", SketchPlane::on_face(Face::Top)).unwrap()
    }

    /// A closed chain of lines through the given corners, in millimetres.
    fn polygon(s: &mut Sketch, corners: &[(i64, i64)]) {
        let ids: Vec<_> = corners
            .iter()
            .map(|(x, y)| s.add_point(mm(*x), mm(*y)).unwrap())
            .collect();
        for i in 0..ids.len() {
            s.add_line(ids[i], ids[(i + 1) % ids.len()]).unwrap();
        }
    }

    fn circle(s: &mut Sketch, x: i64, y: i64, r: i64) {
        let c = s.add_point(mm(x), mm(y)).unwrap();
        s.add_circle(c, mm(r)).unwrap();
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * b.abs().max(1.0)
    }

    #[test]
    fn the_limits_come_from_the_settings() {
        let l = limits();
        assert_eq!(l.join_nm, 1_000);
        assert_eq!(l.min_area_nm2, 10_000_000_000);
        assert_eq!(l.chord_nm, 5_000);
        let settings = SketchSettings::with_user_config(
            "[sketch]\nprofile_join = 0.01\nprofile_chord = 0.1\n",
        )
        .unwrap();
        let wide = ProfileLimits::from_settings(&settings);
        assert_eq!((wide.join_nm, wide.chord_nm), (10_000, 100_000));
    }

    #[test]
    fn a_rectangle_with_a_round_hole_has_the_area_worked_out_by_hand() {
        // 40 by 25 mm with a hole of diameter 6 mm: 1000 - 9 pi = 971.7256 mm2 (python: 971.7256...)
        let mut s = new();
        polygon(&mut s, &[(0, 0), (40, 0), (40, 25), (0, 25)]);
        circle(&mut s, 20, 12, 3);
        let p = Profile::from_sketch(&s, &limits()).unwrap();
        assert_eq!(p.regions.len(), 1);
        assert_eq!(p.regions[0].holes.len(), 1);
        let mm2 = p.area_nm2() / 1e12;
        assert!(near(mm2, 1000.0 - 9.0 * PI), "{mm2}");
        // the prism 10 mm high: 40 * 25 * 10 - pi * 3^2 * 10 = 9717.2566 mm3 = 9.7172566e21 nm3
        let volume = p.volume_nm3(10_000_000);
        let expect = 1000.0 * 10.0 - PI * 9.0 * 10.0;
        assert!(near(volume as f64 / 1e18, expect), "{volume}");
        // the same with 38 digits (python decimal): 9717256661176918608538 nm3
        assert!((volume - 9_717_256_661_176_918_608_538i128).abs() < 10_000_000);
    }

    #[test]
    fn the_outer_outline_runs_counter_clockwise_and_the_hole_clockwise() {
        let mut s = new();
        // drawn clockwise on purpose
        polygon(&mut s, &[(0, 0), (0, 25), (40, 25), (40, 0)]);
        polygon(&mut s, &[(10, 5), (20, 5), (20, 15), (10, 15)]);
        let p = Profile::from_sketch(&s, &limits()).unwrap();
        assert_eq!(p.regions.len(), 1);
        assert!(p.regions[0].outer.signed2() > 0);
        assert!(p.regions[0].holes[0].signed2() < 0);
        assert!(near(p.area_nm2() / 1e12, 1000.0 - 100.0));
    }

    #[test]
    fn an_island_inside_a_hole_is_a_region_of_its_own() {
        let mut s = new();
        polygon(&mut s, &[(0, 0), (60, 0), (60, 60), (0, 60)]);
        polygon(&mut s, &[(10, 10), (50, 10), (50, 50), (10, 50)]);
        polygon(&mut s, &[(20, 20), (40, 20), (40, 40), (20, 40)]);
        let p = Profile::from_sketch(&s, &limits()).unwrap();
        assert_eq!(p.regions.len(), 2);
        assert_eq!(p.regions[0].holes.len(), 1);
        assert_eq!(p.regions[1].holes.len(), 0);
        // 3600 - 1600 + 400
        assert!(near(p.area_nm2() / 1e12, 2400.0));
    }

    #[test]
    fn two_separate_outlines_are_two_regions_in_order_of_their_corner() {
        let mut s = new();
        circle(&mut s, 50, 10, 4);
        polygon(&mut s, &[(0, 0), (10, 0), (10, 10), (0, 10)]);
        let p = Profile::from_sketch(&s, &limits()).unwrap();
        assert_eq!(p.regions.len(), 2);
        assert!(p.regions[0].outer.circle_of().is_none());
        assert!(p.regions[1].outer.circle_of().is_some());
    }

    #[test]
    fn arcs_close_an_outline_and_their_segments_count_in_the_area() {
        // a half disc of radius 10 mm: a line from (10, 0) to (-10, 0) and the lower arc
        let mut s = new();
        let c = s.add_point(mm(0), mm(0)).unwrap();
        let a = s.add_point(mm(-10), mm(0)).unwrap();
        let b = s.add_point(mm(10), mm(0)).unwrap();
        s.add_arc(c, a, b).unwrap();
        s.add_line(b, a).unwrap();
        let p = Profile::from_sketch(&s, &limits()).unwrap();
        assert_eq!(p.regions.len(), 1);
        assert!(p.regions[0].outer.signed2() > 0);
        // pi r^2 / 2 = 157.0796 mm2
        assert!(near(p.area_nm2() / 1e12, 50.0 * PI), "{}", p.area_nm2());
        // the straight lines stand for the arc within the chord of the settings
        let n = p.regions[0].outer.polygon().len();
        assert!(n > 20, "{n}");
    }

    #[test]
    fn a_slot_made_of_two_lines_and_two_arcs_has_its_area() {
        // length 30 and width 10 (radius 5): 20 * 10 + pi * 25 = 278.5398 mm2
        let mut s = new();
        let c1 = s.add_point(mm(5), mm(5)).unwrap();
        let c2 = s.add_point(mm(25), mm(5)).unwrap();
        let a = s.add_point(mm(5), mm(0)).unwrap();
        let b = s.add_point(mm(25), mm(0)).unwrap();
        let c = s.add_point(mm(25), mm(10)).unwrap();
        let d = s.add_point(mm(5), mm(10)).unwrap();
        s.add_line(a, b).unwrap();
        s.add_arc(c2, b, c).unwrap();
        s.add_line(c, d).unwrap();
        s.add_arc(c1, d, a).unwrap();
        let p = Profile::from_sketch(&s, &limits()).unwrap();
        assert!(
            near(p.area_nm2() / 1e12, 200.0 + 25.0 * PI),
            "{}",
            p.area_nm2()
        );
    }

    #[test]
    fn construction_geometry_is_not_part_of_the_profile() {
        let mut s = new();
        polygon(&mut s, &[(0, 0), (40, 0), (40, 25), (0, 25)]);
        let c = s.add_point(mm(20), mm(12)).unwrap();
        let guide = s.add_circle(c, mm(3)).unwrap();
        s.set_construction(guide, true).unwrap();
        let p = Profile::from_sketch(&s, &limits()).unwrap();
        assert!(p.regions[0].holes.is_empty());
        assert!(near(p.area_nm2() / 1e12, 1000.0));
    }

    fn refused(s: &Sketch) -> String {
        let e = Profile::from_sketch(s, &limits()).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-036");
        e.to_string()
    }

    #[test]
    fn an_open_chain_a_branch_and_an_empty_sketch_are_refused_with_the_place() {
        let mut s = new();
        let a = s.add_point(mm(0), mm(0)).unwrap();
        let b = s.add_point(mm(10), mm(0)).unwrap();
        let c = s.add_point(mm(10), mm(10)).unwrap();
        s.add_line(a, b).unwrap();
        s.add_line(b, c).unwrap();
        assert!(refused(&s).contains("not closed at (0, 0) mm"));
        // a third line at (10, 0) is a branch
        let d = s.add_point(mm(20), mm(0)).unwrap();
        s.add_line(c, a).unwrap();
        s.add_line(b, d).unwrap();
        assert!(refused(&s).contains("three or more"));
        let mut only_points = new();
        only_points.add_point(mm(1), mm(1)).unwrap();
        assert!(refused(&only_points).contains("no line, arc or circle"));
        let mut guide = new();
        let c = guide.add_point(mm(0), mm(0)).unwrap();
        let k = guide.add_circle(c, mm(5)).unwrap();
        guide.set_construction(k, true).unwrap();
        assert!(refused(&guide).contains("no line, arc or circle"));
    }

    #[test]
    fn a_bow_tie_a_crossing_and_a_touch_are_refused() {
        let mut bow = new();
        polygon(&mut bow, &[(0, 0), (10, 10), (10, 0), (0, 10)]);
        assert!(refused(&bow).contains("crosses or touches itself"));
        // a circle that crosses a rectangle
        let mut s = new();
        polygon(&mut s, &[(0, 0), (40, 0), (40, 25), (0, 25)]);
        circle(&mut s, 40, 12, 3);
        assert!(refused(&s).contains("cross or touch"));
        // a circle that touches the side from inside
        let mut t = new();
        polygon(&mut t, &[(0, 0), (40, 0), (40, 25), (0, 25)]);
        circle(&mut t, 20, 3, 3);
        assert!(refused(&t).contains("cross or touch"));
    }

    #[test]
    fn two_lines_over_each_other_fold_back_and_are_refused() {
        // the corner (10, 0) goes out and comes back over the same line
        let mut s = new();
        polygon(&mut s, &[(0, 0), (10, 0), (5, 0), (5, 10)]);
        let e = refused(&s);
        assert!(
            e.contains("folds back") || e.contains("crosses or touches"),
            "{e}"
        );
    }

    #[test]
    fn an_outline_under_the_least_area_is_refused() {
        // 0.05 by 0.1 mm = 0.005 mm2, under the 0.01 of the settings
        let mut s = new();
        let a = s.add_point(um(0), um(0)).unwrap();
        let b = s.add_point(um(50), um(0)).unwrap();
        let c = s.add_point(um(50), um(100)).unwrap();
        let d = s.add_point(um(0), um(100)).unwrap();
        s.add_line(a, b).unwrap();
        s.add_line(b, c).unwrap();
        s.add_line(c, d).unwrap();
        s.add_line(d, a).unwrap();
        assert!(refused(&s).contains("less area"));
        let settings =
            SketchSettings::with_user_config("[sketch]\nprofile_min_area = 0.001\n").unwrap();
        let small = ProfileLimits::from_settings(&settings);
        assert!(Profile::from_sketch(&s, &small).is_ok());
    }

    #[test]
    fn ends_within_the_join_tolerance_are_one_corner() {
        let mut s = new();
        let a = s.add_point(mm(0), mm(0)).unwrap();
        let b = s.add_point(mm(10), mm(0)).unwrap();
        let b2 = s
            .add_point(
                Length::new(10_000_000 + 400).unwrap(),
                Length::new(-300).unwrap(),
            )
            .unwrap();
        let c = s.add_point(mm(10), mm(10)).unwrap();
        let d = s.add_point(mm(0), mm(10)).unwrap();
        s.add_line(a, b).unwrap();
        s.add_line(b2, c).unwrap();
        s.add_line(c, d).unwrap();
        s.add_line(d, a).unwrap();
        assert!(Profile::from_sketch(&s, &limits()).is_ok());
        // 0.01 mm apart is beyond the join of 0.001 mm
        let mut t = new();
        let a = t.add_point(mm(0), mm(0)).unwrap();
        let b = t.add_point(mm(10), mm(0)).unwrap();
        let b2 = t
            .add_point(Length::new(10_010_000).unwrap(), mm(0))
            .unwrap();
        let c = t.add_point(mm(10), mm(10)).unwrap();
        let d = t.add_point(mm(0), mm(10)).unwrap();
        t.add_line(a, b).unwrap();
        t.add_line(b2, c).unwrap();
        t.add_line(c, d).unwrap();
        t.add_line(d, a).unwrap();
        assert!(refused(&t).contains("not closed"));
    }

    #[test]
    fn the_bounds_and_the_pieces_are_those_of_the_drawing() {
        let mut s = new();
        polygon(&mut s, &[(2, 3), (42, 3), (42, 28), (2, 28)]);
        let p = Profile::from_sketch(&s, &limits()).unwrap();
        assert_eq!(
            p.bounds(),
            ((2_000_000, 3_000_000), (42_000_000, 28_000_000))
        );
        assert_eq!(p.regions[0].outer.pieces().len(), 4);
        assert_eq!(p.outlines().len(), 1);
        assert_eq!(p.volume_nm3(0), 0);
    }

    #[test]
    fn the_same_sketch_gives_the_same_profile() {
        let mut s = new();
        polygon(&mut s, &[(0, 0), (40, 0), (40, 25), (0, 25)]);
        circle(&mut s, 10, 12, 3);
        circle(&mut s, 30, 12, 3);
        let a = Profile::from_sketch(&s, &limits()).unwrap();
        assert_eq!(a, Profile::from_sketch(&s, &limits()).unwrap());
        assert_eq!(a.regions[0].holes.len(), 2);
        assert!(a.regions[0].holes[0].circle_of().unwrap().0 .0 < 20_000_000);
    }
}
