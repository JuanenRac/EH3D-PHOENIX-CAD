// SPDX-License-Identifier: GPL-3.0-or-later
//! The hole schedule: every hole of the box in a table for the person who drills it.
//!
//! A drawing shows where the holes are; a workshop wants a list. [`rows`] lists the holes of the
//! box by face (front, back, left, right, top, bottom), then by their position `u` and `v` from the
//! lower left corner of the face as seen from outside, and gives each a tag `H1`, `H2`, ... in that
//! order, so the same box always gives the same tags. [`tools`] groups them by diameter and shape,
//! which is the list of drills and cutters that the job needs. Both are written as comma
//! separated values with the lengths in millimetres.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::enclosure::{Enclosure, HoleStyle};
use crate::scad::mm_text;

/// A line of the schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoleRow {
    /// The tag, `H1` upwards.
    pub tag: String,
    /// The name of the hole.
    pub name: String,
    /// The face.
    pub face: &'static str,
    /// The position to the right from the lower left corner of the face, in nanometres.
    pub u: i64,
    /// The position upwards, in nanometres.
    pub v: i64,
    /// The diameter (the width of a rectangle or a slot), in nanometres.
    pub diameter: i64,
    /// The word of the shape.
    pub style: &'static str,
    /// What else makes the shape, in words with millimetres.
    pub detail: String,
}

/// The word of a style of hole.
pub fn style_name(style: &HoleStyle) -> &'static str {
    match style {
        HoleStyle::Plain => "plain",
        HoleStyle::Countersunk { .. } => "countersunk",
        HoleStyle::Counterbored { .. } => "counterbored",
        HoleStyle::Rectangular { .. } => "rectangular",
        HoleStyle::Oval { .. } => "slot",
    }
}

fn detail_of(style: &HoleStyle) -> String {
    match style {
        HoleStyle::Plain => String::new(),
        HoleStyle::Countersunk { head_diameter } => {
            format!("head {}", mm_text(head_diameter.get()))
        }
        HoleStyle::Counterbored {
            head_diameter,
            depth,
        } => format!(
            "head {} depth {}",
            mm_text(head_diameter.get()),
            mm_text(depth.get())
        ),
        HoleStyle::Rectangular { height } => format!("height {}", mm_text(height.get())),
        HoleStyle::Oval { length, vertical } => format!(
            "length {} {}",
            mm_text(length.get()),
            if *vertical { "vertical" } else { "horizontal" }
        ),
    }
}

/// The holes of the box in order, with their tags.
pub fn rows(enclosure: &Enclosure) -> Vec<HoleRow> {
    let mut holes: Vec<_> = enclosure.holes().iter().collect();
    holes.sort_by(|(ia, a), (ib, b)| {
        (a.face, a.u.get(), a.v.get(), &a.name, *ia).cmp(&(
            b.face,
            b.u.get(),
            b.v.get(),
            &b.name,
            *ib,
        ))
    });
    holes
        .into_iter()
        .enumerate()
        .map(|(n, (_, h))| HoleRow {
            tag: format!("H{}", n + 1),
            name: h.name.clone(),
            face: h.face.name(),
            u: h.u.get(),
            v: h.v.get(),
            diameter: h.diameter.get(),
            style: style_name(&h.style),
            detail: detail_of(&h.style),
        })
        .collect()
}

/// The schedule as comma separated values, with a last line that counts the holes.
pub fn rows_csv(rows: &[HoleRow]) -> String {
    let mut out = String::from("tag,name,face,u_mm,v_mm,diameter_mm,style,detail\n");
    for r in rows {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{}",
            r.tag,
            r.name,
            r.face,
            mm_text(r.u),
            mm_text(r.v),
            mm_text(r.diameter),
            r.style,
            r.detail
        );
    }
    let _ = writeln!(out, "total,{},,,,,,", rows.len());
    out
}

/// The tools the holes need: for each diameter, style and detail how many holes, the smallest
/// diameter first.
pub fn tools(rows: &[HoleRow]) -> Vec<(i64, &'static str, String, usize)> {
    let mut groups: BTreeMap<(i64, &'static str, String), usize> = BTreeMap::new();
    for r in rows {
        *groups
            .entry((r.diameter, r.style, r.detail.clone()))
            .or_insert(0) += 1;
    }
    groups
        .into_iter()
        .map(|((d, s, detail), n)| (d, s, detail, n))
        .collect()
}

/// The tools as comma separated values, with a last line that counts the holes.
pub fn tools_csv(rows: &[HoleRow]) -> String {
    let mut out = String::from("diameter_mm,style,detail,count\n");
    for (d, style, detail, n) in tools(rows) {
        let _ = writeln!(out, "{},{style},{detail},{n}", mm_text(d));
    }
    let _ = writeln!(out, "total,,,{}", rows.len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::{EnclosureParams, Face, Hole, Id, Operation};
    use crate::Length;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn id(n: u64) -> Id {
        Id::from_parts(1_700_000_000_000, 1, n).unwrap()
    }

    fn model() -> Enclosure {
        Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap()
    }

    fn put(e: &mut Enclosure, n: u64, name: &str, face: Face, at: [i64; 3], s: HoleStyle) {
        e.apply(Operation::AddHole {
            id: id(n),
            hole: Hole {
                name: name.into(),
                face,
                u: mm(at[0]),
                v: mm(at[1]),
                diameter: mm(at[2]),
                style: s,
            },
        })
        .unwrap();
    }

    fn with_holes() -> Enclosure {
        let mut e = model();
        put(&mut e, 1, "usb", Face::Back, [30, 15, 8], HoleStyle::Plain);
        put(
            &mut e,
            2,
            "led b",
            Face::Front,
            [70, 15, 5],
            HoleStyle::Plain,
        );
        put(
            &mut e,
            3,
            "led a",
            Face::Front,
            [20, 15, 5],
            HoleStyle::Plain,
        );
        put(
            &mut e,
            4,
            "screw",
            Face::Top,
            [30, 20, 3],
            HoleStyle::Countersunk {
                head_diameter: mm(6),
            },
        );
        put(
            &mut e,
            5,
            "slot",
            Face::Top,
            [60, 20, 4],
            HoleStyle::Oval {
                length: mm(10),
                vertical: true,
            },
        );
        put(
            &mut e,
            6,
            "window",
            Face::Left,
            [20, 10, 12],
            HoleStyle::Rectangular { height: mm(8) },
        );
        put(&mut e, 7, "pin", Face::Top, [90, 20, 5], HoleStyle::Plain);
        e
    }

    #[test]
    fn the_holes_are_listed_by_face_then_by_position_with_stable_tags() {
        let r = rows(&with_holes());
        let order: Vec<(&str, &str, &str)> = r
            .iter()
            .map(|h| (h.tag.as_str(), h.face, h.name.as_str()))
            .collect();
        assert_eq!(
            order,
            [
                ("H1", "front", "led a"),
                ("H2", "front", "led b"),
                ("H3", "back", "usb"),
                ("H4", "left", "window"),
                ("H5", "top", "screw"),
                ("H6", "top", "slot"),
                ("H7", "top", "pin"),
            ]
        );
        assert_eq!(r[0].u, 20_000_000);
        assert_eq!(r[0].diameter, 5_000_000);
        // the same box gives the same table
        assert_eq!(r, rows(&with_holes()));
    }

    #[test]
    fn the_csv_has_the_detail_of_each_shape_and_the_count() {
        let csv = rows_csv(&rows(&with_holes()));
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "tag,name,face,u_mm,v_mm,diameter_mm,style,detail");
        assert_eq!(lines[1], "H1,led a,front,20,15,5,plain,");
        assert_eq!(lines[4], "H4,window,left,20,10,12,rectangular,height 8");
        assert_eq!(lines[5], "H5,screw,top,30,20,3,countersunk,head 6");
        assert_eq!(lines[6], "H6,slot,top,60,20,4,slot,length 10 vertical");
        assert_eq!(lines[8], "total,7,,,,,,");
        assert_eq!(
            rows_csv(&[]),
            "tag,name,face,u_mm,v_mm,diameter_mm,style,detail\ntotal,0,,,,,,\n"
        );
    }

    #[test]
    fn the_tools_group_the_holes_by_diameter_and_shape_smallest_first() {
        let csv = tools_csv(&rows(&with_holes()));
        assert_eq!(
            csv,
            "diameter_mm,style,detail,count\n\
             3,countersunk,head 6,1\n\
             4,slot,length 10 vertical,1\n\
             5,plain,,3\n\
             8,plain,,1\n\
             12,rectangular,height 8,1\n\
             total,,,7\n"
        );
    }

    #[test]
    fn a_counterbore_says_its_head_and_its_depth() {
        let mut e = model();
        put(
            &mut e,
            1,
            "socket",
            Face::Bottom,
            [50, 30, 3],
            HoleStyle::Counterbored {
                head_diameter: mm(6),
                depth: Length::new(1_500_000).unwrap(),
            },
        );
        let r = rows(&e);
        assert_eq!(r[0].detail, "head 6 depth 1.5");
        assert_eq!(r[0].style, "counterbored");
    }
}
