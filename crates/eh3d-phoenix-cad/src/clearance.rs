// SPDX-License-Identifier: GPL-3.0-or-later
//! The check of the clearance between the placed parts: which pairs are closer than a minimum but
//! do not touch.
//!
//! The interference check finds the parts that share volume. A design also needs room: a tool to
//! reach a screw, a cable to pass, a part that heats up and a part that must not be near it. This
//! check measures, for every pair of parts, the distance between their bounding boxes (zero when
//! the boxes touch or overlap) and lists the pairs that are closer than `clearance.min_gap`
//! without touching: a pair that touches is placed that way on purpose (a stack, a gap of zero)
//! and a pair that overlaps is the business of the interference check.
//!
//! The distance between two boxes is the length of the shortest segment between them: along each
//! axis the gap is zero when the ranges overlap, and the distance is the root of the sum of the
//! squares of the three gaps. It is compared as squares, in integers, so nothing is rounded.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::catalog::PartInstance;
use crate::config::Config;
use crate::enclosure::Id;
use crate::scad::mm_text;
use crate::{CadError, Length};

/// The most that the minimum gap can be set to, in nanometres (a metre).
pub const GAP_CAP_NM: i64 = 1_000_000_000;

/// The most pairs that the report can be set to list.
pub const PAIRS_CAP: u32 = 100_000;

/// The settings of the check (`[clearance]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClearanceSettings {
    /// The least distance between two parts that do not touch; zero turns the check off.
    pub min_gap: Length,
    /// The most pairs a report lists (the rest are counted).
    pub max_pairs: usize,
}

impl ClearanceSettings {
    /// The settings of the shipped file.
    pub fn shipped() -> ClearanceSettings {
        crate::config::Settings::defaults().clearance
    }

    /// Reads and checks the keys of `[clearance]`.
    pub fn from_config(c: &Config) -> Result<ClearanceSettings, CadError> {
        let min_gap = c.length("clearance.min_gap")?;
        if min_gap.get() < 0 || min_gap.get() > GAP_CAP_NM {
            return Err(CadError::Config(format!(
                "clearance.min_gap must be from 0 to {} mm",
                GAP_CAP_NM / 1_000_000
            )));
        }
        let pairs = c.number("clearance.max_pairs")?;
        if pairs == 0 || pairs > PAIRS_CAP {
            return Err(CadError::Config(format!(
                "clearance.max_pairs must be from 1 to {PAIRS_CAP}"
            )));
        }
        Ok(ClearanceSettings {
            min_gap,
            max_pairs: usize::try_from(pairs)
                .map_err(|_| CadError::Config("clearance.max_pairs is too large".into()))?,
        })
    }
}

/// A pair of parts closer than the minimum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Narrow {
    /// The part with the smaller id.
    pub a: Id,
    /// The part with the larger id.
    pub b: Id,
    /// The square of the distance between the bounding boxes, in square nanometres.
    pub distance_sq: i128,
}

impl Narrow {
    /// The distance in nanometres, rounded down.
    pub fn distance_nm(&self) -> i64 {
        isqrt(self.distance_sq)
    }
}

/// The result of the check.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// The pairs closer than the minimum, the closest first.
    pub pairs: Vec<Narrow>,
}

/// The integer square root, rounded down.
pub fn isqrt(n: i128) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = (n as f64).sqrt() as i128;
    while x * x > n {
        x -= 1;
    }
    while (x + 1) * (x + 1) <= n {
        x += 1;
    }
    i64::try_from(x).unwrap_or(i64::MAX)
}

/// The square of the distance between two boxes, in square nanometres.
pub fn box_distance_sq(a: &([i64; 3], [i64; 3]), b: &([i64; 3], [i64; 3])) -> i128 {
    (0..3)
        .map(|k| {
            let gap = (b.0[k] - a.1[k]).max(a.0[k] - b.1[k]).max(0);
            i128::from(gap) * i128::from(gap)
        })
        .sum()
}

/// Finds the pairs of parts that do not touch and are closer than `settings.min_gap`. The parts
/// should be the ones the mates give. With a minimum of zero nothing is checked.
pub fn find(
    parts: &BTreeMap<Id, PartInstance>,
    settings: &ClearanceSettings,
) -> Result<Report, CadError> {
    let min = i128::from(settings.min_gap.get());
    if min == 0 {
        return Ok(Report::default());
    }
    let mut boxes = Vec::with_capacity(parts.len());
    for (id, part) in parts {
        boxes.push((*id, part.bounds()?));
    }
    boxes.sort_by_key(|(id, (lo, _))| (lo[0], *id));
    let mut pairs = Vec::new();
    for (i, (ia, ba)) in boxes.iter().enumerate() {
        for (ib, bb) in &boxes[i + 1..] {
            // sorted by the smallest x: from here on no box is within reach along x
            if i128::from(bb.0[0] - ba.1[0]) >= min {
                break;
            }
            let d = box_distance_sq(ba, bb);
            if d > 0 && d < min * min {
                let (a, b) = if ia < ib { (*ia, *ib) } else { (*ib, *ia) };
                pairs.push(Narrow {
                    a,
                    b,
                    distance_sq: d,
                });
            }
        }
    }
    pairs.sort_by_key(|p| (p.distance_sq, p.a, p.b));
    Ok(Report { pairs })
}

fn name_of(parts: &BTreeMap<Id, PartInstance>, id: &Id) -> String {
    parts
        .get(id)
        .map_or_else(|| id.to_string(), |p| p.name.clone())
}

/// The report as comma separated values: the two parts and the distance in millimetres, the
/// closest pair first; at most `settings.max_pairs` lines and a last line with the number of pairs.
pub fn report_csv(
    report: &Report,
    parts: &BTreeMap<Id, PartInstance>,
    settings: &ClearanceSettings,
) -> String {
    let mut out = String::from("part_a,part_b,distance_mm\n");
    for p in report.pairs.iter().take(settings.max_pairs) {
        let _ = writeln!(
            out,
            "{},{},{}",
            name_of(parts, &p.a),
            name_of(parts, &p.b),
            mm_text(p.distance_nm())
        );
    }
    let _ = writeln!(out, "total,{},", report.pairs.len());
    out
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

    fn settings(gap_mm: i64) -> ClearanceSettings {
        ClearanceSettings {
            min_gap: mm(gap_mm),
            max_pairs: 100,
        }
    }

    /// A solid bar of 10 x 10 mm cut to 10 mm: a cube of 10 mm with its corner at `at`.
    fn cube(name: &str, at: [i64; 3]) -> PartInstance {
        PartInstance {
            name: name.into(),
            library: "lib".into(),
            part: Part {
                name: "cube".into(),
                material: "steel".into(),
                shape: Shape::Profile {
                    width: mm(10),
                    height: mm(10),
                    wall: Length::ZERO,
                    default_cut: mm(10),
                    max_cut: mm(100),
                },
            },
            at: [mm(at[0]), mm(at[1]), mm(at[2])],
            orientation: Orientation::default(),
            cut: Some(mm(10)),
        }
    }

    fn set(list: Vec<(u64, PartInstance)>) -> BTreeMap<Id, PartInstance> {
        list.into_iter().map(|(n, p)| (id(n), p)).collect()
    }

    #[test]
    fn the_shipped_settings_and_their_limits() {
        let s = ClearanceSettings::shipped();
        assert_eq!(s.min_gap, mm(2));
        assert_eq!(s.max_pairs, 100);
        assert_eq!(Settings::defaults().clearance, s);
        let with = |t: &str| Settings::with_user_config(t);
        assert!(with("[clearance]\nmin_gap = 0 mm\n").is_ok());
        assert!(with("[clearance]\nmin_gap = 1000 mm\n").is_ok());
        assert!(with("[clearance]\nmin_gap = 1001 mm\n").is_err());
        assert!(with("[clearance]\nmin_gap = -1 mm\n").is_err());
        assert!(with("[clearance]\nmax_pairs = 0\n").is_err());
        assert!(with("[clearance]\nmax_pairs = 100000\n").is_ok());
        assert!(with("[clearance]\nmax_pairs = 100001\n").is_err());
    }

    #[test]
    fn the_integer_root_is_exact() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(-5), 0);
        assert_eq!(isqrt(15), 3);
        assert_eq!(isqrt(16), 4);
        assert_eq!(isqrt(1_000_000_000_000_000_000), 1_000_000_000);
        assert_eq!(isqrt(999_999_999_999_999_999), 999_999_999);
    }

    #[test]
    fn the_distance_of_two_boxes_is_the_shortest_segment_between_them() {
        let a = ([0, 0, 0], [10, 10, 10]);
        // 3 along x only
        assert_eq!(box_distance_sq(&a, &([13, 0, 0], [20, 10, 10])), 9);
        // 3 along x and 4 along y: the distance is 5
        assert_eq!(box_distance_sq(&a, &([13, 14, 0], [20, 20, 10])), 25);
        // touching and overlapping are zero
        assert_eq!(box_distance_sq(&a, &([10, 0, 0], [20, 10, 10])), 0);
        assert_eq!(box_distance_sq(&a, &([5, 5, 5], [20, 20, 20])), 0);
        // on the other side
        assert_eq!(box_distance_sq(&([13, 0, 0], [20, 10, 10]), &a), 9);
    }

    #[test]
    fn only_the_pairs_that_do_not_touch_and_are_closer_than_the_minimum_are_listed() {
        let parts = set(vec![
            (1, cube("a", [0, 0, 0])),
            (2, cube("touching", [10, 0, 0])),
            (3, cube("one mm away", [21, 0, 0])),
            (4, cube("p", [200, 0, 0])),
            (5, cube("far", [100, 0, 0])),
            (6, cube("diagonal", [0, 13, 14])),
            (7, cube("q", [212, 0, 0])),
        ]);
        let r = find(&parts, &settings(2)).unwrap();
        // "one mm away" is 1 mm from "touching" (which ends at 20); "p" and "q" are 2 mm apart,
        // which is not closer than 2 mm; "diagonal" is 3 and 4 mm from "a": 5 mm, farther than 2 mm
        let names: Vec<(String, String, i64)> = r
            .pairs
            .iter()
            .map(|p| {
                (
                    name_of(&parts, &p.a),
                    name_of(&parts, &p.b),
                    p.distance_nm(),
                )
            })
            .collect();
        assert_eq!(
            names,
            vec![("touching".to_owned(), "one mm away".to_owned(), 1_000_000)]
        );
        // with 6 mm the diagonal pair and the 2 mm pair join in, closest first
        let r = find(&parts, &settings(6)).unwrap();
        let d: Vec<i64> = r.pairs.iter().map(Narrow::distance_nm).collect();
        assert!(d.windows(2).all(|w| w[0] <= w[1]), "{d:?}");
        assert!(d.contains(&5_000_000));
        assert!(d.contains(&2_000_000));
        assert!(!d.contains(&0));
    }

    #[test]
    fn a_minimum_of_zero_checks_nothing() {
        let parts = set(vec![(1, cube("a", [0, 0, 0])), (2, cube("b", [11, 0, 0]))]);
        assert!(find(&parts, &settings(0)).unwrap().pairs.is_empty());
        assert_eq!(find(&parts, &settings(2)).unwrap().pairs.len(), 1);
    }

    #[test]
    fn the_csv_lists_the_closest_pairs_up_to_the_limit_and_counts_all() {
        let parts = set(vec![
            (1, cube("a", [0, 0, 0])),
            (2, cube("b", [11, 0, 0])),
            (3, cube("c", [23, 0, 0])),
        ]);
        let mut s = settings(5);
        let r = find(&parts, &s).unwrap();
        assert_eq!(
            report_csv(&r, &parts, &s),
            "part_a,part_b,distance_mm\na,b,1\nb,c,2\ntotal,2,\n"
        );
        s.max_pairs = 1;
        assert_eq!(
            report_csv(&r, &parts, &s),
            "part_a,part_b,distance_mm\na,b,1\ntotal,2,\n"
        );
    }
}
