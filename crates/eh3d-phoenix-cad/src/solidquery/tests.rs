use super::*;
use crate::prism::Prism;
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
        let count = 1 + self.below(5) as usize;
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

/// A cube of 10 mm with a cavity of 6 mm inside it, the walls 2 mm thick.
fn hollow(offset: i64) -> Solid {
    let outer = solid(&[b([0, 0, 0], [10, 10, 10])]);
    let cavity = solid(&[b([2 + offset, 2, 2], [6, 6, 6])]);
    outer.difference(&cavity, &Limits::default()).unwrap()
}

#[test]
fn the_walls_of_a_hollow_cube_are_found_by_hand() {
    let s = hollow(0);
    for axis in 0..3 {
        let t = thinnest(&s, axis, &Limits::default()).unwrap().unwrap();
        assert_eq!(t.run.thickness(), 2 * MM, "axis {axis}");
    }
    // through the middle of the cube along x: wall, cavity, wall
    let at = thickness_at(&s, [MM, 5 * MM, 5 * MM], 0).unwrap();
    assert_eq!((at.from, at.to), (0, 2 * MM));
    let far = thickness_at(&s, [9 * MM, 5 * MM, 5 * MM], 0).unwrap();
    assert_eq!((far.from, far.to), (8 * MM, 10 * MM));
    // inside the cavity there is no material, and neither outside the cube
    assert!(thickness_at(&s, [5 * MM, 5 * MM, 5 * MM], 0).is_none());
    assert!(thickness_at(&s, [11 * MM, 5 * MM, 5 * MM], 0).is_none());
    // a line off the cavity crosses the whole cube
    let full = thickness_at(&s, [5 * MM, MM, MM], 0).unwrap();
    assert_eq!(full.thickness(), 10 * MM);
    assert!(thickness_at(&s, [0, 0, 0], 3).is_none());
}

#[test]
fn moving_the_cavity_thins_one_wall_and_the_check_says_where() {
    // the cavity starts 1 mm from the left face: 2 mm becomes 1 mm and the right wall 3 mm
    let s = solid(&[b([0, 0, 0], [10, 10, 10])])
        .difference(&solid(&[b([1, 2, 2], [6, 6, 6])]), &Limits::default())
        .unwrap();
    let t = thinnest(&s, 0, &Limits::default()).unwrap().unwrap();
    assert_eq!((t.run.from, t.run.to), (0, MM));
    assert_eq!((t.low, t.high), ([2 * MM, 2 * MM], [8 * MM, 8 * MM]));
    let thin = walls(&s, 0, Some(2 * MM), 100, &Limits::default()).unwrap();
    assert_eq!(thin.len(), 1, "{thin:?}");
    assert_eq!(thin[0], t);
    // with a limit of 1 mm nothing is thinner, because the test is strictly less
    assert!(walls(&s, 0, Some(MM), 100, &Limits::default())
        .unwrap()
        .is_empty());
    // the other axes still have 2 mm walls
    assert_eq!(
        thinnest(&s, 1, &Limits::default())
            .unwrap()
            .unwrap()
            .run
            .thickness(),
        2 * MM
    );
}

#[test]
fn a_pin_beside_a_plate_is_a_thin_wall_and_the_list_is_capped() {
    let s = solid(&[b([0, 0, 0], [20, 20, 2]), b([5, 5, 2], [1, 1, 8])]);
    // along z the plate is 2 mm everywhere except under the pin, which is 10 mm
    let thin = walls(&s, 2, Some(3 * MM), 100, &Limits::default()).unwrap();
    assert!(thin.iter().all(|w| w.run.thickness() == 2 * MM));
    let area: i128 = thin
        .iter()
        .map(|w| i128::from(w.high[0] - w.low[0]) * i128::from(w.high[1] - w.low[1]))
        .sum();
    assert_eq!(area, (20 * 20 - 1) * i128::from(MM) * i128::from(MM));
    // along x the pin is 1 mm thick
    let pin = walls(&s, 0, Some(2 * MM), 100, &Limits::default()).unwrap();
    assert!(pin.iter().any(|w| w.run.thickness() == MM));
    let capped = walls(&s, 2, Some(3 * MM), 1, &Limits::default()).unwrap();
    assert_eq!(capped.len(), 1);
    assert!(walls(&Solid::empty(), 0, None, 10, &Limits::default())
        .unwrap()
        .is_empty());
    assert_eq!(
        thinnest(&Solid::empty(), 0, &Limits::default()).unwrap(),
        None
    );
    assert!(walls(&s, 5, None, 10, &Limits::default()).is_err());
}

#[test]
fn the_surface_area_of_known_solids_is_exact() {
    // a box: 2 (ab + bc + ca)
    let one = solid(&[b([0, 0, 0], [3, 4, 5])]);
    let mm2 = i128::from(MM) * i128::from(MM);
    assert_eq!(
        surface_area_nm2(&one, &Limits::default()).unwrap(),
        2 * (12 + 20 + 15) * mm2
    );
    // two boxes that touch on a face: the shared faces are not surface
    let two = solid(&[b([0, 0, 0], [1, 1, 1]), b([1, 0, 0], [1, 1, 1])]);
    assert_eq!(
        surface_area_nm2(&two, &Limits::default()).unwrap(),
        10 * mm2
    );
    // the hollow cube: outside 600, inside 216
    assert_eq!(
        surface_area_nm2(&hollow(0), &Limits::default()).unwrap(),
        (600 + 216) * mm2
    );
    assert_eq!(
        surface_area_nm2(&Solid::empty(), &Limits::default()).unwrap(),
        0
    );
}

#[test]
fn the_centre_of_mass_of_known_solids_is_exact_and_rounded_half_away() {
    let cube = solid(&[b([0, 0, 0], [10, 10, 10])]);
    assert_eq!(centroid_nm(&cube).unwrap(), Some([5 * MM, 5 * MM, 5 * MM]));
    // a hollow cube with a centred cavity keeps its centre; a moved cavity moves it away
    assert_eq!(
        centroid_nm(&hollow(0)).unwrap(),
        Some([5 * MM, 5 * MM, 5 * MM])
    );
    let moved = centroid_nm(&hollow(-1)).unwrap().unwrap();
    assert!(moved[0] > 5 * MM && moved[1] == 5 * MM && moved[2] == 5 * MM);
    // by hand: 1000 * 5 - 216 * 4 = 4136; 4136 / 784 is the x of the material
    assert_eq!(
        moved[0],
        div_round_half_away(4_136 * i128::from(MM), 784) as i64
    );
    // two boxes of different size: (1 * 0.5 + 8 * 2) / 9 mm
    let two = solid(&[b([0, 0, 0], [1, 1, 1]), b([1, 0, 0], [2, 2, 2])]);
    let c = centroid_nm(&two).unwrap().unwrap();
    assert_eq!(
        c[0],
        div_round_half_away(1_000_000 * 5 + 8_000_000 * 20, 90) as i64
    );
    assert_eq!(centroid_nm(&Solid::empty()).unwrap(), None);
    // 0.5 nm rounds away from zero: two nanometres of material centred on 1.5 nm
    let tiny = Solid::from_prism(
        Prism::new([Length::new(1).unwrap(); 3], [Length::new(1).unwrap(); 3]).unwrap(),
    );
    assert_eq!(centroid_nm(&tiny).unwrap(), Some([2, 2, 2]));
    let negative = Solid::from_prism(
        Prism::new([Length::new(-2).unwrap(); 3], [Length::new(1).unwrap(); 3]).unwrap(),
    );
    assert_eq!(centroid_nm(&negative).unwrap(), Some([-2, -2, -2]));
}

/// The cells of a solid on a grid of whole millimetres, to count by brute force.
fn voxels(s: &Solid) -> Vec<[i64; 3]> {
    let mut out = Vec::new();
    for z in 0..12 {
        for y in 0..12 {
            for x in 0..12 {
                if s.contains([x * MM, y * MM, z * MM]) {
                    out.push([x, y, z]);
                }
            }
        }
    }
    out
}

#[test]
fn random_solids_agree_with_counting_the_cells_one_by_one() {
    let mut seed = Seed(0x5eed_c0de);
    for case in 0..60 {
        let s = seed.boxes();
        let cells = voxels(&s);
        let set: std::collections::BTreeSet<[i64; 3]> = cells.iter().copied().collect();
        // the area: every face of a cell whose neighbour is not material
        let mut faces = 0i128;
        for c in &cells {
            for axis in 0..3 {
                for step in [-1, 1] {
                    let mut n = *c;
                    n[axis] += step;
                    if !set.contains(&n) {
                        faces += 1;
                    }
                }
            }
        }
        let mm2 = i128::from(MM) * i128::from(MM);
        assert_eq!(
            surface_area_nm2(&s, &Limits::default()).unwrap(),
            faces * mm2,
            "case {case}"
        );
        // the volume agrees with the cells too
        assert_eq!(
            s.volume_nm3(),
            cells.len() as i128 * i128::from(MM).pow(3),
            "case {case}"
        );
        // the centre: the mean of the centres of the cells, twice, over twice the count
        let n = cells.len() as i128;
        let c = centroid_nm(&s).unwrap().unwrap();
        for axis in 0..3 {
            let sum: i128 = cells.iter().map(|v| i128::from(2 * v[axis] + 1)).sum();
            let want = div_round_half_away(sum * i128::from(MM), 2 * n) as i64;
            assert_eq!(c[axis], want, "case {case} axis {axis}");
        }
        // the thinnest run along each axis agrees with the runs of the lines of cells
        for axis in 0..3 {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let mut best: Option<i64> = None;
            for a in 0..12 {
                for b2 in 0..12 {
                    let mut run = 0i64;
                    for t in 0..=12 {
                        let mut p = [0i64; 3];
                        p[axis] = t;
                        p[u] = a;
                        p[v] = b2;
                        if t < 12 && set.contains(&p) {
                            run += 1;
                        } else if run > 0 {
                            best = Some(best.map_or(run, |x| x.min(run)));
                            run = 0;
                        }
                    }
                }
            }
            let got = thinnest(&s, axis, &Limits::default())
                .unwrap()
                .unwrap()
                .run
                .thickness();
            assert_eq!(got, best.unwrap() * MM, "case {case} axis {axis}");
        }
    }
}

#[test]
fn a_grid_beyond_the_limit_is_refused_with_its_code() {
    let s = hollow(0);
    let small = Limits {
        max_cells: 4,
        max_prisms: 100,
    };
    for e in [
        surface_area_nm2(&s, &small).unwrap_err(),
        thinnest(&s, 0, &small).unwrap_err(),
    ] {
        assert_eq!(e.code(), "PHOENIX-CAD-053");
    }
}

#[test]
fn mirroring_and_turning_keep_the_volume_and_follow_by_hand() {
    let lim = Limits::default();
    let s = solid(&[b([1, 2, 3], [2, 3, 4])]);
    // mirrored across x at 0: the box goes from [1,3) to (-3,-1]
    let m = s.mirrored(0, l(0), &lim).unwrap();
    assert_eq!(m.bounds().unwrap().0, [-3 * MM, 2 * MM, 3 * MM]);
    assert_eq!(m.bounds().unwrap().1, [-MM, 5 * MM, 7 * MM]);
    assert_eq!(m.mirrored(0, l(0), &lim).unwrap(), s);
    // a quarter turn about z through the origin takes (x, y) to (-y, x)
    let q = s.rotated(2, 1, [l(0); 3], &lim).unwrap();
    assert_eq!(q.bounds().unwrap().0, [-5 * MM, MM, 3 * MM]);
    assert_eq!(q.bounds().unwrap().1, [-2 * MM, 3 * MM, 7 * MM]);
    // four quarters, or none, or minus four, are the same solid; minus one undoes one
    for n in [0, 4, -4, 8] {
        assert_eq!(s.rotated(2, n, [l(1); 3], &lim).unwrap(), s);
    }
    assert_eq!(
        s.rotated(2, 1, [l(1); 3], &lim)
            .unwrap()
            .rotated(2, -1, [l(1); 3], &lim)
            .unwrap(),
        s
    );
    assert_eq!(
        s.rotated(1, 3, [l(0); 3], &lim).unwrap().volume_nm3(),
        s.volume_nm3()
    );
    assert!(s.mirrored(3, l(0), &lim).is_err());
    assert!(s.rotated(3, 1, [l(0); 3], &lim).is_err());
}

#[test]
fn random_mirrors_and_turns_keep_the_measures_and_come_back() {
    let mut seed = Seed(0x0ddb_0a11);
    let lim = Limits::default();
    for case in 0..80 {
        let s = seed.boxes();
        let axis = seed.below(3) as usize;
        let at = l(seed.below(9) - 4);
        let m = s.mirrored(axis, at, &lim).unwrap();
        assert_eq!(m.volume_nm3(), s.volume_nm3(), "case {case}");
        assert_eq!(m.mirrored(axis, at, &lim).unwrap(), s, "case {case}");
        assert_eq!(
            surface_area_nm2(&m, &lim).unwrap(),
            surface_area_nm2(&s, &lim).unwrap(),
            "case {case}"
        );
        let centre = [l(seed.below(5)), l(seed.below(5)), l(seed.below(5))];
        let quarters = seed.below(9) - 4;
        let r = s.rotated(axis, quarters, centre, &lim).unwrap();
        assert_eq!(r.volume_nm3(), s.volume_nm3(), "case {case}");
        assert_eq!(
            surface_area_nm2(&r, &lim).unwrap(),
            surface_area_nm2(&s, &lim).unwrap(),
            "case {case}"
        );
        assert_eq!(
            r.rotated(axis, -quarters, centre, &lim).unwrap(),
            s,
            "case {case}"
        );
        // the centre of mass is turned like a point about the same line
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let c = centroid_nm(&s).unwrap().unwrap();
        let rc = centroid_nm(&r).unwrap().unwrap();
        let (mut du, mut dv) = (c[u] - centre[u].get(), c[v] - centre[v].get());
        for _ in 0..quarters.rem_euclid(4) {
            (du, dv) = (-dv, du);
        }
        assert!((rc[u] - (centre[u].get() + du)).abs() <= 1, "case {case}");
        assert!((rc[v] - (centre[v].get() + dv)).abs() <= 1, "case {case}");
        assert_eq!(rc[axis], c[axis], "case {case}");
    }
}

/// The area of a union of rectangles `[u0, u1) x [v0, v1)`, counted cell by cell on the grid of
/// their own coordinates: a way to the answer that shares no code with the library.
fn union_area(rects: &[[i64; 4]]) -> i128 {
    let mut us: Vec<i64> = rects.iter().flat_map(|r| [r[0], r[1]]).collect();
    let mut vs: Vec<i64> = rects.iter().flat_map(|r| [r[2], r[3]]).collect();
    us.sort_unstable();
    us.dedup();
    vs.sort_unstable();
    vs.dedup();
    let mut total: i128 = 0;
    for uw in us.windows(2) {
        for vw in vs.windows(2) {
            let covered = rects
                .iter()
                .any(|r| r[0] <= uw[0] && uw[1] <= r[1] && r[2] <= vw[0] && vw[1] <= r[3]);
            if covered {
                total += i128::from(uw[1] - uw[0]) * i128::from(vw[1] - vw[0]);
            }
        }
    }
    total
}

const MM2: i128 = (MM as i128) * (MM as i128);

#[test]
fn the_section_and_the_shadow_of_a_hollow_cube_are_worked_by_hand() {
    let s = hollow(0);
    let lim = Limits::default();
    // through the cavity: the square of 10 mm less the square of 6 mm
    assert_eq!(section_area_nm2(&s, 0, 5 * MM, &lim).unwrap(), 64 * MM2);
    // through a wall, on the lower face (which belongs to the cube), and just under the upper
    assert_eq!(section_area_nm2(&s, 0, MM, &lim).unwrap(), 100 * MM2);
    assert_eq!(section_area_nm2(&s, 1, 0, &lim).unwrap(), 100 * MM2);
    assert_eq!(
        section_area_nm2(&s, 2, 10 * MM - 1, &lim).unwrap(),
        100 * MM2
    );
    // on the upper face, outside, and for an axis that does not exist
    assert_eq!(section_area_nm2(&s, 2, 10 * MM, &lim).unwrap(), 0);
    assert_eq!(section_area_nm2(&s, 0, -MM, &lim).unwrap(), 0);
    assert_eq!(section_area_nm2(&s, 3, 0, &lim).unwrap(), 0);
    // a closed cavity does not show in the shadow
    for axis in 0..3 {
        assert_eq!(shadow_area_nm2(&s, axis, &lim).unwrap(), 100 * MM2);
    }
    assert_eq!(shadow_area_nm2(&s, 3, &lim).unwrap(), 0);
    // an L of two boxes: 4x2x1 with 1x2x3 on its end, seen from above and from the side
    let l_shape = solid(&[b([0, 0, 0], [4, 2, 1]), b([0, 0, 1], [1, 2, 3])]);
    assert_eq!(shadow_area_nm2(&l_shape, 2, &lim).unwrap(), 8 * MM2);
    assert_eq!(shadow_area_nm2(&l_shape, 1, &lim).unwrap(), 7 * MM2);
    assert_eq!(
        section_area_nm2(&l_shape, 0, 2 * MM, &lim).unwrap(),
        2 * MM2
    );
    assert_eq!(
        section_area_nm2(&l_shape, 2, 2 * MM, &lim).unwrap(),
        2 * MM2
    );
}

#[test]
fn a_seeded_family_of_sections_and_shadows_agrees_with_counting_rectangles() {
    let lim = Limits::default();
    let mut seed = Seed(0x5ec7_10a5);
    for case in 0..60 {
        let s = seed.boxes();
        for axis in 0..3 {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            // the shadow is the union of the rectangles of all the prisms
            let all: Vec<[i64; 4]> = s
                .prisms()
                .iter()
                .map(|p| [p.min()[u], p.max()[u], p.min()[v], p.max()[v]])
                .collect();
            assert_eq!(
                shadow_area_nm2(&s, axis, &lim).unwrap(),
                union_area(&all),
                "case {case} axis {axis}"
            );
            // a section is the union of the rectangles of the prisms the plane crosses
            let at = seed.below(24) * 500_000 - MM;
            let cut: Vec<[i64; 4]> = s
                .prisms()
                .iter()
                .filter(|p| Prism::min(p)[axis] <= at && at < Prism::max(p)[axis])
                .map(|p| [p.min()[u], p.max()[u], p.min()[v], p.max()[v]])
                .collect();
            assert_eq!(
                section_area_nm2(&s, axis, at, &lim).unwrap(),
                union_area(&cut),
                "case {case} axis {axis} at {at}"
            );
            // the sections at every plane, each as thick as its slab, add up to the volume
            let planes = Grid::new(&s, &lim).unwrap().planes(axis).to_vec();
            let mut volume: i128 = 0;
            for w in planes.windows(2) {
                volume += section_area_nm2(&s, axis, w[0], &lim).unwrap() * i128::from(w[1] - w[0]);
            }
            assert_eq!(volume, s.volume_nm3(), "case {case} axis {axis}");
        }
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs()))
}

#[test]
fn the_inertia_of_a_cube_and_of_a_box_is_worked_by_hand() {
    let cube = solid(&[b([3, -4, 7], [10, 10, 10])]);
    let i = inertia(&cube).unwrap();
    assert!(near(i.volume_mm3, 1000.0));
    assert_eq!(i.centre_mm, [8.0, 1.0, 12.0]);
    // a cube of side a about its centre: a^5 / 6 = 100000 / 6 on the diagonal, nothing off it
    for r in 0..3 {
        for c in 0..3 {
            let want = if r == c { 100_000.0 / 6.0 } else { 0.0 };
            assert!(near(i.tensor[r][c], want), "{r} {c}: {}", i.tensor[r][c]);
        }
    }
    // a box of 2 x 3 x 4: V (b^2 + c^2) / 12 with V = 24
    let bx = inertia(&solid(&[b([0, 0, 0], [2, 3, 4])])).unwrap();
    assert!(near(bx.tensor[0][0], 24.0 * (9.0 + 16.0) / 12.0));
    assert!(near(bx.tensor[1][1], 24.0 * (4.0 + 16.0) / 12.0));
    assert!(near(bx.tensor[2][2], 24.0 * (4.0 + 9.0) / 12.0));
    // two cubes of 2 mm 4 mm apart along x: the products stay zero, and by the parallel axis
    // theorem Ixx = 2 * (8 * 8 / 12) and Iyy = Izz = 2 * (8 * 8 / 12 + 8 * 4)
    let two = inertia(&solid(&[b([0, 0, 0], [2, 2, 2]), b([4, 0, 0], [2, 2, 2])])).unwrap();
    assert!(near(two.tensor[0][0], 2.0 * 64.0 / 12.0));
    assert!(near(two.tensor[1][1], 2.0 * (64.0 / 12.0 + 8.0 * 4.0)));
    assert!(near(two.tensor[2][2], two.tensor[1][1]));
    assert!(near(two.tensor[0][1], 0.0));
    assert!(inertia(&Solid::empty()).is_none());
}

/// The tensor worked out cell by cell on the grid of the solid.
fn inertia_by_cells(s: &Solid) -> [[f64; 3]; 3] {
    let g = Grid::new(s, &Limits::default()).unwrap();
    let d = g.dims();
    let mm = |nm: i64| nm as f64 / 1e6;
    let mut cells: Vec<([f64; 3], [f64; 3])> = Vec::new();
    for k in 0..d[2] {
        for j in 0..d[1] {
            for i in 0..d[0] {
                if g.is_filled([i as isize, j as isize, k as isize]) {
                    let idx = [i, j, k];
                    let size = [0, 1, 2].map(|a| mm(g.width(a, idx[a])));
                    let lo = [0, 1, 2].map(|a| mm(g.planes(a)[idx[a]]));
                    let centre = [0, 1, 2].map(|a| lo[a] + size[a] / 2.0);
                    cells.push((size, centre));
                }
            }
        }
    }
    let volume: f64 = cells.iter().map(|(s, _)| s[0] * s[1] * s[2]).sum();
    let c = [0, 1, 2].map(|a| {
        cells
            .iter()
            .map(|(s, p)| s[0] * s[1] * s[2] * p[a])
            .sum::<f64>()
            / volume
    });
    let mut out = [[0.0; 3]; 3];
    for (size, p) in &cells {
        let v = size[0] * size[1] * size[2];
        let r = [0, 1, 2].map(|a| p[a] - c[a]);
        let r2 = r[0] * r[0] + r[1] * r[1] + r[2] * r[2];
        let own: f64 = size.iter().map(|x| x * x).sum::<f64>() / 12.0;
        for a in 0..3 {
            for bb in 0..3 {
                let own_ab = if a == bb {
                    own - size[a] * size[a] / 12.0
                } else {
                    0.0
                };
                let delta = if a == bb { r2 } else { 0.0 };
                out[a][bb] += v * (delta - r[a] * r[bb] + own_ab);
            }
        }
    }
    out
}

#[test]
fn a_seeded_family_of_inertias_agrees_with_the_cells_and_moves_with_the_solid() {
    let lim = Limits::default();
    let mut seed = Seed(0x1e47_1a00);
    for case in 0..40 {
        let s = seed.boxes();
        let i = inertia(&s).unwrap();
        let cells = inertia_by_cells(&s);
        for (r, row) in cells.iter().enumerate() {
            for (c, want) in row.iter().enumerate() {
                assert!(near(i.tensor[r][c], *want), "case {case} {r} {c}");
                assert!(
                    near(i.tensor[r][c], i.tensor[c][r]),
                    "case {case} symmetric"
                );
            }
        }
        // the tensor about the centre does not change when the solid is moved
        let by = [
            l(seed.below(9) - 4),
            l(seed.below(9) - 4),
            l(seed.below(9) - 4),
        ];
        let moved = inertia(&s.translate(by).unwrap()).unwrap();
        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    near(moved.tensor[r][c], i.tensor[r][c]),
                    "case {case} moved"
                );
            }
        }
        // a quarter turn about z swaps x and y: Ixx and Iyy trade places, Izz stays
        let turned = inertia(&s.rotated(2, 1, [l(0), l(0), l(0)], &lim).unwrap()).unwrap();
        assert!(
            near(turned.tensor[0][0], i.tensor[1][1]),
            "case {case} turn"
        );
        assert!(
            near(turned.tensor[1][1], i.tensor[0][0]),
            "case {case} turn"
        );
        assert!(
            near(turned.tensor[2][2], i.tensor[2][2]),
            "case {case} turn"
        );
        // a moment of inertia is never negative
        assert!(i.tensor[0][0] > 0.0 && i.tensor[1][1] > 0.0 && i.tensor[2][2] > 0.0);
    }
}
