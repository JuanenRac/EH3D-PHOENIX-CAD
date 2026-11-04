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
| PHOENIX-CAD-006 | Angle outside the range | an angle whose absolute value is not below a million degrees |
| PHOENIX-CAD-007 | Count or ratio outside its range | a count that is negative or above a million, or a ratio beyond a thousand times the whole |
| PHOENIX-CAD-008 | Number with a unit not understood | a text such as `1.6 mm` or `1/16 in` that has no number, no known unit or an invalid fraction |
| PHOENIX-CAD-009 | Integer calculation overflows | a scaling or a division that does not fit in the integers used, or a division by zero |
| PHOENIX-CAD-010 | Reference to an element that does not exist | a name of a face, edge or hole wall that is unknown, names a hole that is gone, or is not written in the canonical way; the operation that uses it fails and no other element is chosen in its place |
| PHOENIX-CAD-011 | Settings not valid | a settings file with a line that is not a section, a `key = value` or a comment, a repeated or missing key, or a value outside its limits; the message names the line or the key |
| PHOENIX-CAD-012 | File cannot be read or written | a file that cannot be created, read, replaced or copied, or that is larger than the limit given to the loader; a failed save leaves the old file as it was |
| PHOENIX-CAD-013 | Dependency cycle | a dependency that would make an object depend on itself, directly or through others; the graph is left as it was |
| PHOENIX-CAD-014 | Unknown name or id | an id, a variable, a variant or a domain that does not exist |
| PHOENIX-CAD-015 | Name or id already used | an object, a variable or a variant added under a name or an id that is taken |
| PHOENIX-CAD-016 | Object still in use | an object removed while others still depend on it |
| PHOENIX-CAD-017 | Name not valid | a variable, a variant or a binding named with something other than a letter followed by letters, digits and underscores (up to 64 characters) |
| PHOENIX-CAD-018 | Project file not valid | a project file with a wrong header, a version that is zero or newer than this program, a domain that is repeated, cut short or badly named, or a size above the limit |
| PHOENIX-CAD-019 | Edge treatment not valid | a rounded or chamfered edge whose size is larger than half of the box, that reaches the inner space, or whose lid and floor treatments are together higher than the box; the model is left as it was |
