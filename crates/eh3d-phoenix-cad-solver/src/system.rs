//! A ready problem made of variables and constraints given as functions, for a caller that has
//! no model of its own: add the variables, add the constraints, solve, read the variables.

use crate::{degrees_of_freedom, solve, Problem, Settings, SolverError, Verdict};

/// A variable of a [`System`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarId(pub usize);

/// A constraint of a [`System`]; also the index that the verdict names it by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConstraintId(pub usize);

type Rows = Box<dyn Fn(&[f64]) -> Vec<f64>>;

struct Entry {
    name: String,
    rows: usize,
    f: Rows,
    enabled: bool,
}

/// Variables and constraints.
#[derive(Default)]
pub struct System {
    values: Vec<f64>,
    constraints: Vec<Entry>,
}

struct Built<'a> {
    n: usize,
    active: Vec<&'a Entry>,
    owners: Vec<Option<usize>>,
}

impl Problem for Built<'_> {
    fn unknowns(&self) -> usize {
        self.n
    }

    fn residuals(&self, x: &[f64]) -> Vec<f64> {
        let mut out = Vec::with_capacity(self.owners.len());
        for e in &self.active {
            let mut rows = (e.f)(x);
            rows.resize(e.rows, f64::NAN);
            out.extend(rows);
        }
        out
    }

    fn owners(&self) -> &[Option<usize>] {
        &self.owners
    }
}

impl System {
    /// A system with nothing in it.
    pub fn new() -> System {
        System::default()
    }

    /// Adds a variable with the value it starts from.
    pub fn variable(&mut self, value: f64) -> VarId {
        self.values.push(value);
        VarId(self.values.len() - 1)
    }

    /// The value of a variable now (after a solution, the solution).
    pub fn value(&self, v: VarId) -> Result<f64, SolverError> {
        self.values
            .get(v.0)
            .copied()
            .ok_or_else(|| SolverError::Unknown(format!("variable {}", v.0)))
    }

    /// The number of variables.
    pub fn variables(&self) -> usize {
        self.values.len()
    }

    /// The number of constraints, the disabled ones included.
    pub fn constraints(&self) -> usize {
        self.constraints.len()
    }

    /// Adds a constraint of `rows` residual rows. The function receives all the variables, in
    /// the order they were added, and gives its rows (numbers that are zero when it holds).
    pub fn constraint(
        &mut self,
        name: &str,
        rows: usize,
        f: impl Fn(&[f64]) -> Vec<f64> + 'static,
    ) -> Result<ConstraintId, SolverError> {
        if rows == 0 {
            return Err(SolverError::Residuals(format!("{name}: no rows")));
        }
        self.constraints.push(Entry {
            name: name.to_string(),
            rows,
            f: Box::new(f),
            enabled: true,
        });
        Ok(ConstraintId(self.constraints.len() - 1))
    }

    /// The variable has this value.
    pub fn fix(&mut self, v: VarId, value: f64) -> Result<ConstraintId, SolverError> {
        self.value(v)?;
        self.constraint("fix", 1, move |x| vec![x[v.0] - value])
    }

    /// Two variables are equal.
    pub fn equal(&mut self, a: VarId, b: VarId) -> Result<ConstraintId, SolverError> {
        self.value(a)?;
        self.value(b)?;
        self.constraint("equal", 1, move |x| vec![x[a.0] - x[b.0]])
    }

    /// The point `(bx, by)` is `distance` away from the point `(ax, ay)`.
    pub fn distance(
        &mut self,
        a: (VarId, VarId),
        b: (VarId, VarId),
        distance: f64,
    ) -> Result<ConstraintId, SolverError> {
        for v in [a.0, a.1, b.0, b.1] {
            self.value(v)?;
        }
        self.constraint("distance", 1, move |x| {
            let (dx, dy) = (x[b.0 .0] - x[a.0 .0], x[b.1 .0] - x[a.1 .0]);
            vec![(dx * dx + dy * dy).sqrt() - distance]
        })
    }

    /// Turns a constraint off or on; a disabled one is not in the problem.
    pub fn set_enabled(&mut self, c: ConstraintId, enabled: bool) -> Result<(), SolverError> {
        let e = self
            .constraints
            .get_mut(c.0)
            .ok_or_else(|| SolverError::Unknown(format!("constraint {}", c.0)))?;
        e.enabled = enabled;
        Ok(())
    }

    /// The name a constraint was given.
    pub fn name(&self, c: ConstraintId) -> Option<&str> {
        self.constraints.get(c.0).map(|e| e.name.as_str())
    }

    fn built(&self) -> Built<'_> {
        let mut active = Vec::new();
        let mut owners = Vec::new();
        for (i, e) in self.constraints.iter().enumerate() {
            if e.enabled {
                owners.extend(std::iter::repeat_n(Some(i), e.rows));
                active.push(e);
            }
        }
        Built {
            n: self.values.len(),
            active,
            owners,
        }
    }

    /// Solves the system. The variables are moved only when the verdict is
    /// [`Verdict::Solved`]; in any other case they are left as they were. The constraints named by
    /// the verdict are the [`ConstraintId`] numbers.
    pub fn solve(&mut self, settings: &Settings) -> Result<Verdict, SolverError> {
        let built = self.built();
        let solution = solve(&built, &self.values, settings)?;
        if matches!(solution.verdict, Verdict::Solved { .. }) {
            self.values = solution.x;
        }
        Ok(solution.verdict)
    }

    /// The degrees of freedom where the variables are now, without moving anything.
    pub fn degrees_of_freedom(&self, settings: &Settings) -> Result<usize, SolverError> {
        let built = self.built();
        degrees_of_freedom(&built, &self.values, settings)
    }
}
