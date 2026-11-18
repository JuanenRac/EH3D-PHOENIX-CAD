// SPDX-License-Identifier: GPL-3.0-or-later
//! A swept body: the profile of a sketch of the project swept along the path of another sketch,
//! kept in the model as a body of its own, like a turned body (see [`crate::turned`]).
//!
//! The recipe keeps only what is asked ([`SweptSpec`]: the two sketches by their names, where the
//! origin of their plane is put in the model and the material); the profile and the path are taken
//! from the sketches every time the model is built. The volume comes from [`crate::sweep`]; the
//! box that holds the body and its centre of mass are worked out from the closed surface of its
//! triangles, so the box is exact for the vertices of that surface (a point of an arc between two
//! vertices is not looked at, and the box is that of the polygon the circle table makes).

use crate::balance::Weighed;
use crate::catalog::CatalogLimits;
use crate::config::MeshSettings;
use crate::mass::mass_micrograms;
use crate::mesh::Mesh;
use crate::profile::{Profile, ProfileLimits};
use crate::sketch::Sketch;
use crate::sweep::{self, Path, Swept as Figures};
use crate::turned::check_body;
use crate::{CadError, Length};

/// The sides of a circle and the most triangles of the surface that gives the box of a body.
const BOX_SIDES: u32 = 64;
const BOX_TRIANGLES: usize = 2_000_000;

/// What the recipe keeps of a swept body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweptSpec {
    /// The name shown to people.
    pub name: String,
    /// The name of the sketch whose closed outlines are swept.
    pub profile: String,
    /// The name of the sketch that draws the path.
    pub path: String,
    /// Where the origin of the plane of the sketches is put in the model, along `x`, `y` and `z`.
    pub at: [Length; 3],
    /// The material, a name of the table of densities of the settings.
    pub material: String,
}

impl SweptSpec {
    /// Whether the names are within the limit of the settings and the body is no farther from the
    /// origin than the settings allow a part to be placed.
    pub fn check(&self, limits: &CatalogLimits) -> Result<(), CadError> {
        check_body(&self.name, &self.material, &self.at, limits)
    }
}

/// A swept body with the profile and the path its sketches have now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweptBody {
    /// What was asked.
    pub spec: SweptSpec,
    /// The outline of the profile sketch.
    pub profile: Profile,
    /// The path of the path sketch.
    pub path: Path,
    /// The box that holds the body, in its own coordinates, in nanometres.
    local_box: ([i64; 3], [i64; 3]),
}

fn find<'a>(sketches: &'a [Sketch], name: &str) -> Result<&'a Sketch, CadError> {
    sketches
        .iter()
        .find(|s| s.name() == name)
        .ok_or_else(|| CadError::Feature(format!("the sketch {name} does not exist")))
}

fn round3(p: [f64; 3]) -> [i64; 3] {
    p.map(|v| v.round() as i64)
}

impl SweptBody {
    /// The body of a spec. Refused with `PHOENIX-CAD-037` when a sketch does not exist, and with
    /// the errors of [`crate::profile`] and `PHOENIX-CAD-036` for a profile or a path that cannot
    /// be swept (see [`sweep::sweep`]).
    pub fn resolve(
        spec: &SweptSpec,
        sketches: &[Sketch],
        limits: &ProfileLimits,
    ) -> Result<SweptBody, CadError> {
        let profile = Profile::from_sketch(find(sketches, &spec.profile)?, limits)?;
        let path = sweep::path_of(find(sketches, &spec.path)?)?;
        sweep::sweep(&profile, &path)?;
        let surface = sweep::mesh(&profile, &path, BOX_SIDES, BOX_TRIANGLES)?;
        let (lo, hi) = surface
            .bounds()
            .ok_or_else(|| CadError::Part(format!("{}: the body has no volume", spec.name)))?;
        Ok(SweptBody {
            spec: spec.clone(),
            profile,
            path,
            local_box: (round3(lo), round3(hi)),
        })
    }

    /// The figures of the body, in its own coordinates.
    pub fn figures(&self) -> Result<Figures, CadError> {
        sweep::sweep(&self.profile, &self.path)
    }

    /// The volume in cubic nanometres.
    pub fn volume_nm3(&self) -> Result<i128, CadError> {
        Ok(self.figures()?.volume_nm3)
    }

    /// The mass in micrograms for a density in kilograms per cubic metre.
    pub fn mass_ug(&self, density_kg_m3: u32) -> Result<i128, CadError> {
        mass_micrograms(self.volume_nm3()?, density_kg_m3)
    }

    /// The box that holds the body, in the model, in nanometres.
    pub fn bounds(&self) -> Result<([i64; 3], [i64; 3]), CadError> {
        let (lo, hi) = self.local_box;
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

    /// The triangles of the body in the model (see [`sweep::mesh`]); `limit` is the most
    /// triangles that are made.
    pub fn mesh(&self, settings: &MeshSettings, limit: usize) -> Result<Mesh, CadError> {
        let local = sweep::mesh(&self.profile, &self.path, settings.circle_segments, limit)?;
        let at = self.spec.at.map(|v| v.get() as f64);
        Ok(local.mapped(|p| [p[0] + at[0], p[1] + at[1], p[2] + at[2]]))
    }

    /// The mass and the centre of mass of the body.
    pub fn weigh(&self, density_kg_m3: u32, settings: &MeshSettings) -> Result<Weighed, CadError> {
        let mass_ug = self.mass_ug(density_kg_m3)?;
        let mesh = self.mesh(settings, settings.max_triangles as usize)?;
        let centre = mesh
            .centroid_nm()
            .ok_or_else(|| CadError::Part(format!("{}: the body has no volume", self.spec.name)))?;
        Ok(Weighed {
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

    /// A path of one line of 100 mm along the first axis, and a profile of 4 by 2 mm.
    fn sketches() -> Vec<Sketch> {
        let mut path = Sketch::new("rail", SketchPlane::on_face(Face::Front)).unwrap();
        let a = path.add_point(mm(0), mm(0)).unwrap();
        let b = path.add_point(mm(100), mm(0)).unwrap();
        path.add_line(a, b).unwrap();
        let mut profile = Sketch::new("section", SketchPlane::on_face(Face::Front)).unwrap();
        profile
            .add_rectangle((mm(0), mm(-1)), (mm(4), mm(1)))
            .unwrap();
        vec![path, profile]
    }

    fn spec() -> SweptSpec {
        SweptSpec {
            name: "rail".into(),
            profile: "section".into(),
            path: "rail".into(),
            at: [Length::ZERO; 3],
            material: "PLA".into(),
        }
    }

    fn body(spec: &SweptSpec) -> SweptBody {
        SweptBody::resolve(spec, &sketches(), &ProfileLimits::shipped()).unwrap()
    }

    #[test]
    fn a_profile_swept_along_a_line_is_a_prism() {
        // 4 x 2 mm over 100 mm: 800 mm3; at 1240 kg/m3 that is 0.992 g
        let b = body(&spec());
        let off = b.volume_nm3().unwrap() - 800_000_000_000_000_000_000;
        assert!(off.abs() < 1_000_000, "{off}");
        assert_eq!(b.mass_ug(1240).unwrap(), 992_000);
    }

    #[test]
    fn the_box_is_the_one_of_the_surface_moved_to_the_position() {
        // 100 mm along x, 4 mm to one side of the path and 1 mm above and below its plane
        let (lo, hi) = body(&spec()).bounds().unwrap();
        assert_eq!((lo[0], hi[0]), (0, 100_000_000));
        assert_eq!(hi[1] - lo[1], 4_000_000);
        assert_eq!((lo[2], hi[2]), (-1_000_000, 1_000_000));
        let mut moved = spec();
        moved.at = [mm(10), mm(20), mm(30)];
        let (lo2, hi2) = body(&moved).bounds().unwrap();
        assert_eq!(
            lo2,
            [lo[0] + 10_000_000, lo[1] + 20_000_000, lo[2] + 30_000_000]
        );
        assert_eq!(
            hi2,
            [hi[0] + 10_000_000, hi[1] + 20_000_000, hi[2] + 30_000_000]
        );
    }

    #[test]
    fn the_triangles_are_closed_and_the_centre_of_mass_is_in_the_middle() {
        let settings = Settings::defaults();
        let mut s = spec();
        s.at = [mm(10), mm(0), mm(0)];
        let b = body(&s);
        let mesh = b.mesh(&settings.mesh, 1_000_000).unwrap();
        mesh.check_closed().unwrap();
        let w = b.weigh(1240, &settings.mesh).unwrap();
        // the middle is at x = 60 mm, 2 mm to the side of the path and z = 0, each doubled, to a
        // micrometre
        let near = |got: i128, want: i64| (got - i128::from(want) * 2).abs() < 2_000;
        assert!(near(w.middle2_nm[0], 60_000_000), "{w:?}");
        assert!(near(w.middle2_nm[1].abs(), 2_000_000), "{w:?}");
        assert!(near(w.middle2_nm[2], 0), "{w:?}");
        assert_eq!(w.mass_ug, 992_000);
    }

    #[test]
    fn a_missing_sketch_and_a_path_that_cannot_be_swept_are_refused() {
        let limits = ProfileLimits::shipped();
        let mut lost = spec();
        lost.path = "nothing".into();
        let e = SweptBody::resolve(&lost, &sketches(), &limits).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-037");
        let mut lost = spec();
        lost.profile = "nothing".into();
        let e = SweptBody::resolve(&lost, &sketches(), &limits).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-037");
        // a path made of a point only has nothing to walk
        let mut empty = Sketch::new("rail", SketchPlane::on_face(Face::Front)).unwrap();
        empty.add_point(mm(0), mm(0)).unwrap();
        let mut set = sketches();
        set[0] = empty;
        assert!(SweptBody::resolve(&spec(), &set, &limits).is_err());
    }

    #[test]
    fn the_spec_is_checked_against_the_limits_of_the_settings() {
        let limits = Settings::defaults().design.catalog;
        assert!(spec().check(&limits).is_ok());
        let mut s = spec();
        s.name = String::new();
        assert_eq!(s.check(&limits).unwrap_err().code(), "PHOENIX-CAD-045");
        let mut s = spec();
        s.at[2] = Length::new(limits.max_dimension.get() + 1).unwrap();
        assert_eq!(s.check(&limits).unwrap_err().code(), "PHOENIX-CAD-045");
    }
}
