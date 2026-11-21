// SPDX-License-Identifier: GPL-3.0-or-later
//! The tolerance written next to the value of a dimension: a class of the system of limits and
//! fits of ISO 286 (`H7`, `g6`, or a fit `H7/g6`), read from the table of `iso286`, or deviations
//! given by hand (`+0.1/-0.2`, which is written with the plus-minus sign when they are alike). Everything is whole
//! nanometres; the table says whether it has been checked and the sheet repeats it.

use super::model::DimKind;
use super::prim::nm_text;
use super::settings::TolDisplay;
use crate::iso286::{self, Class, Table};

/// The most a deviation given by hand may be, in nanometres (10 mm).
pub const MAX_DEVIATION_NM: i64 = 10_000_000;

/// The tolerance of a dimension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tol {
    /// A class of ISO 286 (`H7`, `g6`) or a fit (`H7/g6`, a hole over a shaft), as written.
    Class(String),
    /// The upper and the lower deviation from the nominal size, in nanometres.
    Dev {
        /// The upper deviation.
        upper_nm: i64,
        /// The lower deviation.
        lower_nm: i64,
    },
}

/// The deviations of a tolerance at a size: one pair, or the hole and then the shaft of a fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The class as written, when the tolerance has one.
    pub class: Option<String>,
    /// `(upper, lower)` in nanometres: one pair, or two for a fit (hole, shaft).
    pub devs: Vec<(i64, i64)>,
}

/// Whether a tolerance may be given to a dimension of this kind, without looking at the size.
/// The error is the reason, in English.
pub fn check(tol: &Tol, kind: DimKind) -> Result<(), String> {
    if kind == DimKind::Angle {
        return Err("an angle takes no tolerance of lengths".to_string());
    }
    match tol {
        Tol::Dev { upper_nm, lower_nm } => {
            if lower_nm > upper_nm {
                return Err(format!(
                    "the upper deviation {upper_nm} nm is below the lower one {lower_nm} nm"
                ));
            }
            if upper_nm.abs() > MAX_DEVIATION_NM || lower_nm.abs() > MAX_DEVIATION_NM {
                return Err("a deviation is within 10 mm".to_string());
            }
            Ok(())
        }
        Tol::Class(text) => match text.split_once('/') {
            None => Class::parse(text).map(|_| ()),
            Some((hole, shaft)) => {
                let (h, s) = (Class::parse(hole)?, Class::parse(shaft)?);
                if !h.is_hole() || s.is_hole() {
                    return Err(format!(
                        "{text}: a fit is a hole (capital letters) over a shaft (small letters)"
                    ));
                }
                Ok(())
            }
        },
    }
}

/// The deviations of a tolerance at a size in nanometres. A size the table has no range for, a
/// size that is not above nothing and a class the table has no value for are errors.
pub fn apply(tol: &Tol, size_nm: i64, table: &Table) -> Result<Applied, String> {
    match tol {
        Tol::Dev { upper_nm, lower_nm } => Ok(Applied {
            class: None,
            devs: vec![(*upper_nm, *lower_nm)],
        }),
        Tol::Class(text) => {
            if size_nm <= 0 {
                return Err(format!("{text}: the size is not above nothing"));
            }
            let size_mm = size_nm as f64 / 1.0e6;
            let one = |c: &str| -> Result<(i64, i64), String> {
                let class = Class::parse(c)?;
                let l = iso286::limits(table, size_mm, &class)?;
                Ok((l.upper_dev_nm, l.lower_dev_nm))
            };
            let devs = match text.split_once('/') {
                None => vec![one(text)?],
                Some((h, s)) => vec![one(h)?, one(s)?],
            };
            Ok(Applied {
                class: Some(text.clone()),
                devs,
            })
        }
    }
}

fn dev_text(v: i64, decimals: u32) -> String {
    if v == 0 {
        "0".to_string()
    } else if v > 0 {
        format!("+{}", nm_text(v, decimals))
    } else {
        format!("-{}", nm_text(-v, decimals))
    }
}

/// A pair of deviations as text: `+0.021/0`, or the plus-minus sign and one number when the two
/// are alike.
pub fn pair_text(upper: i64, lower: i64, decimals: u32) -> String {
    if upper == -lower && upper != 0 {
        format!("\u{b1}{}", nm_text(upper, decimals))
    } else {
        format!(
            "{}/{}",
            dev_text(upper, decimals),
            dev_text(lower, decimals)
        )
    }
}

/// The text of a dimension with its tolerance: the value as measured, then what the style of the
/// settings says (the class, the deviations or both).
pub fn annotate(
    base: &str,
    tol: &Tol,
    size_nm: i64,
    table: &Table,
    style: TolDisplay,
    decimals: u32,
) -> Result<String, String> {
    let a = apply(tol, size_nm, table)?;
    let devs = a
        .devs
        .iter()
        .map(|(u, l)| pair_text(*u, *l, decimals))
        .collect::<Vec<_>>()
        .join(" | ");
    Ok(match (&a.class, style) {
        (None, _) => format!("{base} {devs}"),
        (Some(c), TolDisplay::Class) => format!("{base} {c}"),
        (Some(_), TolDisplay::Deviation) => format!("{base} {devs}"),
        (Some(c), TolDisplay::Both) => format!("{base} {c} ({devs})"),
    })
}
