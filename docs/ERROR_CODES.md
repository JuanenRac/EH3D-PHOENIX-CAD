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
| PHOENIX-CAD-005 | Hole or post id used twice or unknown | an operation that adds a hole or a post under an id already used, or names a hole or a post that does not exist |
| PHOENIX-CAD-006 | Angle outside the range | an angle whose absolute value is not below a million degrees |
| PHOENIX-CAD-007 | Count or ratio outside its range | a count that is negative or above a million, or a ratio beyond a thousand times the whole |
| PHOENIX-CAD-008 | Number with a unit not understood | a text such as `1.6 mm` or `1/16 in` that has no number, no known unit or an invalid fraction |
| PHOENIX-CAD-009 | Integer calculation overflows | a scaling or a division that does not fit in the integers used, or a division by zero |
| PHOENIX-CAD-010 | Reference to an element that does not exist | a name of a face, edge or hole wall that is unknown, names a hole that is gone, or is not written in the canonical way; the operation that uses it fails and no other element is chosen in its place |
| PHOENIX-CAD-011 | Settings not valid | a settings file with a line that is not a section, a `key = value` or a comment, a repeated or missing key, or a value outside its limits; the message names the line or the key |
| PHOENIX-CAD-012 | File cannot be read or written | a file that cannot be created, read, replaced or copied, or that is larger than the limit given to the loader; a failed save leaves the old file as it was |
| PHOENIX-CAD-013 | Dependency cycle | a dependency that would make an object depend on itself, directly or through others, including a parameter whose expression refers to itself or to a parameter that refers back to it; the graph is left as it was |
| PHOENIX-CAD-014 | Unknown name or id | an id, a variable, a variant or a domain that does not exist, or a name in an expression that is no parameter, variable or constant (the message gives the column) |
| PHOENIX-CAD-015 | Name or id already used | an object, a variable or a variant added under a name or an id that is taken |
| PHOENIX-CAD-016 | Object still in use | an object removed while others still depend on it |
| PHOENIX-CAD-017 | Name not valid | a variable, a variant or a binding named with something other than a letter followed by letters, digits and underscores (up to 64 characters) |
| PHOENIX-CAD-018 | Project file not valid | a project file with a wrong header, a version that is zero or newer than this program, a domain that is repeated, cut short or badly named, or a size above the limit |
| PHOENIX-CAD-019 | Edge treatment not valid | a rounded or chamfered edge whose size is larger than half of the box, that reaches the inner space, or whose lid and floor treatments are together higher than the box; the model is left as it was |
| PHOENIX-CAD-020 | Hole shape not valid | a countersink or counterbore that is not wider than its hole or that is as deep as the material, a counterbore depth that is not positive, a rectangle without height, or a slot shorter than it is wide; the model is left as it was |
| PHOENIX-CAD-021 | Screw size not in the table | a screw named in a request that the table of screws of the settings does not have |
| PHOENIX-CAD-022 | Shape not drawn by this export | a model with rounded or chamfered edges or with a countersunk, counterbored, rectangular or oval hole, asked of the STEP export, which draws only the box with straight round holes; the script export draws all of them |
| PHOENIX-CAD-023 | Expression not valid | an expression of a parameter that is not written in the language: a character that does not belong, a number with more than 18 digits, a missing parenthesis or operand, an unknown function, the wrong number of arguments, a comparison chained with another, a unit without a number; the message gives the column |
| PHOENIX-CAD-024 | Expression cannot be evaluated | an expression that is well written but makes no calculation: a length added to a number, two lengths multiplied, a division by zero, a square root that is not exact, a power with a length or a fraction as exponent, a condition that is not true or false, a result that is not a length where a length is needed; the message gives the column of the operation |
| PHOENIX-CAD-025 | Expression limit exceeded | an expression longer than the limit, nested deeper than the limit, that needs more steps than the limit allows, or with an exponent above the limit; the limits are the `[expr]` keys of the settings file |
| PHOENIX-CAD-026 | Expression arithmetic overflows | an operation of an expression whose exact result does not fit in the 128-bit numbers used (a product, a sum, a power or a comparison); the message gives the column of the operation |
| PHOENIX-CAD-027 | Post shape not valid | a post whose bore is not narrower than the post or is deeper than the post is high, whose foot is not wider than the post or is as high as the post, a post made for a board hole narrower than the thread of its screw, or whose wall around the bore is thinner than the least wall of the settings |
| PHOENIX-CAD-028 | Post meets a wall, a post or a hole | a post whose footprint (the foot when it is wider) crosses a wall of the inner space, overlaps another post or crosses a hole of the floor, or a board that does not lie inside the inner space; touching is allowed, overlapping is not |
| PHOENIX-CAD-029 | Post too high for the space above it | a post that is not lower than the inner height, or posts, board and tallest part together higher than the inner height; the message gives the figures |
| PHOENIX-CAD-030 | Lid fixing not valid | a lid fixing with a head that is not round (a rectangle or a slot), a corner inset that leaves no room between the screws, a centring lip as deep as the inner space or that leaves no opening, or a post that reaches the lip and meets it; the holes of the lid are also checked as any other hole and give their own codes |
