// SPDX-License-Identifier: GPL-3.0-or-later
//! What a sketch holds, at a glance: how many points, lines, circles and arcs (and how many of them
//! are construction geometry), how many constraints (how many of them dimensions and how many
//! switched off), the size of what is drawn and its drawn length.
//!
//! Only the elements that are part of the profile are measured: construction geometry guides the
//! drawing and is not drawn. The size is the box that holds the lines, the circles and the arcs
//! (an arc counts its two ends and the extreme points of its circle that it passes through), in
//! millimetres; the drawn length is the sum of the lines, the circumferences of the circles and
//! the lengths of the arcs. The numbers are a reading for the person, in floating point.

use std::f64::consts::PI;

use crate::lang::{fill, Lang, Msg};
use crate::sketch::{Element, Sketch};

const NM_PER_MM: f64 = 1.0e6;

/// The counts and the size of a sketch.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    /// The name of the sketch.
    pub name: String,
    /// How many points.
    pub points: usize,
    /// How many lines.
    pub lines: usize,
    /// How many circles.
    pub circles: usize,
    /// How many arcs.
    pub arcs: usize,
    /// How many of the elements are construction geometry.
    pub construction: usize,
    /// How many constraints.
    pub constraints: usize,
    /// How many of them are dimensions.
    pub dimensions: usize,
    /// How many of them are switched off.
    pub disabled: usize,
    /// The box that holds the drawn geometry, `[x0, y0, x1, y1]` in millimetres, when there is any.
    pub bounds_mm: Option<[f64; 4]>,
    /// The sum of the lengths of the drawn lines, circles and arcs in millimetres.
    pub length_mm: f64,
}

fn grow(bounds: &mut Option<[f64; 4]>, x: f64, y: f64) {
    match bounds {
        Some(b) => {
            b[0] = b[0].min(x);
            b[1] = b[1].min(y);
            b[2] = b[2].max(x);
            b[3] = b[3].max(y);
        }
        None => *bounds = Some([x, y, x, y]),
    }
}

/// Counts what a sketch holds and measures what is drawn.
pub fn info(sketch: &Sketch) -> Info {
    let at = |id| {
        sketch
            .point(id)
            .map(|(x, y)| (x.get() as f64 / NM_PER_MM, y.get() as f64 / NM_PER_MM))
            .unwrap_or((0.0, 0.0))
    };
    let mut out = Info {
        name: sketch.name().to_owned(),
        points: 0,
        lines: 0,
        circles: 0,
        arcs: 0,
        construction: 0,
        constraints: sketch.constraints().len(),
        dimensions: sketch
            .constraints()
            .iter()
            .filter(|c| c.constraint.is_dimension())
            .count(),
        disabled: sketch.constraints().iter().filter(|c| !c.enabled).count(),
        bounds_mm: None,
        length_mm: 0.0,
    };
    for entry in sketch.elements() {
        match entry.element {
            Element::Point { .. } => out.points += 1,
            Element::Line { .. } => out.lines += 1,
            Element::Circle { .. } => out.circles += 1,
            Element::Arc { .. } => out.arcs += 1,
        }
        if entry.construction {
            out.construction += 1;
            continue;
        }
        match entry.element {
            Element::Point { .. } => {}
            Element::Line { a, b } => {
                let (pa, pb) = (at(a), at(b));
                grow(&mut out.bounds_mm, pa.0, pa.1);
                grow(&mut out.bounds_mm, pb.0, pb.1);
                out.length_mm += (pb.0 - pa.0).hypot(pb.1 - pa.1);
            }
            Element::Circle { center, radius } => {
                let (c, r) = (at(center), radius.get() as f64 / NM_PER_MM);
                grow(&mut out.bounds_mm, c.0 - r, c.1 - r);
                grow(&mut out.bounds_mm, c.0 + r, c.1 + r);
                out.length_mm += 2.0 * PI * r;
            }
            Element::Arc { center, start, end } => {
                let (c, s, e) = (at(center), at(start), at(end));
                let r = (s.0 - c.0).hypot(s.1 - c.1);
                let a0 = (s.1 - c.1).atan2(s.0 - c.0);
                let a1 = (e.1 - c.1).atan2(e.0 - c.0);
                let mut sweep = (a1 - a0).rem_euclid(2.0 * PI);
                if sweep < 1e-12 {
                    sweep = 2.0 * PI;
                }
                grow(&mut out.bounds_mm, s.0, s.1);
                grow(&mut out.bounds_mm, e.0, e.1);
                for quarter in 0..4 {
                    let direction = f64::from(quarter) * PI / 2.0;
                    if (direction - a0).rem_euclid(2.0 * PI) <= sweep {
                        let (sin, cos) = direction.sin_cos();
                        grow(&mut out.bounds_mm, c.0 + r * cos, c.1 + r * sin);
                    }
                }
                out.length_mm += r * sweep;
            }
        }
    }
    out
}

fn num(value: f64) -> String {
    let text = format!("{value:.3}");
    if text.starts_with('-') && text.trim_start_matches(['-', '0', '.']).is_empty() {
        text[1..].to_owned()
    } else {
        text
    }
}

/// The counts and the size as lines of text in a language.
pub fn text_in(info: &Info, lang: Lang) -> String {
    let mut out = fill(
        Msg::SketchInfoHead.tr(lang),
        &[
            &info.name,
            &info.points.to_string(),
            &info.lines.to_string(),
            &info.circles.to_string(),
            &info.arcs.to_string(),
            &info.construction.to_string(),
        ],
    );
    out.push('\n');
    out.push_str(&fill(
        Msg::SketchInfoConstraints.tr(lang),
        &[
            &info.constraints.to_string(),
            &info.dimensions.to_string(),
            &info.disabled.to_string(),
        ],
    ));
    out.push('\n');
    match info.bounds_mm {
        Some([x0, y0, x1, y1]) => out.push_str(&fill(
            Msg::SketchInfoSize.tr(lang),
            &[&num(x1 - x0), &num(y1 - y0), &num(info.length_mm)],
        )),
        None => out.push_str(Msg::SketchInfoEmpty.tr(lang)),
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::Face;
    use crate::sketch::SketchPlane;
    use crate::Length;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn sketch() -> Sketch {
        Sketch::new("s1", SketchPlane::on_face(Face::Top)).unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn an_empty_sketch_has_nothing_drawn() {
        let i = info(&sketch());
        assert_eq!(
            (
                i.points,
                i.lines,
                i.circles,
                i.arcs,
                i.constraints,
                i.construction
            ),
            (0, 0, 0, 0, 0, 0)
        );
        assert_eq!(i.bounds_mm, None);
        assert_eq!(i.length_mm, 0.0);
        assert_eq!(i.name, "s1");
        assert!(text_in(&i, Lang::En).ends_with("Nothing is drawn yet: there is no size\n"));
    }

    #[test]
    fn a_rectangle_has_four_points_four_lines_and_the_four_constraints_that_keep_it_square() {
        let mut s = sketch();
        s.add_rectangle((mm(2), mm(3)), (mm(12), mm(8))).unwrap();
        let i = info(&s);
        assert_eq!((i.points, i.lines, i.circles, i.arcs), (4, 4, 0, 0));
        assert_eq!((i.constraints, i.dimensions, i.disabled), (4, 0, 0));
        assert_eq!(i.bounds_mm, Some([2.0, 3.0, 12.0, 8.0]));
        assert!(close(i.length_mm, 30.0));
    }

    #[test]
    fn a_circle_counts_its_circumference_and_construction_geometry_is_not_drawn() {
        let mut s = sketch();
        let c = s.add_point(mm(10), mm(10)).unwrap();
        s.add_circle(c, mm(4)).unwrap();
        let a = s.add_point(mm(100), mm(100)).unwrap();
        let b = s.add_point(mm(200), mm(100)).unwrap();
        let guide = s.add_line(a, b).unwrap();
        s.set_construction(guide, true).unwrap();
        let i = info(&s);
        assert_eq!((i.points, i.lines, i.circles, i.construction), (3, 1, 1, 1));
        assert_eq!(i.bounds_mm, Some([6.0, 6.0, 14.0, 14.0]));
        assert!(close(i.length_mm, 2.0 * PI * 4.0));
    }

    #[test]
    fn an_arc_counts_its_length_and_the_extreme_points_it_passes_through() {
        let mut s = sketch();
        let c = s.add_point(mm(0), mm(0)).unwrap();
        // a quarter from (10, 0) to (0, 10), counter-clockwise
        let from = s.add_point(mm(10), mm(0)).unwrap();
        let to = s.add_point(mm(0), mm(10)).unwrap();
        s.add_arc(c, from, to).unwrap();
        let i = info(&s);
        assert_eq!(i.arcs, 1);
        assert!(close(i.length_mm, 2.0 * PI * 10.0 / 4.0));
        let b = i.bounds_mm.unwrap();
        assert!(close(b[0], 0.0) && close(b[1], 0.0) && close(b[2], 10.0) && close(b[3], 10.0));
        // three quarters, from (0, 10) to (10, 0): it passes through (-10, 0) and (0, -10)
        let mut s = sketch();
        let c = s.add_point(mm(0), mm(0)).unwrap();
        let from = s.add_point(mm(0), mm(10)).unwrap();
        let to = s.add_point(mm(10), mm(0)).unwrap();
        s.add_arc(c, from, to).unwrap();
        let i = info(&s);
        assert!(close(i.length_mm, 2.0 * PI * 10.0 * 0.75));
        let b = i.bounds_mm.unwrap();
        assert!(close(b[0], -10.0) && close(b[1], -10.0) && close(b[2], 10.0) && close(b[3], 10.0));
    }

    #[test]
    fn a_switched_off_constraint_is_counted() {
        let mut s = sketch();
        s.add_rectangle((mm(0), mm(0)), (mm(5), mm(5))).unwrap();
        let id = s.constraints()[0].id;
        s.set_enabled(id, false).unwrap();
        let i = info(&s);
        assert_eq!((i.constraints, i.disabled), (4, 1));
    }

    #[test]
    fn the_text_gives_the_same_figures_in_every_language() {
        let mut s = sketch();
        s.add_rectangle((mm(0), mm(0)), (mm(20), mm(10))).unwrap();
        let i = info(&s);
        for lang in Lang::ALL {
            let text = text_in(&i, lang);
            assert_eq!(text.lines().count(), 3, "{}", lang.code());
            assert!(!text.contains('{'));
            assert!(text.contains("s1") && text.contains("20.000") && text.contains("10.000"));
            assert!(text.contains("60.000"));
        }
        let en = text_in(&i, Lang::En);
        assert_eq!(
            en,
            "Sketch s1: 4 points, 4 lines, 0 circles, 0 arcs (0 construction)\n4 constraints (0 dimensions, 0 switched off)\nSize 20.000 x 10.000 mm, drawn length 60.000 mm\n"
        );
    }

    #[test]
    fn a_value_that_rounds_to_zero_has_no_minus_sign() {
        assert_eq!(num(-0.0001), "0.000");
        assert_eq!(num(-2.5), "-2.500");
    }
}
