// SPDX-License-Identifier: GPL-3.0-or-later
//! A flat DXF drawing read into a sketch: the lines, circles, arcs and polylines of the section
//! `ENTITIES` of a text DXF file become the points, lines, circles and arcs of a sketch, without
//! constraints, ready to be given some and to be raised from a face or cut into it.
//!
//! What is read: `LINE`, `CIRCLE`, `ARC`, `LWPOLYLINE` and the old `POLYLINE` with its `VERTEX`
//! records (a segment with a bulge is an arc, the closing flag joins the last vertex to the first).
//! Anything else (text, dimensions, splines, ellipses, hatches, blocks) is left out and counted by
//! its name, so the person is told what was not read. The units of the drawing are not looked
//! at: the caller says how many nanometres one unit is. The layers are not looked at either.
//!
//! Two ends that are no farther apart than the join distance (on each axis) become one point, so
//! the lines of a closed outline share their corners and the profile of the sketch is closed. A
//! line, an arc or a circle that has no length after that, or no radius, is left out and counted.
//! A binary DXF file is not understood. The number of lines, circles and arcs is limited by the
//! caller (`drawing.max_entities` in the settings) and the size of the file by the caller too.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use crate::sketch::{ElementId, Sketch, SketchPlane};
use crate::{CadError, Length};

/// What was read.
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    /// The sketch made of it.
    pub sketch: Sketch,
    /// How many lines it has (the segments of the polylines included).
    pub lines: usize,
    /// How many circles it has.
    pub circles: usize,
    /// How many arcs it has (the segments of the polylines with a bulge included).
    pub arcs: usize,
    /// What was left out, by the name of the entity of the file (`degenerate` for a line, a circle
    /// or an arc without length or radius), with how many.
    pub skipped: BTreeMap<String, usize>,
}

fn file_error(why: impl Into<String>) -> CadError {
    CadError::SketchFile(why.into())
}

/// The points of one cell of the grid: the element and its place in nanometres.
type Cell = Vec<(ElementId, (i64, i64))>;

/// The points of the sketch, found again by a grid of cells as wide as the join distance.
struct Joiner {
    join: i64,
    cells: BTreeMap<(i64, i64), Cell>,
}

impl Joiner {
    fn point(&mut self, sketch: &mut Sketch, x: i64, y: i64) -> Result<ElementId, CadError> {
        let cell = |v: i64| v.div_euclid(self.join);
        let (cx, cy) = (cell(x), cell(y));
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(list) = self.cells.get(&(cx + dx, cy + dy)) {
                    for (id, (px, py)) in list {
                        if (px - x).abs() <= self.join && (py - y).abs() <= self.join {
                            return Ok(*id);
                        }
                    }
                }
            }
        }
        let id = sketch.add_point(Length::new(x)?, Length::new(y)?)?;
        self.cells.entry((cx, cy)).or_default().push((id, (x, y)));
        Ok(id)
    }
}

/// One vertex of a polyline: its place and the bulge of the segment that leaves it.
#[derive(Debug, Clone, Copy, Default)]
struct Vertex {
    x: f64,
    y: f64,
    bulge: f64,
}

/// A polyline being read.
#[derive(Debug, Default)]
struct Poly {
    closed: bool,
    vertices: Vec<Vertex>,
}

struct Reader<'a> {
    sketch: Sketch,
    joiner: Joiner,
    unit: f64,
    max: usize,
    lines: usize,
    circles: usize,
    arcs: usize,
    skipped: BTreeMap<String, usize>,
    group: &'a [(i32, &'a str)],
}

fn number(value: &str) -> Result<f64, CadError> {
    value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| file_error(format!("{value:?} is not a number")))
}

impl Reader<'_> {
    /// The value of the first group with this code, or zero when there is none.
    fn first(&self, code: i32) -> Result<f64, CadError> {
        match self.group.iter().find(|(c, _)| *c == code) {
            Some((_, v)) => number(v),
            None => Ok(0.0),
        }
    }

    /// A number of drawing units as whole nanometres.
    fn nm(&self, units: f64) -> Result<i64, CadError> {
        let v = (units * self.unit).round();
        if v.abs() >= (1u64 << 40) as f64 {
            return Err(file_error("a coordinate is outside the range of lengths"));
        }
        Ok(v as i64)
    }

    fn count(&mut self) -> Result<(), CadError> {
        if self.lines + self.circles + self.arcs >= self.max {
            return Err(CadError::ExportLimit(format!(
                "the drawing has more than the {} lines, circles and arcs the settings allow",
                self.max
            )));
        }
        Ok(())
    }

    fn skip(&mut self, name: &str) {
        *self.skipped.entry(name.to_string()).or_insert(0) += 1;
    }

    fn point_at(&mut self, x: f64, y: f64) -> Result<ElementId, CadError> {
        let (nx, ny) = (self.nm(x)?, self.nm(y)?);
        self.joiner.point(&mut self.sketch, nx, ny)
    }

    fn line(&mut self, a: (f64, f64), b: (f64, f64)) -> Result<(), CadError> {
        let ia = self.point_at(a.0, a.1)?;
        let ib = self.point_at(b.0, b.1)?;
        if ia == ib {
            self.skip("degenerate");
            return Ok(());
        }
        self.count()?;
        self.sketch.add_line(ia, ib)?;
        self.lines += 1;
        Ok(())
    }

    /// An arc about a centre, counter-clockwise from the first end to the second.
    fn arc(&mut self, centre: (f64, f64), from: ElementId, to: ElementId) -> Result<(), CadError> {
        let ic = self.point_at(centre.0, centre.1)?;
        if ic == from || ic == to || from == to {
            self.skip("degenerate");
            return Ok(());
        }
        self.count()?;
        self.sketch.add_arc(ic, from, to)?;
        self.arcs += 1;
        Ok(())
    }

    fn polyline(&mut self, poly: &Poly) -> Result<(), CadError> {
        let n = poly.vertices.len();
        let segments = if poly.closed { n } else { n.saturating_sub(1) };
        for i in 0..segments {
            let (p, q) = (poly.vertices[i], poly.vertices[(i + 1) % n]);
            if p.bulge == 0.0 {
                self.line((p.x, p.y), (q.x, q.y))?;
                continue;
            }
            // the arc of a bulge: the angle of the arc is four times the arctangent of the bulge,
            // and its centre is on the left of the way from `p` to `q` (on the right when the
            // bulge is negative) at (c / 2) / tan(half the angle) from the middle of the chord
            let (dx, dy) = (q.x - p.x, q.y - p.y);
            let chord = dx.hypot(dy);
            if chord == 0.0 {
                self.skip("degenerate");
                continue;
            }
            let half = 2.0 * p.bulge.atan();
            let h = (chord / 2.0) / half.tan();
            let centre = (
                (p.x + q.x) / 2.0 - dy / chord * h,
                (p.y + q.y) / 2.0 + dx / chord * h,
            );
            let a = self.point_at(p.x, p.y)?;
            let b = self.point_at(q.x, q.y)?;
            if p.bulge > 0.0 {
                self.arc(centre, a, b)?;
            } else {
                self.arc(centre, b, a)?;
            }
        }
        Ok(())
    }

    fn entity(&mut self, name: &str, poly: &mut Option<Poly>) -> Result<(), CadError> {
        match name {
            "LINE" => {
                let a = (self.first(10)?, self.first(20)?);
                let b = (self.first(11)?, self.first(21)?);
                self.line(a, b)
            }
            "CIRCLE" => {
                let (centre, radius) = ((self.first(10)?, self.first(20)?), self.first(40)?);
                let r = self.nm(radius)?;
                if r <= 0 {
                    self.skip("degenerate");
                    return Ok(());
                }
                let ic = self.point_at(centre.0, centre.1)?;
                self.count()?;
                self.sketch.add_circle(ic, Length::new(r)?)?;
                self.circles += 1;
                Ok(())
            }
            "ARC" => {
                let (centre, radius) = ((self.first(10)?, self.first(20)?), self.first(40)?);
                if self.nm(radius)? <= 0 {
                    self.skip("degenerate");
                    return Ok(());
                }
                let end_of = |degrees: f64| {
                    let a = degrees * PI / 180.0;
                    (centre.0 + radius * a.cos(), centre.1 + radius * a.sin())
                };
                let (start, end) = (end_of(self.first(50)?), end_of(self.first(51)?));
                let a = self.point_at(start.0, start.1)?;
                let b = self.point_at(end.0, end.1)?;
                self.arc(centre, a, b)
            }
            "LWPOLYLINE" => {
                let mut p = Poly {
                    closed: (self.first(70)? as i64) & 1 == 1,
                    vertices: Vec::new(),
                };
                for (code, value) in self.group {
                    match *code {
                        10 => p.vertices.push(Vertex {
                            x: number(value)?,
                            ..Vertex::default()
                        }),
                        20 => {
                            if let Some(v) = p.vertices.last_mut() {
                                v.y = number(value)?;
                            }
                        }
                        42 => {
                            if let Some(v) = p.vertices.last_mut() {
                                v.bulge = number(value)?;
                            }
                        }
                        _ => {}
                    }
                }
                self.polyline(&p)
            }
            "POLYLINE" => {
                *poly = Some(Poly {
                    closed: (self.first(70)? as i64) & 1 == 1,
                    vertices: Vec::new(),
                });
                Ok(())
            }
            "VERTEX" => {
                let vertex = Vertex {
                    x: self.first(10)?,
                    y: self.first(20)?,
                    bulge: self.first(42)?,
                };
                if let Some(p) = poly {
                    p.vertices.push(vertex);
                }
                Ok(())
            }
            "SEQEND" => match poly.take() {
                Some(p) => self.polyline(&p),
                None => Ok(()),
            },
            other => {
                self.skip(other);
                Ok(())
            }
        }
    }
}

/// The sketch that a text DXF drawing makes. `unit_nm` is the nanometres of one unit of the
/// drawing (1 000 000 for millimetres), `join_nm` the distance within which two ends are one
/// point and `max_entities` the most lines, circles and arcs. Refused with `PHOENIX-CAD-035` when
/// the text is not a DXF drawing in text form or has nothing to read, and with `PHOENIX-CAD-050`
/// when it has more than `max_entities`.
pub fn from_dxf(
    text: &str,
    name: &str,
    plane: SketchPlane,
    unit_nm: i64,
    join_nm: i64,
    max_entities: usize,
) -> Result<Import, CadError> {
    if unit_nm <= 0 {
        return Err(file_error("the unit of the drawing is not positive"));
    }
    let mut pairs: Vec<(i32, &str)> = Vec::new();
    let mut lines = text.lines();
    while let Some(code_line) = lines.next() {
        if code_line.trim().is_empty() {
            continue;
        }
        let value = lines
            .next()
            .ok_or_else(|| file_error("a group code of the drawing has no value"))?;
        let code = code_line.trim().parse::<i32>().map_err(|_| {
            file_error("the drawing is not a text DXF file (a group code is not a number)")
        })?;
        pairs.push((code, value.trim()));
    }
    let start = (0..pairs.len().saturating_sub(1))
        .find(|i| pairs[*i] == (0, "SECTION") && pairs[i + 1] == (2, "ENTITIES"))
        .ok_or_else(|| file_error("the drawing has no section of entities"))?;
    let mut reader = Reader {
        sketch: Sketch::new(name, plane)?,
        joiner: Joiner {
            join: join_nm.max(1),
            cells: BTreeMap::new(),
        },
        unit: unit_nm as f64,
        max: max_entities,
        lines: 0,
        circles: 0,
        arcs: 0,
        skipped: BTreeMap::new(),
        group: &[],
    };
    let mut poly: Option<Poly> = None;
    let mut i = start + 2;
    while i < pairs.len() && pairs[i] != (0, "ENDSEC") {
        if pairs[i].0 != 0 {
            i += 1;
            continue;
        }
        let name = pairs[i].1;
        let mut j = i + 1;
        while j < pairs.len() && pairs[j].0 != 0 {
            j += 1;
        }
        reader.group = &pairs[i + 1..j];
        reader.entity(name, &mut poly)?;
        i = j;
    }
    if let Some(p) = poly.take() {
        reader.polyline(&p)?;
    }
    if reader.lines + reader.circles + reader.arcs == 0 {
        return Err(file_error("the drawing has no line, circle or arc to read"));
    }
    Ok(Import {
        sketch: reader.sketch,
        lines: reader.lines,
        circles: reader.circles,
        arcs: reader.arcs,
        skipped: reader.skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::Entity;
    use crate::enclosure::Face;
    use crate::profile::{Profile, ProfileLimits};
    use crate::sketch_export::drawing_of;

    const MM: i64 = 1_000_000;

    fn dxf(entities: &str) -> String {
        format!("0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n")
    }

    fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> String {
        format!("0\nLINE\n8\n0\n10\n{x1}\n20\n{y1}\n30\n0\n11\n{x2}\n21\n{y2}\n31\n0\n")
    }

    fn circle(x: f64, y: f64, r: f64) -> String {
        format!("0\nCIRCLE\n8\n0\n10\n{x}\n20\n{y}\n30\n0\n40\n{r}\n")
    }

    fn read(text: &str) -> Result<Import, CadError> {
        from_dxf(
            text,
            "drawn",
            SketchPlane::on_face(Face::Front),
            MM,
            1_000,
            1000,
        )
    }

    fn rectangle() -> String {
        [
            line(0.0, 0.0, 100.0, 0.0),
            line(100.0, 0.0, 100.0, 60.0),
            line(100.0, 60.0, 0.0, 60.0),
            line(0.0, 60.0, 0.0, 0.0),
        ]
        .concat()
    }

    fn profile(i: &Import) -> Profile {
        Profile::from_sketch(&i.sketch, &ProfileLimits::shipped()).unwrap()
    }

    #[test]
    fn lines_that_meet_make_a_closed_outline_and_a_circle_makes_a_hole() {
        let i = read(&dxf(&format!("{}{}", rectangle(), circle(20.0, 30.0, 5.0)))).unwrap();
        assert_eq!((i.lines, i.circles, i.arcs), (4, 1, 0));
        assert!(i.skipped.is_empty());
        // the four lines share their corners: four points and one for the centre of the circle
        assert_eq!(i.sketch.elements().len(), 4 + 4 + 1 + 1);
        let p = profile(&i);
        assert_eq!(p.regions.len(), 1);
        assert_eq!(p.regions[0].holes.len(), 1);
        assert_eq!(p.bounds(), ((0, 0), (100 * MM, 60 * MM)));
    }

    #[test]
    fn ends_within_the_join_distance_are_one_point() {
        // the last line stops 0.0005 mm (500 nm) short of the first corner: joined at 1000 nm
        let text = dxf(&[
            line(0.0, 0.0, 100.0, 0.0),
            line(100.0, 0.0, 100.0, 60.0),
            line(100.0, 60.0, 0.0, 60.0),
            line(0.0, 60.0, 0.0, 0.0005),
        ]
        .concat());
        assert!(profile(&read(&text).unwrap()).regions.len() == 1);
        // and with a join distance of one nanometre the outline is open
        let open = from_dxf(
            &text,
            "drawn",
            SketchPlane::on_face(Face::Front),
            MM,
            1,
            1000,
        )
        .unwrap();
        assert!(Profile::from_sketch(&open.sketch, &ProfileLimits::shipped()).is_err());
    }

    #[test]
    fn an_arc_goes_counter_clockwise_between_its_two_angles() {
        // a quarter of a circle of 10 mm about (5, 5), from 0 to 90 degrees, and the two radii
        let text = dxf(&[
            "0\nARC\n8\n0\n10\n5\n20\n5\n40\n10\n50\n0\n51\n90\n".to_string(),
            line(5.0, 5.0, 15.0, 5.0),
            line(5.0, 15.0, 5.0, 5.0),
        ]
        .concat());
        let i = read(&text).unwrap();
        assert_eq!((i.lines, i.arcs), (2, 1));
        // 25 pi mm2 of a quarter of a disc
        let p = profile(&i);
        assert_eq!(p.regions.len(), 1);
        let (lo, hi) = p.bounds();
        assert_eq!(lo, (5 * MM, 5 * MM));
        assert!(
            (hi.0 - 15 * MM).abs() < 6_000 && (hi.1 - 15 * MM).abs() < 6_000,
            "{hi:?}"
        );
    }

    #[test]
    fn a_polyline_with_a_bulge_is_a_line_and_an_arc() {
        // a half disc: from (0, 0) to (10, 0) by a bulge of 1 (counter-clockwise, below the
        // chord) and back by a straight segment
        let text = dxf("0\nLWPOLYLINE\n8\n0\n90\n2\n70\n1\n10\n0\n20\n0\n42\n1\n10\n10\n20\n0\n");
        let i = read(&text).unwrap();
        assert_eq!((i.lines, i.arcs), (1, 1));
        let p = profile(&i);
        assert_eq!(p.regions.len(), 1);
        let (lo, hi) = p.bounds();
        assert!((lo.1 + 5 * MM).abs() < 6_000, "{lo:?}");
        assert_eq!((lo.0, hi.0, hi.1), (0, 10 * MM, 0));
        // the same half disc with a negative bulge is above the chord
        let up = read(&dxf(
            "0\nLWPOLYLINE\n8\n0\n90\n2\n70\n1\n10\n0\n20\n0\n42\n-1\n10\n10\n20\n0\n",
        ))
        .unwrap();
        let (lo, hi) = profile(&up).bounds();
        assert!((hi.1 - 5 * MM).abs() < 6_000, "{hi:?}");
        assert_eq!(lo.1, 0);
    }

    #[test]
    fn an_old_style_polyline_with_its_vertices_is_read_too() {
        let mut text = String::from("0\nPOLYLINE\n8\n0\n66\n1\n70\n1\n");
        for (x, y) in [(0, 0), (10, 0), (10, 10), (0, 10)] {
            text.push_str(&format!("0\nVERTEX\n8\n0\n10\n{x}\n20\n{y}\n"));
        }
        text.push_str("0\nSEQEND\n");
        let i = read(&dxf(&text)).unwrap();
        assert_eq!(i.lines, 4);
        assert_eq!(profile(&i).regions.len(), 1);
    }

    #[test]
    fn what_is_not_read_is_counted_by_name_and_the_units_scale_the_drawing() {
        let text = dxf(&format!(
            "{}0\nTEXT\n8\n0\n1\nhello\n0\nSPLINE\n8\n0\n0\nTEXT\n8\n0\n1\nagain\n{}",
            line(0.0, 0.0, 1.0, 0.0),
            line(0.0, 0.0, 0.0, 0.0)
        ));
        let i = read(&text).unwrap();
        assert_eq!(i.lines, 1);
        assert_eq!(i.skipped["TEXT"], 2);
        assert_eq!(i.skipped["SPLINE"], 1);
        assert_eq!(i.skipped["degenerate"], 1);
        // in inches a line of 1 unit is 25.4 mm
        let inches = from_dxf(
            &dxf(&line(0.0, 0.0, 1.0, 0.0)),
            "drawn",
            SketchPlane::on_face(Face::Front),
            25_400_000,
            1_000,
            10,
        )
        .unwrap();
        let ends: Vec<i64> = inches
            .sketch
            .elements()
            .iter()
            .filter_map(|e| match e.element {
                crate::sketch::Element::Point { x, .. } => Some(x.get()),
                _ => None,
            })
            .collect();
        assert_eq!(ends, vec![0, 25_400_000]);
    }

    #[test]
    fn a_drawing_that_cannot_be_read_is_refused_with_its_code() {
        for (text, why) in [
            ("AutoCAD Binary DXF\nrest\n".to_string(), "not a text DXF"),
            ("0\n".to_string(), "no value"),
            ("0\nEOF\n".to_string(), "no section of entities"),
            (dxf(""), "no line, circle or arc"),
            (dxf("0\nTEXT\n1\nhi\n"), "no line, circle or arc"),
            (dxf(&line(0.0, 0.0, 1.0e30, 0.0)), "outside the range"),
            (dxf("0\nLINE\n10\nabc\n"), "not a number"),
        ] {
            let e = read(&text).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-035", "{why}: {e}");
            assert!(e.to_string().contains(why), "{why}: {e}");
        }
        // more lines than the settings allow
        let e = from_dxf(
            &dxf(&rectangle()),
            "drawn",
            SketchPlane::on_face(Face::Front),
            MM,
            1_000,
            3,
        )
        .unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-050");
        let e = from_dxf(
            &dxf(&rectangle()),
            "not a name",
            SketchPlane::on_face(Face::Front),
            MM,
            1_000,
            10,
        )
        .unwrap_err();
        assert!(e.code().starts_with("PHOENIX-CAD-"));
    }

    #[test]
    fn what_the_program_writes_it_reads_back() {
        // the DXF of the outline and the hole of a sketch gives the same outline again
        let first = read(&dxf(&format!("{}{}", rectangle(), circle(20.0, 30.0, 5.0)))).unwrap();
        let drawing = drawing_of(&profile(&first));
        let count =
            |kind: fn(&Entity) -> bool| drawing.entities.iter().filter(|e| kind(&e.1)).count();
        assert_eq!(count(|e| matches!(e, Entity::Line(..))), 4);
        assert_eq!(count(|e| matches!(e, Entity::Circle(..))), 1);
        assert_eq!(count(|e| matches!(e, Entity::Arc(..))), 0);
        let again = read(&drawing.to_dxf()).unwrap();
        assert_eq!((again.lines, again.circles, again.arcs), (4, 1, 0));
        let (a, b) = (profile(&first), profile(&again));
        assert_eq!(b.regions.len(), a.regions.len());
        assert_eq!(b.regions[0].holes.len(), 1);
        assert_eq!(b.bounds(), a.bounds());
    }
}
