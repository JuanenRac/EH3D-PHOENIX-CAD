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
//! layers, and a fourth `COTAS` with the dimensions) and the SVG file (scaled to the width of the
//! page of the settings, with the dimensions when the settings ask for them). The dimensions are
//! the width and the height of the box and the position of every hole of the face the view looks
//! at, measured from the lower left corner and stacked outwards with the whole box outermost. Both
//! are text that is the same for the same model. Neither has been opened in a CAD program or a
//! drawing program.

use crate::config::DrawingSettings;
use crate::enclosure::{Enclosure, Face, FeatureKind, Hole, HoleStyle, Treatment};
use crate::lang::{fill, Lang, Msg};
use crate::scad::mm_text;
use crate::schedule;
use crate::solid::Frame;
use crate::titleblock::{self, Block, Item, TitleBlockSettings};
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
    /// The hatching of a section.
    Hatch,
    /// The cavity of the box, which the view does not see, as dashed lines.
    Hidden,
    /// The border drawn round the views, their dimensions and their legend.
    Frame,
}

impl Layer {
    /// The name of the layer in the files.
    pub fn name(self) -> &'static str {
        match self {
            Layer::Outline => "CONTORNO",
            Layer::Holes => "AGUJEROS",
            Layer::Axes => "EJES",
            Layer::Hatch => "RAYADO",
            Layer::Hidden => "OCULTAS",
            Layer::Frame => "MARCO",
        }
    }

    /// The colour of the layer in the DXF file (white or black, blue, red).
    fn dxf_colour(self) -> u8 {
        match self {
            Layer::Outline => 7,
            Layer::Holes => 5,
            Layer::Axes => 1,
            Layer::Hatch => 8,
            Layer::Hidden => 6,
            Layer::Frame => 7,
        }
    }

    /// The colour of the layer in the SVG file.
    fn svg_colour(self) -> &'static str {
        match self {
            Layer::Outline => "#111111",
            Layer::Holes => "#1f4fd8",
            Layer::Axes => "#d03030",
            Layer::Hatch => "#8a8a8a",
            Layer::Hidden => "#7a3f9d",
            Layer::Frame => "#333333",
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

/// The room between the line a dimension measures and its dimension line, in millimetres.
const DIM_OFFSET: f64 = 8.0;

/// The room between two dimension lines one above the other, in millimetres.
const DIM_STEP: f64 = 6.0;

/// The height of the text of a dimension in the DXF file, in millimetres.
const DIM_TEXT: f64 = 2.5;

/// The layer of the dimensions in the DXF file.
pub const DIMENSION_LAYER: &str = "COTAS";

/// The level that marks a note as a line of the legend (a label, not a dimension).
pub const LEGEND_LEVEL: u32 = u32::MAX;

/// How wide a character of the legend is taken to be when the SVG page is made wide enough for it,
/// in millimetres.
const LEGEND_CHAR: f64 = 2.0;

/// A dimension or a label of the drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// Where the text goes.
    pub at: (f64, f64),
    /// The text.
    pub text: String,
    /// The line it measures, when it is a dimension: the two ends.
    pub measures: Option<((f64, f64), (f64, f64))>,
    /// How many dimension lines lie between the measured line and this one (0 is the nearest).
    pub level: u32,
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
    /// Whether the DXF file has the dimensions of `notes` (on the layer `COTAS`).
    pub dxf_dimensions: bool,
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
        // the cavity, which the view does not see: the inside of the walls, the floor and the lid
        if settings.hidden_lines {
            let (il, iw, ih) = (
                mm(params.inner_length.get()),
                mm(params.inner_width.get()),
                mm(params.inner_height.get()),
            );
            let (wall, floor) = (mm(params.wall.get()), mm(params.floor.get()));
            let (lo, hi) = match view {
                View::Top => ((wall, wall), (wall + il, wall + iw)),
                View::Front => ((wall, floor), (wall + il, floor + ih)),
                View::Side => ((wall, floor), (wall + iw, floor + ih)),
            };
            entities.extend(rect_lines(Layer::Hidden, lo, hi));
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
        for body in enclosure.turned().values() {
            let (lo, hi) = body.bounds()?;
            let a = [lo[0] as f64, lo[1] as f64, lo[2] as f64];
            let b = [hi[0] as f64, hi[1] as f64, hi[2] as f64];
            entities.extend(box_in_view(view, a, b));
        }
        for body in enclosure.swept().values() {
            let (lo, hi) = body.bounds()?;
            let a = [lo[0] as f64, lo[1] as f64, lo[2] as f64];
            let b = [hi[0] as f64, hi[1] as f64, hi[2] as f64];
            entities.extend(box_in_view(view, a, b));
        }
        // the openings of the face that the view looks at, and the centre lines
        let over = mm(settings.axis_overshoot.get());
        // the positions of the holes of this face, each once, to dimension from the lower left
        // corner: the nearest dimension line is the nearest hole and the whole box is the last
        let (mut across, mut up): (Vec<i64>, Vec<i64>) = (Vec::new(), Vec::new());
        if settings.hole_dimensions {
            for hole in enclosure.all_holes() {
                if hole.face == near {
                    across.push(hole.u.get());
                    up.push(hole.v.get());
                }
            }
            for list in [&mut across, &mut up] {
                list.sort_unstable();
                list.dedup();
            }
            if across.len() + up.len() > settings.max_dimensions as usize {
                return Err(CadError::ExportLimit(format!(
                    "the drawing has {} dimensions of positions and the settings allow {}",
                    across.len() + up.len(),
                    settings.max_dimensions
                )));
            }
        }
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
                level: 0,
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
        for (i, u) in across.iter().enumerate() {
            notes.push(Note {
                at: (mm(*u) / 2.0, 0.0),
                text: mm_text(*u),
                measures: Some(((0.0, 0.0), (mm(*u), 0.0))),
                level: i as u32,
            });
        }
        for (i, v) in up.iter().enumerate() {
            notes.push(Note {
                at: (0.0, mm(*v) / 2.0),
                text: mm_text(*v),
                measures: Some(((0.0, 0.0), (0.0, mm(*v)))),
                level: i as u32,
            });
        }
        notes.push(Note {
            at: (w / 2.0, 0.0),
            text: mm_text(match view {
                View::Top | View::Front => x.get(),
                View::Side => y.get(),
            }),
            measures: Some(((0.0, 0.0), (w, 0.0))),
            level: across.len() as u32,
        });
        notes.push(Note {
            at: (0.0, h / 2.0),
            text: mm_text(match view {
                View::Top => y.get(),
                View::Front | View::Side => z.get(),
            }),
            measures: Some(((0.0, 0.0), (0.0, h))),
            level: up.len() as u32,
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
            dxf_dimensions: settings.dxf_dimensions,
        })
    }

    /// Adds the legend to the right of the box: the title, the size of the box, the thickness of
    /// its walls, floor and lid and, for the holes of the face the view looks at, one line each
    /// with its tag (the one of the hole schedule), its diameter (or width) and its position, up
    /// to `drawing.max_dimensions` of them. The lines are labels at the level [`LEGEND_LEVEL`], so
    /// they are written where the dimensions are.
    pub fn add_legend(&mut self, enclosure: &Enclosure, settings: &DrawingSettings, lang: Lang) {
        let params = enclosure.params();
        let (x, y, z) = params.outer();
        let mut lines = vec![
            Msg::LegendTitle.tr(lang).to_string(),
            fill(
                Msg::LegendBox.tr(lang),
                &[&mm_text(x.get()), &mm_text(y.get()), &mm_text(z.get())],
            ),
            fill(
                Msg::LegendThickness.tr(lang),
                &[
                    &mm_text(params.wall.get()),
                    &mm_text(params.floor.get()),
                    &mm_text(params.lid.get()),
                ],
            ),
        ];
        let near = self.view.face().name();
        let rows: Vec<_> = schedule::rows(enclosure)
            .into_iter()
            .filter(|r| r.face == near)
            .collect();
        let shown = settings.max_dimensions as usize;
        for r in rows.iter().take(shown) {
            let size = match r.shape {
                HoleStyle::Rectangular { .. } | HoleStyle::Oval { .. } => mm_text(r.diameter),
                _ => format!("\u{2300}{}", mm_text(r.diameter)),
            };
            lines.push(fill(
                Msg::LegendHole.tr(lang),
                &[&r.tag, &size, &mm_text(r.u), &mm_text(r.v)],
            ));
        }
        if rows.len() > shown {
            lines.push(fill(
                Msg::LegendMore.tr(lang),
                &[&(rows.len() - shown).to_string()],
            ));
        }
        let left = self.extent.0 + mm(settings.legend_gap.get());
        for (i, text) in lines.into_iter().enumerate() {
            self.notes.push(Note {
                at: (left, self.extent.1 - DIM_TEXT - 2.0 * DIM_TEXT * i as f64),
                text,
                measures: None,
                level: LEGEND_LEVEL,
            });
        }
    }

    /// How far to the right the legend reaches, an estimate from the number of characters of its
    /// lines (the very least number when there is none).
    fn legend_right(&self) -> f64 {
        self.notes
            .iter()
            .filter(|n| n.level == LEGEND_LEVEL)
            .map(|n| n.at.0 + n.text.chars().count() as f64 * LEGEND_CHAR)
            .fold(f64::MIN, f64::max)
    }

    /// How far the dimension lines (and their text) reach under and to the left of the drawing, in
    /// millimetres, whether or not the files will carry them (zero when nothing is dimensioned).
    fn dimension_room(&self) -> f64 {
        if self.notes.iter().any(|n| n.measures.is_some()) {
            DIM_OFFSET + DIM_STEP * f64::from(self.levels()) + 2.0 * DIM_TEXT
        } else {
            0.0
        }
    }

    /// Draws a border (layer `MARCO`) round everything the drawing holds: what is drawn, the
    /// dimension lines under and to the left of it, and the legend to the right of it, with
    /// `margin` millimetres of air on every side. A second call draws a second border outside the
    /// first, so a drawing that already has one is left as it is.
    pub fn add_frame(&mut self, margin: f64) {
        if self.count(Layer::Frame) > 0 || self.entities.is_empty() {
            return;
        }
        let (lo, mut hi) = self.bounds();
        let room = self.dimension_room();
        let legend = self.legend_right();
        if legend > hi.0 {
            hi.0 = legend;
        }
        let lo = (lo.0 - margin - room, lo.1 - margin - room);
        let hi = (hi.0 + margin, hi.1 + margin);
        self.entities.extend(rect_lines(Layer::Frame, lo, hi));
    }

    /// The most dimension lines that lie one above the other.
    fn levels(&self) -> u32 {
        self.notes
            .iter()
            .filter(|n| n.measures.is_some())
            .map(|n| n.level)
            .max()
            .unwrap_or(0)
    }

    /// How much room the dimensions of the DXF file take under the drawing, in millimetres (zero
    /// without them): the gap to leave before a title block so that it does not lie on them.
    pub fn dimension_reach(&self) -> f64 {
        if self.dxf_dimensions && self.notes.iter().any(|n| n.measures.is_some()) {
            DIM_OFFSET + DIM_STEP * f64::from(self.levels()) + 2.0 * DIM_TEXT
        } else {
            0.0
        }
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

    /// The title block of this drawing: under the box, with its upper right corner at the right
    /// edge of the drawing. `gap` is the room left between the box and the block, `fallback` the
    /// title when the settings have none and `scale` the text of the scale.
    pub fn title_block(
        &self,
        settings: &TitleBlockSettings,
        fallback: &str,
        scale: &str,
        gap: f64,
        lang: Lang,
    ) -> Block {
        let (lo, hi) = self.bounds();
        titleblock::layout(settings, fallback, scale, (hi.0, lo.1 - gap), lang)
    }

    /// The DXF file (R12 text, millimetres).
    pub fn to_dxf(&self) -> String {
        self.to_dxf_with(None)
    }

    /// The DXF file with a title block (layer `ROTULO`) when one is given.
    pub fn to_dxf_with(&self, block: Option<&Block>) -> String {
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
        let hidden = self.count(Layer::Hidden) > 0;
        put(0, "TABLE");
        put(2, "LTYPE");
        put(70, if hidden { "2" } else { "1" });
        put(0, "LTYPE");
        put(2, "CONTINUOUS");
        put(70, "0");
        put(3, "Solid line");
        put(72, "65");
        put(73, "0");
        put(40, "0.0");
        if hidden {
            put(0, "LTYPE");
            put(2, "DASHED");
            put(70, "0");
            put(3, "Dashed line");
            put(72, "65");
            put(73, "2");
            put(40, "4.5");
            put(49, "3.0");
            put(49, "-1.5");
        }
        put(0, "ENDTAB");
        put(0, "TABLE");
        put(2, "LAYER");
        let dimensioned = self.dxf_dimensions && !self.notes.is_empty();
        let hatched = self.count(Layer::Hatch) > 0;
        let framed = self.count(Layer::Frame) > 0;
        let layers = 3
            + usize::from(hatched)
            + usize::from(framed)
            + usize::from(hidden)
            + usize::from(block.is_some())
            + usize::from(dimensioned);
        put(70, &layers.to_string());
        for layer in [
            Layer::Outline,
            Layer::Holes,
            Layer::Axes,
            Layer::Hatch,
            Layer::Hidden,
            Layer::Frame,
        ] {
            if (layer == Layer::Hatch && !hatched)
                || (layer == Layer::Hidden && !hidden)
                || (layer == Layer::Frame && !framed)
            {
                continue;
            }
            put(0, "LAYER");
            put(2, layer.name());
            put(70, "0");
            put(62, &layer.dxf_colour().to_string());
            put(
                6,
                if layer == Layer::Hidden {
                    "DASHED"
                } else {
                    "CONTINUOUS"
                },
            );
        }
        if dimensioned {
            put(0, "LAYER");
            put(2, DIMENSION_LAYER);
            put(70, "0");
            put(62, "3");
            put(6, "CONTINUOUS");
        }
        if block.is_some() {
            put(0, "LAYER");
            put(2, titleblock::LAYER);
            put(70, "0");
            put(62, "7");
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
        if dimensioned {
            for note in &self.notes {
                let mut line = |a: (f64, f64), b: (f64, f64)| {
                    put(0, "LINE");
                    put(8, DIMENSION_LAYER);
                    put(10, &n(a.0));
                    put(20, &n(a.1));
                    put(30, "0.0");
                    put(11, &n(b.0));
                    put(21, &n(b.1));
                    put(31, "0.0");
                };
                let (at, rotation) = match note.measures {
                    Some((a, b)) => {
                        let off = DIM_OFFSET + DIM_STEP * f64::from(note.level);
                        if a.1 == b.1 {
                            let y = a.1 - off;
                            line((a.0, y), (b.0, y));
                            line((a.0, a.1), (a.0, y));
                            line((b.0, b.1), (b.0, y));
                            (((a.0 + b.0) / 2.0, y + 1.0), 0.0)
                        } else {
                            let x = a.0 - off;
                            line((x, a.1), (x, b.1));
                            line((a.0, a.1), (x, a.1));
                            line((b.0, b.1), (x, b.1));
                            ((x - 1.0, (a.1 + b.1) / 2.0), 90.0)
                        }
                    }
                    None => (note.at, 0.0),
                };
                put(0, "TEXT");
                put(8, DIMENSION_LAYER);
                put(10, &n(at.0));
                put(20, &n(at.1));
                put(30, "0.0");
                put(40, &n(DIM_TEXT));
                put(1, &titleblock::dxf_text(&note.text));
                if rotation != 0.0 {
                    put(50, &n(rotation));
                }
                if note.measures.is_some() {
                    // the text is centred on the point
                    put(72, "1");
                    put(11, &n(at.0));
                    put(21, &n(at.1));
                    put(31, "0.0");
                }
            }
        }
        if let Some(block) = block {
            for item in &block.items {
                match item {
                    Item::Line(a, b) => {
                        put(0, "LINE");
                        put(8, titleblock::LAYER);
                        put(10, &n(a.0));
                        put(20, &n(a.1));
                        put(30, "0.0");
                        put(11, &n(b.0));
                        put(21, &n(b.1));
                        put(31, "0.0");
                    }
                    Item::Text { at, height, text } => {
                        put(0, "TEXT");
                        put(8, titleblock::LAYER);
                        put(10, &n(at.0));
                        put(20, &n(at.1));
                        put(30, "0.0");
                        put(40, &n(*height));
                        put(1, &titleblock::dxf_text(text));
                    }
                }
            }
        }
        put(0, "ENDSEC");
        put(0, "EOF");
        o
    }

    /// The SVG file, scaled to the width of the page of the settings.
    pub fn to_svg(&self, settings: &DrawingSettings) -> String {
        self.to_svg_with(settings, None)
    }

    /// The SVG file with a title block under the drawing when the settings of the title block
    /// are given with the title to use when they have none and the language of the cells. The
    /// page grows to hold the block.
    pub fn to_svg_with(
        &self,
        settings: &DrawingSettings,
        title: Option<(&TitleBlockSettings, &str, Lang)>,
    ) -> String {
        let (mut lo, mut hi) = self.bounds();
        if settings.dimensions {
            hi.0 = hi.0.max(self.legend_right());
        }
        let stack = DIM_STEP * f64::from(self.levels());
        let gap = if settings.dimensions {
            16.0 + stack
        } else {
            6.0
        };
        if let Some((tb, fallback, lang)) = title {
            let shape = self.title_block(tb, fallback, "", gap, lang);
            lo = (lo.0.min(shape.lo.0), lo.1.min(shape.lo.1));
            hi = (hi.0.max(shape.hi.0), hi.1.max(shape.hi.1));
        }
        let margin = if settings.dimensions {
            14.0 + stack
        } else {
            4.0
        };
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
        for layer in [
            Layer::Outline,
            Layer::Holes,
            Layer::Axes,
            Layer::Hatch,
            Layer::Hidden,
            Layer::Frame,
        ] {
            let always = matches!(layer, Layer::Outline | Layer::Holes | Layer::Axes);
            if !always && self.count(layer) == 0 {
                continue;
            }
            let dash = if layer == Layer::Axes {
                format!(
                    " stroke-dasharray=\"{} {}\"",
                    num(6.0 / scale),
                    num(1.5 / scale)
                )
            } else if layer == Layer::Hidden {
                format!(
                    " stroke-dasharray=\"{} {}\"",
                    num(3.0 / scale),
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
                        let off = DIM_OFFSET + DIM_STEP * f64::from(note.level);
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
        if let Some((tb, fallback, lang)) = title {
            let block = self.title_block(tb, fallback, &format!("1:{:.2}", 1.0 / scale), gap, lang);
            let _ = writeln!(
                o,
                "<g id=\"{}\" fill=\"#111111\" stroke=\"#111111\" stroke-width=\"{}\" font-family=\"sans-serif\">",
                titleblock::LAYER,
                num(line)
            );
            for item in &block.items {
                match item {
                    Item::Line(a, b) => {
                        let _ = writeln!(
                            o,
                            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
                            num(px(a.0)),
                            num(py(a.1)),
                            num(px(b.0)),
                            num(py(b.1))
                        );
                    }
                    Item::Text { at, height, text } => {
                        let _ = writeln!(
                            o,
                            "<text x=\"{}\" y=\"{}\" font-size=\"{}\" stroke=\"none\">{}</text>",
                            num(px(at.0)),
                            num(py(at.1)),
                            num(*height),
                            titleblock::xml_text(text)
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

    /// The settings without the dimensions in the DXF file.
    fn bare() -> DrawingSettings {
        DrawingSettings {
            dxf_dimensions: false,
            hidden_lines: false,
            ..settings()
        }
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
        let d = Drawing::of(&e, View::Front, &bare()).unwrap();
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

    /// The dimensions of a drawing as (text, level, horizontal).
    fn dimensions(d: &Drawing) -> Vec<(String, u32, bool)> {
        d.notes
            .iter()
            .filter_map(|n| {
                n.measures
                    .map(|(a, b)| (n.text.clone(), n.level, a.1 == b.1))
            })
            .collect()
    }

    #[test]
    fn the_cavity_of_the_box_is_a_dashed_rectangle_in_every_view() {
        // 100 x 60 x 30 inside, a wall of 2, a floor of 2 and a lid of 3
        let e = model();
        let on = settings();
        let rect = |lo: (f64, f64), hi: (f64, f64)| -> Vec<(Layer, Entity)> {
            rect_lines(Layer::Hidden, lo, hi)
        };
        for (view, lo, hi) in [
            (View::Top, (2.0, 2.0), (102.0, 62.0)),
            (View::Front, (2.0, 2.0), (102.0, 32.0)),
            (View::Side, (2.0, 2.0), (62.0, 32.0)),
        ] {
            let d = Drawing::of(&e, view, &on).unwrap();
            assert_eq!(d.count(Layer::Hidden), 4, "{view:?}");
            for line in rect(lo, hi) {
                assert!(d.entities.contains(&line), "{view:?} {line:?}");
            }
            // the outline of the box is what it was
            assert_eq!(
                d.count(Layer::Outline),
                Drawing::of(&e, view, &bare())
                    .unwrap()
                    .count(Layer::Outline)
            );
        }
        // the setting leaves them out
        let off = DrawingSettings {
            hidden_lines: false,
            ..settings()
        };
        assert_eq!(
            Drawing::of(&e, View::Top, &off)
                .unwrap()
                .count(Layer::Hidden),
            0
        );
    }

    #[test]
    fn the_hidden_lines_have_their_layer_and_a_dashed_line_type_in_the_files() {
        let e = model();
        let d = Drawing::of(&e, View::Front, &settings()).unwrap();
        let dxf = d.to_dxf();
        // a second line type and a layer that uses it, counted in their tables
        assert!(dxf.contains("2\nLTYPE\n70\n2\n"));
        assert!(dxf.contains("0\nLTYPE\n2\nDASHED\n70\n0\n3\nDashed line\n72\n65\n73\n2\n40\n4.5\n49\n3.0\n49\n-1.5\n"));
        assert!(dxf.contains("2\nOCULTAS\n70\n0\n62\n6\n6\nDASHED\n"));
        assert_eq!(dxf.matches("0\nLINE\n8\nOCULTAS\n").count(), 4);
        // without them the file is what it was: one line type and no such layer
        let plain = Drawing::of(&e, View::Front, &bare()).unwrap().to_dxf();
        assert!(plain.contains("2\nLTYPE\n70\n1\n"));
        assert!(!plain.contains("OCULTAS") && !plain.contains("DASHED"));
        // the SVG file has a group of the layer with a dash that the centre lines do not have
        let svg = d.to_svg(&settings());
        assert!(svg.contains("<g id=\"OCULTAS\" fill=\"none\" stroke=\"#7a3f9d\""));
        assert_eq!(svg.matches("stroke-dasharray").count(), 2);
        assert!(!Drawing::of(&e, View::Front, &bare())
            .unwrap()
            .to_svg(&settings())
            .contains("OCULTAS"));
    }

    fn legend(d: &Drawing) -> Vec<String> {
        d.notes
            .iter()
            .filter(|n| n.level == LEGEND_LEVEL)
            .map(|n| n.text.clone())
            .collect()
    }

    #[test]
    fn the_legend_lists_the_box_the_thickness_and_the_holes_of_the_face_of_the_view() {
        let mut e = model();
        hole(&mut e, 1, Face::Front, 60, 17, 6, HoleStyle::Plain);
        hole(
            &mut e,
            2,
            Face::Front,
            30,
            17,
            4,
            HoleStyle::Rectangular { height: mmv(3) },
        );
        hole(&mut e, 3, Face::Top, 10, 10, 3, HoleStyle::Plain);
        let mut d = Drawing::of(&e, View::Front, &settings()).unwrap();
        let before = dimensions(&d);
        d.add_legend(&e, &settings(), Lang::En);
        let lines = legend(&d);
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert_eq!(lines[0], "Dimensions (mm)");
        assert_eq!(lines[1], "box: 104 x 64 x 35");
        assert_eq!(lines[2], "wall 2, floor 2, lid 3");
        // the holes of the face of the view, with the tag of the hole schedule: a diameter for a
        // round hole and the width for a rectangle
        assert!(lines[3].ends_with(": 4 at 30, 17"), "{lines:?}");
        assert!(lines[4].ends_with(": \u{2300}6 at 60, 17"), "{lines:?}");
        let tags: Vec<&str> = lines[3..]
            .iter()
            .map(|l| &l[..l.find(':').unwrap()])
            .collect();
        assert!(tags.iter().all(|t| t.starts_with('H')), "{tags:?}");
        // the legend is on the right of the box, going down one line after the other, and it does
        // not change the dimensions
        let legend_notes: Vec<&Note> = d.notes.iter().filter(|n| n.level == LEGEND_LEVEL).collect();
        assert!(legend_notes
            .iter()
            .all(|n| n.at.0 == 104.0 + 10.0 && n.measures.is_none()));
        assert!(legend_notes.windows(2).all(|p| p[0].at.1 > p[1].at.1));
        assert_eq!(dimensions(&d), before);
        // the top view lists its own hole only
        let mut top = Drawing::of(&e, View::Top, &settings()).unwrap();
        top.add_legend(&e, &settings(), Lang::Es);
        let es = legend(&top);
        assert_eq!(es.len(), 4, "{es:?}");
        assert_eq!(es[0], "Cotas (mm)");
        assert_eq!(es[1], "caja: 104 x 64 x 35");
        assert_eq!(es[2], "pared 2, suelo 2, tapa 3");
        assert!(es[3].ends_with(": \u{2300}3 en 10, 10"), "{es:?}");
        // more holes than the settings allow dimensions are counted and not listed
        let few = DrawingSettings {
            max_dimensions: 1,
            ..settings()
        };
        let mut front = Drawing::of(&e, View::Front, &settings()).unwrap();
        front.add_legend(&e, &few, Lang::En);
        let lines = legend(&front);
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[4], "1 more holes are not listed");
    }

    #[test]
    fn the_legend_is_in_the_files_where_the_dimensions_are_and_widens_the_svg_page() {
        let mut e = model();
        hole(&mut e, 1, Face::Front, 60, 17, 6, HoleStyle::Plain);
        let plain = Drawing::of(&e, View::Front, &settings()).unwrap();
        let mut d = plain.clone();
        d.add_legend(&e, &settings(), Lang::En);
        // the DXF file has the lines as text on the layer of the dimensions
        let dxf = d.to_dxf();
        assert!(dxf.contains("1\nDimensions (mm)\n"));
        assert!(dxf.contains("1\nbox: 104 x 64 x 35\n"));
        assert!(!plain.to_dxf().contains("Dimensions (mm)"));
        // without the dimensions in the DXF file the legend goes with them
        let mut off = d.clone();
        off.dxf_dimensions = false;
        assert!(!off.to_dxf().contains("Dimensions (mm)"));
        // the SVG page grows to the right to hold it, and does not when the settings leave the
        // dimensions out
        let svg = d.to_svg(&settings());
        assert!(svg.contains(">Dimensions (mm)</text>"));
        let width = |text: &str| -> f64 {
            let at = text.find("viewBox=\"0 0 ").unwrap() + 13;
            text[at..].split(' ').next().unwrap().parse().unwrap()
        };
        assert!(width(&svg) > width(&plain.to_svg(&settings())));
        let none = DrawingSettings {
            dimensions: false,
            ..settings()
        };
        assert!(!d.to_svg(&none).contains("Dimensions (mm)"));
        assert_eq!(width(&d.to_svg(&none)), width(&plain.to_svg(&none)));
    }

    #[test]
    fn the_positions_of_the_holes_are_dimensioned_from_the_corner_with_the_whole_box_outermost() {
        let mut e = model();
        hole(&mut e, 1, Face::Front, 60, 17, 6, HoleStyle::Plain);
        hole(&mut e, 2, Face::Front, 30, 17, 4, HoleStyle::Plain);
        // a hole of another face is not in this view
        hole(&mut e, 3, Face::Top, 10, 10, 3, HoleStyle::Plain);
        let d = Drawing::of(&e, View::Front, &settings()).unwrap();
        // two positions across (30 and 60, nearest first), one up (17, shared by both holes),
        // then the box: 104 across at level 2 and 35 up at level 1
        assert_eq!(
            dimensions(&d),
            vec![
                ("30".to_owned(), 0, true),
                ("60".to_owned(), 1, true),
                ("17".to_owned(), 0, false),
                ("104".to_owned(), 2, true),
                ("35".to_owned(), 1, false),
            ]
        );
        assert!(d.notes.iter().any(|n| n.text == "\u{2300}6"));
        // the top view has its own hole only
        let top = Drawing::of(&e, View::Top, &settings()).unwrap();
        assert_eq!(
            dimensions(&top),
            vec![
                ("10".to_owned(), 0, true),
                ("10".to_owned(), 0, false),
                ("104".to_owned(), 1, true),
                ("64".to_owned(), 1, false),
            ]
        );
        // the setting that leaves them out gives back the dimensions of the box alone
        let mut off = settings();
        off.hole_dimensions = false;
        let plain = Drawing::of(&e, View::Front, &off).unwrap();
        assert_eq!(
            dimensions(&plain),
            vec![("104".to_owned(), 0, true), ("35".to_owned(), 0, false)]
        );
        // more positions than the settings allow are refused
        let mut few = settings();
        few.max_dimensions = 2;
        let err = Drawing::of(&e, View::Front, &few).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-050");
        few.max_dimensions = 3;
        assert!(Drawing::of(&e, View::Front, &few).is_ok());
    }

    #[test]
    fn the_dxf_file_has_the_dimensions_on_their_own_layer_when_the_settings_say_so() {
        let d = Drawing::of(&model(), View::Top, &settings()).unwrap();
        let dxf = d.to_dxf();
        // the three layers, the hidden lines and the dimensions
        assert!(dxf.contains("2\nLAYER\n70\n5\n"));
        assert!(dxf.contains("2\nCOTAS\n70\n0\n62\n3\n"));
        // three lines and a text for each of the two dimensions of the box
        assert_eq!(dxf.matches("8\nCOTAS\n").count(), 8);
        // the width at 8 mm under the box, with its two extension lines, and its text
        assert!(dxf.contains(
            "8\nCOTAS\n10\n0.000000\n20\n-8.000000\n30\n0.0\n11\n104.000000\n21\n-8.000000\n31\n0.0\n"
        ));
        assert!(dxf.contains(
            "8\nCOTAS\n10\n0.000000\n20\n0.000000\n30\n0.0\n11\n0.000000\n21\n-8.000000\n"
        ));
        assert!(dxf.contains("1\n104\n"));
        // the height on the left, with the text turned a quarter and centred
        assert!(dxf.contains(
            "8\nCOTAS\n10\n-8.000000\n20\n0.000000\n30\n0.0\n11\n-8.000000\n21\n64.000000\n"
        ));
        assert!(dxf.contains("1\n64\n50\n90.000000\n72\n1\n"));
        // every group is a code line and a value line, and the file ends as it did
        assert_eq!(dxf.lines().count() % 2, 0);
        assert!(dxf.ends_with("0\nENDSEC\n0\nEOF\n"));
        // the label of a hole is a text on the layer, with the diameter sign escaped
        let mut e = model();
        hole(&mut e, 1, Face::Front, 30, 17, 6, HoleStyle::Plain);
        let with_hole = Drawing::of(&e, View::Front, &settings()).unwrap().to_dxf();
        assert!(with_hole.is_ascii());
        // the dimension of the hole is at the first level and the box is outside it
        assert!(with_hole.contains("20\n-8.000000\n30\n0.0\n11\n30.000000\n21\n-8.000000\n"));
        assert!(with_hole.contains("20\n-14.000000\n30\n0.0\n11\n104.000000\n21\n-14.000000\n"));
        // without the setting the file has no dimensions and the three layers it had
        let none = Drawing::of(&model(), View::Top, &bare()).unwrap().to_dxf();
        assert!(!none.contains("COTAS"));
        assert!(none.contains("2\nLAYER\n70\n3\n"));
    }

    #[test]
    fn the_title_block_leaves_room_for_the_dimensions_of_the_dxf_file() {
        let mut e = model();
        hole(&mut e, 1, Face::Front, 30, 17, 6, HoleStyle::Plain);
        let d = Drawing::of(&e, View::Front, &settings()).unwrap();
        // one level of dimensions under the box: 8 + 6 for the second line and 5 for the text
        assert_eq!(d.dimension_reach(), 8.0 + 6.0 + 5.0);
        let plain = Drawing::of(&e, View::Front, &bare()).unwrap();
        assert_eq!(plain.dimension_reach(), 0.0);
        // a drawing that has no dimensions has no reach, as the drawings of a section do
        let nothing = Drawing {
            view: View::Top,
            entities: Vec::new(),
            notes: Vec::new(),
            extent: (0.0, 0.0),
            dxf_dimensions: true,
        };
        assert_eq!(nothing.dimension_reach(), 0.0);
        // the block of the export is put below the dimensions and not over them
        let gap = 6.0 + d.dimension_reach();
        let block = d.title_block(&block_settings(), "box", "1:1", gap, Lang::En);
        let (lo, _) = d.bounds();
        assert_eq!(block.hi.1, lo.1 - gap);
        assert!(block.hi.1 < -14.0 - 3.0);
    }

    #[test]
    fn the_svg_file_is_scaled_to_the_page_and_has_the_dimensions() {
        let mut e = model();
        hole(&mut e, 1, Face::Front, 30, 17, 6, HoleStyle::Plain);
        let d = Drawing::of(&e, View::Front, &settings()).unwrap();
        let svg = d.to_svg(&settings());
        assert!(svg.starts_with("<?xml"));
        assert!(svg.contains("width=\"200mm\""));
        // 104 + 6 + 40 wide, 35 + 6 + 40 high (a margin of 14 and 6 for the second level of
        // dimension lines, because the hole is dimensioned too): 200 / 150 per mm
        assert!(svg.contains("viewBox=\"0 0 150 81\""), "{svg}");
        for id in ["CONTORNO", "AGUJEROS", "EJES", "COTAS"] {
            assert!(svg.contains(&format!("id=\"{id}\"")), "{id}");
        }
        assert!(svg.contains(">104</text>"));
        assert!(svg.contains(">35</text>"));
        assert!(svg.contains(">\u{2300}6</text>"));
        assert!(svg.contains(">1:0.75</text>"), "{svg}");
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

    fn block_settings() -> TitleBlockSettings {
        TitleBlockSettings {
            enabled: true,
            organisation: "Workshop".into(),
            title: String::new(),
            number: "A-12".into(),
            revision: "B".into(),
            author: "Ana".into(),
            width: mmv(100),
            row: mmv(8),
        }
    }

    #[test]
    fn the_title_block_goes_under_the_drawing_in_the_dxf_file_on_its_own_layer() {
        let d = Drawing::of(&model(), View::Top, &bare()).unwrap();
        let block = d.title_block(&block_settings(), "box", "1:1", 6.0, Lang::En);
        // the drawing runs from -3 to 107 along x and from -3 to 67 along y
        assert_eq!(block.hi, (107.0, -9.0));
        assert_eq!(block.lo, (7.0, -41.0));
        let plain = d.to_dxf();
        assert!(!plain.contains("ROTULO"));
        assert_eq!(plain, d.to_dxf_with(None));
        let with = d.to_dxf_with(Some(&block));
        assert!(with.contains("2\nLAYER\n70\n4\n"));
        // the layer, eight lines and twelve texts
        assert_eq!(with.matches("ROTULO").count(), 1 + 8 + 12);
        assert_eq!(with.matches("\nTEXT\n").count(), 12);
        assert!(with.contains("1\nWorkshop\n"));
        assert!(with.contains("1\nDrawing number\n"));
        assert!(with.contains("1\nbox\n"));
        assert!(with.ends_with("0\nENDSEC\n0\nEOF\n"));
        // what was in the file before is still there
        assert!(with.contains("0\nLINE\n8\nCONTORNO\n"));
    }

    #[test]
    fn the_title_block_grows_the_svg_page_and_escapes_its_texts() {
        let d = Drawing::of(&model(), View::Top, &settings()).unwrap();
        let plain = d.to_svg(&settings());
        assert_eq!(plain, d.to_svg_with(&settings(), None));
        assert!(plain.contains("viewBox=\"0 0 138 98\""), "{plain}");
        let tb = block_settings();
        let svg = d.to_svg_with(&settings(), Some((&tb, "a & b", Lang::Es)));
        // 16 mm under the drawing and 4 rows of 8 mm: 48 mm more
        assert!(svg.contains("viewBox=\"0 0 138 146\""), "{svg}");
        assert!(svg.contains("<g id=\"ROTULO\""));
        assert!(svg.contains(">Organización</text>"));
        assert!(svg.contains(">Número de plano</text>"));
        assert!(svg.contains(">a &amp; b</text>"));
        assert!(svg.contains(">1:0.69</text>"), "{svg}");
        assert_eq!(
            svg.matches("<text").count(),
            plain.matches("<text").count() + 12
        );
        // a title given by the settings wins over the name
        let mut own = block_settings();
        own.title = "Front <panel>".into();
        let svg = d.to_svg_with(&settings(), Some((&own, "box", Lang::En)));
        assert!(svg.contains(">Front &lt;panel&gt;</text>"));
        assert!(!svg.contains(">box</text>"));
    }

    #[test]
    fn the_frame_goes_round_the_views_the_dimension_lines_and_the_legend() {
        let mut e = model();
        hole(&mut e, 1, Face::Front, 60, 17, 6, HoleStyle::Plain);
        let mut d = Drawing::of(&e, View::Front, &settings()).unwrap();
        d.add_legend(&e, &settings(), Lang::En);
        let ((lx, ly), (hx, _)) = d.bounds();
        let right = d.legend_right();
        let room = d.dimension_room();
        assert!(room > 0.0);
        d.add_frame(5.0);
        assert_eq!(d.count(Layer::Frame), 4);
        let frame: Vec<_> = d
            .entities
            .iter()
            .filter(|(l, _)| *l == Layer::Frame)
            .collect();
        let xs: Vec<f64> = frame
            .iter()
            .flat_map(|(_, e)| match *e {
                Entity::Line(a, b) => vec![a.0, b.0],
                _ => vec![],
            })
            .collect();
        let ys: Vec<f64> = frame
            .iter()
            .flat_map(|(_, e)| match *e {
                Entity::Line(a, b) => vec![a.1, b.1],
                _ => vec![],
            })
            .collect();
        let min = |v: &[f64]| v.iter().copied().fold(f64::MAX, f64::min);
        let max = |v: &[f64]| v.iter().copied().fold(f64::MIN, f64::max);
        assert_eq!(min(&xs), lx - 5.0 - room);
        assert_eq!(min(&ys), ly - 5.0 - room);
        assert_eq!(max(&xs), right.max(hx) + 5.0);
        // the legend is inside it, further right than the box
        assert!(right > hx);
    }

    #[test]
    fn a_second_frame_is_not_drawn_and_an_empty_drawing_has_none() {
        let mut d = Drawing::of(&model(), View::Top, &settings()).unwrap();
        d.add_frame(2.0);
        let n = d.entities.len();
        d.add_frame(9.0);
        assert_eq!(d.entities.len(), n);
        let mut empty = d.clone();
        empty.entities.clear();
        empty.add_frame(2.0);
        assert_eq!(empty.count(Layer::Frame), 0);
    }

    #[test]
    fn the_frame_has_its_layer_in_the_dxf_and_its_group_in_the_svg_and_only_when_asked() {
        let e = model();
        let plain = Drawing::of(&e, View::Top, &settings()).unwrap();
        assert!(!plain.to_dxf().contains("MARCO"));
        assert!(!plain.to_svg(&settings()).contains("MARCO"));
        let mut d = plain.clone();
        d.add_frame(5.0);
        let dxf = d.to_dxf();
        // the layer is counted in its table, with its own line each side
        assert!(dxf.contains("2
MARCO
70
0
62
7
6
CONTINUOUS
"));
        assert_eq!(dxf.matches("0
LINE
8
MARCO
").count(), 4);
        let layers = |t: &str| t.matches("0
LAYER
2
").count();
        assert_eq!(layers(&dxf), layers(&plain.to_dxf()) + 1);
        let svg = d.to_svg(&settings());
        assert!(svg.contains("<g id=\"MARCO\""));
        assert_eq!(svg.matches("<line").count(), plain.to_svg(&settings()).matches("<line").count() + 4);
    }

    #[test]
    fn the_settings_ask_for_the_frame_and_it_is_off_by_default() {
        assert!(!settings().frame);
        assert_eq!(settings().frame_margin.get(), 5_000_000);
        let own = crate::config::Settings::with_user_config(
            "[drawing]
frame = 1
frame_margin = 12 mm
",
        )
        .unwrap();
        assert!(own.drawing.frame);
        assert_eq!(own.drawing.frame_margin.get(), 12_000_000);
        for bad in [
            "[drawing]
frame = 2
",
            "[drawing]
frame_margin = -1 mm
",
            "[drawing]
frame_margin = 1001 mm
",
        ] {
            assert!(crate::config::Settings::with_user_config(bad).is_err(), "{bad}");
        }
    }
}
