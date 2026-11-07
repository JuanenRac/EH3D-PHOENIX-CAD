//! The settings of the window: the size it opens at, how the views turn and zoom, the box a new
//! project starts with and the envelope checked when a project has no board.
//!
//! They live in `phoenix-cad-window.default.cfg`, the same form as the settings of the library;
//! the code holds none of the values. A user file only needs the keys it changes.

use crate::config::Config;
use crate::enclosure::{Enclosure, EnclosureParams, Face};
use crate::mount::BoreKind;
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

impl NewStepDefaults {
    fn from_config(c: &Config) -> Result<NewStepDefaults, CadError> {
        let face = word(c, "new_step.hole_face")?;
        let kind = word(c, "new_step.post_kind")?;
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
    }

    #[test]
    fn the_defaults_of_a_new_step_are_read_and_checked() {
        let s =
            UiSettings::with_user_config("[new_step]\nhole_face = top\npost_kind = self-tapping\n")
                .unwrap();
        assert_eq!(s.new_step.hole_face, Face::Top);
        assert_eq!(s.new_step.post_kind, BoreKind::SelfTapping);
        for text in [
            "[new_step]\nhole_face = side\n",
            "[new_step]\npost_kind = glue\n",
            "[new_step]\ntries = 0\n",
            "[new_step]\npitch = wide\n",
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
