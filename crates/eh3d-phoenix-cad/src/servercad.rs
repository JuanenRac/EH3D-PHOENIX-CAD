// SPDX-License-Identifier: GPL-3.0-or-later
//! The project of this program as the plugin `cad` of the EH3D server reads it, and the answers of
//! that plugin.
//!
//! The server keeps no geometry kernel for this plugin: its station reads the *structure* of a
//! project (the sketches, the features made from them or working on one another, and the parts) and
//! counts and checks that. This module writes that structure from the project file
//! ([`project_json`], the contract `eh3d.cad.project/1`), checks it with the same rules as the
//! station ([`check_json`], so a project can be checked here before anything is sent and the answer
//! compared with the station's), reads the answers of a `check` or a `report` job ([`parse_answer`])
//! and, for the store in memory of the tests, answers a job the way the station does
//! ([`station_answer`]).
//!
//! Only the steps that are made from a sketch are features of the contract: an extrusion (`extrude`),
//! a pocket (`pocket`), a turned body (`revolve`) and a swept body (`sweep`, which the station does
//! not know and says so with a warning). Holes, posts, the lid, edgings, fins and patterns are
//! parameters of the enclosure, not features of the contract. A suppressed step, and the steps after
//! the tip, are not in the project that is sent.

use std::collections::BTreeMap;

use eh3d_serverclient::json::quote;
use eh3d_serverclient::Json;

use crate::changes::unescape_word;
use crate::project::Document;
use crate::CadError;

/// The `schema` of the project the station reads.
pub const PROJECT_SCHEMA: &str = "eh3d.cad.project/1";
/// The `schema` of the answers of the station.
pub const REPORT_SCHEMA: &str = "eh3d.cad.report/1";
/// The file of the project store that holds the project as the station reads it.
pub const CONTRACT_FILE: &str = "project.json";
/// The kinds of feature made from a sketch.
const FROM_SKETCH: [&str; 4] = ["extrude", "revolve", "pocket", "hole"];
/// The kinds of feature that work on another feature.
const ON_FEATURE: [&str; 4] = ["fillet", "chamfer", "mirror", "pattern"];

/// One thing the check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// `error` or `warning`.
    pub severity: String,
    /// The code: `bad_schema`, `unknown_units`, `missing_id`, `duplicate_id`, `missing_ref`,
    /// `unknown_kind` or `empty_part`.
    pub code: String,
    /// The id the issue is about (empty when it is about the file).
    pub id: String,
    /// The sentence the station gave it.
    pub message: String,
}

/// The result of a check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// True while there is no error.
    pub pass: bool,
    /// The number of errors.
    pub errors: usize,
    /// The number of warnings.
    pub warnings: usize,
    /// Everything it found, in the order the check found it.
    pub issues: Vec<Issue>,
}

/// The counts of a `report`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counts {
    /// The parts.
    pub parts: usize,
    /// The sketches.
    pub sketches: usize,
    /// The features.
    pub features: usize,
}

/// The answer of a job of the station.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// `check` or `report`.
    pub kind: String,
    /// The verdict.
    pub verdict: Verdict,
    /// The counts of a report.
    pub counts: Option<Counts>,
    /// What the station says its evidence is (`simulator`).
    pub evidence: String,
}

fn bad(why: &str) -> CadError {
    CadError::Format(why.to_string())
}

/// The sketches of the domain `sketch`, by name (the line `name` that follows each header).
fn sketch_names(doc: &Document) -> Vec<String> {
    let Some(domain) = doc.get("sketch") else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&domain.data);
    let mut out = Vec::new();
    let mut after_header = false;
    for line in text.lines() {
        if after_header {
            if let Some(name) = line.strip_prefix("name ") {
                out.push(name.to_string());
            }
        }
        after_header = line.starts_with("phoenix-sketch ");
    }
    out
}

/// A feature of the contract.
struct Feature {
    id: String,
    kind: &'static str,
    sketch: String,
}

/// A part of the contract.
struct PartOut {
    id: String,
    name: String,
    features: Vec<String>,
}

/// The project file as the station reads it (`eh3d.cad.project/1`), as JSON text. `name` is the name
/// the project has on the server. A project with no mechanical domain has no features and no parts.
pub fn project_json(doc: &Document, name: &str) -> Result<String, CadError> {
    let units = match doc.project_units()? {
        Some(u) if matches!(u.unit.suffix(), "in" | "mil") => "in",
        _ => "mm",
    };
    let mut features: Vec<Feature> = Vec::new();
    let mut parts: Vec<PartOut> = Vec::new();
    if let Some(domain) = doc.get("mechanical") {
        let text = std::str::from_utf8(&domain.data)
            .map_err(|_| bad("the mechanical domain is not text"))?;
        let mut in_steps = false;
        let mut tip: Option<usize> = None;
        let mut steps: Vec<Vec<&str>> = Vec::new();
        for line in text.lines() {
            if line.starts_with('[') {
                in_steps = line == "[steps]";
                continue;
            }
            if !in_steps {
                continue;
            }
            let words: Vec<&str> = line.split(' ').collect();
            match words.as_slice() {
                ["tip", n] => tip = n.parse().ok(),
                ["step", ..] => steps.push(words),
                _ => {}
            }
        }
        let active = tip.unwrap_or(steps.len()).min(steps.len());
        let mut enclosure: Vec<String> = Vec::new();
        for w in steps.iter().take(active) {
            if w.get(1) == Some(&"1") || w.len() < 5 {
                continue;
            }
            let id = w[4].to_string();
            match w[3] {
                "extrude" | "pocket" if w.len() > 5 => {
                    let kind = if w[3] == "extrude" {
                        "extrude"
                    } else {
                        "pocket"
                    };
                    enclosure.push(id.clone());
                    features.push(Feature {
                        id,
                        kind,
                        sketch: w[5].to_string(),
                    });
                }
                "add-turned" | "add-swept" if w.len() > 6 => {
                    let kind = if w[3] == "add-turned" {
                        "revolve"
                    } else {
                        "sweep"
                    };
                    parts.push(PartOut {
                        id: id.clone(),
                        name: unescape_word(w[5]),
                        features: vec![id.clone()],
                    });
                    features.push(Feature {
                        id,
                        kind,
                        sketch: w[6].to_string(),
                    });
                }
                "add-part" if w.len() > 5 => parts.push(PartOut {
                    id,
                    name: unescape_word(w[5]),
                    features: Vec::new(),
                }),
                _ => {}
            }
        }
        if !enclosure.is_empty() {
            parts.insert(
                0,
                PartOut {
                    id: "enclosure".to_string(),
                    name: name.to_string(),
                    features: enclosure,
                },
            );
        }
    }
    let sketches: Vec<String> = sketch_names(doc)
        .iter()
        .map(|s| format!("{{\"id\":{}}}", quote(s)))
        .collect();
    let features: Vec<String> = features
        .iter()
        .map(|f| {
            format!(
                "{{\"id\":{},\"kind\":{},\"sketch\":{}}}",
                quote(&f.id),
                quote(f.kind),
                quote(&f.sketch)
            )
        })
        .collect();
    let parts: Vec<String> = parts
        .iter()
        .map(|p| {
            let list: Vec<String> = p.features.iter().map(|f| quote(f)).collect();
            format!(
                "{{\"id\":{},\"name\":{},\"features\":[{}]}}",
                quote(&p.id),
                quote(&p.name),
                list.join(",")
            )
        })
        .collect();
    Ok(format!(
        "{{\"schema\":{},\"name\":{},\"units\":{},\"sketches\":[{}],\"features\":[{}],\"parts\":[{}]}}",
        quote(PROJECT_SCHEMA),
        quote(name),
        quote(units),
        sketches.join(","),
        features.join(","),
        parts.join(",")
    ))
}

fn list<'a>(project: &'a Json, key: &str) -> Vec<&'a Json> {
    project
        .get(key)
        .and_then(Json::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

fn id_of(item: &Json) -> Option<&str> {
    item.get("id")
        .and_then(Json::as_str)
        .filter(|s| !s.is_empty())
}

fn issue(severity: &str, code: &str, id: &str, message: String) -> Issue {
    Issue {
        severity: severity.to_string(),
        code: code.to_string(),
        id: id.to_string(),
        message,
    }
}

/// Checks the structure of a project with the rules of the station: the schema is right, the units
/// are `mm` or `in`, the ids of each list are there and different, a feature points to a sketch or
/// to another feature that is there, a part to features that are there; a kind the station does not
/// know and a part with no feature are warnings.
pub fn check_json(project: &Json) -> Verdict {
    let mut issues: Vec<Issue> = Vec::new();
    if project.get("schema").and_then(Json::as_str) != Some(PROJECT_SCHEMA) {
        issues.push(issue(
            "error",
            "bad_schema",
            "",
            format!("schema is {PROJECT_SCHEMA}"),
        ));
    }
    if let Some(units) = project.get("units") {
        if !matches!(units.as_str(), Some("mm" | "in")) {
            issues.push(issue(
                "error",
                "unknown_units",
                "",
                "units is mm or in".to_string(),
            ));
        }
    }
    let mut ids: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for key in ["sketches", "features", "parts"] {
        let seen = ids.entry(key).or_default();
        for item in list(project, key) {
            match id_of(item) {
                None => issues.push(issue(
                    "error",
                    "missing_id",
                    "",
                    format!("an item of {key} has no id"),
                )),
                Some(id) => {
                    if seen.contains(&id) {
                        issues.push(issue(
                            "error",
                            "duplicate_id",
                            id,
                            format!("{id} is twice in {key}"),
                        ));
                    } else {
                        seen.push(id);
                    }
                }
            }
        }
    }
    let none: Vec<&str> = Vec::new();
    let sketches = ids.get("sketches").unwrap_or(&none);
    let features = ids.get("features").unwrap_or(&none);
    for f in list(project, "features") {
        let id = id_of(f).unwrap_or("");
        let kind = f.get("kind").and_then(Json::as_str).unwrap_or("");
        if FROM_SKETCH.contains(&kind) {
            match f.get("sketch").and_then(Json::as_str) {
                Some(s) if sketches.contains(&s) => {}
                other => issues.push(issue(
                    "error",
                    "missing_ref",
                    id,
                    format!(
                        "{id} ({kind}) points to the sketch {} and there is none",
                        other.unwrap_or("(none)")
                    ),
                )),
            }
        } else if ON_FEATURE.contains(&kind) {
            match f.get("target").and_then(Json::as_str) {
                Some(t) if features.contains(&t) && t != id => {}
                other => issues.push(issue(
                    "error",
                    "missing_ref",
                    id,
                    format!(
                        "{id} ({kind}) works on the feature {} and there is none",
                        other.unwrap_or("(none)")
                    ),
                )),
            }
        } else {
            issues.push(issue(
                "warning",
                "unknown_kind",
                id,
                format!("{id} is of the kind {kind:?}, which the station does not know"),
            ));
        }
    }
    for p in list(project, "parts") {
        let id = id_of(p).unwrap_or("");
        let used: Vec<&str> = p
            .get("features")
            .and_then(Json::as_array)
            .map(|a| a.iter().filter_map(Json::as_str).collect())
            .unwrap_or_default();
        if used.is_empty() {
            issues.push(issue(
                "warning",
                "empty_part",
                id,
                format!("the part {id} has no feature"),
            ));
        }
        for f in used {
            if !features.contains(&f) {
                issues.push(issue(
                    "error",
                    "missing_ref",
                    id,
                    format!("the part {id} uses the feature {f} and there is none"),
                ));
            }
        }
    }
    let errors = issues.iter().filter(|i| i.severity == "error").count();
    let warnings = issues.len() - errors;
    Verdict {
        pass: errors == 0,
        errors,
        warnings,
        issues,
    }
}

fn check_text(v: &Verdict) -> String {
    let issues: Vec<String> = v
        .issues
        .iter()
        .map(|i| {
            format!(
                "{{\"severity\":{},\"code\":{},\"id\":{},\"message\":{}}}",
                quote(&i.severity),
                quote(&i.code),
                quote(&i.id),
                quote(&i.message)
            )
        })
        .collect();
    format!(
        "{{\"verdict\":{},\"errors\":{},\"warnings\":{},\"issues\":[{}]}}",
        quote(if v.pass { "pass" } else { "fail" }),
        v.errors,
        v.warnings,
        issues.join(",")
    )
}

/// The document the station gives for a `check` or a `report` of a project: the same keys, the same
/// counts and the same evidence, so that the store in memory of the tests answers as the server does.
pub fn station_answer(kind: &str, project: &Json) -> Result<String, String> {
    if !matches!(kind, "check" | "report") {
        return Err("kind is check or report".to_string());
    }
    if !matches!(project, Json::Obj(_)) {
        return Err("the project is a JSON object".to_string());
    }
    let verdict = check_json(project);
    if kind == "check" {
        return Ok(format!(
            "{{\"schema\":{},\"kind\":\"check\",\"check\":{},\"evidence\":\"simulator\"}}",
            quote(REPORT_SCHEMA),
            check_text(&verdict)
        ));
    }
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for f in list(project, "features") {
        let k = f.get("kind").and_then(Json::as_str).unwrap_or("unknown");
        *by_kind.entry(k.to_string()).or_default() += 1;
    }
    let by_kind: Vec<String> = by_kind
        .iter()
        .map(|(k, n)| format!("{}:{n}", quote(k)))
        .collect();
    let parts: Vec<String> = list(project, "parts")
        .into_iter()
        .map(|p| {
            format!(
                "{{\"id\":{},\"name\":{},\"features\":{}}}",
                quote(id_of(p).unwrap_or("")),
                quote(p.get("name").and_then(Json::as_str).unwrap_or("")),
                p.get("features")
                    .and_then(Json::as_array)
                    .map_or(0, <[Json]>::len)
            )
        })
        .collect();
    Ok(format!(
        "{{\"schema\":{},\"kind\":\"report\",\"name\":{},\"units\":{},\"counts\":{{\"parts\":{},\"sketches\":{},\"features\":{},\"features_by_kind\":{{{}}}}},\"parts\":[{}],\"check\":{},\"evidence\":\"simulator\"}}",
        quote(REPORT_SCHEMA),
        quote(project.get("name").and_then(Json::as_str).unwrap_or("")),
        quote(project.get("units").and_then(Json::as_str).unwrap_or("mm")),
        list(project, "parts").len(),
        list(project, "sketches").len(),
        list(project, "features").len(),
        by_kind.join(","),
        parts.join(","),
        check_text(&verdict)
    ))
}

fn count_of(v: &Json, key: &str) -> Option<usize> {
    v.get(key)
        .and_then(Json::as_u64)
        .and_then(|n| usize::try_from(n).ok())
}

/// Reads the document a job of the station gave (the `result` of the view of the job). A document
/// that does not have the keys of the contract is refused with what is missing.
pub fn parse_answer(result: &Json) -> Result<Answer, String> {
    let text = |v: &Json, key: &str| -> Result<String, String> {
        v.get(key)
            .and_then(Json::as_str)
            .map(str::to_string)
            .ok_or_else(|| key.to_string())
    };
    if result.get("schema").and_then(Json::as_str) != Some(REPORT_SCHEMA) {
        return Err("schema".to_string());
    }
    let kind = text(result, "kind")?;
    let check = result.get("check").ok_or_else(|| "check".to_string())?;
    let pass = match text(check, "verdict")?.as_str() {
        "pass" => true,
        "fail" => false,
        _ => return Err("verdict".to_string()),
    };
    let mut issues = Vec::new();
    for i in check
        .get("issues")
        .and_then(Json::as_array)
        .ok_or_else(|| "issues".to_string())?
    {
        issues.push(Issue {
            severity: text(i, "severity")?,
            code: text(i, "code")?,
            id: text(i, "id")?,
            message: text(i, "message")?,
        });
    }
    let counts = match result.get("counts") {
        Some(c) => Some(Counts {
            parts: count_of(c, "parts").ok_or_else(|| "counts.parts".to_string())?,
            sketches: count_of(c, "sketches").ok_or_else(|| "counts.sketches".to_string())?,
            features: count_of(c, "features").ok_or_else(|| "counts.features".to_string())?,
        }),
        None => None,
    };
    Ok(Answer {
        kind,
        verdict: Verdict {
            pass,
            errors: count_of(check, "errors").ok_or_else(|| "errors".to_string())?,
            warnings: count_of(check, "warnings").ok_or_else(|| "warnings".to_string())?,
            issues,
        },
        counts,
        evidence: text(result, "evidence")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    fn example(name: &str) -> Document {
        let bytes = std::fs::read(format!(
            "{}/examples/{name}.phoenixproj",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        Document::from_bytes(&bytes, Settings::defaults().mechanical.max_bytes).unwrap()
    }

    fn json_of(doc: &Document, name: &str) -> Json {
        Json::parse(&project_json(doc, name).unwrap()).unwrap()
    }

    fn codes(v: &Verdict) -> Vec<(&str, &str)> {
        v.issues
            .iter()
            .map(|i| (i.severity.as_str(), i.code.as_str()))
            .collect()
    }

    #[test]
    fn a_project_with_a_sketch_and_an_extrusion_is_written_in_the_contract_and_passes() {
        let doc = example("plate");
        let json = json_of(&doc, "plate");
        assert_eq!(
            json.get("schema").and_then(Json::as_str),
            Some(PROJECT_SCHEMA)
        );
        assert_eq!(json.get("units").and_then(Json::as_str), Some("mm"));
        assert_eq!(list(&json, "sketches").len(), 1);
        assert_eq!(id_of(list(&json, "sketches")[0]), Some("plate"));
        let features = list(&json, "features");
        assert!(!features.is_empty());
        for f in &features {
            assert!(matches!(
                f.get("kind").and_then(Json::as_str),
                Some("extrude" | "pocket")
            ));
            assert_eq!(f.get("sketch").and_then(Json::as_str), Some("plate"));
        }
        // the features of the enclosure are the part named like the project
        let parts = list(&json, "parts");
        assert_eq!(id_of(parts[0]), Some("enclosure"));
        assert_eq!(parts[0].get("name").and_then(Json::as_str), Some("plate"));
        let v = check_json(&json);
        assert!(v.pass, "{v:?}");
        assert_eq!(v.errors, 0);
    }

    #[test]
    fn a_box_with_holes_has_no_feature_of_the_contract_and_the_placed_parts_are_parts_with_a_warning(
    ) {
        let json = json_of(&example("box"), "box");
        assert!(list(&json, "features").is_empty());
        assert!(list(&json, "parts").is_empty());
        assert!(check_json(&json).pass);
        let json = json_of(&example("assembly"), "assembly");
        let parts = list(&json, "parts");
        assert_eq!(parts.len(), 6);
        let v = check_json(&json);
        assert!(v.pass, "{v:?}");
        assert_eq!(v.errors, 0);
        assert_eq!(v.warnings, 6);
        assert!(v.issues.iter().all(|i| i.code == "empty_part"));
        // the names of the parts are the names of the steps, read back from the file
        assert_eq!(
            parts[0].get("name").and_then(Json::as_str),
            Some("base rail")
        );
    }

    #[test]
    fn the_units_of_the_project_become_mm_or_in() {
        use crate::project::ProjectUnits;
        use crate::units::Unit;
        let mut doc = example("box");
        for (unit, expected) in [
            (Unit::Inch, "in"),
            (Unit::Mil, "in"),
            (Unit::Millimetre, "mm"),
            (Unit::Metre, "mm"),
            (Unit::Micrometre, "mm"),
        ] {
            doc.set_project_units(Some(ProjectUnits { unit, decimals: 3 }))
                .unwrap();
            let json = json_of(&doc, "box");
            assert_eq!(
                json.get("units").and_then(Json::as_str),
                Some(expected),
                "{unit:?}"
            );
        }
    }

    #[test]
    fn a_suppressed_step_and_the_steps_after_the_tip_are_not_sent() {
        let doc = example("plate");
        let s = Settings::defaults();
        let mut m = doc.mechanical(&s.expr, &s.mechanical).unwrap().unwrap();
        let all = list(&json_of(&doc, "plate"), "features").len();
        assert!(all >= 1);
        let mut with = doc.clone();
        let steps = m.steps.len();
        m.steps[steps - 1].suppressed = true;
        with.set_mechanical(&m, &s.mechanical).unwrap();
        let fewer = list(&json_of(&with, "plate"), "features").len();
        assert!(fewer < all, "{fewer} {all}");
        m.steps[steps - 1].suppressed = false;
        m.tip = 0;
        with.set_mechanical(&m, &s.mechanical).unwrap();
        assert!(list(&json_of(&with, "plate"), "features").is_empty());
    }

    #[test]
    fn the_check_finds_what_the_station_finds() {
        let parse = |t: &str| Json::parse(t).unwrap();
        let v = check_json(&parse(
            r#"{"schema":"x","units":"cm","sketches":[{"id":"a"},{"id":"a"},{}],"features":[{"id":"f1","kind":"extrude","sketch":"nope"},{"id":"f2","kind":"fillet","target":"f2"},{"id":"f3","kind":"loft"}],"parts":[{"id":"p","features":["zz"]},{"id":"q","features":[]}]}"#,
        ));
        assert!(!v.pass);
        assert_eq!(
            codes(&v),
            vec![
                ("error", "bad_schema"),
                ("error", "unknown_units"),
                ("error", "duplicate_id"),
                ("error", "missing_id"),
                ("error", "missing_ref"),
                ("error", "missing_ref"),
                ("warning", "unknown_kind"),
                ("error", "missing_ref"),
                ("warning", "empty_part"),
            ]
        );
        assert_eq!((v.errors, v.warnings), (7, 2));
        // a feature of kind hole points to a sketch and a mirror to a feature
        let v = check_json(&parse(
            r#"{"schema":"eh3d.cad.project/1","sketches":[{"id":"s"}],"features":[{"id":"h","kind":"hole","sketch":"s"},{"id":"m","kind":"mirror","target":"h"}],"parts":[{"id":"p","features":["h","m"]}]}"#,
        ));
        assert!(v.pass && v.issues.is_empty(), "{v:?}");
    }

    #[test]
    fn the_answer_of_a_station_is_read_back_and_a_wrong_one_is_refused_with_what_is_missing() {
        let json = json_of(&example("plate"), "plate");
        for kind in ["check", "report"] {
            let text = station_answer(kind, &json).unwrap();
            let answer = parse_answer(&Json::parse(&text).unwrap()).unwrap();
            assert_eq!(answer.kind, kind);
            assert_eq!(answer.evidence, "simulator");
            assert!(answer.verdict.pass);
            assert_eq!(answer.counts.is_some(), kind == "report");
            assert_eq!(answer.verdict, check_json(&json));
        }
        let report =
            parse_answer(&Json::parse(&station_answer("report", &json).unwrap()).unwrap()).unwrap();
        let counts = report.counts.unwrap();
        assert_eq!(counts.sketches, 1);
        assert_eq!(counts.features, list(&json, "features").len());
        assert!(station_answer("export", &json).is_err());
        assert!(station_answer("check", &Json::Null).is_err());
        for (text, missing) in [
            (r#"{"kind":"check"}"#, "schema"),
            (r#"{"schema":"eh3d.cad.report/1"}"#, "kind"),
            (r#"{"schema":"eh3d.cad.report/1","kind":"check"}"#, "check"),
            (
                r#"{"schema":"eh3d.cad.report/1","kind":"check","check":{"verdict":"maybe"}}"#,
                "verdict",
            ),
            (
                r#"{"schema":"eh3d.cad.report/1","kind":"check","check":{"verdict":"pass"}}"#,
                "issues",
            ),
            (
                r#"{"schema":"eh3d.cad.report/1","kind":"check","check":{"verdict":"pass","issues":[],"errors":0}}"#,
                "warnings",
            ),
            (
                r#"{"schema":"eh3d.cad.report/1","kind":"check","check":{"verdict":"pass","issues":[],"errors":0,"warnings":0}}"#,
                "evidence",
            ),
            (
                r#"{"schema":"eh3d.cad.report/1","kind":"check","check":{"verdict":"pass","issues":[{"severity":"error"}],"errors":0,"warnings":0},"evidence":"simulator"}"#,
                "code",
            ),
        ] {
            assert_eq!(
                parse_answer(&Json::parse(text).unwrap()).unwrap_err(),
                missing,
                "{text}"
            );
        }
    }

    #[test]
    fn random_projects_are_checked_without_a_panic_and_the_answer_reads_back_with_a_fixed_seed() {
        let mut state = 0x57A7_1090_0016_BEEFu64;
        let mut next = |n: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % n
        };
        let ids = ["a", "b", "c", "d", "e", ""];
        let kinds = [
            "extrude", "pocket", "revolve", "hole", "fillet", "chamfer", "mirror", "pattern",
            "sweep", "",
        ];
        let (mut passed, mut failed) = (0, 0);
        for round in 0..400 {
            let mut text = String::from("{\"schema\":");
            text.push_str(if next(10) == 0 {
                "\"other\""
            } else {
                "\"eh3d.cad.project/1\""
            });
            if next(8) != 0 {
                text.push_str(&format!(
                    ",\"units\":{}",
                    quote(["mm", "in", "cm"][next(3) as usize])
                ));
            }
            let mut sketches: Vec<String> = Vec::new();
            for _ in 0..next(5) {
                let id = ids[next(6) as usize];
                sketches.push(if id.is_empty() {
                    "{}".to_string()
                } else {
                    format!("{{\"id\":{}}}", quote(id))
                });
            }
            let mut features: Vec<String> = Vec::new();
            for _ in 0..next(5) {
                let (id, kind) = (ids[next(6) as usize], kinds[next(10) as usize]);
                let (sketch, target) = (ids[next(6) as usize], ids[next(6) as usize]);
                features.push(format!(
                    "{{\"id\":{},\"kind\":{},\"sketch\":{},\"target\":{}}}",
                    quote(id),
                    quote(kind),
                    quote(sketch),
                    quote(target)
                ));
            }
            let mut parts: Vec<String> = Vec::new();
            for _ in 0..next(5) {
                let id = ids[next(6) as usize];
                let mut uses: Vec<String> = Vec::new();
                for _ in 0..next(3) {
                    uses.push(quote(ids[next(6) as usize]));
                }
                parts.push(format!(
                    "{{\"id\":{},\"features\":[{}]}}",
                    quote(id),
                    uses.join(",")
                ));
            }
            text.push_str(&format!(
                ",\"sketches\":[{}],\"features\":[{}],\"parts\":[{}]}}",
                sketches.join(","),
                features.join(","),
                parts.join(",")
            ));
            let json = Json::parse(&text).unwrap();
            let verdict = check_json(&json);
            // the counts are those of the list, and a pass is a list with no error
            assert_eq!(
                verdict.errors + verdict.warnings,
                verdict.issues.len(),
                "round {round}"
            );
            assert_eq!(verdict.pass, verdict.errors == 0, "round {round}");
            assert!(verdict
                .issues
                .iter()
                .all(|i| matches!(i.severity.as_str(), "error" | "warning")));
            if verdict.pass {
                passed += 1;
            } else {
                failed += 1;
            }
            // the answer of the station reads back to the same verdict, for a check and for a report
            for kind in ["check", "report"] {
                let answer =
                    parse_answer(&Json::parse(&station_answer(kind, &json).unwrap()).unwrap())
                        .unwrap_or_else(|e| panic!("round {round} {kind}: {e}\n{text}"));
                assert_eq!(answer.verdict, verdict, "round {round} {kind}");
                assert_eq!(answer.counts.is_some(), kind == "report");
            }
        }
        assert!(passed > 5 && failed > 100, "{passed} {failed}");
    }
}
