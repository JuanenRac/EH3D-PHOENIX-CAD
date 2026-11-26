// SPDX-License-Identifier: GPL-3.0-or-later
//! The history panel of the window: the list of the changes that can be undone and redone, with
//! the place where the project is now, and going to any of them in one move. How many changes
//! are kept is the setting `undo.depth`; how many the list shows is `panels.history_shown`.

use super::*;
use crate::changes::{changed_fields, FieldKind, How};

impl Window {
    /// The text of the history panel in the language of the window: how many changes are kept
    /// (of the most `undo.depth` allows), the changes already made, oldest first and numbered from
    /// the oldest one that is kept, a line that says where the project is now, and the changes
    /// that were undone and can be redone, the next one first. The lists show at most
    /// `panels.history_shown` changes on each side of that line (the newest ones before it and
    /// the next ones after it) and say how many they leave out.
    pub fn history_text(&self) -> String {
        let lang = self.lang;
        let done = self.history.undo_names();
        let undone = self.history.redo_names();
        if done.is_empty() && undone.is_empty() {
            return Msg::HistoryNone.tr(lang).to_string();
        }
        let shown = self.ui.panels.history_shown as usize;
        let mut lines = vec![fill(
            Msg::HistoryKept.tr(lang),
            &[
                &(done.len() + undone.len()).to_string(),
                &self.settings.undo_depth.to_string(),
            ],
        )];
        let hidden = done.len().saturating_sub(shown);
        if hidden > 0 {
            lines.push(fill(Msg::HistoryOlder.tr(lang), &[&hidden.to_string()]));
        }
        let states = self.history.states(&self.mech);
        let fields = |i: usize| self.history_fields_line(states.get(i), states.get(i + 1));
        for (i, name) in done.iter().enumerate().skip(hidden) {
            lines.push(fill(
                Msg::HistoryRow.tr(lang),
                &[&(i + 1).to_string(), name],
            ));
            lines.extend(fields(i));
        }
        lines.push(Msg::HistoryHere.tr(lang).to_string());
        for (k, name) in undone.iter().rev().take(shown).enumerate() {
            lines.push(fill(
                Msg::HistoryRow.tr(lang),
                &[&(done.len() + k + 1).to_string(), name],
            ));
            lines.extend(fields(done.len() + k));
        }
        if undone.len() > shown {
            lines.push(fill(
                Msg::HistoryLater.tr(lang),
                &[&(undone.len() - shown).to_string()],
            ));
        }
        lines.join("\n")
    }

    /// The fields that differ between two states of the recipe, one by one: what the change from
    /// `before` to `after` touched (a parameter, a variable, a step, the tip, the board, a sketch),
    /// at most `panels.history_fields` of them and then how many more there are.
    pub fn history_fields_of(&self, before: &Mechanical, after: &Mechanical) -> Vec<String> {
        let lang = self.lang;
        let changes = changed_fields(before, after, &self.settings.mechanical);
        let shown = self.ui.panels.history_fields as usize;
        let mut out: Vec<String> = changes
            .iter()
            .take(shown)
            .map(|c| {
                let subject = match c.kind {
                    FieldKind::Parameter => Msg::ChangeParameter,
                    FieldKind::Variable => Msg::ChangeVariable,
                    FieldKind::Step => Msg::ChangeStep,
                    FieldKind::Tip => Msg::ChangeTip,
                    FieldKind::Board => Msg::ChangeBoard,
                    FieldKind::Sketch => Msg::ChangeSketch,
                    FieldKind::Variant => Msg::ChangeVariant,
                };
                let verb = match c.how {
                    How::Added => Msg::ChangeAdded,
                    How::Removed => Msg::ChangeRemoved,
                    How::Changed => Msg::ChangeChanged,
                };
                fill(verb.tr(lang), &[&fill(subject.tr(lang), &[&c.name])])
            })
            .collect();
        if changes.len() > shown {
            out.push(fill(
                Msg::ChangeMore.tr(lang),
                &[&(changes.len() - shown).to_string()],
            ));
        }
        out
    }

    /// The line under a change of the history panel that names its fields; none when the setting
    /// is 0 or the change touched nothing that the recipe keeps.
    fn history_fields_line(
        &self,
        before: Option<&&Mechanical>,
        after: Option<&&Mechanical>,
    ) -> Option<String> {
        if self.ui.panels.history_fields == 0 {
            return None;
        }
        let names = self.history_fields_of(before?, after?);
        if names.is_empty() {
            return None;
        }
        Some(fill(Msg::HistoryFields.tr(self.lang), &[&names.join(", ")]))
    }

    /// How many changes the project has gone through that are kept, and how many of those are
    /// done (the rest can be redone).
    pub fn history_counts(&self) -> (usize, usize) {
        let done = self.history.undo_names().len();
        (done, done + self.history.redo_names().len())
    }

    /// Goes to the state after the change with a number of the list of [`Window::history_text`]
    /// (0 is the state before the oldest change that is kept), undoing or redoing as many changes
    /// as that takes. A number that is not in the list, or one typed as something else, is
    /// refused and the project is not touched. True when the project is now in that state.
    pub fn history_goto_text(&mut self, text: &str) -> bool {
        match text.trim().parse::<usize>() {
            Ok(target) => self.history_goto(target),
            Err(_) => {
                self.status = Status::Error(CadError::ValueOutOfRange("history step"));
                false
            }
        }
    }

    /// Goes to the state after `target` changes of the ones that are kept; see
    /// [`Window::history_goto_text`].
    pub fn history_goto(&mut self, target: usize) -> bool {
        let (done, total) = self.history_counts();
        if target > total || self.drag.is_some() {
            self.status = Status::Error(CadError::ValueOutOfRange("history step"));
            return false;
        }
        for _ in target..done {
            self.undo();
            if matches!(self.status, Status::Error(_)) {
                return false;
            }
        }
        for _ in done..target {
            self.redo();
            if matches!(self.status, Status::Error(_)) {
                return false;
            }
        }
        true
    }
}
