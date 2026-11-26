//! The parameters of the box as a table: the value of each, how it is defined (a fixed length, a
//! variable or an expression) and, when a limits file is given, whether the value is inside its
//! smallest and largest value and on its step.
//!
//! A limits file has lines `<parameter>.min = 1.2 mm`, `<parameter>.max = 4 mm` and
//! `<parameter>.step = 0.1 mm`; the parameters are the names of the fields of the box
//! (`inner_length`, `wall`...). Empty lines and lines that start with `#` are ignored; anything
//! else is refused with `PHOENIX-CAD-011`.

use std::collections::BTreeMap;

use crate::enclosure::EnclosureParams;
use crate::mechanical::Mechanical;
use crate::parametric::Parameter;
use crate::units::parse_length;
use crate::vars::Param;
use crate::CadError;

/// The most bytes of a limits file.
const MAX_BYTES: usize = 64 * 1024;

/// What a parameter should stay inside.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limit {
    /// The smallest value, in nanometres.
    pub min: Option<i64>,
    /// The largest value.
    pub max: Option<i64>,
    /// The value is a whole number of steps counted from the smallest value (from zero without
    /// one).
    pub step: Option<i64>,
}

/// Reads a limits file.
pub fn parse_limits(text: &str) -> Result<BTreeMap<Param, Limit>, CadError> {
    if text.len() > MAX_BYTES {
        return Err(CadError::Config("the limits file is too large".to_owned()));
    }
    let mut out: BTreeMap<Param, Limit> = BTreeMap::new();
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fail = |why: &str| CadError::Config(format!("limits line {}: {why}", number + 1));
        let (key, value) = line.split_once('=').ok_or_else(|| fail("no = sign"))?;
        let (name, what) = key
            .trim()
            .split_once('.')
            .ok_or_else(|| fail("the key is parameter.min, .max or .step"))?;
        let param = Param::from_name(name).ok_or_else(|| fail("unknown parameter"))?;
        let length = parse_length(value.trim())
            .map_err(|_| fail("not a length with its unit"))?
            .get();
        let limit = out.entry(param).or_default();
        let slot = match what {
            "min" => &mut limit.min,
            "max" => &mut limit.max,
            "step" if length > 0 => &mut limit.step,
            "step" => return Err(fail("a step is positive")),
            _ => return Err(fail("the key is parameter.min, .max or .step")),
        };
        if slot.replace(length).is_some() {
            return Err(fail("the key is given twice"));
        }
    }
    if out
        .values()
        .any(|l| matches!((l.min, l.max), (Some(a), Some(b)) if a > b))
    {
        return Err(CadError::Config(
            "limits: a smallest value is above its largest value".to_owned(),
        ));
    }
    Ok(out)
}

/// How a value breaks its limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breach {
    /// Below the smallest value.
    Below,
    /// Above the largest value.
    Above,
    /// Not on a step.
    Step,
}

impl Breach {
    /// The word of the comma separated values.
    pub fn word(self) -> &'static str {
        match self {
            Breach::Below => "below",
            Breach::Above => "above",
            Breach::Step => "step",
        }
    }
}

/// One parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The parameter.
    pub param: Param,
    /// Its value in the built box, in nanometres.
    pub value: i64,
    /// `fixed`, `variable` or `expression`.
    pub kind: &'static str,
    /// The name of the variable or the text of the expression.
    pub definition: String,
    /// What it breaks.
    pub breaches: Vec<Breach>,
}

fn breaches(value: i64, limit: Limit) -> Vec<Breach> {
    let mut out = Vec::new();
    if limit.min.is_some_and(|min| value < min) {
        out.push(Breach::Below);
    }
    if limit.max.is_some_and(|max| value > max) {
        out.push(Breach::Above);
    }
    if let Some(step) = limit.step.filter(|s| *s > 0) {
        if (i128::from(value) - i128::from(limit.min.unwrap_or(0))) % i128::from(step) != 0 {
            out.push(Breach::Step);
        }
    }
    out
}

/// The table of the parameters, in the order of the fields of the box. `params` are the ones of
/// the built box.
pub fn rows(
    mech: &Mechanical,
    params: &EnclosureParams,
    limits: &BTreeMap<Param, Limit>,
) -> Vec<Row> {
    Param::ALL
        .into_iter()
        .map(|param| {
            let value = param.get(params).get();
            let (kind, definition) = match mech.model.get(param) {
                None | Some(Parameter::Fixed(_)) => ("fixed", String::new()),
                Some(Parameter::Variable(name)) => ("variable", name.clone()),
                Some(Parameter::Expr(expr)) => ("expression", expr.to_string()),
            };
            Row {
                param,
                value,
                kind,
                definition,
                breaches: limits
                    .get(&param)
                    .map_or_else(Vec::new, |l| breaches(value, *l)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eh3dfile::tests::Seeded;
    use crate::parametric::ParamModel;
    use crate::uiconfig::UiSettings;
    use crate::vars::Variables;
    use crate::Length;

    #[test]
    fn limits_are_read_and_wrong_lines_are_refused() {
        let text = "# walls\nwall.min = 1.2 mm\nwall.max = 4 mm\n\nwall.step = 0.1 mm\n";
        let limits = parse_limits(text).unwrap();
        assert_eq!(
            limits.get(&Param::Wall),
            Some(&Limit {
                min: Some(1_200_000),
                max: Some(4_000_000),
                step: Some(100_000),
            })
        );
        for wrong in [
            "wall.min",
            "wall = 1 mm",
            "roof.min = 1 mm",
            "wall.mid = 1 mm",
            "wall.min = 1",
            "wall.step = 0 mm",
            "wall.min = 1 mm\nwall.min = 2 mm",
            "wall.min = 3 mm\nwall.max = 2 mm",
        ] {
            let error = parse_limits(wrong).unwrap_err();
            assert_eq!(error.code(), "PHOENIX-CAD-011", "{wrong}");
        }
    }

    #[test]
    fn the_table_names_what_each_value_breaks() {
        let params = UiSettings::defaults().new_box;
        let mut mech = Mechanical::new(ParamModel::new(params).unwrap(), Variables::new());
        mech.model
            .set(Param::Wall, Parameter::Fixed(Length::from_mm(2).unwrap()))
            .unwrap();
        let mut built = params;
        built.wall = Length::new(2_050_000).unwrap();
        let limits =
            parse_limits("wall.min = 3 mm\nlid.max = 0.001 mm\nwall.step = 0.1 mm\n").unwrap();
        let table = rows(&mech, &built, &limits);
        assert_eq!(table.len(), 8);
        let of = |p: Param| table.iter().find(|r| r.param == p).unwrap();
        assert!(of(Param::Wall).breaches.contains(&Breach::Below));
        assert_eq!(of(Param::Lid).breaches, vec![Breach::Above]);
        assert_eq!(of(Param::Wall).kind, "fixed");
        assert!(of(Param::InnerLength).breaches.is_empty());
    }

    #[test]
    fn random_values_break_exactly_what_the_arithmetic_says() {
        let mut random = Seeded(0x00CA_DB07);
        for _ in 0..2000 {
            let min = random.below(1000) as i64;
            let max = min + random.below(1000) as i64;
            let step = 1 + random.below(50) as i64;
            let value = random.below(2500) as i64;
            let found = breaches(
                value,
                Limit {
                    min: Some(min),
                    max: Some(max),
                    step: Some(step),
                },
            );
            assert_eq!(found.contains(&Breach::Below), value < min);
            assert_eq!(found.contains(&Breach::Above), value > max);
            let on = (0..=2500).any(|k| min + k * step == value || min - k * step == value);
            assert_eq!(found.contains(&Breach::Step), !on);
        }
    }
}
