# The 2D drawing sheets

This page describes `eh3d_phoenix_cad::plans` (the sheet, its views, dimensions, balloons and layers, and its files), the commands `drawing-check` and `drawing-export`, the panel «Planos» of the window and the optional domain `drawing` of the project file. Nothing here has been opened in a drawing program, and the views have been tried on solids made by the tests only.

## What a sheet is

A **sheet** is a format of paper (the ISO series A4 to A0 and any format the settings add), an orientation, a scale (a ratio `n:d`, or `auto`) and a projection (third angle or first angle). On it there are:

* **views** of the solids of the history, in millimetres on the sheet: the **front** (from the side of the lowest `y`: `x` to the right, `z` up), the **top** (`x` to the right, `y` up), the **right** (from the side of the highest `x`: `y` to the right, `z` up) and the **isometric** (from the front, the right and above), each with its visible edges and, when the view says so, its hidden ones; and the **sketch** view, a sketch of the project drawn in its own plane (a **section** or a **detail** is a view of the front, top, right or isometric kind that says more, below). A view looks at the result of the history (`source tip`), at the solid of one node (`source node ID`) or at a sketch (`source sketch NAME`).
* **section and detail views** of other views (below).
* **dimensions** tied to the model (below).
* **balloons** tied to the bill of materials (below).
* a **title block** and the **layers** the lines are drawn on.

The top and the side views are aligned with the front one by the coordinates of the world they share: in the third angle the top is over the front and the side to its right, in the first angle the top is under it and the side to its left; the isometric view takes the free corner of the third angle; any other view goes in a row to the right. The group is centred in the room the frame leaves. A view can be put by hand (`at X Y`, the lower left corner of its box in micrometres). With the scale `auto` the largest of the standard scales of the settings at which the views fit is taken; with a fixed scale a view that does not fit the frame is a problem (`PHOENIX-CAD-054`).

### Sections and details

A **section** is a view of the front, top or right kind with a cut: `section PARENT LETTERS POSITION`. The plane is across the axis the view looks along (`y` for the front, `z` for the top, `x` for the right) at the position given in nanometres. The half of the solid that lies away from the viewer stays (for the front view the part with `y` above the cut, for the top and the right the part with `z` or `x` below it), its edges are the edges of the view, and the faces the plane leaves open, the **cut faces**, are hatched. The **line of the cut** is drawn in the view named by `PARENT`: a chain line over the whole view and a little past it (`sheet.section.overshoot`), a thick mark at each end (`end_length`), a stem with an arrow that shows where the cut is looked at from (`arrow_length`) and the letters (one to three capitals or digits, used once on the sheet; the next free letters, skipping `I`, `O` and `Q`, are `A`, `B`...). The caption `A-A` goes under the view. The parent has to show the axis of the cut across its picture: a cut across `x` is drawn in the front or the top view, across `y` in the top, across `z` in the front or the right one; the isometric and the sketch views show none (`PHOENIX-CAD-059`). A plane that does not cut the solid, or leaves nothing of it, is a problem of the view, and the rest of the sheet is drawn.

The **hatching** is made of parallel lines at an angle from the first axis of the view and a distance apart that are measured on the sheet (`sheet.hatch.angle`, `sheet.hatch.spacing`), numbered from the origin of the sheet so that two views of one part are hatched alike, joined where two cut faces touch and broken by every hole. Each material has its own angle and distance (`sheet.hatch.materials` and `sheet.hatch.material.NAME.*`); the material is the one named by the line `hatch MATERIAL` of the view, or the material of the part when the view looks at one node, or else the default style. A hatching of more lines than `sheet.hatch.max_lines` is a problem of the view. The lines are on the layer `HATCH`, the marks on `SECTION`, the thick marks of the cut on `OUTLINE`, the letters and the captions on `TEXT`.

A **detail** is a view that is the part of another view inside a circle or a rectangle, drawn larger: `detail PARENT LETTER circle U V RADIUS` or `detail PARENT LETTER rect U V WIDTH HEIGHT`, the centre in nanometres in the picture of the parent, which has to be a view of a solid that comes before it and has the same kind. The lines of the parent are cut at the shape; the shape is drawn as the border of the detail and again in the parent, with its letter, on the layer `DETAIL`; the scale of the detail is the `scale` of the view, and the caption is `B (4:1)`. A section and a detail are not the views of the projection: they are not aligned with the others (they go in the row to the right, or where `at` puts them). A **broken view** is not done (`docs/NOT_YET.md`).

### The edges

The coordinates of the prisms cut space into a grid. A line of the grid is an edge of the solid where the four cells round it are one inside, three inside, or two inside across a diagonal. An axis view finds, for each cell of the picture, how near the first material is; an edge is visible when nothing is nearer on either side of it, and a hidden edge that lies on a visible one is left out. The isometric view tests points of each edge against the prisms along the line of sight (the direction `1 -1 1`) and finds where the answer changes to the nanometre. The work is limited by `sheet.max_cells`, `sheet.max_edges` and `sheet.max_iso_tests`; beyond them the view is refused with `PHOENIX-CAD-053`.

## Associative dimensions

A dimension names the things of the model it measures by ids that do not change, and its value is worked out from the model every time the sheet is evaluated, so editing a parameter of the history changes the value and the drawing and nothing else has to be done.

| Kind | References | Value |
|---|---|---|
| `linear` | two faces of a node, across the same axis | the distance between them |
| `cumulative` | three or more faces across the same axis; the first is the datum | the distance from the datum to each of the others, stacked |
| `chain` | three or more faces across the same axis | the distance from each face to the next one, on one line, with an arrow at each end of each run |
| `ordinate` | three or more faces across the same axis; the first is the origin | the coordinate of each of the others from the origin, written at the end of its extension line under a line with no arrows; the origin is written `0` |
| `radius`, `diameter` | a circle or an arc of a sketch | the radius, or twice it |
| `angle` | two lines of a sketch | the angle between their directions, from 0 to 180 degrees |

A face is `ref face NODE-ID x|y|z min|max`: the lowest or the highest face of the box that holds the solid of the node. An element of a sketch is `ref element NUMBER SKETCH-NAME`. A length is drawn in the front, top and right views (in the one that shows its axis), a radius, a diameter and an angle in a sketch view. The style (text height, arrows, distances, decimals) is in the section `[sheet]` of the settings (`sheet.dim.*`).

The kind `cumulative` is the parallel (baseline) dimension: every distance from the datum, one line for each, stacked `sheet.dim.step` apart (or the `step` of the dimension).

**Placing.** A dimension with no `offset` is put by the sheet: the dimensions of one view that run along the same side are taken in the order of the file and each goes on the first line, counted outward from the view, where it does not meet another along its line (with `sheet.dim.clearance` of room between them), so that dimensions that overlap are stacked and the ones that do not share the line; at most `sheet.max_auto_dims` of a sheet are placed this way. A dimension with an `offset` stays where it was put.

**Dragging.** The picture of the sheet in the panel takes the pointer: pressing on a dimension line takes it (`PlanState::begin_drag`, the nearest line under the pointer within two millimetres), moving drags it and the `offset` follows the pointer on a multiple of `sheet.dim.drag_snap` within a metre (`drag_to`); a locked layer refuses. The logic, `DimLine::hit` and `DimLine::offset_for`, is tested without a window; the drawing of the pointer in the window has not been opened on a screen.

**Tolerances.** A dimension of lengths (not an angle) can carry a tolerance, written next to every value: `tol class H7`, `tol class g6` or a fit `tol class H7/g6` (a hole over a shaft), worked out from the table of `iso286` at the value of the dimension, or `tol dev UPPER LOWER` in nanometres. The text follows `sheet.dim.tol_style` (`class`: `25.00 H7`; `deviation`: `25.00 +0.021/0`, a fit `... | -0.007/-0.020`; `both`: `25.00 H7 (+0.021/0)`) with `sheet.dim.tol_decimals` decimals, and `+-` when the two deviations are alike. A class that is not in the table, a hole over a hole, a class on an angle, or deviations the wrong way round are `PHOENIX-CAD-058` when the dimension is set or read; a combination only the size can tell (a `k` above IT7, a size beyond 500 mm or not above nothing) makes the dimension a problem of the evaluation with the same code, and it is not drawn. The table that ships says `verified = false`, and the panel says so in a note until a checked table replaces it.

**Orphans.** A dimension whose thing is gone (the node was removed or is not a feature, it failed to build or has no solid, the project has no history, the sketch or its element is not in the project) is an orphan: `PHOENIX-CAD-055` with the reason. It is neither drawn with an old value nor dropped. A dimension that cannot be made (faces across different axes, lines that are parallel, a kind of dimension that its view cannot show, a view that could not be made) is `PHOENIX-CAD-056`.

## The bill of materials and the balloons

The bill is derived from the history. The parts are the boxes that end up in the result: the walk starts at the tip (or at every feature no other one takes), goes through a move to its input, through a union to both inputs and through a difference to the first input only (what is cut away is a tool, not a part); an array of `n` copies multiplies the quantity of what is below it by `n`; an intersection is one part of its own; a suppressed feature gives what its first input gave. A line has the item number, the quantity, the description (the name of the node unless the drawing says otherwise) and the material (`sheet.default_material` unless the drawing says otherwise).

**Stable numbers.** A part is a node and keeps its item number when the history is edited: a number is given the first time a part is seen, in the order the walk finds it, and is kept in the drawing (`item NODE-ID NUMBER`); it is never taken back or given to another part. The balloon (`balloon ID NAME`, `part NODE-ID`, `at`, `to`) shows the number of its part; a balloon whose part is not in the bill is `PHOENIX-CAD-057`. The bill is written as comma separated values (`drawing-export ... --format bom`), safe to open in a spreadsheet, and drawn as a table above the title block (`sheet.bom.*`).

## Revisions and title blocks

The table of revisions (`revision NUMBER`, `date TEXT`, `description TEXT`, `author TEXT`) is drawn at the upper right corner of the frame, the oldest first, and the views leave room for it (`sheet.revisions.*`: row, width, the most revisions, and whether the mark is a letter, `A`, `B` ... `AA`, or a number). The date and the author are free texts that the program never fills in or reads (no clock), and may be empty. A number is given when the revision is added and is never given again, even when a revision is taken away (`nextrevision`); a table out of order, a text of more than one line or an empty description is `PHOENIX-CAD-060`.

The settings can hold more than one title block: `sheet.titleblock.*` is `default` and `sheet.titleblock.templates` names others, each with `sheet.titleblock.template.NAME.width|row|columns|fields`; `sheet.titleblock.template` says which a new sheet has, and a sheet chooses its own with `titleblock NAME`.

## Layers

The layers are `OUTLINE` (visible edges), `HIDDEN`, `CENTER`, `DIMENSION`, `TEXT`, `BALLOON`, `FRAME`, `SECTION`, `HATCH` and `DETAIL`, with a colour, a width, a type of line (continuous, dashed, dash-dot), and whether they are shown and whether they are locked, all from the settings (`sheet.layer.*`; the width is `thick`, `thin` or a length, from `sheet.line_group`). The drawing can change them for itself (`layer NAME visible|locked|color|width VALUE`). A layer that is hidden is left out of every file. A locked layer cannot be changed: a view cannot be added to or taken from a locked `OUTLINE`, a dimension on a locked layer cannot be added or removed, a locked `BALLOON` or `FRAME` refuses balloons and fields of the title block. The type of line follows ISO 128 in spirit (dashes for hidden edges, dash-dot for centre lines).

**Groups of lines.** The thick line of the sheet is one of the groups of the standard in `sheet.line_groups` (0.25, 0.35, 0.5, 0.7, 1, 1.4 and 2 mm); the thin line is half of it and the extra thick one (`extra`) twice. A layer whose width is `thick`, `thin` or `extra` follows the group, so a sheet that says `linegroup 500` is drawn with the 0.5 mm group whatever the settings say; a width in micrometres stays. The pattern of the broken lines (the dash and the gap, and for the dash-dot the dot and the second gap, in tenths of the width of the line) is `sheet.linetype.dashed` and `sheet.linetype.dashdot`.

## The title block

`sheet.titleblock.*` says which fields it has (`organisation title number revision author scale sheet material` by default), in how many columns, how wide and how tall, and the values the fields have unless the drawing says otherwise (`field KEY TEXT`). The words that name the fields are words of the window in the language asked for (`--lang`), a field with another name shows its key; `scale` and `sheet` are worked out. A value goes in as it is typed, in any language.

## The file: the domain `drawing`

The domain is optional (a program that does not know it keeps it byte for byte) and its text is in one canonical form with a version; the lines it does not know, those that start with `x-`, are kept where they were (in the head, or after the record they belong to). A text that is not in the canonical form is refused (`PHOENIX-CAD-054`), so what is read is what is written. The grammar is in the documentation of `plans::model`.

```text
drawing 1
format A4
orientation landscape
scale auto
projection third
nextitem 2
nextrevision 2
titleblock compact
linegroup 500
field title Bracket
layer CENTER visible 0
item <node id> 1
part <node id>
material aluminium
description Base plate
view <id> front front
source tip
hidden 1
section <view id> A 15000000
hatch steel
detail <view id> B circle 10000000 0 3000000
dim <id> linear width
ref face <node id> x min
ref face <node id> x max
view <view id>
step 9000
tol class H7/g6
layer DIMENSION
revision 1
date 2026-01-05
description First issue
author A. Author
balloon <id> b1
part <node id>
at 50000 50000
to 60000 60000
```

## The commands

`eh3d-phoenix-cad drawing-check PROJECT [--text] [--lang CODE] [--config FILE]` builds the history and the sketches, evaluates the sheet and reports every view, dimension and balloon as comma separated values (`record,name,state,code,detail`) or as lines for a person in the language of `--lang`. Exit 0: the sheet agrees with the model (a project without a sheet passes). Exit 2: the verification did not pass. Exit 1: the command could not run (a file that cannot be read, a drawing that is not one).

`eh3d-phoenix-cad drawing-export PROJECT OUT [--format svg|pdf|dxf|bom] [--create] [--sheet NAME] [--portrait] [--scale auto|N:D] [--force] [--overwrite]` writes the sheet as SVG, PDF (the writer of the drawings of the box, with no dependency) or a simple DXF (R12 text: lines, circles, arcs and texts on the layers), or the bill as comma separated values (the format follows the extension when `--format` is not given). The files are the same bytes for the same project: no date, no host, no version. It writes nothing while the check does not pass (exit 2) unless `--force` is given, and never over a file that exists unless `--overwrite` is given. `--create` makes the four views of the result of the history for a project that has no sheet; the project file is not touched.

## The panel «Planos»

The window has a panel `drawing` (title in the eight languages) in the arrangement of the Model and Review workspaces and in the menu Tools. It reads the sheet of the project that is open: the head line, the views, the dimensions with their values, the balloons, the bill, the layers (shown and locked from check boxes), the problems in red, and a small picture of the sheet. The buttons create the sheet from the history, update it from the model, keep it in the project file (the numbers of the items are fixed then; the other domains are written back as they are) and write it as SVG next to the project. The lines and the actions are `plans::panel`, tested without a screen. An arrangement saved before the panel existed loads whole and the panel is brought back from the menu. The window has never been opened on a screen.

## The settings

The section `[sheet]` of `phoenix-cad-drawing.default.cfg` (printed by `eh3d-phoenix-cad config`, read from the file given with `--config`) holds every tuned value: the formats (`sheet.format.NAME = 210 mm x 297 mm`, short side by long side), the default format, orientation and scale, the standard scales, the projection, the margin and the gap between views, the style of the dimensions, the balloons, the title block, the table of the bill, the groups of lines and the pattern of the broken lines, the hatching (`sheet.hatch.*`), the marks of sections and details (`sheet.section.*`), the tolerances and the placing and dragging of dimensions (`sheet.dim.tol_style`, `tol_decimals`, `clearance`, `drag_snap`), the table of revisions (`sheet.revisions.*`), the other title blocks, the layers and the limits (`sheet.max_file_bytes`, `max_views`, `max_dims`, `max_auto_dims`, `max_balloons`, `max_parts`, `max_extra_lines`, `max_extra_bytes`, `max_text`, `max_cells`, `max_edges`, `max_iso_tests`).

## Limits of what is done

* The views are of solids of aligned prisms; there is no broken view, no section along a broken plane, no section across an axis that is not one of the three, and a section or a detail of a sketch view is not made. A section is cut across one axis at a time and its hatching is by material, not by pattern.
* The tolerance is written on one line next to the value (`25.00 H7`), not stacked, and the table of ISO 286 that ships has not been checked against the standard; the sheet and the panel say so.
* The dimension lines are dragged in the picture of the panel; the cut line, the balloons and the views are not dragged, and no symbol or free note can be placed yet.
* The angle of a dimension and the isometric projection use floating point, rounded to a micrometre on the sheet; the same program on another system could differ in the last micrometre.
* The extension lines start at the box of the node, not at the edge of the face.
* The PDF uses the font Helvetica of the viewer; the Japanese and Chinese words of the title block and of the table of revisions have no glyph in it and are written as question marks (the SVG and the DXF carry them). Embedding a font would need a font file or a new dependency, which is not done; the PDF stays for the Latin words, and a person who needs the others uses the SVG.
* Nothing has been compared with a drawing program; every state in `REQUIREMENTS.md` stays `prototype` and `evidence=simulator`.
