// SPDX-License-Identifier: GPL-3.0-or-later
//! Variables that follow a reference dimension of a sketch: the drawing says a length, the variable
//! takes it, and every parameter and step that follows the variable follows the drawing.
//!
//! A link holds the sketch and the constraint. It is read when it is made and again every time the
//! sketch is kept in the project; a link whose dimension is gone, is no longer a reference or sits
//! in a sketch whose own formulas use the variable (which would go round in a circle) is broken,
//! and the variable keeps the value it has. The links are kept in the project file in the optional
//! domain [`LINKS_DOMAIN`]: the header `PHOENIX-CAD-LINKS 1` and then one line
//! `follow VARIABLE SKETCH CONSTRAINT` for each, in the order of the variables.

use std::collections::BTreeMap;

use crate::config::Settings;
use crate::expr::parse;
use crate::project::Document;
use crate::sketch::{check_sketch_name, Constraint, ConstraintId, Sketch};
use crate::vars::check_name;
use crate::{CadError, Length};

/// The name of the domain that holds the links of a project.
pub const LINKS_DOMAIN: &str = "variable-links";

/// The most links of a project.
pub const MAX_LINKS: usize = 1024;

const HEADER: &str = "PHOENIX-CAD-LINKS 1";

/// The dimension a variable follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimensionLink {
    /// The name of the sketch.
    pub sketch: String,
    /// The constraint of the sketch.
    pub constraint: ConstraintId,
}

/// The links of a project, by variable.
pub type Links = BTreeMap<String, DimensionLink>;

/// The constraint named by a text such as `c3` or `3`.
pub fn constraint_of(text: &str) -> Result<ConstraintId, CadError> {
    let t = text.trim();
    t.strip_prefix('c')
        .unwrap_or(t)
        .parse::<u32>()
        .map(ConstraintId)
        .map_err(|_| CadError::UnknownName(t.to_string()))
}

/// The length that the reference dimension `id` of the sketch measures now. A dimension that is
/// not a reference and an angle are refused: a variable is a length that is only read.
pub fn reference_value(sketch: &Sketch, id: ConstraintId) -> Result<Length, CadError> {
    let entry = sketch
        .constraint(id)
        .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
    if !entry.reference {
        return Err(CadError::SketchConstraint(format!(
            "{id} is not a reference dimension"
        )));
    }
    match sketch.measured(id)? {
        Constraint::Distance { dim, .. }
        | Constraint::DistanceX { dim, .. }
        | Constraint::DistanceY { dim, .. }
        | Constraint::LineLength { dim, .. }
        | Constraint::PointLineDistance { dim, .. }
        | Constraint::Radius { dim, .. }
        | Constraint::Diameter { dim, .. } => Ok(dim.value),
        _ => Err(CadError::SketchConstraint(format!(
            "{id} is an angle: a variable is a length"
        ))),
    }
}

/// Whether a formula of a dimension of the sketch names the variable.
pub fn sketch_uses(sketch: &Sketch, variable: &str, settings: &Settings) -> bool {
    sketch.constraints().iter().any(|c| {
        let text = match &c.constraint {
            Constraint::Distance { dim, .. }
            | Constraint::DistanceX { dim, .. }
            | Constraint::DistanceY { dim, .. }
            | Constraint::LineLength { dim, .. }
            | Constraint::PointLineDistance { dim, .. }
            | Constraint::Radius { dim, .. }
            | Constraint::Diameter { dim, .. } => dim.expr.as_deref(),
            Constraint::AngleBetween { dim, .. } | Constraint::LineAngle { dim, .. } => {
                dim.expr.as_deref()
            }
            _ => None,
        };
        text.and_then(|t| parse(t, &settings.expr).ok())
            .is_some_and(|e| e.names().iter().any(|(name, _)| name == variable))
    })
}

/// The value that a link gives now, or why it is broken.
pub fn linked_value(
    sketches: &[Sketch],
    variable: &str,
    link: &DimensionLink,
    settings: &Settings,
) -> Result<Length, CadError> {
    let sketch = sketches
        .iter()
        .find(|s| s.name() == link.sketch)
        .ok_or_else(|| CadError::UnknownName(link.sketch.clone()))?;
    if sketch_uses(sketch, variable, settings) {
        return Err(CadError::InUse(variable.to_string()));
    }
    reference_value(sketch, link.constraint)
}

/// The text of the domain.
pub fn to_bytes(links: &Links) -> Vec<u8> {
    let mut out = format!("{HEADER}\n");
    for (variable, link) in links {
        out.push_str(&format!(
            "follow {variable} {} {}\n",
            link.sketch, link.constraint.0
        ));
    }
    out.into_bytes()
}

/// Reads the text of the domain. What is not well formed is refused with `PHOENIX-CAD-018`.
pub fn from_bytes(bytes: &[u8]) -> Result<Links, CadError> {
    let bad = |why: &str| CadError::Format(format!("variable links: {why}"));
    let text = std::str::from_utf8(bytes).map_err(|_| bad("not text"))?;
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(bad("no header of this version"));
    }
    let mut links = Links::new();
    for line in lines {
        let words: Vec<&str> = line.split(' ').collect();
        let ["follow", variable, sketch, constraint] = words.as_slice() else {
            return Err(bad("a line is not understood"));
        };
        check_name(variable).map_err(|_| bad("a name is not valid"))?;
        check_sketch_name(sketch).map_err(|_| bad("the name of a sketch is not valid"))?;
        let constraint = constraint
            .parse::<u32>()
            .map(ConstraintId)
            .map_err(|_| bad("a constraint is not a number"))?;
        let link = DimensionLink {
            sketch: (*sketch).to_string(),
            constraint,
        };
        if links.len() >= MAX_LINKS || links.insert((*variable).to_string(), link).is_some() {
            return Err(bad("a link is repeated or there are too many"));
        }
    }
    Ok(links)
}

impl Document {
    /// The links of the project (none when the file has no such domain).
    pub fn variable_links(&self) -> Result<Links, CadError> {
        match self.get(LINKS_DOMAIN) {
            Some(domain) => from_bytes(&domain.data),
            None => Ok(Links::new()),
        }
    }

    /// Writes the links in the optional domain [`LINKS_DOMAIN`], or takes the domain away when
    /// there is none. A domain that cannot be read is not written over.
    pub fn set_variable_links(&mut self, links: &Links) -> Result<(), CadError> {
        if let Err(e) = self.variable_links() {
            return if links.is_empty() { Ok(()) } else { Err(e) };
        }
        if links.is_empty() {
            if self.get(LINKS_DOMAIN).is_some() {
                self.remove(LINKS_DOMAIN)?;
            }
            Ok(())
        } else {
            self.set(LINKS_DOMAIN, false, to_bytes(links))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Links {
        Links::from([
            (
                "width".to_string(),
                DimensionLink {
                    sketch: "plate".to_string(),
                    constraint: ConstraintId(3),
                },
            ),
            (
                "depth".to_string(),
                DimensionLink {
                    sketch: "side-1".to_string(),
                    constraint: ConstraintId(12),
                },
            ),
        ])
    }

    #[test]
    fn the_text_is_canonical_and_read_back_equal() {
        let text = String::from_utf8(to_bytes(&sample())).unwrap();
        assert_eq!(
            text,
            "PHOENIX-CAD-LINKS 1\nfollow depth side-1 12\nfollow width plate 3\n"
        );
        assert_eq!(from_bytes(text.as_bytes()).unwrap(), sample());
        assert_eq!(constraint_of(" c7 ").unwrap(), ConstraintId(7));
        assert_eq!(constraint_of("7").unwrap(), ConstraintId(7));
        assert_eq!(constraint_of("e7").unwrap_err().code(), "PHOENIX-CAD-014");
    }

    #[test]
    fn a_domain_that_is_not_well_formed_is_refused_and_not_written_over() {
        for text in [
            "",
            "PHOENIX-CAD-LINKS 2\n",
            "PHOENIX-CAD-LINKS 1\nfollow a s\n",
            "PHOENIX-CAD-LINKS 1\nfollow 1a s 1\n",
            "PHOENIX-CAD-LINKS 1\nfollow a s.x 1\n",
            "PHOENIX-CAD-LINKS 1\nfollow a s c1\n",
            "PHOENIX-CAD-LINKS 1\nfollow a s 1\nfollow a s 2\n",
        ] {
            let e = from_bytes(text.as_bytes()).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-018", "{text:?}");
        }
        let mut doc = Document::new();
        assert!(doc.variable_links().unwrap().is_empty());
        doc.set_variable_links(&sample()).unwrap();
        assert!(!doc.get(LINKS_DOMAIN).unwrap().critical);
        assert_eq!(doc.variable_links().unwrap(), sample());
        doc.set_variable_links(&Links::new()).unwrap();
        assert!(doc.get(LINKS_DOMAIN).is_none());
        let future = b"PHOENIX-CAD-LINKS 9".to_vec();
        doc.set(LINKS_DOMAIN, false, future.clone()).unwrap();
        doc.set_variable_links(&Links::new()).unwrap();
        assert!(doc.set_variable_links(&sample()).is_err());
        assert_eq!(doc.get(LINKS_DOMAIN).unwrap().data, future);
    }
}
