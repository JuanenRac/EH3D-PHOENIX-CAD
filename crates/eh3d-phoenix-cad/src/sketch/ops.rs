//! Edits of the geometry of a sketch that change what is drawn: chamfering the corner of two
//! lines, trimming a line at another one, extending a line to another one, repeating elements
//! along a line or round a point, and offsetting a closed outline.
//!
//! Every operation leaves the sketch exactly as it was when it is refused. Positions are worked
//! out in floating point from the nanometres and rounded back to the nanometre, with the
//! operations that every platform computes the same way (sums, products, quotients and square
//! roots); the turns of a circular pattern use the integer sine and cosine of the units module.

use std::collections::BTreeMap;

use super::{bad_constraint, bad_element, Constraint, Element, ElementId, Entry, Kind, Sketch};
use crate::units::{div_round_half_away, Angle};
use crate::{CadError, Length};

/// The most positions of a pattern (the original included).
pub const MAX_PATTERN_COUNT: u32 = 1000;

/// What is always left of a line that is shortened: a millimetre.
const ROOM_NM: f64 = 1_000_000.0;

/// The corner of an offset outline is mitred only while the mitre is at most six times the
/// distance long: that is `2 / 36` in the formula `k = 1 + cos` of the turn between two sides.
const MITRE_LIMIT: f64 = 2.0 / 36.0;

type Ends = (ElementId, ElementId);

type Pt = (f64, f64);

fn place(p: Pt) -> Result<(Length, Length), CadError> {
    let round = |v: f64| -> Result<Length, CadError> {
        if !v.is_finite() || v.abs() > 9.0e18 {
            return Err(CadError::Overflow("sketch coordinates"));
        }
        Length::new(v.round() as i64)
    };
    Ok((round(p.0)?, round(p.1)?))
}

fn cross(a: Pt, b: Pt) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

fn dot(a: Pt, b: Pt) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

/// Where two lines (each through a point with a direction) meet: the two parameters, or nothing
/// when they are parallel.
fn meet(p: Pt, r: Pt, q: Pt, s: Pt) -> Option<(f64, f64)> {
    let den = cross(r, s);
    if den.abs() <= 1e-9 * (dot(r, r).sqrt() * dot(s, s).sqrt()) {
        return None;
    }
    let qp = (q.0 - p.0, q.1 - p.1);
    Some((cross(qp, s) / den, cross(qp, r) / den))
}

/// Whether two segments cross in their insides.
fn segments_cross(a: Pt, b: Pt, c: Pt, d: Pt) -> bool {
    let (r, s) = ((b.0 - a.0, b.1 - a.1), (d.0 - c.0, d.1 - c.1));
    match meet(a, r, c, s) {
        Some((t, u)) => t > 1e-9 && t < 1.0 - 1e-9 && u > 1e-9 && u < 1.0 - 1e-9,
        None => false,
    }
}

impl Sketch {
    fn xy(&self, id: ElementId) -> Result<Pt, CadError> {
        let (x, y) = self.point(id)?;
        Ok((x.get() as f64, y.get() as f64))
    }

    /// Adds a point and moves it to just before `before` in the list, so that a line that is
    /// made of it comes after it.
    fn add_point_before(
        &mut self,
        at: (Length, Length),
        before: ElementId,
    ) -> Result<ElementId, CadError> {
        let id = self.add_point(at.0, at.1)?;
        if let Some(entry) = self.elements.pop() {
            let index = self
                .elements
                .iter()
                .position(|e| e.id == before)
                .unwrap_or(0);
            self.elements.insert(index, entry);
        }
        Ok(id)
    }

    /// Whether anything but `except` is made of the point or has a constraint about it.
    fn point_in_use(&self, point: ElementId, except: ElementId) -> bool {
        self.elements
            .iter()
            .any(|e| e.id != except && e.element.parts().contains(&point))
            || self
                .constraints
                .iter()
                .any(|c| c.constraint.elements().contains(&point))
    }

    /// Checks that the length and the ends of a line can change: the only constraints about it
    /// are the ones that keep its direction.
    fn check_free_to_change(&self, line: ElementId) -> Result<(), CadError> {
        for c in &self.constraints {
            let keeps = matches!(
                c.constraint,
                Constraint::Horizontal(_)
                    | Constraint::Vertical(_)
                    | Constraint::Parallel(..)
                    | Constraint::Perpendicular(..)
                    | Constraint::Collinear(..)
            );
            if c.constraint.elements().contains(&line) && !keeps {
                return Err(bad_constraint(
                    "a constraint is about the length or the ends of the line",
                ));
            }
        }
        Ok(())
    }

    fn two_lines(&self, a: ElementId, b: ElementId) -> Result<(Ends, Ends), CadError> {
        if a == b {
            return Err(bad_element("two different lines are needed"));
        }
        Ok((self.line_points(a)?, self.line_points(b)?))
    }

    /// Replaces the end `old` of `line` by a new point at `at`, which is put on the other line
    /// with a point-on-object constraint so it follows it. The old point is removed when nothing
    /// else uses it.
    fn move_end_to(
        &mut self,
        line: ElementId,
        old: ElementId,
        at: (Length, Length),
        onto: ElementId,
    ) -> Result<ElementId, CadError> {
        let new = self.add_point_before(at, line)?;
        if let Some(Entry {
            element: Element::Line { a, b },
            ..
        }) = self.elements.iter_mut().find(|e| e.id == line)
        {
            if *a == old {
                *a = new;
            } else {
                *b = new;
            }
        }
        if !self.point_in_use(old, line) {
            self.elements.retain(|e| e.id != old);
        }
        self.add_constraint(Constraint::PointOnObject {
            point: new,
            object: onto,
        })?;
        Ok(new)
    }

    // ---------------------------------------------------------------------------------------
    // Chamfer
    // ---------------------------------------------------------------------------------------

    /// Cuts the corner where two lines meet with a straight line: both lines are shortened by
    /// `distance` and a new line joins the two new ends. The same conditions as for rounding a
    /// corner hold: the lines share exactly one point, which belongs to nothing else and has no
    /// constraint; a line with a constraint other than horizontal, vertical, parallel,
    /// perpendicular or collinear is refused; two lines on one line have no corner; and a
    /// millimetre of each line is always left. Returns the new line; the sketch is left as it
    /// was when the corner is refused.
    pub fn chamfer_corner(
        &mut self,
        a: ElementId,
        b: ElementId,
        distance: Length,
    ) -> Result<ElementId, CadError> {
        let saved = self.clone();
        let result = self.chamfer_inner(a, b, distance);
        if result.is_err() {
            *self = saved;
        }
        result
    }

    fn chamfer_inner(
        &mut self,
        a: ElementId,
        b: ElementId,
        distance: Length,
    ) -> Result<ElementId, CadError> {
        if distance <= Length::ZERO {
            return Err(bad_element("the distance of a chamfer is positive"));
        }
        let ((a0, a1), (b0, b1)) = self.two_lines(a, b)?;
        let shared: Vec<ElementId> = [a0, a1]
            .into_iter()
            .filter(|p| *p == b0 || *p == b1)
            .collect();
        let [corner] = shared[..] else {
            return Err(bad_element(
                "the two lines must share one point, the corner",
            ));
        };
        let far_a = if a0 == corner { a1 } else { a0 };
        let far_b = if b0 == corner { b1 } else { b0 };
        if self
            .elements
            .iter()
            .any(|e| e.id != a && e.id != b && e.element.parts().contains(&corner))
        {
            return Err(bad_element("the corner point belongs to something else"));
        }
        for c in &self.constraints {
            if c.constraint.elements().contains(&corner) {
                return Err(bad_constraint("a constraint is about the corner"));
            }
        }
        self.check_free_to_change(a)?;
        self.check_free_to_change(b)?;
        let (p, pa, pb) = (self.xy(corner)?, self.xy(far_a)?, self.xy(far_b)?);
        let (da, db) = ((pa.0 - p.0, pa.1 - p.1), (pb.0 - p.0, pb.1 - p.1));
        let (la, lb) = (dot(da, da).sqrt(), dot(db, db).sqrt());
        let (u, v) = ((da.0 / la, da.1 / la), (db.0 / lb, db.1 / lb));
        if cross(u, v).abs() < 1e-9 {
            return Err(bad_element(
                "the two lines are on one line: there is no corner",
            ));
        }
        let d = distance.get() as f64;
        if d + ROOM_NM > la || d + ROOM_NM > lb {
            return Err(bad_element(
                "the lines are too short for this distance (a millimetre of each must be left)",
            ));
        }
        let t1 = place((p.0 + u.0 * d, p.1 + u.1 * d))?;
        let t2 = place((p.0 + v.0 * d, p.1 + v.1 * d))?;
        let first_line = self
            .elements
            .iter()
            .find(|e| e.id == a || e.id == b)
            .map_or(a, |e| e.id);
        let t1_id = self.add_point_before(t1, first_line)?;
        let t2_id = self.add_point_before(t2, first_line)?;
        for (line, new) in [(a, t1_id), (b, t2_id)] {
            if let Some(Entry {
                element: Element::Line { a, b },
                ..
            }) = self.elements.iter_mut().find(|e| e.id == line)
            {
                if *a == corner {
                    *a = new;
                } else {
                    *b = new;
                }
            }
        }
        self.elements.retain(|e| e.id != corner);
        self.add_line(t1_id, t2_id)
    }

    // ---------------------------------------------------------------------------------------
    // Trim and extend
    // ---------------------------------------------------------------------------------------

    /// Cuts `line` where the line `cutter` crosses it: the part on one side of the crossing is
    /// removed, and the new end of what is left is tied to the cutter with a point-on-object
    /// constraint. `drop` is the end of `line` whose side is cut off; without it the shorter
    /// side goes (the first end when both are as long). The cutter must cross `line` inside
    /// both, a millimetre away from the ends of `line`. A line with a constraint other than
    /// horizontal, vertical, parallel, perpendicular or collinear is refused. The old end is
    /// removed unless something else uses it. Returns the new end; the sketch is left as it was
    /// when the trim is refused.
    pub fn trim_line(
        &mut self,
        line: ElementId,
        cutter: ElementId,
        drop: Option<ElementId>,
    ) -> Result<ElementId, CadError> {
        let saved = self.clone();
        let result = self.trim_inner(line, cutter, drop);
        if result.is_err() {
            *self = saved;
        }
        result
    }

    fn trim_inner(
        &mut self,
        line: ElementId,
        cutter: ElementId,
        drop: Option<ElementId>,
    ) -> Result<ElementId, CadError> {
        let ((a, b), (c, d)) = self.two_lines(line, cutter)?;
        self.check_free_to_change(line)?;
        let (pa, pb, pc, pd) = (self.xy(a)?, self.xy(b)?, self.xy(c)?, self.xy(d)?);
        let (r, s) = ((pb.0 - pa.0, pb.1 - pa.1), (pd.0 - pc.0, pd.1 - pc.1));
        let (t, u) = meet(pa, r, pc, s)
            .ok_or_else(|| bad_element("the two lines are parallel: nothing is cut"))?;
        let len = dot(r, r).sqrt();
        let len_c = dot(s, s).sqrt();
        if t * len <= ROOM_NM || (1.0 - t) * len <= ROOM_NM {
            return Err(bad_element(
                "the cutter must cross the line, a millimetre away from its ends",
            ));
        }
        if u * len_c < -1.0 || (1.0 - u) * len_c < -1.0 {
            return Err(bad_element("the cutter does not reach the line"));
        }
        let end = match drop {
            Some(p) if p == a || p == b => p,
            Some(_) => return Err(bad_element("the end to cut off is not an end of the line")),
            None => {
                if t <= 0.5 {
                    a
                } else {
                    b
                }
            }
        };
        let at = place((pa.0 + r.0 * t, pa.1 + r.1 * t))?;
        self.move_end_to(line, end, at, cutter)
    }

    /// Extends `line` until it meets the line `cutter`: the end that is nearer to the meeting
    /// point is moved to it, and the new end is tied to the cutter with a point-on-object
    /// constraint. The meeting point must lie beyond one end of `line` and inside `cutter`. A
    /// line with a constraint other than horizontal, vertical, parallel, perpendicular or
    /// collinear is refused. Returns the new end; the sketch is left as it was when the
    /// extension is refused.
    pub fn extend_line(
        &mut self,
        line: ElementId,
        target: ElementId,
    ) -> Result<ElementId, CadError> {
        let saved = self.clone();
        let result = self.extend_inner(line, target);
        if result.is_err() {
            *self = saved;
        }
        result
    }

    fn extend_inner(&mut self, line: ElementId, target: ElementId) -> Result<ElementId, CadError> {
        let ((a, b), (c, d)) = self.two_lines(line, target)?;
        self.check_free_to_change(line)?;
        let (pa, pb, pc, pd) = (self.xy(a)?, self.xy(b)?, self.xy(c)?, self.xy(d)?);
        let (r, s) = ((pb.0 - pa.0, pb.1 - pa.1), (pd.0 - pc.0, pd.1 - pc.1));
        let (t, u) = meet(pa, r, pc, s)
            .ok_or_else(|| bad_element("the two lines are parallel: they never meet"))?;
        let len = dot(r, r).sqrt();
        let len_c = dot(s, s).sqrt();
        if u * len_c < -1.0 || (1.0 - u) * len_c < -1.0 {
            return Err(bad_element(
                "the other line does not reach the meeting point",
            ));
        }
        if t * len >= -1.0 && (1.0 - t) * len >= -1.0 {
            return Err(bad_element(
                "the lines already meet: the line is not short of the meeting point",
            ));
        }
        let end = if t < 0.0 { a } else { b };
        let at = place((pa.0 + r.0 * t, pa.1 + r.1 * t))?;
        self.move_end_to(line, end, at, target)
    }

    // ---------------------------------------------------------------------------------------
    // Patterns
    // ---------------------------------------------------------------------------------------

    fn check_items(&self, items: &[ElementId]) -> Result<(), CadError> {
        if items.is_empty() {
            return Err(bad_element("there is nothing to repeat"));
        }
        for (i, id) in items.iter().enumerate() {
            if self.element(*id).is_none() {
                return Err(CadError::UnknownName(id.to_string()));
            }
            if items[..i].contains(id) {
                return Err(bad_element(format!("{id} is named twice")));
            }
        }
        Ok(())
    }

    /// Makes one copy of `items` with every point moved by `to`: a point becomes a point, a line
    /// a line between the copies of its ends, a circle a circle of the same radius (with an
    /// equal-radius constraint) and an arc an arc between the copies of its points. A point
    /// that does not move is its own copy, so the copy is joined to the original there, and an
    /// element that would lie on top of itself has no copy. Returns the new items.
    fn copy_items(
        &mut self,
        items: &[ElementId],
        to: &dyn Fn(Pt) -> Result<(Length, Length), CadError>,
    ) -> Result<Vec<ElementId>, CadError> {
        let mut needed: Vec<ElementId> = Vec::new();
        for id in items {
            let parts = match self.element(*id) {
                Some(Element::Point { .. }) => vec![*id],
                Some(e) => e.parts(),
                None => Vec::new(),
            };
            for part in parts {
                if !needed.contains(&part) {
                    needed.push(part);
                }
            }
        }
        let mut image: BTreeMap<ElementId, ElementId> = BTreeMap::new();
        for id in needed {
            let here = self.point(id)?;
            let there = to(self.xy(id)?)?;
            if there == here {
                image.insert(id, id);
            } else {
                let new = self.add_point(there.0, there.1)?;
                image.insert(id, new);
            }
        }
        let mut made = Vec::new();
        for id in items {
            let Some(entry) = self.elements.iter().find(|e| e.id == *id).cloned() else {
                continue;
            };
            let new = match entry.element {
                Element::Point { .. } => (image[id] != *id).then_some(image[id]),
                Element::Line { a, b } => {
                    let (ia, ib) = (image[&a], image[&b]);
                    if (ia, ib) == (a, b) || (ia, ib) == (b, a) {
                        None
                    } else {
                        Some(self.add_line(ia, ib)?)
                    }
                }
                Element::Circle { center, radius } => {
                    if image[&center] == center {
                        None
                    } else {
                        let new = self.add_circle(image[&center], radius)?;
                        self.add_constraint(Constraint::Equal(*id, new))?;
                        Some(new)
                    }
                }
                Element::Arc { center, start, end } => {
                    let (ic, is, ie) = (image[&center], image[&start], image[&end]);
                    if (ic, is, ie) == (center, start, end) {
                        None
                    } else {
                        Some(self.add_arc(ic, is, ie)?)
                    }
                }
            };
            if let Some(new) = new {
                if entry.construction {
                    self.set_construction(new, true)?;
                }
                made.push(new);
            }
        }
        Ok(made)
    }

    /// Repeats `items` along a straight line: `count` positions in all (the original is the
    /// first), each one `(dx, dy)` further than the one before. The copies are free geometry
    /// (a circle keeps an equal radius with its original), so the person ties them as wanted.
    /// From 2 to [`MAX_PATTERN_COUNT`] positions and a step that is not zero. Returns the new
    /// items, position by position; the sketch is left as it was when anything is refused.
    pub fn linear_pattern(
        &mut self,
        items: &[ElementId],
        dx: Length,
        dy: Length,
        count: u32,
    ) -> Result<Vec<ElementId>, CadError> {
        let saved = self.clone();
        let result = self.linear_inner(items, dx, dy, count);
        if result.is_err() {
            *self = saved;
        }
        result
    }

    fn linear_inner(
        &mut self,
        items: &[ElementId],
        dx: Length,
        dy: Length,
        count: u32,
    ) -> Result<Vec<ElementId>, CadError> {
        self.check_items(items)?;
        if !(2..=MAX_PATTERN_COUNT).contains(&count) {
            return Err(bad_element(format!(
                "a pattern has from 2 to {MAX_PATTERN_COUNT} positions"
            )));
        }
        if dx == Length::ZERO && dy == Length::ZERO {
            return Err(bad_element("the step of a pattern is not zero"));
        }
        let mut made = Vec::new();
        for k in 1..i64::from(count) {
            let (sx, sy) = (
                dx.get()
                    .checked_mul(k)
                    .ok_or(CadError::Overflow("sketch coordinates"))?,
                dy.get()
                    .checked_mul(k)
                    .ok_or(CadError::Overflow("sketch coordinates"))?,
            );
            let to = move |p: Pt| place((p.0 + sx as f64, p.1 + sy as f64));
            made.extend(self.copy_items(items, &to)?);
        }
        Ok(made)
    }

    /// Repeats `items` round the point `center`: `count` positions in all (the original is the
    /// first), each one turned `step` further than the one before (counter-clockwise for a
    /// positive step), with the sine and cosine of the units module so the same sketch gives the
    /// same positions on every system. From 2 to [`MAX_PATTERN_COUNT`] positions and a step that
    /// is not a whole number of turns. Returns the new items, position by position; the sketch
    /// is left as it was when anything is refused.
    pub fn circular_pattern(
        &mut self,
        items: &[ElementId],
        center: ElementId,
        count: u32,
        step: Angle,
    ) -> Result<Vec<ElementId>, CadError> {
        let saved = self.clone();
        let result = self.circular_inner(items, center, count, step);
        if result.is_err() {
            *self = saved;
        }
        result
    }

    fn circular_inner(
        &mut self,
        items: &[ElementId],
        center: ElementId,
        count: u32,
        step: Angle,
    ) -> Result<Vec<ElementId>, CadError> {
        self.check_items(items)?;
        self.need_point(center)?;
        if !(2..=MAX_PATTERN_COUNT).contains(&count) {
            return Err(bad_element(format!(
                "a pattern has from 2 to {MAX_PATTERN_COUNT} positions"
            )));
        }
        if step.normalized() == Angle::ZERO {
            return Err(bad_element("the step of a pattern is not a whole turn"));
        }
        let c = self.point(center)?;
        let mut made = Vec::new();
        for k in 1..i128::from(count) {
            let turn = i64::try_from(i128::from(step.get()) * k)
                .map_err(|_| CadError::Overflow("angles"))?;
            let angle = Angle::new(turn)?;
            let (sin, cos) = (i128::from(angle.sin()), i128::from(angle.cos()));
            let to = move |p: Pt| -> Result<(Length, Length), CadError> {
                let (x, y) = (
                    p.0 as i128 - i128::from(c.0.get()),
                    p.1 as i128 - i128::from(c.1.get()),
                );
                let rx = div_round_half_away(x * cos - y * sin, 1_000_000_000)?;
                let ry = div_round_half_away(x * sin + y * cos, 1_000_000_000)?;
                let (ox, oy) = (rx + i128::from(c.0.get()), ry + i128::from(c.1.get()));
                Ok((
                    Length::new(
                        i64::try_from(ox).map_err(|_| CadError::Overflow("sketch coordinates"))?,
                    )?,
                    Length::new(
                        i64::try_from(oy).map_err(|_| CadError::Overflow("sketch coordinates"))?,
                    )?,
                ))
            };
            made.extend(self.copy_items(items, &to)?);
        }
        Ok(made)
    }

    // ---------------------------------------------------------------------------------------
    // Offset
    // ---------------------------------------------------------------------------------------

    /// Draws a parallel copy of an outline at `distance` (a positive distance goes outwards, a
    /// negative one inwards). `items` are either circles (each gets a circle about the same
    /// centre, with the radius changed by the distance, which must stay positive) or the lines
    /// of one closed outline of straight lines (each point shared by exactly two of them), whose
    /// corners are mitred: the new corner is where the two offset sides meet. A corner whose mitre
    /// would be more than six times as long as the distance, a distance that makes
    /// a side turn round or the outline cross itself is refused, and so is an arc. Returns the
    /// new lines (in the order of the outline, starting from the first given) or the new
    /// circles; the sketch is left as it was when the offset is refused.
    pub fn offset_outline(
        &mut self,
        items: &[ElementId],
        distance: Length,
    ) -> Result<Vec<ElementId>, CadError> {
        let saved = self.clone();
        let result = self.offset_inner(items, distance);
        if result.is_err() {
            *self = saved;
        }
        result
    }

    fn offset_inner(
        &mut self,
        items: &[ElementId],
        distance: Length,
    ) -> Result<Vec<ElementId>, CadError> {
        self.check_items(items)?;
        if distance == Length::ZERO {
            return Err(bad_element("the distance of an offset is not zero"));
        }
        let kinds: Vec<Kind> = items
            .iter()
            .filter_map(|id| self.element(*id).map(Element::kind))
            .collect();
        if kinds.iter().all(|k| *k == Kind::Circle) {
            return self.offset_circles(items, distance);
        }
        if kinds.iter().all(|k| *k == Kind::Line) {
            return self.offset_lines(items, distance);
        }
        Err(bad_element(
            "an offset takes circles or the lines of one closed outline (an arc is not offset)",
        ))
    }

    fn offset_circles(
        &mut self,
        items: &[ElementId],
        distance: Length,
    ) -> Result<Vec<ElementId>, CadError> {
        let mut made = Vec::new();
        for id in items {
            let Some(Element::Circle { center, radius }) = self.element(*id).copied() else {
                continue;
            };
            let new = radius
                .get()
                .checked_add(distance.get())
                .ok_or(CadError::Overflow("sketch coordinates"))?;
            if new <= 0 {
                return Err(bad_element("the offset circle would have no radius"));
            }
            let circle = self.add_circle(center, Length::new(new)?)?;
            let construction = self.elements.iter().any(|e| e.id == *id && e.construction);
            if construction {
                self.set_construction(circle, true)?;
            }
            made.push(circle);
        }
        Ok(made)
    }

    /// The points of the closed outline made of `items`, in order, and the line between each
    /// point and the next.
    fn outline_of(
        &self,
        items: &[ElementId],
    ) -> Result<(Vec<ElementId>, Vec<ElementId>), CadError> {
        if items.len() < 3 {
            return Err(bad_element("an outline has at least three lines"));
        }
        let mut ends: BTreeMap<ElementId, Vec<ElementId>> = BTreeMap::new();
        for line in items {
            let (a, b) = self.line_points(*line)?;
            ends.entry(a).or_default().push(*line);
            ends.entry(b).or_default().push(*line);
        }
        if ends.values().any(|lines| lines.len() != 2) {
            return Err(bad_element(
                "the lines are not one closed outline: every point must be shared by two lines",
            ));
        }
        let first = items[0];
        let (start, _) = self.line_points(first)?;
        let (mut points, mut lines) = (vec![start], vec![first]);
        let mut at = self.line_points(first)?.1;
        let mut came = first;
        while at != start {
            points.push(at);
            let next = ends[&at]
                .iter()
                .copied()
                .find(|l| *l != came)
                .ok_or_else(|| bad_element("the lines are not one closed outline"))?;
            let (x, y) = self.line_points(next)?;
            lines.push(next);
            at = if x == at { y } else { x };
            came = next;
            if lines.len() > items.len() {
                return Err(bad_element("the lines are not one closed outline"));
            }
        }
        if lines.len() != items.len() {
            return Err(bad_element(
                "the lines are not one closed outline: some are left over",
            ));
        }
        Ok((points, lines))
    }

    fn offset_lines(
        &mut self,
        items: &[ElementId],
        distance: Length,
    ) -> Result<Vec<ElementId>, CadError> {
        let (points, lines) = self.outline_of(items)?;
        let n = points.len();
        let pts: Vec<Pt> = points
            .iter()
            .map(|p| self.xy(*p))
            .collect::<Result<_, _>>()?;
        let area: f64 = (0..n).map(|i| cross(pts[i], pts[(i + 1) % n])).sum::<f64>() / 2.0;
        if area == 0.0 {
            return Err(bad_element("the outline has no area"));
        }
        // the outward normal of each side, from the point to the next one
        let sign = if area > 0.0 { 1.0 } else { -1.0 };
        let normals: Vec<Pt> = (0..n)
            .map(|i| {
                let (p, q) = (pts[i], pts[(i + 1) % n]);
                let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                let len = dx.hypot(dy);
                (sign * dy / len, -sign * dx / len)
            })
            .collect();
        let d = distance.get() as f64;
        let mut moved: Vec<Pt> = Vec::with_capacity(n);
        for i in 0..n {
            let (before, after) = (normals[(i + n - 1) % n], normals[i]);
            let k = 1.0 + dot(before, after);
            if k < MITRE_LIMIT {
                return Err(bad_element(
                    "a corner of the outline is too sharp to be offset",
                ));
            }
            moved.push((
                pts[i].0 + d * (before.0 + after.0) / k,
                pts[i].1 + d * (before.1 + after.1) / k,
            ));
        }
        for i in 0..n {
            let j = (i + 1) % n;
            let old = (pts[j].0 - pts[i].0, pts[j].1 - pts[i].1);
            let new = (moved[j].0 - moved[i].0, moved[j].1 - moved[i].1);
            if dot(old, new) <= 0.0 {
                return Err(bad_element(
                    "the distance is too large: a side of the outline would turn round",
                ));
            }
        }
        for i in 0..n {
            for j in (i + 2)..n {
                if (j + 1) % n == i {
                    continue;
                }
                if segments_cross(moved[i], moved[(i + 1) % n], moved[j], moved[(j + 1) % n]) {
                    return Err(bad_element("the offset outline would cross itself"));
                }
            }
        }
        let mut ids = Vec::with_capacity(n);
        for p in &moved {
            let (x, y) = place(*p)?;
            ids.push(self.add_point(x, y)?);
        }
        let mut made = Vec::with_capacity(n);
        for i in 0..n {
            made.push(self.add_line(ids[i], ids[(i + 1) % n])?);
        }
        let construction = self
            .elements
            .iter()
            .any(|e| e.id == lines[0] && e.construction);
        if construction {
            for line in &made {
                self.set_construction(*line, true)?;
            }
        }
        Ok(made)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::Face;
    use crate::sketch::settings::SketchSettings;
    use crate::sketch::SketchPlane;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn nm(v: i64) -> Length {
        Length::new(v).unwrap()
    }

    fn sketch() -> Sketch {
        Sketch::new("s1", SketchPlane::on_face(Face::Top)).unwrap()
    }

    fn at(s: &Sketch, id: ElementId) -> (i64, i64) {
        let (x, y) = s.point(id).unwrap();
        (x.get(), y.get())
    }

    fn ends(s: &Sketch, line: ElementId) -> ((i64, i64), (i64, i64)) {
        let (a, b) = s.line_points(line).unwrap();
        (at(s, a), at(s, b))
    }

    fn line_count(s: &Sketch) -> usize {
        s.elements()
            .iter()
            .filter(|e| e.element.kind() == Kind::Line)
            .count()
    }

    fn point_count(s: &Sketch) -> usize {
        s.elements()
            .iter()
            .filter(|e| e.element.kind() == Kind::Point)
            .count()
    }

    /// A line from (x0, y0) to (x1, y1) in millimetres.
    fn segment(s: &mut Sketch, x0: i64, y0: i64, x1: i64, y1: i64) -> ElementId {
        let a = s.add_point(mm(x0), mm(y0)).unwrap();
        let b = s.add_point(mm(x1), mm(y1)).unwrap();
        s.add_line(a, b).unwrap()
    }

    /// A closed outline of lines through the points, in millimetres.
    fn outline(s: &mut Sketch, points: &[(i64, i64)]) -> Vec<ElementId> {
        let list: Vec<(Length, Length)> = points.iter().map(|p| (mm(p.0), mm(p.1))).collect();
        s.add_polyline(&list, true).unwrap()
    }

    fn length_dimension(s: &mut Sketch, line: ElementId, value: Length) {
        s.add_constraint(Constraint::LineLength {
            line,
            dim: crate::sketch::Dim::fixed(value),
        })
        .unwrap();
    }

    // ----- chamfer -------------------------------------------------------------------------

    #[test]
    fn a_chamfer_shortens_both_lines_and_joins_the_new_ends_with_a_line() {
        let mut s = sketch();
        let r = s.add_rectangle((mm(0), mm(0)), (mm(40), mm(25))).unwrap();
        let line = s.chamfer_corner(r.lines[0], r.lines[1], mm(5)).unwrap();
        assert_eq!(ends(&s, r.lines[0]), ((0, 0), (35_000_000, 0)));
        assert_eq!(
            ends(&s, r.lines[1]),
            ((40_000_000, 5_000_000), (40_000_000, 25_000_000))
        );
        assert_eq!(ends(&s, line), ((35_000_000, 0), (40_000_000, 5_000_000)));
        // the corner point is gone, and the sketch is whole and still solves
        assert_eq!(point_count(&s), 5);
        assert_eq!(line_count(&s), 5);
        s.validate().unwrap();
        assert!(s.solve(&SketchSettings::defaults()).is_ok());
        assert_eq!(ends(&s, line), ((35_000_000, 0), (40_000_000, 5_000_000)));
    }

    #[test]
    fn a_chamfer_works_on_a_corner_that_is_not_a_right_angle() {
        let mut s = sketch();
        let a = segment(&mut s, 0, 0, 30, 0);
        let (_, corner) = s.line_points(a).unwrap();
        let tip = s.add_point(mm(15), mm(20)).unwrap();
        let second = s.add_line(corner, tip).unwrap();
        let chamfer = s.chamfer_corner(a, second, mm(4)).unwrap();
        let (p, q) = ends(&s, chamfer);
        // both cut ends are 4 mm from the corner, on their lines
        assert_eq!(p, (26_000_000, 0));
        let dq = ((q.0 as f64 - 30_000_000.0).powi(2) + (q.1 as f64).powi(2)).sqrt();
        assert!((dq - 4_000_000.0).abs() <= 1.0, "{dq}");
        s.validate().unwrap();
    }

    #[test]
    fn a_chamfer_that_is_refused_leaves_the_sketch_as_it_was() {
        let mut s = sketch();
        let r = s.add_rectangle((mm(0), mm(0)), (mm(40), mm(25))).unwrap();
        let before = s.clone();
        // too large, zero, negative
        for d in [mm(40), mm(25), nm(0), mm(-3)] {
            assert!(s.chamfer_corner(r.lines[0], r.lines[1], d).is_err());
            assert_eq!(s, before);
        }
        // the same line twice, two lines that do not meet, an element that is not a line
        assert!(s.chamfer_corner(r.lines[0], r.lines[0], mm(2)).is_err());
        assert!(s.chamfer_corner(r.lines[0], r.lines[2], mm(2)).is_err());
        assert!(s.chamfer_corner(r.lines[0], r.points[0], mm(2)).is_err());
        assert_eq!(s, before);
        // a length dimension on a side
        length_dimension(&mut s, r.lines[1], mm(25));
        let with_length = s.clone();
        assert!(s.chamfer_corner(r.lines[0], r.lines[1], mm(2)).is_err());
        assert_eq!(s, with_length);
    }

    #[test]
    fn two_lines_on_one_line_have_no_corner_to_chamfer() {
        let mut s = sketch();
        let a = segment(&mut s, 0, 0, 10, 0);
        let (_, a1) = s.line_points(a).unwrap();
        let c = s.add_point(mm(20), mm(0)).unwrap();
        let b = s.add_line(a1, c).unwrap();
        let before = s.clone();
        assert!(s.chamfer_corner(a, b, mm(2)).is_err());
        assert_eq!(s, before);
    }

    // ----- trim ----------------------------------------------------------------------------

    #[test]
    fn a_trim_cuts_off_the_shorter_side_and_ties_the_new_end_to_the_cutter() {
        let mut s = sketch();
        let line = segment(&mut s, 0, 0, 40, 0);
        let cutter = segment(&mut s, 10, -5, 10, 5);
        let end = s.trim_line(line, cutter, None).unwrap();
        assert_eq!(ends(&s, line), ((10_000_000, 0), (40_000_000, 0)));
        assert_eq!(at(&s, end), (10_000_000, 0));
        // the old end was used by nothing else and is gone
        assert_eq!(point_count(&s), 4);
        assert!(s.constraints().iter().any(|c| matches!(
            c.constraint,
            Constraint::PointOnObject { point, object } if point == end && object == cutter
        )));
        s.validate().unwrap();
        assert!(s.solve(&SketchSettings::defaults()).is_ok());
    }

    #[test]
    fn a_trim_can_cut_off_the_end_that_is_named() {
        let mut s = sketch();
        let line = segment(&mut s, 0, 0, 40, 0);
        let cutter = segment(&mut s, 10, -5, 10, 5);
        let (_, b) = s.line_points(line).unwrap();
        s.trim_line(line, cutter, Some(b)).unwrap();
        assert_eq!(ends(&s, line), ((0, 0), (10_000_000, 0)));
    }

    #[test]
    fn a_trim_keeps_an_end_that_something_else_uses() {
        let mut s = sketch();
        let r = s.add_rectangle((mm(0), mm(0)), (mm(40), mm(25))).unwrap();
        let cutter = segment(&mut s, 30, -5, 30, 5);
        // the bottom side; its second end is a corner of the rectangle (two lines), so it stays
        s.trim_line(r.lines[0], cutter, Some(r.points[1])).unwrap();
        assert_eq!(ends(&s, r.lines[0]), ((0, 0), (30_000_000, 0)));
        assert!(s.element(r.points[1]).is_some());
        s.validate().unwrap();
    }

    #[test]
    fn a_trim_that_is_refused_leaves_the_sketch_as_it_was() {
        let mut s = sketch();
        let line = segment(&mut s, 0, 0, 40, 0);
        let far = segment(&mut s, 10, 2, 10, 5);
        let parallel = segment(&mut s, 0, 3, 40, 3);
        let near_end = segment(&mut s, 0, -5, 0, 5);
        let inside = segment(&mut s, 20, -5, 20, 5);
        let before = s.clone();
        // the cutter does not reach the line, is parallel, crosses at the very end
        assert!(s.trim_line(line, far, None).is_err());
        assert!(s.trim_line(line, parallel, None).is_err());
        assert!(s.trim_line(line, near_end, None).is_err());
        // the line is the cutter; the end named is not an end of the line
        assert!(s.trim_line(line, line, None).is_err());
        let stranger = s.line_points(inside).unwrap().0;
        assert!(s.trim_line(line, inside, Some(stranger)).is_err());
        assert_eq!(s, before);
        // a length dimension on the line
        length_dimension(&mut s, line, mm(40));
        let with_length = s.clone();
        assert!(s.trim_line(line, inside, None).is_err());
        assert_eq!(s, with_length);
    }

    // ----- extend --------------------------------------------------------------------------

    #[test]
    fn an_extension_moves_the_nearer_end_to_the_meeting_point() {
        let mut s = sketch();
        let line = segment(&mut s, 0, 0, 10, 0);
        let target = segment(&mut s, 30, -5, 30, 5);
        let end = s.extend_line(line, target).unwrap();
        assert_eq!(ends(&s, line), ((0, 0), (30_000_000, 0)));
        assert_eq!(at(&s, end), (30_000_000, 0));
        assert!(s.constraints().iter().any(|c| matches!(
            c.constraint,
            Constraint::PointOnObject { point, object } if point == end && object == target
        )));
        s.validate().unwrap();
        assert!(s.solve(&SketchSettings::defaults()).is_ok());
        // the first end, towards a line behind it
        let mut s = sketch();
        let line = segment(&mut s, 10, 0, 20, 0);
        let target = segment(&mut s, 0, -5, 0, 5);
        s.extend_line(line, target).unwrap();
        assert_eq!(ends(&s, line), ((0, 0), (20_000_000, 0)));
    }

    #[test]
    fn an_extension_that_is_refused_leaves_the_sketch_as_it_was() {
        let mut s = sketch();
        let line = segment(&mut s, 0, 0, 10, 0);
        let reaching = segment(&mut s, 5, -5, 5, 5);
        let short = segment(&mut s, 30, 2, 30, 5);
        let parallel = segment(&mut s, 0, 3, 40, 3);
        let before = s.clone();
        // already meets, the other line stops short, parallel, itself
        assert!(s.extend_line(line, reaching).is_err());
        assert!(s.extend_line(line, short).is_err());
        assert!(s.extend_line(line, parallel).is_err());
        assert!(s.extend_line(line, line).is_err());
        assert_eq!(s, before);
    }

    // ----- patterns ------------------------------------------------------------------------

    #[test]
    fn a_linear_pattern_makes_a_copy_at_each_step() {
        let mut s = sketch();
        let line = segment(&mut s, 0, 0, 5, 0);
        let hole_centre = s.add_point(mm(2), mm(3)).unwrap();
        let circle = s.add_circle(hole_centre, mm(1)).unwrap();
        let made = s.linear_pattern(&[line, circle], mm(10), mm(2), 4).unwrap();
        // 3 copies of 2 items
        assert_eq!(made.len(), 6);
        assert_eq!(
            ends(&s, made[0]),
            ((10_000_000, 2_000_000), (15_000_000, 2_000_000))
        );
        assert_eq!(
            ends(&s, made[2]),
            ((20_000_000, 4_000_000), (25_000_000, 4_000_000))
        );
        assert_eq!(
            ends(&s, made[4]),
            ((30_000_000, 6_000_000), (35_000_000, 6_000_000))
        );
        // each new circle has the radius of its original and an equal constraint with it
        let equals = s
            .constraints()
            .iter()
            .filter(|c| matches!(c.constraint, Constraint::Equal(a, _) if a == circle))
            .count();
        assert_eq!(equals, 3);
        s.validate().unwrap();
    }

    #[test]
    fn the_copies_of_a_pattern_share_the_points_that_the_originals_share() {
        let mut s = sketch();
        let lines = s
            .add_polyline(&[(mm(0), mm(0)), (mm(5), mm(0)), (mm(5), mm(5))], false)
            .unwrap();
        let before = point_count(&s);
        let made = s.linear_pattern(&lines, mm(0), mm(10), 2).unwrap();
        assert_eq!(made.len(), 2);
        // three points were copied once each, not four
        assert_eq!(point_count(&s), before + 3);
        let (a0, a1) = s.line_points(made[0]).unwrap();
        let (b0, _) = s.line_points(made[1]).unwrap();
        assert_ne!(a0, a1);
        assert_eq!(a1, b0);
    }

    #[test]
    fn a_pattern_that_is_refused_leaves_the_sketch_as_it_was() {
        let mut s = sketch();
        let line = segment(&mut s, 0, 0, 5, 0);
        let before = s.clone();
        assert!(s.linear_pattern(&[line], mm(10), mm(0), 1).is_err());
        assert!(s.linear_pattern(&[line], mm(10), mm(0), 0).is_err());
        assert!(s
            .linear_pattern(&[line], mm(10), mm(0), MAX_PATTERN_COUNT + 1)
            .is_err());
        assert!(s.linear_pattern(&[line], mm(0), mm(0), 3).is_err());
        assert!(s.linear_pattern(&[], mm(1), mm(0), 3).is_err());
        assert!(s.linear_pattern(&[line, line], mm(1), mm(0), 3).is_err());
        assert!(s
            .linear_pattern(&[ElementId(999)], mm(1), mm(0), 3)
            .is_err());
        // a step so large that the third position leaves the range
        let huge = nm(crate::LENGTH_LIMIT_NM - 1);
        assert!(s.linear_pattern(&[line], huge, mm(0), 3).is_err());
        assert_eq!(s, before);
        // the largest number of positions is allowed
        let mut t = sketch();
        let one = t.add_point(mm(0), mm(0)).unwrap();
        let made = t
            .linear_pattern(&[one], mm(1), mm(0), MAX_PATTERN_COUNT)
            .unwrap();
        assert_eq!(made.len(), MAX_PATTERN_COUNT as usize - 1);
    }

    #[test]
    fn a_circular_pattern_turns_the_copies_about_the_centre() {
        let mut s = sketch();
        let centre = s.add_point(mm(0), mm(0)).unwrap();
        let p = s.add_point(mm(10), mm(0)).unwrap();
        let made = s
            .circular_pattern(&[p], centre, 4, Angle::from_deg(90).unwrap())
            .unwrap();
        let spots: Vec<(i64, i64)> = made.iter().map(|id| at(&s, *id)).collect();
        assert_eq!(
            spots,
            vec![(0, 10_000_000), (-10_000_000, 0), (0, -10_000_000)]
        );
        // a third of a turn, with the integer sine and cosine
        let mut s = sketch();
        let centre = s.add_point(mm(0), mm(0)).unwrap();
        let p = s.add_point(mm(10), mm(0)).unwrap();
        let made = s
            .circular_pattern(&[p], centre, 3, Angle::from_deg(120).unwrap())
            .unwrap();
        assert_eq!(at(&s, made[0]), (-5_000_000, 8_660_254));
        assert_eq!(at(&s, made[1]), (-5_000_000, -8_660_254));
    }

    #[test]
    fn a_circular_pattern_about_a_point_of_the_item_joins_the_copies_there() {
        let mut s = sketch();
        let line = segment(&mut s, 0, 0, 10, 0);
        let (centre, _) = s.line_points(line).unwrap();
        let before = point_count(&s);
        let made = s
            .circular_pattern(&[line], centre, 4, Angle::from_deg(90).unwrap())
            .unwrap();
        assert_eq!(made.len(), 3);
        // the centre is its own copy: only the far ends were added
        assert_eq!(point_count(&s), before + 3);
        for l in &made {
            let (a, _) = s.line_points(*l).unwrap();
            assert_eq!(a, centre);
        }
        s.validate().unwrap();
    }

    #[test]
    fn a_circular_pattern_is_the_same_every_time_and_refuses_what_makes_no_sense() {
        let build = || {
            let mut s = sketch();
            let centre = s.add_point(mm(3), mm(-4)).unwrap();
            let p = s.add_point(mm(17), mm(9)).unwrap();
            let q = s.add_point(mm(-6), mm(11)).unwrap();
            let l = s.add_line(p, q).unwrap();
            s.circular_pattern(&[l], centre, 7, Angle::new(51_428_571_429).unwrap())
                .unwrap();
            s
        };
        assert_eq!(build(), build());
        let mut s = sketch();
        let centre = s.add_point(mm(0), mm(0)).unwrap();
        let p = s.add_point(mm(10), mm(0)).unwrap();
        let line = segment(&mut s, 1, 1, 2, 2);
        let before = s.clone();
        let turn = Angle::from_deg(90).unwrap();
        assert!(s.circular_pattern(&[p], centre, 1, turn).is_err());
        assert!(s
            .circular_pattern(&[p], centre, MAX_PATTERN_COUNT + 1, turn)
            .is_err());
        assert!(s.circular_pattern(&[p], centre, 3, Angle::ZERO).is_err());
        assert!(s
            .circular_pattern(&[p], centre, 3, Angle::from_deg(360).unwrap())
            .is_err());
        assert!(s.circular_pattern(&[p], line, 3, turn).is_err());
        assert!(s.circular_pattern(&[], centre, 3, turn).is_err());
        assert_eq!(s, before);
    }

    // ----- offset --------------------------------------------------------------------------

    fn corners(s: &Sketch, lines: &[ElementId]) -> Vec<(i64, i64)> {
        lines.iter().map(|l| ends(s, *l).0).collect()
    }

    #[test]
    fn an_offset_of_a_rectangle_moves_every_side_out_or_in() {
        let mut s = sketch();
        let r = s.add_rectangle((mm(0), mm(0)), (mm(40), mm(25))).unwrap();
        let out = s.offset_outline(&r.lines, mm(3)).unwrap();
        assert_eq!(
            corners(&s, &out),
            vec![
                (-3_000_000, -3_000_000),
                (43_000_000, -3_000_000),
                (43_000_000, 28_000_000),
                (-3_000_000, 28_000_000)
            ]
        );
        let inside = s.offset_outline(&r.lines, mm(-3)).unwrap();
        assert_eq!(
            corners(&s, &inside),
            vec![
                (3_000_000, 3_000_000),
                (37_000_000, 3_000_000),
                (37_000_000, 22_000_000),
                (3_000_000, 22_000_000)
            ]
        );
        s.validate().unwrap();
    }

    #[test]
    fn an_offset_does_not_depend_on_the_way_round_the_outline_is_drawn() {
        let mut s = sketch();
        let clockwise = outline(&mut s, &[(0, 0), (0, 25), (40, 25), (40, 0)]);
        let out = s.offset_outline(&clockwise, mm(3)).unwrap();
        let mut c = corners(&s, &out);
        c.sort_unstable();
        assert_eq!(
            c,
            vec![
                (-3_000_000, -3_000_000),
                (-3_000_000, 28_000_000),
                (43_000_000, -3_000_000),
                (43_000_000, 28_000_000)
            ]
        );
    }

    #[test]
    fn an_offset_of_a_shape_with_a_notch_mitres_the_inner_corner() {
        let mut s = sketch();
        let l = outline(
            &mut s,
            &[(0, 0), (40, 0), (40, 10), (10, 10), (10, 30), (0, 30)],
        );
        let out = s.offset_outline(&l, mm(2)).unwrap();
        let c = corners(&s, &out);
        assert!(c.contains(&(12_000_000, 12_000_000)));
        assert!(c.contains(&(-2_000_000, -2_000_000)));
        assert!(c.contains(&(42_000_000, -2_000_000)));
        // an arm that is narrower than twice the distance collapses
        let before = s.clone();
        assert!(s.offset_outline(&l, mm(-6)).is_err());
        assert_eq!(s, before);
    }

    #[test]
    fn an_offset_of_circles_changes_the_radius_about_the_same_centre() {
        let mut s = sketch();
        let c = s.add_point(mm(5), mm(5)).unwrap();
        let circle = s.add_circle(c, mm(10)).unwrap();
        let made = s.offset_outline(&[circle], mm(2)).unwrap();
        assert!(matches!(
            s.element(made[0]),
            Some(Element::Circle { center, radius }) if *center == c && *radius == mm(12)
        ));
        let inner = s.offset_outline(&[circle], mm(-4)).unwrap();
        assert!(matches!(
            s.element(inner[0]),
            Some(Element::Circle { radius, .. }) if *radius == mm(6)
        ));
        let before = s.clone();
        assert!(s.offset_outline(&[circle], mm(-10)).is_err());
        assert!(s.offset_outline(&[circle], mm(-11)).is_err());
        assert_eq!(s, before);
    }

    #[test]
    fn an_offset_that_is_refused_leaves_the_sketch_as_it_was() {
        let mut s = sketch();
        let r = s.add_rectangle((mm(0), mm(0)), (mm(40), mm(25))).unwrap();
        let c = s.add_point(mm(5), mm(5)).unwrap();
        let circle = s.add_circle(c, mm(2)).unwrap();
        let end = s.add_point(mm(30), mm(30)).unwrap();
        let start = s.add_point(mm(20), mm(30)).unwrap();
        let arc = s.add_arc(c, start, end).unwrap();
        let before = s.clone();
        // zero, a circle and a line together, an arc, an open chain, a line twice
        assert!(s.offset_outline(&r.lines, nm(0)).is_err());
        assert!(s.offset_outline(&[circle, r.lines[0]], mm(1)).is_err());
        assert!(s.offset_outline(&[arc], mm(1)).is_err());
        assert!(s.offset_outline(&r.lines[..3], mm(1)).is_err());
        assert!(s
            .offset_outline(&[r.lines[0], r.lines[0], r.lines[1]], mm(1))
            .is_err());
        assert!(s.offset_outline(&[], mm(1)).is_err());
        // a rectangle of 25 mm cannot go in by 13 mm
        assert!(s.offset_outline(&r.lines, mm(-13)).is_err());
        assert_eq!(s, before);
    }

    #[test]
    fn an_outline_that_is_too_sharp_is_not_offset() {
        let mut s = sketch();
        // a triangle with an angle of about 1 degree at its tip
        let t = outline(&mut s, &[(0, 0), (400, 0), (400, 7)]);
        let before = s.clone();
        assert!(s.offset_outline(&t, mm(1)).is_err());
        assert_eq!(s, before);
        // a wide triangle is fine
        let w = outline(&mut s, &[(0, 0), (40, 0), (20, 20)]);
        assert!(s.offset_outline(&w, mm(1)).is_ok());
    }

    // ----- fixed-seed properties -----------------------------------------------------------

    /// A small generator with a fixed seed (xorshift), so the cases are the same on every run.
    struct Seeded(u64);

    impl Seeded {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        /// A whole number from `low` to `high` inclusive.
        fn between(&mut self, low: i64, high: i64) -> i64 {
            low + (self.next() % ((high - low + 1) as u64)) as i64
        }
    }

    #[test]
    fn the_edits_of_a_sketch_either_give_a_valid_sketch_or_change_nothing() {
        let mut seed = Seeded(0x00C0_FFEE_1234_5678);
        let (mut made, mut refused) = (0, 0);
        for _ in 0..600 {
            let mut s = sketch();
            let (w, h) = (seed.between(3, 80), seed.between(3, 80));
            let r = s.add_rectangle((mm(0), mm(0)), (mm(w), mm(h))).unwrap();
            let extra = segment(
                &mut s,
                seed.between(-10, 90),
                seed.between(-10, 90),
                seed.between(-10, 90),
                seed.between(-10, 90),
            );
            let before = s.clone();
            let side = r.lines[seed.between(0, 3) as usize];
            let other = r.lines[seed.between(0, 3) as usize];
            let result: Result<usize, CadError> = match seed.between(0, 5) {
                0 => {
                    let d = nm(seed.between(-2, 60) * 1_000_000);
                    s.chamfer_corner(side, other, d).map(|_| 1)
                }
                1 => s.trim_line(extra, side, None).map(|_| 1),
                2 => s.extend_line(extra, side).map(|_| 1),
                3 => s
                    .linear_pattern(
                        &r.lines,
                        mm(seed.between(-30, 30)),
                        mm(seed.between(-30, 30)),
                        seed.between(0, 6) as u32,
                    )
                    .map(|m| m.len()),
                4 => s
                    .offset_outline(&r.lines, mm(seed.between(-15, 15)))
                    .map(|m| m.len()),
                _ => s
                    .circular_pattern(
                        &[extra],
                        r.points[0],
                        seed.between(0, 6) as u32,
                        Angle::from_deg(seed.between(-180, 180)).unwrap(),
                    )
                    .map(|m| m.len()),
            };
            match result {
                Ok(_) => {
                    made += 1;
                    s.validate().unwrap();
                }
                Err(_) => {
                    refused += 1;
                    assert_eq!(s, before);
                }
            }
        }
        // both outcomes were met many times, so the property is not empty
        assert!(made > 100, "{made}");
        assert!(refused > 100, "{refused}");
    }

    #[test]
    fn a_linear_pattern_puts_every_point_where_the_step_says() {
        let mut seed = Seeded(0x0BAD_5EED_0000_0042);
        for _ in 0..300 {
            let mut s = sketch();
            let (x, y) = (seed.between(-50, 50), seed.between(-50, 50));
            let p = s.add_point(mm(x), mm(y)).unwrap();
            let (dx, dy) = (seed.between(-9, 9), seed.between(-9, 9));
            let count = seed.between(2, 12) as u32;
            let result = s.linear_pattern(&[p], mm(dx), mm(dy), count);
            if dx == 0 && dy == 0 {
                assert!(result.is_err());
                continue;
            }
            let made = result.unwrap();
            assert_eq!(made.len(), count as usize - 1);
            for (k, id) in made.iter().enumerate() {
                let k = k as i64 + 1;
                assert_eq!(at(&s, *id), (mm(x + k * dx).get(), mm(y + k * dy).get()));
            }
        }
    }

    #[test]
    fn a_circular_pattern_keeps_every_point_at_its_distance_from_the_centre() {
        let mut seed = Seeded(0x5EED_CAFE_0000_0007);
        for _ in 0..300 {
            let mut s = sketch();
            let centre = s
                .add_point(mm(seed.between(-20, 20)), mm(seed.between(-20, 20)))
                .unwrap();
            let p = s
                .add_point(mm(seed.between(-40, 40)), mm(seed.between(-40, 40)))
                .unwrap();
            let (c, q) = (at(&s, centre), at(&s, p));
            let count = seed.between(2, 9) as u32;
            let step = Angle::from_deg(seed.between(1, 359)).unwrap();
            let made = s.circular_pattern(&[p], centre, count, step).unwrap();
            let radius = (((q.0 - c.0) as f64).powi(2) + ((q.1 - c.1) as f64).powi(2)).sqrt();
            for id in made {
                let r = at(&s, id);
                let d = (((r.0 - c.0) as f64).powi(2) + ((r.1 - c.1) as f64).powi(2)).sqrt();
                // rounding to the nanometre in each coordinate
                assert!((d - radius).abs() <= 2.0, "{d} {radius}");
            }
        }
    }
}
