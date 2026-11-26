// SPDX-License-Identifier: GPL-3.0-or-later
//! The variants of a project: named sets of values of its variables (a small, a medium and a large
//! box of the same recipe), kept in the project file in the optional domain [`VARIANTS_DOMAIN`].
//!
//! A variant holds a value for some of the variables of the project; the variables it does not name
//! keep the value they have. Applying a variant writes its values into the sheet of variables, so
//! every parameter, step and dimension that follows a variable follows the variant. A variant that
//! names a variable the project no longer has is kept and is refused with `PHOENIX-CAD-014` when it
//! is applied.
//!
//! The text is one canonical form: the header `PHOENIX-CAD-VARIANTS 1` and then, for each variant in
//! the order of the names, a line `variant NAME` followed by one line `set VARIABLE NANOMETRES` for
//! each value, in the order of the variables. A domain that is not well formed, repeats a name, has a
//! variant with no value or is over the limits of the settings (`[variants]`) is refused with
//! `PHOENIX-CAD-018`. A program that does not know the domain keeps it as it is.

use std::collections::BTreeMap;

use crate::project::Document;
use crate::vars::{check_name, Variables};
use crate::{CadError, Length};

/// The name of the domain that holds the variants of a project.
pub const VARIANTS_DOMAIN: &str = "variants";

const HEADER: &str = "PHOENIX-CAD-VARIANTS 1";

/// The limits of the variants (`[variants]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VariantLimits {
    /// The most variants of a project.
    pub max_count: usize,
    /// The most values of one variant.
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
        if values.is_empty() || values.len() > limits.max_values {
            return Err(CadError::ValueOutOfRange("values of the variant"));
        }
        if !self.list.contains_key(name) && self.list.len() >= limits.max_count {
            return Err(CadError::ValueOutOfRange("variants"));
        }
        self.list.insert(name.to_string(), values);
        Ok(())
    }

    /// Takes a variant away. An unknown name is refused with `PHOENIX-CAD-014`.
    pub fn remove(&mut self, name: &str) -> Result<(), CadError> {
        self.list
            .remove(name)
            .map(|_| ())
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
        let mut out = format!("{HEADER}\n");
        for (name, values) in &self.list {
            out.push_str(&format!("variant {name}\n"));
            for (variable, value) in values {
                out.push_str(&format!("set {variable} {}\n", value.get()));
            }
        }
        out.into_bytes()
    }

    /// Reads the text of the domain under the limits.
    pub fn from_bytes(bytes: &[u8], limits: &VariantLimits) -> Result<VariantSet, CadError> {
        let bad = |why: &str| CadError::Format(format!("variants: {why}"));
        let text = std::str::from_utf8(bytes).map_err(|_| bad("not text"))?;
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return Err(bad("no header of this version"));
        }
        let mut set = VariantSet::default();
        let mut open: Option<(String, BTreeMap<String, Length>)> = None;
        let keep = |set: &mut VariantSet, open: Option<(String, BTreeMap<String, Length>)>| {
            let Some((name, values)) = open else {
                return Ok(());
            };
            if set.list.contains_key(&name) {
                return Err(bad("a variant is repeated"));
            }
            set.put(&name, values, limits)
                .map_err(|e| bad(&format!("variant {name}: {e}")))
        };
        for line in lines {
            let words: Vec<&str> = line.split(' ').collect();
            match words.as_slice() {
                ["variant", name] => {
                    keep(&mut set, open.take())?;
                    open = Some(((*name).to_string(), BTreeMap::new()));
                }
                ["set", variable, value] => {
                    let Some((_, values)) = open.as_mut() else {
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
            "PHOENIX-CAD-VARIANTS 2\n",
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
