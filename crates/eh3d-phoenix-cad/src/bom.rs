// SPDX-License-Identifier: GPL-3.0-or-later
//! The bill of materials with cost: the placed parts and the turned and swept bodies of the model
//! grouped into lines (the same part, material and size, or a body by its name), each with its
//! material and the density of it, the volume, the mass, the price of the material and the cost,
//! and the totals.
//!
//! The figures of a piece are the ones of [`crate::partlist`] (so the mass of the bill is the mass
//! of the balance). The cost of a piece is the cost of its mass in its material, worked out from
//! the price of a kilogram in the settings (`price_per_kg` of `[material.*]`) and rounded half up
//! to a thousandth of the currency, plus what a piece costs besides its material (`cost.per_piece`).
//! A line costs a piece times its quantity; the markup (`cost.markup_percent`) is a percentage of
//! the sum of the lines, rounded half up too. Money is whole thousandths, so the sums are exact.
//! A material without a price costs nothing in the bill and the bill says how many lines have no
//! price, so a cost is never presented as complete when it is not.
//!
//! The comma separated values never change with the language and are written to be opened in a
//! spreadsheet: a text that holds a comma, a quote or a line break is quoted, and one that begins
//! with `=`, `+`, `-` or `@` gets a quote mark in front so that a spreadsheet does not take the
//! name of a part for a formula.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::assembly::Mate;
use crate::catalog::{mass_g_text, Kind, PartInstance};
use crate::config::CostSettings;
use crate::enclosure::Id;
use crate::lang::{fill, Lang, Msg};
use crate::partlist::{List, Row};
use crate::revolve::scaled;
use crate::scad::mm_text;
use crate::swept::SweptBody;
use crate::turned::Turned;
use crate::window::kind_msg;
use crate::CadError;

const NM3_PER_MM3: i128 = 1_000_000_000_000_000_000;
const UG_PER_KG: i128 = 1_000_000_000;

/// The columns of the comma separated values.
const COLUMNS: usize = 17;

/// One line of the bill: some pieces that are the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The number of the line, from 1.
    pub item: usize,
    /// The name of the part in its library, or the name of a turned or swept body.
    pub name: String,
    /// What kind of part it is (`Profile` for a turned or a swept body: read `turned` and
    /// `swept`).
    pub kind: Kind,
    /// Whether the line is a turned body.
    pub turned: bool,
    /// Whether the line is a swept body.
    pub swept: bool,
    /// The material.
    pub material: String,
    /// The density of the material in kilograms per cubic metre.
    pub density: u32,
    /// The size of the bounding box of one piece along `x`, `y` and `z`, in nanometres.
    pub size: [i64; 3],
    /// How many pieces.
    pub quantity: u32,
    /// The volume of one piece in cubic nanometres.
    pub unit_volume_nm3: i128,
    /// The mass of one piece in micrograms.
    pub unit_mass_ug: i128,
    /// The price of a kilogram of the material in thousandths, when the settings give one.
    pub price_milli_per_kg: Option<u32>,
    /// The cost of one piece in thousandths: its material and what a piece costs besides it.
    pub unit_cost_milli: i128,
}

impl Line {
    /// The cost of all the pieces of the line in thousandths.
    pub fn total_cost_milli(&self) -> i128 {
        self.unit_cost_milli * i128::from(self.quantity)
    }

    /// The mass of all the pieces of the line in micrograms.
    pub fn total_mass_ug(&self) -> i128 {
        self.unit_mass_ug * i128::from(self.quantity)
    }
}

/// The bill with its totals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bom {
    /// The lines: parts by kind, name, material and size, then the turned and the swept bodies.
    pub lines: Vec<Line>,
    /// The code of the currency of the figures.
    pub currency: String,
    /// The percentage of the markup.
    pub markup_percent: u32,
    /// How many pieces in all.
    pub pieces: u32,
    /// The volume of all of them in cubic nanometres.
    pub volume_nm3: i128,
    /// The mass of all of them in micrograms.
    pub mass_ug: i128,
    /// The sum of the lines in thousandths.
    pub subtotal_milli: i128,
    /// The markup in thousandths.
    pub markup_milli: i128,
    /// The subtotal and the markup, in thousandths.
    pub total_milli: i128,
    /// How many lines have a material without a price.
    pub unpriced: usize,
}

/// A figure in thousandths as a number with three decimals (`12.345`).
pub fn money_text(milli: i128) -> String {
    let sign = if milli < 0 { "-" } else { "" };
    let abs = milli.unsigned_abs();
    format!("{sign}{}.{:03}", abs / 1_000, abs % 1_000)
}

/// The cost of a mass in a material at a price, in thousandths, rounded half up.
fn material_cost_milli(mass_ug: i128, price_milli_per_kg: u32) -> Result<i128, CadError> {
    mass_ug
        .checked_mul(i128::from(price_milli_per_kg))
        .and_then(|v| v.checked_add(UG_PER_KG / 2))
        .map(|v| v / UG_PER_KG)
        .ok_or(CadError::Overflow("cost"))
}

/// The bill of a list of parts, priced with the settings of the cost. Refused with
/// `PHOENIX-CAD-045` for a quantity or a sum that overflows.
pub fn from_list(list: &List, cost: &CostSettings) -> Result<Bom, CadError> {
    type Key = (Kind, bool, bool, String, String, [i64; 3]);
    let mut groups: BTreeMap<Key, (u32, &Row)> = BTreeMap::new();
    for r in &list.rows {
        // a part is known by the part of its library, a body by its own name
        let who = if r.turned || r.swept {
            r.name.clone()
        } else {
            r.part.clone()
        };
        let key = (r.kind, r.turned, r.swept, who, r.material.clone(), r.size);
        groups
            .entry(key)
            .and_modify(|e| e.0 = e.0.saturating_add(1))
            .or_insert((1, r));
    }
    let per_piece = i128::from(cost.per_piece_milli);
    let mut lines = Vec::with_capacity(groups.len());
    let (mut pieces, mut volume, mut mass, mut subtotal) = (0u32, 0i128, 0i128, 0i128);
    let overflow = |what: &'static str| CadError::Overflow(what);
    for (i, ((kind, turned, swept, name, material, size), (quantity, row))) in
        groups.into_iter().enumerate()
    {
        let price = cost.prices_milli_per_kg.get(&material).copied();
        let material_milli = match price {
            Some(p) => material_cost_milli(row.mass_ug, p)?,
            None => 0,
        };
        let line = Line {
            item: i + 1,
            name,
            kind,
            turned,
            swept,
            material,
            density: row.density,
            size,
            quantity,
            unit_volume_nm3: row.volume_nm3,
            unit_mass_ug: row.mass_ug,
            price_milli_per_kg: price,
            unit_cost_milli: material_milli + per_piece,
        };
        pieces = pieces
            .checked_add(quantity)
            .ok_or_else(|| overflow("pieces"))?;
        volume = line
            .unit_volume_nm3
            .checked_mul(i128::from(quantity))
            .and_then(|v| volume.checked_add(v))
            .ok_or_else(|| overflow("volume"))?;
        mass = line
            .unit_mass_ug
            .checked_mul(i128::from(quantity))
            .and_then(|v| mass.checked_add(v))
            .ok_or_else(|| overflow("mass"))?;
        subtotal = line
            .unit_cost_milli
            .checked_mul(i128::from(quantity))
            .and_then(|v| subtotal.checked_add(v))
            .ok_or_else(|| overflow("cost"))?;
        lines.push(line);
    }
    let markup = subtotal
        .checked_mul(i128::from(cost.markup_percent))
        .and_then(|v| v.checked_add(50))
        .map(|v| v / 100)
        .ok_or_else(|| overflow("cost"))?;
    let unpriced = lines
        .iter()
        .filter(|l| l.price_milli_per_kg.is_none())
        .count();
    Ok(Bom {
        lines,
        currency: cost.currency.clone(),
        markup_percent: cost.markup_percent,
        pieces,
        volume_nm3: volume,
        mass_ug: mass,
        subtotal_milli: subtotal,
        markup_milli: markup,
        total_milli: subtotal + markup,
        unpriced,
    })
}

/// One cell of the comma separated values: quoted when it holds a comma, a quote or a line break,
/// and with a quote mark in front when a spreadsheet could take it for a formula.
pub(crate) fn cell(text: &str) -> String {
    let mut s = text.to_string();
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        s.insert(0, '\'');
    }
    if s.contains([',', '"', '\n', '\r']) {
        s = format!("\"{}\"", s.replace('"', "\"\""));
    }
    s
}

/// What the totals of the bill are grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    /// One group for each material.
    Material,
    /// One group for each kind of piece (profile, screw, bracket, turned body, swept body).
    Kind,
    /// One group for each name of part (the lines of one part in other materials or sizes are
    /// added up).
    Name,
}

impl GroupBy {
    /// The word of the command line.
    pub fn name(self) -> &'static str {
        match self {
            GroupBy::Material => "material",
            GroupBy::Kind => "kind",
            GroupBy::Name => "name",
        }
    }

    /// The grouping with a word of the command line.
    pub fn from_name(name: &str) -> Option<GroupBy> {
        match name {
            "material" => Some(GroupBy::Material),
            "kind" => Some(GroupBy::Kind),
            "name" => Some(GroupBy::Name),
            _ => None,
        }
    }
}

/// The totals of the lines of the bill that share a material or a kind of piece. The cost is the
/// one of the lines, before the markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The material, the name of the part, or the kind of piece (the word of the comma separated
    /// values, or the word of the language in the text).
    pub name: String,
    /// How many lines of the bill are in the group.
    pub lines: usize,
    /// How many pieces.
    pub pieces: u32,
    /// The volume of all of them, in cubic nanometres.
    pub volume_nm3: i128,
    /// The mass of all of them, in micrograms.
    pub mass_ug: i128,
    /// The cost of all of them, in thousandths of the currency.
    pub cost_milli: i128,
}

fn kind_label(line: &Line, lang: Lang) -> &'static str {
    if line.turned {
        Msg::KindTurned.tr(lang)
    } else if line.swept {
        Msg::KindSwept.tr(lang)
    } else {
        kind_msg(line.kind).tr(lang)
    }
}

/// The totals of the bill by material or by kind, in the order of the names. With a language the
/// kinds are named in it; without one they are the words of the comma separated values.
pub fn groups(bom: &Bom, by: GroupBy, lang: Option<Lang>) -> Vec<Group> {
    let mut found: BTreeMap<String, Group> = BTreeMap::new();
    for l in &bom.lines {
        let name = match (by, lang) {
            (GroupBy::Material, _) => l.material.clone(),
            (GroupBy::Name, _) => l.name.clone(),
            (GroupBy::Kind, None) => kind_word(l).to_string(),
            (GroupBy::Kind, Some(lang)) => kind_label(l, lang).to_string(),
        };
        let g = found.entry(name.clone()).or_insert(Group {
            name,
            lines: 0,
            pieces: 0,
            volume_nm3: 0,
            mass_ug: 0,
            cost_milli: 0,
        });
        let quantity = i128::from(l.quantity);
        g.lines += 1;
        g.pieces = g.pieces.saturating_add(l.quantity);
        g.volume_nm3 = g
            .volume_nm3
            .saturating_add(l.unit_volume_nm3.saturating_mul(quantity));
        g.mass_ug = g.mass_ug.saturating_add(l.total_mass_ug());
        g.cost_milli = g.cost_milli.saturating_add(l.total_cost_milli());
    }
    found.into_values().collect()
}

/// The totals by group as comma separated values, the same text in any language: a row for each
/// group and a last row `total` with the figures of the whole bill (the cost before the markup).
pub fn groups_csv(bom: &Bom, by: GroupBy) -> String {
    let mut out = format!(
        "{},lines,pieces,volume_mm3,mass_g,cost,currency\n",
        by.name()
    );
    let mut row = |name: &str, lines: usize, g: (u32, i128, i128, i128)| {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{}",
            cell(name),
            lines,
            g.0,
            scaled(g.1, NM3_PER_MM3),
            mass_g_text(g.2),
            money_text(g.3),
            bom.currency
        );
    };
    for g in groups(bom, by, None) {
        row(
            &g.name,
            g.lines,
            (g.pieces, g.volume_nm3, g.mass_ug, g.cost_milli),
        );
    }
    row(
        "total",
        bom.lines.len(),
        (bom.pieces, bom.volume_nm3, bom.mass_ug, bom.subtotal_milli),
    );
    out
}

/// The totals by group as lines of text in a language: the way they are grouped and a line for
/// each group.
pub fn groups_text_in(bom: &Bom, by: GroupBy, lang: Lang) -> String {
    let title = match by {
        GroupBy::Material => Msg::BomByMaterial,
        GroupBy::Kind => Msg::BomByKind,
        GroupBy::Name => Msg::BomByName,
    };
    let mut out = format!("{}\n", title.tr(lang));
    for g in groups(bom, by, Some(lang)) {
        let _ = writeln!(
            out,
            "{}",
            fill(
                Msg::BomGroupLine.tr(lang),
                &[
                    &g.name,
                    &g.lines.to_string(),
                    &g.pieces.to_string(),
                    &scaled(g.volume_nm3, NM3_PER_MM3),
                    &mass_g_text(g.mass_ug),
                    &money_text(g.cost_milli),
                    &bom.currency,
                ]
            )
        );
    }
    out
}

fn kind_word(line: &Line) -> &'static str {
    if line.turned {
        "turned"
    } else if line.swept {
        "swept"
    } else {
        line.kind.name()
    }
}

/// The bill as comma separated values, the same text in any language: a line for each line of the
/// bill and then the rows `subtotal`, `markup` and `total`.
pub fn csv(bom: &Bom) -> String {
    let mut out = String::from(
        "item,name,kind,material,density_kg_m3,size_x_mm,size_y_mm,size_z_mm,quantity,unit_volume_mm3,unit_mass_g,total_mass_g,price_per_kg,unit_cost,total_cost,currency,priced\n",
    );
    for l in &bom.lines {
        let priced = if l.price_milli_per_kg.is_some() {
            "yes"
        } else {
            "no"
        };
        let cells = [
            l.item.to_string(),
            cell(&l.name),
            kind_word(l).to_string(),
            cell(&l.material),
            l.density.to_string(),
            mm_text(l.size[0]),
            mm_text(l.size[1]),
            mm_text(l.size[2]),
            l.quantity.to_string(),
            scaled(l.unit_volume_nm3, NM3_PER_MM3),
            mass_g_text(l.unit_mass_ug),
            mass_g_text(l.total_mass_ug()),
            l.price_milli_per_kg
                .map_or_else(String::new, |p| money_text(i128::from(p))),
            money_text(l.unit_cost_milli),
            money_text(l.total_cost_milli()),
            bom.currency.clone(),
            priced.to_string(),
        ];
        let _ = writeln!(out, "{}", cells.join(","));
    }
    let summary = |name: &str, set: &dyn Fn(&mut Vec<String>)| {
        let mut cells = vec![String::new(); COLUMNS];
        cells[0] = name.to_string();
        cells[15] = bom.currency.clone();
        set(&mut cells);
        cells.join(",")
    };
    let _ = writeln!(
        out,
        "{}",
        summary("subtotal", &|c| {
            c[8] = bom.pieces.to_string();
            c[9] = scaled(bom.volume_nm3, NM3_PER_MM3);
            c[11] = mass_g_text(bom.mass_ug);
            c[14] = money_text(bom.subtotal_milli);
        })
    );
    let _ = writeln!(
        out,
        "{}",
        summary("markup", &|c| {
            c[1] = format!("{}%", bom.markup_percent);
            c[14] = money_text(bom.markup_milli);
        })
    );
    let _ = writeln!(
        out,
        "{}",
        summary("total", &|c| {
            c[14] = money_text(bom.total_milli);
        })
    );
    out
}

/// The bill as lines of text in a language: a line for each line of the bill, the totals, the cost
/// and, when some material has no price, how many lines are left out of the cost.
pub fn text_in(bom: &Bom, lang: Lang) -> String {
    let mut out = String::new();
    for l in &bom.lines {
        let kind = kind_label(l, lang);
        let size = format!(
            "{} x {} x {} mm",
            mm_text(l.size[0]),
            mm_text(l.size[1]),
            mm_text(l.size[2])
        );
        let _ = writeln!(
            out,
            "{}",
            fill(
                Msg::BomLine.tr(lang),
                &[
                    &l.item.to_string(),
                    &l.name,
                    kind,
                    &l.material,
                    &size,
                    &l.quantity.to_string(),
                    &mass_g_text(l.unit_mass_ug),
                    &mass_g_text(l.total_mass_ug()),
                    &money_text(l.total_cost_milli()),
                    &bom.currency,
                ]
            )
        );
    }
    let _ = writeln!(
        out,
        "{}",
        fill(
            Msg::BomTotals.tr(lang),
            &[
                &bom.pieces.to_string(),
                &scaled(bom.volume_nm3, NM3_PER_MM3),
                &mass_g_text(bom.mass_ug)
            ]
        )
    );
    let _ = writeln!(
        out,
        "{}",
        fill(
            Msg::BomCost.tr(lang),
            &[
                &money_text(bom.subtotal_milli),
                &money_text(bom.markup_milli),
                &money_text(bom.total_milli),
                &bom.currency,
            ]
        )
    );
    if bom.unpriced > 0 {
        let _ = writeln!(
            out,
            "{}",
            fill(Msg::BomUnpriced.tr(lang), &[&bom.unpriced.to_string()])
        );
    }
    out
}

/// The bill of one assembly: the parts that mates hold together, or the parts and bodies that no
/// mate holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssemblyBom {
    /// The name of the part of the assembly that comes first by name; empty for the parts and
    /// bodies that belong to no assembly.
    pub name: String,
    /// The names of the parts of the assembly in order (the bodies of the rest, by name).
    pub parts: Vec<String>,
    /// The bill of the assembly alone: its lines, its totals and its markup.
    pub bom: Bom,
}

/// The first part of the set that a part belongs to, found with the sets joined so far.
fn set_root(parent: &mut BTreeMap<Id, Id>, id: Id) -> Id {
    let mut top = id;
    while let Some(next) = parent.get(&top).copied().filter(|n| *n != top) {
        top = next;
    }
    let mut at = id;
    while at != top {
        let next = parent.insert(at, top).unwrap_or(top);
        at = next;
    }
    top
}

/// The bills of the assemblies of a model. An assembly is a set of parts joined by mates (a mate
/// joins the part it places to the part it is measured from, and mates that join through a third
/// part make one set); each has its own bill, priced alone. The parts that no mate holds and the
/// turned and swept bodies, which a mate cannot place, are the last bill, with an empty name.
/// Assemblies come in the order of the name of their first part, and a model with no mates has
/// only the last bill.
pub fn by_assembly(
    parts: &BTreeMap<Id, PartInstance>,
    turned: &BTreeMap<Id, Turned>,
    swept: &BTreeMap<Id, SweptBody>,
    densities: &BTreeMap<String, u32>,
    mates: &BTreeMap<Id, Mate>,
    cost: &CostSettings,
) -> Result<Vec<AssemblyBom>, CadError> {
    let mut parent: BTreeMap<Id, Id> = parts.keys().map(|k| (*k, *k)).collect();
    let mut mated: BTreeSet<Id> = BTreeSet::new();
    for m in mates.values() {
        if m.a != m.b && parts.contains_key(&m.a) && parts.contains_key(&m.b) {
            let (ra, rb) = (set_root(&mut parent, m.a), set_root(&mut parent, m.b));
            if ra != rb {
                parent.insert(rb, ra);
            }
            mated.insert(m.a);
            mated.insert(m.b);
        }
    }
    let mut sets: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
    for id in &mated {
        let top = set_root(&mut parent, *id);
        sets.entry(top).or_default().push(*id);
    }
    let mut named: Vec<(String, Id, Vec<Id>)> = sets
        .into_values()
        .filter_map(|ids| {
            ids.iter()
                .map(|i| (parts[i].name.clone(), *i))
                .min()
                .map(|(name, first)| (name, first, ids))
        })
        .collect();
    named.sort();
    let mut out = Vec::new();
    for (name, _, ids) in named {
        let subset: BTreeMap<Id, PartInstance> =
            ids.iter().map(|i| (*i, parts[i].clone())).collect();
        let list =
            crate::partlist::list_with(&subset, &BTreeMap::new(), &BTreeMap::new(), densities)?;
        let mut names: Vec<String> = subset.values().map(|p| p.name.clone()).collect();
        names.sort();
        out.push(AssemblyBom {
            name,
            parts: names,
            bom: from_list(&list, cost)?,
        });
    }
    let loose: BTreeMap<Id, PartInstance> = parts
        .iter()
        .filter(|(id, _)| !mated.contains(*id))
        .map(|(id, p)| (*id, p.clone()))
        .collect();
    let list = crate::partlist::list_with(&loose, turned, swept, densities)?;
    if !list.rows.is_empty() {
        let mut names: Vec<String> = list.rows.iter().map(|r| r.name.clone()).collect();
        names.sort();
        out.push(AssemblyBom {
            name: String::new(),
            parts: names,
            bom: from_list(&list, cost)?,
        });
    }
    Ok(out)
}

/// The bills of the assemblies as comma separated values, the same text in any language: the
/// columns of the bill with the name of the assembly in front (`(no assembly)` for the last bill),
/// and under each its rows `subtotal`, `markup` and `total`.
pub fn assemblies_csv(list: &[AssemblyBom]) -> String {
    let mut out = String::new();
    for (k, a) in list.iter().enumerate() {
        let label = if a.name.is_empty() {
            "(no assembly)".to_string()
        } else {
            cell(&a.name)
        };
        let text = csv(&a.bom);
        for (n, line) in text.lines().enumerate() {
            if n == 0 {
                if k == 0 {
                    let _ = writeln!(out, "assembly,{line}");
                }
                continue;
            }
            let _ = writeln!(out, "{label},{line}");
        }
    }
    if out.is_empty() {
        out.push_str("assembly\n");
    }
    out
}

/// The bills of the assemblies as lines of text in a language: a title for each assembly with the
/// number of its parts, then its bill.
pub fn assemblies_text_in(list: &[AssemblyBom], lang: Lang) -> String {
    let mut out = String::new();
    for a in list {
        let title = if a.name.is_empty() {
            Msg::BomNoAssembly.tr(lang).to_string()
        } else {
            fill(
                Msg::BomAssembly.tr(lang),
                &[&a.name, &a.parts.len().to_string()],
            )
        };
        let _ = writeln!(out, "{title}");
        out.push_str(&text_in(&a.bom, lang));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, part: &str, material: &str, density: u32, mass_ug: i128) -> Row {
        Row {
            name: name.into(),
            kind: Kind::Profile,
            turned: false,
            swept: false,
            part: part.into(),
            material: material.into(),
            density,
            // a volume that goes with the mass
            volume_nm3: mass_ug * NM3_PER_MM3 / i128::from(density),
            mass_ug,
            min: [0, 0, 0],
            size: [10_000_000, 10_000_000, 10_000_000],
        }
    }

    fn body(name: &str, material: &str, density: u32, mass_ug: i128) -> Row {
        Row {
            turned: true,
            part: "plate".into(),
            size: [20_000_000, 20_000_000, 5_000_000],
            ..row(name, "plate", material, density, mass_ug)
        }
    }

    fn list(rows: Vec<Row>) -> List {
        List {
            volume_nm3: rows.iter().map(|r| r.volume_nm3).sum(),
            mass_ug: rows.iter().map(|r| r.mass_ug).sum(),
            rows,
        }
    }

    fn cost(per_piece: u32, markup: u32) -> CostSettings {
        CostSettings {
            currency: "EUR".into(),
            per_piece_milli: per_piece,
            markup_percent: markup,
            prices_milli_per_kg: BTreeMap::from([
                ("steel".to_owned(), 2_000),
                ("PLA".to_owned(), 20_000),
            ]),
        }
    }

    /// Three steel cubes of 8 g, an aluminium cube of 2.7 g that has no price and a turned ring of
    /// 12.4 g of PLA.
    fn sample() -> List {
        list(vec![
            row("a", "cube", "steel", 8000, 8_000_000),
            row("b", "cube", "steel", 8000, 8_000_000),
            row("c", "cube", "steel", 8000, 8_000_000),
            row("d", "cube", "aluminium", 2700, 2_700_000),
            body("ring", "PLA", 1240, 12_400_000),
        ])
    }

    #[test]
    fn the_totals_by_material_and_by_kind_add_up_to_the_bill_before_the_markup() {
        let bom = from_list(&sample(), &cost(100, 10)).unwrap();
        // by material: aluminium 0.100, PLA 0.348, steel 3 x 0.116 = 0.348
        let by_material = groups(&bom, GroupBy::Material, None);
        let summary: Vec<(&str, usize, u32, i128)> = by_material
            .iter()
            .map(|g| (g.name.as_str(), g.lines, g.pieces, g.cost_milli))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("PLA", 1, 1, 348),
                ("aluminium", 1, 1, 100),
                ("steel", 1, 3, 348)
            ]
        );
        assert_eq!(by_material[2].mass_ug, 3 * 8_000_000);
        assert_eq!(
            by_material.iter().map(|g| g.cost_milli).sum::<i128>(),
            bom.subtotal_milli
        );
        // by kind: the three cubes and the aluminium one are profiles, the ring is a turned body
        let by_kind = groups(&bom, GroupBy::Kind, None);
        let kinds: Vec<(&str, usize, u32)> = by_kind
            .iter()
            .map(|g| (g.name.as_str(), g.lines, g.pieces))
            .collect();
        assert_eq!(kinds, vec![("profile", 2, 4), ("turned", 1, 1)]);
        assert_eq!(by_kind.iter().map(|g| g.pieces).sum::<u32>(), bom.pieces);
        assert_eq!(by_kind.iter().map(|g| g.mass_ug).sum::<i128>(), bom.mass_ug);
        assert_eq!(
            by_kind.iter().map(|g| g.volume_nm3).sum::<i128>(),
            bom.volume_nm3
        );
        // the kinds are named in the language when it is given
        let es = groups(&bom, GroupBy::Kind, Some(Lang::Es));
        assert!(es.iter().any(|g| g.name == Msg::KindTurned.tr(Lang::Es)));
        assert_eq!(GroupBy::from_name("kind"), Some(GroupBy::Kind));
        assert_eq!(GroupBy::from_name("size"), None);
        assert_eq!(GroupBy::Material.name(), "material");
    }

    #[test]
    fn the_totals_by_name_add_up_the_lines_of_a_part_in_other_materials() {
        let bom = from_list(&sample(), &cost(100, 10)).unwrap();
        let by_name = groups(&bom, GroupBy::Name, None);
        // the cubes are one part in two materials and the ring is a body of its own
        let names: Vec<(&str, usize, u32)> = by_name
            .iter()
            .map(|g| (g.name.as_str(), g.lines, g.pieces))
            .collect();
        let mut expected: Vec<(String, usize, u32)> = Vec::new();
        for l in &bom.lines {
            match expected.iter_mut().find(|e| e.0 == l.name) {
                Some(e) => {
                    e.1 += 1;
                    e.2 += l.quantity;
                }
                None => expected.push((l.name.clone(), 1, l.quantity)),
            }
        }
        expected.sort();
        let expected: Vec<(&str, usize, u32)> =
            expected.iter().map(|e| (e.0.as_str(), e.1, e.2)).collect();
        assert_eq!(names, expected);
        assert!(by_name.len() < bom.lines.len(), "{names:?}");
        assert_eq!(by_name.iter().map(|g| g.pieces).sum::<u32>(), bom.pieces);
        assert_eq!(by_name.iter().map(|g| g.mass_ug).sum::<i128>(), bom.mass_ug);
        assert_eq!(
            by_name.iter().map(|g| g.cost_milli).sum::<i128>(),
            bom.subtotal_milli
        );
        assert_eq!(GroupBy::from_name("name"), Some(GroupBy::Name));
        assert_eq!(GroupBy::Name.name(), "name");
        assert!(groups_csv(&bom, GroupBy::Name).starts_with("name,lines,pieces,"));
        assert!(groups_text_in(&bom, GroupBy::Name, Lang::En).starts_with("Totals by part name\n"));
        // every language names the grouping and leaves no hole in the text
        for lang in Lang::ALL {
            let t = groups_text_in(&bom, GroupBy::Name, lang);
            assert!(t.lines().count() >= 2, "{t}");
            assert!(!t.contains('{'), "{t}");
        }
    }

    #[test]
    fn the_totals_by_group_are_written_as_values_and_as_text_in_each_language() {
        let bom = from_list(&sample(), &cost(100, 10)).unwrap();
        let csv = groups_csv(&bom, GroupBy::Material);
        assert_eq!(
            csv,
            "material,lines,pieces,volume_mm3,mass_g,cost,currency\n\
PLA,1,1,10000.000,12.400,0.348,EUR\n\
aluminium,1,1,1000.000,2.700,0.100,EUR\n\
steel,1,3,3000.000,24.000,0.348,EUR\n\
total,3,5,14000.000,39.100,0.796,EUR\n"
        );
        assert!(groups_csv(&bom, GroupBy::Kind).starts_with("kind,lines,pieces,"));
        let text = groups_text_in(&bom, GroupBy::Material, Lang::En);
        assert!(text.starts_with("Totals by material\n"), "{text}");
        assert!(text.contains("steel: 1 lines, 3 pieces,"), "{text}");
        assert!(text.contains("cost 0.348 EUR"), "{text}");
        for lang in Lang::ALL {
            for by in [GroupBy::Material, GroupBy::Kind] {
                let t = groups_text_in(&bom, by, lang);
                assert!(t.lines().count() >= 3, "{t}");
                assert!(!t.contains('{'), "{t}");
            }
        }
        // a name that could be taken for a formula is quoted as in the bill
        let mut risky = bom.clone();
        risky.lines[0].material = "=SUM(A1)".into();
        assert!(groups_csv(&risky, GroupBy::Material).contains("\n'=SUM(A1),"));
    }

    #[test]
    fn the_same_parts_are_one_line_and_the_cost_is_worked_out_by_hand() {
        // steel: 8 g at 2 per kg is 0.016; a piece costs 0.100 besides its material, so a cube is
        // 0.116 and three are 0.348. The aluminium cube has no price: 0.100. The ring: 12.4 g at
        // 20 per kg is 0.248, plus 0.100 is 0.348. The sum is 0.796; 10 % of it is 0.0796, which
        // rounds to 0.080; the total is 0.876.
        let bom = from_list(&sample(), &cost(100, 10)).unwrap();
        assert_eq!(bom.lines.len(), 3);
        let names: Vec<(&str, &str, u32)> = bom
            .lines
            .iter()
            .map(|l| (l.name.as_str(), l.material.as_str(), l.quantity))
            .collect();
        assert_eq!(
            names,
            vec![
                ("cube", "aluminium", 1),
                ("cube", "steel", 3),
                ("ring", "PLA", 1)
            ]
        );
        assert_eq!(
            bom.lines.iter().map(|l| l.item).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(bom.lines[1].unit_cost_milli, 116);
        assert_eq!(bom.lines[1].total_cost_milli(), 348);
        assert_eq!(bom.lines[0].unit_cost_milli, 100);
        assert_eq!(bom.lines[0].price_milli_per_kg, None);
        assert_eq!(bom.lines[2].unit_cost_milli, 348);
        assert_eq!(bom.pieces, 5);
        assert_eq!(bom.mass_ug, 3 * 8_000_000 + 2_700_000 + 12_400_000);
        assert_eq!(bom.lines[1].total_mass_ug(), 24_000_000);
        assert_eq!(
            (bom.subtotal_milli, bom.markup_milli, bom.total_milli),
            (796, 80, 876)
        );
        assert_eq!(bom.unpriced, 1);
        assert_eq!(bom.currency, "EUR");
        // a piece costs only its material by default and nothing is added
        let plain = from_list(&sample(), &cost(0, 0)).unwrap();
        assert_eq!(
            (plain.subtotal_milli, plain.markup_milli, plain.total_milli),
            (3 * 16 + 248, 0, 3 * 16 + 248)
        );
    }

    #[test]
    fn an_empty_bill_has_no_lines_and_every_total_is_zero() {
        let bom = from_list(&list(Vec::new()), &cost(100, 10)).unwrap();
        assert!(bom.lines.is_empty());
        assert_eq!(
            (bom.pieces, bom.volume_nm3, bom.mass_ug, bom.unpriced),
            (0, 0, 0, 0)
        );
        assert_eq!(
            (bom.subtotal_milli, bom.markup_milli, bom.total_milli),
            (0, 0, 0)
        );
        // the values have the header and the three rows of the totals
        let text = csv(&bom);
        let rows: Vec<&str> = text.lines().collect();
        assert_eq!(rows.len(), 4, "{text}");
        for r in &rows {
            assert_eq!(r.split(',').count(), COLUMNS, "{r}");
        }
        assert!(rows[1].starts_with("subtotal,"), "{text}");
        // no group and only the row of the whole in the totals by group
        for by in [GroupBy::Material, GroupBy::Kind, GroupBy::Name] {
            assert!(groups(&bom, by, None).is_empty());
            let by_group = groups_csv(&bom, by);
            assert_eq!(by_group.lines().count(), 2, "{by_group}");
            assert!(
                by_group.ends_with("total,0,0,0.000,0.000,0.000,EUR\n"),
                "{by_group}"
            );
        }
        // the text says nothing about prices when there is no line to price
        for lang in Lang::ALL {
            let t = text_in(&bom, lang);
            assert_eq!(t.lines().count(), 2, "{t}");
            assert!(!t.contains('{'), "{t}");
        }
    }

    #[test]
    fn the_markup_rounds_half_up_and_a_price_of_zero_is_a_price() {
        let free = |per_piece: u32, markup: u32| {
            let mut c = cost(per_piece, markup);
            c.prices_milli_per_kg.insert("steel".to_owned(), 0);
            from_list(&list(vec![row("a", "cube", "steel", 8000, 8_000_000)]), &c).unwrap()
        };
        // 5 thousandths with 10 % is half a thousandth: it goes up
        let half = free(5, 10);
        assert_eq!(
            (half.subtotal_milli, half.markup_milli, half.total_milli),
            (5, 1, 6)
        );
        // 4 thousandths with 10 % is 0.4: it goes down
        let below = free(4, 10);
        assert_eq!((below.markup_milli, below.total_milli), (0, 4));
        // a price of zero is a price: the line is priced and costs only its piece
        assert_eq!(half.unpriced, 0);
        assert_eq!(half.lines[0].price_milli_per_kg, Some(0));
        assert_eq!(half.lines[0].unit_cost_milli, 5);
        assert!(csv(&half).contains(",0.000,0.005,0.005,EUR,yes\n"));
        // a markup of 100 % doubles the subtotal and one of 0 % leaves it
        assert_eq!(free(7, 100).total_milli, 14);
        assert_eq!(free(7, 0).total_milli, 7);
    }

    #[test]
    fn the_same_part_in_another_size_or_kind_is_a_line_of_its_own() {
        let mut rows = vec![
            row("a", "bar", "steel", 8000, 8_000_000),
            row("b", "bar", "steel", 8000, 8_000_000),
            row("c", "bar", "steel", 8000, 16_000_000),
            row("d", "bar", "steel", 8000, 8_000_000),
        ];
        rows[2].size = [20_000_000, 10_000_000, 10_000_000];
        rows[3].kind = Kind::Screw;
        let bom = from_list(&list(rows), &cost(0, 0)).unwrap();
        let seen: Vec<(Kind, u32, i64)> = bom
            .lines
            .iter()
            .map(|l| (l.kind, l.quantity, l.size[0]))
            .collect();
        assert_eq!(bom.lines.len(), 3, "{seen:?}");
        assert_eq!(bom.pieces, 4);
        assert_eq!(bom.lines.iter().map(|l| l.quantity).sum::<u32>(), 4);
        // the numbers of the lines run from 1 without a gap
        assert_eq!(
            bom.lines.iter().map(|l| l.item).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        // a turned body and a part of the same name are not the same line
        let mut mixed = list(vec![
            row("ring", "ring", "PLA", 1240, 1_000_000),
            body("ring", "PLA", 1240, 1_000_000),
        ]);
        mixed.rows[1].size = mixed.rows[0].size;
        assert_eq!(from_list(&mixed, &cost(0, 0)).unwrap().lines.len(), 2);
    }

    #[test]
    fn money_and_cells_hold_at_the_edges_of_their_ranges() {
        assert_eq!(money_text(-5), "-0.005");
        assert_eq!(money_text(999), "0.999");
        assert_eq!(money_text(1_000), "1.000");
        // the most negative figure does not panic and keeps its sign
        assert!(money_text(i128::MIN).starts_with("-170141183460469231731687303715884105"));
        assert!(money_text(i128::MAX).starts_with("170141183460469231731687303715884105"));
        // nothing to cost and the dearest price in the settings
        assert_eq!(material_cost_milli(0, u32::MAX).unwrap(), 0);
        assert_eq!(
            material_cost_milli(1_000_000_000, u32::MAX).unwrap(),
            i128::from(u32::MAX)
        );
        // cells: empty, a tab or a carriage return in front, a name with only a quote
        assert_eq!(cell(""), "");
        assert_eq!(cell("\tcmd"), "'\tcmd");
        assert_eq!(cell("\rcmd"), "\"'\rcmd\"");
        assert_eq!(cell("\""), "\"\"\"\"");
        assert_eq!(cell("tube \u{00d8}20"), "tube \u{00d8}20");
        assert_eq!(cell("x-y+z@w"), "x-y+z@w");
    }

    #[test]
    fn money_is_written_with_three_decimals() {
        assert_eq!(money_text(0), "0.000");
        assert_eq!(money_text(5), "0.005");
        assert_eq!(money_text(12_345), "12.345");
        assert_eq!(money_text(-1_500), "-1.500");
        // half a thousandth rounds up: 1 g at 0.5 per kg is 0.0005
        assert_eq!(material_cost_milli(1_000_000, 500).unwrap(), 1);
        assert_eq!(material_cost_milli(999_999, 500).unwrap(), 0);
        assert!(material_cost_milli(i128::MAX, 2).is_err());
    }

    #[test]
    fn the_csv_has_a_line_for_each_line_and_the_rows_of_the_totals() {
        let bom = from_list(&sample(), &cost(100, 10)).unwrap();
        let text = csv(&bom);
        let rows: Vec<&str> = text.lines().collect();
        assert_eq!(rows.len(), 1 + 3 + 3);
        let header: Vec<&str> = rows[0].split(',').collect();
        assert_eq!(header.len(), COLUMNS);
        assert_eq!(header[8], "quantity");
        for r in &rows[1..] {
            assert_eq!(r.split(',').count(), COLUMNS, "{r}");
        }
        let steel: Vec<&str> = rows[2].split(',').collect();
        assert_eq!(&steel[..5], ["2", "cube", "profile", "steel", "8000"]);
        assert_eq!(&steel[5..8], ["10", "10", "10"]);
        assert_eq!(steel[8], "3");
        assert_eq!(steel[10], "8.000");
        assert_eq!(steel[11], "24.000");
        assert_eq!(&steel[12..], ["2.000", "0.116", "0.348", "EUR", "yes"]);
        // the part without a price has no price and says so
        let light: Vec<&str> = rows[1].split(',').collect();
        assert_eq!(&light[12..], ["", "0.100", "0.100", "EUR", "no"]);
        let ring: Vec<&str> = rows[3].split(',').collect();
        assert_eq!(ring[2], "turned");
        assert_eq!(&ring[5..8], ["20", "20", "5"]);
        assert!(rows[4].starts_with("subtotal,"), "{}", rows[4]);
        assert!(rows[4].ends_with(",0.796,EUR,"), "{}", rows[4]);
        assert!(rows[5].starts_with("markup,10%,"), "{}", rows[5]);
        assert!(rows[5].ends_with(",0.080,EUR,"), "{}", rows[5]);
        assert!(rows[6].ends_with(",0.876,EUR,"), "{}", rows[6]);
        // a swept body says so too
        let mut swept = sample();
        swept.rows[4].turned = false;
        swept.rows[4].swept = true;
        let again = csv(&from_list(&swept, &cost(0, 0)).unwrap());
        assert!(again.contains(",swept,"), "{again}");
    }

    #[test]
    fn a_name_cannot_break_the_row_or_become_a_formula() {
        assert_eq!(cell("plain"), "plain");
        assert_eq!(cell("a,b"), "\"a,b\"");
        assert_eq!(cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(cell("two\nlines"), "\"two\nlines\"");
        assert_eq!(cell("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(cell("+1"), "'+1");
        assert_eq!(cell("-1"), "'-1");
        assert_eq!(cell("@sum"), "'@sum");
        assert_eq!(cell("a=b"), "a=b");
        let mut hostile = sample();
        hostile.rows[4].name = "=cmd,\"x\"\nnext".into();
        let text = csv(&from_list(&hostile, &cost(0, 0)).unwrap());
        // the quoted name holds the line break; the columns of every other row are as many
        assert!(text.contains("\"'=cmd,\"\"x\"\"\nnext\""), "{text}");
    }

    #[test]
    fn the_text_is_in_the_language_and_says_what_has_no_price() {
        let bom = from_list(&sample(), &cost(100, 10)).unwrap();
        let en = text_in(&bom, Lang::En);
        let lines: Vec<&str> = en.lines().collect();
        assert_eq!(lines.len(), 3 + 3);
        assert_eq!(
            lines[1],
            "2. cube (profile, steel, 10 x 10 x 10 mm): 3 x 8.000 g = 24.000 g, cost 0.348 EUR"
        );
        assert_eq!(lines[3], "pieces: 5, volume: 14000.000 mm3, mass: 39.100 g");
        assert_eq!(
            lines[4],
            "subtotal 0.796 EUR, markup 0.080 EUR, total 0.876 EUR"
        );
        assert_eq!(
            lines[5],
            "no price for the material of 1 lines (cost left out)"
        );
        // with every material priced the last line is not there
        let mut priced = cost(0, 0);
        priced.prices_milli_per_kg.insert("aluminium".into(), 6_000);
        let all = text_in(&from_list(&sample(), &priced).unwrap(), Lang::En);
        assert_eq!(all.lines().count(), 5);
        for lang in Lang::ALL {
            let t = text_in(&bom, lang);
            assert_eq!(t.lines().count(), 6, "{lang:?}");
            assert!(t.contains("0.876"), "{lang:?}");
        }
        assert!(text_in(&bom, Lang::De).contains("Zwischensumme 0.796 EUR"));
        assert!(text_in(&bom, Lang::Es).contains("coste 0.348 EUR"));
    }

    fn mm(v: i64) -> crate::Length {
        crate::Length::from_mm(v).unwrap()
    }

    fn nid(n: u64) -> Id {
        Id::from_parts(1_700_000_000_000, 1, n).unwrap()
    }

    fn cube(name: &str, material: &str, x: i64) -> PartInstance {
        use crate::catalog::{Orientation, Part, Shape};
        PartInstance {
            name: name.into(),
            library: "lib".into(),
            part: Part {
                name: "cube".into(),
                material: material.into(),
                shape: Shape::Profile {
                    width: mm(10),
                    height: mm(10),
                    wall: crate::Length::ZERO,
                    default_cut: mm(10),
                    max_cut: mm(100),
                },
            },
            at: [mm(x), crate::Length::ZERO, crate::Length::ZERO],
            orientation: Orientation::default(),
            cut: Some(mm(10)),
        }
    }

    fn densities() -> BTreeMap<String, u32> {
        BTreeMap::from([("steel".to_owned(), 8000), ("PLA".to_owned(), 1240)])
    }

    fn stack(a: u64, b: u64) -> (Id, Mate) {
        use crate::assembly::MateKind;
        (
            nid(100 + a * 10 + b),
            Mate {
                a: nid(a),
                b: nid(b),
                kind: MateKind::Stack,
            },
        )
    }

    fn parts_of(list: &[(u64, &str, &str)]) -> BTreeMap<Id, PartInstance> {
        list.iter()
            .map(|(n, name, material)| (nid(*n), cube(name, material, *n as i64 * 20)))
            .collect()
    }

    fn split(parts: &BTreeMap<Id, PartInstance>, mates: &[(Id, Mate)]) -> Vec<AssemblyBom> {
        let mates: BTreeMap<Id, Mate> = mates.iter().cloned().collect();
        by_assembly(
            parts,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &densities(),
            &mates,
            &cost(100, 10),
        )
        .unwrap()
    }

    #[test]
    fn the_parts_that_mates_join_are_an_assembly_and_the_rest_is_the_last_bill() {
        let parts = parts_of(&[
            (1, "frame", "steel"),
            (2, "post", "steel"),
            (3, "lid", "PLA"),
            (4, "spare", "steel"),
            (5, "bolt", "steel"),
        ]);
        // the lid stands on the post and the post on the frame (a chain through a third part)
        let out = split(&parts, &[stack(1, 2), stack(2, 3)]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "frame");
        assert_eq!(out[0].parts, ["frame", "lid", "post"]);
        assert_eq!(out[0].bom.pieces, 3);
        assert_eq!(out[1].name, "");
        assert_eq!(out[1].parts, ["bolt", "spare"]);
        assert_eq!(out[1].bom.pieces, 2);
        // between them they hold every piece and every microgram of the model
        let whole = from_list(
            &crate::partlist::list(&parts, &densities()).unwrap(),
            &cost(100, 10),
        )
        .unwrap();
        assert_eq!(out.iter().map(|a| a.bom.pieces).sum::<u32>(), whole.pieces);
        assert_eq!(
            out.iter().map(|a| a.bom.mass_ug).sum::<i128>(),
            whole.mass_ug
        );
        assert_eq!(
            out.iter().map(|a| a.bom.volume_nm3).sum::<i128>(),
            whole.volume_nm3
        );
    }

    #[test]
    fn assemblies_come_in_the_order_of_their_first_name_and_what_mates_cannot_join_is_ignored() {
        let parts = parts_of(&[
            (1, "zeta", "steel"),
            (2, "zed", "steel"),
            (3, "beta", "steel"),
            (4, "alpha", "steel"),
            (6, "lonely", "PLA"),
        ]);
        let ghost = stack(6, 9);
        let itself = (
            nid(200),
            Mate {
                a: nid(6),
                b: nid(6),
                kind: crate::assembly::MateKind::Stack,
            },
        );
        let out = split(&parts, &[stack(1, 2), stack(3, 4), ghost, itself]);
        let names: Vec<&str> = out.iter().map(|a| a.name.as_str()).collect();
        // the first name of {beta, alpha} is alpha and of {zeta, zed} is zed; the lonely part is the rest
        assert_eq!(names, ["alpha", "zed", ""]);
        assert_eq!(out[2].parts, ["lonely"]);
        // without any mate the model is one bill with no name
        let one = split(&parts, &[]);
        assert_eq!(one.len(), 1);
        assert!(one[0].name.is_empty());
        assert_eq!(one[0].bom.pieces, 5);
        assert!(split(&BTreeMap::new(), &[]).is_empty());
    }

    #[test]
    fn the_bills_of_the_assemblies_are_comma_separated_values_with_the_assembly_in_front() {
        let parts = parts_of(&[
            (1, "=frame", "steel"),
            (2, "post", "steel"),
            (3, "loose one", "PLA"),
        ]);
        let out = split(&parts, &[stack(1, 2)]);
        let csv = assemblies_csv(&out);
        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines[0].starts_with("assembly,item,name,kind,material,"));
        // one header, and for each assembly its lines and its three rows of totals
        assert_eq!(lines.len(), 1 + (1 + 3) + (1 + 3));
        assert!(lines[1].starts_with("'=frame,1,cube,"), "{}", lines[1]);
        assert!(lines.iter().any(|l| l.starts_with("'=frame,subtotal,")));
        assert!(lines.iter().any(|l| l.starts_with("(no assembly),1,cube,")));
        assert!(lines.last().unwrap().starts_with("(no assembly),total,"));
        // the columns are those of the bill, plus the assembly
        let width = lines[0].split(',').count();
        assert!(lines.iter().all(|l| l.split(',').count() == width));
        assert_eq!(assemblies_csv(&[]), "assembly\n");
    }

    #[test]
    fn the_bills_of_the_assemblies_are_told_in_each_of_the_eight_languages() {
        let parts = parts_of(&[
            (1, "frame", "steel"),
            (2, "post", "steel"),
            (3, "bolt", "PLA"),
        ]);
        let out = split(&parts, &[stack(1, 2)]);
        let en = assemblies_text_in(&out, Lang::En);
        assert!(en.starts_with("Assembly frame: 2 parts\n"), "{en}");
        assert!(en.contains("Parts and bodies in no assembly\n"));
        let es = assemblies_text_in(&out, Lang::Es);
        assert!(es.starts_with("Conjunto frame: 2 piezas\n"), "{es}");
        let mut seen = Vec::new();
        for lang in Lang::ALL {
            let t = assemblies_text_in(&out, lang);
            assert!(t.contains(&text_in(&out[0].bom, lang)));
            assert!(!seen.contains(&t));
            seen.push(t);
        }
    }
}
