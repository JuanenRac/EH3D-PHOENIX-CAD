//! The settings of the sketch: the solver (tolerance, iterations, step, damping) and the limits
//! of a sketch and of its file.
//!
//! They live in `phoenix-cad-sketch.default.cfg`, the same form as the other settings files; the
//! code holds none of the values. A user file only needs the keys it changes.

use crate::config::Config;
use crate::CadError;

/// The shipped settings of the sketch, as text.
pub const DEFAULT_SKETCH_CONFIG: &str = include_str!("../../phoenix-cad-sketch.default.cfg");

/// The settings of the sketch with their types.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchSettings {
    /// The largest residual, in millimetres, of a solution.
    pub tolerance: f64,
    /// The most steps of the solver.
    pub max_iterations: u32,
    /// The largest move of a coordinate in one step, in millimetres.
    pub max_step: f64,
    /// The step of the central differences, in millimetres.
    pub fd_step: f64,
    /// The relative size under which a row of the Jacobian adds nothing.
    pub rank_tolerance: f64,
    /// The first damping.
    pub damping_start: f64,
    /// The factor by which the damping grows or shrinks.
    pub damping_factor: f64,
    /// The damping beyond which the solver stops.
    pub damping_max: f64,
    /// The gradient under which the solver is at a minimum.
    pub stall_tolerance: f64,
    /// The most elements of a sketch.
    pub max_elements: usize,
    /// The most constraints of a sketch.
    pub max_constraints: usize,
    /// The largest file read.
    pub max_file_bytes: u64,
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

impl SketchSettings {
    /// The defaults that ship with the crate.
    pub fn defaults() -> SketchSettings {
        let config = Config::parse(DEFAULT_SKETCH_CONFIG).expect("the shipped settings are valid");
        SketchSettings::from_config(&config).expect("the shipped settings are complete")
    }

    /// The defaults with the keys of a user file put over them. Keys of other parts are accepted
    /// and ignored here.
    pub fn with_user_config(text: &str) -> Result<SketchSettings, CadError> {
        let mut config = Config::parse(DEFAULT_SKETCH_CONFIG)?;
        config.overlay(&Config::parse(text)?);
        SketchSettings::from_config(&config)
    }

    /// Reads the typed settings from a complete configuration.
    pub fn from_config(c: &Config) -> Result<SketchSettings, CadError> {
        let s = SketchSettings {
            tolerance: real(c, "sketch.tolerance", 1e-12, 1e-3)?,
            max_iterations: count(c, "sketch.max_iterations", 1, 100_000)?,
            max_step: real(c, "sketch.max_step", 1e-3, 1e6)?,
            fd_step: real(c, "sketch.fd_step", 1e-9, 1.0)?,
            rank_tolerance: real(c, "sketch.rank_tolerance", 1e-12, 0.1)?,
            damping_start: real(c, "sketch.damping_start", 1e-12, 1e6)?,
            damping_factor: real(c, "sketch.damping_factor", 1.5, 1000.0)?,
            damping_max: real(c, "sketch.damping_max", 1.0, 1e30)?,
            stall_tolerance: real(c, "sketch.stall_tolerance", 1e-15, 1.0)?,
            max_elements: count(c, "sketch.max_elements", 1, 100_000)? as usize,
            max_constraints: count(c, "sketch.max_constraints", 1, 200_000)? as usize,
            max_file_bytes: u64::from(count(c, "sketch.max_file_bytes", 1024, 1 << 30)?),
        };
        if s.damping_start >= s.damping_max {
            return Err(CadError::Config(
                "sketch.damping_start must be below sketch.damping_max".into(),
            ));
        }
        Ok(s)
    }
}

/// The text of the shipped settings of the sketch, to write them to a file.
pub fn default_file() -> &'static str {
    DEFAULT_SKETCH_CONFIG
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_gives_the_defaults() {
        let s = SketchSettings::defaults();
        assert_eq!(s.tolerance, 1e-8);
        assert_eq!(s.max_iterations, 100);
        assert_eq!(s.max_step, 50.0);
        assert_eq!(s.fd_step, 1e-4);
        assert_eq!(s.rank_tolerance, 1e-6);
        assert_eq!((s.damping_start, s.damping_factor), (1e-3, 10.0));
        assert_eq!(s.damping_max, 1e12);
        assert_eq!(s.stall_tolerance, 1e-7);
        assert_eq!((s.max_elements, s.max_constraints), (500, 1000));
        assert_eq!(s.max_file_bytes, 1_048_576);
        assert_eq!(SketchSettings::with_user_config(default_file()).unwrap(), s);
    }

    #[test]
    fn a_user_file_changes_only_its_keys_and_can_share_a_file_with_the_other_settings() {
        let s = SketchSettings::with_user_config("[sketch]\nmax_iterations = 7\n").unwrap();
        assert_eq!(s.max_iterations, 7);
        assert_eq!(s.tolerance, 1e-8);
        let both = format!(
            "{}\n{}\n{}",
            crate::config::DEFAULT_CONFIG,
            crate::uiconfig::default_file(),
            default_file()
        );
        assert_eq!(
            SketchSettings::with_user_config(&both).unwrap(),
            SketchSettings::defaults()
        );
        assert!(crate::config::Settings::with_user_config(&both).is_ok());
        assert!(crate::uiconfig::UiSettings::with_user_config(&both).is_ok());
    }

    #[test]
    fn values_outside_their_range_or_not_numbers_are_refused() {
        for text in [
            "[sketch]\ntolerance = 0\n",
            "[sketch]\ntolerance = small\n",
            "[sketch]\nmax_iterations = 0\n",
            "[sketch]\ndamping_factor = 1\n",
            "[sketch]\ndamping_start = 100000000000000\n",
            "[sketch]\nmax_file_bytes = 10\n",
            "[sketch]\nfd_step = 5\n",
        ] {
            let error = SketchSettings::with_user_config(text).unwrap_err();
            assert_eq!(error.code(), "PHOENIX-CAD-011", "{text:?}: {error}");
        }
    }
}
