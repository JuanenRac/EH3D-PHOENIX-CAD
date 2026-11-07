//! A body: the base parameters of an enclosure and an ordered list of operations on it, with a
//! tip, suppressed steps and an undo and redo history.
//!
//! The enclosure of a body is always built again from its recipe ([`Body::build`]): the base
//! parameters, then each step up to the tip in order. A step that is suppressed is skipped without
//! being deleted. A step that fails is reported with its code and skipped, and it does not stop
//! the steps after it. The same recipe gives the same enclosure and the same states.
//!
//! A [`Session`] edits a body in named transactions: all the changes of a transaction are undone
//! together, and a transaction that fails leaves the body exactly as it was.

use crate::config::DesignLimits;
use crate::enclosure::{Enclosure, EnclosureParams, Operation};
use crate::profile::ProfileLimits;
use crate::sketch::Sketch;
use crate::CadError;

/// One step of the recipe of a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The name shown to people.
    pub name: String,
    /// The operation.
    pub op: Operation,
    /// A suppressed step is kept but not applied.
    pub suppressed: bool,
}

/// What happened to a step when the body was built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepState {
    /// The step was applied.
    Valid,
    /// The step is suppressed and was skipped.
    Suppressed,
    /// The step was refused with this error and skipped.
    Failed(CadError),
    /// The step is after the tip and was not tried.
    AfterTip,
}

/// The result of building a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    /// The enclosure after the steps up to the tip.
    pub enclosure: Enclosure,
    /// One state per step, in the order of the steps.
    pub states: Vec<StepState>,
}

/// The recipe of an enclosure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    base: EnclosureParams,
    steps: Vec<Step>,
    tip: usize,
}

impl Body {
    /// A body without steps; the base parameters must make a valid enclosure.
    pub fn new(base: EnclosureParams) -> Result<Body, CadError> {
        Enclosure::new(base)?;
        Ok(Body {
            base,
            steps: Vec::new(),
            tip: 0,
        })
    }

    /// A body from its parts, as they were stored: the base parameters, the steps and the tip. The
    /// base must make a valid enclosure and the tip may not be beyond the last step. The steps are
    /// not tried here; [`Body::build`] reports what each one does.
    pub fn from_parts(
        base: EnclosureParams,
        steps: Vec<Step>,
        tip: usize,
    ) -> Result<Body, CadError> {
        Enclosure::new(base)?;
        if tip > steps.len() {
            return Err(CadError::ValueOutOfRange("tip"));
        }
        Ok(Body { base, steps, tip })
    }

    /// The base parameters.
    pub fn base(&self) -> &EnclosureParams {
        &self.base
    }

    /// The steps in order.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// How many steps are active: the steps from this index on are not applied.
    pub fn tip(&self) -> usize {
        self.tip
    }

    /// Replaces the base parameters; they must make a valid enclosure.
    pub fn set_base(&mut self, base: EnclosureParams) -> Result<(), CadError> {
        Enclosure::new(base)?;
        self.base = base;
        Ok(())
    }

    /// Inserts a step at the tip, after the active steps, and moves the tip after it. Returns the
    /// index of the new step.
    pub fn insert(&mut self, name: &str, op: Operation) -> usize {
        let index = self.tip;
        self.steps.insert(
            index,
            Step {
                name: name.to_string(),
                op,
                suppressed: false,
            },
        );
        self.tip += 1;
        index
    }

    /// Removes a step. The tip follows: it stays after the same active steps.
    pub fn remove(&mut self, index: usize) -> Result<Step, CadError> {
        if index >= self.steps.len() {
            return Err(CadError::ValueOutOfRange("step"));
        }
        if index < self.tip {
            self.tip -= 1;
        }
        Ok(self.steps.remove(index))
    }

    /// Suppresses a step or brings it back.
    pub fn set_suppressed(&mut self, index: usize, suppressed: bool) -> Result<(), CadError> {
        match self.steps.get_mut(index) {
            Some(step) => {
                step.suppressed = suppressed;
                Ok(())
            }
            None => Err(CadError::ValueOutOfRange("step")),
        }
    }

    /// Moves the tip: `0` is the base alone, the number of steps is the whole recipe.
    pub fn set_tip(&mut self, tip: usize) -> Result<(), CadError> {
        if tip > self.steps.len() {
            return Err(CadError::ValueOutOfRange("tip"));
        }
        self.tip = tip;
        Ok(())
    }

    /// Builds the enclosure from the recipe.
    pub fn build(&self) -> Result<Built, CadError> {
        self.build_with(&[], None)
    }

    /// Builds the enclosure from the recipe, with the sketches that its extrusions and pockets
    /// are made from and the limits of their profiles (the shipped ones when there are none).
    pub fn build_with(
        &self,
        sketches: &[Sketch],
        limits: Option<ProfileLimits>,
    ) -> Result<Built, CadError> {
        self.build_with_limits(sketches, limits, None)
    }

    /// Like [`Body::build_with`], with the limits of the operations that repeat things (the
    /// shipped ones when there are none).
    pub fn build_with_limits(
        &self,
        sketches: &[Sketch],
        limits: Option<ProfileLimits>,
        design: Option<DesignLimits>,
    ) -> Result<Built, CadError> {
        let mut enclosure = Enclosure::new(self.base)?;
        if !sketches.is_empty() {
            enclosure.set_sketches(sketches.to_vec(), limits);
        }
        if let Some(design) = design {
            enclosure.set_design_limits(design);
        }
        let mut states = Vec::with_capacity(self.steps.len());
        for (i, step) in self.steps.iter().enumerate() {
            let state = if i >= self.tip {
                StepState::AfterTip
            } else if step.suppressed {
                StepState::Suppressed
            } else {
                match enclosure.apply(step.op.clone()) {
                    Ok(_) => StepState::Valid,
                    Err(e) => StepState::Failed(e),
                }
            };
            states.push(state);
        }
        Ok(Built { enclosure, states })
    }
}

type Snapshot = (String, Body);

/// A body with its history of named transactions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    body: Body,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    depth: usize,
}

impl Session {
    /// A session over a body that keeps at most `depth` transactions to undo.
    pub fn new(body: Body, depth: usize) -> Session {
        Session {
            body,
            undo: Vec::new(),
            redo: Vec::new(),
            depth,
        }
    }

    /// The body.
    pub fn body(&self) -> &Body {
        &self.body
    }

    /// Runs the changes of `edit` as one named transaction. When `edit` fails the body is exactly
    /// as it was and nothing is recorded; when it succeeds the whole transaction can be undone in
    /// one step, and what could be redone is forgotten.
    pub fn transaction<F>(&mut self, name: &str, edit: F) -> Result<(), CadError>
    where
        F: FnOnce(&mut Body) -> Result<(), CadError>,
    {
        let before = self.body.clone();
        match edit(&mut self.body) {
            Ok(()) => {
                self.undo.push((name.to_string(), before));
                self.redo.clear();
                while self.undo.len() > self.depth {
                    self.undo.remove(0);
                }
                Ok(())
            }
            Err(e) => {
                self.body = before;
                Err(e)
            }
        }
    }

    /// Undoes the last transaction and returns its name, or `None` when there is nothing to undo.
    pub fn undo(&mut self) -> Option<String> {
        let (name, previous) = self.undo.pop()?;
        let current = std::mem::replace(&mut self.body, previous);
        self.redo.push((name.clone(), current));
        Some(name)
    }

    /// Redoes the last undone transaction and returns its name.
    pub fn redo(&mut self) -> Option<String> {
        let (name, next) = self.redo.pop()?;
        let current = std::mem::replace(&mut self.body, next);
        self.undo.push((name.clone(), current));
        Some(name)
    }

    /// The names of the transactions that can be undone, the oldest first.
    pub fn undo_names(&self) -> Vec<&str> {
        self.undo.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// The names of the transactions that can be redone, the next one last.
    pub fn redo_names(&self) -> Vec<&str> {
        self.redo.iter().map(|(n, _)| n.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::{Face, Hole};
    use crate::Length;
    use eh3d_container::ObjectId;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn params() -> EnclosureParams {
        EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        }
    }

    fn add(n: u64, u: i64) -> Operation {
        Operation::AddHole {
            id: ObjectId::from_parts(1_700_000_000_000, (n & 0x0fff) as u16, n).unwrap(),
            hole: Hole {
                name: format!("h{n}"),
                face: Face::Front,
                u: mm(u),
                v: mm(17),
                diameter: mm(6),
                style: crate::enclosure::HoleStyle::Plain,
            },
        }
    }

    fn body_with_three() -> Body {
        let mut b = Body::new(params()).unwrap();
        b.insert("first", add(1, 20));
        b.insert("second", add(2, 50));
        b.insert("third", add(3, 80));
        b
    }

    #[test]
    fn building_applies_the_steps_in_order_and_is_repeatable() {
        let b = body_with_three();
        let built = b.build().unwrap();
        assert_eq!(built.enclosure.holes().len(), 3);
        assert_eq!(built.states, vec![StepState::Valid; 3]);
        assert_eq!(built, b.build().unwrap());
    }

    #[test]
    fn a_suppressed_step_is_skipped_and_comes_back_unchanged() {
        let mut b = body_with_three();
        let full = b.build().unwrap();
        b.set_suppressed(1, true).unwrap();
        let built = b.build().unwrap();
        assert_eq!(built.enclosure.holes().len(), 2);
        assert_eq!(built.states[1], StepState::Suppressed);
        assert_eq!(b.steps().len(), 3);
        b.set_suppressed(1, false).unwrap();
        assert_eq!(b.build().unwrap(), full);
    }

    #[test]
    fn a_failing_step_is_reported_and_does_not_stop_the_next_ones() {
        let mut b = Body::new(params()).unwrap();
        b.insert("first", add(1, 20));
        // too close to the first one: 5 mm between centres, 6 mm holes and a web of 2
        b.insert("too close", add(2, 25));
        b.insert("third", add(3, 80));
        let built = b.build().unwrap();
        assert_eq!(built.enclosure.holes().len(), 2);
        assert_eq!(built.states[0], StepState::Valid);
        match &built.states[1] {
            StepState::Failed(e) => assert_eq!(e.code(), "PHOENIX-CAD-004"),
            other => panic!("expected a failure, got {other:?}"),
        }
        assert_eq!(built.states[2], StepState::Valid);
    }

    #[test]
    fn the_tip_hides_the_later_steps_and_a_new_step_goes_after_the_tip() {
        let mut b = body_with_three();
        b.set_tip(1).unwrap();
        let built = b.build().unwrap();
        assert_eq!(built.enclosure.holes().len(), 1);
        assert_eq!(
            built.states,
            vec![StepState::Valid, StepState::AfterTip, StepState::AfterTip]
        );
        let at = b.insert("inserted", add(4, 65));
        assert_eq!((at, b.tip(), b.steps().len()), (1, 2, 4));
        assert_eq!(b.steps()[1].name, "inserted");
        assert_eq!(b.steps()[2].name, "second");
        b.set_tip(4).unwrap();
        assert_eq!(b.build().unwrap().enclosure.holes().len(), 4);
        assert_eq!(b.set_tip(5).unwrap_err(), CadError::ValueOutOfRange("tip"));
    }

    #[test]
    fn removing_a_step_keeps_the_tip_after_the_same_active_steps() {
        let mut b = body_with_three();
        b.set_tip(2).unwrap();
        let removed = b.remove(0).unwrap();
        assert_eq!(removed.name, "first");
        assert_eq!((b.tip(), b.steps().len()), (1, 2));
        b.remove(1).unwrap();
        assert_eq!((b.tip(), b.steps().len()), (1, 1));
        assert_eq!(b.remove(5).unwrap_err().code(), "PHOENIX-CAD-007");
        assert_eq!(
            b.set_suppressed(5, true).unwrap_err().code(),
            "PHOENIX-CAD-007"
        );
    }

    #[test]
    fn a_base_that_makes_no_enclosure_is_refused() {
        let mut bad = params();
        bad.wall = Length::ZERO;
        assert_eq!(Body::new(bad).unwrap_err().code(), "PHOENIX-CAD-002");
        let mut b = body_with_three();
        let before = b.clone();
        assert!(b.set_base(bad).is_err());
        assert_eq!(b, before);
    }

    #[test]
    fn a_transaction_that_fails_leaves_the_body_and_the_history_as_they_were() {
        let mut s = Session::new(Body::new(params()).unwrap(), 10);
        s.transaction("one", |b| {
            b.insert("first", add(1, 20));
            Ok(())
        })
        .unwrap();
        let before = s.clone();
        let result = s.transaction("broken", |b| {
            b.insert("second", add(2, 50));
            b.set_tip(99)
        });
        assert_eq!(result.unwrap_err().code(), "PHOENIX-CAD-007");
        assert_eq!(s, before);
    }

    #[test]
    fn one_transaction_with_several_changes_is_undone_and_redone_as_one() {
        let mut s = Session::new(Body::new(params()).unwrap(), 10);
        s.transaction("three holes", |b| {
            b.insert("a", add(1, 20));
            b.insert("b", add(2, 50));
            b.insert("c", add(3, 80));
            Ok(())
        })
        .unwrap();
        assert_eq!(s.body().steps().len(), 3);
        assert_eq!(s.undo().as_deref(), Some("three holes"));
        assert_eq!(s.body().steps().len(), 0);
        assert_eq!(s.redo_names(), vec!["three holes"]);
        assert_eq!(s.redo().as_deref(), Some("three holes"));
        assert_eq!(s.body().steps().len(), 3);
        assert_eq!(s.undo_names(), vec!["three holes"]);
        assert_eq!(s.redo(), None);
    }

    #[test]
    fn a_new_transaction_forgets_what_could_be_redone_and_the_depth_is_limited() {
        let mut s = Session::new(Body::new(params()).unwrap(), 2);
        for (name, n, u) in [("a", 1, 20), ("b", 2, 50), ("c", 3, 80)] {
            s.transaction(name, |b| {
                b.insert(name, add(n, u));
                Ok(())
            })
            .unwrap();
        }
        // only the last two can be undone
        assert_eq!(s.undo_names(), vec!["b", "c"]);
        assert_eq!(s.undo().as_deref(), Some("c"));
        s.transaction("d", |b| {
            b.insert("d", add(4, 65));
            Ok(())
        })
        .unwrap();
        assert!(s.redo_names().is_empty());
        assert_eq!(s.redo(), None);
        assert_eq!(s.undo().as_deref(), Some("d"));
        assert_eq!(s.undo().as_deref(), Some("b"));
        assert_eq!(s.undo(), None);
        assert_eq!(s.body().steps().len(), 1);
    }
}
