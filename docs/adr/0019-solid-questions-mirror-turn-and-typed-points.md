# 0019: questions about a solid, mirror and turn, and typed points

Status: accepted.

## Context

The history of features of record 0012 builds solids of prisms (record 0014) but nothing could ask a solid how thick its walls are, how large its surface is or where its centre of mass lies, the history had no way to repeat a part symmetrically or turn it, the solid could only leave the program as a glTF file, and a sketch could only be given points by the pointer. A design for fabrication needs the first, a symmetric part needs the second, a slicer wants an STL file, and a person typing a sketch wants to give a point as a distance and an angle. All of it must stay exact, must not link any outside library and must read no code of another program.

## Decision

1. **One grid for every question.** `solidquery::Grid` is the grid that the planes of the prisms cut space into, with a flag for every cell, limited by `history.max_cells` like a boolean (`PHOENIX-CAD-053`). Thickness is the length of a run of cells along an axis, the surface is the faces of the grid between a cell with material and one without, and the centre of mass is a sum of exact 128-bit products with checked arithmetic; a sum that would not fit is refused and never wraps. Thickness is along the three axes only: a slanted line needs more than the grid and is written down in `docs/NOT_YET.md`.
2. **The verification fails on a thin wall.** `history-measure --min-wall` exits with 2 when a run is strictly thinner than the limit, with 0 when none is, and with 1 when it could not run, like every check of the suite. A bare number is in millimetres.
3. **`mirror` keeps the input.** The feature gives the input together with its reflection, which is the common case (a symmetric bracket); the reflection alone is a `difference` of the two. Both operations are quarter turns and planes across the axes, so the boxes stay aligned and nothing is rounded. They are added at the end of the list of operations, so a history written before them reads as it did, and a program that does not know them refuses a history that uses them (the domain is critical).
4. **The STL file is the surface.** Only the faces with material on one side are written, joined greedily in a fixed order; the volume and the area of the rectangles are exact and the tests compare them with the solid. T-junctions are accepted and written down; welding them needs a second pass that is left for when a slicer has opened a file.
5. **Typed points are logic.** `coordentry` reads `x,y`, `@dx,dy`, `r<a` and `@r<a`; a number without a unit is in millimetres; a quarter turn is exact and any other angle is rounded half away from zero to the nanometre. The failures reuse `PHOENIX-CAD-008` and `PHOENIX-CAD-001`, so there is no new code. The command `sketch-points` exposes it without a window; the entry line of the window is left for when the window has a place for it.
6. **Readers are tested on damage.** A single test damages every file the library writes with a fixed seed and gives it to every reader; the files of other programs are not part of it.

## Consequences

* Nothing in the project file changes: the history gets two more operation names and the other files are the same bytes as before.
* The answers are exact for the model and say nothing about a real machine: a wall that passes the check is not a promise that it can be made.
* Nothing has been compared with a CAD program, a slicer or a manufacturing check: the state stays `prototype` and `evidence=simulator`.
