// SPDX-License-Identifier: GPL-3.0-or-later
//! A file of another program seen inside this one, with nothing written: a file of triangles
//! (STL, OBJ, 3MF, STEP, PLY, OFF or the faces of a DXF) as a shaded solid with its sharp edges,
//! or a flat DXF drawing as its lines. The file may be one on the disk, an attachment of the
//! project of the suite or the original that an import kept.
//!
//! Everything but the painting is here and has no window: reading (with the limits of the
//! settings, by the readers of the program, which never panic), the facts told about what was read
//! (solids, triangles, size, volume when the surface is closed, whether it fits inside the box),
//! the picture with its hidden surfaces left out, the corner under the pointer and the distance
//! between two corners.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eh3d_project::attachments::AttachmentSource;

use crate::config::{MeshSettings, Settings};
use crate::dxf_import::{self, Import};
use crate::enclosure::Face;
use crate::interchange::{self, ReadLimits};
use crate::lang::{fill, Lang, Msg};
use crate::mesh::Mesh;
use crate::meshcheck::{self, Fit};
use crate::profile::ProfileLimits;
use crate::projection::{rotate, Camera, Point3};
use crate::section::PreviewLine;
use crate::sketch::settings::SketchSettings;
use crate::sketch::{Element, ElementId, SketchPlane};
use crate::stlread::ReadWarning;
use crate::suitewin::IMPORT_EXTENSIONS;
use crate::uiconfig::ModelViewSettings;
use crate::window::error_text;

const NM_PER_MM: f64 = 1.0e6;

/// The side of the square in which the lines of a flat drawing are kept.
const SQUARE: f64 = 1000.0;

/// What bounds the reading and the drawing of a file that is looked at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewOptions {
    /// The limits of the files of triangles.
    pub mesh: MeshSettings,
    /// The most bytes of a file that is looked at.
    pub max_bytes: usize,
    /// The most triangles that are drawn; a larger file is told about and not drawn.
    pub max_triangles: usize,
    /// An edge between two triangles is drawn when they turn by more than this many degrees.
    pub edge_angle: f64,
    /// The distance within which two ends of a flat drawing are one point, in nanometres.
    pub dxf_join_nm: i64,
    /// The most entities of a flat drawing.
    pub dxf_max_entities: usize,
}

impl ViewOptions {
    /// The bounds that the settings give.
    pub fn new(
        settings: &Settings,
        view: &ModelViewSettings,
        sketch: &SketchSettings,
    ) -> ViewOptions {
        ViewOptions {
            mesh: settings.mesh,
            max_bytes: (view.max_mb as usize) << 20,
            max_triangles: view.max_triangles as usize,
            edge_angle: view.edge_angle,
            dxf_join_nm: ProfileLimits::from_settings(sketch).join_nm,
            dxf_max_entities: settings.drawing.max_entities as usize,
        }
    }
}

/// Whether a file of this name is shown here (by its extension, in any letter case).
pub fn can_show(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        IMPORT_EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    })
}

/// One triangle that can be painted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    /// Its corners, by their number in [`Solid::points`].
    pub corners: [u32; 3],
    /// Its unit normal, seen from the side the corners turn counter-clockwise.
    pub normal: Point3,
    /// Which of its sides are sharp edges: bit 0 from the first corner to the second, bit 1 from
    /// the second to the third and bit 2 from the third to the first.
    pub edges: u8,
}

/// What a file of triangles holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Solid {
    /// The corners, in millimetres.
    pub points: Vec<Point3>,
    /// The triangles that have an area; none when there are more than can be drawn.
    pub triangles: Vec<Triangle>,
    /// How many solids the file holds.
    pub bodies: usize,
    /// How many triangles the file holds.
    pub count: usize,
    /// The smallest corner of all of them, in millimetres.
    pub low: Point3,
    /// The largest corner.
    pub high: Point3,
    /// Edges used by one triangle only, over every solid.
    pub open_edges: usize,
    /// Whether every solid is closed and wound one way.
    pub closed: bool,
    /// The volume of the solids in cubic millimetres, when they are closed.
    pub volume_mm3: Option<f64>,
    /// What the reader left out or did not expect.
    pub warnings: Vec<ReadWarning>,
    /// The limit that stopped the drawing, when the file has more triangles than it.
    pub over_limit: Option<usize>,
}

impl Solid {
    /// The size along the three axes, in millimetres.
    pub fn size(&self) -> Point3 {
        [0, 1, 2].map(|k| self.high[k] - self.low[k])
    }

    fn centre(&self) -> Point3 {
        [0, 1, 2].map(|k| (self.high[k] + self.low[k]) / 2.0)
    }

    fn radius(&self) -> f64 {
        let size = self.size();
        (size.iter().map(|v| v * v).sum::<f64>().sqrt() / 2.0).max(1e-6)
    }
}

/// A flat drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct Flat {
    /// What was read.
    pub import: Import,
    /// The size of the drawing along its two axes, in millimetres.
    pub size: [f64; 2],
    /// Where the unit of the drawing came from, as a line for a person.
    pub unit_note: String,
    /// The lines of the drawing in a square of its own, worked out once.
    lines: Vec<PreviewLine>,
}

/// What was read of a file.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// A file of triangles.
    Solid(Solid),
    /// A flat drawing.
    Flat(Flat),
}

fn sub(a: Point3, b: Point3) -> Point3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: Point3, b: Point3) -> Point3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: Point3, b: Point3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: Point3) -> Option<Point3> {
    let length = dot(a, a).sqrt();
    (length > 1e-12).then(|| a.map(|v| v / length))
}

/// Adds the triangles of one solid to what is drawn, with its sharp edges marked: a side used by
/// one triangle or by more than two, or by two whose normals differ by more than the angle.
fn add_body(solid: &mut Solid, mesh: &Mesh, sharp_cosine: f64) {
    let base = solid.points.len() as u32;
    solid
        .points
        .extend(mesh.vertices.iter().map(|v| v.map(|x| x / NM_PER_MM)));
    let first = solid.triangles.len();
    for t in &mesh.triangles {
        let corners = t.map(|v| v + base);
        let [a, b, c] = corners.map(|v| solid.points[v as usize]);
        // a triangle without area has no direction and is not drawn
        if let Some(normal) = unit(cross(sub(b, a), sub(c, a))) {
            solid.triangles.push(Triangle {
                corners,
                normal,
                edges: 0,
            });
        }
    }
    let mut sides: BTreeMap<(u32, u32), Vec<(usize, u8)>> = BTreeMap::new();
    for (index, t) in solid.triangles.iter().enumerate().skip(first) {
        for k in 0..3 {
            let (a, b) = (t.corners[k], t.corners[(k + 1) % 3]);
            sides
                .entry((a.min(b), a.max(b)))
                .or_default()
                .push((index, 1 << k));
        }
    }
    for users in sides.values() {
        let sharp = match users.as_slice() {
            [(one, _), (two, _)] => {
                dot(solid.triangles[*one].normal, solid.triangles[*two].normal) < sharp_cosine
            }
            _ => true,
        };
        if sharp {
            for (index, bit) in users {
                solid.triangles[*index].edges |= bit;
            }
        }
    }
}

fn solid_of(read: interchange::Read, options: &ViewOptions) -> Result<Solid, String> {
    let count: usize = read.bodies.iter().map(|(_, m)| m.triangles.len()).sum();
    if count == 0 {
        return Err("the file has no triangle".to_owned());
    }
    let mut solid = Solid {
        points: Vec::new(),
        triangles: Vec::new(),
        bodies: read.bodies.len(),
        count,
        low: [f64::INFINITY; 3],
        high: [f64::NEG_INFINITY; 3],
        open_edges: 0,
        closed: true,
        volume_mm3: None,
        warnings: read.warnings,
        over_limit: (count > options.max_triangles).then_some(options.max_triangles),
    };
    let sharp_cosine = options.edge_angle.to_radians().cos();
    let mut volume = 0.0;
    for (_, mesh) in &read.bodies {
        // the size is that of the corners the triangles use
        for t in &mesh.triangles {
            for corner in mesh.corners(t) {
                for (k, value) in corner.into_iter().enumerate() {
                    solid.low[k] = solid.low[k].min(value / NM_PER_MM);
                    solid.high[k] = solid.high[k].max(value / NM_PER_MM);
                }
            }
        }
        let report = meshcheck::analyse(mesh);
        solid.open_edges += report.open_edges;
        solid.closed &= report.open_edges == 0
            && report.non_manifold_edges == 0
            && report.inconsistent_edges == 0;
        volume += mesh.volume_nm3().abs() / (NM_PER_MM * NM_PER_MM * NM_PER_MM);
        if solid.over_limit.is_none() {
            add_body(&mut solid, mesh, sharp_cosine);
        }
    }
    solid.volume_mm3 = solid.closed.then_some(volume);
    Ok(solid)
}

/// The size of a flat drawing in millimetres: its points, and the whole circle of every circle and
/// arc.
fn flat_size(import: &Import) -> [f64; 2] {
    let points: BTreeMap<ElementId, [f64; 2]> = import
        .sketch
        .elements()
        .iter()
        .filter_map(|e| match e.element {
            Element::Point { x, y } => Some((e.id, [x.get() as f64, y.get() as f64])),
            _ => None,
        })
        .collect();
    let point = |id: ElementId| points.get(&id).copied();
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    let mut take = |p: [f64; 2], around: f64| {
        for k in 0..2 {
            low[k] = low[k].min(p[k] - around);
            high[k] = high[k].max(p[k] + around);
        }
    };
    for entry in import.sketch.elements() {
        match &entry.element {
            Element::Line { a, b } => {
                for end in [a, b] {
                    if let Some(p) = point(*end) {
                        take(p, 0.0);
                    }
                }
            }
            Element::Circle { center, radius } => {
                if let Some(c) = point(*center) {
                    take(c, radius.get() as f64);
                }
            }
            Element::Arc { center, start, .. } => {
                if let (Some(c), Some(s)) = (point(*center), point(*start)) {
                    take(c, (s[0] - c[0]).hypot(s[1] - c[1]));
                }
            }
            _ => {}
        }
    }
    [0, 1].map(|k| ((high[k] - low[k]) / NM_PER_MM).max(0.0))
}

fn flat_of(name: &str, text: &str, options: &ViewOptions, lang: Lang) -> Result<Flat, String> {
    let say = |e| error_text(&e, lang);
    let (unit_nm, unit_note) = dxf_import::auto_unit(text, lang).map_err(say)?;
    let import = dxf_import::from_dxf(
        text,
        name,
        SketchPlane::on_face(Face::Top),
        unit_nm,
        options.dxf_join_nm,
        options.dxf_max_entities,
    )
    .map_err(say)?;
    Ok(Flat {
        size: flat_size(&import),
        lines: dxf_import::preview(&import, SQUARE, SQUARE, 0.0, 72),
        import,
        unit_note,
    })
}

/// Reads what the bytes of a file named `name` hold. A DXF without faces is read as a flat
/// drawing. The words of a refusal are for a person.
pub fn load(
    name: &str,
    bytes: &[u8],
    options: &ViewOptions,
    lang: Lang,
) -> Result<Content, String> {
    if !can_show(name) {
        return Err(Msg::MvNotThis.tr(lang).to_owned());
    }
    if bytes.len() > options.max_bytes {
        let megabytes = (options.max_bytes >> 20).to_string();
        return Err(fill(Msg::MvTooBig.tr(lang), &[&megabytes]));
    }
    let limits = ReadLimits::from_settings(&options.mesh);
    let read = interchange::read_file_checked(name, bytes, &limits);
    let solid = read.and_then(|read| solid_of(read, options));
    let flat = name.to_ascii_lowercase().ends_with(".dxf");
    match solid {
        Ok(solid) => Ok(Content::Solid(solid)),
        Err(why) if !flat => Err(why),
        Err(_) => {
            let text =
                std::str::from_utf8(bytes).map_err(|_| "the file is not UTF-8 text".to_owned())?;
            let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
            flat_of(stem, text, options, lang).map(Content::Flat)
        }
    }
}

fn millimetres(value: f64) -> String {
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// What is told about what was read, one line each. With the inner space of the box (length,
/// width and height in millimetres) a solid is also said to fit in it or not.
pub fn facts(content: &Content, inner: Option<Point3>, lang: Lang) -> Vec<String> {
    let mut out = Vec::new();
    match content {
        Content::Solid(solid) => {
            out.push(fill(
                Msg::MvCounts.tr(lang),
                &[&solid.bodies.to_string(), &solid.count.to_string()],
            ));
            let [x, y, z] = solid.size().map(millimetres);
            out.push(fill(Msg::MvSize.tr(lang), &[&x, &y, &z]));
            match solid.volume_mm3 {
                Some(volume) => out.push(fill(
                    Msg::MvVolume.tr(lang),
                    &[&millimetres(volume / 1000.0)],
                )),
                None => out.push(fill(Msg::MvOpen.tr(lang), &[&solid.open_edges.to_string()])),
            }
            if let Some(inner) = inner {
                let words = match meshcheck::fits(solid.size(), inner) {
                    Fit::Fits => Msg::MvFits,
                    Fit::Turned => Msg::MvFitsTurned,
                    Fit::No => Msg::MvNoFit,
                };
                let [x, y, z] = inner.map(millimetres);
                out.push(fill(words.tr(lang), &[&x, &y, &z]));
            }
            if let Some(limit) = solid.over_limit {
                out.push(fill(
                    Msg::MvTooMany.tr(lang),
                    &[&solid.count.to_string(), &limit.to_string()],
                ));
            }
            out.extend(
                solid
                    .warnings
                    .iter()
                    .map(|w| crate::mesh_cli::warning_text(w, lang)),
            );
        }
        Content::Flat(flat) => {
            let [x, y] = flat.size.map(millimetres);
            out.push(fill(Msg::MvFlatSize.tr(lang), &[&x, &y]));
            out.extend(
                dxf_import::report_text(&flat.import, lang)
                    .lines()
                    .map(str::to_owned),
            );
            out.push(flat.unit_note.clone());
        }
    }
    out
}

/// How a solid or a drawing is looked at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// The point of view (a flat drawing is not turned).
    pub camera: Camera,
    /// How much larger than fitted to the pane.
    pub zoom: f64,
    /// How far the picture is moved from the middle of the pane, in pixels.
    pub pan: (f64, f64),
}

impl Pose {
    /// The pose that fits what is shown into the pane, seen from a camera.
    pub fn fitted(camera: Camera) -> Pose {
        Pose {
            camera,
            zoom: 1.0,
            pan: (0.0, 0.0),
        }
    }

    /// Turns by a drag of so many degrees; the view can go under the solid, not past straight
    /// down or straight up.
    pub fn turn(&mut self, yaw: f64, pitch: f64) {
        self.camera.yaw = (self.camera.yaw + yaw).rem_euclid(360.0);
        self.camera.pitch = (self.camera.pitch + pitch).clamp(-90.0, 90.0);
    }

    /// Multiplies the zoom, kept between a twentieth and fifty times the fitted size.
    pub fn zoom_by(&mut self, factor: f64) {
        if factor.is_finite() && factor > 0.0 {
            self.zoom = (self.zoom * factor).clamp(0.05, 50.0);
        }
    }
}

/// The six straight views and the point of view of each.
pub const STRAIGHT_VIEWS: [(Msg, Camera); 6] = [
    (
        Msg::FaceFront,
        Camera {
            yaw: 0.0,
            pitch: 0.0,
        },
    ),
    (
        Msg::FaceBack,
        Camera {
            yaw: 180.0,
            pitch: 0.0,
        },
    ),
    (
        Msg::FaceLeft,
        Camera {
            yaw: 90.0,
            pitch: 0.0,
        },
    ),
    (
        Msg::FaceRight,
        Camera {
            yaw: 270.0,
            pitch: 0.0,
        },
    ),
    (Msg::FaceTop, Camera::TOP),
    (
        Msg::FaceBottom,
        Camera {
            yaw: 0.0,
            pitch: -90.0,
        },
    ),
];

/// A corner on the pane: to the right and down from its upper left corner in pixels, and the
/// depth (larger is farther from the person).
fn on_pane(solid: &Solid, pose: &Pose, pane: (f64, f64), corner: Point3) -> (f64, f64, f64) {
    let scale = 0.45 * pane.0.min(pane.1).max(1.0) / solid.radius() * pose.zoom;
    let (right, up, depth) = rotate(corner, solid.centre(), pose.camera);
    (
        pane.0 / 2.0 + pose.pan.0 + right * scale,
        pane.1 / 2.0 + pose.pan.1 - up * scale,
        depth,
    )
}

/// The light colour of a face that is fully lit.
const FACE: [f64; 3] = [150.0, 196.0, 224.0];
/// The colour of a sharp edge.
const EDGE: [u8; 4] = [10, 20, 28, 255];

/// The widest and tallest picture that is worked out.
pub const PICTURE_MAX: usize = 4096;

/// The picture of a solid: its pixels and how far each one is from the person.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    /// The pixels across.
    pub width: usize,
    /// The pixels down.
    pub height: usize,
    /// Red, green, blue and alpha of every pixel, row after row; nothing is drawn where alpha
    /// is 0.
    pub rgba: Vec<u8>,
    /// The depth of the nearest surface at every pixel, infinite where there is none.
    depth: Vec<f64>,
}

impl Picture {
    /// The colour of a pixel; transparent outside the picture.
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let mut out = [0; 4];
        if x < self.width && y < self.height {
            let at = (y * self.width + x) * 4;
            out.copy_from_slice(&self.rgba[at..at + 4]);
        }
        out
    }

    /// Whether a point at this place and depth is not behind a surface, with `slack` of depth,
    /// looking also at the pixels around it (a corner sits on the rim of its faces).
    fn sees(&self, x: f64, y: f64, depth: f64, slack: f64) -> bool {
        if x < 0.0 || y < 0.0 || x >= self.width as f64 || y >= self.height as f64 {
            return false;
        }
        let (x, y) = (x as usize, y as usize);
        for v in y.saturating_sub(1)..=(y + 1).min(self.height - 1) {
            for u in x.saturating_sub(1)..=(x + 1).min(self.width - 1) {
                if depth <= self.depth[v * self.width + u] + slack {
                    return true;
                }
            }
        }
        false
    }

    fn put(&mut self, at: usize, colour: [u8; 4]) {
        self.rgba[at * 4..at * 4 + 4].copy_from_slice(&colour);
    }

    /// Fills a triangle given by its corners (across, down, depth) where it is the nearest.
    fn fill(&mut self, [a, b, c]: [(f64, f64, f64); 3], colour: [u8; 4]) {
        let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        if area.abs() < 1e-12 {
            return;
        }
        let low_x = a.0.min(b.0).min(c.0).floor().max(0.0) as usize;
        let low_y = a.1.min(b.1).min(c.1).floor().max(0.0) as usize;
        let high_x = (a.0.max(b.0).max(c.0).ceil().max(0.0) as usize).min(self.width);
        let high_y = (a.1.max(b.1).max(c.1).ceil().max(0.0) as usize).min(self.height);
        for y in low_y..high_y {
            for x in low_x..high_x {
                // the middle of the pixel, in the shares of the three corners
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let wa = ((b.0 - px) * (c.1 - py) - (b.1 - py) * (c.0 - px)) / area;
                let wb = ((c.0 - px) * (a.1 - py) - (c.1 - py) * (a.0 - px)) / area;
                let wc = 1.0 - wa - wb;
                if wa < -1e-9 || wb < -1e-9 || wc < -1e-9 {
                    continue;
                }
                let depth = wa * a.2 + wb * b.2 + wc * c.2;
                let at = y * self.width + x;
                if depth < self.depth[at] {
                    self.depth[at] = depth;
                    self.put(at, colour);
                }
            }
        }
    }

    /// Draws a side one pixel wide where it is not behind a surface.
    fn stroke(&mut self, a: (f64, f64, f64), b: (f64, f64, f64), slack: f64) {
        let longest = (b.0 - a.0).abs().max((b.1 - a.1).abs());
        // a side far longer than the picture (a very close zoom) is walked in bounded steps
        let steps = (longest.ceil() as usize).clamp(1, 4 * PICTURE_MAX);
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            let x = a.0 + (b.0 - a.0) * t;
            let y = a.1 + (b.1 - a.1) * t;
            if x < 0.0 || y < 0.0 || x >= self.width as f64 || y >= self.height as f64 {
                continue;
            }
            let depth = a.2 + (b.2 - a.2) * t;
            let at = y as usize * self.width + x as usize;
            if depth <= self.depth[at] + slack {
                self.put(at, EDGE);
            }
        }
    }
}

/// The depth by which an edge or a corner may be behind the surface it belongs to.
fn slack(solid: &Solid) -> f64 {
    solid.radius() * 0.01
}

/// The picture of a solid, `width` by `height` pixels: every pixel shows the surface that is
/// nearest to the person, lit by its direction, and with `edges` the sharp edges that are not
/// hidden. A closed solid leaves out the triangles that look away; an open surface is lit from
/// both sides.
pub fn render(solid: &Solid, pose: &Pose, width: usize, height: usize, edges: bool) -> Picture {
    let width = width.clamp(1, PICTURE_MAX);
    let height = height.clamp(1, PICTURE_MAX);
    let mut out = Picture {
        width,
        height,
        rgba: vec![0; width * height * 4],
        depth: vec![f64::INFINITY; width * height],
    };
    // the light comes from over the left shoulder of the person
    let light = unit([-0.35, 0.55, -0.75]).unwrap_or([0.0, 0.0, -1.0]);
    let pane = (width as f64, height as f64);
    let corners: Vec<(f64, f64, f64)> = solid
        .points
        .iter()
        .map(|p| on_pane(solid, pose, pane, *p))
        .collect();
    let mut sides = Vec::new();
    for t in &solid.triangles {
        let (right, up, depth) = rotate(t.normal, [0.0; 3], pose.camera);
        let away = depth > 1e-9;
        if away && solid.closed {
            continue;
        }
        let side = if away { -1.0 } else { 1.0 };
        let lit = dot([right * side, up * side, depth * side], light).max(0.0);
        let shade = 0.25 + 0.75 * lit;
        let [r, g, b] = FACE.map(|v| (v * shade).clamp(0.0, 255.0) as u8);
        let at = t.corners.map(|v| corners[v as usize]);
        out.fill(at, [r, g, b, 255]);
        for k in 0..3 {
            if edges && t.edges & (1 << k) != 0 {
                sides.push((at[k], at[(k + 1) % 3]));
            }
        }
    }
    // the edges after every face, so that a face drawn later does not cut them in half
    let slack = slack(solid);
    for (a, b) in sides {
        out.stroke(a, b, slack);
    }
    out
}

/// The corner of the solid within `radius` pixels of a place of the picture that is not hidden:
/// of those, the one nearest to the place, and of two on one pixel the one nearer to the person.
pub fn pick(
    solid: &Solid,
    pose: &Pose,
    picture: &Picture,
    at: (f64, f64),
    radius: f64,
) -> Option<Point3> {
    let pane = (picture.width as f64, picture.height as f64);
    let slack = slack(solid);
    let mut found: Option<(f64, f64, Point3)> = None;
    for t in &solid.triangles {
        for v in t.corners {
            let corner = solid.points[v as usize];
            let (x, y, depth) = on_pane(solid, pose, pane, corner);
            let apart = (x - at.0).hypot(y - at.1);
            if apart > radius || !picture.sees(x, y, depth, slack) {
                continue;
            }
            let better = found
                .is_none_or(|(a, d, _)| apart < a - 0.5 || ((apart - a).abs() <= 0.5 && depth < d));
            if better {
                found = Some((apart, depth, corner));
            }
        }
    }
    found.map(|(_, _, corner)| corner)
}

/// Where a corner of the solid is on the pane.
pub fn place_of(solid: &Solid, pose: &Pose, pane: (f64, f64), corner: Point3) -> (f64, f64) {
    let (x, y, _) = on_pane(solid, pose, pane, corner);
    (x, y)
}

/// The words of the distance between two corners.
pub fn distance_text(a: Point3, b: Point3, lang: Lang) -> String {
    let d = sub(b, a);
    let all = dot(d, d).sqrt();
    fill(
        Msg::MeasureCliDistance.tr(lang),
        &[
            &millimetres(all),
            &millimetres(d[0].abs()),
            &millimetres(d[1].abs()),
            &millimetres(d[2].abs()),
        ],
    )
}

/// A line of a flat drawing on the pane, and whether it belongs to a circle.
pub type FlatLine = ((f64, f64), (f64, f64), bool);

/// The lines of a flat drawing on the pane: fitted into it with a margin, then zoomed about the
/// middle and moved as the pose says.
pub fn flat_lines(flat: &Flat, pose: &Pose, pane: (f64, f64)) -> Vec<FlatLine> {
    let mut low = (f64::INFINITY, f64::INFINITY);
    let mut high = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for end in flat.lines.iter().flat_map(|l| [l.a, l.b]) {
        low = (low.0.min(end.0), low.1.min(end.1));
        high = (high.0.max(end.0), high.1.max(end.1));
    }
    let room = ((pane.0 - 24.0).max(1.0), (pane.1 - 24.0).max(1.0));
    let across = (high.0 - low.0).max(1e-9);
    let down = (high.1 - low.1).max(1e-9);
    let scale = (room.0 / across).min(room.1 / down) * pose.zoom;
    let middle = ((low.0 + high.0) / 2.0, (low.1 + high.1) / 2.0);
    let to_pane = |p: (f64, f64)| {
        (
            pane.0 / 2.0 + pose.pan.0 + (p.0 - middle.0) * scale,
            pane.1 / 2.0 + pose.pan.1 + (p.1 - middle.1) * scale,
        )
    };
    flat.lines
        .iter()
        .map(|l| (to_pane(l.a), to_pane(l.b), l.hole))
        .collect()
}

/// Where the bytes that are looked at come from.
#[derive(Debug, Clone)]
pub enum Origin {
    /// A file on the disk.
    File(PathBuf),
    /// An attachment inside the project of the suite.
    Attachment(Box<AttachmentSource>),
    /// Bytes already in memory under a name (the original that an import kept).
    Bytes(String, Vec<u8>),
}

impl Origin {
    /// The name of the file, without its folders.
    pub fn name(&self) -> String {
        match self {
            Origin::File(path) => file_name(path),
            Origin::Attachment(source) => source.name().to_owned(),
            Origin::Bytes(name, _) => name.clone(),
        }
    }

    /// The bytes, never more than one past `limit` of them.
    fn read(&self, limit: usize) -> Result<Vec<u8>, String> {
        let most = (limit as u64).saturating_add(1);
        let mut out = Vec::new();
        let done = match self {
            Origin::File(path) => {
                std::fs::File::open(path).and_then(|f| f.take(most).read_to_end(&mut out))
            }
            Origin::Attachment(source) => source
                .reader()
                .and_then(|r| r.take(most).read_to_end(&mut out)),
            Origin::Bytes(_, bytes) => return Ok(bytes.clone()),
        };
        done.map(|_| out).map_err(|e| e.to_string())
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A file being read on a thread of its own, so that a large one does not stop the window.
#[derive(Debug)]
pub struct Job {
    answer: Receiver<Result<Content, String>>,
}

impl Job {
    /// Starts reading what comes from `origin`.
    pub fn start(origin: Origin, options: ViewOptions, lang: Lang) -> Job {
        let (send, answer) = mpsc::channel();
        std::thread::spawn(move || {
            let name = origin.name();
            let done = if can_show(&name) {
                origin
                    .read(options.max_bytes)
                    .and_then(|bytes| load(&name, &bytes, &options, lang))
            } else {
                Err(Msg::MvNotThis.tr(lang).to_owned())
            };
            // the window may have been closed meanwhile
            let _ = send.send(done);
        });
        Job { answer }
    }

    /// What was read, waiting for it.
    pub fn wait(self) -> Result<Content, String> {
        self.answer
            .recv()
            .unwrap_or_else(|_| Err("the file could not be read".to_owned()))
    }

    /// What was read, once it is there.
    pub fn poll(&self) -> Option<Result<Content, String>> {
        match self.answer.try_recv() {
            Ok(done) => Some(done),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the file could not be read".to_owned())),
        }
    }
}

#[cfg(test)]
#[path = "modelview_tests.rs"]
mod tests;
