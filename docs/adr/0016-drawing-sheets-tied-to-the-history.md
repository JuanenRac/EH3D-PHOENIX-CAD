# 0016: drawing sheets are tied to the history by ids, and the bill and the numbers come from the history

Status: accepted.

## Context

The next step after the history of features is the 2D drawing: views, dimensions, balloons and a bill. A drawing that copies numbers out of the model goes stale as soon as a parameter is edited, and a bill typed by hand does not follow the design. The history already has ids that do not change (`featuredag`) and a rebuild that only does what an edit touches.

## Decision

1. A sheet is kept in its own optional domain `drawing` of the project file, in one canonical text with a version and the lines it does not know kept where they were. The history and the sketches are not changed by it, and a program that does not know the domain keeps it byte for byte.
2. A dimension keeps **references**, never values: a face of the box of the solid of a node (node id, axis, low or high), or an element of a sketch (sketch name, element number). Its value is worked out from the rebuilt model every time. A reference whose thing is gone makes the dimension an **orphan** (`PHOENIX-CAD-055`), reported with the reason; it is not drawn with an old value and not silently dropped. Radius, diameter and angle are tied to the elements of the sketches because the prism solids have no curves and the sketches are the only place the project keeps circles, arcs and lines with ids.
3. The bill of materials is derived from the history by a fixed walk, never typed: the boxes that end up in the result, with the quantity multiplied by the arrays above them, what is cut away left out. The description and the material of a part are the only things the drawing says about it. The item number of a part is given the first time the part is seen and kept in the drawing; it is never taken back or given again. A balloon is tied to the node of its part; a balloon whose part is not in the bill is `PHOENIX-CAD-057`.
4. A view is drawn from the exact edges of the prism solid: an edge is a line of the grid with an odd number of cells round it or a diagonal pair; visible and hidden are told with integer depth for the front, top and right, and by testing the line of sight, to the nanometre, for the isometric view. The work is limited by settings and refused beyond them (`PHOENIX-CAD-053`).
5. Every tuned value is in the section `[sheet]` of a settings file: formats, scales, the style of the dimensions, the title block, the layers and the limits. There is one view of the drawing, with no beginner and advanced modes.
6. The files are SVG, PDF (the writer that already draws the sheets of the box) and a simple DXF, deterministic, and the bill as comma separated values. No dependency is added.
7. `drawing-check` has the exit codes of every check of the suite (0 passes, 2 the verification did not pass, 1 it could not run); `drawing-export` writes nothing while the check does not pass unless it is forced.

## Consequences

* Editing a parameter changes the dimensions, the views and the bill with no further step, and an edit that removes a node shows up as an orphan.
* A drawing says nothing about a face or an edge by position: a dimension tied to the node follows it when it moves, and cannot follow a face that is created or split by an operation (the face of a node is the face of the box of its solid).
* Sections, details, hatching, tolerances and chain dimensions are not part of this record; they need a record of their own.
* The first state of the drawing is `prototype` with `evidence=simulator`: nothing has been opened in a drawing program.
