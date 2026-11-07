//! The settings of the window: the size it opens at, how the views turn and zoom, how a hole or a
//! post is dragged, how things are measured, how the files written from the window are made, the
//! box a new project starts with and the envelope checked when a project has no board.
//!
//! They live in `phoenix-cad-window.default.cfg`, the same form as the settings of the library;
//! the code holds none of the values. A user file only needs the keys it changes.

use crate::catalog::Axis;
use crate::config::Config;
use crate::enclosure::{Enclosure, EnclosureParams, Face};
use crate::export::Format;
use crate::fins::FinAxis;
use crate::mount::BoreKind;
use crate::units::{parse_angle, Angle, Unit};
use crate::{CadError, Length};

/// The shipped settings of the window, as text.
pub const DEFAULT_UI_CONFIG: &str = include_str!("../phoenix-cad-window.default.cfg");

/// What a step added from the window starts with. The position of a hole or a post moves along
/// by `pitch` for each try until the model accepts it, so that a second step does not land on the
/// first.
#[derive(Debug, Clone, PartialEq)]
pub struct NewStepDefaults {
    /// How many positions are tried before the step is refused.
    pub tries: u32,
    /// The distance between two tries.
    pub pitch: Length,
    /// The face of a new hole.
    pub hole_face: Face,
    /// The position `u` of a new hole.
    pub hole_u: Length,
    /// The position `v` of a new hole.
    pub hole_v: Length,
    /// The diameter of a new hole.
    pub hole_diameter: Length,
    /// The position `x` of a new post.
    pub post_x: Length,
    /// The position `y` of a new post.
    pub post_y: Length,
    /// The outer diameter of a new post.
    pub post_outer: Length,
    /// The height of a new post.
    pub post_height: Length,
    /// What the bore of a new post is for.
    pub post_kind: BoreKind,
    /// The diameter of the bore of a new post.
    pub post_bore_diameter: Length,
    /// The depth of the bore of a new post.
    pub post_bore_depth: Length,
    /// The height of a new extrusion.
    pub extrude_height: Length,
    /// The depth of a new pocket; zero cuts through the face.
    pub pocket_depth: Length,
    /// How many positions a new pattern has.
    pub pattern_count: u32,
    /// The step between two positions of a new line of holes or posts, along `u` or `x`.
    pub pattern_step: Length,
    /// The distance from the seed to the centre of a pattern that is changed to a circle, along
    /// both axes (so that the positions of a circle of four make a square the seed is a corner of).
    pub pattern_radius: Length,
    /// The angle between two positions of a circle.
    pub pattern_angle: Angle,
    /// The face of a new set of cooling fins.
    pub fin_face: Face,
    /// The direction the fins of a new set run.
    pub fin_along: FinAxis,
    /// How many fins a new set has.
    pub fin_count: u32,
    /// The thickness of a fin of a new set.
    pub fin_thickness: Length,
    /// The height of a fin of a new set.
    pub fin_height: Length,
    /// The distance from the middle of one fin to the middle of the next in a new set.
    pub fin_pitch: Length,
    /// Where the first fin of a new set is, across the direction they run.
    pub fin_first: Length,
    /// Where the fins of a new set start, along the direction they run.
    pub fin_start: Length,
    /// The length of a fin of a new set.
    pub fin_length: Length,
    /// The position `x` of a new part of the catalog (the smallest corner of its bounding box).
    pub part_x: Length,
    /// The position `y` of a new part of the catalog.
    pub part_y: Length,
    /// The position `z` of a new part of the catalog.
    pub part_z: Length,
    /// The direction the length of a new part of the catalog runs along.
    pub part_axis: Axis,
    /// The roll of a new part about its length, in quarter turns (0 to 3).
    pub part_roll: u8,
}

/// How a hole or a post is dragged with the mouse in the views.
#[derive(Debug, Clone, PartialEq)]
pub struct DragSettings {
    /// Whether a dragged centre snaps to the grid when the window opens.
    pub snap: bool,
    /// The grid a dragged centre snaps to; zero means no grid.
    pub grid: Length,
    /// How near to the drawn centre, in pixels, a press has to be to pick the hole or the post.
    pub hit_radius: f64,
}

/// How the measurement tools of the views read and keep what they measure.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasureSettings {
    /// The unit of a length; `None` is the unit shown everywhere else (`units.display`).
    pub unit: Option<Unit>,
    /// The decimals of a length.
    pub decimals: u32,
    /// The decimals of an angle, in degrees.
    pub angle_decimals: u32,
    /// How near to a drawn point or edge, in pixels, a click has to be to pick it.
    pub pick_radius: f64,
    /// How many readings are kept; the oldest is dropped.
    pub keep: u32,
    /// The size of the cross drawn on a point that is picked or measured.
    pub marker: Length,
}

/// How the files written from the window are made.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportSettings {
    /// The format the export dialog starts with.
    pub format: Format,
    /// Whether the box to write over an existing file is ticked when the window opens; the
    /// shipped value is no.
    pub overwrite: bool,
    /// The largest text written, in KiB.
    pub max_kib: u32,
}

/// The settings of the window with their types.
#[derive(Debug, Clone, PartialEq)]
pub struct UiSettings {
    /// The size of the window when it opens, in pixels.
    pub window_size: (u32, u32),
    /// The smallest size of the window, in pixels.
    pub window_min: (u32, u32),
    /// The turn of the 3D view about the vertical axis, in degrees.
    pub yaw: f64,
    /// The angle of the 3D view above the horizon, in degrees.
    pub pitch: f64,
    /// The smallest zoom of a view.
    pub zoom_min: f64,
    /// The largest zoom of a view.
    pub zoom_max: f64,
    /// Degrees of turn for each pixel dragged.
    pub rotate_speed: f64,
    /// The zoom for each unit of the mouse wheel.
    pub wheel_speed: f64,
    /// The segments of the circle that draws a hole or a post.
    pub circle_segments: u32,
    /// The pixels left empty around a drawing.
    pub margin: f64,
    /// The box a new project starts with.
    pub new_box: EnclosureParams,
    /// The length, the width and the height of the envelope checked without a board.
    pub fit: [Length; 3],
    /// The size given to an edge that goes from sharp to rounded or chamfered.
    pub edge_size: Length,
    /// What a step added from the window starts with.
    pub new_step: NewStepDefaults,
    /// How a hole or a post is dragged.
    pub drag: DragSettings,
    /// How things are measured.
    pub measure: MeasureSettings,
    /// How the files written from the window are made.
    pub export: ExportSettings,
}

fn missing(key: &str) -> CadError {
    CadError::Config(format!("{key} is missing"))
}

fn real(c: &Config, key: &str, low: f64, high: f64) -> Result<f64, CadError> {
    let text = c.get(key).ok_or_else(|| missing(key))?;
    let value: f64 = text
        .parse()
        .map_err(|_| CadError::Config(format!("{key} is not a number")))?;
    if value.is_finite() && (low..=high).contains(&value) {
        Ok(value)
    } else {
        Err(CadError::Config(format!(
            "{key} must be from {low} to {high}"
        )))
    }
}

fn count(c: &Config, key: &str, low: u32, high: u32) -> Result<u32, CadError> {
    let value = c.number(key)?;
    if (low..=high).contains(&value) {
        Ok(value)
    } else {
        Err(CadError::Config(format!(
            "{key} must be from {low} to {high}"
        )))
    }
}

fn word<'a>(c: &'a Config, key: &str) -> Result<&'a str, CadError> {
    c.get(key).ok_or_else(|| missing(key))
}

fn positive(c: &Config, key: &str) -> Result<Length, CadError> {
    let value = c.length(key)?;
    if value.get() > 0 {
        Ok(value)
    } else {
        Err(CadError::Config(format!("{key} must be positive")))
    }
}

impl NewStepDefaults {
    fn from_config(c: &Config) -> Result<NewStepDefaults, CadError> {
        let face = word(c, "new_step.hole_face")?;
        let kind = word(c, "new_step.post_kind")?;
        let fin_face = word(c, "new_step.fin_face")?;
        let fin_along = word(c, "new_step.fin_along")?;
        let part_axis = word(c, "new_step.part_axis")?;
        let part_roll = count(c, "new_step.part_roll", 0, 270)?;
        if part_roll % 90 != 0 {
            return Err(CadError::Config(
                "new_step.part_roll must be 0, 90, 180 or 270".into(),
            ));
        }
        Ok(NewStepDefaults {
            tries: count(c, "new_step.tries", 1, 1000)?,
            pitch: c.length("new_step.pitch")?,
            hole_face: Face::from_name(face).ok_or_else(|| {
                CadError::Config(format!("new_step.hole_face: {face} is not a face"))
            })?,
            hole_u: c.length("new_step.hole_u")?,
            hole_v: c.length("new_step.hole_v")?,
            hole_diameter: c.length("new_step.hole_diameter")?,
            post_x: c.length("new_step.post_x")?,
            post_y: c.length("new_step.post_y")?,
            post_outer: c.length("new_step.post_outer")?,
            post_height: c.length("new_step.post_height")?,
            post_kind: BoreKind::from_name(kind).ok_or_else(|| {
                CadError::Config(format!(
                    "new_step.post_kind: {kind} is not insert or self-tapping"
                ))
            })?,
            post_bore_diameter: c.length("new_step.post_bore_diameter")?,
            post_bore_depth: c.length("new_step.post_bore_depth")?,
            extrude_height: c.length("new_step.extrude_height")?,
            pocket_depth: c.length("new_step.pocket_depth")?,
            pattern_count: count(c, "new_step.pattern_count", 2, 1000)?,
            pattern_step: positive(c, "new_step.pattern_step")?,
            pattern_radius: positive(c, "new_step.pattern_radius")?,
            pattern_angle: {
                let key = "new_step.pattern_angle";
                let angle = parse_angle(word(c, key)?)
                    .map_err(|e| CadError::Config(format!("{key}: {e}")))?;
                if angle == Angle::ZERO {
                    return Err(CadError::Config(format!("{key} must not be zero")));
                }
                angle
            },
            fin_face: Face::from_name(fin_face).ok_or_else(|| {
                CadError::Config(format!("new_step.fin_face: {fin_face} is not a face"))
            })?,
            fin_along: FinAxis::from_name(fin_along).ok_or_else(|| {
                CadError::Config(format!("new_step.fin_along: {fin_along} is not u or v"))
            })?,
            fin_count: count(c, "new_step.fin_count", 1, 1000)?,
            fin_thickness: positive(c, "new_step.fin_thickness")?,
            fin_height: positive(c, "new_step.fin_height")?,
            fin_pitch: positive(c, "new_step.fin_pitch")?,
            fin_first: c.length("new_step.fin_first")?,
            fin_start: c.length("new_step.fin_start")?,
            fin_length: positive(c, "new_step.fin_length")?,
            part_x: c.length("new_step.part_x")?,
            part_y: c.length("new_step.part_y")?,
            part_z: c.length("new_step.part_z")?,
            part_axis: Axis::from_name(part_axis).ok_or_else(|| {
                CadError::Config(format!(
                    "new_step.part_axis: {part_axis} is not +x, -x, +y, -y, +z or -z"
                ))
            })?,
            part_roll: u8::try_from(part_roll / 90).unwrap_or(0),
        })
    }
}

impl DragSettings {
    fn from_config(c: &Config) -> Result<DragSettings, CadError> {
        let grid = c.length("drag.grid")?;
        if grid.get() < 0 || grid.get() > 1_000_000_000 {
            return Err(CadError::Config(
                "drag.grid must be from 0 mm to 1000 mm".into(),
            ));
        }
        Ok(DragSettings {
            snap: count(c, "drag.snap", 0, 1)? == 1,
            grid,
            hit_radius: real(c, "drag.hit_radius", 1.0, 200.0)?,
        })
    }
}

impl MeasureSettings {
    fn from_config(c: &Config) -> Result<MeasureSettings, CadError> {
        let unit = match word(c, "measure.unit")? {
            "display" => None,
            other => Some(Unit::from_suffix(other).ok_or_else(|| {
                CadError::Config(format!("measure.unit: {other} is not a unit or display"))
            })?),
        };
        Ok(MeasureSettings {
            unit,
            decimals: count(c, "measure.decimals", 0, 12)?,
            angle_decimals: count(c, "measure.angle_decimals", 0, 9)?,
            pick_radius: real(c, "measure.pick_radius", 1.0, 200.0)?,
            keep: count(c, "measure.keep", 1, 1000)?,
            marker: c.length("measure.marker")?,
        })
    }
}

impl ExportSettings {
    fn from_config(c: &Config) -> Result<ExportSettings, CadError> {
        let name = word(c, "export.format")?;
        Ok(ExportSettings {
            format: Format::from_name(name).ok_or_else(|| {
                CadError::Config(format!(
                    "export.format: {name} is not script, step or report"
                ))
            })?,
            overwrite: count(c, "export.overwrite", 0, 1)? == 1,
            max_kib: count(c, "export.max_kib", 1, 1_048_576)?,
        })
    }
}

impl UiSettings {
    /// The defaults that ship with the crate.
    pub fn defaults() -> UiSettings {
        let config = Config::parse(DEFAULT_UI_CONFIG).expect("the shipped settings are valid");
        UiSettings::from_config(&config).expect("the shipped settings are complete")
    }

    /// The defaults with the keys of a user file put over them. Keys of other parts (the
    /// settings of the library) are accepted and ignored here.
    pub fn with_user_config(text: &str) -> Result<UiSettings, CadError> {
        let mut config = Config::parse(DEFAULT_UI_CONFIG)?;
        config.overlay(&Config::parse(text)?);
        UiSettings::from_config(&config)
    }

    /// Reads the typed settings from a complete configuration.
    pub fn from_config(c: &Config) -> Result<UiSettings, CadError> {
        let window_size = (
            count(c, "window.width", 320, 8192)?,
            count(c, "window.height", 240, 8192)?,
        );
        let window_min = (
            count(c, "window.min_width", 200, 8192)?,
            count(c, "window.min_height", 150, 8192)?,
        );
        let new_box = EnclosureParams {
            inner_length: c.length("new_box.inner_length")?,
            inner_width: c.length("new_box.inner_width")?,
            inner_height: c.length("new_box.inner_height")?,
            wall: c.length("new_box.wall")?,
            floor: c.length("new_box.floor")?,
            lid: c.length("new_box.lid")?,
            edge_margin: c.length("new_box.edge_margin")?,
            min_web: c.length("new_box.min_web")?,
        };
        Enclosure::new(new_box).map_err(|e| CadError::Config(format!("new_box: {e}")))?;
        let zoom_min = real(c, "view.zoom_min", 0.01, 1.0)?;
        let zoom_max = real(c, "view.zoom_max", 1.0, 1000.0)?;
        Ok(UiSettings {
            window_size,
            window_min,
            yaw: real(c, "view.yaw", -360.0, 360.0)?,
            pitch: real(c, "view.pitch", 0.0, 90.0)?,
            zoom_min,
            zoom_max,
            rotate_speed: real(c, "view.rotate_speed", 0.01, 10.0)?,
            wheel_speed: real(c, "view.wheel_speed", 0.00001, 0.1)?,
            circle_segments: count(c, "view.circle_segments", 8, 256)?,
            margin: real(c, "view.margin", 0.0, 400.0)?,
            new_box,
            fit: [
                c.length("fit.length")?,
                c.length("fit.width")?,
                c.length("fit.height")?,
            ],
            edge_size: c.length("edges.size")?,
            new_step: NewStepDefaults::from_config(c)?,
            drag: DragSettings::from_config(c)?,
            measure: MeasureSettings::from_config(c)?,
            export: ExportSettings::from_config(c)?,
        })
    }
}

/// The text of the shipped settings of the window, to write them to a file.
pub fn default_file() -> &'static str {
    DEFAULT_UI_CONFIG
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    #[test]
    fn the_shipped_file_gives_the_defaults() {
        let s = UiSettings::defaults();
        assert_eq!(s.window_size, (1180, 760));
        assert_eq!(s.window_min, (760, 480));
        assert_eq!((s.yaw, s.pitch), (35.0, 28.0));
        assert_eq!((s.zoom_min, s.zoom_max), (0.2, 12.0));
        assert_eq!(s.circle_segments, 32);
        assert_eq!(s.new_box.inner_length, mm(100));
        assert_eq!(s.new_box.min_web, mm(2));
        assert_eq!(s.fit, [mm(90), mm(50), mm(20)]);
        assert_eq!(s.edge_size, mm(1));
        let n = &s.new_step;
        assert_eq!((n.tries, n.pitch), (12, mm(12)));
        assert_eq!(n.hole_face, Face::Front);
        assert_eq!(
            (n.hole_u, n.hole_v, n.hole_diameter),
            (mm(14), mm(15), mm(6))
        );
        assert_eq!(n.post_kind, BoreKind::Insert);
        assert_eq!(n.post_bore_diameter, Length::new(4_200_000).unwrap());
        assert!(s.drag.snap);
        assert_eq!(s.drag.grid, Length::new(500_000).unwrap());
        assert_eq!(s.drag.hit_radius, 12.0);
        assert_eq!(s.measure.unit, None);
        assert_eq!((s.measure.decimals, s.measure.angle_decimals), (3, 2));
        assert_eq!((s.measure.pick_radius, s.measure.keep), (10.0, 16));
        assert_eq!(s.measure.marker, Length::new(1_500_000).unwrap());
        assert_eq!(s.export.format, Format::Script);
        assert!(!s.export.overwrite);
        assert_eq!(s.export.max_kib, 65_536);
        assert_eq!((n.pattern_count, n.pattern_step), (4, mm(10)));
        assert_eq!(n.pattern_radius, mm(5));
        assert_eq!(n.pattern_angle, Angle::from_deg(90).unwrap());
        assert_eq!(
            (n.fin_face, n.fin_along, n.fin_count),
            (Face::Top, FinAxis::U, 5)
        );
        assert_eq!(
            (n.fin_thickness, n.fin_height, n.fin_pitch),
            (mm(2), mm(15), mm(8))
        );
        assert_eq!(
            (n.fin_first, n.fin_start, n.fin_length),
            (mm(12), mm(10), mm(80))
        );
        assert_eq!((n.part_x, n.part_y, n.part_z), (mm(0), mm(0), mm(0)));
        assert_eq!((n.part_axis, n.part_roll), (Axis::PlusZ, 0));
    }

    #[test]
    fn the_export_is_read_and_checked() {
        let s = UiSettings::with_user_config(
            "[export]
format = step
overwrite = 1
max_kib = 512
",
        )
        .unwrap();
        assert_eq!(s.export.format, Format::Step);
        assert!(s.export.overwrite);
        assert_eq!(s.export.max_kib, 512);
        let report = UiSettings::with_user_config(
            "[export]
format = report
",
        )
        .unwrap();
        assert_eq!(report.export.format, Format::Report);
        // 1 KiB is the least and 1 048 576 KiB (1 GiB) the most
        assert_eq!(
            UiSettings::with_user_config(
                "[export]
max_kib = 1
"
            )
            .unwrap()
            .export
            .max_kib,
            1
        );
        for text in [
            "[export]
format = stl
",
            "[export]
format = 
",
            "[export]
overwrite = 2
",
            "[export]
max_kib = 0
",
            "[export]
max_kib = 1048577
",
            "[export]
max_kib = big
",
        ] {
            let error = UiSettings::with_user_config(text).unwrap_err();
            assert_eq!(error.code(), "PHOENIX-CAD-011", "{text:?}: {error}");
        }
    }

    #[test]
    fn the_dragging_and_the_measuring_are_read_and_checked() {
        let s = UiSettings::with_user_config(
            "[drag]
snap = 0
grid = 1/16 in
hit_radius = 20
[measure]
unit = in
decimals = 4
keep = 3
",
        )
        .unwrap();
        assert!(!s.drag.snap);
        assert_eq!(s.drag.grid, Length::new(1_587_500).unwrap());
        assert_eq!(s.drag.hit_radius, 20.0);
        assert_eq!(s.measure.unit, Some(Unit::Inch));
        assert_eq!((s.measure.decimals, s.measure.keep), (4, 3));
        let none = UiSettings::with_user_config(
            "[drag]
grid = 0 mm
",
        )
        .unwrap();
        assert_eq!(none.drag.grid, Length::ZERO);
        for text in [
            "[drag]
grid = -1 mm
",
            "[drag]
grid = 2 m
",
            "[drag]
snap = 2
",
            "[drag]
hit_radius = 0
",
            "[drag]
hit_radius = near
",
            "[measure]
unit = furlong
",
            "[measure]
decimals = 13
",
            "[measure]
angle_decimals = 10
",
            "[measure]
pick_radius = 500
",
            "[measure]
keep = 0
",
            "[measure]
marker = big
",
        ] {
            let error = UiSettings::with_user_config(text).unwrap_err();
            assert_eq!(error.code(), "PHOENIX-CAD-011", "{text:?}: {error}");
        }
    }

    #[test]
    fn the_defaults_of_a_new_step_are_read_and_checked() {
        let s =
            UiSettings::with_user_config("[new_step]\nhole_face = top\npost_kind = self-tapping\n")
                .unwrap();
        assert_eq!(s.new_step.hole_face, Face::Top);
        assert_eq!(s.new_step.post_kind, BoreKind::SelfTapping);
        let s = UiSettings::with_user_config(
            "[new_step]\npattern_count = 1000\npattern_step = 2.5 mm\npattern_radius = 1/2 in\npattern_angle = 22.5 deg\n",
        )
        .unwrap();
        assert_eq!(s.new_step.pattern_count, 1000);
        assert_eq!(s.new_step.pattern_step, Length::new(2_500_000).unwrap());
        assert_eq!(s.new_step.pattern_radius, Length::new(12_700_000).unwrap());
        assert_eq!(s.new_step.pattern_angle.get(), 22_500_000_000);
        let s = UiSettings::with_user_config(
            "[new_step]\nfin_face = right\nfin_along = v\nfin_count = 1000\nfin_first = 0 mm\n",
        )
        .unwrap();
        assert_eq!(
            (
                s.new_step.fin_face,
                s.new_step.fin_along,
                s.new_step.fin_count
            ),
            (Face::Right, FinAxis::V, 1000)
        );
        assert_eq!(s.new_step.fin_first, Length::ZERO);
        let s = UiSettings::with_user_config(
            "[new_step]\npart_x = -5 mm\npart_y = 1 in\npart_axis = -y\npart_roll = 270\n",
        )
        .unwrap();
        assert_eq!(s.new_step.part_x, Length::new(-5_000_000).unwrap());
        assert_eq!(s.new_step.part_y, Length::new(25_400_000).unwrap());
        assert_eq!(
            (s.new_step.part_axis, s.new_step.part_roll),
            (Axis::MinusY, 3)
        );
        for text in [
            "[new_step]\nhole_face = side\n",
            "[new_step]\npost_kind = glue\n",
            "[new_step]\ntries = 0\n",
            "[new_step]\npitch = wide\n",
            "[new_step]\npattern_count = 1\n",
            "[new_step]\npattern_count = 1001\n",
            "[new_step]\npattern_step = 0 mm\n",
            "[new_step]\npattern_radius = -1 mm\n",
            "[new_step]\npattern_angle = 0\n",
            "[new_step]\npattern_angle = round\n",
            "[new_step]\nfin_face = roof\n",
            "[new_step]\nfin_along = w\n",
            "[new_step]\nfin_count = 0\n",
            "[new_step]\nfin_count = 1001\n",
            "[new_step]\nfin_thickness = 0 mm\n",
            "[new_step]\nfin_height = -2 mm\n",
            "[new_step]\nfin_pitch = 0 mm\n",
            "[new_step]\nfin_length = thin\n",
            "[new_step]\npart_axis = up\n",
            "[new_step]\npart_axis = z\n",
            "[new_step]\npart_roll = 45\n",
            "[new_step]\npart_roll = 360\n",
            "[new_step]\npart_x = far\n",
        ] {
            let error = UiSettings::with_user_config(text).unwrap_err();
            assert_eq!(error.code(), "PHOENIX-CAD-011", "{text:?}: {error}");
        }
    }

    #[test]
    fn a_user_file_changes_only_the_keys_it_names() {
        let s = UiSettings::with_user_config("[view]\nyaw = 10\n[fit]\nlength = 4 in\n").unwrap();
        assert_eq!(s.yaw, 10.0);
        assert_eq!(s.fit[0], Length::new(101_600_000).unwrap());
        assert_eq!(s.pitch, 28.0);
    }

    #[test]
    fn the_settings_of_the_library_can_sit_in_the_same_file() {
        let both = format!("{}\n{}", crate::config::DEFAULT_CONFIG, default_file());
        assert_eq!(
            UiSettings::with_user_config(&both).unwrap(),
            UiSettings::defaults()
        );
        assert!(crate::config::Settings::with_user_config(&both).is_ok());
    }

    #[test]
    fn values_outside_their_range_or_not_numbers_are_refused() {
        for text in [
            "[view]\npitch = 91\n",
            "[view]\nzoom_min = 2\n",
            "[view]\nyaw = north\n",
            "[view]\ncircle_segments = 3\n",
            "[window]\nwidth = 100\n",
            "[new_box]\nwall = 0 mm\n",
            "[fit]\nlength = 5\n",
        ] {
            let error = UiSettings::with_user_config(text).unwrap_err();
            assert_eq!(error.code(), "PHOENIX-CAD-011", "{text:?}: {error}");
        }
    }
}
