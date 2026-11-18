// SPDX-License-Identifier: GPL-3.0-or-later
//! The mass of the placed parts and where their centre of mass is.
//!
//! Each part has an exact mass (its volume times the density of its material, the same figure as
//! the bill of materials). The centre of mass of the whole is the mean of the centres of the parts
//! weighted by their masses. The centre of a part is the middle of its bounding box, which is the
//! exact centre of mass of a profile (solid or hollow, its section is symmetric) and an estimate
//! for a screw (the head pulls it towards the head) and for a bracket (the corner is heavier than
//! the ends of the legs); the report counts how many parts are estimated, so the figure is never
//! given as more exact than it is. The sums are made in integers: the masses in micrograms and the
//! middles in doubled nanometres, and the centre is rounded to the nanometre once, at the end.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::catalog::{mass_g_text, Kind, PartInstance};
use crate::enclosure::Id;
use crate::mass::mass_micrograms;
use crate::scad::mm_text;
use crate::units::div_round_half_away;
use crate::CadError;

/// The mass and the centre of mass of some parts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Balance {
    /// How many parts.
    pub parts: usize,
    /// How many of them have an estimated centre (screws and brackets).
    pub estimated: usize,
    /// The mass in micrograms.
    pub mass_ug: i128,
    /// The centre of mass in nanometres, or `None` without mass.
    pub centre_nm: Option<[i64; 3]>,
}

/// A body whose mass and centre are known already, such as a turned body (see
/// [`crate::turned`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Weighed {
    /// The mass in micrograms.
    pub mass_ug: i128,
    /// Twice the centre of mass in nanometres.
    pub middle2_nm: [i128; 3],
}

/// The balance of the parts. The density of each material is in `densities` (kilograms per cubic
/// metre, the table of the settings); a material that is not there is refused with
/// `PHOENIX-CAD-045`, as in the bill of materials.
pub fn balance(
    parts: &BTreeMap<Id, PartInstance>,
    densities: &BTreeMap<String, u32>,
) -> Result<Balance, CadError> {
    balance_with(parts, &[], densities)
}

/// The balance of the parts and of other bodies that were weighed already; each of them counts as
/// one more part with an exact centre.
pub fn balance_with(
    parts: &BTreeMap<Id, PartInstance>,
    weighed: &[Weighed],
    densities: &BTreeMap<String, u32>,
) -> Result<Balance, CadError> {
    let mut total: i128 = 0;
    let mut moment2 = [0i128; 3];
    let mut estimated = 0usize;
    for w in weighed {
        total = total
            .checked_add(w.mass_ug)
            .ok_or(CadError::Overflow("mass"))?;
        for (k, slot) in moment2.iter_mut().enumerate() {
            *slot = w
                .mass_ug
                .checked_mul(w.middle2_nm[k])
                .and_then(|x| x.checked_add(*slot))
                .ok_or(CadError::Overflow("moment"))?;
        }
    }
    for p in parts.values() {
        let density = *densities.get(&p.part.material).ok_or_else(|| {
            CadError::Part(format!(
                "the material {} of {} is not in the settings",
                p.part.material, p.part.name
            ))
        })?;
        let m = mass_micrograms(p.volume_nm3()?, density)?;
        if p.part.kind() != Kind::Profile {
            estimated += 1;
        }
        let (lo, hi) = p.bounds()?;
        total = total.checked_add(m).ok_or(CadError::Overflow("mass"))?;
        for k in 0..3 {
            let middle2 = i128::from(lo[k]) + i128::from(hi[k]);
            moment2[k] = m
                .checked_mul(middle2)
                .and_then(|x| x.checked_add(moment2[k]))
                .ok_or(CadError::Overflow("moment"))?;
        }
    }
    let centre_nm = if total > 0 {
        let mut c = [0i64; 3];
        for k in 0..3 {
            let v = div_round_half_away(moment2[k], 2 * total)?;
            c[k] = i64::try_from(v).map_err(|_| CadError::Overflow("centre"))?;
        }
        Some(c)
    } else {
        None
    };
    Ok(Balance {
        parts: parts.len() + weighed.len(),
        estimated,
        mass_ug: total,
        centre_nm,
    })
}

/// The balance as lines of text: the number of parts, the mass in grams, the centre of mass in
/// millimetres and how many parts have an estimated centre.
pub fn balance_text(b: &Balance) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "parts: {}", b.parts);
    let _ = writeln!(out, "mass: {} g", mass_g_text(b.mass_ug));
    match b.centre_nm {
        Some(c) => {
            let _ = writeln!(
                out,
                "centre of mass: {} {} {} mm",
                mm_text(c[0]),
                mm_text(c[1]),
                mm_text(c[2])
            );
        }
        None => out.push_str("centre of mass: none\n"),
    }
    let _ = writeln!(
        out,
        "estimated centres: {} of {} parts (screws and brackets use the middle of their box)",
        b.estimated, b.parts
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

    /// A solid cube of 10 mm of a material, with its corner at `x` millimetres along x.
    fn cube(material: &str, x: i64) -> PartInstance {
        PartInstance {
            name: "cube".into(),
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
            name: "screw".into(),
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
    fn one_cube_weighs_its_volume_times_its_density_and_is_centred_in_its_box() {
        // 1000 mm3 of steel at 8000 kg/m3 is 8 g
        let b = balance(&set(vec![(1, cube("steel", 0))]), &densities()).unwrap();
        assert_eq!(b.mass_ug, 8_000_000);
        assert_eq!(b.centre_nm, Some([5_000_000; 3]));
        assert_eq!((b.parts, b.estimated), (1, 0));
    }

    #[test]
    fn two_equal_cubes_have_their_centre_half_way() {
        let b = balance(
            &set(vec![(1, cube("steel", 0)), (2, cube("steel", 100))]),
            &densities(),
        )
        .unwrap();
        assert_eq!(b.mass_ug, 16_000_000);
        assert_eq!(b.centre_nm, Some([55_000_000, 5_000_000, 5_000_000]));
    }

    #[test]
    fn the_heavier_part_pulls_the_centre_towards_itself() {
        // 8 g at 5 mm and 2.7 g at 105 mm: (8 * 5 + 2.7 * 105) / 10.7 = 30.2336448... mm
        let b = balance(
            &set(vec![(1, cube("steel", 0)), (2, cube("light", 100))]),
            &densities(),
        )
        .unwrap();
        assert_eq!(b.mass_ug, 10_700_000);
        assert_eq!(b.centre_nm.unwrap()[0], 30_233_645);
    }

    #[test]
    fn screws_are_counted_as_estimated() {
        let b = balance(
            &set(vec![(1, cube("steel", 0)), (2, screw(50))]),
            &densities(),
        )
        .unwrap();
        assert_eq!((b.parts, b.estimated), (2, 1));
        let text = balance_text(&b);
        assert!(text.contains("parts: 2\n"));
        assert!(text.contains("estimated centres: 1 of 2 parts"));
    }

    #[test]
    fn no_parts_have_no_centre_and_an_unknown_material_is_refused() {
        let b = balance(&BTreeMap::new(), &densities()).unwrap();
        assert_eq!(b, Balance::default());
        assert!(balance_text(&b).contains("centre of mass: none\n"));
        assert!(balance_text(&b).contains("mass: 0.000 g\n"));
        let e = balance(&set(vec![(1, cube("wood", 0))]), &densities()).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-045");
    }

    #[test]
    fn the_text_gives_the_mass_in_grams_and_the_centre_in_millimetres() {
        let b = balance(
            &set(vec![(1, cube("steel", 0)), (2, cube("steel", 100))]),
            &densities(),
        )
        .unwrap();
        assert_eq!(
            balance_text(&b),
            "parts: 2\nmass: 16.000 g\ncentre of mass: 55 5 5 mm\nestimated centres: 0 of 2 parts (screws and brackets use the middle of their box)\n"
        );
    }

    #[test]
    fn a_body_that_was_weighed_counts_as_a_part_with_an_exact_centre() {
        // a cube of 10 mm of steel at x = 0 (8 g) and a body of 2 g whose centre is at x = 40 mm:
        // (8 x 5 + 2 x 40) / 10 = 12 mm
        let parts = set(vec![(1, cube("steel", 0))]);
        let weighed = [Weighed {
            mass_ug: 2_000_000,
            middle2_nm: [80_000_000, 0, 0],
        }];
        let b = balance_with(&parts, &weighed, &densities()).unwrap();
        assert_eq!((b.parts, b.estimated), (2, 0));
        assert_eq!(b.mass_ug, 10_000_000);
        assert_eq!(b.centre_nm.unwrap()[0], 12_000_000);
        // with no part at all the body is the whole of it
        let alone = balance_with(&BTreeMap::new(), &weighed, &densities()).unwrap();
        assert_eq!(alone.parts, 1);
        assert_eq!(alone.centre_nm, Some([40_000_000, 0, 0]));
        // and without bodies it is the balance of the parts
        assert_eq!(
            balance_with(&parts, &[], &densities()).unwrap(),
            balance(&parts, &densities()).unwrap()
        );
    }
}
