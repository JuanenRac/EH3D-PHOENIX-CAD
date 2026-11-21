use super::*;
use crate::mesh::read_stl;
use crate::prism::Prism;
use crate::solidquery::surface_area_nm2;
use crate::Length;

const MM: i64 = 1_000_000;

fn l(mm: i64) -> Length {
    Length::from_mm(mm).unwrap()
}

fn b(c: [i64; 3], s: [i64; 3]) -> Prism {
    Prism::new([l(c[0]), l(c[1]), l(c[2])], [l(s[0]), l(s[1]), l(s[2])]).unwrap()
}

fn solid(boxes: &[Prism]) -> Solid {
    Solid::from_prisms(boxes, &Limits::default()).unwrap()
}

/// A generator of numbers with a fixed seed, the same on every system.
struct Seed(u64);

impl Seed {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> i64 {
        (self.next() % n) as i64
    }

    fn boxes(&mut self) -> Solid {
        let count = 1 + self.below(6) as usize;
        let boxes: Vec<Prism> = (0..count)
            .map(|_| {
                b(
                    [self.below(7), self.below(7), self.below(7)],
                    [1 + self.below(4), 1 + self.below(4), 1 + self.below(4)],
                )
            })
            .collect();
        solid(&boxes)
    }
}

#[test]
fn a_box_is_six_rectangles_and_twelve_triangles() {
    let s = solid(&[b([0, 0, 0], [2, 3, 4])]);
    let q = boundary_quads(&s, &Limits::default()).unwrap();
    assert_eq!(q.len(), 6);
    assert_eq!(quads_volume_nm3(&q), s.volume_nm3());
    let bytes = solid_to_stl(&s, 100, &Limits::default()).unwrap();
    assert_eq!(bytes.len(), 84 + 50 * 12);
    assert_eq!(&bytes[..4], b"EH3D");
    assert_eq!(u32::from_le_bytes(bytes[80..84].try_into().unwrap()), 12);
}

#[test]
fn an_l_shaped_bar_keeps_its_flat_sides_whole_and_a_cavity_faces_inward() {
    // an L seen along y: ten rectangles, six sides and two caps of two rectangles each
    let l_bar = solid(&[b([0, 0, 0], [2, 1, 1]), b([0, 0, 1], [1, 1, 1])]);
    let q = boundary_quads(&l_bar, &Limits::default()).unwrap();
    assert_eq!(q.len(), 10, "{q:?}");
    // a cube with a cavity: six outside and six inside, the inside ones looking at the cavity
    let hollow = solid(&[b([0, 0, 0], [10, 10, 10])])
        .difference(&solid(&[b([2, 2, 2], [6, 6, 6])]), &Limits::default())
        .unwrap();
    let q = boundary_quads(&hollow, &Limits::default()).unwrap();
    assert_eq!(q.len(), 12);
    let x_faces: Vec<_> = q.iter().filter(|f| f.axis == 0).collect();
    assert_eq!(x_faces.len(), 4);
    // the cavity wall at x = 2 mm has the material on its low side, so its normal looks to the
    // high side, into the cavity; the one at x = 8 mm looks to the low side, into it as well
    let inner = x_faces.iter().find(|f| f.plane == 2 * MM).unwrap();
    assert!(inner.outward_high);
    let other = x_faces.iter().find(|f| f.plane == 8 * MM).unwrap();
    assert!(!other.outward_high);
    assert_eq!(quads_volume_nm3(&q), hollow.volume_nm3());
    assert_eq!(quads_volume_nm3(&q), (1000 - 216) * i128::from(MM).pow(3));
}

fn tetra_volume_mm3(file: &[[f64; 9]]) -> f64 {
    file.iter()
        .map(|t| {
            let (a, b, c) = ([t[0], t[1], t[2]], [t[3], t[4], t[5]], [t[6], t[7], t[8]]);
            let cross = [
                b[1] * c[2] - b[2] * c[1],
                b[2] * c[0] - b[0] * c[2],
                b[0] * c[1] - b[1] * c[0],
            ];
            (a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]) / 6.0
        })
        .sum()
}

#[test]
fn random_solids_give_a_surface_with_the_exact_volume_and_area() {
    let mut seed = Seed(0x0571_501d);
    for case in 0..80 {
        let s = seed.boxes();
        let lim = Limits::default();
        let q = boundary_quads(&s, &lim).unwrap();
        assert_eq!(quads_volume_nm3(&q), s.volume_nm3(), "case {case}");
        assert_eq!(
            q.iter().map(Quad::area_nm2).sum::<i128>(),
            surface_area_nm2(&s, &lim).unwrap(),
            "case {case}"
        );
        // no two rectangles of the same side overlap: their areas add up to the surface only
        // if they are disjoint, which the area above already says; the planes are on the grid
        assert!(q
            .iter()
            .all(|f| f.low[0] < f.high[0] && f.low[1] < f.high[1]));
        let bytes = solid_to_stl(&s, 1_000_000, &lim).unwrap();
        let read = read_stl(&bytes).unwrap();
        assert_eq!(read.len(), q.len() * 2, "case {case}");
        // whole millimetres are exact in 32 bits, so the volume of the triangles is exact too
        let volume = tetra_volume_mm3(&read);
        let want = (s.volume_nm3() / i128::from(MM).pow(3)) as f64;
        assert!((volume - want).abs() < 1e-6, "case {case}: {volume} {want}");
        // the same solid, the same bytes
        assert_eq!(bytes, solid_to_stl(&s, 1_000_000, &lim).unwrap());
    }
}

#[test]
fn a_moved_solid_gives_the_same_file_shape_and_the_refusals_have_their_codes() {
    let s = solid(&[b([0, 0, 0], [3, 3, 3])]);
    let moved = s.translate([l(5), l(-2), l(7)]).unwrap();
    let lim = Limits::default();
    let a = boundary_quads(&s, &lim).unwrap();
    let m = boundary_quads(&moved, &lim).unwrap();
    assert_eq!(a.len(), m.len());
    assert_eq!(quads_volume_nm3(&m), moved.volume_nm3());
    assert_eq!(
        solid_to_stl(&Solid::empty(), 100, &lim).unwrap_err().code(),
        "PHOENIX-CAD-041"
    );
    assert_eq!(
        solid_to_stl(&s, 11, &lim).unwrap_err().code(),
        "PHOENIX-CAD-050"
    );
    assert!(solid_to_stl(&s, 12, &lim).is_ok());
    let tiny = Limits {
        max_cells: 1,
        max_prisms: 10,
    };
    let two = solid(&[b([0, 0, 0], [1, 1, 1]), b([3, 0, 0], [1, 1, 1])]);
    assert_eq!(
        boundary_quads(&two, &tiny).unwrap_err().code(),
        "PHOENIX-CAD-053"
    );
}
