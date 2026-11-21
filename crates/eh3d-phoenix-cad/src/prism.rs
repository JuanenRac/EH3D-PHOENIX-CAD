//! Solids made of prisms aligned with the axes, and the three booleans between them.
//!
//! A [`Prism`] is a box `[min, max)` in whole nanometres. A [`Solid`] is a set of such boxes that
//! do not overlap, kept in a **canonical form**: the same solid is always the same list of boxes,
//! whatever the boxes it was made from or the order of the operations that made it, so two solids
//! are equal when `==` says so.
//!
//! The union, the difference and the intersection are exact (whole nanometres, no tolerance, no
//! outside library): the coordinates of both operands cut space into a grid, every cell of the
//! grid is inside or outside each operand, the operation is a rule on those two answers, and the
//! cells that are inside are put back together. The grid is then reduced to the planes where the
//! solid really changes, and the boxes are made by joining cells along the first axis, then the
//! second, then the third, in a fixed order.
//!
//! The work grows with the number of cells of the grid, so it is limited by [`Limits`]
//! (`history.max_cells` and `history.max_prisms` of the settings): an operation that needs more
//! is refused with `PHOENIX-CAD-053` and nothing is built.

use crate::{CadError, Length};

/// The limits of an operation between solids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most cells of the grid of an operation.
    pub max_cells: usize,
    /// The most prisms of a solid.
    pub max_prisms: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_cells: 2_000_000,
            max_prisms: 100_000,
        }
    }
}

/// A box aligned with the axes, `[min, max)` on each axis, in nanometres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Prism {
    min: [i64; 3],
    max: [i64; 3],
}

impl Prism {
    /// A box from its lowest corner and its sizes. A size that is not positive is refused
    /// (`PHOENIX-CAD-002`), and so is a corner that would leave the range of lengths.
    pub fn new(corner: [Length; 3], size: [Length; 3]) -> Result<Prism, CadError> {
        let mut min = [0i64; 3];
        let mut max = [0i64; 3];
        for i in 0..3 {
            if size[i].get() <= 0 {
                return Err(CadError::NotPositive("size of a prism"));
            }
            min[i] = corner[i].get();
            let top = corner[i]
                .checked_add(size[i])
                .map_err(|_| CadError::Overflow("corner of a prism"))?;
            max[i] = top.get();
        }
        Ok(Prism { min, max })
    }

    /// The lowest corner, in nanometres.
    pub fn min(&self) -> [i64; 3] {
        self.min
    }

    /// The corner opposite to the lowest one, in nanometres.
    pub fn max(&self) -> [i64; 3] {
        self.max
    }

    /// The sizes, in nanometres.
    pub fn size(&self) -> [i64; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }

    /// The volume in cubic nanometres.
    pub fn volume_nm3(&self) -> i128 {
        let s = self.size();
        i128::from(s[0]) * i128::from(s[1]) * i128::from(s[2])
    }

    /// Whether a point is inside (the lower faces belong to the box, the upper ones do not).
    pub fn contains(&self, p: [i64; 3]) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] && p[i] < self.max[i])
    }
}

/// What an operation does with the cells that are in one operand and not in the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boolean {
    /// In either.
    Union,
    /// In the first and not in the second.
    Difference,
    /// In both.
    Intersection,
}

impl Boolean {
    fn keep(self, a: bool, b: bool) -> bool {
        match self {
            Boolean::Union => a || b,
            Boolean::Difference => a && !b,
            Boolean::Intersection => a && b,
        }
    }
}

/// A solid: boxes that do not overlap, in the canonical form.
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct Solid {
    prisms: Vec<Prism>,
}

fn sorted_unique(mut v: Vec<i64>) -> Vec<i64> {
    v.sort_unstable();
    v.dedup();
    v
}

fn cell_count(grid: &[Vec<i64>; 3]) -> Option<usize> {
    let mut total = 1usize;
    for axis in grid {
        total = total.checked_mul(axis.len().saturating_sub(1))?;
    }
    Some(total)
}

/// Where a coordinate is among the sorted coordinates of a grid.
fn slot(axis: &[i64], value: i64) -> usize {
    axis.binary_search(&value).unwrap_or(0)
}

/// Takes out of the grid the planes across an axis where nothing changes: a slab of cells that
/// is the same as the slab before it joins that one.
fn reduce_axis(grid: &mut [Vec<i64>; 3], filled: &mut Vec<bool>, axis: usize) {
    let dims = [grid[0].len() - 1, grid[1].len() - 1, grid[2].len() - 1];
    let n = dims[axis];
    if n < 2 {
        return;
    }
    let (u, v) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let at = |d: &[usize; 3], s: usize, p: usize, q: usize| -> usize {
        let mut pos = [0usize; 3];
        pos[axis] = s;
        pos[u] = p;
        pos[v] = q;
        (pos[2] * d[1] + pos[1]) * d[0] + pos[0]
    };
    let same = |s: usize| {
        (0..dims[v]).all(|q| {
            (0..dims[u]).all(|p| filled[at(&dims, s, p, q)] == filled[at(&dims, s - 1, p, q)])
        })
    };
    let keep: Vec<bool> = (0..n).map(|s| s == 0 || !same(s)).collect();
    let kept = keep.iter().filter(|k| **k).count();
    if kept == n {
        return;
    }
    let mut new_dims = dims;
    new_dims[axis] = kept;
    let mut next = vec![false; new_dims.iter().product()];
    let mut new_slab = 0usize;
    for (s, k) in keep.iter().enumerate() {
        if !*k {
            continue;
        }
        for q in 0..dims[v] {
            for p in 0..dims[u] {
                next[at(&new_dims, new_slab, p, q)] = filled[at(&dims, s, p, q)];
            }
        }
        new_slab += 1;
    }
    let mut coords: Vec<i64> = keep
        .iter()
        .zip(&grid[axis])
        .filter(|(k, _)| **k)
        .map(|(_, c)| *c)
        .collect();
    coords.push(grid[axis][n]);
    grid[axis] = coords;
    *filled = next;
}

impl Solid {
    /// The solid with nothing in it.
    pub fn empty() -> Solid {
        Solid::default()
    }

    /// The union of some boxes (they may overlap), in the canonical form.
    pub fn from_prisms(prisms: &[Prism], limits: &Limits) -> Result<Solid, CadError> {
        Solid::combine(prisms, &[], Boolean::Union, limits)
    }

    /// One box.
    pub fn from_prism(prism: Prism) -> Solid {
        Solid {
            prisms: vec![prism],
        }
    }

    /// The boxes, in the canonical order.
    pub fn prisms(&self) -> &[Prism] {
        &self.prisms
    }

    /// Whether there is nothing in the solid.
    pub fn is_empty(&self) -> bool {
        self.prisms.is_empty()
    }

    /// The volume in cubic nanometres (exact).
    pub fn volume_nm3(&self) -> i128 {
        self.prisms.iter().map(Prism::volume_nm3).sum()
    }

    /// Whether a point is inside the solid.
    pub fn contains(&self, p: [i64; 3]) -> bool {
        self.prisms.iter().any(|b| b.contains(p))
    }

    /// The smallest box that holds the solid, as the two corners, or `None` when it is empty.
    pub fn bounds(&self) -> Option<([i64; 3], [i64; 3])> {
        let first = self.prisms.first()?;
        let (mut lo, mut hi) = (first.min, first.max);
        for b in &self.prisms {
            for i in 0..3 {
                lo[i] = lo[i].min(b.min[i]);
                hi[i] = hi[i].max(b.max[i]);
            }
        }
        Some((lo, hi))
    }

    /// The solid moved by the offsets. A corner that would leave the range of lengths is refused
    /// (`PHOENIX-CAD-001`).
    pub fn translate(&self, by: [Length; 3]) -> Result<Solid, CadError> {
        let mut prisms = Vec::with_capacity(self.prisms.len());
        for b in &self.prisms {
            let mut min = [0i64; 3];
            let mut max = [0i64; 3];
            for i in 0..3 {
                min[i] = Length::new(b.min[i])?.checked_add(by[i])?.get();
                max[i] = Length::new(b.max[i])?.checked_add(by[i])?.get();
            }
            prisms.push(Prism { min, max });
        }
        // moving keeps the shape of the decomposition; the boxes are listed by the lowest corner,
        // third axis first, which is the order the cells are joined in
        prisms.sort_unstable_by_key(|p| (p.min[2], p.min[1], p.min[0]));
        Ok(Solid { prisms })
    }

    /// The solid reflected in the plane across `axis` (0 for x, 1 for y, 2 for z) at the
    /// coordinate `at`: a point at `v` goes to `2 * at - v`. The result is in the canonical form;
    /// a coordinate that would leave the range of lengths is refused (`PHOENIX-CAD-001`) and an
    /// axis above 2 is refused (`PHOENIX-CAD-053`).
    pub fn mirrored(&self, axis: usize, at: Length, limits: &Limits) -> Result<Solid, CadError> {
        if axis > 2 {
            return Err(CadError::PrismLimit(format!(
                "the axis {axis} is not 0, 1 or 2"
            )));
        }
        let twice = at
            .get()
            .checked_mul(2)
            .ok_or(CadError::Overflow("plane of a mirror"))?;
        let mut prisms = Vec::with_capacity(self.prisms.len());
        for b in &self.prisms {
            let mut p = *b;
            p.min[axis] = Length::new(
                twice
                    .checked_sub(b.max[axis])
                    .ok_or(CadError::Overflow("mirrored prism"))?,
            )?
            .get();
            p.max[axis] = Length::new(
                twice
                    .checked_sub(b.min[axis])
                    .ok_or(CadError::Overflow("mirrored prism"))?,
            )?
            .get();
            prisms.push(p);
        }
        Solid::from_prisms(&prisms, limits)
    }

    /// The solid turned `quarters` quarter turns (counter-clockwise seen from the high end of the
    /// axis, so one quarter of the z axis takes x to y) about the line parallel to `axis` that
    /// passes through `centre`. Any whole number of quarters is taken, a negative one turns the
    /// other way. Only quarter turns keep the boxes aligned with the axes, so nothing is rounded.
    pub fn rotated(
        &self,
        axis: usize,
        quarters: i64,
        centre: [Length; 3],
        limits: &Limits,
    ) -> Result<Solid, CadError> {
        if axis > 2 {
            return Err(CadError::PrismLimit(format!(
                "the axis {axis} is not 0, 1 or 2"
            )));
        }
        let turns = quarters.rem_euclid(4);
        // the two axes of the plane of the turn, in the order that makes the turn counter-clockwise
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let (cu, cv) = (centre[u].get(), centre[v].get());
        let spin = |p: [i64; 3]| -> Result<[i64; 3], CadError> {
            let mut q = p;
            for _ in 0..turns {
                let (du, dv) = (
                    q[u].checked_sub(cu)
                        .ok_or(CadError::Overflow("turned prism"))?,
                    q[v].checked_sub(cv)
                        .ok_or(CadError::Overflow("turned prism"))?,
                );
                q[u] = Length::new(
                    cu.checked_sub(dv)
                        .ok_or(CadError::Overflow("turned prism"))?,
                )?
                .get();
                q[v] = Length::new(
                    cv.checked_add(du)
                        .ok_or(CadError::Overflow("turned prism"))?,
                )?
                .get();
            }
            Ok(q)
        };
        let mut prisms = Vec::with_capacity(self.prisms.len());
        for b in &self.prisms {
            let (a, c) = (spin(b.min)?, spin(b.max)?);
            let mut p = *b;
            for i in 0..3 {
                p.min[i] = a[i].min(c[i]);
                p.max[i] = a[i].max(c[i]);
            }
            prisms.push(p);
        }
        Solid::from_prisms(&prisms, limits)
    }

    /// The solid grown by `by` in every direction: the points that are at most `by` away from it
    /// along each axis (the sum of the solid and a cube, so the corners stay square). A negative
    /// distance is refused (`PHOENIX-CAD-002`) and a coordinate that would leave the range of
    /// lengths is refused (`PHOENIX-CAD-001`); zero gives the solid.
    pub fn grown(&self, by: Length, limits: &Limits) -> Result<Solid, CadError> {
        let d = by.get();
        if d < 0 {
            return Err(CadError::NotPositive("distance of a growth"));
        }
        if d == 0 {
            return Ok(self.clone());
        }
        let mut prisms = Vec::with_capacity(self.prisms.len());
        for b in &self.prisms {
            let mut p = *b;
            for i in 0..3 {
                p.min[i] = Length::new(
                    b.min[i]
                        .checked_sub(d)
                        .ok_or(CadError::Overflow("grown prism"))?,
                )?
                .get();
                p.max[i] = Length::new(
                    b.max[i]
                        .checked_add(d)
                        .ok_or(CadError::Overflow("grown prism"))?,
                )?
                .get();
            }
            prisms.push(p);
        }
        Solid::from_prisms(&prisms, limits)
    }

    /// The solid shrunk by `by`: the points whose whole cube of half side `by` is in the solid,
    /// the inverse of [`Solid::grown`] for the empty space around it (the space is grown and
    /// taken away). Walls thinner than twice the distance disappear. The same refusals as
    /// [`Solid::grown`].
    pub fn shrunk(&self, by: Length, limits: &Limits) -> Result<Solid, CadError> {
        let d = by.get();
        if d < 0 {
            return Err(CadError::NotPositive("distance of a shrinking"));
        }
        let Some((lo, hi)) = self.bounds() else {
            return Ok(Solid::empty());
        };
        if d == 0 {
            return Ok(self.clone());
        }
        // the space around the solid, in a box a little larger than the solid
        let mut corner = [Length::ZERO; 3];
        let mut size = [Length::ZERO; 3];
        for i in 0..3 {
            let low = lo[i]
                .checked_sub(d)
                .ok_or(CadError::Overflow("space around a solid"))?;
            let high = hi[i]
                .checked_add(d)
                .ok_or(CadError::Overflow("space around a solid"))?;
            corner[i] = Length::new(low)?;
            size[i] = Length::new(
                high.checked_sub(low)
                    .ok_or(CadError::Overflow("space around a solid"))?,
            )?;
        }
        let space = Solid::from_prism(Prism::new(corner, size)?).difference(self, limits)?;
        self.difference(&space.grown(by, limits)?, limits)
    }

    /// A hollow shell: the solid less its shrunk self, so a wall of thickness `by` stays along
    /// every face, the cavity is closed and the outside is the same. When the solid is thinner than
    /// twice the thickness nothing is taken away. The same refusals as [`Solid::grown`].
    pub fn shelled(&self, by: Length, limits: &Limits) -> Result<Solid, CadError> {
        let inner = self.shrunk(by, limits)?;
        self.difference(&inner, limits)
    }

    /// The union with another solid.
    pub fn union(&self, other: &Solid, limits: &Limits) -> Result<Solid, CadError> {
        Solid::combine(&self.prisms, &other.prisms, Boolean::Union, limits)
    }

    /// What is in this solid and not in the other.
    pub fn difference(&self, other: &Solid, limits: &Limits) -> Result<Solid, CadError> {
        Solid::combine(&self.prisms, &other.prisms, Boolean::Difference, limits)
    }

    /// What is in both solids.
    pub fn intersection(&self, other: &Solid, limits: &Limits) -> Result<Solid, CadError> {
        Solid::combine(&self.prisms, &other.prisms, Boolean::Intersection, limits)
    }

    /// The operation named, between two solids.
    pub fn apply(&self, op: Boolean, other: &Solid, limits: &Limits) -> Result<Solid, CadError> {
        Solid::combine(&self.prisms, &other.prisms, op, limits)
    }

    fn combine(a: &[Prism], b: &[Prism], op: Boolean, limits: &Limits) -> Result<Solid, CadError> {
        // the planes of both operands cut space into cells
        let mut grid: [Vec<i64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for p in a.iter().chain(b) {
            for (i, axis) in grid.iter_mut().enumerate() {
                axis.push(p.min[i]);
                axis.push(p.max[i]);
            }
        }
        let grid = [
            sorted_unique(std::mem::take(&mut grid[0])),
            sorted_unique(std::mem::take(&mut grid[1])),
            sorted_unique(std::mem::take(&mut grid[2])),
        ];
        let cells = cell_count(&grid)
            .filter(|c| *c <= limits.max_cells)
            .ok_or_else(|| {
                CadError::PrismLimit(format!(
                    "the grid of the operation has more than {} cells",
                    limits.max_cells
                ))
            })?;
        if cells == 0 {
            return Ok(Solid::empty());
        }
        let (nx, ny) = (grid[0].len() - 1, grid[1].len() - 1);
        let index = |i: usize, j: usize, k: usize| (k * ny + j) * nx + i;
        let mut in_a = vec![false; cells];
        let mut in_b = vec![false; cells];
        for (set, boxes) in [(&mut in_a, a), (&mut in_b, b)] {
            for p in boxes {
                let lo: Vec<usize> = (0..3).map(|i| slot(&grid[i], p.min[i])).collect();
                let hi: Vec<usize> = (0..3).map(|i| slot(&grid[i], p.max[i])).collect();
                for k in lo[2]..hi[2] {
                    for j in lo[1]..hi[1] {
                        for i in lo[0]..hi[0] {
                            set[index(i, j, k)] = true;
                        }
                    }
                }
            }
        }
        let filled: Vec<bool> = in_a
            .iter()
            .zip(&in_b)
            .map(|(x, y)| op.keep(*x, *y))
            .collect();
        Solid::from_cells(grid, filled, limits)
    }

    /// Puts the cells that are inside together: the planes where nothing changes are taken out
    /// of the grid, then the boxes are joined along the first axis, the second and the third.
    fn from_cells(
        mut grid: [Vec<i64>; 3],
        mut filled: Vec<bool>,
        limits: &Limits,
    ) -> Result<Solid, CadError> {
        for axis in 0..3 {
            reduce_axis(&mut grid, &mut filled, axis);
        }
        // join the cells into boxes, in a fixed order
        let (nx, ny, nz) = (grid[0].len() - 1, grid[1].len() - 1, grid[2].len() - 1);
        let index = |i: usize, j: usize, k: usize| (k * ny + j) * nx + i;
        let mut taken = vec![false; filled.len()];
        let mut prisms: Vec<Prism> = Vec::new();
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    if !filled[index(i, j, k)] || taken[index(i, j, k)] {
                        continue;
                    }
                    let mut i1 = i + 1;
                    while i1 < nx && filled[index(i1, j, k)] && !taken[index(i1, j, k)] {
                        i1 += 1;
                    }
                    let mut j1 = j + 1;
                    while j1 < ny
                        && (i..i1).all(|x| filled[index(x, j1, k)] && !taken[index(x, j1, k)])
                    {
                        j1 += 1;
                    }
                    let mut k1 = k + 1;
                    while k1 < nz
                        && (j..j1).all(|y| {
                            (i..i1).all(|x| filled[index(x, y, k1)] && !taken[index(x, y, k1)])
                        })
                    {
                        k1 += 1;
                    }
                    for z in k..k1 {
                        for y in j..j1 {
                            for x in i..i1 {
                                taken[index(x, y, z)] = true;
                            }
                        }
                    }
                    if prisms.len() >= limits.max_prisms {
                        return Err(CadError::PrismLimit(format!(
                            "the solid has more than {} prisms",
                            limits.max_prisms
                        )));
                    }
                    prisms.push(Prism {
                        min: [grid[0][i], grid[1][j], grid[2][k]],
                        max: [grid[0][i1], grid[1][j1], grid[2][k1]],
                    });
                }
            }
        }
        Ok(Solid { prisms })
    }
}

#[cfg(test)]
mod tests;
