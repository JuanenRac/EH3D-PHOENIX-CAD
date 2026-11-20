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

use crate::assembly::AssemblyLimits;
use crate::catalog::CatalogLimits;
use crate::clearance::ClearanceSettings;
use crate::enclosure::{Face, Hole, HoleStyle};
use crate::expr::{parse, Constants, ExprLimits, Value};
use crate::nesting::StockSettings;
use crate::placement::PlacementSettings;
use crate::printer::PrinterProfile;
use crate::titleblock::TitleBlockSettings;
use crate::units::{parse_length, Unit};
use crate::{CadError, Length};

/// The default settings, as shipped.
pub const DEFAULT_CONFIG: &str = include_str!("../phoenix-cad.default.cfg");

/// The most copies that a save may keep.
pub const MAX_BACKUPS: u32 = 99;

/// The most transactions that the history may keep.
pub const MAX_UNDO_DEPTH: usize = 10_000;

/// The limits of the domain, read from the settings file (`[mechanical]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MechanicalLimits {
    /// The most bytes of the domain.
    pub max_bytes: usize,
    /// The most steps.
    pub max_steps: usize,
    /// The longest name, in bytes, before it is escaped.
    pub max_name_bytes: usize,
    /// The most points of the outline of the board.
    pub max_outline_points: usize,
    /// The most mounting holes of the board.
    pub max_board_holes: usize,
}

impl MechanicalLimits {
    /// Limits checked against the hard caps: each one is at least 1 and at most its cap.
    pub fn new(
        max_bytes: u32,
        max_steps: u32,
        max_name_bytes: u32,
        max_outline_points: u32,
        max_board_holes: u32,
    ) -> Result<MechanicalLimits, CadError> {
        let mut out = [0usize; 5];
        for (i, (key, value, cap)) in [
            ("max_bytes", max_bytes, 1u32 << 28),
            ("max_steps", max_steps, 1_000_000),
            ("max_name_bytes", max_name_bytes, 4_096),
            ("max_outline_points", max_outline_points, 1_000_000),
            ("max_board_holes", max_board_holes, 100_000),
        ]
        .into_iter()
        .enumerate()
        {
            if value == 0 || value > cap {
                return Err(CadError::Config(format!(
                    "mechanical.{key} must be from 1 to {cap}"
                )));
            }
            out[i] = usize::try_from(value)
                .map_err(|_| CadError::Config(format!("mechanical.{key} is too large")))?;
        }
        Ok(MechanicalLimits {
            max_bytes: out[0],
            max_steps: out[1],
            max_name_bytes: out[2],
            max_outline_points: out[3],
            max_board_holes: out[4],
        })
    }
}

/// The limits of the operations that repeat things or grow the box, read from the settings file
/// (`[pattern]` and `[fins]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesignLimits {
    /// The most positions of one pattern of holes or posts.
    pub pattern_max: u32,
    /// The most positions of one pattern of parts of the catalog.
    pub part_pattern_max: u32,
    /// The most rows of any table of the order sheet.
    pub order_max_rows: u32,
    /// The most fins of one set of cooling fins.
    pub fin_max_count: u32,
    /// The thinnest fin.
    pub fin_min_thickness: Length,
    /// The least air between two fins.
    pub fin_min_gap: Length,
    /// The tallest fin, as a multiple of its thickness.
    pub fin_max_ratio: u32,
    /// The limits of the catalog of parts and of the instances placed in a model.
    pub catalog: CatalogLimits,
    /// The limits of the mates between placed parts and of the report of the interferences.
    pub assembly: AssemblyLimits,
}

impl DesignLimits {
    /// The most that any limit can be set to: a pattern is checked position by position against
    /// the others, so the work grows with the square of the count.
    pub const PATTERN_CAP: u32 = 10_000;

    /// The most that the count of the fins of a set can be set to.
    pub const FIN_CAP: u32 = 10_000;

    /// The most that the ratio of the height of a fin to its thickness can be set to.
    pub const FIN_RATIO_CAP: u32 = 1_000;

    /// The limits of the shipped settings.
    pub fn shipped() -> DesignLimits {
        Settings::defaults().design
    }

    fn from_config(c: &Config) -> Result<DesignLimits, CadError> {
        let pattern_max = c.number("pattern.max_count")?;
        if !(2..=Self::PATTERN_CAP).contains(&pattern_max) {
            return Err(CadError::Config(format!(
                "pattern.max_count must be from 2 to {}",
                Self::PATTERN_CAP
            )));
        }
        let part_pattern_max = c.number("pattern.max_parts")?;
        if !(2..=Self::PATTERN_CAP).contains(&part_pattern_max) {
            return Err(CadError::Config(format!(
                "pattern.max_parts must be from 2 to {}",
                Self::PATTERN_CAP
            )));
        }
        let order_max_rows = c.number("order.max_rows")?;
        if !(1..=1_000_000).contains(&order_max_rows) {
            return Err(CadError::Config(
                "order.max_rows must be from 1 to 1000000".to_string(),
            ));
        }
        let fin_max_count = c.number("fins.max_count")?;
        if !(1..=Self::FIN_CAP).contains(&fin_max_count) {
            return Err(CadError::Config(format!(
                "fins.max_count must be from 1 to {}",
                Self::FIN_CAP
            )));
        }
        let fin_max_ratio = c.number("fins.max_height_ratio")?;
        if !(1..=Self::FIN_RATIO_CAP).contains(&fin_max_ratio) {
            return Err(CadError::Config(format!(
                "fins.max_height_ratio must be from 1 to {}",
                Self::FIN_RATIO_CAP
            )));
        }
        let fin_min_thickness = c.length("fins.min_thickness")?;
        let fin_min_gap = c.length("fins.min_gap")?;
        for (what, v) in [
            ("fins.min_thickness", fin_min_thickness),
            ("fins.min_gap", fin_min_gap),
        ] {
            if v.get() <= 0 {
                return Err(CadError::Config(format!("{what} must be positive")));
            }
        }
        Ok(DesignLimits {
            pattern_max,
            part_pattern_max,
            order_max_rows,
            fin_max_count,
            fin_min_thickness,
            fin_min_gap,
            fin_max_ratio,
            catalog: CatalogLimits::from_config(c)?,
            assembly: AssemblyLimits::from_config(c)?,
        })
    }
}

/// The values of the estimate of the heat that cooling fins give to the air (`[thermal]`), in
/// thousandths of their unit so that the settings stay whole numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThermalSettings {
    /// The coefficient of heat transfer to still air, in milliwatts per square metre and kelvin.
    pub convection_mw_m2k: u32,
    /// The conductivity of the material of the fins, in milliwatts per metre and kelvin.
    pub conductivity_mw_mk: u32,
    /// How many millikelvin the base of the fins is warmer than the air.
    pub delta_t_mk: u32,
    /// The emissivity of the surface of the fins in thousandths (from 0 to 1000); 0 leaves the
    /// radiation out of the estimate.
    pub emissivity_milli: u32,
    /// The temperature of the air in millikelvin (the radiation depends on the absolute
    /// temperature).
    pub ambient_mk: u32,
    /// Whether the opening between two fins radiates as a cavity, with the view factor from the
    /// surfaces inside it to the opening (the radiation is then more than the outer envelope
    /// alone and less than all the surface); off leaves the estimate as the outer envelope only.
    pub fin_view_factor: bool,
}

impl ThermalSettings {
    /// The most any of the three can be set to, in thousandths: a thousand units.
    pub const CAP_MILLI: u32 = 1_000_000;

    fn from_config(c: &Config) -> Result<ThermalSettings, CadError> {
        let get = |key: &str| -> Result<u32, CadError> {
            let v = c.milli(key)?;
            if v == 0 || v > Self::CAP_MILLI {
                return Err(CadError::Config(format!(
                    "{key} must be above 0 and at most {}",
                    Self::CAP_MILLI / 1_000
                )));
            }
            Ok(v)
        };
        let emissivity_milli = c.milli("thermal.emissivity")?;
        if emissivity_milli > 1_000 {
            return Err(CadError::Config(
                "thermal.emissivity must be from 0 to 1".into(),
            ));
        }
        let ambient_mk = c.milli("thermal.ambient_k")?;
        if !(Self::AMBIENT_MIN_MK..=Self::AMBIENT_MAX_MK).contains(&ambient_mk) {
            return Err(CadError::Config(
                "thermal.ambient_k must be from 200 to 500".into(),
            ));
        }
        let view_factor = c.number("thermal.fin_view_factor")?;
        if view_factor > 1 {
            return Err(CadError::Config(
                "thermal.fin_view_factor must be 0 or 1".into(),
            ));
        }
        Ok(ThermalSettings {
            convection_mw_m2k: get("thermal.convection_w_m2k")?,
            conductivity_mw_mk: get("thermal.conductivity_w_mk")?,
            delta_t_mk: get("thermal.delta_t_k")?,
            emissivity_milli,
            ambient_mk,
            fin_view_factor: view_factor == 1,
        })
    }

    /// The least temperature of the air that is accepted, in millikelvin (200 K).
    pub const AMBIENT_MIN_MK: u32 = 200_000;
    /// The greatest temperature of the air that is accepted, in millikelvin (500 K).
    pub const AMBIENT_MAX_MK: u32 = 500_000;

    /// The emissivity of the fins, from 0 to 1.
    pub fn emissivity(&self) -> f64 {
        f64::from(self.emissivity_milli) / 1_000.0
    }

    /// The temperature of the air in kelvin.
    pub fn ambient_k(&self) -> f64 {
        f64::from(self.ambient_mk) / 1_000.0
    }

    /// The coefficient of heat transfer in watts per square metre and kelvin.
    pub fn convection_w_m2k(&self) -> f64 {
        f64::from(self.convection_mw_m2k) / 1_000.0
    }

    /// The conductivity of the fins in watts per metre and kelvin.
    pub fn conductivity_w_mk(&self) -> f64 {
        f64::from(self.conductivity_mw_mk) / 1_000.0
    }

    /// The temperature of the base over the air, in kelvin.
    pub fn delta_t_k(&self) -> f64 {
        f64::from(self.delta_t_mk) / 1_000.0
    }
}

/// The settings of the solid made for the STL file (`[mesh]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshSettings {
    /// The sides of every circle (a multiple of four, so that a quarter of a circle has whole sides).
    pub circle_segments: u32,
    /// The most triangles of one export.
    pub max_triangles: u32,
    /// Whether the STL file is binary (1) or text (0).
    pub binary: bool,
    /// How far, in parts per million of the volume of the model, the volume of the triangles may
    /// be from the exact volume.
    pub volume_tolerance_ppm: u32,
    /// The largest STEP file that is read, in kibibytes.
    pub step_max_kib: u32,
    /// The most entities (lines of the data section) of a STEP file that is read.
    pub step_max_entities: u32,
    /// The most faces of a STEP file that is read.
    pub step_max_faces: u32,
}

impl MeshSettings {
    /// The most kibibytes of a STEP file that can be asked for.
    pub const STEP_KIB_CAP: u32 = 1_048_576;
    /// The most entities of a STEP file that can be asked for.
    pub const STEP_ENTITIES_CAP: u32 = 50_000_000;
    /// The most faces of a STEP file that can be asked for.
    pub const STEP_FACES_CAP: u32 = 10_000_000;
    /// The least sides of a circle.
    pub const MIN_SEGMENTS: u32 = 8;
    /// The most sides of a circle.
    pub const MAX_SEGMENTS: u32 = 720;
    /// The most triangles that can be asked for.
    pub const TRIANGLES_CAP: u32 = 20_000_000;
    /// The widest tolerance of the volume: one tenth.
    pub const TOLERANCE_CAP_PPM: u32 = 100_000;

    fn from_config(c: &Config) -> Result<MeshSettings, CadError> {
        let circle_segments = c.number("mesh.circle_segments")?;
        if !(Self::MIN_SEGMENTS..=Self::MAX_SEGMENTS).contains(&circle_segments)
            || circle_segments % 4 != 0
        {
            return Err(CadError::Config(format!(
                "mesh.circle_segments must be a multiple of 4 from {} to {}",
                Self::MIN_SEGMENTS,
                Self::MAX_SEGMENTS
            )));
        }
        let max_triangles = c.number("mesh.max_triangles")?;
        if !(12..=Self::TRIANGLES_CAP).contains(&max_triangles) {
            return Err(CadError::Config(format!(
                "mesh.max_triangles must be from 12 to {}",
                Self::TRIANGLES_CAP
            )));
        }
        let binary = match c.number("mesh.stl_binary")? {
            0 => false,
            1 => true,
            _ => return Err(CadError::Config("mesh.stl_binary must be 0 or 1".into())),
        };
        let volume_tolerance_ppm = c.number("mesh.volume_tolerance_ppm")?;
        if !(1..=Self::TOLERANCE_CAP_PPM).contains(&volume_tolerance_ppm) {
            return Err(CadError::Config(format!(
                "mesh.volume_tolerance_ppm must be from 1 to {}",
                Self::TOLERANCE_CAP_PPM
            )));
        }
        let step_max_kib = c.number("mesh.step_max_kib")?;
        if !(1..=Self::STEP_KIB_CAP).contains(&step_max_kib) {
            return Err(CadError::Config(format!(
                "mesh.step_max_kib must be from 1 to {}",
                Self::STEP_KIB_CAP
            )));
        }
        let step_max_entities = c.number("mesh.step_max_entities")?;
        if !(10..=Self::STEP_ENTITIES_CAP).contains(&step_max_entities) {
            return Err(CadError::Config(format!(
                "mesh.step_max_entities must be from 10 to {}",
                Self::STEP_ENTITIES_CAP
            )));
        }
        let step_max_faces = c.number("mesh.step_max_faces")?;
        if !(6..=Self::STEP_FACES_CAP).contains(&step_max_faces) {
            return Err(CadError::Config(format!(
                "mesh.step_max_faces must be from 6 to {}",
                Self::STEP_FACES_CAP
            )));
        }
        Ok(MeshSettings {
            circle_segments,
            max_triangles,
            binary,
            volume_tolerance_ppm,
            step_max_kib,
            step_max_entities,
            step_max_faces,
        })
    }
}

/// The settings of the flat drawings, DXF and SVG (`[drawing]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawingSettings {
    /// The most entities (lines, circles and arcs) of one drawing.
    pub max_entities: u32,
    /// The width of the page of the SVG drawing, which is scaled to fit it.
    pub svg_width: Length,
    /// Whether the SVG drawing has the dimensions of the box and the sizes of the holes (1) or
    /// not (0).
    pub dimensions: bool,
    /// How far the centre lines go past what they mark.
    pub axis_overshoot: Length,
    /// Whether the DXF drawing of a view has the dimensions too, on the layer `COTAS` (1), or only
    /// the outline, the openings and the centre lines (0). The SVG drawing has them when
    /// `dimensions` is 1.
    pub dxf_dimensions: bool,
    /// Whether the positions of the holes of the face that a view looks at are dimensioned from
    /// the lower left corner of the box (1) or only the size of the box is (0).
    pub hole_dimensions: bool,
    /// The most dimensions of the positions of the holes in one view; more is refused.
    pub max_dimensions: u32,
    /// The distance between the hatching lines of a section drawing, measured across them; zero
    /// leaves the hatching out.
    pub hatch_spacing: Length,
    /// Whether the drawing of a view has a legend with the list of its dimensions (the size of the
    /// box, the thickness of its walls, floor and lid, and the tag, diameter and position of each
    /// hole of the face the view looks at) to the right of the box (1) or not (0). It is written
    /// where the dimensions are.
    pub legend: bool,
    /// The room between the box and the legend, from 0 to 1000 mm.
    pub legend_gap: Length,
    /// Whether the drawing of a view draws the cavity of the box (the inside of its walls, floor
    /// and lid) as dashed hidden lines on their own layer (1) or not (0).
    pub hidden_lines: bool,
    /// Whether the drawing of a view has a border (layer `MARCO`) round the views, their
    /// dimensions and their legend (1) or not (0).
    pub frame: bool,
    /// The air between what is drawn and the border, from 0 to 1000 mm.
    pub frame_margin: Length,
    /// Where the sheet of several views puts the plan and the side: 1 is first-angle projection
    /// (the plan under the front and the side to the left of it), 3 third-angle projection (the
    /// plan over the front and the side to the right of it).
    pub sheet_angle: u8,
    /// The air between two views on the sheet, from 0 to 1000 mm, besides the room their
    /// dimension lines take.
    pub sheet_gap: Length,
    /// Whether the legend of the sheet of views lists the holes of every view under the name of
    /// the view (1) or only those of the front (0).
    pub sheet_legends: bool,
    /// Whether the sheet of views has the symbol of the projection (first or third angle) under
    /// its lower left corner (1) or not (0).
    pub sheet_symbol: bool,
    /// The height of the large end of that symbol, from 4 to 50 mm.
    pub sheet_symbol_size: Length,
    /// The paper of the SVG drawing: 0 scales the drawing to `svg_width`, 1 to 5 put it on a
    /// sheet of the A series (A1 to A5) at the largest standard scale that fits, centred.
    pub paper: u8,
    /// The air between the drawing and the edge of the paper, from 0 to 100 mm.
    pub paper_margin: Length,
    /// Whether the paper is turned to stand upright (1) or lies across (0).
    pub paper_portrait: bool,
}

impl DrawingSettings {
    /// The most entities that can be asked for.
    pub const ENTITIES_CAP: u32 = 5_000_000;

    /// The most dimensions of positions that can be asked for.
    pub const DIMENSIONS_CAP: u32 = 1000;

    fn from_config(c: &Config) -> Result<DrawingSettings, CadError> {
        let max_entities = c.number("drawing.max_entities")?;
        if !(8..=Self::ENTITIES_CAP).contains(&max_entities) {
            return Err(CadError::Config(format!(
                "drawing.max_entities must be from 8 to {}",
                Self::ENTITIES_CAP
            )));
        }
        let svg_width = c.length("drawing.svg_width")?;
        if svg_width.get() < 10_000_000 || svg_width.get() > 10_000_000_000 {
            return Err(CadError::Config(
                "drawing.svg_width must be from 10 mm to 10000 mm".into(),
            ));
        }
        let dimensions = match c.number("drawing.dimensions")? {
            0 => false,
            1 => true,
            _ => return Err(CadError::Config("drawing.dimensions must be 0 or 1".into())),
        };
        let axis_overshoot = c.length("drawing.axis_overshoot")?;
        if axis_overshoot.get() < 0 || axis_overshoot.get() > 1_000_000_000 {
            return Err(CadError::Config(
                "drawing.axis_overshoot must be from 0 mm to 1000 mm".into(),
            ));
        }
        let flag = |key: &str| -> Result<bool, CadError> {
            match c.number(key)? {
                0 => Ok(false),
                1 => Ok(true),
                _ => Err(CadError::Config(format!("{key} must be 0 or 1"))),
            }
        };
        let hatch_spacing = c.length("drawing.hatch_spacing")?;
        if hatch_spacing.get() < 0
            || (hatch_spacing.get() > 0 && hatch_spacing.get() < 200_000)
            || hatch_spacing.get() > 100_000_000
        {
            return Err(CadError::Config(
                "drawing.hatch_spacing must be 0 or from 0.2 mm to 100 mm".into(),
            ));
        }
        let max_dimensions = c.number("drawing.max_dimensions")?;
        if !(1..=Self::DIMENSIONS_CAP).contains(&max_dimensions) {
            return Err(CadError::Config(format!(
                "drawing.max_dimensions must be from 1 to {}",
                Self::DIMENSIONS_CAP
            )));
        }
        let legend_gap = c.length("drawing.legend_gap")?;
        if legend_gap.get() < 0 || legend_gap.get() > 1_000_000_000 {
            return Err(CadError::Config(
                "drawing.legend_gap must be from 0 mm to 1000 mm".into(),
            ));
        }
        let frame_margin = c.length("drawing.frame_margin")?;
        if frame_margin.get() < 0 || frame_margin.get() > 1_000_000_000 {
            return Err(CadError::Config(
                "drawing.frame_margin must be from 0 mm to 1000 mm".into(),
            ));
        }
        let sheet_angle = match c.number("drawing.sheet_angle")? {
            1 => 1,
            3 => 3,
            _ => {
                return Err(CadError::Config(
                    "drawing.sheet_angle must be 1 or 3".into(),
                ))
            }
        };
        let sheet_gap = c.length("drawing.sheet_gap")?;
        if sheet_gap.get() < 0 || sheet_gap.get() > 1_000_000_000 {
            return Err(CadError::Config(
                "drawing.sheet_gap must be from 0 mm to 1000 mm".into(),
            ));
        }
        let paper = c.number("drawing.paper")?;
        if paper > 5 {
            return Err(CadError::Config(
                "drawing.paper must be 0 (none) or 1 to 5 (A1 to A5)".into(),
            ));
        }
        let paper_margin = c.length("drawing.paper_margin")?;
        if paper_margin.get() < 0 || paper_margin.get() > 100_000_000 {
            return Err(CadError::Config(
                "drawing.paper_margin must be from 0 mm to 100 mm".into(),
            ));
        }
        let sheet_symbol_size = c.length("drawing.sheet_symbol_size")?;
        if sheet_symbol_size.get() < 4_000_000 || sheet_symbol_size.get() > 50_000_000 {
            return Err(CadError::Config(
                "drawing.sheet_symbol_size must be from 4 mm to 50 mm".into(),
            ));
        }
        Ok(DrawingSettings {
            max_entities,
            svg_width,
            dimensions,
            axis_overshoot,
            dxf_dimensions: flag("drawing.dxf_dimensions")?,
            hole_dimensions: flag("drawing.hole_dimensions")?,
            max_dimensions,
            hatch_spacing,
            legend: flag("drawing.legend")?,
            legend_gap,
            hidden_lines: flag("drawing.hidden_lines")?,
            frame: flag("drawing.frame")?,
            frame_margin,
            sheet_angle,
            sheet_gap,
            sheet_legends: flag("drawing.sheet_legends")?,
            sheet_symbol: flag("drawing.sheet_symbol")?,
            sheet_symbol_size,
            paper: paper as u8,
            paper_margin,
            paper_portrait: flag("drawing.paper_portrait")?,
        })
    }
}

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

    /// A number from 0 up with at most three decimals (a point or a comma), as whole thousandths:
    /// `8` is 8000 and `0.25` is 250. The key must exist.
    pub fn milli(&self, key: &str) -> Result<u32, CadError> {
        let text = self.need(key)?;
        let bad = || CadError::Config(format!("{key} is not a number with at most three decimals"));
        let (int, frac) = match text.split_once(['.', ',']) {
            Some((a, b)) => (a, b),
            None => (text, ""),
        };
        let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if int.is_empty() || int.len() > 6 || frac.len() > 3 || !digits(int) || !digits(frac) {
            return Err(bad());
        }
        let whole: u32 = int.parse().map_err(|_| bad())?;
        let fraction: u32 = if frac.is_empty() {
            0
        } else {
            format!("{frac:0<3}").parse().map_err(|_| bad())?
        };
        whole
            .checked_mul(1_000)
            .and_then(|w| w.checked_add(fraction))
            .ok_or_else(bad)
    }

    /// A whole number from 0 up; the key must exist.
    pub fn number(&self, key: &str) -> Result<u32, CadError> {
        self.need(key)?
            .parse::<u32>()
            .map_err(|_| CadError::Config(format!("{key} is not a whole number")))
    }
}

/// How the bill of materials with cost is priced (`[cost]` and the `price_per_kg` of each
/// `[material.*]`). Money is whole thousandths of the currency, so the sums are exact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostSettings {
    /// The code of the currency, two to eight capital letters (`EUR`); it is only written next to
    /// the figures.
    pub currency: String,
    /// What each piece costs besides its material (cutting, drilling, handling), in thousandths.
    pub per_piece_milli: u32,
    /// The percentage added to the sum of the lines (waste, margin), from 0 to 1000.
    pub markup_percent: u32,
    /// The price of a kilogram of each material that has one, in thousandths; a material without
    /// a price is left out of the cost and the bill says so.
    pub prices_milli_per_kg: BTreeMap<String, u32>,
}

impl CostSettings {
    /// The most that the markup can be set to, in percent.
    pub const MARKUP_CAP: u32 = 1000;

    /// The settings of the shipped file.
    pub fn shipped() -> CostSettings {
        Settings::defaults().cost
    }

    /// Reads and checks the keys of `[cost]` and the prices of the materials (`densities` are the
    /// materials the settings know: a price for another one is refused).
    pub fn from_config(
        c: &Config,
        densities: &BTreeMap<String, u32>,
    ) -> Result<CostSettings, CadError> {
        let currency = c
            .get("cost.currency")
            .ok_or_else(|| CadError::Config("cost.currency is missing".into()))?;
        if !(2..=8).contains(&currency.len()) || !currency.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(CadError::Config(
                "cost.currency must be two to eight capital letters".into(),
            ));
        }
        let markup_percent = c.number("cost.markup_percent")?;
        if markup_percent > Self::MARKUP_CAP {
            return Err(CadError::Config(format!(
                "cost.markup_percent must be from 0 to {}",
                Self::MARKUP_CAP
            )));
        }
        let mut prices = BTreeMap::new();
        for key in c.keys() {
            if let Some(name) = key
                .strip_prefix("material.")
                .and_then(|rest| rest.strip_suffix(".price_per_kg"))
            {
                if !densities.contains_key(name) {
                    return Err(CadError::Config(format!(
                        "{key}: {name} is not a material of the settings"
                    )));
                }
                prices.insert(name.to_string(), c.milli(key)?);
            }
        }
        Ok(CostSettings {
            currency: currency.to_string(),
            per_piece_milli: c.milli("cost.per_piece")?,
            markup_percent,
            prices_milli_per_kg: prices,
        })
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

/// What a post for a screw of the table needs: the outer diameter of the post and the bore for a
/// threaded insert or for the thread of a self-tapping screw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fixing {
    /// The outer diameter of the post.
    pub post_diameter: Length,
    /// The diameter of the bore for a threaded insert.
    pub insert_diameter: Length,
    /// The depth of the bore for a threaded insert.
    pub insert_depth: Length,
    /// The diameter of the pilot hole for a self-tapping screw.
    pub pilot_diameter: Length,
    /// The depth of the pilot hole.
    pub pilot_depth: Length,
}

/// The values that every post shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostSettings {
    /// The least wall between the bore and the outside of a post.
    pub min_wall: Length,
    /// Added to the diameter of a post for the diameter of its reinforced foot.
    pub base_extra: Length,
    /// The height of the reinforced foot.
    pub base_height: Length,
}

/// The values of the screwed lid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LidSettings {
    /// The thickness of the centring lip.
    pub lip_thickness: Length,
    /// How far the centring lip hangs below the lid.
    pub lip_depth: Length,
    /// The distance from a corner screw to the two walls of its corner.
    pub corner_inset: Length,
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
    /// The posts for each screw of the table that has an entry (`[fixing.M3]`), by name.
    pub fixings: BTreeMap<String, Fixing>,
    /// The values that every post shares.
    pub posts: PostSettings,
    /// The values of the screwed lid.
    pub lid: LidSettings,
    /// The density in kilograms per cubic metre of each material by name (`PLA`).
    pub materials: BTreeMap<String, u32>,
    /// The limits of the language of expressions.
    pub expr: ExprLimits,
    /// The limits of the mechanical domain of the project file.
    pub mechanical: MechanicalLimits,
    /// The limits of the operations that repeat things or grow the box.
    pub design: DesignLimits,
    /// The values of the estimate of the heat that cooling fins give to the air.
    pub thermal: ThermalSettings,
    /// The settings of the solid made for the STL file.
    pub mesh: MeshSettings,
    /// The settings of the flat drawings (DXF and SVG).
    pub drawing: DrawingSettings,
    /// The settings of the cutting plan of the profiles.
    pub stock: StockSettings,
    /// How the bill of materials is priced.
    pub cost: CostSettings,
    /// The settings of the check of the room between parts.
    pub clearance: ClearanceSettings,
    /// The settings of the table of placements and of the exploded view.
    pub placement: PlacementSettings,
    /// The settings of the title block of the drawings.
    pub title_block: TitleBlockSettings,
    /// The link to the EH3D server (`[server]`): the keys of the client of the suite.
    pub server: eh3d_serverclient::Settings,
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

    /// The post sizes of a screw of the table, or the error `PHOENIX-CAD-021` when the screw is not
    /// in the table or has no `[fixing.<screw>]` entry.
    pub fn fixing(&self, name: &str) -> Result<&Fixing, CadError> {
        self.fixings
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
        let mut fixings = BTreeMap::new();
        for key in c.keys() {
            let name = match key
                .strip_prefix("fixing.")
                .and_then(|rest| rest.strip_suffix(".post_diameter"))
            {
                Some(name) => name,
                None => continue,
            };
            if !screws.contains_key(name) {
                return Err(CadError::Config(format!(
                    "fixing.{name} has no screw.{name}"
                )));
            }
            let get = |field: &str| c.length(&format!("fixing.{name}.{field}"));
            let fixing = Fixing {
                post_diameter: get("post_diameter")?,
                insert_diameter: get("insert_diameter")?,
                insert_depth: get("insert_depth")?,
                pilot_diameter: get("pilot_diameter")?,
                pilot_depth: get("pilot_depth")?,
            };
            for (what, v) in [
                ("post_diameter", fixing.post_diameter),
                ("insert_diameter", fixing.insert_diameter),
                ("insert_depth", fixing.insert_depth),
                ("pilot_diameter", fixing.pilot_diameter),
                ("pilot_depth", fixing.pilot_depth),
            ] {
                if v.get() <= 0 {
                    return Err(CadError::Config(format!(
                        "fixing.{name}.{what} must be positive"
                    )));
                }
            }
            if fixing.insert_diameter >= fixing.post_diameter
                || fixing.pilot_diameter >= fixing.post_diameter
            {
                return Err(CadError::Config(format!(
                    "fixing.{name}: the bores must be narrower than the post"
                )));
            }
            fixings.insert(name.to_string(), fixing);
        }
        let posts = PostSettings {
            min_wall: c.length("posts.min_wall")?,
            base_extra: c.length("posts.base_extra")?,
            base_height: c.length("posts.base_height")?,
        };
        let lid = LidSettings {
            lip_thickness: c.length("lid.lip_thickness")?,
            lip_depth: c.length("lid.lip_depth")?,
            corner_inset: c.length("lid.corner_inset")?,
        };
        for (what, v) in [
            ("posts.min_wall", posts.min_wall),
            ("posts.base_extra", posts.base_extra),
            ("posts.base_height", posts.base_height),
            ("lid.lip_thickness", lid.lip_thickness),
            ("lid.lip_depth", lid.lip_depth),
            ("lid.corner_inset", lid.corner_inset),
        ] {
            if v.get() <= 0 {
                return Err(CadError::Config(format!("{what} must be positive")));
            }
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
        let mechanical = MechanicalLimits::new(
            c.number("mechanical.max_bytes")?,
            c.number("mechanical.max_steps")?,
            c.number("mechanical.max_name_bytes")?,
            c.number("mechanical.max_outline_points")?,
            c.number("mechanical.max_board_holes")?,
        )?;
        let design = DesignLimits::from_config(c)?;
        let thermal = ThermalSettings::from_config(c)?;
        let mesh = MeshSettings::from_config(c)?;
        let drawing = DrawingSettings::from_config(c)?;
        let stock = StockSettings::from_config(c)?;
        let cost = CostSettings::from_config(c, &materials)?;
        let clearance = ClearanceSettings::from_config(c)?;
        let placement = PlacementSettings::from_config(c)?;
        let title_block = TitleBlockSettings::from_config(c)?;
        let server = crate::server::from_config(c)?;
        let constants = build_constants(c, &screws, &printer, counterbore_extra, &expr)?;
        Ok(Settings {
            backups,
            undo_depth,
            unit,
            decimals,
            printer,
            counterbore_extra,
            screws,
            fixings,
            posts,
            lid,
            materials,
            expr,
            mechanical,
            design,
            thermal,
            mesh,
            drawing,
            stock,
            cost,
            clearance,
            placement,
            title_block,
            server,
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
        // every material of the shipped file has a price, to be replaced by the real one
        assert_eq!(s.cost.currency, "EUR");
        assert_eq!(s.cost.prices_milli_per_kg["PLA"], 20_000);
        assert_eq!(s.cost.prices_milli_per_kg.len(), s.materials.len());
        assert_eq!((s.cost.per_piece_milli, s.cost.markup_percent), (0, 0));
        assert_eq!(CostSettings::shipped(), s.cost);
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
    fn the_cost_is_read_in_thousandths_and_checked() {
        let s = Settings::with_user_config(
            "[cost]
currency = USD
per_piece = 0.5
markup_percent = 15
[material.PLA]
price_per_kg = 18.25
",
        )
        .unwrap();
        assert_eq!(s.cost.currency, "USD");
        assert_eq!((s.cost.per_piece_milli, s.cost.markup_percent), (500, 15));
        assert_eq!(s.cost.prices_milli_per_kg["PLA"], 18_250);
        assert_eq!(s.cost.prices_milli_per_kg["steel"], 2_000);
        for bad in [
            "[cost]
currency = eur
",
            "[cost]
currency = E
",
            "[cost]
currency = ABCDEFGHI
",
            "[cost]
markup_percent = 1001
",
            "[cost]
per_piece = 1.2345
",
            "[cost]
per_piece = -1
",
            "[material.PLA]
price_per_kg = -1
",
            "[material.PLA]
price_per_kg = cheap
",
            "[material.unobtainium]
price_per_kg = 1
",
        ] {
            assert!(Settings::with_user_config(bad).is_err(), "{bad}");
        }
        // 1000 percent is the most
        assert!(Settings::with_user_config(
            "[cost]
markup_percent = 1000
"
        )
        .is_ok());
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

    #[test]
    fn the_posts_the_lid_and_the_limits_of_the_domain_are_in_the_settings() {
        let s = Settings::defaults();
        let names: Vec<&str> = s.fixings.keys().map(String::as_str).collect();
        assert_eq!(names, vec!["M2", "M2.5", "M3", "M4", "M5", "M6", "M8"]);
        let m3 = s.fixing("M3").unwrap();
        assert_eq!(m3.post_diameter.get(), 8_000_000);
        assert_eq!(m3.insert_diameter.get(), 4_200_000);
        assert_eq!(m3.insert_depth.get(), 5_000_000);
        assert_eq!(m3.pilot_diameter.get(), 2_500_000);
        assert_eq!(m3.pilot_depth.get(), 8_000_000);
        assert_eq!(s.fixing("M2.5").unwrap().post_diameter.get(), 6_500_000);
        assert_eq!(s.fixing("M8").unwrap().insert_diameter.get(), 10_000_000);
        assert_eq!(s.posts.min_wall.get(), 1_200_000);
        assert_eq!(s.posts.base_extra.get(), 4_000_000);
        assert_eq!(s.posts.base_height.get(), 2_000_000);
        assert_eq!(s.lid.lip_thickness.get(), 1_200_000);
        assert_eq!(s.lid.lip_depth.get(), 3_000_000);
        assert_eq!(s.lid.corner_inset.get(), 6_000_000);
        assert_eq!(
            s.mechanical,
            MechanicalLimits::new(16_777_216, 10_000, 256, 10_000, 1_000).unwrap()
        );
        let err = s.fixing("M99").unwrap_err();
        assert_eq!(err, CadError::Screw("M99".into()));
        assert_eq!(err.code(), "PHOENIX-CAD-021");
    }

    #[test]
    fn the_table_of_posts_is_consistent_with_the_table_of_screws() {
        let s = Settings::defaults();
        assert_eq!(s.fixings.len(), s.screws.len());
        for (name, fixing) in &s.fixings {
            let screw = &s.screws[name];
            // the pilot hole is narrower than the thread, the insert is wider than the thread, and
            // the wall of the post is at least the least wall around either bore
            assert!(fixing.pilot_diameter < screw.nominal, "{name}");
            assert!(fixing.insert_diameter > screw.nominal, "{name}");
            let least = 2 * s.posts.min_wall.get();
            assert!(fixing.post_diameter.get() - fixing.insert_diameter.get() >= least);
            assert!(fixing.post_diameter.get() - fixing.pilot_diameter.get() >= least);
        }
    }

    #[test]
    fn a_user_file_can_add_a_post_entry_for_a_screw_of_its_own_and_change_the_values() {
        let own = "[screw.M3.5]\nnominal = 3.5 mm\nclearance = 3.9 mm\nhead_diameter = 6 mm\nhead_height = 3.5 mm\ncountersink_diameter = 7.3 mm\n\n[fixing.M3.5]\npost_diameter = 9 mm\ninsert_diameter = 5 mm\ninsert_depth = 5 mm\npilot_diameter = 3 mm\npilot_depth = 8 mm\n\n[lid]\ncorner_inset = 7 mm\n\n[mechanical]\nmax_steps = 50\n";
        let s = Settings::with_user_config(own).unwrap();
        assert_eq!(s.fixings.len(), 8);
        assert_eq!(s.fixing("M3.5").unwrap().post_diameter.get(), 9_000_000);
        assert_eq!(s.lid.corner_inset.get(), 7_000_000);
        assert_eq!(s.lid.lip_depth.get(), 3_000_000);
        assert_eq!(s.mechanical.max_steps, 50);
        assert_eq!(s.mechanical.max_bytes, 16_777_216);
    }

    #[test]
    fn values_of_the_posts_the_lid_and_the_domain_that_make_no_sense_are_refused() {
        for text in [
            "[fixing.M99]\npost_diameter = 5 mm\n",
            "[fixing.M3]\ninsert_diameter = 8 mm\n",
            "[fixing.M3]\npilot_diameter = 9 mm\n",
            "[fixing.M3]\ninsert_depth = 0 mm\n",
            "[fixing.M3]\npost_diameter = 4 mm\n",
            "[posts]\nmin_wall = 0 mm\n",
            "[posts]\nbase_extra = 0 mm\n",
            "[posts]\nbase_height = 0 mm\n",
            "[lid]\nlip_thickness = 0 mm\n",
            "[lid]\nlip_depth = 0 mm\n",
            "[lid]\ncorner_inset = 0 mm\n",
            "[lid]\ncorner_inset = 6\n",
            "[mechanical]\nmax_bytes = 0\n",
            "[mechanical]\nmax_bytes = 268435457\n",
            "[mechanical]\nmax_steps = 0\n",
            "[mechanical]\nmax_steps = 1000001\n",
            "[mechanical]\nmax_name_bytes = 4097\n",
            "[mechanical]\nmax_outline_points = 1000001\n",
            "[mechanical]\nmax_board_holes = 100001\n",
            "[mechanical]\nmax_board_holes = many\n",
        ] {
            let err = Settings::with_user_config(text).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-011", "{text:?}: {err}");
        }
        let err = Settings::with_user_config("[fixing.M99]\npost_diameter = 5 mm\n").unwrap_err();
        assert!(
            err.to_string().contains("fixing.M99 has no screw.M99"),
            "{err}"
        );
        for text in [
            "[pattern]\nmax_count = 1\n",
            "[pattern]\nmax_count = 10001\n",
            "[pattern]\nmax_count = lots\n",
            "[pattern]\nmax_parts = 1\n",
            "[pattern]\nmax_parts = 10001\n",
            "[pattern]\nmax_parts = many\n",
            "[order]\nmax_rows = 0\n",
            "[order]\nmax_rows = 1000001\n",
            "[order]\nmax_rows = all\n",
        ] {
            let err = Settings::with_user_config(text).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-011", "{text:?}: {err}");
        }
        assert_eq!(DesignLimits::shipped().pattern_max, 256);
        assert_eq!(DesignLimits::shipped().part_pattern_max, 64);
        assert_eq!(DesignLimits::shipped().order_max_rows, 2000);
        let few = Settings::with_user_config("[pattern]\nmax_parts = 3\n").unwrap();
        assert_eq!(few.design.part_pattern_max, 3);
        for text in [
            "[fins]\nmax_count = 0\n",
            "[fins]\nmax_count = 10001\n",
            "[fins]\nmax_height_ratio = 0\n",
            "[fins]\nmax_height_ratio = 1001\n",
            "[fins]\nmin_thickness = 0 mm\n",
            "[fins]\nmin_gap = -1 mm\n",
            "[fins]\nmin_gap = wide\n",
            "[thermal]\nconvection_w_m2k = 0\n",
            "[thermal]\nconvection_w_m2k = 1001\n",
            "[thermal]\nconductivity_w_mk = -5\n",
            "[thermal]\nconductivity_w_mk = 1.2345\n",
            "[thermal]\ndelta_t_k = hot\n",
            "[thermal]\ndelta_t_k = 1e2\n",
            "[thermal]\ndelta_t_k = \n",
        ] {
            let err = Settings::with_user_config(text).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-011", "{text:?}: {err}");
        }
        let s = Settings::defaults();
        assert_eq!(s.design.fin_max_count, 128);
        assert_eq!(s.design.fin_min_thickness.get(), 800_000);
        assert_eq!(s.design.fin_min_gap.get(), 3_000_000);
        assert_eq!(s.design.fin_max_ratio, 12);
        assert_eq!(
            (
                s.thermal.convection_mw_m2k,
                s.thermal.conductivity_mw_mk,
                s.thermal.delta_t_mk
            ),
            (8_000, 200_000, 40_000)
        );
        let own = Settings::with_user_config(
            "[thermal]\nconvection_w_m2k = 5,5\nconductivity_w_mk = 0.2\ndelta_t_k = 25.125\n[fins]\nmin_gap = 2.5 mm\nmax_count = 10000\n",
        )
        .unwrap();
        assert_eq!(own.thermal.convection_w_m2k(), 5.5);
        assert_eq!(own.thermal.conductivity_w_mk(), 0.2);
        assert_eq!(own.thermal.delta_t_k(), 25.125);
        assert_eq!(own.design.fin_min_gap.get(), 2_500_000);
        assert_eq!(own.design.fin_max_count, DesignLimits::FIN_CAP);
        let wide = Settings::with_user_config("[pattern]\nmax_count = 10000\n").unwrap();
        assert_eq!(wide.design.pattern_max, DesignLimits::PATTERN_CAP);
        // the caps themselves are allowed
        let top = "[mechanical]\nmax_bytes = 268435456\nmax_steps = 1000000\nmax_name_bytes = 4096\nmax_outline_points = 1000000\nmax_board_holes = 100000\n";
        assert!(Settings::with_user_config(top).is_ok());
    }

    #[test]
    fn the_settings_of_the_solid_and_of_the_drawings_are_the_shipped_ones() {
        let s = Settings::defaults();
        assert_eq!(
            s.mesh,
            MeshSettings {
                circle_segments: 64,
                max_triangles: 400_000,
                binary: true,
                volume_tolerance_ppm: 2000,
                step_max_kib: 65_536,
                step_max_entities: 1_000_000,
                step_max_faces: 100_000,
            }
        );
        assert_eq!(
            s.drawing,
            DrawingSettings {
                max_entities: 20_000,
                svg_width: Length::new(200_000_000).unwrap(),
                dimensions: true,
                axis_overshoot: Length::new(3_000_000).unwrap(),
                dxf_dimensions: true,
                hole_dimensions: true,
                max_dimensions: 40,
                hatch_spacing: Length::new(2_000_000).unwrap(),
                legend: true,
                legend_gap: Length::new(10_000_000).unwrap(),
                hidden_lines: true,
                frame: false,
                frame_margin: Length::new(5_000_000).unwrap(),
                sheet_angle: 1,
                sheet_gap: Length::new(20_000_000).unwrap(),
                sheet_legends: true,
                sheet_symbol: false,
                sheet_symbol_size: Length::new(10_000_000).unwrap(),
                paper: 0,
                paper_margin: Length::new(10_000_000).unwrap(),
                paper_portrait: false,
            }
        );
        let own = Settings::with_user_config(
            "[mesh]\ncircle_segments = 720\nmax_triangles = 12\nstl_binary = 0\nvolume_tolerance_ppm = 100000\n[drawing]\nmax_entities = 8\nsvg_width = 10 mm\ndimensions = 0\naxis_overshoot = 0 mm\ndxf_dimensions = 0\nhole_dimensions = 0\nmax_dimensions = 1\nhatch_spacing = 0 mm\n",
        )
        .unwrap();
        assert!(!own.drawing.dxf_dimensions && !own.drawing.hole_dimensions);
        assert_eq!(own.drawing.max_dimensions, 1);
        assert_eq!(own.drawing.hatch_spacing, Length::ZERO);
        assert_eq!(own.mesh.circle_segments, 720);
        assert!(!own.mesh.binary);
        assert_eq!(own.mesh.max_triangles, 12);
        assert_eq!(own.drawing.max_entities, 8);
        assert!(!own.drawing.dimensions);
        assert_eq!(own.drawing.axis_overshoot, Length::ZERO);
    }

    #[test]
    fn every_value_of_the_solid_and_of_the_drawings_is_checked_on_both_sides() {
        for text in [
            "[mesh]\ncircle_segments = 6\n",
            "[mesh]\ncircle_segments = 724\n",
            "[mesh]\ncircle_segments = 10\n",
            "[mesh]\nmax_triangles = 11\n",
            "[mesh]\nmax_triangles = 20000001\n",
            "[mesh]\nstl_binary = 2\n",
            "[mesh]\nvolume_tolerance_ppm = 0\n",
            "[mesh]\nvolume_tolerance_ppm = 100001\n",
            "[drawing]\nmax_entities = 7\n",
            "[drawing]\nmax_entities = 5000001\n",
            "[drawing]\nsvg_width = 9 mm\n",
            "[drawing]\nsvg_width = 10001 mm\n",
            "[drawing]\ndimensions = 2\n",
            "[drawing]\ndxf_dimensions = 2\n",
            "[drawing]\nhole_dimensions = yes\n",
            "[drawing]\nmax_dimensions = 0\n",
            "[drawing]\nhatch_spacing = 0.1 mm\n",
            "[drawing]\nhatch_spacing = 101 mm\n",
            "[drawing]\nhatch_spacing = -1 mm\n",
            "[drawing]\nmax_dimensions = 1001\n",
            "[drawing]\naxis_overshoot = -1 mm\n",
            "[drawing]\naxis_overshoot = 1001 mm\n",
            "[drawing]\nsvg_width = wide\n",
        ] {
            let error = Settings::with_user_config(text).unwrap_err();
            assert_eq!(error.code(), "PHOENIX-CAD-011", "{text:?}: {error}");
        }
        // the edges of the ranges are allowed
        for text in [
            "[mesh]\ncircle_segments = 8\n",
            "[mesh]\nmax_triangles = 20000000\n",
            "[mesh]\nvolume_tolerance_ppm = 1\n",
            "[drawing]\nmax_entities = 5000000\n",
            "[drawing]\nsvg_width = 10000 mm\n",
            "[drawing]\naxis_overshoot = 1000 mm\n",
        ] {
            assert!(Settings::with_user_config(text).is_ok(), "{text:?}");
        }
    }
}
