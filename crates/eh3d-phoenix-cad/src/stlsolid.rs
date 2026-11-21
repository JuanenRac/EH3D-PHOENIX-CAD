// SPDX-License-Identifier: GPL-3.0-or-later
//! A solid of prisms as a binary STL file: only the faces on the surface, joined into as few
//! rectangles as the grid allows.
//!
//! Where two prisms touch, the faces between them are not written, so the file is the surface of
//! the solid and not a heap of boxes; the faces of a cavity are written with their normals pointing
//! into the cavity. Rectangles are made greedily, row by row in the order of the grid, so the same
//! solid always gives the same bytes. Two rectangles that meet along a longer edge of a neighbour
//! share a point of that edge only on one side (a T-junction), which a slicer repairs but a strict
//! check of "every edge is shared by two triangles" would count; the volume and the area that the
//! rectangles add up to are exact, and the tests compare them with the solid.
//!
//! The corners are written in millimetres as 32-bit floats, like the other STL files of the
//! program, so a coordinate beyond about 16 m loses its last nanometres in the file (the
//! rectangles themselves are whole nanometres).

use crate::interchange;
use crate::mesh::{Builder, Mesh};
use crate::off;
use crate::ply;
use crate::prism::{Limits, Solid};
use crate::solidquery::Grid;
use crate::CadError;

/// A rectangle on the surface of a solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quad {
    /// The axis the rectangle is perpendicular to: 0 for x, 1 for y, 2 for z.
    pub axis: usize,
    /// Whether the material is on the low side (the outward normal points to the high side).
    pub outward_high: bool,
    /// Where the plane of the rectangle cuts the axis, in nanometres.
    pub plane: i64,
    /// The lowest corner in the two other axes, in the order of the axes after `axis` (y, z for
    /// x; z, x for y; x, y for z).
    pub low: [i64; 2],
    /// The highest corner.
    pub high: [i64; 2],
}

impl Quad {
    /// The area in square nanometres.
    pub fn area_nm2(&self) -> i128 {
        i128::from(self.high[0] - self.low[0]) * i128::from(self.high[1] - self.low[1])
    }

    /// The four corners, counter-clockwise seen from outside the solid.
    pub fn corners(&self) -> [[i64; 3]; 4] {
        let (u, v) = ((self.axis + 1) % 3, (self.axis + 2) % 3);
        let at = |a: i64, b: i64| {
            let mut p = [0i64; 3];
            p[self.axis] = self.plane;
            p[u] = a;
            p[v] = b;
            p
        };
        let ring = [
            at(self.low[0], self.low[1]),
            at(self.high[0], self.low[1]),
            at(self.high[0], self.high[1]),
            at(self.low[0], self.high[1]),
        ];
        if self.outward_high {
            ring
        } else {
            [ring[0], ring[3], ring[2], ring[1]]
        }
    }
}

/// The rectangles of the surface of a solid, in the order of the axes, the planes, the side and
/// the grid.
pub fn boundary_quads(solid: &Solid, limits: &Limits) -> Result<Vec<Quad>, CadError> {
    let grid = Grid::new(solid, limits)?;
    let d = grid.dims();
    let mut out = Vec::new();
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for high_side in [false, true] {
            for plane in 0..=d[axis] {
                // the cells of this plane whose face is on the surface with the material below
                // (outward to the high side) or above it
                let mut mask = vec![false; d[u] * d[v]];
                for iu in 0..d[u] {
                    for iv in 0..d[v] {
                        let cell = |layer: isize| {
                            let mut at = [0isize; 3];
                            at[axis] = layer;
                            at[u] = iu as isize;
                            at[v] = iv as isize;
                            grid.is_filled(at)
                        };
                        let (below, above) = (cell(plane as isize - 1), cell(plane as isize));
                        mask[iu * d[v] + iv] = if high_side {
                            below && !above
                        } else {
                            above && !below
                        };
                    }
                }
                for iu in 0..d[u] {
                    for iv in 0..d[v] {
                        if !mask[iu * d[v] + iv] {
                            continue;
                        }
                        let mut iu1 = iu + 1;
                        while iu1 < d[u] && mask[iu1 * d[v] + iv] {
                            iu1 += 1;
                        }
                        let mut iv1 = iv + 1;
                        while iv1 < d[v] && (iu..iu1).all(|x| mask[x * d[v] + iv1]) {
                            iv1 += 1;
                        }
                        for x in iu..iu1 {
                            for y in iv..iv1 {
                                mask[x * d[v] + y] = false;
                            }
                        }
                        out.push(Quad {
                            axis,
                            outward_high: high_side,
                            plane: grid.planes(axis)[plane],
                            low: [grid.planes(u)[iu], grid.planes(v)[iv]],
                            high: [grid.planes(u)[iu1], grid.planes(v)[iv1]],
                        });
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The volume enclosed by the rectangles, in cubic nanometres: the sum over the faces
/// perpendicular to x of the plane times the area, with the sign of the normal. It is exact, and
/// the solid's own volume is what it must equal.
pub fn quads_volume_nm3(quads: &[Quad]) -> i128 {
    quads
        .iter()
        .filter(|q| q.axis == 0)
        .map(|q| {
            let sign = if q.outward_high { 1 } else { -1 };
            sign * i128::from(q.plane) * q.area_nm2()
        })
        .sum()
}

fn mm(nm: i64) -> f32 {
    (nm as f64 / 1.0e6) as f32
}

/// Writes a solid as a binary STL file in millimetres. An empty solid is refused
/// (`PHOENIX-CAD-041`) and so is a surface with more than `max_triangles` triangles
/// (`PHOENIX-CAD-050`); the grid is limited by `limits` (`PHOENIX-CAD-053`).
pub fn solid_to_stl(
    solid: &Solid,
    max_triangles: u32,
    limits: &Limits,
) -> Result<Vec<u8>, CadError> {
    if solid.is_empty() {
        return Err(CadError::Export("the solid is empty: no file".to_string()));
    }
    let quads = boundary_quads(solid, limits)?;
    let triangles = quads.len() as u64 * 2;
    if triangles > u64::from(max_triangles) {
        return Err(CadError::ExportLimit(format!(
            "{triangles} triangles, more than the {max_triangles} allowed"
        )));
    }
    let mut out = Vec::with_capacity(84 + 50 * triangles as usize);
    out.extend_from_slice(b"EH3D PHOENIX-CAD binary STL, millimetres");
    out.resize(80, b' ');
    out.extend_from_slice(&(triangles as u32).to_le_bytes());
    for q in &quads {
        let mut normal = [0.0f32; 3];
        normal[q.axis] = if q.outward_high { 1.0 } else { -1.0 };
        let c = q.corners();
        for tri in [[c[0], c[1], c[2]], [c[0], c[2], c[3]]] {
            for n in normal {
                out.extend_from_slice(&n.to_le_bytes());
            }
            for corner in tri {
                for v in corner {
                    out.extend_from_slice(&mm(v).to_le_bytes());
                }
            }
            out.extend_from_slice(&[0, 0]);
        }
    }
    Ok(out)
}

/// The surface of a solid of prisms as a mesh of welded vertices (in nanometres), two triangles to
/// a rectangle, counter-clockwise seen from outside. An empty solid is refused (`PHOENIX-CAD-041`)
/// and so is a surface with more than `max_triangles` triangles (`PHOENIX-CAD-050`).
pub fn solid_to_mesh(solid: &Solid, max_triangles: u32, limits: &Limits) -> Result<Mesh, CadError> {
    if solid.is_empty() {
        return Err(CadError::Export("the solid is empty: no file".to_string()));
    }
    let quads = boundary_quads(solid, limits)?;
    let triangles = quads.len() as u64 * 2;
    if triangles > u64::from(max_triangles) {
        return Err(CadError::ExportLimit(format!(
            "{triangles} triangles, more than the {max_triangles} allowed"
        )));
    }
    let mut builder = Builder::new(max_triangles as usize);
    for q in &quads {
        let c = q.corners().map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]);
        let ids = builder.ring(&c);
        builder.triangle(ids[0], ids[1], ids[2])?;
        builder.triangle(ids[0], ids[2], ids[3])?;
    }
    Ok(builder.finish())
}

/// The files that the surface of a solid can be written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    /// Binary STL.
    Stl,
    /// Wavefront OBJ.
    Obj,
    /// 3MF, a package with one model.
    ThreeMf,
    /// Binary PLY.
    Ply,
    /// OFF.
    Off,
}

impl FileFormat {
    /// Every format.
    pub const ALL: [FileFormat; 5] = [
        FileFormat::Stl,
        FileFormat::Obj,
        FileFormat::ThreeMf,
        FileFormat::Ply,
        FileFormat::Off,
    ];

    /// The extension of the file, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            FileFormat::Stl => "stl",
            FileFormat::Obj => "obj",
            FileFormat::ThreeMf => "3mf",
            FileFormat::Ply => "ply",
            FileFormat::Off => "off",
        }
    }

    /// The format of a file name, by its extension in any case.
    pub fn from_name(name: &str) -> Option<FileFormat> {
        let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
        FileFormat::ALL.into_iter().find(|f| f.extension() == ext)
    }
}

/// The surface of a solid as the bytes of a file of the format, in millimetres, with the same
/// refusals as [`solid_to_stl`]. The same solid always gives the same bytes.
pub fn solid_to_file(
    solid: &Solid,
    format: FileFormat,
    name: &str,
    max_triangles: u32,
    limits: &Limits,
) -> Result<Vec<u8>, CadError> {
    if format == FileFormat::Stl {
        return solid_to_stl(solid, max_triangles, limits);
    }
    let mesh = solid_to_mesh(solid, max_triangles, limits)?;
    let body = [(name, &mesh)];
    Ok(match format {
        FileFormat::Stl => unreachable!("the binary STL is written above"),
        FileFormat::Obj => interchange::obj(&body).into_bytes(),
        FileFormat::ThreeMf => interchange::three_mf(&body),
        FileFormat::Ply => ply::write_binary(&body),
        FileFormat::Off => off::write(&body).into_bytes(),
    })
}

#[cfg(test)]
mod tests;
