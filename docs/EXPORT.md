# Exporting the model

The model is written to a file from the **Export** menu of the window or with the command `export`. The formats:

| Format | Word | Extension | What it is |
|---|---|---|---|
| Script | `script` | `.scad` | the model as an OpenSCAD-style script |
| STEP | `step` | `.step` | a boundary representation of the box with its straight holes (see the limits in [SCOPE.md](SCOPE.md)) |
| STL | `stl` | `.stl` | the solid as triangles, binary or text |
| OBJ | `obj` | `.obj` | the same solids as a Wavefront file, in millimetres |
| 3MF | `3mf` | `.3mf` | the same solids as a 3MF package (a zip file without compression with the model as XML), in millimetres |
| DXF | `dxf` | `.dxf` | the plan, the front or the side as a drawing |
| SVG | `svg` | `.svg` | the same views, scaled to a page, with dimensions |
| Report | `report` | `.txt` | the fit report |
| Bill | `bill` | `.csv` | the bill of materials of the parts placed in the model |

```
eh3d-phoenix-cad export PROJECT.phoenixproj --format stl|obj|3mf|dxf|svg|step|script|report|bill
                        [--view top|front|side] [--out FILE] [--overwrite] [--ascii|--binary]
```

* `--out` defaults to the name of the project with the extension of the format, in the current folder.
* A file that exists is **never** written over unless `--overwrite` is given (in the window, unless the box of the dialog is ticked); the project that is open and a name that belongs to a project or a sketch are never written over. The check is the one of [`export::check_target`](../crates/eh3d-phoenix-cad/src/export.rs): `PHOENIX-CAD-040` when the file exists, `PHOENIX-CAD-041` for the other refusals.
* `--view` is for `dxf` and `svg` only (`top` is the default); `--ascii` and `--binary` are for `stl` only and override `mesh.stl_binary`.
* The exit code is 0 when the file was written and 1 when it was not; the code of the reason is printed.

## The STL file

`eh3d_phoenix_cad::solid` makes the solid out of the shapes the model already knows and `eh3d_phoenix_cad::mesh` keeps and writes the triangles. The same model always gives the same bytes.

What is in it: the box with its inner space; every hole in its five styles (the straight one, the countersink as a cone, the counterbore with its flat step, the rectangle and the slot); the posts with their foot and their bore; the centring lip of the lid; the extrusions and pockets of the sketches (a pocket through the face opens into the inner space, the islands of an outline stay); the cooling fins; and every part placed from the catalog, in the place its mates give it (profiles solid or hollow, screws with a cylindrical, countersunk or hexagonal head, brackets with their two holes). The vertical edges may be rounded or chamfered.

**One closed surface for the box, one for each part.** The box is not cut out of a block: its outside and its inner space are two surfaces joined only by the walls of the holes, so that every edge is used by exactly two triangles, one in each way. The parts and the turned and swept bodies (`docs/SKETCH.md`) are separate solids in the same file (a text file has one `solid` for each, a binary file has them one after the other); two parts that touch each other are not merged.

**The circles.** Every circle has `mesh.circle_segments` sides and the polygon has the **area** of the true circle (its corners are a little outside the circle), so that a plain hole, a post, a cone or a screw has exactly the volume of the model. The rounded corners of the box and the arcs of a sketch are polygons whose sides are inside the arc.

**The checks.** Before a solid is given out it is checked, and a model that does not pass is refused and nothing is written:

* the surface is closed (`PHOENIX-CAD-049`);
* the volume of the triangles (the sum of the signed tetrahedra) is the exact volume of the model within `mesh.volume_tolerance_ppm` parts per million (`PHOENIX-CAD-049`). The exact volume is worked out from the formulas of the model (the volume of the holes of [`mass`](../crates/eh3d-phoenix-cad/src/mass.rs), the area of the outlines of the sketches, the catalog's volume for every part). With 64 sides the plain pieces agree to about 10^-12 and a box with rounded corners to about 10 parts per million;
* the number of triangles is at most `mesh.max_triangles` (`PHOENIX-CAD-050`), and the file at most `export.max_kib` KiB (`PHOENIX-CAD-041`).

**What the solid cannot be made of** (`PHOENIX-CAD-048`, with the name of the shape and of the one it meets):

* rounded or chamfered edges around the lid or the floor (only the vertical edges can be treated);
* a hole, a pocket through the face or a feature that is not inside the thin part of its face: over the corner columns of the walls, over the floor or the lid of a side face, or crossing the rounded corner;
* an extrusion, a fin or a pocket that touches the edge of its face, or two shapes of one face that cross or touch (a hole under an extrusion, a fin over a hole);
* a post that touches a wall, another post or a hole of the floor;
* a lip that meets a hole of the lid.

There is no test for a shape of one body that passes through another body (a screw through a bracket is allowed, as in the check of the assembly); the check is of each body on its own.

The file has not been opened in a slicer or in a CAD program.

## The OBJ file and the 3MF package

`eh3d_phoenix_cad::interchange` writes the solids that the STL file has (the box and every placed part, each checked against its exact volume, and every turned and swept body, checked to be closed) in two more formats, with the same triangles, in millimetres with six decimals and with the same bytes for the same model. The OBJ file has a group `o` for each solid, its vertices `v` and its faces `f` numbered from 1 over the whole file. The 3MF package holds `[Content_Types].xml`, `_rels/.rels` and `3D/3dmodel.model` (one `object` for each solid and one `item` of the build for each object); its entries are stored without compression and dated 1 January 1980, so the bytes do not depend on a compression library. Both are refused with the codes of the STL file (`PHOENIX-CAD-048` to `-050`) when the solid cannot be made. The 3MF package has not been opened in a slicer or a CAD program; the tests read it back and check every entry against its CRC-32.

## The drawings

The three views look at the box from above (`top`: `x` to the right, `y` up), from the front (`front`: `x` right, `z` up) and from the right (`side`: `y` right, `z` up). Everything is in millimetres.

| Layer | What is on it |
|---|---|
| `CONTORNO` | the outline of the box (the rounded or chamfered corners in the plan), and the bounding rectangles of what stands out of it: extrusions, fins and placed parts; the outline of an extrusion or a fin of the face that the view looks at |
| `AGUJEROS` | the openings of the holes of the face that the view looks at (a countersink or a counterbore as two circles, a slot as two lines and two arcs) and of the pockets of that face |
| `EJES` | the centre lines of the box and of every hole drawn, `drawing.axis_overshoot` past what they mark |

The DXF file is R12 text with the entities `LINE`, `CIRCLE` and `ARC` and the header variable `$INSUNITS` set to millimetres. The SVG file is scaled to `drawing.svg_width`; with `drawing.dimensions = 1` it has the width and the height of the box as dimension lines, the size of every hole as a label (`⌀6`, `⌀3 / ⌀8` for a countersink, `4×10` for a slot) and the scale in the corner. Posts, the lip and the inside of the box are not drawn, because they are not seen from outside. A drawing with more than `drawing.max_entities` entities is refused (`PHOENIX-CAD-050`). Neither format has been opened in a drawing program.

## The hole schedule

`eh3d_phoenix_cad::schedule` lists the holes of the box for the person who drills them: by face (front, back, left, right, top, bottom), then by position `u` and `v` from the lower left corner of the face as seen from outside, each with a tag `H1`, `H2`, ... that follows that order, so the same box always gives the same tags. The command `eh3d-phoenix-cad holes PROJECT` prints `tag,name,face,u_mm,v_mm,diameter_mm,style,detail` (the detail is the head of a countersink, the head and the depth of a counterbore, the height of a rectangle or the length and direction of a slot) and a line `total,N`. With `--tools` it prints the drills and cutters the job needs, `diameter_mm,style,detail,count`, the smallest diameter first. The positions are the nominal ones of the model, without the compensation of the printer (see the model for fabrication).

## A sketch as a flat drawing

`eh3d_phoenix_cad::sketch_export` writes the closed outlines of a sketch (the ones the extrusions and pockets are made from) as a DXF or SVG file for a laser, a router or a plotter: the outer outline of each region on the layer `CONTORNO` and the holes in it on `AGUJEROS`, in millimetres and in the coordinates of the sketch. Lines stay lines, arcs stay arcs (counter-clockwise from the first angle to the second) and circles stay circles; construction geometry and points alone are left out. The command is `eh3d-phoenix-cad sketch-export PROJECT|SKETCH --format dxf|svg [--sketch NAME] [--out FILE] [--overwrite]`: a project with one sketch needs no name, one with several needs `--sketch`; `--out` defaults to the name of the sketch in the current folder. As with every export, a file that exists is never written over unless `--overwrite` is given, nor the project that is open. A sketch with no closed outline is refused with `PHOENIX-CAD-036` and a drawing with more entities than `drawing.max_entities` with `PHOENIX-CAD-050`.

## Settings

In `phoenix-cad.default.cfg` (the `config` command writes them):

| Key | Default | Meaning |
|---|---|---|
| `mesh.circle_segments` | 64 | sides of every circle, a multiple of 4 from 8 to 720 |
| `mesh.max_triangles` | 400000 | the most triangles of one export |
| `mesh.stl_binary` | 1 | 1 writes a binary STL file, 0 a text one |
| `mesh.volume_tolerance_ppm` | 2000 | how far the volume of the triangles may be from the exact one |
| `drawing.max_entities` | 20000 | the most entities of one drawing |
| `drawing.svg_width` | 200 mm | the width of the SVG page |
| `drawing.dimensions` | 1 | dimensions and labels in the SVG drawing |
| `drawing.axis_overshoot` | 3 mm | how far the centre lines go past what they mark |

### The title block

With `title_block.enabled = 1` the DXF and SVG drawings of the views and of a sketch have a title block under the drawing, aligned to its right edge: a table of four rows with the organisation, the title, the number of the drawing with its revision, and the author with the scale. The names of the cells are words of the window in the language of the window (or of `--lang`); the values are the ones typed in the settings and go in as they are, in any language. In the DXF file the block is on its own layer `ROTULO` as lines and `TEXT` entities, and a character that is not printable ASCII is written as the escape `\U+XXXX`; in the SVG file the block is a group `ROTULO` under the drawing, the page grows to hold it and the five characters that XML keeps for itself are written as entities. The scale of the DXF block is `1:1` (the file is in millimetres) and that of the SVG block is the scale of the page.

| Setting | Default | Meaning |
|---|---|---|
| `title_block.enabled` | 0 | 1 draws the block, 0 leaves it out (the files are then exactly what they were) |
| `title_block.organisation` | empty | the organisation, at most 80 characters on one line |
| `title_block.title` | empty | the title; empty means the name of the project (or of the sketch) |
| `title_block.number` | empty | the number of the drawing |
| `title_block.revision` | A | the revision |
| `title_block.author` | empty | the author |
| `title_block.width` | 120 mm | the width of the block, from 60 mm to 400 mm |
| `title_block.row` | 7 mm | the height of each of the four rows, from 4 mm to 20 mm |

In `phoenix-cad-window.default.cfg`: `export.format` (the format the dialog starts with: `script`, `step`, `stl`, `dxf-top`, `dxf-front`, `dxf-side`, `svg-top`, `svg-front`, `svg-side`, `report`, `bill`, `obj` or `3mf`), `export.overwrite` and `export.max_kib`.

## A section of the model

`eh3d_phoenix_cad::section` cuts the model with a plane across one of the axes (`x`, `y` or `z`) at a position and draws where the plane meets the material. The command is `eh3d-phoenix-cad section PROJECT --axis x|y|z --at LENGTH [--format dxf|svg] [--out FILE] [--overwrite] [--lang CODE]`; the Reports menu has a panel with the same choices.

* The section is made from the triangles of every solid of the model (the box with everything on it, the placed parts and the turned and swept bodies, the same surfaces as the STL file, so a model that the STL file refuses has no section). A vertex whose coordinate is not below the plane is above it; an edge with its ends on different sides gives one point, named by the pair of vertices of the edge, and the two points of a triangle make a segment. The surfaces are closed, so the segments join end to end by those names into closed outlines, with no comparison of coordinates.
* The segment of a triangle runs in the direction that keeps the material on its left, so an outer outline turns counter-clockwise and a hole clockwise in the drawing, and the signed areas of the outlines add up to the area of the material in the plane. The drawing has the two axes that the plane leaves, in this order: `y` and `z` for a plane across `x`, `x` and `z` for a plane across `y`, and `x` and `y` for a plane across `z`. Corners that lie on the straight line between their neighbours (the diagonal of a wall made of two triangles) are left out. The curves of a hole, a turned body or a swept body are the polygons of the STL file (`mesh.circle_segments`), not arcs.
* The report gives the plane, the number of outlines and segments, the area of the material and, for each solid, its outlines and area, in the language of `--lang` or of the window. With `--out` (or the box of the panel) the drawing is written as DXF or SVG, in millimetres on the layer `CONTORNO`, with the title block of `[title_block]` when `title_block.enabled` is 1; the format is `--format` or the extension of the file, and the file goes through the checks of every export (never over a file that exists unless `--overwrite` is given, never over the project that is open, the size limited by `export.max_kib`, and in the panel only a name next to the project). A drawing with more lines than `drawing.max_entities` is refused with `PHOENIX-CAD-050`, and a plane that does not cut the model has nothing to draw (`PHOENIX-CAD-041`).
* `[panels]` `section_axis` and `section_position` of the window settings say what the panel starts with. The section is of the model as it is built, with the tip of the history where it is.

## A DXF drawing as a sketch

`eh3d_phoenix_cad::dxf_import` and the command `eh3d-phoenix-cad sketch-import DRAWING.dxf [--out FILE.phoenixsketch] [--name NAME] [--face NAME] [--unit mm|cm|m|in] [--overwrite] [--lang CODE]` read a flat text DXF drawing (a front panel, a board outline, a template of holes) into a new sketch.

* **What is read** is the section `ENTITIES`: `LINE`, `CIRCLE`, `ARC` (counter-clockwise from the first angle to the second), `LWPOLYLINE` and the old `POLYLINE` with its `VERTEX` records. A segment of a polyline with a bulge `b` is an arc of four times the arctangent of `b` (counter-clockwise for a positive bulge), and the closing flag joins the last vertex to the first. Everything else is left out and counted by the name of its entity (`TEXT`, `DIMENSION`, `SPLINE`, `ELLIPSE`, `HATCH`, `INSERT`...), and so is a line, an arc or a circle that has no length or radius once its ends are joined (`degenerate`); the command tells both. The layers are not looked at, and a binary DXF file is not understood.
* **The units** of the drawing are not read from the file: `--unit` says what one unit is (millimetres without it). The sketch lies on the face given by `--face` (the front without it) and has the name given by `--name` (the name of the file without it); the sketch has no constraints, so it keeps the drawing as it was.
* **The ends that meet** become one point: two ends within `sketch.profile_join` of each other on each axis are the same point, so the lines of a closed outline share their corners and the profile of the sketch is closed. A hole is a circle or a closed chain inside the outline, as for every sketch.
* **The limits.** A file larger than `export.max_kib` is not read, a coordinate outside the range of lengths is refused, and so is a drawing with more lines, circles and arcs than `drawing.max_entities` (`PHOENIX-CAD-050`); a drawing that is not valid, has no section of entities or has nothing to read is refused with `PHOENIX-CAD-035`. The sketch file is written like any other, never over a file that exists unless `--overwrite` is given (`PHOENIX-CAD-040`).
* **In the window** the button **Import a DXF drawing...** of the top bar opens a panel (it is also in the Reports menu) with the path of the drawing, typed or chosen in the file window, the name of the sketch, the face and the unit; **Import** reads the drawing into a sketch of the open project as one entry of the history and the panel says what was read and what was left out, in the language of the window. A refusal is in the status bar and the project is as it was, and a project opened read-only is not changed. `[panels]` `import_unit` and `import_face` of the window settings say what the panel starts with.
* **What it was tried on** is the drawings made by the program itself and by the tests; no drawing from another program has been read yet.

## The reports in the window and in the eight languages

The **Reports** menu of the window opens one panel for each report that the commands print: the cutting plan (`cutplan`), the hole schedule and the list of drills and cutters (`holes`), the clearance between parts (`clearance`), the placements with the exploded view (`placements`), the list of the placed parts with their properties (`partlist`), the mass and the centre of mass (`balance`), the drawing of a sketch (`sketch-export`) the figures of a sketch turned about an axis (`revolve`) and the section of the model by a plane (`section`). A panel shows its report as text that can be selected and copied, in the language of the window; the choices in it (drills and cutters instead of holes, how far the parts are pulled apart, the sketch, DXF or SVG, the name of the file and the box to write over it) are applied at once. The drawing of a sketch is written next to the project that is open under a name typed in the panel: the name must be a file name and not a folder, `..` or a drive, and a file that exists, the project that is open and a name that belongs to a project or a sketch are refused as in every export (`PHOENIX-CAD-040` and `PHOENIX-CAD-041`).

The commands print comma separated values for a program to read, the same text in every language. For a person, `cutplan`, `clearance`, `placements`, `holes` and `partlist` take `--text` (the cutting plan always had it) and the six reports, `sketch-export`, `revolve` and `sweep` take `--lang CODE` (`en`, `es`, `de`, `fr`, `it`, `ja`, `zh` or `pt`); without it they are in English, so that a report is the same text on every computer. The words of every report are in `eh3d_phoenix_cad::lang` and the layout of the lines in `eh3d_phoenix_cad::panels`; the numbers are the ones of the comma separated values.

In `phoenix-cad-window.default.cfg`, section `[panels]`: `panels.holes_tools` (1 starts the hole panel with the drills and cutters, 0 with the holes), `panels.explode` (1 starts the placements panel with the parts pulled apart), `panels.explode_percent` (50 by default, up to 10000; the library setting `placement.max_explode_percent` is the most that is accepted) and `panels.sketch_format` (`dxf` or `svg`). The box to write over a file starts as `export.overwrite` says.

## Codes

`PHOENIX-CAD-048` (shape the solid cannot be made of), `PHOENIX-CAD-049` (solid does not match the model), `PHOENIX-CAD-050` (export above the limits), and `PHOENIX-CAD-040` and `PHOENIX-CAD-041` for the writing; see [ERROR_CODES.md](ERROR_CODES.md).
