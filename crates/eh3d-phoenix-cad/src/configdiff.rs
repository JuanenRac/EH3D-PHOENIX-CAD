// SPDX-License-Identifier: GPL-3.0-or-later
//! What a settings file changes: every key of a user file put next to the value it has in the
//! shipped files (the settings of the library, of the window and of the sketch), so that a person
//! can see at a glance which values they tuned, which they only repeated and which are not keys of
//! the defaults at all (a typing mistake, or a key of a section that the person adds, such as the
//! price of a material).
//!
//! The values are compared as the text the settings parser keeps (comments and the blanks around a
//! value are already gone), so `0.2 mm` and `0.20 mm` are two different values for this check even
//! though they mean the same length. Nothing is changed and nothing is validated: reading the
//! file for use is what [`crate::config::Settings::with_user_config`] does.

use std::fmt::Write as _;

use crate::bom::cell;
use crate::config::{Config, DEFAULT_CONFIG};
use crate::lang::{fill, Lang, Msg};
use crate::CadError;

/// How a key of the user file compares with the defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The value is not the default one.
    Changed,
    /// The value is the default one.
    Same,
    /// The key is not in the defaults.
    Extra,
}

impl Status {
    /// The word the comma separated values use.
    pub fn word(self) -> &'static str {
        match self {
            Status::Changed => "changed",
            Status::Same => "same",
            Status::Extra => "extra",
        }
    }
}

/// One key of the user file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The key as `section.key`.
    pub key: String,
    /// The value in the defaults, when the key is there.
    pub default: Option<String>,
    /// The value in the user file.
    pub value: String,
    /// How the two compare.
    pub status: Status,
}

/// The keys of a user settings file with their defaults, in the order of the keys.
pub fn diff(user_text: &str) -> Result<Vec<Row>, CadError> {
    let user = Config::parse(user_text)?;
    let defaults = [
        Config::parse(DEFAULT_CONFIG)?,
        Config::parse(crate::uiconfig::default_file())?,
        Config::parse(crate::sketch::settings::default_file())?,
        Config::parse(crate::featuredag::default_file())?,
        Config::parse(crate::plans::settings::default_file())?,
    ];
    let mut rows = Vec::new();
    for key in user.keys() {
        let value = user.get(key).unwrap_or_default().to_owned();
        let default = defaults.iter().find_map(|d| d.get(key)).map(str::to_owned);
        let status = match &default {
            None => Status::Extra,
            Some(d) if *d == value => Status::Same,
            Some(_) => Status::Changed,
        };
        rows.push(Row {
            key: key.to_owned(),
            default,
            value,
            status,
        });
    }
    Ok(rows)
}

/// How many keys have each status: changed, the same and extra.
pub fn counts(rows: &[Row]) -> (usize, usize, usize) {
    let of = |s: Status| rows.iter().filter(|r| r.status == s).count();
    (of(Status::Changed), of(Status::Same), of(Status::Extra))
}

/// The rows as comma separated values, the same text in any language.
pub fn csv(rows: &[Row]) -> String {
    let mut out = String::from("key,default,value,status\n");
    for r in rows {
        let _ = writeln!(
            out,
            "{},{},{},{}",
            cell(&r.key),
            cell(r.default.as_deref().unwrap_or("")),
            cell(&r.value),
            r.status.word()
        );
    }
    out
}

/// A summary line and a line for each key that is not the default, in a language; the keys that are
/// the same as the default are only counted.
pub fn text_in(rows: &[Row], lang: Lang) -> String {
    let (changed, same, extra) = counts(rows);
    let mut out = fill(
        Msg::ConfigDiffHead.tr(lang),
        &[
            &rows.len().to_string(),
            &changed.to_string(),
            &same.to_string(),
            &extra.to_string(),
        ],
    );
    out.push('\n');
    for r in rows {
        match (r.status, &r.default) {
            (Status::Changed, Some(d)) => out.push_str(&fill(
                Msg::ConfigDiffChanged.tr(lang),
                &[&r.key, d, &r.value],
            )),
            (Status::Extra, _) => {
                out.push_str(&fill(Msg::ConfigDiffExtra.tr(lang), &[&r.key, &r.value]))
            }
            _ => continue,
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = "[printer]\nhole_compensation = 0.2 mm   # tuned\nfit_clearance = 0.20 mm\n[nonsense]\nfoo = 1\n";

    #[test]
    fn a_key_is_changed_the_same_or_not_in_the_defaults() {
        let rows = diff(USER).unwrap();
        let by = |k: &str| rows.iter().find(|r| r.key == k).unwrap().clone();
        let changed = by("printer.hole_compensation");
        assert_eq!(changed.status, Status::Changed);
        assert_eq!(changed.default.as_deref(), Some("0.15 mm"));
        assert_eq!(changed.value, "0.2 mm");
        let same = by("printer.fit_clearance");
        assert_eq!(same.status, Status::Same);
        assert_eq!(same.default.as_deref(), Some("0.20 mm"));
        let extra = by("nonsense.foo");
        assert_eq!(extra.status, Status::Extra);
        assert_eq!(extra.default, None);
        assert_eq!(counts(&rows), (1, 1, 1));
        // the keys come in order
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn an_empty_file_and_a_file_of_comments_have_no_keys() {
        assert!(diff("").unwrap().is_empty());
        assert!(diff("# nothing here\n\n").unwrap().is_empty());
        assert_eq!(counts(&[]), (0, 0, 0));
    }

    #[test]
    fn a_file_that_does_not_parse_is_refused_with_the_error_of_the_settings() {
        let e = diff("[printer\nx = 1\n").unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-011");
    }

    #[test]
    fn every_key_of_the_shipped_files_is_the_same_as_its_default() {
        // the shipped files read as a user file change nothing
        let text = DEFAULT_CONFIG.to_owned();
        let rows = diff(&text).unwrap();
        assert!(rows.len() > 50);
        assert_eq!(counts(&rows), (0, rows.len(), 0));
        let ui = diff(crate::uiconfig::default_file()).unwrap();
        assert_eq!(counts(&ui), (0, ui.len(), 0));
        let sk = diff(crate::sketch::settings::default_file()).unwrap();
        assert_eq!(counts(&sk), (0, sk.len(), 0));
    }

    #[test]
    fn the_csv_has_a_row_for_each_key_and_guards_the_cells() {
        let rows = diff(USER).unwrap();
        let out = csv(&rows);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "key,default,value,status");
        assert_eq!(lines.len(), 4);
        assert!(lines.contains(&"printer.hole_compensation,0.15 mm,0.2 mm,changed"));
        assert!(lines.contains(&"printer.fit_clearance,0.20 mm,0.20 mm,same"));
        assert!(lines.contains(&"nonsense.foo,,1,extra"));
        let odd = vec![Row {
            key: "a.b".into(),
            default: None,
            value: "=1+1, \"x\"".into(),
            status: Status::Extra,
        }];
        assert_eq!(
            csv(&odd).lines().nth(1).unwrap(),
            "a.b,,\"'=1+1, \"\"x\"\"\",extra"
        );
    }

    #[test]
    fn the_text_counts_every_key_and_lists_the_ones_that_are_not_the_default() {
        let rows = diff(USER).unwrap();
        for lang in Lang::ALL {
            let text = text_in(&rows, lang);
            assert_eq!(text.lines().count(), 3, "{}", lang.code());
            assert!(!text.contains('{'));
            assert!(text.contains("printer.hole_compensation") && text.contains("nonsense.foo"));
            assert!(!text.contains("fit_clearance"));
        }
        let en = text_in(&rows, Lang::En);
        assert!(en.starts_with(
            "3 keys read: 1 changed, 1 the same as the default, 1 not in the defaults\n"
        ));
        assert!(en.contains("printer.hole_compensation: 0.15 mm -> 0.2 mm\n"));
        assert!(en.contains("nonsense.foo = 1 (not a key of the defaults)\n"));
    }
}
