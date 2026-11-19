//! The sketch file (`.phoenixsketch`): one sketch in plain text, the way in and the way out of the
//! sketches of a project, which are kept in the project file itself (`super::domain`).
//!
//! The file has its own version in its first line (1, or 2 when the sketch has a kind of constraint
//! that came with the second version: `collinear`, `point_line_distance`, `line_angle`,
//! `concentric`, `symmetric_lines` and `symmetric_circles`; a program that reads only version 1 refuses such a file with its version, and a
//! file of version 1 that has one of those kinds is refused), and the same text is what the domain `sketch`
//! of the project file holds for each sketch. One line for the header, one for the name, one for the plane, one for the counters of
//! the identifiers and then one line for each element and each constraint, in the order they
//! were added:
//!
//! ```text
//! phoenix-sketch 1
//! name plate
//! plane top 0 0 0
//! next 5 3
//! e1 point 0 0
//! e2 point 40000000 0
//! e3 line e1 e2
//! c1 on horizontal e3
//! c2 on distance_x e1 e2 40000000 = width / 2
//! ```
//!
//! Lengths are integers in nanometres and angles integers in nano-degrees. The plane is the
//! name of a face and the three lengths `u0 v0 offset`. A line may end with `construction`, a
//! constraint is `on` or `off`, and a dimension may end with ` = ` and its expression. Empty
//! lines and lines that start with `#` are ignored; the text written is always the same for the
//! same sketch. A file of a newer version, a line that is not understood and a sketch that does
//! not hold together (an element that needs one that does not exist, a constraint about
//! elements of the wrong kind) are refused with `PHOENIX-CAD-035` and the number of the line.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::settings::SketchSettings;
use super::{
    Constraint, ConstraintEntry, ConstraintId, Dim, Element, ElementId, Entry, Sketch, SketchPlane,
};
use crate::enclosure::Face;
use crate::store;
use crate::units::Angle;
use crate::{CadError, Length};

/// The newest version of the format that this program reads.
pub const FILE_VERSION: u32 = 2;

/// The version of a sketch that has only the kinds of constraint of the first version.
pub const BASE_VERSION: u32 = 1;

/// The kinds of constraint that came with the second version of the format.
const VERSION_2_KINDS: [&str; 6] = [
    "collinear",
    "point_line_distance",
    "line_angle",
    "concentric",
    "symmetric_lines",
    "symmetric_circles",
];

/// The version a sketch is written with: 2 when it has a constraint that came with that version
/// and 1 otherwise, so that a sketch that does not need the newer kinds can still be read by a
/// program that reads only the first version.
pub fn version_of(sketch: &Sketch) -> u32 {
    let newer = sketch
        .constraints()
        .iter()
        .any(|c| VERSION_2_KINDS.contains(&c.constraint.kind_name()));
    if newer {
        FILE_VERSION
    } else {
        BASE_VERSION
    }
}

/// The extension of a sketch file.
pub const EXTENSION: &str = "phoenixsketch";

const HEADER: &str = "phoenix-sketch";

fn bad(line: usize, why: impl AsRef<str>) -> CadError {
    CadError::SketchFile(format!("line {line}: {}", why.as_ref()))
}

/// The path of the sketch file that goes with a project file: the same name with the extension of
/// the sketch. A project no longer keeps its sketch there; the file is only looked for when a
/// project has no sketch of its own, so that one kept that way can be put in the project.
pub fn sibling_path(project: &Path) -> PathBuf {
    project.with_extension(EXTENSION)
}

fn dim_tail<T>(dim: &Dim<T>) -> String {
    match &dim.expr {
        Some(text) => format!(" = {text}"),
        None => String::new(),
    }
}

fn constraint_text(c: &Constraint) -> String {
    let name = c.kind_name();
    match c {
        Constraint::Coincident(a, b)
        | Constraint::HorizontalPoints(a, b)
        | Constraint::VerticalPoints(a, b)
        | Constraint::Parallel(a, b)
        | Constraint::Perpendicular(a, b)
        | Constraint::Tangent(a, b)
        | Constraint::Collinear(a, b)
        | Constraint::Concentric(a, b)
        | Constraint::Equal(a, b) => format!("{name} {a} {b}"),
        Constraint::PointOnObject { point, object } => format!("{name} {point} {object}"),
        Constraint::CollinearPoints(a, b, c) => format!("{name} {a} {b} {c}"),
        Constraint::EqualDistance { a, b, c, d } | Constraint::EqualAngle { a, b, c, d } => {
            format!("{name} {a} {b} {c} {d}")
        }
        Constraint::Equidistant { point, a, b } => format!("{name} {point} {a} {b}"),
        Constraint::Horizontal(l) | Constraint::Vertical(l) => format!("{name} {l}"),
        Constraint::SymmetricLine { a, b, axis }
        | Constraint::SymmetricLines { a, b, axis }
        | Constraint::SymmetricCircles { a, b, axis } => format!("{name} {a} {b} {axis}"),
        Constraint::SymmetricPoint { a, b, center } => format!("{name} {a} {b} {center}"),
        Constraint::Midpoint { point, line } => format!("{name} {point} {line}"),
        Constraint::Fixed { point, x, y } => format!("{name} {point} {} {}", x.get(), y.get()),
        Constraint::Distance { a, b, dim }
        | Constraint::DistanceX { a, b, dim }
        | Constraint::DistanceY { a, b, dim } => {
            format!("{name} {a} {b} {}{}", dim.value.get(), dim_tail(dim))
        }
        Constraint::LineLength { line, dim } => {
            format!("{name} {line} {}{}", dim.value.get(), dim_tail(dim))
        }
        Constraint::Radius { round, dim } | Constraint::Diameter { round, dim } => {
            format!("{name} {round} {}{}", dim.value.get(), dim_tail(dim))
        }
        Constraint::AngleBetween { a, b, dim } => {
            format!("{name} {a} {b} {}{}", dim.value.get(), dim_tail(dim))
        }
        Constraint::PointLineDistance { point, line, dim } => {
            format!("{name} {point} {line} {}{}", dim.value.get(), dim_tail(dim))
        }
        Constraint::LineAngle { line, dim } => {
            format!("{name} {line} {}{}", dim.value.get(), dim_tail(dim))
        }
    }
}

/// The text of a sketch.
pub fn to_text(sketch: &Sketch) -> String {
    let mut out = String::new();
    let plane = sketch.plane();
    let (next_e, next_c) = sketch.next_ids();
    let _ = writeln!(out, "{HEADER} {}", version_of(sketch));
    let _ = writeln!(out, "name {}", sketch.name());
    let _ = writeln!(
        out,
        "plane {} {} {} {}",
        plane.face.name(),
        plane.u0.get(),
        plane.v0.get(),
        plane.offset.get()
    );
    let _ = writeln!(out, "next {next_e} {next_c}");
    for e in sketch.elements() {
        let body = match e.element {
            Element::Point { x, y } => format!("point {} {}", x.get(), y.get()),
            Element::Line { a, b } => format!("line {a} {b}"),
            Element::Circle { center, radius } => format!("circle {center} {}", radius.get()),
            Element::Arc { center, start, end } => format!("arc {center} {start} {end}"),
        };
        let tail = if e.construction { " construction" } else { "" };
        let _ = writeln!(out, "{} {body}{tail}", e.id);
    }
    for c in sketch.constraints() {
        let state = if c.enabled { "on" } else { "off" };
        let _ = writeln!(out, "{} {state} {}", c.id, constraint_text(&c.constraint));
    }
    out
}

fn number(line: usize, text: &str) -> Result<i64, CadError> {
    text.parse::<i64>()
        .map_err(|_| bad(line, format!("{text:?} is not a whole number")))
}

fn counter(line: usize, text: &str) -> Result<u32, CadError> {
    text.parse::<u32>()
        .map_err(|_| bad(line, format!("{text:?} is not a counter")))
}

fn length(line: usize, text: &str) -> Result<Length, CadError> {
    Length::new(number(line, text)?).map_err(|e| bad(line, e.to_string()))
}

fn element_id(line: usize, text: &str) -> Result<ElementId, CadError> {
    text.strip_prefix('e')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .map(ElementId)
        .ok_or_else(|| bad(line, format!("{text:?} is not an element identifier")))
}

fn constraint_id(line: usize, text: &str) -> Result<ConstraintId, CadError> {
    text.strip_prefix('c')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .map(ConstraintId)
        .ok_or_else(|| bad(line, format!("{text:?} is not a constraint identifier")))
}

/// Checks that there are exactly `count` words.
fn exactly(line: usize, what: &str, words: &[&str], count: usize) -> Result<(), CadError> {
    if words.len() == count {
        Ok(())
    } else {
        Err(bad(
            line,
            format!("{what} takes {count} values after its name"),
        ))
    }
}

fn parse_constraint(
    line: usize,
    kind: &str,
    words: &[&str],
    expr: Option<String>,
) -> Result<Constraint, CadError> {
    let id = |i: usize| element_id(line, words[i]);
    let has_expr_ok = matches!(
        kind,
        "distance"
            | "distance_x"
            | "distance_y"
            | "length"
            | "radius"
            | "diameter"
            | "angle"
            | "point_line_distance"
            | "line_angle"
    );
    if expr.is_some() && !has_expr_ok {
        return Err(bad(line, format!("{kind} has no expression")));
    }
    let len_dim = |i: usize| -> Result<Dim<Length>, CadError> {
        Ok(Dim {
            value: length(line, words[i])?,
            expr: expr.clone(),
        })
    };
    let c = match kind {
        "coincident" | "horizontal_points" | "vertical_points" | "parallel" | "perpendicular"
        | "tangent" | "equal" | "point_on_object" | "midpoint" | "collinear" | "concentric" => {
            exactly(line, kind, words, 2)?;
            let (a, b) = (id(0)?, id(1)?);
            match kind {
                "coincident" => Constraint::Coincident(a, b),
                "horizontal_points" => Constraint::HorizontalPoints(a, b),
                "vertical_points" => Constraint::VerticalPoints(a, b),
                "parallel" => Constraint::Parallel(a, b),
                "perpendicular" => Constraint::Perpendicular(a, b),
                "tangent" => Constraint::Tangent(a, b),
                "equal" => Constraint::Equal(a, b),
                "collinear" => Constraint::Collinear(a, b),
                "concentric" => Constraint::Concentric(a, b),
                "point_on_object" => Constraint::PointOnObject {
                    point: a,
                    object: b,
                },
                _ => Constraint::Midpoint { point: a, line: b },
            }
        }
        "horizontal" | "vertical" => {
            exactly(line, kind, words, 1)?;
            if kind == "horizontal" {
                Constraint::Horizontal(id(0)?)
            } else {
                Constraint::Vertical(id(0)?)
            }
        }
        "collinear_points" => {
            exactly(line, kind, words, 3)?;
            Constraint::CollinearPoints(id(0)?, id(1)?, id(2)?)
        }
        "equal_distance" => {
            exactly(line, kind, words, 4)?;
            Constraint::EqualDistance {
                a: id(0)?,
                b: id(1)?,
                c: id(2)?,
                d: id(3)?,
            }
        }
        "equal_angle" => {
            exactly(line, kind, words, 4)?;
            Constraint::EqualAngle {
                a: id(0)?,
                b: id(1)?,
                c: id(2)?,
                d: id(3)?,
            }
        }
        "equidistant" => {
            exactly(line, kind, words, 3)?;
            Constraint::Equidistant {
                point: id(0)?,
                a: id(1)?,
                b: id(2)?,
            }
        }
        "symmetric_line" => {
            exactly(line, kind, words, 3)?;
            Constraint::SymmetricLine {
                a: id(0)?,
                b: id(1)?,
                axis: id(2)?,
            }
        }
        "symmetric_lines" => {
            exactly(line, kind, words, 3)?;
            Constraint::SymmetricLines {
                a: id(0)?,
                b: id(1)?,
                axis: id(2)?,
            }
        }
        "symmetric_circles" => {
            exactly(line, kind, words, 3)?;
            Constraint::SymmetricCircles {
                a: id(0)?,
                b: id(1)?,
                axis: id(2)?,
            }
        }
        "symmetric_point" => {
            exactly(line, kind, words, 3)?;
            Constraint::SymmetricPoint {
                a: id(0)?,
                b: id(1)?,
                center: id(2)?,
            }
        }
        "fixed" => {
            exactly(line, kind, words, 3)?;
            Constraint::Fixed {
                point: id(0)?,
                x: length(line, words[1])?,
                y: length(line, words[2])?,
            }
        }
        "distance" | "distance_x" | "distance_y" => {
            exactly(line, kind, words, 3)?;
            let (a, b, dim) = (id(0)?, id(1)?, len_dim(2)?);
            match kind {
                "distance" => Constraint::Distance { a, b, dim },
                "distance_x" => Constraint::DistanceX { a, b, dim },
                _ => Constraint::DistanceY { a, b, dim },
            }
        }
        "length" => {
            exactly(line, kind, words, 2)?;
            Constraint::LineLength {
                line: id(0)?,
                dim: len_dim(1)?,
            }
        }
        "radius" | "diameter" => {
            exactly(line, kind, words, 2)?;
            let (round, dim) = (id(0)?, len_dim(1)?);
            if kind == "radius" {
                Constraint::Radius { round, dim }
            } else {
                Constraint::Diameter { round, dim }
            }
        }
        "point_line_distance" => {
            exactly(line, kind, words, 3)?;
            Constraint::PointLineDistance {
                point: id(0)?,
                line: id(1)?,
                dim: len_dim(2)?,
            }
        }
        "line_angle" => {
            exactly(line, kind, words, 2)?;
            Constraint::LineAngle {
                line: id(0)?,
                dim: Dim {
                    value: Angle::new(number(line, words[1])?)
                        .map_err(|e| bad(line, e.to_string()))?,
                    expr,
                },
            }
        }
        "angle" => {
            exactly(line, kind, words, 3)?;
            Constraint::AngleBetween {
                a: id(0)?,
                b: id(1)?,
                dim: Dim {
                    value: Angle::new(number(line, words[2])?)
                        .map_err(|e| bad(line, e.to_string()))?,
                    expr,
                },
            }
        }
        other => return Err(bad(line, format!("{other:?} is not a kind of constraint"))),
    };
    Ok(c)
}

fn parse_element(line: usize, words: &[&str]) -> Result<(Element, bool), CadError> {
    let (construction, words) = match words.split_last() {
        Some((&"construction", rest)) => (true, rest),
        _ => (false, words),
    };
    let Some((kind, args)) = words.split_first() else {
        return Err(bad(line, "an element needs a kind"));
    };
    let element = match *kind {
        "point" => {
            exactly(line, "point", args, 2)?;
            Element::Point {
                x: length(line, args[0])?,
                y: length(line, args[1])?,
            }
        }
        "line" => {
            exactly(line, "line", args, 2)?;
            Element::Line {
                a: element_id(line, args[0])?,
                b: element_id(line, args[1])?,
            }
        }
        "circle" => {
            exactly(line, "circle", args, 2)?;
            Element::Circle {
                center: element_id(line, args[0])?,
                radius: length(line, args[1])?,
            }
        }
        "arc" => {
            exactly(line, "arc", args, 3)?;
            Element::Arc {
                center: element_id(line, args[0])?,
                start: element_id(line, args[1])?,
                end: element_id(line, args[2])?,
            }
        }
        other => return Err(bad(line, format!("{other:?} is not a kind of element"))),
    };
    Ok((element, construction))
}

/// Reads a sketch from its text.
pub fn from_text(text: &str) -> Result<Sketch, CadError> {
    let mut header = false;
    let mut version = 0u32;
    let mut name: Option<String> = None;
    let mut plane: Option<SketchPlane> = None;
    let mut next: Option<(u32, u32)> = None;
    let mut elements: Vec<Entry> = Vec::new();
    let mut constraints: Vec<ConstraintEntry> = Vec::new();
    let mut last_line = 0;
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        last_line = line;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !header {
            let mut words = trimmed.split_whitespace();
            if words.next() != Some(HEADER) {
                return Err(bad(line, "this is not a sketch file"));
            }
            version = words
                .next()
                .and_then(|v| v.parse::<u32>().ok())
                .ok_or_else(|| bad(line, "the version is missing"))?;
            if words.next().is_some() {
                return Err(bad(line, "the header has more than a version"));
            }
            if version > FILE_VERSION {
                return Err(bad(
                    line,
                    format!("the file is of version {version} and this program reads up to {FILE_VERSION}"),
                ));
            }
            if version == 0 {
                return Err(bad(line, "the version is 1 or more"));
            }
            header = true;
            continue;
        }
        // an expression is everything after the first " = "
        let (head, expr) = match trimmed.split_once(" = ") {
            Some((h, e)) => (h, Some(e.trim().to_string())),
            None => (trimmed, None),
        };
        let words: Vec<&str> = head.split_whitespace().collect();
        let Some((first, rest)) = words.split_first() else {
            continue;
        };
        match *first {
            "name" if expr.is_none() => {
                if name.is_some() || rest.len() != 1 {
                    return Err(bad(line, "one name"));
                }
                name = Some(rest[0].to_string());
            }
            "plane" if expr.is_none() => {
                if plane.is_some() || rest.len() != 4 {
                    return Err(bad(line, "one plane: face u0 v0 offset"));
                }
                let face = Face::from_name(rest[0])
                    .ok_or_else(|| bad(line, format!("{:?} is not a face", rest[0])))?;
                plane = Some(SketchPlane {
                    face,
                    u0: length(line, rest[1])?,
                    v0: length(line, rest[2])?,
                    offset: length(line, rest[3])?,
                });
            }
            "next" if expr.is_none() => {
                if next.is_some() || rest.len() != 2 {
                    return Err(bad(line, "one line of counters: next E C"));
                }
                next = Some((counter(line, rest[0])?, counter(line, rest[1])?));
            }
            w if w.starts_with('e') => {
                if expr.is_some() {
                    return Err(bad(line, "an element has no expression"));
                }
                let id = element_id(line, w)?;
                let (element, construction) = parse_element(line, rest)?;
                elements.push(Entry {
                    id,
                    element,
                    construction,
                });
            }
            w if w.starts_with('c') => {
                let id = constraint_id(line, w)?;
                let Some((state, rest)) = rest.split_first() else {
                    return Err(bad(line, "a constraint needs on or off"));
                };
                let enabled = match *state {
                    "on" => true,
                    "off" => false,
                    other => return Err(bad(line, format!("{other:?} is not on or off"))),
                };
                let Some((kind, args)) = rest.split_first() else {
                    return Err(bad(line, "a constraint needs a kind"));
                };
                if version < FILE_VERSION && VERSION_2_KINDS.contains(kind) {
                    return Err(bad(
                        line,
                        format!("{kind} needs a file of version {FILE_VERSION}"),
                    ));
                }
                let constraint = parse_constraint(line, kind, args, expr)?;
                constraints.push(ConstraintEntry {
                    id,
                    constraint,
                    enabled,
                });
            }
            other => return Err(bad(line, format!("{other:?} is not understood"))),
        }
    }
    if !header {
        return Err(bad(last_line.max(1), "this is not a sketch file"));
    }
    let name = name.ok_or_else(|| bad(last_line, "the name is missing"))?;
    let plane = plane.ok_or_else(|| bad(last_line, "the plane is missing"))?;
    let next = next.ok_or_else(|| bad(last_line, "the counters are missing"))?;
    Sketch::from_parts(&name, plane, elements, constraints, next)
        .map_err(|e| CadError::SketchFile(e.to_string()))
}

/// Writes a sketch to a file, atomically, keeping `backups` older copies.
pub fn write_file(path: &Path, sketch: &Sketch, backups: u32) -> Result<(), CadError> {
    store::save_atomic(path, to_text(sketch).as_bytes(), backups)
}

/// Reads a sketch file under the size limit of the settings.
pub fn read_file(path: &Path, settings: &SketchSettings) -> Result<Sketch, CadError> {
    let bytes = store::load_limited(path, settings.max_file_bytes)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| CadError::SketchFile("the file is not text".into()))?;
    let sketch = from_text(&text)?;
    if sketch.elements().len() > settings.max_elements
        || sketch.constraints().len() > settings.max_constraints
    {
        return Err(CadError::SketchFile(
            "the sketch is larger than the settings allow".into(),
        ));
    }
    Ok(sketch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::tests::{mm, sketch};

    fn rich() -> Sketch {
        let mut k = Sketch::new(
            "plate",
            SketchPlane {
                face: Face::Front,
                u0: mm(3),
                v0: mm(-4),
                offset: mm(1),
            },
        )
        .unwrap();
        let r = k.add_rectangle((mm(0), mm(0)), (mm(40), mm(25))).unwrap();
        let c = k.add_point(mm(20), mm(12)).unwrap();
        let circle = k.add_circle(c, mm(5)).unwrap();
        let arc_c = k.add_point(mm(50), mm(0)).unwrap();
        let arc_s = k.add_point(mm(55), mm(0)).unwrap();
        let arc_e = k.add_point(mm(50), mm(5)).unwrap();
        let arc = k.add_arc(arc_c, arc_s, arc_e).unwrap();
        k.set_construction(arc, true).unwrap();
        k.add_constraint(Constraint::DistanceX {
            a: r.points[0],
            b: r.points[1],
            dim: Dim::with_expr(mm(40), "width / 2 + 1 mm"),
        })
        .unwrap();
        let off = k
            .add_constraint(Constraint::Tangent(r.lines[0], circle))
            .unwrap();
        k.set_enabled(off, false).unwrap();
        k.add_constraint(Constraint::AngleBetween {
            a: r.lines[0],
            b: r.lines[1],
            dim: Dim::fixed(Angle::from_deg(90).unwrap()),
        })
        .unwrap();
        k.add_constraint(Constraint::Fixed {
            point: c,
            x: mm(20),
            y: mm(12),
        })
        .unwrap();
        k.add_constraint(Constraint::Diameter {
            round: circle,
            dim: Dim::fixed(mm(10)),
        })
        .unwrap();
        let removed = k
            .add_constraint(Constraint::Equal(r.lines[0], r.lines[2]))
            .unwrap();
        k.remove_constraint(removed).unwrap();
        k
    }

    #[test]
    fn a_sketch_written_and_read_again_is_the_same_sketch() {
        let k = rich();
        let text = to_text(&k);
        let back = from_text(&text).unwrap();
        assert_eq!(back, k);
        assert_eq!(to_text(&back), text);
        assert!(text.starts_with(
            "phoenix-sketch 1\nname plate\nplane front 3000000 -4000000 1000000\nnext 15 11\n"
        ));
        assert!(text.contains("e14 arc e11 e12 e13 construction\n"));
        assert!(text.contains(" = width / 2 + 1 mm\n"));
        assert!(text.contains("c6 off tangent e5 e10\n"), "{text}");
        // the counters keep the removed identifier from coming back
        let mut again = back;
        let id = again
            .add_constraint(Constraint::Horizontal(ElementId(5)))
            .unwrap();
        assert_eq!(id, ConstraintId(11));
    }

    #[test]
    fn the_text_is_exact_for_a_small_sketch() {
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(40), mm(0)).unwrap();
        let l = k.add_line(a, b).unwrap();
        k.add_constraint(Constraint::Horizontal(l)).unwrap();
        k.add_constraint(Constraint::LineLength {
            line: l,
            dim: Dim::fixed(mm(40)),
        })
        .unwrap();
        assert_eq!(
            to_text(&k),
            "phoenix-sketch 1\nname s1\nplane top 0 0 0\nnext 4 3\n\
             e1 point 0 0\ne2 point 40000000 0\ne3 line e1 e2\n\
             c1 on horizontal e3\nc2 on length e3 40000000\n"
        );
    }

    #[test]
    fn comments_and_empty_lines_are_ignored() {
        let text = "# a sketch\n\nphoenix-sketch 1\nname a\n\nplane top 0 0 0\nnext 2 1\n# one point\ne1 point 1 2\n";
        let k = from_text(text).unwrap();
        assert_eq!(k.elements().len(), 1);
    }

    #[test]
    fn a_file_that_is_wrong_is_refused_with_its_line() {
        let head = "phoenix-sketch 1\nname a\nplane top 0 0 0\nnext 4 2\n";
        let cases: Vec<(String, &str)> = vec![
            ("".into(), "not a sketch file"),
            ("hello 1\n".into(), "not a sketch file"),
            ("phoenix-sketch 3\n".into(), "version 3"),
            ("phoenix-sketch 0\n".into(), "1 or more"),
            ("phoenix-sketch x\n".into(), "version is missing"),
            ("phoenix-sketch 1\n".into(), "name is missing"),
            ("phoenix-sketch 1\nname a\n".into(), "plane is missing"),
            (
                "phoenix-sketch 1\nname a\nplane top 0 0 0\n".into(),
                "counters are missing",
            ),
            (format!("{head}e1 blob 1 2\n"), "line 5: \"blob\""),
            (format!("{head}e1 point 1\n"), "line 5: point takes 2"),
            (
                format!("{head}e1 point 1 x\n"),
                "line 5: \"x\" is not a whole number",
            ),
            (format!("{head}e1 point 1 99999999999999999\n"), "line 5"),
            (format!("{head}e0 point 1 2\n"), "not an element identifier"),
            (format!("{head}e1 point 1 2\ne1 point 3 4\n"), "repeated"),
            (
                format!("{head}e1 point 1 2\ne2 line e1 e1\n"),
                "two different points",
            ),
            (format!("{head}e2 line e1 e3\n"), "needs e1 before it"),
            (
                format!("{head}e1 point 1 2\nc1 maybe horizontal e1\n"),
                "not on or off",
            ),
            (
                format!("{head}e1 point 1 2\nc1 on horizontal e1\n"),
                "is not a line",
            ),
            (
                format!("{head}e1 point 1 2\nc1 on wobble e1\n"),
                "not a kind of constraint",
            ),
            (
                format!("{head}e1 point 1 2\nc1 on fixed e1 1\n"),
                "fixed takes 3",
            ),
            (
                format!("{head}e1 point 1 2\ne2 point 3 4\nc1 on coincident e1 e2 = x\n"),
                "no expression",
            ),
            (
                format!("{head}e1 point 1 2\nc9 on fixed e1 1 2\n"),
                "out of order",
            ),
            (format!("{head}e1 point 1 2 = 3\n"), "no expression"),
            (format!("{head}name b\n"), "one name"),
            (format!("{head}plane top 0 0\n"), "one plane"),
            (
                "phoenix-sketch 1\nname a\nplane roof 0 0 0\n".into(),
                "not a face",
            ),
            (format!("{head}wobble\n"), "not understood"),
        ];
        for (text, wanted) in cases {
            let err = from_text(&text).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-035", "{text:?}");
            assert!(err.to_string().contains(wanted), "{text:?}: {err}");
        }
    }

    #[test]
    fn the_file_goes_next_to_the_project_and_is_read_under_a_size_limit() {
        assert_eq!(
            sibling_path(Path::new("work/box.phoenixproj")),
            Path::new("work/box.phoenixsketch")
        );
        let dir = std::env::temp_dir().join(format!("phoenix_sketch_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.phoenixsketch");
        let k = rich();
        write_file(&path, &k, 2).unwrap();
        let settings = SketchSettings::defaults();
        assert_eq!(read_file(&path, &settings).unwrap(), k);
        // written again over the first: one numbered copy stays
        write_file(&path, &k, 2).unwrap();
        assert!(store::backup_path(&path, 1).exists());
        let small = SketchSettings::with_user_config("[sketch]\nmax_file_bytes = 1024\n").unwrap();
        let long = dir.join("long.phoenixsketch");
        std::fs::write(&long, format!("{}# {}\n", to_text(&k), "x".repeat(2000))).unwrap();
        assert_eq!(
            read_file(&long, &small).unwrap_err().code(),
            "PHOENIX-CAD-012"
        );
        assert!(read_file(&long, &settings).is_ok());
        let few = SketchSettings::with_user_config("[sketch]\nmax_elements = 3\n").unwrap();
        assert_eq!(
            read_file(&path, &few).unwrap_err().code(),
            "PHOENIX-CAD-035"
        );
        std::fs::write(dir.join("bytes.phoenixsketch"), [0xff, 0xfe]).unwrap();
        assert_eq!(
            read_file(&dir.join("bytes.phoenixsketch"), &settings)
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-035"
        );
        assert_eq!(
            read_file(&dir.join("missing.phoenixsketch"), &settings)
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-012"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn collinear_points_and_equal_distances_are_written_and_read_back() {
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(10), mm(0)).unwrap();
        let c = k.add_point(mm(20), mm(0)).unwrap();
        let d = k.add_point(mm(30), mm(1)).unwrap();
        k.add_constraint(Constraint::CollinearPoints(a, b, c))
            .unwrap();
        k.add_constraint(Constraint::EqualDistance { a, b, c, d })
            .unwrap();
        let text = to_text(&k);
        assert!(text.contains("c1 on collinear_points e1 e2 e3\n"), "{text}");
        assert!(
            text.contains("c2 on equal_distance e1 e2 e3 e4\n"),
            "{text}"
        );
        let back = from_text(&text).unwrap();
        assert_eq!(back, k);
        assert_eq!(to_text(&back), text);
        // the number of values is checked for each kind
        for bad in [
            "c1 on collinear_points e1 e2\n",
            "c1 on collinear_points e1 e2 e3 e4\n",
            "c1 on equal_distance e1 e2 e3\n",
            "c1 on equal_distance e1 e2 e3 e4 e1\n",
            "c1 on equal_distance e1 e2 e3 x\n",
        ] {
            let broken = text
                .lines()
                .take_while(|l| !l.starts_with("c1 "))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
                + bad;
            assert!(from_text(&broken).is_err(), "{bad}");
        }
    }

    /// The next number of a fixed sequence (xorshift), so that a failure can be repeated.
    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    /// A damaged copy of a sketch file: a line dropped, repeated, swapped with another or joined to
    /// the next one, a line from a pool of valid and invalid ones put in, one character changed,
    /// or the text cut short.
    fn damaged(text: &str, state: &mut u64) -> String {
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let n = lines.len();
        let at = (next(state) % n as u64) as usize;
        match next(state) % 7 {
            0 => {
                lines.remove(at);
            }
            1 => {
                let copy = lines[at].clone();
                lines.insert(at, copy);
            }
            2 => {
                let other = (next(state) % n as u64) as usize;
                lines.swap(at, other);
            }
            3 => {
                let joined = format!("{} {}", lines[at], lines[(at + 1) % n]);
                lines[at] = joined;
            }
            4 => {
                let pool = [
                    "e99 point 1 2",
                    "e1 point 0 0",
                    "c9 on equal e1 e2",
                    "c9 on parallel e1 e1",
                    "c9 off midpoint e1 e999",
                    "",
                    "sketch",
                    "e5 line e1",
                    "e6 circle e1 -5",
                    "c7 on distance e1 e2 999999999999999999",
                ];
                let line = pool[(next(state) % pool.len() as u64) as usize];
                lines.insert(at, line.to_owned());
            }
            5 => {
                let chars: Vec<char> = lines[at].chars().collect();
                if !chars.is_empty() {
                    let i = (next(state) % chars.len() as u64) as usize;
                    let with = ['0', '9', '-', 'x', 'e', '.', ' ', '\u{e9}'];
                    let mut out = chars;
                    out[i] = with[(next(state) % with.len() as u64) as usize];
                    lines[at] = out.into_iter().collect();
                }
            }
            _ => {
                lines.truncate(at);
                let text = lines.join("\n");
                let chars: Vec<char> = text.chars().collect();
                let keep = if chars.is_empty() {
                    0
                } else {
                    (next(state) % chars.len() as u64) as usize
                };
                return chars[..keep].iter().collect();
            }
        }
        lines.join("\n") + "\n"
    }

    #[test]
    fn a_damaged_sketch_file_is_refused_or_read_and_what_is_read_is_written_back_the_same() {
        let text = to_text(&rich());
        assert_eq!(from_text(&text).unwrap(), rich());
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let (mut read, mut refused) = (0, 0);
        for round in 0..3000 {
            // the damage is done once or twice to the same text
            let mut bad = damaged(&text, &mut state);
            if round % 2 == 1 && !bad.is_empty() {
                bad = damaged(&bad, &mut state);
            }
            match from_text(&bad) {
                Ok(sketch) => {
                    read += 1;
                    let again = to_text(&sketch);
                    let back = from_text(&again)
                        .unwrap_or_else(|e| panic!("{e}\n--- read\n{bad}\n--- written\n{again}"));
                    assert_eq!(back, sketch, "{bad}");
                    assert_eq!(to_text(&back), again, "{bad}");
                }
                Err(e) => {
                    refused += 1;
                    assert!(e.code().starts_with("PHOENIX-CAD-"), "{e} for {bad}");
                }
            }
        }
        // most of the damage is refused and some of it is harmless: both happen
        assert!(read > 0 && refused > 1000, "{read} read, {refused} refused");
    }

    #[test]
    fn equidistant_and_equal_angle_are_written_and_read_back() {
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(10), mm(0)).unwrap();
        let c = k.add_point(mm(5), mm(5)).unwrap();
        let l1 = k.add_line(a, b).unwrap();
        let l2 = k.add_line(a, c).unwrap();
        let l3 = k.add_line(b, c).unwrap();
        k.add_constraint(Constraint::Equidistant { point: c, a, b })
            .unwrap();
        k.add_constraint(Constraint::EqualAngle {
            a: l1,
            b: l2,
            c: l1,
            d: l3,
        })
        .unwrap();
        let text = to_text(&k);
        assert!(text.contains("c1 on equidistant e3 e1 e2\n"), "{text}");
        assert!(text.contains("c2 on equal_angle e4 e5 e4 e6\n"), "{text}");
        let back = from_text(&text).unwrap();
        assert_eq!(back, k);
        assert_eq!(to_text(&back), text);
        for bad in [
            "c1 on equidistant e3 e1\n",
            "c1 on equidistant e3 e1 e2 e1\n",
            "c1 on equal_angle e4 e5 e4\n",
            "c1 on equal_angle e4 e5 e4 e6 e4\n",
            "c1 on equal_angle e4 e5 e4 x\n",
        ] {
            let broken = text
                .lines()
                .take_while(|l| !l.starts_with("c1 "))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
                + bad;
            assert!(from_text(&broken).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_collinear_and_the_line_dimensions_are_written_and_read_back() {
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(10), mm(0)).unwrap();
        let c = k.add_point(mm(20), mm(0)).unwrap();
        let d = k.add_point(mm(30), mm(1)).unwrap();
        let l1 = k.add_line(a, b).unwrap();
        let l2 = k.add_line(c, d).unwrap();
        k.add_constraint(Constraint::Collinear(l1, l2)).unwrap();
        k.add_constraint(Constraint::PointLineDistance {
            point: d,
            line: l1,
            dim: Dim::fixed(mm(5)),
        })
        .unwrap();
        k.add_constraint(Constraint::LineAngle {
            line: l2,
            dim: Dim::with_expr(Angle::from_deg(30).unwrap(), "20 deg + 10 deg"),
        })
        .unwrap();
        let text = to_text(&k);
        assert!(text.starts_with("phoenix-sketch 2\n"), "{text}");
        assert!(text.contains("c1 on collinear e5 e6\n"), "{text}");
        assert!(
            text.contains("c2 on point_line_distance e4 e5 5000000\n"),
            "{text}"
        );
        assert!(
            text.contains("c3 on line_angle e6 30000000000 = 20 deg + 10 deg\n"),
            "{text}"
        );
        let back = from_text(&text).unwrap();
        assert_eq!(back, k);
        assert_eq!(to_text(&back), text);
        // the number of values is checked for each kind
        for bad in [
            "c1 on collinear e5\n",
            "c1 on collinear e5 e6 e7\n",
            "c1 on point_line_distance e4 e5\n",
            "c1 on line_angle e6\n",
            "c1 on line_angle e6 x\n",
        ] {
            let broken = text
                .lines()
                .take_while(|l| !l.starts_with("c1 "))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
                + bad;
            assert!(from_text(&broken).is_err(), "{bad}");
        }
        // collinear has no expression
        let with_expr = text.replace("c1 on collinear e5 e6", "c1 on collinear e5 e6 = 3");
        assert!(from_text(&with_expr).is_err());
    }

    #[test]
    fn the_symmetry_of_lines_and_of_circles_is_written_as_version_two_and_read_back() {
        let mut k = sketch();
        let o = k.add_point(mm(10), mm(0)).unwrap();
        let t = k.add_point(mm(10), mm(20)).unwrap();
        let axis = k.add_line(o, t).unwrap();
        let (a, b) = (
            k.add_point(mm(0), mm(0)).unwrap(),
            k.add_point(mm(0), mm(5)).unwrap(),
        );
        let (c, d) = (
            k.add_point(mm(20), mm(0)).unwrap(),
            k.add_point(mm(20), mm(5)).unwrap(),
        );
        let first = k.add_line(a, b).unwrap();
        let second = k.add_line(c, d).unwrap();
        k.add_constraint(Constraint::SymmetricLines {
            a: first,
            b: second,
            axis,
        })
        .unwrap();
        let r1 = k.add_circle(a, mm(2)).unwrap();
        let r2 = k.add_circle(c, mm(2)).unwrap();
        k.add_constraint(Constraint::SymmetricCircles { a: r1, b: r2, axis })
            .unwrap();
        assert_eq!(version_of(&k), FILE_VERSION);
        let text = to_text(&k);
        assert!(
            text.starts_with(
                "phoenix-sketch 2
"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "on symmetric_lines e8 e9 e3
"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "on symmetric_circles e10 e11 e3
"
            ),
            "{text}"
        );
        let back = from_text(&text).unwrap();
        assert_eq!(back, k);
        assert_eq!(to_text(&back), text);
        // a file of version 1 with one of the kinds, or with a missing element, is refused
        let old = text.replacen("phoenix-sketch 2", "phoenix-sketch 1", 1);
        let e = from_text(&old).unwrap_err().to_string();
        assert!(e.contains("needs a file of version 2"), "{e}");
        let wrong = text.replace("on symmetric_lines e8 e9 e3", "on symmetric_lines e10 e11");
        assert!(from_text(&wrong).is_err());
        let kind = text.replace(
            "on symmetric_circles e10 e11 e3",
            "on symmetric_circles e8 e11 e3",
        );
        assert!(from_text(&kind).is_err());
    }

    #[test]
    fn a_sketch_is_written_as_version_two_only_when_it_needs_the_newer_constraints() {
        // the first version's kinds only: version 1, as before
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(10), mm(0)).unwrap();
        let l = k.add_line(a, b).unwrap();
        k.add_constraint(Constraint::Horizontal(l)).unwrap();
        assert_eq!(version_of(&k), BASE_VERSION);
        assert!(to_text(&k).starts_with(
            "phoenix-sketch 1
"
        ));
        // a concentric constraint needs version 2, and it reads back the same
        let c1 = k.add_point(mm(20), mm(20)).unwrap();
        let c2 = k.add_point(mm(21), mm(20)).unwrap();
        let r1 = k.add_circle(c1, mm(3)).unwrap();
        let r2 = k.add_circle(c2, mm(5)).unwrap();
        k.add_constraint(Constraint::Concentric(r1, r2)).unwrap();
        assert_eq!(version_of(&k), FILE_VERSION);
        let text = to_text(&k);
        assert!(
            text.starts_with(
                "phoenix-sketch 2
"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "on concentric e6 e7
"
            ),
            "{text}"
        );
        let back = from_text(&text).unwrap();
        assert_eq!(back, k);
        assert_eq!(to_text(&back), text);
        // the same text with the header of version 1 is refused with the kind that needs 2,
        // and a file of a newer version than this program reads is refused with both versions
        let old = text.replacen("phoenix-sketch 2", "phoenix-sketch 1", 1);
        let e = from_text(&old).unwrap_err().to_string();
        assert!(e.contains("concentric needs a file of version 2"), "{e}");
        let newer = text.replacen("phoenix-sketch 2", "phoenix-sketch 3", 1);
        let e = from_text(&newer).unwrap_err().to_string();
        assert!(
            e.contains("version 3 and this program reads up to 2"),
            "{e}"
        );
        // a file of version 1 without the newer kinds is read as it always was, and so is the
        // same sketch written with the header of version 2
        let plain = "phoenix-sketch 1
name a
plane top 0 0 0
next 4 2
e1 point 0 0
e2 point 1 0
e3 line e1 e2
c1 on horizontal e3
";
        let one = from_text(plain).unwrap();
        let two = from_text(&plain.replacen("phoenix-sketch 1", "phoenix-sketch 2", 1)).unwrap();
        assert_eq!(one, two);
        assert_eq!(to_text(&two), plain);
    }
}
