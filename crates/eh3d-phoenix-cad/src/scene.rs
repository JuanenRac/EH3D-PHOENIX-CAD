//! The drawing of the box as lines: the segments of the box, its inner space, its holes, its
//! posts, the extrusions and pockets of its sketches and the board, and the projection of those lines onto a pane for the 3D view and the
//! plan.
//!
//! Everything is calculated from the enclosure and drawn nowhere here: the window only paints the
//! lines this module gives. Lengths become millimetres as floating point numbers, which is fine
//! for a picture and is never used to decide anything about the model.

use crate::datum::Datum;
use crate::enclosure::{Enclosure, EnclosureParams, Face, Feature, FeatureKind, Hole, Id};
use crate::mount::Post;

/// A point in millimetres: x along the length of the box, y along its width, z up. The smallest
/// corner of the outer box is the origin.
pub type Point3 = [f64; 3];

/// A length in nanometres as millimetres.
pub fn mm(nm: i64) -> f64 {
    nm as f64 / 1_000_000.0
}

/// What a segment is part of, which decides its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The outer box.
    Outer,
    /// The inner space.
    Cavity,
    /// The plane where the lid meets the walls.
    LidPlane,
    /// A hole of a face.
    Hole,
    /// A screw hole of the lid.
    Screw,
    /// A post.
    Post,
    /// The board (or the envelope that is checked).
    Board,
    /// The object that is selected in the tree.
    Selected,
}

/// A line of the drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// One end.
    pub a: Point3,
    /// The other end.
    pub b: Point3,
    /// What it belongs to.
    pub kind: Kind,
}

/// What is drawn in the colour of a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Highlight {
    /// Nothing.
    Nothing,
    /// The hole or the post with this id.
    Object(Id),
    /// The screw hole of the lid with this position in the list of the lid holes.
    Screw(usize),
}

/// The box that holds the board (or the envelope that is checked), in millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoardBox {
    /// The smallest corner.
    pub min: Point3,
    /// The largest corner.
    pub max: Point3,
}

fn push_box(out: &mut Vec<Segment>, lo: Point3, hi: Point3, kind: Kind) {
    let corner = |i: usize| -> Point3 {
        [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ]
    };
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                out.push(Segment {
                    a: corner(i),
                    b: corner(i | bit),
                    kind,
                });
            }
        }
    }
}

fn push_rectangle_z(out: &mut Vec<Segment>, lo: [f64; 2], hi: [f64; 2], z: f64, kind: Kind) {
    let corners = [
        [lo[0], lo[1], z],
        [hi[0], lo[1], z],
        [hi[0], hi[1], z],
        [lo[0], hi[1], z],
    ];
    push_ring(out, corners, kind);
}

/// The four sides of a quadrilateral, in order.
fn push_ring(out: &mut Vec<Segment>, corners: [Point3; 4], kind: Kind) {
    let mut previous = corners[3];
    for corner in corners {
        out.push(Segment {
            a: previous,
            b: corner,
            kind,
        });
        previous = corner;
    }
}

/// A circle of `segments` lines about `centre`, in the plane of the two axes `u` and `v`.
fn push_circle(
    out: &mut Vec<Segment>,
    centre: Point3,
    axes: [Point3; 2],
    radius: f64,
    segments: u32,
    kind: Kind,
) {
    let n = segments.max(3);
    let point = |k: u32| -> Point3 {
        let t = std::f64::consts::TAU * f64::from(k % n) / f64::from(n);
        let (s, c) = t.sin_cos();
        [
            centre[0] + radius * (c * axes[0][0] + s * axes[1][0]),
            centre[1] + radius * (c * axes[0][1] + s * axes[1][1]),
            centre[2] + radius * (c * axes[0][2] + s * axes[1][2]),
        ]
    };
    for k in 0..n {
        out.push(Segment {
            a: point(k),
            b: point(k + 1),
            kind,
        });
    }
}

fn thickness(p: &EnclosureParams, face: Face) -> f64 {
    mm(match face {
        Face::Front | Face::Back | Face::Left | Face::Right => p.wall.get(),
        Face::Top => p.lid.get(),
        Face::Bottom => p.floor.get(),
    })
}

fn push_hole(
    out: &mut Vec<Segment>,
    enclosure: &Enclosure,
    hole: &Hole,
    segments: u32,
    kind: Kind,
) {
    let datum = Datum::of_face(enclosure, hole.face);
    let unit = |axis: [i64; 3]| -> Point3 {
        let r = datum.rotation.apply(axis);
        [r[0] as f64, r[1] as f64, r[2] as f64]
    };
    let (ax, ay, az) = (unit([1, 0, 0]), unit([0, 1, 0]), unit([0, 0, 1]));
    let origin = [
        mm(datum.origin[0].get()),
        mm(datum.origin[1].get()),
        mm(datum.origin[2].get()),
    ];
    let at = |u: f64, v: f64, w: f64| -> Point3 {
        [
            origin[0] + ax[0] * u + ay[0] * v + az[0] * w,
            origin[1] + ax[1] * u + ay[1] * v + az[1] * w,
            origin[2] + ax[2] * u + ay[2] * v + az[2] * w,
        ]
    };
    let (u, v) = (mm(hole.u.get()), mm(hole.v.get()));
    let (su, sv) = hole.style.footprint(hole.diameter);
    let (su, sv) = (su as f64 / 1_000_000.0, sv as f64 / 1_000_000.0);
    if hole.style.is_round() {
        push_circle(out, at(u, v, 0.0), [ax, ay], su / 2.0, segments, kind);
        let depth = thickness(enclosure.params(), hole.face);
        let bore = mm(hole.diameter.get()) / 2.0;
        push_circle(out, at(u, v, -depth), [ax, ay], bore, segments, kind);
    } else {
        let corners = [
            at(u - su / 2.0, v - sv / 2.0, 0.0),
            at(u + su / 2.0, v - sv / 2.0, 0.0),
            at(u + su / 2.0, v + sv / 2.0, 0.0),
            at(u - su / 2.0, v + sv / 2.0, 0.0),
        ];
        push_ring(out, corners, kind);
    }
}

fn push_post(
    out: &mut Vec<Segment>,
    params: &EnclosureParams,
    post: &Post,
    segments: u32,
    kind: Kind,
) {
    let (cx, cy) = (
        mm(params.wall.get() + post.x.get()),
        mm(params.wall.get() + post.y.get()),
    );
    let z0 = mm(params.floor.get());
    let z1 = mm(params.floor.get() + post.height.get());
    let flat = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let r = mm(post.outer_diameter.get()) / 2.0;
    push_circle(out, [cx, cy, z0], flat, r, segments, kind);
    push_circle(out, [cx, cy, z1], flat, r, segments, kind);
    push_circle(
        out,
        [cx, cy, z1],
        flat,
        mm(post.bore_diameter.get()) / 2.0,
        segments,
        kind,
    );
    if let Some(base) = post.base {
        push_circle(
            out,
            [cx, cy, z0],
            flat,
            mm(base.diameter.get()) / 2.0,
            segments,
            kind,
        );
    }
    for (dx, dy) in [(r, 0.0), (0.0, r), (-r, 0.0), (0.0, -r)] {
        out.push(Segment {
            a: [cx + dx, cy + dy, z0],
            b: [cx + dx, cy + dy, z1],
            kind,
        });
    }
}

/// An extrusion or a pocket: its outline on the face and at the height or depth it goes to, joined
/// by a line at each corner (four points of a circle).
fn push_feature(out: &mut Vec<Segment>, enclosure: &Enclosure, feature: &Feature, kind: Kind) {
    let datum = Datum::of_face(enclosure, feature.face);
    let unit = |axis: [i64; 3]| -> Point3 {
        let r = datum.rotation.apply(axis);
        [r[0] as f64, r[1] as f64, r[2] as f64]
    };
    let (ax, ay, az) = (unit([1, 0, 0]), unit([0, 1, 0]), unit([0, 0, 1]));
    let origin = [
        mm(datum.origin[0].get()),
        mm(datum.origin[1].get()),
        mm(datum.origin[2].get()),
    ];
    let at = |u: f64, v: f64, w: f64| -> Point3 {
        [
            origin[0] + ax[0] * u + ay[0] * v + az[0] * w,
            origin[1] + ax[1] * u + ay[1] * v + az[1] * w,
            origin[2] + ax[2] * u + ay[2] * v + az[2] * w,
        ]
    };
    let depth = mm(feature.depth_in(enclosure.params()).get());
    let w = match feature.kind {
        FeatureKind::Extrude => depth,
        FeatureKind::Pocket => -depth,
    };
    let (u0, v0) = (mm(feature.u0.get()), mm(feature.v0.get()));
    let on = |p: (i64, i64)| -> (f64, f64) { (u0 + mm(p.0), v0 + mm(p.1)) };
    for outline in feature.profile.outlines() {
        let poly = outline.polygon();
        let n = poly.len();
        for i in 0..n {
            let (a, b) = (on(poly[i]), on(poly[(i + 1) % n]));
            out.push(Segment {
                a: at(a.0, a.1, 0.0),
                b: at(b.0, b.1, 0.0),
                kind,
            });
            out.push(Segment {
                a: at(a.0, a.1, w),
                b: at(b.0, b.1, w),
                kind,
            });
        }
        let corners: Vec<(i64, i64)> = match outline.circle_of() {
            Some(((cx, cy), r)) => vec![(cx + r, cy), (cx, cy + r), (cx - r, cy), (cx, cy - r)],
            None => outline.pieces().iter().map(|piece| piece.start()).collect(),
        };
        for p in corners {
            let a = on(p);
            out.push(Segment {
                a: at(a.0, a.1, 0.0),
                b: at(a.0, a.1, w),
                kind,
            });
        }
    }
}

/// The size of the outer box in millimetres.
pub fn outer_size(enclosure: &Enclosure) -> Point3 {
    let (x, y, z) = enclosure.params().outer();
    [mm(x.get()), mm(y.get()), mm(z.get())]
}

/// The lines of the whole model: the outer box, the inner space, the plane of the lid, the holes
/// of the faces, the screw holes of the lid, the posts and the board. `segments` is the number of
/// lines of a circle.
pub fn scene_of(
    enclosure: &Enclosure,
    board: Option<BoardBox>,
    highlight: Highlight,
    segments: u32,
) -> Vec<Segment> {
    let p = enclosure.params();
    let size = outer_size(enclosure);
    let mut out = Vec::new();
    push_box(&mut out, [0.0, 0.0, 0.0], size, Kind::Outer);
    let wall = mm(p.wall.get());
    let floor = mm(p.floor.get());
    let inner = [
        mm(p.inner_length.get()),
        mm(p.inner_width.get()),
        mm(p.inner_height.get()),
    ];
    push_box(
        &mut out,
        [wall, wall, floor],
        [wall + inner[0], wall + inner[1], floor + inner[2]],
        Kind::Cavity,
    );
    push_rectangle_z(
        &mut out,
        [0.0, 0.0],
        [size[0], size[1]],
        floor + inner[2],
        Kind::LidPlane,
    );
    for (id, hole) in enclosure.holes() {
        let kind = if highlight == Highlight::Object(*id) {
            Kind::Selected
        } else {
            Kind::Hole
        };
        push_hole(&mut out, enclosure, hole, segments, kind);
    }
    for (k, hole) in enclosure.lid_holes().iter().enumerate() {
        let kind = if highlight == Highlight::Screw(k) {
            Kind::Selected
        } else {
            Kind::Screw
        };
        push_hole(&mut out, enclosure, hole, segments, kind);
    }
    for (id, post) in enclosure.posts() {
        let kind = if highlight == Highlight::Object(*id) {
            Kind::Selected
        } else {
            Kind::Post
        };
        push_post(&mut out, p, post, segments, kind);
    }
    for (id, feature) in enclosure.features() {
        // an extrusion is drawn like a post (material) and a pocket like a hole
        let kind = if highlight == Highlight::Object(*id) {
            Kind::Selected
        } else if feature.kind == FeatureKind::Extrude {
            Kind::Post
        } else {
            Kind::Hole
        };
        push_feature(&mut out, enclosure, feature, kind);
    }
    if let Some(b) = board {
        push_box(&mut out, b.min, b.max, Kind::Board);
    }
    out
}

/// A point of view: the turn about the vertical axis and the angle above the horizon, in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// The turn about the vertical axis.
    pub yaw: f64,
    /// The angle above the horizon; 90 looks straight down.
    pub pitch: f64,
}

impl Camera {
    /// The point of view of the plan: straight down, the length to the right and the width up.
    pub const TOP: Camera = Camera {
        yaw: 0.0,
        pitch: 90.0,
    };
}

/// A point of the model seen from a camera: to the right, up and the depth (larger is farther
/// from the viewer).
pub type Seen = (f64, f64, f64);

/// A point of the model seen from a camera, about `centre`.
pub fn rotate(p: Point3, centre: Point3, camera: Camera) -> Seen {
    let (x, y, z) = (p[0] - centre[0], p[1] - centre[1], p[2] - centre[2]);
    let (sy, cy) = camera.yaw.to_radians().sin_cos();
    let (sp, cp) = camera.pitch.to_radians().sin_cos();
    let right = x * cy - y * sy;
    let ahead = x * sy + y * cy;
    (right, ahead * sp + z * cp, ahead * cp - z * sp)
}

/// How the size of the drawing is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// The sphere that holds the box fits the pane, so the size does not change when it turns.
    Sphere,
    /// The rectangle that holds the drawing fits the pane.
    Bounds,
}

/// A line on the pane, in pixels from its upper left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line2 {
    /// One end.
    pub a: (f64, f64),
    /// The other end.
    pub b: (f64, f64),
    /// What it belongs to.
    pub kind: Kind,
    /// The depth of its middle (larger is farther from the viewer).
    pub depth: f64,
}

/// What a view needs to put the lines on a pane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// The point of view.
    pub camera: Camera,
    /// The zoom over the size that fits the pane.
    pub zoom: f64,
    /// The movement of the drawing, in pixels.
    pub pan: (f64, f64),
    /// The width and the height of the pane, in pixels.
    pub pane: (f64, f64),
    /// The pixels left empty around the drawing.
    pub margin: f64,
    /// How the size is chosen.
    pub fit: Fit,
}

/// Puts the lines on a pane. `size` is the size of the outer box, whose centre is the point the
/// camera turns about.
pub fn project(segments: &[Segment], size: Point3, frame: &Frame) -> Vec<Line2> {
    let centre = [size[0] / 2.0, size[1] / 2.0, size[2] / 2.0];
    let seen: Vec<(Seen, Seen, Kind)> = segments
        .iter()
        .map(|s| {
            (
                rotate(s.a, centre, frame.camera),
                rotate(s.b, centre, frame.camera),
                s.kind,
            )
        })
        .collect();
    let (width, height) = frame.pane;
    let room = (
        (width - 2.0 * frame.margin).max(1.0),
        (height - 2.0 * frame.margin).max(1.0),
    );
    let (scale, middle) = match frame.fit {
        Fit::Sphere => {
            let radius = 0.5 * (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt();
            (room.0.min(room.1) / (2.0 * radius.max(1e-9)), (0.0, 0.0))
        }
        Fit::Bounds => {
            let mut low = (f64::INFINITY, f64::INFINITY);
            let mut high = (f64::NEG_INFINITY, f64::NEG_INFINITY);
            for (a, b, _) in &seen {
                for p in [a, b] {
                    low = (low.0.min(p.0), low.1.min(p.1));
                    high = (high.0.max(p.0), high.1.max(p.1));
                }
            }
            if low.0 > high.0 {
                (1.0, (0.0, 0.0))
            } else {
                let (w, h) = ((high.0 - low.0).max(1e-9), (high.1 - low.1).max(1e-9));
                (
                    (room.0 / w).min(room.1 / h),
                    ((low.0 + high.0) / 2.0, (low.1 + high.1) / 2.0),
                )
            }
        }
    };
    let scale = scale * frame.zoom;
    let to_pane = |p: &Seen| -> (f64, f64) {
        (
            width / 2.0 + (p.0 - middle.0) * scale + frame.pan.0,
            height / 2.0 - (p.1 - middle.1) * scale + frame.pan.1,
        )
    };
    seen.iter()
        .map(|(a, b, kind)| Line2 {
            a: to_pane(a),
            b: to_pane(b),
            kind: *kind,
            depth: (a.2 + b.2) / 2.0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::{EnclosureParams, HoleStyle, Operation};
    use crate::Length;
    use eh3d_container::ObjectId;

    fn mmlen(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn enclosure() -> Enclosure {
        Enclosure::new(EnclosureParams {
            inner_length: mmlen(100),
            inner_width: mmlen(60),
            inner_height: mmlen(30),
            wall: mmlen(2),
            floor: mmlen(2),
            lid: mmlen(3),
            edge_margin: mmlen(3),
            min_web: mmlen(2),
        })
        .unwrap()
    }

    fn id(n: u64) -> Id {
        ObjectId::from_parts(1_700_000_000_000, n as u16, n).unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn with_plate() -> Enclosure {
        use crate::sketch::{Sketch, SketchPlane};
        let plane = SketchPlane {
            face: Face::Top,
            u0: mmlen(10),
            v0: mmlen(10),
            offset: Length::ZERO,
        };
        let mut k = Sketch::new("plate", plane).unwrap();
        k.add_rectangle((mmlen(0), mmlen(0)), (mmlen(40), mmlen(25)))
            .unwrap();
        let c = k.add_point(mmlen(20), mmlen(12)).unwrap();
        k.add_circle(c, mmlen(3)).unwrap();
        let mut e = enclosure();
        e.set_sketches(vec![k], None);
        e
    }

    #[test]
    fn an_extrusion_is_drawn_above_the_lid_and_a_pocket_into_it() {
        let mut e = with_plate();
        e.apply(Operation::Extrude {
            id: id(1),
            sketch: "plate".into(),
            height: mmlen(10),
        })
        .unwrap();
        let lines = scene_of(&e, None, Highlight::Nothing, 16);
        let boss: Vec<&Segment> = lines.iter().filter(|s| s.kind == Kind::Post).collect();
        // the outline of the rectangle and of the hole at the lid and 10 mm above it, and a
        // vertical line at each of the four corners and at four points of the hole
        let zs: Vec<f64> = boss.iter().flat_map(|s| [s.a[2], s.b[2]]).collect();
        let (lo, hi) = zs
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), z| (l.min(*z), h.max(*z)));
        assert!(close(lo, 35.0) && close(hi, 45.0), "{lo} {hi}");
        let vertical = boss
            .iter()
            .filter(|s| close(s.a[0], s.b[0]) && close(s.a[1], s.b[1]))
            .count();
        assert_eq!(vertical, 8);
        assert!(lines.iter().all(|s| s.kind != Kind::Selected));
        let picked = scene_of(&e, None, Highlight::Object(id(1)), 16);
        assert_eq!(
            picked.iter().filter(|s| s.kind == Kind::Selected).count(),
            boss.len()
        );
        let mut p = with_plate();
        p.apply(Operation::Pocket {
            id: id(2),
            sketch: "plate".into(),
            depth: None,
        })
        .unwrap();
        let lines = scene_of(&p, None, Highlight::Nothing, 16);
        let cut: Vec<&Segment> = lines.iter().filter(|s| s.kind == Kind::Hole).collect();
        let zs: Vec<f64> = cut.iter().flat_map(|s| [s.a[2], s.b[2]]).collect();
        let (lo, hi) = zs
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), z| (l.min(*z), h.max(*z)));
        // through the lid, which is 3 mm thick
        assert!(close(lo, 32.0) && close(hi, 35.0), "{lo} {hi}");
    }

    #[test]
    fn the_empty_box_is_two_boxes_and_the_plane_of_the_lid() {
        let lines = scene_of(&enclosure(), None, Highlight::Nothing, 16);
        let count = |k: Kind| lines.iter().filter(|s| s.kind == k).count();
        assert_eq!(count(Kind::Outer), 12);
        assert_eq!(count(Kind::Cavity), 12);
        assert_eq!(count(Kind::LidPlane), 4);
        assert_eq!(lines.len(), 28);
        assert_eq!(outer_size(&enclosure()), [104.0, 64.0, 35.0]);
        // the plane of the lid is 3 mm below the top
        let plane = lines.iter().find(|s| s.kind == Kind::LidPlane).unwrap();
        assert!(close(plane.a[2], 32.0));
    }

    #[test]
    fn a_round_hole_is_two_circles_and_a_rectangular_one_is_four_lines() {
        let mut e = enclosure();
        let round = crate::Hole {
            name: "usb".into(),
            face: Face::Front,
            u: mmlen(30),
            v: mmlen(17),
            diameter: mmlen(6),
            style: HoleStyle::Plain,
        };
        e.apply(Operation::AddHole {
            id: id(1),
            hole: round,
        })
        .unwrap();
        let lines = scene_of(&e, None, Highlight::Nothing, 16);
        assert_eq!(lines.iter().filter(|s| s.kind == Kind::Hole).count(), 32);
        // the outer circle lies in the plane of the front face (y = 0) around (30, 17)
        let first = lines.iter().find(|s| s.kind == Kind::Hole).unwrap();
        assert!(close(first.a[1], 0.0));
        assert!(close(first.a[0], 33.0) && close(first.a[2], 17.0));
        let square = crate::Hole {
            name: "slot".into(),
            face: Face::Back,
            u: mmlen(50),
            v: mmlen(17),
            diameter: mmlen(8),
            style: HoleStyle::Rectangular { height: mmlen(4) },
        };
        e.apply(Operation::AddHole {
            id: id(2),
            hole: square,
        })
        .unwrap();
        let lines = scene_of(&e, None, Highlight::Nothing, 16);
        assert_eq!(lines.iter().filter(|s| s.kind == Kind::Hole).count(), 36);
    }

    #[test]
    fn a_selected_hole_is_drawn_apart() {
        let mut e = enclosure();
        e.apply(Operation::AddHole {
            id: id(1),
            hole: crate::Hole {
                name: "a".into(),
                face: Face::Top,
                u: mmlen(50),
                v: mmlen(30),
                diameter: mmlen(6),
                style: HoleStyle::Plain,
            },
        })
        .unwrap();
        let lines = scene_of(&e, None, Highlight::Object(id(1)), 8);
        assert_eq!(
            lines.iter().filter(|s| s.kind == Kind::Selected).count(),
            16
        );
        assert_eq!(lines.iter().filter(|s| s.kind == Kind::Hole).count(), 0);
    }

    #[test]
    fn the_board_is_one_more_box() {
        let board = BoardBox {
            min: [10.0, 10.0, 2.0],
            max: [70.0, 50.0, 10.0],
        };
        let lines = scene_of(&enclosure(), Some(board), Highlight::Nothing, 8);
        assert_eq!(lines.iter().filter(|s| s.kind == Kind::Board).count(), 12);
    }

    #[test]
    fn seen_from_above_the_length_goes_right_and_the_width_goes_up() {
        let c = [0.0, 0.0, 0.0];
        let (r, u, _) = rotate([3.0, 4.0, 5.0], c, Camera::TOP);
        assert!(close(r, 3.0) && close(u, 4.0));
        // from the front at the horizon the height goes up and the depth goes away
        let front = Camera {
            yaw: 0.0,
            pitch: 0.0,
        };
        let (r, u, d) = rotate([3.0, 4.0, 5.0], c, front);
        assert!(close(r, 3.0) && close(u, 5.0) && close(d, 4.0));
        // a quarter of a turn takes the length to the depth
        let quarter = Camera {
            yaw: 90.0,
            pitch: 0.0,
        };
        let (r, u, d) = rotate([1.0, 0.0, 0.0], c, quarter);
        assert!(close(r, 0.0) && close(u, 0.0) && close(d, 1.0));
    }

    #[test]
    fn a_line_is_fitted_to_the_pane_with_its_margin() {
        let segment = Segment {
            a: [0.0, 0.0, 0.0],
            b: [10.0, 0.0, 0.0],
            kind: Kind::Outer,
        };
        let frame = Frame {
            camera: Camera::TOP,
            zoom: 1.0,
            pan: (0.0, 0.0),
            pane: (120.0, 120.0),
            margin: 10.0,
            fit: Fit::Bounds,
        };
        let lines = project(&[segment], [10.0, 10.0, 10.0], &frame);
        assert_eq!(lines.len(), 1);
        assert!(close(lines[0].a.0, 10.0) && close(lines[0].a.1, 60.0));
        assert!(close(lines[0].b.0, 110.0) && close(lines[0].b.1, 60.0));
        // zooming by two doubles the line about the middle, and the pan moves it
        let zoomed = Frame {
            zoom: 2.0,
            pan: (5.0, -3.0),
            ..frame
        };
        let lines = project(&[segment], [10.0, 10.0, 10.0], &zoomed);
        assert!(close(lines[0].a.0, -40.0 + 5.0) && close(lines[0].a.1, 57.0));
        assert!(close(lines[0].b.0, 160.0 + 5.0));
    }

    #[test]
    fn the_sphere_fit_does_not_change_when_the_box_turns() {
        let e = enclosure();
        let lines = scene_of(&e, None, Highlight::Nothing, 8);
        let size = outer_size(&e);
        let mut frame = Frame {
            camera: Camera {
                yaw: 0.0,
                pitch: 0.0,
            },
            zoom: 1.0,
            pan: (0.0, 0.0),
            pane: (400.0, 300.0),
            margin: 20.0,
            fit: Fit::Sphere,
        };
        // the vertical edges of the outer box are 35 mm long and keep their size when it turns
        let vertical = |frame: &Frame| {
            let seen = project(&lines[..12], size, frame);
            lines[..12]
                .iter()
                .zip(seen.iter())
                .filter(|(s, _)| s.a[0] == s.b[0] && s.a[1] == s.b[1])
                .map(|(_, l)| ((l.a.0 - l.b.0).powi(2) + (l.a.1 - l.b.1).powi(2)).sqrt())
                .fold(0.0f64, f64::max)
        };
        // 260 px for a sphere of diameter sqrt(104^2 + 64^2 + 35^2) mm
        let diameter = (104.0f64 * 104.0 + 64.0 * 64.0 + 35.0 * 35.0).sqrt();
        let expected = 35.0 * 260.0 / diameter;
        assert!(close(vertical(&frame), expected));
        frame.camera.yaw = 90.0;
        assert!(close(vertical(&frame), expected));
    }
}
