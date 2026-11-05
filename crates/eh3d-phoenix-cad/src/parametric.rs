//! The parameters of an enclosure as values, variables or expressions.
//!
//! A [`ParamModel`] holds the base parameters of an enclosure and, for any of them, a
//! [`Parameter`]: a fixed length, the name of a variable, or an expression (see [`crate::expr`])
//! that may use variables, the constants of the settings and the other parameters by their names
//! (`wall`, `inner_length`, ...). The dependencies go to the graph of [`crate::graph`]: one object
//! for each variable and one for each parameter, each parameter depending on the variables and the
//! parameters its expression names. A definition that would make a parameter depend on itself is
//! refused with `PHOENIX-CAD-013` and the model stays as it was.
//!
//! [`ParamModel::resolve_with`] calculates the parameters through that graph and keeps the results
//! in a cache: when only a variable changes, only the parameters that depend on it are calculated
//! again. A parameter takes its value rounded to the nanometre, so an expression that names it sees
//! the same length as the enclosure does. A name is looked up first among the parameters, then
//! among the variables, then among the constants.

use std::collections::BTreeMap;

use eh3d_container::ObjectId;

use crate::enclosure::{Enclosure, EnclosureParams, Id};
use crate::expr::{parse, Budget, Constants, Expr, ExprLimits, Names, Value};
use crate::graph::{fnv64, Cache, Graph, NodeState};
use crate::vars::{check_name, Param, Variables};
use crate::{CadError, Length};

/// How one parameter of an enclosure is defined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parameter {
    /// A fixed length.
    Fixed(Length),
    /// The value of a variable.
    Variable(String),
    /// An expression, which must give a length.
    Expr(Expr),
}

/// The parameters of an enclosure: a base, and a definition for any of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamModel {
    base: EnclosureParams,
    entries: BTreeMap<Param, Parameter>,
}

/// The result of calculating the parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The parameters of the enclosure.
    pub params: EnclosureParams,
    /// What happened to each parameter: calculated, or reused from the cache.
    pub states: BTreeMap<Param, NodeState>,
}

enum Def<'a> {
    Var(Length),
    Leaf(Length),
    Bound,
    Expr(&'a Expr),
}

struct Built<'a> {
    graph: Graph,
    defs: BTreeMap<Id, Def<'a>>,
    params: Vec<(Param, Id)>,
    param_ids: BTreeMap<String, Id>,
    var_ids: BTreeMap<String, Id>,
}

impl Built<'_> {
    /// The object a name refers to: a parameter first, then a variable.
    fn lookup(&self, name: &str) -> Option<Id> {
        self.param_ids
            .get(name)
            .or_else(|| self.var_ids.get(name))
            .copied()
    }
}

/// The values a calculation sees: the objects the expression depends on, then the constants.
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

/// The text as numbers: its length, then its bytes eight at a time.
fn text_parts(text: &str) -> Vec<u64> {
    let mut parts = vec![text.len() as u64];
    for chunk in text.as_bytes().chunks(8) {
        let mut bytes = [0u8; 8];
        bytes[..chunk.len()].copy_from_slice(chunk);
        parts.push(u64::from_le_bytes(bytes));
    }
    parts
}

/// The id of an object, made from its kind and its name so that it is the same in every
/// calculation: the saved results stay good when other variables are added or removed.
fn node_id(kind: u64, name: &str) -> Result<Id, CadError> {
    let mut parts = vec![kind];
    parts.extend(text_parts(name));
    let h = fnv64(&parts);
    ObjectId::from_parts(1_700_000_000_000, (h & 0x0fff) as u16, h)
        .map_err(|_| CadError::ValueOutOfRange("id"))
}

fn leaf(length: Length) -> (Def<'static>, u64) {
    (Def::Leaf(length), fnv64(&[1, length.get() as u64]))
}

impl ParamModel {
    /// A model whose parameters are all the base ones; the base must make a valid enclosure.
    pub fn new(base: EnclosureParams) -> Result<ParamModel, CadError> {
        Enclosure::new(base)?;
        Ok(ParamModel {
            base,
            entries: BTreeMap::new(),
        })
    }

    /// The base parameters, which a parameter without a definition keeps.
    pub fn base(&self) -> &EnclosureParams {
        &self.base
    }

    /// Replaces the base parameters; they must make a valid enclosure.
    pub fn set_base(&mut self, base: EnclosureParams) -> Result<(), CadError> {
        Enclosure::new(base)?;
        self.base = base;
        Ok(())
    }

    /// How a parameter is defined, or `None` when it keeps the base value.
    pub fn get(&self, param: Param) -> Option<&Parameter> {
        self.entries.get(&param)
    }

    /// The defined parameters, in the order of [`Param::ALL`].
    pub fn entries(&self) -> impl Iterator<Item = (Param, &Parameter)> {
        self.entries.iter().map(|(p, v)| (*p, v))
    }

    /// Defines a parameter. A definition that would make a parameter depend on itself, directly
    /// or through other parameters, is refused with `PHOENIX-CAD-013` and nothing changes; so is
    /// the name of a variable that is not a plain word.
    pub fn set(&mut self, param: Param, parameter: Parameter) -> Result<(), CadError> {
        if let Parameter::Variable(name) = &parameter {
            check_name(name)?;
        }
        let mut next = self.clone();
        next.entries.insert(param, parameter);
        next.check_cycles()?;
        *self = next;
        Ok(())
    }

    /// Reads an expression and defines a parameter with it.
    pub fn set_expr(
        &mut self,
        param: Param,
        text: &str,
        limits: &ExprLimits,
    ) -> Result<(), CadError> {
        self.set(param, Parameter::Expr(parse(text, limits)?))
    }

    /// Takes the definition of a parameter away: it keeps the base value again.
    pub fn clear(&mut self, param: Param) {
        self.entries.remove(&param);
    }

    /// Looks for a cycle among the parameters only (the names of variables and constants are not
    /// known here).
    fn check_cycles(&self) -> Result<(), CadError> {
        self.build(&Variables::new(), None)?;
        Ok(())
    }

    /// The graph of the parameters and the variables. With `consts` the names must all exist; without
    /// them a name that is not a parameter is ignored (the check for cycles needs no more).
    fn build<'a>(
        &'a self,
        vars: &Variables,
        consts: Option<&Constants>,
    ) -> Result<Built<'a>, CadError> {
        let mut graph = Graph::new();
        let mut defs: BTreeMap<Id, Def<'a>> = BTreeMap::new();
        let mut var_ids = BTreeMap::new();
        for name in vars.names() {
            let value = vars.require(name)?;
            let id = node_id(2, name)?;
            graph.add(id, name, fnv64(&[2, value.get() as u64]), &[])?;
            defs.insert(id, Def::Var(value));
            var_ids.insert(name.to_string(), id);
        }
        let mut params = Vec::new();
        let mut param_ids = BTreeMap::new();
        for param in Param::ALL {
            let id = node_id(1, param.name())?;
            param_ids.insert(param.name().to_string(), id);
            params.push((param, id));
        }
        let mut plan: Vec<(Id, Vec<Id>)> = Vec::new();
        for (param, id) in &params {
            let mut deps: Vec<Id> = Vec::new();
            let (def, hash) = match self.entries.get(param) {
                None => leaf(param.get(&self.base)),
                Some(Parameter::Fixed(length)) => leaf(*length),
                Some(Parameter::Variable(name)) => {
                    match var_ids.get(name) {
                        Some(var) => deps.push(*var),
                        None => {
                            if consts.is_some() {
                                return Err(CadError::UnknownName(name.clone()));
                            }
                        }
                    }
                    (Def::Bound, fnv64(&[3]))
                }
                Some(Parameter::Expr(expr)) => {
                    let mut parts = vec![4];
                    parts.extend(text_parts(&expr.to_text()));
                    for (name, pos) in expr.names() {
                        if let Some(dep) = param_ids.get(&name).or_else(|| var_ids.get(&name)) {
                            deps.push(*dep);
                        } else if let Some(constants) = consts {
                            match constants.get(&name) {
                                Some(value) => parts.extend(value.hash_parts()),
                                None => {
                                    return Err(CadError::UnknownName(format!(
                                        "{name} (column {})",
                                        pos + 1
                                    )))
                                }
                            }
                        }
                    }
                    (Def::Expr(expr), fnv64(&parts))
                }
            };
            graph.add(*id, param.name(), hash, &[])?;
            defs.insert(*id, def);
            plan.push((*id, deps));
        }
        for ((param, _), (id, deps)) in params.iter().zip(&plan) {
            graph.set_deps(*id, deps).map_err(|e| match e {
                CadError::Cycle(_) => CadError::Cycle(format!(
                    "the definition of {} depends on itself",
                    param.name()
                )),
                other => other,
            })?;
        }
        Ok(Built {
            graph,
            defs,
            params,
            param_ids,
            var_ids,
        })
    }

    /// Calculates the parameters. Everything is calculated from nothing; see
    /// [`ParamModel::resolve_with`] to keep the results between two calculations.
    pub fn resolve(
        &self,
        vars: &Variables,
        consts: &Constants,
        limits: &ExprLimits,
    ) -> Result<EnclosureParams, CadError> {
        let mut cache = Cache::new();
        Ok(self.resolve_with(vars, consts, limits, &mut cache)?.params)
    }

    /// Calculates the parameters through the graph, reusing the results of `cache` that are still
    /// good: a parameter is calculated again only when its definition changed or something it
    /// depends on did. The first parameter (in the order of [`Param::ALL`]) that fails gives its
    /// error. The parameters must make a valid enclosure.
    pub fn resolve_with(
        &self,
        vars: &Variables,
        consts: &Constants,
        limits: &ExprLimits,
        cache: &mut Cache<Value>,
    ) -> Result<Resolved, CadError> {
        let built = self.build(vars, Some(consts))?;
        let mut budget = Budget::new(limits);
        let states = built.graph.recompute(
            cache,
            |id: Id, inputs: &[&Value]| -> Result<Value, CadError> {
                match built.defs.get(&id) {
                    Some(Def::Var(length)) | Some(Def::Leaf(length)) => {
                        Ok(Value::from_length(*length))
                    }
                    Some(Def::Bound) => inputs
                        .first()
                        .map(|value| **value)
                        .ok_or_else(|| CadError::UnknownName(id.to_string())),
                    Some(Def::Expr(expr)) => {
                        let deps = built.graph.dependencies(id)?;
                        let mut by_id: BTreeMap<Id, Value> = BTreeMap::new();
                        for (dep, value) in deps.iter().zip(inputs.iter()) {
                            by_id.insert(*dep, **value);
                        }
                        let mut local = BTreeMap::new();
                        for (name, _) in expr.names() {
                            if let Some(value) =
                                built.lookup(&name).and_then(|dep| by_id.get(&dep).copied())
                            {
                                local.insert(name, value);
                            }
                        }
                        let env = Env {
                            local,
                            constants: consts,
                        };
                        let value = expr.eval_with(&env, limits, &mut budget)?;
                        Ok(Value::from_length(value.to_length(0)?))
                    }
                    None => Err(CadError::UnknownName(id.to_string())),
                }
            },
        )?;
        let mut report = BTreeMap::new();
        for (param, id) in &built.params {
            if let Some(state) = states.get(id) {
                report.insert(*param, state.clone());
            }
        }
        for state in report.values() {
            if let NodeState::Failed(error) = state {
                return Err(error.clone());
            }
        }
        let mut out = self.base;
        for (param, id) in &built.params {
            let value = cache
                .get(id)
                .map(|saved| saved.value)
                .ok_or_else(|| CadError::UnknownName(param.name().to_string()))?;
            param.set(&mut out, value.to_length(0)?);
        }
        Enclosure::new(out)?;
        Ok(Resolved {
            params: out,
            states: report,
        })
    }

    /// The graph of the variables and the parameters as Graphviz text, the same text for the same
    /// model.
    pub fn to_dot(&self, vars: &Variables, consts: &Constants) -> Result<String, CadError> {
        self.build(vars, Some(consts))?.graph.to_dot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn base_params() -> EnclosureParams {
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

    fn variables() -> Variables {
        let mut v = Variables::new();
        v.set("length", mm(100)).unwrap();
        v.set("width", mm(60)).unwrap();
        v
    }

    fn settings() -> Settings {
        Settings::defaults()
    }

    /// The box of the examples: the wall is five nozzles, the floor is the wall, the lid a
    /// millimetre more, the inner length the variable plus two walls.
    fn model() -> ParamModel {
        let lim = settings().expr;
        let mut m = ParamModel::new(base_params()).unwrap();
        m.set_expr(Param::Wall, "5 * nozzle", &lim).unwrap();
        m.set_expr(Param::Floor, "wall", &lim).unwrap();
        m.set_expr(Param::Lid, "wall + 1 mm", &lim).unwrap();
        m.set_expr(Param::InnerLength, "length + 2 * wall", &lim)
            .unwrap();
        m.set_expr(Param::InnerWidth, "width", &lim).unwrap();
        m.set_expr(Param::EdgeMargin, "screw_M3_clearance", &lim)
            .unwrap();
        m
    }

    #[test]
    fn an_expression_may_name_variables_constants_and_other_parameters() {
        let s = settings();
        let p = model()
            .resolve(&variables(), &s.constants, &s.expr)
            .unwrap();
        assert_eq!(p.wall, mm(2));
        assert_eq!(p.floor, mm(2));
        assert_eq!(p.lid, mm(3));
        assert_eq!(p.inner_length, mm(104));
        assert_eq!(p.inner_width, mm(60));
        assert_eq!(p.edge_margin.get(), 3_400_000);
        // what has no definition keeps the base
        assert_eq!(p.inner_height, mm(30));
        assert_eq!(p.min_web, mm(2));
    }

    #[test]
    fn a_parameter_may_be_a_fixed_length_a_variable_or_an_expression() {
        let s = settings();
        let mut m = ParamModel::new(base_params()).unwrap();
        m.set(Param::Wall, Parameter::Fixed(mm(4))).unwrap();
        m.set(Param::InnerLength, Parameter::Variable("length".into()))
            .unwrap();
        m.set_expr(Param::InnerHeight, "length / 2 - wall", &s.expr)
            .unwrap();
        let mut vars = variables();
        let p = m.resolve(&vars, &s.constants, &s.expr).unwrap();
        assert_eq!(p.wall, mm(4));
        assert_eq!(p.inner_length, mm(100));
        assert_eq!(p.inner_height, mm(46));
        // the variable moves, the parameters that name it move with it
        vars.set("length", mm(120)).unwrap();
        let p = m.resolve(&vars, &s.constants, &s.expr).unwrap();
        assert_eq!(p.inner_length, mm(120));
        assert_eq!(p.inner_height, mm(56));
        assert_eq!(m.get(Param::Wall), Some(&Parameter::Fixed(mm(4))));
        assert_eq!(m.get(Param::Lid), None);
        assert_eq!(m.entries().count(), 3);
        m.clear(Param::Wall);
        assert_eq!(m.get(Param::Wall), None);
        assert_eq!(m.base().wall, mm(2));
    }

    #[test]
    fn a_parameter_sees_the_length_the_other_one_has_after_rounding() {
        let s = settings();
        let mut m = ParamModel::new(base_params()).unwrap();
        m.set_expr(Param::Wall, "10 mm / 3", &s.expr).unwrap();
        m.set_expr(Param::Floor, "wall * 3", &s.expr).unwrap();
        let p = m.resolve(&variables(), &s.constants, &s.expr).unwrap();
        assert_eq!(p.wall.get(), 3_333_333);
        assert_eq!(p.floor.get(), 9_999_999);
    }

    #[test]
    fn a_definition_that_depends_on_itself_is_refused_and_changes_nothing() {
        let s = settings();
        let mut m = model();
        let before = m.clone();
        // directly
        let err = m.set_expr(Param::Wall, "wall + 1 mm", &s.expr).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-013");
        assert!(err.to_string().contains("wall"), "{err}");
        // through the floor, which is the wall
        let err = m.set_expr(Param::Wall, "floor * 2", &s.expr).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-013");
        // through two steps
        let err = m
            .set_expr(Param::Wall, "inner_length / 100", &s.expr)
            .unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-013");
        assert_eq!(m, before);
        // the same names in a way that has no cycle are fine
        assert!(m.set_expr(Param::MinWeb, "floor - 1 mm", &s.expr).is_ok());
    }

    #[test]
    fn a_name_that_is_nothing_is_refused_when_the_parameters_are_calculated() {
        let s = settings();
        let mut m = model();
        m.set_expr(Param::MinWeb, "1 mm + gap", &s.expr).unwrap();
        let err = m.resolve(&variables(), &s.constants, &s.expr).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-014");
        assert!(err.to_string().contains("gap (column 8)"), "{err}");
        // the variable exists: now it works
        let mut vars = variables();
        vars.set("gap", mm(1)).unwrap();
        assert_eq!(
            m.resolve(&vars, &s.constants, &s.expr).unwrap().min_web,
            mm(2)
        );
        // a binding to a variable that does not exist
        let mut m = ParamModel::new(base_params()).unwrap();
        m.set(Param::Wall, Parameter::Variable("thickness".into()))
            .unwrap();
        let err = m.resolve(&variables(), &s.constants, &s.expr).unwrap_err();
        assert_eq!(err, CadError::UnknownName("thickness".into()));
        let err = m
            .set(Param::Wall, Parameter::Variable("bad name".into()))
            .unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-017");
    }

    #[test]
    fn a_calculation_that_fails_gives_its_error_and_the_model_is_still_there() {
        let s = settings();
        let vars = variables();
        for (text, code) in [
            ("1 mm / 0", "PHOENIX-CAD-024"),
            ("2", "PHOENIX-CAD-024"),
            ("2 mm + 1", "PHOENIX-CAD-024"),
            ("1 m * 1000000", "PHOENIX-CAD-001"),
            ("0 mm", "PHOENIX-CAD-002"),
            ("-1 mm", "PHOENIX-CAD-002"),
        ] {
            let mut m = model();
            m.set_expr(Param::InnerHeight, text, &s.expr).unwrap();
            assert_eq!(
                m.resolve(&vars, &s.constants, &s.expr).unwrap_err().code(),
                code,
                "{text}"
            );
        }
        // a failure passes to what depends on it, and the error is the one of the cause
        let mut m = model();
        m.set_expr(Param::Wall, "1 mm / 0", &s.expr).unwrap();
        let mut cache = Cache::new();
        let err = m
            .resolve_with(&vars, &s.constants, &s.expr, &mut cache)
            .unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-024");
        assert!(err.to_string().contains("column 6"), "{err}");
    }

    #[test]
    fn the_steps_of_all_the_parameters_are_limited_together() {
        let s = settings();
        let mut m = ParamModel::new(base_params()).unwrap();
        m.set_expr(Param::Wall, "1 mm + 1 mm + 1 mm", &s.expr)
            .unwrap();
        m.set_expr(Param::Floor, "wall + wall + wall", &s.expr)
            .unwrap();
        let ok = crate::expr::ExprLimits::new(512, 64, 20, 64).unwrap();
        assert!(m.resolve(&variables(), &s.constants, &ok).is_ok());
        let tight = crate::expr::ExprLimits::new(512, 64, 12, 64).unwrap();
        let err = m.resolve(&variables(), &s.constants, &tight).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-025");
    }

    #[test]
    fn changing_a_variable_calculates_only_the_parameters_that_depend_on_it() {
        let s = settings();
        let m = model();
        let mut vars = variables();
        let mut cache = Cache::new();
        let first = m
            .resolve_with(&vars, &s.constants, &s.expr, &mut cache)
            .unwrap();
        assert!(first.states.values().all(|st| *st == NodeState::Computed));
        assert_eq!(first.states.len(), 8);
        // nothing changed: nothing is calculated
        let again = m
            .resolve_with(&vars, &s.constants, &s.expr, &mut cache)
            .unwrap();
        assert!(again.states.values().all(|st| *st == NodeState::Reused));
        assert_eq!(again.params, first.params);
        // the variable length moves: the inner length, and nothing else
        vars.set("length", mm(130)).unwrap();
        let moved = m
            .resolve_with(&vars, &s.constants, &s.expr, &mut cache)
            .unwrap();
        assert_eq!(moved.params.inner_length, mm(134));
        for param in Param::ALL {
            let expected = if param == Param::InnerLength {
                NodeState::Computed
            } else {
                NodeState::Reused
            };
            assert_eq!(moved.states[&param], expected, "{}", param.name());
        }
        // a variable that nobody names changes nothing
        vars.set("unused", mm(5)).unwrap();
        let same = m
            .resolve_with(&vars, &s.constants, &s.expr, &mut cache)
            .unwrap();
        assert!(same.states.values().all(|st| *st == NodeState::Reused));
        // the wall changes: the floor, the lid and the inner length follow
        let mut m2 = m.clone();
        m2.set_expr(Param::Wall, "6 * nozzle", &s.expr).unwrap();
        let moved = m2
            .resolve_with(&vars, &s.constants, &s.expr, &mut cache)
            .unwrap();
        assert_eq!(moved.params.wall.get(), 2_400_000);
        assert_eq!(moved.params.lid.get(), 3_400_000);
        let computed: Vec<&str> = Param::ALL
            .iter()
            .filter(|p| moved.states[*p] == NodeState::Computed)
            .map(|p| p.name())
            .collect();
        assert_eq!(computed, vec!["inner_length", "wall", "floor", "lid"]);
    }

    #[test]
    fn a_constant_that_changes_makes_the_parameters_that_use_it_calculate_again() {
        let s = settings();
        let m = model();
        let vars = variables();
        let mut cache = Cache::new();
        m.resolve_with(&vars, &s.constants, &s.expr, &mut cache)
            .unwrap();
        let user = Settings::with_user_config("[constants]\nnozzle = 0.6 mm\n").unwrap();
        let moved = m
            .resolve_with(&vars, &user.constants, &user.expr, &mut cache)
            .unwrap();
        assert_eq!(moved.params.wall.get(), 3_000_000);
        assert_eq!(moved.states[&Param::Wall], NodeState::Computed);
        assert_eq!(moved.states[&Param::MinWeb], NodeState::Reused);
        assert_eq!(moved.states[&Param::InnerWidth], NodeState::Reused);
    }

    #[test]
    fn the_same_definitions_in_any_order_give_the_same_parameters_and_the_same_graph() {
        let s = settings();
        let lim = &s.expr;
        let vars = variables();
        let mut a = ParamModel::new(base_params()).unwrap();
        a.set_expr(Param::Wall, "5 * nozzle", lim).unwrap();
        a.set_expr(Param::Floor, "wall", lim).unwrap();
        a.set_expr(Param::Lid, "floor + 1 mm", lim).unwrap();
        let mut b = ParamModel::new(base_params()).unwrap();
        b.set_expr(Param::Lid, "floor + 1 mm", lim).unwrap();
        b.set_expr(Param::Floor, "wall", lim).unwrap();
        b.set_expr(Param::Wall, "5 * nozzle", lim).unwrap();
        assert_eq!(a, b);
        assert_eq!(
            a.resolve(&vars, &s.constants, lim),
            b.resolve(&vars, &s.constants, lim)
        );
        let dot = a.to_dot(&vars, &s.constants).unwrap();
        assert_eq!(dot, b.to_dot(&vars, &s.constants).unwrap());
        assert!(dot.starts_with("digraph phoenix {"));
        assert!(dot.contains("label=\"length\""));
        assert!(dot.contains("label=\"wall\""));
        // the floor depends on the wall and the lid on the floor; the variables are named by nobody
        assert_eq!(dot.matches("->").count(), 2);
    }

    #[test]
    fn a_variable_named_like_a_parameter_is_hidden_by_the_parameter() {
        let s = settings();
        let mut vars = variables();
        vars.set("wall", mm(50)).unwrap();
        let mut m = ParamModel::new(base_params()).unwrap();
        m.set_expr(Param::Lid, "wall + 1 mm", &s.expr).unwrap();
        // the parameter wall (2 mm), not the variable wall (50 mm)
        assert_eq!(m.resolve(&vars, &s.constants, &s.expr).unwrap().lid, mm(3));
    }

    #[test]
    fn the_model_needs_a_valid_base_and_a_valid_result() {
        let mut bad = base_params();
        bad.wall = Length::ZERO;
        assert_eq!(ParamModel::new(bad).unwrap_err().code(), "PHOENIX-CAD-002");
        let mut m = ParamModel::new(base_params()).unwrap();
        assert!(m.set_base(bad).is_err());
        assert_eq!(m.base(), &base_params());
        let mut other = base_params();
        other.wall = mm(3);
        assert!(m.set_base(other).is_ok());
    }
}
