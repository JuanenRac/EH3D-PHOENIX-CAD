// SPDX-License-Identifier: GPL-3.0-or-later
//! From the model and the drawing to the list of primitives of the sheet: the frame, the views
//! with their visible and hidden edges, the dimensions, the balloons, the table of the bill and the
//! title block, with the problems found on the way. Nothing here reads the clock or the system:
//! the same model and the same drawing give the same list.

use std::collections::BTreeMap;

use super::bom::{part_nodes, BomLine};
use super::geom::{bounds_of, view_segments};
use super::measure::{draw, measure, Frame, Issue, Measured, Model};
use super::model::{DimKind, Place, Plan, SheetScale, Source, View, ViewKind};
use super::prim::{mm_text, Anchor, Prim, Shape};
use super::settings::SheetSettings;
use crate::config::Settings;
use crate::enclosure::Id;
use crate::lang::{Lang, Msg};
use crate::sketch::{Element, ElementId, Sketch};
use crate::CadError;

const BAD_VIEW: &str = "PHOENIX-CAD-054";
const BAD_DIM: &str = "PHOENIX-CAD-056";
const BAD_BALLOON: &str = "PHOENIX-CAD-057";

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
    /// The texts of its values, empty when it has an issue.
    pub texts: Vec<String>,
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
    /// The bill, when the project has a history.
    pub bom: Vec<BomLine>,
    /// The problems, in the order of the records.
    pub issues: Vec<Issue>,
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
        })
    }
}

fn build_view(v: &View, model: &Model, s: &SheetSettings) -> Result<Built, String> {
    if let Source::Sketch(name) = &v.source {
        let sk = model
            .sketches
            .iter()
            .find(|k| k.name() == name)
            .ok_or_else(|| format!("the sketch {name} is not in the project"))?;
        return sketch_items(sk).ok_or_else(|| format!("the sketch {name} has nothing to draw"));
    }
    let dag = model.dag.ok_or("the project has no history of features")?;
    let solid = match &v.source {
        Source::Tip => dag.result().ok_or("the history has no result")?,
        Source::Node(id) => dag
            .solid(*id)
            .ok_or_else(|| format!("the node {id} has no solid in the last rebuild"))?,
        Source::Sketch(_) => return Err("not a sketch".to_string()),
    };
    if solid.is_empty() {
        return Err("the solid is empty".to_string());
    }
    let segs = view_segments(
        solid,
        v.kind,
        v.hidden,
        s.max_cells,
        s.max_edges,
        s.max_iso_tests,
    )
    .map_err(|e| e.to_string())?;
    let (lo, hi) = bounds_of(&segs).ok_or("nothing to draw in this view")?;
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
    })
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
        .find(|(_, v, _)| v.kind == ViewKind::Front)
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
        if base.1.kind == ViewKind::Front {
            if let Some(t) = auto
                .iter()
                .find(|(i, v, _)| !taken.contains(i) && v.kind == ViewKind::Top)
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
            if let Some(r) = auto
                .iter()
                .find(|(i, v, _)| !taken.contains(i) && v.kind == ViewKind::Right)
            {
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
                if let Some(i) = auto
                    .iter()
                    .find(|(i, v, _)| !taken.contains(i) && v.kind == ViewKind::Iso)
                {
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
        match build_view(v, model, s) {
            Ok(b) => built.push((i, v, b)),
            Err(why) => issues.push(Issue {
                code: BAD_VIEW,
                subject: format!("view {}", v.name),
                detail: why,
            }),
        }
    }
    let refs: Vec<(usize, &View, &Built)> = built.iter().map(|(i, v, b)| (*i, *v, b)).collect();
    // the room
    let margin = s.margin_um;
    let rows = if s.title_block && !s.title_fields.is_empty() {
        (s.title_fields.len() as i64 + i64::from(s.title_columns) - 1) / i64::from(s.title_columns)
    } else {
        0
    };
    let title_h = rows * s.title_row_um;
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
            sh - margin - s.view_gap_um / 2,
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
        };
        let (x1, y1) = (p.origin.0 + p.size.0, p.origin.1 + p.size.1);
        if p.origin.0 < margin || p.origin.1 < margin || x1 > sw - margin || y1 > sh - margin {
            issues.push(Issue {
                code: BAD_VIEW,
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
                layer: (*layer).to_string(),
                shape: map_shape(shape, &frame),
            });
        }
        frames.insert(v.id, frame);
        view_reports.push(ViewReport {
            id: v.id,
            name: v.name.clone(),
            kind: v.kind,
            scale: p.scale,
            origin: p.origin,
            size: p.size,
            visible: b.visible,
            hidden: b.hidden,
        });
    }
    // the dimensions
    let mut dim_reports = Vec::new();
    for d in &plan.dims {
        let subject = format!("dimension {}", d.name);
        // what the dimension is tied to is looked for first: a thing that is gone is an orphan,
        // whatever became of the view
        let measured: Result<Measured, _> = measure(d, model, &s.dim);
        let sketch_name = plan.view(d.view).and_then(|v| match &v.source {
            Source::Sketch(n) => Some(n.as_str()),
            _ => None,
        });
        let drawn = measured.and_then(|m| match frames.get(&d.view) {
            Some(frame) => draw(d, &m, frame, &s.dim, sketch_name).map(|p| (m, p)),
            None => Err((BAD_DIM, "its view could not be made".to_string())),
        });
        match drawn {
            Ok((m, ps)) => {
                prims.extend(ps);
                dim_reports.push(DimReport {
                    id: d.id,
                    name: d.name.clone(),
                    kind: d.kind,
                    texts: m.texts,
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
    let right = sw - margin;
    let mut y = margin;
    if s.title_block && rows > 0 {
        let w = s.title_width_um.min(sw - 2 * margin);
        let x0 = right - w;
        let cols = i64::from(s.title_columns);
        let cw = w / cols;
        prims.extend(rect("FRAME", x0, y, x0 + cw * cols, y + title_h));
        for (k, key) in s.title_fields.iter().enumerate() {
            let (col, row) = (k as i64 % cols, k as i64 / cols);
            let (cx, cy) = (x0 + col * cw, y + title_h - (row + 1) * s.title_row_um);
            prims.extend(rect("FRAME", cx, cy, cx + cw, cy + s.title_row_um));
            let value = match key.as_str() {
                "scale" => format!("{}:{}", scale.0, scale.1),
                "sheet" => "1/1".to_string(),
                other => plan
                    .fields
                    .get(other)
                    .or_else(|| s.title_values.get(other))
                    .cloned()
                    .unwrap_or_default(),
            };
            let label_h = s.title_row_um * 22 / 100;
            let value_h = s.title_row_um * 40 / 100;
            prims.push(Prim::text(
                "TEXT",
                (cx + label_h / 2, cy + s.title_row_um - label_h * 12 / 10),
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
        let widths = [
            w / 10,
            w / 10,
            w * 55 / 100,
            w - w / 10 - w / 10 - w * 55 / 100,
        ];
        let cols = [
            x0,
            x0 + widths[0],
            x0 + widths[0] + widths[1],
            x0 + widths[0] + widths[1] + widths[2],
            right,
        ];
        let heads = [Msg::PlItem, Msg::PlQty, Msg::PlDescription, Msg::TbMaterial];
        let rh = s.bom_row_um;
        let th = rh * 55 / 100;
        for (r, row) in std::iter::once(None)
            .chain(bom.iter().map(Some))
            .enumerate()
        {
            let ry = y + r as i64 * rh;
            prims.extend(rect("FRAME", x0, ry, right, ry + rh));
            for x in &cols[1..4] {
                prims.push(Prim::line("FRAME", (*x, ry), (*x, ry + rh)));
            }
            let texts: [String; 4] = match row {
                None => heads.map(|m| m.tr(lang).to_string()),
                Some(l) => [
                    l.number.to_string(),
                    l.quantity.to_string(),
                    l.description.clone(),
                    l.material.clone(),
                ],
            };
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
        },
        scale,
        views: view_reports,
        dims: dim_reports,
        balloons: balloon_reports,
        bom,
        issues,
    })
}
