// SPDX-License-Identifier: GPL-3.0-or-later
//! The cutting plan of the profiles: which pieces are sawn from which bar.
//!
//! The cut list of the catalog says how many pieces of each length a model needs. A workshop
//! buys bars, and a saw takes a little material at every cut, so the plan fits the pieces into the
//! bars: each profile comes in bars of its longest length (`max_cut` of the part in the library),
//! a bar loses `end_trim` at each end, and every cut between two pieces takes `kerf`.
//!
//! The pieces are placed longest first, each into the bar that it fits and that is left with the
//! least room (best fit decreasing), which is deterministic: the same model gives the same plan.
//! It is a good plan and not always the best one (the problem is NP-hard), and the figures say
//! so in the documentation.
//!
//! A bar holds `n` pieces when the pieces together plus `n - 1` kerfs are not longer than the bar
//! less its two trimmed ends. What is left, less the kerf of the cut that frees the last piece, is
//! the offcut; an offcut at least `reusable_offcut` long is kept as stock, a shorter one is scrap.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::catalog::{Kind, PartInstance, Shape};
use crate::config::Config;
use crate::scad::mm_text;
use crate::{CadError, Length};

/// The most that a length of the settings can be (a metre).
pub const LENGTH_CAP_NM: i64 = 1_000_000_000;

/// The most bars that the number of bars can be set to.
pub const BARS_CAP: u32 = 1_000_000;

/// The settings of the cutting plan (`[stock]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StockSettings {
    /// The material the saw takes at every cut.
    pub kerf: Length,
    /// What is cut away from each end of a bar.
    pub end_trim: Length,
    /// The shortest offcut that is kept as stock.
    pub reusable_offcut: Length,
    /// The most bars one plan may need; more is refused.
    pub max_bars: usize,
}

fn stock_error(why: impl AsRef<str>) -> CadError {
    CadError::Part(why.as_ref().to_string())
}

impl StockSettings {
    /// The settings of the shipped file.
    pub fn shipped() -> StockSettings {
        crate::config::Settings::defaults().stock
    }

    /// Reads and checks the keys of `[stock]`.
    pub fn from_config(c: &Config) -> Result<StockSettings, CadError> {
        let length = |key: &str| -> Result<Length, CadError> {
            let v = c.length(key)?;
            if v.get() < 0 || v.get() > LENGTH_CAP_NM {
                return Err(CadError::Config(format!(
                    "{key} must be from 0 mm to {} mm",
                    LENGTH_CAP_NM / 1_000_000
                )));
            }
            Ok(v)
        };
        let bars = c.number("stock.max_bars")?;
        if bars == 0 || bars > BARS_CAP {
            return Err(CadError::Config(format!(
                "stock.max_bars must be from 1 to {BARS_CAP}"
            )));
        }
        Ok(StockSettings {
            kerf: length("stock.kerf")?,
            end_trim: length("stock.end_trim")?,
            reusable_offcut: length("stock.reusable_offcut")?,
            max_bars: usize::try_from(bars)
                .map_err(|_| CadError::Config("stock.max_bars is too large".into()))?,
        })
    }
}

/// One bar and what is sawn from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    /// The pieces, longest first, in nanometres.
    pub pieces: Vec<i64>,
    /// What is left after the pieces and the kerfs, in nanometres.
    pub offcut_nm: i64,
}

/// The plan of one profile: the bars it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockLine {
    /// The name of the profile.
    pub part: String,
    /// The material.
    pub material: String,
    /// The length of one bar.
    pub bar_nm: i64,
    /// The bars.
    pub bars: Vec<Bar>,
}

impl StockLine {
    /// How many pieces in all.
    pub fn pieces(&self) -> usize {
        self.bars.iter().map(|b| b.pieces.len()).sum()
    }

    /// The length of all the pieces, in nanometres.
    pub fn used_nm(&self) -> i128 {
        self.bars
            .iter()
            .flat_map(|b| b.pieces.iter())
            .map(|p| i128::from(*p))
            .sum()
    }

    /// The length bought, in nanometres.
    pub fn bought_nm(&self) -> i128 {
        i128::from(self.bar_nm) * self.bars.len() as i128
    }

    /// How much of the length bought is in the pieces, in parts per million (rounded down).
    pub fn yield_ppm(&self) -> u32 {
        let bought = self.bought_nm();
        if bought == 0 {
            return 0;
        }
        u32::try_from(self.used_nm() * 1_000_000 / bought).unwrap_or(1_000_000)
    }
}

/// The cutting plan of a model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// One line for each profile and material, in order of name.
    pub lines: Vec<StockLine>,
}

impl Plan {
    /// How many bars in all.
    pub fn bars(&self) -> usize {
        self.lines.iter().map(|l| l.bars.len()).sum()
    }
}

/// The room a bar has for pieces and kerfs.
fn usable(bar_nm: i64, settings: &StockSettings) -> Result<i64, CadError> {
    bar_nm
        .checked_sub(settings.end_trim.get().saturating_mul(2))
        .filter(|u| *u > 0)
        .ok_or_else(|| stock_error("the two trimmed ends take the whole bar"))
}

/// What a bar uses for the pieces it holds and the cuts between them.
fn used_by(pieces: &[i64], kerf: i64) -> i64 {
    let sum: i64 = pieces.iter().sum();
    sum + kerf * (pieces.len() as i64 - 1).max(0)
}

/// Fits the pieces of one profile into bars of `bar_nm`.
fn pack(
    mut cuts: Vec<i64>,
    bar_nm: i64,
    settings: &StockSettings,
    name: &str,
) -> Result<Vec<Bar>, CadError> {
    let room = usable(bar_nm, settings)?;
    let kerf = settings.kerf.get();
    cuts.sort_unstable_by(|a, b| b.cmp(a));
    let mut bars: Vec<Vec<i64>> = Vec::new();
    for cut in cuts {
        if cut > room {
            return Err(stock_error(format!(
                "{name}: a piece of {} mm does not fit a bar of {} mm with the ends trimmed",
                mm_text(cut),
                mm_text(bar_nm)
            )));
        }
        // the bar that has the least room left among those where the piece fits
        let mut best: Option<(usize, i64)> = None;
        for (i, bar) in bars.iter().enumerate() {
            let left = room - used_by(bar, kerf) - kerf;
            if cut <= left && best.is_none_or(|(_, l)| left - cut < l) {
                best = Some((i, left - cut));
            }
        }
        match best {
            Some((i, _)) => bars[i].push(cut),
            None => {
                if bars.len() >= settings.max_bars {
                    return Err(stock_error(format!(
                        "{name}: more than the {} bars the settings allow",
                        settings.max_bars
                    )));
                }
                bars.push(vec![cut]);
            }
        }
    }
    Ok(bars
        .into_iter()
        .map(|pieces| {
            let used = used_by(&pieces, kerf) + kerf;
            Bar {
                offcut_nm: (room - used).max(0),
                pieces,
            }
        })
        .collect())
}

/// The cutting plan of the profiles among some placed parts. A profile comes in bars of its
/// longest cut (`max_cut`); the parts that are not profiles are left out. Refused with
/// `PHOENIX-CAD-045` when a piece does not fit a bar or more bars are needed than the settings
/// allow.
pub fn plan<'a>(
    instances: impl Iterator<Item = &'a PartInstance>,
    settings: &StockSettings,
) -> Result<Plan, CadError> {
    let mut groups: BTreeMap<(String, String), (i64, Vec<i64>)> = BTreeMap::new();
    for i in instances {
        if i.part.kind() != Kind::Profile {
            continue;
        }
        let (Shape::Profile { max_cut, .. }, Some(cut)) = (&i.part.shape, i.cut_length()) else {
            continue;
        };
        let entry = groups
            .entry((i.part.name.clone(), i.part.material.clone()))
            .or_insert((max_cut.get(), Vec::new()));
        entry.1.push(cut.get());
    }
    let mut lines = Vec::with_capacity(groups.len());
    let mut total = 0usize;
    for ((part, material), (bar_nm, cuts)) in groups {
        let bars = pack(cuts, bar_nm, settings, &part)?;
        total += bars.len();
        if total > settings.max_bars {
            return Err(stock_error(format!(
                "more than the {} bars the settings allow",
                settings.max_bars
            )));
        }
        lines.push(StockLine {
            part,
            material,
            bar_nm,
            bars,
        });
    }
    Ok(Plan { lines })
}

/// The plan as comma separated values, the same text for the same plan: for each bar the profile,
/// the material, the number of the bar, the length of the bar, its pieces joined with `+`, the
/// offcut, and whether the offcut is kept; and a last line with the totals.
pub fn plan_csv(plan: &Plan, settings: &StockSettings) -> String {
    let mut out = String::from("part,material,bar,bar_mm,pieces_mm,offcut_mm,offcut_use\n");
    for line in &plan.lines {
        for (n, bar) in line.bars.iter().enumerate() {
            let pieces: Vec<String> = bar.pieces.iter().map(|p| mm_text(*p)).collect();
            let keep = if bar.offcut_nm > 0 && bar.offcut_nm >= settings.reusable_offcut.get() {
                "stock"
            } else if bar.offcut_nm > 0 {
                "scrap"
            } else {
                "none"
            };
            let _ = writeln!(
                out,
                "{},{},{},{},{},{},{}",
                line.part,
                line.material,
                n + 1,
                mm_text(line.bar_nm),
                pieces.join("+"),
                mm_text(bar.offcut_nm),
                keep
            );
        }
    }
    let pieces: usize = plan.lines.iter().map(StockLine::pieces).sum();
    let _ = writeln!(out, "total,,{},,{pieces},,", plan.bars());
    out
}

/// The plan as lines of text for a person: for each profile the bars, the pieces and how much of
/// what is bought is used.
pub fn plan_text(plan: &Plan) -> String {
    let mut out = String::new();
    for line in &plan.lines {
        let ppm = line.yield_ppm();
        let _ = writeln!(
            out,
            "{} ({}): {} bars of {} mm, {} pieces, {}.{} % used",
            line.part,
            line.material,
            line.bars.len(),
            mm_text(line.bar_nm),
            line.pieces(),
            ppm / 10_000,
            (ppm / 1_000) % 10
        );
        for (n, bar) in line.bars.iter().enumerate() {
            let pieces: Vec<String> = bar.pieces.iter().map(|p| mm_text(*p)).collect();
            let _ = writeln!(
                out,
                "  bar {}: {} (offcut {} mm)",
                n + 1,
                pieces.join(" + "),
                mm_text(bar.offcut_nm)
            );
        }
    }
    let _ = writeln!(out, "{} bars in all", plan.bars());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Orientation, Part};
    use crate::config::Settings;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn settings() -> StockSettings {
        StockSettings {
            kerf: mm(3),
            end_trim: mm(5),
            reusable_offcut: mm(100),
            max_bars: 100,
        }
    }

    fn bar(name: &str, bar_mm: i64) -> Part {
        Part {
            name: name.into(),
            material: "aluminium".into(),
            shape: Shape::Profile {
                width: mm(20),
                height: mm(20),
                wall: Length::ZERO,
                default_cut: mm(100),
                max_cut: mm(bar_mm),
            },
        }
    }

    fn cut(part: &Part, len_mm: i64) -> PartInstance {
        PartInstance {
            name: "p".into(),
            library: "lib".into(),
            part: part.clone(),
            at: [Length::ZERO; 3],
            orientation: Orientation::default(),
            cut: Some(mm(len_mm)),
        }
    }

    #[test]
    fn the_shipped_settings_are_the_ones_of_the_file() {
        let s = StockSettings::shipped();
        assert_eq!(s.kerf, mm(3));
        assert_eq!(s.end_trim, mm(5));
        assert_eq!(s.reusable_offcut, mm(100));
        assert_eq!(s.max_bars, 1000);
        assert_eq!(Settings::defaults().stock, s);
    }

    #[test]
    fn every_value_of_the_settings_is_checked_on_both_sides() {
        let with = |key: &str, value: &str| {
            Settings::with_user_config(&format!(
                "[stock]
{key} = {value}
"
            ))
        };
        assert!(with("kerf", "1000 mm").is_ok());
        assert!(with("kerf", "1001 mm").is_err());
        assert!(with("kerf", "0 mm").is_ok());
        assert!(with("end_trim", "1001 mm").is_err());
        assert!(with("reusable_offcut", "1001 mm").is_err());
        assert!(with("max_bars", "1000000").is_ok());
        assert!(with("max_bars", "1000001").is_err());
        assert!(with("max_bars", "0").is_err());
    }

    #[test]
    fn the_pieces_go_longest_first_into_the_bar_with_the_least_room_left() {
        // a bar of 1000 mm with 5 mm cut off each end leaves 990 mm; kerf 3 mm
        let p = bar("b", 1000);
        let parts = [400, 300, 300, 200].map(|l| cut(&p, l));
        let plan = plan(parts.iter(), &settings()).unwrap();
        assert_eq!(plan.lines.len(), 1);
        let line = &plan.lines[0];
        // 400 + 300 + 200 + 2 kerfs = 906 mm, then the second 300 does not fit (906 + 3 + 300 > 990)
        // so it goes to a second bar
        assert_eq!(line.bars.len(), 2);
        assert_eq!(
            line.bars[0].pieces,
            vec![400_000_000, 300_000_000, 200_000_000]
        );
        assert_eq!(line.bars[1].pieces, vec![300_000_000]);
        // the offcut of the first: 990 - (900 + 2 * 3) - 3 = 81 mm
        assert_eq!(line.bars[0].offcut_nm, 81_000_000);
        // the offcut of the second: 990 - 300 - 3 = 687 mm
        assert_eq!(line.bars[1].offcut_nm, 687_000_000);
        assert_eq!(line.pieces(), 4);
        assert_eq!(line.used_nm(), 1_200_000_000);
        assert_eq!(line.bought_nm(), 2_000_000_000);
        assert_eq!(line.yield_ppm(), 600_000);
        assert_eq!(plan.bars(), 2);
    }

    #[test]
    fn a_piece_that_fills_the_bar_exactly_leaves_no_offcut() {
        let p = bar("b", 1000);
        let parts = [cut(&p, 990)];
        let plan = plan(parts.iter(), &settings()).unwrap();
        assert_eq!(plan.lines[0].bars[0].offcut_nm, 0);
        // a piece of 987 mm leaves 3 mm, all of it the kerf of the cut that frees it
        let parts = [cut(&p, 987)];
        let plan = super::plan(parts.iter(), &settings()).unwrap();
        assert_eq!(plan.lines[0].bars[0].offcut_nm, 0);
        let parts = [cut(&p, 986)];
        let plan = super::plan(parts.iter(), &settings()).unwrap();
        assert_eq!(plan.lines[0].bars[0].offcut_nm, 1_000_000);
    }

    #[test]
    fn a_piece_longer_than_the_room_of_a_bar_is_refused_with_the_code_of_the_part() {
        let p = bar("b", 1000);
        let parts = [cut(&p, 991)];
        let err = plan(parts.iter(), &settings()).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-045");
        assert!(err.to_string().contains("991"));
        // trimmed ends that take the whole bar
        let s = StockSettings {
            end_trim: mm(500),
            ..settings()
        };
        assert!(plan(parts.iter(), &s).is_err());
    }

    #[test]
    fn the_number_of_bars_is_limited_by_the_settings() {
        let p = bar("b", 1000);
        let parts: Vec<PartInstance> = (0..3).map(|_| cut(&p, 900)).collect();
        let s = StockSettings {
            max_bars: 2,
            ..settings()
        };
        assert!(plan(parts.iter(), &s).is_err());
        let s = StockSettings {
            max_bars: 3,
            ..settings()
        };
        assert_eq!(plan(parts.iter(), &s).unwrap().bars(), 3);
    }

    #[test]
    fn the_profiles_are_planned_apart_and_the_other_parts_are_left_out() {
        let a = bar("a", 1000);
        let b = bar("b", 2000);
        let screw = Part {
            name: "s".into(),
            material: "steel".into(),
            shape: Shape::Screw {
                metric: "M3".into(),
                diameter: mm(3),
                length: mm(10),
                head: crate::catalog::Head::Cylinder,
                head_diameter: mm(5),
                head_height: mm(3),
            },
        };
        let screw_instance = PartInstance {
            name: "s".into(),
            library: "lib".into(),
            part: screw,
            at: [Length::ZERO; 3],
            orientation: Orientation::default(),
            cut: None,
        };
        let parts = [cut(&b, 1500), cut(&a, 600), screw_instance, cut(&a, 600)];
        let plan = plan(parts.iter(), &settings()).unwrap();
        assert_eq!(plan.lines.len(), 2);
        assert_eq!(plan.lines[0].part, "a");
        assert_eq!(plan.lines[0].bars.len(), 2);
        assert_eq!(plan.lines[1].part, "b");
        assert_eq!(plan.lines[1].bar_nm, 2_000_000_000);
        assert_eq!(plan.lines[1].bars.len(), 1);
    }

    #[test]
    fn the_same_model_gives_the_same_plan_whatever_the_order_of_the_parts() {
        let p = bar("b", 1000);
        let a: Vec<PartInstance> = [100, 250, 400, 250, 100, 330]
            .iter()
            .map(|l| cut(&p, *l))
            .collect();
        let b: Vec<PartInstance> = a.iter().rev().cloned().collect();
        assert_eq!(
            plan(a.iter(), &settings()).unwrap(),
            plan(b.iter(), &settings()).unwrap()
        );
    }

    #[test]
    fn the_csv_has_a_line_for_each_bar_and_the_totals() {
        let p = bar("b", 1000);
        let parts = [400, 300, 300, 200].map(|l| cut(&p, l));
        let s = settings();
        let plan = plan(parts.iter(), &s).unwrap();
        let csv = plan_csv(&plan, &s);
        assert_eq!(
            csv,
            "part,material,bar,bar_mm,pieces_mm,offcut_mm,offcut_use\n\
             b,aluminium,1,1000,400+300+200,81,scrap\n\
             b,aluminium,2,1000,300,687,stock\n\
             total,,2,,4,,\n"
        );
    }

    #[test]
    fn the_text_says_how_much_of_the_bars_is_used() {
        let p = bar("b", 1000);
        let parts = [400, 300, 300, 200].map(|l| cut(&p, l));
        let plan = plan(parts.iter(), &settings()).unwrap();
        let text = plan_text(&plan);
        assert!(text.starts_with("b (aluminium): 2 bars of 1000 mm, 4 pieces, 60.0 % used\n"));
        assert!(text.contains("  bar 1: 400 + 300 + 200 (offcut 81 mm)\n"));
        assert!(text.ends_with("2 bars in all\n"));
    }
}
