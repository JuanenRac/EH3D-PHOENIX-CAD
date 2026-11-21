// SPDX-License-Identifier: GPL-3.0-or-later
//! The bill of materials derived from the history of features, and the numbering of its items.
//!
//! The parts are the boxes of the history that end up in the result: the walk starts at the tip
//! (or at every feature no other one takes), goes through a move to its input, through a union to
//! both inputs and through a difference to the first input only (what is cut away is a tool, not a
//! part); an array of `n` copies multiplies the quantity of what is below it by `n`; an
//! intersection is one part of its own, because the common volume is not any of its inputs. A
//! suppressed feature gives what its first input gave. Each part is a node, so it keeps its item
//! number when the history is edited: a number is given the first time a part is seen, in the
//! order the walk finds it, and is never taken back or given to another part.

use std::collections::BTreeMap;

use super::model::Plan;
use super::settings::SheetSettings;
use crate::config::Settings;
use crate::enclosure::Id;
use crate::expr::Value;
use crate::featuredag::{FeatureDag, Kind, Op};
use crate::CadError;

/// One line of the bill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BomLine {
    /// The node of the history that is the part.
    pub node: Id,
    /// The number of its item.
    pub number: u32,
    /// How many there are.
    pub quantity: u64,
    /// What it is called.
    pub description: String,
    /// What it is made of.
    pub material: String,
}

/// The nodes that are parts, with their quantity, in the order the walk finds them.
pub fn part_nodes(
    dag: &FeatureDag,
    cfg: &Settings,
    max_parts: usize,
) -> Result<Vec<(Id, u64)>, CadError> {
    let used: std::collections::BTreeSet<Id> = dag
        .nodes()
        .values()
        .flat_map(|n| match &n.kind {
            Kind::Feature { inputs, .. } => inputs.clone(),
            Kind::Parameter { .. } => Vec::new(),
        })
        .collect();
    let roots: Vec<Id> = match dag.tip() {
        Some(t) => vec![t],
        None => dag
            .nodes()
            .values()
            .filter(|n| matches!(n.kind, Kind::Feature { .. }) && !used.contains(&n.id))
            .map(|n| n.id)
            .collect(),
    };
    let mut stack: Vec<(Id, u64)> = roots.into_iter().rev().map(|r| (r, 1)).collect();
    let mut out: Vec<(Id, u64)> = Vec::new();
    let mut steps = 0usize;
    while let Some((id, mult)) = stack.pop() {
        steps += 1;
        if steps > dag.len().saturating_mul(64).saturating_add(64) {
            return Err(CadError::Plan("the walk of the bill does not end".into()));
        }
        let Some(node) = dag.node(id) else { continue };
        let Kind::Feature {
            op,
            inputs,
            suppressed,
            ..
        } = &node.kind
        else {
            continue;
        };
        let add = |out: &mut Vec<(Id, u64)>| -> Result<(), CadError> {
            match out.iter_mut().find(|(i, _)| *i == id) {
                Some(l) => l.1 = l.1.saturating_add(mult),
                None => {
                    if out.len() >= max_parts {
                        return Err(CadError::Plan(format!(
                            "more than {max_parts} parts in the bill"
                        )));
                    }
                    out.push((id, mult));
                }
            }
            Ok(())
        };
        if *suppressed {
            if let Some(first) = inputs.first() {
                stack.push((*first, mult));
            }
            continue;
        }
        match op {
            Op::Box | Op::Intersection => add(&mut out)?,
            Op::Move | Op::Difference => {
                if let Some(first) = inputs.first() {
                    stack.push((*first, mult));
                }
            }
            Op::Array => {
                let n = match dag.eval_param(id, "count", cfg)? {
                    Value::Number(r) if r.is_integer() && r.numer() >= 1 => {
                        u64::try_from(r.numer()).unwrap_or(u64::MAX)
                    }
                    _ => 1,
                };
                if let Some(first) = inputs.first() {
                    stack.push((*first, mult.saturating_mul(n)));
                }
            }
            Op::Union => {
                for i in inputs.iter().rev() {
                    stack.push((*i, mult));
                }
            }
        }
    }
    Ok(out)
}

impl Plan {
    /// The table of numbers as it would be after the parts were seen: the numbers already given,
    /// and the next ones for the parts that have none.
    pub fn numbering(&self, parts: &[(Id, u64)]) -> (BTreeMap<Id, u32>, u32) {
        let mut numbers = self.numbers.clone();
        let mut next = self.next_item;
        for (id, _) in parts {
            if !numbers.contains_key(id) {
                numbers.insert(*id, next);
                next = next.saturating_add(1);
            }
        }
        (numbers, next)
    }

    /// Gives a number to every part that has none (the numbers given are kept), and says whether
    /// anything changed.
    pub fn assign_numbers(&mut self, parts: &[(Id, u64)]) -> bool {
        let (numbers, next) = self.numbering(parts);
        let changed = numbers != self.numbers;
        self.numbers = numbers;
        self.next_item = next;
        changed
    }

    /// The bill: one line per part, by number.
    pub fn bom(
        &self,
        dag: &FeatureDag,
        cfg: &Settings,
        s: &SheetSettings,
    ) -> Result<Vec<BomLine>, CadError> {
        let parts = part_nodes(dag, cfg, s.max_parts)?;
        let (numbers, _) = self.numbering(&parts);
        let mut lines: Vec<BomLine> = parts
            .iter()
            .map(|(id, qty)| {
                let info = self.parts.get(id);
                BomLine {
                    node: *id,
                    number: numbers.get(id).copied().unwrap_or(0),
                    quantity: *qty,
                    description: info
                        .and_then(|p| p.description.clone())
                        .or_else(|| dag.node(*id).map(|n| n.name.clone()))
                        .unwrap_or_default(),
                    material: info
                        .and_then(|p| p.material.clone())
                        .unwrap_or_else(|| s.default_material.clone()),
                }
            })
            .collect();
        lines.sort_by_key(|l| l.number);
        Ok(lines)
    }
}

/// The bill as comma separated values, safe to open in a spreadsheet: a text that starts with
/// `=`, `+`, `-`, `@` or a tab gets a quote in front, and a field with a comma or a quote is
/// quoted.
pub fn to_csv(lines: &[BomLine]) -> String {
    fn field(t: &str) -> String {
        let t = if t.starts_with(['=', '+', '-', '@', '\t']) {
            format!("'{t}")
        } else {
            t.to_string()
        };
        if t.contains([',', '"', '\n']) {
            format!("\"{}\"", t.replace('"', "\"\""))
        } else {
            t
        }
    }
    let mut out = String::from("item,quantity,description,material\n");
    for l in lines {
        out.push_str(&format!(
            "{},{},{},{}\n",
            l.number,
            l.quantity,
            field(&l.description),
            field(&l.material)
        ));
    }
    out
}
