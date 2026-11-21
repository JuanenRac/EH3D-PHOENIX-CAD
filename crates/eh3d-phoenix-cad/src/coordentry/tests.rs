use super::*;

const MM: i64 = 1_000_000;

fn at(text: &str, last: [i64; 2]) -> [i64; 2] {
    place(&parse(text).unwrap(), last).unwrap()
}

#[test]
fn the_four_ways_of_typing_a_point_give_the_points_worked_out_by_hand() {
    let last = [3 * MM, 4 * MM];
    assert_eq!(at("10,5", last), [10 * MM, 5 * MM]);
    assert_eq!(at("@10,5", last), [13 * MM, 9 * MM]);
    assert_eq!(at("  @ -1.5 , 2 ", last), [last[0] - 1_500_000, 6 * MM]);
    assert_eq!(at("10 mm, 1/4 in", [0, 0]), [10 * MM, 6_350_000]);
    // polar at the quarter turns is exact, from the origin and from the last point
    assert_eq!(at("20<0", last), [20 * MM, 0]);
    assert_eq!(at("20<90", last), [0, 20 * MM]);
    assert_eq!(at("@20<180", last), [3 * MM - 20 * MM, 4 * MM]);
    assert_eq!(at("@20<270", last), [3 * MM, 4 * MM - 20 * MM]);
    assert_eq!(at("@20<-90", last), [3 * MM, 4 * MM - 20 * MM]);
    assert_eq!(at("@20<450", last), [3 * MM, 24 * MM]);
    assert_eq!(at("@-20<90", last), [3 * MM, 4 * MM - 20 * MM]);
    // 30 and 45 degrees: 10 mm * cos 30 = 8.660254 mm, and the rounding is to the nanometre
    assert_eq!(at("10<30", [0, 0]), [8_660_254, 5_000_000]);
    assert_eq!(at("10<45 deg", [0, 0]), [7_071_068, 7_071_068]);
    assert_eq!(at("@10 mm<120", [0, 0]), [-5_000_000, 8_660_254]);
    // a semicolon allows a comma as the decimal mark
    assert_eq!(at("0,5; 2,5", [0, 0]), [500_000, 2_500_000]);
}

#[test]
fn what_is_read_says_how_it_was_typed() {
    let p = parse("@5<30").unwrap();
    assert_eq!((p.base, p.form), (Base::Last, Form::Polar));
    let c = parse("1,2").unwrap();
    assert_eq!(
        (c.base, c.form, c.offset),
        (Base::Origin, Form::Cartesian, [MM, 2 * MM])
    );
}

#[test]
fn a_list_is_placed_point_by_point_and_a_bad_one_stops_everything() {
    let pts = resolve(
        &["0,0", "@10,0", "@10<90", "@-10,0", "@0,-10"],
        [7 * MM, 7 * MM],
    )
    .unwrap();
    assert_eq!(
        pts,
        vec![
            [0, 0],
            [10 * MM, 0],
            [10 * MM, 10 * MM],
            [0, 10 * MM],
            [0, 0]
        ]
    );
    // the first one is relative to the start
    assert_eq!(
        resolve(&["@1,1"], [MM, MM]).unwrap(),
        vec![[2 * MM, 2 * MM]]
    );
    assert!(resolve(&["1,1", "@1;;1"], [0, 0]).is_err());
    assert!(resolve(&[], [0, 0]).unwrap().is_empty());
}

#[test]
fn text_that_is_not_a_point_is_refused_with_the_code_of_a_bad_unit_text() {
    for text in [
        "", "@", "1", "1,2,3", "1,", ",1", "a,b", "10<", "<45", "10<x", "1 2", "1e3,1", "10 km,1",
        "@@1,1", "10<45<3",
    ] {
        let e = parse(text).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-008", "{text:?}");
    }
    // a point beyond the range of lengths is refused when it is placed, and nothing moves
    let far = parse("@1000000 mm,0").unwrap();
    let mut last = [0i64; 2];
    for _ in 0..3 {
        match place(&far, last) {
            Ok(p) => last = p,
            Err(e) => assert_eq!(e.code(), "PHOENIX-CAD-001"),
        }
    }
    assert_eq!(
        parse("2000000000 m,0").unwrap_err().code(),
        "PHOENIX-CAD-001"
    );
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
}

#[test]
fn polar_points_come_back_to_their_distance_and_angle_within_a_nanometre() {
    let mut seed = Seed(0x00c0_004d);
    for case in 0..500 {
        let r = 1 + (seed.next() % 900_000_000) as i64; // up to 900 mm
        let deg = (seed.next() % 360_000) as i64; // thousandths of a degree
        let text = format!("{r} nm<{}.{:03}", deg / 1000, deg % 1000);
        let p = at(&text, [0, 0]);
        let dist = ((p[0] as f64).powi(2) + (p[1] as f64).powi(2)).sqrt();
        assert!(
            (dist - r as f64).abs() <= 1.0,
            "case {case}: {text} -> {p:?}"
        );
        let angle = (p[1] as f64)
            .atan2(p[0] as f64)
            .to_degrees()
            .rem_euclid(360.0);
        let want = deg as f64 / 1000.0;
        // one nanometre at the distance r is at most this many degrees
        let slack = (1.5 / r as f64).to_degrees() + 1e-9;
        let diff = ((angle - want + 540.0).rem_euclid(360.0) - 180.0).abs();
        assert!(diff <= slack, "case {case}: {text} -> {p:?} {angle} {want}");
        // from another point the same offset is the same vector
        let q = at(&format!("@{text}"), [5 * MM, -3 * MM]);
        assert_eq!([q[0] - 5 * MM, q[1] + 3 * MM], p, "case {case}");
    }
}

#[test]
fn cartesian_points_round_trip_through_their_text_for_a_seeded_family() {
    let mut seed = Seed(0x000a_11e5);
    for case in 0..300 {
        let x = (seed.next() % 2_000_001) as i64 - 1_000_000;
        let y = (seed.next() % 2_000_001) as i64 - 1_000_000;
        let relative = seed.next().is_multiple_of(2);
        let last = [
            (seed.next() % 1000) as i64 * 1000,
            (seed.next() % 1000) as i64 * 1000,
        ];
        let text = format!("{}{x} um,{y} um", if relative { "@" } else { "" });
        let p = at(&text, last);
        let base = if relative { last } else { [0, 0] };
        assert_eq!(p, [base[0] + x * 1000, base[1] + y * 1000], "case {case}");
    }
}
