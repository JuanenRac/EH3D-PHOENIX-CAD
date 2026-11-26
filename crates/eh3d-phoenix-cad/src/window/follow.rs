// SPDX-License-Identifier: GPL-3.0-or-later
//! The variables that follow a reference dimension of a sketch ([`crate::follow`]): a link is made
//! and taken away as a change of the project like any other, and the variables follow when a sketch
//! is kept in the project, in the same entry of the history.

use super::*;
use crate::follow::{constraint_of, linked_value, DimensionLink};

/// A link as the sheet of variables and the command `variables` show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRow {
    /// The variable.
    pub variable: String,
    /// The sketch it follows.
    pub sketch: String,
    /// The reference dimension (`c3`).
    pub constraint: String,
    /// Why the link is broken, or `None` when it reads the drawing.
    pub broken: Option<String>,
}

impl Window {
    /// The links of the project, in the order of the variables.
    pub fn link_rows(&self) -> Vec<LinkRow> {
        self.mech
            .links
            .iter()
            .map(|(variable, link)| LinkRow {
                variable: variable.clone(),
                sketch: link.sketch.clone(),
                constraint: link.constraint.to_string(),
                broken: linked_value(&self.mech.sketches, variable, link, &self.settings)
                    .err()
                    .map(|e| error_text(&e, self.lang)),
            })
            .collect()
    }

    /// Makes the variable `variable` (made when there is none) follow the reference dimension
    /// `constraint` (`c3`) of a sketch of the project, and gives it the length measured now. A
    /// dimension that is not a reference, an angle, a sketch whose formulas use the variable and a
    /// length the model does not accept are refused, and nothing changes.
    pub fn follow_dimension(&mut self, variable: &str, sketch: &str, constraint: &str) -> bool {
        if self.read_only_refusal() {
            return false;
        }
        let result = self.follow_change(variable.trim(), sketch.trim(), constraint);
        let ok = result.is_ok();
        self.finish(result);
        ok
    }

    fn follow_change(
        &mut self,
        variable: &str,
        sketch: &str,
        constraint: &str,
    ) -> Result<(), CadError> {
        let link = DimensionLink {
            sketch: sketch.to_string(),
            constraint: constraint_of(constraint)?,
        };
        let value = linked_value(&self.mech.sketches, variable, &link, &self.settings)?;
        let mut next = self.mech.clone();
        next.vars.set(variable, value)?;
        let shown = vec![
            variable.to_string(),
            link.constraint.to_string(),
            link.sketch.clone(),
            self.unit_text(value),
        ];
        next.links.insert(variable.to_string(), link);
        let title = fill(
            Msg::FollowKept.tr(self.lang),
            &[&shown[0], &shown[1], &shown[2], &shown[3]],
        );
        let done = Status::Done(Msg::FollowKept, shown);
        self.change(&title, done.clone(), next, None)?;
        self.status = done;
        Ok(())
    }

    /// Takes the link of a variable away; the variable keeps its value.
    pub fn stop_following(&mut self, variable: &str) -> bool {
        if self.read_only_refusal() {
            return false;
        }
        let result = self.follow_removal(variable.trim());
        let ok = result.is_ok();
        self.finish(result);
        ok
    }

    fn follow_removal(&mut self, variable: &str) -> Result<(), CadError> {
        let mut next = self.mech.clone();
        next.links
            .remove(variable)
            .ok_or_else(|| CadError::UnknownName(variable.to_string()))?;
        let title = fill(Msg::FollowStopped.tr(self.lang), &[variable]);
        let done = Status::Done(Msg::FollowStopped, vec![variable.to_string()]);
        self.change(&title, done, next, None)
    }

    /// Gives the variables that follow the sketch `sketch` of `next` what it measures. A broken
    /// link leaves its variable as it is.
    pub(super) fn follow_sketch(
        &self,
        next: &mut Mechanical,
        sketch: &str,
    ) -> Result<(), CadError> {
        let links = next.links.clone();
        for (variable, link) in links.iter().filter(|(_, l)| l.sketch == sketch) {
            if let Ok(value) = linked_value(&next.sketches, variable, link, &self.settings) {
                next.vars.set(variable, value)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::{Constraint, ConstraintId, Dim, Sketch, SketchPlane};

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    /// A sketch named `plate` with a line `length` mm long and a reference dimension on it.
    fn plate(length: i64) -> (Sketch, ConstraintId) {
        let mut k = Sketch::new("plate", SketchPlane::on_face(Face::Top)).unwrap();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(length), mm(0)).unwrap();
        let c = k
            .add_constraint(Constraint::Distance {
                a,
                b,
                dim: Dim::fixed(mm(length)),
            })
            .unwrap();
        k.set_reference(c, true).unwrap();
        (k, c)
    }

    /// A new project whose inner width follows the variable `width`, with the sketch of a plate.
    fn project() -> (Window, ConstraintId) {
        let mut w = Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap();
        w.add_variable("width", "60 mm");
        w.mech
            .model
            .set(Param::InnerWidth, Parameter::Variable("width".to_string()))
            .unwrap();
        w.built = w.rebuild(&w.mech).unwrap();
        w.refresh();
        let (k, c) = plate(75);
        w.keep_sketch(k);
        (w, c)
    }

    #[test]
    fn a_variable_takes_the_reference_dimension_and_follows_the_drawing() {
        let (mut w, c) = project();
        assert!(
            w.follow_dimension("width", "plate", &c.to_string()),
            "{:?}",
            w.status
        );
        assert_eq!(w.mech.vars.get("width"), Some(mm(75)));
        assert_eq!(w.params().inner_width, mm(75));
        assert!(w.link_rows()[0].broken.is_none());
        // the drawing changes and the box follows, in one entry of the history
        let entries = w.undo_names().len();
        w.keep_sketch(plate(82).0);
        assert_eq!(w.params().inner_width, mm(82));
        assert_eq!(w.undo_names().len(), entries + 1);
        w.undo();
        assert_eq!(w.params().inner_width, mm(75));
        // a followed variable is not set by hand
        w.set_variable("width", "10 mm");
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-016"));
        assert_eq!(w.mech.vars.get("width"), Some(mm(75)));
        assert!(w.stop_following("width"));
        w.set_variable("width", "61 mm");
        assert_eq!(w.params().inner_width, mm(61));
        assert!(!w.stop_following("width"));
        // and a new variable is made by following
        assert!(w.follow_dimension("span", "plate", &c.to_string()));
        assert_eq!(w.mech.vars.get("span"), Some(mm(75)));
        w.remove_variable("span");
        assert!(w.mech.links.is_empty());
    }

    #[test]
    fn what_cannot_be_followed_is_refused_and_a_link_that_breaks_keeps_the_value() {
        let (mut w, c) = project();
        let before = w.mech.clone();
        for (variable, sketch, constraint) in [
            ("width", "none", "c1"),
            ("width", "plate", "c99"),
            ("width", "plate", "e1"),
            ("1st", "plate", "c1"),
        ] {
            assert!(!w.follow_dimension(variable, sketch, constraint));
        }
        assert_eq!(w.mech, before);
        assert!(w.follow_dimension("width", "plate", &c.to_string()));
        // the dimension drives again: the link is broken and the value stays
        let (mut k, c) = plate(90);
        k.set_reference(c, false).unwrap();
        w.keep_sketch(k);
        assert_eq!(w.mech.vars.get("width"), Some(mm(75)));
        assert!(w.link_rows()[0].broken.is_some());
        // a sketch whose formula uses the variable would go round in a circle
        let (mut k, _) = plate(90);
        let a = k.add_point(mm(0), mm(5)).unwrap();
        let b = k.add_point(mm(9), mm(5)).unwrap();
        k.add_constraint(Constraint::Distance {
            a,
            b,
            dim: Dim::with_expr(mm(9), "width / 2"),
        })
        .unwrap();
        w.keep_sketch(k);
        assert_eq!(w.mech.vars.get("width"), Some(mm(75)));
        assert!(w.link_rows()[0]
            .broken
            .as_deref()
            .is_some_and(|t| t.contains("PHOENIX-CAD-016")));
    }

    #[test]
    fn a_variant_does_not_change_a_followed_variable_and_the_link_is_saved() {
        let (mut w, c) = project();
        assert!(w.keep_variant("small", "width"));
        assert!(w.follow_dimension("width", "plate", &c.to_string()));
        assert!(!w.apply_variant("small"));
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-016"));
        let dir = std::env::temp_dir().join(format!("phoenix-cad-follow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for file in ["linked.phoenixproj", "linked.eh3d"] {
            let path = dir.join(file);
            assert!(w.save_to(&path), "{:?}", w.status);
            let mut again =
                Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap();
            assert!(again.open(&path), "{:?}", again.status);
            assert_eq!(again.mech.links, w.mech.links);
            assert!(!again.read_only);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
