# EH3D-PHOENIX-CAD: not yet

Good ideas that are not in the first version. Writing them here avoids implementing them early.

| Idea | Why not yet | Revisit when |
|---|---|---|
| Full parametric CAD with a constraint solver | Needs the first enclosure generator and a chosen geometry kernel | after the enclosure generator works |
| Finite-element or CFD coupling | Belongs to the thermal lab | after the thermal lab's second stage |
| A formula language for parameters (an expression such as `wall * 2 + 1 mm` with units) | Proposal, requires a decision: it is not in SCOPE. The library binds a parameter to one named variable instead. Options: keep one variable per parameter; a small language with only `+ - * /`, parentheses and variables, evaluated in integers or ratios with the rounding of the units module; or a full expression language | when the scope is widened |
| Rounded corners and chamfers of the box | The script and the STEP writers do not draw them yet, and the names of the new edges and faces must be settled first | after the first release of the box with holes |
| Countersunk, counterbored, rectangular and oval holes | The table of screws is in the settings file, but a hole is still round and straight in the model, the script and the STEP file | with the rounded corners |
| Standoffs, ribs and board supports made from the board | Needs the mechanical domain of the project format, whose contract is not accepted | when that contract is accepted |
| Lid fixing (a lip, tabs or screws) | Proposal, requires a decision on the kind of fixing; the clearance of the printer profile is ready for it | when the kind is chosen |
| What the mechanical domain of a project holds | The envelope of the file exists; the content belongs to the same contract | when that contract is accepted |
| Centre of mass and moments of inertia | The volume and the mass exist; the rest needs the volume of each part of the box with its holes | after the volume of the holes is exact for every position |
