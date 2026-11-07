# EH3D-PHOENIX-CAD: the 2D sketch

A sketch is a drawing on a plane of the box (points, lines, circles, arcs) that is held together by constraints. The person says what must be true (this line is horizontal, that circle is tangent to these two lines, this side is 40 mm) and the solver moves the drawing until it is. The library part is `eh3d_phoenix_cad::sketch`; it needs no window. The window draws it (see *The window*), the sketches of a project are kept in the project file (see *The sketches in the project file*), and a closed outline can be raised from a face or cut into it (see *Extrusions and pockets*).

## The plane

A sketch lives on a **plane**: one of the six faces of the box, an origin given as a distance along the two axes of the face (`u0`, `v0`) and an offset along the normal of the face, positive outwards. The axes are the ones of the frame of the face (`datum::Datum::of_face`), so a point of the sketch becomes a point of the model with `SketchPlane::to_model`, and a point of the model that lies in the plane becomes a point of the sketch with `from_model` (a point off the plane is refused). All coordinates are `Length`, integer nanometres, like the rest of the model.

## The geometry

| Element | Made of | Unknowns of the solver |
|---|---|---|
| Point | two coordinates | `x`, `y` |
| Line | two points | none: it follows its points |
| Circle | a centre point and a radius | `radius` |
| Arc | a centre and a start and an end point, counter-clockwise from the start to the end | none; the end is kept as far from the centre as the start is |
| Polyline | points and the lines between them (the last back to the first when closed); the lines share their points, so nothing has to be glued | the points |
| Rectangle | four points, four lines and four constraints (two horizontal, two vertical) | the points; four degrees of freedom are left (the position of a corner, the width and the height) |

Any element can be marked as **construction**: it guides the drawing and takes part in the constraints, and it is not part of the profile.

The identifiers are counters inside the sketch (`e1`, `e2` for elements and `c1`, `c2` for constraints). The same steps give the same identifiers and an identifier that has been removed is never given again. An element that another element or a constraint is made of cannot be removed.

## The constraints

Geometric:

| Constraint | Holds when | Rows |
|---|---|---|
| Coincident | two points are the same | 2 |
| Point on object | a point lies on a line (the whole line, not only the segment), a circle or the circle of an arc | 1 |
| Horizontal / Vertical | a line, or two points, share the second / first coordinate | 1 |
| Parallel | two lines are parallel | 1 |
| Perpendicular | two lines are perpendicular | 1 |
| Tangent | a line touches a circle or an arc, or two of those touch | 1 |
| Equal | two lines have the same length, or two circles or arcs the same radius | 1 |
| Symmetric about a line | two points are mirror images across a line | 2 |
| Symmetric about a point | two points are mirror images through a point | 2 |
| Midpoint | a point is the midpoint of a line | 2 |
| Fixed | a point is held where it was when the constraint was added | 2 |

Dimensional (each has a value):

| Dimension | Value |
|---|---|
| Distance | between two points, positive |
| Distance X / Y | the coordinate of the second point minus the one of the first (it has a sign) |
| Length | of a line, positive |
| Radius / Diameter | of a circle or an arc, positive |
| Angle | from the direction of the first line to the direction of the second, counter-clockwise |

A constraint is checked when it is added: the elements must exist and be of the right kind, the same element cannot be tied to itself, and a dimension that must be positive is positive (`PHOENIX-CAD-032`). A constraint can be turned off without removing it; the solver ignores it.

Two choices that the solver makes from where the drawing is when it starts, because the equations hold on both sides: a **tangent** keeps the side of the line the centre is on (or whether two circles touch from outside or one lies inside the other), and an **angle** is a direction, so an angle and the same angle plus a half turn are the same constraint and the one nearer to the drawing is kept. Draw the sketch roughly as it should be and the solver keeps it that way.

## Dimensions that follow the variables

A dimension holds a value and, if wanted, an **expression**: a variable of the project (`width`), or a formula over variables and the constants of the settings (`width / 2 + 2 mm`, `30 deg + 15 deg`), in the language described in `EXPRESSIONS.md`. `Sketch::resolve_dimensions(variables, settings)` calculates every expression and sets the values. It is all or nothing: when one expression fails (an unknown name, a length where an angle is wanted, a result that must be positive and is not) nothing changes and the error names the dimension.

## The solver

Every constraint gives one or two **residuals**, numbers that are zero when the constraint holds (a distance minus its value; for two lines that must be parallel, the offset of the second end of one from the other line). The unknowns are the coordinates of the points and the radii of the circles, in millimetres. The solver looks for the values that make every residual zero:

1. it starts from the positions of the sketch;
2. at each step it builds the Jacobian by central differences and solves `(J'J + lambda I) d = -J'r` (damped least squares, Levenberg and Marquardt) by Cholesky; the step is cut to the largest step of the settings;
3. the step is kept when the sum of squares goes down, and the damping then shrinks; otherwise the damping grows, until a step is kept or the damping passes its maximum;
4. it stops when no residual is larger than the tolerance, when it has used all its steps, or when no step gets nearer.

When it succeeds the points and radii are rounded to the nanometre and written into the sketch. For the sketch of a rectangle of 40 by 25 mm with its centre held at the origin, the corners come out at exactly (-20, -12.5), (20, -12.5), (20, 12.5) and (-20, 12.5) mm.

The result is one of three:

| Outcome | Meaning | The sketch |
|---|---|---|
| Solved | The constraints hold. The **degrees of freedom** left are the unknowns minus the rank of the Jacobian; zero means fully constrained. | moved to the solution |
| Over-constrained | Some constraints add nothing to the ones before them (their rows of the Jacobian are a combination of earlier rows). **Redundant**: the sketch has a solution and the constraint repeats others. **In conflict**: it has none and the solver stopped at the least bad place. Each is listed with the earlier constraints it depends on. | unchanged |
| Not converged | The solver used all its steps, or could not get nearer without being at a conflict. The message gives the steps and the largest residual. | unchanged |

`Outcome::into_result` turns the last two into the errors `PHOENIX-CAD-033` and `PHOENIX-CAD-034`. `Sketch::degrees_of_freedom` counts the degrees of freedom of the sketch where it is, without moving anything.

The solver is deterministic bit for bit: it uses only sums, products, quotients and square roots (which every platform computes the same way, with no fused operations) and, for angles, the integer table of sines of `units::Angle`; the loops run in a fixed order. Solving the same sketch with the same settings gives the same bits on every computer; solving a solved sketch changes nothing.

Limits to know: the matrices are dense, so a sketch is limited by the settings to a few hundred elements; the solver finds a solution near the drawing and does not look for the other ones; a conflict is found by comparing rows of the Jacobian, so two constraints that are nearly but not exactly dependent (closer than the rank tolerance) are treated as dependent.

## The settings

All tuned values are in `phoenix-cad-sketch.default.cfg` (section `[sketch]`), read with `SketchSettings::with_user_config`; the code holds none. A user file only needs the keys it changes, and can be the same file as the other settings.

| Key | Default | Meaning |
|---|---|---|
| `tolerance` | 0.00000001 | mm: the largest residual of a solution |
| `max_iterations` | 100 | steps before the solver gives up |
| `max_step` | 50 | mm: the largest move of any coordinate in one step |
| `fd_step` | 0.0001 | mm: the step of the central differences |
| `rank_tolerance` | 0.000001 | relative size under which a row adds nothing |
| `damping_start`, `damping_factor`, `damping_max` | 0.001, 10, 1e12 | the damping of the least squares |
| `stall_tolerance` | 0.0000001 | the gradient under which the solver is at a minimum that is not a solution |
| `max_elements`, `max_constraints` | 500, 1000 | the most of each in a sketch |
| `max_file_bytes` | 1048576 | the largest sketch file read |
| `max_sketches` | 64 | the most sketches a project keeps in its file |
| `profile_join` | 0.001 | mm: two ends of lines and arcs this near (on each axis) are the same corner of an outline |
| `profile_min_area` | 0.01 | mm2: an outline, or a hole in it, with less area than this is refused |
| `profile_chord` | 0.005 | mm: the most an arc or a circle sags from the straight lines that stand for it in the checks and in the drawing |

## The sketches in the project file

The sketches of a project are kept **inside the project file**, in a domain of their own named `sketch`, beside the domain `mechanical`. The envelope of the project does not change: a domain is a name, `optional` or `critical`, its length in bytes and its bytes. The domain `sketch` is `optional` and has its own version in its first line, so a program that does not know it opens the project, keeps the domain byte for byte and writes it back as it was. A project that has no sketch has no such domain and is the same file as before. The code is `sketch::domain` and the contract is in `PROJECT_DOMAIN.md`.

```text
sketch 1
[sketch]
phoenix-sketch 1
name plate
plane top 0 0 0
next 5 3
e1 point 0 0
e2 point 40000000 0
e3 line e1 e2
c1 on horizontal e3
c2 on distance_x e1 e2 40000000 = width / 2
[sketch]
phoenix-sketch 1
name ...
```

Each `[sketch]` is followed by the text of one sketch in the form below, and the sketches come in the order of their names (a name is used once). Lengths are integer nanometres and angles integer nano-degrees. The line of an element may end with `construction`; a constraint is `on` or `off`; a dimension may end with ` = ` and its expression. The text written is always the same for the same sketches. A text that is not in that one form, a sketch that does not hold together, two sketches of one name or more sketches than `max_sketches` allows is refused with `PHOENIX-CAD-035`, and the message names the sketch.

* **Old projects**: a project without the domain opens as it always did, with no sketch, and saving it writes no domain.
* **A domain this program does not know to read**: of a newer version, or damaged, the project opens **read-only** with the error in the status bar, the steps that need the sketch are shown as failed, and nothing can be written over it; the bytes stay as they were.
* **The sketch file** (`.phoenixsketch`, `sketch::file`) is the same text of one sketch in a file of its own, with its own version, written atomically with numbered copies and read under `max_file_bytes`. It is no longer where a project keeps its sketch: it is the way **in** (a file is opened in the window and kept in the project with *Keep in project*, or `Window::import_sketch_file` does both in one step, as one entry of the history) and the way **out** (*Save* writes the sketch being edited, `Window::export_sketch_file` one of the project). Opening a project that has no sketch of its own, with `--view sketch`, opens the file of that name next to it (`box.phoenixproj` goes with `box.phoenixsketch`) when there is one, so that the sketches that were kept that way can be migrated: keep the sketch in the project, save the project, and the file next to it is no longer needed.
* **The name** of the sketch is what the steps refer to. The box beside the button *Keep in project* gives the sketch the name typed (letters, digits, `_` and `-`); keeping a sketch under a name that the project already has replaces that sketch, and the steps that use it are built again from the new one.

## The window

The window has a **Sketch** part beside the **Box** part (the switch in the top bar). `eh3d-phoenix-cad SKETCH.phoenixsketch` opens it on a sketch file at once, and `eh3d-phoenix-cad PROJECT.phoenixproj --view sketch` opens the project and, in the Sketch part, the first sketch of the project (or the file next to the project when the project has none). Everything the window decides is in `eh3d_phoenix_cad::sketch_ui` and is tested without a screen; `gui` only draws the primitives that `SketchEditor::scene` returns and passes on the clicks, drags, wheel and keys.

* **Tools**: select, point, line, polyline, rectangle, circle and arc, and a switch that draws the next lines, circles and arcs as construction geometry. A line takes two clicks, a polyline one click for each corner (Enter, a right click, a double click or a click on its first corner ends it, and the whole polyline is one step of the history), a rectangle two opposite corners, a circle its centre and a point of it, an arc its centre, its start and its end (the end is brought to the circle of the start).
* **Snaps**: a click lands on a point that exists, else on a line, circle or arc that exists (the new point is then tied to it with *point on object*), else on the grid, else where the pointer is. A line that ends on a point uses that point, so nothing has to be glued. A corner of a rectangle clicked on a point is made *coincident* with it.
* **Proposed constraints**: a new line within a few degrees of horizontal or vertical is made so, and the window says it before the second click; nothing is added when it would make the sketch unsolvable. The angle and every other tuned value are in the `[sketch_ui]` section of the settings.
* **Selection**: a click picks a point first and then the nearest line, circle or arc; Ctrl adds or takes away; the lists of elements and constraints pick too. A selected constraint draws what it is about in its own colour.
* **Constraints from the selection** (the buttons of the left panel and the menu of a right click on the canvas offer only what fits):

| Selection | Constraints offered |
|---|---|
| two points | coincident, horizontal, vertical |
| lines | horizontal, vertical (each line), parallel, perpendicular and equal (two lines) |
| a point and a line, circle or arc | point on object; with a line also midpoint |
| a line and a circle or arc, or two of those | tangent; equal for two circles or arcs |
| three points, or two points and a line | symmetric about a point, or about the line |
| points | fixed (each one where it is) |

* **Dimensions** (the right panel): the distance, the horizontal and vertical distance of two points, the length of a line, the radius or diameter of a circle or arc and the angle of two lines. A dimension starts with the value the drawing has, or with the text typed in the box above the buttons; the value of a dimension that exists is edited in its own line. The text is a number (millimetres, or degrees for an angle), a length with its unit, a variable of the project (`width`) or a formula (`width / 2 + 2 mm`); a number stays a plain value and anything else is kept as the expression of the dimension and follows the variables. A text the model refuses leaves the sketch as it was.
* **Dragging**: a point, a line (both its points), a circle or an arc (its centre) is dragged with the solver running at each move, with the dragged points held at the pointer, so what is tied to them follows and what cannot move stays. When the constraints between the dragged points forbid holding all of them, only the first is held. A point dropped on another point is made coincident with it. The whole drag is one step of the history, and Escape puts everything back.
* **State of the sketch**: an indicator and a sentence after every change. *Green*: fully constrained. *Amber*: degrees of freedom left (or nothing drawn). *Red*: over-constrained (the constraints that repeat or contradict the others are listed, drawn in the colours of the list, and one button switches them off, the latest named one first, until the sketch holds together) or no solution found. A constraint that makes the sketch red is kept so that it can be seen; each constraint has a switch to turn it off and a button to remove it.
* **History and file**: undo and redo go back and forward through the changes of the window (`undo.depth` changes are kept); the sketch is opened and saved as a `.phoenixsketch` file, by name, by the buttons, with Ctrl+O and Ctrl+S, or by dropping the file on the window; the button *Keep in project* puts the sketch in the project under the name typed beside it.
* **Canvas**: the grid (lines every `grid` millimetres, drawn every tenth line when they would crowd each other), the axes, the wheel to zoom around the pointer, the middle button or Space with a drag to pan, a double click to fit the drawing. The signs of the constraints (`H`, `V`, `//`, `+`, `T`, `=`, `C`, `O`, `S`, `M`, `F`) and the dimensions are drawn beside what they are about.

The picture mode takes a sketch: `eh3d-phoenix-cad gui box.phoenixsketch --view sketch --shot sketch.png`. `eh3d-phoenix-cad config` writes the settings of the solver and of the sketch window with the other settings.

## Extrusions and pockets

A closed outline of a sketch can be **raised** from the face of its plane (`Extrude`, which adds material outwards) or **cut** into it (`Pocket`, which takes it away inwards, down to a depth or through the whole thickness of the face). Both are steps of the recipe, so they are in the history (undo and redo), in the tree of the model (the group *Extrusions and pockets*), in the volume of the status bar, in the lines of the 3D view and the plan, and in the project file (`PROJECT_DOMAIN.md`). A step names the sketch and a height or a depth; **the outline is taken from the sketch every time the box is built**, so a change of the sketch changes the extrusion, and a sketch that no longer closes makes the step fail with its code while the other steps still run.

The profile (`eh3d_phoenix_cad::profile`) is taken from what is not construction geometry:

1. lines and arcs are joined end to end into chains (two ends within `profile_join` on each axis are one corner); every corner must have exactly two ends, a chain must close, and a line or an arc that ends where it starts is refused; a circle is an outline of its own; points alone are ignored;
2. each outline is checked: at least three corners, no crossing or touching itself (exact in integers; an arc or a circle stands in the check for straight lines that sag by at most `profile_chord`), no folding back over a line, and an area of at least `profile_min_area`;
3. two outlines may not cross or touch; the outlines are then sorted by containment: the outermost ones bound **regions**, the ones inside them are **holes**, the ones inside a hole are islands that bound regions again, and so on. Outer outlines run counter-clockwise and holes clockwise, whatever way they were drawn.

Every failure is `PHOENIX-CAD-036` and says where (the corner, in millimetres). A rectangle of 40 by 25 mm with a circle of diameter 6 mm inside it is one region with one hole; raised 10 mm it adds `40 * 25 * 10 - pi * 3^2 * 10` = 9717.2567 mm3.

A step is accepted when:

* the sketch exists and the plane of the sketch is on its face with **no offset** (a sketch on a plane off its face is refused);
* the outline lies inside the face, in the frame of the face (`u0`, `v0` of the plane and then the coordinates of the sketch); for a pocket it also keeps the `edge_margin` of the box from the edges, like a hole;
* the height or depth is positive and a pocket is not deeper than the material of the face (the wall, the lid or the floor); a pocket **through** has the thickness of the face and follows it when the thickness changes;
* when the parameters of the box change, the extrusions and the pockets are checked again with them and a change that no longer fits is refused like any other.

Those are `PHOENIX-CAD-037` (and `PHOENIX-CAD-005` for an id used twice). A refused step leaves the box as it was.

The volume is the area of the profile (the polygon of the corners with the circular segments of the arcs, or pi r2 for a circle) times the height or depth, rounded to a cubic nanometre. It is a floating point figure, so the volume of a model that has an extrusion or a pocket is marked as not exact; the extrusions are added to the material and the pockets taken away, and where a pocket meets a hole of the same face both are counted.

The STEP export and the model for fabrication **refuse** a box with an extrusion or a pocket (`PHOENIX-CAD-022`) and the script says in a comment that it does not draw them: none of them draws a shape that is not a box with holes yet. The 3D view and the plan draw the outline on the face and at the height or depth it goes to, joined by a line at each corner (an extrusion in the colour of the posts, a pocket in the one of the holes).

## Codes

`PHOENIX-CAD-031` element not valid, `-032` constraint not valid, `-033` over-constrained, `-034` solver did not converge, `-035` sketch file not valid (the sketch file and the domain of the project), `-036` profile not valid and `-037` extrusion or pocket not valid. See `ERROR_CODES.md`; the text of each is in the eight languages.

## What is not here yet

Reference (driven) dimensions, ellipses and splines, tangent continuity at the end of an arc, extruding only some of the outlines of a sketch, tapered or symmetric extrusions, sketches on a plane with an offset or on a face of an extrusion (the faces of an extrusion have no names yet), a pocket checked against the holes and posts it meets, and the drawing of extrusions and pockets in the STEP file and the script. The sketch has only been tried on sketches made by its own tests, with values worked out by hand. The drawing code of the window of the sketch has never been opened on a screen: its look, its layout and its behaviour with the mouse are checked by no test.
