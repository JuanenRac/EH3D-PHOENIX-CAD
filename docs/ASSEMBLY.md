# Simple assemblies

A chassis is parts held in place against each other. The catalog (see [CATALOG.md](CATALOG.md)) puts each part at a position typed in nanometres; an assembly says instead that one part lies against another, and checks that no two parts occupy the same place. It is a prototype: it has only been tried on the parts made up for the examples and the tests, and nothing has been compared with a real chassis or with a CAD program.

## Mates

A mate places one part (`b`) relative to another (`a`) along **one** axis. It takes away one of the three coordinates of `b` and leaves the other two as they were typed. There are three kinds:

| Kind | Text | What it asks for |
|---|---|---|
| gap | `gap <direction> <distance>` | `b` lies on the side of `a` that the direction names (`+x`, `-x`, `+y`, `-y`, `+z` or `-z`), with `distance` of air between the two faces (0 touches); on the side of larger coordinates `b` starts at the upper face of `a` plus the distance, on the other side it ends at the lower face of `a` less the distance |
| align | `align <axis> <min or max or center>` | the lower faces of the two bounding boxes lie on one plane (`min`), the upper faces do (`max`) or the middles do (`center`); the middles are refused when they differ by half a nanometre, because positions are whole nanometres |
| stack | `stack` | `b` stands on `a`: its lower face on the upper face of `a` (the same as `gap +z 0`) |

The sizes are those of the bounding box of each part in the model, which depend on how the part is turned (24 turns, see the catalog), never on where it is. So a profile along `+x` of 300 mm is 300 mm long on the `x` axis, and the faces of a screw are those of its bounding square.

A mate is a **constraint, not a move**: the position typed for the part stays in the model and the parts are put where their mates say (`Enclosure::placed_parts`, `eh3d_phoenix_cad::assembly::place`). Taking a mate away puts the part back where it was typed. The rules:

* a part is placed by one mate along each axis; a second mate for the same part and axis is refused, even if it is of another kind;
* a mate may refer to a part that another mate places (the chain is solved from its free end, whatever the order of the ids); a cycle (`a` on `b` and `b` on `a`, or longer) is refused;
* a mate that names a part that is not in the model, or a part and itself, is refused, and so is a part that a mate uses when it is taken away (take the mate away first);
* the result of every mate is checked like a placed part: it has to lie within the range of lengths and within `catalog.max_dimension` of the origin;
* there are at most `assembly.max_mates` mates in a model.

All of these are refused with `PHOENIX-CAD-046` (or `PHOENIX-CAD-045` for a part that ends up too far) and leave the model as it was. A mate is a step of the history like a hole or a part, with an exact inverse (`Operation::AddMate` and `Operation::RemoveMate`), so it is in the tree, can be undone and redone and can be suppressed.

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

* The tree has a group of **mates**, one item for each, with the name of the step (`Mate 1`). The **Add mate** button puts a mate between the selected part (the one it places) and the part before it in the tree; the kind and its values start as the settings `[new_step]` say (`mate_kind`, `mate_direction`, `mate_distance`, `mate_axis` and `mate_edge` in `phoenix-cad-window.default.cfg`).
* The fields of a mate are the part it is relative to and the part it places (typed by name; a name that is not there, or is used twice, is refused), the kind (a choice), the side and the distance of a gap or the axis and the faces of an alignment, and, read only, where the placed part really is. Each change of a field is an entry of the history, and a value the model refuses is told in the status bar and leaves the mate as it was.
* A part says where its mates put it when that is not where it was typed, and which parts it shares volume with.
* The parts in conflict are drawn in the colour of danger in the 3D view and in the plan (the selected part keeps the colour of the selection), and the **fit report** has the number of mates, then the line `Interference: ...` with the pairs in conflict (the first `assembly.max_report_pairs`, then how many more) or `Interference: none (...)` when there are two parts or more.

## From the command line

`eh3d-phoenix-cad interference PROJECT.phoenixproj [--lang CODE] [--config FILE]` prints the same lines as the fit report about the assembly and exits with 0 when every step of the project was applied and no two parts share volume, and with 1 otherwise. `check` prints them at the end of its report.

## In the project file

A project with a mate writes the mechanical domain as version 5, with the steps `add-mate` and `remove-mate`; a project without one keeps the version and the bytes it had (see [PROJECT_DOMAIN.md](PROJECT_DOMAIN.md)):

```text
step 0 on%20top add-mate 018bcfe5-6800-7003-8000-000000000003 018bcfe5-6800-7001-8000-000000000001 018bcfe5-6800-7002-8000-000000000002 stack
step 0 apart add-mate 018bcfe5-6800-7005-8000-000000000005 018bcfe5-6800-7001-8000-000000000001 018bcfe5-6800-7002-8000-000000000002 gap -y 5000000
step 0 aligned add-mate 018bcfe5-6800-7004-8000-000000000004 018bcfe5-6800-7001-8000-000000000001 018bcfe5-6800-7002-8000-000000000002 align x max
```

After the id of the mate come the id of the fixed part, the id of the part it places and the kind with its values; a distance is a whole number of nanometres from zero up.

## Settings

The keys of `[assembly]` in `phoenix-cad.default.cfg`: `max_mates` (1 to 100000), `hole_clearance` (0 to 1000 mm) and `max_report_pairs` (1 to 100000). The keys `mate_kind`, `mate_direction`, `mate_distance`, `mate_axis` and `mate_edge` of `[new_step]` in `phoenix-cad-window.default.cfg` say how a mate added from the window starts. Every value is checked when the file is read.

## Examples

`crates/eh3d-phoenix-cad/examples/assembly.phoenixproj` is a gantry made of the parts of `chassis.phoenixlib`: a base rail, two posts and a top rail placed with nine mates (stacks, alignments by the lower face, the upper face and the middle), a bracket against a post (a gap and an alignment) and a bolt through the hole of the bracket, which is typed. `eh3d-phoenix-cad interference` finds no conflict and one pass in it. In `frame.phoenixproj` the four bolts go through the holes of the four brackets. The tests read both.
