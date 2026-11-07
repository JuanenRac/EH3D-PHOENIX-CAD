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

The parts are separate bodies: they are not added to the volume or the mass of the box, and a change of the box does not move or refuse them. The STEP file does not draw them and says so (`PHOENIX-CAD-022`); the script has one block for each part, placed with a `multmatrix`, and the views draw the bounding box of each.

## Volume, mass, bill of materials and cut list

The volume of a profile and of a bracket is exact in cubic nanometres. A screw and a bracket with holes use the ratio of pi to twelve decimals, and a hexagonal head the ratio of the square root of three to twelve decimals, so the same model gives the same figure on every system. A head is taken as a cylinder, a cone cut off (countersunk) or a prism with a hexagon base; threads, chamfers and radii are not drawn, so the volume of a real part is a little different.

The mass is the volume times the density of the material, in whole micrograms. The bill of materials groups the pieces that are the same part of the same material, cut to the same length, and gives for each line the count, the cut, the volume and the mass, and the totals. The cut list gives, for each profile, the lengths it is cut to with how many of each (longest first) and the length of all the pieces together. It does not fit the cuts into bars and does not allow for the kerf of the saw.

The window shows the bill and the cut list in the fit report (in the language of the window) and writes the bill as comma separated values from the export menu; the command `eh3d-phoenix-cad bill PROJECT` prints the same values, and `eh3d-phoenix-cad library FILE` reads a library, checks it against the settings and lists its parts.

## In the project file

A project with a placed part writes the mechanical domain in version 4, with the steps `add-part` and `remove-part`; a project without one keeps the version and the bytes it had. See [PROJECT_DOMAIN.md](PROJECT_DOMAIN.md) for the form of the lines.

## Settings

The keys of `[catalog]` in `phoenix-cad.default.cfg` hold the limits (`max_bytes`, `max_parts`, `max_name_bytes`, `max_dimension`, `max_instances`, `min_cut`, `refuse_profile_overlap`); the keys `part_x`, `part_y`, `part_z`, `part_axis` and `part_roll` of `[new_step]` in `phoenix-cad-window.default.cfg` say where a part added from the window starts. Every value is checked when the file is read.

## Examples

`crates/eh3d-phoenix-cad/examples/chassis.phoenixlib` is a library of eight generic parts, and `crates/eh3d-phoenix-cad/examples/frame.phoenixproj` places eighteen of them: two rails, two cross members, four uprights, four brackets and six screws. The tests read both.
