//! The board as the other branch of the suite file tells it: the artifact `outline2d` (the
//! outline, the mounting holes, the sizes, and optionally the footprints of the tall parts and the
//! openings that connectors need), and the artifact `mounts2d` that this program hands back (where
//! the posts of the box are, in the frame of the board). Both are JSON with whole nanometres in
//! the frame of the board (`y` upwards as seen from the component side), described in
//! `docs/BOARD_INTERCHANGE.md`. A file that is not valid is refused with `PHOENIX-CAD-066`; a
//! field that this version does not know is ignored.

use std::collections::BTreeSet;
use std::str::FromStr;

use eh3d_container::{sha256_hex, to_canonical_file_bytes, ObjectId};
use serde_json::{json, Value};

use crate::config::MechanicalLimits;
use crate::enclosure::Enclosure;
use crate::mount::{BoardHole, BoardInterface};
use crate::{CadError, Length};

/// The name of the artifact of the outline of a board.
pub const ARTIFACT: &str = "outline2d";

/// The version of the artifact that this program reads.
pub const ARTIFACT_VERSION: u32 = 1;

/// The branch of the board program.
pub const SOURCE_BRANCH: &str = "pcb";

/// The file of the artifact in the branch of the board program.
pub const SOURCE_ENTRY: &str = "outline2d.json";

/// The copy of the artifact that the branch `cad` was derived from.
pub const SNAPSHOT_ENTRY: &str = "board/outline2d.json";

/// The file of the posts that this program hands back, in the branch `cad`.
pub const MOUNTS_ENTRY: &str = "mounts2d.json";

/// The most footprints of parts of one board.
pub const MAX_PARTS: usize = 10_000;

/// The most connectors of one board.
pub const MAX_CONNECTORS: usize = 1_000;

/// The footprint of a part with its height: the lower left corner and the sizes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// The name shown to people.
    pub name: String,
    /// The lower left corner along X of the board.
    pub x: Length,
    /// The lower left corner along Y of the board.
    pub y: Length,
    /// The size along X.
    pub width: Length,
    /// The size along Y.
    pub depth: Length,
    /// The height above its face of the board.
    pub height: Length,
    /// Whether it is under the board.
    pub bottom: bool,
}

/// The side of the board that a connector looks out of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// The side at the smallest Y.
    Front,
    /// The side at the largest Y.
    Back,
    /// The side at the smallest X.
    Left,
    /// The side at the largest X.
    Right,
}

impl Edge {
    /// The four sides.
    pub const ALL: [Edge; 4] = [Edge::Front, Edge::Back, Edge::Left, Edge::Right];

    /// A stable lower-case name.
    pub fn name(self) -> &'static str {
        match self {
            Edge::Front => "front",
            Edge::Back => "back",
            Edge::Left => "left",
            Edge::Right => "right",
        }
    }
}

/// A connector that needs an opening in the wall of its side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connector {
    /// The name shown to people.
    pub name: String,
    /// The side it looks out of.
    pub edge: Edge,
    /// Where its centre is along that side: the `x` of the board for the front and the back, the
    /// `y` for the left and the right.
    pub at: Length,
    /// The width of the opening it needs.
    pub width: Length,
    /// The height of the opening it needs.
    pub height: Length,
    /// How far above the top face of the board the opening starts.
    pub above: Length,
}

/// The board as the artifact tells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardArtifact {
    /// The outline, the holes and the sizes (without a placement, which is of the box).
    pub board: BoardInterface,
    /// The footprints of the parts.
    pub parts: Vec<Part>,
    /// The connectors.
    pub connectors: Vec<Connector>,
}

fn bad(why: impl Into<String>) -> CadError {
    CadError::BoardArtifact(why.into())
}

fn length(value: Option<&Value>, what: &str) -> Result<Length, CadError> {
    let number = value
        .and_then(Value::as_i64)
        .ok_or_else(|| bad(format!("{what} is not a whole number of nanometres")))?;
    Length::new(number).map_err(|_| bad(format!("{what} is outside the range of lengths")))
}

fn positive(value: Option<&Value>, what: &str) -> Result<Length, CadError> {
    let out = length(value, what)?;
    if out.get() <= 0 {
        return Err(bad(format!("{what} is not positive")));
    }
    Ok(out)
}

fn name_of(value: Option<&Value>, limits: &MechanicalLimits) -> Result<String, CadError> {
    let name = value
        .and_then(Value::as_str)
        .ok_or_else(|| bad("a name is missing"))?;
    if name.len() > limits.max_name_bytes || name.chars().any(char::is_control) {
        return Err(bad("a name is too long or has a control character"));
    }
    Ok(name.to_owned())
}

fn list<'a>(
    root: &'a Value,
    key: &str,
    required: bool,
    most: usize,
) -> Result<&'a [Value], CadError> {
    match root.get(key) {
        None if !required => Ok(&[]),
        Some(Value::Array(items)) if items.len() <= most => Ok(items),
        Some(Value::Array(_)) => Err(bad(format!("{key} has more items than the limit"))),
        _ => Err(bad(format!("{key} is not a list"))),
    }
}

/// An identity worked out from a text, for a hole that has none of its own.
pub(crate) fn id_from(text: &str) -> Result<ObjectId, CadError> {
    let digest = sha256_hex(text.as_bytes());
    let part = |range: std::ops::Range<usize>| {
        digest
            .get(range)
            .and_then(|t| u64::from_str_radix(t, 16).ok())
            .unwrap_or(0)
    };
    ObjectId::from_parts(part(0..12), part(12..15) as u16, part(16..32))
        .map_err(|_| CadError::ValueOutOfRange("id"))
}

fn header(root: &Value, artifact: &str) -> Result<(), CadError> {
    if root.get("artifact").and_then(Value::as_str) != Some(artifact) {
        return Err(bad(format!("this is not an artifact {artifact}")));
    }
    if root.get("version").and_then(Value::as_u64) != Some(u64::from(ARTIFACT_VERSION)) {
        return Err(bad(format!(
            "only the version {ARTIFACT_VERSION} of {artifact} is read"
        )));
    }
    if root.get("units").and_then(Value::as_str) != Some("nm") {
        return Err(bad("the units are not nm"));
    }
    Ok(())
}

/// Reads the artifact `outline2d`.
pub fn parse(bytes: &[u8], limits: &MechanicalLimits) -> Result<BoardArtifact, CadError> {
    if bytes.len() > limits.max_bytes {
        return Err(bad("the file is larger than the limit"));
    }
    let root: Value =
        serde_json::from_slice(bytes).map_err(|e| bad(format!("the file is not JSON: {e}")))?;
    header(&root, ARTIFACT)?;
    let mut outline = Vec::new();
    for point in list(&root, "outline", true, limits.max_outline_points)? {
        let pair = point
            .as_array()
            .filter(|p| p.len() == 2)
            .ok_or_else(|| bad("a point of the outline is not a pair"))?;
        outline.push((
            length(pair.first(), "a point of the outline")?,
            length(pair.get(1), "a point of the outline")?,
        ));
    }
    if outline.len() < 3 {
        return Err(bad("the outline has fewer than three points"));
    }
    let mut holes = Vec::new();
    let mut ids = BTreeSet::new();
    for item in list(&root, "holes", false, limits.max_board_holes)? {
        let name = name_of(item.get("name"), limits)?;
        let x = length(item.get("x"), "the position of a hole")?;
        let y = length(item.get("y"), "the position of a hole")?;
        let diameter = positive(item.get("diameter"), "the diameter of a hole")?;
        let id = match item.get("id") {
            None => id_from(&format!("hole {name} {} {}", x.get(), y.get()))?,
            Some(value) => value
                .as_str()
                .and_then(|t| ObjectId::from_str(t).ok())
                .ok_or_else(|| bad("the id of a hole is not valid"))?,
        };
        if !ids.insert(id) {
            return Err(bad("two holes have the same id"));
        }
        holes.push(BoardHole {
            id,
            name,
            x,
            y,
            diameter,
        });
    }
    let mut parts = Vec::new();
    for item in list(&root, "parts", false, MAX_PARTS)? {
        let bottom = match item.get("side").map(Value::as_str) {
            None | Some(Some("top")) => false,
            Some(Some("bottom")) => true,
            _ => return Err(bad("the side of a part is top or bottom")),
        };
        parts.push(Part {
            name: name_of(item.get("name"), limits)?,
            x: length(item.get("x"), "the position of a part")?,
            y: length(item.get("y"), "the position of a part")?,
            width: positive(item.get("width"), "the width of a part")?,
            depth: positive(item.get("depth"), "the depth of a part")?,
            height: positive(item.get("height"), "the height of a part")?,
            bottom,
        });
    }
    let mut connectors = Vec::new();
    for item in list(&root, "connectors", false, MAX_CONNECTORS)? {
        let word = item.get("edge").and_then(Value::as_str);
        let edge = Edge::ALL
            .into_iter()
            .find(|e| Some(e.name()) == word)
            .ok_or_else(|| bad("the edge of a connector is front, back, left or right"))?;
        let above = match item.get("above") {
            None => Length::ZERO,
            value => length(value, "the height above the board of a connector")?,
        };
        connectors.push(Connector {
            name: name_of(item.get("name"), limits)?,
            edge,
            at: length(item.get("at"), "the position of a connector")?,
            width: positive(item.get("width"), "the width of a connector")?,
            height: positive(item.get("height"), "the height of a connector")?,
            above,
        });
    }
    let thickness = positive(root.get("thickness"), "the thickness of the board")?;
    let tallest = match root.get("tallest") {
        None => parts.iter().filter(|p| !p.bottom).map(|p| p.height).max(),
        value => Some(length(value, "the height of the tallest part")?),
    };
    Ok(BoardArtifact {
        board: BoardInterface {
            outline,
            holes,
            thickness,
            tallest_component: tallest,
            placement: None,
        },
        parts,
        connectors,
    })
}

fn canonical(value: &Value) -> Result<Vec<u8>, CadError> {
    to_canonical_file_bytes(value).map_err(|e| bad(e.to_string()))
}

/// Writes the artifact `outline2d` in its canonical form.
pub fn to_bytes(artifact: &BoardArtifact) -> Result<Vec<u8>, CadError> {
    let board = &artifact.board;
    let outline: Vec<Value> = board
        .outline
        .iter()
        .map(|(x, y)| json!([x.get(), y.get()]))
        .collect();
    let holes: Vec<Value> = board
        .holes
        .iter()
        .map(|h| {
            json!({"id": h.id.to_string(), "name": h.name, "x": h.x.get(), "y": h.y.get(),
                   "diameter": h.diameter.get()})
        })
        .collect();
    let parts: Vec<Value> = artifact
        .parts
        .iter()
        .map(|p| {
            json!({"name": p.name, "x": p.x.get(), "y": p.y.get(), "width": p.width.get(),
                   "depth": p.depth.get(), "height": p.height.get(),
                   "side": if p.bottom { "bottom" } else { "top" }})
        })
        .collect();
    let connectors: Vec<Value> = artifact
        .connectors
        .iter()
        .map(|c| {
            json!({"name": c.name, "edge": c.edge.name(), "at": c.at.get(),
                   "width": c.width.get(), "height": c.height.get(), "above": c.above.get()})
        })
        .collect();
    let mut root = json!({
        "artifact": ARTIFACT,
        "version": ARTIFACT_VERSION,
        "units": "nm",
        "outline": outline,
        "holes": holes,
        "parts": parts,
        "connectors": connectors,
        "thickness": board.thickness.get(),
    });
    if let Some(tallest) = board.tallest_component {
        root["tallest"] = Value::from(tallest.get());
    }
    canonical(&root)
}

/// One difference between the board the box was made from and the board as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The corners of the outline.
    Outline,
    /// The thickness of the board.
    Thickness,
    /// The height of the tallest part.
    Tallest,
    /// A hole that is new, by its name.
    HoleAdded(String),
    /// A hole that is gone.
    HoleRemoved(String),
    /// A hole that moved.
    HoleMoved(String),
    /// A hole with another diameter.
    HoleResized(String),
    /// The footprints of the parts.
    Parts,
    /// The connectors.
    Connectors,
}

/// What changed from `old` to `new`, in a fixed order.
pub fn changes(old: &BoardArtifact, new: &BoardArtifact) -> Vec<Change> {
    let mut out = Vec::new();
    if old.board.outline != new.board.outline {
        out.push(Change::Outline);
    }
    if old.board.thickness != new.board.thickness {
        out.push(Change::Thickness);
    }
    if old.board.tallest_component != new.board.tallest_component {
        out.push(Change::Tallest);
    }
    for hole in &old.board.holes {
        match new.board.holes.iter().find(|h| h.id == hole.id) {
            None => out.push(Change::HoleRemoved(hole.name.clone())),
            Some(now) => {
                if (now.x, now.y) != (hole.x, hole.y) {
                    out.push(Change::HoleMoved(hole.name.clone()));
                }
                if now.diameter != hole.diameter {
                    out.push(Change::HoleResized(hole.name.clone()));
                }
            }
        }
    }
    for hole in &new.board.holes {
        if old.board.holes.iter().all(|h| h.id != hole.id) {
            out.push(Change::HoleAdded(hole.name.clone()));
        }
    }
    if old.parts != new.parts {
        out.push(Change::Parts);
    }
    if old.connectors != new.connectors {
        out.push(Change::Connectors);
    }
    out
}

/// Where a board lies in a box: the corner of the rectangle that holds its outline in the inner
/// space, the lower corner of that rectangle in the frame of the board, and the height of the
/// underside of the board above the floor (the height of the posts that were made for it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seat {
    /// The corner in the inner space along X, in nanometres.
    pub x: i64,
    /// The same along Y.
    pub y: i64,
    /// The smallest X of the outline.
    pub min_x: i64,
    /// The smallest Y of the outline.
    pub min_y: i64,
    /// The size of the rectangle along X.
    pub width: i64,
    /// The size along Y.
    pub depth: i64,
    /// The height of the underside of the board above the floor.
    pub rise: i64,
}

/// Where `board` lies in `enclosure`: at its placement, or centred when it has none.
pub fn seat(enclosure: &Enclosure, board: &BoardInterface) -> Result<Seat, CadError> {
    let ((min_x, min_y), (max_x, max_y)) = board
        .bounds()
        .ok_or_else(|| bad("the board has no outline"))?;
    let params = enclosure.params();
    let (width, depth) = (max_x - min_x, max_y - min_y);
    let (x, y) = match board.placement {
        Some((x, y)) => (x.get(), y.get()),
        None => (
            (params.inner_length.get() - width) / 2,
            (params.inner_width.get() - depth) / 2,
        ),
    };
    let rise = enclosure
        .posts()
        .values()
        .filter(|p| p.board_hole.is_some())
        .map(|p| p.height.get())
        .max()
        .unwrap_or(0);
    Ok(Seat {
        x,
        y,
        min_x,
        min_y,
        width,
        depth,
        rise,
    })
}

/// The artifact `mounts2d`: the posts of the box in the frame of the board, for the board program
/// to read. It never writes the branch of the board.
pub fn mounts(enclosure: &Enclosure, board: &BoardInterface) -> Result<Vec<u8>, CadError> {
    let place = seat(enclosure, board)?;
    let posts: Vec<Value> = enclosure
        .posts()
        .iter()
        .map(|(id, p)| {
            let mut item = json!({
                "id": id.to_string(),
                "name": p.name,
                "x": p.x.get() - place.x + place.min_x,
                "y": p.y.get() - place.y + place.min_y,
                "bore_diameter": p.bore_diameter.get(),
                "outer_diameter": p.outer_diameter.get(),
                "height": p.height.get(),
            });
            if let Some(hole) = p.board_hole {
                item["board_hole"] = Value::from(hole.to_string());
            }
            item
        })
        .collect();
    canonical(&json!({"artifact": "mounts2d", "version": 1, "units": "nm", "mounts": posts}))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::eh3dfile::tests::Seeded;

    pub(crate) fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    /// A made-up board of 80 by 50 mm with four holes, two parts and one connector.
    pub(crate) fn sample() -> BoardArtifact {
        let hole = |name: &str, x: i64, y: i64| BoardHole {
            id: id_from(name).unwrap(),
            name: name.to_owned(),
            x: mm(x),
            y: mm(y),
            diameter: mm(4),
        };
        BoardArtifact {
            board: BoardInterface {
                outline: vec![
                    (mm(0), mm(0)),
                    (mm(80), mm(0)),
                    (mm(80), mm(50)),
                    (mm(0), mm(50)),
                ],
                holes: vec![
                    hole("H1", 4, 4),
                    hole("H2", 76, 4),
                    hole("H3", 76, 46),
                    hole("H4", 4, 46),
                ],
                thickness: Length::from_um(1600).unwrap(),
                tallest_component: Some(mm(12)),
                placement: None,
            },
            parts: vec![
                Part {
                    name: "C1".to_owned(),
                    x: mm(20),
                    y: mm(20),
                    width: mm(10),
                    depth: mm(10),
                    height: mm(12),
                    bottom: false,
                },
                Part {
                    name: "U2".to_owned(),
                    x: mm(40),
                    y: mm(20),
                    width: mm(8),
                    depth: mm(8),
                    height: mm(2),
                    bottom: true,
                },
            ],
            connectors: vec![Connector {
                name: "J1".to_owned(),
                edge: Edge::Front,
                at: mm(40),
                width: mm(10),
                height: mm(5),
                above: Length::ZERO,
            }],
        }
    }

    fn limits() -> MechanicalLimits {
        Settings::defaults().mechanical
    }

    #[test]
    fn the_artifact_is_read_back_the_same_and_unknown_fields_are_ignored() {
        let board = sample();
        let bytes = to_bytes(&board).unwrap();
        assert_eq!(parse(&bytes, &limits()).unwrap(), board);
        assert_eq!(to_bytes(&parse(&bytes, &limits()).unwrap()).unwrap(), bytes);
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        value["later"] = Value::from(7);
        value.as_object_mut().unwrap().remove("tallest");
        let read = parse(value.to_string().as_bytes(), &limits()).unwrap();
        // without a stated height the tallest part of the top face counts, not the one below
        assert_eq!(read.board.tallest_component, Some(mm(12)));
    }

    #[test]
    fn values_out_of_their_range_are_refused_with_their_code() {
        let bytes = to_bytes(&sample()).unwrap();
        let good: Value = serde_json::from_slice(&bytes).unwrap();
        let cases: [(&str, Value); 8] = [
            ("artifact", Value::from("outline3d")),
            ("version", Value::from(2)),
            ("units", Value::from("mm")),
            ("thickness", Value::from(0)),
            ("thickness", Value::from(1.5)),
            ("outline", json!([[0, 0], [1, 1]])),
            (
                "holes",
                json!([{"name": "a", "x": 0, "y": 0, "diameter": -1}]),
            ),
            ("connectors", json!([{"name": "a", "edge": "up"}])),
        ];
        for (key, value) in cases {
            let mut broken = good.clone();
            broken[key] = value.clone();
            let error = parse(broken.to_string().as_bytes(), &limits()).unwrap_err();
            assert_eq!(error.code(), "PHOENIX-CAD-066", "{key} {value}");
        }
        assert!(parse(b"[", &limits()).is_err());
        let mut small = limits();
        small.max_bytes = 10;
        assert!(parse(&bytes, &small).is_err());
    }

    #[test]
    fn damaged_artifacts_never_panic() {
        let good = to_bytes(&sample()).unwrap();
        let mut random = Seeded(0x00CA_DB03);
        for _ in 0..600 {
            let mut bytes = good.clone();
            for _ in 0..=random.below(5) {
                let at = random.below(bytes.len() as u64) as usize;
                bytes[at] = random.below(128) as u8;
            }
            if let Ok(read) = parse(&bytes, &limits()) {
                assert!(read.board.outline.len() >= 3 && read.board.thickness.get() > 0);
            }
        }
    }

    #[test]
    fn the_changes_of_a_board_are_named_hole_by_hole() {
        let old = sample();
        assert!(changes(&old, &old).is_empty());
        let mut new = old.clone();
        new.board.holes[0].x = mm(5);
        new.board.holes[1].diameter = mm(5);
        new.board.holes.remove(2);
        new.board.holes.push(BoardHole {
            id: id_from("H9").unwrap(),
            name: "H9".to_owned(),
            x: mm(40),
            y: mm(4),
            diameter: mm(4),
        });
        new.board.thickness = mm(2);
        new.connectors.clear();
        assert_eq!(
            changes(&old, &new),
            vec![
                Change::Thickness,
                Change::HoleMoved("H1".to_owned()),
                Change::HoleResized("H2".to_owned()),
                Change::HoleRemoved("H3".to_owned()),
                Change::HoleAdded("H9".to_owned()),
                Change::Connectors,
            ]
        );
    }
}
