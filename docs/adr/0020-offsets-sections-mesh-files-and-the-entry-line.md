# 0020: offsets, sections, mesh files and the entry line

Status: accepted.

## Context

The history of features of record 0012 could widen nothing and hollow nothing, a solid could say how thick its walls are but not what a cut through it looks like or how it resists a turn, the surface of a solid could leave the program only as a glTF file or an STL file, and the typed points of record 0019 had no entry line in the sketch window. A box with a lid, a casting with a skin and a part that goes to a slicer all need them. All of it must stay exact where it can, link no outside library and read no code of another program.

## Decision

1. **Offsets are cubes.** `grow` widens every prism by the length on all six sides and joins them; `shrink` is the dual (the space around the solid, in a box one length larger than the solid, is grown and taken away), so it is exact and needs no new kernel; `shell` is the solid less its shrunk self. A cube keeps the boxes aligned with the axes, so nothing is rounded; a round offset would leave the grid and is in `docs/NOT_YET.md`. The length is a parameter like any other and a negative one is a failure of the node (`PHOENIX-CAD-052`).
2. **Laws, not examples.** The tests check the laws of morphology on a seeded family (grown holds the solid, shrunk is held by it, two offsets are one by the sum, closing never loses, opening never gains, shell and shrunk are the solid and share nothing) besides the cases worked by hand, because an offset with a wrong half-open edge would pass every single example and break a law.
3. **Sections and shadows are grid counts.** The area of a cut is the sum of the cells of one slab and the shadow is the columns with material, both whole square nanometres, limited by `max_cells`; the lower face of a prism belongs to it, like `Solid::contains`.
4. **Inertia is floating point and says so.** The exact sums of the second moments of a large solid do not fit in 128 bits, so the tensor is worked out from the exact corners in floating point (each prism about its own centre, then the parallel axis theorem) and its relative error is stated; the tests compare it with the cells of the grid to 1e-9 and check that it does not change when the solid moves.
5. **One surface, five files.** The rectangles of the STL file become a welded mesh, which the writers of the OBJ, 3MF, PLY and OFF files of the library write; each file is read back by the reader of the library and compared with the exact volume. The T-junctions of the STL file stay (record 0019).
6. **The entry line is the same logic.** `SketchEditor::type_point` reads the box with `coordentry`, places the point as the active tool would place a click (a first point of a line waits for the second), measures `@` from the last point placed by a click or by typing, and keeps the text in the box when it was refused. The Select tool draws nothing, so it is told to choose a tool. A typed point is the place typed and does not snap.

## Consequences

* The history gets three more operation names at the end of the list, so a history written before reads as it did and a program that does not know them refuses one that uses them (the domain is critical).
* The state stays `prototype` and `evidence=simulator`: nothing has been compared with a CAD program or a slicer, and the entry line has not been seen in a window.
