// SPDX-License-Identifier: GPL-3.0-or-later
//! Reads an SVG drawing as a flat drawing of lines and circles, in millimetres, as a text DXF file
//! that the DXF reader of the program turns into a sketch (`sketch-import` takes the file as it
//! takes a DXF one).
//!
//! The reader takes `line`, `polyline`, `polygon`, `rect` (also with rounded corners), `circle`,
//! `ellipse` and `path` (all the commands: `M L H V C S Q T A Z`, absolute and relative, with the
//! numbers written as compactly as the format allows), the `transform` of every element and of the
//! groups it is in, and the size of the page (`width`, `height` and `viewBox`, in `mm`, `cm`, `in`,
//! `pt`, `pc`, `px` or none, a user unit being a pixel of 96 to the inch when the page says
//! nothing else). Curves and arcs are cut into straight lines that sag from them by no more than
//! the tolerance it is given (the setting `sketch.profile_chord`), a circle that stays a circle
//! under the transformation is a circle, and the second axis points up as in a drawing, not down as
//! on a screen. Everything it does not draw (text, images, clones, shapes in definitions) is
//! counted and told by name. It never panics on a damaged file and refuses a drawing with more
//! lines than it is given room for.

use std::collections::BTreeMap;

use crate::threemf::{attr, tokens, Token};

/// The most straight lines for one curve or arc.
const MAX_CHORDS: usize = 1024;

/// The farthest a place may be from the origin, in millimetres (a thousand kilometres).
const FAR_MM: f64 = 1.0e6;

/// A transformation of the plane: `[a, b, c, d, e, f]` as in `matrix(a b c d e f)`.
type Matrix = [f64; 6];

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `outer` applied after `inner`.
fn compose(outer: &Matrix, inner: &Matrix) -> Matrix {
    [
        outer[0] * inner[0] + outer[2] * inner[1],
        outer[1] * inner[0] + outer[3] * inner[1],
        outer[0] * inner[2] + outer[2] * inner[3],
        outer[1] * inner[2] + outer[3] * inner[3],
        outer[0] * inner[4] + outer[2] * inner[5] + outer[4],
        outer[1] * inner[4] + outer[3] * inner[5] + outer[5],
    ]
}

fn apply(m: &Matrix, p: [f64; 2]) -> [f64; 2] {
    [
        m[0] * p[0] + m[2] * p[1] + m[4],
        m[1] * p[0] + m[3] * p[1] + m[5],
    ]
}

/// The numbers inside the parentheses of one transformation function.
fn numbers(text: &str) -> Result<Vec<f64>, String> {
    text.split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|w| !w.is_empty())
        .map(|w| {
            w.parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("{w} is not a number of a transformation"))
        })
        .collect()
}

/// The matrix of a `transform` attribute (the functions are applied left to right as nested).
fn parse_transform(text: &str) -> Result<Matrix, String> {
    let mut total = IDENTITY;
    let mut rest = text.trim();
    while !rest.is_empty() {
        let open = rest
            .find('(')
            .ok_or_else(|| format!("{text}: a transformation function has no parenthesis"))?;
        let close = rest[open..]
            .find(')')
            .ok_or_else(|| format!("{text}: a parenthesis is not closed"))?
            + open;
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let n = numbers(&rest[open + 1..close])?;
        let m: Matrix = match (name, n.as_slice()) {
            ("matrix", [a, b, c, d, e, f]) => [*a, *b, *c, *d, *e, *f],
            ("translate", [x]) => [1.0, 0.0, 0.0, 1.0, *x, 0.0],
            ("translate", [x, y]) => [1.0, 0.0, 0.0, 1.0, *x, *y],
            ("scale", [s]) => [*s, 0.0, 0.0, *s, 0.0, 0.0],
            ("scale", [x, y]) => [*x, 0.0, 0.0, *y, 0.0, 0.0],
            ("rotate", [a]) | ("rotate", [a, _, _]) => {
                let (s, c) = (a.to_radians().sin(), a.to_radians().cos());
                let turn = [c, s, -s, c, 0.0, 0.0];
                if let [_, cx, cy] = n.as_slice() {
                    compose(
                        &compose(&[1.0, 0.0, 0.0, 1.0, *cx, *cy], &turn),
                        &[1.0, 0.0, 0.0, 1.0, -cx, -cy],
                    )
                } else {
                    turn
                }
            }
            ("skewX", [a]) => [1.0, 0.0, a.to_radians().tan(), 1.0, 0.0, 0.0],
            ("skewY", [a]) => [1.0, a.to_radians().tan(), 0.0, 1.0, 0.0, 0.0],
            _ => {
                return Err(format!(
                    "{text}: the transformation {name} is not understood"
                ))
            }
        };
        if m.iter().any(|v| !v.is_finite()) {
            return Err(format!("{text}: a transformation is not a finite one"));
        }
        total = compose(&total, &m);
        rest = rest[close + 1..].trim_start_matches([',', ' ', '\t', '\r', '\n']);
    }
    Ok(total)
}

/// A length of the page: its value and the millimetres of its unit, when the unit is absolute.
fn length(text: &str) -> Result<(f64, Option<f64>), String> {
    let t = text.trim();
    // the number first (its exponent included), then the unit
    let bytes = t.as_bytes();
    let mut split = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        split += 1;
    }
    while matches!(bytes.get(split), Some(b'0'..=b'9' | b'.')) {
        split += 1;
    }
    if matches!(bytes.get(split), Some(b'e' | b'E')) {
        let mut j = split + 1;
        if matches!(bytes.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        if matches!(bytes.get(j), Some(b'0'..=b'9')) {
            while matches!(bytes.get(j), Some(b'0'..=b'9')) {
                j += 1;
            }
            split = j;
        }
    }
    let value: f64 = t[..split]
        .trim()
        .parse()
        .ok()
        .filter(|v: &f64| v.is_finite())
        .ok_or_else(|| format!("{t}: not a length"))?;
    let unit = match &t[split..] {
        "" | "px" => Some(25.4 / 96.0),
        "mm" => Some(1.0),
        "cm" => Some(10.0),
        "in" => Some(25.4),
        "pt" => Some(25.4 / 72.0),
        "pc" => Some(25.4 / 6.0),
        "%" | "em" | "ex" => None,
        other => return Err(format!("{t}: the unit {other} is not known")),
    };
    Ok((value, unit))
}

/// What was read and what was left out.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SvgReport {
    /// How many shapes were drawn.
    pub shapes: usize,
    /// What was not drawn, by the name of the element, with how many.
    pub skipped: BTreeMap<String, usize>,
}

/// One thing to draw, in millimetres with the second axis up.
enum Drawn {
    Line(Vec<[f64; 2]>, bool),
    Circle([f64; 2], f64),
}

struct Scanner<'a> {
    text: &'a [u8],
    at: usize,
}

impl Scanner<'_> {
    fn skip(&mut self) {
        while self.at < self.text.len()
            && (self.text[self.at].is_ascii_whitespace() || self.text[self.at] == b',')
        {
            self.at += 1;
        }
    }

    fn done(&mut self) -> bool {
        self.skip();
        self.at >= self.text.len()
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.text.get(self.at).copied()
    }

    /// A number as the path data write it: `-.5`, `1e-3`, `1.5.5` (two numbers), `10-5`.
    fn number(&mut self) -> Result<f64, String> {
        self.skip();
        let start = self.at;
        let t = self.text;
        let mut i = self.at;
        if matches!(t.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let mut digits = 0;
        while matches!(t.get(i), Some(b'0'..=b'9')) {
            i += 1;
            digits += 1;
        }
        if t.get(i) == Some(&b'.') {
            i += 1;
            while matches!(t.get(i), Some(b'0'..=b'9')) {
                i += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return Err("a number is expected in the path".to_owned());
        }
        if matches!(t.get(i), Some(b'e' | b'E')) {
            let mut j = i + 1;
            if matches!(t.get(j), Some(b'+' | b'-')) {
                j += 1;
            }
            if matches!(t.get(j), Some(b'0'..=b'9')) {
                while matches!(t.get(j), Some(b'0'..=b'9')) {
                    j += 1;
                }
                i = j;
            }
        }
        self.at = i;
        std::str::from_utf8(&t[start..i])
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .ok_or_else(|| "a number of the path is not valid".to_owned())
    }

    /// A flag of an arc: a single `0` or `1`, which may touch the next number.
    fn flag(&mut self) -> Result<bool, String> {
        self.skip();
        match self.text.get(self.at) {
            Some(b'0') => {
                self.at += 1;
                Ok(false)
            }
            Some(b'1') => {
                self.at += 1;
                Ok(true)
            }
            _ => Err("a flag of an arc is 0 or 1".to_owned()),
        }
    }
}

/// How many straight pieces a curve needs so that none sags more than `tol` (from the largest
/// second difference of its control points and the number of times the curve is derived twice).
fn pieces(second: f64, factor: f64, tol: f64) -> usize {
    let n = (factor * second / (8.0 * tol)).sqrt().ceil();
    if n.is_finite() {
        (n as usize).clamp(1, MAX_CHORDS)
    } else {
        MAX_CHORDS
    }
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// The points of an arc from `p0` to `p1` in the form of the endpoints of the format (the notes of
/// the standard on conversion to the form of the centre), cut so that none sags more than `tol`.
fn arc(
    p0: [f64; 2],
    radii: (f64, f64),
    rotation: f64,
    flags: (bool, bool),
    p1: [f64; 2],
    tol: f64,
) -> Vec<[f64; 2]> {
    let (mut rx, mut ry) = radii;
    let (large, sweep) = flags;
    if p0 == p1 {
        return Vec::new();
    }
    rx = rx.abs();
    ry = ry.abs();
    if rx == 0.0 || ry == 0.0 {
        return vec![p1];
    }
    let phi = rotation.to_radians();
    let (sp, cp) = (phi.sin(), phi.cos());
    let (dx, dy) = ((p0[0] - p1[0]) / 2.0, (p0[1] - p1[1]) / 2.0);
    let x1 = cp * dx + sp * dy;
    let y1 = -sp * dx + cp * dy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut coef = if den == 0.0 {
        0.0
    } else {
        (num / den).max(0.0).sqrt()
    };
    if large == sweep {
        coef = -coef;
    }
    let cx1 = coef * rx * y1 / ry;
    let cy1 = -coef * ry * x1 / rx;
    let cx = cp * cx1 - sp * cy1 + (p0[0] + p1[0]) / 2.0;
    let cy = sp * cx1 + cp * cy1 + (p0[1] + p1[1]) / 2.0;
    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| -> f64 {
        (ux * vy - uy * vx).atan2(ux * vx + uy * vy)
    };
    let theta1 = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut delta = angle(
        (x1 - cx1) / rx,
        (y1 - cy1) / ry,
        (-x1 - cx1) / rx,
        (-y1 - cy1) / ry,
    );
    if !sweep && delta > 0.0 {
        delta -= 2.0 * std::f64::consts::PI;
    } else if sweep && delta < 0.0 {
        delta += 2.0 * std::f64::consts::PI;
    }
    let r = rx.max(ry);
    let step = if tol >= r {
        std::f64::consts::PI
    } else {
        2.0 * (1.0 - tol / r).acos()
    };
    let n = ((delta.abs() / step.max(1e-6)).ceil() as usize).clamp(1, MAX_CHORDS);
    let mut out = Vec::with_capacity(n);
    for k in 1..=n {
        if k == n {
            out.push(p1);
            break;
        }
        let t = theta1 + delta * k as f64 / n as f64;
        let (ex, ey) = (rx * t.cos(), ry * t.sin());
        out.push([cp * ex - sp * ey + cx, sp * ex + cp * ey + cy]);
    }
    out
}

/// A subpath in user units: its points and whether it was closed.
type Subpath = (Vec<[f64; 2]>, bool);

/// The subpaths of path data in user units.
fn path(d: &str, tol: f64) -> Result<Vec<Subpath>, String> {
    let mut s = Scanner {
        text: d.as_bytes(),
        at: 0,
    };
    let mut out: Vec<Subpath> = Vec::new();
    let mut pts: Vec<[f64; 2]> = Vec::new();
    let (mut cur, mut start) = ([0.0f64; 2], [0.0f64; 2]);
    let mut last_cubic: Option<[f64; 2]> = None;
    let mut last_quad: Option<[f64; 2]> = None;
    let mut command = b' ';
    let finish = |pts: &mut Vec<[f64; 2]>, closed: bool, out: &mut Vec<Subpath>| {
        if pts.len() >= 2 {
            out.push((std::mem::take(pts), closed));
        } else {
            pts.clear();
        }
    };
    while !s.done() {
        let c = s.peek().unwrap_or(b' ');
        if c.is_ascii_alphabetic() {
            command = c;
            s.at += 1;
        } else if command == b' ' {
            return Err("the path does not start with a command".to_owned());
        } else if command == b'M' {
            command = b'L';
        } else if command == b'm' {
            command = b'l';
        } else if matches!(command, b'Z' | b'z') {
            return Err("numbers follow a close of the path".to_owned());
        }
        let rel = command.is_ascii_lowercase();
        let base = if rel { cur } else { [0.0, 0.0] };
        let mut new_cubic = None;
        let mut new_quad = None;
        match command.to_ascii_uppercase() {
            b'M' => {
                finish(&mut pts, false, &mut out);
                let p = [base[0] + s.number()?, base[1] + s.number()?];
                cur = p;
                start = p;
                pts.push(p);
            }
            b'L' => {
                let p = [base[0] + s.number()?, base[1] + s.number()?];
                if pts.is_empty() {
                    pts.push(cur);
                }
                pts.push(p);
                cur = p;
            }
            b'H' => {
                let x = s.number()? + if rel { cur[0] } else { 0.0 };
                if pts.is_empty() {
                    pts.push(cur);
                }
                cur = [x, cur[1]];
                pts.push(cur);
            }
            b'V' => {
                let y = s.number()? + if rel { cur[1] } else { 0.0 };
                if pts.is_empty() {
                    pts.push(cur);
                }
                cur = [cur[0], y];
                pts.push(cur);
            }
            b'C' | b'S' => {
                let p1 = if command.eq_ignore_ascii_case(&b'C') {
                    [base[0] + s.number()?, base[1] + s.number()?]
                } else {
                    last_cubic.map_or(cur, |q| [2.0 * cur[0] - q[0], 2.0 * cur[1] - q[1]])
                };
                let p2 = [base[0] + s.number()?, base[1] + s.number()?];
                let p3 = [base[0] + s.number()?, base[1] + s.number()?];
                let p0 = cur;
                if pts.is_empty() {
                    pts.push(p0);
                }
                let second = dist(
                    [p0[0] - 2.0 * p1[0] + p2[0], p0[1] - 2.0 * p1[1] + p2[1]],
                    [0.0; 2],
                )
                .max(dist(
                    [p1[0] - 2.0 * p2[0] + p3[0], p1[1] - 2.0 * p2[1] + p3[1]],
                    [0.0; 2],
                ));
                let n = pieces(second, 6.0, tol);
                for k in 1..=n {
                    let t = k as f64 / n as f64;
                    let u = 1.0 - t;
                    pts.push([
                        u * u * u * p0[0]
                            + 3.0 * u * u * t * p1[0]
                            + 3.0 * u * t * t * p2[0]
                            + t * t * t * p3[0],
                        u * u * u * p0[1]
                            + 3.0 * u * u * t * p1[1]
                            + 3.0 * u * t * t * p2[1]
                            + t * t * t * p3[1],
                    ]);
                }
                cur = p3;
                new_cubic = Some(p2);
            }
            b'Q' | b'T' => {
                let p1 = if command.eq_ignore_ascii_case(&b'Q') {
                    [base[0] + s.number()?, base[1] + s.number()?]
                } else {
                    last_quad.map_or(cur, |q| [2.0 * cur[0] - q[0], 2.0 * cur[1] - q[1]])
                };
                let p2 = [base[0] + s.number()?, base[1] + s.number()?];
                let p0 = cur;
                if pts.is_empty() {
                    pts.push(p0);
                }
                let second = dist(
                    [p0[0] - 2.0 * p1[0] + p2[0], p0[1] - 2.0 * p1[1] + p2[1]],
                    [0.0; 2],
                );
                let n = pieces(second, 2.0, tol);
                for k in 1..=n {
                    let t = k as f64 / n as f64;
                    let u = 1.0 - t;
                    pts.push([
                        u * u * p0[0] + 2.0 * u * t * p1[0] + t * t * p2[0],
                        u * u * p0[1] + 2.0 * u * t * p1[1] + t * t * p2[1],
                    ]);
                }
                cur = p2;
                new_quad = Some(p1);
            }
            b'A' => {
                let (rx, ry, rot) = (s.number()?, s.number()?, s.number()?);
                let (large, sweep) = (s.flag()?, s.flag()?);
                let p = [base[0] + s.number()?, base[1] + s.number()?];
                if pts.is_empty() {
                    pts.push(cur);
                }
                pts.extend(arc(cur, (rx, ry), rot, (large, sweep), p, tol));
                cur = p;
            }
            b'Z' => {
                if !pts.is_empty() {
                    finish(&mut pts, true, &mut out);
                }
                cur = start;
            }
            other => {
                return Err(format!(
                    "the command {} of the path is not known",
                    other as char
                ));
            }
        }
        last_cubic = new_cubic;
        last_quad = new_quad;
        let total: usize = out.iter().map(|(p, _)| p.len()).sum::<usize>() + pts.len();
        if total > 4_000_000 {
            return Err("the path holds more points than are read".to_owned());
        }
    }
    finish(&mut pts, false, &mut out);
    Ok(out)
}

fn number_attr(a: &[(String, String)], key: &str, default: f64) -> Result<f64, String> {
    match attr(a, key) {
        None => Ok(default),
        Some(t) => length(t).map(|(v, _)| v).map_err(|e| format!("{key}: {e}")),
    }
}

/// A rectangle (with rounded corners when `rx` or `ry` is given) as path data.
fn rect_path(a: &[(String, String)]) -> Result<String, String> {
    let (x, y) = (number_attr(a, "x", 0.0)?, number_attr(a, "y", 0.0)?);
    let (w, h) = (
        number_attr(a, "width", 0.0)?,
        number_attr(a, "height", 0.0)?,
    );
    if w <= 0.0 || h <= 0.0 {
        return Ok(String::new());
    }
    let mut rx = number_attr(a, "rx", f64::NAN)?;
    let mut ry = number_attr(a, "ry", f64::NAN)?;
    if rx.is_nan() {
        rx = if ry.is_nan() { 0.0 } else { ry };
    }
    if ry.is_nan() {
        ry = rx;
    }
    let (rx, ry) = (rx.clamp(0.0, w / 2.0), ry.clamp(0.0, h / 2.0));
    if rx == 0.0 || ry == 0.0 {
        return Ok(format!("M{x} {y}H{}V{}H{x}Z", x + w, y + h));
    }
    Ok(format!(
        "M{} {y}H{}A{rx} {ry} 0 0 1 {} {}V{}A{rx} {ry} 0 0 1 {} {}H{}A{rx} {ry} 0 0 1 {x} {}V{}A{rx} {ry} 0 0 1 {} {y}Z",
        x + rx,
        x + w - rx,
        x + w,
        y + ry,
        y + h - ry,
        x + w - rx,
        y + h,
        x + rx,
        y + h - ry,
        y + ry,
        x + rx
    ))
}

/// The drawing of an SVG text as a text DXF file in millimetres, with a report. `tol_mm` is how far
/// a curve may sag from its straight lines and `max_entities` the most lines and circles.
pub fn to_dxf(svg: &str, tol_mm: f64, max_entities: usize) -> Result<(String, SvgReport), String> {
    if !(tol_mm.is_finite() && tol_mm > 0.0) {
        return Err("the tolerance of the curves must be over zero".to_owned());
    }
    let mut report = SvgReport::default();
    // the page: the millimetres of one user unit, and the matrix that places the drawing
    let mut page = 25.4 / 96.0;
    let mut origin = [0.0f64; 2];
    let mut seen_svg = false;
    let mut stack: Vec<(String, Matrix, bool)> = Vec::new();
    let mut drawn: Vec<Drawn> = Vec::new();
    let mut count = 0usize;
    let hidden = |name: &str| {
        matches!(
            name,
            "defs"
                | "clipPath"
                | "mask"
                | "pattern"
                | "symbol"
                | "marker"
                | "metadata"
                | "style"
                | "title"
                | "desc"
        )
    };
    for token in tokens(svg)? {
        match token {
            Token::Open {
                name,
                attributes: a,
                empty,
            } => {
                let (parent, parent_hidden) =
                    stack.last().map_or((IDENTITY, false), |(_, m, h)| (*m, *h));
                let own = match attr(&a, "transform") {
                    Some(t) => parse_transform(t)?,
                    None => IDENTITY,
                };
                let m = compose(&parent, &own);
                let is_hidden = parent_hidden || hidden(&name);
                if name == "svg" {
                    if !seen_svg {
                        seen_svg = true;
                        let view = attr(&a, "viewBox").map(numbers).transpose()?;
                        let width = attr(&a, "width").map(length).transpose()?;
                        if let Some(v) = &view {
                            if v.len() != 4 || v[2] <= 0.0 || v[3] <= 0.0 {
                                return Err("the viewBox needs four numbers and a size".to_owned());
                            }
                            origin = [v[0], v[1]];
                            if let Some((w, Some(unit))) = width {
                                if w > 0.0 {
                                    page = w * unit / v[2];
                                }
                            }
                        }
                    }
                } else if !is_hidden {
                    let mut lines: Vec<Subpath> = Vec::new();
                    let mut circle: Option<([f64; 2], f64)> = None;
                    match name.as_str() {
                        "line" => {
                            let p = |kx, ky| -> Result<[f64; 2], String> {
                                Ok([number_attr(&a, kx, 0.0)?, number_attr(&a, ky, 0.0)?])
                            };
                            lines.push((vec![p("x1", "y1")?, p("x2", "y2")?], false));
                        }
                        "polyline" | "polygon" => {
                            let list = numbers(attr(&a, "points").unwrap_or(""))?;
                            if list.len() % 2 == 1 {
                                return Err("a list of points has a number left over".to_owned());
                            }
                            let pts: Vec<[f64; 2]> = list.chunks(2).map(|c| [c[0], c[1]]).collect();
                            if pts.len() >= 2 {
                                lines.push((pts, name == "polygon"));
                            }
                        }
                        "rect" => lines = path(&rect_path(&a)?, tol_mm / page)?,
                        "circle" => {
                            let r = number_attr(&a, "r", 0.0)?;
                            if r > 0.0 {
                                circle = Some((
                                    [number_attr(&a, "cx", 0.0)?, number_attr(&a, "cy", 0.0)?],
                                    r,
                                ));
                            }
                        }
                        "ellipse" => {
                            let (cx, cy) =
                                (number_attr(&a, "cx", 0.0)?, number_attr(&a, "cy", 0.0)?);
                            let (rx, ry) =
                                (number_attr(&a, "rx", 0.0)?, number_attr(&a, "ry", 0.0)?);
                            if rx > 0.0 && ry > 0.0 {
                                if rx == ry {
                                    circle = Some(([cx, cy], rx));
                                } else {
                                    lines = path(
                                        &format!(
                                            "M{} {cy}A{rx} {ry} 0 1 0 {} {cy}A{rx} {ry} 0 1 0 {} {cy}Z",
                                            cx + rx,
                                            cx - rx,
                                            cx + rx
                                        ),
                                        tol_mm / page,
                                    )?;
                                }
                            }
                        }
                        "path" => lines = path(attr(&a, "d").unwrap_or(""), tol_mm / page)?,
                        "text" | "image" | "use" | "foreignObject" | "switch" => {
                            *report.skipped.entry(name.clone()).or_insert(0) += 1;
                        }
                        _ => {}
                    }
                    // the page: user units to millimetres, the origin of the view box to zero, the
                    // second axis up
                    let to_mm = |p: [f64; 2]| -> Result<[f64; 2], String> {
                        let q = apply(&m, p);
                        let out = [(q[0] - origin[0]) * page, -(q[1] - origin[1]) * page];
                        if out.iter().any(|v| !v.is_finite() || v.abs() > FAR_MM) {
                            return Err(
                                "a point of the drawing is too far from the origin".to_owned()
                            );
                        }
                        Ok(out)
                    };
                    for (pts, closed) in lines {
                        let mapped = pts
                            .iter()
                            .map(|p| to_mm(*p))
                            .collect::<Result<Vec<_>, _>>()?;
                        count += mapped.len().saturating_sub(1) + usize::from(closed);
                        drawn.push(Drawn::Line(mapped, closed));
                        report.shapes += 1;
                    }
                    if let Some((c, r)) = circle {
                        let similar = (m[0] - m[3]).abs()
                            <= 1e-9 * (m[0].abs() + m[3].abs() + 1e-300)
                            && (m[1] + m[2]).abs() <= 1e-9 * (m[0].abs() + m[1].abs() + 1e-300);
                        let scale = (m[0] * m[0] + m[1] * m[1]).sqrt();
                        if similar && scale > 0.0 {
                            count += 1;
                            drawn.push(Drawn::Circle(to_mm(c)?, r * scale * page));
                            report.shapes += 1;
                        } else {
                            let pts = path(
                                &format!(
                                    "M{} {}A{r} {r} 0 1 0 {} {}A{r} {r} 0 1 0 {} {}Z",
                                    c[0] + r,
                                    c[1],
                                    c[0] - r,
                                    c[1],
                                    c[0] + r,
                                    c[1]
                                ),
                                tol_mm / page / scale.max(1e-12),
                            )?;
                            for (p, closed) in pts {
                                let mapped =
                                    p.iter().map(|q| to_mm(*q)).collect::<Result<Vec<_>, _>>()?;
                                count += mapped.len().saturating_sub(1) + usize::from(closed);
                                drawn.push(Drawn::Line(mapped, closed));
                                report.shapes += 1;
                            }
                        }
                    }
                    if count > max_entities {
                        return Err(format!(
                            "the drawing holds more than {max_entities} lines and circles"
                        ));
                    }
                }
                if !empty {
                    stack.push((name, m, is_hidden));
                }
            }
            Token::Close(name) => {
                if stack.pop().map(|(n, _, _)| n).as_deref() != Some(name.as_str()) {
                    return Err(format!(
                        "the drawing is not well formed: </{name}> closes nothing"
                    ));
                }
            }
        }
    }
    if let Some((open, _, _)) = stack.last() {
        return Err(format!(
            "the drawing is not well formed: <{open}> is not closed"
        ));
    }
    if !seen_svg {
        return Err("the file has no <svg> element".to_owned());
    }
    if drawn.is_empty() {
        return Err("the drawing has no line, circle or path to read".to_owned());
    }
    let mut out = String::from(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n",
    );
    let f = |v: f64| format!("{v:.6}");
    for item in &drawn {
        match item {
            Drawn::Line(pts, closed) => {
                let segments = pts.len().saturating_sub(1) + usize::from(*closed);
                for k in 0..segments {
                    let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
                    if a == b {
                        continue;
                    }
                    out.push_str(&format!(
                        "0\nLINE\n8\n0\n10\n{}\n20\n{}\n30\n0\n11\n{}\n21\n{}\n31\n0\n",
                        f(a[0]),
                        f(a[1]),
                        f(b[0]),
                        f(b[1])
                    ));
                }
            }
            Drawn::Circle(c, r) => out.push_str(&format!(
                "0\nCIRCLE\n8\n0\n10\n{}\n20\n{}\n30\n0\n40\n{}\n",
                f(c[0]),
                f(c[1]),
                f(*r)
            )),
        }
    }
    out.push_str("0\nENDSEC\n0\nEOF\n");
    Ok((out, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxf(svg: &str) -> String {
        to_dxf(svg, 0.005, 100_000).unwrap().0
    }

    fn count(text: &str, what: &str) -> usize {
        text.lines().filter(|l| *l == what).count()
    }

    #[test]
    fn a_rectangle_in_millimetres_is_four_lines_with_the_second_axis_up() {
        let text = dxf("<svg xmlns='http://www.w3.org/2000/svg' width='100mm' height='50mm' viewBox='0 0 100 50'><rect x='10' y='5' width='30' height='20'/></svg>");
        assert_eq!(count(&text, "LINE"), 4);
        // the corner (10, 5) of the screen is (10, -5) of the drawing
        assert!(text.contains("10\n10.000000\n20\n-5.000000\n"));
        assert!(
            text.contains("10\n40.000000\n20\n-25.000000\n")
                || text.contains("11\n40.000000\n21\n-25.000000\n")
        );
        assert!(text.contains("$INSUNITS\n70\n4"));
    }

    #[test]
    fn the_size_of_the_page_scales_the_user_units() {
        // 100 user units on a page 25.4 mm wide: one unit is 0.254 mm
        let text = dxf("<svg width='1in' height='1in' viewBox='0 0 100 100'><line x1='0' y1='0' x2='100' y2='0'/></svg>");
        assert!(
            text.contains("11\n25.400000\n21\n0.000000\n")
                || text.contains("11\n25.400000\n21\n-0.000000\n"),
            "{text}"
        );
        // with no size at all a unit is a pixel of 96 to the inch
        let text = dxf("<svg><line x1='0' y1='0' x2='96' y2='0'/></svg>");
        assert!(text.contains("11\n25.400000\n"));
        // units of the page: pt, cm
        let text = dxf("<svg width='72pt' height='72pt' viewBox='0 0 10 10'><line x1='0' y1='0' x2='10' y2='0'/></svg>");
        assert!(text.contains("11\n25.400000\n"));
    }

    #[test]
    fn a_circle_stays_a_circle_when_the_transformation_is_a_similarity() {
        let text = dxf("<svg viewBox='0 0 96 96' width='25.4mm'><g transform='translate(10 20) scale(2)'><circle cx='5' cy='5' r='3'/></g></svg>");
        assert_eq!(count(&text, "CIRCLE"), 1);
        // the page makes a unit 25.4/96 of a millimetre; the radius is 3 * 2 units
        let r = 6.0 * 25.4 / 96.0;
        assert!(text.contains(&format!("40\n{r:.6}\n")), "{text}");
        // a stretch makes it an ellipse of lines
        let text = dxf("<svg viewBox='0 0 96 96' width='25.4mm'><circle transform='scale(2 1)' cx='5' cy='5' r='3'/></svg>");
        assert_eq!(count(&text, "CIRCLE"), 0);
        assert!(count(&text, "LINE") > 8);
        let text = dxf("<svg><ellipse cx='50' cy='50' rx='20' ry='10'/></svg>");
        assert!(count(&text, "LINE") > 8);
    }

    #[test]
    fn a_path_with_every_command_is_read_and_closed_paths_close() {
        let svg = "<svg viewBox='0 0 100 100' width='100mm'><path d='M10 10 h20 v20 H10 z M50,50 C60,40 70,40 80,50 S100,60 90,70 Q80,80 70,70 T50,70 A10 10 0 0 1 50 50 Z'/></svg>";
        let (text, report) = to_dxf(svg, 0.005, 100_000).unwrap();
        assert_eq!(report.shapes, 2);
        // the square: 4 lines; the curve is cut into many
        assert!(count(&text, "LINE") > 20);
        // a compact path: numbers that touch, signs as separators, flags that touch the numbers
        let compact = dxf("<svg><path d='M0 0l10-5.5.5.5a5 5 0 1010 0z'/></svg>");
        assert!(count(&compact, "LINE") >= 3);
    }

    #[test]
    fn the_tolerance_decides_how_many_lines_a_curve_gets() {
        let svg =
            "<svg viewBox='0 0 100 100' width='100mm'><path d='M0 0 C0 50 100 50 100 0'/></svg>";
        let coarse = to_dxf(svg, 1.0, 100_000).unwrap().0;
        let fine = to_dxf(svg, 0.001, 100_000).unwrap().0;
        assert!(count(&fine, "LINE") > 10 * count(&coarse, "LINE") / 2);
        assert!(count(&coarse, "LINE") >= 2);
        // an arc of a full semicircle of radius 10 mm sags 0.005 mm at about 0.0316 rad of step
        let arc = to_dxf(
            "<svg viewBox='0 0 100 100' width='100mm'><path d='M0 0 A10 10 0 0 1 20 0'/></svg>",
            0.005,
            100_000,
        )
        .unwrap()
        .0;
        let n = count(&arc, "LINE");
        assert!((45..=55).contains(&n), "{n}");
    }

    #[test]
    fn rounded_rectangles_polygons_and_hidden_things_are_handled() {
        let text = dxf("<svg viewBox='0 0 100 100' width='100mm'><defs><rect width='5' height='5'/></defs><rect width='20' height='10' rx='3'/><polygon points='0,0 10,0 5,8'/><polyline points='0 0 1 1 2 0'/><text>hi</text><use href='#a'/></svg>");
        assert!(count(&text, "LINE") > 10 + 3 + 2);
        let report = to_dxf(
            "<svg><rect width='20' height='10'/><text>x</text><text>y</text><image/></svg>",
            0.005,
            1000,
        )
        .unwrap()
        .1;
        assert_eq!(report.skipped.get("text"), Some(&2));
        assert_eq!(report.skipped.get("image"), Some(&1));
        assert_eq!(report.shapes, 1);
    }

    #[test]
    fn rotations_and_matrices_move_the_points() {
        let text = dxf("<svg viewBox='0 0 100 100' width='100mm'><line transform='rotate(90 0 0)' x1='10' y1='0' x2='20' y2='0'/></svg>");
        // (10, 0) turned a quarter turn is (0, 10) of the screen, (0, -10) of the drawing
        assert!(
            text.contains("10\n0.000000\n20\n-10.000000\n")
                || text.contains("10\n-0.000000\n20\n-10.000000\n"),
            "{text}"
        );
        let text = dxf("<svg viewBox='0 0 100 100' width='100mm'><line transform='matrix(1 0 0 1 5 6)' x1='0' y1='0' x2='1' y2='0'/></svg>");
        assert!(text.contains("10\n5.000000\n20\n-6.000000\n"));
    }

    #[test]
    fn the_result_is_read_by_the_dxf_reader_into_a_sketch() {
        let (text, _) = to_dxf(
            "<svg viewBox='0 0 100 100' width='100mm'><rect x='0' y='0' width='40' height='20'/><circle cx='20' cy='10' r='4'/></svg>",
            0.005,
            1000,
        )
        .unwrap();
        let import = crate::dxf_import::from_dxf(
            &text,
            "svg_sketch",
            crate::sketch::SketchPlane::on_face(crate::enclosure::Face::Front),
            1_000_000,
            1_000,
            1000,
        )
        .unwrap();
        assert_eq!((import.lines, import.circles), (4, 1));
        assert_eq!(crate::dxf_import::header_unit(&text).unwrap(), Some("mm"));
    }

    #[test]
    fn refusals_say_what_is_wrong() {
        let e = |svg: &str| to_dxf(svg, 0.005, 100).unwrap_err();
        assert!(e("<g/>").contains("no <svg>"));
        assert!(e("<svg></svg>").contains("no line, circle or path"));
        assert!(e("<svg><path d='M0 0 L'/></svg>").contains("number"));
        assert!(e("<svg><path d='X1 1'/></svg>").contains("not known"));
        assert!(e("<svg><line x1='a' y1='0' x2='1' y2='1'/></svg>").contains("x1"));
        assert!(e("<svg viewBox='0 0 0 5'><line x2='1' y2='1'/></svg>").contains("viewBox"));
        assert!(e("<svg><line transform='wobble(1)' x2='1' y2='1'/></svg>").contains("wobble"));
        assert!(e("<svg><line transform='scale(1' x2='1' y2='1'/></svg>").contains("parenthesis"));
        assert!(e("<svg><polyline points='1 2 3'/></svg>").contains("left over"));
        assert!(e("<svg><line x2='1e30' y2='1'/></svg>").contains("too far"));
        assert!(e("<svg><g><line x2='1' y2='1'/></svg>").contains("not well formed"));
        assert!(e("<svg width='5furlong'><line x2='1' y2='1'/></svg>").contains("furlong"));
        assert!(to_dxf("<svg><line x2='1' y2='1'/></svg>", 0.0, 100).is_err());
        let many = format!("<svg>{}</svg>", "<line x2='1' y2='1'/>".repeat(101));
        assert!(e(&many).contains("more than 100"));
    }

    #[test]
    fn damaged_drawings_never_panic_with_a_fixed_seed() {
        let base = "<svg viewBox='0 0 100 100' width='100mm'><g transform='scale(2)'><path d='M10 10 h20 v20 H10 z C1 2 3 4 5 6 S1 1 2 2 A5 5 0 0 1 9 9'/><circle cx='5' cy='5' r='3'/><rect width='5' height='5' rx='1'/></g></svg>".as_bytes().to_vec();
        let mut seed = 8u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as usize
        };
        for _ in 0..3000 {
            let mut bytes = base.clone();
            for _ in 0..1 + next() % 4 {
                let at = next() % bytes.len();
                let alphabet = b" 0123456789-.,<>/='\"aMLCZe";
                bytes[at] = alphabet[next() % alphabet.len()];
            }
            let cut = next() % (bytes.len() + 1);
            let _ = to_dxf(&String::from_utf8_lossy(&bytes[..cut]), 0.005, 5000);
        }
    }
}
