//! Angles, counts and ratios with their own range, exact integer arithmetic and units of length.
//!
//! Nothing here uses floating point: an angle is a whole number of nano-degrees, a ratio a whole
//! number of parts per million, and every division rounds half away from zero
//! ([`div_round_half_away`]). The same input gives the same output on every platform.

use crate::{CadError, Length};

/// One full turn in nano-degrees.
pub const FULL_TURN_NDEG: i64 = 360_000_000_000;

/// A quarter of a turn in nano-degrees.
pub const QUARTER_TURN_NDEG: i64 = 90_000_000_000;

/// The largest absolute value of an angle, in nano-degrees (a million degrees, about 2778 turns).
pub const ANGLE_LIMIT_NDEG: i64 = 1_000_000_000_000_000;

/// The largest count.
pub const COUNT_LIMIT: u32 = 1_000_000;

/// The largest absolute value of a ratio, in parts per million (a thousand times the whole).
pub const RATIO_LIMIT_PPM: i64 = 1_000_000_000;

/// The value of pi in fixed point with 60 fractional bits.
const PI_Q60: i128 = 3_622_009_729_038_561_421;

/// An angle in integer nano-degrees, strictly inside plus or minus [`ANGLE_LIMIT_NDEG`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Angle(i64);

impl Angle {
    /// Zero.
    pub const ZERO: Angle = Angle(0);

    /// An angle from nano-degrees, refused when it is outside the range.
    pub fn new(ndeg: i64) -> Result<Angle, CadError> {
        if ndeg > -ANGLE_LIMIT_NDEG && ndeg < ANGLE_LIMIT_NDEG {
            Ok(Angle(ndeg))
        } else {
            Err(CadError::AngleOutOfRange(ndeg))
        }
    }

    /// An angle from whole degrees.
    pub fn from_deg(deg: i64) -> Result<Angle, CadError> {
        Angle::new(
            deg.checked_mul(1_000_000_000)
                .ok_or(CadError::AngleOutOfRange(deg))?,
        )
    }

    /// The value in nano-degrees.
    pub fn get(self) -> i64 {
        self.0
    }

    /// The sum, refused when it leaves the range.
    pub fn checked_add(self, other: Angle) -> Result<Angle, CadError> {
        Angle::new(self.0 + other.0)
    }

    /// The difference, refused when it leaves the range.
    pub fn checked_sub(self, other: Angle) -> Result<Angle, CadError> {
        Angle::new(self.0 - other.0)
    }

    /// The same direction inside one turn: zero included, a full turn excluded.
    pub fn normalized(self) -> Angle {
        Angle(self.0.rem_euclid(FULL_TURN_NDEG))
    }

    /// The sine in parts per 10^9 (so `1_000_000_000` is one), computed with integers only and
    /// rounded to the nearest.
    pub fn sin(self) -> i64 {
        let a = self.normalized().0;
        let q = QUARTER_TURN_NDEG;
        let (base, negative) = if a <= q {
            (a, false)
        } else if a <= 2 * q {
            (2 * q - a, false)
        } else if a <= 3 * q {
            (a - 2 * q, true)
        } else {
            (4 * q - a, true)
        };
        let s = sin_first_quadrant(base);
        if negative {
            -s
        } else {
            s
        }
    }

    /// The cosine in parts per 10^9.
    pub fn cos(self) -> i64 {
        Angle(self.normalized().0 + QUARTER_TURN_NDEG).sin()
    }
}

fn mul_q60(a: i128, b: i128) -> i128 {
    (a * b) >> 60
}

/// The sine of an angle between 0 and 90 degrees (in nano-degrees), in parts per 10^9.
fn sin_first_quadrant(ndeg: i64) -> i64 {
    let x = i128::from(ndeg) * PI_Q60 / 180_000_000_000;
    let x2 = mul_q60(x, x);
    let mut term = x;
    let mut sum = x;
    for k in 1..=14i128 {
        term = -mul_q60(term, x2) / ((2 * k) * (2 * k + 1));
        sum += term;
    }
    let nano = (sum * 1_000_000_000 + (1i128 << 59)) >> 60;
    i64::try_from(nano.clamp(0, 1_000_000_000)).unwrap_or(0)
}

/// A number of things (holes in a row, copies of a pattern), from 0 to [`COUNT_LIMIT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Count(u32);

impl Count {
    /// A count, refused when it is negative or above [`COUNT_LIMIT`].
    pub fn new(n: i64) -> Result<Count, CadError> {
        let v = u32::try_from(n).map_err(|_| CadError::ValueOutOfRange("count"))?;
        if v > COUNT_LIMIT {
            return Err(CadError::ValueOutOfRange("count"));
        }
        Ok(Count(v))
    }

    /// The value.
    pub fn get(self) -> u32 {
        self.0
    }
}

/// A ratio in whole parts per million, strictly inside plus or minus [`RATIO_LIMIT_PPM`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ratio(i64);

impl Ratio {
    /// A ratio from parts per million, refused when it is outside the range.
    pub fn new(ppm: i64) -> Result<Ratio, CadError> {
        if ppm > -RATIO_LIMIT_PPM && ppm < RATIO_LIMIT_PPM {
            Ok(Ratio(ppm))
        } else {
            Err(CadError::ValueOutOfRange("ratio"))
        }
    }

    /// The value in parts per million.
    pub fn get(self) -> i64 {
        self.0
    }

    /// The part of a length that this ratio is, rounded half away from zero.
    pub fn of(self, length: Length) -> Result<Length, CadError> {
        scale(length, self.0, 1_000_000)
    }
}

/// `num / den` rounded half away from zero (`5 / 2` is `3`, `-5 / 2` is `-3`, `7 / 3` is `2`).
/// A zero denominator or an overflow is refused.
pub fn div_round_half_away(num: i128, den: i128) -> Result<i128, CadError> {
    if den == 0 {
        return Err(CadError::Overflow("division by zero"));
    }
    let (n, d) = if den < 0 {
        (
            num.checked_neg().ok_or(CadError::Overflow("division"))?,
            den.checked_neg().ok_or(CadError::Overflow("division"))?,
        )
    } else {
        (num, den)
    };
    let q = n / d;
    let r = n % d;
    let ra = r.unsigned_abs();
    let da = d.unsigned_abs();
    if ra >= da - ra {
        q.checked_add(n.signum())
            .ok_or(CadError::Overflow("division"))
    } else {
        Ok(q)
    }
}

/// `length * num / den`, rounded half away from zero, with a 128-bit intermediate product.
pub fn scale(length: Length, num: i64, den: i64) -> Result<Length, CadError> {
    let product = i128::from(length.get())
        .checked_mul(i128::from(num))
        .ok_or(CadError::Overflow("scale"))?;
    let nm = div_round_half_away(product, i128::from(den))?;
    Length::new(i64::try_from(nm).map_err(|_| CadError::Overflow("scale"))?)
}

/// The integer square root: the largest `r` with `r * r <= n`.
pub fn isqrt_u128(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    let bits = 128 - n.leading_zeros();
    let mut x = 1u128 << bits.div_ceil(2);
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// The length of the hypotenuse of two lengths, rounded to the nearest nanometre.
pub fn hypot(a: Length, b: Length) -> Result<Length, CadError> {
    let (a, b) = (i128::from(a.get()), i128::from(b.get()));
    let n = (a * a + b * b).unsigned_abs();
    let r = isqrt_u128(n);
    // (r + 1/2)^2 = r^2 + r + 1/4, so n above r^2 + r rounds up
    let r = if n - r * r > r { r + 1 } else { r };
    Length::new(i64::try_from(r).map_err(|_| CadError::Overflow("hypot"))?)
}

/// The units in which lengths are read and shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Nanometres.
    Nanometre,
    /// Micrometres.
    Micrometre,
    /// Millimetres.
    Millimetre,
    /// Centimetres.
    Centimetre,
    /// Metres.
    Metre,
    /// Inches (exactly 25 400 000 nm).
    Inch,
    /// Thousandths of an inch (exactly 25 400 nm).
    Mil,
}

impl Unit {
    /// How many nanometres one unit is.
    pub fn nanometres(self) -> i128 {
        match self {
            Unit::Nanometre => 1,
            Unit::Micrometre => 1_000,
            Unit::Millimetre => 1_000_000,
            Unit::Centimetre => 10_000_000,
            Unit::Metre => 1_000_000_000,
            Unit::Inch => 25_400_000,
            Unit::Mil => 25_400,
        }
    }

    /// The text written after a number.
    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Nanometre => "nm",
            Unit::Micrometre => "um",
            Unit::Millimetre => "mm",
            Unit::Centimetre => "cm",
            Unit::Metre => "m",
            Unit::Inch => "in",
            Unit::Mil => "mil",
        }
    }

    /// The unit named by a text, or `None`.
    pub fn from_suffix(text: &str) -> Option<Unit> {
        match text {
            "nm" => Some(Unit::Nanometre),
            "um" | "\u{b5}m" => Some(Unit::Micrometre),
            "mm" => Some(Unit::Millimetre),
            "cm" => Some(Unit::Centimetre),
            "m" => Some(Unit::Metre),
            "in" => Some(Unit::Inch),
            "mil" => Some(Unit::Mil),
            _ => None,
        }
    }
}

fn parse_int(s: &str) -> Option<i128> {
    let (negative, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if body.is_empty() || body.len() > 18 || !body.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: i128 = body.parse().ok()?;
    Some(if negative { -v } else { v })
}

/// A number written as `12`, `1.6`, `0,5` or `1/16`, as a numerator and a positive denominator.
fn parse_rational(s: &str) -> Option<(i128, i128)> {
    if let Some((a, b)) = s.split_once('/') {
        let n = parse_int(a.trim())?;
        let d = parse_int(b.trim())?;
        return if d > 0 { Some((n, d)) } else { None };
    }
    let (negative, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let mut int: i128 = 0;
    let mut den: i128 = 1;
    let mut separator = false;
    let mut digits = 0;
    for c in body.chars() {
        match c {
            '0'..='9' => {
                digits += 1;
                if digits > 18 {
                    return None;
                }
                int = int * 10 + i128::from(c.to_digit(10)?);
                if separator {
                    den *= 10;
                }
            }
            '.' | ',' if !separator => separator = true,
            _ => return None,
        }
    }
    if digits == 0 {
        return None;
    }
    Some((if negative { -int } else { int }, den))
}

/// Reads a length written with its unit: `1.6 mm`, `62 mil`, `1/16 in`, `0,5mm`. The decimal
/// separator is a point or a comma. The result is rounded half away from zero to the nanometre.
/// A text without a known unit, or a number that is not understood, is refused.
///
/// ```
/// use eh3d_phoenix_cad::units::parse_length;
/// assert_eq!(parse_length("1/16 in").unwrap().get(), 1_587_500);
/// assert_eq!(parse_length("0,5 mm").unwrap().get(), 500_000);
/// assert_eq!(parse_length("5 km").unwrap_err().code(), "PHOENIX-CAD-008");
/// ```
pub fn parse_length(text: &str) -> Result<Length, CadError> {
    let bad = || CadError::UnitText(text.to_string());
    let t = text.trim();
    let split = t.find(|c: char| c.is_alphabetic()).ok_or_else(bad)?;
    let (number, unit) = t.split_at(split);
    let unit = Unit::from_suffix(unit.trim()).ok_or_else(bad)?;
    let (num, den) = parse_rational(number.trim()).ok_or_else(bad)?;
    let scaled = num.checked_mul(unit.nanometres()).ok_or_else(bad)?;
    let nm = div_round_half_away(scaled, den)?;
    Length::new(i64::try_from(nm).map_err(|_| bad())?)
}

/// Writes a length in a unit with a fixed number of decimals (at most 12), rounded half away
/// from zero: `format_length(1_600_000, Millimetre, 3)` is `1.600 mm`.
pub fn format_length(length: Length, unit: Unit, decimals: u32) -> String {
    let decimals = decimals.min(12);
    let factor = 10i128.pow(decimals);
    let scaled = div_round_half_away(i128::from(length.get()) * factor, unit.nanometres())
        .expect("the unit is never zero and the product fits");
    let sign = if scaled < 0 { "-" } else { "" };
    let magnitude = scaled.unsigned_abs();
    let factor = factor.unsigned_abs();
    let suffix = unit.suffix();
    if decimals == 0 {
        format!("{sign}{magnitude} {suffix}")
    } else {
        let (int, frac) = (magnitude / factor, magnitude % factor);
        let width = decimals as usize;
        format!("{sign}{int}.{frac:0width$} {suffix}")
    }
}

/// Reads an angle written in degrees: `90`, `22.5`, `-45 deg`, `0,5°`. The decimal separator is a
/// point or a comma, a fraction such as `1/3` is allowed, and the result is rounded half away from
/// zero to the nano-degree. A text that is not a number of degrees is refused.
///
/// ```
/// use eh3d_phoenix_cad::units::parse_angle;
/// assert_eq!(parse_angle("22.5").unwrap().get(), 22_500_000_000);
/// assert_eq!(parse_angle("-45 deg").unwrap().get(), -45_000_000_000);
/// assert_eq!(parse_angle("one turn").unwrap_err().code(), "PHOENIX-CAD-008");
/// ```
pub fn parse_angle(text: &str) -> Result<Angle, CadError> {
    let bad = || CadError::UnitText(text.to_string());
    let t = text.trim();
    let number = t
        .strip_suffix('\u{b0}')
        .or_else(|| t.strip_suffix("deg"))
        .unwrap_or(t)
        .trim();
    let (num, den) = parse_rational(number).ok_or_else(bad)?;
    let scaled = num.checked_mul(1_000_000_000).ok_or_else(bad)?;
    let ndeg = div_round_half_away(scaled, den)?;
    Angle::new(i64::try_from(ndeg).map_err(|_| bad())?)
}

/// Writes an angle in degrees with the decimals it needs and no more: `90`, `22.5`, `-0.000000001`.
/// [`parse_angle`] reads the text back to the same angle.
pub fn format_angle(angle: Angle) -> String {
    let n = angle.get();
    let sign = if n < 0 { "-" } else { "" };
    let magnitude = n.unsigned_abs();
    let (int, frac) = (magnitude / 1_000_000_000, magnitude % 1_000_000_000);
    if frac == 0 {
        format!("{sign}{int}")
    } else {
        let digits = format!("{frac:09}");
        format!("{sign}{int}.{}", digits.trim_end_matches('0'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_are_read_and_written_in_degrees_and_read_back_to_the_same_angle() {
        for (text, ndeg) in [
            ("90", 90_000_000_000i64),
            ("22.5", 22_500_000_000),
            ("22,5 deg", 22_500_000_000),
            ("-45 deg", -45_000_000_000),
            ("0.5\u{b0}", 500_000_000),
            ("1/3", 333_333_333),
            ("2/3", 666_666_667),
            ("-1/3", -333_333_333),
            ("0.0000000004", 0),
            ("0.0000000005", 1),
            ("360", 360_000_000_000),
        ] {
            assert_eq!(parse_angle(text).unwrap().get(), ndeg, "{text}");
        }
        for (ndeg, text) in [
            (0i64, "0"),
            (90_000_000_000, "90"),
            (22_500_000_000, "22.5"),
            (-45_000_000_000, "-45"),
            (1, "0.000000001"),
            (-1, "-0.000000001"),
            (333_333_333, "0.333333333"),
            (1_000_000_000_000_000 - 1, "999999.999999999"),
        ] {
            let a = Angle::new(ndeg).unwrap();
            assert_eq!(format_angle(a), text);
            assert_eq!(parse_angle(text).unwrap(), a);
        }
        for text in [
            "", "deg", "ninety", "1e2", "5 turns", "1/0", "1000000", "--3",
        ] {
            assert!(parse_angle(text).is_err(), "{text}");
        }
        assert_eq!(
            parse_angle("1000000").unwrap_err().code(),
            "PHOENIX-CAD-006"
        );
        assert_eq!(parse_angle("ninety").unwrap_err().code(), "PHOENIX-CAD-008");
    }

    #[test]
    fn angles_have_their_own_range_and_normalise_into_one_turn() {
        assert!(Angle::new(ANGLE_LIMIT_NDEG - 1).is_ok());
        assert_eq!(
            Angle::new(ANGLE_LIMIT_NDEG),
            Err(CadError::AngleOutOfRange(ANGLE_LIMIT_NDEG))
        );
        assert!(Angle::from_deg(i64::MAX).is_err());
        let a = Angle::from_deg(-90).unwrap();
        assert_eq!(a.normalized().get(), 270_000_000_000);
        assert_eq!(Angle::from_deg(360).unwrap().normalized(), Angle::ZERO);
        assert_eq!(
            Angle::from_deg(30)
                .unwrap()
                .checked_add(Angle::from_deg(60).unwrap())
                .unwrap(),
            Angle::from_deg(90).unwrap()
        );
        assert_eq!(
            Angle::from_deg(30)
                .unwrap()
                .checked_sub(Angle::from_deg(60).unwrap())
                .unwrap(),
            Angle::from_deg(-30).unwrap()
        );
    }

    #[test]
    fn sine_and_cosine_are_exact_integers_at_the_known_angles() {
        let deg = |d: i64| Angle::from_deg(d).unwrap();
        assert_eq!(deg(0).sin(), 0);
        assert_eq!(deg(30).sin(), 500_000_000);
        assert_eq!(deg(45).sin(), 707_106_781);
        assert_eq!(deg(60).sin(), 866_025_404);
        assert_eq!(deg(90).sin(), 1_000_000_000);
        assert_eq!(deg(180).sin(), 0);
        assert_eq!(deg(270).sin(), -1_000_000_000);
        assert_eq!(deg(-30).sin(), -500_000_000);
        assert_eq!(deg(0).cos(), 1_000_000_000);
        assert_eq!(deg(60).cos(), 500_000_000);
        assert_eq!(deg(180).cos(), -1_000_000_000);
        assert_eq!(deg(390).sin(), deg(30).sin());
    }

    #[test]
    fn counts_and_ratios_have_their_own_range() {
        assert_eq!(Count::new(6).unwrap().get(), 6);
        assert_eq!(Count::new(0).unwrap().get(), 0);
        assert!(Count::new(-1).is_err());
        assert!(Count::new(i64::from(COUNT_LIMIT) + 1).is_err());
        assert!(Count::new(i64::from(COUNT_LIMIT)).is_ok());
        assert!(Ratio::new(RATIO_LIMIT_PPM).is_err());
        let half = Ratio::new(500_000).unwrap();
        assert_eq!(
            half.of(Length::from_mm(3).unwrap()).unwrap(),
            Length::new(1_500_000).unwrap()
        );
        assert_eq!(Ratio::new(-1_000).unwrap().get(), -1_000);
    }

    #[test]
    fn division_rounds_half_away_from_zero() {
        assert_eq!(div_round_half_away(5, 2), Ok(3));
        assert_eq!(div_round_half_away(-5, 2), Ok(-3));
        assert_eq!(div_round_half_away(5, -2), Ok(-3));
        assert_eq!(div_round_half_away(7, 3), Ok(2));
        assert_eq!(div_round_half_away(-7, 3), Ok(-2));
        assert_eq!(div_round_half_away(6, 3), Ok(2));
        assert_eq!(div_round_half_away(0, 5), Ok(0));
        assert!(div_round_half_away(1, 0).is_err());
    }

    #[test]
    fn scaling_uses_a_wide_product_and_stays_in_range() {
        let big = Length::new((1 << 40) - 1).unwrap();
        assert_eq!(scale(big, 1, 1).unwrap(), big);
        assert!(scale(big, 2, 1).is_err());
        assert_eq!(
            scale(Length::from_mm(10).unwrap(), 1, 3).unwrap().get(),
            3_333_333
        );
        assert_eq!(
            scale(Length::from_mm(10).unwrap(), -2, 3).unwrap().get(),
            -6_666_667
        );
        assert!(scale(big, i64::MAX, 1).is_err());
    }

    #[test]
    fn the_integer_square_root_is_exact_and_the_hypotenuse_rounds_to_the_nearest() {
        assert_eq!(isqrt_u128(0), 0);
        assert_eq!(isqrt_u128(1), 1);
        assert_eq!(isqrt_u128(15), 3);
        assert_eq!(isqrt_u128(16), 4);
        assert_eq!(isqrt_u128(u128::MAX), u128::from(u64::MAX));
        let mm = |v: i64| Length::from_mm(v).unwrap();
        assert_eq!(hypot(mm(3), mm(4)).unwrap(), mm(5));
        assert_eq!(hypot(mm(-3), mm(4)).unwrap(), mm(5));
        // sqrt(2) mm is 1.41421356... mm
        assert_eq!(hypot(mm(1), mm(1)).unwrap().get(), 1_414_214);
    }

    #[test]
    fn lengths_are_read_with_their_unit() {
        let nm = |t: &str| parse_length(t).unwrap().get();
        assert_eq!(nm("1.6 mm"), 1_600_000);
        assert_eq!(nm("1,6mm"), 1_600_000);
        assert_eq!(nm("  -0.5 mm "), -500_000);
        assert_eq!(nm("62 mil"), 1_574_800);
        assert_eq!(nm("1 in"), 25_400_000);
        assert_eq!(nm("1/16 in"), 1_587_500);
        assert_eq!(nm("2 cm"), 20_000_000);
        assert_eq!(nm("3 um"), 3_000);
        assert_eq!(nm("3 \u{b5}m"), 3_000);
        assert_eq!(nm("1 m"), 1_000_000_000);
        assert_eq!(nm("0.0000005 mm"), 1);
        // half a nanometre rounds away from zero
        assert_eq!(nm("0.0000015 mm"), 2);
        assert_eq!(nm("-0.0000015 mm"), -2);
    }

    #[test]
    fn unit_texts_that_make_no_sense_are_refused_with_one_code() {
        for text in [
            "", "abc", "5", "5 km", "mm", "1.2.3 mm", "1/0 in", "- mm", "1e3 mm",
        ] {
            let err = parse_length(text).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-008", "{text}");
        }
        assert_eq!(
            parse_length("99999999 m").unwrap_err().code(),
            "PHOENIX-CAD-001"
        );
    }

    #[test]
    fn lengths_are_shown_in_a_chosen_unit_with_fixed_decimals() {
        let l = |nm: i64| Length::new(nm).unwrap();
        assert_eq!(format_length(l(1_600_000), Unit::Millimetre, 3), "1.600 mm");
        assert_eq!(format_length(l(-500_000), Unit::Millimetre, 1), "-0.5 mm");
        assert_eq!(format_length(l(1_574_800), Unit::Mil, 0), "62 mil");
        assert_eq!(format_length(l(25_400_000), Unit::Inch, 2), "1.00 in");
        assert_eq!(format_length(l(1_500), Unit::Micrometre, 1), "1.5 um");
        assert_eq!(format_length(l(-1), Unit::Millimetre, 1), "0.0 mm");
        // what is shown can be read back
        let shown = format_length(l(1_587_500), Unit::Millimetre, 6);
        assert_eq!(parse_length(&shown).unwrap(), l(1_587_500));
    }
}
