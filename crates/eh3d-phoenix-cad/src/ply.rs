// SPDX-License-Identifier: GPL-3.0-or-later
//! The Stanford PLY file, the format of scanners and of many mesh programs: read in its three
//! encodings (text, binary little endian and binary big endian) and written as text or as binary
//! little endian.
//!
//! The reader takes the vertices (`x`, `y`, `z`) and the faces (the list `vertex_indices` or
//! `vertex_index`, a face of more than three corners being cut into a fan of triangles) and says,
//! as a warning with a name, every element it leaves out (colours, normals and texture
//! coordinates of the vertices are skipped without a word: they are not shape). Coordinates are in
//! millimetres, as in the other formats. It checks every count against the bytes that are left
//! before it reserves anything, refuses indices that do not exist and never panics.

use crate::interchange::Body;
use crate::mesh::Mesh;
use crate::stlread::ReadWarning;

/// The nanometres of a millimetre.
const NM_PER_MM: f64 = 1.0e6;

/// The farthest a coordinate may be from the origin, in nanometres (a thousand kilometres).
const FAR_NM: f64 = 1.0e15;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Encoding {
    Ascii,
    Little,
    Big,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scalar {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Scalar {
    fn parse(word: &str) -> Option<Scalar> {
        Some(match word {
            "char" | "int8" => Scalar::I8,
            "uchar" | "uint8" => Scalar::U8,
            "short" | "int16" => Scalar::I16,
            "ushort" | "uint16" => Scalar::U16,
            "int" | "int32" => Scalar::I32,
            "uint" | "uint32" => Scalar::U32,
            "float" | "float32" => Scalar::F32,
            "double" | "float64" => Scalar::F64,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            Scalar::I8 | Scalar::U8 => 1,
            Scalar::I16 | Scalar::U16 => 2,
            Scalar::I32 | Scalar::U32 | Scalar::F32 => 4,
            Scalar::F64 => 8,
        }
    }
}

#[derive(Debug)]
struct Property {
    name: String,
    /// The type of the count when the property is a list.
    list: Option<Scalar>,
    kind: Scalar,
}

#[derive(Debug)]
struct Element {
    name: String,
    count: usize,
    properties: Vec<Property>,
}

/// A reader of the values after the header.
struct Values<'a> {
    encoding: Encoding,
    bytes: &'a [u8],
    at: usize,
    words: std::str::SplitAsciiWhitespace<'a>,
}

impl Values<'_> {
    fn next(&mut self, kind: Scalar) -> Result<f64, String> {
        if self.encoding == Encoding::Ascii {
            let word = self
                .words
                .next()
                .ok_or_else(|| "the file ends before its data do".to_owned())?;
            let value: f64 = word
                .parse()
                .map_err(|_| format!("{word} is not a number"))?;
            return Ok(value);
        }
        let size = kind.size();
        let raw = self
            .bytes
            .get(self.at..self.at + size)
            .ok_or_else(|| "the file ends before its data do".to_owned())?;
        self.at += size;
        let mut b = [0u8; 8];
        b[..size].copy_from_slice(raw);
        if self.encoding == Encoding::Big {
            b[..size].reverse();
        }
        Ok(match kind {
            Scalar::I8 => f64::from(b[0] as i8),
            Scalar::U8 => f64::from(b[0]),
            Scalar::I16 => f64::from(i16::from_le_bytes([b[0], b[1]])),
            Scalar::U16 => f64::from(u16::from_le_bytes([b[0], b[1]])),
            Scalar::I32 => f64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            Scalar::U32 => f64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            Scalar::F32 => f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            Scalar::F64 => f64::from_le_bytes(b),
        })
    }

    /// The bytes that are left to read (a count of elements cannot be larger than this).
    fn left(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }
}

/// The solid of a PLY file, with the warnings; more than `max_triangles` triangles is refused.
pub fn read(bytes: &[u8], max_triangles: usize) -> Result<(Body, Vec<ReadWarning>), String> {
    let end = bytes
        .windows(10)
        .position(|w| w == b"end_header")
        .ok_or_else(|| "the file has no end_header".to_owned())?;
    let after = end + 10;
    let body_at = match bytes.get(after..) {
        Some([b'\r', b'\n', ..]) => after + 2,
        Some([b'\n', ..]) | Some([b'\r', ..]) => after + 1,
        _ => return Err("the header does not end with a line".to_owned()),
    };
    let header =
        std::str::from_utf8(&bytes[..end]).map_err(|_| "the header is not text".to_owned())?;
    let mut lines = header.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some("ply") {
        return Err("the file does not start with ply".to_owned());
    }
    let mut encoding = None;
    let mut elements: Vec<Element> = Vec::new();
    for line in lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["format", kind, _] => {
                encoding = Some(match *kind {
                    "ascii" => Encoding::Ascii,
                    "binary_little_endian" => Encoding::Little,
                    "binary_big_endian" => Encoding::Big,
                    other => return Err(format!("the format {other} is not read")),
                })
            }
            ["comment", ..] | ["obj_info", ..] => {}
            ["element", name, count] => elements.push(Element {
                name: (*name).to_owned(),
                count: count
                    .parse()
                    .map_err(|_| format!("{count} is not a count of elements"))?,
                properties: Vec::new(),
            }),
            ["property", "list", count, kind, name] => {
                let element = elements
                    .last_mut()
                    .ok_or("a property comes before any element")?;
                element.properties.push(Property {
                    name: (*name).to_owned(),
                    list: Some(
                        Scalar::parse(count)
                            .ok_or_else(|| format!("the type {count} is not known"))?,
                    ),
                    kind: Scalar::parse(kind)
                        .ok_or_else(|| format!("the type {kind} is not known"))?,
                });
            }
            ["property", kind, name] => {
                let element = elements
                    .last_mut()
                    .ok_or("a property comes before any element")?;
                element.properties.push(Property {
                    name: (*name).to_owned(),
                    list: None,
                    kind: Scalar::parse(kind)
                        .ok_or_else(|| format!("the type {kind} is not known"))?,
                });
            }
            _ => return Err(format!("{line}: this line of the header is not understood")),
        }
    }
    let encoding = encoding.ok_or("the header has no format")?;
    let data = &bytes[body_at..];
    let text;
    let mut values = Values {
        encoding,
        bytes: data,
        at: 0,
        words: "".split_ascii_whitespace(),
    };
    if encoding == Encoding::Ascii {
        text = std::str::from_utf8(data).map_err(|_| "the data are not text".to_owned())?;
        values.words = text.split_ascii_whitespace();
    }
    let mut mesh = Mesh::default();
    let mut warnings = Vec::new();
    let mut saw_vertex = false;
    for element in &elements {
        let smallest = element
            .properties
            .iter()
            .map(|p| p.kind.size())
            .sum::<usize>()
            .max(1);
        if encoding != Encoding::Ascii
            && element.properties.iter().all(|p| p.list.is_none())
            && element.count.saturating_mul(smallest) > values.left()
        {
            return Err(format!(
                "the file says {} {} and does not hold them",
                element.count, element.name
            ));
        }
        if element.count > max_triangles.saturating_mul(3).max(1_000_000) {
            return Err(format!(
                "the file says {} {}, which is more than is read",
                element.count, element.name
            ));
        }
        match element.name.as_str() {
            "vertex" => {
                saw_vertex = true;
                let find = |n: &str| element.properties.iter().position(|p| p.name == n);
                let (Some(ix), Some(iy), Some(iz)) = (find("x"), find("y"), find("z")) else {
                    return Err("the vertices have no x, y and z".to_owned());
                };
                mesh.vertices.reserve(element.count.min(1 << 20));
                for _ in 0..element.count {
                    let mut p = [0.0f64; 3];
                    for (k, property) in element.properties.iter().enumerate() {
                        if let Some(kind_count) = property.list {
                            let n = values.next(kind_count)?;
                            for _ in 0..(n.max(0.0) as usize).min(1 << 20) {
                                values.next(property.kind)?;
                            }
                            continue;
                        }
                        let v = values.next(property.kind)?;
                        if k == ix {
                            p[0] = v;
                        } else if k == iy {
                            p[1] = v;
                        } else if k == iz {
                            p[2] = v;
                        }
                    }
                    let nm = [p[0] * NM_PER_MM, p[1] * NM_PER_MM, p[2] * NM_PER_MM];
                    if nm.iter().any(|c| !c.is_finite() || c.abs() > FAR_NM) {
                        return Err("a vertex is not a finite place near the origin".to_owned());
                    }
                    mesh.vertices
                        .push([nm[0].round(), nm[1].round(), nm[2].round()]);
                }
            }
            "face" => {
                let Some(list) = element.properties.iter().find(|p| {
                    p.list.is_some() && matches!(p.name.as_str(), "vertex_indices" | "vertex_index")
                }) else {
                    return Err("the faces have no list of vertices".to_owned());
                };
                for _ in 0..element.count {
                    let mut corners: Vec<u32> = Vec::new();
                    for property in &element.properties {
                        match property.list {
                            Some(kind_count) => {
                                let n = values.next(kind_count)?;
                                if !(0.0..=1.0e6).contains(&n) {
                                    return Err(
                                        "a face lists a count of vertices that is not valid"
                                            .to_owned(),
                                    );
                                }
                                let wanted = std::ptr::eq(property, list);
                                for _ in 0..n as usize {
                                    let v = values.next(property.kind)?;
                                    if wanted {
                                        if v < 0.0
                                            || v >= mesh.vertices.len() as f64
                                            || v.fract() != 0.0
                                        {
                                            return Err(format!(
                                                "a face names the vertex {v}, which does not exist"
                                            ));
                                        }
                                        corners.push(v as u32);
                                    }
                                }
                            }
                            None => {
                                values.next(property.kind)?;
                            }
                        }
                    }
                    if corners.len() < 3 {
                        return Err("a face has fewer than three vertices".to_owned());
                    }
                    for k in 1..corners.len() - 1 {
                        if mesh.triangles.len() >= max_triangles {
                            return Err(format!(
                                "the file holds more than {max_triangles} triangles"
                            ));
                        }
                        mesh.triangles
                            .push([corners[0], corners[k], corners[k + 1]]);
                    }
                }
            }
            other => {
                // an element that is not read is skipped value by value
                for _ in 0..element.count {
                    for property in &element.properties {
                        match property.list {
                            Some(kind_count) => {
                                let n = values.next(kind_count)?;
                                if !(0.0..=1.0e7).contains(&n) {
                                    return Err("a list has a count that is not valid".to_owned());
                                }
                                for _ in 0..n as usize {
                                    values.next(property.kind)?;
                                }
                            }
                            None => {
                                values.next(property.kind)?;
                            }
                        }
                    }
                }
                warnings.push(ReadWarning::Ignored(format!("the element {other}")));
            }
        }
    }
    if !saw_vertex {
        return Err("the file has no vertices".to_owned());
    }
    Ok((("ply".to_owned(), mesh), warnings))
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

/// The solids as one PLY text file (the solids are put in one, as the format has one object).
pub fn write_ascii(bodies: &[(&str, &Mesh)]) -> String {
    let mesh = crate::interchange::merge(bodies);
    let mut out = String::from("ply\nformat ascii 1.0\ncomment EH3D PHOENIX-CAD, millimetres\n");
    out.push_str(&format!(
        "element vertex {}\nproperty float x\nproperty float y\nproperty float z\n",
        mesh.vertices.len()
    ));
    out.push_str(&format!(
        "element face {}\nproperty list uchar int vertex_indices\nend_header\n",
        mesh.triangles.len()
    ));
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

/// The solids as one binary PLY file, little endian, with 32-bit floats.
pub fn write_binary(bodies: &[(&str, &Mesh)]) -> Vec<u8> {
    let mesh = crate::interchange::merge(bodies);
    let mut out = format!(
        "ply\nformat binary_little_endian 1.0\ncomment EH3D PHOENIX-CAD, millimetres\n\
         element vertex {}\nproperty float x\nproperty float y\nproperty float z\n\
         element face {}\nproperty list uchar int vertex_indices\nend_header\n",
        mesh.vertices.len(),
        mesh.triangles.len()
    )
    .into_bytes();
    for v in &mesh.vertices {
        for c in v {
            out.extend_from_slice(&((c / NM_PER_MM) as f32).to_le_bytes());
        }
    }
    for t in &mesh.triangles {
        out.push(3);
        for k in t {
            out.extend_from_slice(&(*k as i32).to_le_bytes());
        }
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
    fn text_and_binary_files_read_back_to_the_same_solid() {
        let m = tetra();
        let (text, w1) = read(write_ascii(&[("t", &m)]).as_bytes(), 100).unwrap();
        let (bin, w2) = read(&write_binary(&[("t", &m)]), 100).unwrap();
        assert_eq!(text.1, m);
        assert_eq!(bin.1, m);
        assert!(w1.is_empty() && w2.is_empty());
        assert_eq!(text.0, "ply");
    }

    #[test]
    fn the_same_data_in_big_endian_with_other_types_and_extra_properties_is_read() {
        let mut bytes = b"ply\r\nformat binary_big_endian 1.0\r\ncomment made elsewhere\r\n\
            element vertex 3\r\nproperty double x\r\nproperty double y\r\nproperty double z\r\n\
            property uchar red\r\nproperty float nx\r\n\
            element face 1\r\nproperty uchar flags\r\nproperty list ushort uint vertex_index\r\n\
            element edge 1\r\nproperty int a\r\nproperty int b\r\nend_header\r\n"
            .to_vec();
        for v in [[0.0f64, 0.0, 0.0], [5.0, 0.0, 0.0], [0.0, 5.0, 0.0]] {
            for c in v {
                bytes.extend_from_slice(&c.to_be_bytes());
            }
            bytes.push(200);
            bytes.extend_from_slice(&1.0f32.to_be_bytes());
        }
        bytes.push(9);
        bytes.extend_from_slice(&3u16.to_be_bytes());
        for k in [0u32, 1, 2] {
            bytes.extend_from_slice(&k.to_be_bytes());
        }
        bytes.extend_from_slice(&0i32.to_be_bytes());
        bytes.extend_from_slice(&1i32.to_be_bytes());
        let (body, warnings) = read(&bytes, 10).unwrap();
        assert_eq!(body.1.triangles, vec![[0, 1, 2]]);
        assert_eq!(body.1.vertices[1], [5.0e6, 0.0, 0.0]);
        assert_eq!(
            warnings,
            vec![ReadWarning::Ignored("the element edge".to_owned())]
        );
        assert_eq!(warnings[0].name(), "ignored");
    }

    #[test]
    fn a_face_with_more_than_three_corners_is_cut_into_a_fan() {
        let text = "ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\n\
                    property float z\nelement face 1\nproperty list uchar int vertex_indices\n\
                    end_header\n0 0 0\n1 0 0\n1 1 0\n0 1 0\n4 0 1 2 3\n";
        let (body, _) = read(text.as_bytes(), 10).unwrap();
        assert_eq!(body.1.triangles, vec![[0, 1, 2], [0, 2, 3]]);
    }

    #[test]
    fn broken_files_are_refused_by_what_is_wrong() {
        let m = tetra();
        let good = write_ascii(&[("t", &m)]);
        let e = |text: &str| read(text.as_bytes(), 100).unwrap_err();
        assert!(e("hello").contains("end_header"));
        assert!(e("hello\nend_header\n").contains("does not start with ply"));
        assert!(e(&good.replace("format ascii 1.0", "format xml 1.0")).contains("xml"));
        assert!(e(&good.replace("3 0 2 1", "3 0 2 9")).contains("vertex 9"));
        assert!(e(&good.replace("3 0 2 1", "2 0 2")).contains("fewer than three"));
        assert!(e(&good.replace(
            "3 1 2 3
", ""
        ))
        .contains("ends before"));
        assert!(e(&good.replace("property float y\n", "")).contains("no x, y and z"));
        assert!(e(&good.replace("property float x", "property bits x")).contains("bits"));
        assert!(e(&good.replace("0 0 0\n", "nan 0 0\n")).contains("finite"));
        assert!(read(good.as_bytes(), 3)
            .unwrap_err()
            .contains("more than 3"));
        let mut bin = write_binary(&[("t", &m)]);
        bin.truncate(bin.len() - 6);
        assert!(read(&bin, 100).is_err());
        let huge = good.replace("element vertex 4", "element vertex 99999999999");
        assert!(e(&huge).contains("more than is read"));
        let mut huge = b"ply
format binary_little_endian 1.0
element vertex 4000000
property float x
            property float y
property float z
end_header
"
        .to_vec();
        huge.extend_from_slice(&[0u8; 12]);
        assert!(read(&huge, 100).unwrap_err().contains("does not hold"));
    }

    #[test]
    fn the_text_file_writes_short_numbers_and_the_same_solid_gives_the_same_bytes() {
        let m = tetra();
        let text = write_ascii(&[("t", &m)]);
        assert!(text.contains("\n10 0 0\n0 12.5 0\n"));
        assert_eq!(text, write_ascii(&[("t", &m)]));
        assert_eq!(write_binary(&[("t", &m)]), write_binary(&[("t", &m)]));
    }

    #[test]
    fn damaged_files_never_panic_with_a_fixed_seed() {
        let m = tetra();
        let bases = [
            write_ascii(&[("t", &m)]).into_bytes(),
            write_binary(&[("t", &m)]),
        ];
        let mut seed = 31u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as usize
        };
        for _ in 0..2000 {
            for base in &bases {
                let mut bytes = base.clone();
                for _ in 0..1 + next() % 4 {
                    let at = next() % bytes.len();
                    bytes[at] = next() as u8;
                }
                let cut = next() % (bytes.len() + 1);
                let _ = read(&bytes[..cut], 1000);
            }
        }
    }
}
