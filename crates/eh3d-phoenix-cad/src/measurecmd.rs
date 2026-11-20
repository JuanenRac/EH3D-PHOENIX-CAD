// SPDX-License-Identifier: GPL-3.0-or-later
//! Measures between points typed on the command line, with the same measurements the window makes
//! on the model ([`crate::measure`]): the distance between two points, from a point to an edge and
//! between two edges, and the angle at a vertex between three points and between two edges.
//!
//! A point is written `X:Y:Z`, each coordinate a length with its own unit (`10 mm`, `1 1/2 in`) or
//! a bare number of millimetres, so `10 mm:0:2.5 mm` is the point 10, 0 and 2.5 millimetres (a unit
//! does not carry over to the coordinates before it). An edge is written as its two ends. The
//! numbers are a reading for the person, in millimetres and degrees.

use crate::lang::{fill, Lang, Msg};
use crate::measure::{angle, distance, Label, Measurement, Reference};
use crate::projection::Point3;
use crate::units::parse_length;
use crate::{CadError, Length};

const NM_PER_MM: f64 = 1.0e6;

/// A length with its unit, or a bare number that is millimetres.
pub fn length_or_mm(text: &str) -> Result<Length, CadError> {
    let t = text.trim();
    if t.contains(|c: char| c.is_alphabetic()) {
        parse_length(t)
    } else {
        parse_length(&format!("{t} mm"))
    }
}

/// A coordinate in millimetres.
fn coordinate(text: &str) -> Result<f64, CadError> {
    Ok(length_or_mm(text)?.get() as f64 / NM_PER_MM)
}

/// A point written `X:Y:Z`, in millimetres.
pub fn parse_point(text: &str) -> Result<Point3, CadError> {
    let parts: Vec<&str> = text.split(':').collect();
    let [x, y, z] = parts.as_slice() else {
        return Err(CadError::Measure(format!(
            "{text}: a point is written X:Y:Z"
        )));
    };
    Ok([coordinate(x)?, coordinate(y)?, coordinate(z)?])
}

fn point(text: &str) -> Result<Reference, CadError> {
    Ok(Reference::point(Label::OuterCorner, parse_point(text)?))
}

fn edge(a: &str, b: &str) -> Result<Reference, CadError> {
    Reference::edge(Label::OuterEdge, parse_point(a)?, parse_point(b)?)
}

/// The distance between what the points name: two points (a distance), a point and the two ends of
/// an edge, or the two ends of an edge and the two ends of another.
pub fn measure_distance(points: &[String]) -> Result<Measurement, CadError> {
    match points {
        [a, b] => Ok(distance(&point(a)?, &point(b)?)),
        [p, e0, e1] => Ok(distance(&point(p)?, &edge(e0, e1)?)),
        [a0, a1, b0, b1] => Ok(distance(&edge(a0, a1)?, &edge(b0, b1)?)),
        _ => Err(CadError::Measure(
            "a distance needs two points, a point and an edge (three points) or two edges (four points)"
                .into(),
        )),
    }
}

/// The angle that the points name: three points (the second is the vertex) or the two ends of an
/// edge and the two ends of another.
pub fn measure_angle(points: &[String]) -> Result<Measurement, CadError> {
    match points {
        [a, v, b] => angle(&[point(a)?, point(v)?, point(b)?]),
        [a0, a1, b0, b1] => angle(&[edge(a0, a1)?, edge(b0, b1)?]),
        _ => Err(CadError::Measure(
            "an angle needs three points (the vertex second) or two edges (four points)".into(),
        )),
    }
}

fn num(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    if text.starts_with('-') && text.trim_start_matches(['-', '0', '.']).is_empty() {
        text[1..].to_owned()
    } else {
        text
    }
}

/// The reading as a line of text in a language.
pub fn text_in(m: &Measurement, lang: Lang) -> String {
    match m.quantity {
        crate::measure::Quantity::Distance => fill(
            Msg::MeasureCliDistance.tr(lang),
            &[
                &num(m.value, 4),
                &num(m.delta[0], 4),
                &num(m.delta[1], 4),
                &num(m.delta[2], 4),
            ],
        ),
        crate::measure::Quantity::Angle => fill(Msg::MeasureCliAngle.tr(lang), &[&num(m.value, 3)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(list: &[&str]) -> Vec<String> {
        list.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn a_point_has_three_coordinates_with_a_unit_or_bare_millimetres() {
        assert_eq!(parse_point("10:0:5").unwrap(), [10.0, 0.0, 5.0]);
        assert_eq!(
            parse_point("1 in:0 mm:-0.5 in").unwrap(),
            [25.4, 0.0, -12.7]
        );
        assert_eq!(parse_point("1 1/2 in:0,5:2 cm").unwrap(), [38.1, 0.5, 20.0]);
        for bad in ["", "1:2", "1:2:3:4", "a:b:c", "1:2:3 ft", "::"] {
            let e = parse_point(bad).unwrap_err();
            assert!(
                ["PHOENIX-CAD-008", "PHOENIX-CAD-038"].contains(&e.code()),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn the_distance_between_two_points_is_the_length_of_the_line_and_its_steps() {
        let m = measure_distance(&s(&["0:0:0", "3:4:12"])).unwrap();
        assert!((m.value - 13.0).abs() < 1e-9);
        assert_eq!(m.delta, [3.0, 4.0, 12.0]);
        let text = text_in(&m, Lang::En);
        assert_eq!(
            text,
            "Distance 13.0000 mm (x 3.0000, y 4.0000, z 12.0000 mm)"
        );
    }

    #[test]
    fn the_distance_from_a_point_to_an_edge_is_to_its_nearest_point() {
        // the edge runs along x from 0 to 10; the point is above its middle
        let m = measure_distance(&s(&["5:7:0", "0:0:0", "10:0:0"])).unwrap();
        assert!((m.value - 7.0).abs() < 1e-9);
        // past the end of the edge the nearest point is the end
        let m = measure_distance(&s(&["13:4:0", "0:0:0", "10:0:0"])).unwrap();
        assert!((m.value - 5.0).abs() < 1e-9);
    }

    #[test]
    fn the_distance_between_two_edges_is_between_their_nearest_points() {
        // two lines that cross over each other 3 mm apart in z
        let m = measure_distance(&s(&["0:0:0", "10:0:0", "5:-5:3", "5:5:3"])).unwrap();
        assert!((m.value - 3.0).abs() < 1e-9);
    }

    #[test]
    fn an_angle_at_a_vertex_and_between_edges() {
        let m = measure_angle(&s(&["10:0:0", "0:0:0", "0:10:0"])).unwrap();
        assert!((m.value - 90.0).abs() < 1e-9);
        let m = measure_angle(&s(&["1:0:0", "0:0:0", "1:1:0"])).unwrap();
        assert!((m.value - 45.0).abs() < 1e-9);
        assert_eq!(text_in(&m, Lang::En), "Angle 45.000 degrees");
        // two edges: the acute angle between the lines they lie on
        let m = measure_angle(&s(&["0:0:0", "10:0:0", "0:0:0", "-10:10:0"])).unwrap();
        assert!((m.value - 45.0).abs() < 1e-9);
    }

    #[test]
    fn the_wrong_number_of_points_a_point_that_is_not_a_point_and_an_edge_without_length_are_refused(
    ) {
        assert_eq!(
            measure_distance(&s(&["0:0:0"])).unwrap_err().code(),
            "PHOENIX-CAD-038"
        );
        assert_eq!(
            measure_distance(&s(&[])).unwrap_err().code(),
            "PHOENIX-CAD-038"
        );
        assert_eq!(
            measure_distance(&s(&["0:0:0", "1:1:1", "2:2:2", "3:3:3", "4:4:4"]))
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-038"
        );
        assert_eq!(
            measure_angle(&s(&["0:0:0", "1:0:0"])).unwrap_err().code(),
            "PHOENIX-CAD-038"
        );
        assert_eq!(
            measure_angle(&s(&["0:0:0", "x", "0:1:0"]))
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-038"
        );
        // an edge whose two ends are one point
        assert_eq!(
            measure_distance(&s(&["0:0:0", "1:1:1", "1:1:1"]))
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-038"
        );
        // an angle at a vertex that is one of the points
        assert_eq!(
            measure_angle(&s(&["0:0:0", "0:0:0", "0:1:0"]))
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-038"
        );
    }

    #[test]
    fn the_text_is_the_same_reading_in_every_language() {
        let d = measure_distance(&s(&["0:0:0", "3:4:0"])).unwrap();
        let a = measure_angle(&s(&["1:0:0", "0:0:0", "0:1:0"])).unwrap();
        for lang in Lang::ALL {
            let td = text_in(&d, lang);
            let ta = text_in(&a, lang);
            assert!(
                td.contains("5.0000") && td.contains("3.0000") && td.contains("4.0000"),
                "{td}"
            );
            assert!(ta.contains("90.000"), "{ta}");
            assert!(!td.contains('{') && !ta.contains('{'));
        }
        assert_ne!(text_in(&d, Lang::En), text_in(&d, Lang::Es));
    }

    #[test]
    fn a_value_that_rounds_to_zero_has_no_minus_sign() {
        assert_eq!(num(-0.00001, 4), "0.0000");
        assert_eq!(num(-3.5, 3), "-3.500");
    }
}
