// SPDX-License-Identifier: GPL-3.0-or-later
//! Limits and fits of the system of tolerances for sizes of ISO 286-1 (`H7/g6`), worked out from a
//! table of data.
//!
//! The table is the file `phoenix-cad-iso286.default.cfg` (or another one given to the command):
//! the width of the zone of tolerance of the grades IT5 to IT12 and the fundamental deviation of
//! the letters d, e, f, g, h, k, m, n and p (and, for the holes, D, E, F, G, H) for each range of
//! nominal sizes up to 500 mm. The shipped table says `verified = false` and every answer says so
//! until a checked table replaces it. Deviations are kept in nanometres so that the sizes are whole
//! numbers; the symmetric classes `js` and `JS` are half the zone to each side.

use crate::lang::{fill, Lang, Msg};

/// The shipped table.
const SHIPPED: &str = include_str!("../phoenix-cad-iso286.default.cfg");

/// The grades the table holds, in order of its columns.
pub const GRADES: [u8; 8] = [5, 6, 7, 8, 9, 10, 11, 12];

/// The largest nominal size of the table, in millimetres.
pub const MAX_SIZE_MM: f64 = 500.0;

/// The table of widths and deviations.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// Whether the numbers were checked against the standard.
    pub verified: bool,
    /// For each range: over, up to and including (mm) and the widths of the grades (µm).
    ranges: Vec<(f64, f64, [i64; 8])>,
    /// For each letter, the deviation of each range (µm).
    deviations: Vec<(char, Vec<i64>)>,
}

impl Table {
    /// The shipped table.
    pub fn shipped() -> Table {
        // the shipped file is read by a test, so it always parses
        Table::parse(SHIPPED).unwrap_or(Table {
            verified: false,
            ranges: Vec::new(),
            deviations: Vec::new(),
        })
    }

    /// Reads a table, refusing the line that is wrong by its number.
    pub fn parse(text: &str) -> Result<Table, String> {
        let mut verified = None;
        let mut ranges: Vec<(f64, f64, [i64; 8])> = Vec::new();
        let mut deviations: Vec<(char, Vec<i64>)> = Vec::new();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let at = n + 1;
            let words: Vec<&str> = line.split_whitespace().collect();
            let number = |w: &str| -> Result<f64, String> {
                w.parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| format!("line {at}: {w} is not a number"))
            };
            let whole = |w: &str| -> Result<i64, String> {
                w.parse::<i64>()
                    .ok()
                    .filter(|v| v.abs() <= 1_000_000)
                    .ok_or_else(|| format!("line {at}: {w} is not a whole number of micrometres"))
            };
            match words.as_slice() {
                ["verified", "=", value] => {
                    verified = Some(match *value {
                        "true" => true,
                        "false" => false,
                        other => {
                            return Err(format!(
                                "line {at}: verified is true or false, not {other}"
                            ))
                        }
                    });
                }
                ["it", over, up_to, widths @ ..] => {
                    if widths.len() != GRADES.len() {
                        return Err(format!(
                            "line {at}: a range needs {} widths, one for each grade",
                            GRADES.len()
                        ));
                    }
                    let (over, up_to) = (number(over)?, number(up_to)?);
                    if over < 0.0 || up_to <= over || up_to > MAX_SIZE_MM {
                        return Err(format!(
                            "line {at}: the range must grow and end by {MAX_SIZE_MM} mm"
                        ));
                    }
                    if let Some((_, previous_end, _)) = ranges.last() {
                        if over != *previous_end {
                            return Err(format!(
                                "line {at}: the range must start where the one before ends"
                            ));
                        }
                    } else if over != 0.0 {
                        return Err(format!("line {at}: the first range starts at 0"));
                    }
                    let mut w = [0i64; 8];
                    for (slot, word) in w.iter_mut().zip(widths) {
                        *slot = whole(word)?;
                        if *slot <= 0 {
                            return Err(format!("line {at}: a width is positive"));
                        }
                    }
                    ranges.push((over, up_to, w));
                }
                ["dev", letter, values @ ..] => {
                    let mut chars = letter.chars();
                    let (Some(c), None) = (chars.next(), chars.next()) else {
                        return Err(format!("line {at}: a letter is one character"));
                    };
                    if !"defghkmnp".contains(c) {
                        return Err(format!(
                            "line {at}: the letter {c} is not one of d e f g h k m n p"
                        ));
                    }
                    if deviations.iter().any(|(l, _)| *l == c) {
                        return Err(format!("line {at}: the letter {c} is given twice"));
                    }
                    let values = values
                        .iter()
                        .map(|w| whole(w))
                        .collect::<Result<Vec<_>, _>>()?;
                    deviations.push((c, values));
                }
                _ => return Err(format!("line {at}: {line}: this line is not understood")),
            }
        }
        let verified = verified.ok_or("the table does not say whether it is verified")?;
        if ranges.is_empty() {
            return Err("the table has no range of sizes".to_owned());
        }
        for (c, v) in &deviations {
            if v.len() != ranges.len() {
                return Err(format!(
                    "the letter {c} has {} deviations and there are {} ranges",
                    v.len(),
                    ranges.len()
                ));
            }
        }
        Ok(Table {
            verified,
            ranges,
            deviations,
        })
    }

    fn range_of(&self, size_mm: f64) -> Result<usize, String> {
        if !size_mm.is_finite() || size_mm <= 0.0 || size_mm > MAX_SIZE_MM {
            return Err(format!(
                "the size must be over 0 and at most {MAX_SIZE_MM} mm"
            ));
        }
        self.ranges
            .iter()
            .position(|(over, up_to, _)| size_mm > *over && size_mm <= *up_to)
            .ok_or_else(|| format!("the table has no range for {size_mm} mm"))
    }
}

/// A class of tolerance: its letters and its grade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    /// The letters as written (`H`, `g`, `JS`, `js`).
    pub letters: String,
    /// The grade (5 to 12).
    pub grade: u8,
}

impl Class {
    /// A class written like `H7`, `g6` or `JS7`.
    pub fn parse(text: &str) -> Result<Class, String> {
        let letters: String = text
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        let digits = &text[letters.len()..];
        let grade: u8 = digits
            .parse()
            .map_err(|_| format!("{text}: a class is letters and a grade, as in H7 or g6"))?;
        if !GRADES.contains(&grade) {
            return Err(format!("{text}: the grades of the table are IT5 to IT12"));
        }
        if !matches!(
            letters.as_str(),
            "D" | "E"
                | "F"
                | "G"
                | "H"
                | "JS"
                | "d"
                | "e"
                | "f"
                | "g"
                | "h"
                | "js"
                | "k"
                | "m"
                | "n"
                | "p"
        ) {
            return Err(format!(
                "{text}: the letters {letters} are not in the table"
            ));
        }
        Ok(Class { letters, grade })
    }

    /// Whether the class is for a hole (capital letters) and not for a shaft.
    pub fn is_hole(&self) -> bool {
        self.letters.chars().all(|c| c.is_ascii_uppercase())
    }
}

/// The limits of a size in nanometres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The lower deviation from the nominal size.
    pub lower_dev_nm: i64,
    /// The upper deviation from the nominal size.
    pub upper_dev_nm: i64,
    /// The smallest size allowed.
    pub min_nm: i64,
    /// The largest size allowed.
    pub max_nm: i64,
}

/// The limits of a class at a nominal size.
pub fn limits(table: &Table, size_mm: f64, class: &Class) -> Result<Limits, String> {
    let range = table.range_of(size_mm)?;
    let width_um =
        table.ranges[range].2[GRADES.iter().position(|g| *g == class.grade).unwrap_or(0)];
    let width = width_um * 1000;
    let dev = |letter: char| -> Result<i64, String> {
        table
            .deviations
            .iter()
            .find(|(l, _)| *l == letter)
            .map(|(_, v)| v[range] * 1000)
            .ok_or_else(|| format!("the table has no deviation for the letter {letter}"))
    };
    let lower = match class.letters.as_str() {
        "JS" | "js" => -(width / 2),
        "H" => 0,
        // the holes D to G are the mirror of the shafts d to g
        "D" | "E" | "F" | "G" => -dev(class
            .letters
            .to_ascii_lowercase()
            .chars()
            .next()
            .unwrap_or('h'))?,
        "h" => -width,
        "d" | "e" | "f" | "g" => dev(class.letters.chars().next().unwrap_or('h'))? - width,
        "k" => {
            if class.grade > 7 {
                return Err(format!(
                    "{}{}: the letter k is in the table for the grades IT5 to IT7",
                    class.letters, class.grade
                ));
            }
            dev('k')?
        }
        "m" | "n" | "p" => dev(class.letters.chars().next().unwrap_or('h'))?,
        other => return Err(format!("the letters {other} are not in the table")),
    };
    let upper = lower + width;
    let nominal = (size_mm * 1.0e6).round() as i64;
    Ok(Limits {
        lower_dev_nm: lower,
        upper_dev_nm: upper,
        min_nm: nominal + lower,
        max_nm: nominal + upper,
    })
}

/// How a hole and a shaft of a fit meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The shaft is always smaller than the hole.
    Clearance,
    /// The shaft may be smaller or larger than the hole.
    Transition,
    /// The shaft is always larger than the hole.
    Interference,
}

/// A hole and a shaft that go together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fit {
    /// The limits of the hole.
    pub hole: Limits,
    /// The limits of the shaft.
    pub shaft: Limits,
    /// The smallest clearance (negative for an interference), in nanometres.
    pub min_clearance_nm: i64,
    /// The largest clearance (negative for an interference), in nanometres.
    pub max_clearance_nm: i64,
    /// The kind of fit.
    pub kind: Kind,
}

/// The fit of a hole class and a shaft class (`H7/g6`) at a nominal size.
pub fn fit(table: &Table, size_mm: f64, text: &str) -> Result<Fit, String> {
    let (hole_text, shaft_text) = text
        .split_once('/')
        .ok_or_else(|| format!("{text}: a fit is written as hole/shaft, as in H7/g6"))?;
    let hole_class = Class::parse(hole_text)?;
    let shaft_class = Class::parse(shaft_text)?;
    if !hole_class.is_hole() || shaft_class.is_hole() {
        return Err(format!(
            "{text}: a fit is a hole (capital letters) over a shaft (small letters)"
        ));
    }
    let hole = limits(table, size_mm, &hole_class)?;
    let shaft = limits(table, size_mm, &shaft_class)?;
    let min = hole.min_nm - shaft.max_nm;
    let max = hole.max_nm - shaft.min_nm;
    let kind = if min >= 0 {
        Kind::Clearance
    } else if max <= 0 {
        Kind::Interference
    } else {
        Kind::Transition
    };
    Ok(Fit {
        hole,
        shaft,
        min_clearance_nm: min,
        max_clearance_nm: max,
        kind,
    })
}

fn um(nm: i64) -> String {
    let sign = if nm < 0 { "-" } else { "" };
    let a = nm.abs();
    if a % 1000 == 0 {
        format!("{sign}{}", a / 1000)
    } else {
        format!("{sign}{}.{:03}", a / 1000, a % 1000)
            .trim_end_matches('0')
            .to_owned()
    }
}

fn mm(nm: i64) -> String {
    format!(
        "{}.{:03}",
        nm.div_euclid(1_000_000),
        (nm.rem_euclid(1_000_000)) / 1000
    )
}

/// The limits of one class as a sentence in the language.
pub fn limits_text(size_mm: f64, text: &str, l: &Limits, lang: Lang) -> String {
    fill(
        Msg::IsoLimits.tr(lang),
        &[
            text,
            &format!("{size_mm}"),
            &um(l.upper_dev_nm),
            &um(l.lower_dev_nm),
            &mm(l.max_nm),
            &mm(l.min_nm),
        ],
    )
}

/// The fit as sentences in the language, with the warning when the table is not verified.
pub fn fit_text(table: &Table, size_mm: f64, text: &str, f: &Fit, lang: Lang) -> String {
    let kind = match f.kind {
        Kind::Clearance => Msg::IsoClearance,
        Kind::Transition => Msg::IsoTransition,
        Kind::Interference => Msg::IsoInterference,
    };
    let (hole_text, shaft_text) = text.split_once('/').unwrap_or((text, ""));
    let mut out = String::new();
    out.push_str(&limits_text(size_mm, hole_text, &f.hole, lang));
    out.push('\n');
    out.push_str(&limits_text(size_mm, shaft_text, &f.shaft, lang));
    out.push('\n');
    out.push_str(&fill(
        Msg::IsoFit.tr(lang),
        &[
            text,
            kind.tr(lang),
            &um(f.min_clearance_nm),
            &um(f.max_clearance_nm),
        ],
    ));
    out.push('\n');
    if !table.verified {
        out.push_str(Msg::IsoUnverified.tr(lang));
        out.push('\n');
    }
    out
}

/// The fit as comma separated values: the text of the fit, the size, the limits of the hole and of
/// the shaft in nanometres, the clearances in nanometres and the kind.
pub fn fit_csv(size_mm: f64, text: &str, f: &Fit) -> String {
    let kind = match f.kind {
        Kind::Clearance => "clearance",
        Kind::Transition => "transition",
        Kind::Interference => "interference",
    };
    format!(
        "fit,size_mm,hole_min_nm,hole_max_nm,shaft_min_nm,shaft_max_nm,min_clearance_nm,max_clearance_nm,kind\n{text},{size_mm},{},{},{},{},{},{},{kind}\n",
        f.hole.min_nm,
        f.hole.max_nm,
        f.shaft.min_nm,
        f.shaft.max_nm,
        f.min_clearance_nm,
        f.max_clearance_nm
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(size: f64, class: &str) -> (i64, i64) {
        let l = limits(&Table::shipped(), size, &Class::parse(class).unwrap()).unwrap();
        (l.upper_dev_nm / 1000, l.lower_dev_nm / 1000)
    }

    #[test]
    fn the_shipped_table_parses_says_it_is_not_verified_and_covers_up_to_500_mm() {
        let t = Table::shipped();
        assert!(!t.verified);
        assert_eq!(t.ranges.len(), 13);
        assert_eq!(t.ranges.last().unwrap().1, 500.0);
        assert_eq!(t.deviations.len(), 9);
    }

    #[test]
    fn well_known_classes_give_their_deviations_in_micrometres() {
        // the values that every table of limits and fits shows for a size of 25 mm
        assert_eq!(dev(25.0, "H7"), (21, 0));
        assert_eq!(dev(25.0, "g6"), (-7, -20));
        assert_eq!(dev(25.0, "f7"), (-20, -41));
        assert_eq!(dev(25.0, "h6"), (0, -13));
        assert_eq!(dev(25.0, "k6"), (15, 2));
        assert_eq!(dev(25.0, "n6"), (28, 15));
        assert_eq!(dev(25.0, "p6"), (35, 22));
        assert_eq!(dev(25.0, "e8"), (-40, -73));
        assert_eq!(dev(25.0, "d9"), (-65, -117));
        assert_eq!(dev(25.0, "m6"), (21, 8));
        // and for 50 mm
        assert_eq!(dev(50.0, "H7"), (25, 0));
        assert_eq!(dev(50.0, "p6"), (42, 26));
        // holes D to G are the mirror of the shafts
        assert_eq!(dev(25.0, "G7"), (28, 7));
        assert_eq!(dev(25.0, "F8"), (53, 20));
        // symmetric classes
        assert_eq!(dev(25.0, "js7"), (10, -10));
        assert_eq!(dev(25.0, "JS9"), (26, -26));
    }

    #[test]
    fn the_edge_of_a_range_belongs_to_the_range_below() {
        // 18 mm is in 10 to 18 (IT7 = 18), 18.001 in 18 to 30 (IT7 = 21)
        assert_eq!(dev(18.0, "H7"), (18, 0));
        assert_eq!(dev(18.001, "H7"), (21, 0));
        assert_eq!(dev(3.0, "H7"), (10, 0));
        assert_eq!(dev(500.0, "H7"), (63, 0));
    }

    #[test]
    fn common_fits_are_the_kind_they_should_be() {
        let t = Table::shipped();
        let f = fit(&t, 25.0, "H7/g6").unwrap();
        assert_eq!(f.kind, Kind::Clearance);
        assert_eq!((f.min_clearance_nm, f.max_clearance_nm), (7_000, 41_000));
        assert_eq!(fit(&t, 25.0, "H7/h6").unwrap().min_clearance_nm, 0);
        assert_eq!(fit(&t, 25.0, "H7/k6").unwrap().kind, Kind::Transition);
        assert_eq!(fit(&t, 25.0, "H7/p6").unwrap().kind, Kind::Interference);
        let n6 = fit(&t, 25.0, "H7/n6").unwrap();
        assert_eq!((n6.min_clearance_nm, n6.max_clearance_nm), (-28_000, 6_000));
        assert_eq!(n6.hole.min_nm, 25_000_000);
        assert_eq!(n6.shaft.max_nm, 25_028_000);
    }

    #[test]
    fn what_is_not_in_the_table_is_refused_by_name() {
        let t = Table::shipped();
        let e = |size: f64, c: &str| fit(&t, size, c).unwrap_err();
        assert!(e(25.0, "H7").contains("hole/shaft"));
        assert!(e(25.0, "g6/H7").contains("hole (capital letters)"));
        assert!(e(25.0, "H7/x6").contains("not in the table"));
        assert!(e(25.0, "H13/g6").contains("IT5 to IT12"));
        assert!(e(25.0, "H/g6").contains("letters and a grade"));
        assert!(e(0.0, "H7/g6").contains("over 0"));
        assert!(e(501.0, "H7/g6").contains("at most 500"));
        assert!(e(f64::NAN, "H7/g6").contains("over 0"));
        assert!(e(25.0, "H7/k8").contains("IT5 to IT7"));
    }

    #[test]
    fn a_table_with_a_mistake_is_refused_by_its_line() {
        let shipped = SHIPPED;
        let e = |text: String| Table::parse(&text).unwrap_err();
        assert!(e(shipped.replace("verified = false", "verified = maybe")).contains("line 9"));
        assert!(e(shipped.replace("it 0 3 4 6", "it 0 3 4")).contains("needs 8 widths"));
        assert!(e(shipped.replace("it 3 6", "it 4 6")).contains("where the one before ends"));
        assert!(
            e(shipped.replace("dev h 0 0 0 0 0 0 0 0 0 0 0 0 0", "dev h 0 0"))
                .contains("2 deviations")
        );
        assert!(e(shipped.replace("dev d", "dev z")).contains("not one of"));
        assert!(e(shipped.replace("dev e", "dev d")).contains("twice"));
        assert!(e(shipped.replace("40 60 100\n", "x 60 100\n")).contains("not a whole number"));
        assert!(e("it 0 3 1 1 1 1 1 1 1 1\n".to_owned()).contains("verified"));
        assert!(e("verified = true\n".to_owned()).contains("no range"));
        assert!(e("verified = true\nwhat\n".to_owned()).contains("not understood"));
    }

    #[test]
    fn the_text_says_when_the_table_is_not_verified_in_every_language() {
        let t = Table::shipped();
        let f = fit(&t, 25.0, "H7/g6").unwrap();
        for lang in Lang::ALL {
            let text = fit_text(&t, 25.0, "H7/g6", &f, lang);
            assert!(text.contains(Msg::IsoUnverified.tr(lang)), "{lang:?}");
            assert!(text.contains("H7/g6"));
        }
        let mut checked = t.clone();
        checked.verified = true;
        assert!(!fit_text(&checked, 25.0, "H7/g6", &f, Lang::En).contains("not verified"));
        let en = fit_text(&t, 25.0, "H7/g6", &f, Lang::En);
        assert!(
            en.contains("H7: size 25 mm, deviations +0.021 / 0 mm") || en.contains("25.021"),
            "{en}"
        );
        assert!(en.contains("clearance from 7 to 41 um"), "{en}");
        let csv = fit_csv(25.0, "H7/g6", &f);
        assert!(csv
            .lines()
            .nth(1)
            .unwrap()
            .starts_with("H7/g6,25,25000000,25021000,24980000,24993000,7000,41000,clearance"));
    }

    #[test]
    fn micrometres_and_millimetres_are_written_without_noise() {
        assert_eq!(um(21_000), "21");
        assert_eq!(um(-7_500), "-7.5");
        assert_eq!(um(0), "0");
        assert_eq!(mm(25_021_000), "25.021");
        assert_eq!(mm(24_980_000), "24.980");
    }
}
