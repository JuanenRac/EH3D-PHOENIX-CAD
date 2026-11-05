# EH3D-PHOENIX-CAD: not yet

Good ideas that are not in the first version. Writing them here avoids implementing them early.

| Idea | Why not yet | Revisit when |
|---|---|---|
| Full parametric CAD with a constraint solver | Needs the first enclosure generator and a chosen geometry kernel | after the enclosure generator works |
| Finite-element or CFD coupling | Belongs to the thermal lab | after the thermal lab's second stage |
| Functions defined by the person, areas and volumes as values, a tangent and inverse trigonometric functions, and variables that hold expressions | The language of expressions is done (see `docs/EXPRESSIONS.md`) with lengths, angles, numbers and booleans; a variable is still a length. These need decisions on the units of an area, on rounding of the inverse functions and on whether a variable may depend on a parameter | when a design needs them |
| Standoffs, ribs and board supports made from the board | Needs the mechanical domain of the project format, whose contract is not accepted | when that contract is accepted |
| Lid fixing (a lip, tabs or screws) | Proposal, requires a decision on the kind of fixing; the clearance of the printer profile is ready for it | when the kind is chosen |
| What the rest of the mechanical domain of a project holds | The envelope of the file exists and the parameters of the enclosure have their domain (`mechanical-parameters`); the holes, the steps and the rest belong to the contract that is not accepted | when that contract is accepted |
| Rounded and chamfered edges, countersinks, counterbores and slots in the STEP file | The file draws the box with straight round holes only; it needs cylindrical, conical and toroidal faces and a closed shell around each of them, and no CAD program has opened even the plain file. Until then `export_step` refuses such a model | after a CAD program has opened the plain file |
| Exact moments for a countersink, a slot and the treated edges | The solid is summed from boxes and cylinders; a countersink is taken as a cylinder of its mean diameter, a slot as the rectangle that holds it, and the treatment of the edges is left out, and the figure says so | when a part with a cone, a stadium or a fillet has its own exact moments |
