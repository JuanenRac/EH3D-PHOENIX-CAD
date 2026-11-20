# 0013: the numeric engine of the sketch solver is a crate of its own

Status: accepted.

## Context

The solver of the sketches was a module of the model with its numeric part (damped least squares, the rank of the constraints, the redundant and conflicting ones) mixed with the rows of each constraint. A solver that others can rely on needs a contract that does not change when the model does.

## Decision

1. The engine is the crate `eh3d-phoenix-cad-solver`, with no dependency. Its contract: a `Problem` (unknowns, residual rows, the owner of each row), `solve`, `degrees_of_freedom`, a `Verdict` (solved with the degrees of freedom left, over-constrained with the redundant and the conflicting constraints, or not converged) and `Settings` read from a configuration file. `System` is a ready problem for callers that have no model of their own.
2. The engine uses no function of the mathematical library, no thread and no random number, so the same problem gives the same answer bit for bit.
3. The residual rows of each constraint of the sketch stay in the model (`sketch/solver.rs`), which calls the engine. The tests of the sketch are the regression tests of the engine; the engine has its own tests with a fixed seed.
4. The tolerances, the damping and the limits are settings, not constants.

## Consequences

* A change of the sketch (a new kind of constraint) does not touch the engine.
* The engine can be used by another program with its own residuals.
* The reasons the engine refuses a problem have the codes `SKSOLVER-001` to `SKSOLVER-004`; the sketch turns them into `PHOENIX-CAD-011` and `PHOENIX-CAD-007`.
