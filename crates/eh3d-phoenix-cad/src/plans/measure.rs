// SPDX-License-Identifier: GPL-3.0-or-later
//! Associative dimensions: a dimension is tied to things of the model by ids that do not change
//! (the nodes of the history, the elements of the sketches), and its value is worked out from the
//! model every time, so it follows an edit of a parameter. When the thing it is tied to is gone
//! the dimension is an **orphan**: `PHOENIX-CAD-055` says which reference, and the dimension is
//! neither drawn with an old value nor dropped.

use super::model::{Axis, Dim, DimKind, Ref, Side, ViewKind};
use super::prim::{angle_text, arrow, nm_text, pt, Anchor, Prim};
use super::settings::DimStyle;
use crate::featuredag::{FeatureDag, Kind};
use crate::sketch::{Element, ElementId, Sketch};

/// The model that the dimensions are measured on: the history as the last rebuild left it and the
/// sketches of the project.
#[derive(Clone, Copy)]
pub struct Model<'a> {
    /// The history, or `None` when the project has none.
    pub dag: Option<&'a FeatureDag>,
    /// The sketches.
    pub sketches: &'a [Sketch],
}

/// A problem of a drawing, with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// `PHOENIX-CAD-054` to `PHOENIX-CAD-057`.
    pub code: &'static str,
    /// What it is about: the kind of record and its name.
    pub subject: String,
    /// The reason, in English, with the figures.
    pub detail: String,
}

/// A reference found in the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// A face of the box of a solid.
    Face {
        /// The axis across it.
        axis: Axis,
        /// Its coordinate, in nanometres.
        pos: i64,
        /// The lowest corner of the box of the solid.
        lo: [i64; 3],
        /// The highest corner of the box of the solid.
        hi: [i64; 3],
    },
    /// A circle or an arc.
    Round {
        /// Its centre.
        c: (i64, i64),
        /// Its radius.
        r: i64,
    },
    /// A line.
    Line {
        /// One end.
        a: (i64, i64),
        /// The other end.
        b: (i64, i64),
    },
}

/// What a dimension came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measured {
    /// The references found.
    pub refs: Vec<Resolved>,
    /// The values: nanometres, or thousandths of a degree for an angle.
    pub values: Vec<i64>,
    /// The texts of the values.
    pub texts: Vec<String>,
}

const ORPHAN: &str = "PHOENIX-CAD-055";
const INVALID: &str = "PHOENIX-CAD-056";

type Fail = (&'static str, String);

/// The axes across the picture and up it for a view along an axis.
pub fn view_axes(kind: ViewKind) -> Option<(Axis, Axis)> {
    match kind {
        ViewKind::Front => Some((Axis::X, Axis::Z)),
        ViewKind::Top => Some((Axis::X, Axis::Y)),
        ViewKind::Right => Some((Axis::Y, Axis::Z)),
        _ => None,
    }
}

/// Finds a reference in the model.
pub fn resolve(r: &Ref, model: &Model) -> Result<Resolved, Fail> {
    match r {
        Ref::Face { node, axis, side } => {
            let dag = model
                .dag
                .ok_or((ORPHAN, "the project has no history of features".to_string()))?;
            let n = dag
                .node(*node)
                .ok_or((ORPHAN, format!("the node {node} is not in the history")))?;
            if !matches!(n.kind, Kind::Feature { .. }) {
                return Err((ORPHAN, format!("{} is a parameter, not a feature", n.name)));
            }
            let solid = dag.solid(*node).ok_or((
                ORPHAN,
                format!("{} has no solid in the last rebuild", n.name),
            ))?;
            let (lo, hi) = solid
                .bounds()
                .ok_or((ORPHAN, format!("the solid of {} is empty", n.name)))?;
            let a = axis.index();
            Ok(Resolved::Face {
                axis: *axis,
                pos: if *side == Side::Min { lo[a] } else { hi[a] },
                lo,
                hi,
            })
        }
        Ref::Element { sketch, element } => {
            let sk = model
                .sketches
                .iter()
                .find(|s| s.name() == sketch)
                .ok_or((ORPHAN, format!("the sketch {sketch} is not in the project")))?;
            let el = sk.element(ElementId(*element)).ok_or((
                ORPHAN,
                format!("the element {element} is not in the sketch {sketch}"),
            ))?;
            let point = |id: ElementId| -> Result<(i64, i64), Fail> {
                sk.point(id)
                    .map(|(x, y)| (x.get(), y.get()))
                    .map_err(|e| (ORPHAN, format!("{sketch}: {e}")))
            };
            match *el {
                Element::Circle { center, radius } => Ok(Resolved::Round {
                    c: point(center)?,
                    r: radius.get(),
                }),
                Element::Arc { center, start, .. } => {
                    let c = point(center)?;
                    let s = point(start)?;
                    let (dx, dy) = ((s.0 - c.0) as f64, (s.1 - c.1) as f64);
                    Ok(Resolved::Round {
                        c,
                        r: (dx * dx + dy * dy).sqrt().round() as i64,
                    })
                }
                Element::Line { a, b } => Ok(Resolved::Line {
                    a: point(a)?,
                    b: point(b)?,
                }),
                Element::Point { .. } => Err((
                    INVALID,
                    format!("the element {element} of {sketch} is a point"),
                )),
            }
        }
    }
}

/// Works out a dimension from the model.
pub fn measure(dim: &Dim, model: &Model, style: &DimStyle) -> Result<Measured, Fail> {
    let mut refs = Vec::new();
    for r in &dim.refs {
        refs.push(resolve(r, model)?);
    }
    let mut values = Vec::new();
    match dim.kind {
        DimKind::Linear | DimKind::Cumulative | DimKind::Chain | DimKind::Ordinate => {
            let mut axis = None;
            let mut pos = Vec::new();
            for r in &refs {
                match r {
                    Resolved::Face {
                        axis: a, pos: p, ..
                    } => {
                        if *axis.get_or_insert(*a) != *a {
                            return Err((
                                INVALID,
                                format!("{}: the faces are across different axes", dim.name),
                            ));
                        }
                        pos.push(*p);
                    }
                    _ => return Err((INVALID, format!("{} measures faces", dim.name))),
                }
            }
            match dim.kind {
                DimKind::Chain => {
                    for w in pos.windows(2) {
                        values.push((w[1] - w[0]).abs());
                    }
                }
                DimKind::Ordinate => {
                    for p in &pos[1..] {
                        values.push(p - pos[0]);
                    }
                }
                _ => {
                    for p in &pos[1..] {
                        values.push((p - pos[0]).abs());
                    }
                }
            }
        }
        DimKind::Radius | DimKind::Diameter => match refs[0] {
            Resolved::Round { r, .. } => values.push(if dim.kind == DimKind::Radius {
                r
            } else {
                r * 2
            }),
            _ => return Err((INVALID, format!("{} measures a circle or an arc", dim.name))),
        },
        DimKind::Angle => {
            let (Resolved::Line { a, b }, Resolved::Line { a: c, b: d }) = (&refs[0], &refs[1])
            else {
                return Err((INVALID, format!("{} measures two lines", dim.name)));
            };
            let d1 = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
            let d2 = ((d.0 - c.0) as f64, (d.1 - c.1) as f64);
            if (d1.0 == 0.0 && d1.1 == 0.0) || (d2.0 == 0.0 && d2.1 == 0.0) {
                return Err((INVALID, format!("{}: a line has no length", dim.name)));
            }
            let cross = d1.0 * d2.1 - d1.1 * d2.0;
            let dot = d1.0 * d2.0 + d1.1 * d2.1;
            values.push((cross.abs().atan2(dot).to_degrees() * 1000.0).round() as i64);
        }
    }
    let texts = values
        .iter()
        .map(|v| match dim.kind {
            DimKind::Radius => format!("R{}", nm_text(*v, style.decimals)),
            DimKind::Diameter => format!("\u{d8}{}", nm_text(*v, style.decimals)),
            DimKind::Angle => format!("{}\u{b0}", angle_text(*v, style.decimals)),
            _ => nm_text(*v, style.decimals),
        })
        .collect();
    Ok(Measured {
        refs,
        values,
        texts,
    })
}

/// How a view is placed on the sheet: the box of its picture in nanometres and where the lower
/// left corner of that box is on the sheet in micrometres, with the scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    /// What the view looks at.
    pub kind: ViewKind,
    /// The lowest corner of the box of the picture, in nanometres.
    pub lo: (i64, i64),
    /// The highest corner.
    pub hi: (i64, i64),
    /// Where `lo` is on the sheet, in micrometres.
    pub origin: (i64, i64),
    /// The scale `n:d`.
    pub scale: (u32, u32),
    /// The squeeze of a broken view: `lo` and `hi` are the box of the picture after it.
    pub brk: Option<super::brk::Warp>,
}

fn scaled(v: i64, n: u32, d: u32) -> i64 {
    let num = i128::from(v) * i128::from(n);
    let den = i128::from(d) * 1000;
    let half = den / 2;
    (if num >= 0 {
        (num + half) / den
    } else {
        -((-num + half) / den)
    }) as i64
}

impl Frame {
    /// A point of the picture on the sheet.
    pub fn map(&self, p: (i64, i64)) -> (i64, i64) {
        let p = self.brk.map_or(p, |w| w.point(p));
        (
            self.origin.0 + scaled(p.0 - self.lo.0, self.scale.0, self.scale.1),
            self.origin.1 + scaled(p.1 - self.lo.1, self.scale.0, self.scale.1),
        )
    }

    /// A length of the picture on the sheet.
    pub fn len(&self, nm: i64) -> i64 {
        scaled(nm, self.scale.0, self.scale.1)
    }

    /// The size of the box of the picture on the sheet.
    pub fn size(&self) -> (i64, i64) {
        (
            self.len(self.hi.0 - self.lo.0),
            self.len(self.hi.1 - self.lo.1),
        )
    }
}

/// Where a face is on the sheet: its position along the dimension and where the part starts
/// across it (the side the dimension line is drawn on), in micrometres. A reference that is not a
/// face gives `(0, 0)`.
pub fn face_place(
    r: &Resolved,
    frame: &Frame,
    horizontal: bool,
    u_axis: Axis,
    v_axis: Axis,
) -> (i64, i64) {
    match r {
        Resolved::Face { pos, lo, .. } => {
            let across = if horizontal {
                lo[v_axis.index()]
            } else {
                lo[u_axis.index()]
            };
            if horizontal {
                (frame.map((*pos, 0)).0, frame.map((0, across)).1)
            } else {
                (frame.map((0, *pos)).1, frame.map((across, 0)).0)
            }
        }
        _ => (0, 0),
    }
}

/// Draws a dimension that has been measured, in the frame of its view. A dimension that cannot be
/// drawn in that kind of view is `PHOENIX-CAD-056`.
pub fn draw(
    dim: &Dim,
    m: &Measured,
    frame: &Frame,
    style: &DimStyle,
    sketch_name: Option<&str>,
) -> Result<Vec<Prim>, Fail> {
    let layer = dim.layer.as_str();
    let offset = dim.offset_um.unwrap_or(style.offset_um);
    let h = style.text_height_um;
    let mut out = Vec::new();
    match dim.kind {
        DimKind::Chain | DimKind::Ordinate => {
            return super::extended::draw(dim, m, frame, style);
        }
        DimKind::Linear | DimKind::Cumulative => {
            let (u_axis, v_axis) = view_axes(frame.kind).ok_or((
                INVALID,
                format!(
                    "{}: lengths are drawn in the front, top and right views",
                    dim.name
                ),
            ))?;
            let Resolved::Face { axis, .. } = m.refs[0] else {
                return Err((INVALID, format!("{} measures faces", dim.name)));
            };
            let horizontal = axis == u_axis;
            if !horizontal && axis != v_axis {
                return Err((
                    INVALID,
                    format!(
                        "{}: the axis {} is not in a view of this kind",
                        dim.name,
                        axis.name()
                    ),
                ));
            }
            let place = |r: &Resolved| -> (i64, i64, i64) {
                // the position along the dimension, and where the part starts across it
                let (along, across) = face_place(r, frame, horizontal, u_axis, v_axis);
                (along, across, 0)
            };
            let datum = place(&m.refs[0]);
            for (k, r) in m.refs[1..].iter().enumerate() {
                let end = place(r);
                let level = offset + k as i64 * dim.step_um.unwrap_or(style.step_um);
                let (a, b) = (datum.0.min(end.0), datum.0.max(end.0));
                let text = &m.texts[k];
                if horizontal {
                    let yd = frame.origin.1 - level;
                    for p in [datum, end] {
                        out.push(Prim::line(
                            layer,
                            (p.0, p.1 - style.extension_gap_um),
                            (p.0, yd - style.extension_overshoot_um),
                        ));
                    }
                    out.push(Prim::line(layer, (a, yd), (b, yd)));
                    out.extend(arrow(
                        layer,
                        (a, yd),
                        (1.0, 0.0),
                        style.arrow_length_um,
                        style.arrow_width_um,
                    ));
                    out.extend(arrow(
                        layer,
                        (b, yd),
                        (-1.0, 0.0),
                        style.arrow_length_um,
                        style.arrow_width_um,
                    ));
                    out.push(Prim::text(
                        layer,
                        ((a + b) / 2, yd + h * 3 / 10),
                        h,
                        text,
                        Anchor::Middle,
                    ));
                } else {
                    let xd = frame.origin.0 - level;
                    for p in [datum, end] {
                        out.push(Prim::line(
                            layer,
                            (p.1 - style.extension_gap_um, p.0),
                            (xd - style.extension_overshoot_um, p.0),
                        ));
                    }
                    out.push(Prim::line(layer, (xd, a), (xd, b)));
                    out.extend(arrow(
                        layer,
                        (xd, a),
                        (0.0, 1.0),
                        style.arrow_length_um,
                        style.arrow_width_um,
                    ));
                    out.extend(arrow(
                        layer,
                        (xd, b),
                        (0.0, -1.0),
                        style.arrow_length_um,
                        style.arrow_width_um,
                    ));
                    out.push(Prim::turned(
                        layer,
                        (xd - h * 3 / 10, (a + b) / 2),
                        h,
                        text,
                        Anchor::Middle,
                        90.0,
                    ));
                }
            }
        }
        DimKind::Radius | DimKind::Diameter => {
            if frame.kind != ViewKind::Sketch {
                return Err((
                    INVALID,
                    format!(
                        "{}: a radius or a diameter is drawn in a sketch view",
                        dim.name
                    ),
                ));
            }
            check_sketch(dim, sketch_name)?;
            let Resolved::Round { c, r } = m.refs[0] else {
                return Err((INVALID, format!("{} measures a circle or an arc", dim.name)));
            };
            let centre = frame.map(c);
            let rad = frame.len(r) as f64;
            let ang = f64::from(style.leader_angle_deg).to_radians();
            let dir = (ang.cos(), ang.sin());
            let on = pt(centre.0 as f64 + rad * dir.0, centre.1 as f64 + rad * dir.1);
            let end = pt(
                on.0 as f64 + style.leader_length_um as f64 * dir.0,
                on.1 as f64 + style.leader_length_um as f64 * dir.1,
            );
            let from = if dim.kind == DimKind::Diameter {
                pt(centre.0 as f64 - rad * dir.0, centre.1 as f64 - rad * dir.1)
            } else {
                centre
            };
            out.push(Prim::line(layer, from, end));
            out.extend(arrow(
                layer,
                on,
                (-dir.0, -dir.1),
                style.arrow_length_um,
                style.arrow_width_um,
            ));
            if dim.kind == DimKind::Diameter {
                out.extend(arrow(
                    layer,
                    from,
                    dir,
                    style.arrow_length_um,
                    style.arrow_width_um,
                ));
            }
            let anchor = if dir.0 >= 0.0 {
                Anchor::Start
            } else {
                Anchor::Middle
            };
            out.push(Prim::text(
                layer,
                (end.0 + h / 4, end.1 + h / 4),
                h,
                &m.texts[0],
                anchor,
            ));
        }
        DimKind::Angle => {
            if frame.kind != ViewKind::Sketch {
                return Err((
                    INVALID,
                    format!("{}: an angle is drawn in a sketch view", dim.name),
                ));
            }
            check_sketch(dim, sketch_name)?;
            let (Resolved::Line { a, b }, Resolved::Line { a: c, b: d }) = (&m.refs[0], &m.refs[1])
            else {
                return Err((INVALID, format!("{} measures two lines", dim.name)));
            };
            let (p, q) = ((a.0 as f64, a.1 as f64), (c.0 as f64, c.1 as f64));
            let d1 = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
            let d2 = ((d.0 - c.0) as f64, (d.1 - c.1) as f64);
            let cross = d1.0 * d2.1 - d1.1 * d2.0;
            if cross.abs() < 1e-9 * (d1.0.hypot(d1.1) * d2.0.hypot(d2.1)) {
                return Err((INVALID, format!("{}: the lines are parallel", dim.name)));
            }
            let t = ((q.0 - p.0) * d2.1 - (q.1 - p.1) * d2.0) / cross;
            let apex = (p.0 + t * d1.0, p.1 + t * d1.1);
            let apex_s = frame.map((apex.0.round() as i64, apex.1.round() as i64));
            let (a1, a2) = (d1.1.atan2(d1.0), d2.1.atan2(d2.0));
            let (start, end) = if cross > 0.0 { (a1, a2) } else { (a2, a1) };
            let mut end = end;
            while end < start {
                end += std::f64::consts::TAU;
            }
            let rad = style.arc_radius_um;
            out.push(Prim {
                layer: layer.to_string(),
                shape: super::prim::Shape::Arc {
                    c: apex_s,
                    r: rad,
                    a0: start.to_degrees(),
                    a1: end.to_degrees(),
                },
            });
            for angle in [start, end] {
                let (cs, sn) = (angle.cos(), angle.sin());
                let tip = pt(
                    apex_s.0 as f64 + rad as f64 * cs,
                    apex_s.1 as f64 + rad as f64 * sn,
                );
                out.push(Prim::line(
                    layer,
                    apex_s,
                    pt(
                        apex_s.0 as f64 + (rad + style.extension_overshoot_um) as f64 * cs,
                        apex_s.1 as f64 + (rad + style.extension_overshoot_um) as f64 * sn,
                    ),
                ));
                let tangent = if angle == start { (-sn, cs) } else { (sn, -cs) };
                out.extend(arrow(
                    layer,
                    tip,
                    tangent,
                    style.arrow_length_um,
                    style.arrow_width_um,
                ));
            }
            let mid = (start + end) / 2.0;
            let at = pt(
                apex_s.0 as f64 + (rad + h) as f64 * mid.cos(),
                apex_s.1 as f64 + (rad + h) as f64 * mid.sin(),
            );
            out.push(Prim::text(layer, at, h, &m.texts[0], Anchor::Middle));
        }
    }
    Ok(out)
}

fn check_sketch(dim: &Dim, sketch_name: Option<&str>) -> Result<(), Fail> {
    for r in &dim.refs {
        if let Ref::Element { sketch, .. } = r {
            if Some(sketch.as_str()) != sketch_name {
                return Err((
                    INVALID,
                    format!("{}: the view does not show the sketch {sketch}", dim.name),
                ));
            }
        }
    }
    Ok(())
}
