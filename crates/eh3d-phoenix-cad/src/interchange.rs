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
    fn the_volume_of_the_triangles_of_the_cube_is_a_thousand_cubic_millimetres() {
        let a = cube(0.0);
        assert!((a.volume_nm3() - 1.0e21).abs() < 1.0e9);
    }
}
