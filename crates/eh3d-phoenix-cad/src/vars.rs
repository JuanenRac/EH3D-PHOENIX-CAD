//! Named variables, the binding of the parameters of an enclosure to them, and families of
//! variants (a small, a medium and a large box).
//!
//! A variable is a name and a length. The master dimensions of a design live in one place, and a
//! parameter of the enclosure that is bound to a variable takes its value from it. A variant is a
//! set of values that replace some variables; the family gives each variant its own parameters.
//! There is no formula language here: a parameter is bound to one variable or it is not.

use std::collections::BTreeMap;

use crate::enclosure::{Enclosure, EnclosureParams};
use crate::graph::fnv64;
use crate::{CadError, Length};

/// The longest name of a variable or a variant.
pub const MAX_NAME_LEN: usize = 64;

fn check_name(name: &str) -> Result<(), CadError> {
    let mut chars = name.chars();
    let ok = name.len() <= MAX_NAME_LEN
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(CadError::InvalidName(name.to_string()))
    }
}

/// A set of named lengths.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Variables {
    values: BTreeMap<String, Length>,
}

impl Variables {
    /// No variable.
    pub fn new() -> Variables {
        Variables::default()
    }

    /// Sets a variable, creating it when it does not exist. The name starts with a letter and
    /// holds letters, digits and underscores, up to [`MAX_NAME_LEN`] characters.
    pub fn set(&mut self, name: &str, value: Length) -> Result<(), CadError> {
        check_name(name)?;
        self.values.insert(name.to_string(), value);
        Ok(())
    }

    /// The value of a variable.
    pub fn get(&self, name: &str) -> Option<Length> {
        self.values.get(name).copied()
    }

    /// The value of a variable that must exist.
    pub fn require(&self, name: &str) -> Result<Length, CadError> {
        self.get(name)
            .ok_or_else(|| CadError::UnknownName(name.to_string()))
    }

    /// Removes a variable and returns its value.
    pub fn remove(&mut self, name: &str) -> Result<Length, CadError> {
        self.values
            .remove(name)
            .ok_or_else(|| CadError::UnknownName(name.to_string()))
    }

    /// The names in order.
    pub fn names(&self) -> Vec<&str> {
        self.values.keys().map(String::as_str).collect()
    }

    /// How many variables.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether there is no variable.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// The parameters of an enclosure that can be bound to a variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Param {
    /// `inner_length`.
    InnerLength,
    /// `inner_width`.
    InnerWidth,
    /// `inner_height`.
    InnerHeight,
    /// `wall`.
    Wall,
    /// `floor`.
    Floor,
    /// `lid`.
    Lid,
    /// `edge_margin`.
    EdgeMargin,
    /// `min_web`.
    MinWeb,
}

impl Param {
    /// Every parameter, in the order of the fields of [`EnclosureParams`].
    pub const ALL: [Param; 8] = [
        Param::InnerLength,
        Param::InnerWidth,
        Param::InnerHeight,
        Param::Wall,
        Param::Floor,
        Param::Lid,
        Param::EdgeMargin,
        Param::MinWeb,
    ];

    /// The name of the field.
    pub fn name(self) -> &'static str {
        match self {
            Param::InnerLength => "inner_length",
            Param::InnerWidth => "inner_width",
            Param::InnerHeight => "inner_height",
            Param::Wall => "wall",
            Param::Floor => "floor",
            Param::Lid => "lid",
            Param::EdgeMargin => "edge_margin",
            Param::MinWeb => "min_web",
        }
    }

    /// The parameter with this name, or `None`.
    pub fn from_name(name: &str) -> Option<Param> {
        Param::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The value of this parameter.
    pub fn get(self, p: &EnclosureParams) -> Length {
        match self {
            Param::InnerLength => p.inner_length,
            Param::InnerWidth => p.inner_width,
            Param::InnerHeight => p.inner_height,
            Param::Wall => p.wall,
            Param::Floor => p.floor,
            Param::Lid => p.lid,
            Param::EdgeMargin => p.edge_margin,
            Param::MinWeb => p.min_web,
        }
    }

    /// Sets this parameter.
    pub fn set(self, p: &mut EnclosureParams, value: Length) {
        match self {
            Param::InnerLength => p.inner_length = value,
            Param::InnerWidth => p.inner_width = value,
            Param::InnerHeight => p.inner_height = value,
            Param::Wall => p.wall = value,
            Param::Floor => p.floor = value,
            Param::Lid => p.lid = value,
            Param::EdgeMargin => p.edge_margin = value,
            Param::MinWeb => p.min_web = value,
        }
    }
}

/// A hash of the parameters of an enclosure, the same on every platform, to give to the graph as
/// the hash of the parameters of an object.
pub fn params_hash(p: &EnclosureParams) -> u64 {
    let parts: Vec<u64> = Param::ALL
        .iter()
        .map(|param| param.get(p).get() as u64)
        .collect();
    fnv64(&parts)
}

/// Which variable each bound parameter takes its value from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bindings {
    map: BTreeMap<Param, String>,
}

impl Bindings {
    /// No binding.
    pub fn new() -> Bindings {
        Bindings::default()
    }

    /// Binds a parameter to a variable (replacing an earlier binding of that parameter).
    pub fn bind(&mut self, param: Param, variable: &str) -> Result<(), CadError> {
        check_name(variable)?;
        self.map.insert(param, variable.to_string());
        Ok(())
    }

    /// Frees a parameter.
    pub fn unbind(&mut self, param: Param) {
        self.map.remove(&param);
    }

    /// The variable a parameter is bound to.
    pub fn variable_of(&self, param: Param) -> Option<&str> {
        self.map.get(&param).map(String::as_str)
    }

    /// The parameters of `base` with each bound parameter replaced by the value of its variable.
    /// A variable that does not exist is refused, and so are parameters that do not make an
    /// enclosure.
    pub fn resolve(
        &self,
        base: &EnclosureParams,
        vars: &Variables,
    ) -> Result<EnclosureParams, CadError> {
        let mut out = *base;
        for (param, variable) in &self.map {
            param.set(&mut out, vars.require(variable)?);
        }
        Enclosure::new(out)?;
        Ok(out)
    }
}

/// A family of variants: the base variables and, for each variant, the values that replace some.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Variants {
    base: Variables,
    variants: BTreeMap<String, BTreeMap<String, Length>>,
}

impl Variants {
    /// A family over the base variables.
    pub fn new(base: Variables) -> Variants {
        Variants {
            base,
            variants: BTreeMap::new(),
        }
    }

    /// Adds a variant that replaces the given variables, which must exist in the base.
    pub fn add(&mut self, name: &str, overrides: &[(&str, Length)]) -> Result<(), CadError> {
        check_name(name)?;
        if self.variants.contains_key(name) {
            return Err(CadError::Duplicate(name.to_string()));
        }
        let mut map = BTreeMap::new();
        for (variable, value) in overrides {
            self.base.require(variable)?;
            map.insert((*variable).to_string(), *value);
        }
        self.variants.insert(name.to_string(), map);
        Ok(())
    }

    /// The names of the variants in order.
    pub fn names(&self) -> Vec<&str> {
        self.variants.keys().map(String::as_str).collect()
    }

    /// The variables of a variant: the base with the replacements.
    pub fn variables_for(&self, name: &str) -> Result<Variables, CadError> {
        let replaced = self
            .variants
            .get(name)
            .ok_or_else(|| CadError::UnknownName(name.to_string()))?;
        let mut out = self.base.clone();
        for (variable, value) in replaced {
            out.set(variable, *value)?;
        }
        Ok(out)
    }

    /// The parameters of the enclosure of a variant.
    pub fn params_for(
        &self,
        name: &str,
        base: &EnclosureParams,
        bindings: &Bindings,
    ) -> Result<EnclosureParams, CadError> {
        bindings.resolve(base, &self.variables_for(name)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Cache, Graph, NodeState};
    use eh3d_container::ObjectId;

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

    fn master() -> Variables {
        let mut v = Variables::new();
        v.set("length", mm(100)).unwrap();
        v.set("width", mm(60)).unwrap();
        v.set("wall_thickness", mm(2)).unwrap();
        v
    }

    fn bindings() -> Bindings {
        let mut b = Bindings::new();
        b.bind(Param::InnerLength, "length").unwrap();
        b.bind(Param::InnerWidth, "width").unwrap();
        b.bind(Param::Wall, "wall_thickness").unwrap();
        b
    }

    #[test]
    fn variables_are_set_read_and_removed_by_name() {
        let mut v = master();
        assert_eq!(v.names(), vec!["length", "wall_thickness", "width"]);
        assert_eq!(v.get("length"), Some(mm(100)));
        assert_eq!(v.require("depth").unwrap_err().code(), "PHOENIX-CAD-014");
        v.set("length", mm(120)).unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v.remove("length").unwrap(), mm(120));
        assert_eq!(v.remove("length").unwrap_err().code(), "PHOENIX-CAD-014");
        assert!(!v.is_empty());
    }

    #[test]
    fn names_that_are_not_plain_words_are_refused() {
        let long = "a".repeat(MAX_NAME_LEN + 1);
        let mut v = Variables::new();
        for name in ["", "1abc", "a b", "a-b", "\u{e9}t\u{e9}", long.as_str()] {
            assert_eq!(
                v.set(name, mm(1)).unwrap_err().code(),
                "PHOENIX-CAD-017",
                "{name:?}"
            );
        }
        assert!(v.set("a_1", mm(1)).is_ok());
        assert!(v.set(&"a".repeat(MAX_NAME_LEN), mm(1)).is_ok());
        assert_eq!(
            Bindings::new()
                .bind(Param::Wall, "bad name")
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-017"
        );
    }

    #[test]
    fn the_parameters_have_names_and_can_be_read_and_set_by_them() {
        let mut p = base_params();
        for param in Param::ALL {
            assert_eq!(Param::from_name(param.name()), Some(param));
            let value = param.get(&p);
            param.set(&mut p, mm(7));
            assert_eq!(param.get(&p), mm(7));
            param.set(&mut p, value);
        }
        assert_eq!(p, base_params());
        assert_eq!(Param::from_name("nothing"), None);
    }

    #[test]
    fn a_bound_parameter_takes_the_value_of_its_variable() {
        let b = bindings();
        let mut vars = master();
        let p = b.resolve(&base_params(), &vars).unwrap();
        assert_eq!(p, base_params());
        vars.set("length", mm(150)).unwrap();
        let p = b.resolve(&base_params(), &vars).unwrap();
        assert_eq!(p.inner_length, mm(150));
        // a parameter that is not bound keeps the value of the base
        assert_eq!(p.lid, mm(3));
        assert_eq!(b.variable_of(Param::Wall), Some("wall_thickness"));
        let mut b = b;
        b.unbind(Param::Wall);
        assert_eq!(b.variable_of(Param::Wall), None);
    }

    #[test]
    fn a_missing_variable_or_a_value_that_makes_no_enclosure_is_refused() {
        let b = bindings();
        let mut vars = master();
        vars.remove("width").unwrap();
        assert_eq!(
            b.resolve(&base_params(), &vars).unwrap_err().code(),
            "PHOENIX-CAD-014"
        );
        let mut vars = master();
        vars.set("wall_thickness", Length::ZERO).unwrap();
        assert_eq!(
            b.resolve(&base_params(), &vars).unwrap_err().code(),
            "PHOENIX-CAD-002"
        );
    }

    #[test]
    fn a_family_of_variants_gives_each_its_own_parameters() {
        let mut family = Variants::new(master());
        family
            .add("small", &[("length", mm(60)), ("width", mm(40))])
            .unwrap();
        family.add("medium", &[]).unwrap();
        family
            .add("large", &[("length", mm(160)), ("width", mm(100))])
            .unwrap();
        assert_eq!(family.names(), vec!["large", "medium", "small"]);
        let b = bindings();
        let p = |n: &str| family.params_for(n, &base_params(), &b).unwrap();
        assert_eq!(p("small").inner_length, mm(60));
        assert_eq!(p("medium"), base_params());
        assert_eq!(p("large").inner_width, mm(100));
        assert_eq!(
            family.variables_for("large").unwrap().get("wall_thickness"),
            Some(mm(2))
        );
    }

    #[test]
    fn a_variant_must_be_new_and_may_only_replace_variables_that_exist() {
        let mut family = Variants::new(master());
        family.add("small", &[("length", mm(60))]).unwrap();
        assert_eq!(
            family.add("small", &[]).unwrap_err().code(),
            "PHOENIX-CAD-015"
        );
        assert_eq!(
            family.add("odd", &[("depth", mm(1))]).unwrap_err().code(),
            "PHOENIX-CAD-014"
        );
        assert_eq!(
            family.variables_for("huge").unwrap_err().code(),
            "PHOENIX-CAD-014"
        );
        assert_eq!(family.names(), vec!["small"]);
    }

    #[test]
    fn changing_a_variable_recalculates_what_depends_on_the_parameters_and_nothing_else() {
        let id = |n: u64| ObjectId::from_parts(1_700_000_000_000, (n & 0x0fff) as u16, n).unwrap();
        let b = bindings();
        let mut vars = master();
        let hash_of = |vars: &Variables| params_hash(&b.resolve(&base_params(), vars).unwrap());
        let mut g = Graph::new();
        g.add(id(1), "parameters", hash_of(&vars), &[]).unwrap();
        g.add(id(2), "holes", 5, &[id(1)]).unwrap();
        g.add(id(3), "notes", 6, &[]).unwrap();
        let mut cache: Cache<u64> = Cache::new();
        let calc = |_: crate::enclosure::Id, inputs: &[&u64]| -> Result<u64, CadError> {
            Ok(inputs.iter().copied().sum::<u64>() + 1)
        };
        g.recompute(&mut cache, calc).unwrap();
        // the same value again: nothing to do
        g.set_params(id(1), hash_of(&vars)).unwrap();
        let states = g.recompute(&mut cache, calc).unwrap();
        assert!(states.values().all(|s| *s == NodeState::Reused));
        // another value: the parameters and the holes, not the notes
        vars.set("length", mm(130)).unwrap();
        g.set_params(id(1), hash_of(&vars)).unwrap();
        let states = g.recompute(&mut cache, calc).unwrap();
        assert_eq!(states[&id(1)], NodeState::Computed);
        assert_eq!(states[&id(2)], NodeState::Computed);
        assert_eq!(states[&id(3)], NodeState::Reused);
    }

    #[test]
    fn the_hash_of_the_parameters_changes_with_any_of_them() {
        let base = base_params();
        let h = params_hash(&base);
        assert_eq!(h, params_hash(&base_params()));
        for param in Param::ALL {
            let mut p = base;
            param.set(&mut p, mm(9));
            assert_ne!(params_hash(&p), h, "{}", param.name());
        }
    }
}
