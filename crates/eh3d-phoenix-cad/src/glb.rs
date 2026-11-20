// SPDX-License-Identifier: GPL-3.0-or-later
//! A solid of prisms as a binary glTF file (GLB), in the profile `phoenix-cad-glb/1`, and the check
//! of a file of that profile.
//!
//! The profile is small and fixed so that the same solid always gives the same bytes:
//!
//! * one scene, one node, one mesh with one primitive of triangles; the node carries a name given
//!   by the caller and a rotation that turns the Z axis of the model (up in a CAD program) into the
//!   Y axis that glTF viewers take as up;
//! * the positions are `f32` in metres (a nanometre is a billionth of a metre), the normals are
//!   flat (each prism is drawn on its own: four vertices for each of its six faces, outward
//!   normals, counter-clockwise triangles), the indices are `u32`;
//! * the `extras` of the file say the profile, the unit and the axes; the file holds no date, no
//!   host name, no user name and no version of the program;
//! * the JSON chunk is padded with spaces and the binary chunk with zeros to a multiple of four.
//!
//! [`check_glb`] reads a file of this profile back: the header, the two chunks, the counts of the
//! accessors against the binary chunk, the indices, and the volume and the bounds of the triangles,
//! which are what the caller compares with the solid. It never panics on bytes that are not a file
//! of the profile.

use crate::prism::Solid;
use crate::CadError;

/// The name of the profile, as written in the `extras` of the file.
pub const PROFILE: &str = "phoenix-cad-glb/1";

const MAGIC: u32 = 0x4654_6C67;
const JSON_CHUNK: u32 = 0x4E4F_534A;
const BIN_CHUNK: u32 = 0x004E_4942;
const NM_TO_M: f64 = 1.0e-9;

fn bad(why: impl Into<String>) -> CadError {
    CadError::Format(why.into())
}

fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
}

/// The six faces of a box: the outward normal and the four corners, counter-clockwise seen from
/// outside. A corner is `(x, y, z)` with each coordinate 0 for the lowest and 1 for the highest.
const FACES: [([f32; 3], [[u8; 3]; 4]); 6] = [
    (
        [1.0, 0.0, 0.0],
        [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]],
    ),
    (
        [-1.0, 0.0, 0.0],
        [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]],
    ),
    (
        [0.0, 1.0, 0.0],
        [[0, 1, 0], [0, 1, 1], [1, 1, 1], [1, 1, 0]],
    ),
    (
        [0.0, -1.0, 0.0],
        [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]],
    ),
    (
        [0.0, 0.0, 1.0],
        [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]],
    ),
    (
        [0.0, 0.0, -1.0],
        [[0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]],
    ),
];

/// Writes a solid as a GLB file of the profile. An empty solid, a name that is not made of letters,
/// digits, `_`, `-` and `.` (at most 64) and a solid with more triangles than `max_triangles` are
/// refused (`PHOENIX-CAD-041`, `PHOENIX-CAD-018` and `PHOENIX-CAD-050`).
pub fn solid_to_glb(solid: &Solid, name: &str, max_triangles: u32) -> Result<Vec<u8>, CadError> {
    if !is_plain_name(name) {
        return Err(bad(format!("the name {name:?} is not a plain name")));
    }
    if solid.is_empty() {
        return Err(CadError::Export("the solid is empty: no file".to_string()));
    }
    let boxes = solid.prisms().len();
    let triangles = boxes * 12;
    if triangles as u64 > u64::from(max_triangles) {
        return Err(CadError::ExportLimit(format!(
            "{triangles} triangles, more than the {max_triangles} allowed"
        )));
    }
    let vertices = boxes * 24;
    let mut positions: Vec<f32> = Vec::with_capacity(vertices * 3);
    let mut normals: Vec<f32> = Vec::with_capacity(vertices * 3);
    let mut indices: Vec<u32> = Vec::with_capacity(triangles * 3);
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in solid.prisms() {
        let (min, max) = (p.min(), p.max());
        for (normal, corners) in FACES {
            let base = (positions.len() / 3) as u32;
            for c in corners {
                for axis in 0..3 {
                    let nm = if c[axis] == 0 { min[axis] } else { max[axis] };
                    let v = (nm as f64 * NM_TO_M) as f32;
                    lo[axis] = lo[axis].min(v);
                    hi[axis] = hi[axis].max(v);
                    positions.push(v);
                }
                normals.extend_from_slice(&normal);
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    let (n, m) = (vertices, indices.len());
    let (pos_len, idx_len) = (n * 12, m * 4);
    let bin_len = pos_len * 2 + idx_len;
    let json = format!(
        concat!(
            "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"{profile}\"}},",
            "\"scene\":0,\"scenes\":[{{\"nodes\":[0]}}],",
            "\"nodes\":[{{\"name\":\"{name}\",\"mesh\":0,\"rotation\":[-0.70710678,0,0,0.70710678]}}],",
            "\"meshes\":[{{\"primitives\":[{{\"attributes\":{{\"POSITION\":0,\"NORMAL\":1}},\"indices\":2,\"mode\":4}}]}}],",
            "\"accessors\":[",
            "{{\"bufferView\":0,\"componentType\":5126,\"count\":{n},\"type\":\"VEC3\",\"min\":[{lx},{ly},{lz}],\"max\":[{hx},{hy},{hz}]}},",
            "{{\"bufferView\":1,\"componentType\":5126,\"count\":{n},\"type\":\"VEC3\"}},",
            "{{\"bufferView\":2,\"componentType\":5125,\"count\":{m},\"type\":\"SCALAR\"}}],",
            "\"bufferViews\":[",
            "{{\"buffer\":0,\"byteOffset\":0,\"byteLength\":{pos_len},\"target\":34962}},",
            "{{\"buffer\":0,\"byteOffset\":{pos_len},\"byteLength\":{pos_len},\"target\":34962}},",
            "{{\"buffer\":0,\"byteOffset\":{second},\"byteLength\":{idx_len},\"target\":34963}}],",
            "\"buffers\":[{{\"byteLength\":{bin_len}}}],",
            "\"extras\":{{\"profile\":\"{profile}\",\"unit\":\"m\",\"axes\":\"z up in the model, turned to y up in the node\"}}}}"
        ),
        profile = PROFILE,
        name = name,
        n = n,
        m = m,
        lx = lo[0],
        ly = lo[1],
        lz = lo[2],
        hx = hi[0],
        hy = hi[1],
        hz = hi[2],
        pos_len = pos_len,
        second = pos_len * 2,
        idx_len = idx_len,
        bin_len = bin_len,
    );
    let mut json_bytes = json.into_bytes();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let mut bin: Vec<u8> = Vec::with_capacity(bin_len + 3);
    for v in positions.iter().chain(&normals) {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    for i in &indices {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&JSON_CHUNK.to_le_bytes());
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&BIN_CHUNK.to_le_bytes());
    out.extend_from_slice(&bin);
    Ok(out)
}

/// What [`check_glb`] found in a file.
#[derive(Debug, Clone, PartialEq)]
pub struct GlbInfo {
    /// The vertices.
    pub vertices: usize,
    /// The triangles.
    pub triangles: usize,
    /// The lowest corner of the positions, in metres.
    pub min: [f64; 3],
    /// The highest corner of the positions, in metres.
    pub max: [f64; 3],
    /// The volume the triangles enclose, in cubic metres (negative when they are turned inside out).
    pub volume_m3: f64,
}

/// The whole number that follows `key` in `text`, from `from` on.
fn number_after(text: &str, key: &str, from: usize) -> Option<(usize, usize)> {
    let at = text[from..].find(key)? + from + key.len();
    let digits: String = text[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.is_empty() || digits.len() > 12 {
        return None;
    }
    Some((digits.parse().ok()?, at + digits.len()))
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Reads a GLB file of the profile and gives what it holds. Whatever is wrong is a
/// `PHOENIX-CAD-018` that says what.
pub fn check_glb(bytes: &[u8]) -> Result<GlbInfo, CadError> {
    if word(bytes, 0) != Some(MAGIC) {
        return Err(bad("the file does not start with glTF"));
    }
    if word(bytes, 4) != Some(2) {
        return Err(bad("the version of glTF is not 2"));
    }
    if word(bytes, 8).map(|l| l as usize) != Some(bytes.len()) {
        return Err(bad(
            "the length of the header is not the length of the file",
        ));
    }
    let json_len = word(bytes, 12).ok_or_else(|| bad("no JSON chunk"))? as usize;
    if word(bytes, 16) != Some(JSON_CHUNK) || !json_len.is_multiple_of(4) {
        return Err(bad(
            "the first chunk is not a JSON chunk of a whole number of words",
        ));
    }
    let json_end = 20usize
        .checked_add(json_len)
        .filter(|e| *e <= bytes.len())
        .ok_or_else(|| bad("the JSON chunk is longer than the file"))?;
    let json =
        std::str::from_utf8(&bytes[20..json_end]).map_err(|_| bad("the JSON is not text"))?;
    let bin_len = word(bytes, json_end).ok_or_else(|| bad("no binary chunk"))? as usize;
    if word(bytes, json_end + 4) != Some(BIN_CHUNK) || !bin_len.is_multiple_of(4) {
        return Err(bad(
            "the second chunk is not a binary chunk of a whole number of words",
        ));
    }
    let bin_start = json_end + 8;
    if bin_start.checked_add(bin_len) != Some(bytes.len()) {
        return Err(bad("the binary chunk does not end the file"));
    }
    let bin = &bytes[bin_start..];
    let trimmed = json.trim_end_matches(' ');
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err(bad("the JSON is not an object"));
    }
    if !trimmed.contains(&format!("\"profile\":\"{PROFILE}\"")) {
        return Err(bad("the file is not of the profile phoenix-cad-glb/1"));
    }
    let accessors = trimmed
        .find("\"accessors\":")
        .ok_or_else(|| bad("no accessors"))?;
    let (n, at) = number_after(trimmed, "\"count\":", accessors)
        .ok_or_else(|| bad("no count of positions"))?;
    let (n_normals, at) =
        number_after(trimmed, "\"count\":", at).ok_or_else(|| bad("no count of normals"))?;
    let (m, _) =
        number_after(trimmed, "\"count\":", at).ok_or_else(|| bad("no count of indices"))?;
    let buffers = trimmed
        .find("\"buffers\":")
        .ok_or_else(|| bad("no buffer"))?;
    let (declared, _) = number_after(trimmed, "\"byteLength\":", buffers)
        .ok_or_else(|| bad("no length of the buffer"))?;
    if n != n_normals || n == 0 || m == 0 || !m.is_multiple_of(3) {
        return Err(bad("the counts of the accessors do not make triangles"));
    }
    let expected = n
        .checked_mul(24)
        .and_then(|v| v.checked_add(m.checked_mul(4)?))
        .ok_or_else(|| bad("the counts are too large"))?;
    if declared != expected || bin.len() != expected.div_ceil(4) * 4 {
        return Err(bad("the binary chunk is not the size the accessors need"));
    }
    let float =
        |i: usize| f32::from_le_bytes([bin[i * 4], bin[i * 4 + 1], bin[i * 4 + 2], bin[i * 4 + 3]]);
    let mut pos: Vec<[f64; 3]> = Vec::with_capacity(n);
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for v in 0..n {
        let mut p = [0.0f64; 3];
        for (axis, slot) in p.iter_mut().enumerate() {
            let x = float(v * 3 + axis);
            if !x.is_finite() {
                return Err(bad("a position is not a number"));
            }
            *slot = f64::from(x);
            lo[axis] = lo[axis].min(*slot);
            hi[axis] = hi[axis].max(*slot);
        }
        pos.push(p);
    }
    for i in 0..n * 3 {
        if !float(n * 3 + i).is_finite() {
            return Err(bad("a normal is not a number"));
        }
    }
    let index_at = |k: usize| {
        let o = n * 24 + k * 4;
        u32::from_le_bytes([bin[o], bin[o + 1], bin[o + 2], bin[o + 3]]) as usize
    };
    let mut volume = 0.0f64;
    for t in 0..m / 3 {
        let (a, b, c) = (index_at(t * 3), index_at(t * 3 + 1), index_at(t * 3 + 2));
        if a >= n || b >= n || c >= n {
            return Err(bad("an index points past the last vertex"));
        }
        let (p, q, r) = (pos[a], pos[b], pos[c]);
        // the signed volume of the tetrahedron with the origin
        volume += (p[0] * (q[1] * r[2] - q[2] * r[1]) - p[1] * (q[0] * r[2] - q[2] * r[0])
            + p[2] * (q[0] * r[1] - q[1] * r[0]))
            / 6.0;
    }
    Ok(GlbInfo {
        vertices: n,
        triangles: m / 3,
        min: lo,
        max: hi,
        volume_m3: volume,
    })
}

#[cfg(test)]
mod tests;
