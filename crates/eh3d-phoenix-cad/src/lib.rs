//! The mechanical model of EH3D-PHOENIX-CAD.
//!
//! * [`Length`]: a length in integer nanometres inside the mechanical range (2^40 nm, about 1100 m,
//!   so a chassis larger than a board is representable: the board model has its own, smaller range);
//! * [`enclosure`]: a parametric box with holes in its faces (each with the object identity of
//!   the suite, a UUID v7) and the structured operations that change it. An operation that is refused leaves the model exactly as it was;
//! * [`scad`]: the model as an OpenSCAD-style script, the same text for the same model;
//! * [`step`]: the model as a STEP file (a boundary representation with the inner space as a void),
//!   the same text for the same model, checked for its structure only;
//! * [`units`]: angles, counts and ratios in integers, exact rounding, and lengths read and shown in mm, um, in and mil;
//! * [`names`]: stable names of the faces, the edges and the walls of the holes, which do not depend on the order of anything;
//! * [`body`]: the recipe of an enclosure (base parameters and ordered steps) with a tip, suppressed steps, the state of each step and an undo and redo history in named transactions;
//! * [`config`]: the settings file, the one place for tuned values (copies kept, undo depth, unit shown, printer compensation, table of screws);
//! * [`printer`]: the profile of a 3D printer and the model for fabrication with compensated holes;
//! * [`store`]: saving a file atomically with numbered copies, and loading a file under a size limit;
//! * [`mass`]: the volume of the material and of the holes and the mass for a density, in integers (pi as a ratio of twelve decimals);
//! * [`pattern`]: rows, grids and circles of holes with exact positions, and batches of operations that apply all or nothing;
//! * [`graph`]: the graph of dependencies between objects, with a deterministic order of calculation, refused cycles, a recalculation that reuses what did not change, and a Graphviz dump;
//! * [`vars`]: named variables for the master dimensions, the binding of the parameters of an enclosure to them, and families of variants (small, medium, large);
//! * [`fit`]: whether an envelope (the outline of a board and its tallest part) fits inside.
//!
//! Every failure has a stable code `PHOENIX-CAD-<NNN>` ([`CadError::code`], [`REGISTRY`]).
//! Nothing here draws or meshes; the STEP file has not been opened in a CAD program.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod body;
pub mod config;
pub mod enclosure;
pub mod fit;
pub mod graph;
pub mod mass;
pub mod names;
pub mod pattern;
pub mod printer;
pub mod scad;
pub mod step;
pub mod store;
pub mod units;
pub mod vars;

pub use enclosure::{Enclosure, EnclosureParams, Face, Hole, Id, Operation};
pub use fit::{check_fit, Envelope, FitReport};

use std::fmt;

/// The largest absolute value of a length, in nanometres (2^40).
pub const LENGTH_LIMIT_NM: i64 = 1 << 40;

/// A length in integer nanometres, strictly inside plus or minus [`LENGTH_LIMIT_NM`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Length(i64);

impl Length {
    /// Zero.
    pub const ZERO: Length = Length(0);

    /// A length from nanometres, refused when it is outside the range.
    pub fn new(nm: i64) -> Result<Length, CadError> {
        if nm > -LENGTH_LIMIT_NM && nm < LENGTH_LIMIT_NM {
            Ok(Length(nm))
        } else {
            Err(CadError::LengthOutOfRange(nm))
        }
    }

    /// A length from micrometres.
    pub fn from_um(um: i64) -> Result<Length, CadError> {
        Length::new(
            um.checked_mul(1_000)
                .ok_or(CadError::LengthOutOfRange(um))?,
        )
    }

    /// A length from whole millimetres.
    pub fn from_mm(mm: i64) -> Result<Length, CadError> {
        Length::new(
            mm.checked_mul(1_000_000)
                .ok_or(CadError::LengthOutOfRange(mm))?,
        )
    }

    /// The value in nanometres.
    pub fn get(self) -> i64 {
        self.0
    }

    /// The sum, refused when it leaves the range.
    pub fn checked_add(self, other: Length) -> Result<Length, CadError> {
        Length::new(self.0 + other.0)
    }

    /// The difference, refused when it leaves the range.
    pub fn checked_sub(self, other: Length) -> Result<Length, CadError> {
        Length::new(self.0 - other.0)
    }
}

/// Why a model or an operation is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CadError {
    /// A length is outside the mechanical range.
    LengthOutOfRange(i64),
    /// A parameter that must be positive is not.
    NotPositive(&'static str),
    /// A hole does not lie inside its face with the margin from the edge.
    HoleOutsideFace(String),
    /// Two holes of one face are closer than the minimum web.
    HolesOverlap(String, String),
    /// A hole id is used twice, or an operation names a hole that does not exist.
    HoleId(eh3d_container::ObjectId),
    /// An angle is outside the range of angles.
    AngleOutOfRange(i64),
    /// A count or a ratio is outside its range.
    ValueOutOfRange(&'static str),
    /// A text with a number and a unit is not understood.
    UnitText(String),
    /// An integer calculation would overflow, or divides by zero.
    Overflow(&'static str),
    /// A name of a face, edge or hole wall that does not exist or is not written in the canonical way.
    Reference(String),
    /// A settings file or value that is not valid.
    Config(String),
    /// A file could not be read or written, or is larger than allowed.
    Io(String),
    /// A dependency that would make an object depend on itself.
    Cycle(String),
    /// A name or an id that no object, variable or domain has.
    UnknownName(String),
    /// A name or an id that is already taken.
    Duplicate(String),
    /// An object that others still depend on.
    InUse(String),
    /// A name of a variable or a variant that is not a plain word.
    InvalidName(String),
}

impl CadError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            CadError::LengthOutOfRange(_) => "PHOENIX-CAD-001",
            CadError::NotPositive(_) => "PHOENIX-CAD-002",
            CadError::HoleOutsideFace(_) => "PHOENIX-CAD-003",
            CadError::HolesOverlap(_, _) => "PHOENIX-CAD-004",
            CadError::HoleId(_) => "PHOENIX-CAD-005",
            CadError::AngleOutOfRange(_) => "PHOENIX-CAD-006",
            CadError::ValueOutOfRange(_) => "PHOENIX-CAD-007",
            CadError::UnitText(_) => "PHOENIX-CAD-008",
            CadError::Overflow(_) => "PHOENIX-CAD-009",
            CadError::Reference(_) => "PHOENIX-CAD-010",
            CadError::Config(_) => "PHOENIX-CAD-011",
            CadError::Io(_) => "PHOENIX-CAD-012",
            CadError::Cycle(_) => "PHOENIX-CAD-013",
            CadError::UnknownName(_) => "PHOENIX-CAD-014",
            CadError::Duplicate(_) => "PHOENIX-CAD-015",
            CadError::InUse(_) => "PHOENIX-CAD-016",
            CadError::InvalidName(_) => "PHOENIX-CAD-017",
        }
    }
}

impl fmt::Display for CadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let title = REGISTRY
            .iter()
            .find(|(c, _)| *c == self.code())
            .map_or("", |(_, t)| *t);
        match self {
            CadError::LengthOutOfRange(v) => write!(f, "{title}: {v} nm"),
            CadError::NotPositive(what) => write!(f, "{title}: {what}"),
            CadError::HoleOutsideFace(h) => write!(f, "{title}: hole {h}"),
            CadError::HolesOverlap(a, b) => write!(f, "{title}: holes {a} and {b}"),
            CadError::HoleId(id) => write!(f, "{title}: hole {id}"),
            CadError::AngleOutOfRange(v) => write!(f, "{title}: {v} nano-degrees"),
            CadError::ValueOutOfRange(what) => write!(f, "{title}: {what}"),
            CadError::UnitText(text) => write!(f, "{title}: {text:?}"),
            CadError::Overflow(what) => write!(f, "{title}: {what}"),
            CadError::Reference(name) => write!(f, "{title}: {name:?}"),
            CadError::Config(why) => write!(f, "{title}: {why}"),
            CadError::Io(why) => write!(f, "{title}: {why}"),
            CadError::Cycle(what) => write!(f, "{title}: {what}"),
            CadError::UnknownName(what) => write!(f, "{title}: {what}"),
            CadError::Duplicate(what) => write!(f, "{title}: {what}"),
            CadError::InUse(what) => write!(f, "{title}: {what}"),
            CadError::InvalidName(what) => write!(f, "{title}: {what:?}"),
        }
    }
}

impl std::error::Error for CadError {}

/// Every code with its title, in order.
pub const REGISTRY: &[(&str, &str)] = &[
    ("PHOENIX-CAD-001", "Length outside the mechanical range"),
    ("PHOENIX-CAD-002", "Parameter is not positive"),
    ("PHOENIX-CAD-003", "Hole outside its face"),
    ("PHOENIX-CAD-004", "Holes too close"),
    ("PHOENIX-CAD-005", "Hole id used twice or unknown"),
    ("PHOENIX-CAD-006", "Angle outside the range"),
    ("PHOENIX-CAD-007", "Count or ratio outside its range"),
    ("PHOENIX-CAD-008", "Number with a unit not understood"),
    ("PHOENIX-CAD-009", "Integer calculation overflows"),
    (
        "PHOENIX-CAD-010",
        "Reference to an element that does not exist",
    ),
    ("PHOENIX-CAD-011", "Settings not valid"),
    ("PHOENIX-CAD-012", "File cannot be read or written"),
    ("PHOENIX-CAD-013", "Dependency cycle"),
    ("PHOENIX-CAD-014", "Unknown name or id"),
    ("PHOENIX-CAD-015", "Name or id already used"),
    ("PHOENIX-CAD-016", "Object still in use"),
    ("PHOENIX-CAD-017", "Name not valid"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_have_their_own_larger_range() {
        assert!(Length::new(LENGTH_LIMIT_NM - 1).is_ok());
        assert!(Length::new(-(LENGTH_LIMIT_NM - 1)).is_ok());
        assert_eq!(
            Length::new(LENGTH_LIMIT_NM),
            Err(CadError::LengthOutOfRange(LENGTH_LIMIT_NM))
        );
        // a chassis of 5 m is far beyond the 2^30 nm of the board model and fine here
        assert!(Length::from_mm(5_000).is_ok());
        assert!(Length::from_mm(i64::MAX).is_err());
        assert!(Length::from_um(i64::MAX).is_err());
        let a = Length::from_mm(1).unwrap();
        assert_eq!(a.checked_add(a).unwrap().get(), 2_000_000);
        assert_eq!(a.checked_sub(a).unwrap(), Length::ZERO);
    }

    #[test]
    fn the_codes_are_unique_well_formed_and_documented() {
        let mut seen = std::collections::BTreeSet::new();
        let doc = include_str!("../../../docs/ERROR_CODES.md");
        for (code, title) in REGISTRY {
            let parts: Vec<&str> = code.split('-').collect();
            assert_eq!((parts.len(), parts[0], parts[1]), (3, "PHOENIX", "CAD"));
            assert!(parts[2].len() == 3 && parts[2].bytes().all(|b| b.is_ascii_digit()));
            assert!(seen.insert(*code));
            let row = doc
                .lines()
                .find(|l| l.starts_with(&format!("| {code} |")))
                .unwrap_or_else(|| panic!("{code} is not documented"));
            assert!(row.contains(title), "{code}: {row}");
        }
    }

    #[test]
    fn every_error_has_a_registered_code_and_a_message() {
        let errors = [
            CadError::LengthOutOfRange(1),
            CadError::NotPositive("wall"),
            CadError::HoleOutsideFace("a".into()),
            CadError::HolesOverlap("a".into(), "b".into()),
            CadError::HoleId(eh3d_container::ObjectId::from_parts(1, 2, 3).unwrap()),
            CadError::AngleOutOfRange(1),
            CadError::ValueOutOfRange("count"),
            CadError::UnitText("x".into()),
            CadError::Overflow("scale"),
            CadError::Reference("box:face:x".into()),
            CadError::Config("line 1".into()),
            CadError::Io("disk".into()),
            CadError::Cycle("x".into()),
            CadError::UnknownName("x".into()),
            CadError::Duplicate("x".into()),
            CadError::InUse("x".into()),
            CadError::InvalidName("x y".into()),
        ];
        for e in errors {
            assert!(REGISTRY.iter().any(|(c, _)| *c == e.code()));
            assert!(e
                .to_string()
                .starts_with(REGISTRY.iter().find(|(c, _)| *c == e.code()).unwrap().1));
        }
    }
}
