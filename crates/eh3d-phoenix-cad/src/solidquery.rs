// SPDX-License-Identifier: GPL-3.0-or-later
//! Questions about a solid of prisms that are answered exactly, in whole nanometres: how thick the
//! material is along an axis (the check of thin walls), the area of its surface, and its centre of
//! mass.
//!
//! The thickness is the length of the run of material that a line parallel to an axis meets, from
//! where it enters to where it leaves. It is measured along the three axes only, never along a
//! slanted line, so a wall that is thin along a diagonal is not found; a run that ends in the
//! outside of the solid on both sides is a wall, a run that starts at a face of the bounding box is
//! a wall too. Everything is worked out on the grid that the planes of the prisms cut space into,
//! so the answers have no tolerance and the work is limited by [`Limits`] like the booleans.

use crate::prism::{Limits, Prism, Solid};
use crate::CadError;

/// The grid that the planes of the prisms of a solid cut space into, with a flag for every cell
/// that says whether it is material.
pub struct Grid {
    coords: [Vec<i64>; 3],
    dims: [usize; 3],
    filled: Vec<bool>,
}

fn unique(mut v: Vec<i64>) -> Vec<i64> {
    v.sort_unstable();
    v.dedup();
    v
}

fn slot(axis: &[i64], value: i64) -> usize {
    axis.binary_search(&value)
        .expect("every plane of a prism is on the grid")
}

impl Grid {
    /// The grid of a solid. A grid with more cells than `limits.max_cells` is refused
    /// (`PHOENIX-CAD-053`).
    pub fn new(solid: &Solid, limits: &Limits) -> Result<Grid, CadError> {
        let mut coords: [Vec<i64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for p in solid.prisms() {
            for (i, axis) in coords.iter_mut().enumerate() {
                axis.push(p.min()[i]);
                axis.push(p.max()[i]);
            }
        }
        let coords = [
            unique(std::mem::take(&mut coords[0])),
            unique(std::mem::take(&mut coords[1])),
            unique(std::mem::take(&mut coords[2])),
        ];
        let dims = [
            coords[0].len().saturating_sub(1),
            coords[1].len().saturating_sub(1),
            coords[2].len().saturating_sub(1),
        ];
        let too_many = || {
            CadError::PrismLimit(format!(
                "the grid of the question has more than {} cells",
                limits.max_cells
            ))
        };
        let cells = dims[0]
            .checked_mul(dims[1])
            .and_then(|c| c.checked_mul(dims[2]))
            .filter(|c| *c <= limits.max_cells)
            .ok_or_else(too_many)?;
        let mut filled = vec![false; cells];
        for p in solid.prisms() {
            let lo: Vec<usize> = (0..3).map(|i| slot(&coords[i], p.min()[i])).collect();
            let hi: Vec<usize> = (0..3).map(|i| slot(&coords[i], p.max()[i])).collect();
            for k in lo[2]..hi[2] {
                for j in lo[1]..hi[1] {
                    for i in lo[0]..hi[0] {
                        filled[(k * dims[1] + j) * dims[0] + i] = true;
                    }
                }
            }
        }
        Ok(Grid {
            coords,
            dims,
            filled,
        })
    }

    /// The number of cells along each axis.
    pub fn dims(&self) -> [usize; 3] {
        self.dims
    }

    /// The planes of the grid on an axis, in nanometres, lowest first.
    pub fn planes(&self, axis: usize) -> &[i64] {
        &self.coords[axis]
    }

    /// Whether the cell at the indices is material; a cell outside the grid is not.
    pub fn is_filled(&self, at: [isize; 3]) -> bool {
        let mut idx = [0usize; 3];
        for i in 0..3 {
            match usize::try_from(at[i]) {
                Ok(v) if v < self.dims[i] => idx[i] = v,
                _ => return false,
            }
        }
        self.filled[(idx[2] * self.dims[1] + idx[1]) * self.dims[0] + idx[0]]
    }

    /// The width of a cell on an axis, in nanometres.
    pub fn width(&self, axis: usize, cell: usize) -> i64 {
        self.coords[axis][cell + 1] - self.coords[axis][cell]
    }
}

/// A run of material along an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    /// The axis: 0 for x, 1 for y, 2 for z.
    pub axis: usize,
    /// Where the material starts on the axis, in nanometres.
    pub from: i64,
    /// Where it ends.
    pub to: i64,
}

impl Run {
    /// How thick the material is along the axis.
    pub fn thickness(&self) -> i64 {
        self.to - self.from
    }
}

/// A part of the solid that is thinner than a minimum: the run of material along an axis and the
/// rectangle (in the two other axes, in the order of the axes) of the lines that meet exactly that
/// run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinWall {
    /// The run, the same for every line of the rectangle.
    pub run: Run,
    /// The lowest corner of the rectangle in the two other axes.
    pub low: [i64; 2],
    /// The highest corner.
    pub high: [i64; 2],
}

/// The run of material that the line through `point` parallel to `axis` meets at the point, or
/// `None` when the point is not material (or the axis is above 2). The lower face of a prism
/// belongs to it and the upper does not, like [`Solid::contains`].
pub fn thickness_at(solid: &Solid, point: [i64; 3], axis: usize) -> Option<Run> {
    if axis > 2 {
        return None;
    }
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let mut spans: Vec<(i64, i64)> = solid
        .prisms()
        .iter()
        .filter(|p| {
            let (lo, hi) = (Prism::min(p), Prism::max(p));
            point[u] >= lo[u] && point[u] < hi[u] && point[v] >= lo[v] && point[v] < hi[v]
        })
        .map(|p| (p.min()[axis], p.max()[axis]))
        .collect();
    spans.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (a, b) in spans {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
        .into_iter()
        .find(|(a, b)| point[axis] >= *a && point[axis] < *b)
        .map(|(from, to)| Run { axis, from, to })
}

/// Every run of material along an axis, thinner than `limit` when one is given, in the order of
/// the lines (the third axis, then the second, then the first of the lines) and, along a line,
/// from the low end; lines that meet the same run side by side are one entry when they are
/// neighbours in the grid. At most `max_found` entries are returned.
pub fn walls(
    solid: &Solid,
    axis: usize,
    limit: Option<i64>,
    max_found: usize,
    limits: &Limits,
) -> Result<Vec<ThinWall>, CadError> {
    if axis > 2 {
        return Err(CadError::PrismLimit(format!(
            "the axis {axis} is not 0, 1 or 2"
        )));
    }
    let grid = Grid::new(solid, limits)?;
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let d = grid.dims();
    let mut found: Vec<ThinWall> = Vec::new();
    // the runs of one line, then side by side lines with the same run are put together along u
    for jv in 0..d[v] {
        let mut row: Vec<ThinWall> = Vec::new();
        for iu in 0..d[u] {
            let mut start: Option<usize> = None;
            for ia in 0..=d[axis] {
                let mut at = [0isize; 3];
                at[axis] = ia as isize;
                at[u] = iu as isize;
                at[v] = jv as isize;
                let inside = ia < d[axis] && grid.is_filled(at);
                match (inside, start) {
                    (true, None) => start = Some(ia),
                    (false, Some(s)) => {
                        start = None;
                        let run = Run {
                            axis,
                            from: grid.planes(axis)[s],
                            to: grid.planes(axis)[ia],
                        };
                        if limit.is_some_and(|l| run.thickness() >= l) {
                            continue;
                        }
                        let low = [grid.planes(u)[iu], grid.planes(v)[jv]];
                        let high = [grid.planes(u)[iu + 1], grid.planes(v)[jv + 1]];
                        match row
                            .iter_mut()
                            .find(|w| w.run == run && w.high[0] == low[0] && w.low[1] == low[1])
                        {
                            Some(w) => w.high[0] = high[0],
                            None => row.push(ThinWall { run, low, high }),
                        }
                    }
                    _ => {}
                }
            }
        }
        for w in row {
            // a row of the next line with the same run and the same extent continues the rectangle
            match found.iter_mut().find(|f| {
                f.run == w.run
                    && f.high[1] == w.low[1]
                    && f.low[0] == w.low[0]
                    && f.high[0] == w.high[0]
            }) {
                Some(f) => f.high[1] = w.high[1],
                None => found.push(w),
            }
        }
        if found.len() > max_found {
            found.truncate(max_found);
            break;
        }
    }
    Ok(found)
}

/// The thinnest run of material along an axis, or `None` for an empty solid. When several runs
/// are as thin, the one whose line comes first in the order of [`walls`] is returned.
pub fn thinnest(solid: &Solid, axis: usize, limits: &Limits) -> Result<Option<ThinWall>, CadError> {
    let all = walls(solid, axis, None, usize::MAX, limits)?;
    Ok(all.into_iter().min_by_key(|w| w.run.thickness()))
}

/// The area of the surface of a solid in square nanometres, exact: every face of a cell of the
/// grid that has material on one side and none on the other, so the faces where two prisms touch
/// are not counted and the walls of a closed cavity are.
pub fn surface_area_nm2(solid: &Solid, limits: &Limits) -> Result<i128, CadError> {
    let grid = Grid::new(solid, limits)?;
    let d = grid.dims();
    let mut total: i128 = 0;
    for k in 0..d[2] {
        for j in 0..d[1] {
            for i in 0..d[0] {
                let here = [i as isize, j as isize, k as isize];
                if !grid.is_filled(here) {
                    continue;
                }
                for axis in 0..3 {
                    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                    let face = i128::from(grid.width(u, [i, j, k][u]))
                        * i128::from(grid.width(v, [i, j, k][v]));
                    for step in [-1isize, 1] {
                        let mut other = here;
                        other[axis] += step;
                        if !grid.is_filled(other) {
                            total += face;
                        }
                    }
                }
            }
        }
    }
    Ok(total)
}

fn div_round_half_away(num: i128, den: i128) -> i128 {
    let half = den / 2;
    if num >= 0 {
        (num + half) / den
    } else {
        (num - half) / den
    }
}

/// The centre of mass of a solid of one material, rounded half away from zero to the nanometre,
/// or `None` for an empty solid. The sums are exact whole numbers; a solid so large that they
/// would not fit in 128 bits is refused (`PHOENIX-CAD-053`).
pub fn centroid_nm(solid: &Solid) -> Result<Option<[i64; 3]>, CadError> {
    let too_large = || CadError::PrismLimit("the solid is too large for an exact centre".into());
    let mut total: i128 = 0;
    let mut moment = [0i128; 3];
    for p in solid.prisms() {
        let volume = p.volume_nm3();
        total = total.checked_add(volume).ok_or_else(too_large)?;
        for (i, m) in moment.iter_mut().enumerate() {
            let twice = i128::from(p.min()[i]) + i128::from(p.max()[i]);
            let term = volume.checked_mul(twice).ok_or_else(too_large)?;
            *m = m.checked_add(term).ok_or_else(too_large)?;
        }
    }
    if total == 0 {
        return Ok(None);
    }
    let den = total.checked_mul(2).ok_or_else(too_large)?;
    let mut out = [0i64; 3];
    for i in 0..3 {
        out[i] = i64::try_from(div_round_half_away(moment[i], den)).map_err(|_| too_large())?;
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests;
