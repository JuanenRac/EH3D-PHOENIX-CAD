//! Selectors: a short text that picks faces, edges and holes of the box and answers with their
//! stable names (`crate::names`), so that a script or another program never refers to a position
//! in a list.
//!
//! ```text
//! >X <X >Y <Y >Z <Z          the face at the largest or the smallest coordinate of an axis
//! |X |Y |Z                   the four faces that run along an axis
//! #X #Y #Z                   the two faces across an axis
//! faces                      all six
//! edges|X edges|Y edges|Z    the four edges along an axis
//! holes                      every hole
//! holes@>Z                   the holes of the faces of a face selector
//! holes[d<3mm]               the holes whose diameter (or width) passes a test: < <= = >= >
//! holes@|Z[d>=3 mm]          both
//! ```
//!
//! A text that is not a selector is refused with `PHOENIX-CAD-067`. A selector that picks nothing
//! is not an error: it is an empty list.

use crate::enclosure::{Enclosure, Face};
use crate::names::Element;
use crate::units::parse_length;
use crate::CadError;

/// The longest selector that is read.
const MAX_TEXT: usize = 200;

fn bad(text: &str) -> CadError {
    let shown: String = text.chars().take(60).collect();
    CadError::Selector(shown)
}

/// The axis of a letter, 0 to 2.
fn axis(letter: &str) -> Option<usize> {
    match letter {
        "X" | "x" => Some(0),
        "Y" | "y" => Some(1),
        "Z" | "z" => Some(2),
        _ => None,
    }
}

/// The two faces across an axis: at the smallest and at the largest coordinate.
fn across(axis: usize) -> [Face; 2] {
    match axis {
        0 => [Face::Left, Face::Right],
        1 => [Face::Front, Face::Back],
        _ => [Face::Bottom, Face::Top],
    }
}

fn along(index: usize) -> Vec<Face> {
    Face::ALL
        .into_iter()
        .filter(|f| !across(index).contains(f))
        .collect()
}

fn faces(text: &str, whole: &str) -> Result<Vec<Face>, CadError> {
    if text == "faces" {
        return Ok(Face::ALL.to_vec());
    }
    let mut chars = text.chars();
    let sign = chars.next().ok_or_else(|| bad(whole))?;
    let index = axis(chars.as_str()).ok_or_else(|| bad(whole))?;
    let [low, high] = across(index);
    match sign {
        '>' => Ok(vec![high]),
        '<' => Ok(vec![low]),
        '#' => Ok(vec![low, high]),
        '|' => Ok(along(index)),
        _ => Err(bad(whole)),
    }
}

/// A test of the diameter of a hole.
#[derive(Debug, Clone, Copy)]
struct Test {
    below: bool,
    equal: bool,
    above: bool,
    value: i64,
}

impl Test {
    fn passes(self, diameter: i64) -> bool {
        (self.below && diameter < self.value)
            || (self.equal && diameter == self.value)
            || (self.above && diameter > self.value)
    }
}

fn test(text: &str, whole: &str) -> Result<Test, CadError> {
    let rest = text.strip_prefix('d').ok_or_else(|| bad(whole))?;
    let (below, equal, above, number) = if let Some(n) = rest.strip_prefix("<=") {
        (true, true, false, n)
    } else if let Some(n) = rest.strip_prefix(">=") {
        (false, true, true, n)
    } else if let Some(n) = rest.strip_prefix('<') {
        (true, false, false, n)
    } else if let Some(n) = rest.strip_prefix('>') {
        (false, false, true, n)
    } else if let Some(n) = rest.strip_prefix('=') {
        (false, true, false, n)
    } else {
        return Err(bad(whole));
    };
    let value = parse_length(number.trim()).map_err(|_| bad(whole))?.get();
    Ok(Test {
        below,
        equal,
        above,
        value,
    })
}

/// The names of what `selector` picks in the box, in a fixed order.
pub fn select(enclosure: &Enclosure, selector: &str) -> Result<Vec<String>, CadError> {
    if selector.len() > MAX_TEXT {
        return Err(bad(selector));
    }
    let text = selector.trim();
    if let Some(rest) = text.strip_prefix("edges") {
        let letter = rest.strip_prefix('|').ok_or_else(|| bad(selector))?;
        let index = axis(letter).ok_or_else(|| bad(selector))?;
        let parallel = along(index);
        let mut out = Vec::new();
        for (i, a) in parallel.iter().enumerate() {
            for b in parallel.iter().skip(i + 1) {
                if a.opposite() != *b {
                    out.push(Element::edge(*a, *b)?.name());
                }
            }
        }
        return Ok(out);
    }
    if let Some(rest) = text.strip_prefix("holes") {
        let (rest, limit) = match rest.split_once('[') {
            Some((head, tail)) => {
                let inside = tail.strip_suffix(']').ok_or_else(|| bad(selector))?;
                (head, Some(test(inside, selector)?))
            }
            None => (rest, None),
        };
        let of = match rest {
            "" => Face::ALL.to_vec(),
            _ => faces(
                rest.strip_prefix('@').ok_or_else(|| bad(selector))?,
                selector,
            )?,
        };
        return Ok(enclosure
            .holes()
            .iter()
            .filter(|(_, hole)| of.contains(&hole.face))
            .filter(|(_, hole)| limit.is_none_or(|t| t.passes(hole.diameter.get())))
            .map(|(id, _)| Element::HoleWall(*id).name())
            .collect());
    }
    Ok(faces(text, selector)?
        .into_iter()
        .map(|f| Element::Face(f).name())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eh3dfile::tests::Seeded;
    use crate::enclosure::{EnclosureParams, Hole, HoleStyle, Operation};
    use crate::Length;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn enclosure() -> Enclosure {
        let mut e = Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap();
        for (i, (face, d)) in [(Face::Front, 2), (Face::Front, 5), (Face::Top, 3)]
            .into_iter()
            .enumerate()
        {
            let hole = Hole {
                name: format!("h{i}"),
                face,
                u: mm(20 + 20 * i as i64),
                v: mm(17),
                diameter: mm(d),
                style: HoleStyle::Plain,
            };
            e.apply(Operation::AddHole {
                id: crate::boardart::id_from(&hole.name).unwrap(),
                hole,
            })
            .unwrap();
        }
        e
    }

    #[test]
    fn faces_and_edges_are_picked_by_the_axes() {
        let e = enclosure();
        assert_eq!(select(&e, ">Z").unwrap(), vec!["box:face:top"]);
        assert_eq!(select(&e, "<x").unwrap(), vec!["box:face:left"]);
        assert_eq!(
            select(&e, "#Y").unwrap(),
            vec!["box:face:front", "box:face:back"]
        );
        assert_eq!(select(&e, "|Z").unwrap().len(), 4);
        assert_eq!(select(&e, "faces").unwrap().len(), 6);
        let edges = select(&e, "edges|Z").unwrap();
        assert_eq!(edges.len(), 4);
        assert!(
            edges.contains(&"box:edge:front-left".to_owned()),
            "{edges:?}"
        );
        for axis in ["X", "Y", "Z"] {
            for name in select(&e, &format!("edges|{axis}")).unwrap() {
                assert_eq!(e.resolve(&name).unwrap().name(), name);
            }
        }
    }

    #[test]
    fn holes_are_picked_by_face_and_diameter() {
        let e = enclosure();
        let count = |s: &str| select(&e, s).unwrap().len();
        assert_eq!(count("holes"), 3);
        assert_eq!(count("holes@<Y"), 2);
        assert_eq!(count("holes@>Z"), 1);
        assert_eq!(count("holes[d<3mm]"), 1);
        assert_eq!(count("holes[d<=3 mm]"), 2);
        assert_eq!(count("holes[d=5mm]"), 1);
        assert_eq!(count("holes[d>=3mm]"), 2);
        assert_eq!(count("holes[d>5mm]"), 0);
        assert_eq!(count("holes@<Y[d>3mm]"), 1);
        assert_eq!(count("holes@>X"), 0);
    }

    #[test]
    fn texts_that_are_not_selectors_are_refused_and_none_panics() {
        let e = enclosure();
        for text in [
            "",
            ">",
            "^Z",
            ">W",
            "edges",
            "edges>Z",
            "holes>Z",
            "holes[",
            "holes[d]",
            "holes[d<]",
            "holes[r<3mm]",
            "holes@",
            "face",
        ] {
            let error = select(&e, text).unwrap_err();
            assert_eq!(error.code(), "PHOENIX-CAD-067", "{text}");
        }
        let alphabet: Vec<char> = "<>#|@[]=dXYZ holesedgfc3m.".chars().collect();
        let mut random = Seeded(0x00CA_DB06);
        for _ in 0..3000 {
            let text: String = (0..random.below(14))
                .map(|_| alphabet[random.below(alphabet.len() as u64) as usize])
                .collect();
            if let Ok(names) = select(&e, &text) {
                for name in names {
                    assert!(e.resolve(&name).is_ok(), "{text}: {name}");
                }
            }
        }
        assert!(select(&e, &"x".repeat(500)).is_err());
    }
}
