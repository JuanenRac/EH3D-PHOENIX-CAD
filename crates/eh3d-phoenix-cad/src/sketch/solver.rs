//! The solver of the sketch.
//!
//! Every constraint gives one or two residuals, numbers that are zero when it holds (a distance
//! minus its value, the cross product of two directions that must be parallel). The solver looks
//! for the coordinates that make all of them zero by damped least squares (Levenberg and
//! Marquardt): at each step it solves `(J'J + lambda I) d = -J'r` with the Jacobian `J` made by
//! central differences, keeps the step when the sum of squares goes down, and then asks for more
//! damping when it does not.
//!
//! * the unknowns are the coordinates of the points and the radii of the circles, in
//!   millimetres; the sketch is changed only when the answer is a solution, and then the values
//!   are rounded to the nanometre;
//! * the degrees of freedom left are the unknowns minus the rank of the Jacobian at the solution;
//! * a constraint whose rows of the Jacobian are a combination of the rows of the constraints
//!   before it adds nothing: when the system is solved it is **redundant**, when it is not it is
//!   in **conflict** with them; the report names the constraint and the earlier ones it depends
//!   on, in the order of their identifiers;
//! * no function of the mathematical library is used (only sums, products, quotients and
//!   square roots, which every platform computes the same way, and the integer table of sines
//!   of [`Angle`]), so the same sketch gives the same result bit for bit.
//!
//! A tangent between a line and a circle, or between two circles, keeps the side (or the outside
//! or inside) it has when the solver starts; an angle is a direction, so an angle and the same
//! angle plus a half turn are the same constraint, and the solver keeps the one it starts near.

use std::collections::BTreeMap;

use super::settings::SketchSettings;
use super::{Constraint, ConstraintEntry, ConstraintId, Element, ElementId, Sketch};
use crate::units::Angle;
use crate::{CadError, Length};

/// What the solver found.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The constraints hold, and the sketch has been moved to the solution.
    Solved {
        /// The degrees of freedom left: zero when the sketch is fully constrained.
        dof: usize,
        /// The steps taken.
        iterations: u32,
        /// The largest residual, in millimetres.
        residual: f64,
    },
    /// Some constraints add nothing to the ones before them. The sketch is not changed.
    OverConstrained {
        /// Constraints that repeat others but do not contradict them (the system has a
        /// solution); each one is listed with the earlier ones it depends on.
        redundant: Vec<ConstraintId>,
        /// Constraints that contradict others (there is no solution); each one is listed with the
        /// earlier ones it depends on.
        conflicting: Vec<ConstraintId>,
    },
    /// The solver ran out of steps or could not get nearer. The sketch is not changed.
    NotConverged {
        /// The steps taken.
        iterations: u32,
        /// The largest residual, in millimetres.
        residual: f64,
    },
}

impl Outcome {
    /// The degrees of freedom when the sketch was solved.
    pub fn dof(&self) -> Option<usize> {
        match self {
            Outcome::Solved { dof, .. } => Some(*dof),
            _ => None,
        }
    }

    /// Whether the sketch was solved and has no degrees of freedom left.
    pub fn is_fully_constrained(&self) -> bool {
        self.dof() == Some(0)
    }

    /// The outcome as a result: the degrees of freedom, or the error that says what is wrong
    /// (`PHOENIX-CAD-033` over-constrained, `PHOENIX-CAD-034` not converged).
    pub fn into_result(self) -> Result<usize, CadError> {
        match self {
            Outcome::Solved { dof, .. } => Ok(dof),
            Outcome::OverConstrained {
                redundant,
                conflicting,
            } => {
                let list = |v: &[ConstraintId]| {
                    v.iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                let mut parts = Vec::new();
                if !conflicting.is_empty() {
                    parts.push(format!("in conflict: {}", list(&conflicting)));
                }
                if !redundant.is_empty() {
                    parts.push(format!("redundant: {}", list(&redundant)));
                }
                Err(CadError::SketchOverConstrained(parts.join("; ")))
            }
            Outcome::NotConverged {
                iterations,
                residual,
            } => Err(CadError::SketchNoConvergence(format!(
                "{iterations} steps, residual {residual:e} mm"
            ))),
        }
    }
}

const NM_PER_MM: f64 = 1_000_000.0;
const MIN_LENGTH: f64 = 1e-12;

fn mm_of(l: Length) -> f64 {
    l.get() as f64 / NM_PER_MM
}

struct Model<'a> {
    sketch: &'a Sketch,
    pidx: BTreeMap<ElementId, usize>,
    ridx: BTreeMap<ElementId, usize>,
    n: usize,
    active: Vec<&'a ConstraintEntry>,
    /// The side of a tangent, chosen when the solver starts.
    branch: Vec<f64>,
    /// The constraint (an index of `active`) that owns each row, or none for the rows the
    /// elements bring (the end of an arc is as far from the centre as its start).
    owners: Vec<Option<usize>>,
}

type Pt = (f64, f64);

impl<'a> Model<'a> {
    fn new(sketch: &'a Sketch) -> (Model<'a>, Vec<f64>) {
        let mut pidx = BTreeMap::new();
        let mut ridx = BTreeMap::new();
        let mut x0 = Vec::new();
        for entry in sketch.elements() {
            match entry.element {
                Element::Point { x, y } => {
                    pidx.insert(entry.id, x0.len());
                    x0.push(mm_of(x));
                    x0.push(mm_of(y));
                }
                Element::Circle { radius, .. } => {
                    ridx.insert(entry.id, x0.len());
                    x0.push(mm_of(radius));
                }
                Element::Line { .. } | Element::Arc { .. } => {}
            }
        }
        let active: Vec<&ConstraintEntry> =
            sketch.constraints().iter().filter(|c| c.enabled).collect();
        let mut model = Model {
            sketch,
            pidx,
            ridx,
            n: x0.len(),
            active,
            branch: Vec::new(),
            owners: Vec::new(),
        };
        // the side of every tangent, from where the sketch is now
        let mut branch = Vec::with_capacity(model.active.len());
        for c in &model.active {
            branch.push(model.branch_of(&c.constraint, &x0));
        }
        model.branch = branch;
        // who owns each row
        let mut owners = Vec::new();
        let mut scratch = Vec::new();
        model.element_rows(&x0, &mut scratch);
        owners.extend(scratch.iter().map(|_| None));
        for k in 0..model.active.len() {
            scratch.clear();
            model.constraint_rows(k, &x0, &mut scratch);
            owners.extend(scratch.iter().map(|_| Some(k)));
        }
        model.owners = owners;
        (model, x0)
    }

    fn pt(&self, x: &[f64], id: ElementId) -> Pt {
        let i = self.pidx.get(&id).copied().unwrap_or(0);
        (x[i], x[i + 1])
    }

    fn line_ends(&self, x: &[f64], id: ElementId) -> (Pt, Pt) {
        match self.sketch.element(id) {
            Some(Element::Line { a, b }) => (self.pt(x, *a), self.pt(x, *b)),
            _ => ((0.0, 0.0), (1.0, 0.0)),
        }
    }

    fn center(&self, x: &[f64], id: ElementId) -> Pt {
        match self.sketch.element(id) {
            Some(Element::Circle { center, .. } | Element::Arc { center, .. }) => {
                self.pt(x, *center)
            }
            _ => (0.0, 0.0),
        }
    }

    fn radius(&self, x: &[f64], id: ElementId) -> f64 {
        match self.sketch.element(id) {
            Some(Element::Circle { .. }) => self.ridx.get(&id).map_or(0.0, |i| x[*i]),
            Some(Element::Arc { center, start, .. }) => {
                dist(self.pt(x, *center), self.pt(x, *start))
            }
            _ => 0.0,
        }
    }

    fn is_line(&self, id: ElementId) -> bool {
        matches!(self.sketch.element(id), Some(Element::Line { .. }))
    }

    /// The side of a tangent, from the positions in `x`: `+1`/`-1` for the side of a line, `2`
    /// for two circles that touch from outside and `+1`/`-1` (the sign of the difference of the
    /// radii) when one is inside the other.
    fn branch_of(&self, c: &Constraint, x: &[f64]) -> f64 {
        let Constraint::Tangent(a, b) = c else {
            return 1.0;
        };
        if self.is_line(*a) || self.is_line(*b) {
            let (line, round) = if self.is_line(*a) { (*a, *b) } else { (*b, *a) };
            let (p, q) = self.line_ends(x, line);
            let s = side(self.center(x, round), p, q);
            if s < 0.0 {
                -1.0
            } else {
                1.0
            }
        } else {
            let d = dist(self.center(x, *a), self.center(x, *b));
            let (r1, r2) = (self.radius(x, *a), self.radius(x, *b));
            let outside = (d - (r1 + r2)).abs();
            let inside = (d - (r1 - r2).abs()).abs();
            if outside <= inside {
                2.0
            } else if r1 >= r2 {
                1.0
            } else {
                -1.0
            }
        }
    }

    fn element_rows(&self, x: &[f64], out: &mut Vec<f64>) {
        for entry in self.sketch.elements() {
            if let Element::Arc { center, start, end } = entry.element {
                let c = self.pt(x, center);
                out.push(dist(c, self.pt(x, end)) - dist(c, self.pt(x, start)));
            }
        }
    }

    fn constraint_rows(&self, k: usize, x: &[f64], out: &mut Vec<f64>) {
        let entry = self.active[k];
        match &entry.constraint {
            Constraint::Coincident(a, b) => {
                let (p, q) = (self.pt(x, *a), self.pt(x, *b));
                out.push(p.0 - q.0);
                out.push(p.1 - q.1);
            }
            Constraint::PointOnObject { point, object } => {
                let p = self.pt(x, *point);
                if self.is_line(*object) {
                    let (a, b) = self.line_ends(x, *object);
                    out.push(side(p, a, b));
                } else {
                    out.push(dist(p, self.center(x, *object)) - self.radius(x, *object));
                }
            }
            Constraint::Horizontal(l) => {
                let (a, b) = self.line_ends(x, *l);
                out.push(b.1 - a.1);
            }
            Constraint::Vertical(l) => {
                let (a, b) = self.line_ends(x, *l);
                out.push(b.0 - a.0);
            }
            Constraint::HorizontalPoints(a, b) => {
                out.push(self.pt(x, *b).1 - self.pt(x, *a).1);
            }
            Constraint::VerticalPoints(a, b) => {
                out.push(self.pt(x, *b).0 - self.pt(x, *a).0);
            }
            Constraint::Parallel(a, b) => {
                let ((p1, q1), (p2, q2)) = (self.line_ends(x, *a), self.line_ends(x, *b));
                let (d1, d2) = ((q1.0 - p1.0, q1.1 - p1.1), (q2.0 - p2.0, q2.1 - p2.1));
                out.push((d1.0 * d2.1 - d1.1 * d2.0) / norm(d2));
            }
            Constraint::Perpendicular(a, b) => {
                let ((p1, q1), (p2, q2)) = (self.line_ends(x, *a), self.line_ends(x, *b));
                let (d1, d2) = ((q1.0 - p1.0, q1.1 - p1.1), (q2.0 - p2.0, q2.1 - p2.1));
                out.push((d1.0 * d2.0 + d1.1 * d2.1) / norm(d2));
            }
            Constraint::Tangent(a, b) => {
                let branch = self.branch[k];
                if self.is_line(*a) || self.is_line(*b) {
                    let (line, round) = if self.is_line(*a) { (*a, *b) } else { (*b, *a) };
                    let (p, q) = self.line_ends(x, line);
                    out.push(side(self.center(x, round), p, q) - branch * self.radius(x, round));
                } else {
                    let d = dist(self.center(x, *a), self.center(x, *b));
                    let (r1, r2) = (self.radius(x, *a), self.radius(x, *b));
                    if branch > 1.5 {
                        out.push(d - (r1 + r2));
                    } else {
                        out.push(d - branch * (r1 - r2));
                    }
                }
            }
            Constraint::Equal(a, b) => {
                if self.is_line(*a) {
                    let ((p1, q1), (p2, q2)) = (self.line_ends(x, *a), self.line_ends(x, *b));
                    out.push(dist(p1, q1) - dist(p2, q2));
                } else {
                    out.push(self.radius(x, *a) - self.radius(x, *b));
                }
            }
            Constraint::SymmetricLine { a, b, axis } => {
                let (p, q) = (self.pt(x, *a), self.pt(x, *b));
                let (s, t) = self.line_ends(x, *axis);
                let mid = ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0);
                out.push(side(mid, s, t));
                let d = (t.0 - s.0, t.1 - s.1);
                out.push(((q.0 - p.0) * d.0 + (q.1 - p.1) * d.1) / norm(d));
            }
            Constraint::SymmetricPoint { a, b, center } => {
                let (p, q, c) = (self.pt(x, *a), self.pt(x, *b), self.pt(x, *center));
                out.push(p.0 + q.0 - 2.0 * c.0);
                out.push(p.1 + q.1 - 2.0 * c.1);
            }
            Constraint::Midpoint { point, line } => {
                let p = self.pt(x, *point);
                let (a, b) = self.line_ends(x, *line);
                out.push(p.0 - (a.0 + b.0) / 2.0);
                out.push(p.1 - (a.1 + b.1) / 2.0);
            }
            Constraint::Fixed {
                point,
                x: fx,
                y: fy,
            } => {
                let p = self.pt(x, *point);
                out.push(p.0 - mm_of(*fx));
                out.push(p.1 - mm_of(*fy));
            }
            Constraint::Distance { a, b, dim } => {
                out.push(dist(self.pt(x, *a), self.pt(x, *b)) - mm_of(dim.value));
            }
            Constraint::DistanceX { a, b, dim } => {
                out.push(self.pt(x, *b).0 - self.pt(x, *a).0 - mm_of(dim.value));
            }
            Constraint::DistanceY { a, b, dim } => {
                out.push(self.pt(x, *b).1 - self.pt(x, *a).1 - mm_of(dim.value));
            }
            Constraint::LineLength { line, dim } => {
                let (a, b) = self.line_ends(x, *line);
                out.push(dist(a, b) - mm_of(dim.value));
            }
            Constraint::Radius { round, dim } => {
                out.push(self.radius(x, *round) - mm_of(dim.value));
            }
            Constraint::Diameter { round, dim } => {
                out.push(2.0 * self.radius(x, *round) - mm_of(dim.value));
            }
            Constraint::AngleBetween { a, b, dim } => {
                let ((p1, q1), (p2, q2)) = (self.line_ends(x, *a), self.line_ends(x, *b));
                let (d1, d2) = ((q1.0 - p1.0, q1.1 - p1.1), (q2.0 - p2.0, q2.1 - p2.1));
                let cross = d1.0 * d2.1 - d1.1 * d2.0;
                let dot = d1.0 * d2.0 + d1.1 * d2.1;
                let cos = dim.value.cos() as f64 / 1e9;
                let sin = dim.value.sin() as f64 / 1e9;
                out.push((cross * cos - dot * sin) / norm(d2));
            }
        }
    }

    fn residuals(&self, x: &[f64]) -> Vec<f64> {
        let mut out = Vec::with_capacity(self.owners.len());
        self.element_rows(x, &mut out);
        for k in 0..self.active.len() {
            self.constraint_rows(k, x, &mut out);
        }
        out
    }

    /// The Jacobian by central differences: one row for each residual.
    fn jacobian(&self, x: &[f64], h: f64) -> Vec<Vec<f64>> {
        let rows = self.owners.len();
        let mut j = vec![vec![0.0f64; self.n]; rows];
        let mut probe = x.to_vec();
        for col in 0..self.n {
            let keep = probe[col];
            probe[col] = keep + h;
            let plus = self.residuals(&probe);
            probe[col] = keep - h;
            let minus = self.residuals(&probe);
            probe[col] = keep;
            for (row, (p, m)) in j.iter_mut().zip(plus.iter().zip(minus.iter())) {
                row[col] = (p - m) / (2.0 * h);
            }
        }
        j
    }
}

fn norm(d: Pt) -> f64 {
    let len: f64 = (d.0 * d.0 + d.1 * d.1).sqrt();
    len.max(MIN_LENGTH)
}

fn dist(p: Pt, q: Pt) -> f64 {
    ((p.0 - q.0) * (p.0 - q.0) + (p.1 - q.1) * (p.1 - q.1)).sqrt()
}

/// The signed distance from a point to the line through `a` and `b` (positive on the left).
fn side(p: Pt, a: Pt, b: Pt) -> f64 {
    let d = (b.0 - a.0, b.1 - a.1);
    (d.0 * (p.1 - a.1) - d.1 * (p.0 - a.0)) / norm(d)
}

fn max_abs(v: &[f64]) -> f64 {
    v.iter().fold(0.0f64, |m, e| m.max(e.abs()))
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(p, q)| p * q).sum()
}

/// Solves `(a + lambda I) d = -g` by Cholesky; `None` when the matrix is not positive.
fn solve_damped(a: &[Vec<f64>], lambda: f64, g: &[f64]) -> Option<Vec<f64>> {
    let n = g.len();
    let mut l = vec![vec![0.0f64; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let mut sum = a[i][j];
            if i == j {
                sum += lambda;
            }
            sum -= dot(&l[i][..j], &l[j][..j]);
            if i == j {
                if sum.is_nan() || sum <= 0.0 {
                    return None;
                }
                let root: f64 = sum.sqrt();
                l[i][i] = root;
            } else {
                l[i][j] = sum / l[j][j];
            }
        }
    }
    let mut y = vec![0.0f64; n];
    for i in 0..n {
        let sum = -g[i] - dot(&l[i][..i], &y[..i]);
        y[i] = sum / l[i][i];
    }
    let mut d = vec![0.0f64; n];
    for i in (0..n).rev() {
        let below: f64 = l
            .iter()
            .zip(&d)
            .skip(i + 1)
            .map(|(row, dk)| row[i] * dk)
            .sum();
        d[i] = (y[i] - below) / l[i][i];
    }
    Some(d)
}

/// The rows of the Jacobian that add nothing to the rows before them, and the rank. A row is
/// compared with an orthonormal basis of the rows before it (Gram and Schmidt, twice).
fn dependent_rows(j: &[Vec<f64>], rank_tol: f64) -> (usize, Vec<bool>) {
    let mut basis: Vec<Vec<f64>> = Vec::new();
    let mut dependent = vec![false; j.len()];
    for (i, row) in j.iter().enumerate() {
        let original: f64 = dot(row, row).sqrt();
        let mut v = row.clone();
        for _ in 0..2 {
            for b in &basis {
                let c = dot(&v, b);
                for (e, f) in v.iter_mut().zip(b) {
                    *e -= c * f;
                }
            }
        }
        let rest: f64 = dot(&v, &v).sqrt();
        if rest <= rank_tol * original.max(1.0) {
            dependent[i] = true;
        } else {
            for e in &mut v {
                *e /= rest;
            }
            basis.push(v);
        }
    }
    (basis.len(), dependent)
}

/// Whether the row `target` is a combination of the `rows`.
fn in_span(rows: &[&Vec<f64>], target: &[f64], rank_tol: f64) -> bool {
    let mut basis: Vec<Vec<f64>> = Vec::new();
    for row in rows {
        let mut v = (*row).clone();
        for _ in 0..2 {
            for b in &basis {
                let c = dot(&v, b);
                for (e, f) in v.iter_mut().zip(b) {
                    *e -= c * f;
                }
            }
        }
        let rest: f64 = dot(&v, &v).sqrt();
        if rest > rank_tol {
            for e in &mut v {
                *e /= rest;
            }
            basis.push(v);
        }
    }
    let original: f64 = dot(target, target).sqrt();
    let mut v = target.to_vec();
    for _ in 0..2 {
        for b in &basis {
            let c = dot(&v, b);
            for (e, f) in v.iter_mut().zip(b) {
                *e -= c * f;
            }
        }
    }
    let rest: f64 = dot(&v, &v).sqrt();
    rest <= rank_tol * original.max(1.0)
}

/// The constraints that add nothing, each with the earlier constraints that it depends on, as a
/// sorted list of identifiers.
fn culprits(model: &Model, j: &[Vec<f64>], dependent: &[bool], rank_tol: f64) -> Vec<ConstraintId> {
    let mut out: Vec<ConstraintId> = Vec::new();
    for (row, is_dependent) in dependent.iter().enumerate() {
        let Some(owner) = model.owners[row].filter(|_| *is_dependent) else {
            continue;
        };
        out.push(model.active[owner].id);
        // the independent rows before it
        let before: Vec<usize> = (0..row).filter(|r| !dependent[*r]).collect();
        for skip in &before {
            let others: Vec<&Vec<f64>> = before
                .iter()
                .filter(|r| *r != skip)
                .map(|r| &j[*r])
                .collect();
            if !in_span(&others, &j[row], rank_tol) {
                if let Some(o) = model.owners[*skip] {
                    out.push(model.active[o].id);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn to_length(v: f64) -> Option<Length> {
    if !v.is_finite() {
        return None;
    }
    let nm = (v * NM_PER_MM).round();
    if nm.abs() >= 1.0e12 {
        return None;
    }
    Length::new(nm as i64).ok()
}

impl Sketch {
    fn check_size(&self, s: &SketchSettings) -> Result<(), CadError> {
        if self.elements().len() > s.max_elements || self.constraints().len() > s.max_constraints {
            return Err(CadError::ValueOutOfRange("size of the sketch"));
        }
        Ok(())
    }

    /// Solves the sketch. When the constraints hold the points and radii are moved to the
    /// solution (rounded to the nanometre) and the degrees of freedom left are reported; in any
    /// other case the sketch is left exactly as it was. The same sketch and settings give the
    /// same outcome every time.
    pub fn solve(&mut self, s: &SketchSettings) -> Result<Outcome, CadError> {
        self.check_size(s)?;
        let (model, x0) = Model::new(self);
        let mut x = x0;
        let mut r = model.residuals(&x);
        let mut lambda = s.damping_start;
        let mut iterations = 0u32;
        let mut stuck = false;
        while max_abs(&r) > s.tolerance && iterations < s.max_iterations {
            iterations += 1;
            let jac = model.jacobian(&x, s.fd_step);
            let mut a = vec![vec![0.0f64; model.n]; model.n];
            let mut g = vec![0.0f64; model.n];
            for (row, ri) in jac.iter().zip(&r) {
                for (c1, v1) in row.iter().enumerate() {
                    if *v1 == 0.0 {
                        continue;
                    }
                    g[c1] += v1 * ri;
                    for (c2, v2) in row.iter().enumerate() {
                        a[c1][c2] += v1 * v2;
                    }
                }
            }
            let cost = dot(&r, &r);
            let mut accepted = false;
            while lambda <= s.damping_max {
                if let Some(mut step) = solve_damped(&a, lambda, &g) {
                    let biggest = max_abs(&step);
                    if biggest > s.max_step {
                        let scale = s.max_step / biggest;
                        for e in &mut step {
                            *e *= scale;
                        }
                    }
                    let next: Vec<f64> = x.iter().zip(&step).map(|(p, d)| p + d).collect();
                    let rn = model.residuals(&next);
                    let cost_next = dot(&rn, &rn);
                    if cost_next.is_finite() && cost_next < cost {
                        x = next;
                        r = rn;
                        lambda = (lambda / s.damping_factor).max(1e-18);
                        accepted = true;
                        break;
                    }
                }
                lambda *= s.damping_factor;
            }
            if !accepted {
                stuck = true;
                break;
            }
        }
        let residual = max_abs(&r);
        let converged = residual <= s.tolerance;
        let jac = model.jacobian(&x, s.fd_step);
        let (rank, dependent) = dependent_rows(&jac, s.rank_tolerance);
        let any_dependent = dependent
            .iter()
            .enumerate()
            .any(|(i, d)| *d && model.owners[i].is_some());
        if converged {
            if any_dependent {
                return Ok(Outcome::OverConstrained {
                    redundant: culprits(&model, &jac, &dependent, s.rank_tolerance),
                    conflicting: Vec::new(),
                });
            }
        } else {
            let mut g = vec![0.0f64; model.n];
            for (row, ri) in jac.iter().zip(&r) {
                for (c, v) in row.iter().enumerate() {
                    g[c] += v * ri;
                }
            }
            let at_minimum = stuck && max_abs(&g) <= s.stall_tolerance;
            if at_minimum && any_dependent {
                return Ok(Outcome::OverConstrained {
                    redundant: Vec::new(),
                    conflicting: culprits(&model, &jac, &dependent, s.rank_tolerance),
                });
            }
            return Ok(Outcome::NotConverged {
                iterations,
                residual,
            });
        }
        // a solution: round it, and keep it only if it is a sketch
        let mut points: Vec<(ElementId, Length, Length)> = Vec::new();
        let mut radii: Vec<(ElementId, Length)> = Vec::new();
        for (id, i) in &model.pidx {
            match (to_length(x[*i]), to_length(x[*i + 1])) {
                (Some(px), Some(py)) => points.push((*id, px, py)),
                _ => {
                    return Ok(Outcome::NotConverged {
                        iterations,
                        residual,
                    })
                }
            }
        }
        for (id, i) in &model.ridx {
            match to_length(x[*i]) {
                Some(r) if r > Length::ZERO => radii.push((*id, r)),
                _ => {
                    return Ok(Outcome::NotConverged {
                        iterations,
                        residual,
                    })
                }
            }
        }
        let dof = model.n - rank;
        for (id, px, py) in points {
            self.set_point_exact(id, px, py);
        }
        for (id, r) in radii {
            self.set_radius_exact(id, r);
        }
        Ok(Outcome::Solved {
            dof,
            iterations,
            residual,
        })
    }

    /// The degrees of freedom of the sketch where it is now, without moving anything: the
    /// unknowns minus the rank of the constraints. It is only meaningful when the constraints
    /// hold; call [`Sketch::solve`] first.
    pub fn degrees_of_freedom(&self, s: &SketchSettings) -> Result<usize, CadError> {
        self.check_size(s)?;
        let (model, x) = Model::new(self);
        let jac = model.jacobian(&x, s.fd_step);
        let (rank, _) = dependent_rows(&jac, s.rank_tolerance);
        Ok(model.n - rank)
    }

    /// The angle between two lines as they are drawn now: from the direction of the first to the
    /// direction of the second, counter-clockwise, in the half-open range of minus a half turn
    /// (excluded) to a half turn. It is found by halving an interval with the integer table of
    /// sines of [`Angle`], so it is the same on every platform; it serves the drawing (the value
    /// to show beside an angle), the solver does not use it.
    pub fn measured_angle(&self, a: ElementId, b: ElementId) -> Result<Angle, CadError> {
        let (model, x) = Model::new(self);
        if !model.is_line(a) || !model.is_line(b) {
            return Err(CadError::SketchConstraint(
                "an angle is between two lines".into(),
            ));
        }
        let ((p1, q1), (p2, q2)) = (model.line_ends(&x, a), model.line_ends(&x, b));
        let (d1, d2) = ((q1.0 - p1.0, q1.1 - p1.1), (q2.0 - p2.0, q2.1 - p2.1));
        let cross = d1.0 * d2.1 - d1.1 * d2.0;
        let dot_product = d1.0 * d2.0 + d1.1 * d2.1;
        let half: i64 = 180_000_000_000;
        if cross == 0.0 {
            return Angle::new(if dot_product >= 0.0 { 0 } else { half });
        }
        // the first direction turned by theta, crossed with the second, is positive before the
        // angle and negative after it, inside the half turn on the side the cross product gives
        let turned = |theta: i64| -> Result<f64, CadError> {
            let t = Angle::new(theta)?;
            let c = t.cos() as f64 / 1e9;
            let s = t.sin() as f64 / 1e9;
            let v = (d1.0 * c - d1.1 * s, d1.0 * s + d1.1 * c);
            Ok(v.0 * d2.1 - v.1 * d2.0)
        };
        let (mut lo, mut hi) = if cross > 0.0 { (0, half) } else { (-half, 0) };
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if turned(mid)? > 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Angle::new(hi)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{mm, sketch};
    use super::super::Dim;
    use super::*;
    use crate::enclosure::Face;
    use crate::sketch::SketchPlane;

    fn s() -> SketchSettings {
        SketchSettings::defaults()
    }

    fn um(v: i64) -> Length {
        Length::from_um(v).unwrap()
    }

    fn nm(v: i64) -> Length {
        Length::new(v).unwrap()
    }

    fn solved(sketch: &mut Sketch) -> usize {
        match sketch.solve(&s()).unwrap() {
            Outcome::Solved { dof, residual, .. } => {
                assert!(residual <= 1e-8);
                dof
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_rectangle_of_40_by_25_with_a_fixed_centre_lands_on_exact_coordinates() {
        let mut k = sketch();
        // drawn crooked and too small on purpose
        let r = k
            .add_rectangle((mm(-18), mm(-11)), (mm(22), mm(14)))
            .unwrap();
        let [p0, p1, p2, _p3] = r.points;
        let diagonal = k.add_line(p0, p2).unwrap();
        k.set_construction(diagonal, true).unwrap();
        let centre = k.add_point(mm(1), mm(1)).unwrap();
        k.add_constraint(Constraint::Midpoint {
            point: centre,
            line: diagonal,
        })
        .unwrap();
        k.add_constraint(Constraint::Fixed {
            point: centre,
            x: mm(0),
            y: mm(0),
        })
        .unwrap();
        // 8 + 2 unknowns and 4 + 2 + 2 constraints: two dimensions are missing
        assert_eq!(solved(&mut k), 2);
        k.add_constraint(Constraint::DistanceX {
            a: p0,
            b: p1,
            dim: Dim::fixed(mm(40)),
        })
        .unwrap();
        k.add_constraint(Constraint::DistanceY {
            a: p1,
            b: p2,
            dim: Dim::fixed(mm(25)),
        })
        .unwrap();
        let outcome = k.solve(&s()).unwrap();
        assert!(outcome.is_fully_constrained(), "{outcome:?}");
        assert_eq!(k.point(r.points[0]).unwrap(), (mm(-20), nm(-12_500_000)));
        assert_eq!(k.point(r.points[1]).unwrap(), (mm(20), nm(-12_500_000)));
        assert_eq!(k.point(r.points[2]).unwrap(), (mm(20), nm(12_500_000)));
        assert_eq!(k.point(r.points[3]).unwrap(), (mm(-20), nm(12_500_000)));
        assert_eq!(k.point(centre).unwrap(), (mm(0), mm(0)));
    }

    #[test]
    fn a_circle_tangent_to_two_lines_has_its_centre_at_the_radius_from_both() {
        let mut k = sketch();
        let o = k.add_point(mm(0), mm(0)).unwrap();
        let ex = k.add_point(mm(100), mm(0)).unwrap();
        let ey = k.add_point(mm(0), mm(100)).unwrap();
        let lx = k.add_line(o, ex).unwrap();
        let ly = k.add_line(o, ey).unwrap();
        for p in [o, ex, ey] {
            k.fix_point(p).unwrap();
        }
        let c = k.add_point(mm(12), mm(9)).unwrap();
        let circle = k.add_circle(c, mm(7)).unwrap();
        k.add_constraint(Constraint::Tangent(lx, circle)).unwrap();
        k.add_constraint(Constraint::Tangent(circle, ly)).unwrap();
        // the radius is free: one degree of freedom (the circle grows along the diagonal)
        assert_eq!(solved(&mut k), 1);
        let radius_dimension = k
            .add_constraint(Constraint::Radius {
                round: circle,
                dim: Dim::fixed(mm(10)),
            })
            .unwrap();
        assert_eq!(solved(&mut k), 0);
        assert_eq!(k.point(c).unwrap(), (mm(10), mm(10)));
        assert_eq!(
            k.element(circle),
            Some(&Element::Circle {
                center: c,
                radius: mm(10)
            })
        );
        // a diameter of 30 instead: the centre moves to 15, 15
        k.remove_constraint(radius_dimension).unwrap();
        k.add_constraint(Constraint::Diameter {
            round: circle,
            dim: Dim::fixed(mm(30)),
        })
        .unwrap();
        assert_eq!(solved(&mut k), 0);
        assert_eq!(k.point(c).unwrap(), (mm(15), mm(15)));
    }

    #[test]
    fn two_circles_touch_from_outside_or_from_inside_as_they_start() {
        let mut k = sketch();
        let c1 = k.add_point(mm(0), mm(0)).unwrap();
        let c2 = k.add_point(mm(17), mm(0)).unwrap();
        let a = k.add_circle(c1, mm(10)).unwrap();
        let b = k.add_circle(c2, mm(5)).unwrap();
        k.fix_point(c1).unwrap();
        k.add_constraint(Constraint::HorizontalPoints(c1, c2))
            .unwrap();
        k.add_constraint(Constraint::Radius {
            round: a,
            dim: Dim::fixed(mm(10)),
        })
        .unwrap();
        k.add_constraint(Constraint::Radius {
            round: b,
            dim: Dim::fixed(mm(5)),
        })
        .unwrap();
        let t = k.add_constraint(Constraint::Tangent(a, b)).unwrap();
        // 17 is nearer to 10 + 5 = 15 than to 10 - 5 = 5: outside
        assert_eq!(solved(&mut k), 0);
        assert_eq!(k.point(c2).unwrap(), (mm(15), mm(0)));
        // started inside, it stays inside: the small circle at 6 touches the big one at 5
        k.remove_constraint(t).unwrap();
        k.move_point(c2, mm(6), mm(0)).unwrap();
        k.add_constraint(Constraint::Tangent(a, b)).unwrap();
        assert_eq!(solved(&mut k), 0);
        assert_eq!(k.point(c2).unwrap(), (mm(5), mm(0)));
    }

    #[test]
    fn an_angle_a_length_and_a_perpendicular_give_exact_points() {
        let mut k = sketch();
        let o = k.add_point(mm(0), mm(0)).unwrap();
        let e = k.add_point(mm(10), mm(0)).unwrap();
        let b = k.add_point(mm(9), mm(4)).unwrap();
        let l0 = k.add_line(o, e).unwrap();
        let l1 = k.add_line(o, b).unwrap();
        k.fix_point(o).unwrap();
        k.fix_point(e).unwrap();
        k.add_constraint(Constraint::LineLength {
            line: l1,
            dim: Dim::fixed(mm(10)),
        })
        .unwrap();
        k.add_constraint(Constraint::AngleBetween {
            a: l0,
            b: l1,
            dim: Dim::fixed(Angle::from_deg(30).unwrap()),
        })
        .unwrap();
        assert_eq!(solved(&mut k), 0);
        // 10 cos 30 = 8.660254037844386, 10 sin 30 = 5
        assert_eq!(k.point(b).unwrap(), (nm(8_660_254), mm(5)));
        // the measured angle goes back to 30 degrees, within a thousandth of a degree
        let measured = k.measured_angle(l0, l1).unwrap();
        assert!(
            (measured.get() - 30_000_000_000).abs() < 1_000_000,
            "{measured:?}"
        );

        // perpendicular: the second line stands on the first
        let mut p = sketch();
        let o = p.add_point(mm(0), mm(0)).unwrap();
        let e = p.add_point(mm(10), mm(0)).unwrap();
        let c = p.add_point(mm(1), mm(6)).unwrap();
        let l0 = p.add_line(o, e).unwrap();
        let l1 = p.add_line(o, c).unwrap();
        p.fix_point(o).unwrap();
        p.fix_point(e).unwrap();
        p.add_constraint(Constraint::Perpendicular(l0, l1)).unwrap();
        p.add_constraint(Constraint::LineLength {
            line: l1,
            dim: Dim::fixed(mm(7)),
        })
        .unwrap();
        assert_eq!(solved(&mut p), 0);
        assert_eq!(p.point(c).unwrap(), (mm(0), mm(7)));
    }

    #[test]
    fn parallel_equal_and_symmetry_hold() {
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(10), mm(2)).unwrap();
        let c = k.add_point(mm(0), mm(5)).unwrap();
        let d = k.add_point(mm(7), mm(9)).unwrap();
        let l1 = k.add_line(a, b).unwrap();
        let l2 = k.add_line(c, d).unwrap();
        k.fix_point(a).unwrap();
        k.fix_point(b).unwrap();
        k.fix_point(c).unwrap();
        k.add_constraint(Constraint::Parallel(l1, l2)).unwrap();
        k.add_constraint(Constraint::Equal(l1, l2)).unwrap();
        // the second line is as long as the first (sqrt(104)) and parallel: d = c + (10, 2)
        assert_eq!(solved(&mut k), 0);
        assert_eq!(k.point(d).unwrap(), (mm(10), mm(7)));

        // symmetry through a point and across a line
        let mut m = sketch();
        let p = m.add_point(mm(4), mm(3)).unwrap();
        let q = m.add_point(mm(15), mm(2)).unwrap();
        let o = m.add_point(mm(10), mm(0)).unwrap();
        let top = m.add_point(mm(10), mm(20)).unwrap();
        let axis = m.add_line(o, top).unwrap();
        m.fix_point(p).unwrap();
        m.fix_point(o).unwrap();
        m.fix_point(top).unwrap();
        m.add_constraint(Constraint::SymmetricLine { a: p, b: q, axis })
            .unwrap();
        assert_eq!(solved(&mut m), 0);
        assert_eq!(m.point(q).unwrap(), (mm(16), mm(3)));

        let mut n = sketch();
        let p = n.add_point(mm(1), mm(2)).unwrap();
        let q = n.add_point(mm(0), mm(0)).unwrap();
        let c = n.add_point(mm(5), mm(5)).unwrap();
        n.fix_point(p).unwrap();
        n.fix_point(c).unwrap();
        n.add_constraint(Constraint::SymmetricPoint {
            a: p,
            b: q,
            center: c,
        })
        .unwrap();
        assert_eq!(solved(&mut n), 0);
        assert_eq!(n.point(q).unwrap(), (mm(9), mm(8)));
    }

    #[test]
    fn a_point_on_a_line_a_circle_and_the_circle_of_an_arc() {
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(10), mm(10)).unwrap();
        let line = k.add_line(a, b).unwrap();
        let p = k.add_point(mm(3), mm(4)).unwrap();
        k.fix_point(a).unwrap();
        k.fix_point(b).unwrap();
        k.add_constraint(Constraint::PointOnObject {
            point: p,
            object: line,
        })
        .unwrap();
        k.add_constraint(Constraint::DistanceX {
            a,
            b: p,
            dim: Dim::fixed(mm(6)),
        })
        .unwrap();
        assert_eq!(solved(&mut k), 0);
        assert_eq!(k.point(p).unwrap(), (mm(6), mm(6)));

        // an arc around the origin from (5, 0): its end is kept 5 from the centre
        let mut m = sketch();
        let c = m.add_point(mm(0), mm(0)).unwrap();
        let st = m.add_point(mm(5), mm(0)).unwrap();
        let en = m.add_point(mm(1), mm(4)).unwrap();
        let arc = m.add_arc(c, st, en).unwrap();
        m.fix_point(c).unwrap();
        m.fix_point(st).unwrap();
        // free end: only its distance is set, so it can turn (one degree of freedom)
        assert_eq!(solved(&mut m), 1);
        m.add_constraint(Constraint::DistanceY {
            a: c,
            b: en,
            dim: Dim::fixed(mm(3)),
        })
        .unwrap();
        assert_eq!(solved(&mut m), 0);
        assert_eq!(m.point(en).unwrap(), (mm(4), mm(3)));
        let q = m.add_point(mm(0), mm(4)).unwrap();
        m.add_constraint(Constraint::PointOnObject {
            point: q,
            object: arc,
        })
        .unwrap();
        m.add_constraint(Constraint::VerticalPoints(c, q)).unwrap();
        assert_eq!(solved(&mut m), 0);
        assert_eq!(m.point(q).unwrap(), (mm(0), mm(5)));
    }

    #[test]
    fn the_midpoint_and_the_diameter_are_exact() {
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(7), mm(3)).unwrap();
        let line = k.add_line(a, b).unwrap();
        let m = k.add_point(mm(1), mm(1)).unwrap();
        k.fix_point(a).unwrap();
        k.fix_point(b).unwrap();
        k.add_constraint(Constraint::Midpoint { point: m, line })
            .unwrap();
        assert_eq!(solved(&mut k), 0);
        assert_eq!(k.point(m).unwrap(), (nm(3_500_000), nm(1_500_000)));
    }

    #[test]
    fn degrees_of_freedom_are_counted() {
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(5), mm(1)).unwrap();
        let line = k.add_line(a, b).unwrap();
        assert_eq!(solved(&mut k), 4);
        k.add_constraint(Constraint::Horizontal(line)).unwrap();
        assert_eq!(solved(&mut k), 3);
        k.fix_point(a).unwrap();
        assert_eq!(solved(&mut k), 1);
        k.add_constraint(Constraint::LineLength {
            line,
            dim: Dim::fixed(mm(5)),
        })
        .unwrap();
        assert_eq!(solved(&mut k), 0);
        // making the line horizontal moved both ends to the same height and the first end was
        // held there; the length then puts the second end 5 mm to its right
        let (ax, ay) = k.point(a).unwrap();
        assert_eq!(k.point(b).unwrap(), (ax.checked_add(mm(5)).unwrap(), ay));
        assert_eq!(k.degrees_of_freedom(&s()).unwrap(), 0);
        // a rectangle alone: four
        let mut r = sketch();
        r.add_rectangle((mm(0), mm(0)), (mm(3), mm(2))).unwrap();
        assert_eq!(solved(&mut r), 4);
        assert_eq!(r.degrees_of_freedom(&s()).unwrap(), 4);
        // a circle alone: three
        let mut c = sketch();
        let p = c.add_point(mm(0), mm(0)).unwrap();
        c.add_circle(p, mm(1)).unwrap();
        assert_eq!(solved(&mut c), 3);
    }

    #[test]
    fn disabled_constraints_are_ignored_and_a_constraint_can_come_back() {
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(5), mm(1)).unwrap();
        let line = k.add_line(a, b).unwrap();
        let h = k.add_constraint(Constraint::Horizontal(line)).unwrap();
        k.set_enabled(h, false).unwrap();
        assert_eq!(solved(&mut k), 4);
        assert_eq!(k.point(b).unwrap(), (mm(5), mm(1)));
        k.set_enabled(h, true).unwrap();
        assert_eq!(solved(&mut k), 3);
        assert_eq!(k.point(b).unwrap().1, k.point(a).unwrap().1);
    }

    #[test]
    fn a_repeated_constraint_is_redundant_and_names_the_ones_it_repeats() {
        let mut k = sketch();
        let r = k.add_rectangle((mm(0), mm(0)), (mm(40), mm(25))).unwrap();
        // the top and the bottom are already horizontal: parallel adds nothing
        let extra = k
            .add_constraint(Constraint::Parallel(r.lines[0], r.lines[2]))
            .unwrap();
        let before = k.clone();
        match k.solve(&s()).unwrap() {
            Outcome::OverConstrained {
                redundant,
                conflicting,
            } => {
                assert!(conflicting.is_empty());
                assert!(redundant.contains(&extra), "{redundant:?}");
                // the two horizontals it depends on are named with it (c1 and c3)
                assert_eq!(
                    redundant,
                    vec![ConstraintId(1), ConstraintId(3), extra],
                    "{redundant:?}"
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(k, before);
        let err = k.solve(&s()).unwrap().into_result().unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-033");
        assert!(err.to_string().contains("redundant: c1 c3 c5"), "{err}");
    }

    #[test]
    fn two_lengths_for_one_line_are_in_conflict_and_nothing_moves() {
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(30), mm(0)).unwrap();
        let line = k.add_line(a, b).unwrap();
        k.fix_point(a).unwrap();
        k.add_constraint(Constraint::Horizontal(line)).unwrap();
        let c1 = k
            .add_constraint(Constraint::DistanceX {
                a,
                b,
                dim: Dim::fixed(mm(40)),
            })
            .unwrap();
        let c2 = k
            .add_constraint(Constraint::LineLength {
                line,
                dim: Dim::fixed(mm(41)),
            })
            .unwrap();
        let before = k.clone();
        match k.solve(&s()).unwrap() {
            Outcome::OverConstrained {
                redundant,
                conflicting,
            } => {
                assert!(redundant.is_empty());
                assert_eq!(conflicting, vec![c1, c2]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(k, before);
        let err = k.solve(&s()).unwrap().into_result().unwrap_err();
        assert!(err.to_string().contains("in conflict: c3 c4"), "{err}");
    }

    #[test]
    fn a_triangle_that_cannot_close_does_not_converge_or_is_in_conflict_but_never_changes() {
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(10), mm(0)).unwrap();
        let c = k.add_point(mm(5), mm(5)).unwrap();
        k.fix_point(a).unwrap();
        k.fix_point(b).unwrap();
        // 3 + 4 < 10: no point is at 3 from one end and 4 from the other
        for (p, d) in [(a, 3), (b, 4)] {
            k.add_constraint(Constraint::Distance {
                a: p,
                b: c,
                dim: Dim::fixed(mm(d)),
            })
            .unwrap();
        }
        let before = k.clone();
        let outcome = k.solve(&s()).unwrap();
        assert!(
            matches!(
                outcome,
                Outcome::NotConverged { .. } | Outcome::OverConstrained { .. }
            ),
            "{outcome:?}"
        );
        assert_eq!(k, before);
    }

    #[test]
    fn running_out_of_steps_is_reported_with_the_settings_that_ask_for_few() {
        let mut k = sketch();
        let a = k.add_point(mm(0), mm(0)).unwrap();
        let b = k.add_point(mm(1), mm(1)).unwrap();
        k.fix_point(a).unwrap();
        k.add_constraint(Constraint::Distance {
            a,
            b,
            dim: Dim::fixed(mm(30)),
        })
        .unwrap();
        // a step is at most a thousandth of a millimetre and only one step is allowed
        let tight =
            SketchSettings::with_user_config("[sketch]\nmax_iterations = 1\nmax_step = 0.001\n")
                .unwrap();
        let before = k.clone();
        match k.solve(&tight).unwrap() {
            Outcome::NotConverged { iterations, .. } => assert_eq!(iterations, 1),
            other => panic!("{other:?}"),
        }
        assert_eq!(k, before);
        let err = k.solve(&tight).unwrap().into_result().unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-034");
        // with the shipped settings it is solved
        assert_eq!(solved(&mut k), 1);
    }

    #[test]
    fn the_same_sketch_is_solved_to_the_same_bits_every_time() {
        let build = || {
            let mut k = sketch();
            let r = k
                .add_rectangle((mm(-18), mm(-11)), (mm(22), mm(14)))
                .unwrap();
            let [p0, p1, p2, _] = r.points;
            let o = k.add_point(mm(3), mm(3)).unwrap();
            let circle = k.add_circle(o, um(4_321)).unwrap();
            k.add_constraint(Constraint::Tangent(r.lines[0], circle))
                .unwrap();
            k.add_constraint(Constraint::Tangent(r.lines[3], circle))
                .unwrap();
            k.add_constraint(Constraint::Diameter {
                round: circle,
                dim: Dim::fixed(um(7_777)),
            })
            .unwrap();
            k.fix_point(p0).unwrap();
            k.add_constraint(Constraint::Distance {
                a: p0,
                b: p2,
                dim: Dim::fixed(um(50_000)),
            })
            .unwrap();
            k.add_constraint(Constraint::DistanceX {
                a: p0,
                b: p1,
                dim: Dim::fixed(um(31_415)),
            })
            .unwrap();
            k
        };
        let mut a = build();
        let mut b = build();
        let oa = a.solve(&s()).unwrap();
        let ob = b.solve(&s()).unwrap();
        assert_eq!(oa, ob);
        assert_eq!(a, b);
        match oa {
            Outcome::Solved {
                residual,
                iterations,
                ..
            } => {
                // the bits of the residual are the same too
                match ob {
                    Outcome::Solved {
                        residual: r2,
                        iterations: i2,
                        ..
                    } => {
                        assert_eq!(residual.to_bits(), r2.to_bits());
                        assert_eq!(iterations, i2);
                    }
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        }
        // solving a solved sketch changes nothing
        let again = a.clone();
        a.solve(&s()).unwrap();
        assert_eq!(a, again);
    }

    #[test]
    fn a_sketch_beyond_the_limits_of_the_settings_is_refused() {
        let mut k = Sketch::new("big", SketchPlane::on_face(Face::Front)).unwrap();
        for i in 0..3 {
            k.add_point(mm(i), mm(0)).unwrap();
        }
        let small = SketchSettings::with_user_config("[sketch]\nmax_elements = 2\n").unwrap();
        let err = k.solve(&small).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-007");
        assert!(k.degrees_of_freedom(&small).is_err());
    }

    #[test]
    fn an_empty_sketch_is_solved_with_nothing_free() {
        let mut k = sketch();
        assert_eq!(solved(&mut k), 0);
    }
}
