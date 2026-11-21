use super::*;

fn l(mm: i64) -> Length {
    Length::from_mm(mm).unwrap()
}

fn nm(v: i64) -> Length {
    Length::new(v).unwrap()
}

fn b(c: [i64; 3], s: [i64; 3]) -> Prism {
    Prism::new([l(c[0]), l(c[1]), l(c[2])], [l(s[0]), l(s[1]), l(s[2])]).unwrap()
}

fn solid(boxes: &[Prism]) -> Solid {
    Solid::from_prisms(boxes, &Limits::default()).unwrap()
}

/// One cubic millimetre in cubic nanometres.
const MM3: i128 = 1_000_000_000_000_000_000;

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

    fn solid(&mut self) -> (Vec<Prism>, Solid) {
        let count = 1 + self.below(4) as usize;
        let boxes: Vec<Prism> = (0..count)
            .map(|_| {
                b(
                    [self.below(8), self.below(8), self.below(8)],
                    [1 + self.below(5), 1 + self.below(5), 1 + self.below(5)],
                )
            })
            .collect();
        let s = solid(&boxes);
        (boxes, s)
    }
}

#[test]
fn a_prism_needs_a_positive_size_and_stays_in_the_range_of_lengths() {
    let e = Prism::new([l(0), l(0), l(0)], [l(1), Length::ZERO, l(1)]).unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-002");
    let e = Prism::new([l(0), l(0), l(0)], [l(1), l(-1), l(1)]).unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-002");
    let top = Length::new(crate::LENGTH_LIMIT_NM - 1).unwrap();
    let e = Prism::new([top, l(0), l(0)], [l(1), l(1), l(1)]).unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-009");
    let p = b([1, 2, 3], [4, 5, 6]);
    assert_eq!(p.size(), [4_000_000, 5_000_000, 6_000_000]);
    assert_eq!(p.volume_nm3(), 120 * MM3);
    assert!(p.contains([1_000_000, 2_000_000, 3_000_000]));
    assert!(!p.contains([5_000_000, 2_000_000, 3_000_000]));
}

#[test]
fn two_overlapping_boxes_make_a_union_whose_volume_is_exact() {
    let a = solid(&[b([0, 0, 0], [10, 10, 10])]);
    let c = solid(&[b([5, 5, 5], [10, 10, 10])]);
    let u = a.union(&c, &Limits::default()).unwrap();
    assert_eq!(u.volume_nm3(), (1000 + 1000 - 125) * MM3);
    let i = a.intersection(&c, &Limits::default()).unwrap();
    assert_eq!(i, solid(&[b([5, 5, 5], [5, 5, 5])]));
    let d = a.difference(&c, &Limits::default()).unwrap();
    assert_eq!(d.volume_nm3(), (1000 - 125) * MM3);
    assert!(d.contains([1_000_000, 1_000_000, 1_000_000]));
    assert!(!d.contains([6_000_000, 6_000_000, 6_000_000]));
}

#[test]
fn boxes_that_touch_are_one_box_and_the_form_does_not_depend_on_the_way_it_was_made() {
    let whole = solid(&[b([0, 0, 0], [20, 10, 10])]);
    let halves = solid(&[b([0, 0, 0], [10, 10, 10]), b([10, 0, 0], [10, 10, 10])]);
    assert_eq!(whole, halves);
    assert_eq!(halves.prisms().len(), 1);
    let sliced = solid(&[b([0, 0, 0], [20, 10, 3]), b([0, 0, 3], [20, 10, 7])]);
    assert_eq!(whole, sliced);
    // a solid cut out of a bigger one is the same as the box it leaves
    let big = solid(&[b([0, 0, 0], [30, 10, 10])]);
    let cut = big
        .difference(&solid(&[b([20, 0, 0], [10, 10, 10])]), &Limits::default())
        .unwrap();
    assert_eq!(cut, whole);
}

#[test]
fn a_hole_through_a_plate_leaves_the_plate_in_boxes_that_add_up() {
    let plate = solid(&[b([0, 0, 0], [40, 30, 5])]);
    let hole = solid(&[b([10, 10, 0], [10, 10, 5])]);
    let drilled = plate.difference(&hole, &Limits::default()).unwrap();
    assert_eq!(drilled.volume_nm3(), (6000 - 500) * MM3);
    assert!(!drilled.contains([15_000_000, 15_000_000, 1_000_000]));
    assert!(drilled.contains([5_000_000, 15_000_000, 1_000_000]));
    // the hole put back gives the plate
    assert_eq!(drilled.union(&hole, &Limits::default()).unwrap(), plate);
    // nothing of the plate in the hole
    assert!(drilled
        .intersection(&hole, &Limits::default())
        .unwrap()
        .is_empty());
}

#[test]
fn an_empty_solid_and_the_difference_of_a_solid_with_itself() {
    let e = Solid::empty();
    let a = solid(&[b([0, 0, 0], [3, 3, 3])]);
    let lim = Limits::default();
    assert_eq!(a.union(&e, &lim).unwrap(), a);
    assert_eq!(e.union(&a, &lim).unwrap(), a);
    assert!(a.difference(&a, &lim).unwrap().is_empty());
    assert!(a.intersection(&e, &lim).unwrap().is_empty());
    assert_eq!(a.difference(&e, &lim).unwrap(), a);
    assert!(e.is_empty() && e.bounds().is_none() && e.volume_nm3() == 0);
    assert_eq!(
        a.bounds(),
        Some(([0, 0, 0], [3_000_000, 3_000_000, 3_000_000]))
    );
}

#[test]
fn moving_a_solid_moves_every_box_and_leaves_the_range_alone() {
    let a = solid(&[b([0, 0, 0], [3, 3, 3]), b([5, 0, 0], [2, 2, 2])]);
    let moved = a.translate([l(10), l(-1), nm(7)]).unwrap();
    assert_eq!(moved.volume_nm3(), a.volume_nm3());
    assert_eq!(moved.bounds().unwrap().0, [10_000_000, -1_000_000, 7]);
    let back = moved.translate([l(-10), l(1), nm(-7)]).unwrap();
    assert_eq!(back, a);
    let top = Length::new(crate::LENGTH_LIMIT_NM - 1).unwrap();
    assert!(a.translate([top, l(0), l(0)]).is_err());
}

#[test]
fn whole_nanometres_are_kept_exactly() {
    let a = Solid::from_prism(Prism::new([nm(1), nm(2), nm(3)], [nm(5), nm(7), nm(11)]).unwrap());
    let c = Solid::from_prism(Prism::new([nm(3), nm(4), nm(5)], [nm(5), nm(7), nm(11)]).unwrap());
    let i = a.intersection(&c, &Limits::default()).unwrap();
    assert_eq!(i.volume_nm3(), 3 * 5 * 9);
    let u = a.union(&c, &Limits::default()).unwrap();
    assert_eq!(u.volume_nm3(), 385 + 385 - 135);
}

#[test]
fn an_operation_beyond_the_limits_is_refused_and_builds_nothing() {
    let a = solid(&[b([0, 0, 0], [10, 10, 10]), b([20, 20, 20], [10, 10, 10])]);
    let c = solid(&[b([5, 5, 5], [20, 20, 20])]);
    let few = Limits {
        max_cells: 10,
        max_prisms: 100,
    };
    let e = a.union(&c, &few).unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-053");
    let one = Limits {
        max_cells: 1_000,
        max_prisms: 1,
    };
    let e = a.union(&c, &one).unwrap_err();
    assert_eq!(e.code(), "PHOENIX-CAD-053");
}

#[test]
fn the_booleans_agree_with_the_answers_for_every_point_for_random_solids_with_a_fixed_seed() {
    let mut seed = Seed(0x5eed_00c1);
    let lim = Limits::default();
    for round in 0..60 {
        let ((_, a), (_, c)) = (seed.solid(), seed.solid());
        let u = a.union(&c, &lim).unwrap();
        let d = a.difference(&c, &lim).unwrap();
        let i = a.intersection(&c, &lim).unwrap();
        for x in (0..14).map(|v| v * 1_000_000 + 500_000) {
            for y in (0..14).map(|v| v * 1_000_000 + 500_000) {
                for z in (0..14).map(|v| v * 1_000_000 + 500_000) {
                    let p = [x, y, z];
                    let (ia, ic) = (a.contains(p), c.contains(p));
                    assert_eq!(u.contains(p), ia || ic, "round {round} union {p:?}");
                    assert_eq!(d.contains(p), ia && !ic, "round {round} difference {p:?}");
                    assert_eq!(i.contains(p), ia && ic, "round {round} intersection {p:?}");
                }
            }
        }
        // the volumes add up
        assert_eq!(
            u.volume_nm3() + i.volume_nm3(),
            a.volume_nm3() + c.volume_nm3(),
            "round {round}"
        );
        assert_eq!(
            d.volume_nm3(),
            a.volume_nm3() - i.volume_nm3(),
            "round {round}"
        );
        // the boxes of a solid never overlap
        let boxes = u.prisms();
        for (k, p) in boxes.iter().enumerate() {
            for q in &boxes[k + 1..] {
                let overlap =
                    (0..3).all(|ax| p.min()[ax] < q.max()[ax] && q.min()[ax] < p.max()[ax]);
                assert!(!overlap, "round {round}: {p:?} {q:?}");
            }
        }
    }
}

#[test]
fn the_form_is_the_same_whatever_the_order_of_the_boxes_and_of_the_operations() {
    let mut seed = Seed(0x5eed_00c2);
    let lim = Limits::default();
    for round in 0..60 {
        let ((boxes_a, a), (_, c)) = (seed.solid(), seed.solid());
        // the boxes in another order make the same solid
        let mut reversed = boxes_a.clone();
        reversed.reverse();
        assert_eq!(solid(&reversed), a, "round {round}");
        // a union does not depend on the side
        assert_eq!(
            a.union(&c, &lim).unwrap(),
            c.union(&a, &lim).unwrap(),
            "round {round}"
        );
        assert_eq!(
            a.intersection(&c, &lim).unwrap(),
            c.intersection(&a, &lim).unwrap(),
            "round {round}"
        );
        // what is cut and what is left give the solid back
        let left = a.difference(&c, &lim).unwrap();
        let cut = a.intersection(&c, &lim).unwrap();
        assert_eq!(left.union(&cut, &lim).unwrap(), a, "round {round}");
        // the operations are idempotent
        assert_eq!(a.union(&a, &lim).unwrap(), a, "round {round}");
        assert_eq!(a.intersection(&a, &lim).unwrap(), a, "round {round}");
        // moving and moving back
        let by = [l(seed.below(5)), l(seed.below(5)), l(seed.below(5))];
        let back = [
            Length::new(-by[0].get()).unwrap(),
            Length::new(-by[1].get()).unwrap(),
            Length::new(-by[2].get()).unwrap(),
        ];
        assert_eq!(
            a.translate(by).unwrap().translate(back).unwrap(),
            a,
            "round {round}"
        );
    }
}

#[test]
fn a_cube_grown_shrunk_and_hollowed_is_worked_by_hand() {
    let lim = Limits::default();
    let cube = solid(&[b([0, 0, 0], [10, 10, 10])]);
    // grown by 2 mm on every side: 14 mm across, from -2 to 12
    let g = cube.grown(l(2), &lim).unwrap();
    assert_eq!(g, solid(&[b([-2, -2, -2], [14, 14, 14])]));
    assert_eq!(g.volume_nm3(), 14 * 14 * 14 * MM3);
    // an L is grown into an L whose corner is still square
    let ell = solid(&[b([0, 0, 0], [4, 1, 1]), b([0, 0, 1], [1, 1, 3])]);
    let ge = ell.grown(l(1), &lim).unwrap();
    assert_eq!(ge.bounds().unwrap().0, [-1_000_000, -1_000_000, -1_000_000]);
    assert!(ge.contains([-1_000_000, 0, 3_999_999]));
    assert!(!ge.contains([2_000_000, 0, 3_000_000]));
    // shrunk by 2 mm: the cube of 6 mm in the middle; by 4 mm 2 mm; by 5 mm nothing is left
    assert_eq!(
        cube.shrunk(l(2), &lim).unwrap(),
        solid(&[b([2, 2, 2], [6, 6, 6])])
    );
    assert_eq!(
        cube.shrunk(l(4), &lim).unwrap(),
        solid(&[b([4, 4, 4], [2, 2, 2])])
    );
    assert!(cube.shrunk(l(5), &lim).unwrap().is_empty());
    // a cube with a cavity, shrunk, loses the wall and the cavity grows into the rest
    let hollow = cube
        .difference(&solid(&[b([3, 3, 3], [4, 4, 4])]), &lim)
        .unwrap();
    let sh = hollow.shrunk(l(1), &lim).unwrap();
    // the outer wall is 3 mm thick and the cavity 4 mm, so 1 mm off each side leaves a shell of
    // [1, 9) less [2, 8)
    assert_eq!(
        sh,
        solid(&[b([1, 1, 1], [8, 8, 8])])
            .difference(&solid(&[b([2, 2, 2], [6, 6, 6])]), &lim)
            .unwrap()
    );
    // a shell of 2 mm keeps the outside and takes the cube of 6 mm out of the middle
    let shell = cube.shelled(l(2), &lim).unwrap();
    assert_eq!(shell.volume_nm3(), (1000 - 216) * MM3);
    assert!(!shell.contains([5_000_000, 5_000_000, 5_000_000]));
    assert!(shell.contains([1_000_000, 5_000_000, 5_000_000]));
    assert_eq!(shell.bounds(), cube.bounds());
    // too thin to hollow: the shell is the whole solid
    let slab = solid(&[b([0, 0, 0], [3, 10, 10])]);
    assert_eq!(slab.shelled(l(2), &lim).unwrap(), slab);
    // zero changes nothing, an empty solid stays empty
    assert_eq!(cube.grown(Length::ZERO, &lim).unwrap(), cube);
    assert_eq!(cube.shrunk(Length::ZERO, &lim).unwrap(), cube);
    assert!(Solid::empty().shrunk(l(1), &lim).unwrap().is_empty());
}

#[test]
fn growing_and_shrinking_refuse_a_negative_distance_and_a_coordinate_out_of_range() {
    let lim = Limits::default();
    let cube = solid(&[b([0, 0, 0], [10, 10, 10])]);
    for e in [
        cube.grown(l(-1), &lim).unwrap_err(),
        cube.shrunk(l(-1), &lim).unwrap_err(),
        cube.shelled(l(-1), &lim).unwrap_err(),
    ] {
        assert_eq!(e.code(), "PHOENIX-CAD-002");
    }
    let huge = Length::new(crate::LENGTH_LIMIT_NM - 1).unwrap();
    assert_eq!(
        cube.grown(huge, &lim).unwrap_err().code(),
        "PHOENIX-CAD-001"
    );
    assert_eq!(
        cube.shrunk(huge, &lim).unwrap_err().code(),
        "PHOENIX-CAD-001"
    );
    // too many cells in the work is refused like every other operation
    let tight = Limits {
        max_cells: 1,
        max_prisms: 100_000,
    };
    let two = solid(&[b([0, 0, 0], [2, 2, 2]), b([5, 5, 5], [2, 2, 2])]);
    assert_eq!(
        two.shrunk(l(1), &tight).unwrap_err().code(),
        "PHOENIX-CAD-053"
    );
}

#[test]
fn a_seeded_family_obeys_the_laws_of_growing_and_shrinking() {
    let lim = Limits::default();
    let mut seed = Seed(0x9120_5e11);
    for round in 0..60 {
        let (_, s) = seed.solid();
        let a = 1 + seed.below(2);
        let c = 1 + seed.below(2);
        let (da, dc) = (l(a), l(c));
        let grown = s.grown(da, &lim).unwrap();
        let shrunk = s.shrunk(da, &lim).unwrap();
        // the grown solid holds the solid, which holds the shrunk one
        assert!(
            s.difference(&grown, &lim).unwrap().is_empty(),
            "round {round}"
        );
        assert!(
            shrunk.difference(&s, &lim).unwrap().is_empty(),
            "round {round}"
        );
        // growing twice is growing by the sum, and so is shrinking
        assert_eq!(
            grown.grown(dc, &lim).unwrap(),
            s.grown(l(a + c), &lim).unwrap(),
            "round {round}"
        );
        assert_eq!(
            shrunk.shrunk(dc, &lim).unwrap(),
            s.shrunk(l(a + c), &lim).unwrap(),
            "round {round}"
        );
        // closing never loses the solid, opening never gains
        let closed = grown.shrunk(da, &lim).unwrap();
        assert!(
            s.difference(&closed, &lim).unwrap().is_empty(),
            "round {round}"
        );
        let opened = shrunk.grown(da, &lim).unwrap();
        assert!(
            opened.difference(&s, &lim).unwrap().is_empty(),
            "round {round}"
        );
        // the shell and the shrunk solid are the solid, with nothing in common
        let shell = s.shelled(da, &lim).unwrap();
        assert_eq!(shell.union(&shrunk, &lim).unwrap(), s, "round {round}");
        assert!(shell.intersection(&shrunk, &lim).unwrap().is_empty());
        assert_eq!(
            shell.volume_nm3() + shrunk.volume_nm3(),
            s.volume_nm3(),
            "round {round}"
        );
    }
}
