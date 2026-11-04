//! The graph of dependencies between the objects of a model.
//!
//! Every object has an id (UUID v7), a name, a hash of its own parameters and the ids of the
//! objects it depends on. The graph can never hold a cycle: an edit that would make one is
//! refused and changes nothing. The order of calculation is the same for the same graph whatever
//! the order in which it was built: an object comes after everything it depends on, and among the
//! objects that could come next the one with the smallest id goes first.
//!
//! [`Graph::recompute`] calculates only what changed. Each result is kept with the hash of the
//! parameters and the keys of the results it was made from; an object whose inputs are equal to
//! the saved ones is reused. An object that fails is reported with its error and the objects that
//! depend on it are marked as inheriting the failure; the rest of the model goes on.

use std::collections::{BTreeMap, BTreeSet};

use crate::enclosure::Id;
use crate::CadError;

/// The 64-bit FNV-1a hash of a list of numbers (each as 8 little-endian bytes). It is the same on
/// every platform and in every version, unlike the hashers of the standard library.
pub fn fnv64(parts: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.to_le_bytes() {
            h ^= u64::from(byte);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    name: String,
    params: u64,
    deps: BTreeSet<Id>,
}

/// A graph of objects and the objects they depend on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Graph {
    nodes: BTreeMap<Id, Node>,
}

/// A result kept between two calculations, with what it was made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cached<T> {
    params: u64,
    dep_keys: Vec<u64>,
    key: u64,
    /// The result.
    pub value: T,
}

impl<T> Cached<T> {
    /// The key that identifies this result for the objects that depend on it.
    pub fn key(&self) -> u64 {
        self.key
    }
}

/// The results kept by id.
pub type Cache<T> = BTreeMap<Id, Cached<T>>;

/// What happened to an object in a calculation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeState {
    /// The object was calculated.
    Computed,
    /// The saved result was still good and was reused.
    Reused,
    /// The calculation of the object failed with this error.
    Failed(CadError),
    /// The object depends on this one, which failed (directly or by inheriting the failure).
    Inherited(Id),
}

impl Graph {
    /// An empty graph.
    pub fn new() -> Graph {
        Graph::default()
    }

    /// The number of objects.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph has no object.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Whether the object exists.
    pub fn contains(&self, id: Id) -> bool {
        self.nodes.contains_key(&id)
    }

    fn node(&self, id: Id) -> Result<&Node, CadError> {
        self.nodes
            .get(&id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))
    }

    /// Adds an object that depends on objects that already exist.
    pub fn add(&mut self, id: Id, name: &str, params: u64, deps: &[Id]) -> Result<(), CadError> {
        if self.contains(id) {
            return Err(CadError::Duplicate(id.to_string()));
        }
        for d in deps {
            self.node(*d)?;
        }
        self.nodes.insert(
            id,
            Node {
                name: name.to_string(),
                params,
                deps: deps.iter().copied().collect(),
            },
        );
        Ok(())
    }

    /// Replaces the hash of the parameters of an object (it changed).
    pub fn set_params(&mut self, id: Id, params: u64) -> Result<(), CadError> {
        self.node(id)?;
        if let Some(n) = self.nodes.get_mut(&id) {
            n.params = params;
        }
        Ok(())
    }

    /// Whether `from` depends on `target`, directly or through other objects.
    fn depends_on(&self, from: Id, target: Id) -> bool {
        let mut seen = BTreeSet::new();
        let mut stack = vec![from];
        while let Some(current) = stack.pop() {
            if current == target {
                return true;
            }
            if !seen.insert(current) {
                continue;
            }
            if let Some(n) = self.nodes.get(&current) {
                stack.extend(n.deps.iter().copied());
            }
        }
        false
    }

    /// Replaces the dependencies of an object. A dependency that would close a cycle is refused
    /// with the code `PHOENIX-CAD-013` and the graph stays as it was.
    pub fn set_deps(&mut self, id: Id, deps: &[Id]) -> Result<(), CadError> {
        self.node(id)?;
        for d in deps {
            self.node(*d)?;
            if self.depends_on(*d, id) {
                return Err(CadError::Cycle(format!("{id} -> {d}")));
            }
        }
        if let Some(n) = self.nodes.get_mut(&id) {
            n.deps = deps.iter().copied().collect();
        }
        Ok(())
    }

    /// Removes an object that no other object depends on.
    pub fn remove(&mut self, id: Id) -> Result<(), CadError> {
        self.node(id)?;
        if self.nodes.values().any(|n| n.deps.contains(&id)) {
            return Err(CadError::InUse(id.to_string()));
        }
        self.nodes.remove(&id);
        Ok(())
    }

    /// The objects an object depends on, in id order.
    pub fn dependencies(&self, id: Id) -> Result<Vec<Id>, CadError> {
        Ok(self.node(id)?.deps.iter().copied().collect())
    }

    /// The objects that depend directly on an object, in id order.
    pub fn dependents(&self, id: Id) -> Result<Vec<Id>, CadError> {
        self.node(id)?;
        Ok(self
            .nodes
            .iter()
            .filter(|(_, n)| n.deps.contains(&id))
            .map(|(i, _)| *i)
            .collect())
    }

    /// The order of calculation: every object after the objects it depends on.
    pub fn order(&self) -> Result<Vec<Id>, CadError> {
        let mut missing: BTreeMap<Id, usize> = self
            .nodes
            .iter()
            .map(|(id, n)| (*id, n.deps.len()))
            .collect();
        let mut ready: BTreeSet<Id> = missing
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut out = Vec::with_capacity(self.nodes.len());
        while let Some(id) = ready.iter().next().copied() {
            ready.remove(&id);
            out.push(id);
            for (other, node) in &self.nodes {
                if node.deps.contains(&id) {
                    if let Some(count) = missing.get_mut(other) {
                        *count -= 1;
                        if *count == 0 {
                            ready.insert(*other);
                        }
                    }
                }
            }
        }
        if out.len() == self.nodes.len() {
            Ok(out)
        } else {
            Err(CadError::Cycle("the graph holds a cycle".into()))
        }
    }

    /// The objects that must be calculated again when an object changes: the object itself and
    /// everything that depends on it, directly or not, in the order of calculation.
    pub fn affected_by(&self, id: Id) -> Result<Vec<Id>, CadError> {
        self.node(id)?;
        let mut set = BTreeSet::new();
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            if set.insert(current) {
                for (other, node) in &self.nodes {
                    if node.deps.contains(&current) {
                        stack.push(*other);
                    }
                }
            }
        }
        Ok(self
            .order()?
            .into_iter()
            .filter(|i| set.contains(i))
            .collect())
    }

    /// The graph as Graphviz text, for looking at it: the same text for the same graph.
    pub fn to_dot(&self) -> Result<String, CadError> {
        let escape = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let mut out = String::from("digraph phoenix {\n");
        let order = self.order()?;
        for id in &order {
            out.push_str(&format!(
                "  \"{id}\" [label=\"{}\"];\n",
                escape(&self.nodes[id].name)
            ));
        }
        for id in &order {
            for d in &self.nodes[id].deps {
                out.push_str(&format!("  \"{d}\" -> \"{id}\";\n"));
            }
        }
        out.push_str("}\n");
        Ok(out)
    }

    /// Calculates what changed since the saved results in `cache`.
    ///
    /// `compute` receives the id of an object and the results of its dependencies in id order. The
    /// results of objects that no longer exist are dropped from the cache. The state of every
    /// object is returned.
    pub fn recompute<T, F>(
        &self,
        cache: &mut Cache<T>,
        mut compute: F,
    ) -> Result<BTreeMap<Id, NodeState>, CadError>
    where
        F: FnMut(Id, &[&T]) -> Result<T, CadError>,
    {
        cache.retain(|id, _| self.nodes.contains_key(id));
        let mut states: BTreeMap<Id, NodeState> = BTreeMap::new();
        for id in self.order()? {
            let node = &self.nodes[&id];
            let broken = node.deps.iter().find(|d| {
                matches!(
                    states.get(*d),
                    Some(NodeState::Failed(_) | NodeState::Inherited(_))
                )
            });
            if let Some(d) = broken {
                cache.remove(&id);
                states.insert(id, NodeState::Inherited(*d));
                continue;
            }
            let dep_keys: Vec<u64> = node
                .deps
                .iter()
                .map(|d| cache.get(d).map_or(0, Cached::key))
                .collect();
            if let Some(saved) = cache.get(&id) {
                if saved.params == node.params && saved.dep_keys == dep_keys {
                    states.insert(id, NodeState::Reused);
                    continue;
                }
            }
            let result = {
                let inputs: Vec<&T> = node
                    .deps
                    .iter()
                    .filter_map(|d| cache.get(d).map(|c| &c.value))
                    .collect();
                compute(id, &inputs)
            };
            match result {
                Ok(value) => {
                    let mut parts = vec![node.params, dep_keys.len() as u64];
                    parts.extend(&dep_keys);
                    cache.insert(
                        id,
                        Cached {
                            params: node.params,
                            key: fnv64(&parts),
                            dep_keys,
                            value,
                        },
                    );
                    states.insert(id, NodeState::Computed);
                }
                Err(e) => {
                    cache.remove(&id);
                    states.insert(id, NodeState::Failed(e));
                }
            }
        }
        Ok(states)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eh3d_container::ObjectId;

    fn id(n: u64) -> Id {
        ObjectId::from_parts(1_700_000_000_000, (n & 0x0fff) as u16, n).unwrap()
    }

    /// 1 depends on 3, 2 on nothing, 4 on 1 and 2.
    fn small() -> Graph {
        let mut g = Graph::new();
        g.add(id(3), "c", 30, &[]).unwrap();
        g.add(id(2), "b", 20, &[]).unwrap();
        g.add(id(1), "a", 10, &[id(3)]).unwrap();
        g.add(id(4), "d", 40, &[id(1), id(2)]).unwrap();
        g
    }

    /// A calculation that sums the parameters of an object and of what it depends on, and logs the
    /// objects it was asked to calculate.
    macro_rules! sum {
        ($g:expr, $calls:expr) => {{
            let params: BTreeMap<Id, u64> = $g.nodes.iter().map(|(i, n)| (*i, n.params)).collect();
            let log = &mut $calls;
            move |i: Id, inputs: &[&u64]| -> Result<u64, CadError> {
                log.push(i);
                Ok(params[&i] + inputs.iter().copied().sum::<u64>())
            }
        }};
    }

    #[test]
    fn the_order_comes_after_the_dependencies_and_takes_the_smallest_id_first() {
        let g = small();
        assert_eq!(g.order().unwrap(), vec![id(2), id(3), id(1), id(4)]);
        // built in another order, the same graph gives the same order
        let mut h = Graph::new();
        h.add(id(2), "b", 20, &[]).unwrap();
        h.add(id(3), "c", 30, &[]).unwrap();
        h.add(id(1), "a", 10, &[id(3)]).unwrap();
        h.add(id(4), "d", 40, &[id(2), id(1)]).unwrap();
        assert_eq!(h.order().unwrap(), g.order().unwrap());
        assert_eq!(h, g);
    }

    #[test]
    fn a_dependency_that_closes_a_cycle_is_refused_and_changes_nothing() {
        let mut g = small();
        let before = g.clone();
        // 3 would depend on 4, which depends on 1, which depends on 3
        let err = g.set_deps(id(3), &[id(4)]).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-013");
        assert_eq!(g, before);
        assert_eq!(
            g.set_deps(id(1), &[id(1)]).unwrap_err().code(),
            "PHOENIX-CAD-013"
        );
        assert_eq!(g, before);
        // a dependency that does not close a cycle is fine
        g.set_deps(id(2), &[id(3)]).unwrap();
        assert_eq!(g.order().unwrap(), vec![id(3), id(1), id(2), id(4)]);
    }

    #[test]
    fn unknown_duplicate_and_used_objects_are_refused_with_their_own_codes() {
        let mut g = small();
        assert_eq!(
            g.add(id(9), "z", 0, &[id(8)]).unwrap_err().code(),
            "PHOENIX-CAD-014"
        );
        assert_eq!(
            g.add(id(1), "again", 0, &[]).unwrap_err().code(),
            "PHOENIX-CAD-015"
        );
        assert_eq!(g.remove(id(1)).unwrap_err().code(), "PHOENIX-CAD-016");
        assert_eq!(g.remove(id(8)).unwrap_err().code(), "PHOENIX-CAD-014");
        assert_eq!(
            g.set_params(id(8), 1).unwrap_err().code(),
            "PHOENIX-CAD-014"
        );
        g.remove(id(4)).unwrap();
        g.remove(id(1)).unwrap();
        assert_eq!(g.len(), 2);
        assert!(!g.is_empty());
    }

    #[test]
    fn what_depends_on_an_object_is_found_directly_and_through_others() {
        let g = small();
        assert_eq!(g.dependents(id(3)).unwrap(), vec![id(1)]);
        assert_eq!(g.dependencies(id(4)).unwrap(), vec![id(1), id(2)]);
        assert_eq!(g.affected_by(id(3)).unwrap(), vec![id(3), id(1), id(4)]);
        assert_eq!(g.affected_by(id(2)).unwrap(), vec![id(2), id(4)]);
        assert_eq!(g.affected_by(id(4)).unwrap(), vec![id(4)]);
        assert!(g.affected_by(id(8)).is_err());
    }

    #[test]
    fn the_graph_is_written_as_graphviz_text_in_a_fixed_order() {
        let mut g = Graph::new();
        g.add(id(1), "base \"box\"", 0, &[]).unwrap();
        g.add(id(2), "holes", 0, &[id(1)]).unwrap();
        let text = g.to_dot().unwrap();
        let expected = format!(
            "digraph phoenix {{\n  \"{a}\" [label=\"base \\\"box\\\"\"];\n  \"{b}\" [label=\"holes\"];\n  \"{a}\" -> \"{b}\";\n}}\n",
            a = id(1),
            b = id(2)
        );
        assert_eq!(text, expected);
        assert_eq!(Graph::new().to_dot().unwrap(), "digraph phoenix {\n}\n");
    }

    #[test]
    fn the_hash_is_fnv_one_a_and_depends_on_the_order() {
        assert_eq!(fnv64(&[]), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv64(&[1, 2]), fnv64(&[1, 2]));
        assert_ne!(fnv64(&[1, 2]), fnv64(&[2, 1]));
        assert_ne!(fnv64(&[0]), fnv64(&[]));
    }

    #[test]
    fn the_first_calculation_does_everything_and_the_second_reuses_everything() {
        let g = small();
        let mut cache: Cache<u64> = Cache::new();
        let mut calls = Vec::new();
        let states = g.recompute(&mut cache, sum!(g, calls)).unwrap();
        assert_eq!(calls, vec![id(2), id(3), id(1), id(4)]);
        assert!(states.values().all(|s| *s == NodeState::Computed));
        // 4 = 40 + (10 + 30) + 20
        assert_eq!(cache[&id(4)].value, 100);
        let mut calls = Vec::new();
        let states = g.recompute(&mut cache, sum!(g, calls)).unwrap();
        assert!(calls.is_empty());
        assert!(states.values().all(|s| *s == NodeState::Reused));
    }

    #[test]
    fn a_change_recalculates_only_the_object_and_what_depends_on_it() {
        let mut g = small();
        let mut cache: Cache<u64> = Cache::new();
        let mut calls = Vec::new();
        g.recompute(&mut cache, sum!(g, calls)).unwrap();
        g.set_params(id(3), 31).unwrap();
        let mut calls = Vec::new();
        let states = g.recompute(&mut cache, sum!(g, calls)).unwrap();
        assert_eq!(calls, vec![id(3), id(1), id(4)]);
        assert_eq!(states[&id(2)], NodeState::Reused);
        assert_eq!(cache[&id(4)].value, 101);
        // the objects that were recalculated are the ones the graph says are affected
        assert_eq!(calls, g.affected_by(id(3)).unwrap());
    }

    #[test]
    fn a_failing_object_is_reported_and_its_dependents_inherit_the_failure() {
        let g = small();
        let mut cache: Cache<u64> = Cache::new();
        let failing = |i: Id, inputs: &[&u64]| -> Result<u64, CadError> {
            if i == id(1) {
                Err(CadError::Overflow("test"))
            } else {
                Ok(inputs.iter().copied().sum::<u64>() + 1)
            }
        };
        let states = g.recompute(&mut cache, failing).unwrap();
        assert_eq!(
            states[&id(1)],
            NodeState::Failed(CadError::Overflow("test"))
        );
        assert_eq!(states[&id(4)], NodeState::Inherited(id(1)));
        assert_eq!(states[&id(2)], NodeState::Computed);
        assert_eq!(states[&id(3)], NodeState::Computed);
        assert!(!cache.contains_key(&id(1)));
        assert!(!cache.contains_key(&id(4)));
        assert!(cache.contains_key(&id(2)));
    }

    #[test]
    fn the_results_of_objects_that_are_gone_are_dropped_from_the_cache() {
        let mut g = small();
        let mut cache: Cache<u64> = Cache::new();
        let mut calls = Vec::new();
        g.recompute(&mut cache, sum!(g, calls)).unwrap();
        g.remove(id(4)).unwrap();
        let mut calls = Vec::new();
        g.recompute(&mut cache, sum!(g, calls)).unwrap();
        assert_eq!(cache.len(), 3);
        assert!(!cache.contains_key(&id(4)));
    }
}
