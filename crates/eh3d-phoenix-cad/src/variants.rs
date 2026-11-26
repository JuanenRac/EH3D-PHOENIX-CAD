// SPDX-License-Identifier: GPL-3.0-or-later
//! The variants of a project: named sets of values of its variables (a small, a medium and a large
//! box of the same recipe), kept in the project file in the optional domain [`VARIANTS_DOMAIN`].
//!
//! A variant holds a value for some of the variables of the project; the variables it does not name
//! keep the value they have. It can also switch steps of the recipe off or on (a small box without
//! the fins, say), by the keys of [`crate::mechanical::step_keys`]; the steps it does not name
//! stay as they are. Applying a variant writes its values into the sheet of variables, so
//! every parameter, step and dimension that follows a variable follows the variant. A variant that
//! names a variable the project no longer has is kept and is refused with `PHOENIX-CAD-014` when it
//! is applied.
//!
//! The text is one canonical form: the header `PHOENIX-CAD-VARIANTS 1` and then, for each variant in
//! the order of the names, a line `variant NAME` followed by one line `set VARIABLE NANOMETRES` for
//! each value, in the order of the variables. When a variant switches a step, the header is
//! `PHOENIX-CAD-VARIANTS 2` and its lines `off KEY` and `on KEY` follow its values, in the order of
//! the keys; without any, version 1 is written, which older programs read. A domain that is not well
//! formed, repeats a name, has a variant with nothing in it or is over the limits of the settings
//! (`[variants]`) is refused with `PHOENIX-CAD-018`. A program that does not know the domain keeps
//! it as it is.

use std::collections::BTreeMap;

use crate::project::Document;
use crate::vars::{check_name, Variables};
use crate::{CadError, Length};

/// The name of the domain that holds the variants of a project.
pub const VARIANTS_DOMAIN: &str = "variants";

const HEADER: &str = "PHOENIX-CAD-VARIANTS 1";
const HEADER_STEPS: &str = "PHOENIX-CAD-VARIANTS 2";
const MAX_KEY_LEN: usize = 128;

fn check_key(key: &str) -> Result<(), CadError> {
    let ok = !key.is_empty()
        && key.len() <= MAX_KEY_LEN
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | ':' | '@'));
    if ok {
        Ok(())
    } else {
        Err(CadError::InvalidName(key.to_string()))
    }
}

/// The limits of the variants (`[variants]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VariantLimits {
    /// The most variants of a project.
    pub max_count: usize,
    /// The most values of one variant, and the most steps one switches.
    pub max_values: usize,
}

impl VariantLimits {
    /// The most that either limit can be set to.
    pub const CAP: u32 = 10_000;

    /// Limits checked against the cap: each one is from 1 to [`VariantLimits::CAP`].
    pub fn new(max_count: u32, max_values: u32) -> Result<VariantLimits, CadError> {
        let check = |key: &str, value: u32| {
            if value == 0 || value > Self::CAP {
                Err(CadError::Config(format!(
                    "variants.{key} must be from 1 to {}",
                    Self::CAP
                )))
            } else {
                Ok(value as usize)
            }
        };
        Ok(VariantLimits {
            max_count: check("max_count", max_count)?,
            max_values: check("max_values", max_values)?,
        })
    }
}

/// The variants of a project, by name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VariantSet {
    list: BTreeMap<String, BTreeMap<String, Length>>,
    /// The steps that variants switch: by variant and then by key, true for off. A variant that
    /// switches none has no entry.
    steps: BTreeMap<String, BTreeMap<String, bool>>,
}

impl VariantSet {
    /// Whether there is no variant.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// How many variants there are.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// The names of the variants in order.
    pub fn names(&self) -> Vec<&str> {
        self.list.keys().map(String::as_str).collect()
    }

    /// The values of a variant, by variable. An unknown name is refused with `PHOENIX-CAD-014`.
    pub fn values(&self, name: &str) -> Result<&BTreeMap<String, Length>, CadError> {
        self.list
            .get(name)
            .ok_or_else(|| CadError::UnknownName(name.to_string()))
    }

    /// Keeps a variant under a name, replacing the one of that name. The name and the names of the
    /// variables are plain words, there is at least one value, and the limits hold; otherwise
    /// nothing changes.
    pub fn put(
        &mut self,
        name: &str,
        values: BTreeMap<String, Length>,
        limits: &VariantLimits,
    ) -> Result<(), CadError> {
        check_name(name)?;
        for variable in values.keys() {
            check_name(variable)?;
        }
        if (values.is_empty() && !self.steps.contains_key(name)) || values.len() > limits.max_values
        {
            return Err(CadError::ValueOutOfRange("values of the variant"));
        }
        if !self.list.contains_key(name) && self.list.len() >= limits.max_count {
            return Err(CadError::ValueOutOfRange("variants"));
        }
        self.list.insert(name.to_string(), values);
        Ok(())
    }

    /// The steps that a variant switches, by key: true for off, false for on.
    pub fn steps_of(&self, name: &str) -> impl Iterator<Item = (&String, &bool)> {
        self.steps.get(name).into_iter().flatten()
    }

    /// Says what a variant does with the step `key`: off, on or (`None`) nothing. A variant that
    /// does not exist is made when it is given something to hold. A change that would leave a
    /// variant with nothing in it, or go over the limits, is refused and nothing changes.
    pub fn set_step(
        &mut self,
        name: &str,
        key: &str,
        off: Option<bool>,
        limits: &VariantLimits,
    ) -> Result<(), CadError> {
        check_name(name)?;
        check_key(key)?;
        let known = self.list.contains_key(name);
        let mut steps = self.steps.get(name).cloned().unwrap_or_default();
        match off {
            Some(off) => {
                steps.insert(key.to_string(), off);
            }
            None => {
                if steps.remove(key).is_none() {
                    return Err(CadError::UnknownName(key.to_string()));
                }
            }
        }
        if steps.len() > limits.max_values {
            return Err(CadError::ValueOutOfRange("steps of the variant"));
        }
        if !known && self.list.len() >= limits.max_count {
            return Err(CadError::ValueOutOfRange("variants"));
        }
        if steps.is_empty() && self.list.get(name).is_none_or(BTreeMap::is_empty) {
            return Err(CadError::ValueOutOfRange("values of the variant"));
        }
        self.list.entry(name.to_string()).or_default();
        if steps.is_empty() {
            self.steps.remove(name);
        } else {
            self.steps.insert(name.to_string(), steps);
        }
        Ok(())
    }

    /// Takes a variant away. An unknown name is refused with `PHOENIX-CAD-014`.
    pub fn remove(&mut self, name: &str) -> Result<(), CadError> {
        self.list
            .remove(name)
            .map(|_| {
                self.steps.remove(name);
            })
            .ok_or_else(|| CadError::UnknownName(name.to_string()))
    }

    /// The variables of the project with the values of a variant written over them. A variable that
    /// the project does not have is refused with `PHOENIX-CAD-014`.
    pub fn variables_for(&self, name: &str, base: &Variables) -> Result<Variables, CadError> {
        let mut out = base.clone();
        for (variable, value) in self.values(name)? {
            base.require(variable)?;
            out.set(variable, *value)?;
        }
        Ok(out)
    }

    /// The text of the domain.
    pub fn to_bytes(&self) -> Vec<u8> {
        let header = if self.steps.is_empty() {
            HEADER
        } else {
            HEADER_STEPS
        };
        let mut out = format!("{header}\n");
        for (name, values) in &self.list {
            out.push_str(&format!("variant {name}\n"));
            for (variable, value) in values {
                out.push_str(&format!("set {variable} {}\n", value.get()));
            }
            for (key, off) in self.steps_of(name) {
                let word = if *off { "off" } else { "on" };
                out.push_str(&format!("{word} {key}\n"));
            }
        }
        out.into_bytes()
    }

    /// Reads the text of the domain under the limits.
    pub fn from_bytes(bytes: &[u8], limits: &VariantLimits) -> Result<VariantSet, CadError> {
        let bad = |why: &str| CadError::Format(format!("variants: {why}"));
        let text = std::str::from_utf8(bytes).map_err(|_| bad("not text"))?;
        let mut lines = text.lines();
        let with_steps = match lines.next() {
            Some(HEADER) => false,
            Some(HEADER_STEPS) => true,
            _ => return Err(bad("no header of this version")),
        };
        let mut set = VariantSet::default();
        type Open = (String, BTreeMap<String, Length>, BTreeMap<String, bool>);
        let mut open: Option<Open> = None;
        let keep = |set: &mut VariantSet, open: Option<Open>| {
            let Some((name, values, steps)) = open else {
                return Ok(());
            };
            if set.list.contains_key(&name) {
                return Err(bad("a variant is repeated"));
            }
            if !steps.is_empty() {
                set.steps.insert(name.clone(), steps);
            }
            set.put(&name, values, limits)
                .map_err(|e| bad(&format!("variant {name}: {e}")))
        };
        for line in lines {
            let words: Vec<&str> = line.split(' ').collect();
            match words.as_slice() {
                ["variant", name] => {
                    keep(&mut set, open.take())?;
                    open = Some(((*name).to_string(), BTreeMap::new(), BTreeMap::new()));
                }
                ["set", variable, value] => {
                    let Some((_, values, _)) = open.as_mut() else {
                        return Err(bad("a value has no variant"));
                    };
                    let value = value
                        .parse::<i64>()
                        .ok()
                        .and_then(|nm| Length::new(nm).ok())
                        .ok_or_else(|| bad("a value is not a length in nanometres"))?;
                    if values.len() >= limits.max_values {
                        return Err(bad("a variant has too many values"));
                    }
                    if values.insert((*variable).to_string(), value).is_some() {
                        return Err(bad("a value is repeated"));
                    }
                }
                [word @ ("off" | "on"), key] if with_steps => {
                    let Some((_, _, steps)) = open.as_mut() else {
                        return Err(bad("a step has no variant"));
                    };
                    check_key(key).map_err(|_| bad("the key of a step is not valid"))?;
                    if steps.len() >= limits.max_values {
                        return Err(bad("a variant has too many steps"));
                    }
                    if steps.insert((*key).to_string(), *word == "off").is_some() {
                        return Err(bad("a step is repeated"));
                    }
                }
                _ => return Err(bad("a line is not understood")),
            }
        }
        keep(&mut set, open)?;
        Ok(set)
    }
}

impl Document {
    /// The variants of the project (none when the file has no such domain).
    pub fn variants(&self, limits: &VariantLimits) -> Result<VariantSet, CadError> {
        match self.get(VARIANTS_DOMAIN) {
            Some(domain) => VariantSet::from_bytes(&domain.data, limits),
            None => Ok(VariantSet::default()),
        }
    }

    /// Writes the variants in the optional domain [`VARIANTS_DOMAIN`], or takes the domain away
    /// when there is none. A domain that cannot be read (damaged, of a newer version or over the
    /// limits) is not written over: it is kept as it is, and the call is refused when there are
    /// variants to write.
    pub fn set_variants(
        &mut self,
        variants: &VariantSet,
        limits: &VariantLimits,
    ) -> Result<(), CadError> {
        if let Err(e) = self.variants(limits) {
            return if variants.is_empty() { Ok(()) } else { Err(e) };
        }
        if variants.is_empty() {
            if self.get(VARIANTS_DOMAIN).is_some() {
                self.remove(VARIANTS_DOMAIN)?;
            }
            Ok(())
        } else {
            self.set(VARIANTS_DOMAIN, false, variants.to_bytes())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn limits() -> VariantLimits {
        VariantLimits::new(3, 2).unwrap()
    }

    fn values(list: &[(&str, i64)]) -> BTreeMap<String, Length> {
        list.iter()
            .map(|(n, v)| ((*n).to_string(), mm(*v)))
            .collect()
    }

    fn sample() -> VariantSet {
        let mut set = VariantSet::default();
        set.put("small", values(&[("width", 40)]), &limits())
            .unwrap();
        set.put(
            "large",
            values(&[("width", 90), ("length", 150)]),
            &limits(),
        )
        .unwrap();
        set
    }

    #[test]
    fn the_text_is_canonical_and_read_back_equal() {
        let set = sample();
        let text = String::from_utf8(set.to_bytes()).unwrap();
        assert_eq!(
            text,
            "PHOENIX-CAD-VARIANTS 1\nvariant large\nset length 150000000\nset width 90000000\n\
             variant small\nset width 40000000\n"
        );
        assert_eq!(
            VariantSet::from_bytes(text.as_bytes(), &limits()).unwrap(),
            set
        );
    }

    #[test]
    fn a_variant_writes_its_values_over_the_variables_that_exist() {
        let set = sample();
        let mut base = Variables::new();
        base.set("width", mm(60)).unwrap();
        base.set("other", mm(1)).unwrap();
        let small = set.variables_for("small", &base).unwrap();
        assert_eq!(small.get("width"), Some(mm(40)));
        assert_eq!(small.get("other"), Some(mm(1)));
        // `large` names a variable that the project does not have
        let e = set.variables_for("large", &base).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-014");
        assert_eq!(
            set.variables_for("none", &base).unwrap_err().code(),
            "PHOENIX-CAD-014"
        );
    }

    #[test]
    fn names_and_limits_are_checked_and_a_refusal_changes_nothing() {
        let mut set = sample();
        let before = set.clone();
        for (name, list) in [
            ("1st", values(&[("width", 1)])),
            ("mm", values(&[("width", 1)])),
            ("ok", values(&[])),
            ("ok", values(&[("a", 1), ("b", 2), ("c", 3)])),
            ("ok", values(&[("bad name", 1)])),
        ] {
            assert!(set.put(name, list, &limits()).is_err(), "{name}");
        }
        assert_eq!(set, before);
        set.put("middle", values(&[("width", 60)]), &limits())
            .unwrap();
        let e = set
            .put("huge", values(&[("width", 200)]), &limits())
            .unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-007");
        // replacing one that exists is still allowed at the limit
        set.put("small", values(&[("width", 41)]), &limits())
            .unwrap();
        set.remove("small").unwrap();
        assert_eq!(set.remove("small").unwrap_err().code(), "PHOENIX-CAD-014");
        assert!(VariantLimits::new(0, 1).is_err());
        assert!(VariantLimits::new(1, VariantLimits::CAP + 1).is_err());
    }

    #[test]
    fn a_domain_that_is_not_well_formed_is_refused() {
        for text in [
            "",
            "PHOENIX-CAD-VARIANTS 3\n",
            "PHOENIX-CAD-VARIANTS 1\nvariant a\noff set-lid\n",
            "PHOENIX-CAD-VARIANTS 2\noff set-lid\n",
            "PHOENIX-CAD-VARIANTS 2\nvariant a\noff set-lid\non set-lid\n",
            "PHOENIX-CAD-VARIANTS 2\nvariant a\noff set_lid\n",
            "PHOENIX-CAD-VARIANTS 2\nvariant a\non a\non b\non c\n",
            "PHOENIX-CAD-VARIANTS 1\nset a 1\n",
            "PHOENIX-CAD-VARIANTS 1\nvariant a\n",
            "PHOENIX-CAD-VARIANTS 1\nvariant a\nset w 1\nset w 2\n",
            "PHOENIX-CAD-VARIANTS 1\nvariant a\nset w 1\nvariant a\nset w 2\n",
            "PHOENIX-CAD-VARIANTS 1\nvariant a\nset w x\n",
            "PHOENIX-CAD-VARIANTS 1\nvariant a\nset w 99999999999999999\n",
            "PHOENIX-CAD-VARIANTS 1\nvariant a b\n",
            "PHOENIX-CAD-VARIANTS 1\nvariant a\nset u 1\nset v 1\nset w 1\n",
        ] {
            let e = VariantSet::from_bytes(text.as_bytes(), &limits()).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-018", "{text:?}");
        }
    }

    #[test]
    fn steps_are_switched_by_variants_in_version_two() {
        let mut set = sample();
        set.set_step("small", "add-fins:a1", Some(true), &limits())
            .unwrap();
        set.set_step("bare", "set-lid", Some(false), &limits())
            .unwrap();
        let text = String::from_utf8(set.to_bytes()).unwrap();
        assert_eq!(
            text,
            "PHOENIX-CAD-VARIANTS 2\nvariant bare\non set-lid\nvariant large\n\
             set length 150000000\nset width 90000000\nvariant small\nset width 40000000\n\
             off add-fins:a1\n"
        );
        assert_eq!(
            VariantSet::from_bytes(text.as_bytes(), &limits()).unwrap(),
            set
        );
        // the last thing of a variant cannot be taken out, and an unknown key neither
        let before = set.clone();
        for (name, key) in [("bare", "set-lid"), ("small", "set-lid"), ("none", "a")] {
            assert!(set.set_step(name, key, None, &limits()).is_err(), "{name}");
        }
        assert!(set
            .set_step("small", "bad key", Some(true), &limits())
            .is_err());
        assert!(set
            .set_step("fourth", "set-lid", Some(true), &limits())
            .is_err());
        assert_eq!(set, before);
        // new values keep the steps, and a removed variant takes them with it
        set.put("small", values(&[("width", 41)]), &limits())
            .unwrap();
        assert_eq!(set.steps_of("small").count(), 1);
        set.set_step("small", "add-fins:a1", None, &limits())
            .unwrap();
        set.remove("bare").unwrap();
        assert!(set.to_bytes().starts_with(b"PHOENIX-CAD-VARIANTS 1\n"));
    }

    #[test]
    fn the_domain_is_optional_and_goes_when_there_is_no_variant() {
        let mut doc = Document::new();
        assert_eq!(doc.variants(&limits()).unwrap(), VariantSet::default());
        doc.set_variants(&sample(), &limits()).unwrap();
        assert!(!doc.get(VARIANTS_DOMAIN).unwrap().critical);
        assert_eq!(doc.variants(&limits()).unwrap(), sample());
        let none = VariantSet::default();
        doc.set_variants(&none, &limits()).unwrap();
        assert!(doc.get(VARIANTS_DOMAIN).is_none());
        doc.set_variants(&none, &limits()).unwrap();
        // a domain that cannot be read is not written over and not taken away
        let future = b"PHOENIX-CAD-VARIANTS 9".to_vec();
        doc.set(VARIANTS_DOMAIN, false, future.clone()).unwrap();
        doc.set_variants(&none, &limits()).unwrap();
        assert!(doc.set_variants(&sample(), &limits()).is_err());
        assert_eq!(doc.get(VARIANTS_DOMAIN).unwrap().data, future);
    }
}
