// SPDX-License-Identifier: GPL-3.0-or-later
//! A flat DXF drawing read into a sketch: the lines, circles, arcs and polylines of the section
//! `ENTITIES` of a text DXF file become the points, lines, circles and arcs of a sketch, without
//! constraints, ready to be given some and to be raised from a face or cut into it.
//!
//! What is read: `LINE`, `CIRCLE`, `ARC`, `LWPOLYLINE` and the old `POLYLINE` with its `VERTEX`
//! records (a segment with a bulge is an arc, the closing flag joins the last vertex to the first).
//! Anything else (text, dimensions, splines, ellipses, hatches, blocks) is left out and counted by
//! its name, so the person is told what was not read. The units of the drawing are not looked
//! at: the caller says how many nanometres one unit is. The layers can be chosen: [`layers`] lists
//! them with how many entities each holds, [`from_dxf_layers`] reads only the entities of the
//! layers it is given (names compared without regard to case; none given reads them all) and
//! counts the others, and [`preview`] fits what was read in a small pane to be drawn before the
//! import is made.
//!
//! Two ends that are no farther apart than the join distance (on each axis) become one point, so
//! the lines of a closed outline share their corners and the profile of the sketch is closed. A
//! line, an arc or a circle that has no length after that, or no radius, is left out and counted.
//! A circle, an arc or a polyline whose extrusion direction (group codes 210, 220 and 230) points
//! down the third axis is drawn in a mirrored system of coordinates (the arbitrary axis algorithm
//! of the format gives its first axis the sign of the first axis reversed), so its first
//! coordinates change sign, an arc swaps its two angles about the vertical and a bulge changes
//! sign; one whose direction is neither up nor down is left out and counted as `tilted`.
//! A binary DXF file is not understood. The number of lines, circles and arcs is limited by the
//! caller (`drawing.max_entities` in the settings) and the size of the file by the caller too.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use crate::drawing::DIMENSION_LAYER;
use crate::lang::{fill, Lang, Msg};
use crate::section::PreviewLine;
use crate::sketch::{ElementId, Sketch, SketchPlane};
use crate::{CadError, Length};

/// The units of a drawing that the commands and the window know, by the word that names them.
pub const UNITS: [&str; 4] = ["mm", "cm", "m", "in"];

/// The nanometres of one unit of a drawing, by the word that names the unit (`mm`, `cm`, `m` or
/// `in`); `None` for any other word.
pub fn unit_nm(word: &str) -> Option<i64> {
    match word {
        "mm" => Some(1_000_000),
        "cm" => Some(10_000_000),
        "m" => Some(1_000_000_000),
        "in" => Some(25_400_000),
        _ => None,
    }
}

/// The unit that the header of a text DXF drawing declares in `$INSUNITS`, as the word of
/// [`UNITS`] (`in` for 1, `mm` for 4, `cm` for 5 and `m` for 6); `None` when the header has no
/// such variable or declares another unit or none (0). Refused with `PHOENIX-CAD-035` when the
/// text is not a text DXF drawing.
pub fn header_unit(text: &str) -> Result<Option<&'static str>, CadError> {
    let pairs = parse_pairs(text)?;
    let value = pairs
        .windows(2)
        .find(|w| w[0] == (9, "$INSUNITS") && w[1].0 == 70)
        .and_then(|w| w[1].1.parse::<i32>().ok());
    Ok(match value {
        Some(1) => Some("in"),
        Some(4) => Some("mm"),
        Some(5) => Some("cm"),
        Some(6) => Some("m"),
        _ => None,
    })
}

/// The nanometres of one unit of a drawing whose unit was asked for as `auto`, with a line in the
/// language that says where the unit came from: the one the header declares, or millimetres when
/// it declares none.
pub fn auto_unit(text: &str, lang: Lang) -> Result<(i64, String), CadError> {
    let (word, msg) = match header_unit(text)? {
        Some(word) => (word, Msg::DxfUnitRead),
        None => ("mm", Msg::DxfUnitDefault),
    };
    Ok((
        unit_nm(word).unwrap_or(1_000_000),
        fill(msg.tr(lang), &[word]),
    ))
}

/// What was read as lines of text in the language: how many lines, circles and arcs, then one
/// line for each kind of entity that was left out.
pub fn report_text(import: &Import, lang: Lang) -> String {
    let mut out = fill(
        Msg::DxfRead.tr(lang),
        &[
            &import.lines.to_string(),
            &import.circles.to_string(),
            &import.arcs.to_string(),
        ],
    );
    for (what, count) in &import.skipped {
        out.push('\n');
        out.push_str(&fill(Msg::DxfSkipped.tr(lang), &[what, &count.to_string()]));
    }
    if import.off_layers > 0 {
        out.push('\n');
        out.push_str(&fill(
            Msg::DxfOffLayers.tr(lang),
            &[&import.off_layers.to_string()],
        ));
    }
    out
}

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
    /// How many entities were left out because they are on a layer that was not chosen (none when
    /// every layer was).
    pub off_layers: usize,
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
    /// Whether the polyline looks down the third axis: its first coordinates and bulges change
    /// sign.
    mirrored: bool,
    /// Whether its extrusion direction is neither up nor down: it is left out.
    tilted: bool,
}

impl Poly {
    /// Puts the vertices of a polyline that looks down the third axis in the world's system: the
    /// first coordinate and the bulge change sign. Done once, when the polyline is complete.
    fn mirror(&mut self) {
        if self.mirrored {
            for v in &mut self.vertices {
                v.x = -v.x;
                v.bulge = -v.bulge;
            }
            self.mirrored = false;
        }
    }
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
    off_layers: usize,
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

    /// Which way the entity of the current group looks: `Some(false)` up the third axis (the
    /// default), `Some(true)` down it (its first coordinates are mirrored) and `None` when the
    /// direction is neither.
    fn mirrored(&self) -> Result<Option<bool>, CadError> {
        let has = |code: i32| self.group.iter().any(|(c, _)| *c == code);
        let nz = if has(230) { self.first(230)? } else { 1.0 };
        if self.first(210)?.abs() > 1e-6 || self.first(220)?.abs() > 1e-6 || nz.abs() < 0.5 {
            return Ok(None);
        }
        Ok(Some(nz < 0.0))
    }

    /// The layer the current group names (`0` when it names none).
    fn layer_of_group(&self) -> String {
        self.group
            .iter()
            .find(|(c, _)| *c == 8)
            .map_or("0", |(_, v)| *v)
            .to_string()
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
        if poly.tilted {
            self.skip("POLYLINE (tilted)");
            return Ok(());
        }
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
                let Some(mirrored) = self.mirrored()? else {
                    self.skip("CIRCLE (tilted)");
                    return Ok(());
                };
                let sign = if mirrored { -1.0 } else { 1.0 };
                let (centre, radius) = ((sign * self.first(10)?, self.first(20)?), self.first(40)?);
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
                let Some(mirrored) = self.mirrored()? else {
                    self.skip("ARC (tilted)");
                    return Ok(());
                };
                let sign = if mirrored { -1.0 } else { 1.0 };
                let (centre, radius) = ((sign * self.first(10)?, self.first(20)?), self.first(40)?);
                if self.nm(radius)? <= 0 {
                    self.skip("degenerate");
                    return Ok(());
                }
                let end_of = |degrees: f64| {
                    let a = degrees * PI / 180.0;
                    (centre.0 + radius * a.cos(), centre.1 + radius * a.sin())
                };
                let (from, to) = (self.first(50)?, self.first(51)?);
                // seen in a mirror an arc goes the other way round: the angles are taken from the
                // vertical and swapped, so that it still runs counter-clockwise
                let (from, to) = if mirrored {
                    (180.0 - to, 180.0 - from)
                } else {
                    (from, to)
                };
                let (start, end) = (end_of(from), end_of(to));
                let a = self.point_at(start.0, start.1)?;
                let b = self.point_at(end.0, end.1)?;
                self.arc(centre, a, b)
            }
            "LWPOLYLINE" => {
                let direction = self.mirrored()?;
                let mut p = Poly {
                    closed: (self.first(70)? as i64) & 1 == 1,
                    vertices: Vec::new(),
                    mirrored: direction == Some(true),
                    tilted: direction.is_none(),
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
                p.mirror();
                self.polyline(&p)
            }
            "POLYLINE" => {
                let direction = self.mirrored()?;
                *poly = Some(Poly {
                    closed: (self.first(70)? as i64) & 1 == 1,
                    vertices: Vec::new(),
                    mirrored: direction == Some(true),
                    tilted: direction.is_none(),
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
                Some(mut p) => {
                    p.mirror();
                    self.polyline(&p)
                }
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
    from_dxf_layers(text, name, plane, unit_nm, join_nm, max_entities, &[])
}

/// The group codes and values of a text DXF file, in order.
fn parse_pairs(text: &str) -> Result<Vec<(i32, &str)>, CadError> {
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
    Ok(pairs)
}

/// Where the records of the section `ENTITIES` begin.
fn entities_start(pairs: &[(i32, &str)]) -> Result<usize, CadError> {
    (0..pairs.len().saturating_sub(1))
        .find(|i| pairs[*i] == (0, "SECTION") && pairs[i + 1] == (2, "ENTITIES"))
        .map(|i| i + 2)
        .ok_or_else(|| file_error("the drawing has no section of entities"))
}

/// The layers of a drawing with how many entities each holds (a polyline with its vertices is one
/// entity), by name in order. An entity that names no layer is on the layer `0`. Refused with
/// `PHOENIX-CAD-035` when the text is not a text DXF drawing or has no section of entities.
pub fn layers(text: &str) -> Result<BTreeMap<String, usize>, CadError> {
    let pairs = parse_pairs(text)?;
    let mut i = entities_start(&pairs)?;
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
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
        if name != "VERTEX" && name != "SEQEND" {
            let layer = pairs[i + 1..j]
                .iter()
                .find(|(c, _)| *c == 8)
                .map_or("0", |(_, v)| *v);
            *out.entry(layer.to_string()).or_insert(0) += 1;
        }
        i = j;
    }
    Ok(out)
}

/// The sketch that a text DXF drawing makes from the entities of some layers only (see
/// [`from_dxf`] for the rest). `chosen` names the layers, without regard to case; none named
/// reads them all but the layer of the dimensions (`COTAS`). The entities of the other layers are counted in `off_layers`. The vertices of
/// a polyline follow the layer of the polyline.
pub fn from_dxf_layers(
    text: &str,
    name: &str,
    plane: SketchPlane,
    unit_nm: i64,
    join_nm: i64,
    max_entities: usize,
    chosen: &[String],
) -> Result<Import, CadError> {
    if unit_nm <= 0 {
        return Err(file_error("the unit of the drawing is not positive"));
    }
    let pairs = parse_pairs(text)?;
    let start = entities_start(&pairs)?;
    let wanted: Vec<String> = chosen
        .iter()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty())
        .collect();
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
        off_layers: 0,
        group: &[],
    };
    let mut poly: Option<Poly> = None;
    let mut i = start;
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
        if name != "VERTEX" && name != "SEQEND" {
            let layer = reader.layer_of_group().to_lowercase();
            // with no layer chosen the dimensions of a drawing this program wrote are not
            // geometry: they are left out, and counted, like the layers that were not chosen
            let off = if wanted.is_empty() {
                layer == DIMENSION_LAYER.to_lowercase()
            } else {
                !wanted.contains(&layer)
            };
            if off {
                reader.off_layers += 1;
                i = j;
                continue;
            }
        }
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
        off_layers: reader.off_layers,
    })
}

/// What was read as lines fitted in a pane of `width` x `height` pixels with a `margin` all round,
/// to be drawn before the import is made: the drawing is scaled to fit with the same scale along
/// both axes, centred, and turned so that the second axis points up the pane. A line is one line,
/// an arc is `arc_segments` lines for a full turn (at least two for the part it covers) and a
/// circle is `arc_segments` lines too, marked `hole` so that it can be drawn in another colour. A
/// drawing with nothing in it, or a pane too small for the margin, gives no line.
pub fn preview(
    import: &Import,
    width: f64,
    height: f64,
    margin: f64,
    arc_segments: usize,
) -> Vec<PreviewLine> {
    let at = |id: ElementId| match import.sketch.element(id) {
        Some(crate::sketch::Element::Point { x, y }) => Some((x.get() as f64, y.get() as f64)),
        _ => None,
    };
    let steps = arc_segments.max(4);
    let line_of = |a: (f64, f64), b: (f64, f64), hole: bool| PreviewLine { a, b, hole };
    // the lines in the coordinates of the drawing, in nanometres, until they are fitted
    let mut world: Vec<PreviewLine> = Vec::new();
    for entry in import.sketch.elements() {
        match &entry.element {
            crate::sketch::Element::Line { a, b } => {
                if let (Some(p), Some(q)) = (at(*a), at(*b)) {
                    world.push(line_of(p, q, false));
                }
            }
            crate::sketch::Element::Circle { center, radius } => {
                if let Some(c) = at(*center) {
                    let r = radius.get() as f64;
                    let ring = |k: usize| {
                        let t = 2.0 * PI * (k % steps) as f64 / steps as f64;
                        (c.0 + r * t.cos(), c.1 + r * t.sin())
                    };
                    for k in 0..steps {
                        world.push(line_of(ring(k), ring(k + 1), true));
                    }
                }
            }
            crate::sketch::Element::Arc { center, start, end } => {
                if let (Some(c), Some(s), Some(e)) = (at(*center), at(*start), at(*end)) {
                    let r = (s.0 - c.0).hypot(s.1 - c.1);
                    let a0 = (s.1 - c.1).atan2(s.0 - c.0);
                    let mut a1 = (e.1 - c.1).atan2(e.0 - c.0);
                    if a1 <= a0 {
                        a1 += 2.0 * PI;
                    }
                    let n = (((a1 - a0) / (2.0 * PI) * steps as f64).ceil() as usize).max(2);
                    let on = |k: usize| {
                        let t = a0 + (a1 - a0) * k as f64 / n as f64;
                        (c.0 + r * t.cos(), c.1 + r * t.sin())
                    };
                    for k in 0..n {
                        world.push(line_of(on(k), on(k + 1), false));
                    }
                }
            }
            _ => {}
        }
    }
    let Some(first) = world.first() else {
        return Vec::new();
    };
    let (mut lo, mut hi) = (first.a, first.a);
    for l in &world {
        for v in [&l.a, &l.b] {
            lo = (lo.0.min(v.0), lo.1.min(v.1));
            hi = (hi.0.max(v.0), hi.1.max(v.1));
        }
    }
    let (room_w, room_h) = (width - 2.0 * margin, height - 2.0 * margin);
    if !(room_w > 0.0 && room_h > 0.0) {
        return Vec::new();
    }
    let (dx, dy) = (hi.0 - lo.0, hi.1 - lo.1);
    let scale = match (dx > 0.0, dy > 0.0) {
        (true, true) => (room_w / dx).min(room_h / dy),
        (true, false) => room_w / dx,
        (false, true) => room_h / dy,
        (false, false) => 1.0,
    };
    let (left, top) = (
        margin + (room_w - dx * scale) / 2.0,
        margin + (room_h - dy * scale) / 2.0,
    );
    let to_pane = |p: (f64, f64)| (left + (p.0 - lo.0) * scale, top + (hi.1 - p.1) * scale);
    world
        .iter()
        .map(|l| PreviewLine {
            a: to_pane(l.a),
            b: to_pane(l.b),
            hole: l.hole,
        })
        .collect()
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

    #[test]
    fn the_header_says_the_unit_of_the_drawing() {
        let with = |unit: &str| {
            format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$INSUNITS\n70\n{unit}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n")
        };
        for (code, word) in [("1", "in"), ("4", "mm"), ("5", "cm"), ("6", "m")] {
            assert_eq!(header_unit(&with(code)).unwrap(), Some(word), "{code}");
        }
        // unitless, an unknown unit and a value that is not a number say nothing
        for code in ["0", "2", "14", "x", ""] {
            assert_eq!(header_unit(&with(code)).unwrap(), None, "{code:?}");
        }
        // no header, and a variable with the wrong group code
        let none = "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
        assert_eq!(header_unit(none).unwrap(), None);
        let odd = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n40\n4\n0\nENDSEC\n";
        assert_eq!(header_unit(odd).unwrap(), None);
        // a text that is not a text DXF file is refused
        assert_eq!(
            header_unit("hello\nworld\n").unwrap_err().code(),
            "PHOENIX-CAD-035"
        );
    }

    #[test]
    fn an_automatic_unit_is_the_one_of_the_header_or_millimetres_and_says_which() {
        let header =
            |unit: &str| format!("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n{unit}\n0\nENDSEC\n");
        let (nm, note) = auto_unit(&header("1"), Lang::En).unwrap();
        assert_eq!(nm, 25_400_000);
        assert_eq!(note, "unit read from the drawing: in");
        let (nm, note) = auto_unit(&header("6"), Lang::Es).unwrap();
        assert_eq!(nm, 1_000_000_000);
        assert_eq!(note, "unidad leída del dibujo: m");
        let (nm, note) = auto_unit(&header("0"), Lang::Pt).unwrap();
        assert_eq!(nm, 1_000_000);
        assert_eq!(
            note,
            "o desenho não indica a sua unidade, por isso usa-se mm"
        );
        // every language has both lines with the unit in them
        for lang in Lang::ALL {
            for text in [header("4"), header("0")] {
                assert!(auto_unit(&text, lang).unwrap().1.contains("mm"), "{lang:?}");
            }
        }
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
        let joined = read(&text).unwrap();
        assert_eq!(profile(&joined).regions.len(), 1);
        let points = |i: &Import| {
            i.sketch
                .elements()
                .iter()
                .filter(|e| matches!(e.element, crate::sketch::Element::Point { .. }))
                .count()
        };
        assert_eq!(points(&joined), 4);
        // and with a join distance of one nanometre the last end is a point of its own
        let open = from_dxf(
            &text,
            "drawn",
            SketchPlane::on_face(Face::Front),
            MM,
            1,
            1000,
        )
        .unwrap();
        assert_eq!(points(&open), 5);
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
    fn an_arc_that_looks_down_is_mirrored_and_still_runs_counter_clockwise() {
        // the quarter of a circle of 10 mm about (5, 5) from 0 to 90 degrees and its two radii,
        // seen from below: in the world it is about (-5, 5), from 90 to 180 degrees
        let text = dxf(&[
            "0\nARC\n8\n0\n10\n5\n20\n5\n40\n10\n50\n0\n51\n90\n210\n0\n220\n0\n230\n-1\n"
                .to_string(),
            line(-5.0, 5.0, -15.0, 5.0),
            line(-5.0, 15.0, -5.0, 5.0),
        ]
        .concat());
        let i = read(&text).unwrap();
        assert_eq!((i.lines, i.arcs), (2, 1));
        let p = profile(&i);
        assert_eq!(
            p.regions.len(),
            1,
            "the arc closes the outline of the two radii"
        );
        let (lo, hi) = p.bounds();
        assert!(
            (lo.0 + 15 * MM).abs() < 6_000 && (hi.0 + 5 * MM).abs() < 6_000,
            "{lo:?} {hi:?}"
        );
        assert!(
            lo.1 == 5 * MM && (hi.1 - 15 * MM).abs() < 6_000,
            "{lo:?} {hi:?}"
        );
        // looking up, written out, gives the original arc
        let up = read(&dxf(
            "0\nARC\n8\n0\n10\n5\n20\n5\n40\n10\n50\n0\n51\n90\n230\n1\n",
        ));
        assert_eq!(up.unwrap().arcs, 1);
    }

    #[test]
    fn a_circle_and_a_polyline_that_look_down_are_mirrored_too() {
        let circle = read(&dxf("0\nCIRCLE\n8\n0\n10\n20\n20\n3\n40\n2\n230\n-1\n")).unwrap();
        assert_eq!(circle.circles, 1);
        let centre = circle
            .sketch
            .elements()
            .iter()
            .find_map(|e| match e.element {
                crate::sketch::Element::Point { x, y } => Some((x.get(), y.get())),
                _ => None,
            })
            .unwrap();
        assert_eq!(centre, (-20 * MM, 3 * MM));
        // a half disc below the chord from (0, 0) to (10, 0) turned over the vertical axis: the
        // chord runs from (0, 0) to (-10, 0) and the bulge, now clockwise in the file, still
        // bulges below
        let half = read(&dxf(
            "0\nLWPOLYLINE\n8\n0\n90\n2\n70\n1\n10\n0\n20\n0\n42\n1\n10\n10\n20\n0\n230\n-1\n",
        ))
        .unwrap();
        assert_eq!((half.lines, half.arcs), (1, 1));
        let (lo, hi) = profile(&half).bounds();
        assert!((lo.1 + 5 * MM).abs() < 6_000, "{lo:?}");
        assert_eq!((lo.0, hi.0, hi.1), (-10 * MM, 0, 0));
        // the old polyline has the direction in its own record, and its vertices follow it
        let mut text = String::from("0\nPOLYLINE\n8\n0\n66\n1\n70\n1\n230\n-1\n");
        for (x, y) in [(0, 0), (10, 0), (10, 10), (0, 10)] {
            text.push_str(&format!("0\nVERTEX\n8\n0\n10\n{x}\n20\n{y}\n"));
        }
        text.push_str("0\nSEQEND\n");
        let old = read(&dxf(&text)).unwrap();
        assert_eq!(profile(&old).bounds(), ((-10 * MM, 0), (0, 10 * MM)));
    }

    #[test]
    fn what_looks_neither_up_nor_down_is_left_out_and_counted() {
        let text = dxf(&format!(
            "{}{}{}{}",
            line(0.0, 0.0, 5.0, 0.0),
            "0\nCIRCLE\n8\n0\n10\n0\n20\n0\n40\n2\n210\n1\n220\n0\n230\n0\n",
            "0\nARC\n8\n0\n10\n0\n20\n0\n40\n2\n50\n0\n51\n90\n210\n0.5\n230\n0.8\n",
            "0\nLWPOLYLINE\n8\n0\n90\n2\n70\n0\n10\n0\n20\n0\n10\n1\n20\n1\n220\n1\n230\n0\n"
        ));
        let i = read(&text).unwrap();
        assert_eq!((i.lines, i.circles, i.arcs), (1, 0, 0));
        assert_eq!(i.skipped.get("CIRCLE (tilted)"), Some(&1));
        assert_eq!(i.skipped.get("ARC (tilted)"), Some(&1));
        assert_eq!(i.skipped.get("POLYLINE (tilted)"), Some(&1));
        // a direction that is up with a rounding error is still up
        let near = read(&dxf(
            "0\nCIRCLE\n8\n0\n10\n0\n20\n0\n40\n2\n210\n0.0000001\n220\n0\n230\n1\n",
        ))
        .unwrap();
        assert_eq!(near.circles, 1);
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

    fn on_layer(layer: &str, entities: &str) -> String {
        entities.replace("8\n0\n", &format!("8\n{layer}\n"))
    }

    fn two_layers() -> String {
        dxf(&format!(
            "{}{}{}",
            on_layer("CUT", &rectangle()),
            on_layer("Holes", &circle(20.0, 30.0, 5.0)),
            "0\nTEXT\n1\nhi\n"
        ))
    }

    #[test]
    fn the_layers_are_listed_with_their_entities_and_a_polyline_counts_once() {
        let mut poly = String::from("0\nPOLYLINE\n8\nOLD\n66\n1\n70\n1\n");
        for (x, y) in [(0, 0), (10, 0), (10, 10)] {
            poly.push_str(&format!("0\nVERTEX\n8\nOTHER\n10\n{x}\n20\n{y}\n"));
        }
        poly.push_str("0\nSEQEND\n");
        let found = layers(&dxf(&format!("{}{poly}", on_layer("CUT", &rectangle())))).unwrap();
        let listed: Vec<(&str, usize)> = found.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(listed, vec![("CUT", 4), ("OLD", 1)]);
        // an entity with no layer is on the layer 0
        let bare = layers(&dxf("0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n")).unwrap();
        assert_eq!(bare["0"], 1);
        assert_eq!(layers("0\nEOF\n").unwrap_err().code(), "PHOENIX-CAD-035");
        assert_eq!(
            layers("hello\nworld\n").unwrap_err().code(),
            "PHOENIX-CAD-035"
        );
    }

    #[test]
    fn the_dimensions_of_a_drawing_of_this_program_are_not_read_as_geometry() {
        use crate::sketch_export::render;
        let mut sketch = Sketch::new("p", SketchPlane::on_face(Face::Front)).unwrap();
        let ids: Vec<_> = [(0, 0), (40, 0), (40, 25), (0, 25)]
            .iter()
            .map(|(x, y)| {
                sketch
                    .add_point(Length::from_mm(*x).unwrap(), Length::from_mm(*y).unwrap())
                    .unwrap()
            })
            .collect();
        for i in 0..4 {
            sketch.add_line(ids[i], ids[(i + 1) % 4]).unwrap();
        }
        let settings = crate::config::Settings::defaults();
        let text = render(
            &sketch,
            &crate::sketch::settings::SketchSettings::defaults(),
            &settings.drawing,
            true,
        )
        .unwrap();
        assert!(text.contains("COTAS"));
        let drawn = read(&text).unwrap();
        // the four lines of the outline come back; the six lines and the two texts of the two
        // dimensions are counted as left out
        assert_eq!((drawn.lines, drawn.circles, drawn.arcs), (4, 0, 0));
        assert_eq!(drawn.off_layers, 8);
        // a person who names the layer asks for it: the lines of the dimensions are read
        let named = from_dxf_layers(
            &text,
            "p",
            SketchPlane::on_face(Face::Front),
            MM,
            1_000,
            1000,
            &["cotas".to_string()],
        )
        .unwrap();
        assert_eq!(named.lines, 6);
    }

    #[test]
    fn only_the_chosen_layers_are_read_and_the_rest_is_counted() {
        let text = two_layers();
        let read_of = |chosen: &[&str]| {
            let chosen: Vec<String> = chosen.iter().map(|c| c.to_string()).collect();
            from_dxf_layers(
                &text,
                "drawn",
                SketchPlane::on_face(Face::Front),
                MM,
                1_000,
                1000,
                &chosen,
            )
        };
        let all = read_of(&[]).unwrap();
        assert_eq!((all.lines, all.circles, all.off_layers), (4, 1, 0));
        assert_eq!(all, read(&text).unwrap());
        let cut = read_of(&["cut"]).unwrap();
        assert_eq!((cut.lines, cut.circles, cut.off_layers), (4, 0, 2));
        assert_eq!(profile(&cut).regions[0].holes.len(), 0);
        let holes = read_of(&[" HOLES ", ""]).unwrap();
        assert_eq!((holes.lines, holes.circles, holes.off_layers), (0, 1, 5));
        let both = read_of(&["CUT", "Holes"]).unwrap();
        assert_eq!((both.lines, both.circles, both.off_layers), (4, 1, 1));
        // a layer that is not in the drawing leaves nothing to read
        let none = read_of(&["missing"]).unwrap_err();
        assert_eq!(none.code(), "PHOENIX-CAD-035");
        // the vertices follow their polyline, whatever layer they name
        let mut poly = String::from("0\nPOLYLINE\n8\nKEEP\n66\n1\n70\n1\n");
        for (x, y) in [(0, 0), (10, 0), (10, 10)] {
            poly.push_str(&format!("0\nVERTEX\n8\nOTHER\n10\n{x}\n20\n{y}\n"));
        }
        poly.push_str("0\nSEQEND\n");
        let kept = from_dxf_layers(
            &dxf(&poly),
            "drawn",
            SketchPlane::on_face(Face::Front),
            MM,
            1_000,
            1000,
            &["keep".to_string()],
        )
        .unwrap();
        assert_eq!(kept.lines, 3);
        let dropped = from_dxf_layers(
            &dxf(&format!("{poly}{}", on_layer("KEEP", &rectangle()))),
            "drawn",
            SketchPlane::on_face(Face::Front),
            MM,
            1_000,
            1000,
            &["other".to_string()],
        )
        .unwrap_err();
        assert_eq!(dropped.code(), "PHOENIX-CAD-035");
    }

    #[test]
    fn the_report_tells_how_many_entities_the_layers_left_out() {
        let chosen = vec!["CUT".to_string()];
        let i = from_dxf_layers(
            &two_layers(),
            "drawn",
            SketchPlane::on_face(Face::Front),
            MM,
            1_000,
            1000,
            &chosen,
        )
        .unwrap();
        assert_eq!(
            report_text(&i, Lang::En),
            "read: 4 lines, 0 circles, 0 arcs\nleft out by layer: 2 entities"
        );
        assert!(report_text(&i, Lang::Pt).contains("camada"));
    }

    #[test]
    fn the_preview_fits_the_drawing_in_the_pane_with_the_first_axis_to_the_right() {
        let i = read(&dxf(&rectangle())).unwrap();
        // a rectangle of 100 x 60 mm in a pane of 120 x 120 with a margin of 10: 100 mm over 100 px
        let lines = preview(&i, 120.0, 120.0, 10.0, 16);
        assert_eq!(lines.len(), 4);
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        let xs: Vec<f64> = lines.iter().flat_map(|l| [l.a.0, l.b.0]).collect();
        let ys: Vec<f64> = lines.iter().flat_map(|l| [l.a.1, l.b.1]).collect();
        let min = |v: &[f64]| v.iter().cloned().fold(f64::MAX, f64::min);
        let max = |v: &[f64]| v.iter().cloned().fold(f64::MIN, f64::max);
        assert!(near(min(&xs), 10.0) && near(max(&xs), 110.0));
        // 60 mm tall: centred in the 100 px of room, from 30 to 90
        assert!(near(min(&ys), 30.0) && near(max(&ys), 90.0));
        assert!(lines.iter().all(|l| !l.hole));
        // the corner at the origin is at the lower left: the largest y of the pane
        assert!(lines.iter().any(|l| near(l.a.0, 10.0) && near(l.a.1, 90.0)));
        // a pane too small, and a drawing of nothing
        assert!(preview(&i, 15.0, 120.0, 10.0, 16).is_empty());
        let blank = Import {
            sketch: Sketch::new("blank", SketchPlane::on_face(Face::Front)).unwrap(),
            lines: 0,
            circles: 0,
            arcs: 0,
            skipped: BTreeMap::new(),
            off_layers: 0,
        };
        assert!(preview(&blank, 120.0, 120.0, 10.0, 16).is_empty());
    }

    #[test]
    fn a_circle_and_an_arc_are_drawn_as_short_lines() {
        let circle_only = read(&dxf(&circle(0.0, 0.0, 10.0))).unwrap();
        let ring = preview(&circle_only, 100.0, 100.0, 0.0, 12);
        assert_eq!(ring.len(), 12);
        assert!(ring.iter().all(|l| l.hole));
        // the ring fills the pane: 20 mm over 100 px
        let widest = ring
            .iter()
            .flat_map(|l| [l.a.0, l.b.0])
            .fold(f64::MIN, f64::max);
        assert!((widest - 100.0).abs() < 1e-6, "{widest}");
        // a quarter of a circle is a quarter of the lines (rounded up, two at least)
        let quarter = read(&dxf("0\nARC\n8\n0\n10\n5\n20\n5\n40\n10\n50\n0\n51\n90\n")).unwrap();
        let arc = preview(&quarter, 100.0, 100.0, 5.0, 16);
        assert_eq!(arc.len(), 4);
        assert!(arc.iter().all(|l| !l.hole));
        // chained: the end of one line is the start of the next
        for pair in arc.windows(2) {
            assert!((pair[0].b.0 - pair[1].a.0).abs() < 1e-9);
            assert!((pair[0].b.1 - pair[1].a.1).abs() < 1e-9);
        }
    }

    #[test]
    fn the_units_are_named_by_a_word_and_the_report_is_in_the_language() {
        assert_eq!(unit_nm("mm"), Some(1_000_000));
        assert_eq!(unit_nm("cm"), Some(10_000_000));
        assert_eq!(unit_nm("m"), Some(1_000_000_000));
        assert_eq!(unit_nm("in"), Some(25_400_000));
        assert_eq!(unit_nm("ft"), None);
        assert!(UNITS.iter().all(|u| unit_nm(u).is_some()));
        let text = dxf(&format!("{}0\nTEXT\n8\n0\n1\nhello\n", rectangle()));
        let imported = read(&text).unwrap();
        assert_eq!(
            report_text(&imported, Lang::En),
            "read: 4 lines, 0 circles, 0 arcs\nleft out: TEXT x 1"
        );
        assert_eq!(
            report_text(&imported, Lang::Es),
            "leído: 4 líneas, 0 círculos, 0 arcos\nomitido: TEXT x 1"
        );
    }
}
