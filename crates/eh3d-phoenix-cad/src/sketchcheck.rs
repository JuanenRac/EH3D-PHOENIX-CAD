// SPDX-License-Identifier: GPL-3.0-or-later
//! The check of a sketch before it is raised or cut, and the command `sketch-check`: what the
//! solver says (constraints that repeat or contradict others, no convergence) and what the drawing
//! hides (two points that look like one and are not joined, a line that is almost but not quite
//! horizontal or vertical, an end of the profile that meets no other end).
//!
//! The sketch is never changed: the solver works on a copy. The distance and the angle that count
//! as "almost" are `check_near` and `check_angle` of the settings of the sketch. The exit code is
//! the one of every check of the suite: 0 when there is nothing to report, 2 when there is, 1 when
//! the command could not run.

use std::path::PathBuf;

use crate::lang::{fill, Lang, Msg};
use crate::sketch::file as sketch_file;
use crate::sketch::settings::SketchSettings;
use crate::sketch::solver::Outcome;
use crate::sketch::{Constraint, ConstraintId, Element, ElementId, Sketch};
use crate::tools_cli::Args;
use crate::window::error_text;
use crate::CadError;

/// The commands of this module, for the program to know which words are theirs.
pub const COMMANDS: [&str; 1] = ["sketch-check"];

const NM_PER_MM: f64 = 1.0e6;

/// One thing to look at.
#[derive(Debug, Clone, PartialEq)]
pub enum Finding {
    /// Two points nearer than `check_near` that no constraint joins.
    Near {
        /// The first point.
        a: ElementId,
        /// The second point.
        b: ElementId,
        /// How far apart they are, in millimetres.
        distance: f64,
    },
    /// A line of the profile nearer than `check_angle` to horizontal without being so.
    Horizontal {
        /// The line.
        line: ElementId,
        /// How far from horizontal, in degrees.
        degrees: f64,
    },
    /// A line of the profile nearer than `check_angle` to vertical without being so.
    Vertical {
        /// The line.
        line: ElementId,
        /// How far from vertical, in degrees.
        degrees: f64,
    },
    /// An end of a line or an arc of the profile that meets no other end.
    OpenEnd {
        /// The point at the end.
        point: ElementId,
        /// The line or arc.
        element: ElementId,
    },
    /// A constraint that repeats others.
    Redundant(ConstraintId),
    /// A constraint that contradicts others.
    Conflicting(ConstraintId),
    /// The solver ran out of steps.
    NotConverged {
        /// The steps taken.
        iterations: u32,
        /// The largest error left, in millimetres.
        residual: f64,
    },
}

impl Finding {
    /// A stable word for the kind, the same in every language.
    pub fn word(&self) -> &'static str {
        match self {
            Finding::Near { .. } => "near",
            Finding::Horizontal { .. } => "horizontal",
            Finding::Vertical { .. } => "vertical",
            Finding::OpenEnd { .. } => "open-end",
            Finding::Redundant(_) => "redundant",
            Finding::Conflicting(_) => "conflicting",
            Finding::NotConverged { .. } => "not-converged",
        }
    }

    /// The code of the diagnostic when the finding is one a build would refuse; the hints about
    /// the drawing have none.
    pub fn code(&self) -> &'static str {
        match self {
            Finding::Redundant(_) | Finding::Conflicting(_) => "PHOENIX-CAD-033",
            Finding::NotConverged { .. } => "PHOENIX-CAD-034",
            Finding::OpenEnd { .. } => "PHOENIX-CAD-036",
            Finding::Near { .. } | Finding::Horizontal { .. } | Finding::Vertical { .. } => "",
        }
    }

    /// The finding as a line of comma separated values: `kind,subject,other,value,code`.
    pub fn row(&self) -> String {
        let (subject, other, value) = match self {
            Finding::Near { a, b, distance } => {
                (a.to_string(), b.to_string(), format!("{distance:.3}"))
            }
            Finding::Horizontal { line, degrees } | Finding::Vertical { line, degrees } => {
                (line.to_string(), String::new(), format!("{degrees:.2}"))
            }
            Finding::OpenEnd { point, element } => {
                (point.to_string(), element.to_string(), String::new())
            }
            Finding::Redundant(c) | Finding::Conflicting(c) => {
                (c.to_string(), String::new(), String::new())
            }
            Finding::NotConverged {
                iterations,
                residual,
            } => (
                iterations.to_string(),
                String::new(),
                format!("{residual:.6}"),
            ),
        };
        format!("{},{subject},{other},{value},{}", self.word(), self.code())
    }

    /// The finding as a sentence.
    pub fn text(&self, lang: Lang) -> String {
        let say = |m: Msg, values: &[&str]| fill(m.tr(lang), values);
        let sentence = match self {
            Finding::Near { a, b, distance } => say(
                Msg::CheckNear,
                &[&a.to_string(), &b.to_string(), &format!("{distance:.3}")],
            ),
            Finding::Horizontal { line, degrees } => say(
                Msg::CheckHorizontal,
                &[&line.to_string(), &format!("{degrees:.2}")],
            ),
            Finding::Vertical { line, degrees } => say(
                Msg::CheckVertical,
                &[&line.to_string(), &format!("{degrees:.2}")],
            ),
            Finding::OpenEnd { point, element } => say(
                Msg::CheckOpenEnd,
                &[&point.to_string(), &element.to_string()],
            ),
            Finding::Redundant(c) => say(Msg::CheckRedundant, &[&c.to_string()]),
            Finding::Conflicting(c) => say(Msg::CheckConflict, &[&c.to_string()]),
            Finding::NotConverged {
                iterations,
                residual,
            } => say(
                Msg::CheckNotConverged,
                &[&iterations.to_string(), &format!("{residual:.6}")],
            ),
        };
        match self.code() {
            "" => sentence,
            code => format!("{code} {sentence}"),
        }
    }
}

/// What the check of a sketch found.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// The things to look at: the ones of the solver first, then the ones of the drawing.
    pub findings: Vec<Finding>,
    /// The degrees of freedom left, when the solver found a solution.
    pub freedom: Option<usize>,
}

/// The point that stands for the group of points joined to `id`.
fn root(joined: &[(ElementId, usize)], parents: &mut [usize], id: ElementId) -> Option<usize> {
    let mut at = joined.iter().find(|(p, _)| *p == id)?.1;
    while parents[at] != at {
        parents[at] = parents[parents[at]];
        at = parents[at];
    }
    Some(at)
}

fn centre_of(sketch: &Sketch, id: ElementId) -> Option<ElementId> {
    match sketch.element(id)? {
        Element::Circle { center, .. } | Element::Arc { center, .. } => Some(*center),
        _ => None,
    }
}

/// The points of the sketch with their places in millimetres.
fn points(sketch: &Sketch) -> Vec<(ElementId, f64, f64)> {
    sketch
        .elements()
        .iter()
        .filter_map(|e| match e.element {
            Element::Point { x, y } => {
                Some((e.id, x.get() as f64 / NM_PER_MM, y.get() as f64 / NM_PER_MM))
            }
            _ => None,
        })
        .collect()
}

/// Pairs of points nearer than `near` millimetres that no coincidence or concentricity joins.
fn near_points(sketch: &Sketch, near: f64) -> Vec<Finding> {
    let all = points(sketch);
    let index: Vec<(ElementId, usize)> = all.iter().enumerate().map(|(i, p)| (p.0, i)).collect();
    let mut parents: Vec<usize> = (0..all.len()).collect();
    for entry in sketch.constraints().iter().filter(|c| c.enabled) {
        let pair = match entry.constraint {
            Constraint::Coincident(a, b) => Some((a, b)),
            Constraint::Concentric(a, b) => centre_of(sketch, a).zip(centre_of(sketch, b)),
            _ => None,
        };
        if let Some((a, b)) = pair {
            let first = root(&index, &mut parents, a);
            let second = root(&index, &mut parents, b);
            if let Some((x, y)) = first.zip(second) {
                parents[x] = y;
            }
        }
    }
    let mut out = Vec::new();
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            let distance = (a.1 - b.1).hypot(a.2 - b.2);
            if distance <= near
                && root(&index, &mut parents, a.0) != root(&index, &mut parents, b.0)
            {
                out.push(Finding::Near {
                    a: a.0,
                    b: b.0,
                    distance,
                });
            }
        }
    }
    out
}

/// The lines of the profile that are almost horizontal or vertical without being constrained.
fn almost_square(sketch: &Sketch, limit: f64) -> Vec<Finding> {
    let held = |line: ElementId| {
        sketch.constraints().iter().any(|c| {
            c.enabled
                && matches!(
                    c.constraint,
                    Constraint::Horizontal(..)
                        | Constraint::Vertical(..)
                        | Constraint::LineAngle { .. }
                )
                && c.constraint.elements().contains(&line)
        })
    };
    let mut out = Vec::new();
    for entry in sketch.elements().iter().filter(|e| !e.construction) {
        let Element::Line { a, b } = entry.element else {
            continue;
        };
        let (Ok(p), Ok(q)) = (sketch.point(a), sketch.point(b)) else {
            continue;
        };
        let dx = (q.0.get() - p.0.get()).unsigned_abs() as f64;
        let dy = (q.1.get() - p.1.get()).unsigned_abs() as f64;
        if (dx == 0.0 && dy == 0.0) || held(entry.id) {
            continue;
        }
        let from_horizontal = dy.atan2(dx).to_degrees();
        let from_vertical = dx.atan2(dy).to_degrees();
        if from_horizontal > 0.0 && from_horizontal <= limit {
            out.push(Finding::Horizontal {
                line: entry.id,
                degrees: from_horizontal,
            });
        } else if from_vertical > 0.0 && from_vertical <= limit {
            out.push(Finding::Vertical {
                line: entry.id,
                degrees: from_vertical,
            });
        }
    }
    out
}

/// The ends of the lines and arcs of the profile that meet no end of another one (within the
/// join of the settings, on each axis, as the outlines are made).
fn open_ends(sketch: &Sketch, join: f64) -> Vec<Finding> {
    let mut ends = Vec::new();
    for entry in sketch.elements().iter().filter(|e| !e.construction) {
        let pair = match entry.element {
            Element::Line { a, b } => [a, b],
            Element::Arc { start, end, .. } => [start, end],
            _ => continue,
        };
        for point in pair {
            if let Ok((x, y)) = sketch.point(point) {
                let place = (x.get() as f64 / NM_PER_MM, y.get() as f64 / NM_PER_MM);
                ends.push((point, entry.id, place));
            }
        }
    }
    ends.iter()
        .filter(|(_, element, p)| {
            !ends.iter().any(|(_, other, q)| {
                other != element && (p.0 - q.0).abs() <= join && (p.1 - q.1).abs() <= join
            })
        })
        .map(|(point, element, _)| Finding::OpenEnd {
            point: *point,
            element: *element,
        })
        .collect()
}

/// Checks a sketch with the limits of the settings.
pub fn check(sketch: &Sketch, s: &SketchSettings) -> Result<Report, CadError> {
    let mut findings = Vec::new();
    let mut freedom = None;
    match sketch.clone().solve(s)? {
        Outcome::Solved { dof, .. } => freedom = Some(dof),
        Outcome::OverConstrained {
            redundant,
            conflicting,
        } => {
            findings.extend(conflicting.into_iter().map(Finding::Conflicting));
            findings.extend(redundant.into_iter().map(Finding::Redundant));
        }
        Outcome::NotConverged {
            iterations,
            residual,
        } => findings.push(Finding::NotConverged {
            iterations,
            residual,
        }),
    }
    findings.extend(near_points(sketch, s.check_near));
    findings.extend(almost_square(sketch, s.check_angle));
    findings.extend(open_ends(sketch, s.profile_join));
    Ok(Report { findings, freedom })
}

/// Runs the command `sketch-check`: the text it prints and whether there was nothing to report.
pub fn run(command: &str, args: &[String]) -> Result<(String, bool), String> {
    if !COMMANDS.contains(&command) {
        return Err(format!("{command}: unknown command (see help)"));
    }
    let a = Args::parse(args, &["--config"], &["--text"])?;
    let path = PathBuf::from(a.only_word("sketch file")?);
    let settings = match a.value("--config") {
        Some(c) => {
            let text = std::fs::read_to_string(c).map_err(|e| format!("{c}: {e}"))?;
            SketchSettings::with_user_config(&text).map_err(|e| format!("{c}: {e}"))?
        }
        None => SketchSettings::defaults(),
    };
    let say = |e: CadError| format!("{}: {}", path.display(), error_text(&e, a.lang));
    let sketch = sketch_file::read_file(&path, &settings).map_err(say)?;
    let report = check(&sketch, &settings).map_err(say)?;
    let clean = report.findings.is_empty();
    let mut out = String::new();
    if a.has("--text") {
        for finding in &report.findings {
            out.push_str(&finding.text(a.lang));
            out.push('\n');
        }
        if clean {
            let freedom = report.freedom.unwrap_or(0).to_string();
            out.push_str(&fill(Msg::CheckClean.tr(a.lang), &[&freedom]));
            out.push('\n');
        }
    } else {
        out.push_str("kind,subject,other,value,code\n");
        for finding in &report.findings {
            out.push_str(&finding.row());
            out.push('\n');
        }
        if let Some(freedom) = report.freedom {
            out.push_str(&format!("freedom,,,{freedom},\n"));
        }
    }
    Ok((out, clean))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eh3dfile::tests::{Scratch, Seeded};
    use crate::enclosure::Face;
    use crate::sketch::{Dim, SketchPlane};
    use crate::Length;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn um(v: i64) -> Length {
        Length::new(v * 1_000).unwrap()
    }

    fn plate() -> Sketch {
        let mut s = Sketch::new("plate", SketchPlane::on_face(Face::Top)).unwrap();
        s.add_rectangle((mm(0), mm(0)), (mm(20), mm(10))).unwrap();
        s
    }

    fn words(sketch: &Sketch) -> Vec<&'static str> {
        let report = check(sketch, &SketchSettings::defaults()).unwrap();
        report.findings.iter().map(Finding::word).collect()
    }

    #[test]
    fn a_closed_rectangle_has_nothing_to_report() {
        let report = check(&plate(), &SketchSettings::defaults()).unwrap();
        assert!(report.findings.is_empty(), "{report:?}");
        assert!(report.freedom.is_some());
    }

    #[test]
    fn a_point_that_looks_joined_is_named_until_a_coincidence_joins_it() {
        let mut s = plate();
        let corner = s.elements()[0].id;
        let extra = s.add_point(um(50), mm(0)).unwrap();
        let report = check(&s, &SketchSettings::defaults()).unwrap();
        match report.findings.as_slice() {
            [Finding::Near { a, b, distance }] => {
                assert_eq!((*a, *b), (corner, extra));
                assert!((distance - 0.05).abs() < 1e-9);
            }
            other => panic!("{other:?}"),
        }
        s.add_constraint(Constraint::Coincident(corner, extra))
            .unwrap();
        assert!(!words(&s).contains(&"near"));
    }

    #[test]
    fn a_line_that_is_almost_straight_is_named_and_a_construction_line_is_not() {
        let mut s = plate();
        let a = s.add_point(mm(30), mm(0)).unwrap();
        let b = s.add_point(mm(40), um(100)).unwrap();
        let line = s.add_line(a, b).unwrap();
        assert_eq!(words(&s), ["horizontal", "open-end", "open-end"]);
        s.set_construction(line, true).unwrap();
        assert!(words(&s).is_empty());
        s.set_construction(line, false).unwrap();
        s.move_point(b, um(30_100), mm(10)).unwrap();
        assert_eq!(words(&s), ["vertical", "open-end", "open-end"]);
        // a line at a clear angle is not "almost" anything
        s.move_point(b, mm(35), mm(5)).unwrap();
        assert_eq!(words(&s), ["open-end", "open-end"]);
    }

    #[test]
    fn contradicting_dimensions_are_listed_with_their_code_and_the_sketch_is_not_changed() {
        let mut s = plate();
        let line = s.elements()[4].id;
        for length in [20, 30] {
            s.add_constraint(Constraint::LineLength {
                line,
                dim: Dim::fixed(mm(length)),
            })
            .unwrap();
        }
        let before = s.clone();
        let report = check(&s, &SketchSettings::defaults()).unwrap();
        assert_eq!(s, before);
        assert!(report.freedom.is_none());
        let first = &report.findings[0];
        assert_eq!(first.word(), "conflicting");
        assert_eq!(first.code(), "PHOENIX-CAD-033");
        assert!(first.row().ends_with(",PHOENIX-CAD-033"), "{}", first.row());
        for l in Lang::ALL {
            assert!(first.text(l).starts_with("PHOENIX-CAD-033 "), "{l:?}");
        }
    }

    #[test]
    fn the_command_reads_a_sketch_file_and_exits_with_the_findings() {
        let dir = Scratch::new("sketch-check");
        let clean = dir.path("plate.phoenixsketch");
        std::fs::write(&clean, sketch_file::to_text(&plate())).unwrap();
        let call = |words: &[&str]| {
            let args: Vec<String> = words.iter().map(|w| (*w).to_owned()).collect();
            run("sketch-check", &args)
        };
        let path = clean.to_str().unwrap();
        let (out, ok) = call(&[path]).unwrap();
        assert!(ok, "{out}");
        assert!(
            out.starts_with("kind,subject,other,value,code\nfreedom,,,"),
            "{out}"
        );
        let (out, ok) = call(&[path, "--text", "--lang", "es"]).unwrap();
        assert!(ok && out.starts_with("Nada que señalar"), "{out}");
        let mut open = plate();
        let a = open.add_point(mm(50), mm(0)).unwrap();
        let b = open.add_point(mm(60), mm(7)).unwrap();
        open.add_line(a, b).unwrap();
        let broken = dir.path("open.phoenixsketch");
        std::fs::write(&broken, sketch_file::to_text(&open)).unwrap();
        let (out, ok) = call(&[broken.to_str().unwrap()]).unwrap();
        assert!(!ok, "{out}");
        assert_eq!(out.matches("open-end,").count(), 2, "{out}");
        assert!(out.contains(",PHOENIX-CAD-036\n"), "{out}");
        assert!(call(&["no.phoenixsketch"]).is_err());
        assert!(run("sketch-nothing", &[]).is_err());
        // the limits are the ones of the settings
        let config = dir.path("tight.cfg");
        std::fs::write(&config, "[sketch]\ncheck_angle = 40\n").unwrap();
        let (out, _) = call(&[
            broken.to_str().unwrap(),
            "--config",
            config.to_str().unwrap(),
        ])
        .unwrap();
        assert!(out.contains("horizontal,"), "{out}");
    }

    #[test]
    fn random_drawings_never_panic_and_say_the_same_twice() {
        let mut random = Seeded(0x00CA_DB28);
        let settings = SketchSettings::defaults();
        for _ in 0..40 {
            let mut s = Sketch::new("r", SketchPlane::on_face(Face::Top)).unwrap();
            let mut ids = Vec::new();
            for _ in 0..2 + random.below(8) {
                let x = um(random.below(200) as i64);
                let y = um(random.below(200) as i64);
                ids.push(s.add_point(x, y).unwrap());
            }
            for pair in ids.windows(2) {
                if random.below(2) == 0 && s.point(pair[0]).unwrap() != s.point(pair[1]).unwrap() {
                    s.add_line(pair[0], pair[1]).unwrap();
                }
            }
            let first = check(&s, &settings).unwrap();
            assert_eq!(first, check(&s, &settings).unwrap());
        }
    }
}
