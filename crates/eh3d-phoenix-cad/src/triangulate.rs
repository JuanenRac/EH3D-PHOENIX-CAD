//! Cutting a flat region into triangles.
//!
//! A region is an outer outline with holes in it. The outline and the holes are lists of points,
//! each with the number of a vertex of the mesh that the caller keeps, so that the triangles that
//! come back are three numbers of vertices. The way is the usual one of cutting off ears: the holes
//! are joined to the outline by a bridge (the nearest outline point that the hole sees along a ray
//! to the left) and the ring that results is cut one convex corner at a time, a corner being an ear
//! when no other point of the ring lies in its triangle. No point is ever dropped from a ring,
//! even a point in the middle of a straight run, because the next piece of the surface has the same
//! point and a point that one side has and the other does not leaves a crack in the mesh.
//!
//! Everything here is calculation with `f64` in millimetres from the same steps for the same
//! input, so the same region always gives the same triangles.

/// A point of an outline: the number of its vertex in the mesh and its place in the plane.
pub type Pt = (u32, f64, f64);

/// The smallest area, in square millimetres, that is told from a straight line.
const EPS: f64 = 1e-9;

#[derive(Debug, Clone, Copy)]
struct Node {
    id: u32,
    x: f64,
    y: f64,
    prev: usize,
    next: usize,
}

/// The doubled area of a closed outline: positive when it goes counter-clockwise.
pub fn area2(poly: &[Pt]) -> f64 {
    let mut sum = 0.0;
    for (i, p) in poly.iter().enumerate() {
        let q = poly[(i + 1) % poly.len()];
        sum += p.1 * q.2 - q.1 * p.2;
    }
    sum
}

/// Positive when `c` is to the left of the way from `a` to `b`.
fn orient(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

fn at(n: &Node) -> (f64, f64) {
    (n.x, n.y)
}

fn same_place(a: (f64, f64), b: (f64, f64)) -> bool {
    a.0 == b.0 && a.1 == b.1
}

/// Whether `p` is in the triangle or on its sides, whichever way the triangle goes round.
fn in_triangle(a: (f64, f64), b: (f64, f64), c: (f64, f64), p: (f64, f64)) -> bool {
    let d1 = orient(a, b, p);
    let d2 = orient(b, c, p);
    let d3 = orient(c, a, p);
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

struct Ring {
    nodes: Vec<Node>,
}

impl Ring {
    /// Adds an outline as a closed ring, counter-clockwise or clockwise as asked, and gives the
    /// number of its first node.
    fn add(&mut self, pts: &[Pt], counter_clockwise: bool) -> usize {
        let first = self.nodes.len();
        let forward = (area2(pts) > 0.0) == counter_clockwise;
        let n = pts.len();
        for k in 0..n {
            let p = if forward { pts[k] } else { pts[n - 1 - k] };
            self.nodes.push(Node {
                id: p.0,
                x: p.1,
                y: p.2,
                prev: first + (k + n - 1) % n,
                next: first + (k + 1) % n,
            });
        }
        first
    }

    fn prev(&self, i: usize) -> usize {
        self.nodes[i].prev
    }

    fn next(&self, i: usize) -> usize {
        self.nodes[i].next
    }

    /// Whether the way from the node `a` towards the point of `b` goes into the region at `a`.
    fn locally_inside(&self, a: usize, b: usize) -> bool {
        let (p, n, q) = (
            at(&self.nodes[self.prev(a)]),
            at(&self.nodes[self.next(a)]),
            at(&self.nodes[b]),
        );
        let here = at(&self.nodes[a]);
        if orient(p, here, n) > 0.0 {
            orient(here, q, n) <= 0.0 && orient(here, p, q) <= 0.0
        } else {
            orient(here, q, p) > 0.0 || orient(here, n, q) > 0.0
        }
    }

    /// The node of the ring that a hole is joined to, found from the leftmost node `h` of the hole.
    fn bridge(&self, h: usize, start: usize) -> Option<usize> {
        let (hx, hy) = (self.nodes[h].x, self.nodes[h].y);
        let mut qx = f64::NEG_INFINITY;
        let mut m: Option<usize> = None;
        let mut p = start;
        loop {
            let q = self.next(p);
            let (a, b) = (&self.nodes[p], &self.nodes[q]);
            if hy <= a.y && hy >= b.y && b.y != a.y {
                let x = a.x + (hy - a.y) * (b.x - a.x) / (b.y - a.y);
                if x <= hx && x > qx {
                    qx = x;
                    m = Some(if a.x < b.x { p } else { q });
                    if x == hx {
                        return m;
                    }
                }
            }
            p = q;
            if p == start {
                break;
            }
        }
        let mut m = m?;
        let (mx, my) = (self.nodes[m].x, self.nodes[m].y);
        let stop = m;
        let mut tan_min = f64::INFINITY;
        let mut p = m;
        loop {
            let n = &self.nodes[p];
            if hx >= n.x
                && n.x >= mx
                && hx != n.x
                && in_triangle((hx, hy), (qx, hy), (mx, my), (n.x, n.y))
            {
                let tan = (hy - n.y).abs() / (hx - n.x);
                if self.locally_inside(p, h) && tan < tan_min {
                    m = p;
                    tan_min = tan;
                }
            }
            p = self.next(p);
            if p == stop {
                break;
            }
        }
        Some(m)
    }

    /// Joins the ring of node `b` to the ring of node `a` along a bridge between the two; the
    /// nodes of the bridge appear twice, once for each way along it.
    fn split(&mut self, a: usize, b: usize) {
        let a2 = self.nodes.len();
        let b2 = a2 + 1;
        let (an, bp) = (self.next(a), self.prev(b));
        let (na, nb) = (self.nodes[a], self.nodes[b]);
        self.nodes.push(Node {
            id: na.id,
            x: na.x,
            y: na.y,
            prev: b2,
            next: an,
        });
        self.nodes.push(Node {
            id: nb.id,
            x: nb.x,
            y: nb.y,
            prev: bp,
            next: a2,
        });
        self.nodes[a].next = b;
        self.nodes[b].prev = a;
        self.nodes[an].prev = a2;
        self.nodes[bp].next = b2;
    }

    fn remove(&mut self, i: usize) {
        let (p, n) = (self.prev(i), self.next(i));
        self.nodes[p].next = n;
        self.nodes[n].prev = p;
    }

    /// Whether the corner at `i` can be cut off. A strict ear turns left; the last resort accepts
    /// a corner on a straight run too, which makes a triangle with no area but keeps every point.
    fn is_ear(&self, i: usize, strict: bool) -> bool {
        let (pi, ni) = (self.prev(i), self.next(i));
        let (a, b, c) = (at(&self.nodes[pi]), at(&self.nodes[i]), at(&self.nodes[ni]));
        let o = orient(a, b, c);
        if strict {
            if o <= EPS {
                return false;
            }
        } else if o < -EPS {
            return false;
        }
        let mut p = self.next(ni);
        while p != pi {
            let q = at(&self.nodes[p]);
            let on_corner = same_place(q, a) || same_place(q, b) || same_place(q, c);
            if !on_corner && in_triangle(a, b, c, q) {
                let (pp, pn) = (at(&self.nodes[self.prev(p)]), at(&self.nodes[self.next(p)]));
                if orient(pp, q, pn) <= EPS {
                    return false;
                }
            }
            p = self.next(p);
        }
        true
    }
}

/// The triangles of a region: the outline and its holes. They go counter-clockwise in the plane
/// of the points. `None` when the region cannot be cut (an outline with fewer than three points,
/// or outlines that cross or touch).
pub fn triangulate(outer: &[Pt], holes: &[Vec<Pt>]) -> Option<Vec<[u32; 3]>> {
    if outer.len() < 3 || holes.iter().any(|h| h.len() < 3) {
        return None;
    }
    let mut ring = Ring {
        nodes: Vec::with_capacity(outer.len() + holes.iter().map(Vec::len).sum::<usize>() + 8),
    };
    let start = ring.add(outer, true);
    let mut queue: Vec<(f64, f64, usize)> = Vec::new();
    for hole in holes {
        let first = ring.add(hole, false);
        let mut left = first;
        for k in 0..hole.len() {
            let (a, b) = (&ring.nodes[first + k], &ring.nodes[left]);
            if a.x < b.x || (a.x == b.x && a.y < b.y) {
                left = first + k;
            }
        }
        queue.push((ring.nodes[left].x, ring.nodes[left].y, left));
    }
    queue.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    for (_, _, h) in queue {
        let m = ring.bridge(h, start)?;
        ring.split(m, h);
    }
    let mut out: Vec<[u32; 3]> = Vec::with_capacity(ring.nodes.len());
    let mut remaining = ring.nodes.len();
    let mut ear = start;
    let mut stop = start;
    let mut strict = true;
    while remaining > 3 {
        if ring.is_ear(ear, strict) {
            let (p, n) = (ring.prev(ear), ring.next(ear));
            out.push([ring.nodes[p].id, ring.nodes[ear].id, ring.nodes[n].id]);
            ring.remove(ear);
            remaining -= 1;
            ear = ring.next(n);
            ear = if ear == p { n } else { ear };
            stop = ear;
            strict = true;
            continue;
        }
        ear = ring.next(ear);
        if ear == stop {
            if strict {
                strict = false;
            } else {
                return None;
            }
        }
    }
    if remaining == 3 {
        let (p, n) = (ring.prev(ear), ring.next(ear));
        out.push([ring.nodes[p].id, ring.nodes[ear].id, ring.nodes[n].id]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn ring(first: u32, pts: &[(f64, f64)]) -> Vec<Pt> {
        pts.iter()
            .enumerate()
            .map(|(i, p)| (first + i as u32, p.0, p.1))
            .collect()
    }

    fn circle(first: u32, cx: f64, cy: f64, r: f64, n: usize) -> Vec<Pt> {
        (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                (first + i as u32, cx + r * a.cos(), cy + r * a.sin())
            })
            .collect()
    }

    fn coordinates(parts: &[&[Pt]]) -> HashMap<u32, (f64, f64)> {
        parts
            .iter()
            .flat_map(|p| p.iter().map(|q| (q.0, (q.1, q.2))))
            .collect()
    }

    /// The area of the triangles, which must all go counter-clockwise, and a check that every
    /// edge of the outlines is used once and every other edge twice, in opposite ways.
    fn check(tris: &[[u32; 3]], parts: &[&[Pt]]) -> f64 {
        let xy = coordinates(parts);
        let mut area = 0.0;
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for t in tris {
            let o = orient(xy[&t[0]], xy[&t[1]], xy[&t[2]]);
            assert!(o >= -1e-9, "a triangle goes clockwise: {t:?} {o}");
            area += o / 2.0;
            for k in 0..3 {
                *edges.entry((t[k], t[(k + 1) % 3])).or_insert(0) += 1;
            }
        }
        let mut boundary: HashMap<(u32, u32), i32> = HashMap::new();
        for p in parts {
            for k in 0..p.len() {
                let (a, b) = (p[k].0, p[(k + 1) % p.len()].0);
                // outline edges are used once, in the way of the ring (either way for a hole)
                boundary.insert((a.min(b), a.max(b)), 1);
            }
        }
        for (&(a, b), &n) in &edges {
            assert_eq!(n, 1, "the edge {a}-{b} is used {n} times the same way");
            let back = edges.get(&(b, a)).copied().unwrap_or(0);
            let is_boundary = boundary.contains_key(&(a.min(b), a.max(b)));
            assert_eq!(back, if is_boundary { 0 } else { 1 }, "the edge {a}-{b}");
        }
        area
    }

    #[test]
    fn a_rectangle_is_two_triangles_with_its_whole_area() {
        let outer = ring(0, &[(0.0, 0.0), (10.0, 0.0), (10.0, 6.0), (0.0, 6.0)]);
        let tris = triangulate(&outer, &[]).unwrap();
        assert_eq!(tris.len(), 2);
        assert!((check(&tris, &[&outer]) - 60.0).abs() < 1e-9);
    }

    #[test]
    fn a_clockwise_outline_gives_the_same_triangles_as_a_counter_clockwise_one() {
        let outer = ring(0, &[(0.0, 0.0), (0.0, 6.0), (10.0, 6.0), (10.0, 0.0)]);
        let tris = triangulate(&outer, &[]).unwrap();
        assert!((check(&tris, &[&outer]) - 60.0).abs() < 1e-9);
    }

    #[test]
    fn a_rectangle_with_a_round_hole_has_the_area_of_the_rest() {
        let outer = ring(0, &[(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (0.0, 30.0)]);
        let hole = circle(100, 20.0, 15.0, 5.0, 32);
        let tris = triangulate(&outer, std::slice::from_ref(&hole)).unwrap();
        // 4 + 32 points and the two ends of the bridge twice: 38 corners, 36 triangles
        assert_eq!(tris.len(), 36);
        let polygon = 0.5 * 32.0 * 25.0 * (std::f64::consts::TAU / 32.0).sin();
        assert!((check(&tris, &[&outer, &hole]) - (1200.0 - polygon)).abs() < 1e-6);
    }

    #[test]
    fn many_holes_in_a_row_and_a_hole_in_the_middle_of_a_run_of_points() {
        // a long outline with points in the middle of its straight sides
        let mut pts = Vec::new();
        for i in 0..=10 {
            pts.push((f64::from(i) * 10.0, 0.0));
        }
        for i in 1..=5 {
            pts.push((100.0, f64::from(i) * 10.0));
        }
        for i in (0..10).rev() {
            pts.push((f64::from(i) * 10.0, 50.0));
        }
        for i in (1..5).rev() {
            pts.push((0.0, f64::from(i) * 10.0));
        }
        let outer = ring(0, &pts);
        let mut holes = Vec::new();
        let mut next = 1000;
        for k in 0..8 {
            holes.push(circle(next, 10.0 + 11.0 * f64::from(k), 25.0, 3.0, 16));
            next += 16;
        }
        let tris = triangulate(&outer, &holes).unwrap();
        let mut parts: Vec<&[Pt]> = vec![&outer];
        parts.extend(holes.iter().map(Vec::as_slice));
        let hole_area = 8.0 * 0.5 * 16.0 * 9.0 * (std::f64::consts::TAU / 16.0).sin();
        assert!((check(&tris, &parts) - (5000.0 - hole_area)).abs() < 1e-6);
        // every point of the outline is used, so no crack is left along the straight sides
        let used: std::collections::HashSet<u32> = tris.iter().flatten().copied().collect();
        assert!(outer.iter().all(|p| used.contains(&p.0)));
    }

    #[test]
    fn a_concave_outline_with_a_hole_is_cut_without_leaving_it() {
        // an L
        let outer = ring(
            0,
            &[
                (0.0, 0.0),
                (30.0, 0.0),
                (30.0, 10.0),
                (10.0, 10.0),
                (10.0, 30.0),
                (0.0, 30.0),
            ],
        );
        let hole = circle(50, 5.0, 20.0, 2.0, 12);
        let tris = triangulate(&outer, std::slice::from_ref(&hole)).unwrap();
        let polygon = 0.5 * 12.0 * 4.0 * (std::f64::consts::TAU / 12.0).sin();
        assert!((check(&tris, &[&outer, &hole]) - (500.0 - polygon)).abs() < 1e-6);
    }

    #[test]
    fn a_star_is_cut_into_its_area() {
        let mut pts = Vec::new();
        for i in 0..10 {
            let a = std::f64::consts::TAU * f64::from(i) / 10.0;
            let r = if i % 2 == 0 { 10.0 } else { 4.0 };
            pts.push((r * a.cos(), r * a.sin()));
        }
        let outer = ring(0, &pts);
        let tris = triangulate(&outer, &[]).unwrap();
        assert_eq!(tris.len(), 8);
        let expected = area2(&outer) / 2.0;
        assert!((check(&tris, &[&outer]) - expected).abs() < 1e-9);
    }

    #[test]
    fn two_holes_side_by_side_and_one_above_the_other() {
        let outer = ring(0, &[(0.0, 0.0), (50.0, 0.0), (50.0, 50.0), (0.0, 50.0)]);
        let holes = vec![
            circle(100, 12.0, 12.0, 5.0, 20),
            circle(200, 38.0, 12.0, 5.0, 20),
            circle(300, 25.0, 38.0, 6.0, 24),
        ];
        let tris = triangulate(&outer, &holes).unwrap();
        let mut parts: Vec<&[Pt]> = vec![&outer];
        parts.extend(holes.iter().map(Vec::as_slice));
        let a = |r: f64, n: f64| 0.5 * n * r * r * (std::f64::consts::TAU / n).sin();
        let expected = 2500.0 - 2.0 * a(5.0, 20.0) - a(6.0, 24.0);
        assert!((check(&tris, &parts) - expected).abs() < 1e-6);
    }

    #[test]
    fn what_cannot_be_cut_is_refused() {
        let outer = ring(0, &[(0.0, 0.0), (1.0, 0.0)]);
        assert!(triangulate(&outer, &[]).is_none());
        let square = ring(0, &[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)]);
        let thin = ring(10, &[(1.0, 1.0), (2.0, 1.0)]);
        assert!(triangulate(&square, &[thin]).is_none());
    }
}
