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

use crate::assembly::Dim;
use crate::config::DrawingSettings;
use crate::enclosure::{Enclosure, Face, FeatureKind, Hole, HoleStyle, Treatment};
use crate::lang::{fill, Lang, Msg};
use crate::pdf::{Anchor, Canvas, POINTS_PER_MM};
use crate::scad::mm_text;
use crate::schedule;
use crate::solid::Frame;
use crate::titleblock::{self, Block, Item, TitleBlockSettings};
use crate::{CadError, Length};
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

/// How far the cutting line goes beyond the box at both ends, in millimetres.
const CUT_OVERSHOOT: f64 = 12.0;

/// The length of the arrow at the end of the cutting line, in millimetres.
const CUT_ARROW: f64 = 6.0;

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
        lines.extend(hole_lines(enclosure, self.view, settings, lang));
        self.put_legend(lines, settings);
    }

    /// Adds the legend of a sheet of views to the right of it: the title, the size of the box and
    /// the thickness of its walls, floor and lid once, and then, under the name of each view (the
    /// front, the plan and the side), the holes of the face that the view looks at, up to
    /// `drawing.max_dimensions` of them for each view. A view whose face has no hole has its name
    /// and no lines.
    pub fn add_sheet_legend(
        &mut self,
        enclosure: &Enclosure,
        settings: &DrawingSettings,
        lang: Lang,
    ) {
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
        for (view, name) in [
            (View::Front, Msg::LegendFront),
            (View::Top, Msg::ViewPlan),
            (View::Side, Msg::LegendSide),
        ] {
            lines.push(format!("{}:", name.tr(lang)));
            lines.extend(hole_lines(enclosure, view, settings, lang));
        }
        self.put_legend(lines, settings);
    }

    /// Draws the symbol of the projection under the lower left corner of the drawing, clear of
    /// the dimension lines: a truncated cone seen from the side (the large end `size` high, the
    /// small end half of that, 1.5 `size` long) and, apart from it, the two circles of its ends
    /// seen from the large end, with their centre line. With first-angle projection (`third` is
    /// false) the circles are to the right of the cone, with third-angle projection to its left.
    /// An empty drawing gets nothing.
    pub fn add_projection_symbol(&mut self, third: bool, size: f64) {
        if self.entities.is_empty() || !size.is_finite() || size <= 0.0 {
            return;
        }
        let (lo, _) = self.bounds();
        let top = lo.1 - self.dimension_room() - size * 0.5;
        let centre = top - size / 2.0;
        let (cone_from, circles_at) = if third {
            (1.6 * size, size / 2.0)
        } else {
            (0.0, 2.6 * size)
        };
        let x0 = lo.0;
        let cone_to = cone_from + 1.5 * size;
        let (small, large) = (size / 4.0, size / 2.0);
        let line = |a: (f64, f64), b: (f64, f64)| (Layer::Outline, Entity::Line(a, b));
        let at = |x: f64, y: f64| (x0 + x, y);
        self.entities.extend([
            line(at(cone_from, centre - small), at(cone_to, centre - large)),
            line(at(cone_from, centre + small), at(cone_to, centre + large)),
            line(at(cone_from, centre - small), at(cone_from, centre + small)),
            line(at(cone_to, centre - large), at(cone_to, centre + large)),
            (
                Layer::Outline,
                Entity::Circle(at(circles_at, centre), large),
            ),
            (
                Layer::Outline,
                Entity::Circle(at(circles_at, centre), small),
            ),
            (
                Layer::Axes,
                Entity::Line(at(0.0, centre), at(3.1 * size + 0.2 * size, centre)),
            ),
        ]);
    }

    /// Draws the cutting line of a plane across `axis` at `at` from the outer face of the box, the
    /// way a drawing marks where a section was taken: a line across the view on the layer of the
    /// axes (which the files draw dashed, as they draw the centre lines), a heavy mark at each end with an arrow that points to the
    /// side the section is looked from (the side of the larger coordinate), and the letters
    /// `label` beside both ends. The line goes a little beyond the box at both ends.
    ///
    /// Only a view that shows the plane edge-on can carry it: the plan shows the planes across
    /// `x` and `y`, the front those across `x` and `z`, the side those across `y` and `z`. False,
    /// and nothing drawn, when the view does not show it, when the plane lies outside the box or
    /// when the drawing is empty.
    pub fn add_cut_line(&mut self, axis: Dim, at: Length, label: &str) -> bool {
        let (across, up) = match self.view {
            View::Top => (Dim::X, Dim::Y),
            View::Front => (Dim::X, Dim::Z),
            View::Side => (Dim::Y, Dim::Z),
        };
        let vertical = if axis == across {
            true
        } else if axis == up {
            false
        } else {
            return false;
        };
        let p = mm(at.get());
        let (width, height) = self.extent;
        let along = if vertical { height } else { width };
        let limit = if vertical { width } else { height };
        if self.entities.is_empty() || p < 0.0 || p > limit {
            return false;
        }
        let over = CUT_OVERSHOOT;
        // the point of the line at `s` along it, in the coordinates of the view
        let at_s = |s: f64| if vertical { (p, s) } else { (s, p) };
        // one line on the layer of the axes, which the files draw as a dashed one
        let end = along + over;
        self.entities
            .push((Layer::Axes, Entity::Line(at_s(-over), at_s(end))));
        // the heavy marks and the arrows, at both ends
        let side = if vertical { (1.0, 0.0) } else { (0.0, 1.0) };
        for (end_s, outward) in [(-over, -1.0), (end, 1.0)] {
            let base = at_s(end_s);
            let tip = (base.0 + side.0 * CUT_ARROW, base.1 + side.1 * CUT_ARROW);
            let back = at_s(end_s - outward * CUT_ARROW * 0.5);
            // the mark lies along the line; the arrow is its shaft and a head of two strokes
            self.entities.push((
                Layer::Outline,
                Entity::Line(base, at_s(end_s - outward * 3.0)),
            ));
            self.entities
                .push((Layer::Outline, Entity::Line(base, tip)));
            let head = |k: f64| {
                (
                    tip.0 - side.0 * CUT_ARROW * 0.4 + (back.0 - base.0) * k,
                    tip.1 - side.1 * CUT_ARROW * 0.4 + (back.1 - base.1) * k,
                )
            };
            self.entities
                .push((Layer::Outline, Entity::Line(tip, head(1.0))));
            self.entities
                .push((Layer::Outline, Entity::Line(tip, head(-1.0))));
            self.notes.push(Note {
                at: at_s(end_s - outward * 5.0),
                text: label.to_owned(),
                measures: None,
                level: 0,
            });
        }
        true
    }

    /// Puts the drawing of a section beside the sheet: moved to the right of everything the sheet
    /// holds, `gap` millimetres from it, with its bottom on the ground line of the front view, and
    /// the letters of the cutting line twice (`label`, as in `A-A`) over it. False, and nothing
    /// drawn, when the section has no line.
    pub fn add_section_view(&mut self, section: Drawing, label: &str, gap: f64) -> bool {
        if section.entities.is_empty() {
            return false;
        }
        let (lo, hi) = section.bounds();
        let right = if self.entities.is_empty() {
            0.0
        } else {
            self.bounds().1 .0
        };
        let (dx, dy) = (right + gap - lo.0, -lo.1);
        let moved = section.shifted(dx, dy);
        self.entities.extend(moved.entities);
        self.notes.push(Note {
            at: (lo.0 + dx, hi.1 + dy + DIM_TEXT),
            text: label.to_owned(),
            measures: None,
            level: 0,
        });
        let (_, top_right) = self.bounds();
        self.extent = top_right;
        true
    }

    /// Writes the lines of a legend, one under the other, to the right of the box.
    fn put_legend(&mut self, lines: Vec<String>, settings: &DrawingSettings) {
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

    /// The sheet of paper that a DXF drawing would be printed on: the size of the paper of the
    /// A series (`paper` 1 to 5, upright with `portrait`) and the standard scale at which what is
    /// drawn, with its dimension lines, its legend and `extra` (the corners of something that is
    /// still to be added, such as the title block), fits inside `margin` millimetres of paper
    /// margin, put round the middle of all that. `None` when `paper` is no size or nothing is
    /// drawn. The drawing is not changed: [`Drawing::add_paper_sheet`] draws the sheet.
    pub fn paper_sheet(
        &self,
        paper: u8,
        portrait: bool,
        margin: f64,
        extra: Option<((f64, f64), (f64, f64))>,
    ) -> Option<PaperSheet> {
        let (paper_w, paper_h) = paper_size(paper, portrait)?;
        if self.entities.is_empty() {
            return None;
        }
        let (mut lo, mut hi) = self.bounds();
        if self.dxf_dimensions {
            let room = self.dimension_room();
            lo = (lo.0 - room, lo.1 - room);
            hi.0 = hi.0.max(self.legend_right());
        }
        if let Some((a, b)) = extra {
            lo = (lo.0.min(a.0), lo.1.min(a.1));
            hi = (hi.0.max(b.0), hi.1.max(b.1));
        }
        let room = 2.0 * margin.max(0.0);
        let fit = ((paper_w - room).max(1.0) / (hi.0 - lo.0).max(1e-9))
            .min((paper_h - room).max(1.0) / (hi.1 - lo.1).max(1e-9));
        let scale = standard_scale(fit);
        let (w, h) = (paper_w / scale, paper_h / scale);
        let middle = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
        Some(PaperSheet {
            scale,
            lo: (middle.0 - w / 2.0, middle.1 - h / 2.0),
            hi: (middle.0 + w / 2.0, middle.1 + h / 2.0),
            inset: margin.max(0.0) / scale,
        })
    }

    /// Draws a sheet of paper (see [`Drawing::paper_sheet`]) on the layer of the border: the edge
    /// of the paper and, inside it, the line of its margin. The drawing stays at full size, so the
    /// sheet is as many times larger than the paper as the scale says.
    pub fn add_paper_sheet(&mut self, sheet: &PaperSheet) {
        self.entities
            .extend(rect_lines(Layer::Frame, sheet.lo, sheet.hi));
        if sheet.inset > 0.0 {
            let inner_lo = (sheet.lo.0 + sheet.inset, sheet.lo.1 + sheet.inset);
            let inner_hi = (sheet.hi.0 - sheet.inset, sheet.hi.1 - sheet.inset);
            if inner_hi.0 > inner_lo.0 && inner_hi.1 > inner_lo.1 {
                self.entities
                    .extend(rect_lines(Layer::Frame, inner_lo, inner_hi));
            }
        }
    }

    /// The drawing moved by `dx` and `dy` millimetres: what is drawn, the dimensions and the labels.
    fn shifted(mut self, dx: f64, dy: f64) -> Drawing {
        let at = |p: (f64, f64)| (p.0 + dx, p.1 + dy);
        for (_, e) in &mut self.entities {
            *e = match *e {
                Entity::Line(a, b) => Entity::Line(at(a), at(b)),
                Entity::Circle(c, r) => Entity::Circle(at(c), r),
                Entity::Arc(c, r, a0, a1) => Entity::Arc(at(c), r, a0, a1),
            };
        }
        for n in &mut self.notes {
            n.at = at(n.at);
            n.measures = n.measures.map(|(a, b)| (at(a), at(b)));
        }
        self
    }

    /// One sheet with the plan, the front and the side, each with its own dimensions and centre
    /// lines. The front is the main view, at the origin; with `settings.sheet_angle` 1 (first
    /// angle, ISO) the plan is under it and the side, which looks from the right, is to its left;
    /// with 3 (third angle) the plan is over it and the side to its right. Between two views are
    /// `settings.sheet_gap` and the room that the dimension lines of the view next to the gap
    /// take, so that no dimension line lies on a neighbour. The legend, when it is added, is to
    /// the right of the whole sheet and lists the holes of the front. Refused with
    /// `PHOENIX-CAD-050` when the three views together have more entities than
    /// `drawing.max_entities`.
    pub fn sheet(enclosure: &Enclosure, settings: &DrawingSettings) -> Result<Drawing, CadError> {
        Drawing::sheet_cut(enclosure, settings, None)
    }

    /// The sheet of [`Drawing::sheet`] with a cutting line (the plane across `axis` at a position,
    /// with its letters) on the first view that shows that plane edge-on: the front when it does
    /// (the planes across `x` and `z`), else the plan (across `y`). The views are tried in that
    /// order and the line is drawn once. Without a cut it is the plain sheet.
    pub fn sheet_cut(
        enclosure: &Enclosure,
        settings: &DrawingSettings,
        cut: Option<(Dim, Length, &str)>,
    ) -> Result<Drawing, CadError> {
        let mut views = Drawing::placed(enclosure, settings, cut)?.into_iter();
        let mut sheet = views.next().expect("a sheet has a front");
        for part in views {
            sheet.entities.extend(part.entities);
            sheet.notes.extend(part.notes);
        }
        if sheet.entities.len() > settings.max_entities as usize {
            return Err(CadError::ExportLimit(format!(
                "the sheet has {} entities and the settings allow {}",
                sheet.entities.len(),
                settings.max_entities
            )));
        }
        let (_, hi) = sheet.bounds();
        sheet.extent = hi;
        Ok(sheet)
    }

    /// The front, the plan and the side, each moved to its place on the sheet.
    fn placed(
        enclosure: &Enclosure,
        settings: &DrawingSettings,
        cut: Option<(Dim, Length, &str)>,
    ) -> Result<Vec<Drawing>, CadError> {
        let mut front = Drawing::of(enclosure, View::Front, settings)?;
        let mut top = Drawing::of(enclosure, View::Top, settings)?;
        let mut side = Drawing::of(enclosure, View::Side, settings)?;
        if let Some((axis, at, label)) = cut {
            let _ = front.add_cut_line(axis, at, label)
                || top.add_cut_line(axis, at, label)
                || side.add_cut_line(axis, at, label);
        }
        let gap = mm(settings.sheet_gap.get());
        let (top_at, side_at) = if settings.sheet_angle == 3 {
            (
                (0.0, front.extent.1 + gap + top.dimension_room()),
                (front.extent.0 + gap + side.dimension_room(), 0.0),
            )
        } else {
            (
                (0.0, -(gap + front.dimension_room() + top.extent.1)),
                (-(gap + front.dimension_room() + side.extent.0), 0.0),
            )
        };
        Ok(vec![
            front,
            top.shifted(top_at.0, top_at.1),
            side.shifted(side_at.0, side_at.1),
        ])
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

    /// The layout of the page of the SVG and PDF files: the corners of what is drawn, the gap under the box, the margin, the width of the page in
    /// millimetres, the scale (millimetres of paper for each millimetre of the model), the size of the page in the units of the drawing and the
    /// offset that centres the drawing on it.
    fn page_layout(
        &self,
        settings: &DrawingSettings,
        title: Option<(&TitleBlockSettings, &str, Lang)>,
    ) -> PageLayout {
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
        let (content_w, content_h) = (hi.0 - lo.0 + 2.0 * margin, hi.1 - lo.1 + 2.0 * margin);
        let (page, scale, width, height, off_x, off_y) =
            match paper_size(settings.paper, settings.paper_portrait) {
                None => {
                    let page = mm(settings.svg_width.get());
                    (page, page / content_w, content_w, content_h, 0.0, 0.0)
                }
                Some((paper_w, paper_h)) => {
                    let room = 2.0 * mm(settings.paper_margin.get());
                    let fit = ((paper_w - room).max(1.0) / content_w)
                        .min((paper_h - room).max(1.0) / content_h);
                    let scale = standard_scale(fit);
                    let (w, h) = (paper_w / scale, paper_h / scale);
                    (
                        paper_w,
                        scale,
                        w,
                        h,
                        (w - content_w) / 2.0,
                        (h - content_h) / 2.0,
                    )
                }
            };
        PageLayout {
            lo,
            hi,
            gap,
            margin,
            page,
            scale,
            width,
            height,
            off_x,
            off_y,
        }
    }

    /// The SVG file with a title block under the drawing when the settings of the title block
    /// are given with the title to use when they have none and the language of the cells. The
    /// page grows to hold the block.
    pub fn to_svg_with(
        &self,
        settings: &DrawingSettings,
        title: Option<(&TitleBlockSettings, &str, Lang)>,
    ) -> String {
        let PageLayout {
            lo,
            hi,
            gap,
            margin,
            page,
            scale,
            width,
            height,
            off_x,
            off_y,
        } = self.page_layout(settings, title);
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
        let px = |x: f64| x - lo.0 + margin + off_x;
        let py = |y: f64| hi.1 - y + margin + off_y;
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
            let block = self.title_block(
                tb,
                fallback,
                &scale_label(scale, settings.paper != 0),
                gap,
                lang,
            );
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
            "<text x=\"{}\" y=\"{}\" font-family=\"sans-serif\" font-size=\"{}\" fill=\"#555555\">{}</text>",
            num(2.0),
            num(height - 2.0),
            num(text_size),
            scale_label(scale, settings.paper != 0)
        );
        o.push_str("</svg>\n");
        o
    }

    /// The drawing as a PDF file of one page, laid out exactly as [`Drawing::to_svg_with`] lays out its page (the same paper, scale, margin and title block),
    /// with the lines, circles and arcs of every layer in its colour and dash, the dimensions and the title block in Helvetica. `document_title` is the
    /// title kept in the file. Nothing is compressed and nothing depends on the date, so the same drawing gives the same bytes.
    pub fn to_pdf_with(
        &self,
        settings: &DrawingSettings,
        title: Option<(&TitleBlockSettings, &str, Lang)>,
        document_title: &str,
    ) -> Vec<u8> {
        let PageLayout {
            lo,
            hi,
            gap,
            margin,
            scale,
            width,
            height,
            off_x,
            off_y,
            ..
        } = self.page_layout(settings, title);
        let line = 0.3 / scale;
        let text_size = 3.2 / scale;
        let px = |x: f64| x - lo.0 + margin + off_x;
        let py = |y: f64| hi.1 - y + margin + off_y;
        let mut c = Canvas::new(width, height, scale * POINTS_PER_MM);
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
            c.stroke(layer.svg_colour());
            c.width(line);
            c.dash(match layer {
                Layer::Axes => Some((6.0 / scale, 1.5 / scale)),
                Layer::Hidden => Some((3.0 / scale, 1.5 / scale)),
                _ => None,
            });
            for (l, e) in &self.entities {
                if *l != layer {
                    continue;
                }
                match *e {
                    Entity::Line(a, b) => c.line((px(a.0), py(a.1)), (px(b.0), py(b.1))),
                    Entity::Circle(m, r) => c.circle((px(m.0), py(m.1)), r),
                    Entity::Arc(m, r, a0, a1) => c.arc((px(m.0), py(m.1)), r, a0, a1),
                }
            }
        }
        c.dash(None);
        c.stroke("#111111");
        c.fill("#111111");
        c.width(line);
        if settings.dimensions {
            for note in &self.notes {
                match note.measures {
                    Some((a, b)) => {
                        let horizontal = a.1 == b.1;
                        let off = DIM_OFFSET + DIM_STEP * f64::from(note.level);
                        let (p0, p1, label_at) = if horizontal {
                            let y = py(a.1) + off;
                            ((px(a.0), y), (px(b.0), y), (px((a.0 + b.0) / 2.0), y - 1.2))
                        } else {
                            let x = px(a.0) - off;
                            ((x, py(a.1)), (x, py(b.1)), (x - 1.2, py((a.1 + b.1) / 2.0)))
                        };
                        c.line(p0, p1);
                        for p in [p0, p1] {
                            let foot = if horizontal {
                                (p.0, p.1 - off)
                            } else {
                                (p.0 + off, p.1)
                            };
                            c.line(foot, p);
                        }
                        let turn = if horizontal { 0.0 } else { 90.0 };
                        c.text(label_at, text_size, &note.text, Anchor::Middle, turn);
                    }
                    None => c.text(
                        (px(note.at.0), py(note.at.1)),
                        text_size,
                        &note.text,
                        Anchor::Start,
                        0.0,
                    ),
                }
            }
        }
        if let Some((tb, fallback, lang)) = title {
            let block = self.title_block(
                tb,
                fallback,
                &scale_label(scale, settings.paper != 0),
                gap,
                lang,
            );
            for item in &block.items {
                match item {
                    Item::Line(a, b) => c.line((px(a.0), py(a.1)), (px(b.0), py(b.1))),
                    Item::Text { at, height, text } => {
                        c.text((px(at.0), py(at.1)), *height, text, Anchor::Start, 0.0)
                    }
                }
            }
        }
        c.fill("#555555");
        c.text(
            (2.0, height - 2.0),
            text_size,
            &scale_label(scale, settings.paper != 0),
            Anchor::Start,
            0.0,
        );
        c.finish(document_title)
    }
}

/// The layout of a page, see [`Drawing::page_layout`].
struct PageLayout {
    lo: (f64, f64),
    hi: (f64, f64),
    gap: f64,
    margin: f64,
    page: f64,
    scale: f64,
    width: f64,
    height: f64,
    off_x: f64,
    off_y: f64,
}

/// The lines of a legend for the holes of the face that `view` looks at: one line each with its
/// tag (the one of the hole schedule), its diameter (or width) and its position, up to
/// `drawing.max_dimensions`, and a last line that says how many more there are.
fn hole_lines(
    enclosure: &Enclosure,
    view: View,
    settings: &DrawingSettings,
    lang: Lang,
) -> Vec<String> {
    let near = view.face().name();
    let rows: Vec<_> = schedule::rows(enclosure)
        .into_iter()
        .filter(|r| r.face == near)
        .collect();
    let shown = settings.max_dimensions as usize;
    let mut lines = Vec::new();
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
    lines
}

/// The standard scales of a drawing (millimetres of paper for each millimetre of the model), from
/// 50:1 down to 1:100.
const STANDARD_SCALES: [f64; 12] = [
    50.0, 20.0, 10.0, 5.0, 2.0, 1.0, 0.5, 0.2, 0.1, 0.05, 0.02, 0.01,
];

/// The size in millimetres (across, then up) of the sheet of the A series given by `paper` (1 to
/// 5 for A1 to A5), standing upright when `portrait` is set; `None` for 0 or a number past 5.
pub fn paper_size(paper: u8, portrait: bool) -> Option<(f64, f64)> {
    let (long, short) = match paper {
        1 => (841.0, 594.0),
        2 => (594.0, 420.0),
        3 => (420.0, 297.0),
        4 => (297.0, 210.0),
        5 => (210.0, 148.0),
        _ => return None,
    };
    Some(if portrait {
        (short, long)
    } else {
        (long, short)
    })
}

/// The largest standard scale that is not larger than `fit` (the scale that would just fill the
/// paper); below 1:100 the exact `fit` is kept so that nothing is cut off.
pub fn standard_scale(fit: f64) -> f64 {
    if !(fit.is_finite() && fit > 0.0) {
        return 1.0;
    }
    STANDARD_SCALES
        .iter()
        .copied()
        .find(|s| *s <= fit * (1.0 + 1e-9))
        .unwrap_or(fit)
}

/// The scale as written on a drawing: `2:1`, `1:5`; a drawing that is not on paper keeps two
/// decimals (`1:1.52`), because its scale is whatever fits the width.
pub fn scale_label(scale: f64, standard: bool) -> String {
    if !standard {
        return format!("1:{:.2}", 1.0 / scale);
    }
    if scale >= 1.0 && (scale - scale.round()).abs() < 1e-9 {
        return format!("{}:1", scale.round());
    }
    let inverse = 1.0 / scale;
    if (inverse - inverse.round()).abs() < 1e-6 {
        return format!("1:{}", inverse.round());
    }
    format!("1:{inverse:.2}")
}

/// The sheet of paper of a drawing that is printed at a standard scale (see
/// [`Drawing::paper_sheet`]), in the millimetres of the model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaperSheet {
    /// Millimetres of paper for each millimetre of the model.
    pub scale: f64,
    /// The lower left corner of the edge of the paper.
    pub lo: (f64, f64),
    /// The upper right corner of the edge of the paper.
    pub hi: (f64, f64),
    /// How far inside the edge the line of the margin runs, in millimetres of the model.
    pub inset: f64,
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

    fn circles_of(d: &Drawing) -> Vec<((f64, f64), f64)> {
        d.entities
            .iter()
            .filter_map(|(_, e)| match e {
                Entity::Circle(c, r) => Some((*c, *r)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_projection_symbol_is_under_the_sheet_with_the_circles_on_the_side_of_the_angle() {
        let e = model();
        let plain = Drawing::sheet(&e, &settings()).unwrap();
        let (lo, _) = plain.bounds();
        // first angle: the cone on the left, the two circles to the right of it
        let mut first = plain.clone();
        first.add_projection_symbol(false, 10.0);
        assert_eq!(first.entities.len(), plain.entities.len() + 7);
        let circles = circles_of(&first);
        let ring: Vec<_> = circles
            .iter()
            .filter(|(_, r)| *r == 5.0 || *r == 2.5)
            .collect();
        assert_eq!(ring.len(), 2);
        assert_eq!(ring[0].0, ring[1].0, "the circles are concentric");
        // the cone runs from the left edge of the sheet to 15 mm, the circles are at 26 mm
        assert!((ring[0].0 .0 - (lo.0 + 26.0)).abs() < 1e-9, "{:?}", ring[0]);
        // third angle: the circles to the left of the cone
        let mut third = plain.clone();
        third.add_projection_symbol(true, 10.0);
        let ring3: Vec<_> = circles_of(&third)
            .into_iter()
            .filter(|(_, r)| *r == 5.0 || *r == 2.5)
            .collect();
        assert!(
            (ring3[0].0 .0 - (lo.0 + 5.0)).abs() < 1e-9,
            "{:?}",
            ring3[0]
        );
        // both are under everything the sheet had, clear of its dimension lines
        let (new_lo, new_hi) = first.bounds();
        let (_, old_hi) = plain.bounds();
        assert!(
            new_lo.1 <= lo.1 - plain.dimension_room() - 5.0 - 10.0 + 1e-9,
            "{new_lo:?}"
        );
        assert_eq!(new_lo.0, lo.0);
        assert!(new_hi.1 >= old_hi.1);
        // the symbol is made of lines of the outline layer and one centre line
        let added = &first.entities[plain.entities.len()..];
        assert_eq!(added.iter().filter(|(l, _)| *l == Layer::Axes).count(), 1);
        assert_eq!(
            added.iter().filter(|(l, _)| *l == Layer::Outline).count(),
            6
        );
    }

    #[test]
    fn the_projection_symbol_scales_with_its_size_and_does_nothing_on_nothing() {
        let e = model();
        let plain = Drawing::sheet(&e, &settings()).unwrap();
        let measure = |size: f64| {
            let mut d = plain.clone();
            d.add_projection_symbol(false, size);
            circles_of(&d).iter().map(|(_, r)| *r).collect::<Vec<f64>>()
        };
        assert_eq!(measure(20.0), vec![10.0, 5.0]);
        assert_eq!(measure(4.0), vec![2.0, 1.0]);
        // a size that is not a size, and an empty drawing, add nothing
        for size in [0.0, -3.0, f64::NAN, f64::INFINITY] {
            let mut d = plain.clone();
            d.add_projection_symbol(true, size);
            assert_eq!(d, plain, "{size}");
        }
        let mut empty = Drawing::of(&e, View::Top, &settings()).unwrap();
        empty.entities.clear();
        let before = empty.clone();
        empty.add_projection_symbol(false, 10.0);
        assert_eq!(empty, before);
    }

    #[test]
    fn a_cutting_line_crosses_the_view_at_the_plane_with_a_mark_and_the_letters_at_both_ends() {
        let e = model();
        // the plan shows the planes across x (a vertical line) and across y (a horizontal one)
        let mut d = Drawing::of(&e, View::Top, &bare()).unwrap();
        let before = (d.entities.len(), d.notes.len());
        assert!(d.add_cut_line(Dim::X, mmv(40), "A"));
        let added: Vec<_> = d.entities[before.0..].to_vec();
        assert!(!added.is_empty());
        // the line is on x = 40 and goes beyond the box at both ends
        let lines: Vec<_> = added
            .iter()
            .filter(|(l, _)| *l == Layer::Axes)
            .map(|(_, e)| *e)
            .collect();
        assert_eq!(
            lines,
            [Entity::Line(
                (40.0, -CUT_OVERSHOOT),
                (40.0, 64.0 + CUT_OVERSHOOT)
            )]
        );
        // a heavy mark and an arrow of three lines at each end, on the outline layer
        assert_eq!(added.len(), 1 + 2 * 4);
        // the letters are beside both ends, on the outer side of each, as notes with no dimension
        let letters: Vec<_> = d.notes[before.1..].iter().collect();
        assert_eq!(letters.len(), 2);
        assert!(letters
            .iter()
            .all(|n| n.text == "A" && n.measures.is_none()));
        assert!(letters[0].at.1 < 0.0 && letters[1].at.1 > 64.0);
        assert!(letters.iter().all(|n| n.at.0 == 40.0));
        // a plane across y is a horizontal line of the plan
        let mut h = Drawing::of(&e, View::Top, &bare()).unwrap();
        assert!(h.add_cut_line(Dim::Y, mmv(10), "B2"));
        assert!(h.entities.iter().any(|(l, e)| *l == Layer::Axes
            && matches!(*e, Entity::Line(a, b) if a.1 == 10.0 && b.1 == 10.0 && a.0 < 0.0 && b.0 > 104.0)));
    }

    #[test]
    fn a_cutting_line_the_view_cannot_show_or_outside_the_box_is_left_out() {
        let e = model();
        let none = |view: View, axis: Dim, at: i64| {
            let mut d = Drawing::of(&e, view, &bare()).unwrap();
            let before = d.clone();
            let drawn = d.add_cut_line(axis, mmv(at), "A");
            (drawn, d == before)
        };
        // the plan does not show the plane across z, the front not the one across y, the side not
        // the one across x: they are seen face-on
        assert_eq!(none(View::Top, Dim::Z, 10), (false, true));
        assert_eq!(none(View::Front, Dim::Y, 10), (false, true));
        assert_eq!(none(View::Side, Dim::X, 10), (false, true));
        // outside the box, on either side
        assert_eq!(none(View::Top, Dim::X, 105), (false, true));
        assert_eq!(none(View::Front, Dim::Z, 36), (false, true));
        // the edges of the box are inside it
        assert!(none(View::Top, Dim::X, 0).0);
        assert!(none(View::Top, Dim::X, 104).0);
        // a drawing with nothing in it gets nothing
        let mut empty = Drawing {
            view: View::Top,
            entities: Vec::new(),
            notes: Vec::new(),
            extent: (10.0, 10.0),
            dxf_dimensions: false,
        };
        assert!(!empty.add_cut_line(Dim::X, mmv(5), "A"));
    }

    #[test]
    fn the_cutting_line_is_in_the_dxf_and_svg_files_only_when_the_settings_ask_for_it() {
        let e = model();
        let plain = crate::config::Settings::defaults();
        assert_eq!(plain.drawing.cut_line, 0);
        assert_eq!(plain.drawing.cut_letters(), "A");
        let with = crate::config::Settings::with_user_config(
            "[drawing]\ncut_line = 1\ncut_at = 40 mm\ncut_label = S1\ndimensions = 0\ndxf_dimensions = 0\n",
        )
        .unwrap();
        assert_eq!(with.drawing.cut_letters(), "S1");
        // the same settings without the line
        let base = crate::config::Settings::with_user_config(
            "[drawing]\ndimensions = 0\ndxf_dimensions = 0\n",
        )
        .unwrap();
        let same = |a: Vec<u8>, b: Vec<u8>| a == b;
        for format in [crate::export::Format::DxfTop, crate::export::Format::SvgTop] {
            let off = crate::export::render(format, &e, "", &base).unwrap();
            let on = crate::export::render(format, &e, "", &with).unwrap();
            assert!(!same(off, on.clone()), "{format:?}");
            // the same bytes every time
            assert!(same(
                on,
                crate::export::render(format, &e, "", &with).unwrap()
            ));
        }
        // the letters are texts of the drawing, so they come with the dimensions
        let lettered = crate::config::Settings::with_user_config(
            "[drawing]\ncut_line = 1\ncut_at = 40 mm\ncut_label = S1\n",
        )
        .unwrap();
        let svg = String::from_utf8(
            crate::export::render(crate::export::Format::SvgTop, &e, "", &lettered).unwrap(),
        )
        .unwrap();
        assert_eq!(svg.matches(">S1</text>").count(), 2);
        let dxf = String::from_utf8(
            crate::export::render(crate::export::Format::DxfTop, &e, "", &lettered).unwrap(),
        )
        .unwrap();
        assert_eq!(dxf.matches("\nS1\n").count(), 2);
        // the sheet of views carries none
        let sheet_off = crate::export::render(crate::export::Format::SvgSheet, &e, "", &base);
        let sheet_on = crate::export::render(crate::export::Format::SvgSheet, &e, "", &with);
        assert!(same(sheet_off.unwrap(), sheet_on.unwrap()));
        // a cutting line across the axis a view cannot show changes nothing in that view
        let z = crate::config::Settings::with_user_config(
            "[drawing]\ncut_line = 3\ncut_at = 10 mm\ndimensions = 0\ndxf_dimensions = 0\n",
        )
        .unwrap();
        assert!(same(
            crate::export::render(crate::export::Format::SvgTop, &e, "", &z).unwrap(),
            crate::export::render(crate::export::Format::SvgTop, &e, "", &base).unwrap()
        ));
        // the settings refuse what cannot be drawn
        for bad in [
            "[drawing]\ncut_line = 4\n",
            "[drawing]\ncut_label = \n",
            "[drawing]\ncut_label = ABCD\n",
            "[drawing]\ncut_label = A-\n",
        ] {
            assert!(
                crate::config::Settings::with_user_config(bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_sheet_has_the_projection_symbol_in_its_files_only_when_the_settings_say_so() {
        let e = model();
        let off = crate::config::Settings::defaults();
        assert!(!off.drawing.sheet_symbol);
        assert_eq!(off.drawing.sheet_symbol_size.get(), 10_000_000);
        let on = crate::config::Settings::with_user_config(
            "[drawing]\nsheet_symbol = 1\nsheet_symbol_size = 12 mm\nsheet_angle = 3\n",
        )
        .unwrap();
        assert!(on.drawing.sheet_symbol);
        assert_eq!(on.drawing.sheet_symbol_size.get(), 12_000_000);
        let render = |s: &crate::config::Settings, f| {
            String::from_utf8(crate::export::render(f, &e, "box", s).unwrap()).unwrap()
        };
        use crate::export::Format;
        let (plain, symbol) = (
            render(&off, Format::SvgSheet),
            render(&on, Format::SvgSheet),
        );
        // the circles of the symbol are circles of 6 and 3 mm
        assert!(!plain.contains("r=\"6\""));
        assert!(
            symbol.contains("r=\"6\"") && symbol.contains("r=\"3\""),
            "{symbol}"
        );
        // a drawing of one view has none, with the setting on
        assert_eq!(
            render(&on, Format::SvgTop).matches("<circle").count(),
            render(&off, Format::SvgTop).matches("<circle").count()
        );
        // and the DXF file of the sheet has the new entities
        assert!(
            render(&on, Format::DxfSheet).matches("0\nCIRCLE\n").count()
                == render(&off, Format::DxfSheet)
                    .matches("0\nCIRCLE\n")
                    .count()
                    + 2
        );
        for bad in [
            "[drawing]\nsheet_symbol_size = 3 mm\n",
            "[drawing]\nsheet_symbol_size = 51 mm\n",
            "[drawing]\nsheet_symbol = 2\n",
        ] {
            assert!(
                crate::config::Settings::with_user_config(bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_legend_of_the_sheet_lists_the_holes_of_each_view_under_its_name() {
        let mut e = model();
        hole(&mut e, 1, Face::Front, 60, 17, 6, HoleStyle::Plain);
        hole(&mut e, 2, Face::Top, 10, 12, 3, HoleStyle::Plain);
        hole(&mut e, 3, Face::Right, 20, 9, 4, HoleStyle::Plain);
        hole(&mut e, 4, Face::Right, 30, 9, 4, HoleStyle::Plain);
        let mut s = Drawing::sheet(&e, &settings()).unwrap();
        s.add_sheet_legend(&e, &settings(), Lang::En);
        let lines = legend(&s);
        // the title, the box and the thickness once, then a name and the holes of each view
        assert_eq!(lines[0], "Dimensions (mm)");
        assert_eq!(lines[1], "box: 104 x 64 x 35");
        assert_eq!(lines[2], "wall 2, floor 2, lid 3");
        let at = |name: &str| lines.iter().position(|l| l == name).unwrap();
        let (front, plan, side) = (at("Front view:"), at("Plan view:"), at("Side view:"));
        assert!(front < plan && plan < side);
        assert_eq!(plan - front, 2, "{lines:?}");
        assert_eq!(side - plan, 2, "{lines:?}");
        assert_eq!(lines.len() - side, 3, "{lines:?}");
        assert!(lines[front + 1].starts_with("H") && lines[front + 1].contains("6 at 60, 17"));
        assert!(lines[plan + 1].contains("3 at 10, 12"));
        assert!(lines[side + 1].contains("4 at 20, 9") && lines[side + 2].contains("4 at 30, 9"));
        // the lines are one under the other, to the right of the whole sheet
        let notes: Vec<_> = s.notes.iter().filter(|n| n.level == LEGEND_LEVEL).collect();
        assert!(notes
            .windows(2)
            .all(|w| w[0].at.1 > w[1].at.1 && w[0].at.0 == w[1].at.0));
        let (_, hi) = s.bounds();
        assert!(notes[0].at.0 >= hi.0);
    }

    #[test]
    fn the_legend_of_the_sheet_names_a_view_with_no_hole_and_cuts_each_view_at_the_limit() {
        let mut e = model();
        hole(&mut e, 1, Face::Top, 10, 12, 3, HoleStyle::Plain);
        hole(&mut e, 2, Face::Top, 20, 12, 3, HoleStyle::Plain);
        let few = DrawingSettings {
            max_dimensions: 1,
            ..settings()
        };
        let mut s = Drawing::sheet(&e, &settings()).unwrap();
        s.add_sheet_legend(&e, &few, Lang::En);
        let lines = legend(&s);
        // the front and the side have no hole: the name and nothing under it
        assert_eq!(
            lines[3..],
            [
                "Front view:",
                "Plan view:",
                "H1: \u{2300}3 at 10, 12",
                "1 more holes are not listed",
                "Side view:"
            ]
            .map(str::to_owned)
        );
        // in another language the names of the views change and the count of lines does not
        let mut es = Drawing::sheet(&e, &settings()).unwrap();
        es.add_sheet_legend(&e, &few, Lang::Es);
        let spanish = legend(&es);
        assert_eq!(spanish.len(), lines.len());
        assert!(spanish.contains(&"Vista frontal:".to_owned()));
        assert!(spanish.contains(&"Vista lateral:".to_owned()));
        assert!(spanish.contains(&"Vista de planta:".to_owned()));
    }

    #[test]
    fn the_sheet_exports_the_legend_of_every_view_unless_the_settings_say_otherwise() {
        let mut e = model();
        hole(&mut e, 1, Face::Top, 10, 12, 3, HoleStyle::Plain);
        assert!(settings().sheet_legends);
        let own =
            crate::config::Settings::with_user_config("[drawing]\nsheet_legends = 0\n").unwrap();
        assert!(!own.drawing.sheet_legends);
        assert!(
            crate::config::Settings::with_user_config("[drawing]\nsheet_legends = 2\n").is_err()
        );
        let all = crate::export::render(
            crate::export::Format::SvgSheet,
            &e,
            "box",
            &crate::config::Settings::defaults(),
        )
        .unwrap();
        let all = String::from_utf8(all).unwrap();
        assert!(all.contains(">Plan view:</text>") && all.contains(">Side view:</text>"));
        let front_only =
            crate::export::render(crate::export::Format::SvgSheet, &e, "box", &own).unwrap();
        let front_only = String::from_utf8(front_only).unwrap();
        assert!(!front_only.contains("Plan view:") && !front_only.contains("Front view:"));
        assert!(front_only.contains(">Dimensions (mm)</text>"));
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
        assert!(dxf.contains(
            "2
MARCO
70
0
62
7
6
CONTINUOUS
"
        ));
        assert_eq!(
            dxf.matches(
                "0
LINE
8
MARCO
"
            )
            .count(),
            4
        );
        let layers = |t: &str| {
            t.matches(
                "0
LAYER
2
",
            )
            .count()
        };
        assert_eq!(layers(&dxf), layers(&plain.to_dxf()) + 1);
        let svg = d.to_svg(&settings());
        assert!(svg.contains("<g id=\"MARCO\""));
        assert_eq!(
            svg.matches("<line").count(),
            plain.to_svg(&settings()).matches("<line").count() + 4
        );
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
            assert!(
                crate::config::Settings::with_user_config(bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_dxf_sheet_has_the_size_of_the_paper_times_the_standard_scale_round_the_drawing() {
        let e = model();
        let d = Drawing::of(&e, View::Top, &settings()).unwrap();
        let (lo, hi) = d.bounds();
        let sheet = d.paper_sheet(4, false, 10.0, None).unwrap();
        // the sheet is the paper of an A4 (297 x 210) at a standard scale
        let (w, h) = (sheet.hi.0 - sheet.lo.0, sheet.hi.1 - sheet.lo.1);
        assert!((w * sheet.scale - 297.0).abs() < 1e-9 && (h * sheet.scale - 210.0).abs() < 1e-9);
        assert!(STANDARD_SCALES.contains(&sheet.scale));
        // everything drawn lies inside the margin of the paper, and the next scale up would not fit
        assert!(sheet.lo.0 + sheet.inset <= lo.0 && sheet.hi.0 - sheet.inset >= hi.0);
        assert!(sheet.lo.1 + sheet.inset <= lo.1 && sheet.hi.1 - sheet.inset >= hi.1);
        assert!((sheet.inset * sheet.scale - 10.0).abs() < 1e-9);
        let larger = STANDARD_SCALES.iter().rev().find(|s| **s > sheet.scale);
        if let Some(next) = larger {
            let (cw, ch) = (hi.0 - lo.0, hi.1 - lo.1);
            assert!(
                cw * next > 277.0 + 1e-6 || ch * next > 190.0 + 1e-6,
                "{next}"
            );
        }
        // upright paper swaps the sides, and no paper or nothing drawn is no sheet
        let up = d.paper_sheet(4, true, 10.0, None).unwrap();
        assert!(((up.hi.0 - up.lo.0) * up.scale - 210.0).abs() < 1e-9);
        assert!(d.paper_sheet(0, false, 10.0, None).is_none());
        assert!(d.paper_sheet(6, false, 10.0, None).is_none());
        let mut empty = d.clone();
        empty.entities.clear();
        assert!(empty.paper_sheet(4, false, 10.0, None).is_none());
        // a larger sheet of the series is at a scale at least as large
        let a2 = d.paper_sheet(2, false, 10.0, None).unwrap();
        assert!(a2.scale >= sheet.scale);
        // what is still to be added makes the scale smaller when it does not fit
        let far = ((hi.0, lo.1 - 5000.0), (hi.0 + 10.0, lo.1));
        let with_block = d.paper_sheet(4, false, 10.0, Some(far)).unwrap();
        assert!(with_block.scale < sheet.scale);
    }

    #[test]
    fn the_sheet_is_drawn_on_the_layer_of_the_frame_with_its_margin_and_nothing_else_changes() {
        let e = model();
        let mut d = Drawing::of(&e, View::Top, &settings()).unwrap();
        let before = d.entities.len();
        let sheet = d.paper_sheet(3, false, 10.0, None).unwrap();
        assert_eq!(
            d.entities.len(),
            before,
            "asking for the sheet draws nothing"
        );
        d.add_paper_sheet(&sheet);
        assert_eq!(d.count(Layer::Frame), 8);
        assert_eq!(d.entities.len(), before + 8);
        let dxf = d.to_dxf();
        assert!(dxf.contains("\nMARCO\n"));
        // no margin, no inner line
        let mut bare = Drawing::of(&e, View::Top, &settings()).unwrap();
        let none = bare.paper_sheet(3, false, 0.0, None).unwrap();
        bare.add_paper_sheet(&none);
        assert_eq!(bare.count(Layer::Frame), 4);
        // a margin as large as the paper leaves no inner line either
        let mut huge = Drawing::of(&e, View::Top, &settings()).unwrap();
        let sheet = huge.paper_sheet(5, false, 100.0, None).unwrap();
        huge.add_paper_sheet(&sheet);
        assert_eq!(huge.count(Layer::Frame), 4);
    }

    #[test]
    fn the_sheet_counts_the_dimensions_and_the_legend_only_when_the_dxf_has_them() {
        let mut e = model();
        hole(&mut e, 1, Face::Front, 60, 17, 6, HoleStyle::Plain);
        let mut with = Drawing::of(&e, View::Front, &settings()).unwrap();
        with.add_legend(&e, &settings(), Lang::En);
        let mut without = with.clone();
        without.dxf_dimensions = false;
        let a = with.paper_sheet(4, false, 10.0, None).unwrap();
        let b = without.paper_sheet(4, false, 10.0, None).unwrap();
        // the same paper, and a sheet that holds the dimension lines and the legend as well
        assert!(a.scale <= b.scale);
        let (lo, hi) = with.bounds();
        assert!(a.lo.0 < lo.0 - with.dimension_room() + 1e-6);
        assert!(a.hi.0 > hi.0.max(with.legend_right()) - 1e-6);
    }

    fn on_paper(paper: u8) -> DrawingSettings {
        DrawingSettings {
            paper,
            ..settings()
        }
    }

    fn svg_size(svg: &str) -> (f64, f64) {
        let read = |key: &str| -> f64 {
            let at = svg.find(key).unwrap() + key.len();
            svg[at..].split("mm").next().unwrap().parse().unwrap()
        };
        (read("width=\""), read("height=\""))
    }

    #[test]
    fn the_paper_sizes_are_the_a_series_across_or_upright() {
        assert_eq!(paper_size(0, false), None);
        assert_eq!(paper_size(6, false), None);
        assert_eq!(paper_size(4, false), Some((297.0, 210.0)));
        assert_eq!(paper_size(4, true), Some((210.0, 297.0)));
        assert_eq!(paper_size(3, false), Some((420.0, 297.0)));
        // each size is half of the one before it along its long side
        for n in 1..5u8 {
            let (a, b) = paper_size(n, false).unwrap();
            let (c, d) = paper_size(n + 1, false).unwrap();
            assert!(
                (a / c - 1.414).abs() < 0.02 && (b / d - 1.414).abs() < 0.02,
                "A{n}"
            );
        }
    }

    #[test]
    fn a_standard_scale_is_the_largest_that_fits() {
        assert_eq!(standard_scale(1.0), 1.0);
        assert_eq!(standard_scale(1.99), 1.0);
        assert_eq!(standard_scale(2.0), 2.0);
        assert_eq!(standard_scale(0.49), 0.2);
        assert_eq!(standard_scale(0.5), 0.5);
        assert_eq!(standard_scale(1000.0), 50.0);
        // below 1:100 the exact fit is kept so that nothing is cut off
        assert_eq!(standard_scale(0.004), 0.004);
        // a value that is not a scale gives 1:1
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(standard_scale(bad), 1.0);
        }
        // the list is in decreasing order
        for pair in STANDARD_SCALES.windows(2) {
            assert!(pair[0] > pair[1]);
        }
    }

    #[test]
    fn the_scale_is_written_as_a_ratio() {
        assert_eq!(scale_label(1.0, true), "1:1");
        assert_eq!(scale_label(2.0, true), "2:1");
        assert_eq!(scale_label(0.5, true), "1:2");
        assert_eq!(scale_label(0.01, true), "1:100");
        assert_eq!(scale_label(0.004, true), "1:250");
        assert_eq!(scale_label(1.0 / 1.52, true), "1:1.52");
        assert_eq!(scale_label(1.0 / 1.52, false), "1:1.52");
        assert_eq!(scale_label(1.0, false), "1:1.00");
    }

    #[test]
    fn the_svg_drawing_on_paper_has_the_size_of_the_sheet_and_a_standard_scale() {
        let e = model();
        let d = Drawing::of(&e, View::Front, &settings()).unwrap();
        for (paper, portrait, size) in [
            (4u8, false, (297.0, 210.0)),
            (4, true, (210.0, 297.0)),
            (3, false, (420.0, 297.0)),
            (5, false, (210.0, 148.0)),
        ] {
            let s = DrawingSettings {
                paper,
                paper_portrait: portrait,
                ..settings()
            };
            let svg = d.to_svg(&s);
            let (w, h) = svg_size(&svg);
            assert!(
                (w - size.0).abs() < 1e-3 && (h - size.1).abs() < 1e-3,
                "{paper} {svg}"
            );
            // the label at the foot is a standard scale
            let label = svg.rsplit("fill=\"#555555\">").next().unwrap();
            let label = label.split("</text>").next().unwrap();
            assert!(label.ends_with(":1") || label.starts_with("1:"), "{label}");
            assert!(!label.contains('.'), "{label}");
        }
        // without paper the width is the one of the settings
        let (w, _) = svg_size(&d.to_svg(&settings()));
        assert!((w - 200.0).abs() < 1e-3);
    }

    #[test]
    fn the_drawing_on_paper_is_centred_and_fits_inside_the_margin() {
        let e = model();
        let d = Drawing::of(&e, View::Top, &settings()).unwrap();
        let s = on_paper(4);
        let svg = d.to_svg(&s);
        let view_box: Vec<f64> = svg
            .split("viewBox=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .split(' ')
            .map(|v| v.parse().unwrap())
            .collect();
        let scale = 297.0 / view_box[2];
        // a box of 100 by 60 inside fits an A4 sheet at 2:1 or less, and the scale is standard
        assert!(
            [2.0, 1.0, 0.5].contains(&((scale * 1e6).round() / 1e6)),
            "{scale}"
        );
        // every point of the outline lies inside the sheet, with the margin of the settings
        let margin = 10.0 / scale;
        for line in svg.lines().filter(|l| l.starts_with("<line")) {
            for key in ["x1", "x2"] {
                let at = line.find(&format!("{key}=\"")).unwrap() + 4;
                let v: f64 = line[at..].split('"').next().unwrap().parse().unwrap();
                assert!(
                    v >= margin - 1e-3 && v <= view_box[2] - margin + 1e-3,
                    "{line}"
                );
            }
        }
        // a wider margin makes the scale smaller or equal, never larger
        let wide = DrawingSettings {
            paper_margin: Length::new(60_000_000).unwrap(),
            ..on_paper(4)
        };
        let svg_wide = d.to_svg(&wide);
        let wide_box = svg_wide.split("viewBox=\"").nth(1).unwrap();
        let wide_w: f64 = wide_box.split(' ').nth(2).unwrap().parse().unwrap();
        assert!(297.0 / wide_w <= scale + 1e-9);
    }

    #[test]
    fn the_title_block_carries_the_standard_scale_of_the_paper() {
        let e = model();
        let d = Drawing::of(&e, View::Front, &settings()).unwrap();
        let tb =
            crate::config::Settings::with_user_config("[title_block]\nenabled = 1\ntitle = Lid\n")
                .unwrap()
                .title_block;
        let svg = d.to_svg_with(&on_paper(3), Some((&tb, "x", Lang::En)));
        let svg_scale = svg.rsplit("fill=\"#555555\">").next().unwrap();
        let label = svg_scale.split("</text>").next().unwrap();
        assert!(!label.contains('.'), "{label}");
        assert!(svg.contains(&format!(">{label}</text>")));
        let (w, h) = svg_size(&svg);
        assert!((w - 420.0).abs() < 1e-3 && (h - 297.0).abs() < 1e-3);
    }

    #[test]
    fn the_paper_settings_have_their_defaults_and_their_limits() {
        assert_eq!(settings().paper, 0);
        assert_eq!(settings().paper_margin.get(), 10_000_000);
        assert!(!settings().paper_portrait);
        let own = crate::config::Settings::with_user_config(
            "[drawing]\npaper = 3\npaper_margin = 0 mm\npaper_portrait = 1\n",
        )
        .unwrap()
        .drawing;
        assert_eq!(own.paper, 3);
        assert_eq!(own.paper_margin, Length::ZERO);
        assert!(own.paper_portrait);
        for bad in [
            "[drawing]\npaper = 6\n",
            "[drawing]\npaper_margin = -1 mm\n",
            "[drawing]\npaper_margin = 101 mm\n",
            "[drawing]\npaper_portrait = 2\n",
        ] {
            assert!(
                crate::config::Settings::with_user_config(bad).is_err(),
                "{bad}"
            );
        }
    }

    fn third_angle() -> DrawingSettings {
        DrawingSettings {
            sheet_angle: 3,
            ..settings()
        }
    }

    #[test]
    fn the_sheet_puts_the_plan_under_the_front_and_the_side_to_its_left_in_first_angle() {
        let e = model();
        let s = Drawing::sheet(&e, &settings()).unwrap();
        // 13 mm of dimension line under a view and 20 mm of air: the plan is 64 mm high and the
        // side 64 mm wide
        let room = Drawing::of(&e, View::Front, &settings())
            .unwrap()
            .dimension_room();
        assert_eq!(room, 13.0);
        let at = |x: f64, y: f64| (x, y);
        let line = |a: (f64, f64), b: (f64, f64)| {
            s.entities.contains(&(Layer::Outline, Entity::Line(a, b)))
                || s.entities.contains(&(Layer::Outline, Entity::Line(b, a)))
        };
        assert!(line(at(0.0, 0.0), at(104.0, 0.0)));
        assert!(line(at(0.0, -97.0), at(104.0, -97.0)));
        assert!(line(at(0.0, -33.0), at(104.0, -33.0)));
        assert!(line(at(-97.0, 0.0), at(-33.0, 0.0)));
        assert!(line(at(-97.0, 35.0), at(-33.0, 35.0)));
        // each view keeps its outline, its cavity and its two centre lines
        assert_eq!(s.count(Layer::Outline), 12);
        assert_eq!(s.count(Layer::Hidden), 12);
        assert_eq!(s.count(Layer::Axes), 6);
        assert_eq!(s.notes.iter().filter(|n| n.measures.is_some()).count(), 6);
        assert_eq!(s.view, View::Front);
    }

    #[test]
    fn the_sheet_puts_the_plan_over_the_front_and_the_side_to_its_right_in_third_angle() {
        let e = model();
        let s = Drawing::sheet(&e, &third_angle()).unwrap();
        let line = |a: (f64, f64), b: (f64, f64)| {
            s.entities.contains(&(Layer::Outline, Entity::Line(a, b)))
                || s.entities.contains(&(Layer::Outline, Entity::Line(b, a)))
        };
        // 35 + 20 + 13 for the plan, 104 + 20 + 13 for the side
        assert!(line((0.0, 68.0), (104.0, 68.0)));
        assert!(line((0.0, 132.0), (104.0, 132.0)));
        assert!(line((137.0, 0.0), (201.0, 0.0)));
        assert!(line((137.0, 35.0), (201.0, 35.0)));
        // the dimensions moved with their views
        let widths: Vec<_> = s
            .notes
            .iter()
            .filter(|n| n.text == "64")
            .filter_map(|n| n.measures)
            .collect();
        assert!(widths.contains(&((137.0, 0.0), (201.0, 0.0))), "{widths:?}");
        assert!(widths.contains(&((0.0, 68.0), (0.0, 132.0))), "{widths:?}");
    }

    #[test]
    fn the_views_of_the_sheet_do_not_lie_on_one_another_nor_on_their_dimension_lines() {
        let mut e = model();
        hole(&mut e, 1, Face::Front, 60, 17, 6, HoleStyle::Plain);
        hole(&mut e, 2, Face::Front, 30, 17, 4, HoleStyle::Plain);
        hole(&mut e, 3, Face::Top, 10, 10, 3, HoleStyle::Plain);
        for settings in [settings(), third_angle()] {
            let views = Drawing::placed(&e, &settings, None).unwrap();
            assert_eq!(views.len(), 3);
            for (i, a) in views.iter().enumerate() {
                // what the view needs: its drawing and the dimension lines under and to the left
                let ((x0, y0), (x1, y1)) = a.bounds();
                let room = a.dimension_room();
                assert!(room >= 13.0);
                let (x0, y0) = (x0 - room, y0 - room);
                for (j, b) in views.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    let ((u0, v0), (u1, v1)) = b.bounds();
                    let apart = u1 < x0 || u0 > x1 || v1 < y0 || v0 > y1;
                    assert!(
                        apart,
                        "view {i} and view {j} touch with angle {}",
                        settings.sheet_angle
                    );
                }
            }
            let s = Drawing::sheet(&e, &settings).unwrap();
            assert_eq!(s.count(Layer::Holes), 3);
        }
    }

    #[test]
    fn the_legend_of_the_sheet_is_to_the_right_of_all_the_views() {
        let mut e = model();
        hole(&mut e, 1, Face::Front, 60, 17, 6, HoleStyle::Plain);
        for settings in [settings(), third_angle()] {
            let mut s = Drawing::sheet(&e, &settings).unwrap();
            let (_, hi) = s.bounds();
            assert_eq!(s.extent, hi);
            s.add_legend(&e, &settings, Lang::En);
            let legend: Vec<_> = s.notes.iter().filter(|n| n.level == LEGEND_LEVEL).collect();
            assert!(legend.len() >= 4);
            for n in &legend {
                assert_eq!(n.at.0, hi.0 + 10.0);
            }
            // the holes it lists are the ones of the front
            assert!(
                legend.iter().any(|n| n.text.starts_with("H1")),
                "{legend:?}"
            );
        }
    }

    #[test]
    fn the_sheet_gap_moves_the_views_apart_and_a_sheet_in_the_files_has_all_the_lines() {
        let e = model();
        let wide = DrawingSettings {
            sheet_gap: Length::new(50_000_000).unwrap(),
            ..settings()
        };
        let near = Drawing::sheet(&e, &settings()).unwrap();
        let far = Drawing::sheet(&e, &wide).unwrap();
        let (a, b) = (near.bounds(), far.bounds());
        assert_eq!(b.0 .0 - a.0 .0, -30.0);
        assert_eq!(b.0 .1 - a.0 .1, -30.0);
        let dxf = near.to_dxf();
        assert_eq!(dxf.matches("0\nLINE\n8\nCONTORNO\n").count(), 12);
        assert_eq!(dxf.matches("0\nLINE\n8\nOCULTAS\n").count(), 12);
        assert!(dxf.ends_with("0\nEOF\n"));
        let svg = near.to_svg(&settings());
        assert!(svg.trim_end().ends_with("</svg>"));
        assert_eq!(svg.matches("<g id=\"CONTORNO\"").count(), 1);
    }

    #[test]
    fn the_sheet_is_refused_when_the_three_views_together_have_too_many_entities() {
        let e = model();
        // each view has 10 entities (its outline, its cavity and its two centre lines)
        let few = DrawingSettings {
            max_entities: 20,
            ..settings()
        };
        assert_eq!(
            Drawing::of(&e, View::Front, &few).unwrap().entities.len(),
            10
        );
        let err = Drawing::sheet(&e, &few).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-050");
        assert!(Drawing::sheet(&e, &settings()).is_ok());
    }

    #[test]
    fn the_settings_of_the_sheet_have_their_defaults_and_their_limits() {
        assert_eq!(settings().sheet_angle, 1);
        assert_eq!(settings().sheet_gap.get(), 20_000_000);
        let own = crate::config::Settings::with_user_config(
            "[drawing]\nsheet_angle = 3\nsheet_gap = 0 mm\n",
        )
        .unwrap();
        assert_eq!(own.drawing.sheet_angle, 3);
        assert_eq!(own.drawing.sheet_gap, Length::ZERO);
        for bad in [
            "[drawing]\nsheet_angle = 2\n",
            "[drawing]\nsheet_angle = 0\n",
            "[drawing]\nsheet_gap = -1 mm\n",
            "[drawing]\nsheet_gap = 1001 mm\n",
        ] {
            assert!(
                crate::config::Settings::with_user_config(bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_cutting_line_is_drawn_exactly_when_the_view_shows_the_plane_and_it_is_in_the_box_with_a_fixed_seed(
    ) {
        let e = model();
        let mut state = 0x6C07_8965_D3E6_1F27u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let (mut drawn, mut left_out) = (0, 0);
        for _ in 0..600 {
            let view = [View::Top, View::Front, View::Side][(next() % 3) as usize];
            let axis = Dim::ALL[(next() % 3) as usize];
            let at = (next() % 140) as i64 - 20;
            let mut d = Drawing::of(&e, view, &bare()).unwrap();
            let before = d.clone();
            let shown = match view {
                View::Top => [Dim::X, Dim::Y],
                View::Front => [Dim::X, Dim::Z],
                View::Side => [Dim::Y, Dim::Z],
            };
            let length = if axis == shown[0] {
                d.extent.0
            } else {
                d.extent.1
            };
            let expected = shown.contains(&axis) && (0..=length as i64).contains(&at);
            let did = d.add_cut_line(axis, Length::from_mm(at).unwrap(), "A");
            assert_eq!(did, expected, "{view:?} {axis:?} {at}");
            if did {
                drawn += 1;
                // one line, four lines for each end, two letters
                assert_eq!(d.entities.len(), before.entities.len() + 9);
                assert_eq!(d.notes.len(), before.notes.len() + 2);
            } else {
                left_out += 1;
                assert_eq!(d, before, "{view:?} {axis:?} {at}");
            }
        }
        assert!(drawn > 50 && left_out > 50, "{drawn} {left_out}");
    }

    #[test]
    fn the_sheet_carries_the_cutting_line_once_on_the_first_view_that_shows_the_plane() {
        let e = model();
        let plain = Drawing::sheet(&e, &bare()).unwrap();
        for (axis, at) in [(Dim::X, 40), (Dim::Y, 10), (Dim::Z, 2)] {
            let cut = Drawing::sheet_cut(&e, &bare(), Some((axis, mmv(at), "A"))).unwrap();
            // one line, a mark and an arrow at each end, and the letters at both ends
            assert_eq!(cut.entities.len(), plain.entities.len() + 9, "{axis:?}");
            assert_eq!(cut.notes.len(), plain.notes.len() + 2, "{axis:?}");
        }
        // the plane across x is edge-on in the front, which is at the origin of the sheet
        let cut = Drawing::sheet_cut(&e, &bare(), Some((Dim::X, mmv(40), "A"))).unwrap();
        assert!(cut.entities.iter().any(|(l, e)| *l == Layer::Axes
            && matches!(*e, Entity::Line(a, b) if a.0 == 40.0 && b.0 == 40.0 && a.1 < 0.0)));
        // a plane outside the box, and no cut at all, leave the sheet as it was
        let out = Drawing::sheet_cut(&e, &bare(), Some((Dim::X, mmv(500), "A"))).unwrap();
        assert_eq!(out.entities, plain.entities);
        assert_eq!(out.notes.len(), plain.notes.len());
        let none = Drawing::sheet_cut(&e, &bare(), None).unwrap();
        assert_eq!(none.entities, plain.entities);
    }

    #[test]
    fn a_section_view_is_put_beside_the_sheet_under_its_letters() {
        let e = model();
        let mut sheet = Drawing::sheet(&e, &bare()).unwrap();
        let (_, hi) = sheet.bounds();
        let line = |a: (f64, f64), b: (f64, f64)| (Layer::Outline, Entity::Line(a, b));
        let section = Drawing {
            view: View::Top,
            entities: vec![
                line((3.0, 5.0), (33.0, 5.0)),
                line((33.0, 5.0), (33.0, 25.0)),
            ],
            notes: Vec::new(),
            extent: (30.0, 20.0),
            dxf_dimensions: false,
        };
        let before = sheet.entities.len();
        assert!(sheet.add_section_view(section.clone(), "A-A", 20.0));
        assert_eq!(sheet.entities.len(), before + 2);
        // 20 mm to the right of the sheet, the bottom of the section on the ground line
        assert_eq!(
            sheet.entities[before].1,
            Entity::Line((hi.0 + 20.0, 0.0), (hi.0 + 50.0, 0.0))
        );
        assert_eq!(
            sheet.entities[before + 1].1,
            Entity::Line((hi.0 + 50.0, 0.0), (hi.0 + 50.0, 20.0))
        );
        let note = sheet.notes.last().unwrap();
        assert_eq!(note.text, "A-A");
        assert_eq!(note.at, (hi.0 + 20.0, 20.0 + DIM_TEXT));
        assert!(note.measures.is_none());
        // the sheet now reaches to the right of the view, where the legend is put
        assert_eq!(sheet.extent, sheet.bounds().1);
        assert!(sheet.extent.0 >= hi.0 + 50.0);
        // a section with no line adds nothing
        let empty = Drawing {
            entities: Vec::new(),
            ..section
        };
        let (n, m) = (sheet.entities.len(), sheet.notes.len());
        assert!(!sheet.add_section_view(empty, "A-A", 20.0));
        assert_eq!((sheet.entities.len(), sheet.notes.len()), (n, m));
    }

    #[test]
    fn the_sheet_has_the_cutting_line_and_the_section_only_when_the_settings_ask_for_both() {
        use crate::config::Settings;
        use crate::export::{render, Format};
        let e = model();
        let text = |format: Format, s: &Settings| {
            String::from_utf8(render(format, &e, "", s).unwrap()).unwrap()
        };
        let user = |extra: &str| {
            Settings::with_user_config(&format!(
                "[drawing]\ncut_line = 1\ncut_at = 40 mm\ncut_label = S1\n{extra}"
            ))
            .unwrap()
        };
        assert!(!Settings::defaults().drawing.sheet_section);
        let off = user("");
        let on = user("sheet_section = 1\n");
        assert!(on.drawing.sheet_section);
        // off: the sheet is as it always was
        let svg_off = text(Format::SvgSheet, &off);
        assert!(!svg_off.contains("S1-S1") && !svg_off.contains(">S1</text>"));
        // on: the letters at both ends of the line and over the section
        let svg = text(Format::SvgSheet, &on);
        assert_eq!(svg.matches(">S1</text>").count(), 2);
        assert_eq!(svg.matches(">S1-S1</text>").count(), 1);
        let dxf = text(Format::DxfSheet, &on);
        assert_eq!(dxf.matches("\nS1\n").count(), 2);
        assert_eq!(dxf.matches("\nS1-S1\n").count(), 1);
        assert!(dxf.len() > text(Format::DxfSheet, &off).len());
        // the same bytes every time, and with the hatching by material too
        assert_eq!(svg, text(Format::SvgSheet, &on));
        let by_material = user("sheet_section = 1\nhatch_by_material = 1\n");
        assert_eq!(
            text(Format::SvgSheet, &by_material)
                .matches(">S1-S1</text>")
                .count(),
            1
        );
        // a plane outside the box draws neither the line nor the view
        let outside = Settings::with_user_config(
            "[drawing]\nsheet_section = 1\ncut_line = 1\ncut_at = 900 mm\n",
        )
        .unwrap();
        let base = Settings::with_user_config("[drawing]\n").unwrap();
        assert_eq!(
            text(Format::SvgSheet, &outside),
            text(Format::SvgSheet, &base)
        );
        // the section counts against the limit of entities
        // room for the sheet and its cutting line but not for the lines of the section
        let room = Drawing::sheet(&e, &off.drawing).unwrap().entities.len() + 10;
        let tight = user(&format!("sheet_section = 1\nmax_entities = {room}\n"));
        let limited = render(Format::SvgSheet, &e, "", &tight);
        assert!(limited.is_err(), "{:?}", limited.map(|b| b.len()));
        // the single views are as they were
        assert_eq!(text(Format::SvgTop, &on), text(Format::SvgTop, &off));
        // the flag takes 0 or 1 only
        assert!(Settings::with_user_config("[drawing]\nsheet_section = 2\n").is_err());
    }
}
