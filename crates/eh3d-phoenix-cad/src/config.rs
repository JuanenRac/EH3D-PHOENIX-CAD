//! The settings file.
//!
//! Every value that someone may tune (copies kept when saving, depth of the undo history, the unit
//! shown, the compensation of a printer, the table of screws) lives in one plain text file with
//! sections, `key = value` lines and comments. The file that ships with the crate
//! ([`DEFAULT_CONFIG`]) holds the defaults and documents each key; a user file only needs the keys
//! it changes. A key that is missing, repeated or not understood is refused with its line number.
//!
//! The same file holds the limits of the language of expressions (`[expr]`) and the constants that
//! expressions can use (`[constants]`, and the sizes of the screws and the compensation of the
//! printer, which are named by rule: `screw_M3_clearance`, `printer_fit_clearance`).

use std::collections::BTreeMap;

use crate::enclosure::{Face, Hole, HoleStyle};
use crate::expr::{parse, Constants, ExprLimits, Value};
use crate::printer::PrinterProfile;
use crate::units::{parse_length, Unit};
use crate::{CadError, Length};

/// The default settings, as shipped.
pub const DEFAULT_CONFIG: &str = include_str!("../phoenix-cad.default.cfg");

/// The most copies that a save may keep.
pub const MAX_BACKUPS: u32 = 99;

/// The most transactions that the history may keep.
pub const MAX_UNDO_DEPTH: usize = 10_000;

/// A parsed settings file: values by `section.key`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    values: BTreeMap<String, String>,
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
}

fn strip_comment(value: &str) -> &str {
    let cut = [value.find(" #"), value.find(" ;")]
        .into_iter()
        .flatten()
        .min();
    match cut {
        Some(i) => &value[..i],
        None => value,
    }
}

impl Config {
    /// Reads a settings text. Lines starting with `#` or `;` are comments; a text after ` #` on a
    /// value line is a comment too.
    pub fn parse(text: &str) -> Result<Config, CadError> {
        let mut values = BTreeMap::new();
        let mut section = String::new();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let err = |why: &str| CadError::Config(format!("line {}: {why}", i + 1));
            if let Some(rest) = line.strip_prefix('[') {
                let name = rest
                    .strip_suffix(']')
                    .ok_or_else(|| err("a section name needs a closing ]"))?
                    .trim();
                if !valid_name(name) {
                    return Err(err("the section name is not valid"));
                }
                section = name.to_string();
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| err("expected key = value"))?;
            let key = key.trim();
            if !valid_name(key) {
                return Err(err("the key is not valid"));
            }
            if section.is_empty() {
                return Err(err("a key before the first section"));
            }
            let full = format!("{section}.{key}");
            let value = strip_comment(value).trim().to_string();
            if values.insert(full, value).is_some() {
                return Err(err("the key is repeated"));
            }
        }
        Ok(Config { values })
    }

    /// Puts the values of `other` over these ones.
    pub fn overlay(&mut self, other: &Config) {
        for (k, v) in &other.values {
            self.values.insert(k.clone(), v.clone());
        }
    }

    /// The value of a key written as `section.key`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// Every key with its value, in order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }

    fn need(&self, key: &str) -> Result<&str, CadError> {
        self.get(key)
            .ok_or_else(|| CadError::Config(format!("{key} is missing")))
    }

    /// A length with its unit; the key must exist.
    pub fn length(&self, key: &str) -> Result<Length, CadError> {
        parse_length(self.need(key)?).map_err(|e| CadError::Config(format!("{key}: {e}")))
    }

    /// A whole number from 0 up; the key must exist.
    pub fn number(&self, key: &str) -> Result<u32, CadError> {
        self.need(key)?
            .parse::<u32>()
            .map_err(|_| CadError::Config(format!("{key} is not a whole number")))
    }
}

/// The size of a metric screw and of the holes it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screw {
    /// The nominal diameter of the thread.
    pub nominal: Length,
    /// The diameter of the hole the screw passes through.
    pub clearance: Length,
    /// The diameter of a socket head.
    pub head_diameter: Length,
    /// The height of a socket head.
    pub head_height: Length,
    /// The diameter of the head of a flat countersunk screw.
    pub countersink_diameter: Length,
}

/// The kind of head that a hole made for a screw has to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Head {
    /// A plain hole that the screw passes through.
    Clearance,
    /// A countersink for a flat countersunk head.
    Countersunk,
    /// A recess for a socket head.
    Counterbored,
}

/// The settings with their types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Copies kept when a file is saved over an older one.
    pub backups: u32,
    /// Transactions that can be undone.
    pub undo_depth: usize,
    /// The unit in which lengths are shown.
    pub unit: Unit,
    /// The decimals shown.
    pub decimals: u32,
    /// The compensation of the printer.
    pub printer: PrinterProfile,
    /// Added to the head diameter for a counterbore.
    pub counterbore_extra: Length,
    /// The screws by name (`M3`), in order of name.
    pub screws: BTreeMap<String, Screw>,
    /// The density in kilograms per cubic metre of each material by name (`PLA`).
    pub materials: BTreeMap<String, u32>,
    /// The limits of the language of expressions.
    pub expr: ExprLimits,
    /// The named values that expressions can use: the sizes of the screws, the compensation of the
    /// printer and the `[constants]` of the settings.
    pub constants: Constants,
}

/// Adds a constant, refusing a name that is already taken or is not a plain word.
fn add_constant(constants: &mut Constants, name: &str, value: Value) -> Result<(), CadError> {
    if constants.get(name).is_some() {
        return Err(CadError::Config(format!(
            "the constant {name} is defined twice"
        )));
    }
    constants
        .set(name, value)
        .map_err(|e| CadError::Config(format!("constant {name}: {e}")))
}

/// The constants of the settings: the printer, the screws (`screw_M2_5_clearance` for the screw
/// `M2.5`) and the keys of `[constants]`, which are expressions that may use the first two kinds.
fn build_constants(
    c: &Config,
    screws: &BTreeMap<String, Screw>,
    printer: &PrinterProfile,
    counterbore_extra: Length,
    limits: &ExprLimits,
) -> Result<Constants, CadError> {
    let mut built = Constants::new();
    for (name, value) in [
        ("counterbore_extra", counterbore_extra),
        ("printer_hole_compensation", printer.hole_compensation),
        ("printer_small_hole_limit", printer.small_hole_limit),
        ("printer_small_hole_extra", printer.small_hole_extra),
        ("printer_fit_clearance", printer.fit_clearance),
    ] {
        add_constant(&mut built, name, Value::from_length(value))?;
    }
    for (name, screw) in screws {
        let id = name.replace(['.', '-'], "_");
        for (field, value) in [
            ("nominal", screw.nominal),
            ("clearance", screw.clearance),
            ("head_diameter", screw.head_diameter),
            ("head_height", screw.head_height),
            ("countersink_diameter", screw.countersink_diameter),
        ] {
            add_constant(
                &mut built,
                &format!("screw_{id}_{field}"),
                Value::from_length(value),
            )?;
        }
    }
    let known = built.clone();
    for key in c.keys() {
        if let Some(name) = key.strip_prefix("constants.") {
            let text = c.need(key)?;
            let value = parse(text, limits)
                .and_then(|expr| expr.eval(&known, limits))
                .map_err(|e| CadError::Config(format!("{key}: {e}")))?;
            add_constant(&mut built, name, value)?;
        }
    }
    Ok(built)
}

impl Settings {
    /// The defaults that ship with the crate.
    pub fn defaults() -> Settings {
        let config = Config::parse(DEFAULT_CONFIG).expect("the shipped settings are valid");
        Settings::from_config(&config).expect("the shipped settings are complete")
    }

    /// The defaults with the keys of a user file put over them.
    pub fn with_user_config(text: &str) -> Result<Settings, CadError> {
        let mut config = Config::parse(DEFAULT_CONFIG)?;
        config.overlay(&Config::parse(text)?);
        Settings::from_config(&config)
    }

    /// A screw of the table, or the error `PHOENIX-CAD-021` when the table does not have it.
    pub fn screw(&self, name: &str) -> Result<&Screw, CadError> {
        self.screws
            .get(name)
            .ok_or_else(|| CadError::Screw(name.to_string()))
    }

    /// A hole for a screw of the table, placed with its centre at `at` (`u`, `v`): the diameter is
    /// the clearance of the screw, the countersink is the diameter of its flat head and the
    /// counterbore is the diameter of its socket head plus `counterbore_extra`, as deep as the head
    /// is high. Whether the hole fits the box is checked when it is added to the enclosure.
    pub fn screw_hole(
        &self,
        screw: &str,
        head: Head,
        name: &str,
        face: Face,
        at: (Length, Length),
    ) -> Result<Hole, CadError> {
        let s = self.screw(screw)?;
        let style = match head {
            Head::Clearance => HoleStyle::Plain,
            Head::Countersunk => HoleStyle::Countersunk {
                head_diameter: s.countersink_diameter,
            },
            Head::Counterbored => HoleStyle::Counterbored {
                head_diameter: s.head_diameter.checked_add(self.counterbore_extra)?,
                depth: s.head_height,
            },
        };
        Ok(Hole {
            name: name.to_string(),
            face,
            u: at.0,
            v: at.1,
            diameter: s.clearance,
            style,
        })
    }

    /// Reads the typed settings from a complete configuration.
    pub fn from_config(c: &Config) -> Result<Settings, CadError> {
        let backups = c.number("save.backups")?;
        if backups > MAX_BACKUPS {
            return Err(CadError::Config("save.backups is too large".into()));
        }
        let undo_depth = usize::try_from(c.number("undo.depth")?)
            .map_err(|_| CadError::Config("undo.depth is too large".into()))?;
        if undo_depth > MAX_UNDO_DEPTH {
            return Err(CadError::Config("undo.depth is too large".into()));
        }
        let unit = Unit::from_suffix(c.need("units.display")?)
            .ok_or_else(|| CadError::Config("units.display is not a unit".into()))?;
        let decimals = c.number("units.decimals")?;
        if decimals > 12 {
            return Err(CadError::Config("units.decimals is above 12".into()));
        }
        let printer = PrinterProfile {
            hole_compensation: c.length("printer.hole_compensation")?,
            small_hole_limit: c.length("printer.small_hole_limit")?,
            small_hole_extra: c.length("printer.small_hole_extra")?,
            fit_clearance: c.length("printer.fit_clearance")?,
        };
        let counterbore_extra = c.length("screws.counterbore_extra")?;
        let mut screws = BTreeMap::new();
        for key in c.keys() {
            let name = match key
                .strip_prefix("screw.")
                .and_then(|rest| rest.strip_suffix(".clearance"))
            {
                Some(name) => name,
                None => continue,
            };
            let get = |field: &str| c.length(&format!("screw.{name}.{field}"));
            let screw = Screw {
                nominal: get("nominal")?,
                clearance: get("clearance")?,
                head_diameter: get("head_diameter")?,
                head_height: get("head_height")?,
                countersink_diameter: get("countersink_diameter")?,
            };
            for (what, v) in [
                ("nominal", screw.nominal),
                ("clearance", screw.clearance),
                ("head_diameter", screw.head_diameter),
                ("head_height", screw.head_height),
                ("countersink_diameter", screw.countersink_diameter),
            ] {
                if v.get() <= 0 {
                    return Err(CadError::Config(format!(
                        "screw.{name}.{what} must be positive"
                    )));
                }
            }
            screws.insert(name.to_string(), screw);
        }
        let mut materials = BTreeMap::new();
        for key in c.keys() {
            if let Some(name) = key
                .strip_prefix("material.")
                .and_then(|rest| rest.strip_suffix(".density_kg_m3"))
            {
                let density = c.number(key)?;
                if density == 0 {
                    return Err(CadError::Config(format!("{key} must be positive")));
                }
                materials.insert(name.to_string(), density);
            }
        }
        let expr = ExprLimits::new(
            c.number("expr.max_length")?,
            c.number("expr.max_depth")?,
            c.number("expr.max_steps")?,
            c.number("expr.max_exponent")?,
        )?;
        let constants = build_constants(c, &screws, &printer, counterbore_extra, &expr)?;
        Ok(Settings {
            backups,
            undo_depth,
            unit,
            decimals,
            printer,
            counterbore_extra,
            screws,
            materials,
            expr,
            constants,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_defaults_are_valid_and_complete() {
        let s = Settings::defaults();
        assert_eq!(s.backups, 3);
        assert_eq!(s.undo_depth, 100);
        assert_eq!(s.unit, Unit::Millimetre);
        assert_eq!(s.decimals, 3);
        assert_eq!(s.printer.hole_compensation.get(), 150_000);
        assert_eq!(s.printer.small_hole_limit.get(), 3_000_000);
        assert_eq!(s.printer.fit_clearance.get(), 200_000);
        assert_eq!(s.counterbore_extra.get(), 500_000);
        let names: Vec<&str> = s.screws.keys().map(String::as_str).collect();
        assert_eq!(names, vec!["M2", "M2.5", "M3", "M4", "M5", "M6", "M8"]);
        assert_eq!(s.materials["PLA"], 1240);
        assert!(s.materials.values().all(|d| *d > 0));
        assert_eq!(
            s.expr,
            ExprLimits {
                max_length: 512,
                max_depth: 64,
                max_steps: 10_000,
                max_exponent: 64
            }
        );
    }

    #[test]
    fn the_screws_the_printer_and_the_constants_of_the_settings_are_named_values_of_expressions() {
        let s = Settings::defaults();
        let nm = |name: &str| s.constants.get(name);
        let len = |nm: i64| Some(Value::from_length(Length::new(nm).unwrap()));
        assert_eq!(nm("screw_M3_clearance"), len(3_400_000));
        assert_eq!(nm("screw_M2_5_nominal"), len(2_500_000));
        assert_eq!(nm("screw_M8_countersink_diameter"), len(16_800_000));
        assert_eq!(nm("counterbore_extra"), len(500_000));
        assert_eq!(nm("printer_fit_clearance"), len(200_000));
        assert_eq!(nm("printer_hole_compensation"), len(150_000));
        assert_eq!(nm("nozzle"), len(400_000));
        assert_eq!(nm("layer_height"), len(200_000));
        assert_eq!(nm("no_such_constant"), None);
        // five values for each of the seven screws, four for the printer, one for the counterbore
        // and the constants of the file
        assert_eq!(s.constants.len(), 7 * 5 + 5 + 2);
    }

    #[test]
    fn a_user_file_adds_constants_that_may_use_the_screws_and_the_printer() {
        let user = "[constants]\nwall_min = 4 * 0.4 mm\nhole = screw_M3_clearance + printer_hole_compensation\nsides = 4\n\n[expr]\nmax_depth = 10\n";
        let s = Settings::with_user_config(user).unwrap();
        let len = |nm: i64| Some(Value::from_length(Length::new(nm).unwrap()));
        assert_eq!(s.constants.get("wall_min"), len(1_600_000));
        assert_eq!(s.constants.get("hole"), len(3_550_000));
        assert_eq!(
            s.constants.get("sides"),
            Some(crate::expr::Value::Number(crate::expr::Rat::int(4)))
        );
        assert_eq!(s.expr.max_depth, 10);
        // the file may change a constant of the defaults
        let s = Settings::with_user_config("[constants]\nnozzle = 0.6 mm\n").unwrap();
        assert_eq!(s.constants.get("nozzle"), len(600_000));
    }

    #[test]
    fn constants_and_limits_that_make_no_sense_are_refused_by_key() {
        for text in [
            "[constants]\nbad = 1 +\n",
            "[constants]\nbad = 1 mm / 0\n",
            "[constants]\nbad = other\n",
            "[constants]\nmm = 1 mm\n",
            "[constants]\nbad.name = 1 mm\n",
            "[constants]\ncounterbore_extra = 1 mm\n",
            "[constants]\nscrew_M3_clearance = 1 mm\n",
            "[expr]\nmax_length = 0\n",
            "[expr]\nmax_depth = 129\n",
            "[expr]\nmax_steps = 100000001\n",
            "[expr]\nmax_exponent = 1025\n",
            "[expr]\nmax_exponent = many\n",
            "[screw.M2_5]\nnominal = 2.5 mm\nclearance = 2.9 mm\nhead_diameter = 4.5 mm\nhead_height = 2.5 mm\ncountersink_diameter = 5 mm\n",
        ] {
            let err = Settings::with_user_config(text).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-011", "{text:?}: {err}");
        }
        // a constant cannot use another constant of the same section
        let err = Settings::with_user_config("[constants]\na = 1 mm\nb = a\n").unwrap_err();
        assert!(err.to_string().contains("constants.b"), "{err}");
    }

    #[test]
    fn the_screw_table_is_consistent_with_itself() {
        let s = Settings::defaults();
        for (name, screw) in &s.screws {
            // the hole is wider than the thread and narrower than the head
            assert!(screw.clearance > screw.nominal, "{name}");
            assert!(screw.head_diameter > screw.clearance, "{name}");
            assert!(screw.countersink_diameter > screw.clearance, "{name}");
        }
        assert_eq!(s.screws["M3"].clearance.get(), 3_400_000);
        assert_eq!(s.screws["M2.5"].nominal.get(), 2_500_000);
    }

    #[test]
    fn a_user_file_changes_only_the_keys_it_names() {
        let user = "[save]\nbackups = 5 # more copies\n\n[printer]\nhole_compensation = 0,2 mm\n";
        let s = Settings::with_user_config(user).unwrap();
        assert_eq!(s.backups, 5);
        assert_eq!(s.printer.hole_compensation.get(), 200_000);
        assert_eq!(s.undo_depth, 100);
        assert_eq!(s.printer.fit_clearance.get(), 200_000);
        // the user may add a screw of their own
        let own = "[screw.M3.5]\nnominal = 3.5 mm\nclearance = 3.9 mm\nhead_diameter = 6 mm\nhead_height = 3.5 mm\ncountersink_diameter = 7.3 mm\n";
        let s = Settings::with_user_config(own).unwrap();
        assert!(s.screws.contains_key("M3.5"));
        assert_eq!(s.screws.len(), 8);
    }

    #[test]
    fn a_bad_file_is_refused_with_the_line() {
        for (text, needle) in [
            ("[save\nbackups = 1\n", "line 1"),
            ("backups = 1\n", "line 1"),
            ("[save]\nbackups\n", "line 2"),
            ("[save]\nbackups = 1\nbackups = 2\n", "line 3"),
            ("[sa ve]\n", "line 1"),
            ("[save]\n = 3\n", "line 2"),
        ] {
            let err = Config::parse(text).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-011", "{text:?}");
            assert!(err.to_string().contains(needle), "{text:?}: {err}");
        }
    }

    #[test]
    fn values_that_make_no_sense_are_refused_by_key() {
        for text in [
            "[save]\nbackups = 100\n",
            "[save]\nbackups = many\n",
            "[undo]\ndepth = 100000\n",
            "[units]\ndisplay = furlong\n",
            "[units]\ndecimals = 13\n",
            "[printer]\nfit_clearance = 0.2\n",
            "[screw.M3]\nclearance = 0 mm\n",
            "[material.PLA]\ndensity_kg_m3 = 0\n",
        ] {
            let err = Settings::with_user_config(text).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-011", "{text:?}");
        }
        let incomplete = Config::parse("[save]\nbackups = 1\n").unwrap();
        let err = Settings::from_config(&incomplete).unwrap_err();
        assert!(err.to_string().contains("undo.depth is missing"));
    }

    #[test]
    fn comments_and_blank_lines_are_ignored_and_a_value_may_hold_a_hash() {
        let c =
            Config::parse("# a comment\n; another\n\n[a]\nk = v#w # note\nj = 1 ; note\n").unwrap();
        assert_eq!(c.get("a.k"), Some("v#w"));
        assert_eq!(c.get("a.j"), Some("1"));
        assert_eq!(c.keys().collect::<Vec<_>>(), vec!["a.j", "a.k"]);
    }

    #[test]
    fn a_hole_for_a_screw_takes_its_sizes_from_the_table() {
        use crate::enclosure::HoleStyle;
        let s = Settings::defaults();
        let at = (Length::from_mm(30).unwrap(), Length::from_mm(17).unwrap());
        let plain = s
            .screw_hole("M3", Head::Clearance, "m3", Face::Front, at)
            .unwrap();
        assert_eq!(plain.diameter.get(), 3_400_000);
        assert_eq!(plain.style, HoleStyle::Plain);
        let sunk = s
            .screw_hole("M3", Head::Countersunk, "m3", Face::Front, at)
            .unwrap();
        assert_eq!(
            sunk.style,
            HoleStyle::Countersunk {
                head_diameter: Length::new(6_300_000).unwrap()
            }
        );
        // the socket head of 5.5 mm and the extra of 0.5 mm, as deep as the head is high
        let bored = s
            .screw_hole("M3", Head::Counterbored, "m3", Face::Front, at)
            .unwrap();
        assert_eq!(
            bored.style,
            HoleStyle::Counterbored {
                head_diameter: Length::new(6_000_000).unwrap(),
                depth: Length::new(3_000_000).unwrap()
            }
        );
        assert_eq!(bored.u, at.0);
        let err = s
            .screw_hole("M99", Head::Clearance, "x", Face::Front, at)
            .unwrap_err();
        assert_eq!(err, CadError::Screw("M99".into()));
        assert_eq!(err.code(), "PHOENIX-CAD-021");
    }

    #[test]
    fn a_screw_hole_is_checked_against_the_box_when_it_is_added() {
        use crate::enclosure::{Enclosure, EnclosureParams, Operation};
        let mm = |v: i64| Length::from_mm(v).unwrap();
        let build = |wall: i64| {
            Enclosure::new(EnclosureParams {
                inner_length: mm(100),
                inner_width: mm(60),
                inner_height: mm(30),
                wall: mm(wall),
                floor: mm(wall),
                lid: mm(wall),
                edge_margin: mm(3),
                min_web: mm(2),
            })
            .unwrap()
        };
        let s = Settings::defaults();
        let at = (mm(50), mm(17));
        let id = eh3d_container::ObjectId::from_parts(1_700_000_000_000, 1, 1).unwrap();
        let hole = s
            .screw_hole("M3", Head::Counterbored, "m3", Face::Front, at)
            .unwrap();
        // the head of an M3 screw is 3 mm high: it does not fit in a wall of 3 mm, it does in 4 mm
        let err = build(3)
            .apply(Operation::AddHole {
                id,
                hole: hole.clone(),
            })
            .unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-020");
        assert!(build(4).apply(Operation::AddHole { id, hole }).is_ok());
    }
}
