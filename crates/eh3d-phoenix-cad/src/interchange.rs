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

/// The colour a material is drawn with in the coloured files: the same name always gives the same
/// colour, with a hue taken from the name (a hash of its bytes) and fixed saturation and value, so
/// that the colours of the materials of a model differ from each other and no file holds a colour
/// that depends on the order of the parts. No material (the box) is a light grey.
pub fn material_colour(material: &str) -> [u8; 3] {
    if material.is_empty() {
        return [190, 190, 190];
    }
    let mut hash: u32 = 0x811c_9dc5;
    for b in material.bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let (hue, sat, val) = (hash % 360, 55u32, 85u32);
    let (sector, f) = (hue / 60, hue % 60);
    let p = val * (100 - sat) / 100;
    let q = val * (6000 - sat * f) / 6000;
    let t = val * (6000 - sat * (60 - f)) / 6000;
    let (r, g, b) = match sector {
        0 => (val, t, p),
        1 => (q, val, p),
        2 => (p, val, t),
        3 => (p, q, val),
        4 => (t, p, val),
        _ => (val, p, q),
    };
    let scale = |x: u32| u8::try_from((x * 255 + 50) / 100).unwrap_or(255);
    [scale(r), scale(g), scale(b)]
}

/// The colour as `#RRGGBB`.
fn hex_colour(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// The solids as a Wavefront OBJ file with the colour of the material of each solid on every
/// vertex line (`v x y z r g b`, the three colours from 0 to 1), the way viewers that show vertex
/// colours read it. The third word of a solid is its material (empty for the box). A reader that
/// does not know the colours reads the same solids, as [`read_obj`] does.
pub fn obj_coloured(bodies: &[(&str, &Mesh, &str)]) -> String {
    let mut out = String::from("# EH3D PHOENIX-CAD, millimetres, colours by material\n");
    let mut base = 0usize;
    for (name, mesh, material) in bodies {
        let c = material_colour(material);
        let rgb = c.map(|x| format!("{:.3}", f64::from(x) / 255.0));
        let _ = writeln!(out, "o {}", solid_name(name));
        for v in &mesh.vertices {
            let _ = writeln!(
                out,
                "v {} {} {} {} {} {}",
                number(v[0] / NM_PER_MM),
                number(v[1] / NM_PER_MM),
                number(v[2] / NM_PER_MM),
                rgb[0],
                rgb[1],
                rgb[2]
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
    model_xml(bodies, None)
}

/// The XML of the model of a 3MF package with a list of base materials, one for each different
/// material of the solids (in the order they first appear), each with its colour
/// ([`material_colour`]); every solid that has a material points at its entry. The box, which has
/// none, has no colour of its own.
pub fn three_mf_model_coloured(bodies: &[(&str, &Mesh, &str)]) -> String {
    let list: Vec<(&str, &Mesh)> = bodies.iter().map(|(n, m, _)| (*n, *m)).collect();
    let materials: Vec<&str> = bodies.iter().map(|b| b.2).collect();
    model_xml(&list, Some(&materials))
}

/// The solids as a 3MF package with the colours of their materials (see
/// [`three_mf_model_coloured`]).
pub fn three_mf_coloured(bodies: &[(&str, &Mesh, &str)]) -> Vec<u8> {
    zip_stored(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes().to_vec()),
        ("_rels/.rels", RELATIONSHIPS.as_bytes().to_vec()),
        (
            "3D/3dmodel.model",
            three_mf_model_coloured(bodies).into_bytes(),
        ),
    ])
}

fn model_xml(bodies: &[(&str, &Mesh)], materials: Option<&[&str]>) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n<resources>\n",
    );
    // the different materials in the order they appear, and the id of the list (after the objects)
    let mut distinct: Vec<&str> = Vec::new();
    if let Some(list) = materials {
        for m in list.iter().filter(|m| !m.is_empty()) {
            if !distinct.contains(m) {
                distinct.push(m);
            }
        }
    }
    let list_id = bodies.len() + 1;
    if !distinct.is_empty() {
        let _ = writeln!(out, "<basematerials id=\"{list_id}\">");
        for m in &distinct {
            let _ = writeln!(
                out,
                "<base name=\"{}\" displaycolor=\"{}\"/>",
                solid_name(m),
                hex_colour(material_colour(m))
            );
        }
        out.push_str("</basematerials>\n");
    }
    for (i, (name, mesh)) in bodies.iter().enumerate() {
        let colour = materials
            .and_then(|list| list.get(i))
            .and_then(|m| distinct.iter().position(|d| d == m))
            .map(|at| format!(" pid=\"{list_id}\" pindex=\"{at}\""))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "<object id=\"{}\" name=\"{}\" type=\"model\"{colour}><mesh><vertices>",
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
    let nm = (v * per_unit).round();
    // a thousand kilometres is far outside the range of the model and keeps every sum finite
    if nm.abs() > 1.0e15 {
        return Err(format!("{what}: {text} is too far from the origin"));
    }
    Ok(nm)
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

/// The solids of a 3MF package, read back: the entries may be kept or compressed, the model is
/// found through the relationships of the package, and the components and the transformations of
/// the build are applied (see [`crate::threemf`]).
pub fn read_three_mf(bytes: &[u8]) -> Result<Vec<Body>, String> {
    let limits = ReadLimits::shipped();
    crate::threemf::read_package(bytes, &limits.zip, limits.max_triangles)
}

/// The surface of the triangles that [`crate::mesh::read_stl`] gives (nine millimetres each): a
/// vertex that lies at the same nanometre as another is the same vertex, as in the file this
/// program writes, so that the surface can be checked for being closed.
pub fn mesh_from_triangles(triangles: &[[f64; 9]]) -> Mesh {
    let nm: Vec<[f64; 9]> = triangles
        .iter()
        .map(|t| {
            let mut out = [0.0; 9];
            for (slot, v) in out.iter_mut().zip(t) {
                *slot = (v * NM_PER_MM).round();
            }
            out
        })
        .collect();
    merge_by(&nm)
}

/// The surface of triangles given in nanometres (nine numbers each): a vertex at the same place as
/// another is the same vertex, in the order of first appearance.
pub fn merge_by(triangles: &[[f64; 9]]) -> Mesh {
    use std::collections::HashMap;
    let mut index: HashMap<[i64; 3], u32> = HashMap::new();
    let mut mesh = Mesh::default();
    for t in triangles {
        let mut corners = [0u32; 3];
        for (k, corner) in corners.iter_mut().enumerate() {
            let v = [t[3 * k], t[3 * k + 1], t[3 * k + 2]];
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

/// The solids put in one surface, one after the other (for the formats that hold one object).
pub fn merge(bodies: &[(&str, &Mesh)]) -> Mesh {
    let mut out = Mesh::default();
    for (_, mesh) in bodies {
        let base = out.vertices.len() as u32;
        out.vertices.extend_from_slice(&mesh.vertices);
        out.triangles.extend(
            mesh.triangles
                .iter()
                .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
        );
    }
    out
}

/// The triangles of a surface as the places of their corners, each starting at its smallest corner
/// and the list in order: two surfaces of the same triangles give the same list whatever the order
/// of their vertices.
#[cfg(test)]
pub(crate) fn canonical_triangles(mesh: &Mesh) -> Vec<[[i64; 3]; 3]> {
    let mut out: Vec<[[i64; 3]; 3]> = mesh
        .triangles
        .iter()
        .map(|t| {
            let c = mesh
                .corners(t)
                .map(|p| [p[0] as i64, p[1] as i64, p[2] as i64]);
            let first = (0..3).min_by_key(|k| c[*k]).unwrap_or(0);
            [c[first], c[(first + 1) % 3], c[(first + 2) % 3]]
        })
        .collect();
    out.sort_unstable();
    out
}

/// The limits of the files of triangles that are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadLimits {
    /// The limits of a STEP file.
    pub step: crate::step_import::StepLimits,
    /// The limits of the package of a 3MF file.
    pub zip: crate::zipread::ZipLimits,
    /// The most bytes of any file.
    pub max_bytes: usize,
    /// The most triangles of any file.
    pub max_triangles: usize,
}

impl ReadLimits {
    /// The limits that the settings give.
    pub fn from_settings(s: &crate::config::MeshSettings) -> ReadLimits {
        ReadLimits {
            step: crate::step_import::StepLimits::from_settings(s),
            zip: crate::zipread::ZipLimits::new(s.zip_max_entries, s.read_max_kib),
            max_bytes: s.read_max_kib as usize * 1024,
            max_triangles: s.read_max_triangles as usize,
        }
    }

    /// The limits of the shipped settings.
    pub fn shipped() -> ReadLimits {
        ReadLimits::from_settings(&crate::config::Settings::defaults().mesh)
    }
}

/// What a file of triangles holds, with the warnings about what was not as it should be.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    /// The solids.
    pub bodies: Vec<Body>,
    /// What was left out or not as expected, each with a name.
    pub warnings: Vec<crate::stlread::ReadWarning>,
}

/// The solids of a file of an interchange format, by its extension (`stl`, `obj`, `3mf`, `step`,
/// `stp`, `ply`, `off` or `dxf`), with the limits given and the warnings of the readers that have
/// them.
pub fn read_file_checked(name: &str, bytes: &[u8], limits: &ReadLimits) -> Result<Read, String> {
    let extension = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let text = |bytes: &[u8]| {
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| "the file is not UTF-8 text".to_owned())
    };
    if extension != "step" && extension != "stp" && bytes.len() > limits.max_bytes {
        return Err(format!(
            "the file has {} bytes and {} are allowed",
            bytes.len(),
            limits.max_bytes
        ));
    }
    let mut warnings = Vec::new();
    let bodies = match extension.as_str() {
        "stl" => {
            let file = crate::stlread::read(bytes, limits.max_triangles)?;
            warnings = file.warnings;
            file.solids
                .iter()
                .map(|(n, t)| (n.clone(), mesh_from_triangles(t)))
                .collect()
        }
        "obj" => read_obj(&text(bytes)?)?,
        "3mf" => crate::threemf::read_package(bytes, &limits.zip, limits.max_triangles)?,
        "step" | "stp" => crate::step_import::read_step(&text(bytes)?, &limits.step)?,
        "ply" => {
            let (body, w) = crate::ply::read(bytes, limits.max_triangles)?;
            warnings = w;
            vec![body]
        }
        "off" => vec![crate::off::read(&text(bytes)?, limits.max_triangles)?],
        "dxf" => {
            let (bodies, w) = crate::dxf_mesh::read(&text(bytes)?, limits.max_triangles)?;
            warnings = w;
            bodies
        }
        _ => {
            return Err(
                "the file must end in .stl, .obj, .3mf, .step, .stp, .ply, .off or .dxf".to_owned(),
            )
        }
    };
    Ok(Read { bodies, warnings })
}

/// The solids of a file of an interchange format, by its extension: `stl` (one solid, called
/// `solid`), `obj`, `3mf` or `step` (`stp`), with the limits of the shipped settings for a STEP
/// file.
pub fn read_file_bytes(name: &str, bytes: &[u8]) -> Result<Vec<Body>, String> {
    read_file_bytes_limited(
        name,
        bytes,
        &crate::step_import::StepLimits::from_settings(&crate::config::Settings::defaults().mesh),
    )
}

/// The solids of a file of an interchange format, by its extension, with the limits of the
/// settings for a STEP file (see [`crate::step_import`]).
pub fn read_file_bytes_limited(
    name: &str,
    bytes: &[u8],
    step_limits: &crate::step_import::StepLimits,
) -> Result<Vec<Body>, String> {
    let limits = ReadLimits {
        step: *step_limits,
        ..ReadLimits::shipped()
    };
    read_file_checked(name, bytes, &limits).map(|r| r.bodies)
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
    fn the_colour_of_a_material_comes_from_its_name_and_differs_between_materials() {
        assert_eq!(material_colour(""), [190, 190, 190]);
        assert_eq!(material_colour("aluminium"), material_colour("aluminium"));
        let names = [
            "aluminium",
            "steel",
            "brass",
            "pla",
            "petg",
            "oak",
            "copper",
            "abs",
        ];
        for (i, a) in names.iter().enumerate() {
            for b in &names[i + 1..] {
                assert_ne!(material_colour(a), material_colour(b), "{a} {b}");
            }
        }
        assert_eq!(hex_colour([0, 15, 255]), "#000FFF");
        // the saturation and the value are fixed: the largest colour is the value, the smallest
        // is the value less the saturation
        for name in names {
            let c = material_colour(name);
            let (hi, lo) = (*c.iter().max().unwrap(), *c.iter().min().unwrap());
            assert_eq!(hi, 217, "{name}");
            assert!((97..=99).contains(&lo), "{name} {c:?}");
        }
    }

    #[test]
    fn the_obj_file_with_colours_has_six_numbers_a_vertex_and_reads_back_to_the_same_solids() {
        let a = cube(0.0);
        let b = cube(20.0);
        let text = obj_coloured(&[("box", &a, ""), ("part", &b, "steel")]);
        assert!(text.starts_with("# EH3D PHOENIX-CAD, millimetres, colours by material\n"));
        let verts: Vec<&str> = text.lines().filter(|l| l.starts_with("v ")).collect();
        assert_eq!(verts.len(), 16);
        assert!(verts.iter().all(|l| l.split(' ').count() == 7), "{verts:?}");
        // the first eight are grey, the others have the colour of the steel
        let grey = format!("{:.3}", 190.0 / 255.0);
        assert!(
            verts[0].ends_with(&format!("{grey} {grey} {grey}")),
            "{}",
            verts[0]
        );
        let c = material_colour("steel").map(|x| format!("{:.3}", f64::from(x) / 255.0));
        assert!(verts[8].ends_with(&c.join(" ")), "{}", verts[8]);
        // the faces number the vertices over the whole file, and a reader that ignores the colours
        // gets the solids that were written
        let read = read_obj(&text).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!((read[0].0.as_str(), &read[0].1), ("box", &a));
        assert_eq!((read[1].0.as_str(), &read[1].1), ("part", &b));
        // the same bytes for the same model
        assert_eq!(
            text,
            obj_coloured(&[("box", &a, ""), ("part", &b, "steel")])
        );
    }

    #[test]
    fn the_3mf_package_with_colours_lists_each_material_once_and_reads_back_to_the_same_solids() {
        let a = cube(0.0);
        let b = cube(20.0);
        let c = cube(40.0);
        let bodies = [
            ("box", &a, ""),
            ("one", &b, "steel"),
            ("two", &c, "steel"),
            ("three", &a, "brass"),
        ];
        let model = three_mf_model_coloured(&bodies);
        assert_eq!(model.matches("<basematerials id=\"5\">").count(), 1);
        assert_eq!(model.matches("<base ").count(), 2);
        assert!(model.contains(&format!(
            "<base name=\"steel\" displaycolor=\"{}\"/>",
            hex_colour(material_colour("steel"))
        )));
        assert!(model.contains("<object id=\"1\" name=\"box\" type=\"model\"><mesh>"));
        assert!(
            model.contains("<object id=\"2\" name=\"one\" type=\"model\" pid=\"5\" pindex=\"0\">")
        );
        assert!(
            model.contains("<object id=\"3\" name=\"two\" type=\"model\" pid=\"5\" pindex=\"0\">")
        );
        assert!(model
            .contains("<object id=\"4\" name=\"three\" type=\"model\" pid=\"5\" pindex=\"1\">"));
        // the list comes before the objects that use it
        assert!(model.find("<basematerials").unwrap() < model.find("<object ").unwrap());
        let bytes = three_mf_coloured(&bodies);
        let read = read_three_mf(&bytes).unwrap();
        assert_eq!(read.len(), 4);
        assert_eq!((read[1].0.as_str(), &read[1].1), ("one", &b));
        assert_eq!((read[3].0.as_str(), &read[3].1), ("three", &a));
        // a model with no material at all is the plain one
        assert_eq!(
            three_mf_model_coloured(&[("box", &a, "")]),
            three_mf_model(&[("box", &a)])
        );
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
        assert_eq!(read_file_bytes("box.stl", &text).unwrap()[0].1, read[0].1);
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
        assert!(read_file_bytes("a.step", &obj_text)
            .unwrap_err()
            .contains("not a STEP file"));
        assert!(read_file_bytes("a.dxf", &obj_text).is_err());
        assert!(read_file_bytes("a", &obj_text).is_err());
        assert!(read_file_bytes("a.obj", &[0xFF, 0xFE]).is_err());
        assert!(read_file_bytes("a.stl", b"short").is_err());
    }

    #[test]
    fn a_step_file_is_picked_by_its_extension_in_any_case_and_keeps_the_limits_it_is_given() {
        let step = "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=CARTESIAN_POINT('',(10.,0.,0.));\n#3=CARTESIAN_POINT('',(0.,10.,0.));\n#4=CARTESIAN_POINT('',(0.,0.,10.));\n#5=POLY_LOOP('',(#1,#3,#2));\n#6=POLY_LOOP('',(#1,#2,#4));\n#7=POLY_LOOP('',(#2,#3,#4));\n#8=POLY_LOOP('',(#3,#1,#4));\n#9=FACE_OUTER_BOUND('',#5,.T.);\n#10=FACE_OUTER_BOUND('',#6,.T.);\n#11=FACE_OUTER_BOUND('',#7,.T.);\n#12=FACE_OUTER_BOUND('',#8,.T.);\n#13=FACE('',(#9));\n#14=FACE('',(#10));\n#15=FACE('',(#11));\n#16=FACE('',(#12));\n#17=CLOSED_SHELL('',(#13,#14,#15,#16));\n#18=FACETED_BREP('corner',#17);\nENDSEC;\nEND-ISO-10303-21;\n";
        // a tetrahedron with three right angles at the origin: a sixth of a cube of 10 mm
        for name in ["t.step", "T.STEP", "dir.v2/t.Stp", "t.stp"] {
            let bodies = read_file_bytes(name, step.as_bytes()).unwrap();
            assert_eq!(bodies.len(), 1, "{name}");
            assert_eq!(bodies[0].0, "corner");
            assert!(bodies[0].1.check_closed().is_ok(), "{name}");
            assert!((bodies[0].1.volume_nm3() / (1.0e21 / 6.0) - 1.0).abs() < 1e-12);
        }
        let mut limits = crate::step_import::StepLimits::from_settings(
            &crate::config::Settings::defaults().mesh,
        );
        limits.max_faces = 3;
        let error = read_file_bytes_limited("t.step", step.as_bytes(), &limits).unwrap_err();
        assert!(error.contains("3 faces"), "{error}");
        // bytes that are not text are refused, and a name with no extension is none of the formats
        assert!(read_file_bytes("t.step", &[0xFF, 0xFE]).is_err());
        assert!(read_file_bytes("t", step.as_bytes()).is_err());
    }

    /// The next number of a fixed sequence (xorshift), so that a failure can be repeated.
    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    /// A damaged copy of a text: a line dropped, repeated or swapped, a character changed, a word
    /// from a pool put in, or the text cut short (some of them damaged twice).
    fn damaged_text(text: &str, state: &mut u64) -> String {
        const POOL: [&str; 8] = [
            "f 1 2 3",
            "v 1 2",
            "f -1 -2 -3",
            "o",
            "v 1e999 0 0",
            "f 0 1 2",
            "f 1/2/3 2 3",
            "",
        ];
        let mut out = text.to_owned();
        for _ in 0..1 + next(state) % 2 {
            let mut lines: Vec<String> = out.lines().map(str::to_owned).collect();
            let n = lines.len().max(1);
            let at = (next(state) % n as u64) as usize;
            match next(state) % 6 {
                0 if at < lines.len() => {
                    lines.remove(at);
                }
                1 if at < lines.len() => {
                    let copy = lines[at].clone();
                    lines.insert(at, copy);
                }
                2 => {
                    let other = (next(state) % n as u64) as usize;
                    if at < lines.len() && other < lines.len() {
                        lines.swap(at, other);
                    }
                }
                3 => {
                    let word = POOL[(next(state) % POOL.len() as u64) as usize];
                    lines.insert(at.min(lines.len()), word.to_owned());
                }
                4 if at < lines.len() => {
                    let mut chars: Vec<char> = lines[at].chars().collect();
                    if !chars.is_empty() {
                        let k = (next(state) % chars.len() as u64) as usize;
                        chars[k] = ['0', '9', '-', '.', 'x', ' ', '"'][(next(state) % 7) as usize];
                        lines[at] = chars.into_iter().collect();
                    }
                }
                _ => {
                    let keep = (next(state) % (lines.len() as u64 + 1)) as usize;
                    lines.truncate(keep);
                }
            }
            out = lines.join("\n");
        }
        out
    }

    #[test]
    fn damaged_obj_and_3mf_files_are_read_or_refused_and_never_panic_with_a_fixed_seed() {
        let a = cube(0.0);
        let b = cube(20.0);
        let obj_text = obj(&[("box", &a), ("part", &b)]);
        let model = three_mf_model(&[("box", &a), ("part", &b)]);
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let (mut read, mut refused) = (0, 0);
        for _ in 0..3000 {
            // OBJ: what is read is written and read again to the same surfaces
            let text = damaged_text(&obj_text, &mut state);
            match read_obj(&text) {
                Ok(bodies) => {
                    read += 1;
                    for (_, mesh) in &bodies {
                        for t in &mesh.triangles {
                            assert!(t.iter().all(|v| (*v as usize) < mesh.vertices.len()));
                        }
                        assert!(mesh.vertices.iter().flatten().all(|v| v.abs() <= 1.0e15));
                    }
                    let refs: Vec<(&str, &Mesh)> =
                        bodies.iter().map(|(n, m)| (n.as_str(), m)).collect();
                    let again = read_obj(&obj(&refs)).unwrap();
                    assert_eq!(
                        again.iter().map(|(_, m)| m).collect::<Vec<_>>(),
                        bodies.iter().map(|(_, m)| m).collect::<Vec<_>>(),
                        "{text}"
                    );
                }
                Err(why) => {
                    refused += 1;
                    assert!(!why.is_empty());
                }
            }
            // 3MF: the model damaged inside a package that is whole
            let xml = damaged_text(&model, &mut state);
            let package = zip_stored(&[("3D/3dmodel.model", xml.into_bytes())]);
            match read_three_mf(&package) {
                Ok(bodies) => {
                    read += 1;
                    for (_, mesh) in &bodies {
                        for t in &mesh.triangles {
                            assert!(t.iter().all(|v| (*v as usize) < mesh.vertices.len()));
                        }
                    }
                }
                Err(why) => {
                    refused += 1;
                    assert!(!why.is_empty());
                }
            }
            // STL: the bytes cut short or changed
            let mut bytes = crate::mesh::stl_binary(&[("box", &a)]);
            let at = (next(&mut state) % bytes.len() as u64) as usize;
            if next(&mut state).is_multiple_of(2) {
                bytes.truncate(at);
            } else {
                bytes[at] ^= 1 << (next(&mut state) % 8);
            }
            match read_file_bytes("x.stl", &bytes) {
                Ok(bodies) => {
                    read += 1;
                    assert_eq!(bodies.len(), 1);
                }
                Err(why) => {
                    refused += 1;
                    assert!(!why.is_empty());
                }
            }
        }
        // both outcomes happen: the damage is neither always seen nor never
        assert!(
            read > 1000 && refused > 1000,
            "{read} read, {refused} refused"
        );
    }

    #[test]
    fn a_number_too_far_from_the_origin_is_refused_by_the_readers() {
        let e = read_obj("v 1e300 0 0\n").unwrap_err();
        assert!(e.contains("too far"), "{e}");
        assert!(read_obj("v 999999999 0 0\n").is_ok());
        assert!(read_obj("v 1000000000 0 0\n").is_ok());
        assert!(read_obj("v 1000000001 0 0\n").is_err());
    }

    #[test]
    fn the_volume_of_the_triangles_of_the_cube_is_a_thousand_cubic_millimetres() {
        let a = cube(0.0);
        assert!((a.volume_nm3() - 1.0e21).abs() < 1.0e9);
    }

    #[test]
    fn colours_of_random_names_are_fixed_and_a_3mf_lists_each_material_once_with_a_fixed_seed() {
        let mut state = 0x1234_5678_9ABC_DEF1u64;
        let pool = ["steel", "brass", "pla", "oak", "copper", "abs", "petg", ""];
        let a = cube(0.0);
        for round in 0..300 {
            // a random name of letters and digits: the same colour every time, with the largest
            // channel at the fixed value and a text of seven characters
            let len = 1 + (next(&mut state) % 12) as usize;
            let name: String = (0..len)
                .map(|_| char::from(b'a' + (next(&mut state) % 26) as u8))
                .collect();
            let c = material_colour(&name);
            assert_eq!(c, material_colour(&name), "{name}");
            assert_eq!(*c.iter().max().unwrap(), 217, "{name}");
            assert_eq!(hex_colour(c).len(), 7);
            // up to six solids with materials from a pool: each different material is listed once
            let count = 1 + (next(&mut state) % 6) as usize;
            let picks: Vec<&str> = (0..count)
                .map(|_| pool[(next(&mut state) % pool.len() as u64) as usize])
                .collect();
            let bodies: Vec<(&str, &Mesh, &str)> =
                picks.iter().map(|m| ("solid", &a, *m)).collect();
            let model = three_mf_model_coloured(&bodies);
            let mut distinct: Vec<&str> = picks.iter().copied().filter(|m| !m.is_empty()).collect();
            distinct.sort_unstable();
            distinct.dedup();
            assert_eq!(
                model.matches("<base ").count(),
                distinct.len(),
                "round {round}"
            );
            assert_eq!(
                model.matches(" pid=").count(),
                picks.iter().filter(|m| !m.is_empty()).count(),
                "round {round}"
            );
            // the package reads back to the same solids
            let read = read_three_mf(&three_mf_coloured(&bodies)).unwrap();
            assert_eq!(read.len(), count);
            assert!(read.iter().all(|(_, m)| *m == a));
        }
    }
}
