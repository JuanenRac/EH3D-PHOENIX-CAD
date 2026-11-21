// SPDX-License-Identifier: GPL-3.0-or-later
//! Points typed as text, the way a person types them into a sketch: `10,5` (absolute), `@3,-2`
//! (relative to the last point), `20<45` (distance and angle from the origin) and `@20<45`
//! (distance and angle from the last point).
//!
//! * A number without a unit is in millimetres; `10 mm`, `1/4 in`, `500 um` and the other units of
//!   [`parse_length`](crate::units::parse_length) are read as they are anywhere else.
//! * The two numbers of `x,y` are separated by a comma, or by a semicolon when a comma is also the
//!   decimal mark (`0,5; 2,5`); a text with more than one comma and no semicolon is ambiguous and
//!   is refused.
//! * An angle is in degrees counter-clockwise from the x axis (`parse_angle`). A negative distance
//!   points the opposite way. The answer is rounded half away from zero to the nanometre, and a
//!   quarter turn (0, 90, 180, 270 degrees and their repeats) is exact, with no trigonometry.
//! * A point outside the range of lengths is refused. Every failure is
//!   `PHOENIX-CAD-008` (text that is not understood) or `PHOENIX-CAD-001` (out of range) and
//!   leaves the last point as it was.

use crate::units::{parse_angle, parse_length};
use crate::{CadError, Length};

/// What a typed point is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    /// The origin of the sketch.
    Origin,
    /// The last point.
    Last,
}

/// How the point is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// Two coordinates.
    Cartesian,
    /// A distance and an angle.
    Polar,
}

/// A typed point read, before it is placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Typed {
    /// What it is measured from.
    pub base: Base,
    /// How it is written.
    pub form: Form,
    /// The offset from there, in nanometres: the two coordinates, or the distance and angle
    /// turned into coordinates.
    pub offset: [i64; 2],
}

fn bad(text: &str) -> CadError {
    CadError::UnitText(text.to_string())
}

/// A length where a bare number is in millimetres.
fn length(text: &str) -> Result<i64, CadError> {
    let t = text.trim();
    if t.is_empty() {
        return Err(bad(text));
    }
    if t.chars().any(char::is_alphabetic) {
        Ok(parse_length(t)?.get())
    } else {
        Ok(parse_length(&format!("{t} mm"))?.get())
    }
}

fn half_away(value: f64) -> Option<i64> {
    let r = value.round();
    if r.is_finite() && r.abs() < 9.0e18 {
        Some(r as i64)
    } else {
        None
    }
}

/// The two coordinates of a distance and an angle (in nano-degrees), rounded to the nanometre.
fn polar(distance: i64, angle_ndeg: i64) -> Result<[i64; 2], CadError> {
    const QUARTER: i64 = 90_000_000_000;
    if angle_ndeg % QUARTER == 0 {
        let q = (angle_ndeg / QUARTER).rem_euclid(4);
        return Ok(match q {
            0 => [distance, 0],
            1 => [0, distance],
            2 => [-distance, 0],
            _ => [0, -distance],
        });
    }
    let radians = (angle_ndeg as f64 / 1.0e9).to_radians();
    let x = half_away(distance as f64 * radians.cos());
    let y = half_away(distance as f64 * radians.sin());
    match (x, y) {
        (Some(x), Some(y)) => Ok([x, y]),
        _ => Err(CadError::LengthOutOfRange(distance)),
    }
}

/// Reads one typed point.
pub fn parse(text: &str) -> Result<Typed, CadError> {
    let t = text.trim();
    let (base, body) = match t.strip_prefix('@') {
        Some(rest) => (Base::Last, rest),
        None => (Base::Origin, t),
    };
    if let Some((distance, angle)) = body.split_once('<') {
        let angle = parse_angle(angle.trim())?.get();
        let distance = length(distance)?;
        return Ok(Typed {
            base,
            form: Form::Polar,
            offset: polar(distance, angle)?,
        });
    }
    let pieces: Vec<&str> = if body.contains(';') {
        body.split(';').collect()
    } else {
        body.split(',').collect()
    };
    match pieces.as_slice() {
        [x, y] => Ok(Typed {
            base,
            form: Form::Cartesian,
            offset: [length(x)?, length(y)?],
        }),
        _ => Err(bad(text)),
    }
}

/// Places a typed point: from the origin as it is, from the last point added to it. The result is
/// in nanometres and inside the range of lengths.
pub fn place(typed: &Typed, last: [i64; 2]) -> Result<[i64; 2], CadError> {
    let base = match typed.base {
        Base::Origin => [0, 0],
        Base::Last => last,
    };
    let mut out = [0i64; 2];
    for i in 0..2 {
        let v = base[i]
            .checked_add(typed.offset[i])
            .ok_or(CadError::Overflow("typed point"))?;
        out[i] = Length::new(v)?.get();
    }
    Ok(out)
}

/// Reads and places a list of typed points one after another, each one measured from the one
/// before (the first from `start`). Nothing is returned if any of them fails.
pub fn resolve(texts: &[&str], start: [i64; 2]) -> Result<Vec<[i64; 2]>, CadError> {
    let mut last = start;
    let mut out = Vec::with_capacity(texts.len());
    for t in texts {
        last = place(&parse(t)?, last)?;
        out.push(last);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
