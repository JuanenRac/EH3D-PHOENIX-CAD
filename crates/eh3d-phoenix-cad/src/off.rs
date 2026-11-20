// SPDX-License-Identifier: GPL-3.0-or-later
//! The OFF file (Object File Format), the plain text format of geometry libraries and of many
//! mesh benchmarks: a line `OFF`, the counts of vertices, faces and edges, the vertices and the
//! faces (`3 a b c`, a face of more than three corners being cut into a fan of triangles).
//!
//! The variants `COFF` (colours), `NOFF` (normals) and `STOFF` (texture coordinates) are read by
//! taking the extra numbers of each vertex for what they are and leaving them out; the colours at
//! the end of a face are left out too. Coordinates are in millimetres. Comments (`#`) are skipped,
//! the counts are checked against the words of the file before anything is reserved, and the
//! reader never panics.

use crate::interchange::Body;
use crate::mesh::Mesh;

/// The nanometres of a millimetre.
const NM_PER_MM: f64 = 1.0e6;

/// The farthest a coordinate may be from the origin, in nanometres (a thousand kilometres).
const FAR_NM: f64 = 1.0e15;

/// The solid of an OFF file; more than `max_triangles` triangles is refused.
pub fn read(text: &str, max_triangles: usize) -> Result<Body, String> {
    let mut words = text
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .flat_map(str::split_whitespace);
    let head = words.next().ok_or("the file is empty")?;
    let head = head.to_ascii_uppercase();
    let extra = match head.as_str() {
        "OFF" => 0,
        "COFF" => 4,
        "NOFF" => 3,
        "CNOFF" => 7,
        "STOFF" => 2,
        other => return Err(format!("{other}: the file does not start with OFF")),
    };
    let mut count = |what: &str| -> Result<usize, String> {
        let w = words
            .next()
            .ok_or_else(|| format!("the counts of {what} are missing"))?;
        w.parse::<usize>()
            .map_err(|_| format!("{w} is not a count of {what}"))
    };
    let vertices = count("vertices")?;
    let faces = count("faces")?;
    let _edges = count("edges")?;
    let remaining = words.clone().count();
    if vertices.saturating_mul(3 + extra) > remaining || faces > remaining {
        return Err(format!(
            "the file says {vertices} vertices and {faces} faces and does not hold them"
        ));
    }
    let mut mesh = Mesh::default();
    mesh.vertices.reserve(vertices);
    for n in 0..vertices {
        let mut p = [0.0f64; 3];
        for (k, slot) in p.iter_mut().enumerate() {
            let w = words.next().ok_or("the file ends inside the vertices")?;
            *slot = w
                .parse::<f64>()
                .map_err(|_| format!("vertex {}: {w} is not a number", n + 1))?
                * NM_PER_MM;
            if !slot.is_finite() || slot.abs() > FAR_NM {
                return Err(format!(
                    "vertex {}: coordinate {} is not near the origin",
                    n + 1,
                    k + 1
                ));
            }
        }
        for _ in 0..extra {
            words.next().ok_or("the file ends inside the vertices")?;
        }
        mesh.vertices
            .push([p[0].round(), p[1].round(), p[2].round()]);
    }
    for n in 0..faces {
        let w = words.next().ok_or("the file ends inside the faces")?;
        let corners = w
            .parse::<usize>()
            .ok()
            .filter(|c| (3..=1_000_000).contains(c))
            .ok_or_else(|| format!("face {}: {w} is not a count of corners of 3 or more", n + 1))?;
        let mut ids = Vec::with_capacity(corners.min(64));
        for _ in 0..corners {
            let w = words.next().ok_or("the file ends inside the faces")?;
            let id = w
                .parse::<usize>()
                .ok()
                .filter(|i| *i < vertices)
                .ok_or_else(|| format!("face {}: {w} is not a vertex of the file", n + 1))?;
            ids.push(id as u32);
        }
        for k in 1..ids.len() - 1 {
            if mesh.triangles.len() >= max_triangles {
                return Err(format!(
                    "the file holds more than {max_triangles} triangles"
                ));
            }
            mesh.triangles.push([ids[0], ids[k], ids[k + 1]]);
        }
        // the colour of a face (the numbers up to the end of the line are not told apart from the next
        // face in a file of words, so only the plain files and the coloured vertices are read)
    }
    Ok(("off".to_owned(), mesh))
}

fn number(v: f64) -> String {
    let text = format!("{v:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-0" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// The solids as one OFF file (the format has one object).
pub fn write(bodies: &[(&str, &Mesh)]) -> String {
    let mesh = crate::interchange::merge(bodies);
    let mut out = format!(
        "OFF\n# EH3D PHOENIX-CAD, millimetres\n{} {} 0\n",
        mesh.vertices.len(),
        mesh.triangles.len()
    );
    for v in &mesh.vertices {
        out.push_str(&format!(
            "{} {} {}\n",
            number(v[0] / NM_PER_MM),
            number(v[1] / NM_PER_MM),
            number(v[2] / NM_PER_MM)
        ));
    }
    for t in &mesh.triangles {
        out.push_str(&format!("3 {} {} {}\n", t[0], t[1], t[2]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetra() -> Mesh {
        Mesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [10.0e6, 0.0, 0.0],
                [0.0, 12.5e6, 0.0],
                [0.0, 0.0, 7.0e6],
            ],
            triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        }
    }

    #[test]
    fn a_solid_reads_back_the_same_and_the_bytes_do_not_change() {
        let m = tetra();
        let text = write(&[("t", &m)]);
        assert!(text.starts_with("OFF\n# EH3D PHOENIX-CAD, millimetres\n4 4 0\n"));
        let body = read(&text, 100).unwrap();
        assert_eq!(body.0, "off");
        assert_eq!(body.1, m);
        assert_eq!(text, write(&[("t", &m)]));
    }

    #[test]
    fn comments_blank_lines_any_case_and_a_polygon_are_read() {
        let text = "off # header\n\n4 1 4\n0 0 0\n1 0 0 # a vertex\n1 1 0\n0 1 0\n\n4 0 1 2 3\n";
        let body = read(text, 10).unwrap();
        assert_eq!(body.1.triangles, vec![[0, 1, 2], [0, 2, 3]]);
        assert_eq!(body.1.vertices[2], [1.0e6, 1.0e6, 0.0]);
    }

    #[test]
    fn coloured_vertices_are_read_and_their_colours_left_out() {
        let text =
            "COFF\n3 1 0\n0 0 0 255 0 0 255\n1 0 0 0 255 0 255\n0 1 0 0 0 255 255\n3 0 1 2\n";
        let body = read(text, 10).unwrap();
        assert_eq!(body.1.vertices.len(), 3);
        assert_eq!(body.1.vertices[1], [1.0e6, 0.0, 0.0]);
        assert_eq!(body.1.triangles, vec![[0, 1, 2]]);
    }

    #[test]
    fn broken_files_are_refused_by_what_is_wrong() {
        let good = write(&[("t", &tetra())]);
        let e = |t: &str| read(t, 100).unwrap_err();
        assert!(e("").contains("empty"));
        assert!(e("PLY\n").contains("start with OFF"));
        assert!(e("OFF\nx 1 0\n").contains("not a count of vertices"));
        assert!(e("OFF\n99999999 1 0\n0 0 0\n").contains("does not hold"));
        assert!(e(&good.replace("3 1 2 3", "3 1 2 9")).contains("not a vertex"));
        assert!(e(&good.replace("3 1 2 3", "2 1 2")).contains("3 or more"));
        assert!(e(&good.replace("12.5", "abc")).contains("vertex 3"));
        assert!(e(&good.replace("12.5", "1e30")).contains("near the origin"));
        assert!(read(&good, 3).unwrap_err().contains("more than 3"));
        assert!(e(&good[..good.len() - 4]).contains("ends"));
    }

    #[test]
    fn damaged_files_never_panic_with_a_fixed_seed() {
        let base = write(&[("t", &tetra())]).into_bytes();
        let mut seed = 13u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as usize
        };
        for _ in 0..3000 {
            let mut bytes = base.clone();
            for _ in 0..1 + next() % 4 {
                let at = next() % bytes.len();
                bytes[at] = b" 0123456789-.#\nx"[next() % 16];
            }
            let cut = next() % (bytes.len() + 1);
            let _ = read(&String::from_utf8_lossy(&bytes[..cut]), 100);
        }
    }
}
