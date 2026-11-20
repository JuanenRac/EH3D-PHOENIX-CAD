// SPDX-License-Identifier: GPL-3.0-or-later
//! Solids as `3DFACE` entities of a DXF file, the way drawing programs and many CAD programs
//! exchange a surface made of triangles.
//!
//! Each solid is written on a layer of its name, one `3DFACE` for each triangle (the fourth corner
//! repeats the third), with the unit of the drawing declared in the header as millimetres. The
//! reader takes every `3DFACE` (a face of four different corners is cut into two triangles along
//! its first diagonal), groups them by layer, reads the unit from `$INSUNITS` when the header
//! gives one and says, as a warning with a name, every kind of entity it leaves out. It checks the
//! pairs of the file as it goes, refuses a file with more triangles than it is given room for and
//! never panics.

use std::collections::BTreeMap;

use crate::interchange::{merge_by, Body};
use crate::mesh::{number, solid_name, Mesh};
use crate::stlread::ReadWarning;

/// The nanometres of a millimetre.
const NM_PER_MM: f64 = 1.0e6;

/// The farthest a coordinate may be from the origin, in nanometres (a thousand kilometres).
const FAR_NM: f64 = 1.0e15;

/// The solids as a text DXF file of `3DFACE` entities.
pub fn write(bodies: &[(&str, &Mesh)]) -> String {
    let mut out = String::from(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n",
    );
    for (name, mesh) in bodies {
        let layer = solid_name(name).to_ascii_uppercase();
        for t in &mesh.triangles {
            let c = mesh.corners(t);
            out.push_str(&format!("0\n3DFACE\n8\n{layer}\n"));
            for (k, p) in [c[0], c[1], c[2], c[2]].iter().enumerate() {
                for (axis, base) in [10, 20, 30].iter().enumerate() {
                    out.push_str(&format!("{}\n{}\n", base + k, number(p[axis] / NM_PER_MM)));
                }
            }
        }
    }
    out.push_str("0\nENDSEC\n0\nEOF\n");
    out
}

/// The unit of the header in nanometres, from `$INSUNITS` (millimetres when none is declared).
fn unit_of(code: i64) -> Option<f64> {
    Some(match code {
        0 | 4 => 1.0e6,
        1 => 25.4e6,
        2 => 304.8e6,
        5 => 1.0e7,
        6 => 1.0e9,
        13 => 1.0e3,
        _ => return None,
    })
}

/// The solids of a text DXF file by layer, with the warnings; more than `max_triangles` triangles
/// is refused.
pub fn read(text: &str, max_triangles: usize) -> Result<(Vec<Body>, Vec<ReadWarning>), String> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    if lines.len() % 2 == 1 && lines.last().is_some_and(|l| !l.is_empty()) {
        return Err("the file ends in the middle of a pair of lines".to_owned());
    }
    let mut pairs: Vec<(i64, &str)> = Vec::with_capacity(lines.len() / 2);
    for (n, pair) in lines.chunks(2).enumerate() {
        let [code, value] = pair else {
            break;
        };
        let code = code
            .parse::<i64>()
            .map_err(|_| format!("line {}: {code} is not a group code", 2 * n + 1))?;
        pairs.push((code, value));
    }
    if !pairs.contains(&(0, "SECTION")) {
        return Err("the file is not a text DXF file: it has no SECTION".to_owned());
    }
    let mut scale = 1.0e6;
    if let Some(at) = pairs.iter().position(|p| *p == (9, "$INSUNITS")) {
        if let Some((70, value)) = pairs.get(at + 1) {
            let code = value.parse::<i64>().unwrap_or(0);
            scale = unit_of(code).ok_or_else(|| format!("the drawing unit {code} is not known"))?;
        }
    }
    let mut layers: BTreeMap<String, Vec<[f64; 9]>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut ignored: BTreeMap<String, usize> = BTreeMap::new();
    let mut in_entities = false;
    let mut total = 0usize;
    let mut at = 0;
    while at < pairs.len() {
        let (code, value) = pairs[at];
        if code == 2 && at > 0 && pairs[at - 1] == (0, "SECTION") {
            in_entities = value == "ENTITIES";
        }
        if code != 0 || !in_entities || matches!(value, "ENDSEC" | "SECTION" | "EOF") {
            at += 1;
            continue;
        }
        let kind = value;
        let mut end = at + 1;
        while end < pairs.len() && pairs[end].0 != 0 {
            end += 1;
        }
        if kind != "3DFACE" {
            *ignored.entry(kind.to_owned()).or_insert(0) += 1;
            at = end;
            continue;
        }
        let mut layer = "0";
        let mut corner = [[None::<f64>; 3]; 4];
        for (code, value) in &pairs[at + 1..end] {
            match *code {
                8 => layer = value,
                10..=13 | 20..=23 | 30..=33 => {
                    let axis = (*code / 10 - 1) as usize;
                    let k = (*code % 10) as usize;
                    let v = value
                        .parse::<f64>()
                        .map_err(|_| format!("{value} is not a number"))?;
                    corner[k][axis] = Some(v * scale);
                }
                _ => {}
            }
        }
        let mut points = [[0.0f64; 3]; 4];
        for (k, c) in corner.iter().enumerate() {
            for (axis, v) in c.iter().enumerate() {
                let v = v.unwrap_or(if k == 3 {
                    corner[2][axis].unwrap_or(0.0)
                } else {
                    0.0
                });
                if !v.is_finite() || v.abs() > FAR_NM {
                    return Err(
                        "a corner of a face is not a finite place near the origin".to_owned()
                    );
                }
                points[k][axis] = v.round();
            }
        }
        let flatten = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| {
            [a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]]
        };
        let mut faces = vec![flatten(points[0], points[1], points[2])];
        if points[3] != points[2] && points[3] != points[0] {
            faces.push(flatten(points[0], points[2], points[3]));
        }
        total += faces.len();
        if total > max_triangles {
            return Err(format!(
                "the file holds more than {max_triangles} triangles"
            ));
        }
        if !layers.contains_key(layer) {
            order.push(layer.to_owned());
        }
        layers.entry(layer.to_owned()).or_default().extend(faces);
        at = end;
    }
    if layers.is_empty() {
        return Err("the file has no 3DFACE".to_owned());
    }
    let mut warnings = Vec::new();
    for (kind, n) in ignored {
        warnings.push(ReadWarning::Ignored(format!("{n} entities {kind}")));
    }
    let bodies = order
        .into_iter()
        .map(|layer| {
            let triangles = layers.remove(&layer).unwrap_or_default();
            (layer, merge_by(&triangles))
        })
        .collect();
    Ok((bodies, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetra(shift: f64) -> Mesh {
        Mesh {
            vertices: vec![
                [shift, 0.0, 0.0],
                [shift + 10.0e6, 0.0, 0.0],
                [shift, 12.5e6, 0.0],
                [shift, 0.0, 7.0e6],
            ],
            triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        }
    }

    #[test]
    fn two_solids_come_back_on_their_layers_with_their_triangles() {
        let (a, b) = (tetra(0.0), tetra(30.0e6));
        let text = write(&[("first part", &a), ("second", &b)]);
        assert!(text.starts_with("0\nSECTION\n2\nHEADER\n"));
        assert_eq!(text.matches("3DFACE").count(), 8);
        assert!(text.contains("8\nFIRST_PART\n"));
        let (bodies, warnings) = read(&text, 100).unwrap();
        assert!(warnings.is_empty());
        let names: Vec<&str> = bodies.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["FIRST_PART", "SECOND"]);
        assert_eq!(
            crate::interchange::canonical_triangles(&bodies[0].1),
            crate::interchange::canonical_triangles(&a)
        );
        assert_eq!(
            crate::interchange::canonical_triangles(&bodies[1].1),
            crate::interchange::canonical_triangles(&b)
        );
        assert_eq!(text, write(&[("first part", &a), ("second", &b)]));
    }

    #[test]
    fn a_face_of_four_corners_is_cut_in_two_and_the_unit_of_the_header_is_used() {
        let text = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
            0\n3DFACE\n8\nQ\n10\n0\n20\n0\n30\n0\n11\n1\n21\n0\n31\n0\n12\n1\n22\n1\n32\n0\n13\n0\n23\n1\n33\n0\n\
            0\nLINE\n8\nQ\n0\nCIRCLE\n8\nQ\n0\nENDSEC\n0\nEOF\n";
        let (bodies, warnings) = read(text, 10).unwrap();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].1.triangles.len(), 2);
        // inches: the point (1, 0, 0) is 25.4 mm
        assert!(bodies[0].1.vertices.contains(&[25.4e6, 0.0, 0.0]));
        assert_eq!(
            warnings,
            vec![
                ReadWarning::Ignored("1 entities CIRCLE".to_owned()),
                ReadWarning::Ignored("1 entities LINE".to_owned())
            ]
        );
    }

    #[test]
    fn broken_files_are_refused_by_what_is_wrong() {
        let good = write(&[("t", &tetra(0.0))]);
        let e = |t: &str| read(t, 100).unwrap_err();
        assert!(e("hello").contains("not a text DXF") || e("hello").contains("middle of a pair"));
        assert!(e("x\ny\n").contains("group code"));
        assert!(e("0\nSECTION\n2\nENTITIES\n0\nENDSEC\n").contains("no 3DFACE"));
        assert!(e(&good.replace("12.5", "abc")).contains("not a number"));
        assert!(e(&good.replace("12.5", "1e30")).contains("near the origin"));
        assert!(e(&good.replace("70\n4", "70\n99")).contains("unit 99"));
        assert!(read(&good, 3).unwrap_err().contains("more than 3"));
    }

    #[test]
    fn damaged_files_never_panic_with_a_fixed_seed() {
        let base = write(&[("t", &tetra(0.0))]).into_bytes();
        let mut seed = 21u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as usize
        };
        for _ in 0..3000 {
            let mut bytes = base.clone();
            for _ in 0..1 + next() % 4 {
                let at = next() % bytes.len();
                bytes[at] = b" 0123456789-.\nxD"[next() % 16];
            }
            let cut = next() % (bytes.len() + 1);
            let _ = read(&String::from_utf8_lossy(&bytes[..cut]), 100);
        }
    }
}
