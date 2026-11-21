//! The history of a design as a graph of dependencies.
//!
//! A history is made of **nodes**. A *parameter* is a name with an expression (`wall = 2 mm`,
//! `width = 3 * wall + 10 mm`); a *feature* is an operation (`box`, `move`, `array`, `union`,
//! `difference`, `intersection`) with the nodes it takes as inputs, in order, and a parameter of
//! each of its numbers written as an expression of the language of [`crate::expr`] that may name
//! any parameter and any constant of the settings. The nodes depend on one another as a graph, not
//! as a list: a feature depends on its inputs and on the parameters its expressions name, a
//! parameter on the parameters its expression names.
//!
//! * the order of calculation is the one of [`crate::graph::Graph`]: a node after everything it
//!   depends on, and among the nodes that could come next the one with the smallest id first, so
//!   the same history is always built in the same order;
//! * [`FeatureDag::rebuild`] builds only what changed: a node whose own definition and whose inputs
//!   are the ones of the last time is reused; editing one parameter builds again that parameter
//!   and what depends on it, nothing else;
//! * an edit that would make a node depend on itself is refused with `PHOENIX-CAD-013` and the
//!   history is left as it was, like every other edit that is refused;
//! * a node whose operation fails is reported with its code (`PHOENIX-CAD-052`, or the code of the
//!   expression that failed) and a text in the eight languages, the nodes that depend on it are
//!   reported as inheriting the failure, and the rest of the model is built;
//! * a suppressed feature is skipped: it gives what its first input gave (nothing when it has
//!   none) without being deleted;
//! * the result of a feature that makes a solid is a [`crate::prism::Solid`], so the booleans are
//!   exact (aligned prisms, whole nanometres, no outside library);
//! * the history is written in the optional-looking but critical domain `history` of the project
//!   file, as text in one canonical form with a version, ids that do not change and the lines it
//!   does not know (those that start with `x-`) kept as they are.
//!
//! ```text
//! history 1
//! tip <id>
//! parameter <id> <name> <expression>
//! feature <id> <operation> <name>
//! input <id>
//! param <key> <expression>
//! suppressed 1
//! x-<anything>
//! ```

use std::collections::{BTreeMap, BTreeSet};

use crate::config::{Config, Settings};
use crate::enclosure::Id;
use crate::expr::{parse, Constants, ExprLimits, Names, Value};
use crate::graph::{fnv64, Cache, Graph, NodeState};
use crate::messages;
use crate::prism::{Boolean, Limits, Prism, Solid};
use crate::project::Document;
use crate::vars::check_name;
use crate::{CadError, Length};

/// The shipped settings of the history, as text.
pub const DEFAULT_HISTORY_CONFIG: &str = include_str!("../phoenix-cad-history.default.cfg");

/// The domain of the project file that holds the history.
pub const HISTORY_DOMAIN: &str = "history";

/// The first line of the domain; the number is the version of its text.
const HEADER: &str = "history 1";

/// The settings of the history, with their types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistorySettings {
    /// The most nodes of a history.
    pub max_nodes: usize,
    /// The largest history file that is read, in bytes.
    pub max_file_bytes: usize,
    /// The most copies of an `array`.
    pub max_array: u32,
    /// The most lines that this program does not know that a node keeps.
    pub max_extra_lines: usize,
    /// The most bytes of one of those lines.
    pub max_extra_bytes: usize,
    /// The most cells of the grid of one operation between solids.
    pub max_cells: usize,
    /// The most prisms of a solid.
    pub max_prisms: usize,
}

fn count(c: &Config, key: &str, low: u32, high: u32) -> Result<u32, CadError> {
    let value = c.number(key)?;
    if (low..=high).contains(&value) {
        Ok(value)
    } else {
        Err(CadError::Config(format!(
            "{key} must be from {low} to {high}"
        )))
    }
}

impl HistorySettings {
    /// The defaults that ship with the crate.
    pub fn defaults() -> HistorySettings {
        let config = Config::parse(DEFAULT_HISTORY_CONFIG).expect("the shipped settings are valid");
        HistorySettings::from_config(&config).expect("the shipped settings are complete")
    }

    /// The defaults with the keys of a user file put over them. Keys of other parts are accepted
    /// and ignored here.
    pub fn with_user_config(text: &str) -> Result<HistorySettings, CadError> {
        let mut config = Config::parse(DEFAULT_HISTORY_CONFIG)?;
        config.overlay(&Config::parse(text)?);
        HistorySettings::from_config(&config)
    }

    /// Reads the typed settings from a complete configuration.
    pub fn from_config(c: &Config) -> Result<HistorySettings, CadError> {
        Ok(HistorySettings {
            max_nodes: count(c, "history.max_nodes", 1, 100_000)? as usize,
            max_file_bytes: count(c, "history.max_file_bytes", 1024, 1 << 30)? as usize,
            max_array: count(c, "history.max_array", 1, 100_000)?,
            max_extra_lines: count(c, "history.max_extra_lines", 0, 1000)? as usize,
            max_extra_bytes: count(c, "history.max_extra_bytes", 8, 65_536)? as usize,
            max_cells: count(c, "history.max_cells", 8, 100_000_000)? as usize,
            max_prisms: count(c, "history.max_prisms", 1, 10_000_000)? as usize,
        })
    }

    /// The limits that an operation between solids works inside.
    pub fn limits(&self) -> Limits {
        Limits {
            max_cells: self.max_cells,
            max_prisms: self.max_prisms,
        }
    }
}

/// The text of the shipped settings of the history, to write them to a file.
pub fn default_file() -> &'static str {
    DEFAULT_HISTORY_CONFIG
}

/// What a feature does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Op {
    /// A box: `x y z` is its lowest corner and `sx sy sz` its sizes.
    Box,
    /// The first input moved by `dx dy dz`.
    Move,
    /// The first input repeated `count` times, each copy `dx dy dz` further than the one before.
    Array,
    /// The union of the two inputs.
    Union,
    /// The first input without the second.
    Difference,
    /// What the two inputs have in common.
    Intersection,
    /// The first input together with its reflection in the plane across `axis` at `at`.
    Mirror,
    /// The first input turned by `quarters` quarter turns about the line parallel to `axis`
    /// through `cx cy cz`.
    Rotate,
}

impl Op {
    /// Every operation.
    pub const ALL: [Op; 8] = [
        Op::Box,
        Op::Move,
        Op::Array,
        Op::Union,
        Op::Difference,
        Op::Intersection,
        Op::Mirror,
        Op::Rotate,
    ];

    /// The name written in the file.
    pub fn name(self) -> &'static str {
        match self {
            Op::Box => "box",
            Op::Move => "move",
            Op::Array => "array",
            Op::Union => "union",
            Op::Difference => "difference",
            Op::Intersection => "intersection",
            Op::Mirror => "mirror",
            Op::Rotate => "rotate",
        }
    }

    /// The operation of a name.
    pub fn from_name(name: &str) -> Option<Op> {
        Op::ALL.into_iter().find(|o| o.name() == name)
    }

    /// The inputs the operation takes.
    pub fn inputs(self) -> usize {
        match self {
            Op::Box => 0,
            Op::Move | Op::Array | Op::Mirror | Op::Rotate => 1,
            Op::Union | Op::Difference | Op::Intersection => 2,
        }
    }

    /// The parameters the operation has, in the order they are written.
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            Op::Box => &["x", "y", "z", "sx", "sy", "sz"],
            Op::Move => &["dx", "dy", "dz"],
            Op::Array => &["count", "dx", "dy", "dz"],
            Op::Mirror => &["axis", "at"],
            Op::Rotate => &["axis", "quarters", "cx", "cy", "cz"],
            Op::Union | Op::Difference | Op::Intersection => &[],
        }
    }
}

/// What a node is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A named value given by an expression.
    Parameter {
        /// The expression.
        expr: String,
    },
    /// An operation.
    Feature {
        /// What it does.
        op: Op,
        /// The nodes it takes, in order.
        inputs: Vec<Id>,
        /// The expression of each of its parameters.
        params: BTreeMap<String, String>,
        /// Whether it is skipped.
        suppressed: bool,
    },
}

/// One node of a history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Its identity, which does not change.
    pub id: Id,
    /// Its name, the one an expression writes for a parameter.
    pub name: String,
    /// What it is.
    pub kind: Kind,
    /// The lines of its record that this program does not know (they start with `x-`).
    pub extra: Vec<String>,
}

/// What a calculation gave for a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Product {
    /// The value of a parameter.
    Value(Value),
    /// The solid of a feature.
    Solid(Solid),
}

/// What happened to a node in a rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildState {
    /// It was calculated.
    Computed,
    /// The result of the last time was still good.
    Reused,
    /// It is suppressed and was skipped.
    Suppressed,
    /// Its operation failed.
    Failed,
    /// It depends on a node that failed.
    Inherited,
}

/// The report of a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeReport {
    /// The node.
    pub id: Id,
    /// Its name.
    pub name: String,
    /// What happened.
    pub state: BuildState,
    /// The code of the reason, for a node that failed or inherits a failure.
    pub code: Option<&'static str>,
    /// The reason, in English, with the figures.
    pub detail: String,
}

impl NodeReport {
    /// The line for a person: the name of the node, the text of the code in a language (`en`,
    /// `es`, `de`, `fr`, `it`, `ja`, `zh` or `pt`, English when it is unknown) and the detail.
    pub fn text(&self, language: &str) -> String {
        match self.code {
            None => self.name.clone(),
            Some(code) => {
                let title = messages::message(code, language)
                    .or_else(|| messages::message(code, "en"))
                    .unwrap_or(code);
                format!("{}: {code} {title} ({})", self.name, self.detail)
            }
        }
    }
}

/// The answer of a rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rebuild {
    /// A report for each node, in the order of calculation.
    pub reports: Vec<NodeReport>,
}

impl Rebuild {
    fn of(&self, state: BuildState) -> usize {
        self.reports.iter().filter(|r| r.state == state).count()
    }

    /// The nodes calculated (a suppressed one counts as calculated).
    pub fn computed(&self) -> usize {
        self.of(BuildState::Computed) + self.of(BuildState::Suppressed)
    }

    /// The nodes whose result was reused.
    pub fn reused(&self) -> usize {
        self.of(BuildState::Reused)
    }

    /// The nodes that failed.
    pub fn failed(&self) -> usize {
        self.of(BuildState::Failed)
    }

    /// The nodes that inherit a failure.
    pub fn inherited(&self) -> usize {
        self.of(BuildState::Inherited)
    }

    /// Whether every node was built.
    pub fn is_clean(&self) -> bool {
        self.failed() == 0 && self.inherited() == 0
    }

    /// The report of a node.
    pub fn report(&self, id: Id) -> Option<&NodeReport> {
        self.reports.iter().find(|r| r.id == id)
    }
}

/// A history of nodes.
#[derive(Debug, Clone)]
pub struct FeatureDag {
    nodes: BTreeMap<Id, Node>,
    tip: Option<Id>,
    cache: Cache<Product>,
    settings: HistorySettings,
}

fn bad(why: impl Into<String>) -> CadError {
    CadError::History(why.into())
}

fn text_parts(text: &str) -> Vec<u64> {
    let mut parts = vec![text.len() as u64];
    for chunk in text.as_bytes().chunks(8) {
        let mut b = [0u8; 8];
        b[..chunk.len()].copy_from_slice(chunk);
        parts.push(u64::from_le_bytes(b));
    }
    parts
}

fn id_parts(id: Id) -> [u64; 2] {
    let b = id.as_bytes();
    let (mut hi, mut lo) = ([0u8; 8], [0u8; 8]);
    hi.copy_from_slice(&b[..8]);
    lo.copy_from_slice(&b[8..]);
    [u64::from_le_bytes(hi), u64::from_le_bytes(lo)]
}

fn constants_hash(c: &Constants) -> u64 {
    let mut parts = Vec::new();
    for name in c.names() {
        parts.extend(text_parts(name));
        if let Some(v) = c.get(name) {
            parts.extend(v.hash_parts());
        }
    }
    fnv64(&parts)
}

/// An expression that parses and is written in its canonical form.
fn check_expr(expr: &str, node: &str, limits: &ExprLimits) -> Result<(), CadError> {
    if canonical(expr, limits)? == expr {
        Ok(())
    } else {
        Err(bad(format!(
            "{node}: the expression {expr:?} is not in its canonical form"
        )))
    }
}

/// The names an expression writes.
fn names_of(expr: &str, limits: &ExprLimits) -> Result<Vec<String>, CadError> {
    let mut out: Vec<String> = parse(expr, limits)?
        .names()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// The expression in its canonical form.
fn canonical(expr: &str, limits: &ExprLimits) -> Result<String, CadError> {
    Ok(parse(expr, limits)?.to_text())
}

struct Env<'a> {
    local: BTreeMap<String, Value>,
    constants: &'a Constants,
}

impl Names for Env<'_> {
    fn lookup(&self, name: &str) -> Option<Value> {
        self.local
            .get(name)
            .copied()
            .or_else(|| self.constants.get(name))
    }
}

impl FeatureDag {
    /// A history with nothing in it.
    pub fn new(settings: HistorySettings) -> FeatureDag {
        FeatureDag {
            nodes: BTreeMap::new(),
            tip: None,
            cache: Cache::new(),
            settings,
        }
    }

    /// The settings the history works with.
    pub fn settings(&self) -> &HistorySettings {
        &self.settings
    }

    /// The nodes, by id.
    pub fn nodes(&self) -> &BTreeMap<Id, Node> {
        &self.nodes
    }

    /// The node of an id.
    pub fn node(&self, id: Id) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// The id of the node of a name.
    pub fn id_of(&self, name: &str) -> Option<Id> {
        self.nodes.values().find(|n| n.name == name).map(|n| n.id)
    }

    /// The node that gives the result of the history.
    pub fn tip(&self) -> Option<Id> {
        self.tip
    }

    /// The number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the history has no node.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The nodes that take the node as an input or name it in an expression.
    pub fn dependents_of(&self, id: Id, cfg: &Settings) -> Result<Vec<Id>, CadError> {
        self.nodes.get(&id).ok_or_else(|| missing(id))?;
        self.graph(cfg)?.dependents(id)
    }

    /// The nodes that must be built again when a node changes: the node itself and everything
    /// that depends on it, directly or not, in the order they are built in.
    pub fn affected_by(&self, id: Id, cfg: &Settings) -> Result<Vec<Id>, CadError> {
        self.nodes.get(&id).ok_or_else(|| missing(id))?;
        self.graph(cfg)?.affected_by(id)
    }

    // ----- checking -----

    fn parameter_ids(&self) -> BTreeMap<&str, Id> {
        self.nodes
            .values()
            .filter(|n| matches!(n.kind, Kind::Parameter { .. }))
            .map(|n| (n.name.as_str(), n.id))
            .collect()
    }

    fn deps_of(
        &self,
        n: &Node,
        params: &BTreeMap<&str, Id>,
        limits: &ExprLimits,
    ) -> Result<Vec<Id>, CadError> {
        let mut deps: BTreeSet<Id> = BTreeSet::new();
        let mut texts: Vec<&str> = Vec::new();
        match &n.kind {
            Kind::Parameter { expr } => texts.push(expr),
            Kind::Feature {
                inputs, params: p, ..
            } => {
                deps.extend(inputs.iter().copied());
                texts.extend(p.values().map(String::as_str));
            }
        }
        for text in texts {
            for name in names_of(text, limits)? {
                if let Some(id) = params.get(name.as_str()) {
                    deps.insert(*id);
                }
            }
        }
        Ok(deps.into_iter().collect())
    }

    fn node_hash(&self, n: &Node, constants: u64) -> u64 {
        let mut parts = vec![constants];
        match &n.kind {
            Kind::Parameter { expr } => {
                parts.push(1);
                parts.extend(text_parts(expr));
            }
            Kind::Feature {
                op,
                inputs,
                params,
                suppressed,
            } => {
                parts.push(2);
                parts.push(*op as u64);
                parts.push(u64::from(*suppressed));
                parts.push(inputs.len() as u64);
                for i in inputs {
                    parts.extend(id_parts(*i));
                }
                for (k, v) in params {
                    parts.extend(text_parts(k));
                    parts.extend(text_parts(v));
                }
            }
        }
        fnv64(&parts)
    }

    fn graph(&self, cfg: &Settings) -> Result<Graph, CadError> {
        let ch = constants_hash(&cfg.constants);
        let params = self.parameter_ids();
        let mut g = Graph::new();
        for n in self.nodes.values() {
            g.add(n.id, &n.name, self.node_hash(n, ch), &[])?;
        }
        for n in self.nodes.values() {
            g.set_deps(n.id, &self.deps_of(n, &params, &cfg.expr)?)?;
        }
        Ok(g)
    }

    /// Checks the whole history: sizes, names, the shape of every node, the expressions and the
    /// graph (a cycle is `PHOENIX-CAD-013`).
    fn validate(&self, cfg: &Settings) -> Result<(), CadError> {
        if self.nodes.len() > self.settings.max_nodes {
            return Err(bad(format!(
                "{} nodes, the settings allow {}",
                self.nodes.len(),
                self.settings.max_nodes
            )));
        }
        let mut names: BTreeSet<&str> = BTreeSet::new();
        for n in self.nodes.values() {
            check_name(&n.name)?;
            if !names.insert(n.name.as_str()) {
                return Err(bad(format!("the name {} is used twice", n.name)));
            }
            if n.extra.len() > self.settings.max_extra_lines
                || n.extra.iter().any(|l| {
                    l.len() > self.settings.max_extra_bytes
                        || !l.starts_with("x-")
                        || l.chars().any(char::is_control)
                })
            {
                return Err(bad(format!("{}: the lines kept are not valid", n.name)));
            }
            match &n.kind {
                Kind::Parameter { expr } => check_expr(expr, &n.name, &cfg.expr)?,
                Kind::Feature {
                    op, inputs, params, ..
                } => {
                    if inputs.len() != op.inputs() {
                        return Err(bad(format!(
                            "{}: {} takes {} inputs, not {}",
                            n.name,
                            op.name(),
                            op.inputs(),
                            inputs.len()
                        )));
                    }
                    for i in inputs {
                        match self.nodes.get(i) {
                            Some(Node {
                                kind: Kind::Feature { .. },
                                ..
                            }) => {}
                            Some(_) => {
                                return Err(bad(format!("{}: an input must be a feature", n.name)))
                            }
                            None => return Err(bad(format!("{}: no node {i}", n.name))),
                        }
                    }
                    let keys: Vec<&str> = params.keys().map(String::as_str).collect();
                    let mut want: Vec<&str> = op.keys().to_vec();
                    want.sort_unstable();
                    if keys != want {
                        return Err(bad(format!(
                            "{}: {} has the parameters {}",
                            n.name,
                            op.name(),
                            op.keys().join(" ")
                        )));
                    }
                    for e in params.values() {
                        check_expr(e, &n.name, &cfg.expr)?;
                    }
                }
            }
        }
        if let Some(t) = self.tip {
            match self.nodes.get(&t) {
                Some(Node {
                    kind: Kind::Feature { .. },
                    ..
                }) => {}
                _ => return Err(bad(format!("the tip {t} is not a feature of the history"))),
            }
        }
        self.graph(cfg)?;
        Ok(())
    }

    /// Makes a change and keeps it only when the whole history is still valid; otherwise the
    /// history is exactly as it was.
    fn edit(
        &mut self,
        cfg: &Settings,
        change: impl FnOnce(&mut FeatureDag) -> Result<(), CadError>,
    ) -> Result<(), CadError> {
        let (nodes, tip) = (self.nodes.clone(), self.tip);
        let result = change(self).and_then(|()| self.validate(cfg));
        if result.is_err() {
            self.nodes = nodes;
            self.tip = tip;
        }
        result
    }

    // ----- editing -----

    /// Adds a parameter. The expression is kept in its canonical form.
    pub fn add_parameter(
        &mut self,
        id: Id,
        name: &str,
        expr: &str,
        cfg: &Settings,
    ) -> Result<(), CadError> {
        self.edit(cfg, |d| {
            if d.nodes.contains_key(&id) {
                return Err(CadError::Duplicate(id.to_string()));
            }
            let expr = canonical(expr, &cfg.expr)?;
            d.nodes.insert(
                id,
                Node {
                    id,
                    name: name.to_string(),
                    kind: Kind::Parameter { expr },
                    extra: Vec::new(),
                },
            );
            Ok(())
        })
    }

    /// Adds a feature with the nodes it takes and the expression of each of its parameters (all
    /// of them, by their keys).
    pub fn add_feature(
        &mut self,
        id: Id,
        name: &str,
        op: Op,
        inputs: &[Id],
        params: &[(&str, &str)],
        cfg: &Settings,
    ) -> Result<(), CadError> {
        self.edit(cfg, |d| {
            if d.nodes.contains_key(&id) {
                return Err(CadError::Duplicate(id.to_string()));
            }
            let mut map = BTreeMap::new();
            for (k, e) in params {
                if map
                    .insert((*k).to_string(), canonical(e, &cfg.expr)?)
                    .is_some()
                {
                    return Err(bad(format!("{name}: the parameter {k} is given twice")));
                }
            }
            d.nodes.insert(
                id,
                Node {
                    id,
                    name: name.to_string(),
                    kind: Kind::Feature {
                        op,
                        inputs: inputs.to_vec(),
                        params: map,
                        suppressed: false,
                    },
                    extra: Vec::new(),
                },
            );
            Ok(())
        })
    }

    /// Changes the expression of a parameter (`key` is `None`) or of a parameter of a feature.
    pub fn set_expression(
        &mut self,
        id: Id,
        key: Option<&str>,
        expr: &str,
        cfg: &Settings,
    ) -> Result<(), CadError> {
        self.edit(cfg, |d| {
            let text = canonical(expr, &cfg.expr)?;
            let node = d.nodes.get_mut(&id).ok_or_else(|| missing(id))?;
            match (&mut node.kind, key) {
                (Kind::Parameter { expr }, None) => *expr = text,
                (Kind::Feature { params, .. }, Some(k)) => match params.get_mut(k) {
                    Some(slot) => *slot = text,
                    None => return Err(bad(format!("{}: no parameter {k}", node.name))),
                },
                _ => {
                    return Err(bad(format!(
                        "{}: the expression to change is not the one of this node",
                        node.name
                    )))
                }
            }
            Ok(())
        })
    }

    /// Changes the inputs of a feature. A node that would depend on itself, directly or through
    /// others, is refused with `PHOENIX-CAD-013`.
    pub fn set_inputs(&mut self, id: Id, inputs: &[Id], cfg: &Settings) -> Result<(), CadError> {
        self.edit(cfg, |d| {
            let node = d.nodes.get_mut(&id).ok_or_else(|| missing(id))?;
            match &mut node.kind {
                Kind::Feature { inputs: slot, .. } => *slot = inputs.to_vec(),
                Kind::Parameter { .. } => {
                    return Err(bad(format!("{}: a parameter has no inputs", node.name)))
                }
            }
            Ok(())
        })
    }

    /// Skips a feature, or puts it back.
    pub fn set_suppressed(&mut self, id: Id, on: bool, cfg: &Settings) -> Result<(), CadError> {
        self.edit(cfg, |d| {
            let node = d.nodes.get_mut(&id).ok_or_else(|| missing(id))?;
            match &mut node.kind {
                Kind::Feature { suppressed, .. } => *suppressed = on,
                Kind::Parameter { .. } => {
                    return Err(bad(format!("{}: a parameter is not suppressed", node.name)))
                }
            }
            Ok(())
        })
    }

    /// Chooses the feature that gives the result of the history, or none.
    pub fn set_tip(&mut self, tip: Option<Id>, cfg: &Settings) -> Result<(), CadError> {
        self.edit(cfg, |d| {
            d.tip = tip;
            Ok(())
        })
    }

    /// Keeps a line that this program does not know (it starts with `x-`) in the record of a node.
    pub fn add_extra(&mut self, id: Id, line: &str, cfg: &Settings) -> Result<(), CadError> {
        self.edit(cfg, |d| {
            let node = d.nodes.get_mut(&id).ok_or_else(|| missing(id))?;
            node.extra.push(line.to_string());
            Ok(())
        })
    }

    /// Takes a node away. A node that another one uses is refused (`PHOENIX-CAD-016`); the tip
    /// goes with the node it was.
    pub fn remove(&mut self, id: Id, cfg: &Settings) -> Result<(), CadError> {
        self.edit(cfg, |d| {
            if !d.nodes.contains_key(&id) {
                return Err(missing(id));
            }
            let used = d.graph(cfg)?.dependents(id)?;
            if let Some(first) = used.first() {
                let name = d.nodes.get(first).map_or("", |n| n.name.as_str());
                return Err(CadError::InUse(format!(
                    "{} is used by {name}",
                    d.nodes.get(&id).map_or("", |n| n.name.as_str())
                )));
            }
            d.nodes.remove(&id);
            d.cache.remove(&id);
            if d.tip == Some(id) {
                d.tip = None;
            }
            Ok(())
        })
    }

    // ----- building -----

    /// The order the nodes are built in.
    pub fn order(&self, cfg: &Settings) -> Result<Vec<Id>, CadError> {
        self.graph(cfg)?.order()
    }

    /// The graph as a Graphviz file.
    pub fn to_dot(&self, cfg: &Settings) -> Result<String, CadError> {
        self.graph(cfg)?.to_dot()
    }

    /// Builds the history again: only the nodes whose definition or inputs changed since the last
    /// time are calculated. A node that fails does not stop the others.
    pub fn rebuild(&mut self, cfg: &Settings) -> Result<Rebuild, CadError> {
        self.validate(cfg)?;
        let graph = self.graph(cfg)?;
        let nodes = &self.nodes;
        let limits = self.settings.limits();
        let max_array = self.settings.max_array;
        let mut cache = std::mem::take(&mut self.cache);
        let params_by_name: BTreeMap<String, Id> = self
            .parameter_ids()
            .into_iter()
            .map(|(n, i)| (n.to_string(), i))
            .collect();
        let outcome = graph.recompute(&mut cache, |id, inputs| {
            let node = &nodes[&id];
            let deps = graph.dependencies(id)?;
            let by_id: BTreeMap<Id, &Product> =
                deps.iter().copied().zip(inputs.iter().copied()).collect();
            compute(
                node,
                &by_id,
                &params_by_name,
                nodes,
                cfg,
                &limits,
                max_array,
            )
        });
        self.cache = cache;
        let states = outcome?;
        let mut reports: Vec<NodeReport> = Vec::new();
        for id in graph.order()? {
            let node = &self.nodes[&id];
            let suppressed = matches!(
                node.kind,
                Kind::Feature {
                    suppressed: true,
                    ..
                }
            );
            let (state, code, detail) = match &states[&id] {
                NodeState::Computed if suppressed => (BuildState::Suppressed, None, String::new()),
                NodeState::Reused if suppressed => (BuildState::Suppressed, None, String::new()),
                NodeState::Computed => (BuildState::Computed, None, String::new()),
                NodeState::Reused => (BuildState::Reused, None, String::new()),
                NodeState::Failed(e) => (BuildState::Failed, Some(e.code()), e.to_string()),
                NodeState::Inherited(from) => {
                    let root = reports.iter().find(|r| r.id == *from);
                    (
                        BuildState::Inherited,
                        root.and_then(|r| r.code),
                        format!("depends on {}", self.nodes[from].name),
                    )
                }
            };
            reports.push(NodeReport {
                id,
                name: node.name.clone(),
                state,
                code,
                detail,
            });
        }
        Ok(Rebuild { reports })
    }

    /// The solid a feature gave in the last rebuild.
    pub fn solid(&self, id: Id) -> Option<&Solid> {
        match self.cache.get(&id).map(|c| &c.value) {
            Some(Product::Solid(s)) => Some(s),
            _ => None,
        }
    }

    /// The value a parameter gave in the last rebuild.
    pub fn value(&self, name: &str) -> Option<Value> {
        let id = self.id_of(name)?;
        match self.cache.get(&id).map(|c| &c.value) {
            Some(Product::Value(v)) => Some(*v),
            _ => None,
        }
    }

    /// The value of the expression of one parameter of a feature (its `count`, its `sx`...), worked
    /// out with the parameters as the last rebuild left them.
    pub fn eval_param(&self, id: Id, key: &str, cfg: &Settings) -> Result<Value, CadError> {
        let node = self.nodes.get(&id).ok_or_else(|| missing(id))?;
        let Kind::Feature { params, .. } = &node.kind else {
            return Err(CadError::FeatureFailed(format!(
                "{}: not a feature",
                node.name
            )));
        };
        let text = params
            .get(key)
            .ok_or_else(|| CadError::FeatureFailed(format!("{}: no parameter {key}", node.name)))?;
        let mut local: BTreeMap<String, Value> = BTreeMap::new();
        for (name, pid) in self.parameter_ids() {
            if let Some(Product::Value(v)) = self.cache.get(&pid).map(|c| &c.value) {
                local.insert(name.to_string(), *v);
            }
        }
        let env = Env {
            local,
            constants: &cfg.constants,
        };
        parse(text, &cfg.expr)?.eval(&env, &cfg.expr)
    }

    /// The solid of the tip in the last rebuild, or, when no tip is chosen, of the only feature
    /// that no other one takes.
    pub fn result(&self) -> Option<&Solid> {
        if let Some(t) = self.tip {
            return self.solid(t);
        }
        let used: BTreeSet<Id> = self
            .nodes
            .values()
            .flat_map(|n| match &n.kind {
                Kind::Feature { inputs, .. } => inputs.clone(),
                Kind::Parameter { .. } => Vec::new(),
            })
            .collect();
        let mut sinks = self
            .nodes
            .values()
            .filter(|n| matches!(n.kind, Kind::Feature { .. }) && !used.contains(&n.id));
        match (sinks.next(), sinks.next()) {
            (Some(only), None) => self.solid(only.id),
            _ => None,
        }
    }

    // ----- the file -----

    /// The text of the history in its one canonical form: the same history gives the same bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = String::new();
        out.push_str(HEADER);
        out.push('\n');
        if let Some(t) = self.tip {
            out.push_str(&format!("tip {t}\n"));
        }
        for n in self.nodes.values() {
            match &n.kind {
                Kind::Parameter { expr } => {
                    out.push_str(&format!("parameter {} {} {expr}\n", n.id, n.name));
                }
                Kind::Feature {
                    op,
                    inputs,
                    params,
                    suppressed,
                } => {
                    out.push_str(&format!("feature {} {} {}\n", n.id, op.name(), n.name));
                    for i in inputs {
                        out.push_str(&format!("input {i}\n"));
                    }
                    for (k, v) in params {
                        out.push_str(&format!("param {k} {v}\n"));
                    }
                    if *suppressed {
                        out.push_str("suppressed 1\n");
                    }
                }
            }
            for line in &n.extra {
                out.push_str(line);
                out.push('\n');
            }
        }
        out.into_bytes()
    }

    /// Reads a history. Whatever is wrong is a `PHOENIX-CAD-051` that names the line (or the
    /// code of the expression or of the cycle that was found); text that is not in the one
    /// canonical form is refused too, so what is read is what is written.
    pub fn from_bytes(
        bytes: &[u8],
        settings: HistorySettings,
        cfg: &Settings,
    ) -> Result<FeatureDag, CadError> {
        if bytes.len() > settings.max_file_bytes {
            return Err(bad(format!(
                "{} bytes, the settings allow {}",
                bytes.len(),
                settings.max_file_bytes
            )));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| bad("the history is not text"))?;
        let mut lines = text.split('\n');
        if lines.next() != Some(HEADER) {
            return Err(bad(
                "the first line is not `history 1` (a newer version is not read)",
            ));
        }
        let mut dag = FeatureDag::new(settings);
        let mut last: Option<Id> = None;
        let lines: Vec<&str> = lines.collect();
        for (n, line) in lines.iter().enumerate() {
            let at = n + 2;
            let last_line = n + 1 == lines.len();
            if line.is_empty() {
                if last_line {
                    continue;
                }
                return Err(bad(format!("line {at}: empty")));
            }
            if last_line {
                return Err(bad(format!(
                    "line {at}: the text does not end with a line end"
                )));
            }
            if line.chars().any(char::is_control) {
                return Err(bad(format!("line {at}: a control character")));
            }
            let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
            let id_of = |s: &str| -> Result<Id, CadError> {
                s.parse::<Id>()
                    .map_err(|_| bad(format!("line {at}: {s:?} is not an id")))
            };
            match word {
                "tip" => {
                    if dag.tip.is_some() || last.is_some() {
                        return Err(bad(format!("line {at}: the tip comes first and only once")));
                    }
                    dag.tip = Some(id_of(rest)?);
                }
                "parameter" | "feature" => {
                    let mut it = rest.splitn(3, ' ');
                    let (id, second, third) = match (it.next(), it.next(), it.next()) {
                        (Some(i), Some(x), Some(y)) => (id_of(i)?, x, y),
                        _ => {
                            return Err(bad(format!(
                                "line {at}: {word} needs three fields after the word"
                            )))
                        }
                    };
                    if dag.nodes.contains_key(&id) {
                        return Err(bad(format!("line {at}: the id {id} is used twice")));
                    }
                    let (name, kind) = if word == "parameter" {
                        (
                            second,
                            Kind::Parameter {
                                expr: third.to_string(),
                            },
                        )
                    } else {
                        let op = Op::from_name(second)
                            .ok_or_else(|| bad(format!("line {at}: no operation {second:?}")))?;
                        (
                            third,
                            Kind::Feature {
                                op,
                                inputs: Vec::new(),
                                params: BTreeMap::new(),
                                suppressed: false,
                            },
                        )
                    };
                    dag.nodes.insert(
                        id,
                        Node {
                            id,
                            name: name.to_string(),
                            kind,
                            extra: Vec::new(),
                        },
                    );
                    last = Some(id);
                }
                "input" | "param" | "suppressed" => {
                    let node = last.and_then(|l| dag.nodes.get_mut(&l));
                    let Some(Node {
                        kind:
                            Kind::Feature {
                                inputs,
                                params,
                                suppressed,
                                ..
                            },
                        extra,
                        ..
                    }) = node
                    else {
                        return Err(bad(format!("line {at}: {word} outside a feature")));
                    };
                    if !extra.is_empty() {
                        return Err(bad(format!("line {at}: {word} after a line to keep")));
                    }
                    match word {
                        "input" => inputs.push(id_of(rest)?),
                        "suppressed" => {
                            if rest != "1" {
                                return Err(bad(format!("line {at}: suppressed is 1 or absent")));
                            }
                            *suppressed = true;
                        }
                        _ => {
                            let Some((k, e)) = rest.split_once(' ') else {
                                return Err(bad(format!(
                                    "line {at}: a parameter needs a key and an expression"
                                )));
                            };
                            if params.insert(k.to_string(), e.to_string()).is_some() {
                                return Err(bad(format!(
                                    "line {at}: the parameter {k} is given twice"
                                )));
                            }
                        }
                    }
                }
                w if w.starts_with("x-") => {
                    // a line of a newer program, kept where it was
                    let Some(node) = last.and_then(|l| dag.nodes.get_mut(&l)) else {
                        return Err(bad(format!("line {at}: a line to keep before any node")));
                    };
                    node.extra.push((*line).to_string());
                }
                _ => return Err(bad(format!("line {at}: not understood"))),
            }
        }
        dag.validate(cfg)?;
        if dag.to_bytes() != bytes {
            return Err(bad("the history is not in the one canonical form"));
        }
        Ok(dag)
    }
}

fn missing(id: Id) -> CadError {
    CadError::UnknownName(id.to_string())
}

/// Calculates one node from the results of the nodes it depends on.
fn compute(
    node: &Node,
    by_id: &BTreeMap<Id, &Product>,
    params_by_name: &BTreeMap<String, Id>,
    nodes: &BTreeMap<Id, Node>,
    cfg: &Settings,
    limits: &Limits,
    max_array: u32,
) -> Result<Product, CadError> {
    let fail = |e: CadError| -> CadError {
        match e {
            CadError::ExprSyntax { .. }
            | CadError::ExprEval { .. }
            | CadError::ExprLimit { .. }
            | CadError::ExprOverflow { .. }
            | CadError::PrismLimit(_)
            | CadError::FeatureFailed(_) => e,
            other => CadError::FeatureFailed(format!("{}: {other}", node.name)),
        }
    };
    // the parameters that the node can see, by name
    let mut local: BTreeMap<String, Value> = BTreeMap::new();
    for (name, id) in params_by_name {
        if let Some(Product::Value(v)) = by_id.get(id) {
            local.insert(name.clone(), *v);
        }
    }
    let env = Env {
        local,
        constants: &cfg.constants,
    };
    let eval =
        |text: &str| -> Result<Value, CadError> { parse(text, &cfg.expr)?.eval(&env, &cfg.expr) };
    match &node.kind {
        Kind::Parameter { expr } => eval(expr).map(Product::Value).map_err(fail),
        Kind::Feature {
            op,
            inputs,
            params,
            suppressed,
        } => {
            let solid_of = |i: usize| -> Result<Solid, CadError> {
                let id = inputs.get(i).ok_or_else(|| {
                    CadError::FeatureFailed(format!("{}: no input {i}", node.name))
                })?;
                match by_id.get(id) {
                    Some(Product::Solid(s)) => Ok(s.clone()),
                    _ => Err(CadError::FeatureFailed(format!(
                        "{}: the input {} has no solid",
                        node.name,
                        nodes.get(id).map_or("", |n| n.name.as_str())
                    ))),
                }
            };
            if *suppressed {
                let passed = if inputs.is_empty() {
                    Solid::empty()
                } else {
                    solid_of(0).map_err(fail)?
                };
                return Ok(Product::Solid(passed));
            }
            let length = |key: &str| -> Result<Length, CadError> {
                let text = params.get(key).ok_or_else(|| {
                    CadError::FeatureFailed(format!("{}: no parameter {key}", node.name))
                })?;
                eval(text)?.to_length(0)
            };
            // a parameter that has to be a whole number inside a range
            let whole = |key: &str, low: i128, high: i128| -> Result<i128, CadError> {
                let text = params.get(key).ok_or_else(|| {
                    CadError::FeatureFailed(format!("{}: no parameter {key}", node.name))
                })?;
                match eval(text)? {
                    Value::Number(r) if r.is_integer() && (low..=high).contains(&r.numer()) => {
                        Ok(r.numer())
                    }
                    _ => Err(CadError::FeatureFailed(format!(
                        "{}: {key} is not a whole number from {low} to {high}",
                        node.name
                    ))),
                }
            };
            let solid: Result<Solid, CadError> = (|| match op {
                Op::Box => {
                    let p = Prism::new(
                        [length("x")?, length("y")?, length("z")?],
                        [length("sx")?, length("sy")?, length("sz")?],
                    )?;
                    Ok(Solid::from_prism(p))
                }
                Op::Move => solid_of(0)?.translate([length("dx")?, length("dy")?, length("dz")?]),
                Op::Array => {
                    let n = match eval(params.get("count").map_or("1", String::as_str))? {
                        Value::Number(r) if r.is_integer() => r.numer(),
                        _ => {
                            return Err(CadError::FeatureFailed(format!(
                                "{}: count is not a whole number",
                                node.name
                            )))
                        }
                    };
                    if n < 1 || n > i128::from(max_array) {
                        return Err(CadError::FeatureFailed(format!(
                            "{}: count {n} is outside 1 to {max_array}",
                            node.name
                        )));
                    }
                    let step = [length("dx")?, length("dy")?, length("dz")?];
                    let base = solid_of(0)?;
                    let mut all = base.clone();
                    for k in 1..n {
                        let mut by = [Length::ZERO; 3];
                        for i in 0..3 {
                            let nm = step[i].get().checked_mul(k as i64).ok_or_else(|| {
                                CadError::FeatureFailed(format!(
                                    "{}: a copy is outside the range of lengths",
                                    node.name
                                ))
                            })?;
                            by[i] = Length::new(nm)?;
                        }
                        all = all.union(&base.translate(by)?, limits)?;
                    }
                    Ok(all)
                }
                Op::Mirror => {
                    let axis = whole("axis", 0, 2)?;
                    let base = solid_of(0)?;
                    let reflected = base.mirrored(axis as usize, length("at")?, limits)?;
                    base.apply(Boolean::Union, &reflected, limits)
                }
                Op::Rotate => {
                    let axis = whole("axis", 0, 2)?;
                    let quarters = whole("quarters", -1_000_000, 1_000_000)?;
                    solid_of(0)?.rotated(
                        axis as usize,
                        quarters as i64,
                        [length("cx")?, length("cy")?, length("cz")?],
                        limits,
                    )
                }
                Op::Union => solid_of(0)?.apply(Boolean::Union, &solid_of(1)?, limits),
                Op::Difference => solid_of(0)?.apply(Boolean::Difference, &solid_of(1)?, limits),
                Op::Intersection => {
                    solid_of(0)?.apply(Boolean::Intersection, &solid_of(1)?, limits)
                }
            })();
            solid.map(Product::Solid).map_err(fail)
        }
    }
}

impl Document {
    /// Writes the history into the domain `history` (critical: a program that does not know it
    /// opens the project read-only).
    pub fn set_history(&mut self, dag: &FeatureDag) -> Result<(), CadError> {
        self.set(HISTORY_DOMAIN, true, dag.to_bytes())
    }

    /// Reads the history, or `None` when the project has none.
    pub fn history(
        &self,
        settings: HistorySettings,
        cfg: &Settings,
    ) -> Result<Option<FeatureDag>, CadError> {
        match self.get(HISTORY_DOMAIN) {
            None => Ok(None),
            Some(d) => FeatureDag::from_bytes(&d.data, settings, cfg).map(Some),
        }
    }
}

#[cfg(test)]
mod tests;
