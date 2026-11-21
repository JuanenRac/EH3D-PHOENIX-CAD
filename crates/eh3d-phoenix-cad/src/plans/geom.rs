// SPDX-License-Identifier: GPL-3.0-or-later
//! The edges of a solid of aligned prisms and their projection on a view, with the visible ones
//! told from the hidden ones.
//!
//! The coordinates of the prisms cut space into a grid; a cell is inside or outside. A line of the
//! grid is an **edge** of the solid where the four cells round it are one inside, three inside, or
//! two inside across a diagonal (two side by side are a flat face). Edges are joined along their
//! line. A view along an axis (front, top, right) finds, for each cell of the picture, how near the
//! first material is; an edge is visible when nothing is nearer than it on either side of it, and
//! a hidden edge that lies on a visible one is left out. The isometric view tests the points of the
//! edges against the prisms along the line of sight, and finds where the answer changes to the
//! nanometre. Everything is integers except the isometric projection, which rounds to the
//! nanometre.

use std::collections::BTreeMap;

use super::model::ViewKind;
use crate::prism::{Prism, Solid};
use crate::CadError;

/// The cells of a solid.
#[derive(Debug, Clone)]
pub struct Grid {
    /// The sorted coordinates of the planes of the grid, per axis.
    pub coords: [Vec<i64>; 3],
    dims: [usize; 3],
    occ: Vec<bool>,
}

fn limit(why: String) -> CadError {
    CadError::PrismLimit(why)
}

impl Grid {
    /// The grid of a solid; refused when it would have more cells than `max_cells`.
    pub fn of(solid: &Solid, max_cells: usize) -> Result<Grid, CadError> {
        let mut coords: [Vec<i64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for (a, axis) in coords.iter_mut().enumerate() {
            for p in solid.prisms() {
                axis.push(p.min()[a]);
                axis.push(p.max()[a]);
            }
        }
        for c in &mut coords {
            c.sort_unstable();
            c.dedup();
        }
        let dims = [
            coords[0].len().saturating_sub(1),
            coords[1].len().saturating_sub(1),
            coords[2].len().saturating_sub(1),
        ];
        let cells = dims[0]
            .checked_mul(dims[1])
            .and_then(|v| v.checked_mul(dims[2]))
            .filter(|v| *v <= max_cells)
            .ok_or_else(|| {
                limit(format!(
                    "the grid of the solid has more than {max_cells} cells"
                ))
            })?;
        let mut grid = Grid {
            coords,
            dims,
            occ: vec![false; cells],
        };
        for p in solid.prisms() {
            let r = |a: usize| -> (usize, usize) {
                let lo = grid.coords[a].binary_search(&p.min()[a]).unwrap_or(0);
                let hi = grid.coords[a].binary_search(&p.max()[a]).unwrap_or(0);
                (lo, hi)
            };
            let (x, y, z) = (r(0), r(1), r(2));
            for k in z.0..z.1 {
                for j in y.0..y.1 {
                    for i in x.0..x.1 {
                        let at = (k * dims[1] + j) * dims[0] + i;
                        grid.occ[at] = true;
                    }
                }
            }
        }
        Ok(grid)
    }

    /// Whether the cell is inside; a cell outside the grid is not.
    pub fn at(&self, i: isize, j: isize, k: isize) -> bool {
        let (Ok(i), Ok(j), Ok(k)) = (usize::try_from(i), usize::try_from(j), usize::try_from(k))
        else {
            return false;
        };
        if i >= self.dims[0] || j >= self.dims[1] || k >= self.dims[2] {
            return false;
        }
        self.occ[(k * self.dims[1] + j) * self.dims[0] + i]
    }

    /// The cells along each axis.
    pub fn dims(&self) -> [usize; 3] {
        self.dims
    }
}

/// An edge of the solid: a straight run along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge3 {
    /// The axis it runs along (0, 1 or 2).
    pub axis: usize,
    /// Its low end.
    pub from: [i64; 3],
    /// Its high end.
    pub to: [i64; 3],
}

/// A segment of a view in the plane of the picture, in nanometres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Seg {
    /// One end.
    pub a: (i64, i64),
    /// The other end.
    pub b: (i64, i64),
    /// Whether the edge is hidden behind the solid.
    pub hidden: bool,
}

/// The edges of the solid, joined along their lines, in a fixed order.
pub fn edges3(grid: &Grid, max_edges: usize) -> Result<Vec<Edge3>, CadError> {
    let mut out = Vec::new();
    let d = grid.dims;
    for a in 0..3 {
        let (b, c) = ((a + 1) % 3, (a + 2) % 3);
        for lb in 0..=d[b] {
            for lc in 0..=d[c] {
                let mut start: Option<usize> = None;
                for i in 0..=d[a] {
                    let flag = i < d[a] && {
                        let cell = |db: usize, dc: usize| -> bool {
                            let mut pos = [0isize; 3];
                            pos[a] = i as isize;
                            pos[b] = lb as isize - 1 + db as isize;
                            pos[c] = lc as isize - 1 + dc as isize;
                            grid.at(pos[0], pos[1], pos[2])
                        };
                        let (q00, q10, q01, q11) = (cell(0, 0), cell(1, 0), cell(0, 1), cell(1, 1));
                        let n = [q00, q10, q01, q11].iter().filter(|x| **x).count();
                        n == 1 || n == 3 || (n == 2 && q00 == q11)
                    };
                    match (flag, start) {
                        (true, None) => start = Some(i),
                        (false, Some(s)) => {
                            let mut from = [0i64; 3];
                            from[a] = grid.coords[a][s];
                            from[b] = grid.coords[b][lb];
                            from[c] = grid.coords[c][lc];
                            let mut to = from;
                            to[a] = grid.coords[a][i];
                            out.push(Edge3 { axis: a, from, to });
                            if out.len() > max_edges {
                                return Err(limit(format!(
                                    "the solid has more than {max_edges} edges"
                                )));
                            }
                            start = None;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The axes of a view along an axis: the one across the picture, the one up it, the one into it,
/// and the sign that makes a smaller depth mean nearer.
fn axes_of(kind: ViewKind) -> (usize, usize, usize, i64) {
    match kind {
        ViewKind::Top => (0, 1, 2, -1),
        ViewKind::Right => (1, 2, 0, -1),
        _ => (0, 2, 1, 1),
    }
}

/// The intervals of a list joined where they touch or overlap.
fn join(mut v: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
    v.sort_unstable();
    let mut out: Vec<(i64, i64)> = Vec::new();
    for (lo, hi) in v {
        match out.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    out
}

/// The parts of the intervals `a` that `b` (joined) does not cover.
fn minus(a: &[(i64, i64)], b: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    for &(lo, hi) in a {
        let mut at = lo;
        for &(blo, bhi) in b {
            if bhi <= at || blo >= hi {
                continue;
            }
            if blo > at {
                out.push((at, blo));
            }
            at = at.max(bhi);
        }
        if at < hi {
            out.push((at, hi));
        }
    }
    out
}

/// The visible and the hidden stretches of one line of a view.
type Spans = (Vec<(i64, i64)>, Vec<(i64, i64)>);

/// Puts the segments of a view in order, drops a hidden part that lies on a visible segment and,
/// when the hidden edges are not wanted, every hidden one. Only axis-aligned segments are joined.
fn tidy(segs: Vec<Seg>, show_hidden: bool) -> Vec<Seg> {
    let mut lines: BTreeMap<(bool, i64), Spans> = BTreeMap::new();
    let mut other: Vec<Seg> = Vec::new();
    for s in segs {
        let (a, b) = if s.a <= s.b { (s.a, s.b) } else { (s.b, s.a) };
        if a == b {
            continue;
        }
        let key = if a.1 == b.1 {
            Some((false, a.1, a.0, b.0))
        } else if a.0 == b.0 {
            Some((true, a.0, a.1, b.1))
        } else {
            None
        };
        match key {
            Some((vertical, fixed, lo, hi)) => {
                let e = lines.entry((vertical, fixed)).or_default();
                if s.hidden {
                    e.1.push((lo, hi));
                } else {
                    e.0.push((lo, hi));
                }
            }
            None => other.push(Seg {
                a,
                b,
                hidden: s.hidden,
            }),
        }
    }
    let mut out = Vec::new();
    for ((vertical, fixed), (vis, hid)) in lines {
        let vis = join(vis);
        let hid = join(minus(&join(hid), &vis));
        let make = |lo: i64, hi: i64, hidden: bool| Seg {
            a: if vertical { (fixed, lo) } else { (lo, fixed) },
            b: if vertical { (fixed, hi) } else { (hi, fixed) },
            hidden,
        };
        out.extend(vis.into_iter().map(|(l, h)| make(l, h, false)));
        if show_hidden {
            out.extend(hid.into_iter().map(|(l, h)| make(l, h, true)));
        }
    }
    other.retain(|s| show_hidden || !s.hidden);
    other.sort_unstable();
    other.dedup();
    out.extend(other);
    out.sort_unstable();
    out
}

/// The segments of the solid seen along one axis: the front, the top or the right.
pub fn ortho_segments(grid: &Grid, edges: &[Edge3], kind: ViewKind, show_hidden: bool) -> Vec<Seg> {
    let (u, v, w, sign) = axes_of(kind);
    let (nu, nv, nw) = (grid.dims[u], grid.dims[v], grid.dims[w]);
    let mut front = vec![i64::MAX; nu * nv];
    for iv in 0..nv {
        for iu in 0..nu {
            let cell = |k: usize| {
                let mut pos = [0isize; 3];
                pos[u] = iu as isize;
                pos[v] = iv as isize;
                pos[w] = k as isize;
                grid.at(pos[0], pos[1], pos[2])
            };
            let found = if sign > 0 {
                (0..nw).find(|k| cell(*k)).map(|k| grid.coords[w][k])
            } else {
                (0..nw)
                    .rev()
                    .find(|k| cell(*k))
                    .map(|k| -grid.coords[w][k + 1])
            };
            if let Some(d) = found {
                front[iv * nu + iu] = d;
            }
        }
    }
    let at = |iu: isize, iv: isize| -> i64 {
        if iu < 0 || iv < 0 || iu as usize >= nu || iv as usize >= nv {
            i64::MAX
        } else {
            front[iv as usize * nu + iu as usize]
        }
    };
    let index = |axis: usize, value: i64| grid.coords[axis].binary_search(&value).unwrap_or(0);
    let mut segs = Vec::new();
    for e in edges.iter().filter(|e| e.axis != w) {
        let depth = sign * e.from[w];
        let along = e.axis;
        let across = if along == u { v } else { u };
        let line = index(across, e.from[across]) as isize;
        let (i0, i1) = (index(along, e.from[along]), index(along, e.to[along]));
        let mut run: Option<(usize, bool)> = None;
        let flush = |segs: &mut Vec<Seg>, from: usize, to: usize, hidden: bool| {
            let (a, b) = (grid.coords[along][from], grid.coords[along][to]);
            let fixed = e.from[across];
            let (pa, pb) = if along == u {
                ((a, fixed), (b, fixed))
            } else {
                ((fixed, a), (fixed, b))
            };
            segs.push(Seg {
                a: pa,
                b: pb,
                hidden,
            });
        };
        for i in i0..i1 {
            let (fa, fb) = if along == u {
                (at(i as isize, line - 1), at(i as isize, line))
            } else {
                (at(line - 1, i as isize), at(line, i as isize))
            };
            let hidden = depth > fa.min(fb);
            match run {
                Some((_, h)) if h == hidden => {}
                Some((s, h)) => {
                    flush(&mut segs, s, i, h);
                    run = Some((i, hidden));
                }
                None => run = Some((i, hidden)),
            }
        }
        if let Some((s, h)) = run {
            flush(&mut segs, s, i1, h);
        }
    }
    tidy(segs, show_hidden)
}

/// The cosine of 30 degrees.
const COS30: f64 = 0.866_025_403_784_438_6;

/// The isometric picture of a point: `x` and `y` go to the right (down and up), `z` goes up.
pub fn iso_point(p: [i64; 3]) -> (i64, i64) {
    let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
    (
        ((x + y) * COS30).round() as i64,
        (z + (y - x) * 0.5).round() as i64,
    )
}

/// Whether the line of sight from a point (moved a little towards the viewer) goes through
/// material. The viewer is at the front, the right and above: the direction is (1, -1, 1).
fn blocked(p: [f64; 3], prisms: &[Prism]) -> bool {
    const EPS: f64 = 0.25;
    let w = [1.0, -1.0, 1.0];
    let s = [p[0] + EPS * w[0], p[1] + EPS * w[1], p[2] + EPS * w[2]];
    prisms.iter().any(|b| {
        let (mut t_in, mut t_out) = (0.0f64, f64::INFINITY);
        for i in 0..3 {
            let (lo, hi) = (b.min()[i] as f64, b.max()[i] as f64);
            let (a, c) = if w[i] > 0.0 {
                (lo - s[i], hi - s[i])
            } else {
                (s[i] - hi, s[i] - lo)
            };
            t_in = t_in.max(a);
            t_out = t_out.min(c);
        }
        t_out - t_in > 1e-6
    })
}

/// The segments of the solid in the isometric view. `max_tests` bounds the work: an edge is tested
/// at several points against every prism.
pub fn iso_segments(
    grid: &Grid,
    solid: &Solid,
    edges: &[Edge3],
    show_hidden: bool,
    max_tests: u64,
) -> Result<Vec<Seg>, CadError> {
    let prisms = solid.prisms();
    let mut tests: u64 = 0;
    let mut segs = Vec::new();
    for e in edges {
        let a = e.axis;
        let mut cuts: Vec<i64> = vec![e.from[a]];
        cuts.extend(
            grid.coords[a]
                .iter()
                .copied()
                .filter(|c| *c > e.from[a] && *c < e.to[a]),
        );
        cuts.push(e.to[a]);
        let mut states: Vec<(i64, i64, bool)> = Vec::new();
        for pair in cuts.windows(2) {
            let (p0, p1) = (pair[0], pair[1]);
            let at = |pos: i64| -> [f64; 3] {
                let mut q = e.from;
                q[a] = pos;
                [q[0] as f64, q[1] as f64, q[2] as f64]
            };
            let mut state_at = |pos: i64| -> Result<bool, CadError> {
                tests = tests.saturating_add(prisms.len() as u64);
                if tests > max_tests {
                    return Err(limit(format!(
                        "an isometric view needs more than {max_tests} tests of an edge against a prism"
                    )));
                }
                Ok(blocked(at(pos), prisms))
            };
            let span = i128::from(p1 - p0);
            let pos_k = |k: i128| p0 + (span * k / 8) as i64;
            let samples: Vec<(i64, bool)> = [1i128, 3, 5, 7]
                .iter()
                .map(|k| {
                    let pos = pos_k(*k);
                    state_at(pos).map(|s| (pos, s))
                })
                .collect::<Result<_, _>>()?;
            let mut start = p0;
            let mut cur = samples[0].1;
            for w in samples.windows(2) {
                if w[0].1 != w[1].1 {
                    let (mut lo, mut hi) = (w[0].0, w[1].0);
                    while hi - lo > 1 {
                        let mid = lo + (hi - lo) / 2;
                        if state_at(mid)? == w[0].1 {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    states.push((start, hi, cur));
                    start = hi;
                    cur = w[1].1;
                }
            }
            states.push((start, p1, cur));
        }
        // join the pieces that follow one another with the same answer
        let mut joined: Vec<(i64, i64, bool)> = Vec::new();
        for s in states {
            match joined.last_mut() {
                Some(l) if l.2 == s.2 && l.1 == s.0 => l.1 = s.1,
                _ => joined.push(s),
            }
        }
        for (p0, p1, hidden) in joined {
            if p0 == p1 {
                continue;
            }
            let mut q0 = e.from;
            q0[a] = p0;
            let mut q1 = e.from;
            q1[a] = p1;
            segs.push(Seg {
                a: iso_point(q0),
                b: iso_point(q1),
                hidden,
            });
        }
    }
    Ok(tidy(segs, show_hidden))
}

/// The segments of a solid in a view of the kinds that look at a solid.
pub fn view_segments(
    solid: &Solid,
    kind: ViewKind,
    show_hidden: bool,
    max_cells: usize,
    max_edges: usize,
    max_iso_tests: u64,
) -> Result<Vec<Seg>, CadError> {
    let grid = Grid::of(solid, max_cells)?;
    let edges = edges3(&grid, max_edges)?;
    match kind {
        ViewKind::Iso => iso_segments(&grid, solid, &edges, show_hidden, max_iso_tests),
        ViewKind::Sketch => Ok(Vec::new()),
        k => Ok(ortho_segments(&grid, &edges, k, show_hidden)),
    }
}

/// The box that holds a list of segments: the lowest and the highest corner, or `None`.
pub fn bounds_of(segs: &[Seg]) -> Option<((i64, i64), (i64, i64))> {
    let mut it = segs.iter().flat_map(|s| [s.a, s.b]);
    let first = it.next()?;
    let (mut lo, mut hi) = (first, first);
    for p in it {
        lo = (lo.0.min(p.0), lo.1.min(p.1));
        hi = (hi.0.max(p.0), hi.1.max(p.1));
    }
    Some((lo, hi))
}
