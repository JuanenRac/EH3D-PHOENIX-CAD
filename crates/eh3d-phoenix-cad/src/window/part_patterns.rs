// SPDX-License-Identifier: GPL-3.0-or-later
//! The pattern panel of the window: a part of the catalog that is already placed, repeated along a
//! line, over a grid or round a vertical axis (see [`crate::partpattern`]). The panel lists the
//! positions, draws the plan of the boxes of the copies and tries the pattern on a copy of the
//! window, so a pattern that the model refuses, or that leaves parts sharing volume, is told
//! before anything is added. **Add the copies** puts them in the recipe as one step each, all in
//! one entry of the history.

use super::*;
use crate::panels::PartPatternDraft;
use crate::partpattern::{self, PartLayout};
use crate::section::PreviewLine;
use crate::units::Angle;

fn count_of(text: &str, what: &'static str) -> Result<Count, CadError> {
    let n = text
        .trim()
        .parse::<i64>()
        .map_err(|_| CadError::ValueOutOfRange(what))?;
    Count::new(n)
}

impl Window {
    /// The parts a pattern can repeat, in the order of their steps (up to the tip of the
    /// history), each with its name.
    pub fn part_pattern_choices(&self) -> Vec<(Id, String)> {
        let e = &self.built.enclosure;
        self.mech
            .steps
            .iter()
            .take(self.mech.tip)
            .filter_map(|s| match &s.op {
                Operation::AddPart { id, .. } if e.parts().contains_key(id) => Some(*id),
                _ => None,
            })
            .map(|id| (id, self.part_name(&id)))
            .collect()
    }

    /// The pattern a panel starts with: a line of the positions, the step and the angle of the
    /// settings of a new pattern of holes (`new_step.pattern_*`).
    pub fn part_pattern_draft_new(&self) -> PartPatternDraft {
        let d = &self.ui.new_step;
        PartPatternDraft {
            part: 0,
            kind: "linear".to_string(),
            count: d.pattern_count.to_string(),
            rows: "2".to_string(),
            step: [
                mm_text(d.pattern_step.get()),
                "0".to_string(),
                "0".to_string(),
            ],
            center: [
                mm_text(d.pattern_radius.get()),
                mm_text(d.pattern_radius.get()),
            ],
            angle: (d.pattern_angle.get() / 1_000_000_000).to_string(),
        }
    }

    /// The part a draft repeats, as it is in the model.
    fn part_pattern_seed(&self, draft: &PartPatternDraft) -> Result<PartInstance, CadError> {
        let (id, _) = self
            .part_pattern_choices()
            .get(draft.part)
            .cloned()
            .ok_or_else(|| CadError::Pattern("place a part in the model first".to_string()))?;
        self.built
            .enclosure
            .parts()
            .get(&id)
            .cloned()
            .ok_or(CadError::HoleId(id))
    }

    /// The layout that a draft says, refused when a value cannot be read.
    pub fn part_pattern_layout(&self, draft: &PartPatternDraft) -> Result<PartLayout, CadError> {
        let length = |text: &str| {
            if text.trim().is_empty() {
                Ok(Length::ZERO)
            } else {
                self.parse_text(text)
            }
        };
        match draft.kind.trim() {
            "linear" => Ok(PartLayout::Linear {
                step: [
                    length(&draft.step[0])?,
                    length(&draft.step[1])?,
                    length(&draft.step[2])?,
                ],
                count: count_of(&draft.count, "pattern count")?,
            }),
            "grid" => Ok(PartLayout::Grid {
                pitch: [length(&draft.step[0])?, length(&draft.step[1])?],
                columns: count_of(&draft.count, "pattern count")?,
                rows: count_of(&draft.rows, "pattern count")?,
            }),
            "circular" => {
                let degrees = draft
                    .angle
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| CadError::ValueOutOfRange("pattern angle"))?;
                Ok(PartLayout::Circular {
                    center: [length(&draft.center[0])?, length(&draft.center[1])?],
                    step: Angle::from_deg(degrees)?,
                    count: count_of(&draft.count, "pattern count")?,
                })
            }
            other => Err(CadError::Reference(other.to_string())),
        }
    }

    /// The copies that a draft would add: the part at each position but the first, which is
    /// the part itself.
    pub fn part_pattern_copies(
        &self,
        draft: &PartPatternDraft,
    ) -> Result<Vec<PartInstance>, CadError> {
        let seed = self.part_pattern_seed(draft)?;
        let layout = self.part_pattern_layout(draft)?;
        partpattern::copies(&seed, &layout, self.settings.design.part_pattern_max)
    }

    /// Puts the copies in the recipe as steps at the tip, one each, in one entry of the history:
    /// all are kept or none is, and a copy the model refuses (or that a limit of the settings
    /// stops) refuses them all.
    fn insert_part_copies(
        &mut self,
        seed_name: &str,
        copies: Vec<PartInstance>,
    ) -> Result<(), CadError> {
        let mut used = self.used_ids();
        let mut n = 0x1000 + self.mech.steps.len() as u64;
        let mut next = self.mech.clone();
        let first = next.tip.min(next.steps.len());
        let total = copies.len();
        for (k, copy) in copies.into_iter().enumerate() {
            let id = loop {
                n += 1;
                let id = Id::from_parts(1_700_000_000_000, (n & 0x0fff) as u16, n)
                    .map_err(|_| CadError::ValueOutOfRange("id"))?;
                if !used.contains(&id) {
                    used.push(id);
                    break id;
                }
            };
            next.steps.insert(
                first + k,
                Step {
                    name: copy.name.clone(),
                    op: Operation::AddPart { id, part: copy },
                    suppressed: false,
                },
            );
        }
        next.tip = first + total;
        let built = self.rebuild(&next)?;
        for k in 0..total {
            if let Some(StepState::Failed(e)) = built.states.get(first + k) {
                return Err(e.clone());
            }
        }
        let title = fill(
            Msg::PartPatAdded.tr(self.lang),
            &[seed_name, &total.to_string()],
        );
        let done = Status::Done(
            Msg::PartPatAdded,
            vec![seed_name.to_string(), total.to_string()],
        );
        self.change(&title, done, next, None)?;
        if let Some(step) = self.mech.steps.get(first) {
            self.selection = the_selection_of(first, step);
        }
        Ok(())
    }

    /// Adds the copies of a draft as steps. A refusal is told in the status bar and the project
    /// is as it was. True when the copies were added.
    pub fn add_part_pattern(&mut self, draft: &PartPatternDraft) -> bool {
        if self.read_only_refusal() {
            return false;
        }
        let result = self.part_pattern_seed(draft).and_then(|seed| {
            let copies = self.part_pattern_copies(draft)?;
            self.insert_part_copies(&seed.name, copies)
        });
        self.finish(result);
        !self.status.is_problem()
    }

    /// The window as it would be with the copies of a draft added, or the refusal. The project of
    /// this window is not touched.
    pub fn part_pattern_trial(&self, draft: &PartPatternDraft) -> Result<Window, CadError> {
        let seed = self.part_pattern_seed(draft)?;
        let copies = self.part_pattern_copies(draft)?;
        let mut trial = self.clone();
        trial.insert_part_copies(&seed.name, copies)?;
        Ok(trial)
    }

    /// The plan of the boxes of the copies of a draft in a pane of `side` pixels, the part itself
    /// marked (see [`crate::partpattern::preview`]); nothing when the draft is refused.
    pub fn part_pattern_lines(&self, draft: &PartPatternDraft, side: f64) -> Vec<PreviewLine> {
        let (Ok(seed), Ok(layout)) = (
            self.part_pattern_seed(draft),
            self.part_pattern_layout(draft),
        ) else {
            return Vec::new();
        };
        match layout.positions(seed.at, self.settings.design.part_pattern_max) {
            Ok(at) => partpattern::preview(&seed, &at, side, side, 8.0),
            Err(_) => Vec::new(),
        }
    }

    /// The text of the pattern panel in the language of the window: the positions (at most
    /// `panels.part_pattern_shown` of them), and the trial of the pattern: whether the model
    /// accepts it and which pairs of parts would share volume.
    pub fn part_pattern_text(&self, draft: &PartPatternDraft) -> String {
        let lang = self.lang;
        if self.part_pattern_choices().is_empty() {
            return Msg::PartPatNeedPart.tr(lang).to_string();
        }
        let refused = |e: &CadError| fill(Msg::PartPatRefused.tr(lang), &[&error_text(e, lang)]);
        let seed = match self.part_pattern_seed(draft) {
            Ok(seed) => seed,
            Err(e) => return refused(&e),
        };
        let positions = match self
            .part_pattern_layout(draft)
            .and_then(|l| l.positions(seed.at, self.settings.design.part_pattern_max))
        {
            Ok(positions) => positions,
            Err(e) => return refused(&e),
        };
        let mut lines = vec![fill(
            Msg::PartPatTitle.tr(lang),
            &[&seed.name, &positions.len().to_string()],
        )];
        let shown = self.ui.panels.part_pattern_shown as usize;
        for (i, at) in positions.iter().enumerate().take(shown) {
            let name = if i == 0 {
                seed.name.clone()
            } else {
                format!("{} {}", seed.name, i + 1)
            };
            lines.push(fill(
                Msg::PartPatLine.tr(lang),
                &[&(i + 1).to_string(), &name, &self.position_text(*at)],
            ));
        }
        if positions.len() > shown {
            lines.push(fill(
                Msg::PartPatMore.tr(lang),
                &[&(positions.len() - shown).to_string()],
            ));
        }
        match self.part_pattern_trial(draft) {
            Err(e) => lines.push(refused(&e)),
            Ok(trial) => match &trial.info.interference {
                Some(r) if !r.conflicts.is_empty() => lines.push(fill(
                    Msg::PartPatConflicts.tr(lang),
                    &[&r.conflicts.len().to_string()],
                )),
                _ => lines.push(Msg::PartPatClear.tr(lang).to_string()),
            },
        }
        lines.join("\n")
    }
}
