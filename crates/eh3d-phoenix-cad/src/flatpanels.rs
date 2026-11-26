//! The six faces of the box as flat templates, each seen from outside with the openings of its
//! holes, packed in rows on a sheet and written as DXF or SVG for marking, drilling or engraving
//! by the laser and the milling programs. They are templates of the faces at their outer size:
//! no finger joints, no kerf and no bend allowance, and the extrusions and pockets are not drawn.

use crate::enclosure::{Enclosure, Face, Hole, HoleStyle};
use crate::scad::mm_text;
use crate::{CadError, Length};

/// One face as a flat template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Panel {
    /// The face.
    pub face: Face,
    /// The size along `u`, in nanometres.
    pub width: i64,
    /// The size along `v`, in nanometres.
    pub height: i64,
    /// The holes of the face.
    pub holes: Vec<Hole>,
}

/// A panel on the sheet: where its lower left corner is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// The panel.
    pub panel: Panel,
    /// Along the width of the sheet.
    pub x: i64,
    /// Along its height.
    pub y: i64,
}

/// The panels on a sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The width of the sheet that was given.
    pub width: i64,
    /// The height that the rows need.
    pub height: i64,
    /// The panels, in the order they were placed.
    pub placed: Vec<Placed>,
}

/// The six faces of a box with their holes (the ones of the lid included), in the order of
/// [`Face::ALL`].
pub fn panels(enclosure: &Enclosure) -> Result<Vec<Panel>, CadError> {
    let lid = enclosure.derived_lid_holes()?;
    Ok(Face::ALL
        .into_iter()
        .map(|face| {
            let (width, height) = enclosure.face_size(face);
            let mut holes: Vec<Hole> = enclosure
                .holes()
                .values()
                .filter(|h| h.face == face)
                .cloned()
                .collect();
            holes.extend(lid.iter().filter(|h| h.face == face).cloned());
            Panel {
                face,
                width: width.get(),
                height: height.get(),
                holes,
            }
        })
        .collect())
}

/// Packs the panels in rows, the tallest first, with `gap` between them and round the edge of the
/// sheet. A panel wider than the sheet is refused with `PHOENIX-CAD-069`.
pub fn pack(mut list: Vec<Panel>, width: Length, gap: Length) -> Result<Layout, CadError> {
    let (width, gap) = (width.get(), gap.get());
    if width <= 0 || gap < 0 {
        return Err(CadError::FlatLayout(
            "the sheet has a width and the gap is not negative".to_owned(),
        ));
    }
    // a stable order: the taller first, then the order of the faces
    list.sort_by(|a, b| b.height.cmp(&a.height).then(a.face.cmp(&b.face)));
    let (mut x, mut y, mut row) = (gap, gap, 0i64);
    let mut placed = Vec::with_capacity(list.len());
    for panel in list {
        if panel.width + 2 * gap > width {
            return Err(CadError::FlatLayout(format!(
                "the face {} is {} mm wide and does not fit the sheet",
                panel.face.name(),
                mm_text(panel.width)
            )));
        }
        if x + panel.width + gap > width {
            x = gap;
            y += row + gap;
            row = 0;
        }
        let at = x;
        x += panel.width + gap;
        row = row.max(panel.height);
        placed.push(Placed { panel, x: at, y });
    }
    Ok(Layout {
        width,
        height: y + row + gap,
        placed,
    })
}

/// A shape of the drawing of a template.
enum Shape {
    Line(i64, i64, i64, i64),
    Circle(i64, i64, i64),
    /// Centre, radius and the angles in degrees, counter-clockwise.
    Arc(i64, i64, i64, i32, i32),
}

fn rectangle(x: i64, y: i64, w: i64, h: i64, out: &mut Vec<Shape>) {
    out.push(Shape::Line(x, y, x + w, y));
    out.push(Shape::Line(x + w, y, x + w, y + h));
    out.push(Shape::Line(x + w, y + h, x, y + h));
    out.push(Shape::Line(x, y + h, x, y));
}

fn hole_shapes(x: i64, y: i64, hole: &Hole, out: &mut Vec<Shape>) {
    let (cx, cy) = (x + hole.u.get(), y + hole.v.get());
    let across = hole.diameter.get();
    match hole.style {
        HoleStyle::Plain | HoleStyle::Countersunk { .. } | HoleStyle::Counterbored { .. } => {
            out.push(Shape::Circle(cx, cy, across / 2));
        }
        HoleStyle::Rectangular { height } => {
            let h = height.get();
            rectangle(cx - across / 2, cy - h / 2, across, h, out);
        }
        HoleStyle::Oval { length, vertical } => {
            let r = across / 2;
            let half = (length.get() - across) / 2;
            if vertical {
                out.push(Shape::Line(cx - r, cy - half, cx - r, cy + half));
                out.push(Shape::Line(cx + r, cy - half, cx + r, cy + half));
                out.push(Shape::Arc(cx, cy + half, r, 0, 180));
                out.push(Shape::Arc(cx, cy - half, r, 180, 360));
            } else {
                out.push(Shape::Line(cx - half, cy - r, cx + half, cy - r));
                out.push(Shape::Line(cx - half, cy + r, cx + half, cy + r));
                out.push(Shape::Arc(cx + half, cy, r, 270, 90));
                out.push(Shape::Arc(cx - half, cy, r, 90, 270));
            }
        }
    }
}

fn shapes(placed: &Placed) -> Vec<Shape> {
    let mut out = Vec::new();
    rectangle(
        placed.x,
        placed.y,
        placed.panel.width,
        placed.panel.height,
        &mut out,
    );
    for hole in &placed.panel.holes {
        hole_shapes(placed.x, placed.y, hole, &mut out);
    }
    out
}

/// The layout as a text DXF drawing (R12) in millimetres: one layer per face, the name of the
/// face being the name of the layer.
pub fn to_dxf(layout: &Layout) -> String {
    let mut out = String::from("0\nSECTION\n2\nENTITIES\n");
    for placed in &layout.placed {
        let layer = placed.panel.face.name();
        for shape in shapes(placed) {
            match shape {
                Shape::Line(x0, y0, x1, y1) => out.push_str(&format!(
                    "0\nLINE\n8\n{layer}\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n",
                    mm_text(x0),
                    mm_text(y0),
                    mm_text(x1),
                    mm_text(y1)
                )),
                Shape::Circle(x, y, r) => out.push_str(&format!(
                    "0\nCIRCLE\n8\n{layer}\n10\n{}\n20\n{}\n40\n{}\n",
                    mm_text(x),
                    mm_text(y),
                    mm_text(r)
                )),
                Shape::Arc(x, y, r, from, to) => out.push_str(&format!(
                    "0\nARC\n8\n{layer}\n10\n{}\n20\n{}\n40\n{}\n50\n{from}\n51\n{to}\n",
                    mm_text(x),
                    mm_text(y),
                    mm_text(r)
                )),
            }
        }
    }
    out.push_str("0\nENDSEC\n0\nEOF\n");
    out
}

/// The layout as an SVG drawing in millimetres, one group per face with its name as its id.
pub fn to_svg(layout: &Layout) -> String {
    let top = layout.height;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" \
         viewBox=\"0 0 {w} {h}\" fill=\"none\" stroke=\"black\" stroke-width=\"0.1\">\n",
        w = mm_text(layout.width),
        h = mm_text(layout.height)
    );
    for placed in &layout.placed {
        out.push_str(&format!("<g id=\"{}\">\n", placed.panel.face.name()));
        for shape in shapes(placed) {
            match shape {
                Shape::Line(x0, y0, x1, y1) => out.push_str(&format!(
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n",
                    mm_text(x0),
                    mm_text(top - y0),
                    mm_text(x1),
                    mm_text(top - y1)
                )),
                Shape::Circle(x, y, r) => out.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>\n",
                    mm_text(x),
                    mm_text(top - y),
                    mm_text(r)
                )),
                Shape::Arc(x, y, r, from, to) => {
                    // the arcs of a slot are half circles, so their ends are on the axes
                    let end = |degrees: i32| match degrees.rem_euclid(360) {
                        0 => (x + r, y),
                        90 => (x, y + r),
                        180 => (x - r, y),
                        _ => (x, y - r),
                    };
                    let (a, b) = (end(from), end(to));
                    out.push_str(&format!(
                        "<path d=\"M {} {} A {r} {r} 0 0 0 {} {}\"/>\n",
                        mm_text(a.0),
                        mm_text(top - a.1),
                        mm_text(b.0),
                        mm_text(top - b.1),
                        r = mm_text(r)
                    ));
                }
            }
        }
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eh3dfile::tests::Seeded;
    use crate::enclosure::{EnclosureParams, Operation};

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn enclosure(length: i64, width: i64, height: i64) -> Enclosure {
        Enclosure::new(EnclosureParams {
            inner_length: mm(length),
            inner_width: mm(width),
            inner_height: mm(height),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap()
    }

    fn with_holes() -> Enclosure {
        let mut e = enclosure(100, 60, 30);
        let styles = [
            HoleStyle::Plain,
            HoleStyle::Rectangular { height: mm(5) },
            HoleStyle::Oval {
                length: mm(12),
                vertical: false,
            },
            HoleStyle::Oval {
                length: mm(12),
                vertical: true,
            },
        ];
        for (i, style) in styles.into_iter().enumerate() {
            let hole = Hole {
                name: format!("h{i}"),
                face: Face::Front,
                u: mm(15 + 25 * i as i64),
                v: mm(17),
                diameter: mm(6),
                style,
            };
            e.apply(Operation::AddHole {
                id: crate::boardart::id_from(&hole.name).unwrap(),
                hole,
            })
            .unwrap();
        }
        e
    }

    #[test]
    fn the_faces_have_their_outer_sizes() {
        let list = panels(&enclosure(100, 60, 30)).unwrap();
        let sizes: Vec<_> = list.iter().map(|p| (p.width, p.height)).collect();
        let n = |v: i64| v * 1_000_000;
        assert_eq!(
            sizes,
            vec![
                (n(104), n(35)),
                (n(104), n(35)),
                (n(64), n(35)),
                (n(64), n(35)),
                (n(104), n(64)),
                (n(104), n(64)),
            ]
        );
    }

    #[test]
    fn panels_of_random_boxes_never_overlap_and_stay_on_the_sheet() {
        let mut random = Seeded(0x00CA_DB05);
        for _ in 0..60 {
            let e = enclosure(
                10 + random.below(200) as i64,
                10 + random.below(200) as i64,
                10 + random.below(80) as i64,
            );
            let sheet = mm(220 + random.below(600) as i64);
            let gap = mm(random.below(6) as i64);
            let layout = pack(panels(&e).unwrap(), sheet, gap).unwrap();
            assert_eq!(layout.placed.len(), 6);
            for (i, a) in layout.placed.iter().enumerate() {
                assert!(a.x >= gap.get() && a.x + a.panel.width + gap.get() <= layout.width);
                assert!(a.y >= gap.get() && a.y + a.panel.height + gap.get() <= layout.height);
                for b in layout.placed.iter().skip(i + 1) {
                    let apart = a.x + a.panel.width + gap.get() <= b.x
                        || b.x + b.panel.width + gap.get() <= a.x
                        || a.y + a.panel.height + gap.get() <= b.y
                        || b.y + b.panel.height + gap.get() <= a.y;
                    assert!(apart, "{a:?} {b:?}");
                }
            }
        }
    }

    #[test]
    fn a_sheet_too_narrow_is_refused_with_its_code() {
        let list = panels(&enclosure(100, 60, 30)).unwrap();
        let error = pack(list, mm(100), mm(2)).unwrap_err();
        assert_eq!(error.code(), "PHOENIX-CAD-069");
        let list = panels(&enclosure(100, 60, 30)).unwrap();
        assert!(pack(list, mm(500), Length::new(-1).unwrap()).is_err());
    }

    #[test]
    fn the_drawings_have_every_opening_and_are_the_same_each_time() {
        let layout = pack(panels(&with_holes()).unwrap(), mm(400), mm(5)).unwrap();
        let dxf = to_dxf(&layout);
        let count = |what: &str| dxf.matches(&format!("0\n{what}\n8\n")).count();
        // six rectangles of four lines, one rectangular opening and two slots of two lines
        assert_eq!(count("LINE"), 24 + 4 + 4);
        assert_eq!(count("CIRCLE"), 1);
        assert_eq!(count("ARC"), 4);
        assert!(dxf.ends_with("0\nENDSEC\n0\nEOF\n"));
        let svg = to_svg(&layout);
        assert_eq!(svg.matches("<line ").count(), 32);
        assert_eq!(svg.matches("<circle ").count(), 1);
        assert_eq!(svg.matches("<path ").count(), 4);
        assert_eq!(svg.matches("<g id=").count(), 6);
        assert_eq!(svg, to_svg(&layout));
        assert_eq!(dxf, to_dxf(&layout));
    }
}
