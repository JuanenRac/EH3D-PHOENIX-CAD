# EH3D-PHOENIX-CAD: the 2D sketch

A sketch is a drawing on a plane of the box (points, lines, circles, arcs) that is held together by constraints. The person says what must be true (this line is horizontal, that circle is tangent to these two lines, this side is 40 mm) and the solver moves the drawing until it is. The library part is `eh3d_phoenix_cad::sketch`; it needs no window. Drawing it, and turning a closed outline into a hole or a boss of the box, come after it and are not part of this page.

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

## The sketch file

A sketch is kept in its own text file, `.phoenixsketch`, next to the project file (`sibling_path`: `box.phoenixproj` goes with `box.phoenixsketch`). It has its own version in the first line and does not change the format of the project file; keeping sketches inside the project file would need a new domain and is left for a later decision.

```text
phoenix-sketch 1
name plate
plane top 0 0 0
next 5 3
e1 point 0 0
e2 point 40000000 0
e3 line e1 e2
c1 on horizontal e3
c2 on distance_x e1 e2 40000000 = width / 2
```

Lengths are integer nanometres and angles integer nano-degrees. The line of an element may end with `construction`; a constraint is `on` or `off`; a dimension may end with ` = ` and its expression. Empty lines and lines that start with `#` are ignored. The text written is always the same for the same sketch, files are saved atomically with numbered copies and read under a size limit. A file of a newer version, a line that is not understood, or a sketch that does not hold together is refused with `PHOENIX-CAD-035` and the number of the line.

## The window

The window has a **Sketch** part beside the **Box** part (the switch in the top bar). `eh3d-phoenix-cad SKETCH.phoenixsketch` opens it on a sketch file at once, and `eh3d-phoenix-cad PROJECT.phoenixproj --view sketch` opens the project and the sketch file next to it when there is one. Everything the window decides is in `eh3d_phoenix_cad::sketch_ui` and is tested without a screen; `gui` only draws the primitives that `SketchEditor::scene` returns and passes on the clicks, drags, wheel and keys.

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
* **History and file**: undo and redo go back and forward through the changes of the window (`undo.depth` changes are kept); the sketch is opened and saved as a `.phoenixsketch` file, by name, by the buttons, with Ctrl+O and Ctrl+S, or by dropping the file on the window.
* **Canvas**: the grid (lines every `grid` millimetres, drawn every tenth line when they would crowd each other), the axes, the wheel to zoom around the pointer, the middle button or Space with a drag to pan, a double click to fit the drawing. The signs of the constraints (`H`, `V`, `//`, `+`, `T`, `=`, `C`, `O`, `S`, `M`, `F`) and the dimensions are drawn beside what they are about.

The picture mode takes a sketch: `eh3d-phoenix-cad gui box.phoenixsketch --view sketch --shot sketch.png`. `eh3d-phoenix-cad config` writes the settings of the solver and of the sketch window with the other settings.

## Codes

`PHOENIX-CAD-031` element not valid, `-032` constraint not valid, `-033` over-constrained, `-034` solver did not converge, `-035` sketch file not valid. See `ERROR_CODES.md`; the text of each is in the eight languages.

## What is not here yet

Turning a closed profile into a hole or a boss of the box, reference (driven) dimensions, ellipses and splines, tangent continuity at the end of an arc, and sketches inside the project file. The sketch has only been tried on sketches made by its own tests, with values worked out by hand. The drawing code of the window of the sketch has never been opened on a screen: its look, its layout and its behaviour with the mouse are checked by no test.
