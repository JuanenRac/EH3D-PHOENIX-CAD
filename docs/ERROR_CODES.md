# EH3D-PHOENIX-CAD: diagnostic codes

Codes have the form `PHOENIX-<AREA>-<NNN>`. A code is stable: it never changes its meaning and is never reused. Programs show the text through the key `diag.<code>`; the codes themselves are never translated.

The table repeats the `REGISTRY` of the crate `eh3d-phoenix-cad` (`src/lib.rs`); a test checks that every code of the registry is in the table with the same title.

## Mechanical model

| Code | Title | Raised by |
|---|---|---|
| PHOENIX-CAD-001 | Length outside the mechanical range | a length whose absolute value is not below 2^40 nanometres, or an outer size that would leave the range |
| PHOENIX-CAD-002 | Parameter is not positive | a size, wall, floor, lid or hole diameter that is zero or negative, or a margin or web that is negative |
| PHOENIX-CAD-003 | Hole outside its face | a hole that does not lie inside its face with the margin from the edges |
| PHOENIX-CAD-004 | Holes too close | two holes of one face with less than the minimum web between them |
| PHOENIX-CAD-005 | Hole id used twice or unknown | an operation that adds an id already used or names a hole that does not exist |
