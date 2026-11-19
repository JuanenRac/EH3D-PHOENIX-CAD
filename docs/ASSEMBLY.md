# Simple assemblies

A chassis is parts held in place against each other. The catalog (see [CATALOG.md](CATALOG.md)) puts each part at a position typed in nanometres; an assembly says instead that one part lies against another, and checks that no two parts occupy the same place. It is a prototype: it has only been tried on the parts made up for the examples and the tests, and nothing has been compared with a real chassis or with a CAD program.

## Mates

A mate places one part (`b`) relative to another (`a`) along **one** axis. It takes away one of the three coordinates of `b` and leaves the other two as they were typed. There are eight kinds (the last five came later and write the project in version 6):

| Kind | Text | What it asks for |
|---|---|---|
| gap | `gap <direction> <distance>` | `b` lies on the side of `a` that the direction names (`+x`, `-x`, `+y`, `-y`, `+z` or `-z`), with `distance` of air between the two faces (0 touches); on the side of larger coordinates `b` starts at the upper face of `a` plus the distance, on the other side it ends at the lower face of `a` less the distance |
| align | `align <axis> <min or max or center>` | the lower faces of the two bounding boxes lie on one plane (`min`), the upper faces do (`max`) or the middles do (`center`); the middles are refused when they differ by half a nanometre, because positions are whole nanometres |
| stack | `stack` | `b` stands on `a`: its lower face on the upper face of `a` (the same as `gap +z 0`) |
| offset | `offset <axis> <distance>` | the middle of `b` is `distance` (with its sign, in whole nanometres) from the middle of `a` along the axis; refused when the middles differ by half a nanometre |
| symmetric | `symmetric <axis> <plane>` | the middle of `b` is the mirror image of the middle of `a` in the plane that lies at that coordinate across the axis (the sizes of the two parts may differ); refused when the result is half a nanometre off |
| inset | `inset <axis> <min or max> <distance>` | the lower (`min`) or the upper (`max`) face of `b` lies `distance` inside the same face of `a` along the axis: with `min`, `b` starts at the lower face of `a` plus the distance; with `max`, it ends at the upper face of `a` minus the distance. A minus sign makes it stick out, and a distance of 0 is the alignment of that face. The middles have no inside, so `center` is refused |
| proportion | `proportion <axis> <thousandths>` | the middle of `b` lies at that many thousandths (0 to 1000) of the size of `a` along the axis, counted from the lower face of `a`: 0 centres `b` on the lower face, 500 is the alignment of the middles and 1000 centres `b` on the upper face; refused when the result is not a whole number of nanometres |
| overlap | `overlap <direction> <distance>` | `b` lies on the side of `a` that the direction names with its near face `distance` inside the face of `a` (a lap or a tongue in a slot; 0 only touches it, like a gap of 0): on the side of larger coordinates `b` starts at the upper face of `a` less the distance, on the other side it ends at the lower face of `a` plus the distance; the distance is from 0 to the largest dimension of the settings |
| turn | `turn <quarters>` | no coordinate is taken away: the roll of `b` about its length is the roll of `a` plus that many quarter turns (0 to 3). It is the only angle that two parts can make, because a part is turned in steps of 90 degrees (24 turns); the position typed for `b` is not touched |

The sizes are those of the bounding box of each part in the model, which depend on how the part is turned (24 turns, see the catalog), never on where it is. So a profile along `+x` of 300 mm is 300 mm long on the `x` axis, and the faces of a screw are those of its bounding square.

A mate is a **constraint, not a move**: the position typed for the part stays in the model and the parts are put where their mates say (`Enclosure::placed_parts`, `eh3d_phoenix_cad::assembly::place`). Taking a mate away puts the part back where it was typed. The rules:

* a part is placed by one mate along each axis; a second mate for the same part and axis is refused, even if it is of another kind; a part is turned by one mate, and turns that depend on each other in a cycle are refused;
* the turns are solved first, so every other mate uses the size a part has once turned;
* a mate may refer to a part that another mate places (the chain is solved from its free end, whatever the order of the ids); a cycle (`a` on `b` and `b` on `a`, or longer) is refused;
* a mate that names a part that is not in the model, or a part and itself, is refused, and so is a part that a mate uses when it is taken away (take the mate away first);
* the result of every mate is checked like a placed part: it has to lie within the range of lengths and within `catalog.max_dimension` of the origin;
* there are at most `assembly.max_mates` mates in a model.

All of these are refused with `PHOENIX-CAD-046` (or `PHOENIX-CAD-045` for a part that ends up too far) and leave the model as it was. A mate is a step of the history like a hole or a part, with an exact inverse (`Operation::AddMate` and `Operation::RemoveMate`), so it is in the tree, can be undone and redone and can be suppressed.

## Turned and swept bodies

A turned body (`add-turned`) and a swept body (`add-swept`) take part in the mates and in the check of the interferences **by their bounding boxes**: for the mates and for the check, each body is a solid bar of a rectangular section with the size of its box, at the corner of the box (`Enclosure::assembled`). A mate may name a body as `a` or as `b`, by its name in the window and by its id in the project file; the mate places the box, and the body is moved with it: in the built model the position of the body (the point where the origin of its sketch lies) is shifted by what the mate moved the box, so the views, the bill, the balance and the files of triangles see the body where the mates put it, while the recipe keeps the position that was typed. Solving the mates again changes nothing. The box of a body turned by less than a full turn is the box of the full turn, and a body whose box is flat in some direction has none.

* a `turn` mate cannot name a body (a body is not turned by a quarter turn): it is refused with `PHOENIX-CAD-046`, and so is a mate that names a body that is not in the model;
* a body that a mate uses cannot be removed until the mate is, as for a part;
* the interference of a body is the interference of its box, so a ring that sits in the hole of a plate shares volume with the plate and is reported (the shape inside the box is not looked at); the pair is named in the report and drawn in the colour of a conflict in the views;
* in the window, a new mate may start from the selected body, and the fields of a body show where the mates put it and the names of the parts and bodies that share volume with it.

## Interference

`eh3d_phoenix_cad::assembly::interference` finds the pairs of placed parts that share volume. A pair that only touches (faces, edges or corners) is not in conflict. For each pair:

1. the bounding boxes are compared; a pair whose boxes do not share volume is clear (the pairs are found by sorting the boxes along `x`, not by trying all of them);
2. when the boxes overlap, the shapes are compared in whole nanometres: a solid profile is a box, a hollow one four boxes (the walls), a bracket two boxes (one for each leg) and a screw two cylinders (the head and the shank). A box meets a box, a box meets a cylinder along an axis and two cylinders along the same axis exactly, with integer arithmetic (squares of distances, doubled so that a centre on a half nanometre is exact). Two cylinders along different axes are compared as their bounding boxes;
3. a **screw through the hole of a bracket** is allowed: the shank of a screw is not in conflict with the leg of a bracket when it points along the hole and its circle lies inside the circle of the hole, with the room `assembly.hole_clearance` (the diameter of the hole has to be at least the diameter of the shank plus the clearance). That pair is counted as a pass.

The report has the pairs in conflict (the part with the smaller id first), how many pairs had overlapping bounding boxes, how many of those are passes and how many are clear because the shapes do not meet (a bar inside the cavity of a hollow section, a round head near the corner of a bar). The volume given for a conflict is the volume the two **bounding boxes** share, which is an upper bound of what the shapes share; it is not the volume of the intersection.

What the check does not know:

* a screw that enters a profile (the thread of a screw in a threaded hole or a nut in a slot) is a conflict: only the holes of brackets are known;
* every head is a cylinder of the diameter of the head (a countersunk head is a cone and a hexagon is wider across its corners than across its flats; the hexagon is taken as its circle across the flats, so a grazing contact with a hexagonal head can be missed);
* nothing has a tolerance: a nanometre of overlap is a conflict;
* a part that is not at right angles to the axes does not exist in the catalog.

## In the window

* The tree has a group of **mates**, one item for each, with the name of the step (`Mate 1`). The **Add mate** button puts a mate between the selected part (the one it places; the last part when none is selected) and the part before it in the tree (the next one when it is the first); the kind and its values start as the settings `[new_step]` say (`mate_kind`, `mate_direction`, `mate_distance`, `mate_axis`, `mate_edge`, `mate_offset`, `mate_plane`, `mate_quarters` and `mate_permille` in `phoenix-cad-window.default.cfg`). The kind is a choice of the eight; an offset has its axis and distance, a symmetry its axis and the position of its plane, a turn the roll in degrees (0, 90, 180 or 270) and a proportion its axis and its fraction in thousandths (0 to 1000).
* The fields of a mate are the part it is relative to and the part it places (typed by name; a name that is not there, or is used twice, is refused), the kind (a choice), the side and the distance of a gap or the axis and the faces of an alignment, and, read only, where the placed part really is. Each change of a field is an entry of the history, and a value the model refuses is told in the status bar and leaves the mate as it was.
* A part says where its mates put it when that is not where it was typed, and which parts it shares volume with.
* The parts in conflict are drawn in the colour of danger in the 3D view and in the plan (the selected part keeps the colour of the selection), and the **fit report** has the number of mates, then the line `Interference: ...` with the pairs in conflict (the first `assembly.max_report_pairs`, then how many more) or `Interference: none (...)` when there are two parts or more.

## From the command line

`eh3d-phoenix-cad interference PROJECT.phoenixproj [--lang CODE] [--config FILE]` prints the same lines as the fit report about the assembly and exits with 0 when every step of the project was applied and no two parts share volume, and with 1 otherwise. `check` prints them at the end of its report.

`eh3d-phoenix-cad faces PROJECT.phoenixproj --from FACE[:inner|:outer] --to FACE[:inner|:outer]` measures between two faces of the box (`measure::between_faces`): `front`, `back`, `left`, `right`, `top` or `bottom`, the outer face of the wall unless `:inner` names the face that looks at the inner space. Parallel faces give their distance in millimetres (the two sides of a wall, the inner height, the outer width, an outer face against the inner face across the box) and an angle of 0; faces that meet give a distance of 0 and an angle of 90. The same face on the same side is refused with `PHOENIX-CAD-038`. The command exits with 0 when every step of the project was applied.

## In the project file

A project with a mate writes the mechanical domain as version 5, with the steps `add-mate` and `remove-mate` (version 6 when a mate is an offset, a symmetry or a turn); a project without one keeps the version and the bytes it had (see [PROJECT_DOMAIN.md](PROJECT_DOMAIN.md)):

```text
step 0 on%20top add-mate 018bcfe5-6800-7003-8000-000000000003 018bcfe5-6800-7001-8000-000000000001 018bcfe5-6800-7002-8000-000000000002 stack
step 0 apart add-mate 018bcfe5-6800-7005-8000-000000000005 018bcfe5-6800-7001-8000-000000000001 018bcfe5-6800-7002-8000-000000000002 gap -y 5000000
step 0 aligned add-mate 018bcfe5-6800-7004-8000-000000000004 018bcfe5-6800-7001-8000-000000000001 018bcfe5-6800-7002-8000-000000000002 align x max
```

After the id of the mate come the id of the fixed part, the id of the part it places and the kind with its values; a distance is a whole number of nanometres from zero up.

## Mates panel

The Reports menu has a panel for the mates (`Window::mates_panel_text`). It lists every mate of the model in words (the placed part, the fixed part and the kind with its values, for example `offset x 40 mm`), any mate the model refused with the reason, and the report of the interferences. Below that it holds the editor of a new mate: the two parts, the kind (`gap`, `align`, `stack`, `offset`, `symmetric` or `turn`) and the values that kind needs, which start from the keys `new_step.mate_*` of the window settings. Before anything is added the panel tries the mate on a copy of the window and says whether the model accepts it, where the placed part would be and which pairs of parts would share volume; **Add the mate** then puts it in the recipe as one step of the history, with the same checks as the button of the tree.

## Clearance between parts

`eh3d_phoenix_cad::clearance` lists the pairs of placed parts that do not touch and are closer than `clearance.min_gap` (2 mm by default; 0 turns the check off), for example to keep room for a tool, a cable or a screw. The distance between two parts is the distance between their bounding boxes: along each axis the gap is zero when the ranges overlap, and the distance is the root of the sum of the squares of the three gaps, compared as squares in integers. A pair that touches (a stack, a gap of zero) is placed so on purpose and a pair that overlaps belongs to the interference check, so neither is listed. The parts are the ones the mates give. The command `eh3d-phoenix-cad clearance PROJECT` prints `part_a,part_b,distance_mm` with the closest pair first (at most `clearance.max_pairs` lines) and a line `total,N,`, and exits with 1 when there is a pair.

## Placements and the exploded view

`eh3d_phoenix_cad::placement` lists where each placed part ends up once the mates are applied: its name, the part of the library, the smallest corner of its bounding box and its size along the three axes in millimetres, and its turn, in order of name, with a last line `total` that gives the box of the whole assembly. The command `eh3d-phoenix-cad placements PROJECT` prints it as comma separated values. With `--explode PERCENT` every part is first pulled away from the middle of the assembly: the middle of each part moves along each axis by that percentage of its distance from the middle of the assembly, rounded to the nanometre, the turn and the cut are kept and the mates of the model are not touched. The setting `placement.max_explode_percent` (500 by default, up to 10000) is the most that is accepted; more is refused with `PHOENIX-CAD-045`, and so is a part that would leave the range of lengths.

## Pattern of a part

A part that is placed in the model can be repeated along a line, over a grid or round a vertical axis (`eh3d_phoenix_cad::partpattern`, the entry **Pattern of a part...** of the Reports menu). The positions follow from the position of the part (the smallest corner of its box, which is also the first position) and the layout: a **line** of `count` positions each `step` (along `x`, `y` and `z`) after the previous one, a **grid** of `columns` by `rows` positions with a pitch along `x` and along `y`, or a **circle** of `count` positions round the vertical axis through a centre (`x`, `y`), each an angle further round, counter-clockwise from above. A circle moves the part round the axis but does not turn it: a part can only be turned by quarter turns, which its orientation already says. The positions are whole nanometres with the integer sine and cosine of the angles, so they are the same on every platform.

* **Copies are parts of their own.** The pattern does not add a new kind of step: each copy (every position but the first, which is the part itself) is one `add-part` step named after the part (`Bar 2`, `Bar 3`...), so the project file keeps its version, and a copy can be moved, cut to another length or taken away afterwards. All the steps of one pattern are one entry of the history: one undo takes them all away.
* **All or nothing.** The copies are tried together with the model before they are kept. If one is refused (it shares volume with a profile while `catalog.refuse_profile_overlap` is 1, it is outside the limits of the catalog, or there are more parts than `catalog.max_instances`) none is added and the project is as it was.
* **The refusals** have the code `PHOENIX-CAD-042`: fewer than two positions, more than `pattern.max_parts` (64 by default, from 2 to 10000), a step or a pitch of nothing, a circle whose centre is the part itself, and two positions that are one (an angle that brings the part back before the last position).
* **The panel** picks the part, the layout and its values (the start values are the ones of the pattern of holes: `new_step.pattern_count`, `pattern_step`, `pattern_radius` and `pattern_angle`), lists the positions (`panels.part_pattern_shown` of them, 24 by default), draws the plan of the boxes of the copies with the part itself in the second colour (`panels.pattern_preview_size`, 240 pixels), and tries the pattern on a copy of the window to say whether the model accepts it and how many pairs of parts would share volume, before **Add the copies** changes anything.

## Settings

The keys of `[assembly]` in `phoenix-cad.default.cfg`: `max_mates` (1 to 100000), `hole_clearance` (0 to 1000 mm), `max_report_pairs` (1 to 100000) and `interference_tolerance` (0 to 1000 mm, 0 by default: how deep two bounding boxes may overlap, along their smallest overlap, and still be left out of the check; the pairs left out are counted in `Report::tolerated` and are not listed). The keys `mate_kind`, `mate_direction`, `mate_distance`, `mate_axis` and `mate_edge` of `[new_step]` in `phoenix-cad-window.default.cfg` say how a mate added from the window starts. Every value is checked when the file is read.

## Examples

`crates/eh3d-phoenix-cad/examples/assembly.phoenixproj` is a gantry made of the parts of `chassis.phoenixlib`: a base rail, two posts and a top rail placed with nine mates (stacks, alignments by the lower face, the upper face and the middle), a bracket against a post (a gap and an alignment) and a bolt through the hole of the bracket, which is typed. `eh3d-phoenix-cad interference` finds no conflict and one pass in it. In `frame.phoenixproj` the four bolts go through the holes of the four brackets. The tests read both.
