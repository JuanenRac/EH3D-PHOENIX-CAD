# 0012: the history of features is a graph of dependencies from the start

Status: accepted.

## Context

A design needs to be edited again: a dimension changes and only what depends on it should be built again. Two shapes were possible for the history: a list of groups that follow one another (simple, with a fixed order) or a graph in which every node says what it depends on.

## Decision

1. The history is a **graph** from the first version (`featuredag.rs`), not a list. A node is a parameter (a name with an expression) or a feature (an operation with ordered inputs and an expression for each number); the dependencies are the inputs and the names that the expressions write.
2. The order of calculation is deterministic: a node after everything it depends on and, among those that could come next, the one with the smallest id first. A cycle is refused with `PHOENIX-CAD-013` and the history stays as it was.
3. A rebuild calculates only what changed. A failure of a node is reported with its code and the nodes that depend on it inherit it; the rest is built.
4. The history is the critical domain `history` of the project file, in one canonical text with a version, stable ids and the lines of a newer program kept as they are.

## Consequences

* Editing a parameter builds again only what depends on it, and the result is the same as building everything again (a property tested with a fixed seed).
* A linear recipe is a graph in which every feature takes the one before it, so nothing is lost by choosing the graph.
* A history that depends on a parameter that no one defines fails with the error of the expression, not silently.
* The recipe of the box (`mechanical`, record 0011) keeps its own domain; the two meet when the box is made from the features of a history, which is not done yet.
