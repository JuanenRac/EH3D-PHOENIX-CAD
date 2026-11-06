//! The mechanical model of EH3D-PHOENIX-CAD.
//!
//! * [`Length`]: a length in integer nanometres inside the mechanical range (2^40 nm, about 1100 m,
//!   so a chassis larger than a board is representable: the board model has its own, smaller range);
//! * [`enclosure`]: a parametric box with round, countersunk, counterbored, rectangular and oval holes in its faces (each with the object identity of
//!   the suite, a UUID v7), rounded or chamfered edges, and the structured operations that change it. An operation that is refused leaves the model exactly as it was;
//! * [`scad`]: the model as an OpenSCAD-style script, the same text for the same model;
//! * [`step`]: the model as a STEP file (a boundary representation with the inner space as a void),
//!   the same text for the same model, checked for its structure only;
//! * [`units`]: angles, counts and ratios in integers, exact rounding, and lengths read and shown in mm, um, in and mil;
//! * [`names`]: stable names of the faces, the edges and the walls of the holes, which do not depend on the order of anything;
//! * [`body`]: the recipe of an enclosure (base parameters and ordered steps) with a tip, suppressed steps, the state of each step and an undo and redo history in named transactions;
//! * [`config`]: the settings file, the one place for tuned values (copies kept, undo depth, unit shown, printer compensation, table of screws);
//! * [`printer`]: the profile of a 3D printer and the model for fabrication with compensated holes;
//! * [`store`]: saving a file atomically with numbered copies, and loading a file under a size limit;
//! * [`mass`]: the volume of the material and of the holes, the mass for a density, the centre of mass and the moments of inertia, in integers (pi as a ratio of twelve decimals);
//! * [`mount`]: posts that rise from the floor (made from the mounting holes of a board, with a bore for an insert or a self-tapping screw and an optional foot) and a lid that is screwed onto them, with the checks of collisions and heights;
//! * [`pattern`]: rows, grids and circles of holes with exact positions, and batches of operations that apply all or nothing;
//! * [`graph`]: the graph of dependencies between objects, with a deterministic order of calculation, refused cycles, a recalculation that reuses what did not change, and a Graphviz dump;
//! * [`vars`]: named variables for the master dimensions, the binding of the parameters of an enclosure to them, and families of variants (small, medium, large);
//! * [`expr`]: the language of expressions for the parameters (`2 * wall + 1 mm`, `if(width > 80 mm, 3 mm, 2 mm)`): exact rational numbers of 128 bits with checked overflow, units and angles, functions, limits read from the settings, and errors that say where;
//! * [`parametric`]: the parameters of an enclosure as values, variables or expressions that refer to variables, to constants of the settings and to each other, calculated through the graph of dependencies;
//! * [`datum`]: reference frames with an origin and one of the 24 axis-aligned rotations, and the frame of each face of the box;
//! * [`project`]: the envelope of a project file with a format version, named domains kept byte for byte when unknown, read-only opening when a critical domain is not understood, and migrations from older versions;
//! * [`mechanical`]: the mechanical domain of the project file: the recipe of steps, the parameters and expressions, and the interface of the board, read again to the same enclosure, with a migration from the domain of the parameters;
//! * [`messages`]: the text of every diagnostic in the eight languages of the suite;
//! * [`fit`]: whether an envelope (the outline of a board and its tallest part) fits inside;
//! * [`sketch`]: the 2D sketch on a face of the box: points, lines, circles, arcs, polylines and rectangles, geometric and dimensional constraints (a dimension may be an expression over the variables), a numeric solver that reports the degrees of freedom left, the constraints that repeat or contradict each other, or that it did not converge, its settings file, and the sketch file kept next to the project;
//! * [`window`], [`scene`], [`history`], [`uiconfig`], [`lang`], [`shot`] and [`browse`]: the state of the window (the tree of the model, the properties, the
//!   views, the history of the changes), the lines it draws, its settings, its texts in the eight languages, the picture mode and the list of files; the window itself is
//!   `gui` (built with `--features gui`).
//!
//! # Example
//!
//! ```
//! use eh3d_phoenix_cad::units::parse_length;
//! use eh3d_phoenix_cad::{Enclosure, EnclosureParams, Length};
//!
//! let mm = |v| Length::from_mm(v).unwrap();
//! let enclosure = Enclosure::new(EnclosureParams {
//!     inner_length: mm(100),
//!     inner_width: mm(60),
//!     inner_height: mm(30),
//!     wall: mm(2),
//!     floor: mm(2),
//!     lid: mm(3),
//!     edge_margin: mm(3),
//!     min_web: mm(2),
//! })
//! .unwrap();
//! // the outer length is the inner one plus two walls
//! assert_eq!(enclosure.params().outer().0, parse_length("104 mm").unwrap());
//! ```
//!
//! Every failure has a stable code `PHOENIX-CAD-<NNN>` ([`CadError::code`], [`REGISTRY`]).
//! The library meshes nothing; the lines of the window are a drawing and decide nothing about the model. The STEP file has not been opened in a CAD program.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod body;
pub mod browse;
pub mod config;
pub mod datum;
pub mod enclosure;
pub mod expr;
pub mod fit;
pub mod graph;
#[cfg(feature = "gui")]
pub mod gui;
pub mod history;
pub mod lang;
pub mod mass;
pub mod mechanical;
pub mod messages;
pub mod mount;
pub mod names;
pub mod parametric;
pub mod pattern;
pub mod printer;
pub mod project;
pub mod scad;
pub mod scene;
pub mod shot;
pub mod sketch;
pub mod sketch_ui;
pub mod step;
pub mod store;
pub mod uiconfig;
pub mod units;
pub mod vars;
pub mod window;

pub use enclosure::{
    Edging, Enclosure, EnclosureParams, Face, Hole, HoleStyle, Id, Operation, Treatment,
};
pub use fit::{check_fit, Envelope, FitReport};
pub use mount::{Lid, LidFixing, Post};

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
    /// A project file that is not well formed, too large or of a newer version.
    Format(String),
    /// A treatment of the edges that does not fit the box.
    Edging(String),
    /// A hole shape (countersink, counterbore, rectangle or slot) that does not fit its hole or
    /// the material.
    HoleStyle(String),
    /// A screw size that is not in the table of screws.
    Screw(String),
    /// A shape of the model that an export does not draw yet.
    NotExported(String),
    /// An expression that is not written in the language: `pos` is the column, counted from 0.
    ExprSyntax {
        /// Where the problem is.
        pos: usize,
        /// What is wrong.
        why: String,
    },
    /// An expression that cannot be calculated (a length added to a number, a division by zero,
    /// a square root that is not exact).
    ExprEval {
        /// Where the problem is.
        pos: usize,
        /// What is wrong.
        why: String,
    },
    /// An expression that goes beyond a limit of the settings (length, depth, steps, exponent).
    ExprLimit {
        /// Where the limit was reached.
        pos: usize,
        /// Which limit.
        what: &'static str,
    },
    /// An expression whose arithmetic does not fit in the numbers used.
    ExprOverflow {
        /// Where it happened.
        pos: usize,
    },
    /// A post whose sizes do not make a post.
    PostShape(String),
    /// A post that meets a wall, another post or a hole in the floor.
    PostPlace(String),
    /// A post that is not lower than the space above it.
    PostHeight(String),
    /// A fixing of the lid that does not fit the box.
    LidFixing(String),
    /// An element of a sketch that does not hold together (a line between two points that are
    /// not points, a circle without a positive radius, a point outside the plane).
    SketchElement(String),
    /// A constraint of a sketch about elements of the wrong kind, tied to itself, or with a
    /// dimension that is not valid.
    SketchConstraint(String),
    /// A sketch with constraints that repeat or contradict each other.
    SketchOverConstrained(String),
    /// A sketch that the solver could not bring to a solution.
    SketchNoConvergence(String),
    /// A sketch file that is not valid.
    SketchFile(String),
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
            CadError::Format(_) => "PHOENIX-CAD-018",
            CadError::Edging(_) => "PHOENIX-CAD-019",
            CadError::HoleStyle(_) => "PHOENIX-CAD-020",
            CadError::Screw(_) => "PHOENIX-CAD-021",
            CadError::NotExported(_) => "PHOENIX-CAD-022",
            CadError::ExprSyntax { .. } => "PHOENIX-CAD-023",
            CadError::ExprEval { .. } => "PHOENIX-CAD-024",
            CadError::ExprLimit { .. } => "PHOENIX-CAD-025",
            CadError::ExprOverflow { .. } => "PHOENIX-CAD-026",
            CadError::PostShape(_) => "PHOENIX-CAD-027",
            CadError::PostPlace(_) => "PHOENIX-CAD-028",
            CadError::PostHeight(_) => "PHOENIX-CAD-029",
            CadError::LidFixing(_) => "PHOENIX-CAD-030",
            CadError::SketchElement(_) => "PHOENIX-CAD-031",
            CadError::SketchConstraint(_) => "PHOENIX-CAD-032",
            CadError::SketchOverConstrained(_) => "PHOENIX-CAD-033",
            CadError::SketchNoConvergence(_) => "PHOENIX-CAD-034",
            CadError::SketchFile(_) => "PHOENIX-CAD-035",
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
            CadError::HoleId(id) => write!(f, "{title}: {id}"),
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
            CadError::Format(why) => write!(f, "{title}: {why}"),
            CadError::Edging(why) => write!(f, "{title}: {why}"),
            CadError::HoleStyle(why) => write!(f, "{title}: {why}"),
            CadError::Screw(name) => write!(f, "{title}: {name:?}"),
            CadError::NotExported(what) => write!(f, "{title}: {what}"),
            CadError::ExprSyntax { pos, why } | CadError::ExprEval { pos, why } => {
                write!(f, "{title}: {why} (column {})", pos + 1)
            }
            CadError::ExprLimit { pos, what } => {
                write!(f, "{title}: {what} (column {})", pos + 1)
            }
            CadError::ExprOverflow { pos } => write!(f, "{title} (column {})", pos + 1),
            CadError::PostShape(why)
            | CadError::PostPlace(why)
            | CadError::PostHeight(why)
            | CadError::LidFixing(why)
            | CadError::SketchElement(why)
            | CadError::SketchConstraint(why)
            | CadError::SketchOverConstrained(why)
            | CadError::SketchNoConvergence(why)
            | CadError::SketchFile(why) => write!(f, "{title}: {why}"),
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
    ("PHOENIX-CAD-005", "Hole or post id used twice or unknown"),
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
    ("PHOENIX-CAD-018", "Project file not valid"),
    ("PHOENIX-CAD-019", "Edge treatment not valid"),
    ("PHOENIX-CAD-020", "Hole shape not valid"),
    ("PHOENIX-CAD-021", "Screw size not in the table"),
    ("PHOENIX-CAD-022", "Shape not drawn by this export"),
    ("PHOENIX-CAD-023", "Expression not valid"),
    ("PHOENIX-CAD-024", "Expression cannot be evaluated"),
    ("PHOENIX-CAD-025", "Expression limit exceeded"),
    ("PHOENIX-CAD-026", "Expression arithmetic overflows"),
    ("PHOENIX-CAD-027", "Post shape not valid"),
    ("PHOENIX-CAD-028", "Post meets a wall, a post or a hole"),
    ("PHOENIX-CAD-029", "Post too high for the space above it"),
    ("PHOENIX-CAD-030", "Lid fixing not valid"),
    ("PHOENIX-CAD-031", "Sketch element not valid"),
    ("PHOENIX-CAD-032", "Sketch constraint not valid"),
    ("PHOENIX-CAD-033", "Sketch over-constrained"),
    ("PHOENIX-CAD-034", "Sketch solver did not converge"),
    ("PHOENIX-CAD-035", "Sketch file not valid"),
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

    /// Regressions: each of these once looked like a place where a rule could slip.
    #[test]
    fn a_hole_exactly_at_the_margin_is_accepted_and_one_nanometre_nearer_is_refused() {
        let mm = |v| Length::from_mm(v).unwrap();
        let mut e = Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap();
        let hole = |u: i64| Hole {
            name: "edge".into(),
            face: Face::Front,
            u: Length::new(u).unwrap(),
            v: mm(17),
            diameter: mm(6),
            style: crate::enclosure::HoleStyle::Plain,
        };
        let id = eh3d_container::ObjectId::from_parts(1_700_000_000_000, 1, 1).unwrap();
        let before = e.clone();
        assert!(e
            .apply(Operation::AddHole {
                id,
                hole: hole(6_000_000 - 1)
            })
            .is_err());
        assert_eq!(e, before);
        assert!(e
            .apply(Operation::AddHole {
                id,
                hole: hole(6_000_000)
            })
            .is_ok());
    }

    #[test]
    fn removing_the_same_hole_twice_is_an_error_the_second_time() {
        let mm = |v| Length::from_mm(v).unwrap();
        let mut e = Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap();
        let id = eh3d_container::ObjectId::from_parts(1_700_000_000_000, 1, 1).unwrap();
        e.apply(Operation::AddHole {
            id,
            hole: Hole {
                name: "a".into(),
                face: Face::Top,
                u: mm(50),
                v: mm(30),
                diameter: mm(6),
                style: crate::enclosure::HoleStyle::Plain,
            },
        })
        .unwrap();
        assert!(e.apply(Operation::RemoveHole { id }).is_ok());
        assert_eq!(
            e.apply(Operation::RemoveHole { id }).unwrap_err(),
            CadError::HoleId(id)
        );
    }

    #[test]
    fn what_is_shown_in_any_unit_can_be_read_back_to_the_same_length() {
        use crate::units::{format_length, parse_length, Unit};
        for nm in [
            0i64,
            1,
            -1,
            1_587_500,
            25_400_000,
            -999_999_999,
            123_456_789_012,
        ] {
            let l = Length::new(nm).unwrap();
            for (unit, decimals) in [
                (Unit::Nanometre, 0),
                (Unit::Micrometre, 3),
                (Unit::Millimetre, 6),
                (Unit::Centimetre, 7),
                (Unit::Metre, 9),
            ] {
                // these decimals show every nanometre exactly in the unit
                let text = format_length(l, unit, decimals);
                assert_eq!(parse_length(&text).unwrap(), l, "{text}");
            }
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
            CadError::Format("cut".into()),
            CadError::Edging("too big".into()),
            CadError::HoleStyle("deep".into()),
            CadError::Screw("M99".into()),
            CadError::NotExported("countersink".into()),
            CadError::ExprSyntax {
                pos: 3,
                why: "a value was expected".into(),
            },
            CadError::ExprEval {
                pos: 0,
                why: "division by zero".into(),
            },
            CadError::ExprLimit {
                pos: 9,
                what: "depth",
            },
            CadError::ExprOverflow { pos: 4 },
            CadError::PostShape("bore".into()),
            CadError::PostPlace("wall".into()),
            CadError::PostHeight("lid".into()),
            CadError::LidFixing("inset".into()),
            CadError::SketchElement("e1".into()),
            CadError::SketchConstraint("c1".into()),
            CadError::SketchOverConstrained("c1 c2".into()),
            CadError::SketchNoConvergence("100 steps".into()),
            CadError::SketchFile("line 3".into()),
        ];
        for e in errors {
            assert!(REGISTRY.iter().any(|(c, _)| *c == e.code()));
            assert!(e
                .to_string()
                .starts_with(REGISTRY.iter().find(|(c, _)| *c == e.code()).unwrap().1));
        }
    }
}
