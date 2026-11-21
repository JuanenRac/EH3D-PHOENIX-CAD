# The 2D drawing sheets

This page describes `eh3d_phoenix_cad::plans` (the sheet, its views, dimensions, balloons and layers, and its files), the commands `drawing-check` and `drawing-export`, the panel «Planos» of the window and the optional domain `drawing` of the project file. Nothing here has been opened in a drawing program, and the views have been tried on solids made by the tests only.

## What a sheet is

A **sheet** is a format of paper (the ISO series A4 to A0 and any format the settings add), an orientation, a scale (a ratio `n:d`, or `auto`) and a projection (third angle or first angle). On it there are:

* **views** of the solids of the history, in millimetres on the sheet: the **front** (from the side of the lowest `y`: `x` to the right, `z` up), the **top** (`x` to the right, `y` up), the **right** (from the side of the highest `x`: `y` to the right, `z` up) and the **isometric** (from the front, the right and above), each with its visible edges and, when the view says so, its hidden ones; and the **sketch** view, a sketch of the project drawn in its own plane. A view looks at the result of the history (`source tip`), at the solid of one node (`source node ID`) or at a sketch (`source sketch NAME`).
* **dimensions** tied to the model (below).
* **balloons** tied to the bill of materials (below).
* a **title block** and the **layers** the lines are drawn on.

The top and the side views are aligned with the front one by the coordinates of the world they share: in the third angle the top is over the front and the side to its right, in the first angle the top is under it and the side to its left; the isometric view takes the free corner of the third angle; any other view goes in a row to the right. The group is centred in the room the frame leaves. A view can be put by hand (`at X Y`, the lower left corner of its box in micrometres). With the scale `auto` the largest of the standard scales of the settings at which the views fit is taken; with a fixed scale a view that does not fit the frame is a problem (`PHOENIX-CAD-054`).

### The edges

The coordinates of the prisms cut space into a grid. A line of the grid is an edge of the solid where the four cells round it are one inside, three inside, or two inside across a diagonal. An axis view finds, for each cell of the picture, how near the first material is; an edge is visible when nothing is nearer on either side of it, and a hidden edge that lies on a visible one is left out. The isometric view tests points of each edge against the prisms along the line of sight (the direction `1 -1 1`) and finds where the answer changes to the nanometre. The work is limited by `sheet.max_cells`, `sheet.max_edges` and `sheet.max_iso_tests`; beyond them the view is refused with `PHOENIX-CAD-053`.

## Associative dimensions

A dimension names the things of the model it measures by ids that do not change, and its value is worked out from the model every time the sheet is evaluated, so editing a parameter of the history changes the value and the drawing and nothing else has to be done.

| Kind | References | Value |
|---|---|---|
| `linear` | two faces of a node, across the same axis | the distance between them |
| `cumulative` | three or more faces across the same axis; the first is the datum | the distance from the datum to each of the others, stacked |
| `radius`, `diameter` | a circle or an arc of a sketch | the radius, or twice it |
| `angle` | two lines of a sketch | the angle between their directions, from 0 to 180 degrees |

A face is `ref face NODE-ID x|y|z min|max`: the lowest or the highest face of the box that holds the solid of the node. An element of a sketch is `ref element NUMBER SKETCH-NAME`. A length is drawn in the front, top and right views (in the one that shows its axis), a radius, a diameter and an angle in a sketch view. The style (text height, arrows, distances, decimals) is in the section `[sheet]` of the settings (`sheet.dim.*`).

**Orphans.** A dimension whose thing is gone (the node was removed or is not a feature, it failed to build or has no solid, the project has no history, the sketch or its element is not in the project) is an orphan: `PHOENIX-CAD-055` with the reason. It is neither drawn with an old value nor dropped. A dimension that cannot be made (faces across different axes, lines that are parallel, a kind of dimension that its view cannot show, a view that could not be made) is `PHOENIX-CAD-056`.

## The bill of materials and the balloons

The bill is derived from the history. The parts are the boxes that end up in the result: the walk starts at the tip (or at every feature no other one takes), goes through a move to its input, through a union to both inputs and through a difference to the first input only (what is cut away is a tool, not a part); an array of `n` copies multiplies the quantity of what is below it by `n`; an intersection is one part of its own; a suppressed feature gives what its first input gave. A line has the item number, the quantity, the description (the name of the node unless the drawing says otherwise) and the material (`sheet.default_material` unless the drawing says otherwise).

**Stable numbers.** A part is a node and keeps its item number when the history is edited: a number is given the first time a part is seen, in the order the walk finds it, and is kept in the drawing (`item NODE-ID NUMBER`); it is never taken back or given to another part. The balloon (`balloon ID NAME`, `part NODE-ID`, `at`, `to`) shows the number of its part; a balloon whose part is not in the bill is `PHOENIX-CAD-057`. The bill is written as comma separated values (`drawing-export ... --format bom`), safe to open in a spreadsheet, and drawn as a table above the title block (`sheet.bom.*`).

## Layers

The layers are `OUTLINE` (visible edges), `HIDDEN`, `CENTER`, `DIMENSION`, `TEXT`, `BALLOON` and `FRAME`, with a colour, a width, a type of line (continuous, dashed, dash-dot), and whether they are shown and whether they are locked, all from the settings (`sheet.layer.*`; the width is `thick`, `thin` or a length, from `sheet.line_group`). The drawing can change them for itself (`layer NAME visible|locked|color|width VALUE`). A layer that is hidden is left out of every file. A locked layer cannot be changed: a view cannot be added to or taken from a locked `OUTLINE`, a dimension on a locked layer cannot be added or removed, a locked `BALLOON` or `FRAME` refuses balloons and fields of the title block. The type of line follows ISO 128 in spirit (dashes for hidden edges, dash-dot for centre lines).

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
field title Bracket
layer CENTER visible 0
item <node id> 1
part <node id>
material aluminium
description Base plate
view <id> front front
source tip
hidden 1
dim <id> linear width
ref face <node id> x min
ref face <node id> x max
view <view id>
layer DIMENSION
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

The section `[sheet]` of `phoenix-cad-drawing.default.cfg` (printed by `eh3d-phoenix-cad config`, read from the file given with `--config`) holds every tuned value: the formats (`sheet.format.NAME = 210 mm x 297 mm`, short side by long side), the default format, orientation and scale, the standard scales, the projection, the margin and the gap between views, the style of the dimensions, the balloons, the title block, the table of the bill, the layers and the limits (`sheet.max_file_bytes`, `max_views`, `max_dims`, `max_balloons`, `max_parts`, `max_extra_lines`, `max_extra_bytes`, `max_text`, `max_cells`, `max_edges`, `max_iso_tests`).

## Limits of what is done

* The views are of solids of aligned prisms; there are no sections, details or broken views, no hatching, no tolerances on a dimension and no ordinate or chain dimensions yet.
* The angle of a dimension and the isometric projection use floating point, rounded to a micrometre on the sheet; the same program on another system could differ in the last micrometre.
* The extension lines start at the box of the node, not at the edge of the face; the dimension line is placed at a distance from the view, not where a person drags it.
* The PDF uses the font Helvetica of the viewer; the Japanese and Chinese words of the title block have no glyph in it and are written as question marks (the SVG and the DXF carry them).
* Nothing has been compared with a drawing program; every state in `REQUIREMENTS.md` stays `prototype` and `evidence=simulator`.
