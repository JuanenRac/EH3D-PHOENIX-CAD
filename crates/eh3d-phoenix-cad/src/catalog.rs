// SPDX-License-Identifier: GPL-3.0-or-later
//! The catalog of parts: profiles, screws and brackets that are placed in a model.
//!
//! A [`Library`] is a plain text file (`.phoenixlib`) with a version of its own. It lists parts, each
//! with a name, a material (a name of the settings, which hold the density) and the numbers that
//! make its shape. There are three kinds:
//!
//! * a **profile** is an extrusion of a section (a rectangle, solid or hollow with a wall) cut to a
//!   length: the library gives the section, the length it is cut to when nothing else is asked and
//!   the longest bar it comes in;
//! * a **screw** has a metric name (`M3`), a diameter, a length under the head and a head (a
//!   cylinder, a countersunk cone or a hexagon);
//! * a **bracket** is an angle of two legs of one width and one thickness, with a hole in each leg.
//!
//! ```text
//! phoenix-library 1
//! name chassis-basic
//! part bar-20x20 profile aluminium 20000000 20000000 0 100000000 2000000000
//! part bolt-m3x10 screw steel M3 3000000 10000000 cylinder 5500000 3000000
//! part angle-30 bracket aluminium 30000000 30000000 20000000 3000000 4500000
//! ```
//!
//! Lengths are whole nanometres. Empty lines and lines that start with `#` are ignored; the text
//! written is always the same for the same library. A file of a newer version, a line that is not
//! understood, a part that is not valid and a file larger than the settings allow are refused with
//! `PHOENIX-CAD-044` and the number of the line.
//!
//! A [`PartInstance`] is a part placed in the model: the smallest corner of its bounding box (the
//! position), one of the 24 turns that keep the axes on the axes ([`Orientation`]: the axis the
//! length of the part runs along and a roll in quarter turns) and, for a profile, the length it is
//! cut to. The instance keeps a copy of the part it came from, so a project never depends on the
//! file of the library it was made with. Everything is whole nanometres: the volumes of a profile
//! and a bracket are exact, those of round and hexagonal shapes use the ratios [`PI_NUM`] /
//! [`PI_DEN`] and [`SQRT3_NUM`] / [`SQRT3_DEN`] (twelve decimals).
//!
//! [`bill`] adds up the instances into the lines of a bill of materials (the same part cut to the same
//! length is one line), with the volume of the material and the mass for the density of its material, and
//! [`cut_list`] gives the lengths each profile is cut to.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use crate::config::Config;
use crate::datum::Rotation;
use crate::enclosure::Id;
use crate::mass::{cylinder_volume_nm3, mass_micrograms, PI_DEN, PI_NUM};
use crate::scad::mm_text;
use crate::store;
use crate::units::div_round_half_away;
use crate::{CadError, Length};

/// The version of the library file that this program writes and reads.
pub const FILE_VERSION: u32 = 1;

/// The extension of a library file.
pub const EXTENSION: &str = "phoenixlib";

const HEADER: &str = "phoenix-library";

/// The numerator of the ratio used for the square root of three (twelve decimals).
pub const SQRT3_NUM: i128 = 1_732_050_807_569;

/// The denominator of the ratio used for the square root of three.
pub const SQRT3_DEN: i128 = 1_000_000_000_000;

/// The limits of the catalog, read from the settings file (`[catalog]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogLimits {
    /// The most bytes of a library file.
    pub max_bytes: usize,
    /// The most parts of one library.
    pub max_parts: usize,
    /// The longest name of a part, a library or a material, in bytes.
    pub max_name_bytes: usize,
    /// The largest size of any one dimension of a part, and the farthest a part may be placed.
    pub max_dimension: Length,
    /// The most instances in one model.
    pub max_instances: usize,
    /// The shortest cut of a profile.
    pub min_cut: Length,
    /// Whether two profiles whose bounding boxes share volume are refused.
    pub refuse_overlap: bool,
}

impl CatalogLimits {
    /// The most that the size of a file can be set to.
    pub const BYTES_CAP: u32 = 1 << 26;
    /// The most that the number of parts can be set to.
    pub const PARTS_CAP: u32 = 100_000;
    /// The most that the length of a name can be set to.
    pub const NAME_CAP: u32 = 256;
    /// The most that the number of instances can be set to.
    pub const INSTANCES_CAP: u32 = 100_000;
    /// The most that the largest dimension can be set to, in nanometres (ten metres).
    pub const DIMENSION_CAP_NM: i64 = 10_000_000_000;

    /// The limits of the shipped settings.
    pub fn shipped() -> CatalogLimits {
        crate::config::DesignLimits::shipped().catalog
    }

    /// Reads and checks the keys of `[catalog]`.
    pub fn from_config(c: &Config) -> Result<CatalogLimits, CadError> {
        let bounded = |key: &str, cap: u32| -> Result<usize, CadError> {
            let v = c.number(key)?;
            if v == 0 || v > cap {
                return Err(CadError::Config(format!("{key} must be from 1 to {cap}")));
            }
            usize::try_from(v).map_err(|_| CadError::Config(format!("{key} is too large")))
        };
        let positive = |key: &str, cap: i64| -> Result<Length, CadError> {
            let v = c.length(key)?;
            if v.get() <= 0 || v.get() > cap {
                return Err(CadError::Config(format!(
                    "{key} must be above 0 and at most {} mm",
                    cap / 1_000_000
                )));
            }
            Ok(v)
        };
        let overlap = c.number("catalog.refuse_profile_overlap")?;
        if overlap > 1 {
            return Err(CadError::Config(
                "catalog.refuse_profile_overlap must be 0 or 1".into(),
            ));
        }
        Ok(CatalogLimits {
            max_bytes: bounded("catalog.max_bytes", Self::BYTES_CAP)?,
            max_parts: bounded("catalog.max_parts", Self::PARTS_CAP)?,
            max_name_bytes: bounded("catalog.max_name_bytes", Self::NAME_CAP)?,
            max_dimension: positive("catalog.max_dimension", Self::DIMENSION_CAP_NM)?,
            max_instances: bounded("catalog.max_instances", Self::INSTANCES_CAP)?,
            min_cut: positive("catalog.min_cut", Self::DIMENSION_CAP_NM)?,
            refuse_overlap: overlap == 1,
        })
    }
}

fn library_error(line: usize, why: impl AsRef<str>) -> CadError {
    CadError::Library(format!("line {line}: {}", why.as_ref()))
}

fn part_error(why: impl AsRef<str>) -> CadError {
    CadError::Part(why.as_ref().to_string())
}

/// Whether a text is a word of the catalog: letters, digits, `.`, `_` and `-`, so that it needs no
/// quoting in the text of a line.
pub fn is_word(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

// ---------------------------------------------------------------------------------------------
// Parts
// ---------------------------------------------------------------------------------------------

/// The kind of a part, in the order of the bill of materials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// An extrusion of a section, cut to a length.
    Profile,
    /// A screw.
    Screw,
    /// An angle bracket.
    Bracket,
}

impl Kind {
    /// The three kinds in their fixed order.
    pub const ALL: [Kind; 3] = [Kind::Profile, Kind::Screw, Kind::Bracket];

    /// The word of the files.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Profile => "profile",
            Kind::Screw => "screw",
            Kind::Bracket => "bracket",
        }
    }

    /// The kind with a word of the files.
    pub fn from_name(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// How many words follow the name, the kind and the material in the text of a part.
    fn numbers(self) -> usize {
        match self {
            Kind::Profile | Kind::Bracket => 5,
            Kind::Screw => 6,
        }
    }
}

/// The shape of the head of a screw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Head {
    /// A cylinder (socket, cheese or pan head, taken as a cylinder): its diameter and height.
    Cylinder,
    /// A flat countersunk head: a cone from the head diameter down to the shank over the height.
    Countersunk,
    /// A hexagon: the head diameter is the distance across the flats.
    Hex,
}

impl Head {
    /// The word of the files.
    pub fn name(self) -> &'static str {
        match self {
            Head::Cylinder => "cylinder",
            Head::Countersunk => "countersunk",
            Head::Hex => "hex",
        }
    }

    /// The head with a word of the files.
    pub fn from_name(name: &str) -> Option<Head> {
        [Head::Cylinder, Head::Countersunk, Head::Hex]
            .into_iter()
            .find(|h| h.name() == name)
    }
}

/// The numbers that make the shape of a part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// An extrusion of a rectangular section.
    Profile {
        /// The section along the local `x`.
        width: Length,
        /// The section along the local `y`.
        height: Length,
        /// The wall of a hollow section; zero is a solid bar.
        wall: Length,
        /// The length it is cut to when nothing else is asked.
        default_cut: Length,
        /// The longest bar it comes in, and so the longest cut.
        max_cut: Length,
    },
    /// A screw.
    Screw {
        /// The metric name (`M3`).
        metric: String,
        /// The diameter of the shank.
        diameter: Length,
        /// The length under the head.
        length: Length,
        /// The shape of the head.
        head: Head,
        /// The diameter of the head (across the flats for a hexagon).
        head_diameter: Length,
        /// The height of the head.
        head_height: Length,
    },
    /// An angle of two legs.
    Bracket {
        /// The length of the leg along the local `x`, measured from the outside of the corner.
        leg_a: Length,
        /// The length of the leg along the local `z`, measured from the outside of the corner.
        leg_b: Length,
        /// The width of both legs along the local `y`.
        width: Length,
        /// The thickness of both legs.
        thickness: Length,
        /// The diameter of the hole in each leg; zero is no hole.
        hole_diameter: Length,
    },
}

/// A part of a library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// The name (a word of the catalog), which identifies the part in its library.
    pub name: String,
    /// The material: a name of the settings, which hold its density.
    pub material: String,
    /// The shape.
    pub shape: Shape,
}

/// A length written as a canonical decimal number of nanometres, from zero up.
fn length_word(word: &str, what: &str) -> Result<Length, String> {
    let nm = word
        .parse::<i64>()
        .ok()
        .filter(|v| *v >= 0 && v.to_string() == word)
        .ok_or_else(|| format!("the {what} is not a whole number of nanometres"))?;
    Length::new(nm).map_err(|_| format!("the {what} is outside the range of lengths"))
}

/// A length written as a canonical decimal number of nanometres, of any sign.
fn signed_word(word: &str, what: &str) -> Result<Length, String> {
    let nm = word
        .parse::<i64>()
        .ok()
        .filter(|v| v.to_string() == word)
        .ok_or_else(|| format!("the {what} is not a whole number of nanometres"))?;
    Length::new(nm).map_err(|_| format!("the {what} is outside the range of lengths"))
}

impl Part {
    /// The kind of the part.
    pub fn kind(&self) -> Kind {
        match self.shape {
            Shape::Profile { .. } => Kind::Profile,
            Shape::Screw { .. } => Kind::Screw,
            Shape::Bracket { .. } => Kind::Bracket,
        }
    }

    /// The words of the part in a line of a file: the name, the kind, the material and the numbers.
    pub fn words(&self) -> Vec<String> {
        let mut out = vec![
            self.name.clone(),
            self.kind().name().to_string(),
            self.material.clone(),
        ];
        let nm = |l: &Length| l.get().to_string();
        match &self.shape {
            Shape::Profile {
                width,
                height,
                wall,
                default_cut,
                max_cut,
            } => out.extend([width, height, wall, default_cut, max_cut].map(nm)),
            Shape::Screw {
                metric,
                diameter,
                length,
                head,
                head_diameter,
                head_height,
            } => {
                out.push(metric.clone());
                out.push(nm(diameter));
                out.push(nm(length));
                out.push(head.name().to_string());
                out.push(nm(head_diameter));
                out.push(nm(head_height));
            }
            Shape::Bracket {
                leg_a,
                leg_b,
                width,
                thickness,
                hole_diameter,
            } => out.extend([leg_a, leg_b, width, thickness, hole_diameter].map(nm)),
        }
        out
    }

    /// The part with the words [`Part::words`] wrote. The words are checked for being canonical, not
    /// for the part being valid: [`Part::validate`] does that.
    pub fn from_words(words: &[&str]) -> Result<Part, String> {
        let [name, kind, material, rest @ ..] = words else {
            return Err("a part has no name, kind and material".into());
        };
        let kind = Kind::from_name(kind).ok_or("the kind of a part is not known")?;
        if rest.len() != kind.numbers() {
            return Err(format!(
                "a {} needs {} values after its material",
                kind.name(),
                kind.numbers()
            ));
        }
        let l = |i: usize, what: &str| length_word(rest[i], what);
        let shape = match kind {
            Kind::Profile => Shape::Profile {
                width: l(0, "width of a profile")?,
                height: l(1, "height of a profile")?,
                wall: l(2, "wall of a profile")?,
                default_cut: l(3, "usual cut of a profile")?,
                max_cut: l(4, "longest cut of a profile")?,
            },
            Kind::Screw => Shape::Screw {
                metric: rest[0].to_string(),
                diameter: l(1, "diameter of a screw")?,
                length: l(2, "length of a screw")?,
                head: Head::from_name(rest[3]).ok_or("the head of a screw is not known")?,
                head_diameter: l(4, "diameter of a head")?,
                head_height: l(5, "height of a head")?,
            },
            Kind::Bracket => Shape::Bracket {
                leg_a: l(0, "first leg of a bracket")?,
                leg_b: l(1, "second leg of a bracket")?,
                width: l(2, "width of a bracket")?,
                thickness: l(3, "thickness of a bracket")?,
                hole_diameter: l(4, "hole of a bracket")?,
            },
        };
        Ok(Part {
            name: (*name).to_string(),
            material: (*material).to_string(),
            shape,
        })
    }

    /// Whether the part holds together: the names are words within the limit, every size is above
    /// zero (a wall and a hole may be zero) and within the largest dimension, a hollow section has
    /// room inside, the usual cut is not longer than the bar, a head is wider than its shank and the
    /// holes of a bracket fit in its legs. The answer is the reason when it does not.
    pub fn validate(&self, limits: &CatalogLimits) -> Result<(), String> {
        let name = &self.name;
        for (what, text) in [("name", &self.name), ("material", &self.material)] {
            if !is_word(text) || text.len() > limits.max_name_bytes {
                return Err(format!(
                    "the {what} of a part is not a word of at most {} bytes",
                    limits.max_name_bytes
                ));
            }
        }
        let max = limits.max_dimension.get();
        let big = |what: &str, v: Length, zero_ok: bool| -> Result<(), String> {
            if v.get() < 0 || (v.get() == 0 && !zero_ok) || v.get() > max {
                Err(format!(
                    "{name}: the {what} must be above zero and at most {} mm",
                    mm_text(max)
                ))
            } else {
                Ok(())
            }
        };
        match &self.shape {
            Shape::Profile {
                width,
                height,
                wall,
                default_cut,
                max_cut,
            } => {
                big("width", *width, false)?;
                big("height", *height, false)?;
                big("wall", *wall, true)?;
                big("usual cut", *default_cut, false)?;
                big("longest cut", *max_cut, false)?;
                if wall.get() > 0 && 2 * wall.get() >= width.get().min(height.get()) {
                    return Err(format!(
                        "{name}: the wall leaves no room inside the section"
                    ));
                }
                if default_cut.get() > max_cut.get() {
                    return Err(format!("{name}: the usual cut is longer than the bar"));
                }
                if default_cut.get() < limits.min_cut.get() {
                    return Err(format!(
                        "{name}: the usual cut is shorter than the least cut of the settings"
                    ));
                }
            }
            Shape::Screw {
                metric,
                diameter,
                length,
                head_diameter,
                head_height,
                ..
            } => {
                let ok_metric = metric.len() >= 2
                    && metric.starts_with('M')
                    && metric[1..].bytes().all(|b| b.is_ascii_digit() || b == b'.')
                    && metric.len() <= limits.max_name_bytes;
                if !ok_metric {
                    return Err(format!("{name}: the metric name is not M and a number"));
                }
                big("diameter", *diameter, false)?;
                big("length", *length, false)?;
                big("head diameter", *head_diameter, false)?;
                big("head height", *head_height, false)?;
                if head_diameter.get() <= diameter.get() {
                    return Err(format!("{name}: the head is not wider than the shank"));
                }
            }
            Shape::Bracket {
                leg_a,
                leg_b,
                width,
                thickness,
                hole_diameter,
            } => {
                big("first leg", *leg_a, false)?;
                big("second leg", *leg_b, false)?;
                big("width", *width, false)?;
                big("thickness", *thickness, false)?;
                big("hole", *hole_diameter, true)?;
                if leg_a.get() <= thickness.get() || leg_b.get() <= thickness.get() {
                    return Err(format!("{name}: a leg is not longer than the thickness"));
                }
                let d = hole_diameter.get();
                if d > 0
                    && (d >= width.get()
                        || d >= leg_a.get() - thickness.get()
                        || d >= leg_b.get() - thickness.get())
                {
                    return Err(format!("{name}: the hole does not fit in a leg"));
                }
            }
        }
        Ok(())
    }

    /// The size of the part in its own frame (the smallest corner of its bounding box at the
    /// origin), for a profile cut to `cut`. A screw has its head at the smallest `z` and its shank
    /// running up; a bracket has the leg `a` along `x`, the width along `y` and the leg `b` along `z`.
    pub fn size(&self, cut: Option<Length>) -> [i64; 3] {
        match &self.shape {
            Shape::Profile {
                width,
                height,
                default_cut,
                ..
            } => [width.get(), height.get(), cut.unwrap_or(*default_cut).get()],
            Shape::Screw {
                diameter,
                length,
                head_diameter,
                head_height,
                ..
            } => {
                let d = diameter.get().max(head_diameter.get());
                [d, d, head_height.get() + length.get()]
            }
            Shape::Bracket {
                leg_a,
                leg_b,
                width,
                ..
            } => [leg_a.get(), width.get(), leg_b.get()],
        }
    }

    /// The volume of the material in cubic nanometres, for a profile cut to `cut`. A bar and a
    /// bracket are exact; a round or hexagonal shape is rounded to the nanometre cubed from the
    /// ratios for pi and the square root of three.
    pub fn volume_nm3(&self, cut: Option<Length>) -> Result<i128, CadError> {
        let overflow = || CadError::Overflow("volume");
        match &self.shape {
            Shape::Profile {
                width,
                height,
                wall,
                default_cut,
                ..
            } => {
                let (w, h, t) = (
                    i128::from(width.get()),
                    i128::from(height.get()),
                    i128::from(wall.get()),
                );
                let inner = if t > 0 { (w - 2 * t) * (h - 2 * t) } else { 0 };
                (w * h - inner)
                    .checked_mul(i128::from(cut.unwrap_or(*default_cut).get()))
                    .ok_or_else(overflow)
            }
            Shape::Screw {
                diameter,
                length,
                head,
                head_diameter,
                head_height,
                ..
            } => {
                let (d, hd, hh) = (
                    i128::from(diameter.get()),
                    i128::from(head_diameter.get()),
                    i128::from(head_height.get()),
                );
                let shank = cylinder_volume_nm3(*diameter, *length)?;
                let head = match head {
                    Head::Cylinder => cylinder_volume_nm3(*head_diameter, *head_height)?,
                    // the area of a hexagon across the flats s is (sqrt 3 / 2) s^2
                    Head::Hex => div_round_half_away(
                        hd.checked_mul(hd)
                            .and_then(|x| x.checked_mul(hh))
                            .and_then(|x| x.checked_mul(SQRT3_NUM))
                            .ok_or_else(overflow)?,
                        2 * SQRT3_DEN,
                    )?,
                    // a cone cut off: pi h (D^2 + D d + d^2) / 12
                    Head::Countersunk => div_round_half_away(
                        (hd * hd + hd * d + d * d)
                            .checked_mul(hh)
                            .and_then(|x| x.checked_mul(PI_NUM))
                            .ok_or_else(overflow)?,
                        12 * PI_DEN,
                    )?,
                };
                shank.checked_add(head).ok_or_else(overflow)
            }
            Shape::Bracket {
                leg_a,
                leg_b,
                width,
                thickness,
                hole_diameter,
            } => {
                let (a, b, w, t) = (
                    i128::from(leg_a.get()),
                    i128::from(leg_b.get()),
                    i128::from(width.get()),
                    i128::from(thickness.get()),
                );
                let body = w
                    .checked_mul(t)
                    .and_then(|x| x.checked_mul(a + b - t))
                    .ok_or_else(overflow)?;
                if hole_diameter.get() > 0 {
                    // one hole in each leg, through the thickness
                    Ok(body - 2 * cylinder_volume_nm3(*hole_diameter, *thickness)?)
                } else {
                    Ok(body)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The library file
// ---------------------------------------------------------------------------------------------

/// A library of parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    /// The name (a word of the catalog).
    pub name: String,
    /// The parts, in the order of the file.
    pub parts: Vec<Part>,
}

impl Library {
    /// The part with a name.
    pub fn part(&self, name: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.name == name)
    }

    /// The text of the library: always the same for the same library.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "{HEADER} {FILE_VERSION}");
        let _ = writeln!(out, "name {}", self.name);
        for part in &self.parts {
            let _ = writeln!(out, "part {}", part.words().join(" "));
        }
        out
    }

    /// Reads the text of a library, refusing with `PHOENIX-CAD-044` and the number of the line
    /// what it does not understand.
    pub fn from_text(text: &str, limits: &CatalogLimits) -> Result<Library, CadError> {
        if text.len() > limits.max_bytes {
            return Err(CadError::Library(format!(
                "the file has {} bytes, more than the {} the settings allow",
                text.len(),
                limits.max_bytes
            )));
        }
        let mut name: Option<String> = None;
        let mut parts: Vec<Part> = Vec::new();
        let mut header = false;
        for (i, raw) in text.lines().enumerate() {
            let n = i + 1;
            if raw.is_empty() || raw.starts_with('#') {
                continue;
            }
            let words: Vec<&str> = raw.split(' ').collect();
            if !header {
                match words.as_slice() {
                    [h, version] if *h == HEADER => {
                        let v = version
                            .parse::<u32>()
                            .ok()
                            .filter(|v| v.to_string() == *version)
                            .ok_or_else(|| library_error(n, "the version is not a number"))?;
                        if v == 0 || v > FILE_VERSION {
                            return Err(library_error(
                                n,
                                format!("version {v} is not one this program reads"),
                            ));
                        }
                        header = true;
                    }
                    _ => return Err(library_error(n, "this is not a library file")),
                }
                continue;
            }
            match words.as_slice() {
                ["name", word] => {
                    if name.is_some() {
                        return Err(library_error(n, "the name is given twice"));
                    }
                    if !is_word(word) || word.len() > limits.max_name_bytes {
                        return Err(library_error(n, "the name is not a word within the limit"));
                    }
                    name = Some((*word).to_string());
                }
                ["part", rest @ ..] => {
                    if name.is_none() {
                        return Err(library_error(n, "a part comes before the name"));
                    }
                    if parts.len() >= limits.max_parts {
                        return Err(library_error(
                            n,
                            format!(
                                "more than the {} parts the settings allow",
                                limits.max_parts
                            ),
                        ));
                    }
                    let part = Part::from_words(rest).map_err(|e| library_error(n, e))?;
                    part.validate(limits).map_err(|e| library_error(n, e))?;
                    if parts.iter().any(|p| p.name == part.name) {
                        return Err(library_error(
                            n,
                            format!("the part {} is listed twice", part.name),
                        ));
                    }
                    parts.push(part);
                }
                _ => return Err(library_error(n, "the line is not understood")),
            }
        }
        if !header {
            return Err(CadError::Library("the file is empty".into()));
        }
        let name = name.ok_or_else(|| CadError::Library("the library has no name".into()))?;
        Ok(Library { name, parts })
    }

    /// Whether every material of the parts is in the settings (the table of densities).
    pub fn check_materials(&self, densities: &BTreeMap<String, u32>) -> Result<(), CadError> {
        for part in &self.parts {
            if !densities.contains_key(&part.material) {
                return Err(CadError::Library(format!(
                    "the material {} of {} is not in the settings",
                    part.material, part.name
                )));
            }
        }
        Ok(())
    }
}

/// Writes a library to a file atomically, keeping `backups` older copies.
pub fn write_file(path: &Path, library: &Library, backups: u32) -> Result<(), CadError> {
    store::save_atomic(path, library.to_text().as_bytes(), backups)
}

/// Reads a library file under the size limit of the settings.
pub fn read_file(path: &Path, limits: &CatalogLimits) -> Result<Library, CadError> {
    let bytes = store::load_limited(path, limits.max_bytes as u64)?;
    let text =
        String::from_utf8(bytes).map_err(|_| CadError::Library("the file is not text".into()))?;
    Library::from_text(&text, limits)
}

// ---------------------------------------------------------------------------------------------
// Orientation
// ---------------------------------------------------------------------------------------------

/// A direction along one of the three axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Towards larger `x`.
    PlusX,
    /// Towards smaller `x`.
    MinusX,
    /// Towards larger `y`.
    PlusY,
    /// Towards smaller `y`.
    MinusY,
    /// Towards larger `z`.
    PlusZ,
    /// Towards smaller `z`.
    MinusZ,
}

impl Axis {
    /// The six directions in a fixed order.
    pub const ALL: [Axis; 6] = [
        Axis::PlusX,
        Axis::MinusX,
        Axis::PlusY,
        Axis::MinusY,
        Axis::PlusZ,
        Axis::MinusZ,
    ];

    /// The word of the files: `+x`, `-x`, `+y`, `-y`, `+z` or `-z`.
    pub fn name(self) -> &'static str {
        match self {
            Axis::PlusX => "+x",
            Axis::MinusX => "-x",
            Axis::PlusY => "+y",
            Axis::MinusY => "-y",
            Axis::PlusZ => "+z",
            Axis::MinusZ => "-z",
        }
    }

    /// The direction with a word of the files.
    pub fn from_name(name: &str) -> Option<Axis> {
        Axis::ALL.into_iter().find(|a| a.name() == name)
    }
}

/// How a part is turned: the direction its length runs along (the local `z` axis goes there) and a
/// roll about that direction in quarter turns (0 to 3). The 24 pairs are the 24 turns that keep the
/// axes on the axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Orientation {
    /// The direction the local `z` axis of the part points along.
    pub axis: Axis,
    /// The quarter turns about the local `z` axis, done before the part is pointed (0 to 3).
    pub roll: u8,
}

impl Default for Orientation {
    /// The length along `+z`, not rolled.
    fn default() -> Orientation {
        Orientation {
            axis: Axis::PlusZ,
            roll: 0,
        }
    }
}

impl Orientation {
    /// The word of the files: the direction, a slash and the roll (`+x/1`).
    pub fn word(self) -> String {
        format!("{}/{}", self.axis.name(), self.roll)
    }

    /// The orientation with a word of the files.
    pub fn from_word(word: &str) -> Option<Orientation> {
        let (axis, roll) = word.split_once('/')?;
        let roll = match roll {
            "0" => 0,
            "1" => 1,
            "2" => 2,
            "3" => 3,
            _ => return None,
        };
        Some(Orientation {
            axis: Axis::from_name(axis)?,
            roll,
        })
    }

    /// The turn: `global = rotation * local`.
    pub fn rotation(self) -> Rotation {
        let (u, v) = match self.axis {
            Axis::PlusZ => ([1, 0, 0], [0, 1, 0]),
            Axis::MinusZ => ([1, 0, 0], [0, -1, 0]),
            Axis::PlusX => ([0, 1, 0], [0, 0, 1]),
            Axis::MinusX => ([0, 1, 0], [0, 0, -1]),
            Axis::PlusY => ([0, 0, 1], [1, 0, 0]),
            Axis::MinusY => ([0, 0, 1], [-1, 0, 0]),
        };
        let (ru, rv) = match self.roll % 4 {
            0 => ([1, 0, 0], [0, 1, 0]),
            1 => ([0, 1, 0], [-1, 0, 0]),
            2 => ([-1, 0, 0], [0, -1, 0]),
            _ => ([0, -1, 0], [1, 0, 0]),
        };
        // the axes are distinct unit vectors, so the turns always exist
        let point = Rotation::from_axes(u, v).unwrap_or(Rotation::IDENTITY);
        let roll = Rotation::from_axes(ru, rv).unwrap_or(Rotation::IDENTITY);
        point.compose(&roll)
    }

    /// Where each local axis goes: for the local axes `x`, `y` and `z`, the vector (one entry is
    /// plus or minus one) it points along in the model. These are the columns of the turn.
    pub fn columns(self) -> [[i8; 3]; 3] {
        let r = self.rotation();
        let mut out = [[0i8; 3]; 3];
        for (j, col) in out.iter_mut().enumerate() {
            let mut unit = [0i64; 3];
            unit[j] = 1;
            let v = r.apply(unit);
            for (c, x) in col.iter_mut().zip(v) {
                *c = i8::try_from(x).unwrap_or(0);
            }
        }
        out
    }

    /// The 24 orientations in a fixed order.
    pub fn all() -> Vec<Orientation> {
        Axis::ALL
            .into_iter()
            .flat_map(|axis| (0..4u8).map(move |roll| Orientation { axis, roll }))
            .collect()
    }

    /// The size of a part in the model, along `x`, `y` and `z`, and how far the smallest corner of
    /// its bounding box lies from the point its own frame turns about: the turned part is moved by
    /// that much so that its smallest corner is where the position says.
    pub fn extent(self, size: [i64; 3]) -> ([i64; 3], [i64; 3]) {
        let cols = self.columns();
        let mut extent = [0i64; 3];
        let mut shift = [0i64; 3];
        for (j, col) in cols.iter().enumerate() {
            for (i, c) in col.iter().enumerate() {
                if *c != 0 {
                    extent[i] = size[j];
                    if *c < 0 {
                        shift[i] = size[j];
                    }
                }
            }
        }
        (extent, shift)
    }
}

// ---------------------------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------------------------

/// A part placed in the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartInstance {
    /// The name shown to people; an attribute, it never identifies the instance.
    pub name: String,
    /// The name of the library the part came from.
    pub library: String,
    /// The part: a copy, so that the model does not depend on the file it came from.
    pub part: Part,
    /// The smallest corner of the bounding box of the part in the model (`x`, `y`, `z`).
    pub at: [Length; 3],
    /// How the part is turned.
    pub orientation: Orientation,
    /// The length a profile is cut to; `None` for a screw or a bracket.
    pub cut: Option<Length>,
}

/// The bounding box of an instance: the smallest corner and the largest corner, in nanometres.
pub type Bounds = ([i64; 3], [i64; 3]);

impl PartInstance {
    /// The length a profile is cut to (its usual cut when none was given), or `None`.
    pub fn cut_length(&self) -> Option<Length> {
        match self.part.shape {
            Shape::Profile { default_cut, .. } => Some(self.cut.unwrap_or(default_cut)),
            _ => None,
        }
    }

    /// The size along `x`, `y` and `z`, and the shift of the turned part (see [`Orientation::extent`]).
    pub fn extent(&self) -> ([i64; 3], [i64; 3]) {
        self.orientation.extent(self.part.size(self.cut))
    }

    /// The bounding box in the model.
    pub fn bounds(&self) -> Result<Bounds, CadError> {
        let (extent, _) = self.extent();
        let min = [self.at[0].get(), self.at[1].get(), self.at[2].get()];
        let mut max = [0i64; 3];
        for i in 0..3 {
            max[i] = min[i]
                .checked_add(extent[i])
                .filter(|v| Length::new(*v).is_ok())
                .ok_or_else(|| {
                    part_error(format!(
                        "{} reaches outside the range of lengths",
                        self.name
                    ))
                })?;
        }
        Ok((min, max))
    }

    /// The volume of the material in cubic nanometres.
    pub fn volume_nm3(&self) -> Result<i128, CadError> {
        self.part.volume_nm3(self.cut)
    }

    /// Whether the instance holds together: its library is a word, its part is valid, a profile
    /// has a cut between the least cut of the settings and its longest bar and the others have none,
    /// and the part lies inside the range of lengths and within the largest distance of the settings.
    pub fn check(&self, limits: &CatalogLimits) -> Result<(), CadError> {
        if self.name.len() > limits.max_name_bytes {
            return Err(part_error("the name of the instance is too long"));
        }
        if !is_word(&self.library) || self.library.len() > limits.max_name_bytes {
            return Err(part_error(format!(
                "{}: the library is not a word within the limit",
                self.name
            )));
        }
        self.part.validate(limits).map_err(part_error)?;
        match (&self.part.shape, self.cut) {
            (Shape::Profile { max_cut, .. }, Some(cut)) => {
                if cut.get() < limits.min_cut.get() {
                    return Err(part_error(format!(
                        "{}: the cut {} mm is shorter than the least cut {} mm",
                        self.name,
                        mm_text(cut.get()),
                        mm_text(limits.min_cut.get())
                    )));
                }
                if cut.get() > max_cut.get() {
                    return Err(part_error(format!(
                        "{}: the cut {} mm is longer than the bar of {} mm",
                        self.name,
                        mm_text(cut.get()),
                        mm_text(max_cut.get())
                    )));
                }
            }
            (Shape::Profile { .. }, None) => {
                return Err(part_error(format!("{}: a profile needs a cut", self.name)));
            }
            (_, Some(_)) => {
                return Err(part_error(format!(
                    "{}: only a profile is cut to a length",
                    self.name
                )));
            }
            (_, None) => {}
        }
        let (min, max) = self.bounds()?;
        let far = limits.max_dimension.get();
        for i in 0..3 {
            if min[i].abs() > far || max[i].abs() > far {
                return Err(part_error(format!(
                    "{}: it lies farther than {} mm from the origin",
                    self.name,
                    mm_text(far)
                )));
            }
        }
        Ok(())
    }

    /// The words of the instance after its name: the library, the part, the position, the
    /// orientation and the cut (`-` when there is none).
    pub fn words(&self) -> Vec<String> {
        let mut out = vec![self.library.clone()];
        out.extend(self.part.words());
        for v in self.at {
            out.push(v.get().to_string());
        }
        out.push(self.orientation.word());
        out.push(self.cut.map_or("-".to_string(), |c| c.get().to_string()));
        out
    }

    /// The instance with the words [`PartInstance::words`] wrote and the given name.
    pub fn from_words(name: &str, words: &[&str]) -> Result<PartInstance, String> {
        let [library, rest @ ..] = words else {
            return Err("an instance has no library".into());
        };
        if rest.len() < 3 {
            return Err("an instance has no part".into());
        }
        let kind = Kind::from_name(rest[1]).ok_or("the kind of a part is not known")?;
        let used = 3 + kind.numbers();
        if rest.len() != used + 5 {
            return Err("an instance has not the values it needs".into());
        }
        let part = Part::from_words(&rest[..used])?;
        let tail = &rest[used..];
        let at = [
            signed_word(tail[0], "position x")?,
            signed_word(tail[1], "position y")?,
            signed_word(tail[2], "position z")?,
        ];
        let orientation =
            Orientation::from_word(tail[3]).ok_or("the orientation is not understood")?;
        let cut = if tail[4] == "-" {
            None
        } else {
            Some(length_word(tail[4], "cut")?)
        };
        Ok(PartInstance {
            name: name.to_string(),
            library: (*library).to_string(),
            part,
            at,
            orientation,
            cut,
        })
    }
}

/// Whether two bounding boxes share volume (touching is not sharing).
pub fn share_volume(a: &Bounds, b: &Bounds) -> bool {
    (0..3).all(|i| a.0[i] < b.1[i] && b.0[i] < a.1[i])
}

/// Refuses an instance that shares volume with a profile of the model, when the limits say so and
/// the instance is a profile too. `others` are the other instances (not this one).
pub fn check_overlap<'a>(
    instance: &PartInstance,
    others: impl Iterator<Item = (&'a Id, &'a PartInstance)>,
    limits: &CatalogLimits,
) -> Result<(), CadError> {
    if !limits.refuse_overlap || instance.part.kind() != Kind::Profile {
        return Ok(());
    }
    let mine = instance.bounds()?;
    for (_, other) in others {
        if other.part.kind() != Kind::Profile {
            continue;
        }
        if share_volume(&mine, &other.bounds()?) {
            return Err(part_error(format!(
                "{} shares volume with {}",
                instance.name, other.name
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The bill of materials and the cut list
// ---------------------------------------------------------------------------------------------

/// A line of the bill of materials: the same part, material and cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillLine {
    /// The kind of the part.
    pub kind: Kind,
    /// The name of the part.
    pub part: String,
    /// The material.
    pub material: String,
    /// How many pieces.
    pub count: u32,
    /// The length of the cut of a profile.
    pub cut: Option<Length>,
    /// The volume of all the pieces of the line, in cubic nanometres.
    pub volume_nm3: i128,
    /// The mass of all the pieces of the line, in micrograms.
    pub mass_ug: i128,
}

/// The bill of materials of a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bill {
    /// The lines: by kind, then name, material and cut.
    pub lines: Vec<BillLine>,
    /// How many pieces in all.
    pub pieces: u32,
    /// The volume of the material in cubic nanometres.
    pub volume_nm3: i128,
    /// The mass in micrograms.
    pub mass_ug: i128,
}

/// The bill of materials of some instances. The density of each material is in `densities`
/// (kilograms per cubic metre, the table of the settings); a material that is not there is
/// refused with `PHOENIX-CAD-045`.
pub fn bill<'a>(
    instances: impl Iterator<Item = &'a PartInstance>,
    densities: &BTreeMap<String, u32>,
) -> Result<Bill, CadError> {
    type Key = (Kind, String, String, Option<i64>);
    let mut groups: BTreeMap<Key, (u32, i128)> = BTreeMap::new();
    for i in instances {
        let key = (
            i.part.kind(),
            i.part.name.clone(),
            i.part.material.clone(),
            i.cut_length().map(Length::get),
        );
        let volume = i.volume_nm3()?;
        let entry = groups.entry(key).or_insert((0, 0));
        entry.0 += 1;
        entry.1 = entry
            .1
            .checked_add(volume)
            .ok_or(CadError::Overflow("volume"))?;
    }
    let mut lines = Vec::with_capacity(groups.len());
    let (mut pieces, mut volume, mut mass) = (0u32, 0i128, 0i128);
    for ((kind, part, material, cut), (count, vol)) in groups {
        let density = *densities.get(&material).ok_or_else(|| {
            part_error(format!(
                "the material {material} of {part} is not in the settings"
            ))
        })?;
        let line_mass = mass_micrograms(vol, density)?;
        pieces += count;
        volume += vol;
        mass += line_mass;
        lines.push(BillLine {
            kind,
            part,
            material,
            count,
            cut: cut.and_then(|c| Length::new(c).ok()),
            volume_nm3: vol,
            mass_ug: line_mass,
        });
    }
    Ok(Bill {
        lines,
        pieces,
        volume_nm3: volume,
        mass_ug: mass,
    })
}

/// What is cut from one profile: the lengths with how many of each, longest first, and the length
/// of all the pieces together in nanometres.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutLine {
    /// The name of the profile.
    pub part: String,
    /// The material.
    pub material: String,
    /// The cuts: length and count, longest first.
    pub cuts: Vec<(Length, u32)>,
    /// The sum of all the cuts.
    pub total_nm: i128,
}

/// The cut list of a bill: for each profile, the lengths it is cut to.
pub fn cut_list(bill: &Bill) -> Vec<CutLine> {
    let mut out: Vec<CutLine> = Vec::new();
    for line in &bill.lines {
        let (Kind::Profile, Some(cut)) = (line.kind, line.cut) else {
            continue;
        };
        let total = i128::from(cut.get()) * i128::from(line.count);
        match out
            .iter_mut()
            .find(|c| c.part == line.part && c.material == line.material)
        {
            Some(c) => {
                c.cuts.push((cut, line.count));
                c.total_nm += total;
            }
            None => out.push(CutLine {
                part: line.part.clone(),
                material: line.material.clone(),
                cuts: vec![(cut, line.count)],
                total_nm: total,
            }),
        }
    }
    for c in &mut out {
        c.cuts.sort_by_key(|(len, _)| std::cmp::Reverse(len.get()));
    }
    out
}

/// A whole number divided by `den`, written with `decimals` decimals, rounded half away from zero.
fn scaled(n: i128, den: i128, decimals: u32) -> String {
    let factor = 10i128.pow(decimals);
    let v = div_round_half_away(n * factor, den).unwrap_or(0);
    let magnitude = v.unsigned_abs();
    let f = factor.unsigned_abs();
    let sign = if v < 0 { "-" } else { "" };
    if decimals == 0 {
        format!("{sign}{magnitude}")
    } else {
        let width = decimals as usize;
        format!("{sign}{}.{:0width$}", magnitude / f, magnitude % f)
    }
}

/// A volume in cubic millimetres with three decimals.
pub fn volume_mm3_text(nm3: i128) -> String {
    scaled(nm3, 1_000_000_000_000_000_000, 3)
}

/// A mass in grams with three decimals.
pub fn mass_g_text(micrograms: i128) -> String {
    scaled(micrograms, 1_000_000, 3)
}

/// The bill as a table of comma separated values, the same text for the same bill: the kind, the
/// part, the material, how many, the cut in millimetres, the volume in cubic millimetres and the
/// mass in grams, and a last line with the totals.
pub fn bill_csv(bill: &Bill) -> String {
    let mut out = String::from("kind,part,material,count,cut_mm,volume_mm3,mass_g\n");
    for l in &bill.lines {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{}",
            l.kind.name(),
            l.part,
            l.material,
            l.count,
            l.cut.map_or(String::new(), |c| mm_text(c.get())),
            volume_mm3_text(l.volume_nm3),
            mass_g_text(l.mass_ug)
        );
    }
    let _ = writeln!(
        out,
        "total,,,{},,{},{}",
        bill.pieces,
        volume_mm3_text(bill.volume_nm3),
        mass_g_text(bill.mass_ug)
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DesignLimits;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn um(v: i64) -> Length {
        Length::from_um(v).unwrap()
    }

    fn limits() -> CatalogLimits {
        CatalogLimits::shipped()
    }

    fn id(n: u64) -> Id {
        Id::from_parts(1_700_000_000_000, 1, n).unwrap()
    }

    fn tube() -> Part {
        Part {
            name: "tube-20x20x2".into(),
            material: "aluminium".into(),
            shape: Shape::Profile {
                width: mm(20),
                height: mm(20),
                wall: mm(2),
                default_cut: mm(100),
                max_cut: mm(2000),
            },
        }
    }

    fn bar() -> Part {
        Part {
            name: "bar-10x30".into(),
            material: "aluminium".into(),
            shape: Shape::Profile {
                width: mm(10),
                height: mm(30),
                wall: Length::ZERO,
                default_cut: mm(100),
                max_cut: mm(2000),
            },
        }
    }

    fn screw(head: Head) -> Part {
        let (name, metric, d, len, hd, hh) = match head {
            Head::Cylinder => ("bolt-m3x10", "M3", mm(3), mm(10), um(5500), um(3000)),
            Head::Countersunk => ("flat-m3x10", "M3", mm(3), mm(10), mm(6), um(1700)),
            Head::Hex => ("hex-m4x16", "M4", mm(4), mm(16), mm(7), um(2800)),
        };
        Part {
            name: name.into(),
            material: "steel".into(),
            shape: Shape::Screw {
                metric: metric.into(),
                diameter: d,
                length: len,
                head,
                head_diameter: hd,
                head_height: hh,
            },
        }
    }

    fn bracket(hole: Length) -> Part {
        Part {
            name: "angle-30".into(),
            material: "aluminium".into(),
            shape: Shape::Bracket {
                leg_a: mm(30),
                leg_b: mm(30),
                width: mm(20),
                thickness: mm(3),
                hole_diameter: hole,
            },
        }
    }

    fn instance(part: Part, cut: Option<Length>) -> PartInstance {
        PartInstance {
            name: "piece".into(),
            library: "chassis-basic".into(),
            part,
            at: [Length::ZERO; 3],
            orientation: Orientation::default(),
            cut,
        }
    }

    fn densities() -> BTreeMap<String, u32> {
        crate::config::Settings::defaults().materials
    }

    #[test]
    fn the_volume_of_a_profile_is_the_section_times_the_cut_and_a_bracket_is_exact() {
        // 20 x 20 with a wall of 2: 400 - 16 x 16 = 144 mm2, over 300 mm = 43200 mm3
        assert_eq!(
            tube().volume_nm3(Some(mm(300))).unwrap(),
            43_200_000_000_000_000_000_000
        );
        assert_eq!(
            tube().volume_nm3(Some(mm(150))).unwrap(),
            21_600_000_000_000_000_000_000
        );
        // with no cut asked it is the usual cut of 100 mm
        assert_eq!(
            tube().volume_nm3(None).unwrap(),
            14_400_000_000_000_000_000_000
        );
        // a solid bar: 10 x 30 over 100 mm = 30000 mm3
        assert_eq!(
            bar().volume_nm3(Some(mm(100))).unwrap(),
            30_000_000_000_000_000_000_000
        );
        // a bracket without holes: 20 x 3 x (30 + 30 - 3) = 3420 mm3
        assert_eq!(
            bracket(Length::ZERO).volume_nm3(None).unwrap(),
            3_420_000_000_000_000_000_000
        );
        // two holes of 4.5 mm through 3 mm: 2 x 47712938426398125000 nm3 less
        assert_eq!(
            bracket(um(4500)).volume_nm3(None).unwrap(),
            3_324_574_123_147_203_750_000
        );
    }

    #[test]
    fn the_volume_of_a_screw_is_its_shank_plus_its_head() {
        // a shank of 3 x 10 mm is 70685834705775000000 nm3 and a head of 5.5 x 3 mm 71274883328323125000
        assert_eq!(
            screw(Head::Cylinder).volume_nm3(None).unwrap(),
            141_960_718_034_098_125_000
        );
        // the same shank under a cone of 6 mm over 1.7 mm
        assert_eq!(
            screw(Head::Countersunk).volume_nm3(None).unwrap(),
            98_724_549_139_065_750_000
        );
        // a hexagon of 7 mm across the flats and 2.8 mm tall over a shank of 4 x 16 mm
        assert_eq!(
            screw(Head::Hex).volume_nm3(None).unwrap(),
            319_880_615_228_993_400_000
        );
    }

    #[test]
    fn the_mass_follows_the_density_of_the_material() {
        let d = densities();
        let one = |part: Part, cut: Option<Length>| {
            let i = instance(part, cut);
            let bill = bill(std::iter::once(&i), &d).unwrap();
            bill.mass_ug
        };
        // 43200 mm3 of aluminium (2700 kg/m3) are 116.64 g
        assert_eq!(one(tube(), Some(mm(300))), 116_640_000);
        assert_eq!(one(bar(), Some(mm(100))), 81_000_000);
        // steel is 7850 kg/m3
        assert_eq!(one(screw(Head::Cylinder), None), 1_114_392);
        assert_eq!(one(screw(Head::Countersunk), None), 774_988);
        assert_eq!(one(screw(Head::Hex), None), 2_511_063);
        assert_eq!(one(bracket(um(4500)), None), 8_976_350);
    }

    #[test]
    fn a_unknown_material_is_refused_with_the_code_of_the_part() {
        let mut p = bar();
        p.material = "unobtainium".into();
        let i = instance(p, Some(mm(100)));
        let e = bill(std::iter::once(&i), &densities()).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-045");
        assert!(e.to_string().contains("unobtainium"));
    }

    #[test]
    fn there_are_24_orientations_that_turn_the_axes_onto_the_axes_and_no_two_are_alike() {
        let all = Orientation::all();
        assert_eq!(all.len(), 24);
        let mut seen = std::collections::BTreeSet::new();
        for o in &all {
            assert!(seen.insert(o.columns()), "{o:?}");
            // every local axis goes to one axis, and no two go to the same one
            let cols = o.columns();
            for c in &cols {
                assert_eq!(c.iter().filter(|v| **v != 0).count(), 1);
                assert!(c.iter().all(|v| (-1..=1).contains(v)));
            }
            for i in 0..3 {
                assert_eq!(cols.iter().filter(|c| c[i] != 0).count(), 1);
            }
            // the word is read back
            assert_eq!(Orientation::from_word(&o.word()), Some(*o));
            // it is one of the 24 rotations of the datums
            assert!(Rotation::all().contains(&o.rotation()));
        }
        assert_eq!(seen.len(), 24);
        assert_eq!(Orientation::default().rotation(), Rotation::IDENTITY);
        for bad in ["", "+z", "+z/4", "z/0", "+w/0", "+z/00", "+z/-1", "+z/ 1"] {
            assert_eq!(Orientation::from_word(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_extent_and_the_shift_of_a_profile_of_20_by_30_by_100_in_every_orientation() {
        // checked by a second calculation: (axis, roll) -> extent, shift, in millimetres
        let size = [20_000_000, 30_000_000, 100_000_000];
        let m = |a: [i64; 3]| a.map(|v| v * 1_000_000);
        let cases: [(Axis, u8, [i64; 3], [i64; 3]); 24] = [
            (Axis::PlusZ, 0, [20, 30, 100], [0, 0, 0]),
            (Axis::PlusZ, 1, [30, 20, 100], [30, 0, 0]),
            (Axis::PlusZ, 2, [20, 30, 100], [20, 30, 0]),
            (Axis::PlusZ, 3, [30, 20, 100], [0, 20, 0]),
            (Axis::MinusZ, 0, [20, 30, 100], [0, 30, 100]),
            (Axis::MinusZ, 1, [30, 20, 100], [30, 20, 100]),
            (Axis::MinusZ, 2, [20, 30, 100], [20, 0, 100]),
            (Axis::MinusZ, 3, [30, 20, 100], [0, 0, 100]),
            (Axis::PlusX, 0, [100, 20, 30], [0, 0, 0]),
            (Axis::PlusX, 1, [100, 30, 20], [0, 30, 0]),
            (Axis::PlusX, 2, [100, 20, 30], [0, 20, 30]),
            (Axis::PlusX, 3, [100, 30, 20], [0, 0, 20]),
            (Axis::MinusX, 0, [100, 20, 30], [100, 0, 30]),
            (Axis::MinusX, 1, [100, 30, 20], [100, 30, 20]),
            (Axis::MinusX, 2, [100, 20, 30], [100, 20, 0]),
            (Axis::MinusX, 3, [100, 30, 20], [100, 0, 0]),
            (Axis::PlusY, 0, [30, 100, 20], [0, 0, 0]),
            (Axis::PlusY, 1, [20, 100, 30], [0, 0, 30]),
            (Axis::PlusY, 2, [30, 100, 20], [30, 0, 20]),
            (Axis::PlusY, 3, [20, 100, 30], [20, 0, 0]),
            (Axis::MinusY, 0, [30, 100, 20], [30, 100, 0]),
            (Axis::MinusY, 1, [20, 100, 30], [20, 100, 30]),
            (Axis::MinusY, 2, [30, 100, 20], [0, 100, 20]),
            (Axis::MinusY, 3, [20, 100, 30], [0, 100, 0]),
        ];
        for (axis, roll, extent, shift) in cases {
            let o = Orientation { axis, roll };
            assert_eq!(o.extent(size), (m(extent), m(shift)), "{axis:?} {roll}");
        }
    }

    #[test]
    fn an_instance_has_its_bounds_from_its_position_and_orientation() {
        let mut i = instance(tube(), Some(mm(300)));
        i.at = [mm(10), mm(-5), mm(2)];
        assert_eq!(
            i.bounds().unwrap(),
            (
                [10_000_000, -5_000_000, 2_000_000],
                [30_000_000, 15_000_000, 302_000_000]
            )
        );
        i.orientation = Orientation {
            axis: Axis::PlusX,
            roll: 0,
        };
        assert_eq!(
            i.bounds().unwrap(),
            (
                [10_000_000, -5_000_000, 2_000_000],
                [310_000_000, 15_000_000, 22_000_000]
            )
        );
        // a screw is as wide as its head and as long as its head and its shank: 5.5 by 5.5 by 13 mm
        let s = instance(screw(Head::Cylinder), None);
        assert_eq!(s.bounds().unwrap().1, [5_500_000, 5_500_000, 13_000_000]);
        // a bracket is 30 along x, 20 along y and 30 along z
        let b = instance(bracket(um(4500)), None);
        assert_eq!(b.bounds().unwrap().1, [30_000_000, 20_000_000, 30_000_000]);
    }

    #[test]
    fn every_limit_of_a_cut_is_met_on_the_inside_and_refused_on_the_outside() {
        let l = limits();
        // the least cut of the shipped settings is 1 mm and the bar is 2000 mm
        assert_eq!(l.min_cut, mm(1));
        let at = |cut: i64| instance(tube(), Some(Length::new(cut).unwrap())).check(&l);
        assert!(at(1_000_000).is_ok());
        assert!(at(2_000_000_000).is_ok());
        for bad in [999_999, 2_000_000_001, 0] {
            let e = at(bad).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-045", "{bad}");
        }
        assert!(instance(tube(), None).check(&l).is_err());
        assert!(instance(screw(Head::Hex), Some(mm(5))).check(&l).is_err());
        assert!(instance(bracket(Length::ZERO), Some(mm(5)))
            .check(&l)
            .is_err());
        assert!(instance(screw(Head::Hex), None).check(&l).is_ok());
    }

    #[test]
    fn a_part_that_does_not_hold_together_says_why() {
        let l = limits();
        let mut t = tube();
        t.shape = Shape::Profile {
            width: mm(20),
            height: mm(20),
            wall: mm(10),
            default_cut: mm(100),
            max_cut: mm(2000),
        };
        assert!(t.validate(&l).unwrap_err().contains("no room"));
        // a wall of 9.999999 mm still leaves 2 nm
        t.shape = Shape::Profile {
            width: mm(20),
            height: mm(20),
            wall: Length::new(9_999_999).unwrap(),
            default_cut: mm(100),
            max_cut: mm(2000),
        };
        assert!(t.validate(&l).is_ok());
        let mut s = screw(Head::Cylinder);
        if let Shape::Screw { head_diameter, .. } = &mut s.shape {
            *head_diameter = mm(3);
        }
        assert!(s.validate(&l).unwrap_err().contains("not wider"));
        if let Shape::Screw { metric, .. } = &mut s.shape {
            *metric = "3mm".into();
        }
        assert!(s.validate(&l).unwrap_err().contains("metric"));
        // the hole of a bracket has to fit in the free part of a leg: 27 mm is free, 20 wide
        assert!(bracket(um(19_999)).validate(&l).is_ok());
        assert!(bracket(mm(20)).validate(&l).unwrap_err().contains("hole"));
        let mut b = bracket(Length::ZERO);
        if let Shape::Bracket { leg_a, .. } = &mut b.shape {
            *leg_a = mm(3);
        }
        assert!(b
            .validate(&l)
            .unwrap_err()
            .contains("longer than the thickness"));
        let mut far = bar();
        if let Shape::Profile { max_cut, .. } = &mut far.shape {
            *max_cut = Length::new(6_000_000_001).unwrap();
        }
        assert!(far.validate(&l).unwrap_err().contains("at most 6000"));
        let mut named = bar();
        named.name = "two words".into();
        assert!(named.validate(&l).is_err());
        named.name = "x".repeat(65);
        assert!(named.validate(&l).is_err());
        named.name = "x".repeat(64);
        assert!(named.validate(&l).is_ok());
    }

    fn example_library() -> Library {
        Library {
            name: "chassis-basic".into(),
            parts: vec![
                tube(),
                bar(),
                screw(Head::Cylinder),
                screw(Head::Hex),
                bracket(um(4500)),
            ],
        }
    }

    #[test]
    fn a_library_is_written_in_the_text_of_its_version_and_read_back_to_the_same_text() {
        let lib = example_library();
        let text = lib.to_text();
        assert_eq!(
            text,
            "phoenix-library 1\n\
             name chassis-basic\n\
             part tube-20x20x2 profile aluminium 20000000 20000000 2000000 100000000 2000000000\n\
             part bar-10x30 profile aluminium 10000000 30000000 0 100000000 2000000000\n\
             part bolt-m3x10 screw steel M3 3000000 10000000 cylinder 5500000 3000000\n\
             part hex-m4x16 screw steel M4 4000000 16000000 hex 7000000 2800000\n\
             part angle-30 bracket aluminium 30000000 30000000 20000000 3000000 4500000\n"
        );
        let back = Library::from_text(&text, &limits()).unwrap();
        assert_eq!(back, lib);
        assert_eq!(back.to_text(), text);
        assert_eq!(back.part("angle-30"), Some(&bracket(um(4500))));
        assert_eq!(back.part("none"), None);
        // comments and empty lines are ignored
        let with_notes = format!("# a library\n\n{text}# the end\n");
        assert_eq!(Library::from_text(&with_notes, &limits()).unwrap(), lib);
    }

    #[test]
    fn a_library_that_is_not_valid_is_refused_with_the_number_of_the_line() {
        let l = limits();
        let ok = "phoenix-library 1\nname lib\npart a bar x 1 1 0 1000000 1000000\n";
        let _ = ok;
        let cases: [(&str, usize, &str); 14] = [
            ("", 0, "empty"),
            ("hello\n", 1, "not a library file"),
            ("phoenix-library 2\nname a\n", 1, "version 2"),
            ("phoenix-library 0\nname a\n", 1, "version 0"),
            ("phoenix-library 01\nname a\n", 1, "version"),
            ("phoenix-library 1\nname a b\n", 2, "not understood"),
            ("phoenix-library 1\nname a\nname b\n", 3, "twice"),
            ("phoenix-library 1\npart a bar x 1\n", 2, "before the name"),
            ("phoenix-library 1\nname a\npart a ring x 1\n", 3, "kind"),
            ("phoenix-library 1\nname a\npart a profile x 1 1\n", 3, "needs 5 values"),
            (
                "phoenix-library 1\nname a\npart a profile x 1e6 1 0 1 1\n",
                3,
                "whole number",
            ),
            (
                "phoenix-library 1\nname a\npart a profile x 20000000 20000000 0 3000000 2000000\n",
                3,
                "longer than the bar",
            ),
            (
                "phoenix-library 1\nname a\npart a profile x 20000000 20000000 0 1000000 2000000\npart a profile x 20000000 20000000 0 1000000 2000000\n",
                4,
                "listed twice",
            ),
            ("phoenix-library 1\nname a\nsomething else\n", 3, "not understood"),
        ];
        for (text, line, why) in cases {
            let e = Library::from_text(text, &l).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-044", "{text:?}");
            let shown = e.to_string();
            assert!(shown.contains(why), "{text:?}: {shown}");
            if line > 0 {
                assert!(
                    shown.contains(&format!("line {line}:")),
                    "{text:?}: {shown}"
                );
            }
        }
    }

    #[test]
    fn the_size_of_a_file_and_the_number_of_parts_are_limits_of_the_settings() {
        let mut l = limits();
        let text = example_library().to_text();
        l.max_bytes = text.len();
        assert!(Library::from_text(&text, &l).is_ok());
        l.max_bytes = text.len() - 1;
        let e = Library::from_text(&text, &l).unwrap_err();
        assert!(e.to_string().contains("more than the"), "{e}");
        let mut l = limits();
        l.max_parts = 5;
        assert!(Library::from_text(&text, &l).is_ok());
        l.max_parts = 4;
        let e = Library::from_text(&text, &l).unwrap_err();
        assert!(
            e.to_string().contains("line 7:") && e.to_string().contains("4 parts"),
            "{e}"
        );
    }

    #[test]
    fn the_materials_of_a_library_have_to_be_in_the_settings() {
        let lib = example_library();
        assert!(lib.check_materials(&densities()).is_ok());
        let mut with_unknown = lib;
        with_unknown.parts[1].material = "brass".into();
        let e = with_unknown.check_materials(&densities()).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-044");
        assert!(e.to_string().contains("brass of bar-10x30"));
    }

    #[test]
    fn an_instance_is_written_in_words_and_read_back() {
        let mut i = instance(tube(), Some(mm(300)));
        i.at = [mm(10), mm(-5), mm(2)];
        i.orientation = Orientation {
            axis: Axis::MinusY,
            roll: 3,
        };
        let words = i.words();
        assert_eq!(
            words.join(" "),
            "chassis-basic tube-20x20x2 profile aluminium 20000000 20000000 2000000 100000000 2000000000 10000000 -5000000 2000000 -y/3 300000000"
        );
        let refs: Vec<&str> = words.iter().map(String::as_str).collect();
        assert_eq!(PartInstance::from_words("piece", &refs).unwrap(), i);
        // a screw has no cut
        let s = instance(screw(Head::Hex), None);
        let words = s.words();
        assert!(words.join(" ").ends_with(" 0 0 0 +z/0 -"));
        let refs: Vec<&str> = words.iter().map(String::as_str).collect();
        assert_eq!(PartInstance::from_words("piece", &refs).unwrap(), s);
        // a word that is not canonical is not read
        for (index, replacement) in [
            (9usize, "+10000000"),
            (9, "010000000"),
            (9, "1e7"),
            (12, "+z/9"),
            (13, "05"),
        ] {
            let mut w = i.words();
            w[index] = replacement.to_string();
            let refs: Vec<&str> = w.iter().map(String::as_str).collect();
            assert!(
                PartInstance::from_words("piece", &refs).is_err(),
                "{index} {replacement}"
            );
        }
        let short: Vec<&str> = vec!["lib", "p", "profile"];
        assert!(PartInstance::from_words("piece", &short).is_err());
    }

    #[test]
    fn two_profiles_that_share_volume_are_refused_and_touching_is_allowed() {
        let l = limits();
        let first = instance(tube(), Some(mm(100)));
        let mut map: BTreeMap<Id, PartInstance> = BTreeMap::new();
        map.insert(id(1), first);
        let mut other = instance(bar(), Some(mm(100)));
        other.name = "other".into();
        // beside it: the first fills x from 0 to 20 mm, so a bar from 20 mm touches
        other.at = [mm(20), Length::ZERO, Length::ZERO];
        assert!(check_overlap(&other, map.iter(), &l).is_ok());
        other.at = [Length::new(19_999_999).unwrap(), Length::ZERO, Length::ZERO];
        let e = check_overlap(&other, map.iter(), &l).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-045");
        assert!(e.to_string().contains("other shares volume with piece"));
        // end to end along z: the first ends at 100 mm
        other.at = [Length::ZERO, Length::ZERO, mm(100)];
        assert!(check_overlap(&other, map.iter(), &l).is_ok());
        // a screw in the way of a profile is fine: it goes through a hole
        let mut s = instance(screw(Head::Cylinder), None);
        s.at = [mm(5), mm(5), mm(5)];
        assert!(check_overlap(&s, map.iter(), &l).is_ok());
        // and the check can be turned off
        let mut off = l;
        off.refuse_overlap = false;
        other.at = [Length::ZERO; 3];
        assert!(check_overlap(&other, map.iter(), &off).is_ok());
        assert!(check_overlap(&other, map.iter(), &l).is_err());
    }

    #[test]
    fn the_bill_groups_the_same_part_cut_to_the_same_length_and_adds_up() {
        let d = densities();
        let mut items: Vec<PartInstance> = Vec::new();
        for (cut, n) in [(300, 2), (150, 3), (300, 1)] {
            for _ in 0..n {
                items.push(instance(tube(), Some(mm(cut))));
            }
        }
        for _ in 0..4 {
            items.push(instance(screw(Head::Cylinder), None));
        }
        items.push(instance(bracket(um(4500)), None));
        items.push(instance(bar(), Some(mm(100))));
        let b = bill(items.iter(), &d).unwrap();
        let shown: Vec<(Kind, &str, u32, Option<i64>)> = b
            .lines
            .iter()
            .map(|l| (l.kind, l.part.as_str(), l.count, l.cut.map(Length::get)))
            .collect();
        assert_eq!(
            shown,
            vec![
                (Kind::Profile, "bar-10x30", 1, Some(100_000_000)),
                (Kind::Profile, "tube-20x20x2", 3, Some(150_000_000)),
                (Kind::Profile, "tube-20x20x2", 3, Some(300_000_000)),
                (Kind::Screw, "bolt-m3x10", 4, None),
                (Kind::Bracket, "angle-30", 1, None),
            ]
        );
        assert_eq!(b.pieces, 12);
        // the tube: 3 x 21600 + 3 x 43200 = 194400 mm3, 524.88 g
        assert_eq!(b.lines[1].volume_nm3, 64_800_000_000_000_000_000_000);
        assert_eq!(b.lines[1].mass_ug, 174_960_000);
        assert_eq!(b.lines[2].mass_ug, 349_920_000);
        // four screws of 141960718034098125000 nm3
        assert_eq!(b.lines[3].volume_nm3, 567_842_872_136_392_500_000);
        let total: i128 = b.lines.iter().map(|l| l.volume_nm3).sum();
        assert_eq!(b.volume_nm3, total);
        let mass: i128 = b.lines.iter().map(|l| l.mass_ug).sum();
        assert_eq!(b.mass_ug, mass);
        let csv = bill_csv(&b);
        let rows: Vec<&str> = csv.lines().collect();
        assert_eq!(rows[0], "kind,part,material,count,cut_mm,volume_mm3,mass_g");
        assert_eq!(
            rows[1],
            "profile,bar-10x30,aluminium,1,100,30000.000,81.000"
        );
        assert_eq!(
            rows[2],
            "profile,tube-20x20x2,aluminium,3,150,64800.000,174.960"
        );
        assert_eq!(rows[4], "screw,bolt-m3x10,steel,4,,567.843,4.458");
        assert_eq!(rows.len(), 7);
        assert!(rows[6].starts_with("total,,,12,,"));
        assert_eq!(bill_csv(&b), csv);
        // the empty bill
        let empty = bill(std::iter::empty(), &d).unwrap();
        assert_eq!((empty.pieces, empty.mass_ug), (0, 0));
        assert_eq!(
            bill_csv(&empty),
            "kind,part,material,count,cut_mm,volume_mm3,mass_g\ntotal,,,0,,0.000,0.000\n"
        );
    }

    #[test]
    fn the_cut_list_gives_the_lengths_of_each_profile_longest_first() {
        let d = densities();
        let items = vec![
            instance(tube(), Some(mm(150))),
            instance(tube(), Some(mm(300))),
            instance(tube(), Some(mm(300))),
            instance(bar(), Some(mm(100))),
            instance(screw(Head::Hex), None),
        ];
        let list = cut_list(&bill(items.iter(), &d).unwrap());
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].part, "bar-10x30");
        assert_eq!(list[1].part, "tube-20x20x2");
        assert_eq!(list[1].cuts, vec![(mm(300), 2), (mm(150), 1)]);
        assert_eq!(list[1].total_nm, 750_000_000);
        assert_eq!(list[0].total_nm, 100_000_000);
    }

    #[test]
    fn the_numbers_are_written_with_the_decimals_that_were_asked() {
        assert_eq!(volume_mm3_text(0), "0.000");
        // half a cubic micrometre-thousandth rounds away from zero
        assert_eq!(volume_mm3_text(500_000_000_000_000), "0.001");
        assert_eq!(volume_mm3_text(499_999_999_999_999), "0.000");
        assert_eq!(volume_mm3_text(-500_000_000_000_000), "-0.001");
        assert_eq!(mass_g_text(116_640_000), "116.640");
        assert_eq!(mass_g_text(500), "0.001");
        assert_eq!(mass_g_text(499), "0.000");
    }

    #[test]
    fn the_limits_of_the_settings_are_checked_when_they_are_read() {
        let base = crate::config::DEFAULT_CONFIG;
        let with = |key: &str, value: &str| {
            let mut config = Config::parse(base).unwrap();
            config.overlay(&Config::parse(&format!("[catalog]\n{key} = {value}\n")).unwrap());
            CatalogLimits::from_config(&config)
        };
        assert_eq!(with("max_parts", "512").unwrap(), limits());
        for (key, bad) in [
            ("max_bytes", "0"),
            ("max_bytes", "67108865"),
            ("max_parts", "0"),
            ("max_parts", "100001"),
            ("max_name_bytes", "257"),
            ("max_instances", "0"),
            ("max_instances", "100001"),
            ("max_dimension", "0 mm"),
            ("max_dimension", "10001 mm"),
            ("min_cut", "-1 mm"),
            ("min_cut", "11 m"),
            ("refuse_profile_overlap", "2"),
            ("max_parts", "many"),
        ] {
            let e = with(key, bad).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-011", "{key} = {bad}: {e}");
        }
        assert!(with("max_parts", "100000").is_ok());
        assert!(with("max_dimension", "10000 mm").is_ok());
        assert!(!with("refuse_profile_overlap", "0").unwrap().refuse_overlap);
        let shipped = DesignLimits::shipped().catalog;
        assert_eq!(shipped.max_instances, 1000);
        assert_eq!(shipped.max_bytes, 262_144);
        assert_eq!(shipped.max_dimension, mm(6000));
        assert!(shipped.refuse_overlap);
    }
}
