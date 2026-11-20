// SPDX-License-Identifier: GPL-3.0-or-later
//! A section of the model: the outline where a plane across one of the axes cuts the solids, as a
//! flat drawing in a DXF or SVG file and as a report.
//!
//! The section is made from the triangles of every solid of the model (the box with everything on
//! it, the placed parts and the turned and swept bodies, see [`crate::solid::bodies`]). A vertex
//! counts as above the plane when its coordinate is not below it, so a plane that passes exactly
//! through vertices cuts them consistently; an edge of a triangle whose ends are on different
//! sides gives one point, named by the pair of vertices of the edge, and the two points of a
//! triangle make a segment. Because the surfaces are closed, the segments join end to end by those
//! names, with no comparison of coordinates, into closed outlines. The segment of a triangle runs
//! from the edge where the walk round the triangle (counter-clockwise from outside) goes down
//! through the plane to the one where it goes up, which keeps the material on its left, so an outer outline turns counter-
//! clockwise and a hole clockwise in the plane of the drawing, and the signed areas add up to the
//! area of the material.
//!
//! The drawing has the first two axes that the plane leaves, in this order: `y` and `z` for a
//! plane across `x`, `x` and `z` for a plane across `y`, and `x` and `y` for a plane across `z`.
//! Everything is in millimetres in the files; the coordinates of the points are nanometres.

use std::collections::{BTreeMap, BTreeSet};

use crate::assembly::Dim;
use crate::config::DrawingSettings;
use crate::drawing::{Drawing, Entity, Layer, View};
use crate::lang::{fill, Lang, Msg};
use crate::mesh::Mesh;
use crate::revolve::scaled;
use crate::scad::mm_text;
use crate::solid::Body;
use crate::titleblock::TitleBlockSettings;
use crate::{CadError, Length};

/// The name of an intersection point: the two vertices of the edge it lies on, the smaller first.
type EdgeKey = (u32, u32);

/// One closed outline of a section.
#[derive(Debug, Clone, PartialEq)]
pub struct Contour {
    /// The name of the solid that it belongs to.
    pub body: String,
    /// The material of that solid; empty for the box.
    pub material: String,
    /// The corners, in nanometres in the plane of the drawing; the last joins the first.
    pub points: Vec<(f64, f64)>,
    /// The area it holds, in square nanometres: positive for an outer outline of material and
    /// negative for a hole.
    pub area_nm2: f64,
}

/// The section of a model by a plane.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// The axis across the plane.
    pub axis: Dim,
    /// The coordinate of the plane along the axis, in nanometres.
    pub position: Length,
    /// The outlines, solid by solid.
    pub contours: Vec<Contour>,
}

/// The closed outlines where the plane `axis = position` cuts the closed surface of a mesh, as
/// lists of corners in the plane of the drawing (see the module documentation).
fn cut_mesh(mesh: &Mesh, axis: usize, position: f64) -> Vec<Vec<(f64, f64)>> {
    let (iu, iv) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let mut next: BTreeMap<EdgeKey, EdgeKey> = BTreeMap::new();
    let mut at: BTreeMap<EdgeKey, (f64, f64)> = BTreeMap::new();
    for t in &mesh.triangles {
        let corners = mesh.corners(t);
        let above = corners.map(|c| c[axis] >= position);
        if above[0] == above[1] && above[1] == above[2] {
            continue;
        }
        // the edges that cross, as the point where they meet the plane, the name of the point and
        // whether the walk round the triangle goes down through the plane there
        let mut down: Option<(EdgeKey, (f64, f64))> = None;
        let mut up: Option<(EdgeKey, (f64, f64))> = None;
        for (i, j) in [(0usize, 1usize), (1, 2), (2, 0)] {
            if above[i] == above[j] {
                continue;
            }
            let (a, b) = (corners[i], corners[j]);
            // a vertex that lies on the plane is the point itself, exactly
            let (u, v) = if a[axis] == position {
                (a[iu], a[iv])
            } else if b[axis] == position {
                (b[iu], b[iv])
            } else {
                let s = (position - a[axis]) / (b[axis] - a[axis]);
                (a[iu] + s * (b[iu] - a[iu]), a[iv] + s * (b[iv] - a[iv]))
            };
            let key = if t[i] < t[j] {
                (t[i], t[j])
            } else {
                (t[j], t[i])
            };
            if above[i] {
                down = Some((key, (u, v)));
            } else {
                up = Some((key, (u, v)));
            }
        }
        // the triangle is counter-clockwise seen from outside, so the material is on the left of
        // the segment that goes from the edge where the walk goes down to the one where it goes
        // up; this holds when the segment has no length too, which the coordinates cannot tell
        let (Some(from), Some(to)) = (down, up) else {
            continue;
        };
        next.insert(from.0, to.0);
        at.insert(from.0, from.1);
        at.insert(to.0, to.1);
    }
    let mut used: BTreeSet<EdgeKey> = BTreeSet::new();
    let mut loops = Vec::new();
    for start in next.keys() {
        if used.contains(start) {
            continue;
        }
        let mut points: Vec<(f64, f64)> = Vec::new();
        let mut current = *start;
        while used.insert(current) {
            points.push(at[&current]);
            match next.get(&current) {
                Some(n) => current = *n,
                None => break,
            }
        }
        points.dedup();
        if points.first() == points.last() && points.len() > 1 {
            points.pop();
        }
        if points.len() >= 3 {
            loops.push(points);
        }
    }
    loops
}

/// The list of corners without the ones that lie on the straight line between their neighbours
/// (the diagonal of a wall that was cut in two triangles leaves one).
fn simplify(mut p: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    let mut i = 0;
    while p.len() > 3 && i < p.len() {
        let n = p.len();
        let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
        let (u, v) = ((b.0 - a.0, b.1 - a.1), (c.0 - b.0, c.1 - b.1));
        let cross = u.0 * v.1 - u.1 * v.0;
        let dot = u.0 * v.0 + u.1 * v.1;
        let scale = u.0.hypot(u.1) * v.0.hypot(v.1);
        if dot > 0.0 && cross.abs() <= 1e-9 * scale {
            p.remove(i);
        } else {
            i += 1;
        }
    }
    p
}

/// The area that a closed list of corners holds, positive when it turns counter-clockwise.
fn signed_area(points: &[(f64, f64)]) -> f64 {
    let mut twice = 0.0;
    for (i, p) in points.iter().enumerate() {
        let q = points[(i + 1) % points.len()];
        twice += p.0 * q.1 - q.0 * p.1;
    }
    twice / 2.0
}

/// The section of some solids by the plane across `axis` at `position`.
pub fn cut(bodies: &[Body], axis: Dim, position: Length) -> Section {
    let k = axis.index();
    // the plane across y is drawn with x to the right and z up, which turns the other way
    let handed = if axis == Dim::Y { -1.0 } else { 1.0 };
    let mut contours = Vec::new();
    for body in bodies {
        for points in cut_mesh(&body.mesh, k, position.get() as f64) {
            let points = simplify(points);
            let area_nm2 = handed * signed_area(&points);
            contours.push(Contour {
                body: body.name.clone(),
                material: body.material.clone(),
                points,
                area_nm2,
            });
        }
    }
    Section {
        axis,
        position,
        contours,
    }
}

impl Section {
    /// The area of the material in the plane, in square nanometres.
    pub fn area_nm2(&self) -> f64 {
        self.contours.iter().map(|c| c.area_nm2).sum()
    }

    /// The number of line segments of all the outlines.
    pub fn segments(&self) -> usize {
        self.contours.iter().map(|c| c.points.len()).sum()
    }
}

/// The hatching of a section: lines at 45 degrees, `spacing_mm` apart measured across them,
/// clipped to the material (the area inside an outer outline and outside its holes, by the
/// even-odd rule over all the outlines, so a hole is not hatched and a part inside a hole is).
/// Refused with `PHOENIX-CAD-050` when the lines would be more than `limit`.
pub fn hatch(
    section: &Section,
    spacing_mm: f64,
    limit: usize,
) -> Result<Vec<(Layer, Entity)>, CadError> {
    let mm = |v: f64| v / 1_000_000.0;
    if spacing_mm <= 0.0 || section.contours.is_empty() {
        return Ok(Vec::new());
    }
    // lines of constant s = x + y, one step apart: the step is the spacing across the lines
    let step = spacing_mm * std::f64::consts::SQRT_2;
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for p in section.contours.iter().flat_map(|c| &c.points) {
        let s = mm(p.0) + mm(p.1);
        lo = lo.min(s);
        hi = hi.max(s);
    }
    let first = (lo / step).floor() as i64 + 1;
    let last = (hi / step).ceil() as i64 - 1;
    if last < first {
        return Ok(Vec::new());
    }
    if (last - first + 1) as usize > limit {
        return Err(CadError::ExportLimit(format!(
            "the hatching would have {} lines, more than the {limit} the settings allow",
            last - first + 1
        )));
    }
    let mut out = Vec::new();
    for n in first..=last {
        let s = n as f64 * step;
        // where the line crosses an edge of an outline; an edge counts when its ends are on
        // different sides, the lower end being on the line's side (half open), so a corner on
        // the line is counted once or twice consistently
        let mut crossings: Vec<f64> = Vec::new();
        for c in &section.contours {
            for (i, p) in c.points.iter().enumerate() {
                let q = c.points[(i + 1) % c.points.len()];
                let (a, b) = (mm(p.0) + mm(p.1), mm(q.0) + mm(q.1));
                if (a <= s) == (b <= s) {
                    continue;
                }
                let t = (s - a) / (b - a);
                crossings.push(mm(p.0) + t * (mm(q.0) - mm(p.0)));
            }
        }
        crossings.sort_by(f64::total_cmp);
        for pair in crossings.windows(2).step_by(2) {
            if pair[1] - pair[0] > 1e-9 {
                out.push((
                    Layer::Hatch,
                    Entity::Line((pair[0], s - pair[0]), (pair[1], s - pair[1])),
                ));
            }
        }
        if out.len() > limit {
            return Err(CadError::ExportLimit(format!(
                "the hatching has more than the {limit} lines the settings allow"
            )));
        }
    }
    Ok(out)
}

/// The lines of one family of hatching: parallel lines `angle_deg` degrees from the first axis of
/// the drawing, `spacing_mm` apart measured across them, clipped to the material of `contours` by
/// the even-odd rule, as [`hatch`] does for 135 degrees. Refused with `PHOENIX-CAD-050` when the
/// lines would be more than `limit`.
fn hatch_lines(
    contours: &[&Contour],
    angle_deg: f64,
    spacing_mm: f64,
    limit: usize,
) -> Result<Vec<(Layer, Entity)>, CadError> {
    let mm = |v: f64| v / 1_000_000.0;
    if spacing_mm <= 0.0 || contours.is_empty() {
        return Ok(Vec::new());
    }
    let (dy, dx) = angle_deg.to_radians().sin_cos();
    let (nx, ny) = (-dy, dx);
    let across = |p: (f64, f64)| mm(p.0) * nx + mm(p.1) * ny;
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for p in contours.iter().flat_map(|c| &c.points) {
        let t = across(*p);
        lo = lo.min(t);
        hi = hi.max(t);
    }
    let first = (lo / spacing_mm).floor() as i64 + 1;
    let last = (hi / spacing_mm).ceil() as i64 - 1;
    if last < first {
        return Ok(Vec::new());
    }
    if (last - first + 1) as usize > limit {
        return Err(CadError::ExportLimit(format!(
            "the hatching would have {} lines, more than the {limit} the settings allow",
            last - first + 1
        )));
    }
    let mut out = Vec::new();
    for n in first..=last {
        let t = n as f64 * spacing_mm;
        // where the line meets an edge of an outline, as the distance along the line; an edge
        // counts when its ends are on different sides, the lower end being on the line's side
        let mut crossings: Vec<f64> = Vec::new();
        for c in contours {
            for (i, p) in c.points.iter().enumerate() {
                let q = c.points[(i + 1) % c.points.len()];
                let (a, b) = (across(*p), across(q));
                if (a <= t) == (b <= t) {
                    continue;
                }
                let u = (t - a) / (b - a);
                let x = mm(p.0) + u * (mm(q.0) - mm(p.0));
                let y = mm(p.1) + u * (mm(q.1) - mm(p.1));
                crossings.push(x * dx + y * dy);
            }
        }
        crossings.sort_by(f64::total_cmp);
        for pair in crossings.windows(2).step_by(2) {
            if pair[1] - pair[0] > 1e-9 {
                let at = |w: f64| (w * dx + t * nx, w * dy + t * ny);
                out.push((Layer::Hatch, Entity::Line(at(pair[0]), at(pair[1]))));
            }
        }
        if out.len() > limit {
            return Err(CadError::ExportLimit(format!(
                "the hatching has more than the {limit} lines the settings allow"
            )));
        }
    }
    Ok(out)
}

/// The materials of a section in the order of their names (the box, which has none, first) and
/// the angle of the hatching of each: the angles of the settings in turn, starting again when
/// there are more materials than angles.
pub fn hatch_key(section: &Section, angles: &[u16; 8]) -> Vec<(String, u16)> {
    let names: BTreeSet<&str> = section
        .contours
        .iter()
        .map(|c| c.material.as_str())
        .collect();
    names
        .into_iter()
        .enumerate()
        .map(|(k, m)| (m.to_string(), angles[k % angles.len()]))
        .collect()
}

/// The hatching of a section with an angle for each material (see [`hatch_key`]), so that the
/// parts of a drawing can be told apart: each material is hatched on its own, by the even-odd rule
/// over its own outlines. The lines of all of them together are limited to `limit`.
pub fn hatch_by_material(
    section: &Section,
    angles: &[u16; 8],
    spacing_mm: f64,
    limit: usize,
) -> Result<Vec<(Layer, Entity)>, CadError> {
    let mut out = Vec::new();
    for (material, angle) in hatch_key(section, angles) {
        let group: Vec<&Contour> = section
            .contours
            .iter()
            .filter(|c| c.material == material)
            .collect();
        let room = limit.saturating_sub(out.len());
        out.extend(hatch_lines(&group, f64::from(angle), spacing_mm, room)?);
    }
    Ok(out)
}

/// The drawing of a section: every outline as lines on the layer `CONTORNO`, in millimetres.
pub fn drawing_of(section: &Section) -> Drawing {
    let mm = |v: f64| v / 1_000_000.0;
    let mut entities = Vec::new();
    for c in &section.contours {
        for (i, p) in c.points.iter().enumerate() {
            let q = c.points[(i + 1) % c.points.len()];
            entities.push((
                Layer::Outline,
                Entity::Line((mm(p.0), mm(p.1)), (mm(q.0), mm(q.1))),
            ));
        }
    }
    let mut drawing = Drawing {
        view: View::Top,
        entities,
        notes: Vec::new(),
        extent: (0.0, 0.0),
        dxf_dimensions: false,
    };
    if !drawing.entities.is_empty() {
        let (lo, hi) = drawing.bounds();
        drawing.extent = (hi.0 - lo.0, hi.1 - lo.1);
    }
    drawing
}

/// One line of the small picture of a section, in the pixels of the pane it is drawn in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewLine {
    /// The start of the line.
    pub a: (f64, f64),
    /// The end of the line.
    pub b: (f64, f64),
    /// True when the line belongs to a hole (an outline that turns clockwise), false for the
    /// outer outline of the material.
    pub hole: bool,
}

/// The outlines of a section as lines fitted in a pane of `width` x `height` pixels with a
/// `margin` all round: the drawing is scaled to fit with the same scale along both axes, centred,
/// and turned so that the second axis of the drawing points up the pane. A section with no
/// outline, or a pane too small for the margin, gives no line.
pub fn preview(section: &Section, width: f64, height: f64, margin: f64) -> Vec<PreviewLine> {
    let points = || section.contours.iter().flat_map(|c| c.points.iter());
    let Some(first) = points().next() else {
        return Vec::new();
    };
    let (mut lo, mut hi) = (*first, *first);
    for p in points() {
        lo = (lo.0.min(p.0), lo.1.min(p.1));
        hi = (hi.0.max(p.0), hi.1.max(p.1));
    }
    let (room_w, room_h) = (width - 2.0 * margin, height - 2.0 * margin);
    if !(room_w > 0.0 && room_h > 0.0) {
        return Vec::new();
    }
    let (dx, dy) = (hi.0 - lo.0, hi.1 - lo.1);
    // a section that is a single point or a line still gets a scale
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
    let at = |p: (f64, f64)| (left + (p.0 - lo.0) * scale, top + (hi.1 - p.1) * scale);
    let mut out = Vec::new();
    for c in &section.contours {
        for (i, p) in c.points.iter().enumerate() {
            let q = c.points[(i + 1) % c.points.len()];
            out.push(PreviewLine {
                a: at(*p),
                b: at(q),
                hole: c.area_nm2 < 0.0,
            });
        }
    }
    out
}

/// The text of the DXF or SVG file of a section, with a title block under the drawing when
/// `title.enabled` is true (its title is `name` when the settings give none). Refused with
/// `PHOENIX-CAD-041` when the plane does not cut the model and with `PHOENIX-CAD-050` when the
/// drawing has more lines than `drawing.max_entities`.
pub fn render_titled(
    section: &Section,
    drawing: &DrawingSettings,
    dxf: bool,
    title: &TitleBlockSettings,
    name: &str,
    lang: Lang,
) -> Result<String, CadError> {
    let d = hatched_drawing(section, drawing)?;
    Ok(match (title.enabled, dxf) {
        (false, true) => d.to_dxf(),
        (false, false) => d.to_svg(drawing),
        (true, true) => {
            let block = d.title_block(title, name, "1:1", 6.0, lang);
            d.to_dxf_with(Some(&block))
        }
        (true, false) => d.to_svg_with(drawing, Some((title, name, lang))),
    })
}

/// The drawing of a section with its hatching: the outlines on the layer `CONTORNO` and the lines
/// of the hatch (by material when `drawing.hatch_by_material` is 1) in the spacing of the settings.
/// Refused with `PHOENIX-CAD-041` when the plane does not cut the model and with `PHOENIX-CAD-050`
/// when the drawing has more lines than `drawing.max_entities`.
pub fn hatched_drawing(section: &Section, drawing: &DrawingSettings) -> Result<Drawing, CadError> {
    let mut d = drawing_of(section);
    if d.entities.is_empty() {
        return Err(CadError::Export(
            "the plane does not cut the model, so there is nothing to draw".into(),
        ));
    }
    if d.entities.len() > drawing.max_entities as usize {
        return Err(CadError::ExportLimit(format!(
            "the section has {} lines, more than the {} the settings allow",
            d.entities.len(),
            drawing.max_entities
        )));
    }
    // the hatching comes after the outlines and counts with them against the limit
    let room = drawing.max_entities as usize - d.entities.len();
    let spacing = drawing.hatch_spacing.get() as f64 / 1_000_000.0;
    let hatching = if drawing.hatch_by_material {
        hatch_by_material(section, &drawing.hatch_angles, spacing, room)?
    } else {
        hatch(section, spacing, room)?
    };
    d.entities.extend(hatching);
    Ok(d)
}

/// The report of a section as lines of text in a language: the plane, how many outlines and
/// segments, the area of the material and, solid by solid, the outlines and the area.
pub fn text_in(section: &Section, lang: Lang) -> String {
    let area = |nm2: f64| scaled(nm2.round() as i128, 1_000_000_000_000);
    let mut lines = vec![fill(
        Msg::SecPlane.tr(lang),
        &[section.axis.name(), &mm_text(section.position.get())],
    )];
    if section.contours.is_empty() {
        lines.push(Msg::SecEmpty.tr(lang).to_string());
    } else {
        lines.push(fill(
            Msg::SecOutlines.tr(lang),
            &[
                &section.contours.len().to_string(),
                &section.segments().to_string(),
            ],
        ));
        lines.push(fill(Msg::SecArea.tr(lang), &[&area(section.area_nm2())]));
        let mut per_body: BTreeMap<&str, (usize, f64)> = BTreeMap::new();
        for c in &section.contours {
            let entry = per_body.entry(c.body.as_str()).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += c.area_nm2;
        }
        for (name, (count, nm2)) in per_body {
            lines.push(fill(
                Msg::SecBody.tr(lang),
                &[name, &count.to_string(), &area(nm2)],
            ));
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::enclosure::{Enclosure, EnclosureParams, Face, Operation};
    use crate::sketch::{Sketch, SketchPlane};
    use crate::solid::bodies;
    use eh3d_container::ObjectId;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn model() -> Enclosure {
        Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap()
    }

    fn section_of(e: &Enclosure, axis: Dim, at: i64) -> Section {
        let settings = Settings::defaults();
        cut(&bodies(e, &settings.mesh).unwrap(), axis, mm(at))
    }

    fn square_mm2(nm2: f64) -> f64 {
        nm2 / 1e12
    }

    #[test]
    fn the_preview_fits_the_outlines_in_the_pane_keeping_the_proportions_and_marks_the_holes() {
        // 104 x 64 mm outside and 100 x 60 mm inside, cut at half the height
        let s = section_of(&model(), Dim::Z, 15);
        let lines = preview(&s, 220.0, 200.0, 10.0);
        assert_eq!(lines.len(), s.segments());
        assert_eq!(lines.iter().filter(|l| l.hole).count(), 4);
        // the wide side fills the room: 200 pixels for 104 mm
        let xs: Vec<f64> = lines.iter().flat_map(|l| [l.a.0, l.b.0]).collect();
        let ys: Vec<f64> = lines.iter().flat_map(|l| [l.a.1, l.b.1]).collect();
        let span = |v: &[f64]| {
            v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min)
        };
        assert!((span(&xs) - 200.0).abs() < 1e-6, "{}", span(&xs));
        assert!(
            (span(&ys) - 200.0 * 64.0 / 104.0).abs() < 1e-6,
            "{}",
            span(&ys)
        );
        assert!(xs.iter().all(|x| (10.0..=210.0).contains(x)));
        assert!(ys.iter().all(|y| (10.0..=190.0).contains(y)));
        // the outer outline is outside the holes in the pane, and a plane that cuts nothing gives
        // no line
        let outer_left = lines
            .iter()
            .filter(|l| !l.hole)
            .map(|l| l.a.0)
            .fold(f64::MAX, f64::min);
        let hole_left = lines
            .iter()
            .filter(|l| l.hole)
            .map(|l| l.a.0)
            .fold(f64::MAX, f64::min);
        assert!(outer_left < hole_left);
        assert!(preview(&section_of(&model(), Dim::X, 500), 220.0, 200.0, 10.0).is_empty());
        // a pane with no room for the margin draws nothing
        assert!(preview(&s, 15.0, 200.0, 10.0).is_empty());
        // a plane across x is 64 mm wide and 35 mm tall: the wide side fills the room and the
        // drawing is centred in the pane
        let upright = section_of(&model(), Dim::X, 50);
        let up = preview(&upright, 220.0, 200.0, 10.0);
        let rows: Vec<f64> = up.iter().flat_map(|l| [l.a.1, l.b.1]).collect();
        let (low, high) = (
            rows.iter().cloned().fold(f64::MAX, f64::min),
            rows.iter().cloned().fold(f64::MIN, f64::max),
        );
        assert!(
            (high - low - 200.0 * 35.0 / 64.0).abs() < 1e-6,
            "{}",
            high - low
        );
        assert!(((low + high) / 2.0 - 100.0).abs() < 1e-6);
    }

    #[test]
    fn the_wall_of_the_box_is_a_frame_of_material_in_a_horizontal_section() {
        // the box is 104 x 64 mm outside and 100 x 60 mm inside: 6656 - 6000 = 656 mm2 of wall
        let s = section_of(&model(), Dim::Z, 15);
        assert_eq!(s.contours.len(), 2);
        let mut areas: Vec<f64> = s.contours.iter().map(|c| square_mm2(c.area_nm2)).collect();
        areas.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((areas[0] + 6000.0).abs() < 0.001, "{areas:?}");
        assert!((areas[1] - 6656.0).abs() < 0.001, "{areas:?}");
        assert!((square_mm2(s.area_nm2()) - 656.0).abs() < 0.001);
        assert_eq!(s.segments(), 8);
    }

    fn length(e: &Entity) -> f64 {
        match *e {
            Entity::Line(a, b) => ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt(),
            _ => 0.0,
        }
    }

    #[test]
    fn the_material_of_a_section_is_hatched_at_45_degrees_and_the_hole_is_not() {
        // the frame of the box: 104 x 64 mm outside, 100 x 60 mm inside, from the corner (0, 0)
        let s = section_of(&model(), Dim::Z, 15);
        let lines = hatch(&s, 2.0, 100_000).unwrap();
        assert!(lines.len() > 50, "{}", lines.len());
        let mut total = 0.0;
        for (layer, e) in &lines {
            assert_eq!(*layer, Layer::Hatch);
            let Entity::Line(a, b) = *e else {
                panic!("not a line: {e:?}");
            };
            // every line runs at 45 degrees: x + y is the same at both ends
            assert!((a.0 + a.1 - (b.0 + b.1)).abs() < 1e-6, "{e:?}");
            let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            // inside the outer outline and outside the hole (the space inside the walls)
            assert!(
                mid.0 > 0.0 && mid.0 < 104.0 && mid.1 > 0.0 && mid.1 < 64.0,
                "{e:?}"
            );
            let in_cavity = mid.0 > 2.0 && mid.0 < 102.0 && mid.1 > 2.0 && mid.1 < 62.0;
            assert!(!in_cavity, "{e:?}");
            total += length(e);
        }
        // lines 2 mm apart across a material of 656 mm2 are about 328 mm long together
        assert!((total - 328.0).abs() < 33.0, "{total}");
        // a larger spacing has fewer lines, and none at all with a spacing of nothing
        let sparse = hatch(&s, 8.0, 100_000).unwrap();
        assert!(sparse.len() < lines.len() / 2);
        assert!(hatch(&s, 0.0, 100_000).unwrap().is_empty());
        // a section that cuts nothing has nothing to hatch
        let none = section_of(&model(), Dim::Z, 500);
        assert!(hatch(&none, 2.0, 100_000).unwrap().is_empty());
        // more lines than the limit are refused
        let err = hatch(&s, 0.3, 10).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-050");
        // the same section always gives the same lines
        assert_eq!(hatch(&s, 2.0, 100_000).unwrap(), lines);
    }

    #[test]
    fn the_hatching_is_a_layer_of_the_files_that_the_settings_turn_off() {
        let s = section_of(&model(), Dim::Z, 15);
        let settings = Settings::defaults();
        let off = TitleBlockSettings::off();
        let dxf = render_titled(&s, &settings.drawing, true, &off, "box", Lang::En).unwrap();
        assert!(dxf.contains("2\nLAYER\n70\n4\n"), "{dxf}");
        assert!(dxf.contains("2\nRAYADO\n70\n0\n62\n8\n"));
        let hatched = dxf.matches("0\nLINE\n8\nRAYADO\n").count();
        assert!(hatched > 50, "{hatched}");
        assert_eq!(dxf.matches("0\nLINE\n8\nCONTORNO\n").count(), 8);
        let svg = render_titled(&s, &settings.drawing, false, &off, "box", Lang::En).unwrap();
        assert!(svg.contains("<g id=\"RAYADO\""), "{svg}");
        // the hatching counts against the limit of lines of the drawing
        let mut tight = settings.drawing;
        tight.max_entities = 40;
        let err = render_titled(&s, &tight, true, &off, "box", Lang::En).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-050");
        // with a spacing of nothing the files are the ones they were: the outline alone
        let mut bare = settings.drawing;
        bare.hatch_spacing = Length::ZERO;
        let plain = render_titled(&s, &bare, true, &off, "box", Lang::En).unwrap();
        assert!(!plain.contains("RAYADO"));
        assert!(plain.contains("2\nLAYER\n70\n3\n"));
        let plain_svg = render_titled(&s, &bare, false, &off, "box", Lang::En).unwrap();
        assert!(!plain_svg.contains("RAYADO"));
    }

    #[test]
    fn the_other_two_planes_turn_the_same_way() {
        // across x: the floor, the lid and the two walls; the box is 64 wide and 35 tall outside
        // and 60 by 30 inside: 64 x 35 - 60 x 30 = 440 mm2
        let s = section_of(&model(), Dim::X, 50);
        assert!(
            (square_mm2(s.area_nm2()) - 440.0).abs() < 0.001,
            "{}",
            s.area_nm2()
        );
        // across y: 104 x 35 - 100 x 30 = 640 mm2
        let s = section_of(&model(), Dim::Y, 30);
        assert!(
            (square_mm2(s.area_nm2()) - 640.0).abs() < 0.001,
            "{}",
            s.area_nm2()
        );
        assert!(s.contours.iter().any(|c| c.area_nm2 > 0.0));
    }

    #[test]
    fn a_plane_outside_the_model_cuts_nothing() {
        let s = section_of(&model(), Dim::Z, 500);
        assert!(s.contours.is_empty());
        assert_eq!(s.segments(), 0);
        let text = text_in(&s, Lang::En);
        assert!(text.contains("does not cut"), "{text}");
        let settings = Settings::defaults();
        let e = render_titled(
            &s,
            &settings.drawing,
            true,
            &TitleBlockSettings::off(),
            "box",
            Lang::En,
        )
        .unwrap_err();
        assert!(e.to_string().contains("nothing to draw"), "{e}");
    }

    #[test]
    fn a_hole_through_a_wall_opens_the_outline() {
        // a hole of 10 mm in the front wall at the height of the plane: the wall of 2 mm has a
        // gap of 10 mm in the section, so the area is 20 mm2 less
        let mut e = model();
        e.apply(Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, 1, 1).unwrap(),
            hole: crate::enclosure::Hole {
                name: "h".into(),
                face: Face::Front,
                u: mm(52),
                v: mm(17),
                diameter: mm(10),
                style: crate::enclosure::HoleStyle::Plain,
            },
        })
        .unwrap();
        // the hole is centred 17 mm above the lower edge of the face: the plane z = 17 mm from the
        // floor goes through its middle, which is 2 + 15 = 17 mm up from the underside
        let whole = section_of(&model(), Dim::Z, 17);
        let holed = section_of(&e, Dim::Z, 17);
        let lost = square_mm2(whole.area_nm2() - holed.area_nm2());
        assert!((lost - 20.0).abs() < 0.5, "{lost}");
    }

    #[test]
    fn a_turned_body_is_cut_as_its_profile_on_both_sides_of_its_axis() {
        use crate::turned::TurnedSpec;
        let mut e = model();
        let mut sketch = Sketch::new("ring", SketchPlane::on_face(Face::Front)).unwrap();
        sketch
            .add_rectangle((mm(0), mm(5)), (mm(10), mm(15)))
            .unwrap();
        e.set_sketches(vec![sketch], None);
        let mut spec = TurnedSpec::full_turn("spacer", "ring", "PLA").unwrap();
        spec.at = [mm(20), mm(30), mm(10)];
        e.apply(Operation::AddTurned {
            id: ObjectId::from_parts(1_700_000_000_000, 2, 2).unwrap(),
            turned: spec,
        })
        .unwrap();
        // the plane through the axis of the tube (z = 10 mm) cuts it in two rectangles of
        // 10 x 10 mm
        let s = section_of(&e, Dim::Z, 10);
        let ring: Vec<&Contour> = s.contours.iter().filter(|c| c.body == "spacer").collect();
        let area: f64 = ring.iter().map(|c| square_mm2(c.area_nm2)).sum();
        assert!(ring.len() >= 2, "{ring:?}");
        assert!((area - 200.0).abs() < 1.0, "{area}");
    }

    #[test]
    fn the_section_is_written_as_a_dxf_and_an_svg_drawing_and_as_a_report() {
        let s = section_of(&model(), Dim::Z, 15);
        let settings = Settings::defaults();
        let off = TitleBlockSettings::off();
        // the outline alone, without the hatching that the shipped settings add
        let mut outline = settings.drawing;
        outline.hatch_spacing = Length::ZERO;
        let dxf = render_titled(&s, &outline, true, &off, "box", Lang::En).unwrap();
        assert!(dxf.contains("CONTORNO"), "{dxf}");
        assert_eq!(dxf.matches("\nLINE\n").count(), 8, "{dxf}");
        let svg = render_titled(&s, &settings.drawing, false, &off, "box", Lang::En).unwrap();
        assert!(svg.starts_with("<?xml"), "{svg}");
        assert_eq!(
            svg,
            render_titled(&s, &settings.drawing, false, &off, "box", Lang::En).unwrap()
        );
        let text = text_in(&s, Lang::En);
        assert!(text.starts_with("section plane: z = 15 mm\n"), "{text}");
        assert!(text.contains("outlines: 2 (8 segments)"), "{text}");
        assert!(text.contains("area of the material: 656.000 mm2"), "{text}");
        assert!(
            text.contains("enclosure: 2 outlines, 656.000 mm2"),
            "{text}"
        );
        for l in Lang::ALL {
            assert_eq!(text_in(&s, l).lines().count(), 4, "{}", l.code());
        }
        // more lines than the settings allow are refused
        let mut small = outline;
        small.max_entities = 8;
        assert!(render_titled(&s, &small, true, &off, "box", Lang::En).is_ok());
        small.max_entities = 7;
        // (the minimum the settings allow is 8, so this only tests the check itself)
        assert!(render_titled(&s, &small, true, &off, "box", Lang::En).is_err());
    }

    fn square(body: &str, material: &str, at: (f64, f64), side: f64, hole: bool) -> Contour {
        let (x0, x1) = (at.0 * 1e6, (at.0 + side) * 1e6);
        let (y0, y1) = (at.1 * 1e6, (at.1 + side) * 1e6);
        let mut points = vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
        if hole {
            points.reverse();
        }
        Contour {
            body: body.into(),
            material: material.into(),
            points,
            area_nm2: if hole { -1.0 } else { 1.0 } * side * side * 1e12,
        }
    }

    fn two_materials() -> Section {
        Section {
            axis: Dim::Z,
            position: Length::ZERO,
            contours: vec![
                square("plate", "steel", (0.0, 0.0), 20.0, false),
                square("cover", "PLA", (40.0, 0.0), 20.0, false),
            ],
        }
    }

    const ANGLES: [u16; 8] = [135, 45, 90, 0, 60, 120, 30, 150];

    fn direction(e: &Entity) -> (f64, f64) {
        let Entity::Line(a, b) = *e else {
            panic!("not a line: {e:?}");
        };
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        ((b.0 - a.0) / len, (b.1 - a.1) / len)
    }

    #[test]
    fn each_material_of_a_section_is_hatched_at_an_angle_of_its_own() {
        let s = two_materials();
        // the names in order: PLA (capitals first) takes the first angle, steel the second
        assert_eq!(
            hatch_key(&s, &ANGLES),
            vec![("PLA".to_string(), 135), ("steel".to_string(), 45)]
        );
        let lines = hatch_by_material(&s, &ANGLES, 2.0, 100_000).unwrap();
        let (mut pla, mut steel) = (0usize, 0usize);
        for (layer, e) in &lines {
            assert_eq!(*layer, Layer::Hatch);
            let Entity::Line(a, b) = *e else { panic!() };
            let (dx, dy) = direction(e);
            if a.0 > 30.0 {
                // the cover (PLA) at 135 degrees: x + y is the same at both ends
                assert!((a.0 + a.1 - (b.0 + b.1)).abs() < 1e-6, "{e:?}");
                assert!((dx + dy).abs() < 1e-6);
                let (lo, hi) = (a.0.min(b.0), a.0.max(b.0));
                assert!(lo >= 40.0 - 1e-6 && hi <= 60.0 + 1e-6, "{e:?}");
                assert!(a.1 >= -1e-6 && a.1 <= 20.0 + 1e-6, "{e:?}");
                pla += 1;
            } else {
                // the plate (steel) at 45 degrees: x - y is the same at both ends
                assert!((a.0 - a.1 - (b.0 - b.1)).abs() < 1e-6, "{e:?}");
                assert!((dx - dy).abs() < 1e-6);
                assert!(
                    a.0.min(b.0) >= -1e-6 && a.0.max(b.0) <= 20.0 + 1e-6,
                    "{e:?}"
                );
                steel += 1;
            }
        }
        // two squares of the same size have the same number of lines, give or take the one that
        // a corner decides
        assert!(pla.abs_diff(steel) <= 1, "{pla} {steel}");
        assert!(pla > 8, "{pla}");
        // the same section always gives the same lines
        assert_eq!(hatch_by_material(&s, &ANGLES, 2.0, 100_000).unwrap(), lines);
    }

    #[test]
    fn a_hole_is_not_hatched_and_one_material_is_hatched_as_the_plain_hatching_is() {
        let mut s = Section {
            axis: Dim::Z,
            position: Length::ZERO,
            contours: vec![
                square("plate", "steel", (0.0, 0.0), 40.0, false),
                square("plate", "steel", (10.0, 10.0), 20.0, true),
            ],
        };
        let lines = hatch_by_material(&s, &ANGLES, 2.0, 100_000).unwrap();
        for (_, e) in &lines {
            let Entity::Line(a, b) = *e else { panic!() };
            let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            assert!(
                !(mid.0 > 10.0 && mid.0 < 30.0 && mid.1 > 10.0 && mid.1 < 30.0),
                "{e:?}"
            );
        }
        // one material at the first angle (135) is the hatching at 45 degrees of the plain one
        s.contours
            .iter_mut()
            .for_each(|c| c.material = "steel".into());
        let one = hatch_by_material(&s, &[135, 0, 1, 2, 3, 4, 5, 6], 2.0, 100_000).unwrap();
        let plain = hatch(&s, 2.0, 100_000).unwrap();
        assert_eq!(one.len(), plain.len());
        // the same lines, each walked from the other end and in the other order
        let ends = |lines: &[(Layer, Entity)]| {
            let mut v: Vec<[f64; 4]> = lines
                .iter()
                .map(|(_, e)| {
                    let Entity::Line(a, b) = *e else { panic!() };
                    let (p, q) = if (a.0, a.1) <= (b.0, b.1) {
                        (a, b)
                    } else {
                        (b, a)
                    };
                    [p.0, p.1, q.0, q.1]
                })
                .collect();
            v.sort_by_key(|x| ((x[0] * 1e6).round() as i64, (x[1] * 1e6).round() as i64));
            v
        };
        for (x, y) in ends(&one).iter().zip(ends(&plain).iter()) {
            for (p, q) in x.iter().zip(y.iter()) {
                assert!((p - q).abs() < 1e-6, "{x:?} {y:?}");
            }
        }
    }

    #[test]
    fn the_angles_start_again_after_the_eighth_material_and_the_limit_is_shared() {
        let mut s = Section {
            axis: Dim::Z,
            position: Length::ZERO,
            contours: Vec::new(),
        };
        for k in 0..10 {
            let name = format!("m{k}");
            s.contours
                .push(square(&name, &name, (k as f64 * 30.0, 0.0), 20.0, false));
        }
        let key = hatch_key(&s, &ANGLES);
        assert_eq!(key.len(), 10);
        assert_eq!(key[8].1, ANGLES[0]);
        assert_eq!(key[9].1, ANGLES[1]);
        let all = hatch_by_material(&s, &ANGLES, 4.0, 100_000).unwrap();
        assert!(all.len() > 10);
        // the lines of all the materials together count against the limit
        let err = hatch_by_material(&s, &ANGLES, 4.0, all.len() - 1).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-050");
        assert!(hatch_by_material(&s, &ANGLES, 4.0, all.len() + 10).is_ok());
        // a spacing of nothing and a section with nothing have no lines
        assert!(hatch_by_material(&s, &ANGLES, 0.0, 10).unwrap().is_empty());
        let none = Section {
            axis: Dim::Z,
            position: Length::ZERO,
            contours: Vec::new(),
        };
        assert!(hatch_by_material(&none, &ANGLES, 2.0, 10)
            .unwrap()
            .is_empty());
        assert!(hatch_key(&none, &ANGLES).is_empty());
    }

    #[test]
    fn the_files_of_a_section_are_hatched_by_material_when_the_settings_ask() {
        let s = two_materials();
        let off = TitleBlockSettings::off();
        let mut settings = Settings::defaults().drawing;
        let plain = render_titled(&s, &settings, true, &off, "x", Lang::En).unwrap();
        settings.hatch_by_material = true;
        let by = render_titled(&s, &settings, true, &off, "x", Lang::En).unwrap();
        assert_ne!(plain, by);
        let count = |t: &str| t.matches("0\nLINE\n8\nRAYADO\n").count();
        assert!(count(&plain) > 8 && count(&by) > 8);
        // the outline is the same, and the hatching of the cover turned from 135 to 45 degrees
        assert_eq!(
            plain.matches("0\nLINE\n8\nCONTORNO\n").count(),
            by.matches("0\nLINE\n8\nCONTORNO\n").count()
        );
        // the other angles change the files
        settings.hatch_angles = [0, 90, 45, 135, 60, 120, 30, 150];
        let other = render_titled(&s, &settings, true, &off, "x", Lang::En).unwrap();
        assert_ne!(other, by);
    }
}
