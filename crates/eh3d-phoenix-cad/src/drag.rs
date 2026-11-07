//! The geometry of picking and dragging with the mouse: which drawn thing is under the pointer,
//! where on a plane of the model the pointer is, and where a dragged centre lands once it is moved
//! with the pointer and snapped to the grid.
//!
//! Everything here works on numbers (pixels, millimetres, integer nanometres) and knows nothing of
//! the model: the window turns the result into a change of the recipe, which the model accepts or
//! refuses with its own checks. Positions are snapped in integers, half away from zero, so that
//! the same drag gives the same position on every system.

use crate::projection::Point3;

/// A plane of the model: a point on it and two directions along it. The point `origin + s * a +
/// t * b` of the plane has the coordinates `(s, t)`; for a hole they are its `u` and `v`, for a
/// post its `x` and `y`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    /// A point of the plane, in millimetres.
    pub origin: Point3,
    /// The direction of the first coordinate.
    pub a: Point3,
    /// The direction of the second coordinate.
    pub b: Point3,
}

impl Plane {
    /// The point of the model that has the coordinates `(s, t)`.
    pub fn at(&self, s: f64, t: f64) -> Point3 {
        [
            self.origin[0] + self.a[0] * s + self.b[0] * t,
            self.origin[1] + self.a[1] * s + self.b[1] * t,
            self.origin[2] + self.a[2] * s + self.b[2] * t,
        ]
    }
}

/// Where something is drawn on a pane, in pixels, and its depth (larger is farther from the
/// viewer).
pub type Drawn = ((f64, f64), f64);

/// A length in millimetres as whole nanometres, rounded half away from zero; zero for a number
/// that is not finite.
pub fn mm_to_nm(mm: f64) -> i64 {
    if !mm.is_finite() {
        return 0;
    }
    let nm = (mm * 1_000_000.0).round();
    // far beyond the range of a length; the model refuses it with its own code
    nm.clamp(-4.0e18, 4.0e18) as i64
}

/// The multiple of `grid` nearest to `value`, half away from zero; the value itself when the
/// grid is not positive.
pub fn snap(value: i64, grid: i64) -> i64 {
    if grid <= 0 {
        return value;
    }
    let (v, g) = (i128::from(value), i128::from(grid));
    let (q, r) = (v.div_euclid(g), v.rem_euclid(g));
    // `v` lies between q * g and (q + 1) * g; exactly half way goes away from zero
    let up = match (2 * r).cmp(&g) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => v > 0,
    };
    let multiple = if up { q + 1 } else { q } * g;
    i64::try_from(multiple).unwrap_or(value)
}

/// How far the centre of a thing is from the pointer when it is picked: add this to the pointer
/// to keep the thing where it was in relation to the pointer, so that it does not jump.
pub fn grab_offset(centre: (f64, f64), pointer: (f64, f64)) -> (f64, f64) {
    (centre.0 - pointer.0, centre.1 - pointer.1)
}

/// Where the centre lands, in nanometres, for a pointer at `pointer` millimetres of the plane,
/// the offset of the grab and a grid of `grid` nanometres (no grid when it is not positive).
pub fn dragged(pointer: (f64, f64), offset: (f64, f64), grid: i64) -> (i64, i64) {
    (
        snap(mm_to_nm(pointer.0 + offset.0), grid),
        snap(mm_to_nm(pointer.1 + offset.1), grid),
    )
}

/// The candidate nearest to `pointer` that is within `radius` pixels of it. A candidate is the
/// pixel where it is drawn and its depth (larger is farther from the viewer); of two at the same
/// distance the nearer to the viewer wins, and then the first.
pub fn nearest(pointer: (f64, f64), candidates: &[Drawn], radius: f64) -> Option<usize> {
    let mut best: Option<(usize, f64, f64)> = None;
    for (k, ((x, y), depth)) in candidates.iter().enumerate() {
        let d = ((x - pointer.0).powi(2) + (y - pointer.1).powi(2)).sqrt();
        if d > radius {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, bd, bdepth)) => d < bd - 1e-9 || ((d - bd).abs() <= 1e-9 && *depth < bdepth),
        };
        if better {
            best = Some((k, d, *depth));
        }
    }
    best.map(|(k, _, _)| k)
}

/// The distance in pixels from a point to the segment `a`-`b`.
pub fn distance_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= 1e-12 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a.0 + dx * t, a.1 + dy * t);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MM: i64 = 1_000_000;

    #[test]
    fn a_length_in_millimetres_becomes_whole_nanometres() {
        assert_eq!(mm_to_nm(14.0), 14 * MM);
        assert_eq!(mm_to_nm(0.000_000_4), 0);
        assert_eq!(mm_to_nm(0.000_000_5), 1);
        assert_eq!(mm_to_nm(-0.000_000_5), -1);
        assert_eq!(mm_to_nm(f64::NAN), 0);
        assert_eq!(mm_to_nm(f64::INFINITY), 0);
        assert!(mm_to_nm(1e300) > 1 << 40);
    }

    #[test]
    fn a_position_snaps_to_the_nearest_multiple_of_the_grid() {
        let g = MM / 2; // 0.5 mm
        assert_eq!(snap(40_100_000, g), 40 * MM);
        assert_eq!(snap(40_300_000, g), 40 * MM + g);
        assert_eq!(snap(40_260_000, g), 40 * MM + g);
        assert_eq!(snap(40_240_000, g), 40 * MM);
        assert_eq!(snap(-40_100_000, g), -40 * MM);
        assert_eq!(snap(-40_300_000, g), -40 * MM - g);
        // exactly half way goes away from zero, on both sides
        assert_eq!(snap(250_000, g), 500_000);
        assert_eq!(snap(-250_000, g), -500_000);
        assert_eq!(snap(750_000, g), 1_000_000);
        assert_eq!(snap(-750_000, g), -1_000_000);
        assert_eq!(snap(0, g), 0);
        // no grid, no change
        assert_eq!(snap(40_123_457, 0), 40_123_457);
        assert_eq!(snap(40_123_457, -5), 40_123_457);
        // a grid that is odd in nanometres
        assert_eq!(snap(14, 3), 15);
        assert_eq!(snap(13, 3), 12);
        assert_eq!(snap(-14, 3), -15);
    }

    #[test]
    fn a_dragged_centre_keeps_its_place_under_the_pointer_and_snaps() {
        // picked at 14.3 mm while the centre is at 14 mm: the centre is 0.3 mm behind the pointer
        let offset = grab_offset((14.0, 15.0), (14.3, 14.8));
        assert!((offset.0 + 0.3).abs() < 1e-12 && (offset.1 - 0.2).abs() < 1e-12);
        // without moving, the centre stays on the grid point it was nearest to
        assert_eq!(dragged((14.3, 14.8), offset, MM / 2), (14 * MM, 15 * MM));
        // 10.1 mm to the right and 3.7 mm up of that: the centre is at 24.1 and 18.7 mm
        assert_eq!(
            dragged((24.4, 18.5), offset, MM / 2),
            (24 * MM, 18 * MM + MM / 2)
        );
        // with a grid of 1 mm
        assert_eq!(dragged((24.4, 18.5), offset, MM), (24 * MM, 19 * MM));
        // with no grid the position is exact to the nanometre
        assert_eq!(dragged((24.4, 18.5), offset, 0), (24_100_000, 18_700_000));
    }

    #[test]
    fn the_nearest_thing_inside_the_radius_is_picked_and_the_nearer_to_the_viewer_wins_a_tie() {
        let drawn = [
            ((100.0, 100.0), 5.0),
            ((110.0, 100.0), 1.0),
            ((300.0, 40.0), 0.0),
        ];
        assert_eq!(nearest((103.0, 100.0), &drawn, 10.0), Some(0));
        assert_eq!(nearest((108.0, 100.0), &drawn, 10.0), Some(1));
        // 3, 4, 5: the second is 5 pixels away, the first 5 too (a 3-4-5 triangle each)
        let tie = [((100.0, 100.0), 5.0), ((106.0, 100.0), 1.0)];
        assert_eq!(nearest((103.0, 104.0), &tie, 10.0), Some(1));
        // the same depth: the first
        let flat = [((100.0, 100.0), 2.0), ((106.0, 100.0), 2.0)];
        assert_eq!(nearest((103.0, 104.0), &flat, 10.0), Some(0));
        // out of reach
        assert_eq!(nearest((200.0, 200.0), &drawn, 10.0), None);
        assert_eq!(nearest((103.0, 100.0), &[], 10.0), None);
        // a radius of exactly the distance still picks
        assert_eq!(nearest((110.0, 110.0), &drawn, 10.0), Some(1));
    }

    #[test]
    fn the_distance_to_a_segment_is_to_its_nearest_point() {
        let (a, b) = ((0.0, 0.0), (10.0, 0.0));
        assert!((distance_to_segment((4.0, 3.0), a, b) - 3.0).abs() < 1e-12);
        assert!((distance_to_segment((13.0, 4.0), a, b) - 5.0).abs() < 1e-12);
        assert!((distance_to_segment((-3.0, 4.0), a, b) - 5.0).abs() < 1e-12);
        // a segment that is a point
        assert!((distance_to_segment((3.0, 4.0), a, a) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn a_plane_gives_the_point_of_its_coordinates() {
        let plane = Plane {
            origin: [2.0, 2.0, 35.0],
            a: [1.0, 0.0, 0.0],
            b: [0.0, 1.0, 0.0],
        };
        assert_eq!(plane.at(14.0, 15.0), [16.0, 17.0, 35.0]);
        // the front face looks the other way round: u goes to the right, v up
        let front = Plane {
            origin: [0.0, 0.0, 0.0],
            a: [1.0, 0.0, 0.0],
            b: [0.0, 0.0, 1.0],
        };
        assert_eq!(front.at(14.0, 15.0), [14.0, 0.0, 15.0]);
    }
}
