//! The numeric engine that solves the constraints of a 2D sketch.
//!
//! The contract is small and stable:
//!
//! * a [`Problem`] gives the number of unknowns, the residuals at a point (numbers that are zero
//!   when every constraint holds) and who owns each residual row (a constraint, or nobody for the
//!   rows an element brings by itself);
//! * [`solve`] looks for the unknowns that make every residual zero by damped least squares
//!   (Levenberg and Marquardt): at each step it solves `(J'J + lambda I) d = -J'r` with the Jacobian
//!   `J` made by central differences, keeps the step when the sum of squares goes down, and asks
//!   for more damping when it does not;
//! * the answer is a [`Verdict`]: solved with the degrees of freedom left (the unknowns minus the
//!   rank of the Jacobian at the solution), over-constrained (the constraints that add nothing to
//!   the ones before them, listed as **redundant** when the rest has a solution and as in
//!   **conflict** when it has none, each with the earlier ones it depends on) or not converged;
//! * [`System`] is a ready problem made of variables and constraints given as functions, for
//!   callers that have no model of their own;
//! * the tolerances, the damping and the limits are [`Settings`], read from the file
//!   `phoenix-cad-solver.default.cfg` (or another one with the keys to change); the code holds
//!   none of those values;
//! * no function of the mathematical library is used by the engine (only sums, products,
//!   quotients and square roots, which every platform computes the same way), no thread and no
//!   random number, so the same problem gives the same answer bit for bit.
//!
//! The engine never changes what it is given: it returns the unknowns it found and the caller
//! decides whether to keep them.

mod system;

pub use system::{ConstraintId, System, VarId};

/// The shipped settings of the engine, as text.
pub const DEFAULT_SETTINGS: &str = include_str!("../phoenix-cad-solver.default.cfg");

/// Why a problem or a settings text was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolverError {
    /// A settings text with a line that is not understood, a value that is not a number or one
    /// outside its range.
    Settings(String),
    /// A problem with more unknowns or more rows than the settings allow.
    TooLarge(String),
    /// A variable or a constraint that does not exist.
    Unknown(String),
    /// A constraint whose residuals are not numbers, or that gives a different number of rows
    /// from the one it declared.
    Residuals(String),
}

impl SolverError {
    /// The stable code of the reason.
    pub fn code(&self) -> &'static str {
        match self {
            SolverError::Settings(_) => "SKSOLVER-001",
            SolverError::TooLarge(_) => "SKSOLVER-002",
            SolverError::Unknown(_) => "SKSOLVER-003",
            SolverError::Residuals(_) => "SKSOLVER-004",
        }
    }
}

impl std::fmt::Display for SolverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (title, why) = match self {
            SolverError::Settings(w) => ("Solver settings not valid", w),
            SolverError::TooLarge(w) => ("Problem larger than the settings allow", w),
            SolverError::Unknown(w) => ("Unknown variable or constraint", w),
            SolverError::Residuals(w) => ("Residuals not valid", w),
        };
        write!(f, "{}: {title}: {why}", self.code())
    }
}

impl std::error::Error for SolverError {}

/// The tuned values of the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// The largest residual of a solution.
    pub tolerance: f64,
    /// The most steps.
    pub max_iterations: u32,
    /// The largest move of an unknown in one step.
    pub max_step: f64,
    /// The step of the central differences.
    pub fd_step: f64,
    /// The relative size under which a row of the Jacobian adds nothing.
    pub rank_tolerance: f64,
    /// The first damping.
    pub damping_start: f64,
    /// The factor by which the damping grows or shrinks.
    pub damping_factor: f64,
    /// The damping beyond which the solver stops.
    pub damping_max: f64,
    /// The gradient under which the solver is at a minimum that is not a solution.
    pub stall_tolerance: f64,
    /// The most unknowns of a problem.
    pub max_unknowns: usize,
    /// The most residual rows of a problem.
    pub max_rows: usize,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings::from_cfg_text("").unwrap_or(Settings {
            tolerance: 1e-8,
            max_iterations: 100,
            max_step: 50.0,
            fd_step: 1e-4,
            rank_tolerance: 1e-6,
            damping_start: 1e-3,
            damping_factor: 10.0,
            damping_max: 1e12,
            stall_tolerance: 1e-7,
            max_unknowns: 4000,
            max_rows: 8000,
        })
    }
}

fn real(key: &str, text: &str, low: f64, high: f64) -> Result<f64, SolverError> {
    let v: f64 = text
        .parse()
        .map_err(|_| SolverError::Settings(format!("{key}: {text:?} is not a number")))?;
    if !v.is_finite() || v < low || v > high {
        return Err(SolverError::Settings(format!(
            "{key}: {text} is outside {low} to {high}"
        )));
    }
    Ok(v)
}

fn count(key: &str, text: &str, low: u64, high: u64) -> Result<u64, SolverError> {
    let v: u64 = text
        .parse()
        .map_err(|_| SolverError::Settings(format!("{key}: {text:?} is not a whole number")))?;
    if v < low || v > high {
        return Err(SolverError::Settings(format!(
            "{key}: {text} is outside {low} to {high}"
        )));
    }
    Ok(v)
}

impl Settings {
    /// The shipped settings with the keys of `text` (a file of the same form, section `[solver]`)
    /// laid over them. An unknown key, a line that is not `key = value`, a number outside its
    /// range or a section other than `[solver]` is refused.
    pub fn from_cfg_text(text: &str) -> Result<Settings, SolverError> {
        let mut s = Settings {
            tolerance: 0.0,
            max_iterations: 0,
            max_step: 0.0,
            fd_step: 0.0,
            rank_tolerance: 0.0,
            damping_start: 0.0,
            damping_factor: 0.0,
            damping_max: 0.0,
            stall_tolerance: 0.0,
            max_unknowns: 0,
            max_rows: 0,
        };
        for source in [DEFAULT_SETTINGS, text] {
            let mut in_section = false;
            for (n, raw) in source.lines().enumerate() {
                let line = raw.split(" #").next().unwrap_or("").trim();
                let line = if line.starts_with('#') { "" } else { line };
                if line.is_empty() {
                    continue;
                }
                if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                    if name.trim() != "solver" {
                        return Err(SolverError::Settings(format!(
                            "line {}: the section {name:?} is not [solver]",
                            n + 1
                        )));
                    }
                    in_section = true;
                    continue;
                }
                let Some((key, value)) = line.split_once('=') else {
                    return Err(SolverError::Settings(format!(
                        "line {}: not key = value",
                        n + 1
                    )));
                };
                if !in_section {
                    return Err(SolverError::Settings(format!(
                        "line {}: a key outside [solver]",
                        n + 1
                    )));
                }
                let (key, value) = (key.trim(), value.trim());
                match key {
                    "tolerance" => s.tolerance = real(key, value, 1e-15, 1.0)?,
                    "max_iterations" => s.max_iterations = count(key, value, 1, 100_000)? as u32,
                    "max_step" => s.max_step = real(key, value, 1e-9, 1e12)?,
                    "fd_step" => s.fd_step = real(key, value, 1e-12, 1.0)?,
                    "rank_tolerance" => s.rank_tolerance = real(key, value, 1e-15, 1.0)?,
                    "damping_start" => s.damping_start = real(key, value, 1e-18, 1e18)?,
                    "damping_factor" => s.damping_factor = real(key, value, 1.0001, 1e6)?,
                    "damping_max" => s.damping_max = real(key, value, 1e-18, 1e30)?,
                    "stall_tolerance" => s.stall_tolerance = real(key, value, 1e-18, 1.0)?,
                    "max_unknowns" => s.max_unknowns = count(key, value, 1, 1_000_000)? as usize,
                    "max_rows" => s.max_rows = count(key, value, 1, 4_000_000)? as usize,
                    other => {
                        return Err(SolverError::Settings(format!(
                            "line {}: unknown key {other:?}",
                            n + 1
                        )))
                    }
                }
            }
        }
        Ok(s)
    }
}

/// What the engine solves.
pub trait Problem {
    /// The number of unknowns.
    fn unknowns(&self) -> usize;

    /// The residual rows at `x` (`x.len() == self.unknowns()`): numbers that are zero when every
    /// constraint holds. The number of rows must not change from one call to another.
    fn residuals(&self, x: &[f64]) -> Vec<f64>;

    /// The constraint that owns each row, as an index chosen by the caller, or `None` for a row
    /// that an element brings by itself (the end of an arc is as far from its centre as its
    /// start). It has one entry for each row.
    fn owners(&self) -> &[Option<usize>];
}

/// How a solution ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// The constraints hold.
    Solved {
        /// The degrees of freedom left: zero when everything is fully constrained.
        dof: usize,
        /// The steps taken.
        iterations: u32,
        /// The largest residual.
        residual: f64,
    },
    /// Some constraints add nothing to the ones before them.
    OverConstrained {
        /// The constraints (their indexes) that repeat others but do not contradict them, each
        /// with the earlier ones it depends on, in ascending order.
        redundant: Vec<usize>,
        /// The constraints that contradict others, each with the earlier ones it depends on, in
        /// ascending order.
        conflicting: Vec<usize>,
    },
    /// The solver ran out of steps or could not get nearer without being at a conflict.
    NotConverged {
        /// The steps taken.
        iterations: u32,
        /// The largest residual.
        residual: f64,
    },
}

/// The unknowns the engine ended at and how it ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    /// The unknowns at the end (the solution when the verdict is [`Verdict::Solved`]).
    pub x: Vec<f64>,
    /// How it ended.
    pub verdict: Verdict,
}

const MIN_LENGTH: f64 = 1e-12;

fn max_abs(v: &[f64]) -> f64 {
    v.iter().fold(0.0f64, |m, e| m.max(e.abs()))
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(p, q)| p * q).sum()
}

/// The length of a vector, never below a tiny floor, for the callers that divide by it.
pub fn norm2(dx: f64, dy: f64) -> f64 {
    let len: f64 = (dx * dx + dy * dy).sqrt();
    len.max(MIN_LENGTH)
}

fn check_size<P: Problem + ?Sized>(p: &P, x: &[f64], s: &Settings) -> Result<(), SolverError> {
    let n = p.unknowns();
    if x.len() != n {
        return Err(SolverError::Unknown(format!(
            "{} values for {n} unknowns",
            x.len()
        )));
    }
    if n > s.max_unknowns {
        return Err(SolverError::TooLarge(format!(
            "{n} unknowns, the settings allow {}",
            s.max_unknowns
        )));
    }
    let rows = p.owners().len();
    if rows > s.max_rows {
        return Err(SolverError::TooLarge(format!(
            "{rows} rows, the settings allow {}",
            s.max_rows
        )));
    }
    let got = p.residuals(x);
    if got.len() != rows {
        return Err(SolverError::Residuals(format!(
            "{} rows, {rows} declared",
            got.len()
        )));
    }
    if got.iter().any(|v| !v.is_finite()) {
        return Err(SolverError::Residuals(
            "a residual is not a number at the start".to_string(),
        ));
    }
    Ok(())
}

/// The Jacobian by central differences: one row for each residual.
pub fn jacobian<P: Problem + ?Sized>(p: &P, x: &[f64], h: f64) -> Vec<Vec<f64>> {
    let rows = p.owners().len();
    let n = p.unknowns();
    let mut j = vec![vec![0.0f64; n]; rows];
    let mut probe = x.to_vec();
    for col in 0..n {
        let keep = probe[col];
        probe[col] = keep + h;
        let plus = p.residuals(&probe);
        probe[col] = keep - h;
        let minus = p.residuals(&probe);
        probe[col] = keep;
        for (row, (a, b)) in j.iter_mut().zip(plus.iter().zip(minus.iter())) {
            row[col] = (a - b) / (2.0 * h);
        }
    }
    j
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

/// Takes from `v` its part along each vector of the orthonormal `basis` (twice, to keep the
/// rounding small).
fn reduce(v: &mut [f64], basis: &[Vec<f64>]) {
    for _ in 0..2 {
        for b in basis {
            let c = dot(v, b);
            for (e, f) in v.iter_mut().zip(b) {
                *e -= c * f;
            }
        }
    }
}

/// The rows of the Jacobian that add nothing to the rows before them, and the rank. A row is
/// compared with an orthonormal basis of the rows before it (Gram and Schmidt, twice).
fn dependent_rows(j: &[Vec<f64>], rank_tol: f64) -> (usize, Vec<bool>) {
    let mut basis: Vec<Vec<f64>> = Vec::new();
    let mut dependent = vec![false; j.len()];
    for (i, row) in j.iter().enumerate() {
        let original: f64 = dot(row, row).sqrt();
        let mut v = row.clone();
        reduce(&mut v, &basis);
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
        reduce(&mut v, &basis);
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
    reduce(&mut v, &basis);
    let rest: f64 = dot(&v, &v).sqrt();
    rest <= rank_tol * original.max(1.0)
}

/// The constraints that add nothing, each with the earlier constraints that it depends on, as a
/// sorted list of indexes.
fn culprits(
    owners: &[Option<usize>],
    j: &[Vec<f64>],
    dependent: &[bool],
    rank_tol: f64,
) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    for (row, is_dependent) in dependent.iter().enumerate() {
        let Some(owner) = owners[row].filter(|_| *is_dependent) else {
            continue;
        };
        out.push(owner);
        // the independent rows before it
        let before: Vec<usize> = (0..row).filter(|r| !dependent[*r]).collect();
        for skip in &before {
            let others: Vec<&Vec<f64>> = before
                .iter()
                .filter(|r| *r != skip)
                .map(|r| &j[*r])
                .collect();
            if !in_span(&others, &j[row], rank_tol) {
                if let Some(o) = owners[*skip] {
                    out.push(o);
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Solves the problem from `x0`. The same problem and settings give the same answer every time.
pub fn solve<P: Problem + ?Sized>(
    p: &P,
    x0: &[f64],
    s: &Settings,
) -> Result<Solution, SolverError> {
    check_size(p, x0, s)?;
    let n = p.unknowns();
    let owners = p.owners();
    let mut x = x0.to_vec();
    let mut r = p.residuals(&x);
    let mut lambda = s.damping_start;
    let mut iterations = 0u32;
    let mut stuck = false;
    while max_abs(&r) > s.tolerance && iterations < s.max_iterations {
        iterations += 1;
        let jac = jacobian(p, &x, s.fd_step);
        let mut a = vec![vec![0.0f64; n]; n];
        let mut g = vec![0.0f64; n];
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
                let next: Vec<f64> = x.iter().zip(&step).map(|(q, d)| q + d).collect();
                let rn = p.residuals(&next);
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
    let jac = jacobian(p, &x, s.fd_step);
    let (rank, dependent) = dependent_rows(&jac, s.rank_tolerance);
    let any_dependent = dependent
        .iter()
        .enumerate()
        .any(|(i, d)| *d && owners[i].is_some());
    let verdict = if converged {
        if any_dependent {
            Verdict::OverConstrained {
                redundant: culprits(owners, &jac, &dependent, s.rank_tolerance),
                conflicting: Vec::new(),
            }
        } else {
            Verdict::Solved {
                dof: n - rank,
                iterations,
                residual,
            }
        }
    } else {
        let mut g = vec![0.0f64; n];
        for (row, ri) in jac.iter().zip(&r) {
            for (c, v) in row.iter().enumerate() {
                g[c] += v * ri;
            }
        }
        let at_minimum = stuck && max_abs(&g) <= s.stall_tolerance;
        if at_minimum && any_dependent {
            Verdict::OverConstrained {
                redundant: Vec::new(),
                conflicting: culprits(owners, &jac, &dependent, s.rank_tolerance),
            }
        } else {
            Verdict::NotConverged {
                iterations,
                residual,
            }
        }
    };
    Ok(Solution { x, verdict })
}

/// The degrees of freedom at `x`, without moving anything: the unknowns minus the rank of the
/// rows. It is only meaningful when the constraints hold at `x`.
pub fn degrees_of_freedom<P: Problem + ?Sized>(
    p: &P,
    x: &[f64],
    s: &Settings,
) -> Result<usize, SolverError> {
    check_size(p, x, s)?;
    let jac = jacobian(p, x, s.fd_step);
    let (rank, _) = dependent_rows(&jac, s.rank_tolerance);
    Ok(p.unknowns() - rank)
}

#[cfg(test)]
mod tests;
