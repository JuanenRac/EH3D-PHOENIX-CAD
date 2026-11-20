// SPDX-License-Identifier: GPL-3.0-or-later
//! What a change of the recipe changed: the fields that differ between two states of the
//! mechanical domain, by name (a parameter, a variable, a step, the tip, the board, a sketch).
//! The history panel lists them under each change, so that a person who undoes a change knows what
//! comes back before doing it.
//!
//! The comparison is made over the canonical text of the domain (`Mechanical::to_bytes`), line by
//! line, so a field is whatever the file keeps: the same recipe always gives the same list, and a
//! field that is the same in both states is never named.

use std::collections::BTreeMap;

use crate::config::MechanicalLimits;
use crate::mechanical::Mechanical;

/// What kind of field changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FieldKind {
    /// A parameter of the enclosure (its base value or its definition).
    Parameter,
    /// A variable of the project.
    Variable,
    /// A step of the recipe (its operation, its values or whether it is suppressed).
    Step,
    /// How many steps are active.
    Tip,
    /// What the board tool wrote.
    Board,
    /// A sketch of the project.
    Sketch,
}

/// What happened to the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum How {
    /// The field is new.
    Added,
    /// The field is gone.
    Removed,
    /// The field is there in both and differs.
    Changed,
}

/// One field that differs between two states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    /// The kind of field.
    pub kind: FieldKind,
    /// Its name (the name of the parameter, variable, step or sketch; empty for the tip and the
    /// board).
    pub name: String,
    /// What happened to it.
    pub how: How,
}

/// The name of a step or of a hole in a word of the file: `%20` is a space again, `~` is no name.
pub(crate) fn unescape_word(word: &str) -> String {
    if word == "~" {
        return String::new();
    }
    let bytes = word.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a word is the text of an object identity (36 characters, four hyphens).
fn looks_like_id(word: &str) -> bool {
    word.len() == 36 && word.bytes().filter(|b| *b == b'-').count() == 4
}

type Fields = BTreeMap<(FieldKind, String, String), String>;

/// The fields of a state: for each one the key (kind, name, the id that tells two steps of one name
/// apart) and the text of its lines.
fn fields_of(m: &Mechanical, limits: &MechanicalLimits) -> Option<Fields> {
    let bytes = m.to_bytes(limits).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let mut out: Fields = BTreeMap::new();
    let mut section = "";
    let mut put = |key: (FieldKind, String, String), line: &str| {
        let slot = out.entry(key).or_default();
        if !slot.is_empty() {
            slot.push('\n');
        }
        slot.push_str(line);
    };
    for line in text.lines() {
        if line.starts_with('[') && line.ends_with(']') {
            section = match line {
                "[parameters]" => "parameters",
                "[steps]" => "steps",
                _ => "board",
            };
            continue;
        }
        let words: Vec<&str> = line.split(' ').collect();
        match section {
            "parameters" => match words.as_slice() {
                ["base" | "param", name, ..] => put(
                    (FieldKind::Parameter, (*name).to_string(), String::new()),
                    line,
                ),
                ["variable", name, ..] => put(
                    (FieldKind::Variable, (*name).to_string(), String::new()),
                    line,
                ),
                _ => {}
            },
            "steps" => match words.as_slice() {
                ["tip", _] => put((FieldKind::Tip, String::new(), String::new()), line),
                ["step", _, name, _op, rest @ ..] => {
                    let id = rest
                        .first()
                        .filter(|w| looks_like_id(w))
                        .map_or_else(String::new, |w| (*w).to_string());
                    let op = words.get(3).copied().unwrap_or("");
                    // the flag of suppression is part of the text, not of the key
                    put(
                        (FieldKind::Step, unescape_word(name), format!("{op} {id}")),
                        &words[1..].join(" "),
                    );
                }
                _ => {}
            },
            _ => put((FieldKind::Board, String::new(), String::new()), line),
        }
    }
    for sketch in &m.sketches {
        let text = crate::sketch::file::to_text(sketch);
        out.insert(
            (FieldKind::Sketch, sketch.name().to_string(), String::new()),
            text,
        );
    }
    Some(out)
}

/// The fields that differ between two states of the recipe, in the order of their kind and name.
/// A state that cannot be written (it is over a limit) gives no list.
pub fn changed_fields(
    before: &Mechanical,
    after: &Mechanical,
    limits: &MechanicalLimits,
) -> Vec<FieldChange> {
    let (Some(a), Some(b)) = (fields_of(before, limits), fields_of(after, limits)) else {
        return Vec::new();
    };
    let mut out: Vec<FieldChange> = Vec::new();
    let mut seen: Vec<&(FieldKind, String, String)> = Vec::new();
    for key in a.keys().chain(b.keys()) {
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let how = match (a.get(key), b.get(key)) {
            (Some(x), Some(y)) if x != y => How::Changed,
            (Some(_), None) => How::Removed,
            (None, Some(_)) => How::Added,
            _ => continue,
        };
        out.push(FieldChange {
            kind: key.0,
            name: key.1.clone(),
            how,
        });
    }
    out.sort_by(|x, y| (x.kind, &x.name, x.how).cmp(&(y.kind, &y.name, y.how)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::enclosure::Face;
    use crate::project::Document;
    use crate::sketch::{Sketch, SketchPlane};
    use crate::Length;

    fn limits() -> MechanicalLimits {
        Settings::defaults().mechanical
    }

    fn recipe() -> Mechanical {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/box.phoenixproj"
        ))
        .unwrap();
        let s = Settings::defaults();
        Document::from_bytes(&bytes, s.mechanical.max_bytes)
            .unwrap()
            .mechanical(&s.expr, &s.mechanical)
            .unwrap()
            .unwrap()
    }

    fn change(kind: FieldKind, name: &str, how: How) -> FieldChange {
        FieldChange {
            kind,
            name: name.to_string(),
            how,
        }
    }

    #[test]
    fn two_equal_states_differ_in_nothing() {
        let m = recipe();
        assert!(changed_fields(&m, &m.clone(), &limits()).is_empty());
    }

    #[test]
    fn the_tip_a_suppressed_step_and_a_removed_step_are_named() {
        let before = recipe();
        let mut tip = before.clone();
        tip.tip -= 1;
        assert_eq!(
            changed_fields(&before, &tip, &limits()),
            vec![change(FieldKind::Tip, "", How::Changed)]
        );
        let mut off = before.clone();
        off.steps[0].suppressed = true;
        assert_eq!(
            changed_fields(&before, &off, &limits()),
            vec![change(FieldKind::Step, "usb port", How::Changed)]
        );
        let mut gone = before.clone();
        let removed = gone.steps.remove(1);
        gone.tip -= 1;
        assert_eq!(
            changed_fields(&before, &gone, &limits()),
            vec![
                change(FieldKind::Step, &removed.name, How::Removed),
                change(FieldKind::Tip, "", How::Changed),
            ]
        );
        // and the other way round the step is added
        assert_eq!(
            changed_fields(&gone, &before, &limits()),
            vec![
                change(FieldKind::Step, &removed.name, How::Added),
                change(FieldKind::Tip, "", How::Changed),
            ]
        );
    }

    #[test]
    fn a_renamed_step_is_a_step_gone_and_a_step_added_and_a_variable_is_named() {
        let before = recipe();
        let mut renamed = before.clone();
        renamed.steps[0].name = "port".to_string();
        assert_eq!(
            changed_fields(&before, &renamed, &limits()),
            vec![
                change(FieldKind::Step, "port", How::Added),
                change(FieldKind::Step, "usb port", How::Removed),
            ]
        );
        let mut with_variable = before.clone();
        with_variable
            .vars
            .set("gap", Length::from_mm(2).unwrap())
            .unwrap();
        assert_eq!(
            changed_fields(&before, &with_variable, &limits()),
            vec![change(FieldKind::Variable, "gap", How::Added)]
        );
        let mut moved = with_variable.clone();
        moved.vars.set("gap", Length::from_mm(3).unwrap()).unwrap();
        assert_eq!(
            changed_fields(&with_variable, &moved, &limits()),
            vec![change(FieldKind::Variable, "gap", How::Changed)]
        );
    }

    #[test]
    fn a_sketch_is_named_when_it_comes_changes_and_goes() {
        let before = recipe();
        let mut with = before.clone();
        let mut sketch = Sketch::new("plate", SketchPlane::on_face(Face::Top)).unwrap();
        with.put_sketch(sketch.clone());
        assert_eq!(
            changed_fields(&before, &with, &limits()),
            vec![change(FieldKind::Sketch, "plate", How::Added)]
        );
        sketch
            .add_point(Length::ZERO, Length::from_mm(1).unwrap())
            .unwrap();
        let mut edited = before.clone();
        edited.put_sketch(sketch);
        assert_eq!(
            changed_fields(&with, &edited, &limits()),
            vec![change(FieldKind::Sketch, "plate", How::Changed)]
        );
        assert_eq!(
            changed_fields(&edited, &before, &limits()),
            vec![change(FieldKind::Sketch, "plate", How::Removed)]
        );
    }

    #[test]
    fn names_are_read_back_from_the_words_of_the_file() {
        assert_eq!(unescape_word("usb%20port"), "usb port");
        assert_eq!(unescape_word("~"), "");
        assert_eq!(unescape_word("plain-name_1.2"), "plain-name_1.2");
        assert_eq!(unescape_word("100%"), "100%");
        assert_eq!(unescape_word("%zz"), "%zz");
        assert_eq!(unescape_word("caf%C3%A9"), "café");
        assert!(looks_like_id("018bcfe5-6800-7000-8000-000000001000"));
        assert!(!looks_like_id("usb"));
        assert!(!looks_like_id("018bcfe5680070008000000000001000xxxx"));
    }

    #[test]
    fn a_state_that_cannot_be_written_gives_no_list() {
        let before = recipe();
        let mut after = before.clone();
        after.tip -= 1;
        let tiny = MechanicalLimits::new(10, 10, 10, 10, 10).unwrap();
        assert!(changed_fields(&before, &after, &tiny).is_empty());
    }

    #[test]
    fn random_edits_are_named_the_same_in_both_directions_with_a_fixed_seed() {
        let mut state = 0x5EED_0016_C4A9_6E57u64;
        let mut next = |n: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % n
        };
        let base = recipe();
        let mut named = 0;
        for round in 0..200 {
            let mut other = base.clone();
            for _ in 0..1 + next(4) {
                match next(4) {
                    0 => {
                        let i = next(other.steps.len() as u64) as usize;
                        other.steps[i].suppressed = !other.steps[i].suppressed;
                    }
                    1 => other.tip = next(other.steps.len() as u64 + 1) as usize,
                    2 if other.steps.len() > 1 => {
                        let i = next(other.steps.len() as u64) as usize;
                        other.steps.remove(i);
                        other.tip = other.tip.min(other.steps.len());
                    }
                    _ => {
                        let i = next(other.steps.len() as u64) as usize;
                        other.steps[i].name = format!("renamed {}", next(5));
                    }
                }
            }
            let forward = changed_fields(&base, &other, &limits());
            let back = changed_fields(&other, &base, &limits());
            assert_eq!(forward.len(), back.len(), "round {round}");
            // what was added one way was removed the other way, and a change is a change
            let flipped = |c: &FieldChange| match c.how {
                How::Added => How::Removed,
                How::Removed => How::Added,
                How::Changed => How::Changed,
            };
            for c in &forward {
                assert!(
                    back.iter()
                        .any(|b| b.kind == c.kind && b.name == c.name && b.how == flipped(c)),
                    "round {round}: {c:?} not in {back:?}"
                );
            }
            // the list is in order and names a field once
            let mut sorted = forward.clone();
            sorted.sort_by(|x, y| (x.kind, &x.name, x.how).cmp(&(y.kind, &y.name, y.how)));
            assert_eq!(forward, sorted, "round {round}");
            assert_eq!(changed_fields(&other, &other, &limits()), Vec::new());
            // the same recipe is the same list
            assert_eq!(forward, changed_fields(&base, &other.clone(), &limits()));
            named += forward.len();
        }
        assert!(named > 150, "{named}");
    }
}
