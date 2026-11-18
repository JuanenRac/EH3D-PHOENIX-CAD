// SPDX-License-Identifier: GPL-3.0-or-later
//! A turned body: a sketch of the project turned about one of its axes, kept in the model as a
//! body of its own.
//!
//! The recipe keeps only what is asked ([`TurnedSpec`]: the sketch by its name, the axis, the
//! angle, where the origin of the sketch is put in the model and the material); the profile is
//! taken from the sketch every time the model is built, so a change of the sketch changes the
//! body, as it does for an extrusion. A turned body is separate from the box, like a placed part:
//! it does not change the volume of the box, it has its own volume, mass and triangles, and it
//! appears in the bill, the balance and the files of triangles.
//!
//! The volume, the surface and the extent come from [`crate::revolve`] (exact up to the
//! rounding of the theorems of Pappus). The centre of mass is worked out from the closed surface
//! of the triangles of the body, which is exact for the body that the triangles make and differs
//! from the turned body by the sides of the circle table only.

use crate::catalog::CatalogLimits;
use crate::config::MeshSettings;
use crate::mass::mass_micrograms;
use crate::mesh::Mesh;
use crate::profile::{Profile, ProfileLimits};
use crate::revolve::{self, Axis, Revolved};
use crate::sketch::Sketch;
use crate::units::Angle;
use crate::{CadError, Length};

/// What the recipe keeps of a turned body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnedSpec {
    /// The name shown to people.
    pub name: String,
    /// The name of the sketch that is turned.
    pub sketch: String,
    /// The axis of the sketch it is turned about.
    pub axis: Axis,
    /// The turn, above zero and up to a full turn.
    pub angle: Angle,
    /// Where the origin of the plane of the sketch is put in the model, along `x`, `y` and `z`.
    pub at: [Length; 3],
    /// The material, a name of the table of densities of the settings.
    pub material: String,
}

fn turned_error(why: impl AsRef<str>) -> CadError {
    CadError::Part(why.as_ref().to_string())
}

/// Whether the names of a body formed from sketches are within the limit of the settings and the
/// body is no farther from the origin than the settings allow a part to be placed.
pub(crate) fn check_body(
    name: &str,
    material: &str,
    at: &[Length; 3],
    limits: &CatalogLimits,
) -> Result<(), CadError> {
    if name.is_empty() || name.len() > limits.max_name_bytes {
        return Err(turned_error(
            "the name of a body is empty or longer than the limit",
        ));
    }
    if material.is_empty() || material.len() > limits.max_name_bytes {
        return Err(turned_error(format!(
            "{name}: the material is empty or longer than the limit"
        )));
    }
    let far = limits.max_dimension.get();
    if at.iter().any(|v| v.get().abs() > far) {
        return Err(turned_error(format!(
            "{name}: the body is farther from the origin than the {far} nm the settings allow"
        )));
    }
    Ok(())
}

impl TurnedSpec {
    /// A body that turns the sketch about its first axis by a full turn, at the origin, of the
    /// material.
    pub fn full_turn(name: &str, sketch: &str, material: &str) -> Result<TurnedSpec, CadError> {
        Ok(TurnedSpec {
            name: name.to_string(),
            sketch: sketch.to_string(),
            axis: Axis::X,
            angle: Angle::from_deg(360)?,
            at: [Length::ZERO; 3],
            material: material.to_string(),
        })
    }

    /// Whether the names are within the limit of the settings and the body is no farther from the
    /// origin than the settings allow a part to be placed.
    pub fn check(&self, limits: &CatalogLimits) -> Result<(), CadError> {
        check_body(&self.name, &self.material, &self.at, limits)
    }
}

/// A turned body with the profile its sketch has now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turned {
    /// What was asked.
    pub spec: TurnedSpec,
    /// The outline of the sketch.
    pub profile: Profile,
}

impl Turned {
    /// The body of a spec, with the profile of its sketch. Refused with `PHOENIX-CAD-037` when
    /// the sketch does not exist, with the errors of [`crate::profile`] when its outlines are not
    /// closed, with `PHOENIX-CAD-007` when the angle is not above zero and up to a full turn and
    /// with `PHOENIX-CAD-036` when the profile crosses the axis.
    pub fn resolve(
        spec: &TurnedSpec,
        sketches: &[Sketch],
        limits: &ProfileLimits,
    ) -> Result<Turned, CadError> {
        let found = sketches
            .iter()
            .find(|s| s.name() == spec.sketch)
            .ok_or_else(|| {
                CadError::Feature(format!("the sketch {} does not exist", spec.sketch))
            })?;
        let profile = Profile::from_sketch(found, limits)?;
        revolve::revolve(&profile, spec.axis, spec.angle)?;
        Ok(Turned {
            spec: spec.clone(),
            profile,
        })
    }

    /// The figures of the body, in its own coordinates.
    pub fn figures(&self) -> Result<Revolved, CadError> {
        revolve::revolve(&self.profile, self.spec.axis, self.spec.angle)
    }

    /// The volume in cubic nanometres.
    pub fn volume_nm3(&self) -> Result<i128, CadError> {
        Ok(self.figures()?.volume_nm3)
    }

    /// The mass in micrograms for a density in kilograms per cubic metre.
    pub fn mass_ug(&self, density_kg_m3: u32) -> Result<i128, CadError> {
        mass_micrograms(self.volume_nm3()?, density_kg_m3)
    }

    /// The box that holds the body, in the model, in nanometres: the extent along the axis by the
    /// greatest distance from the axis on both sides in the other two directions. A part of a
    /// turn has a smaller body than this box.
    pub fn bounds(&self) -> Result<([i64; 3], [i64; 3]), CadError> {
        let f = self.figures()?;
        let reach = f.radial.1;
        let (a0, a1) = f.axial;
        let (lo, hi) = match self.spec.axis {
            Axis::X => ([a0, -reach, -reach], [a1, reach, reach]),
            Axis::Y => ([-reach, a0, -reach], [reach, a1, reach]),
        };
        let shift = |p: [i64; 3]| -> Result<[i64; 3], CadError> {
            let mut out = [0i64; 3];
            for (k, slot) in out.iter_mut().enumerate() {
                *slot = p[k]
                    .checked_add(self.spec.at[k].get())
                    .ok_or(CadError::Overflow("bounds"))?;
            }
            Ok(out)
        };
        Ok((shift(lo)?, shift(hi)?))
    }

    /// The triangles of the body in the model (see [`revolve::mesh`]); `limit` is the most
    /// triangles that are made.
    pub fn mesh(&self, settings: &MeshSettings, limit: usize) -> Result<Mesh, CadError> {
        let local = revolve::mesh(
            &self.profile,
            self.spec.axis,
            self.spec.angle,
            settings.circle_segments,
            limit,
        )?;
        let at = self.spec.at.map(|v| v.get() as f64);
        Ok(local.mapped(|p| [p[0] + at[0], p[1] + at[1], p[2] + at[2]]))
    }

    /// The mass and the centre of mass of the body (see [`crate::balance::Weighed`]).
    pub fn weigh(
        &self,
        density_kg_m3: u32,
        settings: &MeshSettings,
    ) -> Result<crate::balance::Weighed, CadError> {
        let mass_ug = self.mass_ug(density_kg_m3)?;
        let mesh = self.mesh(settings, settings.max_triangles as usize)?;
        let centre = mesh
            .centroid_nm()
            .ok_or_else(|| turned_error(format!("{}: the body has no volume", self.spec.name)))?;
        Ok(crate::balance::Weighed {
            mass_ug,
            middle2_nm: centre.map(|v| (2.0 * v).round() as i128),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::enclosure::Face;
    use crate::sketch::SketchPlane;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    /// A sketch named `ring` with a rectangle from `y0` to `y1` of the first axis, 10 mm long.
    fn ring(y0: i64, y1: i64) -> Sketch {
        let mut s = Sketch::new("ring", SketchPlane::on_face(Face::Front)).unwrap();
        let ids: Vec<_> = [(0, y0), (10, y0), (10, y1), (0, y1)]
            .iter()
            .map(|(x, y)| s.add_point(mm(*x), mm(*y)).unwrap())
            .collect();
        for i in 0..4 {
            s.add_line(ids[i], ids[(i + 1) % 4]).unwrap();
        }
        s
    }

    fn spec() -> TurnedSpec {
        TurnedSpec::full_turn("spacer", "ring", "steel").unwrap()
    }

    fn body(spec: &TurnedSpec) -> Turned {
        Turned::resolve(spec, &[ring(5, 15)], &ProfileLimits::shipped()).unwrap()
    }

    #[test]
    fn a_full_turn_has_the_volume_and_the_mass_of_the_tube() {
        // pi (15^2 - 5^2) 10 = 6283.185 mm3; at 7850 kg/m3 that is 49.322 g
        let b = body(&spec());
        let v = b.volume_nm3().unwrap();
        assert!((v as f64 / 1e18 - 6283.185).abs() < 0.001, "{v}");
        let g = b.mass_ug(7850).unwrap() as f64 / 1e6;
        assert!((g - 49.322).abs() < 0.001, "{g}");
    }

    #[test]
    fn the_box_follows_the_axis_the_reach_and_the_position() {
        let mut s = spec();
        s.at = [mm(100), mm(20), mm(-5)];
        let (lo, hi) = body(&s).bounds().unwrap();
        assert_eq!(lo, [mm(100).get(), mm(5).get(), mm(-20).get()]);
        assert_eq!(hi, [mm(110).get(), mm(35).get(), mm(10).get()]);
        // turned about the second axis the length runs along y
        let mut t = spec();
        t.axis = Axis::Y;
        let mut sketch = Sketch::new("ring", SketchPlane::on_face(Face::Front)).unwrap();
        let ids: Vec<_> = [(5, 0), (15, 0), (15, 10), (5, 10)]
            .iter()
            .map(|(x, y)| sketch.add_point(mm(*x), mm(*y)).unwrap())
            .collect();
        for i in 0..4 {
            sketch.add_line(ids[i], ids[(i + 1) % 4]).unwrap();
        }
        let b = Turned::resolve(&t, &[sketch], &ProfileLimits::shipped()).unwrap();
        let (lo, hi) = b.bounds().unwrap();
        assert_eq!(lo, [mm(-15).get(), 0, mm(-15).get()]);
        assert_eq!(hi, [mm(15).get(), mm(10).get(), mm(15).get()]);
    }

    #[test]
    fn the_triangles_are_closed_and_moved_to_the_position() {
        let settings = Settings::defaults();
        let mut s = spec();
        s.at = [mm(100), Length::ZERO, Length::ZERO];
        let mesh = body(&s).mesh(&settings.mesh, 1_000_000).unwrap();
        mesh.check_closed().unwrap();
        let (lo, hi) = mesh.bounds().unwrap();
        assert!((lo[0] - 100e6).abs() < 1.0 && (hi[0] - 110e6).abs() < 1.0);
        // the volume of the triangles is the one of the tube up to the sides of the circles
        let exact = body(&s).volume_nm3().unwrap() as f64;
        assert!(((mesh.volume_nm3() - exact) / exact).abs() < 0.005);
    }

    #[test]
    fn the_centre_of_mass_of_a_tube_is_on_its_axis_in_the_middle() {
        let settings = Settings::defaults();
        let mut s = spec();
        s.at = [mm(100), mm(20), mm(-5)];
        let w = body(&s).weigh(7850, &settings.mesh).unwrap();
        // twice the middle: x 105 mm, y 20 mm and z -5 mm, each doubled, to a micrometre
        let near = |got: i128, want: i64| (got - i128::from(want) * 2).abs() < 2_000;
        assert!(near(w.middle2_nm[0], mm(105).get()), "{:?}", w);
        assert!(near(w.middle2_nm[1], mm(20).get()), "{:?}", w);
        assert!(near(w.middle2_nm[2], mm(-5).get()), "{:?}", w);
        assert_eq!(w.mass_ug, body(&s).mass_ug(7850).unwrap());
    }

    #[test]
    fn a_missing_sketch_a_crossing_profile_and_a_bad_angle_are_refused() {
        let limits = ProfileLimits::shipped();
        let mut missing = spec();
        missing.sketch = "nothing".into();
        let e = Turned::resolve(&missing, &[ring(5, 15)], &limits).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-037");
        let e = Turned::resolve(&spec(), &[ring(-5, 5)], &limits).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-036");
        let mut zero = spec();
        zero.angle = Angle::ZERO;
        let e = Turned::resolve(&zero, &[ring(5, 15)], &limits).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-007");
    }

    #[test]
    fn the_spec_is_checked_against_the_limits_of_the_settings() {
        let limits = Settings::defaults().design.catalog;
        assert!(spec().check(&limits).is_ok());
        let mut s = spec();
        s.name = String::new();
        assert_eq!(s.check(&limits).unwrap_err().code(), "PHOENIX-CAD-045");
        let mut s = spec();
        s.material = "m".repeat(limits.max_name_bytes + 1);
        assert_eq!(s.check(&limits).unwrap_err().code(), "PHOENIX-CAD-045");
        let mut s = spec();
        s.at[1] = Length::new(limits.max_dimension.get() + 1).unwrap();
        assert_eq!(s.check(&limits).unwrap_err().code(), "PHOENIX-CAD-045");
    }
}
