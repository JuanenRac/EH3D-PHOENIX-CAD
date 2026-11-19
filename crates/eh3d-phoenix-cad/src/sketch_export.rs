// SPDX-License-Identifier: GPL-3.0-or-later
//! A sketch as a flat drawing: its closed outlines in a DXF or an SVG file, for a laser, a router
//! or a plotter.
//!
//! The outlines are the ones the extrusions and pockets are made from ([`Profile`]): the outer
//! outline of each region goes on the layer `CONTORNO` and the holes in it on `AGUJEROS`, in
//! millimetres and in the coordinates of the sketch (the plane of the face it lies on, `u` to the
//! right and `v` up). Lines stay lines, arcs stay arcs (counter-clockwise from the first angle to
//! the second, as DXF wants) and circles stay circles; construction geometry and points alone are
//! left out, as in the profile. The files are the ones of the drawings of the box
//! ([`crate::drawing`]), so the layers, the colours and the number format are the same.

use crate::config::DrawingSettings;
use crate::drawing::{Drawing, Entity, Layer, Note, View};
use crate::lang::Lang;
use crate::profile::{Outline, Piece, Profile, ProfileLimits, Pt};
use crate::scad::mm_text;
use crate::sketch::settings::SketchSettings;
use crate::sketch::Sketch;
use crate::titleblock::TitleBlockSettings;
use crate::CadError;

fn mm(nm: i64) -> f64 {
    nm as f64 / 1_000_000.0
}

fn point(p: Pt) -> (f64, f64) {
    (mm(p.0), mm(p.1))
}

fn angle_deg(center: Pt, p: Pt) -> f64 {
    let dy = (p.1 - center.1) as f64;
    let dx = (p.0 - center.0) as f64;
    dy.atan2(dx).to_degrees()
}

fn outline_entities(layer: Layer, outline: &Outline, out: &mut Vec<(Layer, Entity)>) {
    if let Some((c, r)) = outline.circle_of() {
        out.push((layer, Entity::Circle(point(c), mm(r))));
        return;
    }
    for piece in outline.pieces() {
        match *piece {
            Piece::Line { from, to } => {
                out.push((layer, Entity::Line(point(from), point(to))));
            }
            Piece::Arc {
                center,
                from,
                to,
                ccw,
            } => {
                // DXF arcs run counter-clockwise: a clockwise piece is the same arc walked backwards
                let (a, b) = if ccw { (from, to) } else { (to, from) };
                let dx = (from.0 - center.0) as f64;
                let dy = (from.1 - center.1) as f64;
                let radius = (dx * dx + dy * dy).sqrt();
                let mut start = angle_deg(center, a);
                let mut end = angle_deg(center, b);
                if end <= start {
                    end += 360.0;
                }
                if start < 0.0 {
                    start += 360.0;
                    end += 360.0;
                }
                out.push((
                    layer,
                    Entity::Arc(point(center), mm(radius as i64), start, end),
                ));
            }
        }
    }
}

/// The drawing of a profile: the outer outlines on `CONTORNO` and the holes on `AGUJEROS`.
pub fn drawing_of(profile: &Profile) -> Drawing {
    let mut entities = Vec::new();
    for region in &profile.regions {
        outline_entities(Layer::Outline, &region.outer, &mut entities);
        for hole in &region.holes {
            outline_entities(Layer::Holes, hole, &mut entities);
        }
    }
    let (lo, hi) = profile.bounds();
    Drawing {
        view: View::Top,
        entities,
        notes: Vec::new(),
        extent: (mm(hi.0 - lo.0), mm(hi.1 - lo.1)),
        dxf_dimensions: false,
    }
}

/// Puts the dimensions of a sketch on its drawing, for the settings `drawing.dimensions` (SVG) and
/// `drawing.dxf_dimensions` (DXF, on the layer `COTAS`): the width and the height of the whole
/// outline from its lower left corner and, with `drawing.hole_dimensions`, the diameter of every
/// round hole written beside it (a hole that is a chain of lines and arcs is not labelled).
/// Refused with `PHOENIX-CAD-050` when there are more labels than `drawing.max_dimensions`. A
/// drawing with neither of the two settings is left as it is.
pub fn add_dimensions(
    drawing: &mut Drawing,
    profile: &Profile,
    settings: &DrawingSettings,
) -> Result<(), CadError> {
    drawing.dxf_dimensions = settings.dxf_dimensions;
    if !settings.dimensions && !settings.dxf_dimensions {
        return Ok(());
    }
    let (lo, hi) = profile.bounds();
    let mut labels = Vec::new();
    if settings.hole_dimensions {
        for region in &profile.regions {
            for hole in &region.holes {
                if let Some((c, r)) = hole.circle_of() {
                    labels.push((c, r));
                }
            }
        }
        if labels.len() > settings.max_dimensions as usize {
            return Err(CadError::ExportLimit(format!(
                "the sketch has {} round holes to label and the settings allow {} dimensions",
                labels.len(),
                settings.max_dimensions
            )));
        }
    }
    let corner = point(lo);
    drawing.notes.push(Note {
        at: (mm((lo.0 + hi.0) / 2), corner.1),
        text: mm_text(hi.0 - lo.0),
        measures: Some((corner, (mm(hi.0), corner.1))),
        level: 0,
    });
    drawing.notes.push(Note {
        at: (corner.0, mm((lo.1 + hi.1) / 2)),
        text: mm_text(hi.1 - lo.1),
        measures: Some((corner, (corner.0, mm(hi.1)))),
        level: 0,
    });
    for (c, r) in labels {
        drawing.notes.push(Note {
            at: (mm(c.0 + r) + 1.0, mm(c.1 + r) + 1.0),
            text: format!("\u{2300}{}", mm_text(2 * r)),
            measures: None,
            level: 0,
        });
    }
    Ok(())
}

// The text of the DXF or SVG file of a sketch. Refused with `PHOENIX-CAD-036` when the sketch has
/// no usable outline (see [`Profile::from_sketch`]) and with `PHOENIX-CAD-050` when the drawing
/// has more entities than `drawing.max_entities`.
pub fn render(
    sketch: &Sketch,
    solver: &SketchSettings,
    drawing: &DrawingSettings,
    dxf: bool,
) -> Result<String, CadError> {
    render_titled(
        sketch,
        solver,
        drawing,
        dxf,
        &TitleBlockSettings::off(),
        Lang::En,
    )
}

/// The same as [`render`] with a title block under the drawing when `title.enabled` is true: the
/// title of the block is the name of the sketch when the settings give none, and `lang` is the
/// language of the names of its cells.
pub fn render_titled(
    sketch: &Sketch,
    solver: &SketchSettings,
    drawing: &DrawingSettings,
    dxf: bool,
    title: &TitleBlockSettings,
    lang: Lang,
) -> Result<String, CadError> {
    let profile = Profile::from_sketch(sketch, &ProfileLimits::from_settings(solver))?;
    let mut d = drawing_of(&profile);
    if d.entities.len() > drawing.max_entities as usize {
        return Err(CadError::ExportLimit(format!(
            "the sketch has {} entities, more than the {} the settings allow",
            d.entities.len(),
            drawing.max_entities
        )));
    }
    add_dimensions(&mut d, &profile, drawing)?;
    Ok(match (title.enabled, dxf) {
        (false, true) => d.to_dxf(),
        (false, false) => d.to_svg(drawing),
        (true, true) => {
            let block = d.title_block(title, sketch.name(), "1:1", 6.0, lang);
            d.to_dxf_with(Some(&block))
        }
        (true, false) => d.to_svg_with(drawing, Some((title, sketch.name(), lang))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::enclosure::Face;
    use crate::sketch::SketchPlane;
    use crate::Length;

    fn mmv(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn new() -> Sketch {
        Sketch::new("plate", SketchPlane::on_face(Face::Top)).unwrap()
    }

    fn polygon(s: &mut Sketch, corners: &[(i64, i64)]) {
        let ids: Vec<_> = corners
            .iter()
            .map(|(x, y)| s.add_point(mmv(*x), mmv(*y)).unwrap())
            .collect();
        for i in 0..ids.len() {
            s.add_line(ids[i], ids[(i + 1) % ids.len()]).unwrap();
        }
    }

    fn circle(s: &mut Sketch, x: i64, y: i64, r: i64) {
        let c = s.add_point(mmv(x), mmv(y)).unwrap();
        s.add_circle(c, mmv(r)).unwrap();
    }

    fn plate() -> Sketch {
        let mut s = new();
        polygon(&mut s, &[(0, 0), (40, 0), (40, 25), (0, 25)]);
        circle(&mut s, 20, 12, 3);
        s
    }

    fn settings() -> (SketchSettings, DrawingSettings) {
        (SketchSettings::defaults(), Settings::defaults().drawing)
    }

    #[test]
    fn a_plate_with_a_hole_has_four_lines_on_the_outline_and_a_circle_on_the_holes() {
        let p = Profile::from_sketch(&plate(), &ProfileLimits::shipped()).unwrap();
        let d = drawing_of(&p);
        assert_eq!(d.count(Layer::Outline), 4);
        assert_eq!(d.count(Layer::Holes), 1);
        assert_eq!(d.extent, (40.0, 25.0));
        assert!(d
            .entities
            .iter()
            .any(|(l, e)| *l == Layer::Holes && *e == Entity::Circle((20.0, 12.0), 3.0)));
        // the outline is walked counter-clockwise: the first line goes along the bottom
        let first = d.entities[0];
        assert!(matches!(first, (Layer::Outline, Entity::Line(..))));
    }

    #[test]
    fn the_dxf_file_holds_the_entities_in_millimetres() {
        let (s, mut dr) = settings();
        // without the dimensions, which have their own test
        dr.dxf_dimensions = false;
        let dxf = render(&plate(), &s, &dr, true).unwrap();
        assert_eq!(dxf.matches("\nLINE\n").count(), 4);
        assert_eq!(dxf.matches("\nCIRCLE\n").count(), 1);
        assert!(dxf.contains("\nCONTORNO\n"));
        assert!(dxf.contains("\nAGUJEROS\n"));
        assert!(dxf.contains("10\n20.000000\n20\n12.000000\n30\n0.0\n40\n3.000000\n"));
        assert!(dxf.ends_with("0\nEOF\n"));
        // the same sketch gives the same bytes
        assert_eq!(dxf, render(&plate(), &s, &dr, true).unwrap());
    }

    #[test]
    fn a_sketch_drawing_has_the_width_the_height_and_the_diameter_of_its_round_holes() {
        let (s, dr) = settings();
        let dxf = render(&plate(), &s, &dr, true).unwrap();
        assert!(dxf.contains("\nCOTAS\n"));
        // two dimension lines with their two extension lines each, besides the four of the outline
        assert_eq!(dxf.matches("\nLINE\n").count(), 4 + 6);
        // the texts are the width, the height and the diameter of the hole
        assert_eq!(dxf.matches("\nTEXT\n").count(), 3);
        assert!(dxf.contains("\n1\n40\n"), "{dxf}");
        assert!(dxf.contains("\n1\n25\n"), "{dxf}");
        assert!(dxf.contains("\n1\n\\U+23006\n"), "{dxf}");
        let svg = render(&plate(), &s, &dr, false).unwrap();
        assert!(svg.contains("<g id=\"COTAS\""), "{svg}");
        assert!(
            svg.contains(">40</text>") && svg.contains(">25</text>"),
            "{svg}"
        );
        assert!(svg.contains(">\u{2300}6</text>"), "{svg}");
        // the same sketch gives the same bytes
        assert_eq!(dxf, render(&plate(), &s, &dr, true).unwrap());
    }

    #[test]
    fn the_dimensions_of_a_sketch_follow_the_settings_of_the_drawing() {
        let (s, mut dr) = settings();
        dr.hole_dimensions = false;
        let dxf = render(&plate(), &s, &dr, true).unwrap();
        assert_eq!(dxf.matches("\nTEXT\n").count(), 2);
        assert!(!dxf.contains("U+2300"));
        // the DXF file and the SVG file are switched on their own
        dr.dxf_dimensions = false;
        let dxf = render(&plate(), &s, &dr, true).unwrap();
        assert!(!dxf.contains("COTAS") && !dxf.contains("\nTEXT\n"));
        assert_eq!(dxf.matches("\nLINE\n").count(), 4);
        assert!(render(&plate(), &s, &dr, false)
            .unwrap()
            .contains("<g id=\"COTAS\""));
        dr.dimensions = false;
        let svg = render(&plate(), &s, &dr, false).unwrap();
        assert!(!svg.contains("COTAS"));
        // with both off the drawing is the one without notes
        let p = Profile::from_sketch(&plate(), &ProfileLimits::shipped()).unwrap();
        let mut d = drawing_of(&p);
        add_dimensions(&mut d, &p, &dr).unwrap();
        assert!(d.notes.is_empty());
    }

    #[test]
    fn more_round_holes_than_the_settings_allow_dimensions_for_are_refused() {
        let (s, mut dr) = settings();
        let mut sketch = plate();
        circle(&mut sketch, 8, 6, 2);
        dr.max_dimensions = 1;
        let e = render(&sketch, &s, &dr, false).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-050");
        // without the labels of the holes there is nothing to count; with room for both, both are
        dr.hole_dimensions = false;
        assert!(render(&sketch, &s, &dr, false).is_ok());
        dr.hole_dimensions = true;
        dr.max_dimensions = 2;
        let svg = render(&sketch, &s, &dr, false).unwrap();
        assert!(svg.contains(">\u{2300}4</text>") && svg.contains(">\u{2300}6</text>"));
    }

    #[test]
    fn the_dimensions_start_at_the_lower_left_corner_of_a_sketch_that_is_not_at_the_origin() {
        let (s, dr) = settings();
        let mut sketch = new();
        polygon(&mut sketch, &[(-30, -10), (-10, -10), (-10, 15), (-30, 15)]);
        let p = Profile::from_sketch(&sketch, &ProfileLimits::shipped()).unwrap();
        let mut d = drawing_of(&p);
        add_dimensions(&mut d, &p, &dr).unwrap();
        let measured: Vec<_> = d.notes.iter().filter_map(|n| n.measures).collect();
        assert_eq!(
            measured,
            vec![
                ((-30.0, -10.0), (-10.0, -10.0)),
                ((-30.0, -10.0), (-30.0, 15.0))
            ]
        );
        assert_eq!(d.notes[0].text, "20");
        assert_eq!(d.notes[1].text, "25");
        // a sketch drawing with them still has its page around everything
        let svg = render(&sketch, &s, &dr, false).unwrap();
        assert!(
            svg.contains(">20</text>") && svg.contains(">25</text>"),
            "{svg}"
        );
    }

    #[test]
    fn the_svg_file_is_scaled_to_the_page_of_the_settings() {
        let (s, dr) = settings();
        let svg = render(&plate(), &s, &dr, false).unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"), "{svg}");
        assert!(svg.contains("</svg>"));
        assert!(
            svg.contains("200mm") || svg.contains("width=\"200"),
            "{svg}"
        );
    }

    #[test]
    fn an_arc_is_written_counter_clockwise_from_the_first_angle_to_the_second() {
        // a half disc: the line from (-10, 0) to (10, 0) and the arc over it, centre at the origin
        let mut s = new();
        let c = s.add_point(mmv(0), mmv(0)).unwrap();
        let a = s.add_point(mmv(10), mmv(0)).unwrap();
        let b = s.add_point(mmv(-10), mmv(0)).unwrap();
        s.add_arc(c, a, b).unwrap();
        s.add_line(b, a).unwrap();
        let p = Profile::from_sketch(&s, &ProfileLimits::shipped()).unwrap();
        let d = drawing_of(&p);
        let arcs: Vec<_> = d
            .entities
            .iter()
            .filter_map(|(_, e)| match e {
                Entity::Arc(c, r, a0, a1) => Some((*c, *r, *a0, *a1)),
                _ => None,
            })
            .collect();
        assert_eq!(arcs.len(), 1);
        let (centre, r, a0, a1) = arcs[0];
        assert_eq!(centre, (0.0, 0.0));
        assert!((r - 10.0).abs() < 1e-9);
        assert!(a0.abs() < 1e-9, "{a0}");
        assert!((a1 - 180.0).abs() < 1e-9, "{a1}");
        assert_eq!(d.count(Layer::Outline), 2);
    }

    #[test]
    fn a_sketch_without_outlines_and_a_drawing_above_the_limit_are_refused() {
        let (s, mut dr) = settings();
        let mut empty = new();
        empty.add_point(mmv(1), mmv(1)).unwrap();
        let e = render(&empty, &s, &dr, true).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-036");
        dr.max_entities = 4;
        let e = render(&plate(), &s, &dr, true).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-050");
        dr.max_entities = 5;
        assert!(render(&plate(), &s, &dr, true).is_ok());
    }

    #[test]
    fn the_drawing_of_a_sketch_can_have_a_title_block_named_after_the_sketch() {
        let (s, dr) = settings();
        let tb = TitleBlockSettings {
            enabled: true,
            ..TitleBlockSettings::off()
        };
        let dxf = render_titled(&plate(), &s, &dr, true, &tb, Lang::It).unwrap();
        assert!(dxf.contains("ROTULO"));
        assert!(dxf.contains("1\nNumero del disegno\n"));
        assert!(dxf.contains("1\nplate\n"));
        let svg = render_titled(&plate(), &s, &dr, false, &tb, Lang::En).unwrap();
        assert!(svg.contains("<g id=\"ROTULO\""));
        // off, it is the drawing it always was
        let off = TitleBlockSettings::off();
        assert_eq!(
            render_titled(&plate(), &s, &dr, true, &off, Lang::It).unwrap(),
            render(&plate(), &s, &dr, true).unwrap()
        );
    }
}
