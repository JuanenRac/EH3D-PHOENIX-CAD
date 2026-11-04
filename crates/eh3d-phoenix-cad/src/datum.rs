//! Datums: a reference frame with an origin and an orientation, for placing things in the model.
//!
//! The orientation is one of the 24 rotations that keep the axes of the box on the axes (a signed
//! permutation of the axes with determinant one), so every calculation is exact in integers. The
//! frame of a face has its origin at the lower left corner of the face as seen from outside, its
//! x axis along `u` (to the right), its y axis along `v` (up) and its z axis pointing out of the
//! box. The outer box has its smallest corner at the origin of the model.

use crate::enclosure::{Enclosure, Face};
use crate::{CadError, Length};

/// A point or a vector of three lengths (x, y, z).
pub type Point = [Length; 3];

/// A rotation that maps the axes onto the axes: `global = matrix * local`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rotation {
    m: [[i8; 3]; 3],
}

/// The index of the axis that a vector of the form (0, 0, +-1) in some order points along.
fn axis_of(a: [i8; 3]) -> Option<usize> {
    let nonzero: Vec<usize> = (0..3).filter(|i| a[*i] != 0).collect();
    match nonzero.as_slice() {
        [i] if a[*i] == 1 || a[*i] == -1 => Some(*i),
        _ => None,
    }
}

impl Rotation {
    /// No rotation.
    pub const IDENTITY: Rotation = Rotation {
        m: [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
    };

    /// The rotation whose x axis is `u` and whose y axis is `v`; the z axis is `u x v`. Both
    /// must be unit vectors along different axes.
    pub fn from_axes(u: [i8; 3], v: [i8; 3]) -> Result<Rotation, CadError> {
        match (axis_of(u), axis_of(v)) {
            (Some(a), Some(b)) if a != b => {}
            _ => return Err(CadError::ValueOutOfRange("axis")),
        }
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let m = [[u[0], v[0], n[0]], [u[1], v[1], n[1]], [u[2], v[2], n[2]]];
        Ok(Rotation { m })
    }

    /// The 24 rotations, in a fixed order.
    pub fn all() -> Vec<Rotation> {
        let axes: [[i8; 3]; 6] = [
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, -1, 0],
            [0, 0, 1],
            [0, 0, -1],
        ];
        let mut out = Vec::with_capacity(24);
        for u in axes {
            for v in axes {
                if let Ok(r) = Rotation::from_axes(u, v) {
                    out.push(r);
                }
            }
        }
        out
    }

    /// Rotates a vector of integers.
    pub fn apply(&self, p: [i64; 3]) -> [i64; 3] {
        let mut out = [0i64; 3];
        for (r, o) in out.iter_mut().enumerate() {
            *o = (0..3).map(|c| i64::from(self.m[r][c]) * p[c]).sum();
        }
        out
    }

    /// The rotation that undoes this one.
    pub fn inverse(&self) -> Rotation {
        let mut m = [[0i8; 3]; 3];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = self.m[c][r];
            }
        }
        Rotation { m }
    }

    /// The rotation that does `other` first and this one after.
    pub fn compose(&self, other: &Rotation) -> Rotation {
        let mut m = [[0i8; 3]; 3];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = (0..3).map(|k| self.m[r][k] * other.m[k][c]).sum();
            }
        }
        Rotation { m }
    }
}

/// A reference frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Datum {
    /// The origin in the model.
    pub origin: Point,
    /// The orientation.
    pub rotation: Rotation,
}

fn to_ints(p: Point) -> [i64; 3] {
    [p[0].get(), p[1].get(), p[2].get()]
}

fn from_ints(p: [i64; 3]) -> Result<Point, CadError> {
    Ok([Length::new(p[0])?, Length::new(p[1])?, Length::new(p[2])?])
}

impl Datum {
    /// The point of the model at a point of this frame.
    pub fn to_global(&self, local: Point) -> Result<Point, CadError> {
        let r = self.rotation.apply(to_ints(local));
        let o = to_ints(self.origin);
        from_ints([r[0] + o[0], r[1] + o[1], r[2] + o[2]])
    }

    /// The point of this frame at a point of the model.
    pub fn to_local(&self, global: Point) -> Result<Point, CadError> {
        let g = to_ints(global);
        let o = to_ints(self.origin);
        from_ints(
            self.rotation
                .inverse()
                .apply([g[0] - o[0], g[1] - o[1], g[2] - o[2]]),
        )
    }

    /// The frame of a face of an enclosure.
    pub fn of_face(enclosure: &Enclosure, face: Face) -> Datum {
        let (x, y, z) = enclosure.params().outer();
        let zero = Length::ZERO;
        let (origin, u, v) = match face {
            Face::Front => ([zero, zero, zero], [1, 0, 0], [0, 0, 1]),
            Face::Back => ([x, y, zero], [-1, 0, 0], [0, 0, 1]),
            Face::Left => ([zero, y, zero], [0, -1, 0], [0, 0, 1]),
            Face::Right => ([x, zero, zero], [0, 1, 0], [0, 0, 1]),
            Face::Top => ([zero, zero, z], [1, 0, 0], [0, 1, 0]),
            Face::Bottom => ([zero, y, zero], [1, 0, 0], [0, -1, 0]),
        };
        Datum {
            origin,
            rotation: Rotation::from_axes(u, v).expect("the axes of a face are valid"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::EnclosureParams;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn enclosure() -> Enclosure {
        Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap()
    }

    #[test]
    fn there_are_24_different_rotations_and_each_has_an_inverse() {
        let all = Rotation::all();
        assert_eq!(all.len(), 24);
        let mut unique = all.clone();
        unique.sort_by_key(|r| r.m);
        unique.dedup();
        assert_eq!(unique.len(), 24);
        for r in &all {
            assert_eq!(r.compose(&r.inverse()), Rotation::IDENTITY);
            assert_eq!(r.inverse().compose(r), Rotation::IDENTITY);
            // a rotation keeps lengths: the sum of the absolute values stays the same
            let p = r.apply([1, 20, 300]);
            assert_eq!(p.iter().map(|v| v.abs()).sum::<i64>(), 321);
        }
        assert_eq!(Rotation::IDENTITY.apply([4, 5, 6]), [4, 5, 6]);
    }

    #[test]
    fn composing_rotations_is_doing_one_after_the_other() {
        let quarter = Rotation::from_axes([0, 1, 0], [-1, 0, 0]).unwrap();
        assert_eq!(quarter.apply([1, 0, 0]), [0, 1, 0]);
        assert_eq!(quarter.apply([0, 1, 0]), [-1, 0, 0]);
        let half = quarter.compose(&quarter);
        assert_eq!(half.apply([1, 2, 3]), [-1, -2, 3]);
        let full = half.compose(&half);
        assert_eq!(full, Rotation::IDENTITY);
    }

    #[test]
    fn axes_that_are_not_unit_axes_or_are_parallel_are_refused() {
        for (u, v) in [
            ([1, 1, 0], [0, 0, 1]),
            ([2, 0, 0], [0, 1, 0]),
            ([0, 0, 0], [0, 1, 0]),
            ([1, 0, 0], [-1, 0, 0]),
            ([1, 0, 0], [1, 0, 0]),
        ] {
            assert_eq!(
                Rotation::from_axes(u, v).unwrap_err().code(),
                "PHOENIX-CAD-007",
                "{u:?} {v:?}"
            );
        }
    }

    #[test]
    fn the_frame_of_a_face_puts_a_hole_where_the_script_puts_it() {
        let e = enclosure();
        let (x, y, z) = (104, 64, 35);
        let (u, v) = (20, 17);
        // the same placements as the OpenSCAD-style script uses for a hole at (u, v)
        let expected = [
            (Face::Front, [u, 0, v]),
            (Face::Back, [x - u, y, v]),
            (Face::Left, [0, y - u, v]),
            (Face::Right, [x, u, v]),
            (Face::Top, [u, v, z]),
            (Face::Bottom, [u, y - v, 0]),
        ];
        for (face, want) in expected {
            let d = Datum::of_face(&e, face);
            let got = d.to_global([mm(u), mm(v), Length::ZERO]).unwrap();
            assert_eq!(got, [mm(want[0]), mm(want[1]), mm(want[2])], "{face:?}");
        }
    }

    #[test]
    fn the_z_axis_of_a_face_points_out_of_the_box() {
        let e = enclosure();
        let out = |face| Datum::of_face(&e, face).rotation.apply([0, 0, 1]);
        assert_eq!(out(Face::Front), [0, -1, 0]);
        assert_eq!(out(Face::Back), [0, 1, 0]);
        assert_eq!(out(Face::Left), [-1, 0, 0]);
        assert_eq!(out(Face::Right), [1, 0, 0]);
        assert_eq!(out(Face::Top), [0, 0, 1]);
        assert_eq!(out(Face::Bottom), [0, 0, -1]);
    }

    #[test]
    fn a_point_goes_to_the_model_and_back_unchanged() {
        let e = enclosure();
        for face in Face::ALL {
            let d = Datum::of_face(&e, face);
            let local = [mm(7), mm(-3), mm(11)];
            let global = d.to_global(local).unwrap();
            assert_eq!(d.to_local(global).unwrap(), local, "{face:?}");
        }
        // a point out of the range is refused, not wrapped
        let d = Datum::of_face(&e, Face::Top);
        let far = Length::new((1 << 40) - 1).unwrap();
        assert_eq!(
            d.to_global([Length::ZERO, Length::ZERO, far])
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-001"
        );
    }
}
