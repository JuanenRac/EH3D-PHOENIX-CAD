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
| Sheet of views | `dxf` or `svg` with `--view sheet` | `.dxf`, `.svg` | the plan, the front and the side on one drawing |
| Report | `report` | `.txt` | the fit report |
| Bill | `bill` | `.csv` | the bill of materials of the parts placed in the model |
| Order sheet | `order` | `.html` | the bars to buy, the holes to drill and the parts with their cost, as one self-contained page |

```
eh3d-phoenix-cad export PROJECT.phoenixproj --format stl|obj|3mf|dxf|svg|step|script|report|bill|order
                        [--view top|front|side|sheet] [--out FILE] [--overwrite] [--ascii|--binary]
```

* `--out` defaults to the name of the project with the extension of the format, in the current folder.
* A file that exists is **never** written over unless `--overwrite` is given (in the window, unless the box of the dialog is ticked); the project that is open and a name that belongs to a project or a sketch are never written over. The check is the one of [`export::check_target`](../crates/eh3d-phoenix-cad/src/export.rs): `PHOENIX-CAD-040` when the file exists, `PHOENIX-CAD-041` for the other refusals.
* `--view` is for `dxf` and `svg` only (`top` is the default; `sheet` puts the plan, the front and the side on one drawing, see below); `--ascii` and `--binary` are for `stl` only and override `mesh.stl_binary`.
* The exit code is 0 when the file was written and 1 when it was not; the code of the reason is printed.

## The order sheet

`eh3d_phoenix_cad::ordersheet` and `export --format order` (or **Order sheet (.html)** in the Export menu) write what has to be bought and made to build the model on one page, in the language of `--lang` or of the window:

* **Profile cuts:** for each profile and material the bars to buy (`stock.*` of the settings say the length of the bar, the kerf and the trim) with the pieces sawn from each bar and its offcut, and the number of bars in all; the figures are the ones of the command `cutplan`.
* **Holes to drill:** the schedule of the holes of the box (tag, name, face, position, diameter and shape), as the command `holes` gives it.
* **Parts of the catalog:** the bill of materials with its quantities, masses and cost (the price of the material and the markup of the settings), then the pieces, volume and mass in all and the subtotal, the markup and the total.
* **A page that can be opened anywhere.** The style is inside the file and there is no script, picture, font or link, so nothing is fetched when it is opened or printed; every name, material and library word that comes from the project is written with `&`, `<`, `>`, `"` and `'` as entities, so a name cannot add markup to the page. A model with no part and no hole gets one line that says there is nothing to order.
* **The limit:** a table with more rows than `order.max_rows` (2000 by default) is refused with `PHOENIX-CAD-050` and nothing is written; the file goes through the checks of every export (never over a file that exists unless `--overwrite` is given, and the size is limited by `export.max_kib`).

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

### Reading the files back

`interchange::read_file_bytes_limited` is the same with the limits of the settings for a STEP file, and the extensions `.step` and `.stp` (in any case) read the STEP files described below.

`interchange::read_obj`, `interchange::read_three_mf` and `interchange::read_file_bytes` (which picks the format from the extension, `.stl`, `.obj` or `.3mf`) read the triangles of these files back, each solid with its name and its surface in nanometres, and the command `eh3d-phoenix-cad mesh-info FILE` prints for each solid its vertices, triangles, whether its surface is closed, its volume, its surface and its size, and exits with 1 when a solid is open. An OBJ file may have quads (cut into triangles), negative indices and the texture and normal numbers of a face (ignored); a face that names a vertex of another solid, a number that does not exist or fewer than three corners is refused with the line, and so is a number more than a thousand kilometres from the origin. A 3MF package is read only when its entries are stored without compression, as the one of this program is, and in the unit its model names (micron, millimeter, centimeter, inch, foot or meter). An STL file has no names or shared vertices: its triangles are welded into one solid called `solid` by the nanometre of each corner, so a file this program wrote is read back as the same closed surface (the binary form keeps single precision, so the volume agrees to about a millionth). The files of other programs have only been read in the tests, with files written by the program itself and by hand.

## STEP files of prisms, and reading STEP files

`step::prism_to_step(name, profile, z0, height)` writes the profile of a sketch raised by `height` as a STEP file (the same AP214 text as the file of the box, with the same units and product structure): each region of the profile is a `MANIFOLD_SOLID_BREP` with a floor, a roof (the holes are further bounds of both) and one flat wall for each side of each outline, every side a `LINE` between two vertices. A circle or an arc is the polygon that the profile draws for it, so the file holds no curved face and every face is flat; the text is the same for the same profile, a height of 0 or less is refused (`PHOENIX-CAD-002`) and the name is written with printable ASCII only (anything else is `_`). The command is `eh3d-phoenix-cad sketch-export PROJECT|SKETCH --format step --height LENGTH [--sketch NAME] [--out FILE] [--overwrite]`, from the plane of the sketch (`z0` is 0). No CAD program has opened the file; the tests read it back.

`step_import::read_step(text, limits)` reads a STEP file of solids made of flat faces: the solids a file names (`MANIFOLD_SOLID_BREP`, `BREP_WITH_VOIDS` with its voids and `FACETED_BREP`), their faces on a `PLANE` (`ADVANCED_FACE` or `FACE_SURFACE`) or without a surface (`FACE`), bounded by an `EDGE_LOOP` of straight edges (`LINE`) or a `POLY_LOOP`, with holes as further bounds. Each solid comes back as a surface of triangles in nanometres, turned so that its normals point out of the material (a void turned inward), the way the other readers give it, and its volume is exact for the corners. The unit is the length unit the file names (millimetre, centimetre, metre, micron, kilometre, inch or foot; millimetre when it names none) and every coordinate is rounded to a whole nanometre; a coordinate more than a thousand kilometres from the origin, a corner that is more than a micron off the plane of its face, a face with no area, a loop that does not close and a plane that does not agree with its corners are refused.

What cannot be read is refused in order with the number of the entity and its name, and nothing is left out: a face on a `CYLINDRICAL_SURFACE`, `CONICAL_SURFACE`, `SPHERICAL_SURFACE`, `TOROIDAL_SURFACE`, `SURFACE_OF_REVOLUTION`, `SURFACE_OF_LINEAR_EXTRUSION` or a B-spline surface, an edge on a `CIRCLE`, an ellipse or a B-spline curve, an open shell, and a model that is not a boundary representation (`SHELL_BASED_SURFACE_MODEL`, `TESSELLATED_SOLID`, `CSG_SOLID`, ...). So the STEP file of a box with round holes that this program writes is refused (its holes are cylinders), and the file of a box without holes is read back to the exact volume of the walls. The files of other programs have only been read in the tests, with files written by the program itself and by hand.

The size of the file, the number of its entities, the number of its faces and the number of triangles are limited by `mesh.step_max_kib`, `mesh.step_max_entities`, `mesh.step_max_faces` and `mesh.max_triangles`; a file over a limit is refused with the limit that it passed before the rest of it is looked at.

### What `mesh-info` says about a solid

`eh3d-phoenix-cad mesh-info FILE.stl|.obj|.3mf|.step|.stp [--density KG_M3 | --material NAME] [--config FILE]` also says, for a solid that is a prism along one of the axes (every vertex in one of two planes across the axis, the triangles between them vertical walls, and the volume the area of the floor times the height: `Mesh::as_prism`), the axis, the height, where the floor is, the number of outlines and corners and the area of the floor. With a density in kilograms per cubic metre (1 to 100000, or a `--material` of the settings, not both) it also gives the mass in grams, the centre of mass in millimetres and the tensor of inertia about the centre of mass in g mm2 (`Mesh::inertia_nm5`: the sums of the signed tetrahedra of the triangles, moved to the centre of mass; the same for a surface turned inside out). The figures follow from the triangles that were read, so they are the ones of the polygons that stand for the circles in a file that this program wrote.

## The drawings

The three views look at the box from above (`top`: `x` to the right, `y` up), from the front (`front`: `x` right, `z` up) and from the right (`side`: `y` right, `z` up). Everything is in millimetres.

| Layer | What is on it |
|---|---|
| `CONTORNO` | the outline of the box (the rounded or chamfered corners in the plan), and the bounding rectangles of what stands out of it: extrusions, fins and placed parts; the outline of an extrusion or a fin of the face that the view looks at |
| `AGUJEROS` | the openings of the holes of the face that the view looks at (a countersink or a counterbore as two circles, a slot as two lines and two arcs) and of the pockets of that face |
| `EJES` | the centre lines of the box and of every hole drawn, `drawing.axis_overshoot` past what they mark |
| `COTAS` | the dimensions (lines, extension lines and texts), when `drawing.dxf_dimensions = 1`; the SVG file has them in a group of the same name when `drawing.dimensions = 1` |
| `RAYADO` | the hatching of a section (see below); the drawings of the views have none |
| `MARCO` | the border round the views, their dimension lines and their legend, `drawing.frame_margin` away from them, when `drawing.frame = 1` (it is 0 by default, so a drawing is what it was) |
| `OCULTAS` | the cavity of the box (the inside of the walls, the floor and the lid) as a dashed rectangle, which the view does not see: `drawing.hidden_lines = 0` leaves it out. The DXF file then has a second line type `DASHED` for the layer, and the SVG file a dash for its group |

The DXF file is R12 text with the entities `LINE`, `CIRCLE` and `ARC` and the header variable `$INSUNITS` set to millimetres. The SVG file is scaled to `drawing.svg_width`; with `drawing.dimensions = 1` it has the width and the height of the box as dimension lines, the size of every hole as a label (`⌀6`, `⌀3 / ⌀8` for a countersink, `4×10` for a slot) and the scale in the corner. Posts, the lip and the inside of the box are not drawn, because they are not seen from outside. A drawing with more than `drawing.max_entities` entities is refused (`PHOENIX-CAD-050`). Neither format has been opened in a drawing program.

### The sheet of views

`export --view sheet` (or **Sheet of views** in the Export menu, `dxf-sheet` and `svg-sheet`) puts the front, the plan and the side on one drawing, each with its own outline, cavity, centre lines and dimensions (`Drawing::sheet`). The front is the main view, at the origin. With `drawing.sheet_angle = 1` (the default, first-angle projection as in ISO) the plan is under the front and the side, which looks from the right, is to its left; with `3` (third angle) the plan is over the front and the side to its right. Between two views are `drawing.sheet_gap` (20 mm by default) and the room that the dimension lines of the view next to the gap take, so no dimension line lies on a neighbour. The legend is written once, to the right of the whole sheet, and lists the holes of each view under its name (*Front view*, *Plan view*, *Side view*, each cut at `drawing.max_dimensions`; with `drawing.sheet_legends = 0` only those of the front); the border and the title block go round and under the whole sheet. The three views together count against `drawing.max_entities` (`PHOENIX-CAD-050`).

### Paper size and scale

With `drawing.paper` from 1 to 5 the SVG drawing (a view, the sheet of views, a section or a sketch) is made for a sheet of the A series (A1 841 x 594 mm, A2 594 x 420, A3 420 x 297, A4 297 x 210, A5 210 x 148; upright when `drawing.paper_portrait` is 1) and the file has the size of that sheet. The drawing, with its dimensions, legend and title block, is put in the middle at the largest standard scale (50:1, 20:1, 10:1, 5:1, 2:1, 1:1, 1:2, 1:5, 1:10, 1:20, 1:50, 1:100) that leaves `drawing.paper_margin` of air round it; below 1:100 the exact scale that fits is used so that nothing is cut off. The scale is written in the corner of the page and in the cell of the scale of the title block as a ratio (`2:1`, `1:5`). With `drawing.paper = 0` (the default) the drawing is scaled to `drawing.svg_width` and the scale is the one that fits, with two decimals. The DXF file is always at 1:1, in millimetres, and with `drawing.paper` from 1 to 5 the DXF drawing of the box (a view or the sheet of views) also carries the sheet: the edge of the paper and the line of its margin, as lines of the layer `MARCO`, with the size of the paper times the standard scale (an A4 sheet at 1:2 is 594 x 420 mm in the file, because the drawing stays full size), put round the middle of the drawing, its dimensions, its legend and its title block, whose cell of the scale then says that scale (`1:2`) instead of `1:1` (`Drawing::paper_sheet`, `Drawing::add_paper_sheet`). With `drawing.paper = 0` the DXF file is what it was. The DXF drawings of a section and of a sketch have no sheet.

### The dimensions of a view

The three views are dimensioned by themselves, in the DXF file (on the layer `COTAS`, as `LINE` and `TEXT` entities) and in the SVG file:

* the **width** under the box and the **height** to the left of it, as the overall dimensions;
* the **position of every hole** of the face that the view looks at, measured from the lower left corner of the box: one dimension for each distinct position across (`u`) under the box and for each distinct position up (`v`) to the left, so two holes on one row share the height. They are stacked like a baseline dimensioning, the nearest position on the line next to the box and every next one 6 mm farther out, with the overall dimension outermost; the first line is 8 mm from the box. `drawing.hole_dimensions = 0` leaves the positions out and keeps the two overall dimensions;
* `drawing.dxf_dimensions = 0` writes the DXF file as it was before, with the three layers (and the layer of the hidden lines, unless `drawing.hidden_lines = 0`) and no dimension; `drawing.max_dimensions` (40 by default) is the most positions in one view, and a view with more is refused with `PHOENIX-CAD-050`;
* the text of a dimension is the length in millimetres; the one to the left is turned a quarter and a character that is not printable ASCII is written as the escape of every DXF text. With a title block in the DXF file, the block is put below the dimensions and not over them.

* **The legend.** To the right of the box (`drawing.legend_gap`, 10 mm by default) the drawing lists its dimensions as text in the language of the export: the title, the size of the box, the thickness of the walls, the floor and the lid, and one line for each hole of the face the view looks at with the tag of the hole schedule (`H1`, `H2`, ...), its diameter (the width for a rectangle or a slot) and its position, up to `drawing.max_dimensions` of them and then a line that counts the rest. The lines are written where the dimensions are (the layer `COTAS` of the DXF file and the group `COTAS` of the SVG file, whose page grows to the right to hold them) and not when those are off. `drawing.legend = 0` leaves it out (`Drawing::add_legend`).

The sizes are not tolerances (nothing here knows ISO 286 fits), a hole of a countersink or a slot is dimensioned by its centre, and the dimension lines are plain lines with no arrow heads: they are for reading a drawing, not for a drawing standard.

## The hole schedule

`eh3d_phoenix_cad::schedule` lists the holes of the box for the person who drills them: by face (front, back, left, right, top, bottom), then by position `u` and `v` from the lower left corner of the face as seen from outside, each with a tag `H1`, `H2`, ... that follows that order, so the same box always gives the same tags. The command `eh3d-phoenix-cad holes PROJECT` prints `tag,name,face,u_mm,v_mm,diameter_mm,style,detail` (the detail is the head of a countersink, the head and the depth of a counterbore, the height of a rectangle or the length and direction of a slot) and a line `total,N`. With `--tools` it prints the drills and cutters the job needs, `diameter_mm,style,detail,count`, the smallest diameter first. The positions are the nominal ones of the model, without the compensation of the printer (see the model for fabrication).

## A sketch as a flat drawing

`eh3d_phoenix_cad::sketch_export` writes the closed outlines of a sketch (the ones the extrusions and pockets are made from) as a DXF or SVG file for a laser, a router or a plotter: the outer outline of each region on the layer `CONTORNO` and the holes in it on `AGUJEROS`, in millimetres and in the coordinates of the sketch. Lines stay lines, arcs stay arcs (counter-clockwise from the first angle to the second) and circles stay circles; construction geometry and points alone are left out. The command is `eh3d-phoenix-cad sketch-export PROJECT|SKETCH --format dxf|svg [--sketch NAME] [--out FILE] [--overwrite]`: a project with one sketch needs no name, one with several needs `--sketch`; `--out` defaults to the name of the sketch in the current folder. As with every export, a file that exists is never written over unless `--overwrite` is given, nor the project that is open. A sketch with no closed outline is refused with `PHOENIX-CAD-036` and a drawing with more entities than `drawing.max_entities` with `PHOENIX-CAD-050`. The drawing is dimensioned like the drawings of the box: with `drawing.dimensions` (SVG) or `drawing.dxf_dimensions` (DXF, on the layer `COTAS`) it carries the width and the height of the whole outline, measured from its lower left corner, and with `drawing.hole_dimensions` the diameter of every round hole beside it (a hole that is a chain of lines and arcs has no label); more labels than `drawing.max_dimensions` are refused with `PHOENIX-CAD-050`. A DXF drawing that is read back as a sketch leaves the layer `COTAS` out unless it is named, so the dimensions are not read as lines.

## Settings

In `phoenix-cad.default.cfg` (the `config` command writes them):

| Key | Default | Meaning |
|---|---|---|
| `mesh.circle_segments` | 64 | sides of every circle, a multiple of 4 from 8 to 720 |
| `mesh.max_triangles` | 400000 | the most triangles of one export |
| `mesh.stl_binary` | 1 | 1 writes a binary STL file, 0 a text one |
| `mesh.volume_tolerance_ppm` | 2000 | how far the volume of the triangles may be from the exact one |
| `mesh.step_max_kib` | 65536 | the largest STEP file that is read, in KiB (from 1 to 1048576) |
| `mesh.step_max_entities` | 1000000 | the most entities of a STEP file that is read (from 10 to 50000000) |
| `mesh.step_max_faces` | 100000 | the most faces of a STEP file that is read (from 6 to 10000000) |
| `drawing.max_entities` | 20000 | the most entities of one drawing |
| `drawing.svg_width` | 200 mm | the width of the SVG page |
| `drawing.dimensions` | 1 | dimensions and labels in the SVG drawing |
| `drawing.axis_overshoot` | 3 mm | how far the centre lines go past what they mark |
| `drawing.dxf_dimensions` | 1 | the dimensions in the DXF drawing too, on the layer `COTAS` |
| `drawing.hole_dimensions` | 1 | the positions of the holes of the face of the view are dimensioned from the corner |
| `drawing.max_dimensions` | 40 | the most dimensions of hole positions in one view |
| `drawing.hatch_spacing` | 2 mm | the distance between the hatching lines of a section drawing (0 leaves the hatching out) |
| `drawing.hatch_by_material` | 0 | 1 hatches the material of each part of a section drawing at an angle of its own, 0 hatches everything at the first angle |
| `drawing.hatch_angles` | `135 45 90 0 60 120 30 150` | the eight angles of that hatching, in whole degrees from 0 to 179 from the first axis of the drawing, all different |
| `drawing.legend` | 1 | the list of the dimensions (box, thickness, holes) to the right of the box |
| `drawing.legend_gap` | 10 mm | the room between the box and the legend |
| `drawing.hidden_lines` | 1 | the cavity of the box as dashed lines on the layer `OCULTAS` |
| `drawing.sheet_angle` | 1 | the sheet of views: 1 puts the plan under the front and the side to its left (first angle), 3 the plan over the front and the side to its right (third angle) |
| `drawing.sheet_gap` | 20 mm | the air between two views of the sheet, besides the room of their dimension lines |
| `drawing.sheet_legends` | 1 | the legend of the sheet lists the holes of every view under the name of the view; 0 lists only those of the front |
| `drawing.sheet_symbol` | 0 | 1 draws the symbol of the projection under the lower left corner of the sheet (the circles to the right of a truncated cone for first angle, to its left for third angle) |
| `drawing.sheet_symbol_size` | 10 mm | the height of the large end of that symbol, from 4 mm to 50 mm |
| `drawing.paper` | 0 | the paper of the SVG drawing: 0 scales it to `drawing.svg_width`, 1 to 5 put it centred on a sheet of the A series (A1 to A5) at the largest standard scale that fits |
| `drawing.paper_margin` | 10 mm | the air between the drawing and the edge of the paper, from 0 mm to 100 mm |
| `drawing.paper_portrait` | 0 | 1 stands the paper upright, 0 lays it across |
| `drawing.frame` | 0 | a border on the layer `MARCO` round the views, their dimensions and their legend |
| `drawing.frame_margin` | 5 mm | the air between what is drawn and the border, from 0 mm to 1000 mm |

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

In `phoenix-cad-window.default.cfg`: `export.format` (the format the dialog starts with: `script`, `step`, `stl`, `dxf-top`, `dxf-front`, `dxf-side`, `dxf-sheet`, `svg-top`, `svg-front`, `svg-side`, `svg-sheet`, `report`, `bill`, `obj`, `3mf` or `order`), `export.overwrite` and `export.max_kib`.

## A section of the model

`eh3d_phoenix_cad::section` cuts the model with a plane across one of the axes (`x`, `y` or `z`) at a position and draws where the plane meets the material. The command is `eh3d-phoenix-cad section PROJECT --axis x|y|z --at LENGTH [--format dxf|svg] [--out FILE] [--overwrite] [--lang CODE]`; the Reports menu has a panel with the same choices.

* The section is made from the triangles of every solid of the model (the box with everything on it, the placed parts and the turned and swept bodies, the same surfaces as the STL file, so a model that the STL file refuses has no section). A vertex whose coordinate is not below the plane is above it; an edge with its ends on different sides gives one point, named by the pair of vertices of the edge, and the two points of a triangle make a segment. The surfaces are closed, so the segments join end to end by those names into closed outlines, with no comparison of coordinates.
* The segment of a triangle runs in the direction that keeps the material on its left, so an outer outline turns counter-clockwise and a hole clockwise in the drawing, and the signed areas of the outlines add up to the area of the material in the plane. The drawing has the two axes that the plane leaves, in this order: `y` and `z` for a plane across `x`, `x` and `z` for a plane across `y`, and `x` and `y` for a plane across `z`. Corners that lie on the straight line between their neighbours (the diagonal of a wall made of two triangles) are left out. The curves of a hole, a turned body or a swept body are the polygons of the STL file (`mesh.circle_segments`), not arcs.
* The report gives the plane, the number of outlines and segments, the area of the material and, for each solid, its outlines and area, in the language of `--lang` or of the window. With `--out` (or the box of the panel) the drawing is written as DXF or SVG, in millimetres on the layer `CONTORNO`, with the title block of `[title_block]` when `title_block.enabled` is 1; the format is `--format` or the extension of the file, and the file goes through the checks of every export (never over a file that exists unless `--overwrite` is given, never over the project that is open, the size limited by `export.max_kib`, and in the panel only a name next to the project). A drawing with more lines than `drawing.max_entities` is refused with `PHOENIX-CAD-050`, and a plane that does not cut the model has nothing to draw (`PHOENIX-CAD-041`).
* **The hatching.** The material of the section is hatched with lines at 45 degrees on the layer `RAYADO` (a group of the same name in the SVG file), `drawing.hatch_spacing` apart measured across the lines (2 mm by default; 0 leaves the hatching out; from 0.2 mm to 100 mm). The lines are clipped to the material by the even-odd rule over all the outlines, so the hole of a wall and the cavity of a box are not hatched; two solids of the model that overlap in the plane cancel each other's hatching where they overlap. The hatching counts against `drawing.max_entities` together with the outlines, and a section that would need more lines is refused with `PHOENIX-CAD-050`. The report does not change. With `drawing.hatch_by_material = 1` each material is hatched on its own, by the even-odd rule over its own outlines, at the angle that `drawing.hatch_angles` gives it in turn: the materials in the order of their names with the box, which has none, first (`section::hatch_key` gives the table), so a part is told from the box and from a part of another material by the direction of its lines; with more materials than angles the angles start again, and the lines of all of them together count against `drawing.max_entities`.
* `[panels]` `section_axis` and `section_position` of the window settings say what the panel starts with. The section is of the model as it is built, with the tip of the history where it is.
* **Several planes and a picture of each cut.** Besides the plane of the two fields, the panel has a line for more planes typed as `axis=position` separated by commas or semicolons (`x=10, z=5 mm`); the model is cut into triangles once and the report of each plane follows the one before. Under the fields every plane has a small picture of its cut outline (`eh3d_phoenix_cad::section::preview`: the outlines fitted in a square pane with the same scale along both axes, the second axis of the drawing pointing up, outer outlines and holes in different colours), and **Write a drawing for every plane** writes one DXF or SVG file for each plane next to the project, named like `section-z-15.dxf`, with the same checks of every export, stopping at the first refusal. `panels.section_max_planes` (6 by default, 1 to 64) is the most planes at once, `panels.section_preview_size` (180 pixels, 60 to 1000) the side of each picture and `panels.section_preview_margin` (8 pixels, 0 to 100) the margin round the outline.

## A DXF drawing as a sketch

`eh3d_phoenix_cad::dxf_import` and the command `eh3d-phoenix-cad sketch-import DRAWING.dxf [--out FILE.phoenixsketch] [--name NAME] [--face NAME] [--unit mm|cm|m|in|auto] [--layers NAME,NAME] [--list-layers] [--overwrite] [--lang CODE]` read a flat text DXF drawing (a front panel, a board outline, a template of holes) into a new sketch.

* **What is read** is the section `ENTITIES`: `LINE`, `CIRCLE`, `ARC` (counter-clockwise from the first angle to the second), `LWPOLYLINE` and the old `POLYLINE` with its `VERTEX` records. A segment of a polyline with a bulge `b` is an arc of four times the arctangent of `b` (counter-clockwise for a positive bulge), and the closing flag joins the last vertex to the first. A circle, an arc or a polyline whose extrusion direction (group codes 210, 220 and 230) points down the third axis is drawn in a mirrored system of coordinates, so it is read with its first coordinates, its arc angles and its bulges mirrored (as the arbitrary axis algorithm of the format says); one whose direction is neither up nor down is left out and counted as `CIRCLE (tilted)`, `ARC (tilted)` or `POLYLINE (tilted)`. Everything else is left out and counted by the name of its entity (`TEXT`, `DIMENSION`, `SPLINE`, `ELLIPSE`, `HATCH`, `INSERT`...), and so is a line, an arc or a circle that has no length or radius once its ends are joined (`degenerate`); the command tells both. A binary DXF file is not understood.
* **The units** of the drawing are what `--unit` says one unit is (millimetres without it); `--unit auto` reads them from the variable `$INSUNITS` of the header (1 inches, 4 millimetres, 5 centimetres, 6 metres) and says that it did, or says that the header names no unit of those and that millimetres were used (the window offers it as `auto` next to the other units, and the drawings this program writes carry `$INSUNITS` 4). The sketch lies on the face given by `--face` (the front without it) and has the name given by `--name` (the name of the file without it); the sketch has no constraints, so it keeps the drawing as it was.
* **The layers** can be chosen: `--list-layers` prints the layers of the drawing with how many entities each holds and writes nothing, and `--layers CUT,HOLES` reads only the entities of those layers (names compared without regard to case; an entity that names no layer is on the layer `0`; the vertices of an old polyline follow their polyline) and tells how many entities the other layers left out; a choice that matches nothing leaves nothing to read and is refused with `PHOENIX-CAD-035`.
* **The ends that meet** become one point: two ends within `sketch.profile_join` of each other on each axis are the same point, so the lines of a closed outline share their corners and the profile of the sketch is closed. A hole is a circle or a closed chain inside the outline, as for every sketch.
* **The limits.** A file larger than `export.max_kib` is not read, a coordinate outside the range of lengths is refused, and so is a drawing with more lines, circles and arcs than `drawing.max_entities` (`PHOENIX-CAD-050`); a drawing that is not valid, has no section of entities or has nothing to read is refused with `PHOENIX-CAD-035`. The sketch file is written like any other, never over a file that exists unless `--overwrite` is given (`PHOENIX-CAD-040`).
* **In the window** the button **Import a DXF drawing...** of the top bar opens a panel (it is also in the Reports menu) with the path of the drawing, typed or chosen in the file window, the name of the sketch, the face, the unit and the layers to import (empty: all, separated by commas); **Preview** lists the layers and draws what the choice would read in a small picture (circles in the second colour) without changing the project; **Import** reads the drawing into a sketch of the open project as one entry of the history and the panel says what was read and what was left out, in the language of the window. A refusal is in the status bar and the project is as it was, and a project opened read-only is not changed. `[panels]` `import_unit` and `import_face` of the window settings say what the panel starts with, and `import_preview_size` (240 pixels) and `import_preview_segments` (24 lines to a full circle) tune the picture.
* **What it was tried on** is the drawings made by the program itself and by the tests; no drawing from another program has been read yet.

## The reports in the window and in the eight languages

The **Reports** menu of the window opens one panel for each report that the commands print: the cutting plan (`cutplan`), the hole schedule and the list of drills and cutters (`holes`), the clearance between parts (`clearance`), the placements with the exploded view (`placements`), the list of the placed parts with their properties (`partlist`), the bill of materials with cost (`bom`), the mass and the centre of mass (`balance`), the drawing of a sketch (`sketch-export`) the figures of a sketch turned about an axis (`revolve`) and the section of the model by a plane (`section`). A panel shows its report as text that can be selected and copied, in the language of the window; the choices in it (drills and cutters instead of holes, how far the parts are pulled apart, the sketch, DXF or SVG, the name of the file and the box to write over it) are applied at once. The drawing of a sketch is written next to the project that is open under a name typed in the panel: the name must be a file name and not a folder, `..` or a drive, and a file that exists, the project that is open and a name that belongs to a project or a sketch are refused as in every export (`PHOENIX-CAD-040` and `PHOENIX-CAD-041`).

The commands print comma separated values for a program to read, the same text in every language. For a person, `cutplan`, `clearance`, `placements`, `holes`, `partlist` and `bom` take `--text` (the cutting plan always had it) and the six reports, `sketch-export`, `revolve` and `sweep` take `--lang CODE` (`en`, `es`, `de`, `fr`, `it`, `ja`, `zh` or `pt`); without it they are in English, so that a report is the same text on every computer. The words of every report are in `eh3d_phoenix_cad::lang` and the layout of the lines in `eh3d_phoenix_cad::panels`; the numbers are the ones of the comma separated values.

In `phoenix-cad-window.default.cfg`, section `[panels]`: `panels.holes_tools` (1 starts the hole panel with the drills and cutters, 0 with the holes), `panels.explode` (1 starts the placements panel with the parts pulled apart), `panels.explode_percent` (50 by default, up to 10000; the library setting `placement.max_explode_percent` is the most that is accepted) and `panels.sketch_format` (`dxf` or `svg`). The box to write over a file starts as `export.overwrite` says.

## Codes

`PHOENIX-CAD-048` (shape the solid cannot be made of), `PHOENIX-CAD-049` (solid does not match the model), `PHOENIX-CAD-050` (export above the limits), and `PHOENIX-CAD-040` and `PHOENIX-CAD-041` for the writing; see [ERROR_CODES.md](ERROR_CODES.md).
