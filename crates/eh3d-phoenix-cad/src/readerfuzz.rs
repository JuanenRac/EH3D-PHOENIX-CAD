// SPDX-License-Identifier: GPL-3.0-or-later
//! One test that damages the files the program itself writes, with a fixed seed, and gives them to
//! every reader of the same kind of file: whatever the damage, a reader answers with a value or
//! with a text that says what is wrong, and never panics, never runs past the limits it was given
//! and never reads outside the bytes it has. The files are the ones of the writers of this
//! library (STL, OBJ, OFF, PLY, 3MF, GLB, the project file and the history), so the test needs no
//! file from another program.

use crate::config::Settings;
use crate::featuredag::{FeatureDag, HistorySettings, Op};
use crate::glb;
use crate::interchange;
use crate::mesh::{read_stl, stl_ascii, stl_binary, Mesh};
use crate::prism::{Limits, Prism, Solid};
use crate::project::Document;
use crate::stlsolid;
use crate::threemf;
use crate::zipread::ZipLimits;
use crate::{off, ply, stlread, Length};

/// A generator of numbers with a fixed seed, the same on every system.
struct Seed(u64);

impl Seed {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A damaged copy: flipped bytes, a cut, bytes inserted, a piece repeated or a byte set to an
/// extreme value; one of them, chosen by the seed.
fn damage(good: &[u8], seed: &mut Seed) -> Vec<u8> {
    let mut v = good.to_vec();
    if v.is_empty() {
        return vec![seed.below(256) as u8];
    }
    match seed.below(5) {
        0 => {
            for _ in 0..1 + seed.below(8) {
                let at = seed.below(v.len());
                v[at] ^= 1 << seed.below(8);
            }
        }
        1 => v.truncate(seed.below(v.len())),
        2 => {
            let at = seed.below(v.len() + 1);
            for _ in 0..1 + seed.below(16) {
                v.insert(at, seed.below(256) as u8);
            }
        }
        3 => {
            let a = seed.below(v.len());
            let b = a + seed.below((v.len() - a).min(64) + 1);
            let piece = v[a..b].to_vec();
            let at = seed.below(v.len() + 1);
            v.splice(at..at, piece);
        }
        _ => {
            let at = seed.below(v.len());
            v[at] = [0x00, 0xFF, 0x7F, 0x80][seed.below(4)];
        }
    }
    v
}

fn cube() -> Mesh {
    let vertices = vec![
        [0.0, 0.0, 0.0],
        [10.0e6, 0.0, 0.0],
        [10.0e6, 10.0e6, 0.0],
        [0.0, 10.0e6, 0.0],
        [0.0, 0.0, 10.0e6],
        [10.0e6, 0.0, 10.0e6],
        [10.0e6, 10.0e6, 10.0e6],
        [0.0, 10.0e6, 10.0e6],
    ];
    let triangles = vec![
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    Mesh {
        vertices,
        triangles,
    }
}

fn solid() -> Solid {
    let l = |mm| Length::from_mm(mm).unwrap();
    Solid::from_prisms(
        &[
            Prism::new([l(0), l(0), l(0)], [l(10), l(10), l(10)]).unwrap(),
            Prism::new([l(10), l(0), l(0)], [l(5), l(5), l(5)]).unwrap(),
        ],
        &Limits::default(),
    )
    .unwrap()
}

fn project_bytes() -> Vec<u8> {
    let c = Settings::defaults();
    let id =
        |n: u16| crate::enclosure::Id::from_parts(1_700_000_000_000, n, u64::from(n) + 9).unwrap();
    let mut d = FeatureDag::new(HistorySettings::defaults());
    d.add_parameter(id(1), "side", "10 mm", &c).unwrap();
    d.add_feature(
        id(2),
        "cube",
        Op::Box,
        &[],
        &[
            ("x", "0 mm"),
            ("y", "0 mm"),
            ("z", "0 mm"),
            ("sx", "side"),
            ("sy", "side"),
            ("sz", "side"),
        ],
        &c,
    )
    .unwrap();
    d.add_feature(
        id(3),
        "both",
        Op::Mirror,
        &[id(2)],
        &[("axis", "0"), ("at", "-1 mm")],
        &c,
    )
    .unwrap();
    d.add_feature(
        id(4),
        "turned",
        Op::Rotate,
        &[id(3)],
        &[
            ("axis", "2"),
            ("quarters", "1"),
            ("cx", "0 mm"),
            ("cy", "0 mm"),
            ("cz", "0 mm"),
        ],
        &c,
    )
    .unwrap();
    let mut doc = Document::new();
    doc.set_history(&d).unwrap();
    doc.to_bytes()
}

#[test]
fn damaged_files_never_panic_a_reader_nor_pass_its_limits() {
    let mesh = cube();
    let named: [(&str, &Mesh); 1] = [("cube", &mesh)];
    let c = Settings::defaults();
    let zip = ZipLimits {
        max_entries: 64,
        max_entry_bytes: 1 << 20,
        max_total_bytes: 4 << 20,
    };
    let stl_b = stl_binary(&named);
    let stl_a = stl_ascii(&named).into_bytes();
    let stl_s = stlsolid::solid_to_stl(&solid(), 10_000, &Limits::default()).unwrap();
    let obj = interchange::obj(&named).into_bytes();
    let off_text = off::write(&named).into_bytes();
    let ply_a = ply::write_ascii(&named).into_bytes();
    let ply_b = ply::write_binary(&named);
    let mf = interchange::three_mf(&named);
    let glb_bytes = glb::solid_to_glb(&solid(), "fuzz", 10_000).unwrap();
    let project = project_bytes();
    let history = Document::from_bytes(&project, 1 << 20)
        .unwrap()
        .history(HistorySettings::defaults(), &c)
        .unwrap()
        .unwrap()
        .to_bytes();
    let mut seed = Seed(0xf0cc_ac1d);
    let cap = 20usize;
    for round in 0..250 {
        for good in [&stl_b, &stl_a, &stl_s] {
            let bad = damage(good, &mut seed);
            if let Ok(file) = stlread::read(&bad, cap) {
                let total: usize = file.solids.iter().map(|(_, t)| t.len()).sum();
                assert!(
                    total <= cap,
                    "round {round}: {total} triangles past the limit"
                );
            }
            let _ = read_stl(&bad);
        }
        let bad = damage(&obj, &mut seed);
        let _ = interchange::read_obj(&String::from_utf8_lossy(&bad));
        let bad = damage(&off_text, &mut seed);
        let _ = off::read(&String::from_utf8_lossy(&bad), cap);
        for good in [&ply_a, &ply_b] {
            let bad = damage(good, &mut seed);
            let _ = ply::read(&bad, cap);
        }
        let bad = damage(&mf, &mut seed);
        let _ = threemf::read_package(&bad, &zip, cap);
        let _ = interchange::read_stored_zip(&bad);
        let bad = damage(&glb_bytes, &mut seed);
        let _ = glb::check_glb(&bad);
        let bad = damage(&project, &mut seed);
        if let Ok(doc) = Document::from_bytes(&bad, 1 << 20) {
            if let Ok(Some(mut dag)) = doc.history(HistorySettings::defaults(), &c) {
                // a history that is read must be built or refused, never panic
                let _ = dag.rebuild(&c);
            }
        }
        let bad = damage(&history, &mut seed);
        if let Ok(mut dag) = FeatureDag::from_bytes(&bad, HistorySettings::defaults(), &c) {
            let _ = dag.rebuild(&c);
        }
    }
}

#[test]
fn the_damage_itself_is_the_same_on_every_system() {
    // the seed gives the same damaged bytes everywhere, so a failure can be reproduced
    let mut seed = Seed(7);
    let a = damage(b"0123456789abcdef", &mut seed);
    let mut again = Seed(7);
    assert_eq!(a, damage(b"0123456789abcdef", &mut again));
}
