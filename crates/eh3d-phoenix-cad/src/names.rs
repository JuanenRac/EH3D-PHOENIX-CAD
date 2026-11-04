//! Stable names of the elements of an enclosure.
//!
//! A name is built by rule from what the element is, never from a position in a list: the face
//! `box:face:front`, the edge between two faces `box:edge:front-top` (the two faces in their fixed
//! order, never two opposite faces), the face that a rounded or chamfered edge turns into
//! `box:blend:front-top` (it exists only while that edge is treated) and the wall of a hole
//! `hole:<id>:wall`. Adding, removing or
//! reordering other operations does not change the name of an element, and a name that no longer
//! exists is an error ([`CadError::Reference`]): nothing ever picks "another similar face".

use crate::enclosure::{Enclosure, Face, Id};
use crate::CadError;

/// An element of an enclosure that other objects (a dimension, a datum, a fillet) can refer to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Element {
    /// A face of the box.
    Face(Face),
    /// The edge shared by two faces (the first is the smaller in the fixed order of the faces).
    Edge(Face, Face),
    /// The cylindrical wall of a hole.
    HoleWall(Id),
    /// The face made by rounding or chamfering the edge between two faces. It exists only while
    /// that edge has a treatment.
    Blend(Face, Face),
}

impl Element {
    /// The edge between two faces, refused when they are the same face or opposite faces.
    pub fn edge(a: Face, b: Face) -> Result<Element, CadError> {
        if a == b || a.opposite() == b {
            return Err(CadError::Reference(format!(
                "box:edge:{}-{}",
                a.name(),
                b.name()
            )));
        }
        Ok(if a < b {
            Element::Edge(a, b)
        } else {
            Element::Edge(b, a)
        })
    }

    /// The face made by treating the edge between two faces, refused when they are the same face
    /// or opposite faces.
    pub fn blend(a: Face, b: Face) -> Result<Element, CadError> {
        match Element::edge(a, b)? {
            Element::Edge(x, y) => Ok(Element::Blend(x, y)),
            other => Ok(other),
        }
    }

    /// The canonical name.
    pub fn name(&self) -> String {
        match self {
            Element::Face(f) => format!("box:face:{}", f.name()),
            Element::Edge(a, b) => format!("box:edge:{}-{}", a.name(), b.name()),
            Element::HoleWall(id) => format!("hole:{id}:wall"),
            Element::Blend(a, b) => format!("box:blend:{}-{}", a.name(), b.name()),
        }
    }

    /// Reads a canonical name. A name that is not written exactly as [`Element::name`] writes it
    /// (for instance with capital letters) is refused.
    pub fn parse(text: &str) -> Result<Element, CadError> {
        let bad = || CadError::Reference(text.to_string());
        let parts: Vec<&str> = text.split(':').collect();
        let element = match parts.as_slice() {
            ["box", "face", f] => Element::Face(Face::from_name(f).ok_or_else(bad)?),
            ["box", kind @ ("edge" | "blend"), pair] => {
                let (a, b) = pair.split_once('-').ok_or_else(bad)?;
                let a = Face::from_name(a).ok_or_else(bad)?;
                let b = Face::from_name(b).ok_or_else(bad)?;
                if *kind == "edge" {
                    Element::edge(a, b)?
                } else {
                    Element::blend(a, b)?
                }
            }
            ["hole", id, "wall"] => Element::HoleWall(id.parse::<Id>().map_err(|_| bad())?),
            _ => return Err(bad()),
        };
        if element.name() == text {
            Ok(element)
        } else {
            Err(bad())
        }
    }
}

impl Enclosure {
    /// Whether the element exists in this enclosure.
    pub fn has_element(&self, element: &Element) -> bool {
        match element {
            Element::Face(_) | Element::Edge(_, _) => true,
            Element::HoleWall(id) => self.holes().contains_key(id),
            Element::Blend(a, b) => !self.edging().treatment_of(*a, *b).is_sharp(),
        }
    }

    /// Reads a name and checks that the element exists. A hole that was removed is refused.
    pub fn resolve(&self, text: &str) -> Result<Element, CadError> {
        let element = Element::parse(text)?;
        if self.has_element(&element) {
            Ok(element)
        } else {
            Err(CadError::Reference(text.to_string()))
        }
    }

    /// The names of every element: the six faces, the twelve edges, the faces made by treated
    /// edges and the walls of the holes by id. The list is the same whatever the order in which the holes were added.
    pub fn element_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Face::ALL
            .into_iter()
            .map(|f| Element::Face(f).name())
            .collect();
        for a in Face::ALL {
            for b in Face::ALL {
                if a < b {
                    if let Ok(e) = Element::edge(a, b) {
                        names.push(e.name());
                    }
                }
            }
        }
        for a in Face::ALL {
            for b in Face::ALL {
                if a < b {
                    if let Ok(e) = Element::blend(a, b) {
                        if self.has_element(&e) {
                            names.push(e.name());
                        }
                    }
                }
            }
        }
        names.extend(self.holes().keys().map(|id| Element::HoleWall(*id).name()));
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::{EnclosureParams, Hole, Operation};
    use crate::Length;
    use eh3d_container::ObjectId;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn params() -> EnclosureParams {
        EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        }
    }

    fn id(n: u64) -> Id {
        ObjectId::from_parts(1_700_000_000_000, (n & 0x0fff) as u16, n).unwrap()
    }

    fn add(n: u64, face: Face, u: i64) -> Operation {
        Operation::AddHole {
            id: id(n),
            hole: Hole {
                name: format!("h{n}"),
                face,
                u: mm(u),
                v: mm(17),
                diameter: mm(6),
                style: crate::enclosure::HoleStyle::Plain,
            },
        }
    }

    #[test]
    fn every_name_reads_back_to_the_same_element() {
        let mut e = Enclosure::new(params()).unwrap();
        e.apply(add(1, Face::Front, 20)).unwrap();
        let names = e.element_names();
        assert_eq!(names.len(), 6 + 12 + 1);
        for name in &names {
            let element = e.resolve(name).unwrap();
            assert_eq!(&element.name(), name);
        }
        assert_eq!(names[0], "box:face:front");
        assert_eq!(names[6], "box:edge:front-left");
    }

    #[test]
    fn an_edge_joins_two_faces_that_touch_and_has_one_name() {
        assert_eq!(
            Element::edge(Face::Top, Face::Front).unwrap(),
            Element::edge(Face::Front, Face::Top).unwrap()
        );
        assert_eq!(
            Element::edge(Face::Top, Face::Front).unwrap().name(),
            "box:edge:front-top"
        );
        assert!(Element::edge(Face::Front, Face::Back).is_err());
        assert!(Element::edge(Face::Left, Face::Right).is_err());
        assert!(Element::edge(Face::Top, Face::Bottom).is_err());
        assert!(Element::edge(Face::Top, Face::Top).is_err());
    }

    #[test]
    fn names_that_are_not_canonical_are_refused_with_one_code() {
        let upper = format!("hole:{}:wall", id(1)).to_uppercase();
        for text in [
            "",
            "front",
            "box:face:middle",
            "box:face:Front",
            "box:edge:top-front",
            "box:edge:front-back",
            "box:edge:front",
            "hole:not-an-id:wall",
            "box:face:front:extra",
            upper.as_str(),
        ] {
            let err = Element::parse(text).unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-010", "{text:?}");
        }
    }

    #[test]
    fn the_names_do_not_depend_on_the_order_of_the_operations() {
        let order_a = [1u64, 2, 3];
        let order_b = [3u64, 1, 2];
        let build = |order: &[u64]| {
            let mut e = Enclosure::new(params()).unwrap();
            for n in order {
                e.apply(add(*n, Face::Front, 10 + 20 * i64::try_from(*n).unwrap()))
                    .unwrap();
            }
            e
        };
        assert_eq!(
            build(&order_a).element_names(),
            build(&order_b).element_names()
        );
    }

    #[test]
    fn removing_or_adding_other_holes_keeps_the_name_of_a_hole() {
        let mut e = Enclosure::new(params()).unwrap();
        e.apply(add(1, Face::Front, 20)).unwrap();
        e.apply(add(2, Face::Front, 50)).unwrap();
        let name = Element::HoleWall(id(2)).name();
        assert!(e.resolve(&name).is_ok());
        e.apply(Operation::RemoveHole { id: id(1) }).unwrap();
        e.apply(add(3, Face::Back, 20)).unwrap();
        assert_eq!(e.resolve(&name).unwrap(), Element::HoleWall(id(2)));
        // a bigger box does not rename anything either
        let mut wider = params();
        wider.inner_length = mm(120);
        e.apply(Operation::SetParams(wider)).unwrap();
        assert_eq!(e.resolve(&name).unwrap(), Element::HoleWall(id(2)));
        assert!(e.resolve("box:edge:front-top").is_ok());
    }

    #[test]
    fn a_removed_hole_is_an_error_and_no_other_element_takes_its_place() {
        let mut e = Enclosure::new(params()).unwrap();
        e.apply(add(1, Face::Front, 20)).unwrap();
        e.apply(add(2, Face::Front, 50)).unwrap();
        let name = Element::HoleWall(id(1)).name();
        e.apply(Operation::RemoveHole { id: id(1) }).unwrap();
        let err = e.resolve(&name).unwrap_err();
        assert_eq!(err, CadError::Reference(name));
        assert_eq!(err.code(), "PHOENIX-CAD-010");
        assert!(!e.has_element(&Element::HoleWall(id(1))));
        assert!(e.has_element(&Element::HoleWall(id(2))));
    }

    #[test]
    fn a_treated_edge_gets_a_face_with_a_stable_name_and_a_sharp_one_does_not() {
        use crate::enclosure::{Edging, Treatment};
        let mut e = Enclosure::new(params()).unwrap();
        assert_eq!(e.element_names().len(), 6 + 12);
        assert!(e.resolve("box:blend:front-top").is_err());
        e.apply(Operation::SetEdging(Edging {
            top: Treatment::Chamfer(mm(2)),
            ..Edging::default()
        }))
        .unwrap();
        // four edges around the lid turn into faces
        let names = e.element_names();
        assert_eq!(names.len(), 6 + 12 + 4);
        for name in ["front-top", "back-top", "left-top", "right-top"] {
            let full = format!("box:blend:{name}");
            assert!(names.contains(&full), "{full}");
            assert_eq!(e.resolve(&full).unwrap().name(), full);
        }
        assert!(e.resolve("box:blend:front-left").is_err());
        // the blend of an edge is the same element whichever way its faces are written
        assert_eq!(
            Element::parse("box:blend:front-top").unwrap(),
            Element::blend(Face::Top, Face::Front).unwrap()
        );
        assert!(Element::parse("box:blend:front-back").is_err());
        assert!(Element::parse("box:blend:top-front").is_err());
        // the edge itself keeps its name
        assert!(e.resolve("box:edge:front-top").is_ok());
    }
}
