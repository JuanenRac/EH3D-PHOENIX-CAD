// SPDX-License-Identifier: GPL-3.0-or-later
//! The mates panel of the window: the mates of the model in words, the pairs of parts that share
//! volume after them, and the editor of a new mate with a trial of what it would do before it is
//! added. The trial is made on a copy of the window, so a mate that is refused or that leaves
//! parts in conflict is told without changing the project.

use super::*;
use crate::panels::MateDraft;

impl Window {
    /// The parts and the turned and swept bodies a mate can use, in the order of their steps
    /// (up to the tip of the history), each with its name.
    pub fn mate_choices(&self) -> Vec<(Id, String)> {
        let e = &self.built.enclosure;
        self.mech
            .steps
            .iter()
            .take(self.mech.tip)
            .filter_map(|s| match &s.op {
                Operation::AddPart { id, .. } if e.parts().contains_key(id) => Some(*id),
                Operation::AddTurned { id, .. } | Operation::AddSwept { id, .. }
                    if e.is_body(id) =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .map(|id| (id, self.part_name(&id)))
            .collect()
    }

    /// The mate a panel starts with: the kind and the values of the settings of a new mate
    /// (`new_step.mate_*`), the second part placed on the first.
    pub fn mate_draft_new(&self) -> MateDraft {
        let d = &self.ui.new_step;
        MateDraft {
            kind: d.mate_kind.clone(),
            fixed: 0,
            placed: 1,
            direction: d.mate_direction.name().to_string(),
            axis: d.mate_axis.name().to_string(),
            edge: d.mate_edge.name().to_string(),
            distance: mm_text(d.mate_distance.get()),
            plane: mm_text(d.mate_plane.get()),
            turn: (u32::from(d.mate_quarters) * 90).to_string(),
        }
    }

    /// The mate that a draft says, refused when a part is not in the model, when the two parts
    /// are the same or when a value cannot be read.
    pub fn mate_from_draft(&self, draft: &MateDraft) -> Result<Mate, CadError> {
        let choices = self.mate_choices();
        let pick = |place: usize| {
            choices
                .get(place)
                .map(|(id, _)| *id)
                .ok_or_else(|| CadError::Mate("a mate needs two parts in the model".to_string()))
        };
        let (a, b) = (pick(draft.fixed)?, pick(draft.placed)?);
        if a == b {
            return Err(CadError::Mate(
                "a mate needs two different parts".to_string(),
            ));
        }
        let word = |text: &str| text.trim().to_string();
        let axis = || {
            Dim::from_name(draft.axis.trim()).ok_or_else(|| CadError::Reference(word(&draft.axis)))
        };
        let kind = match draft.kind.trim() {
            "gap" => MateKind::Gap {
                direction: Axis::from_name(draft.direction.trim())
                    .ok_or_else(|| CadError::Reference(word(&draft.direction)))?,
                distance: self.parse_text(&draft.distance)?,
            },
            "align" => MateKind::Align {
                axis: axis()?,
                edge: Edge::from_name(draft.edge.trim())
                    .ok_or_else(|| CadError::Reference(word(&draft.edge)))?,
            },
            "stack" => MateKind::Stack,
            "offset" => MateKind::Offset {
                axis: axis()?,
                distance: self.parse_text(&draft.distance)?,
            },
            "symmetric" => MateKind::Symmetric {
                axis: axis()?,
                plane: self.parse_text(&draft.plane)?,
            },
            "turn" => {
                let degrees = draft
                    .turn
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| CadError::ValueOutOfRange("roll"))?;
                if degrees % 90 != 0 || degrees > 270 {
                    return Err(CadError::ValueOutOfRange("roll"));
                }
                MateKind::Turn {
                    quarters: u8::try_from(degrees / 90).unwrap_or(0),
                }
            }
            other => return Err(CadError::Reference(other.to_string())),
        };
        Ok(Mate { a, b, kind })
    }

    /// The values of a mate in words of the language of the window: the kind and what it asks
    /// for (`gap +z 5 mm`, `align z lower`, `turn 90`).
    pub fn mate_words(&self, mate: &Mate) -> String {
        let lang = self.lang;
        let mm = |l: Length| format!("{} mm", mm_text(l.get()));
        match mate.kind {
            MateKind::Gap {
                direction,
                distance,
            } => format!(
                "{} {} {}",
                Msg::MateGap.tr(lang),
                direction.name(),
                mm(distance)
            ),
            MateKind::Align { axis, edge } => {
                let edge = match edge {
                    Edge::Min => Msg::MateEdgeMin,
                    Edge::Max => Msg::MateEdgeMax,
                    Edge::Center => Msg::MateEdgeCenter,
                };
                format!(
                    "{} {} {}",
                    Msg::MateAlign.tr(lang),
                    axis.name(),
                    edge.tr(lang)
                )
            }
            MateKind::Stack => Msg::MateStack.tr(lang).to_string(),
            MateKind::Offset { axis, distance } => format!(
                "{} {} {}",
                Msg::MateOffset.tr(lang),
                axis.name(),
                mm(distance)
            ),
            MateKind::Symmetric { axis, plane } => format!(
                "{} {} {}",
                Msg::MateSymmetric.tr(lang),
                axis.name(),
                mm(plane)
            ),
            MateKind::Turn { quarters } => {
                format!("{} {}", Msg::MateTurn.tr(lang), u32::from(quarters) * 90)
            }
        }
    }

    /// Puts a mate in the recipe as a new step at the tip, the way the button of the tree does
    /// (see `step_addition`): it is kept only when the model accepts it with the other mates.
    fn insert_mate(&mut self, mate: Mate) -> Result<(), CadError> {
        let count = self
            .mech
            .steps
            .iter()
            .filter(|s| matches!(s.op, Operation::AddMate { .. }))
            .count();
        let label = fill(Msg::NameMate.tr(self.lang), &[&(count + 1).to_string()]);
        let op = Operation::AddMate {
            id: self.fresh_id()?,
            mate,
        };
        let mut next = self.mech.clone();
        let index = next.tip.min(next.steps.len());
        next.steps.insert(
            index,
            Step {
                name: label.clone(),
                op,
                suppressed: false,
            },
        );
        next.tip = index + 1;
        let title = fill(Msg::StepAdded.tr(self.lang), &[&label]);
        let done = Status::Done(Msg::StepAdded, vec![label]);
        self.change(&title, done, next, Some(index))?;
        if let Some(step) = self.mech.steps.get(index) {
            self.selection = the_selection_of(index, step);
        }
        Ok(())
    }

    /// Adds the mate of a draft as a step. A refusal is told in the status bar and the project is
    /// as it was. True when the mate was added.
    pub fn add_mate_from_draft(&mut self, draft: &MateDraft) -> bool {
        if self.read_only_refusal() {
            return false;
        }
        let result = self
            .mate_from_draft(draft)
            .and_then(|mate| self.insert_mate(mate));
        self.finish(result);
        !self.status.is_problem()
    }

    /// The window as it would be with a mate added, or the refusal. The project of this window is
    /// not touched.
    pub fn mate_trial(&self, mate: Mate) -> Result<Window, CadError> {
        let mut trial = self.clone();
        trial.insert_mate(mate)?;
        Ok(trial)
    }

    /// The text of the mates panel in the language of the window: a line for each mate (or the
    /// refusal that left it out), the report of the interferences, and the trial of the mate of
    /// the draft: whether the model accepts it, where the placed part would be and which pairs of
    /// parts would share volume.
    pub fn mates_panel_text(&self, draft: &MateDraft) -> String {
        let lang = self.lang;
        let mut lines = Vec::new();
        let mut n = 0;
        for (i, step) in self.mech.steps.iter().enumerate() {
            let Operation::AddMate { mate, .. } = &step.op else {
                continue;
            };
            n += 1;
            match self.built.states.get(i) {
                Some(StepState::Valid) => lines.push(fill(
                    Msg::MatesLine.tr(lang),
                    &[
                        &n.to_string(),
                        &self.part_name(&mate.b),
                        &self.part_name(&mate.a),
                        &self.mate_words(mate),
                    ],
                )),
                Some(StepState::Failed(e)) => lines.push(fill(
                    Msg::MatesFailed.tr(lang),
                    &[&n.to_string(), &step.name, &error_text(e, lang)],
                )),
                _ => {}
            }
        }
        if n == 0 {
            lines.push(Msg::MatesNone.tr(lang).to_string());
        }
        lines.extend(self.assembly_report_lines(true));
        lines.push(String::new());
        let names = self.mate_choices();
        if names.len() < 2 {
            lines.push(Msg::MatesDraftNeedTwo.tr(lang).to_string());
            return lines.join("\n");
        }
        let refused = |e: &CadError| fill(Msg::MatesDraftRefused.tr(lang), &[&error_text(e, lang)]);
        let mate = match self.mate_from_draft(draft) {
            Ok(mate) => mate,
            Err(e) => {
                lines.push(refused(&e));
                return lines.join("\n");
            }
        };
        lines.push(fill(
            Msg::MatesDraftTitle.tr(lang),
            &[
                &self.part_name(&mate.b),
                &self.part_name(&mate.a),
                &self.mate_words(&mate),
            ],
        ));
        match self.mate_trial(mate) {
            Err(e) => lines.push(refused(&e)),
            Ok(trial) => {
                if let Ok(placed) = trial.built.enclosure.try_placed_all() {
                    if let Some(p) = placed.get(&mate.b) {
                        lines.push(fill(
                            Msg::MatesDraftAt.tr(lang),
                            &[&self.part_name(&mate.b), &self.position_text(p.at)],
                        ));
                    }
                }
                match &trial.info.interference {
                    Some(r) if !r.conflicts.is_empty() => {
                        lines.push(fill(
                            Msg::MatesDraftConflicts.tr(lang),
                            &[&r.conflicts.len().to_string()],
                        ));
                        let shown = self.settings.design.assembly.max_report_pairs;
                        for c in r.conflicts.iter().take(shown) {
                            lines.push(fill(
                                Msg::ReportConflict.tr(lang),
                                &[
                                    &trial.part_name(&c.a),
                                    &trial.part_name(&c.b),
                                    &catalog::volume_mm3_text(c.shared_nm3),
                                ],
                            ));
                        }
                    }
                    _ => lines.push(Msg::MatesDraftClear.tr(lang).to_string()),
                }
            }
        }
        lines.join("\n")
    }
}
