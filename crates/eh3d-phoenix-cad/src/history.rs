//! The history of the changes of a window: named states that can be gone back to and forward
//! again.
//!
//! A change is recorded with the state it started from. Going back swaps that state with the
//! current one, so the same entry can be taken forward again; a new change forgets what could
//! have been taken forward. At most `depth` changes are kept, the oldest ones are dropped first
//! (the depth comes from the settings, key `undo.depth`).

/// A history of states of type `T`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History<T> {
    undo: Vec<(String, T)>,
    redo: Vec<(String, T)>,
    depth: usize,
}

impl<T> History<T> {
    /// An empty history that keeps at most `depth` changes.
    pub fn new(depth: usize) -> History<T> {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            depth,
        }
    }

    /// Records a change made from the state `before`. What could be redone is forgotten.
    pub fn record(&mut self, name: &str, before: T) {
        self.undo.push((name.to_string(), before));
        self.redo.clear();
        while self.undo.len() > self.depth {
            self.undo.remove(0);
        }
    }

    /// Goes back one change: `current` is kept for redo and the earlier state is returned with
    /// the name of the change. `None` (and `current` is not kept) when there is nothing to undo.
    pub fn undo(&mut self, current: T) -> Option<(String, T)> {
        let (name, previous) = self.undo.pop()?;
        self.redo.push((name.clone(), current));
        Some((name, previous))
    }

    /// Goes forward one change that was undone, the same way.
    pub fn redo(&mut self, current: T) -> Option<(String, T)> {
        let (name, next) = self.redo.pop()?;
        self.undo.push((name.clone(), current));
        Some((name, next))
    }

    /// Whether there is a change to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is a change to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The names of the changes that can be undone, the oldest first.
    pub fn undo_names(&self) -> Vec<&str> {
        self.undo.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// The names of the changes that can be redone, the next one last.
    pub fn redo_names(&self) -> Vec<&str> {
        self.redo.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// Forgets every change (a project was opened or started).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undoing_and_redoing_swap_the_states() {
        let mut h: History<i32> = History::new(10);
        let mut state = 1;
        h.record("a", state);
        state = 2;
        h.record("b", state);
        state = 3;
        assert_eq!(h.undo_names(), ["a", "b"]);
        let (name, back) = h.undo(state).unwrap();
        assert_eq!((name.as_str(), back), ("b", 2));
        state = back;
        let (name, back) = h.undo(state).unwrap();
        assert_eq!((name.as_str(), back), ("a", 1));
        state = back;
        assert!(!h.can_undo());
        assert!(h.undo(state).is_none());
        assert_eq!(h.redo_names(), ["b", "a"]);
        let (name, forward) = h.redo(state).unwrap();
        assert_eq!((name.as_str(), forward), ("a", 2));
        state = forward;
        let (name, forward) = h.redo(state).unwrap();
        assert_eq!((name.as_str(), forward), ("b", 3));
        assert!(!h.can_redo());
        assert!(h.redo(forward).is_none());
    }

    #[test]
    fn a_new_change_forgets_what_could_be_redone() {
        let mut h: History<i32> = History::new(10);
        h.record("a", 1);
        let (_, back) = h.undo(2).unwrap();
        assert!(h.can_redo());
        h.record("c", back);
        assert!(!h.can_redo());
        assert_eq!(h.undo_names(), ["c"]);
    }

    #[test]
    fn only_the_newest_changes_up_to_the_depth_are_kept() {
        let mut h: History<i32> = History::new(2);
        for n in 0..5 {
            h.record(&format!("c{n}"), n);
        }
        assert_eq!(h.undo_names(), ["c3", "c4"]);
        let mut none: History<i32> = History::new(0);
        none.record("x", 1);
        assert!(!none.can_undo());
    }

    #[test]
    fn clearing_forgets_both_directions() {
        let mut h: History<i32> = History::new(5);
        h.record("a", 1);
        let _ = h.undo(2);
        h.clear();
        assert!(!h.can_undo() && !h.can_redo());
    }
}
