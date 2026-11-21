// SPDX-License-Identifier: GPL-3.0-or-later
//! From the model and the drawing to the list of primitives of the sheet: the frame, the views
//! with their visible and hidden edges, the dimensions, the balloons, the table of the bill and the
//! title block, with the problems found on the way. Nothing here reads the clock or the system:
//! the same model and the same drawing give the same list.

use std::collections::BTreeMap;

use super::bom::{part_nodes, BomLine};
use super::brk::{clip, marks as break_marks, warped_bounds, Warp};
use super::drag::CutLine;
use super::extended::{place as place_dims, DimLine, Slot};
use super::geom::{bounds_of, view_segments};
use super::measure::{
    draw, face_place, measure, resolve, view_axes, Frame, Issue, Measured, Model, Resolved,
};
use super::model::{
    revision_label, Axis, Detail, DetailShape, DimKind, NoteAnchor, Place, Plan, Ref, SheetScale,
    Source, View, ViewKind,
};
use super::prim::{mm_text, nm_text, Anchor, Prim, Shape};
use super::sectioning::{
    clip_detail, clip_solid, cut_face, cut_marks, detail_box, hatch_lines, CutLayers, Rect,
};
use super::settings::{DashPattern, DimStyle, SheetSettings};
use super::tol::annotate;
use crate::config::Settings;
use crate::enclosure::Id;
use crate::iso286::Table;
use crate::lang::{Lang, Msg};
use crate::prism::Limits;
use crate::sketch::{Element, ElementId, Sketch};
use crate::CadError;

const BAD_VIEW: &str = "PHOENIX-CAD-054";
const BAD_DIM: &str = "PHOENIX-CAD-056";
const BAD_BALLOON: &str = "PHOENIX-CAD-057";
const BAD_TOL: &str = "PHOENIX-CAD-058";
const BAD_SECTION: &str = "PHOENIX-CAD-059";
const BAD_BREAK: &str = "PHOENIX-CAD-061";
const BAD_NOTE: &str = "PHOENIX-CAD-062";

type Fail = (&'static str, String);

/// The sheet as primitives.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// The width of the sheet, in micrometres.
    pub width_um: i64,
    /// The height of the sheet, in micrometres.
    pub height_um: i64,
    /// What is drawn, in order, on every layer (the hidden layers are already left out).
    pub prims: Vec<Prim>,
    /// The colour, width and line type of every layer that is drawn on, by name.
    pub layers: BTreeMap<String, (String, i64, super::settings::LineType)>,
    /// The pattern of the dashed lines (from the settings).
    pub dashed: DashPattern,
    /// The pattern of the dash-dot lines (from the settings).
    pub dash_dot: DashPattern,
}

/// What became of a view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewReport {
    /// Its id.
    pub id: Id,
    /// Its name.
    pub name: String,
    /// What it looks at.
    pub kind: ViewKind,
    /// The scale `n:d` it is drawn at.
    pub scale: (u32, u32),
    /// The lower left corner of its picture on the sheet, in micrometres.
    pub origin: (i64, i64),
    /// The size of its picture on the sheet, in micrometres.
    pub size: (i64, i64),
    /// The box of its picture in the coordinates of the picture, in nanometres (after the range of
    /// a broken view is taken out).
    pub picture: ((i64, i64), (i64, i64)),
    /// The visible lines.
    pub visible: usize,
    /// The hidden lines.
    pub hidden: usize,
}

/// What became of a dimension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimReport {
    /// Its id.
    pub id: Id,
    /// Its name.
    pub name: String,
    /// What it measures.
    pub kind: DimKind,
    /// The texts of its values (with the tolerance), empty when it has an issue.
    pub texts: Vec<String>,
    /// Where its line is on the sheet, for picking it and dragging it; `None` for the kinds that
    /// are drawn with a leader or an arc.
    pub line: Option<DimLine>,
}

/// What became of a balloon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalloonReport {
    /// Its id.
    pub id: Id,
    /// Its name.
    pub name: String,
    /// The number of the item it points at, or `None` when the part is gone.
    pub number: Option<u32>,
}

/// What became of a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteReport {
    /// Its id.
    pub id: Id,
    /// Its text.
    pub text: String,
    /// Where the end of its leader is on the sheet, or `None` when what it points at is gone.
    pub tip: Option<(i64, i64)>,
}

/// Everything that evaluating a drawing gives.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    /// The sheet.
    pub scene: Scene,
    /// The scale of the sheet.
    pub scale: (u32, u32),
    /// The views that could be made.
    pub views: Vec<ViewReport>,
    /// The dimensions that could be measured.
    pub dims: Vec<DimReport>,
    /// The balloons.
    pub balloons: Vec<BalloonReport>,
    /// The notes.
    pub notes: Vec<NoteReport>,
    /// The lines of the cuts of the section views, for picking and dragging them.
    pub cuts: Vec<CutLine>,
    /// The bill, when the project has a history.
    pub bom: Vec<BomLine>,
    /// The problems, in the order of the records.
    pub issues: Vec<Issue>,
    /// Whether a tolerance of the sheet was worked out from a table of ISO 286 that says it has
    /// not been checked against the standard.
    pub unverified_table: bool,
}

impl Evaluation {
    /// Whether the drawing has no problem.
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

type Item = (&'static str, Shape);

struct Built {
    items: Vec<Item>,
    lo: (i64, i64),
    hi: (i64, i64),
    visible: usize,
    hidden: usize,
    /// The faces the cut of a section leaves open, in the picture of the view.
    cut: Vec<Rect>,
    /// The squeeze of a broken view; `lo` and `hi` are the box after it.
    warp: Option<Warp>,
}

/// The layer to draw on: the one asked for when the settings have it, else the outline.
fn layer_for<'a>(s: &SheetSettings, want: &'a str) -> &'a str {
    if s.layer(want).is_some() {
        want
    } else {
        "OUTLINE"
    }
}

fn sketch_items(sk: &Sketch) -> Option<Built> {
    let mut items: Vec<Item> = Vec::new();
    let mut lo = (i64::MAX, i64::MAX);
    let mut hi = (i64::MIN, i64::MIN);
    let mut grow = |p: (i64, i64), r: i64| {
        lo = (lo.0.min(p.0 - r), lo.1.min(p.1 - r));
        hi = (hi.0.max(p.0 + r), hi.1.max(p.1 + r));
    };
    let point = |id: ElementId| sk.point(id).ok().map(|(x, y)| (x.get(), y.get()));
    for e in sk.elements() {
        let layer = if e.construction { "CENTER" } else { "OUTLINE" };
        match e.element {
            Element::Line { a, b } => {
                let (Some(a), Some(b)) = (point(a), point(b)) else {
                    continue;
                };
                grow(a, 0);
                grow(b, 0);
                items.push((layer, Shape::Line { a, b }));
            }
            Element::Circle { center, radius } => {
                let Some(c) = point(center) else { continue };
                let r = radius.get();
                grow(c, r);
                items.push((layer, Shape::Circle { c, r }));
                let t = r / 3;
                items.push((
                    "CENTER",
                    Shape::Line {
                        a: (c.0 - t, c.1),
                        b: (c.0 + t, c.1),
                    },
                ));
                items.push((
                    "CENTER",
                    Shape::Line {
                        a: (c.0, c.1 - t),
                        b: (c.0, c.1 + t),
                    },
                ));
            }
            Element::Arc { center, start, end } => {
                let (Some(c), Some(s), Some(en)) = (point(center), point(start), point(end)) else {
                    continue;
                };
                let r = (((s.0 - c.0) as f64).hypot((s.1 - c.1) as f64)).round() as i64;
                let a0 = ((s.1 - c.1) as f64).atan2((s.0 - c.0) as f64).to_degrees();
                let mut a1 = ((en.1 - c.1) as f64)
                    .atan2((en.0 - c.0) as f64)
                    .to_degrees();
                while a1 <= a0 {
                    a1 += 360.0;
                }
                grow(c, r);
                items.push((layer, Shape::Arc { c, r, a0, a1 }));
            }
            Element::Point { .. } => {}
        }
    }
    if items.is_empty() {
        None
    } else {
        let visible = items.len();
        Some(Built {
            items,
            lo,
            hi,
            visible,
            hidden: 0,
            cut: Vec::new(),
            warp: None,
        })
    }
}

fn build_view(
    v: &View,
    model: &Model,
    s: &SheetSettings,
    prior: &[(usize, &View, Built)],
) -> Result<Built, Fail> {
    if let Some(d) = &v.detail {
        return build_detail(d, prior);
    }
    let bad = |why: String| -> Fail { (BAD_VIEW, why) };
    if let Source::Sketch(name) = &v.source {
        let sk = model
            .sketches
            .iter()
            .find(|k| k.name() == name)
            .ok_or_else(|| bad(format!("the sketch {name} is not in the project")))?;
        return sketch_items(sk)
            .ok_or_else(|| bad(format!("the sketch {name} has nothing to draw")));
    }
    let dag = model
        .dag
        .ok_or_else(|| bad("the project has no history of features".to_string()))?;
    let solid = match &v.source {
        Source::Tip => dag
            .result()
            .ok_or_else(|| bad("the history has no result".to_string()))?,
        Source::Node(id) => dag
            .solid(*id)
            .ok_or_else(|| bad(format!("the node {id} has no solid in the last rebuild")))?,
        Source::Sketch(_) => return Err(bad("not a sketch".to_string())),
    };
    if solid.is_empty() {
        return Err(bad("the solid is empty".to_string()));
    }
    let clipped;
    let mut cut = Vec::new();
    let solid = if let Some(c) = &v.section {
        let limits = Limits {
            max_cells: s.max_cells,
            max_prisms: 100_000,
        };
        clipped = clip_solid(solid, v.kind, c.at_nm, &limits)
            .map_err(|e| (BAD_SECTION, e.to_string()))?;
        if clipped.is_empty() {
            return Err((
                BAD_SECTION,
                "the cut leaves nothing of the solid".to_string(),
            ));
        }
        cut = cut_face(&clipped, v.kind, c.at_nm);
        if cut.is_empty() {
            return Err((
                BAD_SECTION,
                format!(
                    "the plane at {} mm does not cut the solid",
                    mm_text(c.at_nm / 1000)
                ),
            ));
        }
        &clipped
    } else {
        solid
    };
    let segs = view_segments(
        solid,
        v.kind,
        v.hidden,
        s.max_cells,
        s.max_edges,
        s.max_iso_tests,
    )
    .map_err(|e| bad(e.to_string()))?;
    let (segs, warp) = match &v.brk {
        None => (segs, None),
        Some(b) => {
            let w = Warp {
                along_u: b.along_u,
                from: b.from_nm,
                to: b.to_nm,
                gap: s.brk.gap_nm,
            };
            let (blo, bhi) = bounds_of(&segs)
                .ok_or_else(|| (BAD_BREAK, "nothing to draw in this view".to_string()))?;
            let (lo_c, hi_c) = if b.along_u {
                (blo.0, bhi.0)
            } else {
                (blo.1, bhi.1)
            };
            if b.from_nm <= lo_c || b.to_nm >= hi_c {
                return Err((
                    BAD_BREAK,
                    format!(
                        "the range {} to {} mm is not inside the picture of the view ({} to {} mm)",
                        nm_text(b.from_nm, 3),
                        nm_text(b.to_nm, 3),
                        nm_text(lo_c, 3),
                        nm_text(hi_c, 3)
                    ),
                ));
            }
            (clip(&segs, &w), Some(w))
        }
    };
    let bounds = match &warp {
        Some(w) => warped_bounds(&segs, w),
        None => bounds_of(&segs),
    };
    let (lo, hi) = bounds.ok_or_else(|| bad("nothing to draw in this view".to_string()))?;
    let hidden = segs.iter().filter(|g| g.hidden).count();
    let items = segs
        .into_iter()
        .map(|g| {
            (
                if g.hidden { "HIDDEN" } else { "OUTLINE" },
                Shape::Line { a: g.a, b: g.b },
            )
        })
        .collect::<Vec<_>>();
    let visible = items.len() - hidden;
    Ok(Built {
        items,
        lo,
        hi,
        visible,
        hidden,
        cut,
        warp,
    })
}

/// A detail: the lines of the view it enlarges that are inside its circle or rectangle, and the
/// border of that shape.
fn build_detail(d: &Detail, prior: &[(usize, &View, Built)]) -> Result<Built, Fail> {
    let parent = prior.iter().find(|(_, v, _)| v.id == d.parent).ok_or((
        BAD_SECTION,
        "the view that is enlarged could not be made".to_string(),
    ))?;
    let (lo, hi) = detail_box(d.centre, d.shape);
    let mut items: Vec<Item> = Vec::new();
    let (mut visible, mut hidden) = (0, 0);
    for (layer, shape) in &parent.2.items {
        if let Shape::Line { a, b } = shape {
            if let Some((p, q)) = clip_detail(*a, *b, d.centre, d.shape) {
                items.push((layer, Shape::Line { a: p, b: q }));
                if *layer == "HIDDEN" {
                    hidden += 1;
                } else {
                    visible += 1;
                }
            }
        }
    }
    if items.is_empty() {
        return Err((
            BAD_SECTION,
            "nothing of the view is inside the shape of the detail".to_string(),
        ));
    }
    match d.shape {
        DetailShape::Circle { radius_nm } => items.push((
            "DETAIL",
            Shape::Circle {
                c: d.centre,
                r: radius_nm,
            },
        )),
        DetailShape::Rect { .. } => {
            for (a, b) in [
                ((lo.0, lo.1), (hi.0, lo.1)),
                ((hi.0, lo.1), (hi.0, hi.1)),
                ((hi.0, hi.1), (lo.0, hi.1)),
                ((lo.0, hi.1), (lo.0, lo.1)),
            ] {
                items.push(("DETAIL", Shape::Line { a, b }));
            }
        }
    }
    Ok(Built {
        items,
        lo,
        hi,
        visible,
        hidden,
        cut: Vec::new(),
        warp: None,
    })
}

/// The marks of a detail in the view it was taken from: the circle or the rectangle and its letter.
fn detail_marks(d: &Detail, parent: &Frame, layer: &str, text_h: i64) -> Vec<Prim> {
    let c = parent.map(d.centre);
    match d.shape {
        DetailShape::Circle { radius_nm } => {
            let r = parent.len(radius_nm);
            vec![
                Prim {
                    layer: layer.to_string(),
                    shape: Shape::Circle { c, r },
                },
                Prim::text(
                    "TEXT",
                    (c.0 + r * 7 / 10 + text_h / 2, c.1 + r * 7 / 10 + text_h / 2),
                    text_h,
                    &d.label,
                    Anchor::Start,
                ),
            ]
        }
        DetailShape::Rect { .. } => {
            let (lo, hi) = detail_box(d.centre, d.shape);
            let (a, b) = (parent.map(lo), parent.map(hi));
            let mut out = rect(layer, a.0, a.1, b.0, b.1);
            out.push(Prim::text(
                "TEXT",
                (b.0 + text_h / 3, b.1 + text_h / 3),
                text_h,
                &d.label,
                Anchor::Start,
            ));
            out
        }
    }
}

fn map_shape(shape: &Shape, f: &Frame) -> Shape {
    match shape {
        Shape::Line { a, b } => Shape::Line {
            a: f.map(*a),
            b: f.map(*b),
        },
        Shape::Circle { c, r } => Shape::Circle {
            c: f.map(*c),
            r: f.len(*r),
        },
        Shape::Arc { c, r, a0, a1 } => Shape::Arc {
            c: f.map(*c),
            r: f.len(*r),
            a0: *a0,
            a1: *a1,
        },
        other => other.clone(),
    }
}

/// The text of a field of the title block in a language.
fn field_label(key: &str, lang: Lang) -> String {
    match key {
        "organisation" => Msg::TbOrganisation.tr(lang).to_string(),
        "title" => Msg::TbTitle.tr(lang).to_string(),
        "number" => Msg::TbNumber.tr(lang).to_string(),
        "revision" => Msg::TbRevision.tr(lang).to_string(),
        "author" => Msg::TbAuthor.tr(lang).to_string(),
        "scale" => Msg::TbScale.tr(lang).to_string(),
        "sheet" => Msg::TbSheet.tr(lang).to_string(),
        "material" => Msg::TbMaterial.tr(lang).to_string(),
        other => other.to_string(),
    }
}

fn ratio(a: (u32, u32), b: (u32, u32)) -> std::cmp::Ordering {
    (u64::from(a.0) * u64::from(b.1)).cmp(&(u64::from(b.0) * u64::from(a.1)))
}

struct Placed {
    index: usize,
    origin: (i64, i64),
    size: (i64, i64),
    scale: (u32, u32),
    manual: bool,
}

/// Where the views go for a sheet scale: the top and the side views aligned with the front one by
/// the world coordinates they share (above and to the right in the third angle, below and to the
/// left in the first), the isometric view in the free corner of the third angle, the rest in a
/// row to the right, and the group centred in the area. A view placed by hand stays where it is.
fn layout(
    views: &[(usize, &View, &Built)],
    sheet_scale: (u32, u32),
    plan: &Plan,
    s: &SheetSettings,
    area: ((i64, i64), (i64, i64)),
) -> Vec<Placed> {
    let gap = s.view_gap_um;
    let measure_of = |v: &View, b: &Built| -> ((u32, u32), (i64, i64), Frame) {
        let sc = v.scale.unwrap_or(sheet_scale);
        let f = Frame {
            kind: v.kind,
            lo: b.lo,
            hi: b.hi,
            origin: (0, 0),
            scale: sc,
            brk: b.warp,
        };
        (sc, f.size(), f)
    };
    let auto: Vec<&(usize, &View, &Built)> = views
        .iter()
        .filter(|(_, v, _)| matches!(v.place, Place::Auto))
        .collect();
    let mut placed: Vec<Placed> = Vec::new();
    if let Some(base) = auto
        .iter()
        .find(|(_, v, _)| v.kind == ViewKind::Front && !v.is_derived())
        .or_else(|| auto.iter().find(|(_, v, _)| !v.is_derived()))
        .or_else(|| auto.first())
    {
        let (bsc, bsize, bframe) = measure_of(base.1, base.2);
        placed.push(Placed {
            index: base.0,
            origin: (0, 0),
            size: bsize,
            scale: bsc,
            manual: false,
        });
        let mut taken = vec![base.0];
        let mut top: Option<(i64, i64)> = None;
        let mut right: Option<(i64, i64)> = None;
        if base.1.kind == ViewKind::Front && !base.1.is_derived() {
            if let Some(t) = auto
                .iter()
                .find(|(i, v, _)| !taken.contains(i) && v.kind == ViewKind::Top && !v.is_derived())
            {
                let (sc, sz, _) = measure_of(t.1, t.2);
                let x = bframe.len(t.2.lo.0 - base.2.lo.0);
                let y = if plan.third_angle {
                    bsize.1 + gap
                } else {
                    -gap - sz.1
                };
                placed.push(Placed {
                    index: t.0,
                    origin: (x, y),
                    size: sz,
                    scale: sc,
                    manual: false,
                });
                taken.push(t.0);
                top = Some((x, y));
            }
            if let Some(r) = auto.iter().find(|(i, v, _)| {
                !taken.contains(i) && v.kind == ViewKind::Right && !v.is_derived()
            }) {
                let (sc, sz, _) = measure_of(r.1, r.2);
                let x = if plan.third_angle {
                    bsize.0 + gap
                } else {
                    -gap - sz.0
                };
                let y = bframe.len(r.2.lo.1 - base.2.lo.1);
                placed.push(Placed {
                    index: r.0,
                    origin: (x, y),
                    size: sz,
                    scale: sc,
                    manual: false,
                });
                taken.push(r.0);
                right = Some((x, y));
            }
            if let (true, Some(t), Some(r)) = (plan.third_angle, top, right) {
                if let Some(i) = auto.iter().find(|(i, v, _)| {
                    !taken.contains(i) && v.kind == ViewKind::Iso && !v.is_derived()
                }) {
                    let (sc, sz, _) = measure_of(i.1, i.2);
                    placed.push(Placed {
                        index: i.0,
                        origin: (r.0, t.1),
                        size: sz,
                        scale: sc,
                        manual: false,
                    });
                    taken.push(i.0);
                }
            }
        }
        let mut x = placed
            .iter()
            .map(|p| p.origin.0 + p.size.0)
            .max()
            .unwrap_or(0)
            + gap;
        for (i, v, b) in &auto {
            if taken.contains(i) {
                continue;
            }
            let (sc, sz, _) = measure_of(v, b);
            placed.push(Placed {
                index: *i,
                origin: (x, 0),
                size: sz,
                scale: sc,
                manual: false,
            });
            x += sz.0 + gap;
        }
        let lo_x = placed.iter().map(|p| p.origin.0).min().unwrap_or(0);
        let lo_y = placed.iter().map(|p| p.origin.1).min().unwrap_or(0);
        let hi_x = placed
            .iter()
            .map(|p| p.origin.0 + p.size.0)
            .max()
            .unwrap_or(0);
        let hi_y = placed
            .iter()
            .map(|p| p.origin.1 + p.size.1)
            .max()
            .unwrap_or(0);
        let (aw, ah) = (area.1 .0 - area.0 .0, area.1 .1 - area.0 .1);
        let dx = area.0 .0 + (aw - (hi_x - lo_x)) / 2 - lo_x;
        let dy = area.0 .1 + (ah - (hi_y - lo_y)) / 2 - lo_y;
        for p in &mut placed {
            p.origin = (p.origin.0 + dx, p.origin.1 + dy);
        }
    }
    for (i, v, b) in views {
        if let Place::At(x, y) = v.place {
            let (sc, sz, _) = measure_of(v, b);
            placed.push(Placed {
                index: *i,
                origin: (x, y),
                size: sz,
                scale: sc,
                manual: true,
            });
        }
    }
    placed
}

/// The size of the group of views, for choosing a scale, without the centring.
fn group_size(
    views: &[(usize, &View, &Built)],
    sheet_scale: (u32, u32),
    plan: &Plan,
    s: &SheetSettings,
) -> (i64, i64) {
    let big = ((0, 0), (i64::MAX / 4, i64::MAX / 4));
    let placed = layout(views, sheet_scale, plan, s, big);
    let auto: Vec<&Placed> = placed.iter().filter(|p| !p.manual).collect();
    let w = auto
        .iter()
        .map(|p| p.origin.0 + p.size.0)
        .max()
        .unwrap_or(0)
        - auto.iter().map(|p| p.origin.0).min().unwrap_or(0);
    let h = auto
        .iter()
        .map(|p| p.origin.1 + p.size.1)
        .max()
        .unwrap_or(0)
        - auto.iter().map(|p| p.origin.1).min().unwrap_or(0);
    (w, h)
}

fn rect(layer: &str, x0: i64, y0: i64, x1: i64, y1: i64) -> Vec<Prim> {
    vec![
        Prim::line(layer, (x0, y0), (x1, y0)),
        Prim::line(layer, (x1, y0), (x1, y1)),
        Prim::line(layer, (x1, y1), (x0, y1)),
        Prim::line(layer, (x0, y1), (x0, y0)),
    ]
}

/// Evaluates a drawing against the model: the scene and every problem found. An `Err` is only a
/// limit of the settings that was exceeded; a view, a dimension or a balloon that cannot be made
/// is an issue of the evaluation and the rest of the sheet is drawn.
pub fn evaluate(
    plan: &Plan,
    model: &Model,
    cfg: &Settings,
    s: &SheetSettings,
    lang: Lang,
) -> Result<Evaluation, CadError> {
    plan.validate(s)?;
    let (sw, sh) = s
        .sheet_size_um(&plan.format, plan.portrait)
        .ok_or_else(|| {
            CadError::Plan(format!("the format {} is not in the settings", plan.format))
        })?;
    let mut issues: Vec<Issue> = Vec::new();
    // the bill
    let parts = match model.dag {
        Some(d) => part_nodes(d, cfg, s.max_parts)?,
        None => Vec::new(),
    };
    let bom = match model.dag {
        Some(d) => plan.bom(d, cfg, s)?,
        None => Vec::new(),
    };
    // the views
    let mut built: Vec<(usize, &View, Built)> = Vec::new();
    for (i, v) in plan.views.iter().enumerate() {
        match build_view(v, model, s, &built) {
            Ok(b) => built.push((i, v, b)),
            Err((code, why)) => issues.push(Issue {
                code,
                subject: format!("view {}", v.name),
                detail: why,
            }),
        }
    }
    let refs: Vec<(usize, &View, &Built)> = built.iter().map(|(i, v, b)| (*i, *v, b)).collect();
    // the room
    let margin = s.margin_um;
    let tpl = s
        .title_block_of(plan.title_template.as_deref())
        .or_else(|| s.title_block_of(Some("default")))
        .ok_or_else(|| CadError::Plan("the title block is not in the settings".into()))?;
    let rows = if s.title_block && !tpl.fields.is_empty() {
        (tpl.fields.len() as i64 + i64::from(tpl.columns) - 1) / i64::from(tpl.columns)
    } else {
        0
    };
    let title_h = rows * tpl.row_um;
    let rev_rows = if s.revisions.table && !plan.revisions.is_empty() {
        plan.revisions.len() as i64 + 1
    } else {
        0
    };
    let rev_h = rev_rows * s.revisions.row_um;
    let bom_h = if s.bom_table && !bom.is_empty() {
        (bom.len() as i64 + 1) * s.bom_row_um
    } else {
        0
    };
    let area = (
        (
            margin + s.view_gap_um / 2,
            margin + title_h + bom_h + s.view_gap_um / 2,
        ),
        (
            sw - margin - s.view_gap_um / 2,
            sh - margin - s.view_gap_um / 2 - rev_h,
        ),
    );
    // the scale
    let scale = match plan.scale {
        SheetScale::Ratio(n, d) => (n, d),
        SheetScale::Auto => {
            let mut options = s.scales.clone();
            options.sort_by(|a, b| ratio(*b, *a));
            let (aw, ah) = (area.1 .0 - area.0 .0, area.1 .1 - area.0 .1);
            options
                .iter()
                .copied()
                .find(|sc| {
                    let (w, h) = group_size(&refs, *sc, plan, s);
                    w <= aw && h <= ah
                })
                .unwrap_or_else(|| *options.last().unwrap_or(&(1, 1)))
        }
    };
    let placed = layout(&refs, scale, plan, s, area);
    let mut prims: Vec<Prim> = Vec::new();
    // the frame
    prims.extend(rect("FRAME", margin, margin, sw - margin, sh - margin));
    // the views
    let mut frames: BTreeMap<Id, Frame> = BTreeMap::new();
    let mut view_reports = Vec::new();
    for (idx, v, b) in &built {
        let Some(p) = placed.iter().find(|p| p.index == *idx) else {
            continue;
        };
        let frame = Frame {
            kind: v.kind,
            lo: b.lo,
            hi: b.hi,
            origin: p.origin,
            scale: p.scale,
            brk: b.warp,
        };
        let (x1, y1) = (p.origin.0 + p.size.0, p.origin.1 + p.size.1);
        if p.origin.0 < margin || p.origin.1 < margin || x1 > sw - margin || y1 > sh - margin {
            issues.push(Issue {
                code: if v.brk.is_some() { BAD_BREAK } else { BAD_VIEW },
                subject: format!("view {}", v.name),
                detail: format!(
                    "the view is not inside the frame of the sheet (box {} to {} by {} to {} mm)",
                    mm_text(p.origin.0),
                    mm_text(x1),
                    mm_text(p.origin.1),
                    mm_text(y1)
                ),
            });
        }
        for (layer, shape) in &b.items {
            prims.push(Prim {
                layer: layer_for(s, layer).to_string(),
                shape: map_shape(shape, &frame),
            });
        }
        if let Some(w) = &b.warp {
            let at = |c: i64| {
                let p = frame.map(if w.along_u { (c, 0) } else { (0, c) });
                if w.along_u {
                    p.0
                } else {
                    p.1
                }
            };
            prims.extend(break_marks(
                layer_for(s, "BREAK"),
                w,
                at(w.from),
                at(w.to),
                p.origin,
                (p.origin.0 + p.size.0, p.origin.1 + p.size.1),
                &s.brk,
            ));
        }
        if !b.cut.is_empty() {
            let rects: Vec<Rect> = b
                .cut
                .iter()
                .map(|(lo, hi)| (frame.map(*lo), frame.map(*hi)))
                .collect();
            let material = v.hatch.clone().or_else(|| match &v.source {
                Source::Node(n) => plan.parts.get(n).and_then(|i| i.material.clone()),
                _ => None,
            });
            let style = material
                .and_then(|m| s.hatch.materials.get(&m.to_lowercase()).copied())
                .unwrap_or(s.hatch.default);
            match hatch_lines(&rects, style, s.hatch.max_lines) {
                Ok(lines) => {
                    for (a, c) in lines {
                        prims.push(Prim::line(layer_for(s, "HATCH"), a, c));
                    }
                }
                Err(n) => issues.push(Issue {
                    code: BAD_SECTION,
                    subject: format!("view {}", v.name),
                    detail: format!(
                        "the hatching needs {n} lines, the settings allow {}",
                        s.hatch.max_lines
                    ),
                }),
            }
        }
        let caption = match (&v.section, &v.detail) {
            (Some(c), _) => Some(format!("{0}-{0}", c.label)),
            (_, Some(d)) => Some(format!("{} ({}:{})", d.label, p.scale.0, p.scale.1)),
            _ => None,
        };
        if let Some(text) = caption {
            let th = s.section.text_height_um;
            prims.push(Prim::text(
                "TEXT",
                (
                    p.origin.0 + p.size.0 / 2,
                    p.origin.1 - s.section.caption_gap_um - th,
                ),
                th,
                &text,
                Anchor::Middle,
            ));
        }
        frames.insert(v.id, frame);
        view_reports.push(ViewReport {
            id: v.id,
            name: v.name.clone(),
            kind: v.kind,
            scale: p.scale,
            origin: p.origin,
            size: p.size,
            picture: (b.lo, b.hi),
            visible: b.visible,
            hidden: b.hidden,
        });
    }
    // the lines of the cuts and the marks of the details, in the views they are drawn in
    let mut cut_lines: Vec<CutLine> = Vec::new();
    for (_, v, _) in &built {
        if let Some(c) = &v.section {
            if let Some(pf) = frames.get(&c.parent) {
                if let (Some(a), Some((u, _))) =
                    (v.kind.depth_axis().map(Axis::index), view_axes(pf.kind))
                {
                    let size = pf.size();
                    let vertical = a == u.index();
                    let (at_um, span, lo_nm, origin_um) = if vertical {
                        (
                            pf.map((c.at_nm, 0)).0,
                            (pf.origin.1, pf.origin.1 + size.1),
                            pf.lo.0,
                            pf.origin.0,
                        )
                    } else {
                        (
                            pf.map((0, c.at_nm)).1,
                            (pf.origin.0, pf.origin.0 + size.0),
                            pf.lo.1,
                            pf.origin.1,
                        )
                    };
                    cut_lines.push(CutLine {
                        view: v.id,
                        vertical,
                        at_um,
                        span,
                        at_nm: c.at_nm,
                        lo_nm,
                        origin_um,
                        scale: pf.scale,
                        draggable: pf.brk.is_none(),
                    });
                }
            }
            let layers = CutLayers {
                line: layer_for(s, "SECTION").to_string(),
                mark: layer_for(s, "OUTLINE").to_string(),
                text: "TEXT".to_string(),
            };
            let drawn = frames.get(&c.parent).map(|pf| {
                cut_marks(
                    pf,
                    v.kind,
                    c.at_nm,
                    &c.label,
                    &s.section,
                    s.dim.arrow_length_um,
                    s.dim.arrow_width_um,
                    &layers,
                )
            });
            match drawn {
                Some(Some(ps)) => prims.extend(ps),
                Some(None) => issues.push(Issue {
                    code: BAD_SECTION,
                    subject: format!("view {}", v.name),
                    detail: "the view of the line of the cut does not show the axis of the cut"
                        .to_string(),
                }),
                None => issues.push(Issue {
                    code: BAD_SECTION,
                    subject: format!("view {}", v.name),
                    detail: "the view of the line of the cut could not be made".to_string(),
                }),
            }
        }
        if let Some(d) = &v.detail {
            if let Some(pf) = frames.get(&d.parent) {
                prims.extend(detail_marks(
                    d,
                    pf,
                    layer_for(s, "DETAIL"),
                    s.section.text_height_um,
                ));
            }
        }
    }
    // the dimensions: measured first (with their tolerances), then the ones that were not given a
    // place are put on lines where they do not meet another, then drawn
    let table = plan
        .dims
        .iter()
        .any(|d| d.tol.is_some())
        .then(Table::shipped);
    let mut pending: Vec<(&super::model::Dim, Result<Measured, Fail>)> = Vec::new();
    let mut slots: Vec<Slot> = Vec::new();
    for d in &plan.dims {
        // what the dimension is tied to is looked for first: a thing that is gone is an orphan,
        // whatever became of the view
        let mut measured: Result<Measured, Fail> = measure(d, model, &s.dim);
        if let (Ok(m), Some(tol), Some(table)) = (measured.as_mut(), d.tol.as_ref(), table.as_ref())
        {
            if let Err(why) = write_tolerance(m, tol, table, &s.dim) {
                measured = Err((BAD_TOL, format!("{}: {why}", d.name)));
            }
        }
        if let (Ok(m), None, Some(frame)) = (&measured, d.offset_um, frames.get(&d.view)) {
            if slots.len() < s.max_auto_dims {
                if let Some((horizontal, lo, hi)) = extent_of(d.kind, m, frame) {
                    slots.push(Slot {
                        id: d.id,
                        view: d.view,
                        horizontal,
                        lo,
                        hi,
                        step: d.step_um.unwrap_or(s.dim.step_um),
                        lines: if d.kind == DimKind::Cumulative {
                            m.refs.len() as i64 - 1
                        } else {
                            1
                        },
                    });
                }
            }
        }
        pending.push((d, measured));
    }
    let bases = place_dims(&slots, s.dim.clearance_um);
    let mut dim_reports = Vec::new();
    for (d, measured) in pending {
        let subject = format!("dimension {}", d.name);
        let sketch_name = plan.view(d.view).and_then(|v| match &v.source {
            Source::Sketch(n) => Some(n.as_str()),
            _ => None,
        });
        let mut placed = d.clone();
        if d.offset_um.is_none() {
            if let Some(base) = bases.get(&d.id).filter(|b| **b != 0) {
                placed.offset_um = Some(s.dim.offset_um + base);
            }
        }
        let drawn = measured.and_then(|m| match frames.get(&d.view) {
            Some(frame) => draw(&placed, &m, frame, &s.dim, sketch_name).map(|p| (m, p)),
            None => Err((BAD_DIM, "its view could not be made".to_string())),
        });
        match drawn {
            Ok((m, ps)) => {
                prims.extend(ps);
                let line = frames.get(&d.view).and_then(|frame| {
                    extent_of(d.kind, &m, frame).map(|(horizontal, lo, hi)| DimLine {
                        horizontal,
                        view_origin: frame.origin,
                        level_um: placed.offset_um.unwrap_or(s.dim.offset_um),
                        lo,
                        hi,
                    })
                });
                dim_reports.push(DimReport {
                    id: d.id,
                    name: d.name.clone(),
                    kind: d.kind,
                    texts: m.texts,
                    line,
                });
            }
            Err((code, detail)) => issues.push(Issue {
                code,
                subject,
                detail,
            }),
        }
    }
    // the table of the bill and the title block, at the lower right corner
    let main_part: Option<Id> = plan
        .views
        .iter()
        .find_map(|v| match v.source {
            Source::Node(n) => Some(n),
            _ => None,
        })
        .or_else(|| (bom.len() == 1).then(|| bom[0].node));
    let right = sw - margin;
    let mut y = margin;
    if s.title_block && rows > 0 {
        let w = tpl.width_um.min(sw - 2 * margin);
        let x0 = right - w;
        let cols = i64::from(tpl.columns);
        let cw = w / cols;
        prims.extend(rect("FRAME", x0, y, x0 + cw * cols, y + title_h));
        for (k, key) in tpl.fields.iter().enumerate() {
            let (col, row) = (k as i64 % cols, k as i64 / cols);
            let (cx, cy) = (x0 + col * cw, y + title_h - (row + 1) * tpl.row_um);
            prims.extend(rect("FRAME", cx, cy, cx + cw, cy + tpl.row_um));
            let value = match key.as_str() {
                "scale" => format!("{}:{}", scale.0, scale.1),
                "sheet" => "1/1".to_string(),
                other => plan
                    .fields
                    .get(other)
                    .cloned()
                    .or_else(|| {
                        let key = s
                            .title_from_attr
                            .iter()
                            .find(|(f, _)| f == other)
                            .map(|(_, k)| k.as_str())?;
                        plan.attr(main_part?, key).map(str::to_string)
                    })
                    .or_else(|| s.title_values.get(other).cloned())
                    .unwrap_or_default(),
            };
            let label_h = tpl.row_um * 22 / 100;
            let value_h = tpl.row_um * 40 / 100;
            prims.push(Prim::text(
                "TEXT",
                (cx + label_h / 2, cy + tpl.row_um - label_h * 12 / 10),
                label_h,
                &field_label(key, lang),
                Anchor::Start,
            ));
            prims.push(Prim::text(
                "TEXT",
                (cx + label_h, cy + label_h / 2),
                value_h,
                &value,
                Anchor::Start,
            ));
        }
        y += title_h;
    }
    if bom_h > 0 {
        let w = s.bom_width_um.min(sw - 2 * margin);
        let x0 = right - w;
        let k = s.bom_attr_columns.len() as i64;
        let attr_w = w * 10 / 100;
        let mut widths = vec![w / 10, w / 10, w * 55 / 100 - attr_w * k];
        widths.push(w - widths.iter().sum::<i64>() - attr_w * k);
        widths.extend(std::iter::repeat_n(attr_w, s.bom_attr_columns.len()));
        let mut cols = vec![x0];
        for cw in &widths {
            cols.push(cols[cols.len() - 1] + cw);
        }
        if let Some(last) = cols.last_mut() {
            *last = right;
        }
        let heads = [Msg::PlItem, Msg::PlQty, Msg::PlDescription, Msg::TbMaterial];
        let rh = s.bom_row_um;
        let th = rh * 55 / 100;
        for (r, row) in std::iter::once(None)
            .chain(bom.iter().map(Some))
            .enumerate()
        {
            let ry = y + r as i64 * rh;
            prims.extend(rect("FRAME", x0, ry, right, ry + rh));
            for x in &cols[1..cols.len() - 1] {
                prims.push(Prim::line("FRAME", (*x, ry), (*x, ry + rh)));
            }
            let mut texts: Vec<String> = match row {
                None => heads.map(|m| m.tr(lang).to_string()).to_vec(),
                Some(l) => vec![
                    l.number.to_string(),
                    l.quantity.to_string(),
                    l.description.clone(),
                    l.material.clone(),
                ],
            };
            match row {
                None => texts.extend(s.bom_attr_columns.iter().cloned()),
                Some(l) => texts.extend(l.attrs.iter().cloned()),
            }
            for (c, t) in texts.iter().enumerate() {
                prims.push(Prim::text(
                    "TEXT",
                    (cols[c] + th / 3, ry + rh / 4),
                    th,
                    t,
                    Anchor::Start,
                ));
            }
        }
    }
    // the table of revisions, at the upper right corner of the frame
    if rev_h > 0 {
        let w = s.revisions.width_um.min(sw - 2 * margin);
        let x0 = right - w;
        let top = sh - margin;
        let widths = [w / 10, w / 5, w * 5 / 10, w - w / 10 - w / 5 - w * 5 / 10];
        let cols = [
            x0,
            x0 + widths[0],
            x0 + widths[0] + widths[1],
            x0 + widths[0] + widths[1] + widths[2],
            right,
        ];
        let rh = s.revisions.row_um;
        let th = rh * 55 / 100;
        let heads = [
            Msg::TbRevision,
            Msg::TbDate,
            Msg::PlDescription,
            Msg::TbAuthor,
        ];
        for r in 0..rev_rows {
            let ry = top - (r + 1) * rh;
            prims.extend(rect("FRAME", x0, ry, right, ry + rh));
            for x in &cols[1..4] {
                prims.push(Prim::line("FRAME", (*x, ry), (*x, ry + rh)));
            }
            let texts: [String; 4] = if r == 0 {
                heads.map(|m| m.tr(lang).to_string())
            } else {
                let rev = &plan.revisions[(r - 1) as usize];
                [
                    revision_label(rev.number, s.revisions.numbering),
                    rev.date.clone(),
                    rev.description.clone(),
                    rev.author.clone(),
                ]
            };
            for (c, text) in texts.iter().enumerate() {
                prims.push(Prim::text(
                    "TEXT",
                    (cols[c] + th / 3, ry + rh / 4),
                    th,
                    text,
                    Anchor::Start,
                ));
            }
        }
    }
    // the balloons
    let (numbers, _) = plan.numbering(&parts);
    let mut balloon_reports = Vec::new();
    for b in &plan.balloons {
        let in_bom = parts.iter().any(|(id, _)| *id == b.part);
        let number = in_bom.then(|| numbers.get(&b.part).copied()).flatten();
        balloon_reports.push(BalloonReport {
            id: b.id,
            name: b.name.clone(),
            number,
        });
        let Some(n) = number else {
            issues.push(Issue {
                code: BAD_BALLOON,
                subject: format!("balloon {}", b.name),
                detail: format!("the part {} is not in the bill of the history", b.part),
            });
            continue;
        };
        let r = s.balloon_radius_um;
        prims.push(Prim {
            layer: "BALLOON".to_string(),
            shape: Shape::Circle { c: b.at, r },
        });
        let (dx, dy) = ((b.to.0 - b.at.0) as f64, (b.to.1 - b.at.1) as f64);
        let len = dx.hypot(dy);
        if len > r as f64 {
            let from = super::prim::pt(
                b.at.0 as f64 + dx / len * r as f64,
                b.at.1 as f64 + dy / len * r as f64,
            );
            prims.push(Prim::line("BALLOON", from, b.to));
            prims.extend(super::prim::arrow(
                "BALLOON",
                b.to,
                (-dx, -dy),
                s.dim.arrow_length_um,
                s.dim.arrow_width_um,
            ));
        }
        let th = s.balloon_text_um;
        prims.push(Prim::text(
            "BALLOON",
            (b.at.0, b.at.1 - th / 3),
            th,
            &n.to_string(),
            Anchor::Middle,
        ));
    }
    // the notes
    let mut note_reports = Vec::new();
    for n in &plan.notes {
        let subject = format!("note {}", n.text);
        let tip: Result<(i64, i64), Fail> = match &n.anchor {
            NoteAnchor::View { view, u_nm, v_nm } => frames
                .get(view)
                .map(|f| f.map((*u_nm, *v_nm)))
                .ok_or((BAD_NOTE, "its view could not be made".to_string())),
            NoteAnchor::Face {
                view,
                node,
                axis,
                side,
            } => match frames.get(view) {
                None => Err((BAD_NOTE, "its view could not be made".to_string())),
                Some(f) => resolve(
                    &Ref::Face {
                        node: *node,
                        axis: *axis,
                        side: *side,
                    },
                    model,
                )
                .and_then(|r| match (r, view_axes(f.kind)) {
                    (Resolved::Face { axis, pos, lo, hi }, Some((ua, va))) => {
                        let mid = |a: Axis| (lo[a.index()] + hi[a.index()]) / 2;
                        Ok(f.map(if axis == ua {
                            (pos, mid(va))
                        } else if axis == va {
                            (mid(ua), pos)
                        } else {
                            (mid(ua), mid(va))
                        }))
                    }
                    _ => Err((BAD_NOTE, "the view shows no face".to_string())),
                }),
            },
        };
        note_reports.push(NoteReport {
            id: n.id,
            text: n.text.clone(),
            tip: tip.as_ref().ok().copied(),
        });
        let tip = match tip {
            Ok(p) => p,
            Err((code, detail)) => {
                issues.push(Issue {
                    code,
                    subject,
                    detail,
                });
                continue;
            }
        };
        let h = s.note.text_height_um;
        prims.push(Prim::text(&n.layer, n.at, h, &n.text, Anchor::Start));
        let from = (n.at.0 - h / 3, n.at.1 + h / 3);
        let (dx, dy) = ((tip.0 - from.0) as f64, (tip.1 - from.1) as f64);
        if dx.hypot(dy) > 1.0 {
            prims.push(Prim::line(&n.layer, from, tip));
            if !n.centre {
                prims.extend(super::prim::arrow(
                    &n.layer,
                    tip,
                    (-dx, -dy),
                    s.dim.arrow_length_um,
                    s.dim.arrow_width_um,
                ));
            }
        }
        if n.centre {
            let m = s.note.centre_mark_um;
            prims.push(Prim::line(&n.layer, (tip.0 - m, tip.1), (tip.0 + m, tip.1)));
            prims.push(Prim::line(&n.layer, (tip.0, tip.1 - m), (tip.0, tip.1 + m)));
        }
    }
    // only the layers that are shown
    prims.retain(|p| plan.layer_visible(&p.layer, s));
    let mut layers = BTreeMap::new();
    for p in &prims {
        layers.entry(p.layer.clone()).or_insert_with(|| {
            (
                plan.layer_color(&p.layer, s),
                plan.layer_width_um(&p.layer, s),
                s.layer(&p.layer)
                    .map_or(super::settings::LineType::Continuous, |l| l.line),
            )
        });
    }
    Ok(Evaluation {
        scene: Scene {
            width_um: sw,
            height_um: sh,
            prims,
            layers,
            dashed: s.dashed,
            dash_dot: s.dash_dot,
        },
        scale,
        views: view_reports,
        dims: dim_reports,
        balloons: balloon_reports,
        notes: note_reports,
        cuts: cut_lines,
        bom,
        issues,
        unverified_table: table.is_some_and(|t| !t.verified),
    })
}

/// Writes the tolerance next to every value of a measured dimension.
fn write_tolerance(
    m: &mut Measured,
    tol: &super::tol::Tol,
    table: &Table,
    style: &DimStyle,
) -> Result<(), String> {
    for k in 0..m.values.len() {
        m.texts[k] = annotate(
            &m.texts[k],
            tol,
            m.values[k].abs(),
            table,
            style.tol_style,
            style.tol_decimals,
        )?;
    }
    Ok(())
}

/// Whether the lines of a dimension of lengths run across the sheet, and the stretch of the sheet
/// they cover along the line, for the kinds drawn between faces of a view of a solid.
fn extent_of(kind: DimKind, m: &Measured, frame: &Frame) -> Option<(bool, i64, i64)> {
    if !matches!(
        kind,
        DimKind::Linear | DimKind::Cumulative | DimKind::Chain | DimKind::Ordinate
    ) {
        return None;
    }
    let (u, v) = view_axes(frame.kind)?;
    let super::measure::Resolved::Face { axis, .. } = m.refs.first()? else {
        return None;
    };
    let horizontal = *axis == u;
    if !horizontal && *axis != v {
        return None;
    }
    let along: Vec<i64> = m
        .refs
        .iter()
        .map(|r| face_place(r, frame, horizontal, u, v).0)
        .collect();
    Some((horizontal, *along.iter().min()?, *along.iter().max()?))
}
