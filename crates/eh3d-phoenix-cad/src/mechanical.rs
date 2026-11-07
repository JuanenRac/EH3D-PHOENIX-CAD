//! The mechanical domain of the project file: the recipe of the enclosure.
//!
//! The domain `mechanical` of the project shared with the board tool holds what is needed to build
//! the enclosure again, not the enclosure itself: the parameters with their variables and
//! expressions, the ordered steps (holes, edge treatments, posts, the fixing of the lid) with
//! their names and their suppressed flags, the tip, and the interface of the board (its outline,
//! its mounting holes and the sizes the posts must respect). The final state is derived by
//! [`Mechanical::build`]; nothing in the text is a result. The contract is written in
//! `docs/PROJECT_DOMAIN.md` and in the decision record of the domain.
//!
//! The text is made of lines, written in one canonical form (the same content always gives the same
//! bytes, and a text that is not canonical is refused), in this order: the header `mechanical 1`,
//! the section `[parameters]` (the text of the older domain `mechanical-parameters`), the section
//! `[steps]` (`tip` and one `step` line for each step) and, when the board tool has written it,
//! the section `[board]`. Names are written with every byte that is not a letter, a digit, `.`,
//! `_` or `-` as `%XX` (and an empty name as `~`). Lengths are whole nanometres.
//!
//! A project written before this domain existed has only `mechanical-parameters`: it is read as a
//! recipe without steps, and the first time the domain is written the older one is taken away.
//! A domain of a newer version is never read or written: the program keeps it byte for byte.
//!
//! Version 1 holds the steps above. Version 2 adds the steps `extrude`, `pocket` and
//! `remove-feature`, which refer to a sketch of the project by its name; a recipe that has none of
//! them is still written as version 1, so a project without extrusions or pockets opens in the
//! programs that only know version 1. The sketches are not in this domain: they have their own
//! domain, `sketch` (see [`crate::sketch::domain`]).

use std::collections::BTreeSet;
use std::str::FromStr;

use eh3d_container::ObjectId;

use crate::body::{Body, Built, Step};
use crate::config::MechanicalLimits;
use crate::enclosure::{Edging, EnclosureParams, Face, Hole, HoleStyle, Id, Operation, Treatment};
use crate::expr::{Constants, ExprLimits};
use crate::mount::{Base, BoardHole, BoardInterface, BoreKind, Lid, LidFixing, Lip, Post};
use crate::parametric::ParamModel;
use crate::profile::ProfileLimits;
use crate::project::{parameters_from_bytes, parameters_to_bytes, Document, PARAMETERS_DOMAIN};
use crate::sketch::{check_sketch_name, Sketch};
use crate::vars::{Param, Variables};
use crate::{CadError, Length};

/// The name of the domain.
pub const MECHANICAL_DOMAIN: &str = "mechanical";

/// The newest version of the text of the domain that this program writes and reads.
pub const DOMAIN_VERSION: u32 = 2;

/// The version of a recipe that has no extrusion and no pocket.
pub const FIRST_VERSION: u32 = 1;

/// The recipe of an enclosure as the project file keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mechanical {
    /// The parameters of the enclosure: values, variables or expressions.
    pub model: ParamModel,
    /// The variables the parameters may use.
    pub vars: Variables,
    /// The steps in order.
    pub steps: Vec<Step>,
    /// How many steps are active.
    pub tip: usize,
    /// What the board tool told: the outline, the mounting holes and the sizes of the board.
    pub board: Option<BoardInterface>,
    /// The sketches of the project, which the extrusions and pockets are made from. They are not
    /// part of the text of this domain: the domain `sketch` keeps them. They are here so that one
    /// history goes back and forward over the steps and the sketches together.
    pub sketches: Vec<Sketch>,
}

fn bad(why: &str) -> CadError {
    CadError::Format(why.to_string())
}

// ---------------------------------------------------------------------------------------------
// Words of the text
// ---------------------------------------------------------------------------------------------

fn safe_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-'
}

/// A name as one word: every byte that is not safe becomes `%XX`; an empty name is `~`.
fn escape(name: &str, limits: &MechanicalLimits) -> Result<String, CadError> {
    if name.len() > limits.max_name_bytes {
        return Err(bad("a name is longer than the limit"));
    }
    if name.is_empty() {
        return Ok("~".to_string());
    }
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        if safe_byte(b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    Ok(out)
}

/// The name that [`escape`] wrote. Only the canonical writing is accepted: a safe byte is never
/// escaped and the hexadecimal digits are capitals.
fn unescape(word: &str, limits: &MechanicalLimits) -> Result<String, CadError> {
    if word == "~" {
        return Ok(String::new());
    }
    let bytes = word.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            let hex = word
                .get(i + 1..i + 3)
                .ok_or_else(|| bad("a name has an escape that is cut short"))?;
            if !hex
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c))
            {
                return Err(bad("a name has an escape that is not written in capitals"));
            }
            let value =
                u8::from_str_radix(hex, 16).map_err(|_| bad("a name has a wrong escape"))?;
            if safe_byte(value) {
                return Err(bad("a name escapes a byte that is never escaped"));
            }
            out.push(value);
            i += 3;
        } else if safe_byte(b) {
            out.push(b);
            i += 1;
        } else {
            return Err(bad("a name has a byte that must be escaped"));
        }
        if out.len() > limits.max_name_bytes {
            return Err(bad("a name is longer than the limit"));
        }
    }
    if out.is_empty() {
        return Err(bad("a name is empty without being written as ~"));
    }
    String::from_utf8(out).map_err(|_| bad("a name is not text"))
}

/// A whole number of nanometres, written as the canonical decimal number.
fn length_of(word: &str, what: &str) -> Result<Length, CadError> {
    let nm = word
        .parse::<i64>()
        .ok()
        .filter(|v| v.to_string() == word)
        .ok_or_else(|| bad(&format!("the {what} is not a whole number of nanometres")))?;
    Length::new(nm).map_err(|_| bad(&format!("the {what} is outside the range of lengths")))
}

fn count_of(word: &str, what: &str) -> Result<usize, CadError> {
    word.parse::<usize>()
        .ok()
        .filter(|v| v.to_string() == word)
        .ok_or_else(|| bad(&format!("the {what} is not a whole number")))
}

fn flag_of(word: &str) -> Result<bool, CadError> {
    match word {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(bad("a flag is neither 0 nor 1")),
    }
}

fn id_of(word: &str) -> Result<Id, CadError> {
    ObjectId::from_str(word).map_err(|_| bad("an id is not valid"))
}

fn face_of(word: &str) -> Result<Face, CadError> {
    Face::from_name(word).ok_or_else(|| bad("a face is not known"))
}

// ---------------------------------------------------------------------------------------------
// Shapes, holes, posts and the lid
// ---------------------------------------------------------------------------------------------

fn style_word(style: &HoleStyle) -> String {
    match style {
        HoleStyle::Plain => "plain".to_string(),
        HoleStyle::Countersunk { head_diameter } => {
            format!("countersunk:{}", head_diameter.get())
        }
        HoleStyle::Counterbored {
            head_diameter,
            depth,
        } => format!("counterbored:{}:{}", head_diameter.get(), depth.get()),
        HoleStyle::Rectangular { height } => format!("rectangular:{}", height.get()),
        HoleStyle::Oval { length, vertical } => format!(
            "oval:{}:{}",
            length.get(),
            if *vertical { "v" } else { "u" }
        ),
    }
}

fn style_of(word: &str) -> Result<HoleStyle, CadError> {
    let parts: Vec<&str> = word.split(':').collect();
    match parts.as_slice() {
        ["plain"] => Ok(HoleStyle::Plain),
        ["countersunk", head] => Ok(HoleStyle::Countersunk {
            head_diameter: length_of(head, "head diameter")?,
        }),
        ["counterbored", head, depth] => Ok(HoleStyle::Counterbored {
            head_diameter: length_of(head, "head diameter")?,
            depth: length_of(depth, "counterbore depth")?,
        }),
        ["rectangular", height] => Ok(HoleStyle::Rectangular {
            height: length_of(height, "rectangle height")?,
        }),
        ["oval", length, direction] => {
            let vertical = match *direction {
                "v" => true,
                "u" => false,
                _ => return Err(bad("the direction of a slot is neither u nor v")),
            };
            Ok(HoleStyle::Oval {
                length: length_of(length, "slot length")?,
                vertical,
            })
        }
        _ => Err(bad("the shape of a hole is not understood")),
    }
}

fn treatment_word(t: Treatment) -> String {
    match t {
        Treatment::Sharp => "sharp".to_string(),
        Treatment::Round(s) => format!("round:{}", s.get()),
        Treatment::Chamfer(s) => format!("chamfer:{}", s.get()),
    }
}

fn treatment_of(word: &str) -> Result<Treatment, CadError> {
    let parts: Vec<&str> = word.split(':').collect();
    match parts.as_slice() {
        ["sharp"] => Ok(Treatment::Sharp),
        ["round", size] => Ok(Treatment::Round(length_of(size, "edge size")?)),
        ["chamfer", size] => Ok(Treatment::Chamfer(length_of(size, "edge size")?)),
        _ => Err(bad("the treatment of an edge is not understood")),
    }
}

fn hole_words(hole: &Hole, limits: &MechanicalLimits) -> Result<Vec<String>, CadError> {
    Ok(vec![
        escape(&hole.name, limits)?,
        hole.face.name().to_string(),
        hole.u.get().to_string(),
        hole.v.get().to_string(),
        hole.diameter.get().to_string(),
        style_word(&hole.style),
    ])
}

fn hole_of(words: &[&str], limits: &MechanicalLimits) -> Result<Hole, CadError> {
    match words {
        [name, face, u, v, diameter, style] => Ok(Hole {
            name: unescape(name, limits)?,
            face: face_of(face)?,
            u: length_of(u, "position of a hole")?,
            v: length_of(v, "position of a hole")?,
            diameter: length_of(diameter, "diameter of a hole")?,
            style: style_of(style)?,
        }),
        _ => Err(bad("a hole has not the six words it needs")),
    }
}

fn post_words(post: &Post, limits: &MechanicalLimits) -> Result<Vec<String>, CadError> {
    let base = match post.base {
        Some(base) => format!("foot:{}:{}", base.diameter.get(), base.height.get()),
        None => "none".to_string(),
    };
    Ok(vec![
        escape(&post.name, limits)?,
        post.board_hole
            .map_or_else(|| "-".to_string(), |id| id.to_string()),
        post.x.get().to_string(),
        post.y.get().to_string(),
        post.outer_diameter.get().to_string(),
        post.height.get().to_string(),
        post.kind.name().to_string(),
        post.bore_diameter.get().to_string(),
        post.bore_depth.get().to_string(),
        base,
    ])
}

fn post_of(words: &[&str], limits: &MechanicalLimits) -> Result<Post, CadError> {
    match words {
        [name, board_hole, x, y, outer, height, kind, bore, depth, base] => {
            let board_hole = if *board_hole == "-" {
                None
            } else {
                Some(id_of(board_hole)?)
            };
            let parts: Vec<&str> = base.split(':').collect();
            let base = match parts.as_slice() {
                ["none"] => None,
                ["foot", diameter, height] => Some(Base {
                    diameter: length_of(diameter, "diameter of a foot")?,
                    height: length_of(height, "height of a foot")?,
                }),
                _ => return Err(bad("the foot of a post is not understood")),
            };
            Ok(Post {
                name: unescape(name, limits)?,
                board_hole,
                x: length_of(x, "position of a post")?,
                y: length_of(y, "position of a post")?,
                outer_diameter: length_of(outer, "diameter of a post")?,
                height: length_of(height, "height of a post")?,
                kind: BoreKind::from_name(kind).ok_or_else(|| bad("a bore kind is not known"))?,
                bore_diameter: length_of(bore, "diameter of a bore")?,
                bore_depth: length_of(depth, "depth of a bore")?,
                base,
            })
        }
        _ => Err(bad("a post has not the ten words it needs")),
    }
}

fn lid_words(lid: &Lid, limits: &MechanicalLimits) -> Result<Vec<String>, CadError> {
    let fixing = match lid.fixing {
        LidFixing::Posts => "posts".to_string(),
        LidFixing::Corners { inset } => format!("corners:{}", inset.get()),
        LidFixing::Auto { inset } => format!("auto:{}", inset.get()),
    };
    let lip = match lid.lip {
        Some(lip) => format!("lip:{}:{}", lip.thickness.get(), lip.depth.get()),
        None => "nolip".to_string(),
    };
    Ok(vec![
        fixing,
        escape(&lid.screw, limits)?,
        lid.hole_diameter.get().to_string(),
        style_word(&lid.style),
        lid.clearance.get().to_string(),
        lip,
    ])
}

fn lid_of(words: &[&str], limits: &MechanicalLimits) -> Result<Lid, CadError> {
    match words {
        [fixing, screw, diameter, style, clearance, lip] => {
            let parts: Vec<&str> = fixing.split(':').collect();
            let fixing = match parts.as_slice() {
                ["posts"] => LidFixing::Posts,
                ["corners", inset] => LidFixing::Corners {
                    inset: length_of(inset, "corner inset")?,
                },
                ["auto", inset] => LidFixing::Auto {
                    inset: length_of(inset, "corner inset")?,
                },
                _ => return Err(bad("the fixing of the lid is not understood")),
            };
            let parts: Vec<&str> = lip.split(':').collect();
            let lip = match parts.as_slice() {
                ["nolip"] => None,
                ["lip", thickness, depth] => Some(Lip {
                    thickness: length_of(thickness, "thickness of the lip")?,
                    depth: length_of(depth, "depth of the lip")?,
                }),
                _ => return Err(bad("the lip of the lid is not understood")),
            };
            Ok(Lid {
                fixing,
                screw: unescape(screw, limits)?,
                hole_diameter: length_of(diameter, "diameter of a lid screw hole")?,
                style: style_of(style)?,
                clearance: length_of(clearance, "clearance of the lid")?,
                lip,
            })
        }
        _ => Err(bad("the fixing of the lid has not the six words it needs")),
    }
}

// ---------------------------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------------------------

fn operation_words(op: &Operation, limits: &MechanicalLimits) -> Result<Vec<String>, CadError> {
    let mut words: Vec<String> = Vec::new();
    match op {
        Operation::AddHole { id, hole } => {
            words.push("add-hole".to_string());
            words.push(id.to_string());
            words.extend(hole_words(hole, limits)?);
        }
        Operation::RemoveHole { id } => {
            words.push("remove-hole".to_string());
            words.push(id.to_string());
        }
        Operation::ReplaceHole { id, hole } => {
            words.push("replace-hole".to_string());
            words.push(id.to_string());
            words.extend(hole_words(hole, limits)?);
        }
        Operation::SetParams(params) => {
            words.push("set-params".to_string());
            for param in Param::ALL {
                words.push(format!("{}={}", param.name(), param.get(params).get()));
            }
        }
        Operation::SetEdging(edging) => {
            words.push("set-edging".to_string());
            words.push(treatment_word(edging.vertical));
            words.push(treatment_word(edging.top));
            words.push(treatment_word(edging.bottom));
        }
        Operation::AddPost { id, post } => {
            words.push("add-post".to_string());
            words.push(id.to_string());
            words.extend(post_words(post, limits)?);
        }
        Operation::RemovePost { id } => {
            words.push("remove-post".to_string());
            words.push(id.to_string());
        }
        Operation::ReplacePost { id, post } => {
            words.push("replace-post".to_string());
            words.push(id.to_string());
            words.extend(post_words(post, limits)?);
        }
        Operation::SetLid(None) => {
            words.push("set-lid".to_string());
            words.push("none".to_string());
        }
        Operation::SetLid(Some(lid)) => {
            words.push("set-lid".to_string());
            words.extend(lid_words(lid, limits)?);
        }
        Operation::Extrude { id, sketch, height } => {
            words.push("extrude".to_string());
            words.push(id.to_string());
            words.push(sketch.clone());
            words.push(height.get().to_string());
        }
        Operation::Pocket { id, sketch, depth } => {
            words.push("pocket".to_string());
            words.push(id.to_string());
            words.push(sketch.clone());
            words.push(depth.map_or("through".to_string(), |d| d.get().to_string()));
        }
        Operation::RemoveFeature { id } => {
            words.push("remove-feature".to_string());
            words.push(id.to_string());
        }
    }
    Ok(words)
}

/// Whether a step needs version 2 of the domain.
fn needs_version_two(op: &Operation) -> bool {
    matches!(
        op,
        Operation::Extrude { .. } | Operation::Pocket { .. } | Operation::RemoveFeature { .. }
    )
}

/// The name of a sketch as a word of a step: the same letters, digits, `_` and `-` as a sketch
/// name, so nothing has to be escaped.
fn sketch_word(word: &str) -> Result<String, CadError> {
    check_sketch_name(word).map_err(|_| bad("the name of a sketch is not valid"))?;
    Ok(word.to_string())
}

fn params_of(words: &[&str]) -> Result<EnclosureParams, CadError> {
    if words.len() != Param::ALL.len() {
        return Err(bad(
            "a step of parameters has not the eight values it needs",
        ));
    }
    let zero = Length::ZERO;
    let mut params = EnclosureParams {
        inner_length: zero,
        inner_width: zero,
        inner_height: zero,
        wall: zero,
        floor: zero,
        lid: zero,
        edge_margin: zero,
        min_web: zero,
    };
    for (word, param) in words.iter().zip(Param::ALL) {
        let (name, value) = word
            .split_once('=')
            .ok_or_else(|| bad("a value of a step of parameters is not name=value"))?;
        if name != param.name() {
            return Err(bad("the values of a step of parameters are not in order"));
        }
        param.set(&mut params, length_of(value, "parameter of a step")?);
    }
    Ok(params)
}

fn operation_of(words: &[&str], limits: &MechanicalLimits) -> Result<Operation, CadError> {
    match words {
        ["add-hole", id, rest @ ..] => Ok(Operation::AddHole {
            id: id_of(id)?,
            hole: hole_of(rest, limits)?,
        }),
        ["remove-hole", id] => Ok(Operation::RemoveHole { id: id_of(id)? }),
        ["replace-hole", id, rest @ ..] => Ok(Operation::ReplaceHole {
            id: id_of(id)?,
            hole: hole_of(rest, limits)?,
        }),
        ["set-params", rest @ ..] => Ok(Operation::SetParams(params_of(rest)?)),
        ["set-edging", vertical, top, bottom] => Ok(Operation::SetEdging(Edging {
            vertical: treatment_of(vertical)?,
            top: treatment_of(top)?,
            bottom: treatment_of(bottom)?,
        })),
        ["add-post", id, rest @ ..] => Ok(Operation::AddPost {
            id: id_of(id)?,
            post: post_of(rest, limits)?,
        }),
        ["remove-post", id] => Ok(Operation::RemovePost { id: id_of(id)? }),
        ["replace-post", id, rest @ ..] => Ok(Operation::ReplacePost {
            id: id_of(id)?,
            post: post_of(rest, limits)?,
        }),
        ["set-lid", "none"] => Ok(Operation::SetLid(None)),
        ["set-lid", rest @ ..] => Ok(Operation::SetLid(Some(lid_of(rest, limits)?))),
        ["extrude", id, sketch, height] => Ok(Operation::Extrude {
            id: id_of(id)?,
            sketch: sketch_word(sketch)?,
            height: length_of(height, "height of an extrusion")?,
        }),
        ["pocket", id, sketch, depth] => Ok(Operation::Pocket {
            id: id_of(id)?,
            sketch: sketch_word(sketch)?,
            depth: if *depth == "through" {
                None
            } else {
                Some(length_of(depth, "depth of a pocket")?)
            },
        }),
        ["remove-feature", id] => Ok(Operation::RemoveFeature { id: id_of(id)? }),
        _ => Err(bad("a step is not understood")),
    }
}

fn step_line(step: &Step, limits: &MechanicalLimits) -> Result<String, CadError> {
    let flag = if step.suppressed { "1" } else { "0" };
    let mut words = vec![
        "step".to_string(),
        flag.to_string(),
        escape(&step.name, limits)?,
    ];
    words.extend(operation_words(&step.op, limits)?);
    Ok(words.join(" "))
}

fn step_of(line: &str, limits: &MechanicalLimits) -> Result<Step, CadError> {
    let words: Vec<&str> = line.split(' ').collect();
    match words.as_slice() {
        ["step", suppressed, name, op @ ..] if !op.is_empty() => Ok(Step {
            name: unescape(name, limits)?,
            op: operation_of(op, limits)?,
            suppressed: flag_of(suppressed)?,
        }),
        _ => Err(bad("a line of the steps is not a step")),
    }
}

// ---------------------------------------------------------------------------------------------
// The board
// ---------------------------------------------------------------------------------------------

fn board_lines(board: &BoardInterface, limits: &MechanicalLimits) -> Result<String, CadError> {
    if board.outline.len() < 3 || board.outline.len() > limits.max_outline_points {
        return Err(bad(
            "the outline of the board has too few or too many points",
        ));
    }
    if board.holes.len() > limits.max_board_holes {
        return Err(bad("the board has more mounting holes than the limit"));
    }
    if board.thickness.get() <= 0 {
        return Err(bad("the thickness of the board is not positive"));
    }
    let mut ids = BTreeSet::new();
    for hole in &board.holes {
        if hole.diameter.get() <= 0 {
            return Err(bad("a mounting hole of the board is not positive"));
        }
        if !ids.insert(hole.id) {
            return Err(bad("two mounting holes of the board have the same id"));
        }
    }
    let mut out = format!("thickness {}\n", board.thickness.get());
    if let Some(tallest) = board.tallest_component {
        out.push_str(&format!("tallest {}\n", tallest.get()));
    }
    if let Some((x, y)) = board.placement {
        out.push_str(&format!("place {} {}\n", x.get(), y.get()));
    }
    for (x, y) in &board.outline {
        out.push_str(&format!("point {} {}\n", x.get(), y.get()));
    }
    for hole in &board.holes {
        out.push_str(&format!(
            "hole {} {} {} {} {}\n",
            hole.id,
            escape(&hole.name, limits)?,
            hole.x.get(),
            hole.y.get(),
            hole.diameter.get()
        ));
    }
    Ok(out)
}

fn board_of(lines: &[&str], limits: &MechanicalLimits) -> Result<BoardInterface, CadError> {
    // the lines come in one order: thickness, tallest, place, the points, the holes
    let mut rank = 0u8;
    let mut thickness: Option<Length> = None;
    let mut tallest = None;
    let mut placement = None;
    let mut outline: Vec<(Length, Length)> = Vec::new();
    let mut holes: Vec<BoardHole> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let words: Vec<&str> = line.split(' ').collect();
        let line_rank: u8 = match words.as_slice() {
            ["thickness", value] => {
                thickness = Some(length_of(value, "thickness of the board")?);
                0
            }
            ["tallest", value] => {
                tallest = Some(length_of(value, "height of the tallest part")?);
                1
            }
            ["place", x, y] => {
                placement = Some((
                    length_of(x, "placement of the board")?,
                    length_of(y, "placement of the board")?,
                ));
                2
            }
            ["point", x, y] => {
                outline.push((
                    length_of(x, "point of the outline")?,
                    length_of(y, "point of the outline")?,
                ));
                3
            }
            ["hole", id, name, x, y, diameter] => {
                holes.push(BoardHole {
                    id: id_of(id)?,
                    name: unescape(name, limits)?,
                    x: length_of(x, "position of a mounting hole")?,
                    y: length_of(y, "position of a mounting hole")?,
                    diameter: length_of(diameter, "diameter of a mounting hole")?,
                });
                4
            }
            _ => return Err(bad("a line of the board is not understood")),
        };
        // the thickness is the first line; the tallest part and the placement come once, in that
        // order; the points and then the holes come any number of times
        let in_order = match line_rank {
            0 => index == 0,
            1 | 2 => index > 0 && rank < line_rank,
            _ => index > 0 && rank <= line_rank,
        };
        if !in_order {
            return Err(bad("the lines of the board are not in order"));
        }
        rank = line_rank;
        if outline.len() > limits.max_outline_points {
            return Err(bad(
                "the outline of the board has more points than the limit",
            ));
        }
        if holes.len() > limits.max_board_holes {
            return Err(bad("the board has more mounting holes than the limit"));
        }
    }
    let thickness = thickness.ok_or_else(|| bad("the board has no thickness"))?;
    if thickness.get() <= 0 {
        return Err(bad("the thickness of the board is not positive"));
    }
    if outline.len() < 3 {
        return Err(bad("the outline of the board has fewer than three points"));
    }
    let mut ids = BTreeSet::new();
    for hole in &holes {
        if hole.diameter.get() <= 0 {
            return Err(bad("a mounting hole of the board is not positive"));
        }
        if !ids.insert(hole.id) {
            return Err(bad("two mounting holes of the board have the same id"));
        }
    }
    Ok(BoardInterface {
        outline,
        holes,
        thickness,
        tallest_component: tallest,
        placement,
    })
}

// ---------------------------------------------------------------------------------------------
// The whole domain
// ---------------------------------------------------------------------------------------------

impl Mechanical {
    /// A recipe with the given parameters and variables, no steps and no board.
    pub fn new(model: ParamModel, vars: Variables) -> Mechanical {
        Mechanical {
            model,
            vars,
            steps: Vec::new(),
            tip: 0,
            board: None,
            sketches: Vec::new(),
        }
    }

    /// The version of the text this recipe is written in: 2 when a step is an extrusion, a pocket
    /// or the removal of one, else 1.
    pub fn version(&self) -> u32 {
        if self.steps.iter().any(|s| needs_version_two(&s.op)) {
            DOMAIN_VERSION
        } else {
            FIRST_VERSION
        }
    }

    /// Puts a sketch in the project, replacing the one of the same name.
    pub fn put_sketch(&mut self, sketch: Sketch) {
        match self.sketches.iter_mut().find(|s| s.name() == sketch.name()) {
            Some(slot) => *slot = sketch,
            None => self.sketches.push(sketch),
        }
    }

    /// The sketch of a name.
    pub fn sketch(&self, name: &str) -> Option<&Sketch> {
        self.sketches.iter().find(|s| s.name() == name)
    }

    /// The body of the recipe: the parameters calculated, then the steps and the tip as stored.
    pub fn body(&self, constants: &Constants, limits: &ExprLimits) -> Result<Body, CadError> {
        let base = self.model.resolve(&self.vars, constants, limits)?;
        Body::from_parts(base, self.steps.clone(), self.tip)
    }

    /// Builds the enclosure from the recipe: the final state is derived, never stored.
    pub fn build(&self, constants: &Constants, limits: &ExprLimits) -> Result<Built, CadError> {
        self.build_with(constants, limits, None)
    }

    /// Like [`Mechanical::build`], with the limits of the profiles of the sketches (the shipped
    /// ones when there are none).
    pub fn build_with(
        &self,
        constants: &Constants,
        limits: &ExprLimits,
        profiles: Option<ProfileLimits>,
    ) -> Result<Built, CadError> {
        self.body(constants, limits)?
            .build_with(&self.sketches, profiles)
    }

    /// Takes the steps and the tip of a body that was edited (its base comes from the parameters).
    pub fn take_recipe(&mut self, body: &Body) {
        self.steps = body.steps().to_vec();
        self.tip = body.tip();
    }

    /// The text of the domain, the same for the same recipe. A recipe over a limit is refused.
    pub fn to_bytes(&self, limits: &MechanicalLimits) -> Result<Vec<u8>, CadError> {
        if self.steps.len() > limits.max_steps {
            return Err(bad("there are more steps than the limit"));
        }
        if self.tip > self.steps.len() {
            return Err(bad("the tip is beyond the last step"));
        }
        let parameters = String::from_utf8(parameters_to_bytes(&self.model, &self.vars))
            .map_err(|_| bad("the parameters are not text"))?;
        let mut out = format!("mechanical {}\n[parameters]\n{parameters}", self.version());
        out.push_str(&format!("[steps]\ntip {}\n", self.tip));
        for step in &self.steps {
            out.push_str(&step_line(step, limits)?);
            out.push('\n');
        }
        if let Some(board) = &self.board {
            out.push_str("[board]\n");
            out.push_str(&board_lines(board, limits)?);
        }
        if out.len() > limits.max_bytes {
            return Err(bad("the domain is larger than the limit"));
        }
        Ok(out.into_bytes())
    }

    /// Reads the text written by [`Mechanical::to_bytes`]. What is not written in that one form is
    /// refused with `PHOENIX-CAD-018`, and so is a text over a limit or of a newer version.
    pub fn from_bytes(
        bytes: &[u8],
        expr: &ExprLimits,
        limits: &MechanicalLimits,
    ) -> Result<Mechanical, CadError> {
        if bytes.len() > limits.max_bytes {
            return Err(bad("the domain is larger than the limit"));
        }
        // the header is read first: a newer version is told apart from a text that is broken
        let version = header_version(first_line(bytes)?)?;
        if version > DOMAIN_VERSION {
            return Err(bad("the domain was written by a newer version"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| bad("the domain is not text"))?;
        let body = text
            .strip_suffix('\n')
            .ok_or_else(|| bad("the domain does not end with a line end"))?;
        let mut lines = body.split('\n');
        lines.next();
        let mut parameters: Vec<&str> = Vec::new();
        let mut steps: Vec<&str> = Vec::new();
        let mut board: Vec<&str> = Vec::new();
        // 0 before any section, then 1 parameters, 2 steps, 3 board
        let mut section = 0u8;
        for line in lines {
            match line {
                "[parameters]" if section == 0 => section = 1,
                "[steps]" if section == 1 => section = 2,
                "[board]" if section == 2 => section = 3,
                _ => match section {
                    1 => parameters.push(line),
                    2 => steps.push(line),
                    3 => board.push(line),
                    _ => return Err(bad("the sections of the domain are not in order")),
                },
            }
        }
        if section < 2 {
            return Err(bad("the domain lacks its parameters or its steps"));
        }
        let mut parameters_text = parameters.join("\n");
        parameters_text.push('\n');
        let (model, vars) = parameters_from_bytes(parameters_text.as_bytes(), expr)?;
        let (first, step_lines) = steps
            .split_first()
            .ok_or_else(|| bad("the steps lack their tip"))?;
        let tip = match first.split_once(' ') {
            Some(("tip", value)) => count_of(value, "tip")?,
            _ => return Err(bad("the steps do not start with their tip")),
        };
        if step_lines.len() > limits.max_steps {
            return Err(bad("there are more steps than the limit"));
        }
        if tip > step_lines.len() {
            return Err(bad("the tip is beyond the last step"));
        }
        let mut recipe = Vec::with_capacity(step_lines.len());
        for line in step_lines {
            recipe.push(step_of(line, limits)?);
        }
        let board = if section == 3 {
            Some(board_of(&board, limits)?)
        } else {
            None
        };
        let uses_two = recipe.iter().any(|s| needs_version_two(&s.op));
        if uses_two && version < DOMAIN_VERSION {
            return Err(bad("a step of version 2 is in a domain of version 1"));
        }
        if !uses_two && version > FIRST_VERSION {
            return Err(bad(
                "the domain says version 2 and has no step that needs it",
            ));
        }
        Ok(Mechanical {
            model,
            vars,
            steps: recipe,
            tip,
            board,
            sketches: Vec::new(),
        })
    }
}

/// The first line of the domain, which must be text even when the rest is not.
fn first_line(bytes: &[u8]) -> Result<&str, CadError> {
    let first = bytes.split(|b| *b == b'\n').next().unwrap_or(&[]);
    std::str::from_utf8(first).map_err(|_| bad("the header of the domain is not text"))
}

/// The version in the first line of the domain: `mechanical <version>`, with a version that is not
/// zero.
fn header_version(line: &str) -> Result<u32, CadError> {
    match line.split_once(' ') {
        Some(("mechanical", version)) => {
            let v = version
                .parse::<u32>()
                .ok()
                .filter(|v| v.to_string() == version)
                .ok_or_else(|| bad("the version of the domain is not a number"))?;
            if v == 0 {
                Err(bad("the version of the domain is zero"))
            } else {
                Ok(v)
            }
        }
        _ => Err(bad("the domain does not start with its header")),
    }
}

impl Document {
    /// The version of the domain `mechanical` as its first line says, or `None` when the file has
    /// no such domain. A program whose [`DOMAIN_VERSION`] is lower must not edit the project: the
    /// domain is kept as it is.
    pub fn mechanical_version(&self) -> Result<Option<u32>, CadError> {
        match self.get(MECHANICAL_DOMAIN) {
            Some(domain) => header_version(first_line(&domain.data)?).map(Some),
            None => Ok(None),
        }
    }

    /// Reads the recipe: from the domain `mechanical` when the file has it, or else from the older
    /// domain of the parameters (a recipe without steps and without a board). `None` when the file
    /// has neither.
    pub fn mechanical(
        &self,
        expr: &ExprLimits,
        limits: &MechanicalLimits,
    ) -> Result<Option<Mechanical>, CadError> {
        if let Some(domain) = self.get(MECHANICAL_DOMAIN) {
            return Mechanical::from_bytes(&domain.data, expr, limits).map(Some);
        }
        Ok(self
            .parameters(expr)?
            .map(|(model, vars)| Mechanical::new(model, vars)))
    }

    /// Writes the recipe in the domain `mechanical`, which is critical, and takes away the older
    /// domain of the parameters, which it replaces. A domain of a newer version is not written
    /// over: it is kept as it is and the call is refused.
    pub fn set_mechanical(
        &mut self,
        recipe: &Mechanical,
        limits: &MechanicalLimits,
    ) -> Result<(), CadError> {
        if let Some(version) = self.mechanical_version()? {
            if version > DOMAIN_VERSION {
                return Err(bad(
                    "the domain was written by a newer version and is kept as it is",
                ));
            }
        }
        let bytes = recipe.to_bytes(limits)?;
        self.set(MECHANICAL_DOMAIN, true, bytes)?;
        if self.get(PARAMETERS_DOMAIN).is_some() {
            self.remove(PARAMETERS_DOMAIN)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::{Session, StepState};
    use crate::config::{Head, Settings};
    use crate::mount::{lid_for, posts_for_board, PostRequest};

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn nm(v: i64) -> Length {
        Length::new(v).unwrap()
    }

    fn id(n: u64) -> Id {
        ObjectId::from_parts(1_700_000_000_000, n as u16, n).unwrap()
    }

    fn settings() -> Settings {
        Settings::defaults()
    }

    fn base() -> EnclosureParams {
        EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        }
    }

    fn step(name: &str, suppressed: bool, op: Operation) -> Step {
        Step {
            name: name.to_string(),
            op,
            suppressed,
        }
    }

    fn front_hole(name: &str, u: i64) -> Hole {
        Hole {
            name: name.to_string(),
            face: Face::Front,
            u: mm(u),
            v: mm(17),
            diameter: mm(6),
            style: HoleStyle::Plain,
        }
    }

    /// The smallest recipe: the base parameters and one hole.
    fn minimal() -> Mechanical {
        let mut m = Mechanical::new(ParamModel::new(base()).unwrap(), Variables::new());
        m.steps.push(step(
            "usb port",
            false,
            Operation::AddHole {
                id: id(1),
                hole: front_hole("usb", 30),
            },
        ));
        m.tip = 1;
        m
    }

    /// The text of [`minimal`], written by hand: 100 by 60 by 30 mm inside, walls of 2 mm, a lid
    /// of 3 mm, 3 mm from the edges and 2 mm between holes, and a hole 30 mm along and 17 mm up
    /// the front, 6 mm across. The id is the UUID of the time 1 700 000 000 000 ms, counter 1 and
    /// random part 1.
    const MINIMAL_TEXT: &str = "mechanical 1\n\
[parameters]\n\
parameters 1\n\
base inner_length 100000000\n\
base inner_width 60000000\n\
base inner_height 30000000\n\
base wall 2000000\n\
base floor 2000000\n\
base lid 3000000\n\
base edge_margin 3000000\n\
base min_web 2000000\n\
[steps]\n\
tip 1\n\
step 0 usb%20port add-hole 018bcfe5-6800-7001-8000-000000000001 usb front 30000000 17000000 6000000 plain\n";

    fn board() -> BoardInterface {
        let hole = |n: u64, x: i64, y: i64| BoardHole {
            id: id(n),
            name: format!("H{n}"),
            x: mm(x),
            y: mm(y),
            diameter: nm(3_200_000),
        };
        BoardInterface {
            outline: vec![
                (mm(0), mm(0)),
                (mm(60), mm(0)),
                (mm(60), mm(40)),
                (mm(0), mm(40)),
            ],
            holes: vec![
                hole(1, 5, 5),
                hole(2, 55, 5),
                hole(3, 5, 35),
                hole(4, 55, 35),
            ],
            thickness: nm(1_600_000),
            tallest_component: Some(mm(5)),
            placement: None,
        }
    }

    /// A whole recipe: parameters that are expressions over a variable, a hole, rounded corners,
    /// four posts made from the board, and a lid screwed on them with a lip.
    fn recipe() -> Mechanical {
        let s = settings();
        let mut model = ParamModel::new(base()).unwrap();
        model.set_expr(Param::Wall, "5 * nozzle", &s.expr).unwrap();
        model.set_expr(Param::Floor, "wall", &s.expr).unwrap();
        model.set_expr(Param::Lid, "wall + 1 mm", &s.expr).unwrap();
        model
            .set_expr(Param::InnerLength, "length + 2 * wall", &s.expr)
            .unwrap();
        model.set_expr(Param::InnerWidth, "width", &s.expr).unwrap();
        let mut vars = Variables::new();
        vars.set("length", mm(100)).unwrap();
        vars.set("width", mm(60)).unwrap();
        let mut m = Mechanical::new(model, vars);
        m.board = Some(board());
        m.steps.push(step(
            "usb port",
            false,
            Operation::AddHole {
                id: id(10),
                hole: front_hole("usb", 30),
            },
        ));
        m.steps.push(step(
            "round corners",
            false,
            Operation::SetEdging(Edging {
                vertical: Treatment::Round(mm(4)),
                ..Edging::default()
            }),
        ));
        m.tip = m.steps.len();
        let built = m.build(&s.constants, &s.expr).unwrap();
        let req = PostRequest {
            screw: "M3",
            kind: BoreKind::Insert,
            height: mm(6),
            base: false,
        };
        let ops = posts_for_board(&s, &built.enclosure, m.board.as_ref().unwrap(), &req).unwrap();
        for op in ops {
            let name = match &op {
                Operation::AddPost { post, .. } => post.name.clone(),
                _ => String::new(),
            };
            m.steps.push(step(&name, false, op));
        }
        let fixing = LidFixing::Auto {
            inset: s.lid.corner_inset,
        };
        let lid = lid_for(&s, "M3", Head::Countersunk, fixing, true).unwrap();
        m.steps
            .push(step("lid", false, Operation::SetLid(Some(lid))));
        m.tip = m.steps.len();
        m
    }

    struct Rng(u64);

    impl Rng {
        fn draw(&mut self, below: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % below
        }

        fn length(&mut self) -> Length {
            nm(self.draw(2_000_000_000) as i64 - 500_000_000)
        }

        fn name(&mut self) -> String {
            const NAMES: [&str; 9] = [
                "",
                "~",
                "usb port",
                "50% off",
                "naïve",
                "tab\there",
                "two\nlines",
                "plain",
                "x.y_z-1",
            ];
            NAMES[self.draw(9) as usize].to_string()
        }

        fn style(&mut self) -> HoleStyle {
            match self.draw(5) {
                0 => HoleStyle::Plain,
                1 => HoleStyle::Countersunk {
                    head_diameter: self.length(),
                },
                2 => HoleStyle::Counterbored {
                    head_diameter: self.length(),
                    depth: self.length(),
                },
                3 => HoleStyle::Rectangular {
                    height: self.length(),
                },
                _ => HoleStyle::Oval {
                    length: self.length(),
                    vertical: self.draw(2) == 0,
                },
            }
        }

        fn treatment(&mut self) -> Treatment {
            match self.draw(3) {
                0 => Treatment::Sharp,
                1 => Treatment::Round(self.length()),
                _ => Treatment::Chamfer(self.length()),
            }
        }

        fn id(&mut self) -> Id {
            id(self.draw(500) + 1)
        }

        fn hole(&mut self) -> Hole {
            Hole {
                name: self.name(),
                face: Face::ALL[self.draw(6) as usize],
                u: self.length(),
                v: self.length(),
                diameter: self.length(),
                style: self.style(),
            }
        }

        fn post(&mut self) -> Post {
            Post {
                name: self.name(),
                board_hole: if self.draw(2) == 0 {
                    None
                } else {
                    Some(self.id())
                },
                x: self.length(),
                y: self.length(),
                outer_diameter: self.length(),
                height: self.length(),
                kind: if self.draw(2) == 0 {
                    BoreKind::Insert
                } else {
                    BoreKind::SelfTapping
                },
                bore_diameter: self.length(),
                bore_depth: self.length(),
                base: if self.draw(2) == 0 {
                    None
                } else {
                    Some(Base {
                        diameter: self.length(),
                        height: self.length(),
                    })
                },
            }
        }

        fn lid(&mut self) -> Lid {
            Lid {
                fixing: match self.draw(3) {
                    0 => LidFixing::Posts,
                    1 => LidFixing::Corners {
                        inset: self.length(),
                    },
                    _ => LidFixing::Auto {
                        inset: self.length(),
                    },
                },
                screw: self.name(),
                hole_diameter: self.length(),
                style: self.style(),
                clearance: self.length(),
                lip: if self.draw(2) == 0 {
                    None
                } else {
                    Some(Lip {
                        thickness: self.length(),
                        depth: self.length(),
                    })
                },
            }
        }

        fn params(&mut self) -> EnclosureParams {
            EnclosureParams {
                inner_length: self.length(),
                inner_width: self.length(),
                inner_height: self.length(),
                wall: self.length(),
                floor: self.length(),
                lid: self.length(),
                edge_margin: self.length(),
                min_web: self.length(),
            }
        }

        fn operation(&mut self) -> Operation {
            match self.draw(11) {
                0 => Operation::AddHole {
                    id: self.id(),
                    hole: self.hole(),
                },
                1 => Operation::RemoveHole { id: self.id() },
                2 => Operation::ReplaceHole {
                    id: self.id(),
                    hole: self.hole(),
                },
                3 => Operation::SetParams(self.params()),
                4 => Operation::SetEdging(Edging {
                    vertical: self.treatment(),
                    top: self.treatment(),
                    bottom: self.treatment(),
                }),
                5 => Operation::AddPost {
                    id: self.id(),
                    post: self.post(),
                },
                6 => Operation::RemovePost { id: self.id() },
                7 => Operation::ReplacePost {
                    id: self.id(),
                    post: self.post(),
                },
                8 => Operation::SetLid(None),
                _ => Operation::SetLid(Some(self.lid())),
            }
        }

        fn step(&mut self) -> Step {
            Step {
                name: self.name(),
                suppressed: self.draw(4) == 0,
                op: self.operation(),
            }
        }
    }

    #[test]
    fn the_limits_of_the_domain_are_checked_against_their_caps() {
        let ok = MechanicalLimits::new(1 << 28, 1_000_000, 4_096, 1_000_000, 100_000).unwrap();
        assert_eq!(ok.max_bytes, 268_435_456);
        assert_eq!(ok.max_steps, 1_000_000);
        assert_eq!(ok.max_name_bytes, 4_096);
        assert_eq!(ok.max_outline_points, 1_000_000);
        assert_eq!(ok.max_board_holes, 100_000);
        assert!(MechanicalLimits::new(1, 1, 1, 1, 1).is_ok());
        let s = settings().mechanical;
        assert_eq!(
            (s.max_bytes, s.max_steps, s.max_name_bytes),
            (16_777_216, 10_000, 256)
        );
        assert_eq!((s.max_outline_points, s.max_board_holes), (10_000, 1_000));
        for bad in [
            (0, 1, 1, 1, 1),
            (1, 0, 1, 1, 1),
            (1, 1, 0, 1, 1),
            (1, 1, 1, 0, 1),
            (1, 1, 1, 1, 0),
            ((1 << 28) + 1, 1, 1, 1, 1),
            (1, 1_000_001, 1, 1, 1),
            (1, 1, 4_097, 1, 1),
            (1, 1, 1, 1_000_001, 1),
            (1, 1, 1, 1, 100_001),
        ] {
            let err = MechanicalLimits::new(bad.0, bad.1, bad.2, bad.3, bad.4).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-011", "{bad:?}");
        }
    }

    #[test]
    fn a_name_is_one_word_and_only_the_one_writing_is_read() {
        let l = settings().mechanical;
        for (name, word) in [
            ("usb port", "usb%20port"),
            ("", "~"),
            ("~", "%7E"),
            ("%", "%25"),
            ("é", "%C3%A9"),
            ("a-b_c.d9", "a-b_c.d9"),
            ("two\nlines", "two%0Alines"),
        ] {
            assert_eq!(escape(name, &l).unwrap(), word, "{name:?}");
            assert_eq!(unescape(word, &l).unwrap(), name, "{word}");
        }
        for word in [
            "%c3%a9", "%41", "%4", "%", "%ZZ", "%+A", "a b", "é", "", "%FF", "a%2", "%2D",
        ] {
            assert_eq!(
                unescape(word, &l).unwrap_err().code(),
                "PHOENIX-CAD-018",
                "{word:?}"
            );
        }
        // the limit counts the bytes of the name, not of the word
        let short = MechanicalLimits {
            max_name_bytes: 3,
            ..l
        };
        assert!(escape("abc", &short).is_ok());
        assert_eq!(
            escape("abcd", &short).unwrap_err().code(),
            "PHOENIX-CAD-018"
        );
        assert!(unescape("abc", &short).is_ok());
        assert!(unescape("%C3%A9", &short).is_ok());
        assert!(unescape("abcd", &short).is_err());
        assert!(unescape("%C3%A9%C3%A9", &short).is_err());
    }

    #[test]
    fn a_number_is_written_in_one_form_only() {
        assert_eq!(length_of("0", "x").unwrap(), Length::ZERO);
        assert_eq!(length_of("-5", "x").unwrap().get(), -5);
        assert_eq!(
            length_of("1099511627775", "x").unwrap().get(),
            (1 << 40) - 1
        );
        for word in [
            "+5",
            "05",
            "-0",
            "1.5",
            "",
            "1e3",
            " 5",
            "5 ",
            "1099511627776",
            "-1099511627776",
            "9223372036854775808",
        ] {
            assert_eq!(
                length_of(word, "x").unwrap_err().code(),
                "PHOENIX-CAD-018",
                "{word:?}"
            );
        }
        assert_eq!(count_of("3", "x").unwrap(), 3);
        assert!(count_of("03", "x").is_err() && count_of("-1", "x").is_err());
        assert!(flag_of("0").is_ok() && flag_of("1").unwrap());
        assert!(flag_of("2").is_err() && flag_of("true").is_err());
    }

    #[test]
    fn the_smallest_recipe_has_the_text_written_by_hand() {
        let s = settings();
        let m = minimal();
        assert_eq!(
            String::from_utf8(m.to_bytes(&s.mechanical).unwrap()).unwrap(),
            MINIMAL_TEXT
        );
        let back = Mechanical::from_bytes(MINIMAL_TEXT.as_bytes(), &s.expr, &s.mechanical).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn every_kind_of_step_has_its_text_written_by_hand() {
        let l = settings().mechanical;
        let u1 = "018bcfe5-6800-7001-8000-000000000001";
        let u2 = "018bcfe5-6800-7002-8000-000000000002";
        let u7 = "018bcfe5-6800-7007-8000-000000000007";
        let sunk = |head: i64| HoleStyle::Countersunk {
            head_diameter: nm(head),
        };
        let a_hole = |style: HoleStyle| Hole {
            name: "a b".into(),
            face: Face::Top,
            u: mm(10),
            v: mm(20),
            diameter: nm(3_400_000),
            style,
        };
        let a_post = |board_hole: Option<Id>, kind: BoreKind, base: Option<Base>| Post {
            name: "p".into(),
            board_hole,
            x: mm(25),
            y: mm(15),
            outer_diameter: mm(8),
            height: mm(6),
            kind,
            bore_diameter: nm(2_500_000),
            bore_depth: mm(8),
            base,
        };
        let a_lid = |fixing: LidFixing, style: HoleStyle, lip: Option<Lip>| Lid {
            fixing,
            screw: "M2.5".into(),
            hole_diameter: nm(2_900_000),
            style,
            clearance: nm(200_000),
            lip,
        };
        let cases = [
            (
                step("", false, Operation::RemoveHole { id: id(1) }),
                format!("step 0 ~ remove-hole {u1}"),
            ),
            (
                step(
                    "edges",
                    true,
                    Operation::SetEdging(Edging {
                        vertical: Treatment::Round(mm(4)),
                        top: Treatment::Sharp,
                        bottom: Treatment::Chamfer(mm(2)),
                    }),
                ),
                "step 1 edges set-edging round:4000000 sharp chamfer:2000000".to_string(),
            ),
            (
                step("size", false, Operation::SetParams(base())),
                "step 0 size set-params inner_length=100000000 inner_width=60000000 inner_height=30000000 wall=2000000 floor=2000000 lid=3000000 edge_margin=3000000 min_web=2000000".to_string(),
            ),
            (
                step(
                    "r",
                    false,
                    Operation::ReplaceHole {
                        id: id(2),
                        hole: a_hole(sunk(6_300_000)),
                    },
                ),
                format!("step 0 r replace-hole {u2} a%20b top 10000000 20000000 3400000 countersunk:6300000"),
            ),
            (
                step(
                    "c",
                    false,
                    Operation::AddHole {
                        id: id(2),
                        hole: a_hole(HoleStyle::Counterbored {
                            head_diameter: mm(6),
                            depth: mm(3),
                        }),
                    },
                ),
                format!("step 0 c add-hole {u2} a%20b top 10000000 20000000 3400000 counterbored:6000000:3000000"),
            ),
            (
                step(
                    "r",
                    false,
                    Operation::AddHole {
                        id: id(2),
                        hole: a_hole(HoleStyle::Rectangular { height: mm(4) }),
                    },
                ),
                format!("step 0 r add-hole {u2} a%20b top 10000000 20000000 3400000 rectangular:4000000"),
            ),
            (
                step(
                    "o",
                    false,
                    Operation::AddHole {
                        id: id(2),
                        hole: a_hole(HoleStyle::Oval {
                            length: mm(12),
                            vertical: true,
                        }),
                    },
                ),
                format!("step 0 o add-hole {u2} a%20b top 10000000 20000000 3400000 oval:12000000:v"),
            ),
            (
                step(
                    "o",
                    false,
                    Operation::AddHole {
                        id: id(2),
                        hole: a_hole(HoleStyle::Oval {
                            length: mm(12),
                            vertical: false,
                        }),
                    },
                ),
                format!("step 0 o add-hole {u2} a%20b top 10000000 20000000 3400000 oval:12000000:u"),
            ),
            (
                step(
                    "post",
                    false,
                    Operation::AddPost {
                        id: id(7),
                        post: a_post(
                            Some(id(2)),
                            BoreKind::SelfTapping,
                            Some(Base {
                                diameter: mm(12),
                                height: mm(2),
                            }),
                        ),
                    },
                ),
                format!("step 0 post add-post {u7} p {u2} 25000000 15000000 8000000 6000000 self-tapping 2500000 8000000 foot:12000000:2000000"),
            ),
            (
                step(
                    "post",
                    false,
                    Operation::ReplacePost {
                        id: id(7),
                        post: a_post(None, BoreKind::Insert, None),
                    },
                ),
                format!("step 0 post replace-post {u7} p - 25000000 15000000 8000000 6000000 insert 2500000 8000000 none"),
            ),
            (
                step("gone", false, Operation::RemovePost { id: id(7) }),
                format!("step 0 gone remove-post {u7}"),
            ),
            (
                step("no lid", false, Operation::SetLid(None)),
                "step 0 no%20lid set-lid none".to_string(),
            ),
            (
                step(
                    "lid",
                    false,
                    Operation::SetLid(Some(a_lid(
                        LidFixing::Auto { inset: mm(6) },
                        sunk(5_000_000),
                        Some(Lip {
                            thickness: nm(1_200_000),
                            depth: mm(3),
                        }),
                    ))),
                ),
                "step 0 lid set-lid auto:6000000 M2.5 2900000 countersunk:5000000 200000 lip:1200000:3000000".to_string(),
            ),
            (
                step(
                    "lid",
                    false,
                    Operation::SetLid(Some(a_lid(
                        LidFixing::Corners { inset: mm(7) },
                        HoleStyle::Plain,
                        None,
                    ))),
                ),
                "step 0 lid set-lid corners:7000000 M2.5 2900000 plain 200000 nolip".to_string(),
            ),
            (
                step(
                    "lid",
                    false,
                    Operation::SetLid(Some(a_lid(LidFixing::Posts, HoleStyle::Plain, None))),
                ),
                "step 0 lid set-lid posts M2.5 2900000 plain 200000 nolip".to_string(),
            ),
        ];
        for (s, text) in cases {
            assert_eq!(step_line(&s, &l).unwrap(), text);
            assert_eq!(step_of(&text, &l).unwrap(), s, "{text}");
        }
    }

    #[test]
    fn a_step_that_is_not_written_in_the_one_form_is_refused() {
        let l = settings().mechanical;
        let u1 = "018bcfe5-6800-7001-8000-000000000001";
        for text in [
            "step".to_string(),
            "step 0 name".to_string(),
            "step 2 name remove-hole ".to_string() + u1,
            "step 0 name remove-hole".to_string(),
            "step 0 name remove-hole ".to_string() + &u1.to_uppercase(),
            "step 0 name remove-hole 018bcfe5-6800-6001-8000-000000000001".to_string(),
            format!("step 0 name remove-hole {u1} extra"),
            format!("step 0  name remove-hole {u1}"),
            format!("step 0 name remove-hole {u1} "),
            format!("step 0 name unknown-step {u1}"),
            "step 0 name set-params inner_width=1 inner_length=1 inner_height=1 wall=1 floor=1 lid=1 edge_margin=1 min_web=1".to_string(),
            "step 0 name set-params inner_length=1".to_string(),
            "step 0 name set-edging sharp sharp".to_string(),
            "step 0 name set-edging round:0 sharp sharp".to_string() + "x",
            format!("step 0 name add-hole {u1} h front 1 1 1 oval:5:w"),
            format!("step 0 name add-hole {u1} h middle 1 1 1 plain"),
            format!("step 0 name add-post {u1} p - 1 1 1 1 tapped 1 1 none"),
            format!("step 0 name add-post {u1} p - 1 1 1 1 insert 1 1 foot:1"),
            "step 0 name set-lid sometimes M3 1 plain 1 nolip".to_string(),
            "step 0 name set-lid posts M3 1 plain 1 lip:1".to_string(),
        ] {
            let err = step_of(&text, &l).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-018", "{text}");
        }
    }

    #[test]
    fn a_recipe_is_written_and_read_back_to_the_same_recipe_and_the_same_box() {
        let s = settings();
        let m = recipe();
        let before = m.build(&s.constants, &s.expr).unwrap();
        // every step of the recipe works: the box has the hole, the edges, four posts and the lid
        assert!(before.states.iter().all(|state| *state == StepState::Valid));
        assert_eq!(before.enclosure.holes().len(), 1);
        assert_eq!(before.enclosure.posts().len(), 4);
        assert_eq!(before.enclosure.lid_holes().len(), 4);
        // 100 + 2 * 2 mm of length, from the variable and the wall of five nozzles of 0.4 mm
        assert_eq!(before.enclosure.params().inner_length, mm(104));
        let bytes = m.to_bytes(&s.mechanical).unwrap();
        let back = Mechanical::from_bytes(&bytes, &s.expr, &s.mechanical).unwrap();
        assert_eq!(back, m);
        let after = back.build(&s.constants, &s.expr).unwrap();
        assert_eq!(after, before);
        // the same recipe is always the same bytes, and reading then writing changes nothing
        assert_eq!(back.to_bytes(&s.mechanical).unwrap(), bytes);
        assert_eq!(m.clone().to_bytes(&s.mechanical).unwrap(), bytes);
    }

    #[test]
    fn the_final_box_is_derived_from_the_recipe_and_never_stored() {
        let s = settings();
        let mut m = recipe();
        let bytes = m.to_bytes(&s.mechanical).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        // the inner length of 104 mm is the result of an expression, not a figure of the file
        assert!(!text.contains("104000000"));
        assert!(text.contains("param inner_length expr "));
        // a variable moves, the steps stay, and the box is made again
        let steps = m.steps.clone();
        m.vars.set("length", mm(120)).unwrap();
        let built = m.build(&s.constants, &s.expr).unwrap();
        assert_eq!(built.enclosure.params().inner_length, mm(124));
        assert!(built.states.iter().all(|state| *state == StepState::Valid));
        assert_eq!(m.steps, steps);
        // an expression that cannot be calculated is the error of the parameters, not a panic
        let mut broken = recipe();
        broken
            .model
            .set_expr(Param::Wall, "no_such_name + 1 mm", &s.expr)
            .unwrap();
        let err = broken.build(&s.constants, &s.expr).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-014");
    }

    #[test]
    fn a_suppressed_step_and_the_tip_are_kept_and_the_box_follows_them() {
        let s = settings();
        let mut m = recipe();
        m.steps[0].suppressed = true;
        m.tip = m.steps.len() - 1;
        let back =
            Mechanical::from_bytes(&m.to_bytes(&s.mechanical).unwrap(), &s.expr, &s.mechanical)
                .unwrap();
        assert_eq!(back, m);
        let built = back.build(&s.constants, &s.expr).unwrap();
        assert_eq!(built.states[0], StepState::Suppressed);
        assert_eq!(built.states[m.steps.len() - 1], StepState::AfterTip);
        // the hole is suppressed and the lid is after the tip
        assert!(built.enclosure.holes().is_empty());
        assert!(built.enclosure.lid().is_none());
        assert_eq!(built.enclosure.posts().len(), 4);
    }

    #[test]
    fn a_text_that_is_not_in_the_one_canonical_form_is_refused() {
        let s = settings();
        let parse = |text: &str| Mechanical::from_bytes(text.as_bytes(), &s.expr, &s.mechanical);
        assert!(parse(MINIMAL_TEXT).is_ok());
        for text in [
            MINIMAL_TEXT.replace("mechanical 1\n", "mechanical 01\n"),
            MINIMAL_TEXT.replace("mechanical 1\n", "mechanical 0\n"),
            MINIMAL_TEXT.replace("mechanical 1\n", "mechanical  1\n"),
            MINIMAL_TEXT.replace("mechanical 1\n", "mechanic 1\n"),
            MINIMAL_TEXT.replace("mechanical 1\n", "mechanical 2\n"),
            MINIMAL_TEXT.replace("\n", "\r\n"),
            MINIMAL_TEXT.trim_end().to_string(),
            MINIMAL_TEXT.to_string() + "\n",
            MINIMAL_TEXT.replace("[steps]\n", "[steps]\n\n"),
            MINIMAL_TEXT.replace("tip 1", "tip +1"),
            MINIMAL_TEXT.replace("tip 1", "tip 01"),
            MINIMAL_TEXT.replace("tip 1", "tip 2"),
            MINIMAL_TEXT.replace("tip 1\n", ""),
            MINIMAL_TEXT.replace("step 0 ", "step 2 "),
            MINIMAL_TEXT.replace("usb%20port", "usb port"),
            MINIMAL_TEXT.replace("usb%20port", "usb%2eport"),
            MINIMAL_TEXT.replace("[steps]\n", "[steps]\n[steps]\n"),
            MINIMAL_TEXT.replace("[parameters]\n", ""),
            MINIMAL_TEXT.replace("base wall 2000000\n", ""),
            MINIMAL_TEXT.replace("[steps]\ntip 1\n", "[board]\n[steps]\ntip 1\n"),
            MINIMAL_TEXT.to_string() + "[parameters]\n",
            MINIMAL_TEXT.to_string() + "something else\n",
        ] {
            let err = parse(&text).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-018", "{text}");
        }
        assert_eq!(
            Mechanical::from_bytes(b"mechanical 1\n\xff\n", &s.expr, &s.mechanical)
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-018"
        );
        // a parameter that does not read is reported with the same code
        let bad_expression = MINIMAL_TEXT.replace(
            "base min_web 2000000\n",
            "base min_web 2000000\nparam wall expr 1 +\n",
        );
        assert_eq!(
            parse(&bad_expression).unwrap_err().code(),
            "PHOENIX-CAD-018"
        );
    }

    #[test]
    fn every_cut_of_the_domain_is_refused_with_the_code_of_a_bad_file() {
        let s = settings();
        // without a board, no proper cut of the text is a recipe
        let mut plain = recipe();
        plain.board = None;
        let good = plain.to_bytes(&s.mechanical).unwrap();
        for n in 0..good.len() {
            let err = Mechanical::from_bytes(&good[..n], &s.expr, &s.mechanical).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-018", "cut at {n}");
        }
        // with a board a cut after a whole line of the board can be a smaller board, but nothing
        // is ever any other error
        let with_board = recipe().to_bytes(&s.mechanical).unwrap();
        for n in 0..with_board.len() {
            if let Err(e) = Mechanical::from_bytes(&with_board[..n], &s.expr, &s.mechanical) {
                assert_eq!(e.code(), "PHOENIX-CAD-018", "cut at {n}");
            }
        }
    }

    #[test]
    fn the_limits_hold_when_the_domain_is_written_and_when_it_is_read() {
        let s = settings();
        let m = recipe();
        let bytes = m.to_bytes(&s.mechanical).unwrap();
        let steps = m.steps.len();
        assert!(steps > 2);
        let write_read = |l: MechanicalLimits| {
            (
                m.to_bytes(&l).map(|_| ()),
                Mechanical::from_bytes(&bytes, &s.expr, &l).map(|_| ()),
            )
        };
        let (w, r) = write_read(MechanicalLimits {
            max_steps: steps,
            ..s.mechanical
        });
        assert!(w.is_ok() && r.is_ok());
        let (w, r) = write_read(MechanicalLimits {
            max_steps: steps - 1,
            ..s.mechanical
        });
        assert_eq!(w.unwrap_err().code(), "PHOENIX-CAD-018");
        assert_eq!(r.unwrap_err().code(), "PHOENIX-CAD-018");
        let (w, r) = write_read(MechanicalLimits {
            max_bytes: bytes.len(),
            ..s.mechanical
        });
        assert!(w.is_ok() && r.is_ok());
        let (w, r) = write_read(MechanicalLimits {
            max_bytes: bytes.len() - 1,
            ..s.mechanical
        });
        assert!(w.is_err() && r.is_err());
        // the outline has four points and the board four holes
        let (w, r) = write_read(MechanicalLimits {
            max_outline_points: 4,
            max_board_holes: 4,
            ..s.mechanical
        });
        assert!(w.is_ok() && r.is_ok());
        let (w, r) = write_read(MechanicalLimits {
            max_outline_points: 3,
            ..s.mechanical
        });
        assert!(w.is_err() && r.is_err());
        let (w, r) = write_read(MechanicalLimits {
            max_board_holes: 3,
            ..s.mechanical
        });
        assert!(w.is_err() && r.is_err());
        // a name of 256 bytes is the longest
        let mut named = minimal();
        named.steps[0].name = "x".repeat(256);
        assert!(named.to_bytes(&s.mechanical).is_ok());
        named.steps[0].name = "x".repeat(257);
        assert!(named.to_bytes(&s.mechanical).is_err());
        let long = MINIMAL_TEXT.replace("usb%20port", &"x".repeat(257));
        assert!(Mechanical::from_bytes(long.as_bytes(), &s.expr, &s.mechanical).is_err());
        // a tip beyond the last step cannot be written
        let mut beyond = minimal();
        beyond.tip = 2;
        assert!(beyond.to_bytes(&s.mechanical).is_err());
    }

    #[test]
    fn the_board_interface_is_read_in_its_order_and_with_its_rules() {
        let l = settings().mechanical;
        let h = "018bcfe5-6800-7001-8000-000000000001";
        let h2 = "018bcfe5-6800-7002-8000-000000000002";
        let points = ["point 0 0", "point 60000000 0", "point 60000000 40000000"];
        let hole = format!("hole {h} H1 5000000 5000000 3200000");
        let full: Vec<String> = [
            "thickness 1600000".to_string(),
            "tallest 5000000".to_string(),
            "place 10000000 5000000".to_string(),
        ]
        .into_iter()
        .chain(points.iter().map(|p| p.to_string()))
        .chain([hole.clone()])
        .collect();
        let refs = |lines: &[String]| -> Vec<String> { lines.to_vec() };
        let parse = |lines: Vec<String>| {
            let words: Vec<&str> = lines.iter().map(String::as_str).collect();
            board_of(&words, &l)
        };
        let ok = parse(refs(&full)).unwrap();
        assert_eq!(ok.thickness.get(), 1_600_000);
        assert_eq!(ok.tallest_component, Some(mm(5)));
        assert_eq!(ok.placement, Some((mm(10), mm(5))));
        assert_eq!(ok.outline.len(), 3);
        assert_eq!(ok.holes.len(), 1);
        assert_eq!(ok.holes[0].id, id(1));
        // the tallest part and the placement are optional
        let bare: Vec<String> = ["thickness 1600000"]
            .iter()
            .chain(points.iter())
            .map(|p| p.to_string())
            .collect();
        let bare = parse(bare).unwrap();
        assert_eq!((bare.tallest_component, bare.placement), (None, None));
        let swap = |a: usize, b: usize| {
            let mut lines = full.clone();
            lines.swap(a, b);
            lines
        };
        let with = |extra: &str, at: usize| {
            let mut lines = full.clone();
            lines.insert(at, extra.to_string());
            lines
        };
        for lines in [
            // out of order
            swap(0, 1),
            swap(1, 2),
            swap(3, 6),
            // repeated
            with("thickness 1600000", 1),
            with("tallest 1", 2),
            with("place 1 1", 3),
            // two holes with the same id, a hole narrower than nothing, a board without thickness
            with(&format!("hole {h} H1 5000000 5000000 3200000"), 7),
            with(&format!("hole {h2} H2 5000000 5000000 0"), 7),
            full[1..].to_vec(),
            // fewer than three points
            full[..5].to_vec(),
            // not understood
            with("point 1", 3),
            with("hole", 7),
            with("tallest +1", 1),
            ["thickness 0".to_string()]
                .into_iter()
                .chain(full[1..].iter().cloned())
                .collect(),
        ] {
            let err = parse(lines.clone()).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-018", "{lines:?}");
        }
    }

    #[test]
    fn the_board_goes_through_the_file_with_its_outline_and_mounting_holes() {
        let s = settings();
        let mut m = minimal();
        m.board = Some(board());
        let text = String::from_utf8(m.to_bytes(&s.mechanical).unwrap()).unwrap();
        let tail = "[board]\n\
thickness 1600000\n\
tallest 5000000\n\
point 0 0\n\
point 60000000 0\n\
point 60000000 40000000\n\
point 0 40000000\n\
hole 018bcfe5-6800-7001-8000-000000000001 H1 5000000 5000000 3200000\n\
hole 018bcfe5-6800-7002-8000-000000000002 H2 55000000 5000000 3200000\n\
hole 018bcfe5-6800-7003-8000-000000000003 H3 5000000 35000000 3200000\n\
hole 018bcfe5-6800-7004-8000-000000000004 H4 55000000 35000000 3200000\n";
        assert_eq!(text, MINIMAL_TEXT.to_string() + tail);
        let back = Mechanical::from_bytes(text.as_bytes(), &s.expr, &s.mechanical).unwrap();
        assert_eq!(back, m);
        // a board with a placement
        m.board.as_mut().unwrap().placement = Some((mm(10), mm(5)));
        let text = String::from_utf8(m.to_bytes(&s.mechanical).unwrap()).unwrap();
        assert!(text.contains("tallest 5000000\nplace 10000000 5000000\npoint 0 0\n"));
        // an outline with fewer than three points is not written
        m.board.as_mut().unwrap().outline.truncate(2);
        assert!(m.to_bytes(&s.mechanical).is_err());
        // what could not be read back is not written either
        let mut flat = minimal();
        flat.board = Some(board());
        flat.board.as_mut().unwrap().thickness = Length::ZERO;
        assert!(flat.to_bytes(&s.mechanical).is_err());
        let mut hollow = minimal();
        hollow.board = Some(board());
        hollow.board.as_mut().unwrap().holes[0].diameter = Length::ZERO;
        assert!(hollow.to_bytes(&s.mechanical).is_err());
        let mut twice = minimal();
        twice.board = Some(board());
        twice.board.as_mut().unwrap().holes[1].id = id(1);
        assert!(twice.to_bytes(&s.mechanical).is_err());
    }

    #[test]
    fn the_domain_replaces_the_older_one_in_the_project_and_keeps_the_unknown() {
        let s = settings();
        let m = recipe();
        let mut d = Document::new();
        d.set("from-the-future", false, vec![1u8, 2, 255, b'\n'])
            .unwrap();
        d.set_parameters(&m.model, &m.vars).unwrap();
        assert!(d.get(PARAMETERS_DOMAIN).is_some());
        d.set_mechanical(&m, &s.mechanical).unwrap();
        assert!(
            d.get(PARAMETERS_DOMAIN).is_none(),
            "the older domain is replaced"
        );
        assert!(d.get(MECHANICAL_DOMAIN).unwrap().critical);
        assert_eq!(d.mechanical_version().unwrap(), Some(FIRST_VERSION));
        let bytes = d.to_bytes();
        let read = Document::from_bytes(&bytes, 1 << 24).unwrap();
        assert_eq!(
            read.get("from-the-future").unwrap().data,
            vec![1u8, 2, 255, b'\n'],
            "a domain this program does not know is kept byte for byte"
        );
        assert_eq!(read.mechanical(&s.expr, &s.mechanical).unwrap(), Some(m));
        // a program that does not understand the domain opens the project read-only
        assert!(Document::open(&bytes, 1 << 24, &[]).unwrap().read_only);
        let opened = Document::open(&bytes, 1 << 24, &[MECHANICAL_DOMAIN]).unwrap();
        assert!(!opened.read_only);
        // writing the same recipe again gives the same file
        assert_eq!(read.to_bytes(), bytes);
    }

    #[test]
    fn a_project_with_only_the_older_domain_is_read_as_a_recipe_without_steps() {
        let s = settings();
        let m = recipe();
        let mut d = Document::new();
        assert_eq!(d.mechanical_version().unwrap(), None);
        assert_eq!(d.mechanical(&s.expr, &s.mechanical).unwrap(), None);
        d.set_parameters(&m.model, &m.vars).unwrap();
        let read = d.mechanical(&s.expr, &s.mechanical).unwrap().unwrap();
        assert_eq!(read, Mechanical::new(m.model.clone(), m.vars.clone()));
        assert!(read.steps.is_empty() && read.board.is_none() && read.tip == 0);
        // the box of the migrated recipe is the one of the parameters
        let built = read.build(&s.constants, &s.expr).unwrap();
        assert_eq!(built.enclosure.params().inner_length, mm(104));
        assert!(built.enclosure.holes().is_empty());
        // the first write brings the project to the new domain and the older one goes away
        d.set_mechanical(&read, &s.mechanical).unwrap();
        assert_eq!(d.names(), vec![MECHANICAL_DOMAIN]);
        assert_eq!(d.mechanical(&s.expr, &s.mechanical).unwrap().unwrap(), read);
        // when both domains are there the new one is the one that counts
        d.set(
            PARAMETERS_DOMAIN,
            true,
            parameters_to_bytes(&m.model, &m.vars),
        )
        .unwrap();
        d.set_mechanical(&m, &s.mechanical).unwrap();
        assert_eq!(d.mechanical(&s.expr, &s.mechanical).unwrap().unwrap(), m);
    }

    #[test]
    fn a_domain_of_a_newer_version_is_neither_read_nor_overwritten() {
        let s = settings();
        let future = b"mechanical 3\nwhatever \xff comes next\n".to_vec();
        let mut d = Document::new();
        d.set(MECHANICAL_DOMAIN, true, future.clone()).unwrap();
        d.set("notes", false, b"x".to_vec()).unwrap();
        assert_eq!(d.mechanical_version().unwrap(), Some(3));
        let err = d.mechanical(&s.expr, &s.mechanical).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-018");
        assert!(err.to_string().contains("newer version"), "{err}");
        let err = d.set_mechanical(&minimal(), &s.mechanical).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-018");
        assert_eq!(d.get(MECHANICAL_DOMAIN).unwrap().data, future);
        // and it goes through the file as it is
        let read = Document::from_bytes(&d.to_bytes(), 1 << 20).unwrap();
        assert_eq!(read.get(MECHANICAL_DOMAIN).unwrap().data, future);
        // a header that is not a header is a broken file, not a newer one
        d.set(MECHANICAL_DOMAIN, true, b"hello\n".to_vec()).unwrap();
        assert_eq!(
            d.mechanical_version().unwrap_err().code(),
            "PHOENIX-CAD-018"
        );
        d.set(MECHANICAL_DOMAIN, true, b"mechanical 0\n".to_vec())
            .unwrap();
        assert!(d.mechanical_version().is_err());
    }

    #[test]
    fn undoing_gives_back_the_original_recipe_and_redoing_the_edited_one() {
        let s = settings();
        let m = recipe();
        let original = m.to_bytes(&s.mechanical).unwrap();
        let body = m.body(&s.constants, &s.expr).unwrap();
        let mut session = Session::new(body.clone(), 10);
        session
            .transaction("add a hole", |b| {
                b.insert(
                    "vent",
                    Operation::AddHole {
                        id: id(90),
                        hole: front_hole("vent", 70),
                    },
                );
                Ok(())
            })
            .unwrap();
        session
            .transaction("move the tip", |b| b.set_tip(2))
            .unwrap();
        let mut edited = m.clone();
        edited.take_recipe(session.body());
        let edited_bytes = edited.to_bytes(&s.mechanical).unwrap();
        assert_ne!(edited_bytes, original);
        assert_eq!(
            Mechanical::from_bytes(&edited_bytes, &s.expr, &s.mechanical).unwrap(),
            edited
        );
        assert_eq!(session.undo().as_deref(), Some("move the tip"));
        assert_eq!(session.undo().as_deref(), Some("add a hole"));
        assert_eq!(session.body(), &body);
        let mut back = edited.clone();
        back.take_recipe(session.body());
        assert_eq!(back, m);
        assert_eq!(back.to_bytes(&s.mechanical).unwrap(), original);
        assert_eq!(session.redo().as_deref(), Some("add a hole"));
        assert_eq!(session.redo().as_deref(), Some("move the tip"));
        let mut again = m.clone();
        again.take_recipe(session.body());
        assert_eq!(again, edited);
    }

    /// Whatever steps a recipe has, good or not, writing it and reading it back gives the same
    /// recipe, the same bytes and the same box (the same states and the same errors).
    #[test]
    fn random_recipes_survive_the_file_and_build_the_same_box() {
        let s = settings();
        for seed in 1..=30u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let mut m = recipe();
            m.steps.clear();
            let count = rng.draw(25) + 1;
            for _ in 0..count {
                let st = rng.step();
                m.steps.push(st);
            }
            m.tip = rng.draw(m.steps.len() as u64 + 1) as usize;
            if rng.draw(2) == 0 {
                m.board = None;
            } else if let Some(board) = m.board.as_mut() {
                if rng.draw(2) == 0 {
                    board.tallest_component = None;
                }
                if rng.draw(2) == 0 {
                    board.placement = Some((rng.length(), rng.length()));
                }
            }
            let bytes = m.to_bytes(&s.mechanical).unwrap();
            let back = Mechanical::from_bytes(&bytes, &s.expr, &s.mechanical).unwrap();
            assert_eq!(back, m, "seed {seed}");
            assert_eq!(back.to_bytes(&s.mechanical).unwrap(), bytes, "seed {seed}");
            assert_eq!(
                back.build(&s.constants, &s.expr),
                m.build(&s.constants, &s.expr),
                "seed {seed}"
            );
            // and through a project file
            let mut d = Document::new();
            d.set_mechanical(&m, &s.mechanical).unwrap();
            let read = Document::from_bytes(&d.to_bytes(), 1 << 24).unwrap();
            assert_eq!(
                read.mechanical(&s.expr, &s.mechanical).unwrap(),
                Some(m),
                "seed {seed}"
            );
        }
    }
}

#[cfg(test)]
mod feature_tests {
    use super::*;
    use crate::body::StepState;
    use crate::config::Settings;
    use crate::enclosure::Face;
    use crate::sketch::SketchPlane;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn id(n: u64) -> Id {
        ObjectId::from_parts(1_700_000_000_000, n as u16, n).unwrap()
    }

    fn base() -> EnclosureParams {
        EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        }
    }

    fn plate() -> Sketch {
        let plane = SketchPlane {
            face: Face::Top,
            u0: mm(10),
            v0: mm(10),
            offset: Length::ZERO,
        };
        let mut k = Sketch::new("plate", plane).unwrap();
        k.add_rectangle((mm(0), mm(0)), (mm(40), mm(25))).unwrap();
        let c = k.add_point(mm(20), mm(12)).unwrap();
        k.add_circle(c, mm(3)).unwrap();
        k
    }

    fn step(name: &str, op: Operation) -> Step {
        Step {
            name: name.into(),
            op,
            suppressed: false,
        }
    }

    fn with_features() -> Mechanical {
        let mut m = Mechanical::new(ParamModel::new(base()).unwrap(), Variables::new());
        m.sketches.push(plate());
        m.steps.push(step(
            "boss",
            Operation::Extrude {
                id: id(1),
                sketch: "plate".into(),
                height: mm(10),
            },
        ));
        m.steps.push(step(
            "window",
            Operation::Pocket {
                id: id(2),
                sketch: "plate".into(),
                depth: None,
            },
        ));
        m.steps.push(step(
            "shallow",
            Operation::Pocket {
                id: id(3),
                sketch: "plate".into(),
                depth: Some(mm(1)),
            },
        ));
        m.steps
            .push(step("gone", Operation::RemoveFeature { id: id(3) }));
        m.tip = m.steps.len();
        m
    }

    #[test]
    fn extrusions_and_pockets_are_steps_of_version_two_with_a_golden_text() {
        let s = Settings::defaults();
        let m = with_features();
        assert_eq!(m.version(), 2);
        let text = String::from_utf8(m.to_bytes(&s.mechanical).unwrap()).unwrap();
        assert!(text.starts_with("mechanical 2\n[parameters]\n"));
        let steps = text.split("[steps]\n").nth(1).unwrap();
        let ids: Vec<String> = (1..=3).map(|n| id(n).to_string()).collect();
        let expect = format!(
            "tip 4\nstep 0 boss extrude {} plate 10000000\nstep 0 window pocket {} plate through\n\
             step 0 shallow pocket {} plate 1000000\nstep 0 gone remove-feature {}\n",
            ids[0], ids[1], ids[2], ids[2]
        );
        assert_eq!(steps, expect);
        let mut read = Mechanical::from_bytes(text.as_bytes(), &s.expr, &s.mechanical).unwrap();
        assert!(
            read.sketches.is_empty(),
            "the sketches are in their own domain"
        );
        read.sketches = m.sketches.clone();
        assert_eq!(read, m);
        assert_eq!(read.to_bytes(&s.mechanical).unwrap(), text.as_bytes());
    }

    #[test]
    fn a_recipe_without_them_is_still_version_one_and_the_text_does_not_change() {
        let s = Settings::defaults();
        let mut m = with_features();
        m.steps.clear();
        m.tip = 0;
        assert_eq!(m.version(), 1);
        let text = String::from_utf8(m.to_bytes(&s.mechanical).unwrap()).unwrap();
        assert!(text.starts_with("mechanical 1\n"));
        assert!(
            !text.contains("sketch"),
            "the sketches are not in this domain"
        );
    }

    #[test]
    fn the_version_and_the_steps_have_to_agree() {
        let s = Settings::defaults();
        let m = with_features();
        let two = String::from_utf8(m.to_bytes(&s.mechanical).unwrap()).unwrap();
        let parse = |t: &str| Mechanical::from_bytes(t.as_bytes(), &s.expr, &s.mechanical);
        // a step of version 2 in a header of version 1
        let e = parse(&two.replacen("mechanical 2", "mechanical 1", 1)).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-018");
        // a header of version 2 with nothing that needs it
        let mut plain = with_features();
        plain.steps.clear();
        plain.tip = 0;
        let one = String::from_utf8(plain.to_bytes(&s.mechanical).unwrap()).unwrap();
        let e = parse(&one.replacen("mechanical 1", "mechanical 2", 1)).unwrap_err();
        assert!(e.to_string().contains("no step that needs it"), "{e}");
        // words of the steps that are not understood
        for bad in [
            "extrude X plate 10",
            "pocket {} plate",
            "pocket {} plate 0x10",
            "pocket {} plate -",
            "extrude {} plate 010000000",
            "extrude {} pl@te 10000000",
            "remove-feature",
        ] {
            let line = bad.replace("{}", &id(1).to_string());
            let text = two.replace(&format!("extrude {} plate 10000000", id(1)), &line);
            assert!(parse(&text).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_future_version_is_neither_read_nor_overwritten() {
        let s = Settings::defaults();
        let mut d = Document::new();
        let future = b"mechanical 3\nwhatever\n".to_vec();
        d.set(MECHANICAL_DOMAIN, true, future.clone()).unwrap();
        assert_eq!(d.mechanical_version().unwrap(), Some(3));
        assert!(d.mechanical(&s.expr, &s.mechanical).is_err());
        assert!(d.set_mechanical(&with_features(), &s.mechanical).is_err());
        assert_eq!(d.get(MECHANICAL_DOMAIN).unwrap().data, future);
    }

    #[test]
    fn the_steps_build_the_features_from_the_sketches_of_the_recipe() {
        let s = Settings::defaults();
        let m = with_features();
        let built = m.build(&s.constants, &s.expr).unwrap();
        assert!(
            built.states.iter().all(|st| *st == StepState::Valid),
            "{:?}",
            built.states
        );
        // the boss and the window remain; the shallow pocket was removed by the last step
        let e = &built.enclosure;
        assert_eq!(e.features().len(), 2);
        let (added, removed) = e.features_volume_nm3();
        assert!(added > 0 && removed > 0);
        // without the sketch the steps fail with their code and the rest goes on
        let mut lost = m.clone();
        lost.sketches.clear();
        let built = lost.build(&s.constants, &s.expr).unwrap();
        for st in &built.states[..3] {
            match st {
                StepState::Failed(e) => assert_eq!(e.code(), "PHOENIX-CAD-037"),
                other => panic!("{other:?}"),
            }
        }
        assert!(built.enclosure.features().is_empty());
    }

    #[test]
    fn a_project_with_sketches_goes_to_the_file_and_back_and_an_old_one_opens_as_before() {
        let s = Settings::defaults();
        let k = crate::sketch::settings::SketchSettings::defaults();
        let m = with_features();
        let mut d = Document::new();
        d.set("from-another-tool", false, vec![9, 8, 255]).unwrap();
        d.set_mechanical(&m, &s.mechanical).unwrap();
        d.set_sketches(&m.sketches, &k).unwrap();
        let bytes = d.to_bytes();
        let read = Document::from_bytes(&bytes, 1 << 24).unwrap();
        let mut back = read.mechanical(&s.expr, &s.mechanical).unwrap().unwrap();
        back.sketches = read.sketches(&k).unwrap();
        assert_eq!(back, m);
        assert_eq!(read.get("from-another-tool").unwrap().data, vec![9, 8, 255]);
        assert_eq!(read.to_bytes(), bytes);
        // a project written before the sketches existed has no such domain
        let mut old = Document::new();
        let plain = Mechanical::new(ParamModel::new(base()).unwrap(), Variables::new());
        old.set_mechanical(&plain, &s.mechanical).unwrap();
        let old_bytes = old.to_bytes();
        let opened = Document::open(&old_bytes, 1 << 24, &[MECHANICAL_DOMAIN]).unwrap();
        assert!(!opened.read_only);
        assert!(opened.document.sketches(&k).unwrap().is_empty());
        assert_eq!(opened.document.mechanical_version().unwrap(), Some(1));
        // writing it again, with no sketch, gives the same bytes
        let mut again = opened.document;
        again.set_mechanical(&plain, &s.mechanical).unwrap();
        again.set_sketches(&[], &k).unwrap();
        assert_eq!(again.to_bytes(), old_bytes);
    }
}
