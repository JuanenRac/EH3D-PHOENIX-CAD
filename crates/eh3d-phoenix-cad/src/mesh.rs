//! Triangles: the surface of a solid as a list of vertices and of triangles of three vertices, built
//! piece by piece, checked to be closed, measured and written as an STL file.
//!
//! A [`Builder`] makes the surface of a solid out of flat regions (an outline with holes in it,
//! cut into triangles by [`crate::triangulate`]) and strips between two outlines of the same
//! number of points (the wall of a hole, of a post, of a fin). Every vertex is kept once, so two
//! pieces that meet at a point share it, and the check that follows can tell an open edge from a
//! closed one: in a closed surface every edge is used by exactly two triangles, one in each way.
//!
//! Lengths are nanometres in `f64` (every whole nanometre up to 2^53 is exact, far above the range
//! of the model). The circles are drawn by [`Circle`] with the sine and the cosine of a series made
//! of sums and products only, so the same model gives the same bytes on every computer.

use crate::triangulate::{self, Pt};
use crate::CadError;
use std::collections::HashMap;
use std::fmt::Write as _;

/// A point in nanometres.
pub type P3 = [f64; 3];

const NM_PER_MM: f64 = 1_000_000.0;

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

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The surface of a solid.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    /// The vertices, in nanometres.
    pub vertices: Vec<P3>,
    /// The triangles as three vertices each, counter-clockwise as seen from outside the solid.
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    fn corners(&self, t: &[u32; 3]) -> [P3; 3] {
        [
            self.vertices[t[0] as usize],
            self.vertices[t[1] as usize],
            self.vertices[t[2] as usize],
        ]
    }

    /// The volume in cubic nanometres: the sum of the signed tetrahedra of the triangles with a
    /// corner of the mesh, which is exact for a closed surface.
    pub fn volume_nm3(&self) -> f64 {
        let Some(&origin) = self.vertices.first() else {
            return 0.0;
        };
        let mut total = 0.0;
        for t in &self.triangles {
            let [a, b, c] = self.corners(t);
            total += dot(sub(a, origin), cross(sub(b, origin), sub(c, origin)));
        }
        total / 6.0
    }

    /// The unit normal of a triangle (zero for a triangle with no area).
    pub fn normal(&self, t: &[u32; 3]) -> P3 {
        let [a, b, c] = self.corners(t);
        let n = cross(sub(b, a), sub(c, a));
        let len = dot(n, n).sqrt();
        if len > 0.0 {
            [n[0] / len, n[1] / len, n[2] / len]
        } else {
            [0.0; 3]
        }
    }

    /// The area of a triangle in square nanometres.
    pub fn area_nm2(&self, t: &[u32; 3]) -> f64 {
        let [a, b, c] = self.corners(t);
        let n = cross(sub(b, a), sub(c, a));
        dot(n, n).sqrt() / 2.0
    }

    /// The smallest and the largest corner, or `None` for a mesh with no vertex.
    pub fn bounds(&self) -> Option<(P3, P3)> {
        let first = *self.vertices.first()?;
        let mut lo = first;
        let mut hi = first;
        for v in &self.vertices {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        Some((lo, hi))
    }

    /// How many triangles have no area.
    pub fn flat_triangles(&self) -> usize {
        self.triangles
            .iter()
            .filter(|t| self.area_nm2(t) <= 1.0)
            .count()
    }

    /// Checks that the surface is closed: no triangle uses a vertex twice, every edge is used by
    /// exactly one triangle in each way, and so by exactly two triangles in all. The answer says
    /// how many edges are open and how many are used more than twice.
    pub fn check_closed(&self) -> Result<(), String> {
        let mut edges: HashMap<(u32, u32), u32> = HashMap::with_capacity(self.triangles.len() * 3);
        for t in &self.triangles {
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                return Err("a triangle uses a vertex twice".to_owned());
            }
            for k in 0..3 {
                *edges.entry((t[k], t[(k + 1) % 3])).or_insert(0) += 1;
            }
        }
        let mut open = 0usize;
        let mut many = 0usize;
        for (&(a, b), &n) in &edges {
            if n != 1 {
                many += 1;
            } else if edges.get(&(b, a)) != Some(&1) {
                open += 1;
            }
        }
        if open == 0 && many == 0 {
            Ok(())
        } else {
            Err(format!(
                "{open} open edges and {many} edges used more than once the same way"
            ))
        }
    }

    /// The mesh with every vertex moved by `f`.
    pub fn mapped(&self, f: impl Fn(P3) -> P3) -> Mesh {
        Mesh {
            vertices: self.vertices.iter().map(|v| f(*v)).collect(),
            triangles: self.triangles.clone(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Circles
// ---------------------------------------------------------------------------------------------

/// The sine and the cosine of an angle from 0 to a quarter turn, by their series (sums and
/// products only, so every computer gets the same bits).
fn sin_cos_quarter(theta: f64) -> (f64, f64) {
    let t2 = theta * theta;
    let mut sin = theta;
    let mut cos = 1.0;
    let mut s_term = theta;
    let mut c_term = 1.0;
    for k in 1..=14u32 {
        let (a, b) = (f64::from(2 * k - 1), f64::from(2 * k));
        c_term *= -t2 / (a * b);
        cos += c_term;
        s_term *= -t2 / (b * (b + 1.0));
        sin += s_term;
    }
    (sin, cos)
}

/// The points of a circle of `n` sides (a multiple of four), shared by every circle of a solid.
#[derive(Debug, Clone)]
pub struct Circle {
    n: usize,
    unit: Vec<(f64, f64)>,
    scale: f64,
}

impl Circle {
    /// A table for circles of `n` sides: `n` is a multiple of four and at least eight.
    pub fn new(n: u32) -> Circle {
        let n = ((n.max(8) / 4) * 4) as usize;
        let quarter = n / 4;
        let mut unit = Vec::with_capacity(n);
        for k in 0..n {
            let (q, r) = (k / quarter, k % quarter);
            let theta = std::f64::consts::TAU * r as f64 / n as f64;
            let (s, c) = sin_cos_quarter(theta);
            unit.push(match q {
                0 => (c, s),
                1 => (-s, c),
                2 => (-c, -s),
                _ => (s, -c),
            });
        }
        let (side, _) = sin_cos_quarter(std::f64::consts::TAU / n as f64);
        // the radius that gives the polygon the area of the circle
        let scale = (std::f64::consts::TAU / (n as f64 * side)).sqrt();
        Circle { n, unit, scale }
    }

    /// The sides of a circle.
    pub fn sides(&self) -> usize {
        self.n
    }

    /// The point of the circle at the side `k` (the quarter turns are exact), counted from the
    /// positive `x` axis, going counter-clockwise; `k` goes round.
    pub fn at(&self, k: usize) -> (f64, f64) {
        self.unit[k % self.n]
    }

    /// The radius of the polygon that has the area of the circle of radius `r`.
    pub fn radius(&self, r: f64) -> f64 {
        r * self.scale
    }

    /// The polygon of a circle of radius `r` with the same area as the circle.
    pub fn polygon(&self, cx: f64, cy: f64, r: f64) -> Vec<(f64, f64)> {
        let rr = r * self.scale;
        self.unit
            .iter()
            .map(|(c, s)| (cx + rr * c, cy + rr * s))
            .collect()
    }

    /// The points of an arc about `(cx, cy)` from the side `from` to the side `to` (both ends
    /// included, going counter-clockwise) at the radius `r` exactly.
    pub fn arc(&self, cx: f64, cy: f64, r: f64, from: usize, to: usize) -> Vec<(f64, f64)> {
        (from..=to)
            .map(|k| {
                let (c, s) = self.at(k);
                (cx + r * c, cy + r * s)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------------------------
// The builder
// ---------------------------------------------------------------------------------------------

/// What an outline is in a flat region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The edge of the surface: material inside.
    Boundary,
    /// An opening in the surface: no surface inside (the mouth of a hole or the foot of a post).
    Cut,
    /// A piece of surface inside an opening: the island of a pocket, the inside of a lip.
    Island,
}

/// An outline in a flat region, with the vertices it goes through and a name for the messages.
#[derive(Debug, Clone)]
pub struct Loop {
    /// The vertices, in order round the outline.
    pub ids: Vec<u32>,
    /// What it is in the region.
    pub role: Role,
    /// Said in the message when it does not fit.
    pub label: String,
}

impl Loop {
    /// An outline.
    pub fn new(ids: Vec<u32>, role: Role, label: &str) -> Loop {
        Loop {
            ids,
            role,
            label: label.to_owned(),
        }
    }
}

fn shape_error(why: String) -> CadError {
    CadError::MeshShape(why)
}

fn orient(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

fn on_segment(a: (f64, f64), b: (f64, f64), p: (f64, f64)) -> bool {
    p.0 >= a.0.min(b.0) - 1e-12
        && p.0 <= a.0.max(b.0) + 1e-12
        && p.1 >= a.1.min(b.1) - 1e-12
        && p.1 <= a.1.max(b.1) + 1e-12
}

/// Whether two segments cross or touch.
fn segments_meet(p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), p4: (f64, f64)) -> bool {
    let near = |d: f64| if d.abs() <= 1e-12 { 0.0 } else { d };
    let d1 = near(orient(p3, p4, p1));
    let d2 = near(orient(p3, p4, p2));
    let d3 = near(orient(p1, p2, p3));
    let d4 = near(orient(p1, p2, p4));
    if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
        return true;
    }
    (d1 == 0.0 && on_segment(p3, p4, p1))
        || (d2 == 0.0 && on_segment(p3, p4, p2))
        || (d3 == 0.0 && on_segment(p1, p2, p3))
        || (d4 == 0.0 && on_segment(p1, p2, p4))
}

fn inside_polygon(poly: &[Pt], p: (f64, f64)) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.2 > p.1) != (b.2 > p.1) && p.0 < (b.1 - a.1) * (p.1 - a.2) / (b.2 - a.2) + a.1 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn box_of(poly: &[Pt]) -> (f64, f64, f64, f64) {
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in poly {
        b = (b.0.min(p.1), b.1.min(p.2), b.2.max(p.1), b.3.max(p.2));
    }
    b
}

/// Makes the vertices and triangles of a surface.
#[derive(Debug)]
pub struct Builder {
    vertices: Vec<P3>,
    lookup: HashMap<[u64; 3], u32>,
    triangles: Vec<[u32; 3]>,
    limit: usize,
}

impl Builder {
    /// A builder that refuses more than `limit` triangles.
    pub fn new(limit: usize) -> Builder {
        Builder {
            vertices: Vec::new(),
            lookup: HashMap::new(),
            triangles: Vec::new(),
            limit,
        }
    }

    /// The number of the vertex at a place; the same place always gives the same vertex.
    pub fn vertex(&mut self, p: P3) -> u32 {
        // adding zero turns -0.0 into 0.0
        let key = [
            (p[0] + 0.0).to_bits(),
            (p[1] + 0.0).to_bits(),
            (p[2] + 0.0).to_bits(),
        ];
        if let Some(&id) = self.lookup.get(&key) {
            return id;
        }
        let id = self.vertices.len() as u32;
        self.vertices.push([p[0] + 0.0, p[1] + 0.0, p[2] + 0.0]);
        self.lookup.insert(key, id);
        id
    }

    /// The vertices of a list of places.
    pub fn ring(&mut self, pts: &[P3]) -> Vec<u32> {
        pts.iter().map(|p| self.vertex(*p)).collect()
    }

    /// The place of a vertex.
    pub fn at(&self, id: u32) -> P3 {
        self.vertices[id as usize]
    }

    /// How many triangles there are so far.
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    fn push(&mut self, t: [u32; 3]) -> Result<(), CadError> {
        if self.triangles.len() >= self.limit {
            return Err(CadError::ExportLimit(format!(
                "more than the {} triangles that the settings allow",
                self.limit
            )));
        }
        self.triangles.push(t);
        Ok(())
    }

    /// One triangle, as given.
    pub fn triangle(&mut self, a: u32, b: u32, c: u32) -> Result<(), CadError> {
        self.push([a, b, c])
    }

    /// The wall between two outlines of the same number of points: for every pair of neighbours a
    /// quad of two triangles. `solid_inside` says whether the material is inside the outlines (a
    /// post) or the space is (a hole); the triangles face away from the material.
    pub fn strip(&mut self, a: &[u32], b: &[u32], solid_inside: bool) -> Result<(), CadError> {
        if a.len() != b.len() || a.len() < 3 {
            return Err(shape_error(
                "the two outlines of a wall do not have the same points".to_owned(),
            ));
        }
        let pa: Vec<P3> = a.iter().map(|&i| self.at(i)).collect();
        let pb: Vec<P3> = b.iter().map(|&i| self.at(i)).collect();
        // the normal of the first outline (Newell) and the way to the second
        let mut n = [0.0; 3];
        for i in 0..pa.len() {
            let (p, q) = (pa[i], pa[(i + 1) % pa.len()]);
            n[0] += (p[1] - q[1]) * (p[2] + q[2]);
            n[1] += (p[2] - q[2]) * (p[0] + q[0]);
            n[2] += (p[0] - q[0]) * (p[1] + q[1]);
        }
        let centre = |ps: &[P3]| {
            let mut c = [0.0; 3];
            for p in ps {
                for k in 0..3 {
                    c[k] += p[k] / ps.len() as f64;
                }
            }
            c
        };
        let away = dot(n, sub(centre(&pb), centre(&pa))) > 0.0;
        let keep = away == solid_inside;
        for i in 0..a.len() {
            let j = (i + 1) % a.len();
            let (a0, a1, b0, b1) = (a[i], a[j], b[i], b[j]);
            if keep {
                self.push([a0, a1, b1])?;
                self.push([a0, b1, b0])?;
            } else {
                self.push([a0, b1, a1])?;
                self.push([a0, b0, b1])?;
            }
        }
        Ok(())
    }

    /// A flat region: the outlines in one plane, cut into triangles that face `normal`. The
    /// outlines must not cross or touch; each must be where its role says (a boundary or an island
    /// at an even depth of nesting, an opening at an odd one).
    pub fn plane(&mut self, loops: &[Loop], normal: P3) -> Result<(), CadError> {
        if loops.is_empty() {
            return Ok(());
        }
        let k = (0..3)
            .max_by(|&i, &j| normal[i].abs().total_cmp(&normal[j].abs()))
            .unwrap_or(2);
        let (ia, ib) = ((k + 1) % 3, (k + 2) % 3);
        let polys: Vec<Vec<Pt>> = loops
            .iter()
            .map(|l| {
                l.ids
                    .iter()
                    .map(|&id| {
                        let v = self.vertices[id as usize];
                        (id, v[ia] / NM_PER_MM, v[ib] / NM_PER_MM)
                    })
                    .collect()
            })
            .collect();
        let boxes: Vec<(f64, f64, f64, f64)> = polys.iter().map(|p| box_of(p)).collect();
        for i in 0..loops.len() {
            if loops[i].ids.len() < 3 {
                return Err(shape_error(format!(
                    "{} has fewer than three points",
                    loops[i].label
                )));
            }
            for j in (i + 1)..loops.len() {
                let (a, b) = (boxes[i], boxes[j]);
                if a.0 > b.2 + 1e-9 || b.0 > a.2 + 1e-9 || a.1 > b.3 + 1e-9 || b.1 > a.3 + 1e-9 {
                    continue;
                }
                let (pi, pj) = (&polys[i], &polys[j]);
                let meet = (0..pi.len()).any(|s| {
                    let (p1, p2) = (pi[s], pi[(s + 1) % pi.len()]);
                    (0..pj.len()).any(|t| {
                        let (p3, p4) = (pj[t], pj[(t + 1) % pj.len()]);
                        segments_meet((p1.1, p1.2), (p2.1, p2.2), (p3.1, p3.2), (p4.1, p4.2))
                    })
                });
                if meet {
                    return Err(shape_error(format!(
                        "{} and {} cross or touch",
                        loops[i].label, loops[j].label
                    )));
                }
            }
        }
        // how many outlines each one lies inside, and the one it lies directly inside
        let n = loops.len();
        let mut depth = vec![0usize; n];
        let mut containers: Vec<Vec<usize>> = vec![Vec::new(); n];
        for i in 0..n {
            let probe = (polys[i][0].1, polys[i][0].2);
            for (j, other) in polys.iter().enumerate() {
                if i != j && inside_polygon(other, probe) {
                    depth[i] += 1;
                    containers[i].push(j);
                }
            }
        }
        for i in 0..n {
            let odd = depth[i] % 2 == 1;
            let wants_odd = loops[i].role == Role::Cut;
            if odd != wants_odd {
                return Err(shape_error(format!(
                    "{} does not lie where its shape can be made: it is outside the surface or inside another shape",
                    loops[i].label
                )));
            }
        }
        let flip = normal[k] < 0.0;
        for e in 0..n {
            if depth[e] % 2 == 1 {
                continue;
            }
            let holes: Vec<Vec<Pt>> = (0..n)
                .filter(|&h| {
                    depth[h] % 2 == 1
                        && containers[h]
                            .iter()
                            .copied()
                            .max_by_key(|&c| depth[c])
                            .is_some_and(|c| c == e)
                })
                .map(|h| polys[h].clone())
                .collect();
            let tris = triangulate::triangulate(&polys[e], &holes).ok_or_else(|| {
                shape_error(format!("the surface of {} cannot be cut", loops[e].label))
            })?;
            for t in tris {
                self.push(if flip { [t[0], t[2], t[1]] } else { t })?;
            }
        }
        Ok(())
    }

    /// The mesh.
    pub fn finish(self) -> Mesh {
        Mesh {
            vertices: self.vertices,
            triangles: self.triangles,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// STL
// ---------------------------------------------------------------------------------------------

/// A name that fits on a line of a text STL file.
fn solid_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "solid".to_owned()
    } else {
        cleaned
    }
}

fn mm(nm: f64) -> f64 {
    nm / NM_PER_MM
}

/// A number of the text file: millimetres with six decimals (one nanometre), never `-0`.
fn number(v: f64) -> String {
    let text = format!("{v:.6}");
    if text.starts_with('-') && text[1..].bytes().all(|b| b == b'0' || b == b'.') {
        text[1..].to_owned()
    } else {
        text
    }
}

/// The binary STL file of some solids: an 80-byte header, the number of triangles and for every
/// triangle its normal, its three corners (32-bit floats, millimetres) and two bytes of zero. The
/// solids are written one after the other.
pub fn stl_binary(bodies: &[(&str, &Mesh)]) -> Vec<u8> {
    let count: usize = bodies.iter().map(|(_, m)| m.triangles.len()).sum();
    let mut out = Vec::with_capacity(84 + 50 * count);
    let header = b"EH3D PHOENIX-CAD binary STL, millimetres";
    out.extend_from_slice(header);
    out.resize(80, b' ');
    out.extend_from_slice(&(count as u32).to_le_bytes());
    for (_, mesh) in bodies {
        for t in &mesh.triangles {
            let n = mesh.normal(t);
            for v in n {
                out.extend_from_slice(&(v as f32).to_le_bytes());
            }
            for corner in mesh.corners(t) {
                for v in corner {
                    out.extend_from_slice(&(mm(v) as f32).to_le_bytes());
                }
            }
            out.extend_from_slice(&[0, 0]);
        }
    }
    out
}

/// The text STL file of some solids, one `solid` for each, with millimetres to six decimals.
pub fn stl_ascii(bodies: &[(&str, &Mesh)]) -> String {
    let mut out = String::new();
    for (name, mesh) in bodies {
        let name = solid_name(name);
        let _ = writeln!(out, "solid {name}");
        for t in &mesh.triangles {
            let n = mesh.normal(t);
            let _ = writeln!(
                out,
                "  facet normal {} {} {}",
                number(n[0]),
                number(n[1]),
                number(n[2])
            );
            out.push_str("    outer loop\n");
            for corner in mesh.corners(t) {
                let _ = writeln!(
                    out,
                    "      vertex {} {} {}",
                    number(mm(corner[0])),
                    number(mm(corner[1])),
                    number(mm(corner[2]))
                );
            }
            out.push_str("    endloop\n  endfacet\n");
        }
        let _ = writeln!(out, "endsolid {name}");
    }
    out
}

/// What a text or binary STL file holds, read back: the triangles as nine millimetres each.
/// Used by the tests and by anyone who wants to check a file this program wrote.
pub fn read_stl(bytes: &[u8]) -> Result<Vec<[f64; 9]>, String> {
    let is_text = bytes.starts_with(b"solid") && bytes.windows(5).any(|w| w == b"facet");
    if is_text {
        let text = std::str::from_utf8(bytes).map_err(|_| "the text is not UTF-8".to_owned())?;
        let mut tris = Vec::new();
        let mut current: Vec<f64> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("vertex ") {
                for w in rest.split_whitespace() {
                    current.push(w.parse::<f64>().map_err(|_| format!("bad number {w}"))?);
                }
                if current.len() == 9 {
                    let mut t = [0.0; 9];
                    t.copy_from_slice(&current);
                    tris.push(t);
                    current.clear();
                }
            }
        }
        if !current.is_empty() {
            return Err("a facet has fewer than three vertices".to_owned());
        }
        return Ok(tris);
    }
    if bytes.len() < 84 {
        return Err("the file is shorter than the header".to_owned());
    }
    let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    if bytes.len() != 84 + 50 * count {
        return Err(format!(
            "the file has {} bytes and {count} triangles need {}",
            bytes.len(),
            84 + 50 * count
        ));
    }
    let f = |at: usize| {
        f64::from(f32::from_le_bytes([
            bytes[at],
            bytes[at + 1],
            bytes[at + 2],
            bytes[at + 3],
        ]))
    };
    let mut tris = Vec::with_capacity(count);
    for i in 0..count {
        let base = 84 + 50 * i + 12;
        let mut t = [0.0; 9];
        for (k, slot) in t.iter_mut().enumerate() {
            *slot = f(base + 4 * k);
        }
        tris.push(t);
    }
    Ok(tris)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box of the given size at the origin as a mesh made of two triangles for each face.
    fn cuboid(x: f64, y: f64, z: f64) -> Mesh {
        let mut b = Builder::new(1000);
        let v = |b: &mut Builder, a: f64, c: f64, d: f64| b.vertex([a, c, d]);
        let bottom = [
            v(&mut b, 0.0, 0.0, 0.0),
            v(&mut b, x, 0.0, 0.0),
            v(&mut b, x, y, 0.0),
            v(&mut b, 0.0, y, 0.0),
        ];
        let top = [
            v(&mut b, 0.0, 0.0, z),
            v(&mut b, x, 0.0, z),
            v(&mut b, x, y, z),
            v(&mut b, 0.0, y, z),
        ];
        b.plane(
            &[Loop::new(bottom.to_vec(), Role::Boundary, "bottom")],
            [0.0, 0.0, -1.0],
        )
        .unwrap();
        b.plane(
            &[Loop::new(top.to_vec(), Role::Boundary, "top")],
            [0.0, 0.0, 1.0],
        )
        .unwrap();
        b.strip(&bottom, &top, true).unwrap();
        b.finish()
    }

    #[test]
    fn a_box_of_triangles_is_closed_and_has_the_volume_of_the_box() {
        let m = cuboid(10_000_000.0, 20_000_000.0, 30_000_000.0);
        assert_eq!(m.triangles.len(), 12);
        assert_eq!(m.vertices.len(), 8);
        m.check_closed().unwrap();
        assert!((m.volume_nm3() - 6.0e21).abs() < 1.0e6);
        assert_eq!(m.flat_triangles(), 0);
    }

    #[test]
    fn the_normals_of_a_box_point_out_of_it() {
        let m = cuboid(10.0, 10.0, 10.0);
        let centre = [5.0, 5.0, 5.0];
        for t in &m.triangles {
            let n = m.normal(t);
            let corner = m.vertices[t[0] as usize];
            assert!(dot(n, sub(corner, centre)) > 0.0, "{t:?} {n:?}");
        }
    }

    #[test]
    fn an_open_surface_and_a_surface_with_a_crack_are_found() {
        let mut m = cuboid(10.0, 10.0, 10.0);
        let last = m.triangles.pop().unwrap();
        let err = m.check_closed().unwrap_err();
        assert!(err.contains("open edges"), "{err}");
        m.triangles.push([last[0], last[0], last[1]]);
        assert!(m.check_closed().unwrap_err().contains("twice"));
        let mut flipped = cuboid(10.0, 10.0, 10.0);
        flipped.triangles[0].swap(1, 2);
        assert!(flipped.check_closed().is_err());
    }

    #[test]
    fn the_volume_of_a_box_with_a_hole_through_it() {
        // a block 40 x 30 x 5 mm with a round hole of 6 mm through it
        let circle = Circle::new(64);
        let (x, y, z) = (40.0e6, 30.0e6, 5.0e6);
        let mut b = Builder::new(10_000);
        let rect =
            |b: &mut Builder, h: f64| b.ring(&[[0.0, 0.0, h], [x, 0.0, h], [x, y, h], [0.0, y, h]]);
        let (rb, rt) = (rect(&mut b, 0.0), rect(&mut b, z));
        let ring = |b: &mut Builder, h: f64| {
            let pts: Vec<P3> = circle
                .polygon(20.0e6, 15.0e6, 3.0e6)
                .iter()
                .map(|p| [p.0, p.1, h])
                .collect();
            b.ring(&pts)
        };
        let (hb, ht) = (ring(&mut b, 0.0), ring(&mut b, z));
        b.plane(
            &[
                Loop::new(rb.clone(), Role::Boundary, "bottom"),
                Loop::new(hb.clone(), Role::Cut, "hole"),
            ],
            [0.0, 0.0, -1.0],
        )
        .unwrap();
        b.plane(
            &[
                Loop::new(rt.clone(), Role::Boundary, "top"),
                Loop::new(ht.clone(), Role::Cut, "hole"),
            ],
            [0.0, 0.0, 1.0],
        )
        .unwrap();
        b.strip(&rb, &rt, true).unwrap();
        b.strip(&hb, &ht, false).unwrap();
        let m = b.finish();
        m.check_closed().unwrap();
        // 6000 mm3 less the cylinder of 6 mm over 5 mm, whose polygon has the area of the circle
        let expected = (40.0 * 30.0 * 5.0 - std::f64::consts::PI * 9.0 * 5.0) * 1.0e18;
        assert!(
            ((m.volume_nm3() - expected) / expected).abs() < 1e-9,
            "{} {expected}",
            m.volume_nm3()
        );
    }

    #[test]
    fn the_series_of_the_circle_gives_the_exact_quarter_turns_and_the_right_sines() {
        let c = Circle::new(64);
        assert_eq!(c.at(0), (1.0, 0.0));
        assert_eq!(c.at(16), (-0.0, 1.0));
        assert_eq!(c.at(32), (-1.0, -0.0));
        assert_eq!(c.at(48), (0.0, -1.0));
        let (s, co) = c.at(8);
        assert!((s - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-15);
        assert!((co - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-15);
        assert_eq!(c.sides(), 64);
        // the sides are rounded to a multiple of four and never fewer than eight
        assert_eq!(Circle::new(3).sides(), 8);
        assert_eq!(Circle::new(70).sides(), 68);
    }

    #[test]
    fn the_polygon_of_a_circle_has_the_area_of_the_circle() {
        for n in [8u32, 16, 64, 128] {
            let c = Circle::new(n);
            let pts = c.polygon(0.0, 0.0, 10.0);
            let mut area = 0.0;
            for i in 0..pts.len() {
                let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
                area += p.0 * q.1 - q.0 * p.1;
            }
            assert!(
                (area / 2.0 - std::f64::consts::PI * 100.0).abs() < 1e-9,
                "{n}"
            );
        }
    }

    #[test]
    fn a_text_file_and_a_binary_file_hold_the_same_triangles() {
        let m = cuboid(10_000_000.0, 20_000_000.0, 30_000_000.0);
        let text = stl_ascii(&[("box one", &m)]);
        assert!(text.starts_with("solid box_one\n"));
        assert!(text.ends_with("endsolid box_one\n"));
        let bin = stl_binary(&[("box one", &m)]);
        assert_eq!(bin.len(), 84 + 50 * 12);
        assert!(!bin.starts_with(b"solid"));
        assert_eq!(&bin[80..84], &12u32.to_le_bytes());
        let (a, b) = (read_stl(text.as_bytes()).unwrap(), read_stl(&bin).unwrap());
        assert_eq!(a.len(), 12);
        assert_eq!(a, b);
        // the first corner of the first triangle, in millimetres
        let first = m.vertices[m.triangles[0][0] as usize];
        assert_eq!(a[0][0], first[0] / 1.0e6);
    }

    #[test]
    fn a_number_is_never_minus_zero() {
        assert_eq!(number(-0.0), "0.000000");
        assert_eq!(number(-0.0000001), "0.000000");
        assert_eq!(number(-1.5), "-1.500000");
        assert_eq!(number(12.0), "12.000000");
    }

    #[test]
    fn the_limit_of_triangles_is_a_refusal() {
        let mut b = Builder::new(1);
        let ids = b.ring(&[
            [0.0, 0.0, 0.0],
            [1.0e6, 0.0, 0.0],
            [1.0e6, 1.0e6, 0.0],
            [0.0, 1.0e6, 0.0],
        ]);
        let err = b
            .plane(&[Loop::new(ids, Role::Boundary, "square")], [0.0, 0.0, 1.0])
            .unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-050");
    }

    #[test]
    fn outlines_that_cross_or_are_in_the_wrong_place_are_refused() {
        let mut b = Builder::new(1000);
        let big = b.ring(&[
            [0.0, 0.0, 0.0],
            [10.0e6, 0.0, 0.0],
            [10.0e6, 10.0e6, 0.0],
            [0.0, 10.0e6, 0.0],
        ]);
        let crossing = b.ring(&[
            [5.0e6, 5.0e6, 0.0],
            [15.0e6, 5.0e6, 0.0],
            [15.0e6, 8.0e6, 0.0],
            [5.0e6, 8.0e6, 0.0],
        ]);
        let err = b
            .plane(
                &[
                    Loop::new(big.clone(), Role::Boundary, "the face"),
                    Loop::new(crossing, Role::Cut, "the hole"),
                ],
                [0.0, 0.0, 1.0],
            )
            .unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-048");
        assert!(err
            .to_string()
            .contains("the face and the hole cross or touch"));
        // an opening outside the surface
        let outside = b.ring(&[
            [20.0e6, 0.0, 0.0],
            [22.0e6, 0.0, 0.0],
            [22.0e6, 2.0e6, 0.0],
            [20.0e6, 2.0e6, 0.0],
        ]);
        let err = b
            .plane(
                &[
                    Loop::new(big, Role::Boundary, "the face"),
                    Loop::new(outside, Role::Cut, "the far hole"),
                ],
                [0.0, 0.0, 1.0],
            )
            .unwrap_err();
        assert!(err.to_string().contains("the far hole does not lie where"));
    }

    #[test]
    fn an_island_inside_an_opening_is_a_region_of_its_own() {
        // a ring of surface: a square with a square opening and a smaller island in the opening
        let mut b = Builder::new(1000);
        let sq = |b: &mut Builder, lo: f64, hi: f64| {
            b.ring(&[[lo, lo, 0.0], [hi, lo, 0.0], [hi, hi, 0.0], [lo, hi, 0.0]])
        };
        let (outer, opening, island) = (
            sq(&mut b, 0.0, 30.0e6),
            sq(&mut b, 5.0e6, 25.0e6),
            sq(&mut b, 10.0e6, 20.0e6),
        );
        b.plane(
            &[
                Loop::new(outer, Role::Boundary, "face"),
                Loop::new(opening, Role::Cut, "opening"),
                Loop::new(island, Role::Island, "island"),
            ],
            [0.0, 0.0, 1.0],
        )
        .unwrap();
        let m = b.finish();
        let area: f64 = m.triangles.iter().map(|t| m.area_nm2(t)).sum();
        // 900 - 400 + 100 square millimetres
        assert!((area - 600.0e12).abs() < 1.0e6, "{area}");
    }

    #[test]
    fn a_text_file_with_a_missing_vertex_and_a_short_binary_file_are_refused() {
        assert!(read_stl(b"solid x\nfacet normal 0 0 1\nouter loop\nvertex 1 2 3\n").is_err());
        assert!(read_stl(&[0u8; 10]).is_err());
        let mut bin = vec![b' '; 80];
        bin.extend_from_slice(&5u32.to_le_bytes());
        assert!(read_stl(&bin).is_err());
    }
}
