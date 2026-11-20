// SPDX-License-Identifier: GPL-3.0-or-later
//! Two versions of an assembly side by side: what was added, taken away, moved, turned, cut again
//! or replaced among the placed parts, and which mates were added, taken away or changed.
//!
//! A part is the same part in both versions when it has the same id (the id lives as long as the
//! part does, whatever its name becomes), so a part that was renamed or moved is one change and not
//! a removal and an addition. The positions are those of the parts after the mates were applied,
//! the ones people see. The changes come in a fixed order (parts taken away, added, replaced, cut
//! again, moved and turned, then mates added, taken away and changed, each by id), so that the
//! same two versions give the same text.
//!
//! The comma separated values never change with the language; the text is told in the eight
//! languages of the suite.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::assembly::Mate;
use crate::bom::cell;
use crate::catalog::{Orientation, PartInstance};
use crate::enclosure::Id;
use crate::lang::{fill, Lang, Msg};
use crate::scad::mm_text;

/// One difference between two versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A part that the first version does not have.
    PartAdded {
        /// Its name.
        name: String,
    },
    /// A part that the second version does not have.
    PartRemoved {
        /// Its name in the first version.
        name: String,
    },
    /// The part of the library is another one (or the same name with another shape or material).
    PartReplaced {
        /// Its name in the second version.
        name: String,
        /// The part it was.
        from: String,
        /// The part it is.
        to: String,
    },
    /// A profile that is cut to another length.
    PartCut {
        /// Its name in the second version.
        name: String,
        /// The cut it had, in nanometres (`None` for the usual cut).
        from: Option<i64>,
        /// The cut it has.
        to: Option<i64>,
    },
    /// The corner of the box of the part is somewhere else.
    PartMoved {
        /// Its name in the second version.
        name: String,
        /// Where the corner was (`x`, `y`, `z`) in nanometres.
        from: [i64; 3],
        /// Where it is.
        to: [i64; 3],
    },
    /// The part points another way or is rolled another way.
    PartTurned {
        /// Its name in the second version.
        name: String,
        /// How it was turned.
        from: Orientation,
        /// How it is turned.
        to: Orientation,
    },
    /// A mate that the first version does not have.
    MateAdded {
        /// The mate told with the names of its parts.
        mate: String,
    },
    /// A mate that the second version does not have.
    MateRemoved {
        /// The mate told with the names of its parts.
        mate: String,
    },
    /// A mate with the same id that asks for something else.
    MateChanged {
        /// The mate in the first version.
        from: String,
        /// The mate in the second version.
        to: String,
    },
}

impl Change {
    /// The word of the first column of the values: `part-added`, `part-removed`, `part-replaced`,
    /// `part-cut`, `part-moved`, `part-turned`, `mate-added`, `mate-removed` or `mate-changed`.
    pub fn kind(&self) -> &'static str {
        match self {
            Change::PartAdded { .. } => "part-added",
            Change::PartRemoved { .. } => "part-removed",
            Change::PartReplaced { .. } => "part-replaced",
            Change::PartCut { .. } => "part-cut",
            Change::PartMoved { .. } => "part-moved",
            Change::PartTurned { .. } => "part-turned",
            Change::MateAdded { .. } => "mate-added",
            Change::MateRemoved { .. } => "mate-removed",
            Change::MateChanged { .. } => "mate-changed",
        }
    }
}

fn corner(at: [i64; 3]) -> String {
    format!(
        "x={} y={} z={}",
        mm_text(at[0]),
        mm_text(at[1]),
        mm_text(at[2])
    )
}

fn turn(o: Orientation) -> String {
    format!("axis {} roll {}", o.axis.name(), u32::from(o.roll) * 90)
}

/// The cut in millimetres, or `none` for a part that is not cut.
fn cut(c: Option<i64>) -> String {
    c.map_or_else(|| "none".to_string(), mm_text)
}

fn describe(mate: &Mate, names: &BTreeMap<Id, String>) -> String {
    let name = |id: &Id| names.get(id).cloned().unwrap_or_else(|| id.to_string());
    let words = mate.words();
    let rest: Vec<&str> = words.iter().skip(2).map(String::as_str).collect();
    format!("{} / {} {}", name(&mate.a), name(&mate.b), rest.join(" "))
}

/// What changed from the first version of an assembly to the second: the placed parts (the
/// positions after the mates) and the mates of each.
pub fn compare(
    before: &BTreeMap<Id, PartInstance>,
    after: &BTreeMap<Id, PartInstance>,
    mates_before: &BTreeMap<Id, Mate>,
    mates_after: &BTreeMap<Id, Mate>,
) -> Vec<Change> {
    let mut out = Vec::new();
    for (id, p) in before {
        if !after.contains_key(id) {
            out.push(Change::PartRemoved {
                name: p.name.clone(),
            });
        }
    }
    for (id, p) in after {
        if !before.contains_key(id) {
            out.push(Change::PartAdded {
                name: p.name.clone(),
            });
        }
    }
    let both = |f: &mut dyn FnMut(&PartInstance, &PartInstance, &mut Vec<Change>)| {
        let mut list = Vec::new();
        for (id, a) in before {
            if let Some(b) = after.get(id) {
                f(a, b, &mut list);
            }
        }
        list
    };
    out.extend(both(&mut |a, b, l| {
        if a.part != b.part {
            l.push(Change::PartReplaced {
                name: b.name.clone(),
                from: a.part.name.clone(),
                to: b.part.name.clone(),
            });
        }
    }));
    out.extend(both(&mut |a, b, l| {
        if a.part == b.part && a.cut_length() != b.cut_length() {
            l.push(Change::PartCut {
                name: b.name.clone(),
                from: a.cut_length().map(|c| c.get()),
                to: b.cut_length().map(|c| c.get()),
            });
        }
    }));
    out.extend(both(&mut |a, b, l| {
        let (from, to) = (a.at.map(|v| v.get()), b.at.map(|v| v.get()));
        if from != to {
            l.push(Change::PartMoved {
                name: b.name.clone(),
                from,
                to,
            });
        }
        if a.orientation != b.orientation {
            l.push(Change::PartTurned {
                name: b.name.clone(),
                from: a.orientation,
                to: b.orientation,
            });
        }
    }));
    let mut names: BTreeMap<Id, String> =
        before.iter().map(|(id, p)| (*id, p.name.clone())).collect();
    names.extend(after.iter().map(|(id, p)| (*id, p.name.clone())));
    for (id, m) in mates_after {
        if !mates_before.contains_key(id) {
            out.push(Change::MateAdded {
                mate: describe(m, &names),
            });
        }
    }
    for (id, m) in mates_before {
        if !mates_after.contains_key(id) {
            out.push(Change::MateRemoved {
                mate: describe(m, &names),
            });
        }
    }
    for (id, a) in mates_before {
        if let Some(b) = mates_after.get(id) {
            if a != b {
                out.push(Change::MateChanged {
                    from: describe(a, &names),
                    to: describe(b, &names),
                });
            }
        }
    }
    out
}

/// The changes as comma separated values: `change,part,from,to` with a line for each (the part
/// column holds the name of the part, or the mate for the changes of mates). Same text in every
/// language.
pub fn csv(changes: &[Change]) -> String {
    let mut out = String::from("change,part,from,to\n");
    for c in changes {
        let (what, from, to) = match c {
            Change::PartAdded { name } | Change::PartRemoved { name } => {
                (name.clone(), String::new(), String::new())
            }
            Change::PartReplaced { name, from, to } => (name.clone(), from.clone(), to.clone()),
            Change::PartCut { name, from, to } => (name.clone(), cut(*from), cut(*to)),
            Change::PartMoved { name, from, to } => (name.clone(), corner(*from), corner(*to)),
            Change::PartTurned { name, from, to } => (name.clone(), turn(*from), turn(*to)),
            Change::MateAdded { mate } | Change::MateRemoved { mate } => {
                (mate.clone(), String::new(), String::new())
            }
            Change::MateChanged { from, to } => (String::new(), from.clone(), to.clone()),
        };
        let _ = writeln!(
            out,
            "{},{},{},{}",
            c.kind(),
            cell(&what),
            cell(&from),
            cell(&to)
        );
    }
    out
}

/// The changes as lines of text in a language, with a last line that counts them (or one line that
/// says there are none).
pub fn text_in(changes: &[Change], lang: Lang) -> String {
    if changes.is_empty() {
        return format!("{}\n", Msg::CompareNone.tr(lang));
    }
    let mut out = String::new();
    for c in changes {
        let line = match c {
            Change::PartAdded { name } => fill(Msg::CompareAdded.tr(lang), &[name]),
            Change::PartRemoved { name } => fill(Msg::CompareRemoved.tr(lang), &[name]),
            Change::PartReplaced { name, from, to } => {
                fill(Msg::CompareReplaced.tr(lang), &[name, from, to])
            }
            Change::PartCut { name, from, to } => {
                fill(Msg::CompareCut.tr(lang), &[name, &cut(*from), &cut(*to)])
            }
            Change::PartMoved { name, from, to } => fill(
                Msg::CompareMoved.tr(lang),
                &[name, &corner(*from), &corner(*to)],
            ),
            Change::PartTurned { name, from, to } => fill(
                Msg::CompareTurned.tr(lang),
                &[name, &turn(*from), &turn(*to)],
            ),
            Change::MateAdded { mate } => fill(Msg::CompareMateAdded.tr(lang), &[mate]),
            Change::MateRemoved { mate } => fill(Msg::CompareMateRemoved.tr(lang), &[mate]),
            Change::MateChanged { from, to } => fill(Msg::CompareMateChanged.tr(lang), &[from, to]),
        };
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str(&fill(
        Msg::CompareCount.tr(lang),
        &[&changes.len().to_string()],
    ));
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::MateKind;
    use crate::catalog::{Axis, Part, Shape};
    use crate::Length;
    use eh3d_container::ObjectId;

    fn id(n: u64) -> Id {
        ObjectId::from_parts(1_700_000_000_000, 1, n).unwrap()
    }

    fn screw_part(name: &str) -> Part {
        Part {
            name: name.to_string(),
            material: "steel".to_string(),
            shape: Shape::Screw {
                metric: "M6".to_string(),
                diameter: Length::new(6_000_000).unwrap(),
                length: Length::new(20_000_000).unwrap(),
                head: crate::catalog::Head::Cylinder,
                head_diameter: Length::new(10_000_000).unwrap(),
                head_height: Length::new(4_000_000).unwrap(),
            },
        }
    }

    fn bar(cut: Option<i64>) -> PartInstance {
        let mm = |v: i64| Length::new(v * 1_000_000).unwrap();
        PartInstance {
            name: "bar".to_string(),
            library: "lib".to_string(),
            part: Part {
                name: "tube".to_string(),
                material: "aluminium".to_string(),
                shape: Shape::Profile {
                    width: mm(20),
                    height: mm(20),
                    wall: mm(2),
                    default_cut: mm(100),
                    max_cut: mm(1000),
                },
            },
            at: [mm(0), mm(0), mm(0)],
            orientation: Orientation::default(),
            cut: cut.map(mm),
        }
    }

    fn put(name: &str, part: &str, at: [i64; 3]) -> PartInstance {
        PartInstance {
            name: name.to_string(),
            library: "lib".to_string(),
            part: screw_part(part),
            at: at.map(|v| Length::new(v).unwrap()),
            orientation: Orientation::default(),
            cut: None,
        }
    }

    #[test]
    fn two_equal_versions_have_no_change_and_say_so_in_each_language() {
        let mut parts = BTreeMap::new();
        parts.insert(id(1), put("one", "m6", [0, 0, 0]));
        let none = BTreeMap::new();
        let changes = compare(&parts, &parts.clone(), &none, &none);
        assert!(changes.is_empty());
        assert_eq!(csv(&changes), "change,part,from,to\n");
        for lang in Lang::ALL {
            assert_eq!(
                text_in(&changes, lang),
                format!("{}\n", Msg::CompareNone.tr(lang))
            );
        }
    }

    #[test]
    fn the_changes_of_parts_come_in_a_fixed_order_and_a_renamed_part_is_one_part() {
        let mut a = BTreeMap::new();
        a.insert(id(1), put("gone", "m6", [0, 0, 0]));
        a.insert(id(2), put("old name", "m6", [5_000_000, 0, 0]));
        a.insert(id(3), put("swap", "m6", [0, 0, 0]));
        a.insert(id(4), put("turn", "m6", [0, 0, 0]));
        let mut b = a.clone();
        b.remove(&id(1));
        b.insert(id(5), put("new", "m6", [1_000_000, 2_000_000, 3_000_000]));
        b.get_mut(&id(2)).unwrap().name = "new name".to_string();
        b.get_mut(&id(2)).unwrap().at[0] = Length::new(7_500_000).unwrap();
        b.get_mut(&id(3)).unwrap().part = screw_part("m8");
        b.get_mut(&id(4)).unwrap().orientation = Orientation {
            axis: Axis::PlusX,
            roll: 1,
        };
        let none = BTreeMap::new();
        let changes = compare(&a, &b, &none, &none);
        let kinds: Vec<&str> = changes.iter().map(Change::kind).collect();
        assert_eq!(
            kinds,
            [
                "part-removed",
                "part-added",
                "part-replaced",
                "part-moved",
                "part-turned"
            ]
        );
        assert_eq!(
            csv(&changes),
            "change,part,from,to\n\
             part-removed,gone,,\n\
             part-added,new,,\n\
             part-replaced,swap,m6,m8\n\
             part-moved,new name,x=5 y=0 z=0,x=7.5 y=0 z=0\n\
             part-turned,turn,axis +z roll 0,axis +x roll 90\n"
        );
        let text = text_in(&changes, Lang::En);
        assert!(
            text.starts_with("Part removed: gone\nPart added: new\n"),
            "{text}"
        );
        assert!(text.ends_with("5 changes\n"), "{text}");
        // the same two versions always tell the same
        assert_eq!(changes, compare(&a, &b, &none, &none));
    }

    #[test]
    fn mates_that_were_added_taken_away_or_changed_are_told_with_the_names_of_the_parts() {
        let mut parts = BTreeMap::new();
        parts.insert(id(1), put("base", "m6", [0, 0, 0]));
        parts.insert(id(2), put("lid", "m6", [0, 0, 0]));
        let stack = Mate {
            a: id(1),
            b: id(2),
            kind: MateKind::Stack,
        };
        let gap = Mate {
            a: id(1),
            b: id(2),
            kind: MateKind::Gap {
                direction: Axis::PlusZ,
                distance: Length::new(2_000_000).unwrap(),
            },
        };
        let none = BTreeMap::new();
        let one: BTreeMap<Id, Mate> = [(id(9), stack)].into_iter().collect();
        let other: BTreeMap<Id, Mate> = [(id(9), gap)].into_iter().collect();
        let added = compare(&parts, &parts, &none, &one);
        assert_eq!(
            added,
            [Change::MateAdded {
                mate: "base / lid stack".to_string()
            }]
        );
        let removed = compare(&parts, &parts, &one, &none);
        assert_eq!(
            removed,
            [Change::MateRemoved {
                mate: "base / lid stack".to_string()
            }]
        );
        let changed = compare(&parts, &parts, &one, &other);
        assert_eq!(
            csv(&changed),
            "change,part,from,to\nmate-changed,,base / lid stack,base / lid gap +z 2000000\n"
        );
        assert!(compare(&parts, &parts, &one, &one).is_empty());
    }

    #[test]
    fn every_change_is_told_in_each_language_with_its_values_in_place() {
        let changes = [
            Change::PartAdded { name: "A".into() },
            Change::PartRemoved { name: "B".into() },
            Change::PartReplaced {
                name: "C".into(),
                from: "x".into(),
                to: "y".into(),
            },
            Change::PartCut {
                name: "D".into(),
                from: None,
                to: Some(1_500_000),
            },
            Change::PartMoved {
                name: "E".into(),
                from: [0, 0, 0],
                to: [1_000_000, 0, -2_000_000],
            },
            Change::PartTurned {
                name: "F".into(),
                from: Orientation::default(),
                to: Orientation {
                    axis: Axis::MinusY,
                    roll: 3,
                },
            },
            Change::MateAdded { mate: "G".into() },
            Change::MateRemoved { mate: "H".into() },
            Change::MateChanged {
                from: "I".into(),
                to: "J".into(),
            },
        ];
        for lang in Lang::ALL {
            let text = text_in(&changes, lang);
            assert_eq!(text.lines().count(), changes.len() + 1, "{}", lang.code());
            assert!(!text.contains('{'), "{text}");
            for name in ["A", "B", "C", "D", "E", "F", "G", "H"] {
                assert!(text.contains(name), "{} {name}", lang.code());
            }
        }
        let en = text_in(&changes, Lang::En);
        assert!(en.contains("Part cut again: D, from none to 1.5\n"), "{en}");
        assert!(
            en.contains("Part moved: E, from x=0 y=0 z=0 to x=1 y=0 z=-2\n"),
            "{en}"
        );
        assert!(
            en.contains("Part turned: F, from axis +z roll 0 to axis -y roll 270\n"),
            "{en}"
        );
    }

    #[test]
    fn a_profile_cut_again_is_one_change_and_the_usual_cut_counts_as_a_cut() {
        let mut a = BTreeMap::new();
        a.insert(id(1), bar(None));
        let mut b = a.clone();
        b.get_mut(&id(1)).unwrap().cut = Some(Length::new(150_000_000).unwrap());
        let none = BTreeMap::new();
        let changes = compare(&a, &b, &none, &none);
        assert_eq!(
            changes,
            [Change::PartCut {
                name: "bar".to_string(),
                from: Some(100_000_000),
                to: Some(150_000_000)
            }]
        );
        assert_eq!(csv(&changes), "change,part,from,to\npart-cut,bar,100,150\n");
        // the same cut written out is no change
        let mut c = a.clone();
        c.get_mut(&id(1)).unwrap().cut = Some(Length::new(100_000_000).unwrap());
        assert!(compare(&a, &c, &none, &none).is_empty());
    }

    #[test]
    fn names_that_a_spreadsheet_would_take_for_a_formula_are_quoted() {
        let changes = [Change::PartAdded {
            name: "=SUM(A1)".to_string(),
        }];
        let text = csv(&changes);
        assert_eq!(text, "change,part,from,to\npart-added,'=SUM(A1),,\n");
    }

    /// The next number of a fixed sequence (xorshift), so that a failure can be repeated.
    fn next_seed(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    #[test]
    fn random_edits_are_counted_one_for_one_and_swap_when_the_versions_are_swapped_with_a_fixed_seed(
    ) {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let none = BTreeMap::new();
        for round in 0..300 {
            let size = 1 + next_seed(&mut state) % 12;
            let mut a: BTreeMap<Id, PartInstance> = BTreeMap::new();
            for n in 1..=size {
                let at = [
                    (next_seed(&mut state) % 100) as i64 * 1_000_000,
                    (next_seed(&mut state) % 100) as i64 * 1_000_000,
                    0,
                ];
                a.insert(id(n), put(&format!("p{n}"), "m6", at));
            }
            // one edit for each part at most: nothing, taken away, moved, replaced or turned
            let mut b = a.clone();
            let (mut removed, mut moved, mut replaced, mut turned) = (0, 0, 0, 0);
            for n in 1..=size {
                match next_seed(&mut state) % 5 {
                    1 => {
                        b.remove(&id(n));
                        removed += 1;
                    }
                    2 => {
                        let p = b.get_mut(&id(n)).unwrap();
                        p.at[2] =
                            Length::new(((next_seed(&mut state) % 50) as i64 + 1) * 1_000_000)
                                .unwrap();
                        moved += 1;
                    }
                    3 => {
                        b.get_mut(&id(n)).unwrap().part = screw_part("m8");
                        replaced += 1;
                    }
                    4 => {
                        b.get_mut(&id(n)).unwrap().orientation = Orientation {
                            axis: Axis::PlusY,
                            roll: 2,
                        };
                        turned += 1;
                    }
                    _ => {}
                }
            }
            let added = (next_seed(&mut state) % 3) as usize;
            for k in 0..added {
                b.insert(id(100 + k as u64), put("new", "m6", [0, 0, 0]));
            }
            let forward = compare(&a, &b, &none, &none);
            let count =
                |list: &[Change], kind: &str| list.iter().filter(|c| c.kind() == kind).count();
            assert_eq!(count(&forward, "part-removed"), removed, "round {round}");
            assert_eq!(count(&forward, "part-added"), added, "round {round}");
            assert_eq!(count(&forward, "part-moved"), moved, "round {round}");
            assert_eq!(count(&forward, "part-replaced"), replaced, "round {round}");
            assert_eq!(count(&forward, "part-turned"), turned, "round {round}");
            assert_eq!(forward.len(), removed + added + moved + replaced + turned);
            // a version against itself has no change; the csv has a line for each change
            assert!(compare(&b, &b, &none, &none).is_empty());
            assert_eq!(csv(&forward).lines().count(), forward.len() + 1);
            // the other way round, what was added is removed and what was removed is added
            let back = compare(&b, &a, &none, &none);
            assert_eq!(count(&back, "part-removed"), added, "round {round}");
            assert_eq!(count(&back, "part-added"), removed, "round {round}");
            assert_eq!(count(&back, "part-moved"), moved, "round {round}");
            assert_eq!(back.len(), forward.len());
        }
    }
}
