# The history of features as a graph, and the solids of aligned prisms

This page describes `eh3d_phoenix_cad::featuredag` (the history), `eh3d_phoenix_cad::prism` (the solids and their booleans), `eh3d_phoenix_cad::glb` (the glTF file), the command `history-check` and the crate `eh3d-phoenix-cad-solver` (the numeric engine of the sketch). The decisions behind them are the records 0012 to 0015 in `docs/adr/`. Everything here has only been tried on histories made by its own tests; no CAD program has opened anything it writes.

## The history

A history is a set of **nodes** that depend on one another as a graph, not as a list.

* A **parameter** is a name with an expression of the language of [EXPRESSIONS.md](EXPRESSIONS.md): `wall = 2 mm`, `width = 3 * wall + 10 mm`. It may name other parameters and the constants of the settings.
* A **feature** is an operation with the nodes it takes as inputs, in order, and an expression for each of its numbers. The operations are:

| Operation | Inputs | Parameters | Gives |
|---|---|---|---|
| `box` | 0 | `x y z sx sy sz` (lengths) | the box with its lowest corner at `x y z` and sizes `sx sy sz` |
| `move` | 1 | `dx dy dz` (lengths) | the input moved |
| `array` | 1 | `count` (a whole number), `dx dy dz` | the input and `count - 1` copies, each one step further than the one before |
| `union` | 2 | none | what is in either input |
| `difference` | 2 | none | the first input without the second |
| `intersection` | 2 | none | what is in both inputs |

A feature depends on its inputs and on the parameters its expressions name; a parameter depends on the parameters its expression names. The history can name one feature as its **tip**, the one that gives the result; without a tip the result is the only feature that no other one takes.

* **Order.** The order of calculation is the one of the graph of dependencies (`graph.rs`): a node after everything it depends on and, among the nodes that could come next, the one with the smallest id first. The same history is always built in the same order, whatever the order in which it was written (`FeatureDag::order`). `FeatureDag::to_dot` writes the graph for Graphviz.
* **Incremental rebuild.** `FeatureDag::rebuild` calculates only what changed: a node whose own definition and whose inputs are those of the last time is reused. Editing one parameter builds again that parameter and what depends on it, nothing else (`Rebuild::computed` and `Rebuild::reused` say how many).
* **Cycles.** An edit that would make a node depend on itself, directly or through others, is refused with `PHOENIX-CAD-013`. Every edit is checked as a whole and, if it is refused, the history is exactly as it was.
* **Errors by node.** A node whose operation fails is reported with a code and a text in the eight languages: `PHOENIX-CAD-052` for a size that is not positive, a count that is not allowed or a result outside the range of lengths; the code of the expression (`PHOENIX-CAD-023` to `026`) when a formula cannot be calculated; `PHOENIX-CAD-053` for a solid beyond the limits. The nodes that depend on it are reported as inheriting the failure, with the same code, and the rest of the model is built.
* **Suppressed features.** A suppressed feature is skipped: it gives what its first input gave (nothing when it has none) and nothing is deleted.
* **Removing.** A node that another one uses cannot be removed (`PHOENIX-CAD-016`).

## The file

The history is the critical domain `history` of the project file (`Document::set_history`, `Document::history`). A program that does not know the domain opens the project read-only. The text is written in one canonical form, so the same history gives the same bytes and a text that is not in that form is refused:

```text
history 1
tip <id>
parameter <id> <name> <expression>
feature <id> <operation> <name>
input <id>
param <key> <expression>
suppressed 1
x-<anything>
```

The nodes are listed by id; the ids are UUID v7 and never change, so a node keeps its identity when it is renamed or moved. The expressions are written in their canonical form, the keys of a feature by name. A line that starts with `x-` belongs to a newer program: it is read, kept in the record it follows and written back where it was. A damaged file is `PHOENIX-CAD-051` with the number of the line; a cycle in a file is `PHOENIX-CAD-013`. The format is plain text, as every domain of this project is: the canonical JSON of the shared container is not used here.

## The solids

`prism.rs` keeps a solid as a set of boxes aligned with the axes, `[min, max)` in whole nanometres, that do not overlap, in a **canonical form**: the same solid is always the same list of boxes, so two solids are equal when `==` says so. The three booleans are exact and use no library: the coordinates of both operands cut space into a grid, each cell is inside or outside each operand, the operation is a rule on the two answers, and the cells that are inside are put back together after the planes where nothing changes have been taken out. The work grows with the number of cells, so it is limited by `history.max_cells` and `history.max_prisms` (`PHOENIX-CAD-053`). The volume is an exact whole number of cubic nanometres.

What this does not do: a shape that is not a box (a cylinder, a rounded edge) has no representation here, and nothing in this module is written to a STEP file or a mesh yet. The rule of the geometry kernel is in the record 0014.

## The glTF file

`glb.rs` writes a solid of prisms as a binary glTF file in the profile `phoenix-cad-glb/1` (`eh3d-phoenix-cad history-check PROJECT --glb OUT.glb`): one scene, one node, one mesh of triangles, positions in `f32` metres, flat outward normals (each prism on its own, four vertices for each of its six faces), `u32` indices, the Z axis of the model turned into the Y axis of the viewers by the rotation of the node, the profile, the unit and the axes in the `extras`, no date, no host name and no program version, the JSON chunk padded with spaces and the binary one with zeros. The same solid always gives the same bytes. `check_glb` reads a file of the profile back (the header, the two chunks, the counts of the accessors against the binary chunk, the indices) and gives the vertices, the triangles, the bounds and the volume of the triangles, which the tests compare with the exact volume of the solid; bytes that are not a file of the profile are `PHOENIX-CAD-018`, never a panic. A file is never written over one that exists unless `--overwrite` is given. No viewer has opened a file this program wrote.

## The settings

The section `[history]` lives in `phoenix-cad-history.default.cfg` (printed by `eh3d-phoenix-cad config`, read from the file given with `--config`):

| Key | Meaning |
|---|---|
| `max_nodes` | the most nodes of a history |
| `max_file_bytes` | the largest history file that is read |
| `max_array` | the most copies of an array |
| `max_extra_lines`, `max_extra_bytes` | the lines of a newer program that a node keeps, and how long they may be |
| `max_cells`, `max_prisms` | the limits of an operation between solids |

## The command

`eh3d-phoenix-cad history-check PROJECT [--text] [--lang CODE] [--config FILE]` builds the history of a project and reports every node (`node,name,state,code,detail`, or lines for a person with `--text`). It exits with 0 when every node was built, with **2** when the verification did not pass (a node failed or inherits a failure) and with 1 when it could not run. `history-graph` writes the graph for Graphviz.

## The solver as a crate

`crates/eh3d-phoenix-cad-solver` is the numeric engine of the solver of the sketches, with no dependency. Its contract is small: a `Problem` gives the number of unknowns, the residual rows and who owns each row; `solve` returns the unknowns it ended at and a `Verdict` (solved with the degrees of freedom left, over-constrained with the redundant and the conflicting constraints and the earlier ones each depends on, or not converged); `degrees_of_freedom` counts them without moving anything; `System` is a ready problem made of variables and constraints given as functions. The tolerances, the damping and the limits are `Settings`, read from `phoenix-cad-solver.default.cfg`; the engine uses no function of the mathematical library, so the same problem gives the same answer bit for bit. The sketch of this repository (`sketch/solver.rs`) builds its residual rows and calls the engine; the tests of the sketch are the regression tests of the engine as well.
