use super::*;
use crate::prism::{Limits, Prism};
use crate::Length;

fn l(mm: i64) -> Length {
    Length::from_mm(mm).unwrap()
}

fn solid(boxes: &[([i64; 3], [i64; 3])]) -> Solid {
    let prisms: Vec<Prism> = boxes
        .iter()
        .map(|(c, s)| Prism::new([l(c[0]), l(c[1]), l(c[2])], [l(s[0]), l(s[1]), l(s[2])]).unwrap())
        .collect();
    Solid::from_prisms(&prisms, &Limits::default()).unwrap()
}

const BIG: u32 = 1_000_000;

#[test]
fn a_cube_is_written_and_read_back_with_its_volume_and_its_corners() {
    let cube = solid(&[([0, 0, 0], [10, 10, 10])]);
    let bytes = solid_to_glb(&cube, "cube", BIG).unwrap();
    let info = check_glb(&bytes).unwrap();
    assert_eq!((info.vertices, info.triangles), (24, 12));
    assert!(
        (info.volume_m3 - 1.0e-6).abs() < 1.0e-6 * 1.0e-5,
        "{}",
        info.volume_m3
    );
    for axis in 0..3 {
        assert!(info.min[axis].abs() < 1.0e-12);
        assert!((info.max[axis] - 0.01).abs() < 1.0e-9);
    }
    // the header says what the file is
    assert_eq!(&bytes[0..4], b"glTF");
    assert!(bytes.len().is_multiple_of(4));
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("\"name\":\"cube\"") && text.contains(PROFILE));
    assert!(text.contains("\"version\":\"2.0\""));
}

#[test]
fn the_same_solid_gives_the_same_bytes_and_a_plate_with_a_hole_keeps_its_volume() {
    let plate = solid(&[([0, 0, 0], [40, 30, 5])]);
    let hole = solid(&[([10, 10, 0], [10, 10, 5])]);
    let drilled = plate.difference(&hole, &Limits::default()).unwrap();
    let a = solid_to_glb(&drilled, "plate", BIG).unwrap();
    let b = solid_to_glb(&drilled.clone(), "plate", BIG).unwrap();
    assert_eq!(a, b);
    // the same solid made another way
    let pieces = solid(&[
        ([0, 0, 0], [40, 10, 5]),
        ([0, 20, 0], [40, 10, 5]),
        ([0, 10, 0], [10, 10, 5]),
        ([20, 10, 0], [20, 10, 5]),
    ]);
    assert_eq!(pieces, drilled);
    assert_eq!(solid_to_glb(&pieces, "plate", BIG).unwrap(), a);
    let info = check_glb(&a).unwrap();
    let exact = drilled.volume_nm3() as f64 * 1.0e-27;
    assert!(
        (info.volume_m3 - exact).abs() < exact * 1.0e-5,
        "{} {exact}",
        info.volume_m3
    );
    assert_eq!(info.triangles, drilled.prisms().len() * 12);
}

#[test]
fn what_cannot_be_written_is_refused_with_its_code() {
    let cube = solid(&[([0, 0, 0], [1, 1, 1])]);
    assert_eq!(
        solid_to_glb(&Solid::empty(), "empty", BIG)
            .unwrap_err()
            .code(),
        "PHOENIX-CAD-041"
    );
    for name in ["", "a b", "a/b", "a\"b", &"x".repeat(65), "ñ"] {
        assert_eq!(
            solid_to_glb(&cube, name, BIG).unwrap_err().code(),
            "PHOENIX-CAD-018",
            "{name:?}"
        );
    }
    assert_eq!(
        solid_to_glb(&cube, "cube", 11).unwrap_err().code(),
        "PHOENIX-CAD-050"
    );
    assert!(solid_to_glb(&cube, "cube", 12).is_ok());
}

#[test]
fn a_file_that_is_not_of_the_profile_is_refused_and_says_why() {
    let cube = solid(&[([0, 0, 0], [10, 10, 10])]);
    let good = solid_to_glb(&cube, "cube", BIG).unwrap();
    assert!(check_glb(&good).is_ok());
    let mut cases: Vec<(Vec<u8>, &str)> = vec![
        (Vec::new(), "start with glTF"),
        (b"glTF".to_vec(), "version"),
        (good[..good.len() - 4].to_vec(), "length of the header"),
        (good[..30].to_vec(), "length of the header"),
    ];
    let mut v = good.clone();
    v[4] = 1;
    cases.push((v, "version"));
    let mut v = good.clone();
    v[16] = b'X';
    cases.push((v, "JSON chunk"));
    let mut v = good.clone();
    let json_len = u32::from_le_bytes([v[12], v[13], v[14], v[15]]) as usize;
    v[20 + json_len + 4] = b'X';
    cases.push((v, "binary chunk"));
    let mut v = good.clone();
    let at = String::from_utf8_lossy(&v)
        .find("phoenix-cad-glb/1\",\"unit")
        .unwrap();
    v[at + 14] = b'2';
    cases.push((v, "profile"));
    let mut v = good.clone();
    let at = String::from_utf8_lossy(&v).find("\"count\":24").unwrap();
    v[at + 8] = b'9';
    cases.push((v, "counts of the accessors"));
    // an index past the last vertex
    let mut v = good.clone();
    let tail = v.len() - 4;
    v[tail..].copy_from_slice(&999u32.to_le_bytes());
    cases.push((v, "past the last vertex"));
    // a position that is not a number
    let mut v = good.clone();
    let bin = 20 + json_len + 8;
    v[bin..bin + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    cases.push((v, "not a number"));
    for (bytes, part) in cases {
        let e = check_glb(&bytes).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-018");
        assert!(e.to_string().contains(part), "{part}: {e}");
    }
}

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
}

#[test]
fn damaged_files_are_never_a_panic_and_what_still_reads_has_a_closed_surface() {
    let mut seed = Seed(0x5eed_00e1);
    let sources = [
        solid_to_glb(&solid(&[([0, 0, 0], [10, 10, 10])]), "cube", BIG).unwrap(),
        solid_to_glb(
            &solid(&[([0, 0, 0], [40, 10, 5]), ([0, 20, 0], [40, 10, 5])]),
            "two",
            BIG,
        )
        .unwrap(),
    ];
    let (mut read, mut refused) = (0u32, 0u32);
    for round in 0..4000usize {
        let mut bytes = sources[round % 2].clone();
        for _ in 0..1 + seed.next() % 3 {
            let at = (seed.next() % bytes.len() as u64) as usize;
            match seed.next() % 4 {
                0 => bytes[at] ^= 1 << (seed.next() % 8),
                1 => bytes.truncate(at),
                2 => {
                    bytes.remove(at);
                }
                _ => bytes.insert(at, (seed.next() % 256) as u8),
            }
            if bytes.is_empty() {
                break;
            }
        }
        match check_glb(&bytes) {
            Ok(info) => {
                read += 1;
                assert!(info.triangles >= 1 && info.vertices >= 3, "round {round}");
            }
            Err(e) => {
                refused += 1;
                assert_eq!(e.code(), "PHOENIX-CAD-018", "round {round}: {e}");
            }
        }
    }
    assert!(read > 0 && refused > 1000, "{read} read, {refused} refused");
}
