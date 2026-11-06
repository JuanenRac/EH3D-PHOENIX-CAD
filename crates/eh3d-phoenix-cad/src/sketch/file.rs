//! The sketch file (`.phoenixsketch`): one sketch in plain text, kept next to the project file.
//!
//! The file has its own version in its first line and does not touch the format of the project
//! file. One line for the header, one for the name, one for the plane, one for the counters of
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

/// The version of the format that this program writes and reads.
pub const FILE_VERSION: u32 = 1;

/// The extension of a sketch file.
pub const EXTENSION: &str = "phoenixsketch";

const HEADER: &str = "phoenix-sketch";

fn bad(line: usize, why: impl AsRef<str>) -> CadError {
    CadError::SketchFile(format!("line {line}: {}", why.as_ref()))
}

/// The path of the sketch file that goes with a project file: the same name with the extension of
/// the sketch.
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
        | Constraint::Equal(a, b) => format!("{name} {a} {b}"),
        Constraint::PointOnObject { point, object } => format!("{name} {point} {object}"),
        Constraint::Horizontal(l) | Constraint::Vertical(l) => format!("{name} {l}"),
        Constraint::SymmetricLine { a, b, axis } => format!("{name} {a} {b} {axis}"),
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
    }
}

/// The text of a sketch.
pub fn to_text(sketch: &Sketch) -> String {
    let mut out = String::new();
    let plane = sketch.plane();
    let (next_e, next_c) = sketch.next_ids();
    let _ = writeln!(out, "{HEADER} {FILE_VERSION}");
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
        "distance" | "distance_x" | "distance_y" | "length" | "radius" | "diameter" | "angle"
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
        | "tangent" | "equal" | "point_on_object" | "midpoint" => {
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
        "symmetric_line" => {
            exactly(line, kind, words, 3)?;
            Constraint::SymmetricLine {
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
            let version = words
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
            ("phoenix-sketch 2\n".into(), "version 2"),
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
}
