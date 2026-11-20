// SPDX-License-Identifier: GPL-3.0-or-later
//! The holes of a bolt circle for the person who is going to drill them: where each of `count`
//! equally spaced holes on a circle of a given diameter is, the angle of each from the first axis,
//! the angle between neighbours and the straight distance between neighbours (the chord).
//!
//! The angles are whole nano-degrees (the step is rounded half away from zero, one hole at a time,
//! so the holes never drift) and the places are rounded to the nanometre from the sine and the
//! cosine of [`crate::mesh::sin_cos`], which give the same bits on every computer. The first hole is
//! at the starting angle (0 is along the first axis) and the others follow counter-clockwise.

use std::f64::consts::PI;

use crate::lang::{fill, Lang, Msg};
use crate::mesh::sin_cos;
use crate::scad::mm_text;
use crate::units::{div_round_half_away, format_angle, Angle, FULL_TURN_NDEG};
use crate::{CadError, Length};

/// The most holes a bolt circle can have.
pub const MAX_HOLES: u32 = 10_000;

/// One hole of the circle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hole {
    /// The number of the hole, from 1.
    pub number: u32,
    /// The angle of the hole from the first axis, from 0 up to a full turn.
    pub angle: Angle,
    /// The place along the first axis.
    pub x: Length,
    /// The place along the second axis.
    pub y: Length,
}

/// A bolt circle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoltCircle {
    /// The diameter of the circle.
    pub diameter: Length,
    /// The centre of the circle.
    pub center: (Length, Length),
    /// The holes, in order.
    pub holes: Vec<Hole>,
    /// The angle between two neighbours (a full turn over the number of holes, rounded).
    pub step: Angle,
    /// The straight distance between two neighbours (zero for a single hole).
    pub chord: Length,
}

/// The holes of a circle: `count` of them from 1 to [`MAX_HOLES`] on a circle of `diameter` (more
/// than zero) about `center`, the first at the angle `start`. Refused with the code of a value out
/// of range for a number of holes outside its limits and with the code of a parameter that is not
/// positive for a diameter that is not.
pub fn bolt_circle(
    count: u32,
    diameter: Length,
    start: Angle,
    center: (Length, Length),
) -> Result<BoltCircle, CadError> {
    if count == 0 || count > MAX_HOLES {
        return Err(CadError::ValueOutOfRange("number of holes"));
    }
    if diameter.get() <= 0 {
        return Err(CadError::NotPositive("diameter"));
    }
    let radius = diameter.get() as f64 / 2.0;
    let n = i128::from(count);
    let mut holes = Vec::with_capacity(count as usize);
    for k in 0..n {
        let offset = div_round_half_away(k * i128::from(FULL_TURN_NDEG), n)?;
        let ndeg = (i128::from(start.get()) + offset).rem_euclid(i128::from(FULL_TURN_NDEG));
        let angle = Angle::new(ndeg as i64)?;
        let theta = ndeg as f64 / 1.0e9 * (PI / 180.0);
        let (sin, cos) = sin_cos(theta);
        let place = |origin: Length, along: f64| {
            Length::new(origin.get() + (radius * along).round() as i64)
        };
        holes.push(Hole {
            number: k as u32 + 1,
            angle,
            x: place(center.0, cos)?,
            y: place(center.1, sin)?,
        });
    }
    let step = Angle::new(div_round_half_away(i128::from(FULL_TURN_NDEG), n)? as i64)?;
    let chord = if count == 1 {
        Length::ZERO
    } else {
        let (half_sin, _) = sin_cos(PI / f64::from(count));
        Length::new((2.0 * radius * half_sin).round() as i64)?
    };
    Ok(BoltCircle {
        diameter,
        center,
        holes,
        step,
        chord,
    })
}

/// The holes as comma separated values, the same text in any language: the number, the angle in
/// degrees and the place in millimetres.
pub fn csv(circle: &BoltCircle) -> String {
    let mut out = String::from("hole,angle_deg,x_mm,y_mm\n");
    for h in &circle.holes {
        out.push_str(&format!(
            "{},{},{},{}\n",
            h.number,
            format_angle(h.angle),
            mm_text(h.x.get()),
            mm_text(h.y.get())
        ));
    }
    out
}

/// The circle as a line of text in a language, with the holes after it as in [`csv`].
pub fn text_in(circle: &BoltCircle, lang: Lang) -> String {
    let mut out = fill(
        Msg::BoltCircleHead.tr(lang),
        &[
            &circle.holes.len().to_string(),
            &mm_text(circle.diameter.get()),
            &format_angle(circle.step),
            &mm_text(circle.chord.get()),
        ],
    );
    out.push('\n');
    out.push_str(&csv(circle));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn deg(v: i64) -> Angle {
        Angle::from_deg(v).unwrap()
    }

    fn places(c: &BoltCircle) -> Vec<(i64, i64)> {
        c.holes.iter().map(|h| (h.x.get(), h.y.get())).collect()
    }

    #[test]
    fn four_holes_on_a_circle_of_twenty_millimetres_are_on_the_axes() {
        let c = bolt_circle(4, mm(20), deg(0), (mm(0), mm(0))).unwrap();
        assert_eq!(
            places(&c),
            vec![
                (10_000_000, 0),
                (0, 10_000_000),
                (-10_000_000, 0),
                (0, -10_000_000)
            ]
        );
        assert_eq!(c.step, deg(90));
        assert_eq!(c.chord.get(), 14_142_136);
        assert_eq!(
            c.holes.iter().map(|h| h.angle.get()).collect::<Vec<_>>(),
            vec![0, 90_000_000_000, 180_000_000_000, 270_000_000_000]
        );
    }

    #[test]
    fn six_holes_have_a_chord_as_long_as_the_radius_and_the_circle_can_be_moved_and_turned() {
        let c = bolt_circle(6, mm(40), deg(30), (mm(100), mm(-50))).unwrap();
        assert_eq!(c.chord.get(), 20_000_000);
        assert_eq!(c.step, deg(60));
        assert_eq!(c.holes[0].angle, deg(30));
        assert_eq!(c.holes[5].angle, deg(330));
        // the hole at 90 degrees is straight above the centre
        assert_eq!(places(&c)[1], (100_000_000, -30_000_000));
        // the first hole at 30 degrees: (20 cos 30, 20 sin 30)
        assert_eq!(
            places(&c)[0],
            (100_000_000 + 17_320_508, -50_000_000 + 10_000_000)
        );
    }

    #[test]
    fn the_start_may_be_negative_or_more_than_a_turn_and_the_angles_stay_inside_a_turn() {
        let a = bolt_circle(3, mm(10), deg(-90), (mm(0), mm(0))).unwrap();
        let b = bolt_circle(3, mm(10), deg(270), (mm(0), mm(0))).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.holes[0].angle, deg(270));
        assert_eq!(a.holes[1].angle, deg(30));
        assert_eq!(a.holes[2].angle, deg(150));
    }

    #[test]
    fn one_hole_has_no_chord_and_two_holes_are_a_diameter_apart() {
        let one = bolt_circle(1, mm(10), deg(0), (mm(3), mm(4))).unwrap();
        assert_eq!(places(&one), vec![(8_000_000, 4_000_000)]);
        assert_eq!(one.chord, Length::ZERO);
        assert_eq!(one.step, deg(360));
        let two = bolt_circle(2, mm(10), deg(0), (mm(0), mm(0))).unwrap();
        assert_eq!(two.chord, mm(10));
    }

    #[test]
    fn a_circle_with_no_hole_too_many_holes_or_no_diameter_is_refused() {
        let origin = (mm(0), mm(0));
        assert_eq!(
            bolt_circle(0, mm(10), deg(0), origin).unwrap_err().code(),
            "PHOENIX-CAD-007"
        );
        assert_eq!(
            bolt_circle(MAX_HOLES + 1, mm(10), deg(0), origin)
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-007"
        );
        assert_eq!(
            bolt_circle(3, mm(0), deg(0), origin).unwrap_err().code(),
            "PHOENIX-CAD-002"
        );
        assert_eq!(
            bolt_circle(3, Length::new(-5).unwrap(), deg(0), origin)
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-002"
        );
        assert!(bolt_circle(MAX_HOLES, mm(10), deg(0), origin).is_ok());
    }

    #[test]
    fn a_place_that_leaves_the_range_of_lengths_is_refused() {
        let far = Length::new(crate::LENGTH_LIMIT_NM - 1).unwrap();
        assert!(bolt_circle(3, mm(100), deg(0), (far, far)).is_err());
    }

    #[test]
    fn the_csv_and_the_text_list_every_hole() {
        let c = bolt_circle(4, mm(20), deg(45), (mm(0), mm(0))).unwrap();
        let out = csv(&c);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows[0], "hole,angle_deg,x_mm,y_mm");
        assert_eq!(rows[1], "1,45,7.071068,7.071068");
        assert_eq!(rows[2], "2,135,-7.071068,7.071068");
        assert_eq!(rows.len(), 5);
        for lang in Lang::ALL {
            let text = text_in(&c, lang);
            assert!(!text.contains('{'), "{}", lang.code());
            assert!(text.contains("14.142136"));
            assert!(text.ends_with(&out));
        }
        assert!(text_in(&c, Lang::En).starts_with(
            "Bolt circle of 4 holes on a diameter of 20 mm, 90 degrees apart, 14.142136 mm between neighbours\n"
        ));
    }

    #[test]
    fn random_circles_have_every_hole_on_the_circle_and_neighbours_a_chord_apart_with_a_fixed_seed()
    {
        let mut seed = 0xb017_cad4u64;
        let mut next = move |m: u64| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) % m
        };
        for _ in 0..80 {
            let n = 1 + next(60) as u32;
            let d = Length::new(1_000_000 + next(400_000_000) as i64).unwrap();
            let center = (
                Length::new(next(2_000_000_000) as i64 - 1_000_000_000).unwrap(),
                Length::new(next(2_000_000_000) as i64 - 1_000_000_000).unwrap(),
            );
            let start = Angle::new(next(720_000_000_000) as i64 - 360_000_000_000).unwrap();
            let c = bolt_circle(n, d, start, center).unwrap();
            assert_eq!(c.holes.len(), n as usize);
            let r = d.get() as f64 / 2.0;
            for h in &c.holes {
                let dx = (h.x.get() - center.0.get()) as f64;
                let dy = (h.y.get() - center.1.get()) as f64;
                assert!((dx.hypot(dy) - r).abs() <= 1.5, "{n} {d:?}");
                assert!(h.angle.get() >= 0 && h.angle.get() < FULL_TURN_NDEG);
            }
            if n > 1 {
                for w in c
                    .holes
                    .windows(2)
                    .chain(std::iter::once(&[*c.holes.last().unwrap(), c.holes[0]][..]))
                {
                    let dx = (w[1].x.get() - w[0].x.get()) as f64;
                    let dy = (w[1].y.get() - w[0].y.get()) as f64;
                    assert!((dx.hypot(dy) - c.chord.get() as f64).abs() <= 3.0, "{n}");
                }
            }
        }
    }
}
