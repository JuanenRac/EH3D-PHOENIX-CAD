// SPDX-License-Identifier: GPL-3.0-or-later
//! The conversions that the units of [`crate::units`] do not write yet: a length as inches in the
//! fractions of a workshop (`1 1/2 in`, `3/16 in`), an angle as degrees, minutes and seconds
//! (`12°30'15"`), as radians or as gradians, and the one function behind the command `convert` that
//! reads a value and writes it in another unit.
//!
//! The fractions are the nearest multiple of one over a power of two (up to 128), rounded half
//! away from zero and written in their lowest terms, so [`crate::units::parse_length`] reads the
//! text back to a length that is no farther than half a step from the one given. Degrees,
//! minutes and seconds are read with the signs `°`, `'` and `"` (or the letters `d`, `m` and `s`),
//! each at most once and in that order, with the sign of the whole angle in front.

use crate::units::{
    div_round_half_away, format_angle, format_length, parse_angle, parse_length, parse_rational,
    Angle, Unit,
};
use crate::{CadError, Length};

const NM_PER_INCH: i128 = 25_400_000;

/// The most that the denominator of a fraction of an inch can be.
pub const MAX_DENOMINATOR: u32 = 128;

fn gcd(a: u128, b: u128) -> u128 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// A length in inches as a whole number and a fraction of the given denominator (a power of two
/// from 1 to 128), in lowest terms: `1 1/2 in`, `3/16 in`, `-2 in`, `0 in`. The fraction is the
/// nearest one, rounded half away from zero.
///
/// ```
/// use eh3d_phoenix_cad::convert::format_inches_fraction;
/// use eh3d_phoenix_cad::Length;
/// let l = Length::new(38_100_000).unwrap();
/// assert_eq!(format_inches_fraction(l, 16).unwrap(), "1 1/2 in");
/// ```
pub fn format_inches_fraction(length: Length, denominator: u32) -> Result<String, CadError> {
    if denominator == 0 || denominator > MAX_DENOMINATOR || !denominator.is_power_of_two() {
        return Err(CadError::ValueOutOfRange("denominator of a fraction"));
    }
    let den = i128::from(denominator);
    let steps = div_round_half_away(i128::from(length.get()) * den, NM_PER_INCH)?;
    let sign = if steps < 0 { "-" } else { "" };
    let magnitude = steps.unsigned_abs();
    let whole = magnitude / u128::from(denominator);
    let rest = magnitude % u128::from(denominator);
    if rest == 0 {
        return Ok(format!("{sign}{whole} in"));
    }
    let g = gcd(rest, u128::from(denominator));
    let (num, den) = (rest / g, u128::from(denominator) / g);
    Ok(if whole == 0 {
        format!("{sign}{num}/{den} in")
    } else {
        format!("{sign}{whole} {num}/{den} in")
    })
}

/// Whether a text is written with the signs of degrees, minutes or seconds that [`parse_dms`]
/// reads (the signs `°`, `'` and `"`, or a `d` between two digits); a bare number of degrees, with
/// or without `deg`, is read by [`parse_angle`].
pub fn looks_like_dms(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    text.contains(['\u{b0}', '\'', '"', '\u{2032}', '\u{2033}'])
        || chars
            .windows(3)
            .any(|w| w[0].is_ascii_digit() && w[1] == 'd' && w[2].is_ascii_digit())
}

/// Reads an angle written as degrees, minutes and seconds: `12°30'15"`, `12°30'`, `-0°0'30.5"`,
/// `30'`, `15"` or `12d30m15s`. Each part is written once, in that order, and only the last may
/// have decimals or a fraction; a part may be larger than 60. The result is rounded half away from
/// zero to the nano-degree. Anything else is refused with the unit error.
///
/// ```
/// use eh3d_phoenix_cad::convert::parse_dms;
/// assert_eq!(parse_dms("12°30'").unwrap().get(), 12_500_000_000);
/// assert_eq!(parse_dms("-0°0'36\"").unwrap().get(), -10_000_000);
/// ```
pub fn parse_dms(text: &str) -> Result<Angle, CadError> {
    let bad = || CadError::UnitText(text.to_string());
    let t = text.trim();
    let (negative, body) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    // the total in seconds as a fraction, and the rank of the last part read
    let (mut total_num, mut total_den) = (0i128, 1i128);
    let mut rank = 0u8;
    let mut number = String::new();
    for c in body.chars() {
        let part = match c {
            '\u{b0}' | 'd' => Some((1u8, 3600i128)),
            '\'' | '\u{2032}' | 'm' => Some((2, 60)),
            '"' | '\u{2033}' | 's' => Some((3, 1)),
            _ => None,
        };
        let Some((this, seconds)) = part else {
            number.push(c);
            continue;
        };
        let text_of_number = number.trim().to_owned();
        number.clear();
        if this <= rank
            || !text_of_number.starts_with(|d: char| d.is_ascii_digit() || d == '.' || d == ',')
        {
            return Err(bad());
        }
        let (num, den) = parse_rational(&text_of_number).ok_or_else(bad)?;
        // total + num * seconds / den
        let add = num.checked_mul(seconds).ok_or_else(bad)?;
        total_num = total_num
            .checked_mul(den)
            .and_then(|a| add.checked_mul(total_den).and_then(|b| a.checked_add(b)))
            .ok_or_else(bad)?;
        total_den = total_den.checked_mul(den).ok_or_else(bad)?;
        rank = this;
    }
    if rank == 0 || !number.trim().is_empty() {
        return Err(bad());
    }
    // seconds to nano-degrees: one second is 1e9 / 3600 = 2 500 000 / 9 of them
    let ndeg = div_round_half_away(
        total_num.checked_mul(2_500_000).ok_or_else(bad)?,
        total_den.checked_mul(9).ok_or_else(bad)?,
    )?;
    let ndeg = if negative { -ndeg } else { ndeg };
    Angle::new(i64::try_from(ndeg).map_err(|_| bad())?)
}

/// Writes an angle as degrees, minutes and seconds with a number of decimals of the second (at
/// most 6): `12°30'15"`, `-0°00'30.5"`. The minutes and the whole seconds have two digits, and
/// [`parse_dms`] reads the text back; with six decimals it gives the same angle.
///
/// ```
/// use eh3d_phoenix_cad::convert::format_dms;
/// use eh3d_phoenix_cad::units::Angle;
/// assert_eq!(format_dms(Angle::new(12_504_166_667).unwrap(), 0), "12°30'15\"");
/// ```
pub fn format_dms(angle: Angle, decimals: u32) -> String {
    let decimals = decimals.min(6);
    let scale = 10u128.pow(decimals);
    let n = angle.get();
    let sign = if n < 0 { "-" } else { "" };
    // |ndeg| * 3600 / 1e9 seconds, in units of 1/scale second, rounded half up
    let total = (n.unsigned_abs() as u128 * 3600 * scale + 500_000_000) / 1_000_000_000;
    let per_degree = 3600 * scale;
    let degrees = total / per_degree;
    let rest = total % per_degree;
    let minutes = rest / (60 * scale);
    let seconds = rest % (60 * scale);
    let (whole, frac) = (seconds / scale, seconds % scale);
    if decimals == 0 {
        format!("{sign}{degrees}\u{b0}{minutes:02}'{whole:02}\"")
    } else {
        let width = decimals as usize;
        format!("{sign}{degrees}\u{b0}{minutes:02}'{whole:02}.{frac:0width$}\"")
    }
}

/// Reads `value` and writes it in the unit `to`: a length in `nm`, `um`, `mm`, `cm`, `m`, `in` or
/// `mil` (with `fraction`, in inches, as a mixed number of that denominator) or an angle in `deg`,
/// `dms`, `rad` (radians) or `gon` (gradians, 400 to a turn), the last two with `decimals`. A value with the signs of minutes and seconds is an angle in degrees, minutes and
/// seconds; a length needs its unit, an angle in degrees may go without.
///
/// ```
/// use eh3d_phoenix_cad::convert::convert;
/// assert_eq!(convert("1 1/2 in", "mm", 3, None).unwrap(), "38.100 mm");
/// assert_eq!(convert("38.1 mm", "in", 3, Some(16)).unwrap(), "1 1/2 in");
/// assert_eq!(convert("12.5", "dms", 0, None).unwrap(), "12°30'00\"");
/// assert_eq!(convert("180", "rad", 5, None).unwrap(), "3.14159 rad");
/// ```
pub fn convert(
    value: &str,
    to: &str,
    decimals: u32,
    fraction: Option<u32>,
) -> Result<String, CadError> {
    let angle = || {
        if looks_like_dms(value) {
            parse_dms(value)
        } else {
            parse_angle(value).or_else(|_| parse_dms(value))
        }
    };
    match to {
        "dms" => {
            if fraction.is_some() {
                return Err(CadError::Config("--fraction needs --to in".into()));
            }
            Ok(format_dms(angle()?, decimals))
        }
        "deg" => {
            if fraction.is_some() {
                return Err(CadError::Config("--fraction needs --to in".into()));
            }
            Ok(format!("{} deg", format_angle(angle()?)))
        }
        "rad" | "gon" => {
            if fraction.is_some() {
                return Err(CadError::Config("--fraction needs --to in".into()));
            }
            let degrees = angle()?.get() as f64 / 1.0e9;
            let (value, unit) = if to == "rad" {
                (degrees * (std::f64::consts::PI / 180.0), "rad")
            } else {
                (degrees * (10.0 / 9.0), "gon")
            };
            let decimals = decimals.min(12) as usize;
            let text = format!("{value:.decimals$}");
            // a value that rounds to zero has no minus sign
            let text =
                if text.starts_with('-') && text.trim_start_matches(['-', '0', '.']).is_empty() {
                    text[1..].to_owned()
                } else {
                    text
                };
            Ok(format!("{text} {unit}"))
        }
        other => {
            let unit = Unit::from_suffix(other).ok_or_else(|| CadError::UnitText(other.into()))?;
            let length = parse_length(value)?;
            match fraction {
                Some(den) if unit == Unit::Inch => format_inches_fraction(length, den),
                Some(_) => Err(CadError::Config("--fraction needs --to in".into())),
                None => Ok(format_length(length, unit, decimals)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nm(n: i64) -> Length {
        Length::new(n).unwrap()
    }

    #[test]
    fn inches_are_written_as_a_whole_number_and_a_fraction_in_lowest_terms() {
        let cases = [
            (0, 16, "0 in"),
            (25_400_000, 16, "1 in"),
            (38_100_000, 16, "1 1/2 in"),
            (4_762_500, 16, "3/16 in"),
            (-4_762_500, 16, "-3/16 in"),
            (-38_100_000, 2, "-1 1/2 in"),
            (63_500_000, 8, "2 1/2 in"),
            // 12.7 mm is half an inch, however fine the fraction
            (12_700_000, 128, "1/2 in"),
            // half an inch to the nearest whole inch rounds away from zero
            (12_700_000, 1, "1 in"),
            (-12_700_000, 1, "-1 in"),
        ];
        for (n, den, text) in cases {
            assert_eq!(
                format_inches_fraction(nm(n), den).unwrap(),
                text,
                "{n} {den}"
            );
        }
    }

    #[test]
    fn a_length_between_two_steps_goes_to_the_nearer_and_half_a_step_goes_away_from_zero() {
        // a sixteenth is 1 587 500 nm; a thirty-second is 793 750 nm
        assert_eq!(format_inches_fraction(nm(793_750), 16).unwrap(), "1/16 in");
        assert_eq!(format_inches_fraction(nm(793_749), 16).unwrap(), "0 in");
        assert_eq!(
            format_inches_fraction(nm(-793_750), 16).unwrap(),
            "-1/16 in"
        );
        assert_eq!(format_inches_fraction(nm(-793_749), 16).unwrap(), "0 in");
    }

    #[test]
    fn a_denominator_that_is_not_a_power_of_two_up_to_128_is_refused() {
        for bad in [0, 3, 6, 10, 100, 129, 256] {
            let e = format_inches_fraction(nm(1), bad).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-007", "{bad}");
        }
        for good in [1, 2, 4, 8, 16, 32, 64, 128] {
            assert!(format_inches_fraction(nm(1), good).is_ok());
        }
    }

    #[test]
    fn the_fraction_is_read_back_by_the_length_parser() {
        for (n, den) in [
            (38_100_000, 16),
            (4_762_500, 16),
            (-63_500_000, 4),
            (1_000_000, 64),
        ] {
            let text = format_inches_fraction(nm(n), den).unwrap();
            let back = parse_length(&text).unwrap().get();
            let step = NM_PER_INCH as i64 / i64::from(den);
            assert!((back - n).abs() <= step / 2 + 1, "{text}: {back} {n}");
            assert_eq!(back % step, 0, "{text}");
        }
    }

    #[test]
    fn degrees_minutes_and_seconds_are_read_in_every_form_and_in_order() {
        let cases = [
            ("12°30'15\"", 12_504_166_667),
            ("12°30'", 12_500_000_000),
            ("12d30m15s", 12_504_166_667),
            ("-12°30'", -12_500_000_000),
            ("+0°0'36\"", 10_000_000),
            ("30'", 500_000_000),
            ("15\"", 4_166_667),
            ("90'", 1_500_000_000),
            ("0°0'0.5\"", 138_889),
            ("12° 30' 15\"", 12_504_166_667),
            ("10,5°", 10_500_000_000),
            ("1/2°", 500_000_000),
            ("0°0'1/2\"", 138_889),
        ];
        for (text, ndeg) in cases {
            assert_eq!(parse_dms(text).unwrap().get(), ndeg, "{text}");
        }
    }

    #[test]
    fn degrees_minutes_and_seconds_that_are_out_of_order_repeated_or_empty_are_refused() {
        for bad in [
            "",
            "12",
            "°",
            "'",
            "12'30°",
            "12°12°",
            "30\"15'",
            "12°30'15\"7",
            "a°",
            "12°-30'",
            "--12°",
            "12°+30'",
            "1e3°",
            "12 30'",
            "12°30'15\"s",
        ] {
            let e = parse_dms(bad).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-008", "{bad:?}");
        }
        // one that is out of the range of the angles
        assert!(parse_dms("99999999999°").is_err());
    }

    #[test]
    fn an_angle_is_written_as_degrees_minutes_and_seconds_and_read_back() {
        let a = |n: i64| Angle::new(n).unwrap();
        assert_eq!(format_dms(a(12_504_166_667), 0), "12°30'15\"");
        assert_eq!(format_dms(a(0), 0), "0°00'00\"");
        assert_eq!(format_dms(a(-10_000_000), 1), "-0°00'36.0\"");
        assert_eq!(format_dms(a(138_889), 1), "0°00'00.5\"");
        assert_eq!(format_dms(a(359_999_999_999), 0), "360°00'00\"");
        assert_eq!(format_dms(a(90_000_000_000), 3), "90°00'00.000\"");
        // more than six decimals are six
        assert_eq!(format_dms(a(1), 9), format_dms(a(1), 6));
        for n in [0, 1, -1, 10_000_000, 12_504_166_667, -359_999_999_999] {
            assert_eq!(parse_dms(&format_dms(a(n), 6)).unwrap().get(), n, "{n}");
        }
    }

    #[test]
    fn the_command_reads_one_unit_and_writes_another() {
        assert_eq!(convert("1 1/2 in", "mm", 3, None).unwrap(), "38.100 mm");
        assert_eq!(convert("25.4mm", "in", 4, None).unwrap(), "1.0000 in");
        assert_eq!(convert("1.5 in", "mil", 0, None).unwrap(), "1500 mil");
        assert_eq!(convert("38.1 mm", "in", 3, Some(16)).unwrap(), "1 1/2 in");
        assert_eq!(convert("12.5", "dms", 0, None).unwrap(), "12°30'00\"");
        assert_eq!(convert("12.5 deg", "dms", 1, None).unwrap(), "12°30'00.0\"");
        assert_eq!(convert("12°30'", "deg", 0, None).unwrap(), "12.5 deg");
        assert_eq!(convert("12d30m", "deg", 0, None).unwrap(), "12.5 deg");
        assert_eq!(convert("-45", "deg", 0, None).unwrap(), "-45 deg");
        assert_eq!(convert("180", "rad", 5, None).unwrap(), "3.14159 rad");
        assert_eq!(convert("90", "rad", 4, None).unwrap(), "1.5708 rad");
        assert_eq!(convert("-90 deg", "rad", 3, None).unwrap(), "-1.571 rad");
        assert_eq!(convert("90", "gon", 1, None).unwrap(), "100.0 gon");
        assert_eq!(convert("360", "gon", 0, None).unwrap(), "400 gon");
        assert_eq!(convert("12°30'", "gon", 3, None).unwrap(), "13.889 gon");
        assert_eq!(
            convert("0.0000000001", "rad", 3, None).unwrap(),
            "0.000 rad"
        );
        assert_eq!(
            convert("-0.0000000001", "rad", 3, None).unwrap(),
            "0.000 rad"
        );
    }

    #[test]
    fn the_command_refuses_what_it_cannot_convert() {
        assert_eq!(
            convert("1 in", "ft", 3, None).unwrap_err().code(),
            "PHOENIX-CAD-008"
        );
        assert_eq!(
            convert("1", "mm", 3, None).unwrap_err().code(),
            "PHOENIX-CAD-008"
        );
        assert_eq!(
            convert("1 mm", "mm", 3, Some(16)).unwrap_err().code(),
            "PHOENIX-CAD-011"
        );
        assert_eq!(
            convert("1 mm", "deg", 3, Some(16)).unwrap_err().code(),
            "PHOENIX-CAD-011"
        );
        assert_eq!(
            convert("1", "rad", 3, Some(16)).unwrap_err().code(),
            "PHOENIX-CAD-011"
        );
        assert_eq!(
            convert("1 mm", "dms", 0, None).unwrap_err().code(),
            "PHOENIX-CAD-008"
        );
    }

    #[test]
    fn what_counts_as_degrees_minutes_and_seconds() {
        for yes in ["12°30'", "30'", "15\"", "12d30m", "12d30m15s"] {
            assert!(looks_like_dms(yes), "{yes}");
        }
        for no in [
            "12", "12.5", "45 deg", "-45", "1/2", "12 mm", "1m", "5s", "2d",
        ] {
            assert!(!looks_like_dms(no), "{no}");
        }
    }

    /// A small generator with a fixed seed.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 20
        }
    }

    #[test]
    fn random_lengths_and_angles_are_written_and_read_back_within_half_a_step_with_a_fixed_seed() {
        let mut rng = Lcg(0xc0de_cad2);
        for _ in 0..400 {
            let n = (rng.next() % 2_000_000_000) as i64 - 1_000_000_000;
            for den in [1, 2, 4, 8, 16, 32, 64, 128] {
                let text = format_inches_fraction(nm(n), den).unwrap();
                let back = parse_length(&text).unwrap().get();
                let step = NM_PER_INCH as i64 / i64::from(den);
                assert!((back - n).abs() <= step / 2 + 1, "{n} {den} {text}");
            }
            let a = (rng.next() % 2_000_000_000_000) as i64 - 1_000_000_000_000;
            let text = format_dms(Angle::new(a).unwrap(), 6);
            assert_eq!(parse_dms(&text).unwrap().get(), a, "{a} {text}");
            let coarse = format_dms(Angle::new(a).unwrap(), 0);
            let back = parse_dms(&coarse).unwrap().get();
            // half a second is 138 889 nano-degrees
            assert!((back - a).abs() <= 138_889, "{a} {coarse}");
        }
    }
}
