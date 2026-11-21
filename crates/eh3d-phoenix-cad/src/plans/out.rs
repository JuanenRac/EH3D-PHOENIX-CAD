// SPDX-License-Identifier: GPL-3.0-or-later
//! The files of a sheet: SVG, PDF (the writer of the drawings of the box, with no dependency) and
//! a simple DXF of lines, circles, arcs and texts on layers. Each is text or bytes that are the same
//! for the same scene: no date, no host, no version. None of them has been opened in a drawing
//! program.

use std::fmt::Write as _;

use super::prim::{mm_text, Anchor, Prim, Shape};
use super::scene::Scene;
use super::settings::LineType;
use crate::pdf::{Anchor as PdfAnchor, Canvas, POINTS_PER_MM};
use crate::titleblock::{dxf_text, xml_text};

/// A length in micrometres as millimetres with three decimals (`12.500`, `-0.350`).
fn mm3(um: i64) -> String {
    let sign = if um < 0 { "-" } else { "" };
    let a = um.unsigned_abs();
    format!("{sign}{}.{:03}", a / 1000, a % 1000)
}

fn angle6(a: f64) -> String {
    let t = format!("{a:.6}");
    if t.starts_with('-') && t[1..].bytes().all(|b| b == b'0' || b == b'.') {
        t[1..].to_owned()
    } else {
        t
    }
}

fn dash_of(t: LineType, width_um: i64) -> Option<(f64, f64, f64, f64)> {
    // dash, gap, and for dash-dot the dot and the second gap, in millimetres
    let w = (width_um as f64 / 1000.0).max(0.25);
    match t {
        LineType::Continuous => None,
        LineType::Dashed => Some((w * 8.0, w * 4.0, 0.0, 0.0)),
        LineType::DashDot => Some((w * 16.0, w * 3.0, w, w * 3.0)),
    }
}

fn flip(h: i64, p: (i64, i64)) -> (i64, i64) {
    (p.0, h - p.1)
}

/// The sheet as an SVG file in millimetres.
pub fn to_svg(scene: &Scene, title: &str) -> String {
    let (w, h) = (scene.width_um, scene.height_um);
    let mut o = String::new();
    let _ = writeln!(o, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    let _ = writeln!(
        o,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\">",
        mm_text(w),
        mm_text(h),
        mm_text(w),
        mm_text(h)
    );
    let _ = writeln!(o, "<title>{}</title>", xml_text(title));
    let _ = writeln!(
        o,
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>",
        mm_text(w),
        mm_text(h)
    );
    for (name, (color, width, line)) in &scene.layers {
        let _ = write!(
            o,
            "<g id=\"layer-{}\" fill=\"none\" stroke=\"#{}\" stroke-width=\"{}\" stroke-linecap=\"round\"",
            xml_text(name),
            color,
            mm_text(*width)
        );
        if let Some((a, b, c, d)) = dash_of(*line, *width) {
            let fmt = |v: f64| format!("{v:.2}");
            if c == 0.0 {
                let _ = write!(o, " stroke-dasharray=\"{} {}\"", fmt(a), fmt(b));
            } else {
                let _ = write!(
                    o,
                    " stroke-dasharray=\"{} {} {} {}\"",
                    fmt(a),
                    fmt(b),
                    fmt(c),
                    fmt(d)
                );
            }
        }
        let _ = writeln!(o, ">");
        for p in scene.prims.iter().filter(|p| &p.layer == name) {
            match &p.shape {
                Shape::Line { a, b } => {
                    let (a, b) = (flip(h, *a), flip(h, *b));
                    let _ = writeln!(
                        o,
                        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>",
                        mm_text(a.0),
                        mm_text(a.1),
                        mm_text(b.0),
                        mm_text(b.1)
                    );
                }
                Shape::Circle { c, r } => {
                    let c = flip(h, *c);
                    let _ = writeln!(
                        o,
                        "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
                        mm_text(c.0),
                        mm_text(c.1),
                        mm_text(*r)
                    );
                }
                Shape::Arc { c, r, a0, a1 } => {
                    let span = a1 - a0;
                    let at = |a: f64| {
                        let rad = a.to_radians();
                        (
                            (c.0 as f64 + *r as f64 * rad.cos()).round() as i64,
                            h - (c.1 as f64 + *r as f64 * rad.sin()).round() as i64,
                        )
                    };
                    if span.abs() >= 360.0 {
                        let cf = flip(h, *c);
                        let _ = writeln!(
                            o,
                            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
                            mm_text(cf.0),
                            mm_text(cf.1),
                            mm_text(*r)
                        );
                    } else {
                        let (s, e) = (at(*a0), at(*a1));
                        let _ = writeln!(
                            o,
                            "<path d=\"M {} {} A {} {} 0 {} 0 {} {}\"/>",
                            mm_text(s.0),
                            mm_text(s.1),
                            mm_text(*r),
                            mm_text(*r),
                            u8::from(span.abs() > 180.0),
                            mm_text(e.0),
                            mm_text(e.1)
                        );
                    }
                }
                Shape::Text { .. } => {}
            }
        }
        let _ = writeln!(o, "</g>");
    }
    // the texts: filled, no stroke
    for (name, (color, _, _)) in &scene.layers {
        let texts: Vec<&Prim> = scene
            .prims
            .iter()
            .filter(|p| &p.layer == name && matches!(p.shape, Shape::Text { .. }))
            .collect();
        if texts.is_empty() {
            continue;
        }
        let _ = writeln!(
            o,
            "<g id=\"text-{}\" fill=\"#{}\" stroke=\"none\" font-family=\"Helvetica, Arial, sans-serif\">",
            xml_text(name),
            color
        );
        for p in texts {
            if let Shape::Text {
                at,
                h: th,
                text,
                anchor,
                turn,
            } = &p.shape
            {
                let a = flip(h, *at);
                let _ = write!(
                    o,
                    "<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"{}\"",
                    mm_text(a.0),
                    mm_text(a.1),
                    mm_text(*th),
                    if *anchor == Anchor::Middle {
                        "middle"
                    } else {
                        "start"
                    }
                );
                if *turn != 0.0 {
                    let _ = write!(
                        o,
                        " transform=\"rotate({} {} {})\"",
                        angle6(-*turn),
                        mm_text(a.0),
                        mm_text(a.1)
                    );
                }
                let _ = writeln!(o, ">{}</text>", xml_text(text));
            }
        }
        let _ = writeln!(o, "</g>");
    }
    let _ = writeln!(o, "</svg>");
    o
}

fn hex_rgb(hex: &str) -> (i32, i32, i32) {
    let v = |i: usize| i32::from_str_radix(hex.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    (v(0), v(2), v(4))
}

/// The colour of the DXF palette nearest to a colour of the settings.
fn aci(hex: &str) -> u32 {
    const PALETTE: [(u32, (i32, i32, i32)); 9] = [
        (1, (255, 0, 0)),
        (2, (255, 255, 0)),
        (3, (0, 255, 0)),
        (4, (0, 255, 255)),
        (5, (0, 0, 255)),
        (6, (255, 0, 255)),
        (7, (0, 0, 0)),
        (8, (128, 128, 128)),
        (9, (192, 192, 192)),
    ];
    let c = hex_rgb(hex);
    PALETTE
        .iter()
        .min_by_key(|(_, p)| (p.0 - c.0).pow(2) + (p.1 - c.1).pow(2) + (p.2 - c.2).pow(2))
        .map_or(7, |(n, _)| *n)
}

/// The sheet as a DXF file (R12 text) in millimetres.
pub fn to_dxf(scene: &Scene) -> String {
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
    put(70, "3");
    for (name, desc, items) in [
        ("CONTINUOUS", "Solid line", Vec::new()),
        ("DASHED", "Dashed line", vec!["3.0", "-1.5"]),
        (
            "DASHDOT",
            "Dash dot line",
            vec!["6.0", "-1.5", "0.0", "-1.5"],
        ),
    ] {
        put(0, "LTYPE");
        put(2, name);
        put(70, "0");
        put(3, desc);
        put(72, "65");
        put(73, &items.len().to_string());
        put(
            40,
            if items.is_empty() {
                "0.0"
            } else if items.len() == 2 {
                "4.5"
            } else {
                "9.0"
            },
        );
        for i in items {
            put(49, i);
        }
    }
    put(0, "ENDTAB");
    put(0, "TABLE");
    put(2, "LAYER");
    put(70, &scene.layers.len().to_string());
    for (name, (color, _, line)) in &scene.layers {
        put(0, "LAYER");
        put(2, name);
        put(70, "0");
        put(62, &aci(color).to_string());
        put(
            6,
            match line {
                LineType::Continuous => "CONTINUOUS",
                LineType::Dashed => "DASHED",
                LineType::DashDot => "DASHDOT",
            },
        );
    }
    put(0, "ENDTAB");
    put(0, "ENDSEC");
    put(0, "SECTION");
    put(2, "ENTITIES");
    for p in &scene.prims {
        match &p.shape {
            Shape::Line { a, b } => {
                put(0, "LINE");
                put(8, &p.layer);
                put(10, &mm3(a.0));
                put(20, &mm3(a.1));
                put(30, "0.0");
                put(11, &mm3(b.0));
                put(21, &mm3(b.1));
                put(31, "0.0");
            }
            Shape::Circle { c, r } => {
                put(0, "CIRCLE");
                put(8, &p.layer);
                put(10, &mm3(c.0));
                put(20, &mm3(c.1));
                put(30, "0.0");
                put(40, &mm3(*r));
            }
            Shape::Arc { c, r, a0, a1 } => {
                if (a1 - a0).abs() >= 360.0 {
                    put(0, "CIRCLE");
                    put(8, &p.layer);
                    put(10, &mm3(c.0));
                    put(20, &mm3(c.1));
                    put(30, "0.0");
                    put(40, &mm3(*r));
                } else {
                    put(0, "ARC");
                    put(8, &p.layer);
                    put(10, &mm3(c.0));
                    put(20, &mm3(c.1));
                    put(30, "0.0");
                    put(40, &mm3(*r));
                    put(50, &angle6(a0.rem_euclid(360.0)));
                    put(51, &angle6(a1.rem_euclid(360.0)));
                }
            }
            Shape::Text {
                at,
                h,
                text,
                anchor,
                turn,
            } => {
                put(0, "TEXT");
                put(8, &p.layer);
                put(10, &mm3(at.0));
                put(20, &mm3(at.1));
                put(30, "0.0");
                put(40, &mm3(*h));
                put(1, &dxf_text(text));
                put(50, &angle6(turn.rem_euclid(360.0)));
                if *anchor == Anchor::Middle {
                    put(72, "1");
                    put(11, &mm3(at.0));
                    put(21, &mm3(at.1));
                    put(31, "0.0");
                }
            }
        }
    }
    put(0, "ENDSEC");
    put(0, "EOF");
    o
}

fn hex_unit(hex: &str) -> String {
    format!("#{hex}")
}

/// The sheet as a PDF page.
pub fn to_pdf(scene: &Scene, title: &str) -> Vec<u8> {
    let (w, h) = (
        scene.width_um as f64 / 1000.0,
        scene.height_um as f64 / 1000.0,
    );
    let mut c = Canvas::new(w, h, POINTS_PER_MM);
    let page = |p: (i64, i64)| (p.0 as f64 / 1000.0, h - p.1 as f64 / 1000.0);
    for (name, (color, width, line)) in &scene.layers {
        c.stroke(&hex_unit(color));
        c.fill(&hex_unit(color));
        c.width(*width as f64 / 1000.0);
        c.dash(dash_of(*line, *width).map(|(a, b, _, _)| (a, b)));
        for p in scene.prims.iter().filter(|p| &p.layer == name) {
            match &p.shape {
                Shape::Line { a, b } => c.line(page(*a), page(*b)),
                Shape::Circle { c: ce, r } => c.circle(page(*ce), *r as f64 / 1000.0),
                Shape::Arc { c: ce, r, a0, a1 } => c.arc(page(*ce), *r as f64 / 1000.0, *a0, *a1),
                Shape::Text {
                    at,
                    h: th,
                    text,
                    anchor,
                    turn,
                } => {
                    c.dash(None);
                    c.text(
                        page(*at),
                        *th as f64 / 1000.0,
                        text,
                        if *anchor == Anchor::Middle {
                            PdfAnchor::Middle
                        } else {
                            PdfAnchor::Start
                        },
                        *turn,
                    );
                    c.dash(dash_of(*line, *width).map(|(a, b, _, _)| (a, b)));
                }
            }
        }
    }
    c.dash(None);
    let ascii: String = title
        .chars()
        .map(|ch| {
            if ch.is_ascii_graphic() || ch == ' ' {
                ch
            } else {
                '?'
            }
        })
        .collect();
    c.finish(&ascii)
}
