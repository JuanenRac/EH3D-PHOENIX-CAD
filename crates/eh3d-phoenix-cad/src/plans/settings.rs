// SPDX-License-Identifier: GPL-3.0-or-later
//! The settings of the drawings (section `[sheet]`): the sizes of the sheets, the style of the
//! dimensions, the title block, the layers and the limits. The file that ships with the crate is
//! the one place where the values live; a user file only needs the keys it changes.

use std::collections::BTreeMap;

use crate::config::Config;
use crate::units::parse_length;
use crate::{CadError, Length};

/// The shipped settings of the drawings, as text.
pub const DEFAULT_SHEET_CONFIG: &str = include_str!("../../phoenix-cad-drawing.default.cfg");

/// The text of the shipped settings, to write them to a file.
pub fn default_file() -> &'static str {
    DEFAULT_SHEET_CONFIG
}

/// How a line is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineType {
    /// A continuous line.
    Continuous,
    /// A line of dashes.
    Dashed,
    /// Long dashes and dots, for the centre lines.
    DashDot,
}

impl LineType {
    /// The word of the settings and of the file.
    pub fn name(self) -> &'static str {
        match self {
            LineType::Continuous => "continuous",
            LineType::Dashed => "dashed",
            LineType::DashDot => "dashdot",
        }
    }

    /// The type of a word.
    pub fn from_name(name: &str) -> Option<LineType> {
        [LineType::Continuous, LineType::Dashed, LineType::DashDot]
            .into_iter()
            .find(|t| t.name() == name)
    }
}

/// How the width of the lines of a layer is given: as a share of the line group or as a length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidthSpec {
    /// The thick line of the group.
    Thick,
    /// The thin line, half of it.
    Thin,
    /// The extra thick line, twice the thick one.
    Extra,
    /// A length in micrometres.
    Abs(i64),
}

impl WidthSpec {
    /// The width in micrometres for a line group (the thick line).
    pub fn width_um(self, group_um: i64) -> i64 {
        match self {
            WidthSpec::Thick => group_um,
            WidthSpec::Thin => group_um / 2,
            WidthSpec::Extra => group_um * 2,
            WidthSpec::Abs(v) => v,
        }
    }
}

/// What a layer is like unless the drawing says otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerStyle {
    /// Six hexadecimal digits.
    pub color: String,
    /// The width of the lines, in micrometres, for the line group of the settings.
    pub width_um: i64,
    /// How that width was given, so that another line group of the sheet can change it.
    pub width: WidthSpec,
    /// The type of the lines.
    pub line: LineType,
    /// Whether the layer is shown.
    pub visible: bool,
    /// Whether the layer is protected from changes.
    pub locked: bool,
}

/// The style of the dimensions, all in micrometres unless it says otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimStyle {
    /// The height of the text.
    pub text_height_um: i64,
    /// The length of an arrow.
    pub arrow_length_um: i64,
    /// The width of an arrow at its open end.
    pub arrow_width_um: i64,
    /// How far from the part the extension line starts.
    pub extension_gap_um: i64,
    /// How far the extension line goes past the dimension line.
    pub extension_overshoot_um: i64,
    /// The distance of the first dimension line from the view.
    pub offset_um: i64,
    /// The distance between stacked dimension lines.
    pub step_um: i64,
    /// The decimals of a value.
    pub decimals: u32,
    /// The length of the leader of a radius or a diameter.
    pub leader_length_um: i64,
    /// The angle of that leader, in degrees.
    pub leader_angle_deg: u32,
    /// The radius of the arc of an angle.
    pub arc_radius_um: i64,
    /// The room kept between two dimensions that share a line of dimensions, along it.
    pub clearance_um: i64,
    /// A dimension line that is dragged lands on a multiple of this.
    pub drag_snap_um: i64,
    /// How the tolerance of a dimension is written.
    pub tol_style: TolDisplay,
    /// The decimals of a deviation, in millimetres (0 to 6).
    pub tol_decimals: u32,
}

/// How the tolerance of a dimension is written next to its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TolDisplay {
    /// The class (`H7`, `H7/g6`).
    Class,
    /// The deviations (`+0.021/0`).
    Deviation,
    /// Both.
    Both,
}

/// The hatching of one material: the angle from the first axis of the view and the distance
/// between the lines, measured on the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HatchStyle {
    /// The angle in degrees, from 0 to 179.
    pub angle_deg: u32,
    /// The distance between two lines, in micrometres.
    pub spacing_um: i64,
}

/// The hatching of the cut faces of the section views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HatchSettings {
    /// The hatching of a material that has none of its own.
    pub default: HatchStyle,
    /// The hatching of the materials by name (lower case).
    pub materials: BTreeMap<String, HatchStyle>,
    /// The most lines of the hatching of one view.
    pub max_lines: usize,
}

/// The marks of the sections and the details, in micrometres.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionStyle {
    /// How far the line of a cut goes past the view it is drawn in.
    pub overshoot_um: i64,
    /// The length of the thick mark at each end of the line of a cut.
    pub end_length_um: i64,
    /// The length of the stem with the arrow that shows where the cut is looked at from.
    pub arrow_length_um: i64,
    /// The height of the letters and of the captions.
    pub text_height_um: i64,
    /// The room between a view and its caption.
    pub caption_gap_um: i64,
}

/// A block of the sheet that a person can choose for the title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleTemplate {
    /// The width of the block.
    pub width_um: i64,
    /// The height of a row.
    pub row_um: i64,
    /// The columns.
    pub columns: u32,
    /// The fields, in order.
    pub fields: Vec<String>,
}

/// How the revisions are numbered in the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionNumbering {
    /// A, B, C ... (after Z, AA, AB ...).
    Letters,
    /// 1, 2, 3 ...
    Numbers,
}

/// The table of revisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionSettings {
    /// Whether the table is drawn.
    pub table: bool,
    /// The height of a row.
    pub row_um: i64,
    /// The width of the table.
    pub width_um: i64,
    /// The most revisions a sheet keeps.
    pub max: usize,
    /// How they are numbered.
    pub numbering: RevisionNumbering,
}

/// The broken view: how much of the length is left between the two break marks and how the marks
/// are drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakStyle {
    /// The length of the part that is left between the two marks, in nanometres of the model.
    pub gap_nm: i64,
    /// How far the zigzag of a mark goes to each side, on the sheet, in micrometres.
    pub amplitude_um: i64,
    /// The zigzags of one mark.
    pub teeth: u32,
    /// How far a mark goes past the part, on the sheet, in micrometres.
    pub overshoot_um: i64,
}

/// The notes with a leader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteStyle {
    /// The most characters of a note.
    pub max_chars: usize,
    /// The height of the text, in micrometres.
    pub text_height_um: i64,
    /// Half the length of an arm of the cross of a centre mark, in micrometres.
    pub centre_mark_um: i64,
    /// A note, a balloon or a line of a cut that is dragged lands on a multiple of this.
    pub drag_snap_um: i64,
}

/// Which characters the value of an attribute may have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueCharset {
    /// Any character that is not a control one.
    Printable,
    /// Printable ASCII only.
    Ascii,
}

/// The limits of the free attributes of an object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrSettings {
    /// The most attributes of one part.
    pub max_per_node: usize,
    /// The most characters of a key.
    pub key_max: usize,
    /// The most characters of a value.
    pub value_max: usize,
    /// The characters a key is made of.
    pub key_charset: String,
    /// The characters a value may have.
    pub value_charset: ValueCharset,
}

/// The pattern of a type of line, in tenths of the width of the line: the dash, the gap and, for
/// the dash-dot, the dot and the second gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DashPattern {
    /// The dash.
    pub dash: u32,
    /// The gap after it.
    pub gap: u32,
    /// The dot (0 for a dashed line).
    pub dot: u32,
    /// The gap after the dot (0 for a dashed line).
    pub gap2: u32,
}

/// The settings of the drawings, with their types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetSettings {
    /// The name of the format of a new sheet.
    pub default_format: String,
    /// Whether a new sheet is upright.
    pub default_portrait: bool,
    /// The scale of a new sheet (`auto` or a ratio).
    pub default_scale: String,
    /// The standard scales, as `n:d`, in the order of the settings.
    pub scales: Vec<(u32, u32)>,
    /// Whether the projection is of the third angle.
    pub third_angle: bool,
    /// The distance of the frame from the edge of the sheet.
    pub margin_um: i64,
    /// The room between two views.
    pub view_gap_um: i64,
    /// Whether a new view shows the hidden edges.
    pub hidden_lines: bool,
    /// The material of a part that has none.
    pub default_material: String,
    /// The thick line.
    pub line_group_um: i64,
    /// The formats by name: the short and the long side, in micrometres.
    pub formats: BTreeMap<String, (i64, i64)>,
    /// The style of the dimensions.
    pub dim: DimStyle,
    /// The radius of a balloon.
    pub balloon_radius_um: i64,
    /// The height of the text of a balloon.
    pub balloon_text_um: i64,
    /// Whether the sheet has a title block.
    pub title_block: bool,
    /// The width of the title block.
    pub title_width_um: i64,
    /// The height of a row of the title block.
    pub title_row_um: i64,
    /// The columns of the title block.
    pub title_columns: u32,
    /// The fields of the title block, in order.
    pub title_fields: Vec<String>,
    /// The values the fields have unless the drawing says otherwise.
    pub title_values: BTreeMap<String, String>,
    /// Whether the table of the bill is drawn.
    pub bom_table: bool,
    /// The height of a row of that table.
    pub bom_row_um: i64,
    /// The width of that table.
    pub bom_width_um: i64,
    /// The layers by name, in the order of the settings.
    pub layers: Vec<(String, LayerStyle)>,
    /// The standard widths of the thick line, in micrometres (the groups of lines).
    pub line_groups: Vec<i64>,
    /// The pattern of the dashed lines.
    pub dashed: DashPattern,
    /// The pattern of the dash-dot lines.
    pub dash_dot: DashPattern,
    /// The hatching of the cut faces.
    pub hatch: HatchSettings,
    /// The marks of sections and details.
    pub section: SectionStyle,
    /// The name of the title block that a new sheet has.
    pub title_template: String,
    /// The other blocks, by name.
    pub title_templates: BTreeMap<String, TitleTemplate>,
    /// The table of revisions.
    pub revisions: RevisionSettings,
    /// The broken view.
    pub brk: BreakStyle,
    /// The notes.
    pub note: NoteStyle,
    /// The free attributes.
    pub attr: AttrSettings,
    /// The keys of the attributes shown as columns of the bill, in order.
    pub bom_attr_columns: Vec<String>,
    /// The fields of the title block that an attribute of the main part fills: (field, key).
    pub title_from_attr: Vec<(String, String)>,
    /// The most notes of a sheet.
    pub max_notes: usize,
    /// The largest drawing file that is read.
    pub max_file_bytes: usize,
    /// The most views of a sheet.
    pub max_views: usize,
    /// The most dimensions.
    pub max_dims: usize,
    /// The most balloons.
    pub max_balloons: usize,
    /// The most lines of the bill.
    pub max_parts: usize,
    /// The lines of a newer program that a record keeps.
    pub max_extra_lines: usize,
    /// The most bytes of one of those lines.
    pub max_extra_bytes: usize,
    /// The most characters of a text typed by the person.
    pub max_text: usize,
    /// The most cells of the grid of a solid.
    pub max_cells: usize,
    /// The most edges of a view.
    pub max_edges: usize,
    /// The most dimensions placed by themselves in one view.
    pub max_auto_dims: usize,
    /// The most tests of an edge against a prism in an isometric view.
    pub max_iso_tests: u64,
}

fn bad(why: String) -> CadError {
    CadError::Config(why)
}

fn text<'a>(c: &'a Config, key: &str) -> Result<&'a str, CadError> {
    c.get(key).ok_or_else(|| bad(format!("{key} is missing")))
}

fn um(c: &Config, key: &str, low: i64, high: i64) -> Result<i64, CadError> {
    let nm = c.length(key)?.get();
    if nm % 1000 != 0 {
        return Err(bad(format!("{key} is not a whole number of micrometres")));
    }
    let v = nm / 1000;
    if (low..=high).contains(&v) {
        Ok(v)
    } else {
        Err(bad(format!("{key} must be from {low} to {high} um")))
    }
}

fn count(c: &Config, key: &str, low: u32, high: u32) -> Result<u32, CadError> {
    let v = c.number(key)?;
    if (low..=high).contains(&v) {
        Ok(v)
    } else {
        Err(bad(format!("{key} must be from {low} to {high}")))
    }
}

fn flag(c: &Config, key: &str) -> Result<bool, CadError> {
    match text(c, key)? {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(bad(format!("{key} is 0 or 1"))),
    }
}

/// A scale written `n:d`, both from 1 to 1000.
pub fn parse_scale(s: &str) -> Option<(u32, u32)> {
    let (n, d) = s.split_once(':')?;
    let n: u32 = n.trim().parse().ok()?;
    let d: u32 = d.trim().parse().ok()?;
    ((1..=1000).contains(&n) && (1..=1000).contains(&d)).then_some((n, d))
}

/// Whether a name of a format, a layer or a field is a short word of letters, digits and `_`.
pub fn is_word(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 24
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn is_color(s: &str) -> bool {
    s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

impl SheetSettings {
    /// The defaults that ship with the crate.
    pub fn defaults() -> SheetSettings {
        let config = Config::parse(DEFAULT_SHEET_CONFIG).expect("the shipped settings are valid");
        SheetSettings::from_config(&config).expect("the shipped settings are complete")
    }

    /// The defaults with the keys of a user file put over them. Keys of other parts are accepted
    /// and ignored here.
    pub fn with_user_config(text: &str) -> Result<SheetSettings, CadError> {
        let mut config = Config::parse(DEFAULT_SHEET_CONFIG)?;
        config.overlay(&Config::parse(text)?);
        SheetSettings::from_config(&config)
    }

    /// Reads the typed settings from a complete configuration.
    pub fn from_config(c: &Config) -> Result<SheetSettings, CadError> {
        let width_of = |c: &Config, key: &str| -> Result<WidthSpec, CadError> {
            match text(c, key)? {
                "thick" => Ok(WidthSpec::Thick),
                "thin" => Ok(WidthSpec::Thin),
                "extra" => Ok(WidthSpec::Extra),
                _ => Ok(WidthSpec::Abs(um(c, key, 10, 5000)?)),
            }
        };
        let mut line_groups = Vec::new();
        for w in text(c, "sheet.line_groups")?.split_whitespace() {
            let v: i64 = w.parse().map_err(|_| {
                bad(format!(
                    "sheet.line_groups: {w} is not a whole number of um"
                ))
            })?;
            if !(50..=2000).contains(&v) || line_groups.contains(&v) {
                return Err(bad(format!(
                    "sheet.line_groups: {w} is twice or not from 50 to 2000 um"
                )));
            }
            line_groups.push(v);
        }
        if line_groups.is_empty() || line_groups.len() > 16 {
            return Err(bad("sheet.line_groups needs from 1 to 16 widths".into()));
        }
        let line_group_um = um(c, "sheet.line_group", 50, 2000)?;
        if !line_groups.contains(&line_group_um) {
            return Err(bad(format!(
                "sheet.line_group is {line_group_um} um, which is not one of sheet.line_groups"
            )));
        }
        let pattern = |key: &str, dot: bool| -> Result<DashPattern, CadError> {
            let v: Vec<u32> = text(c, key)?
                .split_whitespace()
                .map(|w| w.parse::<u32>())
                .collect::<Result<_, _>>()
                .map_err(|_| bad(format!("{key}: whole numbers of tenths of the line width")))?;
            let want = if dot { 4 } else { 2 };
            if v.len() != want || v.iter().any(|x| *x == 0 || *x > 1000) {
                return Err(bad(format!(
                    "{key}: {want} numbers from 1 to 1000 (tenths of the line width)"
                )));
            }
            Ok(DashPattern {
                dash: v[0],
                gap: v[1],
                dot: if dot { v[2] } else { 0 },
                gap2: if dot { v[3] } else { 0 },
            })
        };
        let dashed = pattern("sheet.linetype.dashed", false)?;
        let dash_dot = pattern("sheet.linetype.dashdot", true)?;
        let mut formats = BTreeMap::new();
        for key in c.keys().filter(|k| k.starts_with("sheet.format.")) {
            let name = &key["sheet.format.".len()..];
            if !is_word(name) {
                return Err(bad(format!("{key}: the name of a format is not valid")));
            }
            let (a, b) = text(c, key)?
                .split_once('x')
                .ok_or_else(|| bad(format!("{key} is `width x height`, each with its unit")))?;
            let a = parse_length(a)
                .map_err(|e| bad(format!("{key}: {e}")))?
                .get();
            let b = parse_length(b)
                .map_err(|e| bad(format!("{key}: {e}")))?
                .get();
            let (short, long) = (a.min(b) / 1000, a.max(b) / 1000);
            if !(20_000..=5_000_000).contains(&short) || !(20_000..=5_000_000).contains(&long) {
                return Err(bad(format!("{key}: the sides must be from 20 mm to 5 m")));
            }
            formats.insert(name.to_string(), (short, long));
        }
        let default_format = text(c, "sheet.default_format")?.to_string();
        if !formats.contains_key(&default_format) {
            return Err(bad(format!(
                "sheet.default_format names no format ({default_format})"
            )));
        }
        let default_portrait = match text(c, "sheet.default_orientation")? {
            "landscape" => false,
            "portrait" => true,
            _ => {
                return Err(bad(
                    "sheet.default_orientation is landscape or portrait".into()
                ))
            }
        };
        let default_scale = text(c, "sheet.default_scale")?.to_string();
        if default_scale != "auto" && parse_scale(&default_scale).is_none() {
            return Err(bad("sheet.default_scale is auto or n:d".into()));
        }
        let mut scales = Vec::new();
        for w in text(c, "sheet.scales")?.split_whitespace() {
            scales
                .push(parse_scale(w).ok_or_else(|| bad(format!("sheet.scales: {w} is not n:d")))?);
        }
        if scales.is_empty() || scales.len() > 64 {
            return Err(bad("sheet.scales needs from 1 to 64 scales".into()));
        }
        let third_angle = match text(c, "sheet.projection")? {
            "third" => true,
            "first" => false,
            _ => return Err(bad("sheet.projection is first or third".into())),
        };
        let dim = DimStyle {
            text_height_um: um(c, "sheet.dim.text_height", 500, 50_000)?,
            arrow_length_um: um(c, "sheet.dim.arrow_length", 500, 50_000)?,
            arrow_width_um: um(c, "sheet.dim.arrow_width", 100, 50_000)?,
            extension_gap_um: um(c, "sheet.dim.extension_gap", 0, 20_000)?,
            extension_overshoot_um: um(c, "sheet.dim.extension_overshoot", 0, 20_000)?,
            offset_um: um(c, "sheet.dim.offset", 1000, 200_000)?,
            step_um: um(c, "sheet.dim.step", 1000, 200_000)?,
            decimals: count(c, "sheet.dim.decimals", 0, 6)?,
            leader_length_um: um(c, "sheet.dim.leader_length", 1000, 200_000)?,
            leader_angle_deg: count(c, "sheet.dim.leader_angle", 0, 359)?,
            arc_radius_um: um(c, "sheet.dim.arc_radius", 1000, 200_000)?,
            clearance_um: um(c, "sheet.dim.clearance", 0, 100_000)?,
            drag_snap_um: um(c, "sheet.dim.drag_snap", 10, 100_000)?,
            tol_style: match text(c, "sheet.dim.tol_style")? {
                "class" => TolDisplay::Class,
                "deviation" => TolDisplay::Deviation,
                "both" => TolDisplay::Both,
                _ => return Err(bad("sheet.dim.tol_style is class, deviation or both".into())),
            },
            tol_decimals: count(c, "sheet.dim.tol_decimals", 0, 6)?,
        };
        let title_fields: Vec<String> = text(c, "sheet.titleblock.fields")?
            .split_whitespace()
            .map(str::to_string)
            .collect();
        if title_fields.len() > 24 || title_fields.iter().any(|f| !is_word(f)) {
            return Err(bad(
                "sheet.titleblock.fields: up to 24 words of letters and digits".into(),
            ));
        }
        let mut title_values = BTreeMap::new();
        for key in c
            .keys()
            .filter(|k| k.starts_with("sheet.titleblock.value."))
        {
            let name = &key["sheet.titleblock.value.".len()..];
            if !is_word(name) {
                return Err(bad(format!("{key}: the name of a field is not valid")));
            }
            title_values.insert(name.to_string(), text(c, key)?.to_string());
        }
        let mut layers = Vec::new();
        for name in text(c, "sheet.layer.names")?.split_whitespace() {
            if !is_word(name) || name.bytes().any(|b| b.is_ascii_lowercase()) {
                return Err(bad(format!(
                    "sheet.layer.names: {name} is not a name in capitals"
                )));
            }
            if layers.iter().any(|(n, _)| n == name) {
                return Err(bad(format!("sheet.layer.names: {name} twice")));
            }
            let k = |part: &str| format!("sheet.layer.{name}.{part}");
            let color = text(c, &k("color"))?.to_ascii_lowercase();
            if !is_color(&color) {
                return Err(bad(format!("{}: six hexadecimal digits", k("color"))));
            }
            let line = LineType::from_name(text(c, &k("line"))?)
                .ok_or_else(|| bad(format!("{}: continuous, dashed or dashdot", k("line"))))?;
            layers.push((
                name.to_string(),
                LayerStyle {
                    color,
                    width_um: width_of(c, &k("width"))?.width_um(line_group_um),
                    width: width_of(c, &k("width"))?,
                    line,
                    visible: flag(c, &k("visible"))?,
                    locked: flag(c, &k("locked"))?,
                },
            ));
        }
        if layers.is_empty() || layers.len() > 32 {
            return Err(bad("sheet.layer.names needs from 1 to 32 layers".into()));
        }
        for needed in [
            "OUTLINE",
            "HIDDEN",
            "CENTER",
            "DIMENSION",
            "TEXT",
            "BALLOON",
            "FRAME",
        ] {
            if !layers.iter().any(|(n, _)| n == needed) {
                return Err(bad(format!("sheet.layer.names lacks {needed}")));
            }
        }
        let columns = count(c, "sheet.titleblock.columns", 1, 4)?;
        let width_of_block = um(c, "sheet.titleblock.width", 40_000, 1_000_000)?;
        let hatch_style = |angle: &str, spacing: &str| -> Result<HatchStyle, CadError> {
            Ok(HatchStyle {
                angle_deg: count(c, angle, 0, 179)?,
                spacing_um: um(c, spacing, 200, 50_000)?,
            })
        };
        let mut hatch_materials = BTreeMap::new();
        for name in text(c, "sheet.hatch.materials")?.split_whitespace() {
            if !is_word(name) || name.bytes().any(|b| b.is_ascii_uppercase()) {
                return Err(bad(format!(
                    "sheet.hatch.materials: {name} is not a name in lower case"
                )));
            }
            let style = hatch_style(
                &format!("sheet.hatch.material.{name}.angle"),
                &format!("sheet.hatch.material.{name}.spacing"),
            )?;
            if hatch_materials.insert(name.to_string(), style).is_some() {
                return Err(bad(format!("sheet.hatch.materials: {name} twice")));
            }
        }
        if hatch_materials.len() > 64 {
            return Err(bad("sheet.hatch.materials: up to 64 materials".into()));
        }
        let hatch = HatchSettings {
            default: hatch_style("sheet.hatch.angle", "sheet.hatch.spacing")?,
            materials: hatch_materials,
            max_lines: count(c, "sheet.hatch.max_lines", 10, 1_000_000)? as usize,
        };
        let section = SectionStyle {
            overshoot_um: um(c, "sheet.section.overshoot", 0, 200_000)?,
            end_length_um: um(c, "sheet.section.end_length", 500, 200_000)?,
            arrow_length_um: um(c, "sheet.section.arrow_length", 500, 200_000)?,
            text_height_um: um(c, "sheet.section.text_height", 500, 100_000)?,
            caption_gap_um: um(c, "sheet.section.caption_gap", 0, 200_000)?,
        };
        let template_of = |prefix: &str| -> Result<TitleTemplate, CadError> {
            let fields: Vec<String> = text(c, &format!("{prefix}.fields"))?
                .split_whitespace()
                .map(str::to_string)
                .collect();
            if fields.len() > 24 || fields.iter().any(|f| !is_word(f)) {
                return Err(bad(format!(
                    "{prefix}.fields: up to 24 words of letters and digits"
                )));
            }
            Ok(TitleTemplate {
                width_um: um(c, &format!("{prefix}.width"), 40_000, 1_000_000)?,
                row_um: um(c, &format!("{prefix}.row"), 3000, 50_000)?,
                columns: count(c, &format!("{prefix}.columns"), 1, 4)?,
                fields,
            })
        };
        let mut title_templates = BTreeMap::new();
        for name in text(c, "sheet.titleblock.templates")?.split_whitespace() {
            if !is_word(name) || name == "default" {
                return Err(bad(format!(
                    "sheet.titleblock.templates: {name} is not a name (default is the block above)"
                )));
            }
            let tpl = template_of(&format!("sheet.titleblock.template.{name}"))?;
            if title_templates.insert(name.to_string(), tpl).is_some() {
                return Err(bad(format!("sheet.titleblock.templates: {name} twice")));
            }
        }
        let title_template = text(c, "sheet.titleblock.template")?.to_string();
        if title_template != "default" && !title_templates.contains_key(&title_template) {
            return Err(bad(format!(
                "sheet.titleblock.template names no block ({title_template})"
            )));
        }
        let revisions = RevisionSettings {
            table: flag(c, "sheet.revisions.table")?,
            row_um: um(c, "sheet.revisions.row", 3000, 50_000)?,
            width_um: um(c, "sheet.revisions.width", 40_000, 1_000_000)?,
            max: count(c, "sheet.revisions.max", 1, 256)? as usize,
            numbering: match text(c, "sheet.revisions.numbering")? {
                "letters" => RevisionNumbering::Letters,
                "numbers" => RevisionNumbering::Numbers,
                _ => return Err(bad("sheet.revisions.numbering is letters or numbers".into())),
            },
        };
        let brk = BreakStyle {
            gap_nm: um(c, "sheet.break.gap", 1000, 1_000_000_000)? * 1000,
            amplitude_um: um(c, "sheet.break.amplitude", 100, 50_000)?,
            teeth: count(c, "sheet.break.teeth", 1, 16)?,
            overshoot_um: um(c, "sheet.break.overshoot", 0, 100_000)?,
        };
        let note = NoteStyle {
            max_chars: count(c, "sheet.note.max_chars", 8, 1000)? as usize,
            text_height_um: um(c, "sheet.note.text_height", 500, 100_000)?,
            centre_mark_um: um(c, "sheet.note.centre_mark", 500, 100_000)?,
            drag_snap_um: um(c, "sheet.note.drag_snap", 10, 100_000)?,
        };
        let attr = AttrSettings {
            max_per_node: count(c, "sheet.attr.max_per_node", 1, 256)? as usize,
            key_max: count(c, "sheet.attr.key_max", 1, 64)? as usize,
            value_max: count(c, "sheet.attr.value_max", 1, 1000)? as usize,
            key_charset: {
                let cs = text(c, "sheet.attr.key_charset")?;
                if cs.is_empty()
                    || cs.len() > 128
                    || !cs.bytes().all(|b| b.is_ascii_graphic())
                    || cs
                        .bytes()
                        .enumerate()
                        .any(|(i, b)| cs.as_bytes()[..i].contains(&b))
                {
                    return Err(bad(
                        "sheet.attr.key_charset: from 1 to 128 printable ASCII characters, none twice, no space".into(),
                    ));
                }
                cs.to_string()
            },
            value_charset: match text(c, "sheet.attr.value_charset")? {
                "printable" => ValueCharset::Printable,
                "ascii" => ValueCharset::Ascii,
                _ => return Err(bad("sheet.attr.value_charset is printable or ascii".into())),
            },
        };
        let bom_attr_columns: Vec<String> = text(c, "sheet.bom.attr_columns")?
            .split_whitespace()
            .map(str::to_string)
            .collect();
        if bom_attr_columns.len() > 4
            || bom_attr_columns.iter().enumerate().any(|(i, k)| {
                k.is_empty()
                    || k.chars().count() > attr.key_max
                    || !k.chars().all(|ch| attr.key_charset.contains(ch))
                    || bom_attr_columns[..i].contains(k)
            })
        {
            return Err(bad(
                "sheet.bom.attr_columns: up to 4 different keys made of the characters of sheet.attr.key_charset".into(),
            ));
        }
        let mut title_from_attr = Vec::new();
        for pair in text(c, "sheet.titleblock.from_attr")?.split_whitespace() {
            let (field, key) = pair.split_once(':').ok_or_else(|| {
                bad(format!(
                    "sheet.titleblock.from_attr: {pair} is not field:key"
                ))
            })?;
            if !is_word(field)
                || key.is_empty()
                || key.chars().count() > attr.key_max
                || !key.chars().all(|ch| attr.key_charset.contains(ch))
                || title_from_attr
                    .iter()
                    .any(|(f, _): &(String, String)| f == field)
            {
                return Err(bad(format!(
                    "sheet.titleblock.from_attr: {pair} is not a field of the title block and a key, each field once"
                )));
            }
            title_from_attr.push((field.to_string(), key.to_string()));
        }
        Ok(SheetSettings {
            default_format,
            default_portrait,
            default_scale,
            scales,
            third_angle,
            margin_um: um(c, "sheet.margin", 0, 100_000)?,
            view_gap_um: um(c, "sheet.view_gap", 0, 500_000)?,
            hidden_lines: flag(c, "sheet.hidden_lines")?,
            default_material: text(c, "sheet.default_material")?.to_string(),
            line_group_um,
            formats,
            dim,
            balloon_radius_um: um(c, "sheet.balloon.radius", 1000, 50_000)?,
            balloon_text_um: um(c, "sheet.balloon.text_height", 500, 50_000)?,
            title_block: flag(c, "sheet.titleblock.enabled")?,
            title_width_um: width_of_block,
            title_row_um: um(c, "sheet.titleblock.row", 3000, 50_000)?,
            title_columns: columns,
            title_fields,
            title_values,
            bom_table: flag(c, "sheet.bom.table")?,
            bom_row_um: um(c, "sheet.bom.row", 3000, 50_000)?,
            bom_width_um: um(c, "sheet.bom.width", 40_000, 1_000_000)?,
            layers,
            line_groups,
            dashed,
            dash_dot,
            hatch,
            section,
            title_template,
            title_templates,
            revisions,
            brk,
            note,
            attr,
            bom_attr_columns,
            title_from_attr,
            max_notes: count(c, "sheet.max_notes", 1, 100_000)? as usize,
            max_file_bytes: count(c, "sheet.max_file_bytes", 1024, 1 << 30)? as usize,
            max_views: count(c, "sheet.max_views", 1, 256)? as usize,
            max_dims: count(c, "sheet.max_dims", 1, 100_000)? as usize,
            max_balloons: count(c, "sheet.max_balloons", 1, 100_000)? as usize,
            max_parts: count(c, "sheet.max_parts", 1, 100_000)? as usize,
            max_extra_lines: count(c, "sheet.max_extra_lines", 0, 1000)? as usize,
            max_extra_bytes: count(c, "sheet.max_extra_bytes", 8, 65_536)? as usize,
            max_text: count(c, "sheet.max_text", 8, 1000)? as usize,
            max_cells: count(c, "sheet.max_cells", 8, 100_000_000)? as usize,
            max_edges: count(c, "sheet.max_edges", 8, 10_000_000)? as usize,
            max_auto_dims: count(c, "sheet.max_auto_dims", 1, 100_000)? as usize,
            max_iso_tests: u64::from(count(c, "sheet.max_iso_tests", 1000, u32::MAX)?),
        })
    }

    /// The style of a layer by name.
    pub fn layer(&self, name: &str) -> Option<&LayerStyle> {
        self.layers.iter().find(|(n, _)| n == name).map(|(_, s)| s)
    }

    /// The size of a format on the sheet as (width, height) in micrometres for an orientation.
    pub fn sheet_size_um(&self, format: &str, portrait: bool) -> Option<(i64, i64)> {
        let (short, long) = *self.formats.get(format)?;
        Some(if portrait {
            (short, long)
        } else {
            (long, short)
        })
    }

    /// The title block of a name (`default` is the one of the keys `sheet.titleblock.*`), or `None`
    /// when no block has that name.
    pub fn title_block_of(&self, name: Option<&str>) -> Option<TitleTemplate> {
        let name = name.unwrap_or(&self.title_template);
        if name == "default" {
            Some(TitleTemplate {
                width_um: self.title_width_um,
                row_um: self.title_row_um,
                columns: self.title_columns,
                fields: self.title_fields.clone(),
            })
        } else {
            self.title_templates.get(name).cloned()
        }
    }

    /// A length of the settings in the range of lengths (for the callers that need a [`Length`]).
    pub fn length_of_um(um: i64) -> Result<Length, CadError> {
        Length::new(um.checked_mul(1000).ok_or(CadError::Overflow("length"))?)
    }
}
