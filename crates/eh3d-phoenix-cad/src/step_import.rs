//! Reading a STEP file (ISO 10303-21) of solids made of flat faces.
//!
//! The reader takes the solids a file names (`MANIFOLD_SOLID_BREP`, `BREP_WITH_VOIDS` and
//! `FACETED_BREP`) and gives the triangles of each one, as the readers of the other interchange
//! formats do. It reads what a turned or milled part is mostly made of: planes (`PLANE`),
//! cylinders (`CYLINDRICAL_SURFACE`) and cones (`CONICAL_SURFACE`) bounded by loops of straight
//! edges (`LINE`) and arcs of circles (`CIRCLE`) or by a `POLY_LOOP`, with holes in the planes as
//! further bounds and with the voids of a `BREP_WITH_VOIDS`. The unit of the file (millimetre,
//! centimetre, metre, micron or inch) is read from its length unit and every coordinate is rounded
//! to a whole nanometre. A curve becomes straight pieces (`mesh.step_segments_per_turn` for each
//! whole turn), so a curved face is flat pieces and its volume is that of the pieces; which shapes
//! of a curved face are read is told in the module `step_curved`.
//!
//! Everything else is refused, in order, with the number and the name of the first entity that
//! cannot be read: a face on a sphere, a torus, a surface of revolution or of linear extrusion or
//! a B-spline surface, an edge on an ellipse or a B-spline curve, an open shell, and the solids
//! that are not a boundary representation. Nothing is guessed and nothing is left out, so a file
//! that is read is the whole of what it holds.
//!
//! The size of the file, the number of its entities, the number of its faces and the number of
//! triangles are limited by the settings (`mesh.step_max_kib`, `mesh.step_max_entities`,
//! `mesh.step_max_faces` and `mesh.max_triangles`), and a file that goes over a limit is refused
//! before the rest of it is looked at.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::f64::consts::TAU;

use crate::config::MeshSettings;
use crate::interchange::Body;
use crate::mesh::{Builder, Loop, Mesh, Role, P3};
use crate::step_curved::{self, Corner, Frame, ON_NM};

/// The limits of a STEP file that is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepLimits {
    /// The most bytes of the file.
    pub max_bytes: usize,
    /// The most entities of its data section.
    pub max_entities: usize,
    /// The most faces of all its solids.
    pub max_faces: usize,
    /// The most triangles of all its solids.
    pub max_triangles: usize,
    /// The straight pieces that stand for a whole turn of a circle.
    pub segments_per_turn: u32,
}

impl StepLimits {
    /// The limits that the settings give.
    pub fn from_settings(s: &MeshSettings) -> StepLimits {
        StepLimits {
            max_bytes: s.step_max_kib as usize * 1024,
            max_entities: s.step_max_entities as usize,
            max_faces: s.step_max_faces as usize,
            max_triangles: s.max_triangles as usize,
            segments_per_turn: s.step_segments_per_turn,
        }
    }
}

/// The deepest nesting of lists in the arguments of an entity.
const MAX_DEPTH: usize = 16;

/// The farthest a coordinate may be from the origin, in nanometres (a thousand kilometres).
const FAR_NM: f64 = 1.0e15;

/// How far from its plane a corner of a flat face may be, in nanometres (a micron).
const FLAT_NM: f64 = 1000.0;

#[derive(Debug, Clone, PartialEq)]
enum Arg {
    Null,
    Star,
    Ref(usize),
    Num(f64),
    Text(String),
    Enum(String),
    List(Vec<Arg>),
    Typed(String, Vec<Arg>),
}

#[derive(Debug, Clone)]
struct Entity {
    /// The name of the entity, empty for one made of several (`(LENGTH_UNIT() ...)`).
    name: String,
    /// Its arguments; for an entity of several names, one `Typed` for each.
    args: Vec<Arg>,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn skip_space(&mut self) {
        while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn word(&mut self) -> String {
        let start = self.at;
        while self.at < self.bytes.len()
            && (self.bytes[self.at].is_ascii_alphanumeric() || self.bytes[self.at] == b'_')
        {
            self.at += 1;
        }
        String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned()
    }

    /// A list of values up to the closing bracket, which is taken. A comma between values is
    /// optional so that the parts of an entity of several names can follow one another.
    fn list(&mut self, depth: usize) -> Result<Vec<Arg>, String> {
        if depth > MAX_DEPTH {
            return Err("lists are nested too deeply".to_owned());
        }
        let mut items = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                None => return Err("a list is not closed".to_owned()),
                Some(b')') => {
                    self.at += 1;
                    return Ok(items);
                }
                Some(b',') => {
                    self.at += 1;
                }
                Some(_) => items.push(self.value(depth)?),
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Arg, String> {
        self.skip_space();
        let Some(c) = self.peek() else {
            return Err("an argument is missing".to_owned());
        };
        match c {
            b'$' => {
                self.at += 1;
                Ok(Arg::Null)
            }
            b'*' => {
                self.at += 1;
                Ok(Arg::Star)
            }
            b'#' => {
                self.at += 1;
                let digits = self.word();
                digits
                    .parse::<usize>()
                    .map(Arg::Ref)
                    .map_err(|_| format!("#{digits} is not a reference"))
            }
            b'\'' => {
                self.at += 1;
                let mut text = Vec::new();
                loop {
                    match self.peek() {
                        None => return Err("a text is not closed".to_owned()),
                        Some(b'\'') => {
                            self.at += 1;
                            if self.peek() == Some(b'\'') {
                                text.push(b'\'');
                                self.at += 1;
                            } else {
                                break;
                            }
                        }
                        Some(b) => {
                            text.push(b);
                            self.at += 1;
                        }
                    }
                }
                Ok(Arg::Text(String::from_utf8_lossy(&text).into_owned()))
            }
            b'(' => {
                self.at += 1;
                Ok(Arg::List(self.list(depth + 1)?))
            }
            b'.' if self
                .bytes
                .get(self.at + 1)
                .is_some_and(|b| b.is_ascii_uppercase()) =>
            {
                self.at += 1;
                let name = self.word();
                if self.peek() != Some(b'.') {
                    return Err(format!(".{name} is not closed"));
                }
                self.at += 1;
                Ok(Arg::Enum(name))
            }
            b'-' | b'+' | b'.' | b'0'..=b'9' => {
                let start = self.at;
                self.at += 1;
                while self.at < self.bytes.len()
                    && (self.bytes[self.at].is_ascii_digit()
                        || matches!(self.bytes[self.at], b'.' | b'e' | b'E')
                        || (matches!(self.bytes[self.at], b'-' | b'+')
                            && matches!(self.bytes[self.at - 1], b'e' | b'E')))
                {
                    self.at += 1;
                }
                let text = String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned();
                match text.parse::<f64>() {
                    Ok(v) if v.is_finite() => Ok(Arg::Num(v)),
                    _ => Err(format!("{text} is not a finite number")),
                }
            }
            b if b.is_ascii_alphabetic() => {
                let name = self.word().to_ascii_uppercase();
                self.skip_space();
                if self.peek() == Some(b'(') {
                    self.at += 1;
                    Ok(Arg::Typed(name, self.list(depth + 1)?))
                } else {
                    Err(format!("{name} has no arguments"))
                }
            }
            other => Err(format!("unexpected character {:?}", other as char)),
        }
    }
}

/// The statements of the data section of a file: the text between `DATA;` and `ENDSEC;`, cut at
/// the semicolons that are not inside a text, with the comments taken out.
fn statements(text: &str) -> Result<Vec<String>, String> {
    let upper_start = text
        .match_indices("DATA;")
        .map(|(i, _)| i)
        .find(|&i| i == 0 || !text.as_bytes()[i - 1].is_ascii_alphanumeric())
        .ok_or("the file has no DATA section")?;
    let body = &text[upper_start + 5..];
    let mut out = Vec::new();
    let (mut start, mut in_text, mut in_comment) = (0usize, false, false);
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_comment {
            if b == b'*' && bytes.get(i + 1) == Some(&b'/') {
                in_comment = false;
                i += 1;
            }
        } else if in_text {
            if b == b'\'' {
                in_text = false;
            }
        } else if b == b'\'' {
            in_text = true;
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            in_comment = true;
            i += 1;
        } else if b == b';' {
            let statement = without_comments(&body[start..i]);
            let statement = statement.trim();
            if statement.eq_ignore_ascii_case("ENDSEC") {
                return Ok(out);
            }
            if !statement.is_empty() {
                out.push(statement.to_owned());
            }
            start = i + 1;
        }
        i += 1;
    }
    Err("the DATA section has no ENDSEC".to_owned())
}

/// A statement without its comments (`/* ... */`); a comment that is not closed runs to the end.
fn without_comments(statement: &str) -> String {
    let mut out = String::new();
    let mut rest = statement;
    let mut in_text = false;
    while !rest.is_empty() {
        if !in_text && rest.starts_with("/*") {
            rest = rest[2..].split_once("*/").map_or("", |(_, r)| r);
            out.push(' ');
            continue;
        }
        let c = rest.chars().next().unwrap_or(' ');
        if c == '\'' {
            in_text = !in_text;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// The entities of a file by their number.
fn entities(text: &str, limits: &StepLimits) -> Result<BTreeMap<usize, Entity>, String> {
    let mut map: BTreeMap<usize, Entity> = BTreeMap::new();
    for statement in statements(text)? {
        let Some(rest) = statement.strip_prefix('#') else {
            return Err(format!(
                "a statement of the data section does not start with a number: {}",
                statement.chars().take(40).collect::<String>()
            ));
        };
        let (digits, body) = rest
            .split_once('=')
            .ok_or_else(|| format!("#{}: there is no = after the number", rest.trim()))?;
        let id: usize = digits
            .trim()
            .parse()
            .map_err(|_| format!("#{}: not a number", digits.trim()))?;
        if map.len() >= limits.max_entities {
            return Err(format!(
                "the file has more than the {} entities that the settings allow",
                limits.max_entities
            ));
        }
        let mut cursor = Cursor {
            bytes: body.as_bytes(),
            at: 0,
        };
        cursor.skip_space();
        let entity = if cursor.peek() == Some(b'(') {
            cursor.at += 1;
            Entity {
                name: String::new(),
                args: cursor.list(0).map_err(|e| format!("#{id}: {e}"))?,
            }
        } else {
            let name = cursor.word().to_ascii_uppercase();
            if name.is_empty() {
                return Err(format!("#{id}: the entity has no name"));
            }
            cursor.skip_space();
            if cursor.peek() != Some(b'(') {
                return Err(format!("#{id}: {name} has no arguments"));
            }
            cursor.at += 1;
            Entity {
                name,
                args: cursor.list(0).map_err(|e| format!("#{id}: {e}"))?,
            }
        };
        if map.insert(id, entity).is_some() {
            return Err(format!("#{id} is defined twice"));
        }
    }
    Ok(map)
}

/// The nanometres of a unit of length named by its entity.
fn unit_nm(entity: &Entity) -> Option<f64> {
    let parts: Vec<(&str, &[Arg])> = if entity.name.is_empty() {
        entity
            .args
            .iter()
            .filter_map(|a| match a {
                Arg::Typed(n, v) => Some((n.as_str(), v.as_slice())),
                _ => None,
            })
            .collect()
    } else {
        vec![(entity.name.as_str(), entity.args.as_slice())]
    };
    if !parts.iter().any(|(n, _)| *n == "LENGTH_UNIT") {
        return None;
    }
    for (name, args) in &parts {
        if *name == "SI_UNIT" {
            let prefix = match args.first() {
                Some(Arg::Enum(p)) => p.as_str(),
                _ => "",
            };
            return Some(match prefix {
                "MILLI" => 1.0e6,
                "CENTI" => 1.0e7,
                "MICRO" => 1.0e3,
                "KILO" => 1.0e12,
                "" => 1.0e9,
                _ => return None,
            });
        }
        if *name == "CONVERSION_BASED_UNIT" {
            if let Some(Arg::Text(t)) = args.first() {
                return match t.to_ascii_uppercase().as_str() {
                    "INCH" => Some(25.4e6),
                    "FOOT" => Some(304.8e6),
                    "MILLIMETRE" | "MILLIMETER" => Some(1.0e6),
                    _ => None,
                };
            }
        }
    }
    None
}

/// The unit of length of the file in nanometres: the first length unit it names, millimetres when
/// it names none.
fn length_unit(map: &BTreeMap<usize, Entity>) -> Result<f64, String> {
    for (id, e) in map {
        let names_length = e.name == "LENGTH_UNIT"
            || e.name == "SI_UNIT"
            || e.name == "CONVERSION_BASED_UNIT"
            || e.args
                .iter()
                .any(|a| matches!(a, Arg::Typed(n, _) if n == "LENGTH_UNIT"));
        if names_length {
            if let Some(nm) = unit_nm(e) {
                return Ok(nm);
            }
            if e.args
                .iter()
                .any(|a| matches!(a, Arg::Typed(n, _) if n == "LENGTH_UNIT"))
            {
                return Err(format!(
                    "#{id}: the unit of length is not one this reader knows"
                ));
            }
        }
    }
    Ok(1.0e6)
}

/// The radians of one unit of the angles of the file: degrees when it names them.
fn angle_unit(map: &BTreeMap<usize, Entity>) -> f64 {
    let degrees = map.values().any(|e| {
        let plane = e
            .args
            .iter()
            .any(|a| matches!(a, Arg::Typed(n, _) if n == "PLANE_ANGLE_UNIT"));
        plane
            && e.args.iter().any(|a| {
                matches!(a, Arg::Typed(n, v) if n == "CONVERSION_BASED_UNIT"
                    && matches!(v.first(), Some(Arg::Text(t)) if t.eq_ignore_ascii_case("DEGREE")))
            })
    });
    if degrees {
        TAU / 360.0
    } else {
        1.0
    }
}

struct Reader<'a> {
    map: &'a BTreeMap<usize, Entity>,
    unit_nm: f64,
    /// The radians of one unit of angle.
    angle: f64,
    /// The straight pieces of a whole turn of a circle.
    segments: u32,
    /// The corners that the edges may still give: a file cannot ask for more than its triangles
    /// could use, however often it names an edge.
    corners_left: Cell<usize>,
}

/// The names of the surfaces, curves and solids that are known and are not read, for the messages.
const SURFACES: [&str; 12] = [
    "CYLINDRICAL_SURFACE",
    "CONICAL_SURFACE",
    "SPHERICAL_SURFACE",
    "TOROIDAL_SURFACE",
    "SURFACE_OF_REVOLUTION",
    "SURFACE_OF_LINEAR_EXTRUSION",
    "B_SPLINE_SURFACE_WITH_KNOTS",
    "RATIONAL_B_SPLINE_SURFACE",
    "BEZIER_SURFACE",
    "DEGENERATE_TOROIDAL_SURFACE",
    "OFFSET_SURFACE",
    "ELLIPTIC_CYLINDER_SURFACE",
];

impl Reader<'_> {
    fn entity(&self, id: usize) -> Result<&Entity, String> {
        self.map
            .get(&id)
            .ok_or_else(|| format!("#{id} is used but never defined"))
    }

    fn typed(&self, id: usize, names: &[&str]) -> Result<&Entity, String> {
        let e = self.entity(id)?;
        if names.contains(&e.name.as_str()) {
            Ok(e)
        } else {
            Err(format!(
                "#{id} is {} where {} was expected",
                if e.name.is_empty() {
                    "an entity of several names"
                } else {
                    &e.name
                },
                names.join(" or ")
            ))
        }
    }

    fn reference(args: &[Arg], at: usize, id: usize, name: &str) -> Result<usize, String> {
        match args.get(at) {
            Some(Arg::Ref(r)) => Ok(*r),
            _ => Err(format!(
                "#{id} {name}: argument {} is not a reference",
                at + 1
            )),
        }
    }

    fn references(args: &[Arg], at: usize, id: usize, name: &str) -> Result<Vec<usize>, String> {
        match args.get(at) {
            Some(Arg::List(items)) => items
                .iter()
                .map(|a| match a {
                    Arg::Ref(r) => Ok(*r),
                    _ => Err(format!(
                        "#{id} {name}: a list holds something that is not a reference"
                    )),
                })
                .collect(),
            _ => Err(format!("#{id} {name}: argument {} is not a list", at + 1)),
        }
    }

    fn flag(args: &[Arg], at: usize) -> bool {
        // a missing or unknown flag is the usual sense
        !matches!(args.get(at), Some(Arg::Enum(f)) if f == "F")
    }

    fn coordinates(&self, args: &[Arg], id: usize) -> Result<[f64; 3], String> {
        match args.get(1) {
            Some(Arg::List(v)) if v.len() == 3 => {
                let mut out = [0.0; 3];
                for (o, a) in out.iter_mut().zip(v) {
                    match a {
                        Arg::Num(n) => *o = *n,
                        _ => return Err(format!("#{id}: a coordinate is not a number")),
                    }
                }
                Ok(out)
            }
            _ => Err(format!("#{id}: a point needs three coordinates")),
        }
    }

    /// A point of the file in nanometres.
    fn point_nm(&self, id: usize) -> Result<P3, String> {
        let e = self.typed(id, &["CARTESIAN_POINT"])?;
        let c = self.coordinates(&e.args, id)?;
        let mut out = [0.0; 3];
        for (o, v) in out.iter_mut().zip(c) {
            let nm = (v * self.unit_nm).round();
            if !nm.is_finite() || nm.abs() > FAR_NM {
                return Err(format!("#{id}: a coordinate is too far from the origin"));
            }
            *o = nm + 0.0;
        }
        Ok(out)
    }

    fn direction(&self, id: usize) -> Result<P3, String> {
        let e = self.typed(id, &["DIRECTION"])?;
        let c = self.coordinates(&e.args, id)?;
        let length = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        if length <= 0.0 || !length.is_finite() {
            return Err(format!("#{id}: a direction has no length"));
        }
        Ok([c[0] / length, c[1] / length, c[2] / length])
    }

    fn vertex(&self, id: usize) -> Result<P3, String> {
        let e = self.typed(id, &["VERTEX_POINT"])?;
        let p = Self::reference(&e.args, 1, id, "VERTEX_POINT")?;
        self.point_nm(p)
    }

    /// The corners of a loop in the order it is walked.
    fn loop_points(&self, id: usize) -> Result<Vec<P3>, String> {
        let e = self.entity(id)?;
        match e.name.as_str() {
            "POLY_LOOP" => Self::references(&e.args, 1, id, "POLY_LOOP")?
                .into_iter()
                .map(|p| self.point_nm(p))
                .collect(),
            "EDGE_LOOP" => {
                let oriented = Self::references(&e.args, 1, id, "EDGE_LOOP")?;
                let mut corners: Vec<P3> = Vec::new();
                let mut last_end: Option<P3> = None;
                for o in oriented {
                    let oe = self.typed(o, &["ORIENTED_EDGE"])?;
                    let edge = Self::reference(&oe.args, 3, o, "ORIENTED_EDGE")?;
                    let forward = Self::flag(&oe.args, 4);
                    let mut along = self.edge_points(edge)?;
                    if !forward {
                        along.reverse();
                    }
                    let (Some(&start), Some(end)) = (along.first(), along.pop()) else {
                        return Err(format!("#{edge}: the edge has no ends"));
                    };
                    if let Some(previous) = last_end {
                        if previous != start {
                            return Err(format!(
                                "#{id}: the edges of the loop do not meet end to end"
                            ));
                        }
                    }
                    let left = self.corners_left.get().checked_sub(along.len()).ok_or(
                        "the file has more corners than the triangles that the settings allow",
                    )?;
                    self.corners_left.set(left);
                    corners.append(&mut along);
                    last_end = Some(end);
                }
                if corners.first().copied() != last_end {
                    return Err(format!("#{id}: the loop is not closed"));
                }
                Ok(corners)
            }
            other => Err(format!(
                "#{id}: a bound of {} is not read, only EDGE_LOOP and POLY_LOOP",
                if other.is_empty() {
                    "several names"
                } else {
                    other
                }
            )),
        }
    }

    /// The corners of an edge from its first vertex to its second: the two of a straight edge,
    /// and the pieces of an arc of a circle.
    fn edge_points(&self, edge: usize) -> Result<Vec<P3>, String> {
        let ee = self.typed(edge, &["EDGE_CURVE"])?;
        let a = self.vertex(Self::reference(&ee.args, 1, edge, "EDGE_CURVE")?)?;
        let b = self.vertex(Self::reference(&ee.args, 2, edge, "EDGE_CURVE")?)?;
        let mut curve = Self::reference(&ee.args, 3, edge, "EDGE_CURVE")?;
        let mut c = self.entity(curve)?;
        // a curve that also says how it runs on its surfaces: the curve in space is its second
        // argument
        if matches!(
            c.name.as_str(),
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
        ) {
            curve = Self::reference(&c.args, 1, curve, &c.name)?;
            c = self.entity(curve)?;
        }
        match c.name.as_str() {
            "LINE" => Ok(vec![a, b]),
            "CIRCLE" => {
                let placement = Self::reference(&c.args, 1, curve, "CIRCLE")?;
                let radius = self.length(&c.args, 2, curve)?;
                let frame = self.frame(placement)?;
                // whether the edge goes the way its curve does
                let with_axis = Self::flag(&ee.args, 4);
                step_curved::arc_points(&frame, radius, a, b, with_axis, self.segments)
                    .map_err(|e| format!("#{edge}: {e}"))
            }
            other => Err(format!(
                "#{edge}: the edge lies on {}, and only straight and circular edges are read",
                if other.is_empty() { "a curve" } else { other }
            )),
        }
    }

    /// A length that is an argument of an entity, in nanometres.
    fn length(&self, args: &[Arg], at: usize, id: usize) -> Result<f64, String> {
        match args.get(at) {
            Some(Arg::Num(n)) if (n * self.unit_nm).abs() <= FAR_NM => Ok(n * self.unit_nm),
            _ => Err(format!("#{id}: argument {} is not a length", at + 1)),
        }
    }

    /// The place and the directions of an `AXIS2_PLACEMENT_3D`.
    fn frame(&self, id: usize) -> Result<Frame, String> {
        let p = self.typed(id, &["AXIS2_PLACEMENT_3D"])?;
        let origin = self.point_nm(Self::reference(&p.args, 1, id, "AXIS2_PLACEMENT_3D")?)?;
        let direction = |at: usize| match p.args.get(at) {
            Some(Arg::Ref(r)) => self.direction(*r).map(Some),
            _ => Ok(None),
        };
        Frame::new(origin, direction(2)?, direction(3)?)
            .ok_or_else(|| format!("#{id}: the two directions of the placement are one"))
    }

    /// The axis of a cylinder or a cone, its radius at the origin of the axis and how much the
    /// radius grows for each unit of height.
    fn curved_surface(&self, surface: usize) -> Result<(Frame, f64, f64), String> {
        let e = self.entity(surface)?;
        let placement = Self::reference(&e.args, 1, surface, &e.name)?;
        let radius = self.length(&e.args, 2, surface)?;
        let slope = if e.name == "CONICAL_SURFACE" {
            match e.args.get(3) {
                Some(Arg::Num(a)) if (a * self.angle).abs() < TAU / 4.0 => (a * self.angle).tan(),
                _ => return Err(format!("#{surface}: the angle of the cone is not one")),
            }
        } else {
            0.0
        };
        Ok((self.frame(placement)?, radius, slope))
    }

    /// The normal of the plane of a face, or an error naming what the surface is.
    fn plane_normal(&self, surface: usize, face: usize) -> Result<P3, String> {
        let e = self.entity(surface)?;
        if e.name != "PLANE" {
            let kind = if SURFACES.contains(&e.name.as_str()) {
                e.name.as_str()
            } else if e.name.is_empty() {
                "a surface of several names"
            } else {
                e.name.as_str()
            };
            return Err(format!(
                "#{face}: the face lies on {kind} (#{surface}), and only planes, cylinders and \
                 cones are read"
            ));
        }
        let placement = Self::reference(&e.args, 1, surface, "PLANE")?;
        let p = self.typed(placement, &["AXIS2_PLACEMENT_3D"])?;
        let axis = Self::reference(&p.args, 2, placement, "AXIS2_PLACEMENT_3D")?;
        self.direction(axis)
    }
}

fn newell(points: &[P3]) -> P3 {
    let mut n = [0.0f64; 3];
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

fn norm(v: P3) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Adds the triangles of one face to the builder of its shell.
fn add_face(
    r: &Reader<'_>,
    b: &mut Builder,
    id: usize,
    faces_seen: &mut usize,
    limits: &StepLimits,
) -> Result<(), String> {
    *faces_seen += 1;
    if *faces_seen > limits.max_faces {
        return Err(format!(
            "the file has more than the {} faces that the settings allow",
            limits.max_faces
        ));
    }
    let e = r.typed(id, &["ADVANCED_FACE", "FACE_SURFACE", "FACE"])?;
    let bounds = Reader::references(&e.args, 1, id, &e.name)?;
    let sense = if e.name == "FACE" {
        true
    } else {
        Reader::flag(&e.args, 3)
    };
    let surface = if e.name == "FACE" {
        None
    } else {
        Some(Reader::reference(&e.args, 2, id, &e.name)?)
    };
    if let Some(s) = surface {
        let kind = r.entity(s)?.name.clone();
        if kind == "CYLINDRICAL_SURFACE" || kind == "CONICAL_SURFACE" {
            return add_curved_face(r, b, id, s, &bounds, sense)
                .map_err(|e| format!("#{id}: the face on a {kind} (#{s}): {e}"));
        }
    }
    let plane = match surface {
        Some(s) => Some(r.plane_normal(s, id)?),
        None => None,
    };
    let mut loops: Vec<(bool, Vec<P3>)> = Vec::new();
    for bound in bounds {
        let be = r.typed(bound, &["FACE_OUTER_BOUND", "FACE_BOUND"])?;
        let outer = be.name == "FACE_OUTER_BOUND";
        let l = Reader::reference(&be.args, 1, bound, &be.name)?;
        let mut points = r.loop_points(l)?;
        if !Reader::flag(&be.args, 2) {
            points.reverse();
        }
        // a closing corner repeated at the end of a polygon is the same corner
        if points.len() > 1 && points.first() == points.last() {
            points.pop();
        }
        if points.len() < 3 {
            return Err(format!(
                "#{id}: a bound of the face has fewer than three corners"
            ));
        }
        loops.push((outer, points));
    }
    if loops.is_empty() {
        return Err(format!("#{id}: the face has no bounds"));
    }
    // the outer bound is the one that says so, or else the one with the most area
    let outer_at = loops.iter().position(|(o, _)| *o).unwrap_or_else(|| {
        (0..loops.len())
            .max_by(|&i, &j| norm(newell(&loops[i].1)).total_cmp(&norm(newell(&loops[j].1))))
            .unwrap_or(0)
    });
    let outer_normal = newell(&loops[outer_at].1);
    let size = norm(outer_normal);
    if size <= 0.0 || !size.is_finite() {
        return Err(format!("#{id}: the face has no area"));
    }
    let unit = [
        outer_normal[0] / size,
        outer_normal[1] / size,
        outer_normal[2] / size,
    ];
    let origin = loops[outer_at].1[0];
    for (_, points) in &loops {
        for p in points {
            let d = (p[0] - origin[0]) * unit[0]
                + (p[1] - origin[1]) * unit[1]
                + (p[2] - origin[2]) * unit[2];
            if d.abs() > FLAT_NM {
                return Err(format!("#{id}: the face is not flat"));
            }
        }
    }
    let normal = match plane {
        Some(n) => {
            let n = if sense { n } else { [-n[0], -n[1], -n[2]] };
            // the plane and the corners must agree on the direction of the face
            if n[0] * unit[0] + n[1] * unit[1] + n[2] * unit[2] == 0.0 {
                return Err(format!(
                    "#{id}: the plane of the face is not the plane of its corners"
                ));
            }
            n
        }
        None => unit,
    };
    let mut region: Vec<Loop> = Vec::new();
    for (i, (_, points)) in loops.iter().enumerate() {
        let ids = b.ring(points);
        let role = if i == outer_at {
            Role::Boundary
        } else {
            Role::Cut
        };
        region.push(Loop::new(ids, role, &format!("a bound of the face #{id}")));
    }
    b.plane(&region, normal).map_err(|e| format!("#{id}: {e}"))
}

/// Adds the triangles of a face on a cylinder or a cone.
fn add_curved_face(
    r: &Reader<'_>,
    b: &mut Builder,
    id: usize,
    surface: usize,
    bounds: &[usize],
    sense: bool,
) -> Result<(), String> {
    let (frame, radius, slope) = r.curved_surface(surface)?;
    let mut loops: Vec<Vec<Corner>> = Vec::with_capacity(bounds.len());
    for &bound in bounds {
        let be = r.typed(bound, &["FACE_OUTER_BOUND", "FACE_BOUND"])?;
        let l = Reader::reference(&be.args, 1, bound, &be.name)?;
        let mut points = r.loop_points(l)?;
        if points.len() > 1 && points.first() == points.last() {
            points.pop();
        }
        let mut corners = Vec::with_capacity(points.len());
        for p in points {
            let (angle, radial, height) = frame.coordinates(p);
            if (radial - (radius + slope * height)).abs() > ON_NM {
                return Err(format!("a corner of #{id} is not on the surface"));
            }
            corners.push(Corner {
                id: b.vertex(p),
                angle: (radial > ON_NM).then_some(angle),
                height,
            });
        }
        loops.push(corners);
    }
    let widest = 2.5 * TAU / f64::from(r.segments.max(3));
    for [one, two, three] in step_curved::band(&loops, widest)? {
        // the pieces face away from the axis; a face of the other sense faces towards it
        let (two, three) = if sense { (two, three) } else { (three, two) };
        b.triangle(one, two, three).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The triangles of one shell, turned so that its normals point out of the material (`outward`)
/// or into a void (`!outward`).
fn shell_mesh(
    r: &Reader<'_>,
    shell: usize,
    outward: bool,
    faces_seen: &mut usize,
    triangles_left: &mut usize,
    limits: &StepLimits,
) -> Result<Mesh, String> {
    let mut id = shell;
    let mut e = r.entity(id)?;
    if e.name == "ORIENTED_CLOSED_SHELL" {
        id = Reader::reference(&e.args, 2, id, "ORIENTED_CLOSED_SHELL")?;
        e = r.entity(id)?;
    }
    match e.name.as_str() {
        "CLOSED_SHELL" => {}
        "OPEN_SHELL" | "ORIENTED_OPEN_SHELL" => {
            return Err(format!("#{id}: an open shell is not a solid"));
        }
        other => {
            return Err(format!(
                "#{id}: {} where a closed shell was expected",
                if other.is_empty() {
                    "an entity of several names"
                } else {
                    other
                }
            ));
        }
    }
    let faces = Reader::references(&e.args, 1, id, "CLOSED_SHELL")?;
    if faces.is_empty() {
        return Err(format!("#{id}: the shell has no faces"));
    }
    let mut builder = Builder::new(*triangles_left);
    for face in faces {
        add_face(r, &mut builder, face, faces_seen, limits)?;
    }
    let mut mesh = builder.finish();
    *triangles_left = triangles_left.saturating_sub(mesh.triangles.len());
    let volume = mesh.volume_nm3();
    if (volume < 0.0) == outward {
        for t in &mut mesh.triangles {
            t.swap(1, 2);
        }
    }
    Ok(mesh)
}

/// The names of the solids of a file that are not a boundary representation.
const OTHER_SOLIDS: [&str; 7] = [
    "SHELL_BASED_SURFACE_MODEL",
    "GEOMETRIC_SET",
    "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION",
    "TESSELLATED_SOLID",
    "TESSELLATED_SHELL",
    "CSG_SOLID",
    "SWEPT_FACE_SOLID",
];

/// The solids of a STEP file with the unit read and the limits kept; see the module.
pub fn read_step(text: &str, limits: &StepLimits) -> Result<Vec<Body>, String> {
    if text.len() > limits.max_bytes {
        return Err(format!(
            "the file is larger than the {} KiB that the settings allow",
            limits.max_bytes / 1024
        ));
    }
    if !text.trim_start().starts_with("ISO-10303-21") {
        return Err("the file is not a STEP file (it does not start with ISO-10303-21)".to_owned());
    }
    let map = entities(text, limits)?;
    let reader = Reader {
        map: &map,
        unit_nm: length_unit(&map)?,
        angle: angle_unit(&map),
        segments: limits.segments_per_turn,
        corners_left: Cell::new(limits.max_triangles.saturating_mul(6).saturating_add(64)),
    };
    let solids: Vec<usize> = map
        .iter()
        .filter(|(_, e)| {
            matches!(
                e.name.as_str(),
                "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS" | "FACETED_BREP"
            )
        })
        .map(|(id, _)| *id)
        .collect();
    if solids.is_empty() {
        if let Some((id, e)) = map
            .iter()
            .find(|(_, e)| OTHER_SOLIDS.contains(&e.name.as_str()))
        {
            return Err(format!(
                "#{id}: {} is not a boundary representation, and only those are read",
                e.name
            ));
        }
        return Err(
            "the file has no solid (MANIFOLD_SOLID_BREP, BREP_WITH_VOIDS or FACETED_BREP)"
                .to_owned(),
        );
    }
    let mut bodies: Vec<Body> = Vec::new();
    let (mut faces_seen, mut triangles_left) = (0usize, limits.max_triangles);
    for id in solids {
        let e = &map[&id];
        let name = match e.args.first() {
            Some(Arg::Text(t)) if !t.trim().is_empty() => t.trim().to_owned(),
            _ => "solid".to_owned(),
        };
        let outer = Reader::reference(&e.args, 1, id, &e.name)?;
        let mut parts = vec![(outer, true)];
        if e.name == "BREP_WITH_VOIDS" {
            for v in Reader::references(&e.args, 2, id, "BREP_WITH_VOIDS")? {
                parts.push((v, false));
            }
        }
        let mut mesh = Mesh::default();
        for (shell, outward) in parts {
            let part = shell_mesh(
                &reader,
                shell,
                outward,
                &mut faces_seen,
                &mut triangles_left,
                limits,
            )?;
            let base = mesh.vertices.len() as u32;
            mesh.vertices.extend(part.vertices);
            mesh.triangles.extend(
                part.triangles
                    .iter()
                    .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
            );
        }
        bodies.push((name, mesh));
    }
    Ok(bodies)
}

#[cfg(test)]
mod tests {
    use super::round_trip_tests::{polygon_area, volume_mm3};
    use super::*;

    fn limits() -> StepLimits {
        StepLimits::from_settings(&crate::config::Settings::defaults().mesh)
    }

    /// A file of one entity per element of the list, with the header and the unit.
    fn file(unit: &str, data: &[&str]) -> String {
        let mut s =
            String::from("ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nENDSEC;\nDATA;\n");
        s.push_str(unit);
        s.push('\n');
        for d in data {
            s.push_str(d);
            s.push('\n');
        }
        s.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
        s
    }

    const MM: &str = "#900=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));";

    /// A faceted solid whose faces are the given point lists (each a polygon).
    fn faceted(points: &[[f64; 3]], faces: &[&[usize]]) -> Vec<String> {
        let mut out = Vec::new();
        for (i, p) in points.iter().enumerate() {
            out.push(format!(
                "#{}=CARTESIAN_POINT('',({:?},{:?},{:?}));",
                10 + i,
                p[0],
                p[1],
                p[2]
            ));
        }
        let mut face_ids = Vec::new();
        for (k, f) in faces.iter().enumerate() {
            let list: Vec<String> = f.iter().map(|i| format!("#{}", 10 + i)).collect();
            out.push(format!("#{}=POLY_LOOP('',({}));", 100 + k, list.join(",")));
            out.push(format!(
                "#{}=FACE_OUTER_BOUND('',#{},.T.);",
                200 + k,
                100 + k
            ));
            out.push(format!("#{}=FACE('',(#{}));", 300 + k, 200 + k));
            face_ids.push(format!("#{}", 300 + k));
        }
        out.push(format!("#500=CLOSED_SHELL('',({}));", face_ids.join(",")));
        out.push("#501=FACETED_BREP('box',#500);".to_owned());
        out
    }

    fn cube_points() -> Vec<[f64; 3]> {
        let mut p = Vec::new();
        for z in [0.0, 10.0] {
            for y in [0.0, 10.0] {
                for x in [0.0, 10.0] {
                    p.push([x, y, z]);
                }
            }
        }
        p
    }

    // corners are numbered x + 2y + 4z; each face is listed counter-clockwise from outside
    const CUBE_FACES: [&[usize]; 6] = [
        &[0, 2, 3, 1],
        &[4, 5, 7, 6],
        &[0, 1, 5, 4],
        &[2, 6, 7, 3],
        &[0, 4, 6, 2],
        &[1, 3, 7, 5],
    ];

    #[test]
    fn a_faceted_cube_is_read_with_its_volume_and_a_closed_surface() {
        let data = faceted(&cube_points(), &CUBE_FACES);
        let refs: Vec<&str> = data.iter().map(String::as_str).collect();
        let bodies = read_step(&file(MM, &refs), &limits()).unwrap();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].0, "box");
        let mesh = &bodies[0].1;
        assert!(mesh.check_closed().is_ok());
        assert_eq!(mesh.volume_nm3(), 1.0e21);
        assert_eq!(mesh.triangles.len(), 12);
        assert_eq!(mesh.vertices.len(), 8);
    }

    #[test]
    fn a_solid_turned_inside_out_in_the_file_is_read_with_its_normals_outward() {
        let reversed: Vec<Vec<usize>> = CUBE_FACES
            .iter()
            .map(|f| f.iter().rev().copied().collect())
            .collect();
        let slices: Vec<&[usize]> = reversed.iter().map(Vec::as_slice).collect();
        let data = faceted(&cube_points(), &slices);
        let refs: Vec<&str> = data.iter().map(String::as_str).collect();
        let bodies = read_step(&file(MM, &refs), &limits()).unwrap();
        assert!(bodies[0].1.volume_nm3() > 0.0);
        assert!(bodies[0].1.check_closed().is_ok());
    }

    #[test]
    fn the_unit_of_the_file_scales_every_coordinate() {
        let data = faceted(&cube_points(), &CUBE_FACES);
        let refs: Vec<&str> = data.iter().map(String::as_str).collect();
        let volume = |unit: &str| {
            read_step(&file(unit, &refs), &limits()).unwrap()[0]
                .1
                .volume_nm3()
        };
        let near = |a: f64, b: f64| (a / b - 1.0).abs() < 1e-12;
        assert!(near(volume(MM), 1.0e21));
        assert!(near(
            volume("#900=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.CENTI.,.METRE.));"),
            1.0e24
        ));
        assert!(near(
            volume("#900=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));"),
            1.0e30
        ));
        assert!(near(
            volume("#900=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MICRO.,.METRE.));"),
            1.0e12
        ));
        // no unit at all is millimetres
        assert_eq!(volume(""), 1.0e21);
        let inch =
            volume("#900=(CONVERSION_BASED_UNIT('INCH',#901)LENGTH_UNIT()NAMED_UNIT(#902));");
        assert!((inch / (10.0f64 * 25.4e6).powi(3) - 1.0).abs() < 1e-12);
        assert!(read_step(
            &file(
                "#900=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.GIGA.,.METRE.));",
                &refs
            ),
            &limits()
        )
        .unwrap_err()
        .contains("unit of length"));
    }

    #[test]
    fn what_cannot_be_read_is_refused_with_the_entity_and_its_name() {
        let cylinder = file(
            MM,
            &[
                "#10=CARTESIAN_POINT('',(0.,0.,0.));",
                "#11=DIRECTION('',(0.,0.,1.));",
                "#12=DIRECTION('',(1.,0.,0.));",
                "#13=AXIS2_PLACEMENT_3D('',#10,#11,#12);",
                "#14=SPHERICAL_SURFACE('',#13,5.);",
                "#15=POLY_LOOP('',(#10,#10,#10));",
                "#16=FACE_OUTER_BOUND('',#15,.T.);",
                "#17=ADVANCED_FACE('',(#16),#14,.T.);",
                "#18=CLOSED_SHELL('',(#17));",
                "#19=MANIFOLD_SOLID_BREP('pipe',#18);",
            ],
        );
        let error = read_step(&cylinder, &limits()).unwrap_err();
        assert!(
            error.contains("#17") && error.contains("SPHERICAL_SURFACE"),
            "{error}"
        );
        for kind in [
            "SURFACE_OF_REVOLUTION",
            "SURFACE_OF_LINEAR_EXTRUSION",
            "TOROIDAL_SURFACE",
            "B_SPLINE_SURFACE_WITH_KNOTS",
        ] {
            let text = cylinder.replace("SPHERICAL_SURFACE", kind);
            let error = read_step(&text, &limits()).unwrap_err();
            assert!(error.contains(kind), "{error}");
        }
        let open = file(
            MM,
            &[
                "#10=OPEN_SHELL('',());",
                "#11=MANIFOLD_SOLID_BREP('s',#10);",
            ],
        );
        assert!(read_step(&open, &limits())
            .unwrap_err()
            .contains("open shell"));
        let none = file(MM, &["#10=CARTESIAN_POINT('',(0.,0.,0.));"]);
        assert!(read_step(&none, &limits())
            .unwrap_err()
            .contains("no solid"));
        let surface_model = file(MM, &["#10=SHELL_BASED_SURFACE_MODEL('s',());"]);
        assert!(read_step(&surface_model, &limits())
            .unwrap_err()
            .contains("SHELL_BASED_SURFACE_MODEL"));
        assert!(read_step("not a step file", &limits())
            .unwrap_err()
            .contains("not a STEP file"));
        assert!(read_step("ISO-10303-21;\nHEADER;\nENDSEC;\n", &limits())
            .unwrap_err()
            .contains("DATA"));
    }

    #[test]
    fn the_limits_of_the_settings_refuse_a_file_before_it_is_read() {
        let data = faceted(&cube_points(), &CUBE_FACES);
        let refs: Vec<&str> = data.iter().map(String::as_str).collect();
        let text = file(MM, &refs);
        assert!(read_step(&text, &limits()).is_ok());
        let mut small = limits();
        small.max_bytes = text.len() - 1;
        assert!(read_step(&text, &small).unwrap_err().contains("KiB"));
        let mut few = limits();
        few.max_entities = refs.len();
        assert!(read_step(&text, &few).unwrap_err().contains("entities"));
        few.max_entities = refs.len() + 1;
        assert!(read_step(&text, &few).is_ok());
        let mut faces = limits();
        faces.max_faces = 5;
        assert!(read_step(&text, &faces).unwrap_err().contains("faces"));
        faces.max_faces = 6;
        assert!(read_step(&text, &faces).is_ok());
        let mut triangles = limits();
        triangles.max_triangles = 11;
        assert!(read_step(&text, &triangles)
            .unwrap_err()
            .contains("triangles"));
        triangles.max_triangles = 12;
        assert!(read_step(&text, &triangles).is_ok());
    }

    #[test]
    fn a_face_that_is_not_flat_or_a_loop_that_is_not_closed_is_refused() {
        let mut points = cube_points();
        points[7][2] += 0.5;
        let data = faceted(&points, &CUBE_FACES);
        let refs: Vec<&str> = data.iter().map(String::as_str).collect();
        assert!(read_step(&file(MM, &refs), &limits())
            .unwrap_err()
            .contains("not flat"));
        let two = file(
            MM,
            &[
                "#10=CARTESIAN_POINT('',(0.,0.,0.));",
                "#11=CARTESIAN_POINT('',(1.,0.,0.));",
                "#12=POLY_LOOP('',(#10,#11));",
                "#13=FACE_OUTER_BOUND('',#12,.T.);",
                "#14=FACE('',(#13));",
                "#15=CLOSED_SHELL('',(#14));",
                "#16=FACETED_BREP('s',#15);",
            ],
        );
        assert!(read_step(&two, &limits())
            .unwrap_err()
            .contains("fewer than three"));
        let missing = file(
            MM,
            &["#15=CLOSED_SHELL('',(#14));", "#16=FACETED_BREP('s',#15);"],
        );
        assert!(read_step(&missing, &limits())
            .unwrap_err()
            .contains("never defined"));
        let twice = file(
            MM,
            &[
                "#1=CARTESIAN_POINT('',(0.,0.,0.));",
                "#1=CARTESIAN_POINT('',(0.,0.,0.));",
            ],
        );
        assert!(read_step(&twice, &limits()).unwrap_err().contains("twice"));
        let far = file(
            MM,
            &[
                "#1=CARTESIAN_POINT('',(1.E+30,0.,0.));",
                "#2=VERTEX_POINT('',#1);",
            ],
        );
        // a coordinate that far is refused only where it is used, and the file has no solid
        assert!(read_step(&far, &limits()).unwrap_err().contains("no solid"));
    }

    #[test]
    fn comments_texts_with_semicolons_and_the_long_numbers_of_other_programs_are_read() {
        let data = faceted(&cube_points(), &CUBE_FACES);
        let mut text = file(MM, &data.iter().map(String::as_str).collect::<Vec<_>>());
        text = text.replace("#501=FACETED_BREP('box',#500);", "/* a comment; with a semicolon */\n#501=FACETED_BREP('it''s; a box',#500); /* another */");
        text = text.replace("(0.0,0.0,0.0)", "(0.0000000000E+00,+0.,-0.)");
        let bodies = read_step(&text, &limits()).unwrap();
        assert_eq!(bodies[0].0, "it's; a box");
        assert_eq!(bodies[0].1.volume_nm3(), 1.0e21);
    }

    /// A solid of a circle of radius 5 at `z = 0` and what closes it above, written by hand: a
    /// cylinder of height 10 (a seam edge and two caps), or a cone with its point.
    fn turned(top_radius: f64, semi_angle: &str, unit: &str) -> String {
        let mut data = vec![
            "#1=CARTESIAN_POINT('',(0.,0.,0.));".to_owned(),
            "#2=CARTESIAN_POINT('',(0.,0.,10.));".to_owned(),
            "#3=DIRECTION('',(0.,0.,1.));".to_owned(),
            "#4=DIRECTION('',(1.,0.,0.));".to_owned(),
            "#5=AXIS2_PLACEMENT_3D('',#1,#3,#4);".to_owned(),
            "#6=AXIS2_PLACEMENT_3D('',#2,#3,#4);".to_owned(),
            "#7=CIRCLE('',#5,5.);".to_owned(),
            "#10=CARTESIAN_POINT('',(5.,0.,0.));".to_owned(),
            "#11=VERTEX_POINT('',#10);".to_owned(),
            "#12=EDGE_CURVE('',#11,#11,#7,.T.);".to_owned(),
            "#13=PLANE('',#5);".to_owned(),
            // the floor, seen from below
            "#20=ORIENTED_EDGE('',*,*,#12,.F.);".to_owned(),
            "#21=EDGE_LOOP('',(#20));".to_owned(),
            "#22=FACE_OUTER_BOUND('',#21,.T.);".to_owned(),
            "#23=ADVANCED_FACE('',(#22),#13,.F.);".to_owned(),
            "#30=LINE('',#10,#31);".to_owned(),
            "#31=VECTOR('',#3,1.);".to_owned(),
        ];
        let mut faces = vec!["#23"];
        if top_radius > 0.0 {
            data.extend([
                format!("#40=CARTESIAN_POINT('',({top_radius:?},0.,10.));"),
                "#41=VERTEX_POINT('',#40);".to_owned(),
                format!("#42=CIRCLE('',#6,{top_radius:?});"),
                "#43=EDGE_CURVE('',#41,#41,#42,.T.);".to_owned(),
                "#44=EDGE_CURVE('',#11,#41,#30,.T.);".to_owned(),
                "#45=PLANE('',#6);".to_owned(),
                "#46=ORIENTED_EDGE('',*,*,#43,.T.);".to_owned(),
                "#47=EDGE_LOOP('',(#46));".to_owned(),
                "#48=FACE_OUTER_BOUND('',#47,.T.);".to_owned(),
                "#49=ADVANCED_FACE('',(#48),#45,.T.);".to_owned(),
                "#50=ORIENTED_EDGE('',*,*,#12,.T.);".to_owned(),
                "#51=ORIENTED_EDGE('',*,*,#44,.T.);".to_owned(),
                "#52=ORIENTED_EDGE('',*,*,#43,.F.);".to_owned(),
                "#53=ORIENTED_EDGE('',*,*,#44,.F.);".to_owned(),
                "#54=EDGE_LOOP('',(#50,#51,#52,#53));".to_owned(),
            ]);
            faces.push("#49");
        } else {
            data.extend([
                "#41=VERTEX_POINT('',#2);".to_owned(),
                "#44=EDGE_CURVE('',#11,#41,#30,.T.);".to_owned(),
                "#50=ORIENTED_EDGE('',*,*,#12,.T.);".to_owned(),
                "#51=ORIENTED_EDGE('',*,*,#44,.T.);".to_owned(),
                "#53=ORIENTED_EDGE('',*,*,#44,.F.);".to_owned(),
                "#54=EDGE_LOOP('',(#50,#51,#53));".to_owned(),
            ]);
        }
        data.push(if semi_angle.is_empty() {
            "#55=CYLINDRICAL_SURFACE('',#5,5.);".to_owned()
        } else {
            format!("#55=CONICAL_SURFACE('',#5,5.,{semi_angle});")
        });
        data.push("#56=FACE_OUTER_BOUND('',#54,.T.);".to_owned());
        data.push("#57=ADVANCED_FACE('',(#56),#55,.T.);".to_owned());
        faces.push("#57");
        data.push(format!("#60=CLOSED_SHELL('',({}));", faces.join(",")));
        data.push("#61=MANIFOLD_SOLID_BREP('turned',#60);".to_owned());
        let refs: Vec<&str> = data.iter().map(String::as_str).collect();
        file(unit, &refs)
    }

    #[test]
    fn a_cylinder_with_a_seam_a_cut_off_cone_and_a_point_are_read_closed() {
        let read = |text: &str| {
            let bodies = read_step(text, &limits()).unwrap();
            assert!(
                bodies[0].1.check_closed().is_ok(),
                "{:?}",
                bodies[0].1.check_closed()
            );
            volume_mm3(&bodies)
        };
        let disc = polygon_area(10.0, 72);
        let cylinder = read(&turned(5.0, "", MM));
        assert!((cylinder - disc * 10.0).abs() < 0.01, "{cylinder}");
        // radius 5 to 2 over 10 mm: the angle of the side is atan(-3 / 10)
        let slope = (-0.3f64).atan();
        let frustum = read(&turned(2.0, &format!("{slope:.12}"), MM));
        let small = polygon_area(4.0, 72);
        let expected = 10.0 / 3.0 * (disc + small + (disc * small).sqrt());
        assert!((frustum - expected).abs() < 0.01, "{frustum} {expected}");
        // the point: atan(-5 / 10), here in degrees
        let degrees = format!(
            "{MM}\n#901=(CONVERSION_BASED_UNIT('DEGREE',#902)NAMED_UNIT(*)PLANE_ANGLE_UNIT());"
        );
        let angle = format!("{:.12}", (-0.5f64).atan().to_degrees());
        let cone = read(&turned(0.0, &angle, &degrees));
        assert!((cone - disc * 10.0 / 3.0).abs() < 0.01, "{cone}");
        // a circle that is not on the cylinder is refused with the face
        let off = turned(5.0, "", MM).replace(
            "#55=CYLINDRICAL_SURFACE('',#5,5.);",
            "#55=CYLINDRICAL_SURFACE('',#5,6.);",
        );
        let error = read_step(&off, &limits()).unwrap_err();
        assert!(
            error.contains("#57") && error.contains("not on the surface"),
            "{error}"
        );
        // an edge on a curve that is not read is refused by its name
        let ellipse =
            turned(5.0, "", MM).replace("#7=CIRCLE('',#5,5.);", "#7=ELLIPSE('',#5,5.,4.);");
        let error = read_step(&ellipse, &limits()).unwrap_err();
        assert!(error.contains("ELLIPSE"), "{error}");
        // a curve wrapped with how it runs on its surfaces is the curve inside
        let wrapped = turned(5.0, "", MM)
            .replace(
                "#12=EDGE_CURVE('',#11,#11,#7,.T.);",
                "#12=EDGE_CURVE('',#11,#11,#8,.T.);",
            )
            .replace(
                "#7=CIRCLE('',#5,5.);",
                "#7=CIRCLE('',#5,5.);\n#8=SURFACE_CURVE('',#7,(),.CURVE_3D.);",
            );
        assert!((read(&wrapped) - disc * 10.0).abs() < 0.01);
    }
}

#[cfg(test)]
mod round_trip_tests {
    use super::*;
    use crate::enclosure::Face;
    use crate::profile::{Profile, ProfileLimits};
    use crate::sketch::{Sketch, SketchPlane};
    use crate::step::prism_to_step;
    use crate::Length;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn limits() -> StepLimits {
        StepLimits::from_settings(&crate::config::Settings::defaults().mesh)
    }

    fn polygon(s: &mut Sketch, points: &[(i64, i64)]) {
        let ids: Vec<_> = points
            .iter()
            .map(|(x, y)| s.add_point(mm(*x), mm(*y)).unwrap())
            .collect();
        for i in 0..ids.len() {
            s.add_line(ids[i], ids[(i + 1) % ids.len()]).unwrap();
        }
    }

    fn sketch(outlines: &[&[(i64, i64)]]) -> Profile {
        let mut s = Sketch::new("part", SketchPlane::on_face(Face::Front)).unwrap();
        for o in outlines {
            polygon(&mut s, o);
        }
        Profile::from_sketch(&s, &ProfileLimits::shipped()).unwrap()
    }

    #[test]
    fn a_prism_is_written_and_read_back_with_its_exact_volume_and_a_closed_surface() {
        let profile = sketch(&[&[(0, 0), (40, 0), (40, 25), (0, 25)]]);
        let text = prism_to_step("plate", &profile, 0, 3_000_000).unwrap();
        let bodies = read_step(&text, &limits()).unwrap();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].0, "plate");
        assert!(bodies[0].1.check_closed().is_ok());
        assert_eq!(bodies[0].1.volume_nm3(), 40.0 * 25.0 * 3.0 * 1.0e18);
        // 6 faces of a box: 12 triangles
        assert_eq!(bodies[0].1.triangles.len(), 12);
        // the same profile gives the same text
        assert_eq!(
            text,
            prism_to_step("plate", &profile, 0, 3_000_000).unwrap()
        );
        assert!(text.starts_with("ISO-10303-21;\nHEADER;\n"));
        assert!(text.contains("MANIFOLD_SOLID_BREP('plate',"));
    }

    #[test]
    fn a_prism_with_a_hole_and_an_outline_that_is_not_a_rectangle_round_trips() {
        // an L shape with a square hole in it
        let l: &[(i64, i64)] = &[(0, 0), (30, 0), (30, 10), (10, 10), (10, 30), (0, 30)];
        let hole: &[(i64, i64)] = &[(3, 3), (7, 3), (7, 7), (3, 7)];
        let profile = sketch(&[l, hole]);
        let text = prism_to_step("angle", &profile, 5_000_000, 8_000_000).unwrap();
        let bodies = read_step(&text, &limits()).unwrap();
        let mesh = &bodies[0].1;
        assert!(mesh.check_closed().is_ok(), "{:?}", mesh.check_closed());
        // the L is 500 mm2, the hole 16 mm2 and the prism is 8 mm high
        assert_eq!(mesh.volume_nm3(), (500.0 - 16.0) * 8.0 * 1.0e18);
        let (lo, hi) = mesh.bounds().unwrap();
        assert_eq!((lo[2], hi[2]), (5.0e6, 13.0e6));
        assert_eq!((lo[0], hi[0], lo[1], hi[1]), (0.0, 30.0e6, 0.0, 30.0e6));
        // the L has six walls and the hole four, with the floor and the roof
        assert_eq!(text.matches("ADVANCED_FACE").count(), 6 + 4 + 2);
    }

    #[test]
    fn two_regions_are_two_solids_and_a_circle_is_the_polygon_of_the_profile() {
        let mut s = Sketch::new("two", SketchPlane::on_face(Face::Front)).unwrap();
        polygon(&mut s, &[(0, 0), (10, 0), (10, 10), (0, 10)]);
        polygon(&mut s, &[(30, 0), (40, 0), (40, 10), (30, 10)]);
        let c = s.add_point(mm(60), mm(5)).unwrap();
        s.add_circle(c, mm(4)).unwrap();
        let profile = Profile::from_sketch(&s, &ProfileLimits::shipped()).unwrap();
        let text = prism_to_step("pair", &profile, 0, 2_000_000).unwrap();
        let bodies = read_step(&text, &limits()).unwrap();
        assert_eq!(bodies.len(), 3);
        let area: f64 = bodies.iter().map(|b| b.1.volume_nm3() / 2.0e6).sum();
        assert!(
            (area - profile.area_nm2()).abs() / profile.area_nm2() < 0.01,
            "{area}"
        );
        for (_, mesh) in &bodies {
            assert!(mesh.check_closed().is_ok());
        }
    }

    #[test]
    fn a_prism_with_no_height_or_a_name_with_quotes_is_handled() {
        let profile = sketch(&[&[(0, 0), (4, 0), (4, 4), (0, 4)]]);
        assert!(prism_to_step("p", &profile, 0, 0).is_err());
        assert!(prism_to_step("p", &profile, 0, -5).is_err());
        let text = prism_to_step("it's a\n\u{e9}", &profile, 0, 1_000_000).unwrap();
        assert!(text.contains("MANIFOLD_SOLID_BREP('it_s_a__',"), "{text}");
        assert_eq!(read_step(&text, &limits()).unwrap().len(), 1);
        assert!(prism_to_step("", &profile, 0, 1_000_000)
            .unwrap()
            .contains("('prism',"));
    }

    fn small_box() -> crate::enclosure::Enclosure {
        use crate::enclosure::{Enclosure, EnclosureParams};
        Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap()
    }

    // the outer box less the inner space: 104 x 64 x 35 less 100 x 60 x 30
    const WALLS_MM3: f64 = 104.0 * 64.0 * 35.0 - 100.0 * 60.0 * 30.0;

    /// The area of a circle of `segments` straight pieces, in square millimetres.
    pub(super) fn polygon_area(diameter: f64, segments: u32) -> f64 {
        let step = std::f64::consts::TAU / f64::from(segments);
        0.5 * f64::from(segments) * (diameter / 2.0).powi(2) * step.sin()
    }

    pub(super) fn volume_mm3(bodies: &[Body]) -> f64 {
        bodies[0].1.volume_nm3() / 1.0e18
    }

    #[test]
    fn the_enclosure_file_of_the_program_is_read_back_with_its_walls() {
        let e = small_box();
        let text = crate::step::export_step(&e).unwrap();
        let bodies = read_step(&text, &limits()).unwrap();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].0, "enclosure");
        assert!(
            bodies[0].1.check_closed().is_ok(),
            "{:?}",
            bodies[0].1.check_closed()
        );
        assert!((volume_mm3(&bodies) / WALLS_MM3 - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_round_holes_and_posts_of_the_export_are_read_back_closed_with_their_volume() {
        use crate::enclosure::{Face, Hole, HoleStyle, Operation, Post};
        use eh3d_container::ObjectId;
        let id = |n: u64| ObjectId::from_parts(1_700_000_000_000, 1, n).unwrap();
        let mut e = small_box();
        let faces = [
            Face::Front,
            Face::Back,
            Face::Left,
            Face::Right,
            Face::Top,
            Face::Bottom,
        ];
        let mut removed = 0.0;
        let walls = [2.0, 2.0, 2.0, 2.0, 3.0, 2.0];
        for (n, (face, wall)) in faces.into_iter().zip(walls).enumerate() {
            e.apply(Operation::AddHole {
                id: id(10 + n as u64),
                hole: Hole {
                    name: format!("h{n}"),
                    face,
                    u: mm(20),
                    v: mm(15),
                    diameter: mm(6),
                    style: HoleStyle::Plain,
                },
            })
            .unwrap();
            removed += polygon_area(6.0, 72) * wall;
        }
        e.apply(Operation::AddPost {
            id: id(30),
            post: Post {
                name: "p".into(),
                board_hole: None,
                kind: crate::mount::BoreKind::SelfTapping,
                x: mm(50),
                y: mm(30),
                outer_diameter: mm(8),
                bore_diameter: mm(3),
                height: mm(10),
                bore_depth: mm(8),
                base: None,
            },
        })
        .unwrap();
        let added = polygon_area(8.0, 72) * 10.0 - polygon_area(3.0, 72) * 8.0;
        let text = crate::step::export_step(&e).unwrap();
        let bodies = read_step(&text, &limits()).unwrap();
        let mesh = &bodies[0].1;
        assert!(mesh.check_closed().is_ok(), "{:?}", mesh.check_closed());
        let expected = WALLS_MM3 - removed + added;
        assert!(
            (volume_mm3(&bodies) - expected).abs() < 0.01,
            "{} against {expected}",
            volume_mm3(&bodies)
        );
        // the outer size is the one of the box
        for k in 0..3 {
            let high = mesh.vertices.iter().map(|v| v[k]).fold(f64::MIN, f64::max);
            let low = mesh.vertices.iter().map(|v| v[k]).fold(f64::MAX, f64::min);
            assert_eq!((low, high), (0.0, [104.0e6, 64.0e6, 35.0e6][k]));
        }
        // fewer pieces for a turn give a smaller file of triangles
        let coarse = StepLimits {
            segments_per_turn: 12,
            ..limits()
        };
        let rough = read_step(&text, &coarse).unwrap();
        assert!(rough[0].1.check_closed().is_ok());
        assert!(rough[0].1.triangles.len() < mesh.triangles.len());
        // and a file cannot ask for more triangles than are allowed
        let few = StepLimits {
            max_triangles: 100,
            ..limits()
        };
        assert!(read_step(&text, &few)
            .unwrap_err()
            .contains("triangles that the settings allow"));
    }

    #[test]
    fn an_edge_that_goes_against_its_curve_is_walked_from_its_first_vertex() {
        // the sense of the curve says nothing about which vertex an edge starts at
        let text = crate::step::export_step(&small_box()).unwrap();
        let flipped: Vec<String> = text
            .lines()
            .map(|line| {
                if line.contains("=EDGE_CURVE(") {
                    line.replace(",.T.);", ",.F.);")
                } else {
                    line.to_owned()
                }
            })
            .collect();
        let flipped = flipped.join("\n");
        assert_ne!(flipped, text);
        let bodies = read_step(&flipped, &limits()).unwrap();
        assert!(bodies[0].1.check_closed().is_ok());
        assert!((volume_mm3(&bodies) / WALLS_MM3 - 1.0).abs() < 1e-12);
    }
}

#[cfg(test)]
mod solid_properties_tests {
    use super::*;
    use crate::enclosure::Face;
    use crate::mesh::Mesh;
    use crate::profile::{Profile, ProfileLimits};
    use crate::sketch::{Sketch, SketchPlane};
    use crate::step::prism_to_step;
    use crate::Length;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn mesh_of(outlines: &[&[(i64, i64)]], z0: i64, height: i64) -> Mesh {
        let mut s = Sketch::new("p", SketchPlane::on_face(Face::Front)).unwrap();
        for o in outlines {
            let ids: Vec<_> = o
                .iter()
                .map(|(x, y)| s.add_point(mm(*x), mm(*y)).unwrap())
                .collect();
            for i in 0..ids.len() {
                s.add_line(ids[i], ids[(i + 1) % ids.len()]).unwrap();
            }
        }
        let profile = Profile::from_sketch(&s, &ProfileLimits::shipped()).unwrap();
        let text = prism_to_step("p", &profile, z0, height).unwrap();
        let limits = StepLimits::from_settings(&crate::config::Settings::defaults().mesh);
        read_step(&text, &limits).unwrap().remove(0).1
    }

    const BOX: &[(i64, i64)] = &[(0, 0), (30, 0), (30, 20), (0, 20)];
    const L: &[(i64, i64)] = &[(0, 0), (30, 0), (30, 10), (10, 10), (10, 30), (0, 30)];

    fn near(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(a.abs())
    }

    #[test]
    fn the_inertia_of_a_box_is_the_formula_whatever_the_place_and_the_side_it_is_turned() {
        // 30 x 20 x 10 mm, high above the origin so that the shift to the centre is tested
        let mesh = mesh_of(&[BOX], 7_000_000, 10_000_000);
        let tensor = mesh.inertia_nm5().unwrap();
        let v = 30.0 * 20.0 * 10.0 * 1.0e18;
        let formula = |a: f64, b: f64| v * (a * a + b * b) / 12.0 * 1.0e12;
        assert!(near(tensor[0][0], formula(20.0, 10.0), 1e-9), "{tensor:?}");
        assert!(near(tensor[1][1], formula(30.0, 10.0), 1e-9), "{tensor:?}");
        assert!(near(tensor[2][2], formula(30.0, 20.0), 1e-9), "{tensor:?}");
        for (i, j) in [(0, 1), (0, 2), (1, 2)] {
            assert!(tensor[i][j].abs() < 1e-9 * tensor[0][0], "{tensor:?}");
            assert_eq!(tensor[i][j], tensor[j][i]);
        }
        // a mesh turned inside out has the same tensor
        let mut inside_out = mesh.clone();
        for t in &mut inside_out.triangles {
            t.swap(1, 2);
        }
        assert_eq!(inside_out.inertia_nm5().unwrap(), tensor);
        assert!(Mesh::default().inertia_nm5().is_none());
    }

    #[test]
    fn the_inertia_of_an_l_follows_the_parallel_axis_theorem_of_its_two_boxes() {
        let mesh = mesh_of(&[L], 0, 4_000_000);
        let c = mesh.centroid_nm().unwrap();
        // the L is 300 mm2 below and 200 mm2 beside it, 4 mm thick: its centre is at 11, 11, 2
        assert!(
            near(c[0], 11.0e6, 1e-9) && near(c[1], 11.0e6, 1e-9) && near(c[2], 2.0e6, 1e-9),
            "{c:?}"
        );
        let tensor = mesh.inertia_nm5().unwrap();
        // about the vertical axis through the centre: each box about its own centre plus its
        // volume times the square of the distance between the two centres
        let box_izz = |w: f64, d: f64, cx: f64, cy: f64| {
            let v = w * d * 4.0;
            v * (w * w + d * d) / 12.0 + v * ((cx - 11.0).powi(2) + (cy - 11.0).powi(2))
        };
        let expected =
            (box_izz(30.0, 10.0, 15.0, 5.0) + box_izz(10.0, 20.0, 5.0, 20.0)) * 1.0e18 * 1.0e12;
        assert!(
            near(tensor[2][2], expected, 1e-9),
            "{} against {expected}",
            tensor[2][2]
        );
        // the product of inertia of the plane of the L is not zero
        assert!(tensor[0][1].abs() > 1e-3 * tensor[2][2]);
    }

    #[test]
    fn a_prism_gives_back_its_axis_height_floor_and_outlines() {
        let mesh = mesh_of(
            &[L, &[(3, 3), (7, 3), (7, 7), (3, 7)]],
            5_000_000,
            8_000_000,
        );
        let prism = mesh.as_prism().unwrap();
        assert_eq!((prism.axis, prism.low, prism.height), (2, 5.0e6, 8.0e6));
        assert_eq!(prism.area_nm2, (500.0 - 16.0) * 1.0e12);
        assert_eq!(prism.outlines.len(), 2);
        let mut corners: Vec<usize> = prism.outlines.iter().map(Vec::len).collect();
        corners.sort_unstable();
        assert_eq!(corners, vec![4, 6]);
        // the same solid along the first axis: what was x and y is now y and z
        let turned = mesh.mapped(|v| [v[2], v[0], v[1]]);
        let along_x = turned.as_prism().unwrap();
        assert_eq!(
            (along_x.axis, along_x.low, along_x.height),
            (0, 5.0e6, 8.0e6)
        );
        assert_eq!(along_x.area_nm2, prism.area_nm2);
        let mut a: Vec<[f64; 2]> = prism.outlines.concat();
        let mut b: Vec<[f64; 2]> = along_x.outlines.concat();
        a.sort_by(|p, q| p.partial_cmp(q).unwrap());
        b.sort_by(|p, q| p.partial_cmp(q).unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn a_solid_that_is_not_a_prism_is_told_why() {
        // a wall that leans by a nanometre is not a wall
        let mut mesh = mesh_of(&[BOX], 0, 5_000_000);
        let top = mesh.vertices.iter().position(|v| v[2] > 0.0).unwrap();
        mesh.vertices[top][0] += 1.0;
        assert!(mesh.as_prism().unwrap_err().contains("vertical wall"));
        // a box with an inner space has more than two planes
        let (lo, hi) = (0.0f64, 10.0e6);
        let mut cube = Mesh::default();
        for z in [lo, hi] {
            for y in [lo, hi] {
                for x in [lo, hi] {
                    cube.vertices.push([x, y, z]);
                }
            }
        }
        cube.vertices.push([5.0e6, 5.0e6, 5.0e6]);
        assert!(cube.as_prism().is_err());
        assert!(Mesh::default().as_prism().is_err());
    }
}

#[cfg(test)]
mod seeded_tests {
    use super::*;
    use crate::enclosure::Face;
    use crate::profile::{Profile, ProfileLimits};
    use crate::sketch::{Sketch, SketchPlane};
    use crate::step::prism_to_step;
    use crate::Length;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn limits() -> StepLimits {
        StepLimits::from_settings(&crate::config::Settings::defaults().mesh)
    }

    /// The next number of a fixed sequence (xorshift), so that a failure can be repeated.
    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    fn polygon(s: &mut Sketch, points: &[(i64, i64)]) {
        let ids: Vec<_> = points
            .iter()
            .map(|(x, y)| s.add_point(mm(*x), mm(*y)).unwrap())
            .collect();
        for i in 0..ids.len() {
            s.add_line(ids[i], ids[(i + 1) % ids.len()]).unwrap();
        }
    }

    /// A star-shaped outline round a centre: evenly spaced angles with a little jitter and radii
    /// from 20 to 60 mm, rounded to whole millimetres (some turn out not simple and are refused).
    fn star(state: &mut u64) -> Vec<(i64, i64)> {
        let n = 6 + (next(state) % 7) as usize;
        (0..n)
            .map(|k| {
                let jitter = (next(state) % 600) as f64 / 1000.0 - 0.3;
                let angle = (k as f64 + jitter) * std::f64::consts::TAU / n as f64;
                let radius = 20.0 + (next(state) % 41) as f64;
                (
                    (radius * angle.cos()).round() as i64,
                    (radius * angle.sin()).round() as i64,
                )
            })
            .collect()
    }

    #[test]
    fn random_prisms_are_written_and_read_back_with_their_volume_and_prism_shape_with_a_fixed_seed()
    {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let (mut tried, mut skipped, mut holes) = (0, 0, 0);
        for _ in 0..300 {
            let outline = star(&mut state);
            let hole = next(&mut state).is_multiple_of(2);
            let mut s = Sketch::new("r", SketchPlane::on_face(Face::Front)).unwrap();
            polygon(&mut s, &outline);
            if hole {
                polygon(&mut s, &[(-3, -3), (3, -3), (3, 3), (-3, 3)]);
            }
            let Ok(profile) = Profile::from_sketch(&s, &ProfileLimits::shipped()) else {
                skipped += 1;
                continue;
            };
            tried += 1;
            holes += usize::from(hole);
            let z0 = (next(&mut state) % 50) as i64 * 1_000_000 - 20_000_000;
            let height = (1 + next(&mut state) % 40) as i64 * 1_000_000;
            let text = prism_to_step("r", &profile, z0, height).unwrap();
            let bodies = read_step(&text, &limits()).unwrap();
            assert_eq!(bodies.len(), profile.regions.len());
            let volume: f64 = bodies.iter().map(|b| b.1.volume_nm3()).sum();
            let expected = profile.area_nm2() * height as f64;
            assert!(
                (volume / expected - 1.0).abs() < 1e-9,
                "{volume} against {expected} for {outline:?}"
            );
            for (_, mesh) in &bodies {
                assert!(mesh.check_closed().is_ok(), "{outline:?}");
                let prism = mesh.as_prism().unwrap();
                assert_eq!(
                    (prism.axis, prism.low, prism.height),
                    (2, z0 as f64, height as f64)
                );
                assert_eq!(prism.outlines.len(), 1 + usize::from(hole));
                assert!(mesh.inertia_nm5().is_some());
            }
            // the same profile, the same text
            assert_eq!(text, prism_to_step("r", &profile, z0, height).unwrap());
        }
        // both kinds of outline happen, and nearly all the stars are simple
        assert!(tried > 250, "{tried} tried, {skipped} skipped");
        assert!(
            holes > 80 && holes + 80 < tried,
            "{holes} of {tried} have a hole"
        );
    }

    /// A damaged copy of a STEP file: a line dropped, repeated or swapped, a character changed, a
    /// piece from a pool put in, or the text cut short (some of them damaged twice).
    fn damaged(text: &str, state: &mut u64) -> String {
        const POOL: [&str; 10] = [
            "#1=CARTESIAN_POINT('',(1.E+300,0.,0.));",
            "#2=PLANE('',#999);",
            "#3=CYLINDRICAL_SURFACE('',#4,1.);",
            "(((((((((((((((((((((((((((((((((",
            "#4=EDGE_LOOP('',());",
            "#5=ADVANCED_FACE('',(),#6,.T.);",
            "#7=FACE_OUTER_BOUND('',#7,.T.);",
            "#8=CLOSED_SHELL('',(#8));",
            "'unclosed text",
            "#9=MANIFOLD_SOLID_BREP('x',#9);",
        ];
        let mut out = text.to_owned();
        for _ in 0..1 + next(state) % 2 {
            let mut lines: Vec<String> = out.lines().map(str::to_owned).collect();
            if lines.is_empty() {
                break;
            }
            let at = (next(state) % lines.len() as u64) as usize;
            match next(state) % 6 {
                0 => {
                    lines.remove(at);
                }
                1 => {
                    let copy = lines[at].clone();
                    lines.insert(at, copy);
                }
                2 => {
                    let other = (next(state) % lines.len() as u64) as usize;
                    lines.swap(at, other);
                }
                3 => {
                    let mut chars: Vec<char> = lines[at].chars().collect();
                    if !chars.is_empty() {
                        let k = (next(state) % chars.len() as u64) as usize;
                        chars[k] = (b' ' + (next(state) % 95) as u8) as char;
                    }
                    lines[at] = chars.into_iter().collect();
                }
                4 => lines.insert(
                    at,
                    POOL[(next(state) % POOL.len() as u64) as usize].to_owned(),
                ),
                _ => {
                    let keep = (next(state) % (lines.len() as u64 + 1)) as usize;
                    lines.truncate(keep);
                }
            }
            out = lines.join("\n");
        }
        out
    }

    #[test]
    fn damaged_step_files_are_read_or_refused_and_never_panic_with_a_fixed_seed() {
        let mut s = Sketch::new("d", SketchPlane::on_face(Face::Front)).unwrap();
        polygon(
            &mut s,
            &[(0, 0), (30, 0), (30, 10), (10, 10), (10, 30), (0, 30)],
        );
        polygon(&mut s, &[(3, 3), (7, 3), (7, 7), (3, 7)]);
        let profile = Profile::from_sketch(&s, &ProfileLimits::shipped()).unwrap();
        let text = prism_to_step("d", &profile, 0, 5_000_000).unwrap();
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let (mut read, mut refused) = (0, 0);
        for _ in 0..3000 {
            let copy = damaged(&text, &mut state);
            match read_step(&copy, &limits()) {
                Ok(bodies) => {
                    read += 1;
                    for (_, mesh) in &bodies {
                        for t in &mesh.triangles {
                            assert!(t.iter().all(|v| (*v as usize) < mesh.vertices.len()));
                        }
                        assert!(mesh
                            .vertices
                            .iter()
                            .flatten()
                            .all(|v| v.is_finite() && v.abs() <= 1.0e15));
                    }
                }
                Err(why) => {
                    refused += 1;
                    assert!(!why.is_empty());
                }
            }
        }
        // both outcomes happen: a line that nothing uses can go, a corner cannot
        assert!(
            read > 100 && refused > 100,
            "{read} read, {refused} refused"
        );
        // hostile shapes: lists nested very deeply, no end, no data
        let deep = format!(
            "ISO-10303-21;\nDATA;\n#1=X({});\nENDSEC;\n",
            "(".repeat(100_000)
        );
        assert!(read_step(&deep, &limits())
            .unwrap_err()
            .contains("nested too deeply"));
        let open = "ISO-10303-21;\nDATA;\n#1=X('abc;\nENDSEC;\n";
        assert!(read_step(open, &limits()).is_err());
        assert!(read_step("ISO-10303-21;", &limits()).is_err());
        assert!(read_step("", &limits()).is_err());
    }
}
