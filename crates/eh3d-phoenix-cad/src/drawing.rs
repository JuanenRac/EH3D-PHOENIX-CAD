//! Flat drawings of the model: the plan, the front and the side as DXF and SVG files.
//!
//! A [`Drawing`] is a list of lines, circles and arcs in millimetres, each on one of three layers:
//! `CONTORNO` (the outline of the box and of what stands out of it), `AGUJEROS` (the openings of
//! the holes and pockets of the face that the view looks at) and `EJES` (centre lines through the
//! holes and the box). The three views look at the box from above (the plan: `x` to the right and
//! `y` up), from the front (`x` to the right and `z` up) and from the right (`y` to the right and
//! `z` up). A hole of the face that the view looks at is drawn at its own `u`, `v`, because in
//! these three faces `u` runs to the right and `v` up. Posts, the lip and the inside of the box
//! are not drawn: they cannot be seen from outside.
//!
//! The same list gives the DXF file (R12 text: `LINE`, `CIRCLE` and `ARC` entities and the three
//! layers) and the SVG file (scaled to the width of the page of the settings, with the size of the
//! box and of the holes as dimensions when the settings ask for them). Both are text that is the
//! same for the same model. Neither has been opened in a CAD program or a drawing program.

use crate::config::DrawingSettings;
use crate::enclosure::{Enclosure, Face, FeatureKind, Hole, HoleStyle, Treatment};
use crate::scad::mm_text;
use crate::solid::Frame;
use crate::CadError;
use std::fmt::Write as _;

/// The view of a drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// From above.
    Top,
    /// From the front.
    Front,
    /// From the right.
    Side,
}

impl View {
    /// The three views, in the order of the menu.
    pub const ALL: [View; 3] = [View::Top, View::Front, View::Side];

    /// The word of the command line.
    pub fn name(self) -> &'static str {
        match self {
            View::Top => "top",
            View::Front => "front",
            View::Side => "side",
        }
    }

    /// The view with a word of the command line.
    pub fn from_name(name: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.name() == name)
    }

    /// The face that the view looks at.
    pub fn face(self) -> Face {
        match self {
            View::Top => Face::Top,
            View::Front => Face::Front,
            View::Side => Face::Right,
        }
    }

    /// A point of the model in the plane of the view.
    fn project(self, p: [f64; 3]) -> (f64, f64) {
        match self {
            View::Top => (p[0], p[1]),
            View::Front => (p[0], p[2]),
            View::Side => (p[1], p[2]),
        }
    }
}

/// The layer of an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// The outline.
    Outline,
    /// The openings.
    Holes,
    /// The centre lines.
    Axes,
}

impl Layer {
    /// The name of the layer in the files.
    pub fn name(self) -> &'static str {
        match self {
            Layer::Outline => "CONTORNO",
            Layer::Holes => "AGUJEROS",
            Layer::Axes => "EJES",
        }
    }

    /// The colour of the layer in the DXF file (white or black, blue, red).
    fn dxf_colour(self) -> u8 {
        match self {
            Layer::Outline => 7,
            Layer::Holes => 5,
            Layer::Axes => 1,
        }
    }

    /// The colour of the layer in the SVG file.
    fn svg_colour(self) -> &'static str {
        match self {
            Layer::Outline => "#111111",
            Layer::Holes => "#1f4fd8",
            Layer::Axes => "#d03030",
        }
    }
}

/// One thing drawn, in millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Entity {
    /// A straight line.
    Line((f64, f64), (f64, f64)),
    /// A circle: the centre and the radius.
    Circle((f64, f64), f64),
    /// An arc, counter-clockwise from the first angle to the second, in degrees.
    Arc((f64, f64), f64, f64, f64),
}

/// A dimension or a label of the SVG drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// Where the text goes.
    pub at: (f64, f64),
    /// The text.
    pub text: String,
    /// The line it measures, when it is a dimension: the two ends.
    pub measures: Option<((f64, f64), (f64, f64))>,
}

/// A drawing of a view.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    /// The view.
    pub view: View,
    /// What is drawn and on which layer.
    pub entities: Vec<(Layer, Entity)>,
    /// The dimensions and labels.
    pub notes: Vec<Note>,
    /// The size of the box in the view, in millimetres.
    pub extent: (f64, f64),
}

fn mm(nm: i64) -> f64 {
    nm as f64 / 1_000_000.0
}

fn rect_lines(layer: Layer, lo: (f64, f64), hi: (f64, f64)) -> Vec<(Layer, Entity)> {
    let corners = [lo, (hi.0, lo.1), hi, (lo.0, hi.1)];
    (0..4)
        .map(|i| (layer, Entity::Line(corners[i], corners[(i + 1) % 4])))
        .collect()
}

/// The outline of an opening as entities, and its text label.
fn opening(hole: &Hole) -> (Vec<Entity>, String) {
    let (u, v) = (mm(hole.u.get()), mm(hole.v.get()));
    let d = mm(hole.diameter.get());
    let label = format!("\u{2300}{}", mm_text(hole.diameter.get()));
    match hole.style {
        HoleStyle::Plain => (vec![Entity::Circle((u, v), d / 2.0)], label),
        HoleStyle::Countersunk { head_diameter }
        | HoleStyle::Counterbored { head_diameter, .. } => (
            vec![
                Entity::Circle((u, v), mm(head_diameter.get()) / 2.0),
                Entity::Circle((u, v), d / 2.0),
            ],
            format!("{label} / \u{2300}{}", mm_text(head_diameter.get())),
        ),
        HoleStyle::Rectangular { height } => {
            let h = mm(height.get());
            let lines = rect_lines(
                Layer::Holes,
                (u - d / 2.0, v - h / 2.0),
                (u + d / 2.0, v + h / 2.0),
            );
            (
                lines.into_iter().map(|l| l.1).collect(),
                format!(
                    "{}\u{d7}{}",
                    mm_text(hole.diameter.get()),
                    mm_text(height.get())
                ),
            )
        }
        HoleStyle::Oval { length, vertical } => {
            let l = mm(length.get());
            let r = d / 2.0;
            let sep = (l - d) / 2.0;
            let text = format!(
                "{}\u{d7}{}",
                mm_text(hole.diameter.get()),
                mm_text(length.get())
            );
            let shapes = if sep <= 0.0 {
                vec![Entity::Circle((u, v), r)]
            } else if vertical {
                vec![
                    Entity::Line((u - r, v - sep), (u - r, v + sep)),
                    Entity::Line((u + r, v - sep), (u + r, v + sep)),
                    Entity::Arc((u, v + sep), r, 0.0, 180.0),
                    Entity::Arc((u, v - sep), r, 180.0, 360.0),
                ]
            } else {
                vec![
                    Entity::Line((u - sep, v - r), (u + sep, v - r)),
                    Entity::Line((u - sep, v + r), (u + sep, v + r)),
                    Entity::Arc((u + sep, v), r, 270.0, 450.0),
                    Entity::Arc((u - sep, v), r, 90.0, 270.0),
                ]
            };
            (shapes, text)
        }
    }
}

impl Drawing {
    /// The drawing of a view of the model.
    pub fn of(
        enclosure: &Enclosure,
        view: View,
        settings: &DrawingSettings,
    ) -> Result<Drawing, CadError> {
        let f = Frame::of(enclosure);
        let params = enclosure.params();
        let (x, y, z) = params.outer();
        let (w, h) = match view {
            View::Top => (mm(x.get()), mm(y.get())),
            View::Front => (mm(x.get()), mm(z.get())),
            View::Side => (mm(y.get()), mm(z.get())),
        };
        let mut entities: Vec<(Layer, Entity)> = Vec::new();
        let mut notes: Vec<Note> = Vec::new();
        // the outline of the box
        let s = mm(enclosure.edging().vertical.size().get());
        match (view, enclosure.edging().vertical) {
            (View::Top, Treatment::Sharp) | (View::Front | View::Side, _) => {
                entities.extend(rect_lines(Layer::Outline, (0.0, 0.0), (w, h)));
            }
            (View::Top, t) => {
                let lines = [
                    ((s, 0.0), (w - s, 0.0)),
                    ((w, s), (w, h - s)),
                    ((w - s, h), (s, h)),
                    ((0.0, h - s), (0.0, s)),
                ];
                for (a, b) in lines {
                    entities.push((Layer::Outline, Entity::Line(a, b)));
                }
                let corners = [
                    ((w - s, s), 270.0, (w - s, 0.0), (w, s)),
                    ((w - s, h - s), 0.0, (w, h - s), (w - s, h)),
                    ((s, h - s), 90.0, (s, h), (0.0, h - s)),
                    ((s, s), 180.0, (0.0, s), (s, 0.0)),
                ];
                for (c, start, from, to) in corners {
                    match t {
                        Treatment::Round(_) => {
                            entities.push((Layer::Outline, Entity::Arc(c, s, start, start + 90.0)))
                        }
                        _ => entities.push((Layer::Outline, Entity::Line(from, to))),
                    }
                }
            }
        }
        // what stands out of the faces: the extrusions, the fins and the placed parts
        let near = view.face();
        for feature in enclosure.features().values() {
            let (u0, v0) = (feature.u0.get(), feature.v0.get());
            let depth = feature.depth_in(params).get();
            if feature.face == near {
                for outline in feature.profile.outlines() {
                    let pts = outline.polygon();
                    let layer = match feature.kind {
                        FeatureKind::Extrude => Layer::Outline,
                        FeatureKind::Pocket => Layer::Holes,
                    };
                    for i in 0..pts.len() {
                        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                        entities.push((
                            layer,
                            Entity::Line(
                                (mm(u0 + a.0), mm(v0 + a.1)),
                                (mm(u0 + b.0), mm(v0 + b.1)),
                            ),
                        ));
                    }
                }
            } else if feature.kind == FeatureKind::Extrude {
                let (lo, hi) = feature.profile.bounds();
                let corners = [
                    f.xyz(feature.face, (u0 + lo.0) as f64, (v0 + lo.1) as f64, 0.0),
                    f.xyz(
                        feature.face,
                        (u0 + hi.0) as f64,
                        (v0 + hi.1) as f64,
                        -(depth as f64),
                    ),
                ];
                entities.extend(box_in_view(view, corners[0], corners[1]));
            }
        }
        for set in enclosure.fins().values() {
            let height = set.height.get() as f64;
            for r in set.rects() {
                let half = |v: i128| v as f64 / 2.0;
                if set.face == near {
                    entities.extend(rect_lines(
                        Layer::Outline,
                        (half(r.u0) / 1e6, half(r.v0) / 1e6),
                        (half(r.u1) / 1e6, half(r.v1) / 1e6),
                    ));
                } else {
                    let a = f.xyz(set.face, half(r.u0), half(r.v0), 0.0);
                    let b = f.xyz(set.face, half(r.u1), half(r.v1), -height);
                    entities.extend(box_in_view(view, a, b));
                }
            }
        }
        for part in enclosure.placed_parts().values() {
            let (lo, hi) = part.bounds()?;
            let a = [lo[0] as f64, lo[1] as f64, lo[2] as f64];
            let b = [hi[0] as f64, hi[1] as f64, hi[2] as f64];
            entities.extend(box_in_view(view, a, b));
        }
        // the openings of the face that the view looks at, and the centre lines
        let over = mm(settings.axis_overshoot.get());
        let mut axes: Vec<(Layer, Entity)> = Vec::new();
        for hole in enclosure.all_holes() {
            if hole.face != near {
                continue;
            }
            let (shapes, label) = opening(&hole);
            let (u, v) = (mm(hole.u.get()), mm(hole.v.get()));
            let (su, sv) = hole.style.footprint(hole.diameter);
            let (ru, rv) = (mm(su as i64) / 2.0, mm(sv as i64) / 2.0);
            entities.extend(shapes.into_iter().map(|e| (Layer::Holes, e)));
            axes.push((
                Layer::Axes,
                Entity::Line((u - ru - over, v), (u + ru + over, v)),
            ));
            axes.push((
                Layer::Axes,
                Entity::Line((u, v - rv - over), (u, v + rv + over)),
            ));
            notes.push(Note {
                at: (u + ru + over / 2.0, v + rv + over / 2.0),
                text: label,
                measures: None,
            });
        }
        axes.push((
            Layer::Axes,
            Entity::Line((-over, h / 2.0), (w + over, h / 2.0)),
        ));
        axes.push((
            Layer::Axes,
            Entity::Line((w / 2.0, -over), (w / 2.0, h + over)),
        ));
        entities.extend(axes);
        notes.push(Note {
            at: (w / 2.0, 0.0),
            text: mm_text(match view {
                View::Top | View::Front => x.get(),
                View::Side => y.get(),
            }),
            measures: Some(((0.0, 0.0), (w, 0.0))),
        });
        notes.push(Note {
            at: (0.0, h / 2.0),
            text: mm_text(match view {
                View::Top => y.get(),
                View::Front | View::Side => z.get(),
            }),
            measures: Some(((0.0, 0.0), (0.0, h))),
        });
        if entities.len() > settings.max_entities as usize {
            return Err(CadError::ExportLimit(format!(
                "the drawing has {} entities and the settings allow {}",
                entities.len(),
                settings.max_entities
            )));
        }
        Ok(Drawing {
            view,
            entities,
            notes,
            extent: (w, h),
        })
    }

    /// The smallest and the largest corner of everything drawn.
    pub fn bounds(&self) -> ((f64, f64), (f64, f64)) {
        let mut lo = (f64::MAX, f64::MAX);
        let mut hi = (f64::MIN, f64::MIN);
        let mut take = |p: (f64, f64)| {
            lo = (lo.0.min(p.0), lo.1.min(p.1));
            hi = (hi.0.max(p.0), hi.1.max(p.1));
        };
        for (_, e) in &self.entities {
            match *e {
                Entity::Line(a, b) => {
                    take(a);
                    take(b);
                }
                Entity::Circle(c, r) | Entity::Arc(c, r, _, _) => {
                    take((c.0 - r, c.1 - r));
                    take((c.0 + r, c.1 + r));
                }
            }
        }
        (lo, hi)
    }

    /// How many entities there are on a layer.
    pub fn count(&self, layer: Layer) -> usize {
        self.entities.iter().filter(|(l, _)| *l == layer).count()
    }

    /// The DXF file (R12 text, millimetres).
    pub fn to_dxf(&self) -> String {
        let n = |v: f64| {
            let t = format!("{v:.6}");
            if t.starts_with('-') && t[1..].bytes().all(|b| b == b'0' || b == b'.') {
                t[1..].to_owned()
            } else {
                t
            }
        };
        let mut o = String::new();
        let mut put = |code: u32, value: &str| {
            let _ = writeln!(o, "{code}\n{value}");
        };
        put(0, "SECTION");
        put(2, "HEADER");
        put(9, "$ACADVER");
        put(1, "AC1009");
        put(9, "$MEASUREMENT");
        put(70, "1");
        put(9, "$INSUNITS");
        put(70, "4");
        put(0, "ENDSEC");
        put(0, "SECTION");
        put(2, "TABLES");
        put(0, "TABLE");
        put(2, "LTYPE");
        put(70, "1");
        put(0, "LTYPE");
        put(2, "CONTINUOUS");
        put(70, "0");
        put(3, "Solid line");
        put(72, "65");
        put(73, "0");
        put(40, "0.0");
        put(0, "ENDTAB");
        put(0, "TABLE");
        put(2, "LAYER");
        put(70, "3");
        for layer in [Layer::Outline, Layer::Holes, Layer::Axes] {
            put(0, "LAYER");
            put(2, layer.name());
            put(70, "0");
            put(62, &layer.dxf_colour().to_string());
            put(6, "CONTINUOUS");
        }
        put(0, "ENDTAB");
        put(0, "ENDSEC");
        put(0, "SECTION");
        put(2, "ENTITIES");
        for (layer, e) in &self.entities {
            match *e {
                Entity::Line(a, b) => {
                    put(0, "LINE");
                    put(8, layer.name());
                    put(10, &n(a.0));
                    put(20, &n(a.1));
                    put(30, "0.0");
                    put(11, &n(b.0));
                    put(21, &n(b.1));
                    put(31, "0.0");
                }
                Entity::Circle(c, r) => {
                    put(0, "CIRCLE");
                    put(8, layer.name());
                    put(10, &n(c.0));
                    put(20, &n(c.1));
                    put(30, "0.0");
                    put(40, &n(r));
                }
                Entity::Arc(c, r, a0, a1) => {
                    put(0, "ARC");
                    put(8, layer.name());
                    put(10, &n(c.0));
                    put(20, &n(c.1));
                    put(30, "0.0");
                    put(40, &n(r));
                    put(50, &n(a0.rem_euclid(360.0)));
                    put(51, &n(a1.rem_euclid(360.0)));
                }
            }
        }
        put(0, "ENDSEC");
        put(0, "EOF");
        o
    }

    /// The SVG file, scaled to the width of the page of the settings.
    pub fn to_svg(&self, settings: &DrawingSettings) -> String {
        let (lo, hi) = self.bounds();
        let margin = if settings.dimensions { 14.0 } else { 4.0 };
        let (width, height) = (hi.0 - lo.0 + 2.0 * margin, hi.1 - lo.1 + 2.0 * margin);
        let page = mm(settings.svg_width.get());
        let scale = page / width;
        let line = 0.3 / scale;
        let text_size = 3.2 / scale;
        let num = |v: f64| {
            let t = format!("{v:.4}");
            let t = t.trim_end_matches('0').trim_end_matches('.').to_owned();
            if t == "-0" || t.is_empty() {
                "0".to_owned()
            } else {
                t
            }
        };
        let px = |x: f64| x - lo.0 + margin;
        let py = |y: f64| hi.1 - y + margin;
        let mut o = String::new();
        let _ = writeln!(o, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        let _ = writeln!(
            o,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\">",
            num(page),
            num(height * scale),
            num(width),
            num(height)
        );
        let _ = writeln!(
            o,
            "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>",
            num(width),
            num(height)
        );
        for layer in [Layer::Outline, Layer::Holes, Layer::Axes] {
            let dash = if layer == Layer::Axes {
                format!(
                    " stroke-dasharray=\"{} {}\"",
                    num(6.0 / scale),
                    num(1.5 / scale)
                )
            } else {
                String::new()
            };
            let _ = writeln!(
                o,
                "<g id=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"{dash}>",
                layer.name(),
                layer.svg_colour(),
                num(line)
            );
            for (l, e) in &self.entities {
                if *l != layer {
                    continue;
                }
                match *e {
                    Entity::Line(a, b) => {
                        let _ = writeln!(
                            o,
                            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
                            num(px(a.0)),
                            num(py(a.1)),
                            num(px(b.0)),
                            num(py(b.1))
                        );
                    }
                    Entity::Circle(c, r) => {
                        let _ = writeln!(
                            o,
                            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
                            num(px(c.0)),
                            num(py(c.1)),
                            num(r)
                        );
                    }
                    Entity::Arc(c, r, a0, a1) => {
                        let at = |deg: f64| {
                            let (s, co) = (deg.to_radians().sin(), deg.to_radians().cos());
                            (px(c.0 + r * co), py(c.1 + r * s))
                        };
                        let (p0, p1) = (at(a0), at(a1));
                        let large = u8::from((a1 - a0).abs() > 180.0);
                        let _ = writeln!(
                            o,
                            "<path d=\"M {} {} A {} {} 0 {large} 0 {} {}\"/>",
                            num(p0.0),
                            num(p0.1),
                            num(r),
                            num(r),
                            num(p1.0),
                            num(p1.1)
                        );
                    }
                }
            }
            o.push_str("</g>\n");
        }
        if settings.dimensions {
            let _ = writeln!(
                o,
                "<g id=\"COTAS\" fill=\"#111111\" stroke=\"#111111\" stroke-width=\"{}\" font-family=\"sans-serif\" font-size=\"{}\">",
                num(line),
                num(text_size)
            );
            for note in &self.notes {
                match note.measures {
                    Some((a, b)) => {
                        let horizontal = a.1 == b.1;
                        let off = 8.0;
                        let (p0, p1, label_at, rotate) = if horizontal {
                            let y = py(a.1) + off;
                            (
                                (px(a.0), y),
                                (px(b.0), y),
                                (px((a.0 + b.0) / 2.0), y - 1.2),
                                false,
                            )
                        } else {
                            let x = px(a.0) - off;
                            (
                                (x, py(a.1)),
                                (x, py(b.1)),
                                (x - 1.2, py((a.1 + b.1) / 2.0)),
                                true,
                            )
                        };
                        let _ = writeln!(
                            o,
                            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
                            num(p0.0),
                            num(p0.1),
                            num(p1.0),
                            num(p1.1)
                        );
                        for p in [p0, p1] {
                            let (ex0, ey0) = if horizontal {
                                (p.0, p.1 - off)
                            } else {
                                (p.0 + off, p.1)
                            };
                            let _ = writeln!(
                                o,
                                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
                                num(ex0),
                                num(ey0),
                                num(p.0),
                                num(p.1)
                            );
                        }
                        let transform = if rotate {
                            format!(
                                " transform=\"rotate(-90 {} {})\"",
                                num(label_at.0),
                                num(label_at.1)
                            )
                        } else {
                            String::new()
                        };
                        let _ = writeln!(
                            o,
                            "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" stroke=\"none\"{transform}>{}</text>",
                            num(label_at.0),
                            num(label_at.1),
                            note.text
                        );
                    }
                    None => {
                        let _ = writeln!(
                            o,
                            "<text x=\"{}\" y=\"{}\" stroke=\"none\">{}</text>",
                            num(px(note.at.0)),
                            num(py(note.at.1)),
                            note.text
                        );
                    }
                }
            }
            o.push_str("</g>\n");
        }
        let _ = writeln!(
            o,
            "<text x=\"{}\" y=\"{}\" font-family=\"sans-serif\" font-size=\"{}\" fill=\"#555555\">1:{:.2}</text>",
            num(2.0),
            num(height - 2.0),
            num(text_size),
            1.0 / scale
        );
        o.push_str("</svg>\n");
        o
    }
}

/// The rectangle that a box of the model makes in a view, on the outline layer.
fn box_in_view(view: View, a: [f64; 3], b: [f64; 3]) -> Vec<(Layer, Entity)> {
    let (p, q) = (view.project(a), view.project(b));
    let lo = (p.0.min(q.0) / 1e6, p.1.min(q.1) / 1e6);
    let hi = (p.0.max(q.0) / 1e6, p.1.max(q.1) / 1e6);
    rect_lines(Layer::Outline, lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::{EnclosureParams, Operation};
    use crate::Length;
    use eh3d_container::ObjectId;

    fn mmv(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn model() -> Enclosure {
        Enclosure::new(EnclosureParams {
            inner_length: mmv(100),
            inner_width: mmv(60),
            inner_height: mmv(30),
            wall: mmv(2),
            floor: mmv(2),
            lid: mmv(3),
            edge_margin: mmv(3),
            min_web: mmv(2),
        })
        .unwrap()
    }

    fn hole(e: &mut Enclosure, n: u64, face: Face, u: i64, v: i64, d: i64, style: HoleStyle) {
        e.apply(Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, n as u16, n).unwrap(),
            hole: Hole {
                name: format!("h{n}"),
                face,
                u: mmv(u),
                v: mmv(v),
                diameter: mmv(d),
                style,
            },
        })
        .unwrap();
    }

    fn settings() -> DrawingSettings {
        crate::config::Settings::defaults().drawing
    }

    #[test]
    fn the_plan_of_a_plain_box_is_a_rectangle_of_104_by_64_with_two_centre_lines() {
        let d = Drawing::of(&model(), View::Top, &settings()).unwrap();
        assert_eq!(d.extent, (104.0, 64.0));
        assert_eq!(d.count(Layer::Outline), 4);
        assert_eq!(d.count(Layer::Holes), 0);
        assert_eq!(d.count(Layer::Axes), 2);
        let ((x0, y0), (x1, y1)) = d.bounds();
        assert_eq!((x0, y0), (-3.0, -3.0));
        assert_eq!((x1, y1), (107.0, 67.0));
    }

    #[test]
    fn the_front_and_the_side_have_the_height_of_the_box() {
        let e = model();
        assert_eq!(
            Drawing::of(&e, View::Front, &settings()).unwrap().extent,
            (104.0, 35.0)
        );
        assert_eq!(
            Drawing::of(&e, View::Side, &settings()).unwrap().extent,
            (64.0, 35.0)
        );
    }

    #[test]
    fn a_hole_is_drawn_in_the_view_of_its_own_face_only() {
        let mut e = model();
        hole(&mut e, 1, Face::Front, 30, 17, 6, HoleStyle::Plain);
        let front = Drawing::of(&e, View::Front, &settings()).unwrap();
        assert_eq!(front.count(Layer::Holes), 1);
        // the box centre lines and the two lines through the hole
        assert_eq!(front.count(Layer::Axes), 4);
        assert!(front
            .entities
            .contains(&(Layer::Holes, Entity::Circle((30.0, 17.0), 3.0))));
        assert_eq!(
            Drawing::of(&e, View::Top, &settings())
                .unwrap()
                .count(Layer::Holes),
            0
        );
        assert_eq!(
            Drawing::of(&e, View::Side, &settings())
                .unwrap()
                .count(Layer::Holes),
            0
        );
    }

    #[test]
    fn every_style_of_hole_has_its_outline() {
        let mut e = model();
        let big = Length::from_mm(8).unwrap();
        hole(
            &mut e,
            1,
            Face::Top,
            20,
            20,
            3,
            HoleStyle::Countersunk { head_diameter: big },
        );
        hole(
            &mut e,
            2,
            Face::Top,
            40,
            20,
            3,
            HoleStyle::Counterbored {
                head_diameter: big,
                depth: mmv(1),
            },
        );
        hole(
            &mut e,
            3,
            Face::Top,
            60,
            20,
            4,
            HoleStyle::Rectangular { height: mmv(8) },
        );
        hole(
            &mut e,
            4,
            Face::Top,
            80,
            20,
            4,
            HoleStyle::Oval {
                length: mmv(10),
                vertical: false,
            },
        );
        let d = Drawing::of(&e, View::Top, &settings()).unwrap();
        // two circles, two circles, four lines, two lines and two arcs
        assert_eq!(d.count(Layer::Holes), 12);
        assert!(d
            .entities
            .contains(&(Layer::Holes, Entity::Arc((83.0, 20.0), 2.0, 270.0, 450.0))));
        assert!(d.notes.iter().any(|n| n.text == "\u{2300}3 / \u{2300}8"));
        assert!(d.notes.iter().any(|n| n.text == "4\u{d7}10"));
    }

    #[test]
    fn rounded_corners_are_four_arcs_and_chamfers_are_four_slanted_lines() {
        let mut e = model();
        e.apply(Operation::SetEdging(crate::enclosure::Edging {
            vertical: Treatment::Round(Length::new(1_500_000).unwrap()),
            ..Default::default()
        }))
        .unwrap();
        let d = Drawing::of(&e, View::Top, &settings()).unwrap();
        assert_eq!(d.count(Layer::Outline), 8);
        assert!(d
            .entities
            .contains(&(Layer::Outline, Entity::Arc((102.5, 1.5), 1.5, 270.0, 360.0))));
        e.apply(Operation::SetEdging(crate::enclosure::Edging {
            vertical: Treatment::Chamfer(Length::new(1_500_000).unwrap()),
            ..Default::default()
        }))
        .unwrap();
        let c = Drawing::of(&e, View::Top, &settings()).unwrap();
        assert_eq!(c.count(Layer::Outline), 8);
        assert!(c
            .entities
            .contains(&(Layer::Outline, Entity::Line((102.5, 0.0), (104.0, 1.5)))));
    }

    #[test]
    fn the_dxf_file_has_the_three_layers_and_the_entities_in_millimetres() {
        let mut e = model();
        hole(&mut e, 1, Face::Front, 30, 17, 6, HoleStyle::Plain);
        let d = Drawing::of(&e, View::Front, &settings()).unwrap();
        let dxf = d.to_dxf();
        assert!(dxf.starts_with("0\nSECTION\n2\nHEADER\n"));
        assert!(dxf.ends_with("0\nENDSEC\n0\nEOF\n"));
        for layer in ["CONTORNO", "AGUJEROS", "EJES"] {
            assert!(dxf.contains(&format!("2\n{layer}\n70\n0\n")), "{layer}");
        }
        assert!(dxf.contains("$INSUNITS\n70\n4\n"));
        assert_eq!(dxf.matches("\n0\nLINE\n").count(), 4 + 4);
        assert_eq!(dxf.matches("\n0\nCIRCLE\n").count(), 1);
        assert!(dxf.contains(
            "0\nCIRCLE\n8\nAGUJEROS\n10\n30.000000\n20\n17.000000\n30\n0.0\n40\n3.000000\n"
        ));
        // every group is a code line and a value line
        assert_eq!(dxf.lines().count() % 2, 0);
    }

    #[test]
    fn the_svg_file_is_scaled_to_the_page_and_has_the_dimensions() {
        let mut e = model();
        hole(&mut e, 1, Face::Front, 30, 17, 6, HoleStyle::Plain);
        let d = Drawing::of(&e, View::Front, &settings()).unwrap();
        let svg = d.to_svg(&settings());
        assert!(svg.starts_with("<?xml"));
        assert!(svg.contains("width=\"200mm\""));
        // 104 + 6 + 28 wide, 35 + 6 + 28 high: 200 / 138 per mm
        assert!(svg.contains("viewBox=\"0 0 138 69\""), "{svg}");
        for id in ["CONTORNO", "AGUJEROS", "EJES", "COTAS"] {
            assert!(svg.contains(&format!("id=\"{id}\"")), "{id}");
        }
        assert!(svg.contains(">104</text>"));
        assert!(svg.contains(">35</text>"));
        assert!(svg.contains(">\u{2300}6</text>"));
        assert!(svg.contains(">1:0.69</text>"), "{svg}");
        assert!(svg.trim_end().ends_with("</svg>"));
        let mut plain = settings();
        plain.dimensions = false;
        let bare = d.to_svg(&plain);
        assert!(!bare.contains("id=\"COTAS\""));
        assert_eq!(d.to_svg(&settings()), svg);
    }

    #[test]
    fn a_drawing_with_more_entities_than_the_settings_allow_is_refused() {
        let mut s = settings();
        s.max_entities = 5;
        let err = Drawing::of(&model(), View::Top, &s).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-050");
    }

    #[test]
    fn the_names_of_the_views_are_the_words_of_the_command_line() {
        for v in View::ALL {
            assert_eq!(View::from_name(v.name()), Some(v));
        }
        assert_eq!(View::from_name("back"), None);
        assert_eq!(View::Side.face(), Face::Right);
    }
}
