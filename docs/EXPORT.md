# Exporting the model

The model is written to a file from the **Export** menu of the window or with the command `export`. The formats:

| Format | Word | Extension | What it is |
|---|---|---|---|
| Script | `script` | `.scad` | the model as an OpenSCAD-style script |
| STEP | `step` | `.step` | a boundary representation of the box with its straight holes (see the limits in [SCOPE.md](SCOPE.md)) |
| STL | `stl` | `.stl` | the solid as triangles, binary or text |
| DXF | `dxf` | `.dxf` | the plan, the front or the side as a drawing |
| SVG | `svg` | `.svg` | the same views, scaled to a page, with dimensions |
| Report | `report` | `.txt` | the fit report |
| Bill | `bill` | `.csv` | the bill of materials of the parts placed in the model |

```
eh3d-phoenix-cad export PROJECT.phoenixproj --format stl|dxf|svg|step|script|report|bill
                        [--view top|front|side] [--out FILE] [--overwrite] [--ascii|--binary]
```

* `--out` defaults to the name of the project with the extension of the format, in the current folder.
* A file that exists is **never** written over unless `--overwrite` is given (in the window, unless the box of the dialog is ticked); the project that is open and a name that belongs to a project or a sketch are never written over. The check is the one of [`export::check_target`](../crates/eh3d-phoenix-cad/src/export.rs): `PHOENIX-CAD-040` when the file exists, `PHOENIX-CAD-041` for the other refusals.
* `--view` is for `dxf` and `svg` only (`top` is the default); `--ascii` and `--binary` are for `stl` only and override `mesh.stl_binary`.
* The exit code is 0 when the file was written and 1 when it was not; the code of the reason is printed.

## The STL file

`eh3d_phoenix_cad::solid` makes the solid out of the shapes the model already knows and `eh3d_phoenix_cad::mesh` keeps and writes the triangles. The same model always gives the same bytes.

What is in it: the box with its inner space; every hole in its five styles (the straight one, the countersink as a cone, the counterbore with its flat step, the rectangle and the slot); the posts with their foot and their bore; the centring lip of the lid; the extrusions and pockets of the sketches (a pocket through the face opens into the inner space, the islands of an outline stay); the cooling fins; and every part placed from the catalog, in the place its mates give it (profiles solid or hollow, screws with a cylindrical, countersunk or hexagonal head, brackets with their two holes). The vertical edges may be rounded or chamfered.

**One closed surface for the box, one for each part.** The box is not cut out of a block: its outside and its inner space are two surfaces joined only by the walls of the holes, so that every edge is used by exactly two triangles, one in each way. The parts are separate solids in the same file (a text file has one `solid` for each, a binary file has them one after the other); two parts that touch each other are not merged.

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

## The drawings

The three views look at the box from above (`top`: `x` to the right, `y` up), from the front (`front`: `x` right, `z` up) and from the right (`side`: `y` right, `z` up). Everything is in millimetres.

| Layer | What is on it |
|---|---|
| `CONTORNO` | the outline of the box (the rounded or chamfered corners in the plan), and the bounding rectangles of what stands out of it: extrusions, fins and placed parts; the outline of an extrusion or a fin of the face that the view looks at |
| `AGUJEROS` | the openings of the holes of the face that the view looks at (a countersink or a counterbore as two circles, a slot as two lines and two arcs) and of the pockets of that face |
| `EJES` | the centre lines of the box and of every hole drawn, `drawing.axis_overshoot` past what they mark |

The DXF file is R12 text with the entities `LINE`, `CIRCLE` and `ARC` and the header variable `$INSUNITS` set to millimetres. The SVG file is scaled to `drawing.svg_width`; with `drawing.dimensions = 1` it has the width and the height of the box as dimension lines, the size of every hole as a label (`⌀6`, `⌀3 / ⌀8` for a countersink, `4×10` for a slot) and the scale in the corner. Posts, the lip and the inside of the box are not drawn, because they are not seen from outside. A drawing with more than `drawing.max_entities` entities is refused (`PHOENIX-CAD-050`). Neither format has been opened in a drawing program.

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

In `phoenix-cad-window.default.cfg`: `export.format` (the format the dialog starts with: `script`, `step`, `stl`, `dxf-top`, `dxf-front`, `dxf-side`, `svg-top`, `svg-front`, `svg-side`, `report` or `bill`), `export.overwrite` and `export.max_kib`.

## Codes

`PHOENIX-CAD-048` (shape the solid cannot be made of), `PHOENIX-CAD-049` (solid does not match the model), `PHOENIX-CAD-050` (export above the limits), and `PHOENIX-CAD-040` and `PHOENIX-CAD-041` for the writing; see [ERROR_CODES.md](ERROR_CODES.md).
