// SPDX-License-Identifier: GPL-3.0-or-later
//! The table of where the parts are, the size of the assembly and its exploded view.
//!
//! The assembly places parts with mates; a person who builds it wants to read where each part
//! ends up. [`table`] lists, for each placed part, the smallest corner of its bounding box, its
//! size along the three axes and its turn, and the box that holds the whole assembly. [`explode`]
//! moves every part away from the middle of the assembly by a percentage of its own distance from
//! it, which pulls the parts apart along the lines from the middle through their own middles
//! without changing their turn; it is a drawing aid, and the mates of the model are not touched.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::catalog::PartInstance;
use crate::config::Config;
use crate::enclosure::Id;
use crate::scad::mm_text;
use crate::units::div_round_half_away;
use crate::{CadError, Length};

/// The most that the exploded view can be set to pull, in percent.
pub const EXPLODE_CAP_PERCENT: u32 = 10_000;

/// The settings of the table and of the exploded view (`[placement]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacementSettings {
    /// The most that a request for an exploded view may pull, in percent of the distance of the
    /// middle of each part from the middle of the assembly.
    pub max_explode_percent: u32,
}

impl PlacementSettings {
    /// The settings of the shipped file.
    pub fn shipped() -> PlacementSettings {
        crate::config::Settings::defaults().placement
    }

    /// Reads and checks the keys of `[placement]`.
    pub fn from_config(c: &Config) -> Result<PlacementSettings, CadError> {
        let v = c.number("placement.max_explode_percent")?;
        if v == 0 || v > EXPLODE_CAP_PERCENT {
            return Err(CadError::Config(format!(
                "placement.max_explode_percent must be from 1 to {EXPLODE_CAP_PERCENT}"
            )));
        }
        Ok(PlacementSettings {
            max_explode_percent: v,
        })
    }
}

/// A line of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The name of the part.
    pub name: String,
    /// The part of the library it is a copy of.
    pub part: String,
    /// The smallest corner of the bounding box, in nanometres.
    pub min: [i64; 3],
    /// The size along `x`, `y` and `z`, in nanometres.
    pub size: [i64; 3],
    /// The turn, as the word of the files (`+x/0`).
    pub turn: String,
}

/// The table of the parts and the box that holds them all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Table {
    /// One row for each part, in order of name and then of id.
    pub rows: Vec<Row>,
    /// The smallest corner of the box of the whole assembly, when there are parts.
    pub min: Option<[i64; 3]>,
    /// The size of the box of the whole assembly (zeros without parts).
    pub size: [i64; 3],
}

/// The table of some placed parts.
pub fn table(parts: &BTreeMap<Id, PartInstance>) -> Result<Table, CadError> {
    let mut rows = Vec::with_capacity(parts.len());
    let mut lo = [i64::MAX; 3];
    let mut hi = [i64::MIN; 3];
    for (id, p) in parts {
        let (min, max) = p.bounds()?;
        for k in 0..3 {
            lo[k] = lo[k].min(min[k]);
            hi[k] = hi[k].max(max[k]);
        }
        rows.push((
            p.name.clone(),
            *id,
            Row {
                name: p.name.clone(),
                part: p.part.name.clone(),
                min,
                size: [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
                turn: p.orientation.word(),
            },
        ));
    }
    rows.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
    let rows: Vec<Row> = rows.into_iter().map(|(_, _, r)| r).collect();
    if rows.is_empty() {
        return Ok(Table::default());
    }
    Ok(Table {
        rows,
        min: Some(lo),
        size: [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]],
    })
}

/// The table as comma separated values: for each part its name, the part, the corner and the size
/// in millimetres and the turn; and a last line `total` with the box of the assembly.
pub fn table_csv(t: &Table) -> String {
    let mut out = String::from("name,part,x_mm,y_mm,z_mm,size_x_mm,size_y_mm,size_z_mm,turn\n");
    for r in &t.rows {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{}",
            r.name,
            r.part,
            mm_text(r.min[0]),
            mm_text(r.min[1]),
            mm_text(r.min[2]),
            mm_text(r.size[0]),
            mm_text(r.size[1]),
            mm_text(r.size[2]),
            r.turn
        );
    }
    let min = t.min.unwrap_or([0; 3]);
    let _ = writeln!(
        out,
        "total,{} parts,{},{},{},{},{},{},",
        t.rows.len(),
        mm_text(min[0]),
        mm_text(min[1]),
        mm_text(min[2]),
        mm_text(t.size[0]),
        mm_text(t.size[1]),
        mm_text(t.size[2])
    );
    out
}

/// The parts pulled apart: the middle of each part moves away from the middle of the assembly by
/// `percent` of its distance from it, along each axis, rounded to the nanometre (half away from
/// zero). Refused with `PHOENIX-CAD-045` when `percent` is above the settings or a part would
/// leave the range of lengths. The middles are taken from the bounding boxes, doubled so that a
/// half nanometre is not lost.
pub fn explode(
    parts: &BTreeMap<Id, PartInstance>,
    percent: u32,
    settings: &PlacementSettings,
) -> Result<BTreeMap<Id, PartInstance>, CadError> {
    if percent > settings.max_explode_percent {
        return Err(CadError::Part(format!(
            "an exploded view of {percent} % is more than the {} % the settings allow",
            settings.max_explode_percent
        )));
    }
    let t = table(parts)?;
    let Some(lo) = t.min else {
        return Ok(parts.clone());
    };
    // twice the middle of the assembly, per axis
    let centre2: [i128; 3] = [
        i128::from(2 * lo[0] + t.size[0]),
        i128::from(2 * lo[1] + t.size[1]),
        i128::from(2 * lo[2] + t.size[2]),
    ];
    let mut out = parts.clone();
    for p in out.values_mut() {
        let (min, max) = p.bounds()?;
        for k in 0..3 {
            let middle2 = i128::from(min[k]) + i128::from(max[k]);
            // (middle - centre) * percent / 100, with the doubled values: / 200
            let shift = div_round_half_away((middle2 - centre2[k]) * i128::from(percent), 200)?;
            let moved = i128::from(p.at[k].get()) + shift;
            let nm = i64::try_from(moved).ok().and_then(|v| Length::new(v).ok());
            p.at[k] = nm.ok_or_else(|| {
                CadError::Part(format!(
                    "{} would leave the range of lengths in the exploded view",
                    p.name
                ))
            })?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Orientation, Part, Shape};
    use crate::config::Settings;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn id(n: u64) -> Id {
        Id::from_parts(1_700_000_000_000, 1, n).unwrap()
    }

    fn bar(name: &str, at: [i64; 3], cut: i64) -> PartInstance {
        PartInstance {
            name: name.into(),
            library: "lib".into(),
            part: Part {
                name: "bar-20x30".into(),
                material: "steel".into(),
                shape: Shape::Profile {
                    width: mm(20),
                    height: mm(30),
                    wall: Length::ZERO,
                    default_cut: mm(100),
                    max_cut: mm(1000),
                },
            },
            at: [mm(at[0]), mm(at[1]), mm(at[2])],
            orientation: Orientation::default(),
            cut: Some(mm(cut)),
        }
    }

    fn set(list: Vec<(u64, PartInstance)>) -> BTreeMap<Id, PartInstance> {
        list.into_iter().map(|(n, p)| (id(n), p)).collect()
    }

    #[test]
    fn the_shipped_setting_and_its_limits() {
        assert_eq!(PlacementSettings::shipped().max_explode_percent, 500);
        let with = |t: &str| Settings::with_user_config(t);
        assert!(with("[placement]\nmax_explode_percent = 1\n").is_ok());
        assert!(with("[placement]\nmax_explode_percent = 10000\n").is_ok());
        assert!(with("[placement]\nmax_explode_percent = 10001\n").is_err());
        assert!(with("[placement]\nmax_explode_percent = 0\n").is_err());
    }

    #[test]
    fn the_table_lists_each_part_and_the_box_of_the_assembly() {
        // a bar along z (the profile runs along the local z of its frame, here the default turn)
        let parts = set(vec![
            (2, bar("b", [100, 0, 0], 50)),
            (1, bar("a", [0, 10, 5], 100)),
        ]);
        let t = table(&parts).unwrap();
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].name, "a");
        assert_eq!(t.rows[0].min, [0, 10_000_000, 5_000_000]);
        assert_eq!(t.rows[0].size, [20_000_000, 30_000_000, 100_000_000]);
        assert_eq!(t.rows[1].name, "b");
        // the box: x from 0 to 120, y from 0 to 40, z from 0 to 105
        assert_eq!(t.min, Some([0, 0, 0]));
        assert_eq!(t.size, [120_000_000, 40_000_000, 105_000_000]);
    }

    #[test]
    fn the_csv_has_a_line_for_each_part_and_the_total() {
        let parts = set(vec![
            (1, bar("a", [0, 10, 5], 100)),
            (2, bar("b", [100, 0, 0], 50)),
        ]);
        let csv = table_csv(&table(&parts).unwrap());
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(
            lines[0],
            "name,part,x_mm,y_mm,z_mm,size_x_mm,size_y_mm,size_z_mm,turn"
        );
        assert_eq!(lines[1], "a,bar-20x30,0,10,5,20,30,100,+z/0");
        assert_eq!(lines[3], "total,2 parts,0,0,0,120,40,105,");
        // no parts: only the header and a total of nothing
        let empty = table_csv(&table(&BTreeMap::new()).unwrap());
        assert!(empty.ends_with("total,0 parts,0,0,0,0,0,0,\n"));
    }

    #[test]
    fn parts_are_ordered_by_name_then_by_id() {
        let parts = set(vec![
            (3, bar("same", [0, 0, 0], 10)),
            (1, bar("same", [50, 0, 0], 10)),
            (2, bar("early", [90, 0, 0], 10)),
        ]);
        let t = table(&parts).unwrap();
        let order: Vec<(&str, i64)> = t.rows.iter().map(|r| (r.name.as_str(), r.min[0])).collect();
        assert_eq!(
            order,
            [("early", 90_000_000), ("same", 50_000_000), ("same", 0)]
        );
    }

    #[test]
    fn the_exploded_view_pulls_each_part_away_from_the_middle() {
        // two bars of 20 mm along x, 100 mm apart: the middle of the assembly is at x = 60 mm
        let parts = set(vec![
            (1, bar("a", [0, 0, 0], 10)),
            (2, bar("b", [100, 0, 0], 10)),
        ]);
        let s = PlacementSettings::shipped();
        // 50 %: the middle of a is at 10 mm, 50 mm from the middle of the assembly, and moves 25 mm
        let moved = explode(&parts, 50, &s).unwrap();
        assert_eq!(moved[&id(1)].at[0], mm(-25));
        assert_eq!(moved[&id(2)].at[0], mm(125));
        // y and z: both bars have the same middle there, so nothing moves
        assert_eq!(moved[&id(1)].at[1], Length::ZERO);
        assert_eq!(moved[&id(2)].at[2], Length::ZERO);
        // 0 % puts everything back and the originals are not changed
        assert_eq!(explode(&parts, 0, &s).unwrap(), parts);
        assert_eq!(parts[&id(1)].at[0], Length::ZERO);
        // the turn and the cut are kept
        assert_eq!(moved[&id(1)].orientation, parts[&id(1)].orientation);
        assert_eq!(moved[&id(1)].cut, parts[&id(1)].cut);
    }

    #[test]
    fn the_exploded_view_is_refused_above_the_setting_or_outside_the_range() {
        let parts = set(vec![
            (1, bar("a", [0, 0, 0], 10)),
            (2, bar("b", [100, 0, 0], 10)),
        ]);
        let s = PlacementSettings {
            max_explode_percent: 100,
        };
        assert!(explode(&parts, 100, &s).is_ok());
        let e = explode(&parts, 101, &s).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-045");
        let huge = PlacementSettings {
            max_explode_percent: EXPLODE_CAP_PERCENT,
        };
        assert!(explode(&parts, 10_000, &huge).is_ok());
        let far = set(vec![
            (1, bar("a", [0, 0, 0], 10)),
            (2, bar("b", [50_000, 0, 0], 10)),
        ]);
        // 50 m apart and pulled by 10000 %: far beyond 1100 m
        assert!(explode(&far, 10_000, &huge).is_err());
    }

    #[test]
    fn an_empty_assembly_has_nothing_to_explode() {
        let none = BTreeMap::new();
        assert!(explode(&none, 100, &PlacementSettings::shipped())
            .unwrap()
            .is_empty());
    }
}
