//! The projection of the lines of the drawing onto a pane: the point of view of a camera, how the
//! size of the drawing is chosen, the lines on the pane, and the way back from a point of the pane
//! to a point of a plane of the model (to pick and to drag what is drawn).
//!
//! Nothing here knows the model: it works on segments in millimetres, which is fine for a picture
//! and is never used to decide anything about the model.

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
    /// A measurement: the line it measures and the points that were picked for it.
    Measure,
    /// A placed part that shares volume with another one.
    Conflict,
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

/// How a view puts the model on its pane: the point of view, the scale, the middle of the drawing
/// and the movement, for the segments it was made from. It goes both ways: a point of the model to
/// a point of the pane, and a point of the pane to a point of a plane of the model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mapping {
    camera: Camera,
    centre: Point3,
    scale: f64,
    middle: (f64, f64),
    pane: (f64, f64),
    pan: (f64, f64),
}

/// The vector `d` as seen from the camera, without the depth: to the right and up.
fn seen_vector(d: Point3, camera: Camera) -> (f64, f64) {
    let (sy, cy) = camera.yaw.to_radians().sin_cos();
    let (sp, cp) = camera.pitch.to_radians().sin_cos();
    let right = d[0] * cy - d[1] * sy;
    let ahead = d[0] * sy + d[1] * cy;
    (right, ahead * sp + d[2] * cp)
}

impl Mapping {
    /// The mapping of `segments` (which fix the size when the fit is to the bounds) in a frame.
    /// `size` is the size of the outer box, whose centre is the point the camera turns about.
    pub fn of(segments: &[Segment], size: Point3, frame: &Frame) -> Mapping {
        let centre = [size[0] / 2.0, size[1] / 2.0, size[2] / 2.0];
        let seen: Vec<(Seen, Seen)> = segments
            .iter()
            .map(|s| {
                (
                    rotate(s.a, centre, frame.camera),
                    rotate(s.b, centre, frame.camera),
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
                let radius =
                    0.5 * (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt();
                (room.0.min(room.1) / (2.0 * radius.max(1e-9)), (0.0, 0.0))
            }
            Fit::Bounds => {
                let mut low = (f64::INFINITY, f64::INFINITY);
                let mut high = (f64::NEG_INFINITY, f64::NEG_INFINITY);
                for (a, b) in &seen {
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
        Mapping {
            camera: frame.camera,
            centre,
            scale: scale * frame.zoom,
            middle,
            pane: frame.pane,
            pan: frame.pan,
        }
    }

    /// The point of the pane, in pixels from its upper left corner, where a point of the model
    /// is drawn, and its depth (larger is farther from the viewer).
    pub fn to_pane(&self, p: Point3) -> ((f64, f64), f64) {
        let s = rotate(p, self.centre, self.camera);
        (
            (
                self.pane.0 / 2.0 + (s.0 - self.middle.0) * self.scale + self.pan.0,
                self.pane.1 / 2.0 - (s.1 - self.middle.1) * self.scale + self.pan.1,
            ),
            s.2,
        )
    }

    /// The point `origin + s * a + t * b` of the plane that is drawn at the point `px` of the pane,
    /// as the two numbers `(s, t)`; `None` when the plane is seen edge on (or the scale is nil), so
    /// that the point of the pane says nothing about where on the plane it is.
    pub fn on_plane(
        &self,
        px: (f64, f64),
        origin: Point3,
        a: Point3,
        b: Point3,
    ) -> Option<(f64, f64)> {
        if self.scale <= 1e-12 {
            return None;
        }
        let want = (
            (px.0 - self.pane.0 / 2.0 - self.pan.0) / self.scale + self.middle.0,
            -(px.1 - self.pane.1 / 2.0 - self.pan.1) / self.scale + self.middle.1,
        );
        let o = rotate(origin, self.centre, self.camera);
        let (la, lb) = (seen_vector(a, self.camera), seen_vector(b, self.camera));
        let det = la.0 * lb.1 - lb.0 * la.1;
        if det.abs() < 1e-9 {
            return None;
        }
        let (dr, du) = (want.0 - o.0, want.1 - o.1);
        Some(((dr * lb.1 - lb.0 * du) / det, (la.0 * du - la.1 * dr) / det))
    }
}

/// Puts the lines on a pane with a mapping that was made from other (or the same) segments.
pub fn project_with(mapping: &Mapping, segments: &[Segment]) -> Vec<Line2> {
    segments
        .iter()
        .map(|s| {
            let (a, da) = mapping.to_pane(s.a);
            let (b, db) = mapping.to_pane(s.b);
            Line2 {
                a,
                b,
                kind: s.kind,
                depth: (da + db) / 2.0,
            }
        })
        .collect()
}

/// Puts the lines on a pane. `size` is the size of the outer box, whose centre is the point the
/// camera turns about.
pub fn project(segments: &[Segment], size: Point3, frame: &Frame) -> Vec<Line2> {
    project_with(&Mapping::of(segments, size, frame), segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn plan_frame() -> Frame {
        Frame {
            camera: Camera::TOP,
            zoom: 1.0,
            pan: (0.0, 0.0),
            pane: (400.0, 300.0),
            margin: 0.0,
            fit: Fit::Bounds,
        }
    }

    fn diagonal() -> Vec<Segment> {
        vec![Segment {
            a: [0.0, 0.0, 0.0],
            b: [100.0, 60.0, 0.0],
            kind: Kind::Outer,
        }]
    }

    #[test]
    fn the_plan_puts_the_length_to_the_right_and_the_width_up() {
        // the diagonal of 100 by 60 fits a pane of 400 by 300 at 4 pixels for each millimetre
        // (the length is what limits it), about the middle of the pane
        let m = Mapping::of(&diagonal(), [100.0, 60.0, 30.0], &plan_frame());
        let (a, _) = m.to_pane([0.0, 0.0, 0.0]);
        let (b, _) = m.to_pane([100.0, 60.0, 0.0]);
        assert!(close(a.0, 0.0) && close(a.1, 270.0), "{a:?}");
        assert!(close(b.0, 400.0) && close(b.1, 30.0), "{b:?}");
    }

    #[test]
    fn a_point_of_the_pane_is_a_point_of_the_plane_under_it() {
        let m = Mapping::of(&diagonal(), [100.0, 60.0, 30.0], &plan_frame());
        let x = [1.0, 0.0, 0.0];
        let y = [0.0, 1.0, 0.0];
        // the middle of the pane is (50, 30): 40 and 10 from the origin (10, 20)
        let (s, t) = m.on_plane((200.0, 150.0), [10.0, 20.0, 0.0], x, y).unwrap();
        assert!(close(s, 40.0) && close(t, 10.0), "{s} {t}");
        // 100 pixels to the right is 25 mm more along the length, 40 pixels down 10 mm less
        let (s, t) = m.on_plane((300.0, 190.0), [10.0, 20.0, 0.0], x, y).unwrap();
        assert!(close(s, 65.0) && close(t, 0.0), "{s} {t}");
    }

    #[test]
    fn the_movement_and_the_zoom_of_the_view_are_undone() {
        let frame = Frame {
            zoom: 2.0,
            pan: (30.0, -20.0),
            ..plan_frame()
        };
        let m = Mapping::of(&diagonal(), [100.0, 60.0, 30.0], &frame);
        // the point (50, 30) is drawn at the middle plus the movement, and gives (50, 30) again
        let (px, _) = m.to_pane([50.0, 30.0, 0.0]);
        assert!(close(px.0, 230.0) && close(px.1, 130.0), "{px:?}");
        let (s, t) = m
            .on_plane(px, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
            .unwrap();
        assert!(close(s, 50.0) && close(t, 30.0), "{s} {t}");
    }

    #[test]
    fn a_plane_seen_edge_on_says_nothing_about_where_a_point_of_the_pane_is() {
        let level = Frame {
            camera: Camera {
                yaw: 0.0,
                pitch: 0.0,
            },
            fit: Fit::Sphere,
            ..plan_frame()
        };
        let m = Mapping::of(&diagonal(), [100.0, 60.0, 30.0], &level);
        let x = [1.0, 0.0, 0.0];
        let y = [0.0, 1.0, 0.0];
        let z = [0.0, 0.0, 1.0];
        assert!(m.on_plane((10.0, 10.0), [0.0; 3], x, y).is_none());
        // the front face, though, is seen face on: x to the right and z up
        let (s, t) = m.on_plane((200.0, 150.0), [0.0; 3], x, z).unwrap();
        assert!(close(s, 50.0) && close(t, 15.0), "{s} {t}");
    }

    #[test]
    fn every_point_of_a_plane_comes_back_from_the_3d_view() {
        let frame = Frame {
            camera: Camera {
                yaw: 35.0,
                pitch: 28.0,
            },
            zoom: 1.7,
            pan: (-12.0, 9.0),
            pane: (640.0, 480.0),
            margin: 24.0,
            fit: Fit::Sphere,
        };
        let m = Mapping::of(&diagonal(), [104.0, 64.0, 35.0], &frame);
        let origin = [2.0, 2.0, 35.0];
        let (a, b) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        for (s, t) in [(0.0, 0.0), (14.5, 15.0), (100.0, 60.0), (-3.0, 7.25)] {
            let p = [origin[0] + s * a[0], origin[1] + t * b[1], origin[2]];
            let (px, _) = m.to_pane(p);
            let (s2, t2) = m.on_plane(px, origin, a, b).unwrap();
            assert!(close(s, s2) && close(t, t2), "{s} {t} -> {s2} {t2}");
        }
    }
}
