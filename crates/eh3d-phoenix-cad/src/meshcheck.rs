// SPDX-License-Identifier: GPL-3.0-or-later
//! A check of a triangle mesh for the person who is going to print it or send it to another program:
//! how many separate shells it has, its Euler characteristic and the genus that follows from it,
//! the edges that are open, shared by more than two triangles or wound the other way round by their
//! neighbours, the triangles with no area, the vertices that repeat another one, the shortest and
//! the longest edge, the worst proportions of a triangle, whether the surface is turned inside out
//! and the radii of gyration about its principal axes.
//!
//! The figures are the ones of [`crate::mesh::Mesh`] (nanometres in, millimetres out) and the
//! check never changes the mesh; [`flipped`] gives the mesh with every triangle turned the other
//! way, which is the repair for a surface that is inside out. The verdict is [`Report::problems`]:
//! a mesh with no problem is closed, wound the same way everywhere, free of flat triangles and holds
//! a positive volume. Repeated vertices are only told, because they are what leaves the seams of a
//! mesh open and the person can weld them.
//!
//! [`weld`] merges the vertices that share a place (the repair for the seams that repeated vertices
//! leave open) and [`fits`] tells whether the solid goes into a build volume, as it is or turned by
//! quarter turns.
//!
//! The principal axes come from the tensor of inertia of the solid ([`crate::mesh::Mesh::inertia_nm5`])
//! by the Jacobi method on a symmetric matrix of three rows, which gives the same bits on every
//! computer, and the radius of gyration about an axis is the square root of the moment about it
//! over the volume.

use std::collections::{HashMap, HashSet};

use crate::lang::{fill, Lang, Msg};
use crate::mesh::{Mesh, P3};

const NM_PER_MM: f64 = 1.0e6;

/// A triangle whose area is not above this, in square nanometres, counts as flat.
const FLAT_NM2: f64 = 1.0;

/// What is wrong with a mesh for a slicer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// Edges used by one triangle only: the surface has a hole.
    OpenEdges(usize),
    /// Edges shared by more than two triangles.
    NonManifoldEdges(usize),
    /// Edges that two neighbours use in the same direction: one of them is wound the other way.
    InconsistentEdges(usize),
    /// Triangles that use a vertex twice.
    DegenerateTriangles(usize),
    /// Triangles that have no area.
    FlatTriangles(usize),
    /// The volume is negative: every triangle faces inwards.
    InsideOut,
    /// The surface holds no volume.
    NoVolume,
}

/// Everything the check finds out about one mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// How many vertices the mesh lists.
    pub vertices: usize,
    /// How many of them no triangle uses.
    pub unused_vertices: usize,
    /// How many vertices have the same place as another one.
    pub repeated_vertices: usize,
    /// How many triangles.
    pub triangles: usize,
    /// How many separate shells (groups of triangles that share vertices).
    pub shells: usize,
    /// How many different edges.
    pub edges: usize,
    /// The Euler characteristic: the vertices that are used, minus the edges, plus the triangles.
    pub euler: i64,
    /// The sum of the genus of the shells (a torus is 1), told only when every shell is closed and
    /// wound the same way and the characteristic is even.
    pub genus: Option<i64>,
    /// Edges used by one triangle only.
    pub open_edges: usize,
    /// Edges shared by more than two triangles.
    pub non_manifold_edges: usize,
    /// Edges that two triangles use in the same direction.
    pub inconsistent_edges: usize,
    /// Triangles that use a vertex twice.
    pub degenerate_triangles: usize,
    /// Triangles with no area, the degenerate ones included.
    pub flat_triangles: usize,
    /// The area of the surface in square millimetres.
    pub area_mm2: f64,
    /// The signed volume in cubic millimetres (negative when the surface is inside out).
    pub volume_mm3: f64,
    /// The size along the three axes in millimetres.
    pub size_mm: [f64; 3],
    /// The shortest and the longest edge in millimetres, when there is an edge.
    pub edge_mm: Option<(f64, f64)>,
    /// The longest edge of the worst triangle over its height on that edge (about 1.155 for an
    /// equilateral triangle), for the triangles that have an area.
    pub worst_ratio: Option<f64>,
    /// The radii of gyration about the principal axes in millimetres, smallest first, when the
    /// surface holds volume.
    pub gyration_mm: Option<[f64; 3]>,
    /// The principal axes as unit vectors, in the order of the radii.
    pub axes: Option<[P3; 3]>,
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm(a: P3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// The root of a union of vertices, with the path halved.
fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// The number of groups of vertices that the triangles join, counting only the vertices they use.
fn shells_of(mesh: &Mesh) -> usize {
    let mut parent: Vec<usize> = (0..mesh.vertices.len()).collect();
    let mut used = vec![false; mesh.vertices.len()];
    for t in &mesh.triangles {
        let ids = [t[0] as usize, t[1] as usize, t[2] as usize];
        for id in ids {
            used[id] = true;
        }
        let first = find(&mut parent, ids[0]);
        for id in &ids[1..] {
            let root = find(&mut parent, *id);
            if root != first {
                parent[root] = first;
            }
        }
    }
    let mut roots = 0;
    for (id, flag) in used.iter().enumerate() {
        if *flag && find(&mut parent, id) == id {
            roots += 1;
        }
    }
    roots
}

/// The eigenvalues of a symmetric matrix of three rows, smallest first, and its unit eigenvectors
/// in the same order (as rows), by the Jacobi method of rotations.
pub fn principal_axes(matrix: [[f64; 3]; 3]) -> ([f64; 3], [P3; 3]) {
    let mut a = matrix;
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let scale: f64 = a.iter().flatten().map(|x| x.abs()).fold(0.0, f64::max);
    for _ in 0..60 {
        let off = a[0][1] * a[0][1] + a[0][2] * a[0][2] + a[1][2] * a[1][2];
        if off <= 1.0e-30 * scale * scale {
            break;
        }
        for (p, q) in [(0usize, 1usize), (0, 2), (1, 2)] {
            if a[p][q] == 0.0 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let sign = if theta < 0.0 { -1.0 } else { 1.0 };
            let t = sign / (theta.abs() + (theta * theta + 1.0).sqrt());
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for row in a.iter_mut() {
                let (kp, kq) = (row[p], row[q]);
                row[p] = c * kp - s * kq;
                row[q] = s * kp + c * kq;
            }
            let (row_p, row_q) = (a[p], a[q]);
            for (k, (x, y)) in row_p.iter().zip(row_q.iter()).enumerate() {
                a[p][k] = c * x - s * y;
                a[q][k] = s * x + c * y;
            }
            for row in v.iter_mut() {
                let (kp, kq) = (row[p], row[q]);
                row[p] = c * kp - s * kq;
                row[q] = s * kp + c * kq;
            }
        }
    }
    let mut order = [0usize, 1, 2];
    order.sort_by(|&i, &j| a[i][i].total_cmp(&a[j][j]));
    let values = order.map(|i| a[i][i]);
    // the eigenvectors are the columns of `v`; they come back as rows
    let axes = order.map(|i| [v[0][i], v[1][i], v[2][i]]);
    (values, axes)
}

/// Checks a mesh.
pub fn analyse(mesh: &Mesh) -> Report {
    let mut directed: HashMap<(u32, u32), u32> = HashMap::with_capacity(mesh.triangles.len() * 3);
    let mut degenerate = 0usize;
    let mut used = vec![false; mesh.vertices.len()];
    for t in &mesh.triangles {
        for id in t {
            used[*id as usize] = true;
        }
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            degenerate += 1;
            continue;
        }
        for k in 0..3 {
            *directed.entry((t[k], t[(k + 1) % 3])).or_insert(0) += 1;
        }
    }
    let mut undirected: HashMap<(u32, u32), u32> = HashMap::with_capacity(directed.len());
    let mut inconsistent = 0usize;
    for (&(a, b), &n) in &directed {
        *undirected.entry((a.min(b), a.max(b))).or_insert(0) += n;
        if n >= 2 {
            inconsistent += 1;
        }
    }
    let open = undirected.values().filter(|&&n| n == 1).count();
    let many = undirected.values().filter(|&&n| n > 2).count();
    let (mut shortest, mut longest) = (f64::INFINITY, 0.0f64);
    for &(a, b) in undirected.keys() {
        let len = norm(sub(mesh.vertices[a as usize], mesh.vertices[b as usize]));
        shortest = shortest.min(len);
        longest = longest.max(len);
    }
    let mut worst: Option<f64> = None;
    for t in &mesh.triangles {
        let area = mesh.area_nm2(t);
        if area <= FLAT_NM2 {
            continue;
        }
        let [a, b, c] = mesh.corners(t);
        let long = norm(sub(a, b)).max(norm(sub(b, c))).max(norm(sub(c, a)));
        let ratio = long * long / (2.0 * area);
        worst = Some(worst.map_or(ratio, |w: f64| w.max(ratio)));
    }
    let mut places: HashSet<[u64; 3]> = HashSet::with_capacity(mesh.vertices.len());
    for v in &mesh.vertices {
        // minus zero and zero are the same place
        places.insert(v.map(|x| (x + 0.0).to_bits()));
    }
    let used_count = used.iter().filter(|u| **u).count();
    let shells = shells_of(mesh);
    let edges = undirected.len();
    let euler = used_count as i64 - edges as i64 + (mesh.triangles.len() - degenerate) as i64;
    let closed = open == 0 && many == 0 && inconsistent == 0 && degenerate == 0 && edges > 0;
    let genus = if closed && euler % 2 == 0 {
        Some(shells as i64 - euler / 2)
    } else {
        None
    };
    let volume = mesh.volume_nm3();
    let inertia = mesh.inertia_nm5();
    let (gyration_mm, axes) = match inertia {
        Some(tensor) if volume.abs() > 0.0 => {
            let (moments, axes) = principal_axes(tensor);
            let k = moments.map(|m| (m.max(0.0) / volume.abs()).sqrt() / NM_PER_MM);
            (Some(k), Some(axes))
        }
        _ => (None, None),
    };
    let size_mm = mesh.bounds().map_or([0.0; 3], |(lo, hi)| {
        [
            (hi[0] - lo[0]) / NM_PER_MM,
            (hi[1] - lo[1]) / NM_PER_MM,
            (hi[2] - lo[2]) / NM_PER_MM,
        ]
    });
    Report {
        vertices: mesh.vertices.len(),
        unused_vertices: mesh.vertices.len() - used_count,
        repeated_vertices: mesh.vertices.len() - places.len(),
        triangles: mesh.triangles.len(),
        shells,
        edges,
        euler,
        genus,
        open_edges: open,
        non_manifold_edges: many,
        inconsistent_edges: inconsistent,
        degenerate_triangles: degenerate,
        flat_triangles: mesh.flat_triangles().max(degenerate),
        area_mm2: mesh.triangles.iter().map(|t| mesh.area_nm2(t)).sum::<f64>()
            / (NM_PER_MM * NM_PER_MM),
        volume_mm3: volume / (NM_PER_MM * NM_PER_MM * NM_PER_MM),
        size_mm,
        edge_mm: if edges > 0 {
            Some((shortest / NM_PER_MM, longest / NM_PER_MM))
        } else {
            None
        },
        worst_ratio: worst,
        gyration_mm,
        axes,
    }
}

impl Report {
    /// What keeps the mesh from being printed, in a fixed order; empty for a mesh that can.
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        if self.open_edges > 0 {
            out.push(Problem::OpenEdges(self.open_edges));
        }
        if self.non_manifold_edges > 0 {
            out.push(Problem::NonManifoldEdges(self.non_manifold_edges));
        }
        if self.inconsistent_edges > 0 {
            out.push(Problem::InconsistentEdges(self.inconsistent_edges));
        }
        if self.degenerate_triangles > 0 {
            out.push(Problem::DegenerateTriangles(self.degenerate_triangles));
        }
        if self.flat_triangles > self.degenerate_triangles {
            out.push(Problem::FlatTriangles(self.flat_triangles));
        }
        if self.volume_mm3 < 0.0 {
            out.push(Problem::InsideOut);
        } else if self.volume_mm3 == 0.0 {
            out.push(Problem::NoVolume);
        }
        out
    }

    /// Whether the mesh has no problem.
    pub fn printable(&self) -> bool {
        self.problems().is_empty()
    }
}

/// A mesh with its vertices welded, and what that took away.
#[derive(Debug, Clone, PartialEq)]
pub struct Welded {
    /// The welded mesh.
    pub mesh: Mesh,
    /// How many vertices fewer the mesh has (merged, or no longer used by any triangle).
    pub merged_vertices: usize,
    /// How many triangles were dropped because two of their corners became one.
    pub dropped_triangles: usize,
}

/// Merges the vertices that share a place: with a tolerance of zero (or less) only those that are
/// exactly the same, with a tolerance in nanometres those that fall in the same cell of a grid of
/// that size (the first of them keeps its place; two points a hair apart on either side of a line
/// of the grid stay two). The triangles that lose a corner to the merge are dropped, and so are the
/// vertices that no triangle uses afterwards. The order of the triangles does not change.
pub fn weld(mesh: &Mesh, tolerance_nm: f64) -> Welded {
    let mut cells: HashMap<[i64; 3], u32> = HashMap::with_capacity(mesh.vertices.len());
    let mut vertices: Vec<P3> = Vec::new();
    let mut new_id: Vec<u32> = Vec::with_capacity(mesh.vertices.len());
    for v in &mesh.vertices {
        let key = if tolerance_nm > 0.0 {
            v.map(|x| (x / tolerance_nm).floor() as i64)
        } else {
            v.map(|x| (x + 0.0).to_bits() as i64)
        };
        let id = *cells.entry(key).or_insert_with(|| {
            vertices.push(*v);
            (vertices.len() - 1) as u32
        });
        new_id.push(id);
    }
    let mut triangles: Vec<[u32; 3]> = Vec::with_capacity(mesh.triangles.len());
    let mut dropped = 0usize;
    for t in &mesh.triangles {
        let n = [
            new_id[t[0] as usize],
            new_id[t[1] as usize],
            new_id[t[2] as usize],
        ];
        if n[0] == n[1] || n[1] == n[2] || n[0] == n[2] {
            dropped += 1;
        } else {
            triangles.push(n);
        }
    }
    // the vertices that nothing uses any more go, and the others close ranks
    let mut used = vec![false; vertices.len()];
    for t in &triangles {
        for id in t {
            used[*id as usize] = true;
        }
    }
    let mut closed_ranks = vec![0u32; vertices.len()];
    let mut kept: Vec<P3> = Vec::with_capacity(vertices.len());
    for (i, v) in vertices.iter().enumerate() {
        if used[i] {
            closed_ranks[i] = kept.len() as u32;
            kept.push(*v);
        }
    }
    for t in &mut triangles {
        for id in t.iter_mut() {
            *id = closed_ranks[*id as usize];
        }
    }
    Welded {
        merged_vertices: mesh.vertices.len() - kept.len(),
        dropped_triangles: dropped,
        mesh: Mesh {
            vertices: kept,
            triangles,
        },
    }
}

/// How a solid goes into a build volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// It fits as it is.
    Fits,
    /// It fits only when turned by quarter turns.
    Turned,
    /// It does not fit.
    No,
}

/// Whether a solid of the given size (millimetres along the three axes) goes into a build volume:
/// as it is, turned by quarter turns about some axis, or not at all. A size that is a hair more than
/// the volume (less than a nanometre) still fits.
pub fn fits(size_mm: [f64; 3], bed_mm: [f64; 3]) -> Fit {
    let inside = |s: [f64; 3]| s.iter().zip(&bed_mm).all(|(a, b)| *a <= *b + 1e-6);
    if inside(size_mm) {
        return Fit::Fits;
    }
    let [x, y, z] = size_mm;
    let turns = [[x, z, y], [y, x, z], [y, z, x], [z, x, y], [z, y, x]];
    if turns.into_iter().any(inside) {
        Fit::Turned
    } else {
        Fit::No
    }
}

/// The answer of [`fits`] as a line of text in a language.
pub fn bed_text(fit: Fit, bed_mm: [f64; 3], lang: Lang) -> String {
    let msg = match fit {
        Fit::Fits => Msg::MeshCheckBedFits,
        Fit::Turned => Msg::MeshCheckBedTurned,
        Fit::No => Msg::MeshCheckBedNo,
    };
    fill(
        msg.tr(lang),
        &[
            &bed_mm[0].to_string(),
            &bed_mm[1].to_string(),
            &bed_mm[2].to_string(),
        ],
    )
}

/// The mesh with every triangle turned the other way round: the repair of a surface that is inside
/// out (its volume is negative). Turning it twice gives the mesh back.
pub fn flipped(mesh: &Mesh) -> Mesh {
    Mesh {
        vertices: mesh.vertices.clone(),
        triangles: mesh.triangles.iter().map(|t| [t[0], t[2], t[1]]).collect(),
    }
}

fn num(value: f64, decimals: usize) -> String {
    // a value that rounds to zero is written without its minus sign
    let text = format!("{value:.decimals$}");
    if text.starts_with('-') && text.trim_start_matches(['-', '0', '.']).is_empty() {
        text[1..].to_owned()
    } else {
        text
    }
}

fn problem_line(problem: Problem, lang: Lang) -> String {
    match problem {
        Problem::OpenEdges(n) => fill(Msg::MeshCheckOpen.tr(lang), &[&n.to_string()]),
        Problem::NonManifoldEdges(n) => fill(Msg::MeshCheckNonManifold.tr(lang), &[&n.to_string()]),
        Problem::InconsistentEdges(n) => {
            fill(Msg::MeshCheckInconsistent.tr(lang), &[&n.to_string()])
        }
        Problem::DegenerateTriangles(n) => {
            fill(Msg::MeshCheckDegenerate.tr(lang), &[&n.to_string()])
        }
        Problem::FlatTriangles(n) => fill(Msg::MeshCheckFlat.tr(lang), &[&n.to_string()]),
        Problem::InsideOut => Msg::MeshCheckInsideOut.tr(lang).to_owned(),
        Problem::NoVolume => Msg::MeshCheckNoVolume.tr(lang).to_owned(),
    }
}

/// The report as lines of text in a language, ending with the verdict.
pub fn text_in(report: &Report, lang: Lang) -> String {
    let mut out = String::new();
    out.push_str(&fill(
        Msg::MeshCheckHead.tr(lang),
        &[
            &report.vertices.to_string(),
            &report.triangles.to_string(),
            &report.shells.to_string(),
        ],
    ));
    out.push('\n');
    out.push_str(&fill(
        Msg::MeshCheckSize.tr(lang),
        &[
            &num(report.area_mm2, 3),
            &num(report.volume_mm3, 3),
            &num(report.size_mm[0], 3),
            &num(report.size_mm[1], 3),
            &num(report.size_mm[2], 3),
        ],
    ));
    out.push('\n');
    match report.genus {
        Some(genus) => out.push_str(&fill(
            Msg::MeshCheckEuler.tr(lang),
            &[&report.euler.to_string(), &genus.to_string()],
        )),
        None => out.push_str(&fill(
            Msg::MeshCheckEulerOpen.tr(lang),
            &[&report.euler.to_string()],
        )),
    }
    out.push('\n');
    if let (Some((short, long)), Some(ratio)) = (report.edge_mm, report.worst_ratio) {
        out.push_str(&fill(
            Msg::MeshCheckEdges.tr(lang),
            &[&num(short, 4), &num(long, 3), &num(ratio, 2)],
        ));
        out.push('\n');
    }
    if let Some(k) = report.gyration_mm {
        out.push_str(&fill(
            Msg::MeshCheckGyration.tr(lang),
            &[&num(k[0], 3), &num(k[1], 3), &num(k[2], 3)],
        ));
        out.push('\n');
    }
    if report.repeated_vertices > 0 {
        out.push_str(&fill(
            Msg::MeshCheckRepeated.tr(lang),
            &[&report.repeated_vertices.to_string()],
        ));
        out.push('\n');
    }
    let problems = report.problems();
    for problem in &problems {
        out.push_str(&problem_line(*problem, lang));
        out.push('\n');
    }
    if problems.is_empty() {
        out.push_str(Msg::MeshCheckOk.tr(lang));
    } else {
        out.push_str(&fill(
            Msg::MeshCheckNo.tr(lang),
            &[&problems.len().to_string()],
        ));
    }
    out.push('\n');
    out
}

/// What the tests of this module and of the command line share.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    /// A box of `a` x `b` x `c` millimetres with its corner at the origin: 8 vertices and 12 triangles
    /// facing outwards.
    pub(crate) fn cuboid(a: f64, b: f64, c: f64) -> Mesh {
        let (a, b, c) = (a * NM_PER_MM, b * NM_PER_MM, c * NM_PER_MM);
        let vertices = vec![
            [0.0, 0.0, 0.0],
            [a, 0.0, 0.0],
            [a, b, 0.0],
            [0.0, b, 0.0],
            [0.0, 0.0, c],
            [a, 0.0, c],
            [a, b, c],
            [0.0, b, c],
        ];
        let triangles = vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        Mesh {
            vertices,
            triangles,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::cuboid;
    use super::*;

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn a_box_is_one_closed_shell_of_genus_zero_that_can_be_printed() {
        let r = analyse(&cuboid(10.0, 20.0, 30.0));
        assert_eq!((r.vertices, r.triangles, r.edges, r.shells), (8, 12, 18, 1));
        assert_eq!(r.euler, 2);
        assert_eq!(r.genus, Some(0));
        assert!(r.problems().is_empty() && r.printable());
        assert!(close(r.volume_mm3, 6000.0, 1e-6));
        assert!(close(r.area_mm2, 2.0 * (200.0 + 300.0 + 600.0), 1e-6));
        assert_eq!(r.size_mm, [10.0, 20.0, 30.0]);
        assert_eq!(
            (r.open_edges, r.non_manifold_edges, r.inconsistent_edges),
            (0, 0, 0)
        );
        assert_eq!(r.repeated_vertices, 0);
        assert_eq!(r.unused_vertices, 0);
    }

    #[test]
    fn the_edges_and_the_worst_triangle_of_a_box_are_those_of_its_faces() {
        let r = analyse(&cuboid(10.0, 20.0, 30.0));
        let (short, long) = r.edge_mm.unwrap();
        assert!(close(short, 10.0, 1e-9));
        // the diagonal of the 20 x 30 face
        assert!(close(long, (20.0f64 * 20.0 + 30.0 * 30.0).sqrt(), 1e-9));
        // the worst triangle is a half of the 10 x 30 face, a right triangle of legs 10 and 30 whose
        // longest edge is 31.6 and whose height on it is 9.49
        assert!(close(
            r.worst_ratio.unwrap(),
            (100.0 + 900.0) / (10.0 * 30.0),
            1e-9
        ));
    }

    #[test]
    fn the_radii_of_gyration_of_a_box_follow_its_sides_and_the_axes_are_those_of_the_box() {
        let r = analyse(&cuboid(10.0, 20.0, 30.0));
        let k = r.gyration_mm.unwrap();
        // about the long axis (z) the moment is the smallest: sqrt((a^2 + b^2) / 12)
        assert!(close(k[0], ((100.0f64 + 400.0) / 12.0).sqrt(), 1e-6));
        assert!(close(k[1], ((100.0f64 + 900.0) / 12.0).sqrt(), 1e-6));
        assert!(close(k[2], ((400.0f64 + 900.0) / 12.0).sqrt(), 1e-6));
        let axes = r.axes.unwrap();
        // the axis of the smallest radius is z, up to its sign
        assert!(close(axes[0][2].abs(), 1.0, 1e-9));
        assert!(close(axes[1][1].abs(), 1.0, 1e-9));
        assert!(close(axes[2][0].abs(), 1.0, 1e-9));
    }

    #[test]
    fn the_jacobi_method_finds_the_values_and_the_vectors_of_a_symmetric_matrix() {
        let (values, axes) = principal_axes([[3.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]]);
        assert_eq!(values, [1.0, 2.0, 3.0]);
        assert_eq!(axes[0], [0.0, 1.0, 0.0]);
        // a matrix turned 45 degrees about z has the same values
        let m = [[2.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 0.0, 5.0]];
        let (values, axes) = principal_axes(m);
        assert!(close(values[0], 1.0, 1e-12));
        assert!(close(values[1], 3.0, 1e-12));
        assert!(close(values[2], 5.0, 1e-12));
        for (value, axis) in values.iter().zip(axes) {
            for (i, row) in m.iter().enumerate() {
                let along: f64 = row.iter().zip(axis).map(|(x, y)| x * y).sum();
                assert!(close(along, value * axis[i], 1e-9));
            }
            assert!(close(norm(axis), 1.0, 1e-12));
        }
        // the zero matrix and a multiple of the identity stay as they are
        assert_eq!(principal_axes([[0.0; 3]; 3]).0, [0.0; 3]);
        assert_eq!(
            principal_axes([[4.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]]).0,
            [4.0; 3]
        );
    }

    #[test]
    fn two_separate_boxes_are_two_shells_and_a_missing_triangle_opens_three_edges() {
        let one = cuboid(10.0, 10.0, 10.0);
        let mut vertices = one.vertices.clone();
        let mut triangles = one.triangles.clone();
        for v in &one.vertices {
            vertices.push([v[0] + 50.0 * NM_PER_MM, v[1], v[2]]);
        }
        for t in &one.triangles {
            triangles.push([t[0] + 8, t[1] + 8, t[2] + 8]);
        }
        let two = Mesh {
            vertices,
            triangles,
        };
        let r = analyse(&two);
        assert_eq!(r.shells, 2);
        assert_eq!(r.euler, 4);
        assert_eq!(r.genus, Some(0));
        assert!(close(r.volume_mm3, 2000.0, 1e-6));
        let mut holed = one.clone();
        holed.triangles.pop();
        let r = analyse(&holed);
        assert_eq!(r.open_edges, 3);
        assert_eq!(r.genus, None);
        assert_eq!(r.problems(), vec![Problem::OpenEdges(3)]);
        assert!(!r.printable());
    }

    #[test]
    fn a_surface_turned_inside_out_is_told_and_turning_it_back_repairs_it() {
        let inside = flipped(&cuboid(10.0, 10.0, 10.0));
        let r = analyse(&inside);
        assert!(r.volume_mm3 < 0.0);
        assert_eq!(r.problems(), vec![Problem::InsideOut]);
        // an inside-out solid has the same radii of gyration
        assert!(close(
            r.gyration_mm.unwrap()[0],
            analyse(&cuboid(10.0, 10.0, 10.0)).gyration_mm.unwrap()[0],
            1e-9
        ));
        let fixed = flipped(&inside);
        assert_eq!(fixed, cuboid(10.0, 10.0, 10.0));
        assert!(analyse(&fixed).printable());
    }

    #[test]
    fn one_triangle_wound_the_other_way_makes_its_edges_inconsistent() {
        let mut mesh = cuboid(10.0, 10.0, 10.0);
        let t = mesh.triangles[0];
        mesh.triangles[0] = [t[0], t[2], t[1]];
        let r = analyse(&mesh);
        assert_eq!(r.inconsistent_edges, 3);
        assert_eq!(r.genus, None);
        assert!(r.problems().contains(&Problem::InconsistentEdges(3)));
    }

    #[test]
    fn triangles_with_no_area_and_with_a_repeated_vertex_are_counted_and_do_not_close_the_surface()
    {
        let mut mesh = cuboid(10.0, 10.0, 10.0);
        // a triangle on three points of one line, and one that uses a vertex twice
        mesh.vertices.push([5.0 * NM_PER_MM, 0.0, 0.0]);
        mesh.triangles.push([0, 8, 1]);
        mesh.triangles.push([0, 0, 1]);
        let r = analyse(&mesh);
        assert_eq!(r.degenerate_triangles, 1);
        assert_eq!(r.flat_triangles, 2);
        assert!(r.problems().contains(&Problem::DegenerateTriangles(1)));
        assert!(r.problems().contains(&Problem::FlatTriangles(2)));
        assert_eq!(r.genus, None);
        assert_eq!(r.unused_vertices, 0);
    }

    #[test]
    fn repeated_and_unused_vertices_are_counted_and_minus_zero_is_zero() {
        let mut mesh = cuboid(10.0, 10.0, 10.0);
        mesh.vertices.push([0.0, 0.0, 0.0]);
        mesh.vertices.push([-0.0, 0.0, 0.0]);
        let r = analyse(&mesh);
        assert_eq!(r.repeated_vertices, 2);
        assert_eq!(r.unused_vertices, 2);
        assert!(r.printable());
    }

    #[test]
    fn an_empty_mesh_has_nothing_to_tell_and_holds_no_volume() {
        let r = analyse(&Mesh::default());
        assert_eq!((r.vertices, r.triangles, r.shells, r.edges), (0, 0, 0, 0));
        assert_eq!(r.edge_mm, None);
        assert_eq!(r.worst_ratio, None);
        assert_eq!(r.gyration_mm, None);
        assert_eq!(r.problems(), vec![Problem::NoVolume]);
    }

    #[test]
    fn the_text_is_the_same_report_in_every_language_with_the_same_figures() {
        let good = analyse(&cuboid(10.0, 20.0, 30.0));
        let mut bad_mesh = cuboid(10.0, 20.0, 30.0);
        bad_mesh.triangles.pop();
        let bad = analyse(&bad_mesh);
        for lang in Lang::ALL {
            let ok = text_in(&good, lang);
            assert!(ok.contains("6000.000"), "{}", lang.code());
            assert!(ok.contains("20.000") && ok.contains("30.000"));
            assert!(!ok.contains('{'));
            let no = text_in(&bad, lang);
            assert!(no.contains('3') && !no.contains('{'));
            assert_ne!(ok, no);
        }
        let en = text_in(&good, Lang::En);
        assert!(en.starts_with("8 vertices, 12 triangles, 1 shell(s)\n"));
        assert!(en.contains("Euler characteristic 2, genus 0"));
        assert!(en.ends_with("Printable: closed, wound the same way everywhere and holds volume\n"));
        let en = text_in(&bad, Lang::En);
        assert!(en.contains("3 open edges"));
        assert!(en.ends_with("Not printable: 1 problem(s)\n"));
    }

    /// The box of `cuboid`, with each triangle owning its three vertices (36 in all), as a file that
    /// repeats its corners.
    fn split(mesh: &Mesh) -> Mesh {
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        for t in &mesh.triangles {
            let base = vertices.len() as u32;
            for id in t {
                vertices.push(mesh.vertices[*id as usize]);
            }
            triangles.push([base, base + 1, base + 2]);
        }
        Mesh {
            vertices,
            triangles,
        }
    }

    #[test]
    fn welding_the_repeated_corners_of_a_box_closes_its_seams_and_keeps_its_volume() {
        let apart = split(&cuboid(10.0, 20.0, 30.0));
        let before = analyse(&apart);
        assert_eq!(before.vertices, 36);
        assert_eq!(before.repeated_vertices, 28);
        assert!(!before.printable());
        let w = weld(&apart, 0.0);
        assert_eq!((w.merged_vertices, w.dropped_triangles), (28, 0));
        assert_eq!((w.mesh.vertices.len(), w.mesh.triangles.len()), (8, 12));
        let after = analyse(&w.mesh);
        assert!(after.printable());
        assert_eq!(
            (after.vertices, after.shells, after.repeated_vertices),
            (8, 1, 0)
        );
        assert!(close(after.volume_mm3, 6000.0, 1e-6));
        // welding what is already welded changes nothing
        let again = weld(&w.mesh, 0.0);
        assert_eq!((again.merged_vertices, again.dropped_triangles), (0, 0));
        assert_eq!(again.mesh, w.mesh);
    }

    #[test]
    fn a_tolerance_merges_the_vertices_of_one_cell_and_drops_the_triangles_that_collapse() {
        // every corner nudged by up to 200 nm inside a cell of 1000 nm
        let mut apart = split(&cuboid(10.0, 20.0, 30.0));
        for (i, v) in apart.vertices.iter_mut().enumerate() {
            let nudge = 100.0 + (i % 3) as f64 * 100.0;
            *v = v.map(|x| x + 500.0 + nudge - 200.0);
        }
        assert!(!analyse(&apart).printable());
        let w = weld(&apart, 1000.0);
        assert_eq!((w.merged_vertices, w.dropped_triangles), (28, 0));
        assert!(analyse(&w.mesh).printable());
        // a sliver whose two corners share a cell collapses
        let mut sliver = cuboid(10.0, 10.0, 10.0);
        sliver.vertices.push([5.0 * NM_PER_MM + 1.0, 0.0, 0.0]);
        sliver.vertices.push([5.0 * NM_PER_MM, 0.0, 0.0]);
        sliver.triangles.push([8, 9, 1]);
        let w = weld(&sliver, 100.0);
        assert_eq!(w.dropped_triangles, 1);
        assert_eq!(w.mesh, cuboid(10.0, 10.0, 10.0));
    }

    #[test]
    fn the_vertices_that_no_triangle_uses_are_taken_away_and_the_others_close_ranks() {
        let mut mesh = cuboid(10.0, 10.0, 10.0);
        mesh.vertices.insert(0, [-7.0, -7.0, -7.0]);
        for t in &mut mesh.triangles {
            for id in t.iter_mut() {
                *id += 1;
            }
        }
        let w = weld(&mesh, 0.0);
        assert_eq!(w.merged_vertices, 1);
        assert_eq!(w.mesh, cuboid(10.0, 10.0, 10.0));
        let empty = weld(&Mesh::default(), 5.0);
        assert_eq!(empty.mesh, Mesh::default());
        assert_eq!((empty.merged_vertices, empty.dropped_triangles), (0, 0));
    }

    #[test]
    fn a_solid_fits_a_build_volume_as_it_is_turned_or_not_at_all() {
        let bed = [220.0, 220.0, 250.0];
        assert_eq!(fits([100.0, 100.0, 100.0], bed), Fit::Fits);
        assert_eq!(fits([220.0, 220.0, 250.0], bed), Fit::Fits);
        // a hair over, within a nanometre
        assert_eq!(fits([220.0000001, 10.0, 10.0], bed), Fit::Fits);
        // tall but narrow: it fits lying down
        assert_eq!(
            fits([30.0, 30.0, 240.0], [250.0, 250.0, 100.0]),
            Fit::Turned
        );
        assert_eq!(
            fits([240.0, 10.0, 10.0], [100.0, 250.0, 100.0]),
            Fit::Turned
        );
        assert_eq!(fits([300.0, 300.0, 10.0], bed), Fit::No);
        assert_eq!(fits([251.0, 221.0, 221.0], bed), Fit::No);
        // every way of turning a size gives the same answer
        let size = [30.0, 200.0, 240.0];
        for turned in [
            [30.0, 240.0, 200.0],
            [200.0, 30.0, 240.0],
            [200.0, 240.0, 30.0],
            [240.0, 30.0, 200.0],
            [240.0, 200.0, 30.0],
        ] {
            assert_eq!(fits(turned, bed) == Fit::No, fits(size, bed) == Fit::No);
        }
    }

    #[test]
    fn the_build_volume_is_told_in_every_language() {
        for fit in [Fit::Fits, Fit::Turned, Fit::No] {
            let mut seen = Vec::new();
            for lang in Lang::ALL {
                let text = bed_text(fit, [220.0, 220.5, 250.0], lang);
                assert!(text.contains("220") && text.contains("220.5") && text.contains("250"));
                assert!(!text.contains('{'));
                seen.push(text);
            }
            assert_ne!(seen[0], seen[1]);
        }
        assert_eq!(
            bed_text(Fit::Turned, [220.0, 220.0, 250.0], Lang::En),
            "Fits the 220 x 220 x 250 mm build volume only when turned"
        );
    }

    #[test]
    fn random_boxes_that_repeat_their_corners_are_welded_back_to_themselves_with_a_fixed_seed() {
        let mut rng = Lcg(0x3e1d_cad5);
        for _ in 0..30 {
            let (a, b, c) = (
                rng.between(1.0, 60.0),
                rng.between(1.0, 60.0),
                rng.between(1.0, 60.0),
            );
            let whole = cuboid(a, b, c);
            let apart = split(&whole);
            let w = weld(&apart, 0.0);
            assert_eq!((w.mesh.vertices.len(), w.mesh.triangles.len()), (8, 12));
            assert_eq!((w.merged_vertices, w.dropped_triangles), (28, 0));
            let r = analyse(&w.mesh);
            assert!(r.printable());
            assert!(close(r.volume_mm3, a * b * c, a * b * c * 1e-9 + 1e-6));
            // turned inside out and split, welding and turning back gives a box again
            let back = analyse(&flipped(&weld(&split(&flipped(&whole)), 0.0).mesh));
            assert!(back.printable());
            assert!(close(back.volume_mm3, r.volume_mm3, 1e-6));
        }
    }

    #[test]
    fn a_value_that_rounds_to_zero_has_no_minus_sign() {
        assert_eq!(num(-0.0004, 3), "0.000");
        assert_eq!(num(-0.5, 3), "-0.500");
        assert_eq!(num(12.0, 1), "12.0");
    }

    /// A small generator with a fixed seed, so that the tests that draw random numbers repeat.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }

        fn between(&mut self, low: f64, high: f64) -> f64 {
            low + (high - low) * (self.next() % 10_000) as f64 / 10_000.0
        }
    }

    #[test]
    fn random_boxes_keep_their_figures_when_moved_and_turned_a_quarter_with_a_fixed_seed() {
        let mut rng = Lcg(0x5eed_cad1);
        for _ in 0..40 {
            let (a, b, c) = (
                rng.between(1.0, 80.0),
                rng.between(1.0, 80.0),
                rng.between(1.0, 80.0),
            );
            let base = analyse(&cuboid(a, b, c));
            assert!(base.printable());
            assert!(close(base.volume_mm3, a * b * c, a * b * c * 1e-9 + 1e-6));
            assert!(close(
                base.area_mm2,
                2.0 * (a * b + b * c + a * c),
                1e-6 * a * b * c
            ));
            let (dx, dy, dz) = (
                rng.between(-500.0, 500.0),
                rng.between(-500.0, 500.0),
                rng.between(-500.0, 500.0),
            );
            // moved, and turned a quarter about z (x, y -> -y, x), which keeps the handedness
            let moved = cuboid(a, b, c).mapped(|p| {
                [
                    -p[1] + dx * NM_PER_MM,
                    p[0] + dy * NM_PER_MM,
                    p[2] + dz * NM_PER_MM,
                ]
            });
            let r = analyse(&moved);
            assert!(r.printable());
            assert_eq!(
                (r.shells, r.edges, r.euler),
                (base.shells, base.edges, base.euler)
            );
            assert!(close(r.volume_mm3, base.volume_mm3, base.volume_mm3 * 1e-8));
            assert!(close(r.area_mm2, base.area_mm2, base.area_mm2 * 1e-8));
            let (kb, kr) = (base.gyration_mm.unwrap(), r.gyration_mm.unwrap());
            for (x, y) in kb.iter().zip(kr.iter()) {
                assert!(close(*x, *y, x * 1e-6), "{x} {y}");
            }
            // the radii are the sorted roots of the three moments of the sides
            let mut want = [
                ((b * b + c * c) / 12.0).sqrt(),
                ((a * a + c * c) / 12.0).sqrt(),
                ((a * a + b * b) / 12.0).sqrt(),
            ];
            want.sort_by(f64::total_cmp);
            for (x, w) in kb.iter().zip(want.iter()) {
                assert!(close(*x, *w, w * 1e-6));
            }
            // inside out and back
            let inside = analyse(&flipped(&cuboid(a, b, c)));
            assert_eq!(inside.problems(), vec![Problem::InsideOut]);
            assert!(close(
                inside.volume_mm3,
                -base.volume_mm3,
                1e-6 * base.volume_mm3
            ));
        }
    }
}
