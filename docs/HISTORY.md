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
| `mirror` | 1 | `axis` (a whole number from 0 to 2), `at` (a length) | the input together with its reflection in the plane across the axis (0 for x, 1 for y, 2 for z) at `at` |
| `rotate` | 1 | `axis` (0 to 2), `quarters` (a whole number, negative turns the other way), `cx cy cz` (lengths) | the input turned by that many quarter turns, counter-clockwise seen from the high end of the axis, about the line parallel to it through `cx cy cz` |
| `grow` | 1 | `by` (a length, not negative) | the input grown by that length in every direction: the sum of the solid and a cube, so the corners stay square |
| `shrink` | 1 | `by` (a length, not negative) | the input shrunk by that length: the points whose whole cube of half side `by` is in the solid, so a wall thinner than twice the length disappears |
| `shell` | 1 | `by` (a length, not negative) | the input less its shrunk self: a wall of that thickness along every face with a closed cavity inside, or the whole input when it is too thin to hollow |

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

## Measuring a solid

`solidquery.rs` answers questions about a solid exactly, in whole nanometres, on the grid that the planes of its prisms cut space into (limited by `max_cells`, `PHOENIX-CAD-053`):

* **Thickness.** `thickness_at` gives the run of material that a line parallel to an axis meets at a point, `walls` lists every run along an axis (or only the ones thinner than a limit) with the rectangle of lines that meet exactly that run, and `thinnest` gives the thinnest one. It is measured along the three axes only: a wall that is thin along a diagonal is not found.
* **Surface.** The area counts the faces of the grid that have material on one side and none on the other, so the faces where two prisms touch are not surface and the walls of a closed cavity are.
* **Centre of mass** of a solid of one material: the exact ratio of 128-bit sums, rounded half away from zero to the nanometre; a solid too large for the sums is refused.

* **Section and shadow.** `section_area_nm2` is the area of the cut of the solid by the plane across an axis at a coordinate (the lower face of a prism belongs to it and the upper does not, so a plane through a joint is the section of the prism above it) and `shadow_area_nm2` the area of the shadow along an axis, where a cavity that is closed along the axis does not show. Both are exact whole numbers; the sections of all the slabs of the grid, each times its thickness, add up to the volume, which the tests check.
* **Inertia.** `inertia` gives the tensor of inertia of a solid of one material of density one about its centre of mass, in millimetres to the fifth, with the centre and the volume. It is worked out in floating point from the exact corners (each prism about its own centre, then the parallel axis theorem), so it is deterministic and its relative error is of the order of 1e-12, but unlike the volume and the centre it is not exact: the exact sums would not fit in 128 bits for a solid of a hundred millimetres.

`Solid::mirrored` and `Solid::rotated` are the plain operations behind the features `mirror` and `rotate`; both give the canonical form and refuse a coordinate outside the range of lengths (`PHOENIX-CAD-001`). `Solid::grown`, `Solid::shrunk` and `Solid::shelled` are the ones behind `grow`, `shrink` and `shell`: growing widens every prism by the length and joins them, shrinking grows the space around the solid (a box a little larger than the solid, less the solid) by the same length and takes it away, and the shell is the solid less its shrunk self. They obey the laws of morphology, which the tests check with a fixed seed: the grown solid holds the solid and the shrunk one is held by it, growing or shrinking twice is doing it once by the sum, closing never loses material and opening never gains, and the shell and the shrunk solid together are the solid with nothing in common. A negative length is refused (`PHOENIX-CAD-002`), a coordinate out of the range of lengths `PHOENIX-CAD-001` and a grid larger than `max_cells` `PHOENIX-CAD-053`. The corners are square (a cube, not a ball), so a round offset is not here (`docs/NOT_YET.md`).

## The glTF file

`glb.rs` writes a solid of prisms as a binary glTF file in the profile `phoenix-cad-glb/1` (`eh3d-phoenix-cad history-check PROJECT --glb OUT.glb`): one scene, one node, one mesh of triangles, positions in `f32` metres, flat outward normals (each prism on its own, four vertices for each of its six faces), `u32` indices, the Z axis of the model turned into the Y axis of the viewers by the rotation of the node, the profile, the unit and the axes in the `extras`, no date, no host name and no program version, the JSON chunk padded with spaces and the binary one with zeros. The same solid always gives the same bytes. `check_glb` reads a file of the profile back (the header, the two chunks, the counts of the accessors against the binary chunk, the indices) and gives the vertices, the triangles, the bounds and the volume of the triangles, which the tests compare with the exact volume of the solid; bytes that are not a file of the profile are `PHOENIX-CAD-018`, never a panic. A file is never written over one that exists unless `--overwrite` is given. No viewer has opened a file this program wrote.

## The STL file and the other mesh formats

`history-check PROJECT --export OUT.stl|obj|3mf|ply|off [--overwrite]` writes the same surface in the format of the extension (any case): the STL file below, a Wavefront OBJ, a 3MF package with one model, a binary PLY or an OFF file, all in millimetres, with the vertices welded (the eight corners of a cube are eight vertices) and two triangles to a rectangle, the same bytes for the same solid. The files are read back by the readers of the library, which the tests compare with the exact volume for every format and for a seeded family of solids. A name that does not end in one of the five extensions could not run, and nothing is written over a file that exists unless `--overwrite` is given. None of the files has been opened in a slicer or a CAD program.

`history-check PROJECT --stl OUT.stl [--overwrite]` writes the solid as a binary STL file in millimetres (`stlsolid.rs`): only the faces on the surface, each rectangle of the grid that has material on one side joined to its neighbours of the same side by a greedy pass in a fixed order, so the same solid gives the same bytes. The faces where two prisms touch are not written and the faces of a cavity look into the cavity. The rectangles add up to the exact volume (the sum over the faces across x of the plane times the area) and the exact area of the solid, which the tests compare; the corners are 32-bit floats, so a coordinate beyond about 16 m loses its last nanometres in the file. A rectangle that meets a neighbour along part of its edge leaves a T-junction: a slicer repairs it, a check that wants every edge shared by exactly two triangles would count it. The file has not been opened in a slicer.

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

`eh3d-phoenix-cad history-measure PROJECT [--text] [--min-wall LENGTH] [--section AXIS=POSITION] [--lang CODE] [--config FILE]` builds the history and reports the volume, the surface, the centre of mass, the box, the thinnest wall and the shadow along each axis (`quantity,axis,value,unit` in nanometres and square nanometres, or lines in millimetres for a person with `--text`) and the tensor of inertia about the centre (`inertia,xx|yy|zz|xy|yz|zx` in millimetres to the fifth, density one, floating point). With `--min-wall` it also lists every wall thinner than the length (a bare number is in millimetres) with where it is and how thick, and exits with **2** when there is one. With `--section z=5 mm` it adds the area of the cut by that plane (`section_at` and `section`). `history-check` takes `--stl OUT.stl` beside `--glb OUT.glb`, `--export OUT.ext` for the other mesh formats, and `--overwrite` for all three.

## The solver as a crate

`crates/eh3d-phoenix-cad-solver` is the numeric engine of the solver of the sketches, with no dependency. Its contract is small: a `Problem` gives the number of unknowns, the residual rows and who owns each row; `solve` returns the unknowns it ended at and a `Verdict` (solved with the degrees of freedom left, over-constrained with the redundant and the conflicting constraints and the earlier ones each depends on, or not converged); `degrees_of_freedom` counts them without moving anything; `System` is a ready problem made of variables and constraints given as functions. The tolerances, the damping and the limits are `Settings`, read from `phoenix-cad-solver.default.cfg`; the engine uses no function of the mathematical library, so the same problem gives the same answer bit for bit. The sketch of this repository (`sketch/solver.rs`) builds its residual rows and calls the engine; the tests of the sketch are the regression tests of the engine as well.
