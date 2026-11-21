// SPDX-License-Identifier: GPL-3.0-or-later
//! The model of a drawing sheet and its file: the domain `drawing` of the project file.
//!
//! The text of the domain is in one canonical form, with a version, ids that do not change and
//! the lines it does not know (those that start with `x-`) kept where they were:
//!
//! ```text
//! drawing 1
//! format <name>
//! orientation <landscape|portrait>
//! scale <auto|n:d>
//! projection <third|first>
//! nextitem <n>
//! field <key> <text>
//! layer <NAME> <visible|locked|color|width> <value>
//! x-<anything>                       (the lines of the head that a newer program added)
//! item <node id> <number>
//! part <node id>
//! material <text>
//! description <text>
//! view <id> <front|top|right|iso|sketch> <name>
//! source <tip | node <id> | sketch <name>>
//! at <x um> <y um>
//! scale <n:d>
//! hidden <0|1>
//! dim <id> <linear|radius|diameter|angle|cumulative> <name>
//! ref face <node id> <x|y|z> <min|max>
//! ref element <number> <sketch name>
//! view <view id>
//! offset <um>
//! layer <NAME>
//! balloon <id> <name>
//! part <node id>
//! at <x um> <y um>
//! to <x um> <y um>
//! ```
//!
//! A record is followed by its own lines; an `x-` line after a record belongs to it. Everything
//! is read under the limits of the settings and a text that is not in the one canonical form is
//! refused, so what is read is what is written.

use std::collections::{BTreeMap, BTreeSet};

use super::settings::{is_word, parse_scale, SheetSettings};
use crate::enclosure::Id;
use crate::project::Document;
use crate::CadError;

/// The domain of the project file that holds the drawing.
pub const DRAWING_DOMAIN: &str = "drawing";

/// The first line of the domain; the number is the version of its text.
pub const HEADER: &str = "drawing 1";

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
    /// The lines of a newer program that the record keeps.
    pub extra: Vec<String>,
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
    /// The distances from the first face to each of the others along an axis, stacked.
    Cumulative,
}

impl DimKind {
    /// Every kind.
    pub const ALL: [DimKind; 5] = [
        DimKind::Linear,
        DimKind::Radius,
        DimKind::Diameter,
        DimKind::Angle,
        DimKind::Cumulative,
    ];

    /// The word of the file.
    pub fn name(self) -> &'static str {
        match self {
            DimKind::Linear => "linear",
            DimKind::Radius => "radius",
            DimKind::Diameter => "diameter",
            DimKind::Angle => "angle",
            DimKind::Cumulative => "cumulative",
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
            DimKind::Cumulative => (3, 16),
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
    /// The distance of its first line from the view, when it is not the one of the style.
    pub offset_um: Option<i64>,
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

/// What the person says about a part of the bill.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PartInfo {
    /// The material.
    pub material: Option<String>,
    /// The description.
    pub description: Option<String>,
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
}

fn bad(why: impl Into<String>) -> CadError {
    CadError::Plan(why.into())
}

/// Whether a text typed by the person may go in the file: one line, no control character, no
/// space at either end, not empty and at most `max` characters.
pub fn valid_text(s: &str, max: usize) -> bool {
    !s.is_empty() && s.chars().count() <= max && s.trim() == s && !s.chars().any(char::is_control)
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
            fields: BTreeMap::new(),
            layers: BTreeMap::new(),
            head_extra: Vec::new(),
            numbers: BTreeMap::new(),
            parts: BTreeMap::new(),
            views: Vec::new(),
            dims: Vec::new(),
            balloons: Vec::new(),
        }
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
        self.layers
            .get(name)
            .and_then(|l| l.width_um)
            .or_else(|| s.layer(name).map(|l| l.width_um))
            .unwrap_or(s.line_group_um / 2)
    }

    fn id_in_use(&self, id: Id) -> bool {
        self.views.iter().any(|v| v.id == id)
            || self.dims.iter().any(|d| d.id == id)
            || self.balloons.iter().any(|b| b.id == id)
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
        if p.material.is_none() && p.description.is_none() && p.extra.is_empty() {
            self.parts.remove(&node);
        }
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
        for p in self.parts.values() {
            for t in [&p.material, &p.description].into_iter().flatten() {
                if !valid_text(t, s.max_text) {
                    return Err(bad("a text of the bill is not valid"));
                }
            }
        }
        let lines = self
            .head_extra
            .iter()
            .chain(self.views.iter().flat_map(|v| v.extra.iter()))
            .chain(self.dims.iter().flat_map(|v| v.extra.iter()))
            .chain(self.balloons.iter().flat_map(|v| v.extra.iter()))
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
            self.parts
                .values()
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

    // ----- the file -----

    /// The text of the drawing in its one canonical form: the same drawing gives the same bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut o = String::new();
        let mut line = |s: String| {
            o.push_str(&s);
            o.push('\n');
        };
        line(HEADER.to_string());
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
            for x in &p.extra {
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
        if it.next() != Some(HEADER) {
            return Err(bad(
                "the first line is not `drawing 1` (a newer version is not read)",
            ));
        }
        let lines: Vec<&str> = it.collect();
        let mut p = Plan::new(s);
        p.next_item = 0;
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
                    p.views.push(View {
                        id,
                        name: name.to_string(),
                        kind,
                        source: Source::Tip,
                        place: Place::Auto,
                        scale: None,
                        hidden: false,
                        extra: Vec::new(),
                    });
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
                (Cur::Dim, "layer") => {
                    if let Some(d) = p.dims.last_mut() {
                        d.layer = rest.to_string();
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
        if p.to_bytes() != bytes {
            return Err(bad("the drawing is not in the one canonical form"));
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
