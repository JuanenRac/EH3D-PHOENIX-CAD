# The catalog of parts

A chassis is made of bought parts: lengths of profile, screws and angle brackets. The catalog lets a model say which part it uses, where it is and how it is turned, and adds the parts up. It is a prototype: it has only been tried on parts made up for the examples and the tests, and no part has been compared with a real one.

## The library file

A library is a plain text file with the extension `.phoenixlib` and a version of its own (the first line, `phoenix-library 1`). One line gives its name, and each other line is a part:

```text
phoenix-library 1
name chassis-basic
part tube-20x20 profile aluminium 20000000 20000000 1500000 300000000 2000000000
part bolt-m3x10 screw steel M3 3000000 10000000 cylinder 5500000 3000000
part angle-30 bracket aluminium 30000000 30000000 20000000 3000000 4500000
```

Lengths are whole nanometres (20000000 is 20 mm). Empty lines and lines that start with `#` are ignored. The text written is always the same for the same library. The three kinds of part, with the values that follow the name, the kind and the material:

| Kind | Values | Shape |
|---|---|---|
| `profile` | width, height, wall, usual cut, longest cut | an extrusion of a rectangle, solid when the wall is 0 and hollow otherwise; the longest cut is the length of the bar |
| `screw` | metric name (`M3`), diameter, length under the head, head (`cylinder`, `countersunk` or `hex`), head diameter, head height | a shank and a head; the diameter of a hexagon is across the flats |
| `bracket` | first leg, second leg, width, thickness, hole diameter | two legs at a right angle, with one hole through each leg (0 is no hole) |

The material is a name of the `[material.*]` sections of the settings, which hold the density in kilograms per cubic metre (`aluminium`, `steel` and `stainless` ship with the settings). A part whose numbers do not hold together is refused: a wall that leaves no room inside the section, a usual cut longer than the bar, a head not wider than its shank, a hole that does not fit a leg, a size above the largest dimension of the settings.

A file of a newer version, a line that is not understood, a number that is not a whole count of nanometres, a part listed twice, more parts or more bytes than the settings allow, and a material that the settings do not have are refused with `PHOENIX-CAD-044` and the number of the line.

## Parts placed in the model

A part is placed with a step of the history, like a hole or a set of fins, so it is in the tree, can be undone and redone, can be suppressed and is part of the script. The step holds:

* a copy of the part and the name of its library, so a project never depends on the file it was made from;
* the position: the smallest corner of the bounding box of the part in the model, in whole nanometres of `x`, `y` and `z` (a position may be negative);
* the orientation: the direction the length of the part runs along (`+x`, `-x`, `+y`, `-y`, `+z` or `-z`) and a roll about that direction in quarter turns (0, 90, 180 or 270 degrees), which together are the 24 turns that keep the axes on the axes; the text form is `+x/1`;
* for a profile, the length it is cut to (a screw and a bracket have none).

In its own frame a part fills a box from the origin: a profile is `width` by `height` by the cut, a screw has its head at the smallest `z` and its shank running up, and a bracket has the first leg along `x`, the width along `y` and the second leg along `z`. The orientation turns the frame onto the axes and the part is moved so that the smallest corner of its bounding box lies on the position. Everything is whole nanometres.

A part is refused with `PHOENIX-CAD-045` when:

* a profile is cut shorter than `catalog.min_cut` or longer than its bar, has no cut, or a screw or a bracket has one;
* it lies farther from the origin than `catalog.max_dimension`;
* it is a profile and shares volume with another profile (their bounding boxes overlap by more than a touch), when `catalog.refuse_profile_overlap` is 1; a screw is allowed to go through a profile and a bracket to sit against one;
* there are already `catalog.max_instances` parts in the model.

The parts are separate bodies: they are not added to the volume or the mass of the box, and a change of the box does not move or refuse them. A part can be placed relative to another with a mate instead of by typing its position, and the parts that share volume can be found: see [ASSEMBLY.md](ASSEMBLY.md). The STEP file does not draw them and says so (`PHOENIX-CAD-022`); the script has one block for each part, placed with a `multmatrix`, and the views draw the bounding box of each.

## Volume, mass, bill of materials and cut list

The volume of a profile and of a bracket is exact in cubic nanometres. A screw and a bracket with holes use the ratio of pi to twelve decimals, and a hexagonal head the ratio of the square root of three to twelve decimals, so the same model gives the same figure on every system. A head is taken as a cylinder, a cone cut off (countersunk) or a prism with a hexagon base; threads, chamfers and radii are not drawn, so the volume of a real part is a little different.

The mass is the volume times the density of the material, in whole micrograms. The bill of materials groups the pieces that are the same part of the same material, cut to the same length, and gives for each line the count, the cut, the volume and the mass, and the totals. The cut list gives, for each profile, the lengths it is cut to with how many of each (longest first) and the length of all the pieces together. It does not fit the cuts into bars and does not allow for the kerf of the saw.

The window shows the bill and the cut list in the fit report (in the language of the window) and writes the bill as comma separated values from the export menu; the command `eh3d-phoenix-cad bill PROJECT` prints the same values, and `eh3d-phoenix-cad library FILE` reads a library, checks it against the settings and lists its parts.

## Cutting plan

`eh3d_phoenix_cad::nesting` fits the pieces of the cut list into bars. A profile comes in bars of its longest cut (`max_cut` in the library); a bar loses `stock.end_trim` at each end, and every cut between two pieces takes `stock.kerf`. The pieces are placed longest first, each into the bar where it fits and that is left with the least room (best fit decreasing), so the same model always gives the same plan; it is a good plan and not always the best possible one. A bar with `n` pieces needs the pieces plus `n - 1` kerfs inside its room, and the offcut is what is left less the kerf of the cut that frees the last piece; an offcut of at least `stock.reusable_offcut` is marked `stock`, a shorter one `scrap` and none `none`. A piece longer than the room of a bar, or more bars than `stock.max_bars`, is refused with `PHOENIX-CAD-045`.

The command `eh3d-phoenix-cad cutplan PROJECT` prints one line per bar as comma separated values (`part,material,bar,bar_mm,pieces_mm,offcut_mm,offcut_use`, and a line of totals), or with `--text` lines for a person with the share of the bars that is used.

## Mass and centre of mass of the parts

`eh3d_phoenix_cad::partlist` lists the placed parts with their properties (the command `partlist`, a panel of the Reports menu): for each part the name, the kind, the part of the library, the material and its density, the volume, the mass and the corner and the size of its bounding box, in the order of kind, name and identifier, with the totals; the comma separated values have the columns `name,kind,part,material,density_kg_m3,volume_mm3,mass_g,x_mm,y_mm,z_mm,size_x_mm,size_y_mm,size_z_mm` and a last line `total`, and `--text` writes the same figures as lines in the language of `--lang`. The turned bodies of the model (`docs/SKETCH.md`) come after the parts, with the kind `turned`, the sketch as the part, the volume of the profile turned and the box that holds them. The mass of the list is the mass of the balance.

`eh3d_phoenix_cad::balance` adds up the mass of the placed parts (the volume of each times the density of its material, the same figure as the bill of materials) and finds their centre of mass: the mean of the centres of the parts weighted by their masses, calculated in integers (masses in micrograms, middles in doubled nanometres) and rounded to the nanometre once, at the end. The centre of a part is the middle of its bounding box. That is the exact centre of mass of a profile, solid or hollow, because its section is symmetric; for a screw (the head pulls the centre towards the head) and for a bracket (the corner is heavier than the ends of the legs) it is an estimate, and the report says how many parts are estimated. The parts are the ones the mates give, and each turned body counts as one more part whose mass is its exact volume times its density and whose centre of mass comes from its closed surface (so the figure is not an estimate); the box of the enclosure is not included (see the mass of the model for it). The command `eh3d-phoenix-cad balance PROJECT` prints the number of parts, the mass in grams, the centre in millimetres and the number of estimated centres; a material that is not in the settings is refused with `PHOENIX-CAD-045`.

## In the project file

A project with a placed part writes the mechanical domain in version 4, with the steps `add-part` and `remove-part`; a project without one keeps the version and the bytes it had. See [PROJECT_DOMAIN.md](PROJECT_DOMAIN.md) for the form of the lines.

## Settings

The keys of `[catalog]` in `phoenix-cad.default.cfg` hold the limits (`max_bytes`, `max_parts`, `max_name_bytes`, `max_dimension`, `max_instances`, `min_cut`, `refuse_profile_overlap`); the keys `part_x`, `part_y`, `part_z`, `part_axis` and `part_roll` of `[new_step]` in `phoenix-cad-window.default.cfg` say where a part added from the window starts. Every value is checked when the file is read.

## Examples

`crates/eh3d-phoenix-cad/examples/chassis.phoenixlib` is a library of eight generic parts, and `crates/eh3d-phoenix-cad/examples/frame.phoenixproj` places eighteen of them: two rails, two cross members, four uprights, four brackets and six screws. The tests read both.
