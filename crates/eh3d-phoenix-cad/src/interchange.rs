// SPDX-License-Identifier: GPL-3.0-or-later
//! The solids of a model in two more interchange formats: Wavefront OBJ and 3MF.
//!
//! Both write the same triangles as the STL file, in millimetres, one object for each solid
//! (the box and every placed part), and the same bytes for the same model: nothing in them holds a
//! date, a host or a version of the program.
//!
//! * OBJ is text: a group `o` for each solid, its vertices `v` with six decimals and its faces `f`
//!   (counter-clockwise seen from outside), numbered from 1 over the whole file as the format
//!   asks.
//! * 3MF is a package (a zip file) with the model as XML. The package is written without
//!   compression (the method "stored"), with the date of the entries fixed, so that the bytes do
//!   not depend on a library of compression.

use std::fmt::Write as _;

use crate::mesh::{number, solid_name, Mesh};

const NM_PER_MM: f64 = 1_000_000.0;

/// The solids as a Wavefront OBJ file.
pub fn obj(bodies: &[(&str, &Mesh)]) -> String {
    let mut out = String::from("# EH3D PHOENIX-CAD, millimetres\n");
    let mut base = 0usize;
    for (name, mesh) in bodies {
        let _ = writeln!(out, "o {}", solid_name(name));
        for v in &mesh.vertices {
            let _ = writeln!(
                out,
                "v {} {} {}",
                number(v[0] / NM_PER_MM),
                number(v[1] / NM_PER_MM),
                number(v[2] / NM_PER_MM)
            );
        }
        for t in &mesh.triangles {
            let _ = writeln!(
                out,
                "f {} {} {}",
                base + t[0] as usize + 1,
                base + t[1] as usize + 1,
                base + t[2] as usize + 1
            );
        }
        base += mesh.vertices.len();
    }
    out
}

/// The CRC-32 of some bytes (the one of zip files, polynomial `0xEDB88320`).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// A zip file of entries kept as they are (method 0). The date of every entry is 1 January 1980.
fn zip_stored(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    const DATE: u16 = 0x0021;
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let size = data.len() as u32;
        let name_len = name.len() as u16;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&0u16.to_le_bytes()); // stored
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&DATE.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        central.extend_from_slice(&20u16.to_le_bytes()); // needed
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&DATE.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let start = out.len() as u32;
    let count = entries.len() as u16;
    let size = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/></Types>";

const RELATIONSHIPS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>";

/// The XML of the model of a 3MF package.
pub fn three_mf_model(bodies: &[(&str, &Mesh)]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n<resources>\n",
    );
    for (i, (name, mesh)) in bodies.iter().enumerate() {
        let _ = writeln!(
            out,
            "<object id=\"{}\" name=\"{}\" type=\"model\"><mesh><vertices>",
            i + 1,
            solid_name(name)
        );
        for v in &mesh.vertices {
            let _ = writeln!(
                out,
                "<vertex x=\"{}\" y=\"{}\" z=\"{}\"/>",
                number(v[0] / NM_PER_MM),
                number(v[1] / NM_PER_MM),
                number(v[2] / NM_PER_MM)
            );
        }
        out.push_str("</vertices><triangles>\n");
        for t in &mesh.triangles {
            let _ = writeln!(
                out,
                "<triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>",
                t[0], t[1], t[2]
            );
        }
        out.push_str("</triangles></mesh></object>\n");
    }
    out.push_str("</resources>\n<build>\n");
    for i in 0..bodies.len() {
        let _ = writeln!(out, "<item objectid=\"{}\"/>", i + 1);
    }
    out.push_str("</build>\n</model>\n");
    out
}

/// The solids as a 3MF package: a zip file with the content types, the relationship and the model.
pub fn three_mf(bodies: &[(&str, &Mesh)]) -> Vec<u8> {
    zip_stored(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes().to_vec()),
        ("_rels/.rels", RELATIONSHIPS.as_bytes().to_vec()),
        ("3D/3dmodel.model", three_mf_model(bodies).into_bytes()),
    ])
}

/// What a package written by [`three_mf`] holds, read back: the names of the entries with their
/// bytes, each checked against its CRC-32. For the tests and for anyone who wants to check a file
/// this program wrote.
pub fn read_stored_zip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let u16_at = |at: usize| -> Result<usize, String> {
        bytes
            .get(at..at + 2)
            .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
            .ok_or_else(|| "the file ends too soon".to_owned())
    };
    let u32_at = |at: usize| -> Result<u32, String> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| "the file ends too soon".to_owned())
    };
    let mut entries = Vec::new();
    let mut at = 0usize;
    while u32_at(at)? == 0x0403_4b50 {
        if u16_at(at + 8)? != 0 {
            return Err("an entry is compressed".to_owned());
        }
        let crc = u32_at(at + 14)?;
        let size = u32_at(at + 18)? as usize;
        let name_len = u16_at(at + 26)?;
        let extra_len = u16_at(at + 28)?;
        let name_at = at + 30;
        let data_at = name_at + name_len + extra_len;
        let name = bytes
            .get(name_at..name_at + name_len)
            .and_then(|b| std::str::from_utf8(b).ok())
            .ok_or_else(|| "a name is not text".to_owned())?
            .to_owned();
        let data = bytes
            .get(data_at..data_at + size)
            .ok_or_else(|| "an entry ends too soon".to_owned())?
            .to_vec();
        if crc32(&data) != crc {
            return Err(format!("{name}: the CRC-32 does not match"));
        }
        entries.push((name, data));
        at = data_at + size;
    }
    if u32_at(at)? != 0x0201_4b50 {
        return Err("the directory of the package is missing".to_owned());
    }
    Ok(entries)
}

/// One solid read back from a file: its name and its surface in nanometres.
pub type Body = (String, Mesh);

/// The nanometres of a number of millimetres (or of another unit, scaled by `per_unit`).
fn nanometres(text: &str, per_unit: f64, what: &str) -> Result<f64, String> {
    let v: f64 = text
        .parse()
        .map_err(|_| format!("{what}: {text} is not a number"))?;
    if !v.is_finite() {
        return Err(format!("{what}: {text} is not a finite number"));
    }
    Ok((v * per_unit).round())
}

/// The solids of a Wavefront OBJ file, read back (millimetres, as [`obj`] writes them).
///
/// A group `o` starts a solid; the vertices and faces before the first group make a solid named
/// `solid`. The indices of the faces count over the whole file (a negative one counts back from
/// the last vertex read) and a face with more than three corners is cut into a fan of triangles.
/// A face may carry texture and normal numbers (`3/1/2`); they are ignored, like every other line
/// that is not a group, a vertex or a face. A face that uses a vertex of another solid, an index
/// that does not exist or a face with fewer than three corners is an error that says its line.
pub fn read_obj(text: &str) -> Result<Vec<Body>, String> {
    let mut bodies: Vec<Body> = Vec::new();
    let mut total = 0usize;
    // the first vertex (counted from 0 over the file) of the solid that is being read
    let mut first = 0usize;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let at = n + 1;
        let mut words = line.split_whitespace();
        match words.next() {
            Some("o") => {
                let name = words.collect::<Vec<_>>().join(" ");
                first = total;
                bodies.push((name, Mesh::default()));
            }
            Some("v") => {
                let w: Vec<&str> = words.collect();
                if w.len() < 3 {
                    return Err(format!("line {at}: a vertex needs three numbers"));
                }
                if bodies.is_empty() {
                    first = total;
                    bodies.push(("solid".to_owned(), Mesh::default()));
                }
                let what = format!("line {at}");
                let v = [
                    nanometres(w[0], NM_PER_MM, &what)?,
                    nanometres(w[1], NM_PER_MM, &what)?,
                    nanometres(w[2], NM_PER_MM, &what)?,
                ];
                if let Some(body) = bodies.last_mut() {
                    body.1.vertices.push(v);
                }
                total += 1;
            }
            Some("f") => {
                let mut corners = Vec::new();
                for w in words {
                    let head = w.split('/').next().unwrap_or("");
                    let k: i64 = head
                        .parse()
                        .map_err(|_| format!("line {at}: {w} is not a vertex number"))?;
                    let global = if k < 0 {
                        i64::try_from(total).unwrap_or(i64::MAX) + k
                    } else {
                        k - 1
                    };
                    let global = usize::try_from(global)
                        .ok()
                        .filter(|g| *g < total)
                        .ok_or_else(|| format!("line {at}: there is no vertex {k}"))?;
                    if global < first {
                        return Err(format!("line {at}: a face uses a vertex of another solid"));
                    }
                    corners.push(u32::try_from(global - first).map_err(|_| {
                        format!("line {at}: the solid has more vertices than a face can number")
                    })?);
                }
                if corners.len() < 3 {
                    return Err(format!("line {at}: a face needs three corners"));
                }
                let Some(body) = bodies.last_mut() else {
                    return Err(format!("line {at}: a face before any vertex"));
                };
                for k in 1..corners.len() - 1 {
                    body.1
                        .triangles
                        .push([corners[0], corners[k], corners[k + 1]]);
                }
            }
            _ => {}
        }
    }
    Ok(bodies)
}

/// The text of the attribute `name` of an XML tag (`<vertex x="1"/>` gives `1` for `x`).
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let from = tag.find(&key)? + key.len();
    let len = tag[from..].find('"')?;
    Some(&tag[from..from + len])
}

/// How many nanometres a unit of the 3MF model is.
fn unit_nanometres(unit: &str) -> Option<f64> {
    match unit {
        "micron" => Some(1_000.0),
        "millimeter" => Some(NM_PER_MM),
        "centimeter" => Some(10.0 * NM_PER_MM),
        "inch" => Some(25.4 * NM_PER_MM),
        "foot" => Some(304.8 * NM_PER_MM),
        "meter" => Some(1000.0 * NM_PER_MM),
        _ => None,
    }
}

/// The solids of the model of a 3MF package written like [`three_mf`], read back: the package must
/// be stored without compression and its model must be in `3D/3dmodel.model`.
pub fn read_three_mf(bytes: &[u8]) -> Result<Vec<Body>, String> {
    let entries = read_stored_zip(bytes)?;
    let model = entries
        .iter()
        .find(|(name, _)| name == "3D/3dmodel.model")
        .ok_or_else(|| "the package has no 3D/3dmodel.model".to_owned())?;
    let xml = std::str::from_utf8(&model.1).map_err(|_| "the model is not text".to_owned())?;
    let head = xml
        .find("<model")
        .and_then(|i| xml[i..].find('>').map(|j| &xml[i..i + j]))
        .ok_or_else(|| "the model has no <model> element".to_owned())?;
    let unit = attribute(head, "unit").unwrap_or("millimeter");
    let scale = unit_nanometres(unit).ok_or_else(|| format!("the unit {unit} is not known"))?;
    let mut bodies = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<object ") {
        let after = &rest[i..];
        let end = after
            .find("</object>")
            .ok_or_else(|| "an object is not closed".to_owned())?;
        let object = &after[..end];
        let open = object.find('>').unwrap_or(object.len());
        let name = attribute(&object[..open], "name").unwrap_or("").to_owned();
        let mut mesh = Mesh::default();
        for tag in object.split('<').skip(1) {
            if tag.starts_with("vertex ") {
                let get = |k: &str| {
                    attribute(tag, k)
                        .ok_or_else(|| format!("a vertex of {name} has no {k}"))
                        .and_then(|t| nanometres(t, scale, "a vertex"))
                };
                mesh.vertices.push([get("x")?, get("y")?, get("z")?]);
            } else if tag.starts_with("triangle ") {
                let get = |k: &str| {
                    attribute(tag, k)
                        .and_then(|t| t.parse::<u32>().ok())
                        .filter(|v| (*v as usize) < mesh.vertices.len())
                        .ok_or_else(|| format!("a triangle of {name} has no vertex {k}"))
                };
                let t = [get("v1")?, get("v2")?, get("v3")?];
                mesh.triangles.push(t);
            }
        }
        bodies.push((name, mesh));
        rest = &after[end + "</object>".len()..];
    }
    Ok(bodies)
}

/// The surface of the triangles that [`crate::mesh::read_stl`] gives (nine millimetres each): a
/// vertex that lies at the same nanometre as another is the same vertex, as in the file this
/// program writes, so that the surface can be checked for being closed.
pub fn mesh_from_triangles(triangles: &[[f64; 9]]) -> Mesh {
    use std::collections::HashMap;
    let mut index: HashMap<[i64; 3], u32> = HashMap::new();
    let mut mesh = Mesh::default();
    for t in triangles {
        let mut corners = [0u32; 3];
        for (k, corner) in corners.iter_mut().enumerate() {
            let v = [
                (t[3 * k] * NM_PER_MM).round(),
                (t[3 * k + 1] * NM_PER_MM).round(),
                (t[3 * k + 2] * NM_PER_MM).round(),
            ];
            let key = [v[0] as i64, v[1] as i64, v[2] as i64];
            *corner = *index.entry(key).or_insert_with(|| {
                mesh.vertices.push(v);
                (mesh.vertices.len() - 1) as u32
            });
        }
        mesh.triangles.push(corners);
    }
    mesh
}

/// The solids of a file of an interchange format, by its extension: `stl` (one solid, called
/// `solid`), `obj` or `3mf`.
pub fn read_file_bytes(name: &str, bytes: &[u8]) -> Result<Vec<Body>, String> {
    let extension = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match extension.as_str() {
        "stl" => {
            let triangles = crate::mesh::read_stl(bytes)?;
            Ok(vec![("solid".to_owned(), mesh_from_triangles(&triangles))])
        }
        "obj" => read_obj(
            std::str::from_utf8(bytes).map_err(|_| "the file is not UTF-8 text".to_owned())?,
        ),
        "3mf" => read_three_mf(bytes),
        _ => Err("the file must end in .stl, .obj or .3mf".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cube of 10 mm with a corner at the origin.
    fn cube(origin: f64) -> Mesh {
        let s = 10.0 * NM_PER_MM;
        let o = origin * NM_PER_MM;
        let mut vertices = Vec::new();
        for z in [0.0, s] {
            for y in [0.0, s] {
                for x in [0.0, s] {
                    vertices.push([o + x, y, z]);
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

    #[test]
    fn the_crc_is_the_one_of_zip_files() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn the_obj_file_numbers_the_vertices_over_the_whole_file() {
        let a = cube(0.0);
        let b = cube(20.0);
        let text = obj(&[("box", &a), ("part one", &b)]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "# EH3D PHOENIX-CAD, millimetres");
        assert_eq!(lines[1], "o box");
        assert_eq!(lines[2], "v 0.000000 0.000000 0.000000");
        assert_eq!(lines[3], "v 10.000000 0.000000 0.000000");
        assert_eq!(lines[10], "f 1 3 2");
        // 1 comment, 2 groups, 16 vertices and 24 faces
        assert_eq!(lines.len(), 1 + 2 + 16 + 24);
        assert_eq!(lines[1 + 1 + 8 + 12], "o part_one");
        // the second solid starts at vertex 9: its first face is the first of the cube shifted by 8
        let first_face_of_second = lines.iter().rposition(|l| *l == "f 9 11 10");
        assert!(first_face_of_second.is_some(), "{text}");
        assert!(lines.contains(&"v 20.000000 0.000000 0.000000"));
        // the same model gives the same bytes
        assert_eq!(text, obj(&[("box", &a), ("part one", &b)]));
    }

    #[test]
    fn the_package_of_3mf_reads_back_with_its_three_entries() {
        let a = cube(0.0);
        let b = cube(20.0);
        let bytes = three_mf(&[("box", &a), ("part", &b)]);
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        let entries = read_stored_zip(&bytes).unwrap();
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["[Content_Types].xml", "_rels/.rels", "3D/3dmodel.model"]
        );
        let model = String::from_utf8(entries[2].1.clone()).unwrap();
        assert_eq!(model.matches("<vertex ").count(), 16);
        assert_eq!(model.matches("<triangle ").count(), 24);
        assert_eq!(model.matches("<object ").count(), 2);
        assert_eq!(model.matches("<item ").count(), 2);
        assert!(model.contains("unit=\"millimeter\""));
        assert!(model.contains("<vertex x=\"20.000000\" y=\"0.000000\" z=\"0.000000\"/>"));
        assert!(model.contains("<triangle v1=\"0\" v2=\"2\" v3=\"1\"/>"));
        // the ends of the file: the directory has three entries
        let end = bytes.len() - 22;
        assert_eq!(&bytes[end..end + 4], b"PK\x05\x06");
        assert_eq!(u16::from_le_bytes([bytes[end + 10], bytes[end + 11]]), 3);
        // the same model gives the same bytes
        assert_eq!(bytes, three_mf(&[("box", &a), ("part", &b)]));
    }

    #[test]
    fn a_damaged_package_is_not_read() {
        let a = cube(0.0);
        let mut bytes = three_mf(&[("box", &a)]);
        assert!(read_stored_zip(&bytes[..10]).is_err());
        // a byte of the model changed breaks its CRC-32
        let at = bytes.len() - 400;
        bytes[at] ^= 0xFF;
        assert!(read_stored_zip(&bytes).is_err());
    }

    #[test]
    fn the_obj_file_reads_back_to_the_same_solids() {
        let a = cube(0.0);
        let b = cube(20.0);
        let text = obj(&[("box", &a), ("part one", &b)]);
        let read = read_obj(&text).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].0, "box");
        assert_eq!(read[1].0, "part_one");
        assert_eq!(read[0].1, a);
        assert_eq!(read[1].1, b);
        assert!(read[1].1.check_closed().is_ok());
        assert!((read[1].1.volume_nm3() - 1.0e21).abs() < 1.0e9);
        // what was read is written again to the same bytes
        let again: Vec<(&str, &Mesh)> = read.iter().map(|(n, m)| (n.as_str(), m)).collect();
        assert_eq!(obj(&again), text);
    }

    #[test]
    fn the_obj_reader_takes_quads_negative_indices_and_lines_it_does_not_use() {
        let text = "# a square and a triangle\nmtllib x.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvn 0 0 1\ns off\nf 1/1/1 2/1/1 3/1/1 4/1/1\nf -4 -3 -2\n";
        let read = read_obj(text).unwrap();
        // no group: the vertices make a solid called solid
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].0, "solid");
        assert_eq!(read[0].1.vertices[2], [1.0e6, 1.0e6, 0.0]);
        // a quad is two triangles, plus the one with negative numbers
        assert_eq!(read[0].1.triangles, vec![[0, 1, 2], [0, 2, 3], [0, 1, 2]]);
        // a name with spaces is kept as it is
        assert_eq!(read_obj("o two words\n").unwrap()[0].0, "two words");
        assert!(read_obj("").unwrap().is_empty());
    }

    #[test]
    fn a_damaged_obj_file_is_refused_with_its_line() {
        for (text, line) in [
            ("v 1 2\n", "line 1"),
            ("v 1 x 3\n", "line 1"),
            ("v 1 NaN 3\n", "line 1"),
            ("o a\nv 0 0 0\nf 1 2 3\n", "line 3"),
            ("o a\nv 0 0 0\nv 1 0 0\nf 1 2\n", "line 4"),
            ("o a\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 x\n", "line 5"),
            ("o a\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 -9\n", "line 5"),
            ("f 1 2 3\n", "line 1"),
            // a face of the second solid that names a vertex of the first
            (
                "o a\nv 0 0 0\nv 1 0 0\nv 0 1 0\no b\nv 5 5 5\nf 1 2 4\n",
                "line 7",
            ),
        ] {
            let e = read_obj(text).unwrap_err();
            assert!(e.starts_with(line), "{text:?}: {e}");
        }
    }

    #[test]
    fn the_3mf_package_reads_back_to_the_same_solids_and_in_the_unit_it_names() {
        let a = cube(0.0);
        let b = cube(20.0);
        let bytes = three_mf(&[("box", &a), ("part", &b)]);
        let read = read_three_mf(&bytes).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!((read[0].0.as_str(), &read[0].1), ("box", &a));
        assert_eq!((read[1].0.as_str(), &read[1].1), ("part", &b));
        // a model in centimetres is ten times as large as the same numbers in millimetres
        let model =
            three_mf_model(&[("box", &a)]).replace("unit=\"millimeter\"", "unit=\"centimeter\"");
        let package = zip_stored(&[("3D/3dmodel.model", model.into_bytes())]);
        let big = read_three_mf(&package).unwrap();
        assert!((big[0].1.volume_nm3() - 1.0e24).abs() < 1.0e12);
        // a unit that is not known, a package with no model, a triangle that names a vertex that
        // does not exist and an object that is not closed are refused
        let model = three_mf_model(&[("box", &a)]).replace("millimeter", "furlong");
        let package = zip_stored(&[("3D/3dmodel.model", model.into_bytes())]);
        assert!(read_three_mf(&package).unwrap_err().contains("furlong"));
        let package = zip_stored(&[("other.xml", b"<a/>".to_vec())]);
        assert!(read_three_mf(&package).is_err());
        let model = three_mf_model(&[("box", &a)]).replace("v3=\"1\"", "v3=\"99\"");
        let package = zip_stored(&[("3D/3dmodel.model", model.into_bytes())]);
        assert!(read_three_mf(&package).unwrap_err().contains("v3"));
        let model = three_mf_model(&[("box", &a)]).replace("</object>", "");
        let package = zip_stored(&[("3D/3dmodel.model", model.into_bytes())]);
        assert!(read_three_mf(&package).is_err());
        assert!(read_three_mf(b"not a package").is_err());
    }

    #[test]
    fn a_stl_file_is_welded_into_a_closed_surface_with_the_same_volume() {
        let a = cube(0.0);
        let stl = crate::mesh::stl_binary(&[("box", &a)]);
        let read = read_file_bytes("box.STL", &stl).unwrap();
        assert_eq!(read.len(), 1);
        // the cube has 8 vertices and 12 triangles however many times the file repeats them
        assert_eq!(read[0].1.vertices.len(), 8);
        assert_eq!(read[0].1.triangles.len(), 12);
        assert!(read[0].1.check_closed().is_ok());
        assert!((read[0].1.volume_nm3() - 1.0e21).abs() < 1.0e12);
        // the text form gives the same surface
        let text = crate::mesh::stl_ascii(&[("box", &a)]).into_bytes();
        assert_eq!(read_file_bytes("box.stl", &text).unwrap(), read);
        // a file that is not closed says so
        let mut open = read[0].1.clone();
        open.triangles.pop();
        assert!(open.check_closed().is_err());
    }

    #[test]
    fn the_reader_picks_its_format_from_the_extension() {
        let a = cube(0.0);
        let obj_text = obj(&[("box", &a)]).into_bytes();
        let three = three_mf(&[("box", &a)]);
        assert_eq!(read_file_bytes("a.obj", &obj_text).unwrap()[0].1, a);
        assert_eq!(read_file_bytes("dir.v2/A.3MF", &three).unwrap()[0].1, a);
        assert!(read_file_bytes("a.step", &obj_text).is_err());
        assert!(read_file_bytes("a", &obj_text).is_err());
        assert!(read_file_bytes("a.obj", &[0xFF, 0xFE]).is_err());
        assert!(read_file_bytes("a.stl", b"short").is_err());
    }

    #[test]
    fn the_volume_of_the_triangles_of_the_cube_is_a_thousand_cubic_millimetres() {
        let a = cube(0.0);
        assert!((a.volume_nm3() - 1.0e21).abs() < 1.0e9);
    }
}
