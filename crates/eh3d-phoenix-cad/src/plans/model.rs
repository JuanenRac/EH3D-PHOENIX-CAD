// SPDX-License-Identifier: GPL-3.0-or-later
//! The model of a drawing sheet and its file: the domain `drawing` of the project file.
//!
//! The text of the domain is in one canonical form, with a version, ids that do not change and
//! the lines it does not know (those that start with `x-`) kept where they were:
//!
//! ```text
//! drawing 1                          (`drawing 2` when the sheet has a break, a note or an attribute)
//! format <name>
//! orientation <landscape|portrait>
//! scale <auto|n:d>
//! projection <third|first>
//! nextitem <n>
//! nextrevision <n>                   (only when it is not 1)
//! titleblock <name>                  (only when the sheet has chosen a block)
//! linegroup <um>                     (only when the sheet has chosen a group of lines)
//! field <key> <text>
//! layer <NAME> <visible|locked|color|width> <value>
//! x-<anything>                       (the lines of the head that a newer program added)
//! item <node id> <number>
//! part <node id>
//! material <text>
//! description <text>
//! attr <key> <value>                 (a free attribute of the part; sorted by key)
//! revision <number>
//! date <text>
//! description <text>
//! author <text>
//! view <id> <front|top|right|iso|sketch> <name>
//! source <tip | node <id> | sketch <name>>
//! at <x um> <y um>
//! scale <n:d>
//! hidden <0|1>
//! section <parent view id> <label> <position nm>        (a section view; the kind says the axis)
//! detail <parent view id> <label> circle <u nm> <v nm> <radius nm>
//! detail <parent view id> <label> rect <u nm> <v nm> <width nm> <height nm>
//! hatch <material>
//! break <u|v> <from nm> <to nm>                           (a broken view: the range is left out)
//! dim <id> <linear|radius|diameter|angle|cumulative> <name>
//! ref face <node id> <x|y|z> <min|max>
//! ref element <number> <sketch name>
//! view <view id>
//! offset <um>
//! step <um>
//! tol class <H7|g6|H7/g6>
//! tol dev <upper nm> <lower nm>
//! layer <NAME>
//! balloon <id> <name>
//! part <node id>
//! at <x um> <y um>
//! to <x um> <y um>
//! note <id> <text>
//! anchor view <view id> <u nm> <v nm>
//! anchor face <view id> <node id> <x|y|z> <min|max>
//! at <x um> <y um>
//! centre 1                           (only when the note marks a centre)
//! layer <NAME>
//! ```
//!
//! A record is followed by its own lines; an `x-` line after a record belongs to it. Everything
//! is read under the limits of the settings and a text that is not in the one canonical form is
//! refused, so what is read is what is written.

use std::collections::{BTreeMap, BTreeSet};

use super::settings::{is_word, parse_scale, RevisionNumbering, SheetSettings};
pub use super::tol::Tol;
use crate::enclosure::Id;
use crate::project::Document;
use crate::CadError;

/// The domain of the project file that holds the drawing.
pub const DRAWING_DOMAIN: &str = "drawing";

/// The first line of the domain; the number is the version of its text. Version 2 is written only
/// when the sheet has a broken view, a note or an attribute, so a sheet without them is still read
/// by a program that knows version 1 only.
pub const HEADER: &str = "drawing 1";

/// The first line of a sheet that has a broken view, a note or an attribute.
pub const HEADER_2: &str = "drawing 2";

/// What a view looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    /// From the front (the side of the lowest `y`): `x` to the right, `z` up.
    Front,
    /// From above: `x` to the right, `y` up.
    Top,
    /// From the right (the side of the highest `x`): `y` to the right, `z` up.
    Right,
    /// Isometric, from the front, the right and above.
    Iso,
    /// A sketch of the project drawn in its own plane.
    Sketch,
}

impl ViewKind {
    /// Every kind.
    pub const ALL: [ViewKind; 5] = [
        ViewKind::Front,
        ViewKind::Top,
        ViewKind::Right,
        ViewKind::Iso,
        ViewKind::Sketch,
    ];

    /// The word of the file.
    pub fn name(self) -> &'static str {
        match self {
            ViewKind::Front => "front",
            ViewKind::Top => "top",
            ViewKind::Right => "right",
            ViewKind::Iso => "iso",
            ViewKind::Sketch => "sketch",
        }
    }

    /// The kind of a word.
    pub fn from_name(name: &str) -> Option<ViewKind> {
        ViewKind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The axis that a view of this kind looks along, which is the axis across a section made for
    /// it: the front looks along `y`, the top along `z`, the right along `x`.
    pub fn depth_axis(self) -> Option<Axis> {
        match self {
            ViewKind::Front => Some(Axis::Y),
            ViewKind::Top => Some(Axis::Z),
            ViewKind::Right => Some(Axis::X),
            _ => None,
        }
    }
}

/// Where the solid of a view comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The result of the history.
    Tip,
    /// The solid of a node of the history.
    Node(Id),
    /// A sketch of the project, by name.
    Sketch(String),
}

/// Where a view is on the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Aligned with the others by the rules of the projection.
    Auto,
    /// With the lower left corner of its box at this point of the sheet, in micrometres.
    At(i64, i64),
}

/// A view on the sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// Its identity.
    pub id: Id,
    /// Its name.
    pub name: String,
    /// What it looks at.
    pub kind: ViewKind,
    /// Where its solid comes from.
    pub source: Source,
    /// Where it is on the sheet.
    pub place: Place,
    /// Its own scale, when it has one (otherwise the one of the sheet).
    pub scale: Option<(u32, u32)>,
    /// Whether it shows the hidden edges.
    pub hidden: bool,
    /// When the view is the section of its source: where the cut is.
    pub section: Option<SectionCut>,
    /// When the view is the enlargement of a part of another view.
    pub detail: Option<Detail>,
    /// The material that the hatching of a section follows, when the person names one.
    pub hatch: Option<String>,
    /// The range of the length that a broken view leaves out.
    pub brk: Option<BreakCut>,
    /// The lines of a newer program that the record keeps.
    pub extra: Vec<String>,
}

/// The range that a broken view leaves out, in the coordinates of its picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakCut {
    /// Whether the range is along the axis across the picture (`u`) or up it (`v`).
    pub along_u: bool,
    /// Where the range starts, in nanometres.
    pub from_nm: i64,
    /// Where it ends, in nanometres.
    pub to_nm: i64,
}

/// What a note points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteAnchor {
    /// A point of the picture of a view, in nanometres; it moves with the view.
    View {
        /// The view.
        view: Id,
        /// The point across the picture.
        u_nm: i64,
        /// The point up the picture.
        v_nm: i64,
    },
    /// A face of the box of a node of the history, as a view shows it; when the node is gone the
    /// note is reported and not drawn.
    Face {
        /// The view that shows the face.
        view: Id,
        /// The node.
        node: Id,
        /// The axis across the face.
        axis: Axis,
        /// The low or the high face.
        side: Side,
    },
}

/// A note: a free text on the sheet with a leader to a view or to a face, and when the person
/// wants it the mark of a centre where the leader ends. There is no roughness or welding symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Its identity.
    pub id: Id,
    /// The text, one line.
    pub text: String,
    /// What it points at.
    pub anchor: NoteAnchor,
    /// Where the text starts, on the sheet, in micrometres.
    pub at: (i64, i64),
    /// Whether the end of the leader has the cross of a centre mark.
    pub centre: bool,
    /// The layer it is drawn on.
    pub layer: String,
    /// The lines of a newer program that the record keeps.
    pub extra: Vec<String>,
}

impl View {
    /// A view of a solid with what the sheet usually has: placed by the rules of the projection,
    /// at the scale of the sheet, no section and no detail.
    pub fn plain(id: Id, name: &str, kind: ViewKind, source: Source, hidden: bool) -> View {
        View {
            id,
            name: name.to_string(),
            kind,
            source,
            place: Place::Auto,
            scale: None,
            hidden,
            section: None,
            detail: None,
            hatch: None,
            brk: None,
            extra: Vec::new(),
        }
    }

    /// Whether the view is a section or a detail, which the layout does not take for one of the
    /// views of the projection.
    pub fn is_derived(&self) -> bool {
        self.section.is_some() || self.detail.is_some()
    }
}

/// The cut of a section view: the plane across the axis that the view looks along, at a position,
/// and the view in which the line of the cut is drawn. The material that stays is the one the
/// view looks at from the side that the projection gives it: for the front view the part with
/// `y` above the cut, for the top the part with `z` below it, for the right the part with `x`
/// below it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionCut {
    /// The view in which the line of the cut is drawn.
    pub parent: Id,
    /// Where the plane is along the axis of the view, in nanometres.
    pub at_nm: i64,
    /// The letters of the cut (one to three capitals or digits), as in `A-A`.
    pub label: String,
}

/// The shape of the part of a view that a detail enlarges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailShape {
    /// A circle of this radius, in nanometres.
    Circle {
        /// The radius.
        radius_nm: i64,
    },
    /// A rectangle of this size, in nanometres.
    Rect {
        /// The width, across the picture.
        width_nm: i64,
        /// The height, up the picture.
        height_nm: i64,
    },
}

/// A detail view: the part of another view inside a circle or a rectangle, drawn larger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    /// The view that is enlarged; it comes before this one.
    pub parent: Id,
    /// The letter of the detail, as in `B (4:1)`.
    pub label: String,
    /// The centre of the shape, in nanometres, in the picture of the parent view.
    pub centre: (i64, i64),
    /// The shape.
    pub shape: DetailShape,
}

/// An axis of the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Axis {
    /// The first axis.
    X,
    /// The second axis.
    Y,
    /// The third axis.
    Z,
}

impl Axis {
    /// The index 0 to 2.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The letter of the file.
    pub fn name(self) -> &'static str {
        ["x", "y", "z"][self as usize]
    }

    /// The axis of a letter.
    pub fn from_name(name: &str) -> Option<Axis> {
        [Axis::X, Axis::Y, Axis::Z]
            .into_iter()
            .find(|a| a.name() == name)
    }
}

/// The low or the high face of a solid along an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The face with the lowest coordinate.
    Min,
    /// The face with the highest coordinate.
    Max,
}

/// What a dimension measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimKind {
    /// The distance between two faces along an axis.
    Linear,
    /// The radius of a circle or an arc of a sketch.
    Radius,
    /// The diameter of a circle or an arc of a sketch.
    Diameter,
    /// The angle between two lines of a sketch.
    Angle,
    /// The distances from the first face to each of the others along an axis, stacked (the
    /// parallel dimensions, all from the datum).
    Cumulative,
    /// The distances between each face and the next one along an axis, on one line (the chain).
    Chain,
    /// The distances from the first face to each of the others, as coordinates on one line from
    /// a common origin, with no arrows.
    Ordinate,
}

impl DimKind {
    /// Every kind.
    pub const ALL: [DimKind; 7] = [
        DimKind::Linear,
        DimKind::Radius,
        DimKind::Diameter,
        DimKind::Angle,
        DimKind::Cumulative,
        DimKind::Chain,
        DimKind::Ordinate,
    ];

    /// The word of the file.
    pub fn name(self) -> &'static str {
        match self {
            DimKind::Linear => "linear",
            DimKind::Radius => "radius",
            DimKind::Diameter => "diameter",
            DimKind::Angle => "angle",
            DimKind::Cumulative => "cumulative",
            DimKind::Chain => "chain",
            DimKind::Ordinate => "ordinate",
        }
    }

    /// The kind of a word.
    pub fn from_name(name: &str) -> Option<DimKind> {
        DimKind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The fewest and the most references the kind takes.
    pub fn arity(self) -> (usize, usize) {
        match self {
            DimKind::Linear | DimKind::Angle => (2, 2),
            DimKind::Radius | DimKind::Diameter => (1, 1),
            DimKind::Cumulative | DimKind::Chain | DimKind::Ordinate => (3, 16),
        }
    }
}

/// A thing of the model that a dimension is tied to, by an id that does not change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ref {
    /// A face of the box that holds the solid of a node of the history.
    Face {
        /// The node.
        node: Id,
        /// The axis across the face.
        axis: Axis,
        /// The low or the high face.
        side: Side,
    },
    /// An element of a sketch of the project.
    Element {
        /// The name of the sketch.
        sketch: String,
        /// The number of the element in it.
        element: u32,
    },
}

/// A dimension: what it measures and where it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dim {
    /// Its identity.
    pub id: Id,
    /// Its name.
    pub name: String,
    /// What it measures.
    pub kind: DimKind,
    /// The things it is tied to, in order.
    pub refs: Vec<Ref>,
    /// The view it is drawn in.
    pub view: Id,
    /// The distance of its first line from the view, when it is not the one of the style. A
    /// dimension with none is placed by the sheet so that it does not lie on another one.
    pub offset_um: Option<i64>,
    /// The distance between its stacked lines, when it is not the one of the style.
    pub step_um: Option<i64>,
    /// The tolerance written next to its value.
    pub tol: Option<Tol>,
    /// The layer it is drawn on.
    pub layer: String,
    /// The lines of a newer program that the record keeps.
    pub extra: Vec<String>,
}

/// A balloon: a number in a circle with a leader to a place of a view, tied to a part of the bill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Balloon {
    /// Its identity.
    pub id: Id,
    /// Its name.
    pub name: String,
    /// The node of the history that is the part.
    pub part: Id,
    /// The centre of the circle, on the sheet, in micrometres.
    pub at: (i64, i64),
    /// The end of the leader, on the sheet, in micrometres.
    pub to: (i64, i64),
    /// The lines of a newer program that the record keeps.
    pub extra: Vec<String>,
}

/// A line of the table of revisions. The number is given when the revision is added and is never
/// given again; the date and the author are free texts (the program reads no clock).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    /// Its number, from 1.
    pub number: u32,
    /// The date, as the person writes it (may be empty).
    pub date: String,
    /// What changed.
    pub description: String,
    /// Who did it (may be empty).
    pub author: String,
    /// The lines of a newer program that the record keeps.
    pub extra: Vec<String>,
}

/// The mark of a revision in the table: `A`, `B` ... `Z`, `AA` ... or its number.
pub fn revision_label(number: u32, numbering: RevisionNumbering) -> String {
    match numbering {
        RevisionNumbering::Numbers => number.to_string(),
        RevisionNumbering::Letters => {
            let mut n = number;
            let mut out = Vec::new();
            while n > 0 {
                n -= 1;
                out.push(b'A' + (n % 26) as u8);
                n /= 26;
            }
            out.reverse();
            String::from_utf8(out).unwrap_or_default()
        }
    }
}

/// Whether a text is the letters of a cut or of a detail: one to three capitals or digits.
pub fn valid_label(s: &str) -> bool {
    (1..=3).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// The letters for the next cut or detail: `A` to `Z` without `I`, `O` and `Q` (they look like
/// digits), then `AA`... The first that no view of the sheet uses.
pub fn next_label(plan: &Plan) -> String {
    let used = |l: &str| {
        plan.views.iter().any(|v| {
            v.section.as_ref().is_some_and(|c| c.label == l)
                || v.detail.as_ref().is_some_and(|c| c.label == l)
        })
    };
    let letters: Vec<char> = ('A'..='Z').filter(|c| !"IOQ".contains(*c)).collect();
    for c in &letters {
        let l = c.to_string();
        if !used(&l) {
            return l;
        }
    }
    for a in &letters {
        for b in &letters {
            let l = format!("{a}{b}");
            if !used(&l) {
                return l;
            }
        }
    }
    "ZZZ".to_string()
}

/// What the person says about a part of the bill.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PartInfo {
    /// The material.
    pub material: Option<String>,
    /// The description.
    pub description: Option<String>,
    /// The free attributes, by key.
    pub attrs: BTreeMap<String, String>,
    /// The lines of a newer program that the record keeps.
    pub extra: Vec<String>,
}

/// What the drawing changes in a layer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LayerState {
    /// Shown or not.
    pub visible: Option<bool>,
    /// Protected or not.
    pub locked: Option<bool>,
    /// Six hexadecimal digits.
    pub color: Option<String>,
    /// The width in micrometres.
    pub width_um: Option<i64>,
}

/// The scale of a sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetScale {
    /// The largest standard scale at which the views fit.
    Auto,
    /// A ratio `n:d`.
    Ratio(u32, u32),
}

/// A drawing sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The name of the format.
    pub format: String,
    /// Whether the sheet is upright.
    pub portrait: bool,
    /// Its scale.
    pub scale: SheetScale,
    /// Whether the projection is of the third angle.
    pub third_angle: bool,
    /// The next number of an item of the bill.
    pub next_item: u32,
    /// The next number of a revision.
    pub next_revision: u32,
    /// The title block that the sheet has chosen, when it is not the one of the settings.
    pub title_template: Option<String>,
    /// The group of lines (the thick line, in micrometres) that the sheet has chosen.
    pub line_group_um: Option<i64>,
    /// The revisions, oldest first.
    pub revisions: Vec<Revision>,
    /// The fields of the title block that the drawing fills.
    pub fields: BTreeMap<String, String>,
    /// What the drawing changes in the layers.
    pub layers: BTreeMap<String, LayerState>,
    /// The lines of a newer program in the head.
    pub head_extra: Vec<String>,
    /// The numbers of the items of the bill, by node: once given they are never taken back.
    pub numbers: BTreeMap<Id, u32>,
    /// What is said about the parts, by node.
    pub parts: BTreeMap<Id, PartInfo>,
    /// The views, in order.
    pub views: Vec<View>,
    /// The dimensions, in order.
    pub dims: Vec<Dim>,
    /// The balloons, in order.
    pub balloons: Vec<Balloon>,
    /// The notes, in order.
    pub notes: Vec<Note>,
}

fn bad(why: impl Into<String>) -> CadError {
    CadError::Plan(why.into())
}

/// Whether a text typed by the person may go in the file: one line, no control character, no
/// space at either end, not empty and at most `max` characters.
pub fn valid_text(s: &str, max: usize) -> bool {
    !s.is_empty() && s.chars().count() <= max && s.trim() == s && !s.chars().any(char::is_control)
}

/// Checks the key and the value of an attribute against the limits of the settings: a key of the
/// characters of `sheet.attr.key_charset`, a value of one line with no space at either end, both
/// not empty and not longer than the settings allow, a value in the character set the settings say.
pub fn valid_attr(key: &str, value: &str, s: &SheetSettings) -> Result<(), CadError> {
    let a = &s.attr;
    let bad = |why: String| CadError::PlanAttr(why);
    if key.is_empty() || key.chars().count() > a.key_max {
        return Err(bad(format!("a key is from 1 to {} characters", a.key_max)));
    }
    if let Some(c) = key.chars().find(|c| !a.key_charset.contains(*c)) {
        return Err(bad(format!(
            "the key {key} has the character {c:?}, which the settings do not allow"
        )));
    }
    if !valid_text(value, a.value_max) {
        return Err(bad(format!(
            "the value of {key} is one line of up to {} characters, with no space at either end",
            a.value_max
        )));
    }
    if a.value_charset == super::settings::ValueCharset::Ascii && !value.is_ascii() {
        return Err(bad(format!(
            "the value of {key} has a character that is not ASCII"
        )));
    }
    Ok(())
}

impl Plan {
    /// A new sheet with what the settings say.
    pub fn new(s: &SheetSettings) -> Plan {
        Plan {
            format: s.default_format.clone(),
            portrait: s.default_portrait,
            scale: match parse_scale(&s.default_scale) {
                Some((n, d)) => SheetScale::Ratio(n, d),
                None => SheetScale::Auto,
            },
            third_angle: s.third_angle,
            next_item: 1,
            next_revision: 1,
            title_template: None,
            line_group_um: None,
            revisions: Vec::new(),
            fields: BTreeMap::new(),
            layers: BTreeMap::new(),
            head_extra: Vec::new(),
            numbers: BTreeMap::new(),
            parts: BTreeMap::new(),
            views: Vec::new(),
            dims: Vec::new(),
            balloons: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Whether the sheet needs version 2 of the text: it has a broken view, a note or an attribute.
    pub fn needs_v2(&self) -> bool {
        !self.notes.is_empty()
            || self.views.iter().any(|v| v.brk.is_some())
            || self.parts.values().any(|p| !p.attrs.is_empty())
    }

    /// Whether a layer is shown (the drawing may change what the settings say).
    pub fn layer_visible(&self, name: &str, s: &SheetSettings) -> bool {
        self.layers
            .get(name)
            .and_then(|l| l.visible)
            .or_else(|| s.layer(name).map(|l| l.visible))
            .unwrap_or(false)
    }

    /// Whether a layer is protected.
    pub fn layer_locked(&self, name: &str, s: &SheetSettings) -> bool {
        self.layers
            .get(name)
            .and_then(|l| l.locked)
            .or_else(|| s.layer(name).map(|l| l.locked))
            .unwrap_or(false)
    }

    /// The colour of a layer.
    pub fn layer_color(&self, name: &str, s: &SheetSettings) -> String {
        self.layers
            .get(name)
            .and_then(|l| l.color.clone())
            .or_else(|| s.layer(name).map(|l| l.color.clone()))
            .unwrap_or_else(|| "000000".to_string())
    }

    /// The width of the lines of a layer, in micrometres.
    pub fn layer_width_um(&self, name: &str, s: &SheetSettings) -> i64 {
        let group = self.line_group_um.unwrap_or(s.line_group_um);
        self.layers
            .get(name)
            .and_then(|l| l.width_um)
            .or_else(|| s.layer(name).map(|l| l.width.width_um(group)))
            .unwrap_or(group / 2)
    }

    /// Chooses the group of lines of the sheet (one of the settings), or goes back to the one of
    /// the settings with `None`. The layers whose width is thick, thin or extra follow it.
    pub fn set_line_group(&mut self, um: Option<i64>, s: &SheetSettings) -> Result<(), CadError> {
        if let Some(v) = um {
            if !s.line_groups.contains(&v) {
                return Err(bad(format!(
                    "{v} um is not one of the groups of lines of the settings"
                )));
            }
        }
        self.line_group_um = um.filter(|v| *v != s.line_group_um);
        Ok(())
    }

    /// Chooses the title block of the sheet (a name of the settings), or the one of the settings
    /// with `None`.
    pub fn set_title_template(
        &mut self,
        name: Option<&str>,
        s: &SheetSettings,
    ) -> Result<(), CadError> {
        self.need_unlocked("FRAME", s)?;
        if let Some(n) = name {
            if s.title_block_of(Some(n)).is_none() {
                return Err(bad(format!("the title block {n} is not in the settings")));
            }
        }
        self.title_template = name.filter(|n| *n != s.title_template).map(str::to_string);
        Ok(())
    }

    /// Adds a revision with the next number (a number is never given twice). The date and the
    /// author may be empty. The layer FRAME must not be locked.
    pub fn add_revision(
        &mut self,
        date: &str,
        description: &str,
        author: &str,
        s: &SheetSettings,
    ) -> Result<u32, CadError> {
        self.need_unlocked("FRAME", s)?;
        if self.revisions.len() >= s.revisions.max {
            return Err(CadError::PlanRevision(format!(
                "more than {} revisions",
                s.revisions.max
            )));
        }
        let mut next = self.clone();
        let number = next.next_revision;
        next.next_revision += 1;
        next.revisions.push(Revision {
            number,
            date: date.to_string(),
            description: description.to_string(),
            author: author.to_string(),
            extra: Vec::new(),
        });
        next.validate(s)?;
        *self = next;
        Ok(number)
    }

    /// Takes a revision away; its number is not given again.
    pub fn remove_revision(&mut self, number: u32, s: &SheetSettings) -> Result<(), CadError> {
        self.need_unlocked("FRAME", s)?;
        let at = self
            .revisions
            .iter()
            .position(|r| r.number == number)
            .ok_or_else(|| CadError::UnknownName(number.to_string()))?;
        self.revisions.remove(at);
        Ok(())
    }

    /// Moves the line of a dimension: its distance from the view, or `None` to let the sheet
    /// place it. The layer of the dimension must not be locked.
    pub fn set_dim_offset(
        &mut self,
        id: Id,
        offset_um: Option<i64>,
        s: &SheetSettings,
    ) -> Result<(), CadError> {
        let at = self
            .dims
            .iter()
            .position(|d| d.id == id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
        self.need_unlocked(&self.dims[at].layer, s)?;
        if let Some(o) = offset_um {
            if !(-1_000_000..=1_000_000).contains(&o) {
                return Err(bad("the offset of a dimension is within a metre"));
            }
        }
        self.dims[at].offset_um = offset_um;
        Ok(())
    }

    /// Sets the tolerance of a dimension, or takes it away with `None`.
    pub fn set_dim_tol(
        &mut self,
        id: Id,
        tol: Option<Tol>,
        s: &SheetSettings,
    ) -> Result<(), CadError> {
        let at = self
            .dims
            .iter()
            .position(|d| d.id == id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
        self.need_unlocked(&self.dims[at].layer, s)?;
        let mut next = self.clone();
        next.dims[at].tol = tol;
        next.validate(s)?;
        *self = next;
        Ok(())
    }

    fn id_in_use(&self, id: Id) -> bool {
        self.views.iter().any(|v| v.id == id)
            || self.dims.iter().any(|d| d.id == id)
            || self.balloons.iter().any(|b| b.id == id)
            || self.notes.iter().any(|n| n.id == id)
    }

    /// The view of an id.
    pub fn view(&self, id: Id) -> Option<&View> {
        self.views.iter().find(|v| v.id == id)
    }

    fn need_unlocked(&self, layer: &str, s: &SheetSettings) -> Result<(), CadError> {
        if self.layer_locked(layer, s) {
            Err(bad(format!("the layer {layer} is locked")))
        } else {
            Ok(())
        }
    }

    /// Adds a view. The layer OUTLINE must not be locked.
    pub fn add_view(&mut self, view: View, s: &SheetSettings) -> Result<(), CadError> {
        self.need_unlocked("OUTLINE", s)?;
        if self.views.len() >= s.max_views {
            return Err(bad(format!("more than {} views", s.max_views)));
        }
        if self.id_in_use(view.id) {
            return Err(CadError::Duplicate(view.id.to_string()));
        }
        let mut next = self.clone();
        next.views.push(view);
        next.validate(s)?;
        *self = next;
        Ok(())
    }

    /// Takes a view away; refused while a dimension is drawn in it.
    pub fn remove_view(&mut self, id: Id, s: &SheetSettings) -> Result<(), CadError> {
        self.need_unlocked("OUTLINE", s)?;
        let at = self
            .views
            .iter()
            .position(|v| v.id == id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
        if self.dims.iter().any(|d| d.view == id) {
            return Err(CadError::InUse(id.to_string()));
        }
        self.views.remove(at);
        Ok(())
    }

    /// Adds a dimension. Its layer must not be locked.
    pub fn add_dim(&mut self, dim: Dim, s: &SheetSettings) -> Result<(), CadError> {
        self.need_unlocked(&dim.layer, s)?;
        if self.dims.len() >= s.max_dims {
            return Err(bad(format!("more than {} dimensions", s.max_dims)));
        }
        if self.id_in_use(dim.id) {
            return Err(CadError::Duplicate(dim.id.to_string()));
        }
        let mut next = self.clone();
        next.dims.push(dim);
        next.validate(s)?;
        *self = next;
        Ok(())
    }

    /// Takes a dimension away.
    pub fn remove_dim(&mut self, id: Id, s: &SheetSettings) -> Result<(), CadError> {
        let at = self
            .dims
            .iter()
            .position(|d| d.id == id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
        self.need_unlocked(&self.dims[at].layer, s)?;
        self.dims.remove(at);
        Ok(())
    }

    /// Adds a balloon. The layer BALLOON must not be locked.
    pub fn add_balloon(&mut self, balloon: Balloon, s: &SheetSettings) -> Result<(), CadError> {
        self.need_unlocked("BALLOON", s)?;
        if self.balloons.len() >= s.max_balloons {
            return Err(bad(format!("more than {} balloons", s.max_balloons)));
        }
        if self.id_in_use(balloon.id) {
            return Err(CadError::Duplicate(balloon.id.to_string()));
        }
        let mut next = self.clone();
        next.balloons.push(balloon);
        next.validate(s)?;
        *self = next;
        Ok(())
    }

    /// Takes a balloon away.
    pub fn remove_balloon(&mut self, id: Id, s: &SheetSettings) -> Result<(), CadError> {
        self.need_unlocked("BALLOON", s)?;
        let at = self
            .balloons
            .iter()
            .position(|b| b.id == id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
        self.balloons.remove(at);
        Ok(())
    }

    /// Moves a balloon (the circle and the end of its leader). The layer BALLOON must not be locked.
    pub fn move_balloon(
        &mut self,
        id: Id,
        at: (i64, i64),
        to: (i64, i64),
        s: &SheetSettings,
    ) -> Result<(), CadError> {
        self.need_unlocked("BALLOON", s)?;
        let b = self
            .balloons
            .iter_mut()
            .find(|b| b.id == id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
        b.at = at;
        b.to = to;
        Ok(())
    }

    /// Sets a field of the title block, or takes it away with an empty text.
    pub fn set_field(&mut self, key: &str, value: &str, s: &SheetSettings) -> Result<(), CadError> {
        self.need_unlocked("FRAME", s)?;
        if !is_word(key) {
            return Err(CadError::InvalidName(key.to_string()));
        }
        if value.is_empty() {
            self.fields.remove(key);
            return Ok(());
        }
        if !valid_text(value, s.max_text) {
            return Err(bad(format!("the field {key} is not a text of one line")));
        }
        self.fields.insert(key.to_string(), value.to_string());
        Ok(())
    }

    /// Shows or hides a layer.
    pub fn set_layer_visible(
        &mut self,
        name: &str,
        on: bool,
        s: &SheetSettings,
    ) -> Result<(), CadError> {
        if s.layer(name).is_none() {
            return Err(CadError::UnknownName(name.to_string()));
        }
        self.layers.entry(name.to_string()).or_default().visible = Some(on);
        Ok(())
    }

    /// Protects a layer or lets it be changed again.
    pub fn set_layer_locked(
        &mut self,
        name: &str,
        on: bool,
        s: &SheetSettings,
    ) -> Result<(), CadError> {
        if s.layer(name).is_none() {
            return Err(CadError::UnknownName(name.to_string()));
        }
        self.layers.entry(name.to_string()).or_default().locked = Some(on);
        Ok(())
    }

    /// Sets the colour of a layer (six hexadecimal digits) and the width of its lines.
    pub fn set_layer_look(
        &mut self,
        name: &str,
        color: &str,
        width_um: i64,
        s: &SheetSettings,
    ) -> Result<(), CadError> {
        if s.layer(name).is_none() {
            return Err(CadError::UnknownName(name.to_string()));
        }
        self.need_unlocked(name, s)?;
        if color.len() != 6 || !color.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad("a colour is six hexadecimal digits"));
        }
        if !(10..=5000).contains(&width_um) {
            return Err(bad("a width is from 10 to 5000 um"));
        }
        let l = self.layers.entry(name.to_string()).or_default();
        l.color = Some(color.to_ascii_lowercase());
        l.width_um = Some(width_um);
        Ok(())
    }

    /// Says what a part is made of and what it is called in the bill; an empty text leaves that
    /// to the defaults.
    pub fn set_part(
        &mut self,
        node: Id,
        material: &str,
        description: &str,
        s: &SheetSettings,
    ) -> Result<(), CadError> {
        for t in [material, description] {
            if !t.is_empty() && !valid_text(t, s.max_text) {
                return Err(bad("a text of the bill is one line"));
            }
        }
        if !self.parts.contains_key(&node) && self.parts.len() >= s.max_parts {
            return Err(bad(format!("more than {} parts", s.max_parts)));
        }
        let p = self.parts.entry(node).or_default();
        p.material = (!material.is_empty()).then(|| material.to_string());
        p.description = (!description.is_empty()).then(|| description.to_string());
        if p.material.is_none()
            && p.description.is_none()
            && p.attrs.is_empty()
            && p.extra.is_empty()
        {
            self.parts.remove(&node);
        }
        Ok(())
    }

    /// Sets a free attribute of a part (a node of the history), or changes its value. The keys,
    /// the values and their number are under the limits of the settings.
    pub fn set_attr(
        &mut self,
        node: Id,
        key: &str,
        value: &str,
        s: &SheetSettings,
    ) -> Result<(), CadError> {
        valid_attr(key, value, s)?;
        if !self.parts.contains_key(&node) && self.parts.len() >= s.max_parts {
            return Err(bad(format!("more than {} parts", s.max_parts)));
        }
        let mut next = self.clone();
        next.parts
            .entry(node)
            .or_default()
            .attrs
            .insert(key.to_string(), value.to_string());
        next.validate(s)?;
        *self = next;
        Ok(())
    }

    /// Takes an attribute away; the key must be there.
    pub fn remove_attr(&mut self, node: Id, key: &str) -> Result<(), CadError> {
        let info = self
            .parts
            .get_mut(&node)
            .ok_or_else(|| CadError::UnknownName(node.to_string()))?;
        if info.attrs.remove(key).is_none() {
            return Err(CadError::UnknownName(key.to_string()));
        }
        if info.material.is_none()
            && info.description.is_none()
            && info.attrs.is_empty()
            && info.extra.is_empty()
        {
            self.parts.remove(&node);
        }
        Ok(())
    }

    /// The value of an attribute of a part.
    pub fn attr(&self, node: Id, key: &str) -> Option<&str> {
        self.parts
            .get(&node)
            .and_then(|p| p.attrs.get(key))
            .map(String::as_str)
    }

    /// Makes a view a broken view with the range given, or an ordinary one with `None`. The layer
    /// OUTLINE must not be locked.
    pub fn set_break(
        &mut self,
        view: Id,
        brk: Option<BreakCut>,
        s: &SheetSettings,
    ) -> Result<(), CadError> {
        self.need_unlocked("OUTLINE", s)?;
        let at = self
            .views
            .iter()
            .position(|v| v.id == view)
            .ok_or_else(|| CadError::UnknownName(view.to_string()))?;
        let mut next = self.clone();
        next.views[at].brk = brk;
        next.validate(s)?;
        *self = next;
        Ok(())
    }

    /// Adds a note. Its layer must not be locked.
    pub fn add_note(&mut self, note: Note, s: &SheetSettings) -> Result<(), CadError> {
        self.need_unlocked(&note.layer, s)?;
        if self.notes.len() >= s.max_notes {
            return Err(CadError::PlanNote(format!(
                "more than {} notes",
                s.max_notes
            )));
        }
        if self.id_in_use(note.id) {
            return Err(CadError::Duplicate(note.id.to_string()));
        }
        let mut next = self.clone();
        next.notes.push(note);
        next.validate(s)?;
        *self = next;
        Ok(())
    }

    /// Takes a note away.
    pub fn remove_note(&mut self, id: Id, s: &SheetSettings) -> Result<(), CadError> {
        let at = self
            .notes
            .iter()
            .position(|n| n.id == id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
        self.need_unlocked(&self.notes[at].layer, s)?;
        self.notes.remove(at);
        Ok(())
    }

    /// Moves the text of a note (the leader follows). The layer of the note must not be locked.
    pub fn move_note(&mut self, id: Id, at: (i64, i64), s: &SheetSettings) -> Result<(), CadError> {
        let i = self
            .notes
            .iter()
            .position(|n| n.id == id)
            .ok_or_else(|| CadError::UnknownName(id.to_string()))?;
        self.need_unlocked(&self.notes[i].layer, s)?;
        if at.0.abs() > 100_000_000 || at.1.abs() > 100_000_000 {
            return Err(CadError::PlanNote(
                "a note is within a hundred metres of the origin".into(),
            ));
        }
        self.notes[i].at = at;
        Ok(())
    }

    /// Moves the plane of the cut of a section view. The layer SECTION must not be locked.
    pub fn move_cut(&mut self, view: Id, at_nm: i64, s: &SheetSettings) -> Result<(), CadError> {
        self.need_unlocked("SECTION", s)?;
        let i = self
            .views
            .iter()
            .position(|v| v.id == view && v.section.is_some())
            .ok_or_else(|| CadError::UnknownName(view.to_string()))?;
        let mut next = self.clone();
        if let Some(c) = next.views[i].section.as_mut() {
            c.at_nm = at_nm;
        }
        next.validate(s)?;
        *self = next;
        Ok(())
    }

    /// Checks everything the file could get wrong; the first problem found is the error.
    pub fn validate(&self, s: &SheetSettings) -> Result<(), CadError> {
        if !s.formats.contains_key(&self.format) {
            return Err(bad(format!(
                "the format {} is not in the settings",
                self.format
            )));
        }
        if let SheetScale::Ratio(n, d) = self.scale {
            if parse_scale(&format!("{n}:{d}")).is_none() {
                return Err(bad("the scale is from 1:1000 to 1000:1"));
            }
        }
        if self.notes.len() > s.max_notes {
            return Err(CadError::PlanNote(format!(
                "more than {} notes",
                s.max_notes
            )));
        }
        if self.views.len() > s.max_views
            || self.dims.len() > s.max_dims
            || self.balloons.len() > s.max_balloons
            || self.parts.len() > s.max_parts
        {
            return Err(bad("more records than the settings allow"));
        }
        for (k, v) in &self.fields {
            if !is_word(k) || !valid_text(v, s.max_text) {
                return Err(bad(format!("the field {k} is not valid")));
            }
        }
        for (name, l) in &self.layers {
            if s.layer(name).is_none() {
                return Err(bad(format!("the layer {name} is not in the settings")));
            }
            if let Some(c) = &l.color {
                if c.len() != 6
                    || !c
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                {
                    return Err(bad(format!(
                        "the colour of {name} is six lower case hexadecimal digits"
                    )));
                }
            }
            if let Some(w) = l.width_um {
                if !(10..=5000).contains(&w) {
                    return Err(bad(format!("the width of {name} is from 10 to 5000 um")));
                }
            }
        }
        let mut ids: BTreeSet<Id> = BTreeSet::new();
        for v in &self.views {
            if !ids.insert(v.id) {
                return Err(bad(format!("the id {} is used twice", v.id)));
            }
            if !valid_text(&v.name, s.max_text) {
                return Err(bad("the name of a view is not valid"));
            }
            if let Some((n, d)) = v.scale {
                if parse_scale(&format!("{n}:{d}")).is_none() {
                    return Err(bad("the scale of a view is from 1:1000 to 1000:1"));
                }
            }
            let sketch_kind = v.kind == ViewKind::Sketch;
            let sketch_source = matches!(v.source, Source::Sketch(_));
            if sketch_kind != sketch_source {
                return Err(bad(format!(
                    "the view {} is a sketch only when its source is one",
                    v.name
                )));
            }
            if let Source::Sketch(n) = &v.source {
                if !valid_text(n, s.max_text) {
                    return Err(bad("the name of a sketch is not valid"));
                }
            }
            self.validate_derived(v, s)?;
        }
        let view_ids: BTreeSet<Id> = self.views.iter().map(|v| v.id).collect();
        for d in &self.dims {
            if !ids.insert(d.id) {
                return Err(bad(format!("the id {} is used twice", d.id)));
            }
            if !valid_text(&d.name, s.max_text) {
                return Err(bad("the name of a dimension is not valid"));
            }
            let (lo, hi) = d.kind.arity();
            if !(lo..=hi).contains(&d.refs.len()) {
                return Err(CadError::PlanDim(format!(
                    "{} takes from {lo} to {hi} references, it has {}",
                    d.name,
                    d.refs.len()
                )));
            }
            if !view_ids.contains(&d.view) {
                return Err(CadError::PlanDim(format!(
                    "{} is drawn in a view that is not there",
                    d.name
                )));
            }
            if s.layer(&d.layer).is_none() {
                return Err(bad(format!("the layer {} is not in the settings", d.layer)));
            }
            if let Some(o) = d.offset_um {
                if !(-1_000_000..=1_000_000).contains(&o) {
                    return Err(bad("the offset of a dimension is within a metre"));
                }
            }
            if let Some(st) = d.step_um {
                if !(1000..=200_000).contains(&st) {
                    return Err(CadError::PlanDim(format!(
                        "{}: the step between lines is from 1 to 200 mm",
                        d.name
                    )));
                }
            }
            if let Some(tol) = &d.tol {
                super::tol::check(tol, d.kind)
                    .map_err(|why| CadError::PlanTol(format!("{}: {why}", d.name)))?;
            }
            for r in &d.refs {
                if let Ref::Element { sketch, .. } = r {
                    if !valid_text(sketch, s.max_text) {
                        return Err(bad("the name of a sketch is not valid"));
                    }
                }
            }
        }
        for b in &self.balloons {
            if !ids.insert(b.id) {
                return Err(bad(format!("the id {} is used twice", b.id)));
            }
            if !valid_text(&b.name, s.max_text) {
                return Err(bad("the name of a balloon is not valid"));
            }
            for p in [b.at, b.to] {
                if p.0.abs() > 100_000_000 || p.1.abs() > 100_000_000 {
                    return Err(bad("a balloon is within a hundred metres of the origin"));
                }
            }
        }
        for n in &self.notes {
            if !ids.insert(n.id) {
                return Err(bad(format!("the id {} is used twice", n.id)));
            }
            let note = |why: &str| CadError::PlanNote(why.to_string());
            if !valid_text(&n.text, s.note.max_chars) {
                return Err(note(&format!(
                    "a note is one line of up to {} characters",
                    s.note.max_chars
                )));
            }
            if s.layer(&n.layer).is_none() {
                return Err(bad(format!("the layer {} is not in the settings", n.layer)));
            }
            if n.at.0.abs() > 100_000_000 || n.at.1.abs() > 100_000_000 {
                return Err(note("a note is within a hundred metres of the origin"));
            }
            let (view, big) = match &n.anchor {
                NoteAnchor::View { view, u_nm, v_nm } => (
                    *view,
                    u_nm.abs() > 1_000_000_000_000 || v_nm.abs() > 1_000_000_000_000,
                ),
                NoteAnchor::Face { view, .. } => (*view, false),
            };
            if big {
                return Err(note("the anchor is within a kilometre of the origin"));
            }
            match self.view(view) {
                None => return Err(note("the note points into a view that is not there")),
                Some(v) => {
                    if matches!(n.anchor, NoteAnchor::Face { .. })
                        && (v.kind.depth_axis().is_none() || matches!(v.source, Source::Sketch(_)))
                    {
                        return Err(note(
                            "a face is shown by the front, the top or the right view of a solid",
                        ));
                    }
                }
            }
        }
        for (node, n) in &self.numbers {
            if *n == 0 || *n >= self.next_item {
                return Err(bad(format!(
                    "the item number {n} of {node} is not below the next one"
                )));
            }
            if self.numbers.iter().filter(|(_, m)| *m == n).count() != 1 {
                return Err(bad(format!("the item number {n} is used twice")));
            }
        }
        if self.next_item == 0 {
            return Err(bad("the next item number is from 1"));
        }
        if let Some(n) = &self.title_template {
            if s.title_block_of(Some(n)).is_none() {
                return Err(bad(format!("the title block {n} is not in the settings")));
            }
        }
        if let Some(g) = self.line_group_um {
            if !s.line_groups.contains(&g) {
                return Err(bad(format!(
                    "{g} um is not one of the groups of lines of the settings"
                )));
            }
        }
        let rev = |why: String| CadError::PlanRevision(why);
        if self.next_revision == 0 || self.revisions.len() > s.revisions.max {
            return Err(rev(format!(
                "from 1 to {} revisions and the next number from 1",
                s.revisions.max
            )));
        }
        let mut last = 0;
        for r in &self.revisions {
            if r.number == 0 || r.number >= self.next_revision || r.number <= last {
                return Err(rev(format!(
                    "the revision {} is not in order below the next number",
                    r.number
                )));
            }
            last = r.number;
            if !valid_text(&r.description, s.max_text) {
                return Err(rev(format!(
                    "the description of the revision {} is one line",
                    r.number
                )));
            }
            for t in [&r.date, &r.author] {
                if !t.is_empty() && !valid_text(t, s.max_text) {
                    return Err(rev(format!(
                        "the date and the author of the revision {} are one line",
                        r.number
                    )));
                }
            }
        }
        for p in self.parts.values() {
            for t in [&p.material, &p.description].into_iter().flatten() {
                if !valid_text(t, s.max_text) {
                    return Err(bad("a text of the bill is not valid"));
                }
            }
            if p.attrs.len() > s.attr.max_per_node {
                return Err(CadError::PlanAttr(format!(
                    "more than {} attributes in a part",
                    s.attr.max_per_node
                )));
            }
            for (k, v) in &p.attrs {
                valid_attr(k, v, s)?;
            }
        }
        let lines = self
            .head_extra
            .iter()
            .chain(self.views.iter().flat_map(|v| v.extra.iter()))
            .chain(self.dims.iter().flat_map(|v| v.extra.iter()))
            .chain(self.balloons.iter().flat_map(|v| v.extra.iter()))
            .chain(self.notes.iter().flat_map(|v| v.extra.iter()))
            .chain(self.revisions.iter().flat_map(|v| v.extra.iter()))
            .chain(self.parts.values().flat_map(|v| v.extra.iter()));
        for l in lines {
            if l.len() > s.max_extra_bytes
                || !l.starts_with("x-")
                || l.chars().any(char::is_control)
            {
                return Err(bad("a line to keep is not valid"));
            }
        }
        for rec in [
            self.head_extra.len(),
            self.views.iter().map(|v| v.extra.len()).max().unwrap_or(0),
            self.dims.iter().map(|v| v.extra.len()).max().unwrap_or(0),
            self.balloons
                .iter()
                .map(|v| v.extra.len())
                .max()
                .unwrap_or(0),
            self.notes.iter().map(|v| v.extra.len()).max().unwrap_or(0),
            self.parts
                .values()
                .map(|v| v.extra.len())
                .max()
                .unwrap_or(0),
            self.revisions
                .iter()
                .map(|v| v.extra.len())
                .max()
                .unwrap_or(0),
        ] {
            if rec > s.max_extra_lines {
                return Err(bad("a record keeps more lines than the settings allow"));
            }
        }
        Ok(())
    }

    /// Checks the cut or the detail of a view.
    fn validate_derived(&self, v: &View, s: &SheetSettings) -> Result<(), CadError> {
        let sec = |why: String| CadError::PlanSection(format!("{}: {why}", v.name));
        if v.section.is_some() && v.detail.is_some() {
            return Err(sec("a view is a section or a detail, not both".into()));
        }
        if v.hatch.is_some() && v.section.is_none() {
            return Err(sec("only a section is hatched".into()));
        }
        if let Some(h) = &v.hatch {
            if !valid_text(h, s.max_text) {
                return Err(sec("the material of the hatching is one line".into()));
            }
        }
        if let Some(b) = &v.brk {
            let brk = |why: String| CadError::PlanBreak(format!("{}: {why}", v.name));
            if v.kind.depth_axis().is_none() || matches!(v.source, Source::Sketch(_)) {
                return Err(brk(
                    "a break is made in a view of a solid, from the front, the top or the right"
                        .into(),
                ));
            }
            if v.section.is_some() || v.detail.is_some() {
                return Err(brk("a section or a detail view cannot be broken".into()));
            }
            let big = 1_000_000_000_000;
            if b.from_nm.abs() > big || b.to_nm.abs() > big {
                return Err(brk("the range is within a kilometre of the origin".into()));
            }
            if b.to_nm <= b.from_nm {
                return Err(brk("the range ends before it starts".into()));
            }
            if b.to_nm - b.from_nm <= s.brk.gap_nm {
                return Err(brk(format!(
                    "the range is not longer than the gap of the break marks ({} nm)",
                    s.brk.gap_nm
                )));
            }
        }
        let used = |label: &str| {
            self.views
                .iter()
                .filter(|o| {
                    o.section.as_ref().is_some_and(|c| c.label == label)
                        || o.detail.as_ref().is_some_and(|c| c.label == label)
                })
                .count()
        };
        if let Some(c) = &v.section {
            if v.kind.depth_axis().is_none() || matches!(v.source, Source::Sketch(_)) {
                return Err(sec(
                    "a section is a view of a solid, from the front, the top or the right".into(),
                ));
            }
            if !valid_label(&c.label) {
                return Err(sec("the letters of a cut are one to three capitals".into()));
            }
            if c.parent == v.id || self.view(c.parent).is_none() {
                return Err(sec(
                    "the view of the line of the cut is not another view".into()
                ));
            }
            if c.at_nm.abs() > 1_000_000_000_000 {
                return Err(sec("the cut is within a kilometre of the origin".into()));
            }
            if used(&c.label) > 1 {
                return Err(sec(format!("the letters {} are used twice", c.label)));
            }
        }
        if let Some(d) = &v.detail {
            if !valid_label(&d.label) {
                return Err(sec("the letter of a detail is one to three capitals".into()));
            }
            if used(&d.label) > 1 {
                return Err(sec(format!("the letters {} are used twice", d.label)));
            }
            let me = self.views.iter().position(|o| o.id == v.id).unwrap_or(0);
            let parent = self.views.iter().position(|o| o.id == d.parent);
            match parent {
                Some(i) if i < me => {
                    let pv = &self.views[i];
                    if pv.brk.is_some() {
                        return Err(CadError::PlanBreak(format!(
                            "{}: a detail cannot enlarge a broken view",
                            v.name
                        )));
                    }
                    if pv.kind == ViewKind::Sketch || v.kind != pv.kind {
                        return Err(sec(
                            "a detail has the kind of its view, and that view is of a solid".into(),
                        ));
                    }
                }
                _ => {
                    return Err(sec(
                        "the view that is enlarged comes before the detail".into()
                    ))
                }
            }
            let big = 1_000_000_000_000;
            if d.centre.0.abs() > big || d.centre.1.abs() > big {
                return Err(sec("the centre is within a kilometre of the origin".into()));
            }
            let ok = match d.shape {
                DetailShape::Circle { radius_nm } => (1..=big).contains(&radius_nm),
                DetailShape::Rect {
                    width_nm,
                    height_nm,
                } => (1..=big).contains(&width_nm) && (1..=big).contains(&height_nm),
            };
            if !ok {
                return Err(sec("the shape of a detail has a size above nothing".into()));
            }
        }
        Ok(())
    }

    // ----- the file -----

    /// The text of the drawing in its one canonical form: the same drawing gives the same bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut o = String::new();
        let mut line = |s: String| {
            o.push_str(&s);
            o.push('\n');
        };
        line(if self.needs_v2() { HEADER_2 } else { HEADER }.to_string());
        line(format!("format {}", self.format));
        line(format!(
            "orientation {}",
            if self.portrait {
                "portrait"
            } else {
                "landscape"
            }
        ));
        line(match self.scale {
            SheetScale::Auto => "scale auto".to_string(),
            SheetScale::Ratio(n, d) => format!("scale {n}:{d}"),
        });
        line(format!(
            "projection {}",
            if self.third_angle { "third" } else { "first" }
        ));
        line(format!("nextitem {}", self.next_item));
        if self.next_revision != 1 {
            line(format!("nextrevision {}", self.next_revision));
        }
        if let Some(n) = &self.title_template {
            line(format!("titleblock {n}"));
        }
        if let Some(g) = self.line_group_um {
            line(format!("linegroup {g}"));
        }
        for (k, v) in &self.fields {
            line(format!("field {k} {v}"));
        }
        for (name, l) in &self.layers {
            if let Some(v) = l.visible {
                line(format!("layer {name} visible {}", u8::from(v)));
            }
            if let Some(v) = l.locked {
                line(format!("layer {name} locked {}", u8::from(v)));
            }
            if let Some(v) = &l.color {
                line(format!("layer {name} color {v}"));
            }
            if let Some(v) = l.width_um {
                line(format!("layer {name} width {v}"));
            }
        }
        for x in &self.head_extra {
            line(x.clone());
        }
        for (node, n) in &self.numbers {
            line(format!("item {node} {n}"));
        }
        for (node, p) in &self.parts {
            line(format!("part {node}"));
            if let Some(m) = &p.material {
                line(format!("material {m}"));
            }
            if let Some(d) = &p.description {
                line(format!("description {d}"));
            }
            for (k, v) in &p.attrs {
                line(format!("attr {k} {v}"));
            }
            for x in &p.extra {
                line(x.clone());
            }
        }
        for r in &self.revisions {
            line(format!("revision {}", r.number));
            if !r.date.is_empty() {
                line(format!("date {}", r.date));
            }
            line(format!("description {}", r.description));
            if !r.author.is_empty() {
                line(format!("author {}", r.author));
            }
            for x in &r.extra {
                line(x.clone());
            }
        }
        for v in &self.views {
            line(format!("view {} {} {}", v.id, v.kind.name(), v.name));
            line(match &v.source {
                Source::Tip => "source tip".to_string(),
                Source::Node(n) => format!("source node {n}"),
                Source::Sketch(n) => format!("source sketch {n}"),
            });
            if let Place::At(x, y) = v.place {
                line(format!("at {x} {y}"));
            }
            if let Some((n, d)) = v.scale {
                line(format!("scale {n}:{d}"));
            }
            line(format!("hidden {}", u8::from(v.hidden)));
            if let Some(c) = &v.section {
                line(format!("section {} {} {}", c.parent, c.label, c.at_nm));
            }
            if let Some(d) = &v.detail {
                match d.shape {
                    DetailShape::Circle { radius_nm } => line(format!(
                        "detail {} {} circle {} {} {radius_nm}",
                        d.parent, d.label, d.centre.0, d.centre.1
                    )),
                    DetailShape::Rect {
                        width_nm,
                        height_nm,
                    } => line(format!(
                        "detail {} {} rect {} {} {width_nm} {height_nm}",
                        d.parent, d.label, d.centre.0, d.centre.1
                    )),
                }
            }
            if let Some(h) = &v.hatch {
                line(format!("hatch {h}"));
            }
            if let Some(b) = &v.brk {
                line(format!(
                    "break {} {} {}",
                    if b.along_u { "u" } else { "v" },
                    b.from_nm,
                    b.to_nm
                ));
            }
            for x in &v.extra {
                line(x.clone());
            }
        }
        for d in &self.dims {
            line(format!("dim {} {} {}", d.id, d.kind.name(), d.name));
            for r in &d.refs {
                line(match r {
                    Ref::Face { node, axis, side } => format!(
                        "ref face {node} {} {}",
                        axis.name(),
                        if *side == Side::Min { "min" } else { "max" }
                    ),
                    Ref::Element { sketch, element } => format!("ref element {element} {sketch}"),
                });
            }
            line(format!("view {}", d.view));
            if let Some(o) = d.offset_um {
                line(format!("offset {o}"));
            }
            if let Some(st) = d.step_um {
                line(format!("step {st}"));
            }
            match &d.tol {
                Some(Tol::Class(c)) => line(format!("tol class {c}")),
                Some(Tol::Dev { upper_nm, lower_nm }) => {
                    line(format!("tol dev {upper_nm} {lower_nm}"));
                }
                None => {}
            }
            line(format!("layer {}", d.layer));
            for x in &d.extra {
                line(x.clone());
            }
        }
        for b in &self.balloons {
            line(format!("balloon {} {}", b.id, b.name));
            line(format!("part {}", b.part));
            line(format!("at {} {}", b.at.0, b.at.1));
            line(format!("to {} {}", b.to.0, b.to.1));
            for x in &b.extra {
                line(x.clone());
            }
        }
        for n in &self.notes {
            line(format!("note {} {}", n.id, n.text));
            line(match &n.anchor {
                NoteAnchor::View { view, u_nm, v_nm } => {
                    format!("anchor view {view} {u_nm} {v_nm}")
                }
                NoteAnchor::Face {
                    view,
                    node,
                    axis,
                    side,
                } => format!(
                    "anchor face {view} {node} {} {}",
                    axis.name(),
                    if *side == Side::Min { "min" } else { "max" }
                ),
            });
            line(format!("at {} {}", n.at.0, n.at.1));
            if n.centre {
                line("centre 1".to_string());
            }
            line(format!("layer {}", n.layer));
            for x in &n.extra {
                line(x.clone());
            }
        }
        o.into_bytes()
    }

    /// Reads a drawing. Whatever is wrong is a `PHOENIX-CAD-054` that names the line; a text that
    /// is not in the one canonical form is refused too, so what is read is what is written.
    pub fn from_bytes(bytes: &[u8], s: &SheetSettings) -> Result<Plan, CadError> {
        if bytes.len() > s.max_file_bytes {
            return Err(bad(format!(
                "{} bytes, the settings allow {}",
                bytes.len(),
                s.max_file_bytes
            )));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| bad("the drawing is not text"))?;
        let mut it = text.split('\n');
        if !matches!(it.next(), Some(HEADER | HEADER_2)) {
            return Err(bad(
                "the first line is not `drawing 1` or `drawing 2` (a newer version is not read)",
            ));
        }
        let lines: Vec<&str> = it.collect();
        let mut p = Plan::new(s);
        p.next_item = 0;
        p.title_template = None;
        let mut cur = Cur::Head;
        let mut head_done = false;
        for (n, l) in lines.iter().enumerate() {
            let at = n + 2;
            let last = n + 1 == lines.len();
            if l.is_empty() {
                if last {
                    continue;
                }
                return Err(bad(format!("line {at}: empty")));
            }
            if last {
                return Err(bad(format!(
                    "line {at}: the text does not end with a line end"
                )));
            }
            if l.chars().any(char::is_control) {
                return Err(bad(format!("line {at}: a control character")));
            }
            let (word, rest) = l.split_once(' ').unwrap_or((l, ""));
            let id_of = |t: &str| -> Result<Id, CadError> {
                t.parse::<Id>()
                    .map_err(|_| bad(format!("line {at}: {t:?} is not an id")))
            };
            let int = |t: &str| -> Result<i64, CadError> {
                t.parse::<i64>()
                    .map_err(|_| bad(format!("line {at}: {t:?} is not a whole number")))
            };
            if word.starts_with("x-") {
                let line = (*l).to_string();
                match &cur {
                    Cur::Head => p.head_extra.push(line),
                    Cur::Part(id) => {
                        if let Some(x) = p.parts.get_mut(id) {
                            x.extra.push(line);
                        }
                    }
                    Cur::View => {
                        if let Some(x) = p.views.last_mut() {
                            x.extra.push(line);
                        }
                    }
                    Cur::Dim => {
                        if let Some(x) = p.dims.last_mut() {
                            x.extra.push(line);
                        }
                    }
                    Cur::Balloon => {
                        if let Some(x) = p.balloons.last_mut() {
                            x.extra.push(line);
                        }
                    }
                    Cur::Note => {
                        if let Some(x) = p.notes.last_mut() {
                            x.extra.push(line);
                        }
                    }
                    Cur::Revision => {
                        if let Some(x) = p.revisions.last_mut() {
                            x.extra.push(line);
                        }
                    }
                }
                continue;
            }
            // the head: the lines come in the one order of `to_bytes`
            match (&cur, word) {
                (Cur::Head, "format") if !head_done => p.format = rest.to_string(),
                (Cur::Head, "orientation") if !head_done => {
                    p.portrait = match rest {
                        "portrait" => true,
                        "landscape" => false,
                        _ => return Err(bad(format!("line {at}: landscape or portrait"))),
                    }
                }
                (Cur::Head, "scale") if !head_done => {
                    p.scale = if rest == "auto" {
                        SheetScale::Auto
                    } else {
                        let (a, b) = parse_scale(rest)
                            .ok_or_else(|| bad(format!("line {at}: a scale is n:d")))?;
                        SheetScale::Ratio(a, b)
                    }
                }
                (Cur::Head, "projection") if !head_done => {
                    p.third_angle = match rest {
                        "third" => true,
                        "first" => false,
                        _ => return Err(bad(format!("line {at}: first or third"))),
                    }
                }
                (Cur::Head, "nextitem") if !head_done => {
                    p.next_item = u32::try_from(int(rest)?)
                        .map_err(|_| bad(format!("line {at}: not a number of item")))?
                }
                (Cur::Head, "nextrevision") if !head_done => {
                    p.next_revision = u32::try_from(int(rest)?)
                        .map_err(|_| bad(format!("line {at}: not a number of revision")))?
                }
                (Cur::Head, "titleblock") if !head_done => {
                    p.title_template = Some(rest.to_string())
                }
                (Cur::Head, "linegroup") if !head_done => p.line_group_um = Some(int(rest)?),
                (Cur::Head, "field") if !head_done => {
                    let (k, v) = rest
                        .split_once(' ')
                        .ok_or_else(|| bad(format!("line {at}: a field needs a key and a text")))?;
                    if p.fields.insert(k.to_string(), v.to_string()).is_some() {
                        return Err(bad(format!("line {at}: the field {k} twice")));
                    }
                }
                (Cur::Head, "layer") if !head_done => {
                    let mut f = rest.splitn(3, ' ');
                    let (name, key, value) = match (f.next(), f.next(), f.next()) {
                        (Some(a), Some(b), Some(c)) => (a, b, c),
                        _ => {
                            return Err(bad(format!(
                                "line {at}: layer needs a name, a key and a value"
                            )))
                        }
                    };
                    let st = p.layers.entry(name.to_string()).or_default();
                    match key {
                        "visible" | "locked" => {
                            let v = match value {
                                "0" => false,
                                "1" => true,
                                _ => return Err(bad(format!("line {at}: 0 or 1"))),
                            };
                            let slot = if key == "visible" {
                                &mut st.visible
                            } else {
                                &mut st.locked
                            };
                            if slot.replace(v).is_some() {
                                return Err(bad(format!("line {at}: {key} twice")));
                            }
                        }
                        "color" => {
                            if st.color.replace(value.to_string()).is_some() {
                                return Err(bad(format!("line {at}: color twice")));
                            }
                        }
                        "width" => {
                            if st.width_um.replace(int(value)?).is_some() {
                                return Err(bad(format!("line {at}: width twice")));
                            }
                        }
                        _ => return Err(bad(format!("line {at}: no layer key {key:?}"))),
                    }
                }
                (_, "revision") => {
                    head_done = true;
                    let number = u32::try_from(int(rest)?)
                        .map_err(|_| bad(format!("line {at}: not a number of revision")))?;
                    p.revisions.push(Revision {
                        number,
                        date: String::new(),
                        description: String::new(),
                        author: String::new(),
                        extra: Vec::new(),
                    });
                    cur = Cur::Revision;
                }
                (Cur::Revision, "date")
                | (Cur::Revision, "description")
                | (Cur::Revision, "author") => {
                    if let Some(r) = p.revisions.last_mut() {
                        let slot = match word {
                            "date" => &mut r.date,
                            "description" => &mut r.description,
                            _ => &mut r.author,
                        };
                        if !slot.is_empty() {
                            return Err(bad(format!("line {at}: {word} twice")));
                        }
                        *slot = rest.to_string();
                    }
                }
                (_, "item") => {
                    head_done = true;
                    let (a, b) = rest
                        .split_once(' ')
                        .ok_or_else(|| bad(format!("line {at}: item needs a node and a number")))?;
                    let num = u32::try_from(int(b)?)
                        .map_err(|_| bad(format!("line {at}: not a number of item")))?;
                    if p.numbers.insert(id_of(a)?, num).is_some() {
                        return Err(bad(format!("line {at}: the item {a} twice")));
                    }
                    cur = Cur::Head;
                }
                (Cur::Balloon, "part") => {
                    if let Some(b) = p.balloons.last_mut() {
                        b.part = id_of(rest)?;
                    }
                }
                (_, "part") => {
                    head_done = true;
                    let id = id_of(rest)?;
                    if p.parts.insert(id, PartInfo::default()).is_some() {
                        return Err(bad(format!("line {at}: the part {id} twice")));
                    }
                    cur = Cur::Part(id);
                }
                (Cur::Part(id), "attr") => {
                    let (k, v) = rest
                        .split_once(' ')
                        .ok_or_else(|| bad(format!("line {at}: attr needs a key and a value")))?;
                    let info = p
                        .parts
                        .get_mut(id)
                        .ok_or_else(|| bad(format!("line {at}: no part")))?;
                    if info.attrs.insert(k.to_string(), v.to_string()).is_some() {
                        return Err(bad(format!("line {at}: the attribute {k} twice")));
                    }
                }
                (Cur::Part(id), "material") | (Cur::Part(id), "description") => {
                    let info = p
                        .parts
                        .get_mut(id)
                        .ok_or_else(|| bad(format!("line {at}: no part")))?;
                    let slot = if word == "material" {
                        &mut info.material
                    } else {
                        &mut info.description
                    };
                    if slot.replace(rest.to_string()).is_some() {
                        return Err(bad(format!("line {at}: {word} twice")));
                    }
                }
                (_, "view") if !matches!(cur, Cur::Dim) => {
                    head_done = true;
                    let mut f = rest.splitn(3, ' ');
                    let (id, kind, name) = match (f.next(), f.next(), f.next()) {
                        (Some(a), Some(b), Some(c)) => (id_of(a)?, b, c),
                        _ => {
                            return Err(bad(format!(
                                "line {at}: view needs an id, a kind and a name"
                            )))
                        }
                    };
                    let kind = ViewKind::from_name(kind)
                        .ok_or_else(|| bad(format!("line {at}: no kind of view {kind:?}")))?;
                    p.views
                        .push(View::plain(id, name, kind, Source::Tip, false));
                    cur = Cur::View;
                }
                (Cur::View, "source") => {
                    let src = if rest == "tip" {
                        Source::Tip
                    } else if let Some(n) = rest.strip_prefix("node ") {
                        Source::Node(id_of(n)?)
                    } else if let Some(n) = rest.strip_prefix("sketch ") {
                        Source::Sketch(n.to_string())
                    } else {
                        return Err(bad(format!("line {at}: the source is tip, node or sketch")));
                    };
                    if let Some(v) = p.views.last_mut() {
                        v.source = src;
                    }
                }
                (Cur::View, "at") | (Cur::Balloon, "at") | (Cur::Balloon, "to") => {
                    let (a, b) = rest
                        .split_once(' ')
                        .ok_or_else(|| bad(format!("line {at}: two numbers")))?;
                    let pt = (int(a)?, int(b)?);
                    match (&cur, word) {
                        (Cur::View, _) => {
                            if let Some(v) = p.views.last_mut() {
                                v.place = Place::At(pt.0, pt.1);
                            }
                        }
                        (_, "at") => {
                            if let Some(bl) = p.balloons.last_mut() {
                                bl.at = pt;
                            }
                        }
                        _ => {
                            if let Some(bl) = p.balloons.last_mut() {
                                bl.to = pt;
                            }
                        }
                    }
                }
                (Cur::View, "scale") => {
                    let sc = parse_scale(rest)
                        .ok_or_else(|| bad(format!("line {at}: a scale is n:d")))?;
                    if let Some(v) = p.views.last_mut() {
                        v.scale = Some(sc);
                    }
                }
                (Cur::View, "hidden") => {
                    let on = match rest {
                        "0" => false,
                        "1" => true,
                        _ => return Err(bad(format!("line {at}: 0 or 1"))),
                    };
                    if let Some(v) = p.views.last_mut() {
                        v.hidden = on;
                    }
                }
                (Cur::View, "section") => {
                    let f: Vec<&str> = rest.split(' ').collect();
                    let [parent, label, pos] = f.as_slice() else {
                        return Err(bad(format!(
                            "line {at}: section needs a view, letters and a position"
                        )));
                    };
                    let cut = SectionCut {
                        parent: id_of(parent)?,
                        label: (*label).to_string(),
                        at_nm: int(pos)?,
                    };
                    if let Some(v) = p.views.last_mut() {
                        if v.section.replace(cut).is_some() {
                            return Err(bad(format!("line {at}: section twice")));
                        }
                    }
                }
                (Cur::View, "detail") => {
                    let f: Vec<&str> = rest.split(' ').collect();
                    let (parent, label, shape) = match f.as_slice() {
                        [a, b, c, tail @ ..] => (id_of(a)?, (*b).to_string(), (*c, tail)),
                        _ => {
                            return Err(bad(format!(
                                "line {at}: detail needs a view, a letter and a shape"
                            )))
                        }
                    };
                    let (centre, shape) = match shape {
                        ("circle", [u, v, r]) => (
                            (int(u)?, int(v)?),
                            DetailShape::Circle { radius_nm: int(r)? },
                        ),
                        ("rect", [u, v, w, h]) => (
                            (int(u)?, int(v)?),
                            DetailShape::Rect {
                                width_nm: int(w)?,
                                height_nm: int(h)?,
                            },
                        ),
                        _ => {
                            return Err(bad(format!(
                                "line {at}: a detail is a circle (centre, radius) or a rect (centre, size)"
                            )))
                        }
                    };
                    if let Some(v) = p.views.last_mut() {
                        let d = Detail {
                            parent,
                            label,
                            centre,
                            shape,
                        };
                        if v.detail.replace(d).is_some() {
                            return Err(bad(format!("line {at}: detail twice")));
                        }
                    }
                }
                (Cur::View, "break") => {
                    let f: Vec<&str> = rest.split(' ').collect();
                    let [axis, from, to] = f.as_slice() else {
                        return Err(bad(format!(
                            "line {at}: break needs u or v, a start and an end"
                        )));
                    };
                    let along_u = match *axis {
                        "u" => true,
                        "v" => false,
                        _ => return Err(bad(format!("line {at}: u or v"))),
                    };
                    let cut = BreakCut {
                        along_u,
                        from_nm: int(from)?,
                        to_nm: int(to)?,
                    };
                    if let Some(v) = p.views.last_mut() {
                        if v.brk.replace(cut).is_some() {
                            return Err(bad(format!("line {at}: break twice")));
                        }
                    }
                }
                (Cur::View, "hatch") => {
                    if let Some(v) = p.views.last_mut() {
                        if v.hatch.replace(rest.to_string()).is_some() {
                            return Err(bad(format!("line {at}: hatch twice")));
                        }
                    }
                }
                (_, "dim") => {
                    head_done = true;
                    let mut f = rest.splitn(3, ' ');
                    let (id, kind, name) = match (f.next(), f.next(), f.next()) {
                        (Some(a), Some(b), Some(c)) => (id_of(a)?, b, c),
                        _ => {
                            return Err(bad(format!(
                                "line {at}: dim needs an id, a kind and a name"
                            )))
                        }
                    };
                    let kind = DimKind::from_name(kind)
                        .ok_or_else(|| bad(format!("line {at}: no kind of dimension {kind:?}")))?;
                    p.dims.push(Dim {
                        id,
                        name: name.to_string(),
                        kind,
                        refs: Vec::new(),
                        view: id,
                        offset_um: None,
                        step_um: None,
                        tol: None,
                        layer: "DIMENSION".to_string(),
                        extra: Vec::new(),
                    });
                    cur = Cur::Dim;
                }
                (Cur::Dim, "ref") => {
                    let r = if let Some(t) = rest.strip_prefix("face ") {
                        let f: Vec<&str> = t.split(' ').collect();
                        match f.as_slice() {
                            [n, a, sd] => Ref::Face {
                                node: id_of(n)?,
                                axis: Axis::from_name(a)
                                    .ok_or_else(|| bad(format!("line {at}: x, y or z")))?,
                                side: match *sd {
                                    "min" => Side::Min,
                                    "max" => Side::Max,
                                    _ => return Err(bad(format!("line {at}: min or max"))),
                                },
                            },
                            _ => {
                                return Err(bad(format!(
                                    "line {at}: face needs a node, an axis and a side"
                                )))
                            }
                        }
                    } else if let Some(t) = rest.strip_prefix("element ") {
                        let (a, name) = t.split_once(' ').ok_or_else(|| {
                            bad(format!("line {at}: element needs a number and a sketch"))
                        })?;
                        Ref::Element {
                            sketch: name.to_string(),
                            element: u32::try_from(int(a)?)
                                .map_err(|_| bad(format!("line {at}: not a number of element")))?,
                        }
                    } else {
                        return Err(bad(format!(
                            "line {at}: a reference is a face or an element"
                        )));
                    };
                    if let Some(d) = p.dims.last_mut() {
                        d.refs.push(r);
                    }
                }
                (Cur::Dim, "view") => {
                    let v = id_of(rest)?;
                    if let Some(d) = p.dims.last_mut() {
                        d.view = v;
                    }
                }
                (Cur::Dim, "offset") => {
                    let v = int(rest)?;
                    if let Some(d) = p.dims.last_mut() {
                        d.offset_um = Some(v);
                    }
                }
                (Cur::Dim, "step") => {
                    let v = int(rest)?;
                    if let Some(d) = p.dims.last_mut() {
                        d.step_um = Some(v);
                    }
                }
                (Cur::Dim, "tol") => {
                    let f: Vec<&str> = rest.split(' ').collect();
                    let tol = match f.as_slice() {
                        ["class", c] => Tol::Class((*c).to_string()),
                        ["dev", u, l] => Tol::Dev {
                            upper_nm: int(u)?,
                            lower_nm: int(l)?,
                        },
                        _ => {
                            return Err(bad(format!(
                                "line {at}: a tolerance is `class H7/g6` or `dev UPPER LOWER`"
                            )))
                        }
                    };
                    if let Some(d) = p.dims.last_mut() {
                        if d.tol.replace(tol).is_some() {
                            return Err(bad(format!("line {at}: tol twice")));
                        }
                    }
                }
                (Cur::Dim, "layer") => {
                    if let Some(d) = p.dims.last_mut() {
                        d.layer = rest.to_string();
                    }
                }
                (_, "note") => {
                    head_done = true;
                    let (a, text) = rest
                        .split_once(' ')
                        .ok_or_else(|| bad(format!("line {at}: note needs an id and a text")))?;
                    let id = id_of(a)?;
                    p.notes.push(Note {
                        id,
                        text: text.to_string(),
                        anchor: NoteAnchor::View {
                            view: id,
                            u_nm: 0,
                            v_nm: 0,
                        },
                        at: (0, 0),
                        centre: false,
                        layer: "NOTE".to_string(),
                        extra: Vec::new(),
                    });
                    cur = Cur::Note;
                }
                (Cur::Note, "anchor") => {
                    let f: Vec<&str> = rest.split(' ').collect();
                    let anchor = match f.as_slice() {
                        ["view", v, u, w] => NoteAnchor::View {
                            view: id_of(v)?,
                            u_nm: int(u)?,
                            v_nm: int(w)?,
                        },
                        ["face", v, n, a, sd] => NoteAnchor::Face {
                            view: id_of(v)?,
                            node: id_of(n)?,
                            axis: Axis::from_name(a)
                                .ok_or_else(|| bad(format!("line {at}: x, y or z")))?,
                            side: match *sd {
                                "min" => Side::Min,
                                "max" => Side::Max,
                                _ => return Err(bad(format!("line {at}: min or max"))),
                            },
                        },
                        _ => {
                            return Err(bad(format!(
                            "line {at}: an anchor is `view VIEW U V` or `face VIEW NODE AXIS SIDE`"
                        )))
                        }
                    };
                    if let Some(n) = p.notes.last_mut() {
                        n.anchor = anchor;
                    }
                }
                (Cur::Note, "at") => {
                    let (a, b) = rest
                        .split_once(' ')
                        .ok_or_else(|| bad(format!("line {at}: two numbers")))?;
                    let pt = (int(a)?, int(b)?);
                    if let Some(n) = p.notes.last_mut() {
                        n.at = pt;
                    }
                }
                (Cur::Note, "centre") => {
                    if rest != "1" {
                        return Err(bad(format!("line {at}: centre is 1 when it is there")));
                    }
                    if let Some(n) = p.notes.last_mut() {
                        n.centre = true;
                    }
                }
                (Cur::Note, "layer") => {
                    if let Some(n) = p.notes.last_mut() {
                        n.layer = rest.to_string();
                    }
                }
                (_, "balloon") => {
                    head_done = true;
                    let (a, name) = rest
                        .split_once(' ')
                        .ok_or_else(|| bad(format!("line {at}: balloon needs an id and a name")))?;
                    let id = id_of(a)?;
                    p.balloons.push(Balloon {
                        id,
                        name: name.to_string(),
                        part: id,
                        at: (0, 0),
                        to: (0, 0),
                        extra: Vec::new(),
                    });
                    cur = Cur::Balloon;
                }
                _ => return Err(bad(format!("line {at}: not understood"))),
            }
        }
        if p.next_item == 0 {
            return Err(bad("the head has no `nextitem` line"));
        }
        p.validate(s)?;
        let written = p.to_bytes();
        if written != bytes {
            let first = written
                .split(|b| *b == b'\n')
                .zip(bytes.split(|b| *b == b'\n'))
                .position(|(x, y)| x != y)
                .map_or(0, |n| n + 1);
            return Err(bad(format!(
                "the drawing is not in the one canonical form (the first line that differs is {})",
                first + 1
            )));
        }
        Ok(p)
    }
}

enum Cur {
    Head,
    Part(Id),
    View,
    Dim,
    Balloon,
    Revision,
    Note,
}

impl Document {
    /// Writes the drawing into the domain `drawing` (optional: a program that does not know it
    /// keeps it as it is).
    pub fn set_plan(&mut self, plan: &Plan) -> Result<(), CadError> {
        self.set(DRAWING_DOMAIN, false, plan.to_bytes())
    }

    /// Reads the drawing, or `None` when the project has none.
    pub fn plan(&self, s: &SheetSettings) -> Result<Option<Plan>, CadError> {
        match self.get(DRAWING_DOMAIN) {
            None => Ok(None),
            Some(d) => Plan::from_bytes(&d.data, s).map(Some),
        }
    }
}
