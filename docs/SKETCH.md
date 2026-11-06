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

## Codes

`PHOENIX-CAD-031` element not valid, `-032` constraint not valid, `-033` over-constrained, `-034` solver did not converge, `-035` sketch file not valid. See `ERROR_CODES.md`; the text of each is in the eight languages.

## What is not here yet

The window of the sketch (drawing, dimensioning, showing the state of the constraints), turning a closed profile into a hole or a boss of the box, reference (driven) dimensions, ellipses and splines, tangent continuity at the end of an arc, and sketches inside the project file. The sketch has only been tried on sketches made by its own tests, with values worked out by hand.
