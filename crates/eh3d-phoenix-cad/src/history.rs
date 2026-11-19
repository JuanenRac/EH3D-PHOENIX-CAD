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

    /// A small generator with a fixed seed (xorshift), so the run is the same every time.
    struct Seeded(u64);

    impl Seeded {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
    }

    #[test]
    fn a_long_random_run_of_changes_undos_and_redos_follows_a_plain_model() {
        for depth in [1usize, 3, 8, 50] {
            let mut seed = Seeded(0x9E37_79B9_7F4A_7C15 ^ depth as u64);
            let mut h: History<u64> = History::new(depth);
            // the model: two lists, the same rules written out again
            let mut undo: Vec<(String, u64)> = Vec::new();
            let mut redo: Vec<(String, u64)> = Vec::new();
            let mut current: u64 = 0;
            let (mut undone, mut redone) = (0, 0);
            for step in 0..4000u32 {
                match seed.next() % 5 {
                    0..=2 => {
                        let name = format!("change {step}");
                        h.record(&name, current);
                        undo.push((name, current));
                        redo.clear();
                        if undo.len() > depth {
                            undo.remove(0);
                        }
                        current = seed.next() % 1000;
                    }
                    3 => {
                        let got = h.undo(current);
                        match undo.pop() {
                            Some((name, before)) => {
                                redo.push((name.clone(), current));
                                assert_eq!(got, Some((name, before)));
                                current = before;
                                undone += 1;
                            }
                            None => assert_eq!(got, None),
                        }
                    }
                    _ => {
                        let got = h.redo(current);
                        match redo.pop() {
                            Some((name, after)) => {
                                undo.push((name.clone(), current));
                                assert_eq!(got, Some((name, after)));
                                current = after;
                                redone += 1;
                            }
                            None => assert_eq!(got, None),
                        }
                    }
                }
                assert_eq!(h.can_undo(), !undo.is_empty());
                assert_eq!(h.can_redo(), !redo.is_empty());
                assert!(h.undo_names().len() <= depth);
                let wanted: Vec<&str> = undo.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(h.undo_names(), wanted);
                let wanted: Vec<&str> = redo.iter().map(|(n, _)| n.as_str()).collect();
                assert_eq!(h.redo_names(), wanted);
            }
            // the run did go back and forward many times
            assert!(undone > 100 && redone > 50, "{depth}: {undone} {redone}");
        }
    }

    #[test]
    fn going_back_and_forward_again_always_returns_to_the_same_state() {
        let mut seed = Seeded(0x2545_F491_4F6C_DD1D);
        for _ in 0..300 {
            let mut h: History<u64> = History::new(20);
            let mut current = 0u64;
            let changes = 1 + seed.next() % 15;
            for n in 0..changes {
                h.record(&format!("c{n}"), current);
                current = seed.next() % 1000;
            }
            let end = current;
            let back = 1 + seed.next() % changes;
            let mut states = Vec::new();
            for _ in 0..back {
                let (_, previous) = h.undo(current).unwrap();
                states.push(current);
                current = previous;
            }
            for _ in 0..back {
                let (_, next) = h.redo(current).unwrap();
                assert_eq!(Some(next), states.pop());
                current = next;
            }
            assert_eq!(current, end);
            assert!(!h.can_redo());
        }
    }
}
