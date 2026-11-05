# EH3D-PHOENIX-CAD: the language of expressions

A parameter of an enclosure can be a formula instead of a number. The formula may use variables, the constants of the settings file and the other parameters, so that a box follows its master dimensions: `wall = 5 * nozzle`, `lid = wall + 1 mm`, `inner_length = board_length + 2 * wall`.

The language is small on purpose. Nothing in it uses floating point: every number is an exact fraction of two 128-bit integers, every operation checks that it does not overflow, and the same text with the same names gives the same value on every machine. It is implemented in `eh3d_phoenix_cad::expr`; the parameters that use it are in `eh3d_phoenix_cad::parametric` and the project file keeps them in `eh3d_phoenix_cad::project`.

## Values

| Kind | Examples | Kept as |
|---|---|---|
| number | `3`, `0.25`, `1 / 3` | a fraction of two 128-bit integers |
| length | `2 mm`, `1/16 in` written `(1 / 16) in`, `wall` | a fraction of nanometres |
| angle | `45 deg`, `0.25 turn` | a fraction of nano-degrees |
| boolean | `true`, `false`, `a < b` | one bit |

A number written with a point has at most 18 digits (`1.5`, `0.001`); the point is always a point, because a comma separates the arguments of a function. A length becomes a whole number of nanometres only when it is the result of a parameter: it is rounded half away from zero, the same rule as the rest of the library.

## Units

A unit is written after a number or after a part in parentheses: `3 mm`, `(a + b) mm`.

| Unit | Meaning |
|---|---|
| `nm` | nanometre |
| `um` | micrometre |
| `mm` | millimetre |
| `cm` | centimetre |
| `m` | metre |
| `in` | inch, exactly 25.4 mm |
| `mil` | thousandth of an inch, exactly 25.4 um |
| `deg` | degree |
| `turn` | full turn, exactly 360 `deg` |

The unit binds tighter than every operator: `1 / 16 in` is a number divided by a length and is refused, `(1 / 16) in` is a sixteenth of an inch.

## Grammar

```
expression  = or ;
or          = and { "or" and } ;
and         = not { "and" not } ;
not         = "not" not | comparison ;
comparison  = sum [ ( "<" | "<=" | ">" | ">=" | "==" | "!=" ) sum ] ;
sum         = term { ( "+" | "-" ) term } ;
term        = unary { ( "*" | "/" ) unary } ;
unary       = ( "-" | "+" ) unary | power ;
power       = postfix [ "^" unary ] ;
postfix     = primary [ unit ] ;
primary     = number | "true" | "false" | name | call | "(" expression ")" ;
call        = function "(" [ expression { "," expression } ] ")" ;
```

From the loosest to the tightest: `or`, `and`, `not`, the comparisons (they do not chain: `a < b < c` is refused, write `a < b and b < c`), `+ -`, `* /`, the sign, `^`, the unit. `^` groups to the right (`2 ^ 3 ^ 2` is 512) and a sign binds looser than it (`-2 ^ 2` is -4). All binary operators except `^` group to the left. Spaces and tabs separate; nothing else does, and only plain ASCII is allowed.

A name starts with a letter and holds letters, digits and underscores. The words `and`, `or`, `not`, `true`, `false` and every unit are reserved and cannot be the name of a variable or a constant. A function name is only a function when a parenthesis follows it, so a variable called `min` is still a variable.

## What can be combined

| Operation | Allowed | Result |
|---|---|---|
| `+` `-` | number and number, length and length, angle and angle | the same kind |
| `*` | number and anything | the kind of the other one |
| `/` | anything by a number; length by length; angle by angle; number by number | the kind of the left side; a number for two of one kind |
| `^` | number to a whole number | number |
| `<` `<=` `>` `>=` | two of one kind | boolean |
| `==` `!=` | two of one kind, or two booleans | boolean |
| `and` `or` `not` | booleans | boolean |

Anything else is refused with a message that says why and where. A length times a length (an area) is not a value of the language, a number cannot be added to a length (`wall + 1` needs a unit), and a power never takes a length. The exponent of `^` is a whole number whose absolute value is at most the limit `max_exponent`.

`and`, `or` and `if` evaluate only what they need: `if(width == 0 mm, 0 mm, 10 mm / width)` never divides by zero, and `false and x > 1` does not look at `x`. The names in the part that is not evaluated still count as dependencies.

## Functions

| Function | Meaning |
|---|---|
| `min(a, b, ...)` | the smallest of one or more values of one kind |
| `max(a, b, ...)` | the largest of one or more values of one kind |
| `abs(x)` | the absolute value of a number, a length or an angle |
| `floor(x)`, `floor(x, step)` | the greatest multiple of `step` (one nanometre, one nano-degree or one for a number) that is not above `x` |
| `ceil(x)`, `ceil(x, step)` | the least multiple of `step` that is not below `x` |
| `round(x)`, `round(x, step)` | the nearest multiple of `step`, half away from zero; `step` has the kind of `x` and is positive |
| `clamp(x, low, high)` | `x` held between `low` and `high`; `low` above `high` is an error |
| `sqrt(n)` | the square root of a number, only when it is exact (`sqrt(16)`, `sqrt(9 / 4)`); `sqrt(2)` is an error |
| `hypot(a, b)` | the hypotenuse of two lengths, rounded to the nearest nanometre like the rest of the library |
| `sin(a)` | the sine of an angle, from the integer table of `units`: a number with nine decimals |
| `cos(a)` | the cosine of an angle, the same table |
| `if(condition, a, b)` | `a` when the condition is true, `b` when it is false |

## Names

A name in an expression is looked up in this order: the parameters of the enclosure (`inner_length`, `inner_width`, `inner_height`, `wall`, `floor`, `lid`, `edge_margin`, `min_web`), then the variables, then the constants. A parameter that has no definition has its base value. A name that is none of them is refused with `PHOENIX-CAD-014` and the column.

A parameter that is named by an expression gives the length it has after rounding, the one the enclosure uses: if `wall` is `10 mm / 3` (3 333 333 nm), then `wall * 3` is 9 999 999 nm.

The constants come from the settings file (`phoenix-cad.default.cfg`):

* every size of the table of screws, named `screw_<name>_<field>` with the point of the name written as an underscore: `screw_M3_clearance`, `screw_M2_5_nominal`, `screw_M4_head_diameter`, `screw_M5_head_height`, `screw_M6_countersink_diameter`;
* the printer: `printer_hole_compensation`, `printer_small_hole_limit`, `printer_small_hole_extra`, `printer_fit_clearance`, and `counterbore_extra`;
* every key of the `[constants]` section, which is itself an expression that may use the sizes above but not the other constants of the section: `nozzle = 0.4 mm` and `hole = screw_M3_clearance + printer_hole_compensation` are fine.

## Limits

The limits are hard and live in the `[expr]` section of the settings file, not in the code. An expression beyond one is refused whatever it says.

| Key | Default | Meaning |
|---|---|---|
| `max_length` | 512 | characters of one expression |
| `max_depth` | 64 | levels of nesting (parentheses, signs, `not`, arguments, exponents) and depth of the tree |
| `max_steps` | 10000 | steps (nodes evaluated) in one calculation of the parameters of a box, shared by all its expressions |
| `max_exponent` | 64 | the largest absolute value of an exponent |

Each key also has a cap in the code that the file cannot raise (65 536 characters, depth 128, 100 million steps, exponent 1024), so a mistaken file cannot turn the language into a way to exhaust the machine.

## Errors

Every error carries the column (counted from 1) where the problem is.

| Code | When |
|---|---|
| `PHOENIX-CAD-023` | the text is not in the language: an unexpected character or word, a missing operand or parenthesis, an unknown function, the wrong number of arguments, a chained comparison, a unit without a number |
| `PHOENIX-CAD-024` | the text is right but the calculation is not: kinds that cannot be combined, a division by zero, a square root that is not exact, a condition that is not a boolean, a result that is not a length where a length is needed |
| `PHOENIX-CAD-025` | a limit of the settings was reached: length, depth, steps or exponent |
| `PHOENIX-CAD-026` | the exact result of an operation does not fit in 128-bit integers |
| `PHOENIX-CAD-013` | the definitions of the parameters depend on themselves, directly or through others; the model stays as it was |
| `PHOENIX-CAD-014` | a name that is no parameter, variable or constant |

A result that is a length but outside the range of lengths is `PHOENIX-CAD-001`; a set of parameters that does not make an enclosure is the usual `PHOENIX-CAD-002`.

## Dependencies and recalculation

Each variable and each parameter is an object of the graph of dependencies. A parameter depends on the variables and the parameters its expression names. When a variable, a constant or a definition changes, only the parameters that depend on it are calculated again; the others keep their saved result. The order of calculation is the same for the same model, whatever the order in which it was defined.

## Examples

`board_length` and `board_height` are variables of the design; `nozzle` is a constant of the settings.

```
wall         = 5 * nozzle
floor        = wall
lid          = wall + 1 mm
inner_length = board_length + 2 * wall + 2 * printer_fit_clearance
inner_height = max(30 mm, round(board_height + 4 mm, 0.5 mm))
edge_margin  = if(inner_length > 80 mm, screw_M3_clearance, 2.5 mm)
min_web      = clamp(wall, 1.2 mm, 3 mm)
```

## In the project file

The parameters are written in the critical domain `mechanical-parameters`: a header line, the eight base values in nanometres, the variables and one line for each defined parameter (`param wall expr 5 * nozzle`). An expression is written in its canonical form: the parentheses that are needed and a space around each operator, so reading it again gives the same expression. A file without the domain opens as before; a program that does not know the domain opens the file read-only.
