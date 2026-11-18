// SPDX-License-Identifier: GPL-3.0-or-later
//! The list of the placed parts with their properties: for each part its kind, its material and
//! the density of it, its volume, its mass, the corner and the size of its bounding box, and the
//! totals.
//!
//! The figures are the ones of the bill of materials and of the balance (the same volume of each
//! part times the density of its material, rounded to a microgram), so the mass of the list is the
//! mass of the balance. The list is in the order of the kind (profiles, screws, brackets), then of
//! the name and then of the identifier, so the same parts always give the same text. The comma
//! separated values never change with the language; the text for a person is made of words of
//! the window.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::catalog::{mass_g_text, Kind, PartInstance};
use crate::enclosure::Id;
use crate::lang::{fill, Lang, Msg};
use crate::mass::mass_micrograms;
use crate::revolve::scaled;
use crate::scad::mm_text;
use crate::swept::SweptBody;
use crate::turned::Turned;
use crate::window::kind_msg;
use crate::CadError;

const NM3_PER_MM3: i128 = 1_000_000_000_000_000_000;

/// One part of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The name of the part in the model.
    pub name: String,
    /// What kind of part it is (`Profile` for a turned or a swept body, which have none: read
    /// `turned` and `swept`).
    pub kind: Kind,
    /// Whether the row is a turned body.
    pub turned: bool,
    /// Whether the row is a swept body.
    pub swept: bool,
    /// The part of the library it is a copy of, or the sketch of a turned body.
    pub part: String,
    /// The material.
    pub material: String,
    /// The density of the material in kilograms per cubic metre.
    pub density: u32,
    /// The volume of material in cubic nanometres.
    pub volume_nm3: i128,
    /// The mass in micrograms.
    pub mass_ug: i128,
    /// The smallest corner of the bounding box, in nanometres.
    pub min: [i64; 3],
    /// The size of the bounding box along `x`, `y` and `z`, in nanometres.
    pub size: [i64; 3],
}

/// The list and its totals.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct List {
    /// The parts, by kind, name and identifier.
    pub rows: Vec<Row>,
    /// The volume of all of them in cubic nanometres.
    pub volume_nm3: i128,
    /// The mass of all of them in micrograms.
    pub mass_ug: i128,
}

/// The list of some placed parts. The density of each material is in `densities` (kilograms per
/// cubic metre); a material that is not there is refused with `PHOENIX-CAD-045`, as in the bill.
pub fn list(
    parts: &BTreeMap<Id, PartInstance>,
    densities: &BTreeMap<String, u32>,
) -> Result<List, CadError> {
    list_with(parts, &BTreeMap::new(), &BTreeMap::new(), densities)
}

/// The list of some placed parts and of the turned and swept bodies of the model, which come after
/// the parts (the turned ones first), by name and identifier. A body has the volume of its profile
/// turned or swept, the mass of that volume in its material, and the box that holds it.
pub fn list_with(
    parts: &BTreeMap<Id, PartInstance>,
    turned: &BTreeMap<Id, Turned>,
    swept: &BTreeMap<Id, SweptBody>,
    densities: &BTreeMap<String, u32>,
) -> Result<List, CadError> {
    let mut keyed: Vec<(Kind, String, Id, Row)> = Vec::with_capacity(parts.len());
    let (mut volume, mut mass) = (0i128, 0i128);
    for (id, p) in parts {
        let density = *densities.get(&p.part.material).ok_or_else(|| {
            CadError::Part(format!(
                "the material {} of {} is not in the settings",
                p.part.material, p.part.name
            ))
        })?;
        let volume_nm3 = p.volume_nm3()?;
        let mass_ug = mass_micrograms(volume_nm3, density)?;
        let (min, max) = p.bounds()?;
        volume = volume
            .checked_add(volume_nm3)
            .ok_or(CadError::Overflow("volume"))?;
        mass = mass
            .checked_add(mass_ug)
            .ok_or(CadError::Overflow("mass"))?;
        let kind = p.part.kind();
        keyed.push((
            kind,
            p.name.clone(),
            *id,
            Row {
                name: p.name.clone(),
                kind,
                turned: false,
                swept: false,
                part: p.part.name.clone(),
                material: p.part.material.clone(),
                density,
                volume_nm3,
                mass_ug,
                min,
                size: [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
            },
        ));
    }
    keyed.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
    let mut rows: Vec<Row> = keyed.into_iter().map(|k| k.3).collect();
    let mut bodies: Vec<(&String, &Id, Row)> = Vec::with_capacity(turned.len());
    for (id, t) in turned {
        let spec = &t.spec;
        let density = *densities.get(&spec.material).ok_or_else(|| {
            CadError::Part(format!(
                "the material {} of {} is not in the settings",
                spec.material, spec.name
            ))
        })?;
        let volume_nm3 = t.volume_nm3()?;
        let mass_ug = mass_micrograms(volume_nm3, density)?;
        let (min, max) = t.bounds()?;
        volume = volume
            .checked_add(volume_nm3)
            .ok_or(CadError::Overflow("volume"))?;
        mass = mass
            .checked_add(mass_ug)
            .ok_or(CadError::Overflow("mass"))?;
        bodies.push((
            &spec.name,
            id,
            Row {
                name: spec.name.clone(),
                kind: Kind::Profile,
                turned: true,
                swept: false,
                part: spec.sketch.clone(),
                material: spec.material.clone(),
                density,
                volume_nm3,
                mass_ug,
                min,
                size: [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
            },
        ));
    }
    bodies.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    rows.extend(bodies.into_iter().map(|b| b.2));
    let mut formed: Vec<(&String, &Id, Row)> = Vec::with_capacity(swept.len());
    for (id, s) in swept {
        let spec = &s.spec;
        let density = *densities.get(&spec.material).ok_or_else(|| {
            CadError::Part(format!(
                "the material {} of {} is not in the settings",
                spec.material, spec.name
            ))
        })?;
        let volume_nm3 = s.volume_nm3()?;
        let mass_ug = mass_micrograms(volume_nm3, density)?;
        let (min, max) = s.bounds()?;
        volume = volume
            .checked_add(volume_nm3)
            .ok_or(CadError::Overflow("volume"))?;
        mass = mass
            .checked_add(mass_ug)
            .ok_or(CadError::Overflow("mass"))?;
        formed.push((
            &spec.name,
            id,
            Row {
                name: spec.name.clone(),
                kind: Kind::Profile,
                turned: false,
                swept: true,
                part: spec.path.clone(),
                material: spec.material.clone(),
                density,
                volume_nm3,
                mass_ug,
                min,
                size: [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
            },
        ));
    }
    formed.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    rows.extend(formed.into_iter().map(|b| b.2));
    Ok(List {
        rows,
        volume_nm3: volume,
        mass_ug: mass,
    })
}

/// The list as comma separated values, the same text in any language.
pub fn csv(l: &List) -> String {
    let mut out = String::from(
        "name,kind,part,material,density_kg_m3,volume_mm3,mass_g,x_mm,y_mm,z_mm,size_x_mm,size_y_mm,size_z_mm\n",
    );
    for r in &l.rows {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{},{},{},{},{}",
            r.name,
            if r.turned {
                "turned"
            } else if r.swept {
                "swept"
            } else {
                r.kind.name()
            },
            r.part,
            r.material,
            r.density,
            scaled(r.volume_nm3, NM3_PER_MM3),
            mass_g_text(r.mass_ug),
            mm_text(r.min[0]),
            mm_text(r.min[1]),
            mm_text(r.min[2]),
            mm_text(r.size[0]),
            mm_text(r.size[1]),
            mm_text(r.size[2])
        );
    }
    let _ = writeln!(
        out,
        "total,{} parts,,,,{},{},,,,,,",
        l.rows.len(),
        scaled(l.volume_nm3, NM3_PER_MM3),
        mass_g_text(l.mass_ug)
    );
    out
}

/// The list as lines of text in a language: a line for each part and a line of totals.
pub fn text_in(l: &List, lang: Lang) -> String {
    let mut out = String::new();
    for r in &l.rows {
        let _ = writeln!(
            out,
            "{}",
            fill(
                Msg::PlPartRow.tr(lang),
                &[
                    &r.name,
                    if r.turned {
                        Msg::KindTurned.tr(lang)
                    } else if r.swept {
                        Msg::KindSwept.tr(lang)
                    } else {
                        kind_msg(r.kind).tr(lang)
                    },
                    &r.material,
                    &scaled(r.volume_nm3, NM3_PER_MM3),
                    &mass_g_text(r.mass_ug),
                    &mm_text(r.min[0]),
                    &mm_text(r.min[1]),
                    &mm_text(r.min[2]),
                    &mm_text(r.size[0]),
                    &mm_text(r.size[1]),
                    &mm_text(r.size[2]),
                ]
            )
        );
    }
    let _ = writeln!(
        out,
        "{}",
        fill(
            Msg::PlPartTotal.tr(lang),
            &[
                &l.rows.len().to_string(),
                &scaled(l.volume_nm3, NM3_PER_MM3),
                &mass_g_text(l.mass_ug)
            ]
        )
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Head, Orientation, Part, Shape};
    use crate::Length;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn id(n: u64) -> Id {
        Id::from_parts(1_700_000_000_000, 1, n).unwrap()
    }

    fn densities() -> BTreeMap<String, u32> {
        BTreeMap::from([("steel".to_owned(), 8000), ("light".to_owned(), 2700)])
    }

    fn cube(name: &str, material: &str, x: i64) -> PartInstance {
        PartInstance {
            name: name.into(),
            library: "lib".into(),
            part: Part {
                name: "cube".into(),
                material: material.into(),
                shape: Shape::Profile {
                    width: mm(10),
                    height: mm(10),
                    wall: Length::ZERO,
                    default_cut: mm(10),
                    max_cut: mm(100),
                },
            },
            at: [mm(x), Length::ZERO, Length::ZERO],
            orientation: Orientation::default(),
            cut: Some(mm(10)),
        }
    }

    fn screw(x: i64) -> PartInstance {
        PartInstance {
            name: "bolt".into(),
            library: "lib".into(),
            part: Part {
                name: "m3".into(),
                material: "steel".into(),
                shape: Shape::Screw {
                    metric: "M3".into(),
                    diameter: mm(3),
                    length: mm(10),
                    head: Head::Cylinder,
                    head_diameter: mm(5),
                    head_height: mm(3),
                },
            },
            at: [mm(x), Length::ZERO, Length::ZERO],
            orientation: Orientation::default(),
            cut: None,
        }
    }

    fn set(list: Vec<(u64, PartInstance)>) -> BTreeMap<Id, PartInstance> {
        list.into_iter().map(|(n, p)| (id(n), p)).collect()
    }

    #[test]
    fn the_rows_are_by_kind_then_name_and_the_totals_add_up() {
        // 1000 mm3 of steel at 8000 kg/m3 is 8 g, and of the light material 2.7 g
        let l = list(
            &set(vec![
                (1, screw(50)),
                (2, cube("b", "light", 100)),
                (3, cube("a", "steel", 0)),
            ]),
            &densities(),
        )
        .unwrap();
        let names: Vec<&str> = l.rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "bolt"]);
        assert_eq!(l.rows[0].mass_ug, 8_000_000);
        assert_eq!(l.rows[1].mass_ug, 2_700_000);
        assert_eq!(l.rows[0].volume_nm3, 1_000_000_000_000_000_000_000);
        assert_eq!(l.rows[0].size, [10_000_000; 3]);
        assert_eq!(l.rows[1].min, [100_000_000, 0, 0]);
        assert_eq!(l.mass_ug, l.rows.iter().map(|r| r.mass_ug).sum::<i128>());
        assert_eq!(
            l.volume_nm3,
            l.rows.iter().map(|r| r.volume_nm3).sum::<i128>()
        );
        // the mass is the one of the balance
        let b = crate::balance::balance(
            &set(vec![
                (1, screw(50)),
                (2, cube("b", "light", 100)),
                (3, cube("a", "steel", 0)),
            ]),
            &densities(),
        )
        .unwrap();
        assert_eq!(b.mass_ug, l.mass_ug);
    }

    #[test]
    fn the_csv_has_a_line_for_each_part_and_the_totals_and_does_not_change_with_the_language() {
        let l = list(
            &set(vec![
                (1, cube("a", "steel", 0)),
                (2, cube("b", "light", 100)),
            ]),
            &densities(),
        )
        .unwrap();
        assert_eq!(
            csv(&l),
            "name,kind,part,material,density_kg_m3,volume_mm3,mass_g,x_mm,y_mm,z_mm,size_x_mm,size_y_mm,size_z_mm\n\
             a,profile,cube,steel,8000,1000.000,8.000,0,0,0,10,10,10\n\
             b,profile,cube,light,2700,1000.000,2.700,100,0,0,10,10,10\n\
             total,2 parts,,,,2000.000,10.700,,,,,,\n"
        );
        // every line has the thirteen columns
        for line in csv(&l).lines() {
            assert_eq!(line.split(',').count(), 13, "{line}");
        }
    }

    #[test]
    fn the_text_is_in_the_words_of_the_language() {
        let l = list(
            &set(vec![(1, cube("a", "steel", 0)), (2, screw(50))]),
            &densities(),
        )
        .unwrap();
        let en = text_in(&l, Lang::En);
        assert!(
            en.starts_with(
                "a (profile, steel): 1000.000 mm3, 8.000 g, corner 0 0 0 mm, size 10 10 10 mm\n"
            ),
            "{en}"
        );
        assert!(en.contains("bolt (screw, steel): "), "{en}");
        assert!(
            en.lines().last().unwrap().starts_with("parts: 2, volume: "),
            "{en}"
        );
        let de = text_in(&l, Lang::De);
        assert!(de.contains("a (Profil, steel): "), "{de}");
        assert!(de.contains(", Ecke 0 0 0 mm, Größe 10 10 10 mm\n"), "{de}");
        for lang in Lang::ALL {
            assert_eq!(text_in(&l, lang).lines().count(), 3, "{}", lang.code());
        }
    }

    #[test]
    fn no_parts_have_an_empty_list_and_an_unknown_material_is_refused() {
        let l = list(&BTreeMap::new(), &densities()).unwrap();
        assert_eq!(l, List::default());
        assert_eq!(csv(&l).lines().count(), 2);
        assert!(text_in(&l, Lang::En).starts_with("parts: 0, volume: 0.000 mm3, mass: 0.000 g"));
        let e = list(&set(vec![(1, cube("a", "wood", 0))]), &densities()).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-045");
    }

    #[test]
    fn the_turned_bodies_come_after_the_parts_and_count_in_the_totals() {
        use crate::enclosure::Face;
        use crate::profile::{Profile, ProfileLimits};
        use crate::sketch::{Sketch, SketchPlane};
        use crate::turned::{Turned, TurnedSpec};
        // 10 mm long from 5 to 15 mm of the axis: pi (15^2 - 5^2) 10 = 6283.185 mm3
        let mut sketch = Sketch::new("ring", SketchPlane::on_face(Face::Front)).unwrap();
        sketch
            .add_rectangle((mm(0), mm(5)), (mm(10), mm(15)))
            .unwrap();
        let profile = Profile::from_sketch(&sketch, &ProfileLimits::shipped()).unwrap();
        let mut spec = TurnedSpec::full_turn("spacer", "ring", "steel").unwrap();
        spec.at = [mm(100), mm(0), mm(0)];
        let turned = BTreeMap::from([(id(9), Turned { spec, profile })]);
        let parts = BTreeMap::from([(id(1), cube("block", "steel", 0))]);
        let none = BTreeMap::new();
        let l = list_with(&parts, &turned, &none, &densities()).unwrap();
        assert_eq!(l.rows.len(), 2);
        assert!(!l.rows[0].turned && l.rows[1].turned);
        let r = &l.rows[1];
        assert_eq!(
            (r.name.as_str(), r.part.as_str(), r.density),
            ("spacer", "ring", 8000)
        );
        assert_eq!(r.min, [mm(100).get(), mm(-15).get(), mm(-15).get()]);
        assert_eq!(r.size, [mm(10).get(), mm(30).get(), mm(30).get()]);
        // the mass is the volume times the density: 6283.185 mm3 of steel is 50.265 g
        assert_eq!(l.mass_ug, l.rows[0].mass_ug + r.mass_ug);
        assert!(
            (r.mass_ug as f64 / 1e6 - 50.265).abs() < 0.001,
            "{}",
            r.mass_ug
        );
        let csv = csv(&l);
        assert!(csv.contains("\nspacer,turned,ring,steel,8000,"), "{csv}");
        assert!(
            csv.ends_with("\n") && csv.contains("total,2 parts,"),
            "{csv}"
        );
        let text = text_in(&l, Lang::En);
        assert!(text.contains("spacer (turned, steel)"), "{text}");
        // without turned bodies the list is the one of the parts
        assert_eq!(
            list_with(&parts, &BTreeMap::new(), &none, &densities()).unwrap(),
            list(&parts, &densities()).unwrap()
        );
        // a material that is not in the settings is refused
        let mut other = turned.clone();
        for t in other.values_mut() {
            t.spec.material = "wood".into();
        }
        assert_eq!(
            list_with(&parts, &other, &none, &densities())
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-045"
        );
    }
}
