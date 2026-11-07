# EH3D-PHOENIX-CAD: the mechanical domain of the project file

This page is the contract between the mechanical tool (this repository) and the board tool (EH3D-PHOENIX-PCB) for the part of the shared project that holds the enclosure. The decision behind it is record `adr/0011-mechanical-domain-recipe.md`. The code is `eh3d_phoenix_cad::mechanical`; the golden texts of this page are tests in that module.

## What the domain keeps

The domain `mechanical` keeps the **recipe** of the enclosure, never the enclosure:

* the parameters, with their variables and expressions;
* the ordered steps (holes, edge treatments, posts, the fixing of the lid, and the extrusions and pockets of a sketch), each with its name and its suppressed flag;
* the tip (how many steps are active);
* the **interface of the board**: its outline, its mounting holes, its thickness and the height of its tallest part, which the board tool writes and the mechanical tool reads.

The final state (the box with its holes, posts and lid) is always derived: `Mechanical::build` makes it again from the recipe, so a recipe can be edited, reordered, suppressed and recalculated, and the same recipe always gives the same box. No result is stored: not a resolved size, not the holes of the lid, not a mesh.

## Where it sits in the file

The file is the envelope of `project.rs` (a header and named domains, each critical or optional, with its length). The domain is named `mechanical` and is **critical**: a program that does not understand it opens the project read-only, because a step changes the geometry.

The older domain `mechanical-parameters` (only the parameters) is still read. A project that has only that one is read as a recipe without steps and without a board; the first time the new domain is written the older one is taken away. When both are in a file the new one counts.

A domain this program does not know (another name, or this name in a newer version) is read, kept and written back **byte for byte**; the program never edits or rewrites it. The sketches of the project are in one of those domains, `sketch`, which this program does know: see *The domain `sketch`* below.

## The version of the domain

The first line of the domain is `mechanical <version>`, a whole number without a sign or leading zeros, not zero. This program reads versions 1 and 2.

* **Version 1** is the grammar below without `extrude`, `pocket` and `remove-feature`. **Version 2** adds those three steps, which refer to a sketch of the project by its name.
* This program **writes version 2 only when the recipe has one of those steps** and version 1 in every other case, so a project without extrusions and pockets is, byte for byte, the file it was before and is read by programs that only know version 1. The two spellings are not interchangeable: a text that says version 2 and has no such step, or says version 1 and has one, is not valid (`PHOENIX-CAD-018`).
* A program that finds a **higher** version does not read the domain and does not write over it. `Document::mechanical_version` tells the number without reading the rest, so the project can be opened read-only; `Document::set_mechanical` refuses and leaves the bytes as they are. A program that only knows version 1 meets a project with extrusions as a domain of a newer version.
* A domain of a lower version is migrated when it is read and written in the current one (and, as above, a recipe is written in the lowest version that can hold it). The migration that exists besides is from `mechanical-parameters`.
* A change that older programs cannot ignore raises the version. Nothing in a version is an "extension line": any line that is not in the grammar below makes the domain not valid.

## The text

The domain is text in one canonical form. The same content always gives the same bytes, and a text that is not written in that form is refused with `PHOENIX-CAD-018`, so there are never two spellings of one recipe. The line end is a single line feed and the last line ends with it.

```text
mechanical 1
[parameters]
<the text of the parameters>
[steps]
tip <number of active steps>
step <flag> <name> <operation> <words...>
...
[board]                      (only when the board tool has written it)
thickness <nm>
tallest <nm>                 (optional)
place <x> <y>                (optional)
point <x> <y>                (three or more)
hole <id> <name> <x> <y> <diameter>
```

The sections come in this order, each once; `[board]` may be absent. Words are separated by one space.

* **Lengths** are whole numbers of nanometres in plain decimal (`-5` and `1500000` are written; `+5`, `05`, `-0` and `1.5` are not), inside plus or minus 2^40. The board tool's own range (2^30) fits inside it.
* **Names** (of a step, a hole, a post, a screw) are one word: every byte that is not a letter, a digit, `.`, `_` or `-` is written as `%` and two capital hexadecimal digits, and an empty name is `~`. A byte that is never escaped is not written escaped.
* **Ids** are the canonical text of the object identity of the suite (UUID v7, lower case).
* **Flags** are `0` or `1`.

### `[parameters]`

The text of `mechanical-parameters` version 1, line for line: the header `parameters 1`, the eight `base` values, the `variable` lines and the `param` lines (a value, a variable or an expression in canonical form). See `EXPRESSIONS.md` for the language of the expressions. The base values are the ones a parameter without a definition keeps.

### `[steps]`

`tip` is the first line. A step is `step <flag> <name> <operation>`; the flag is 1 when the step is suppressed. The operations are those of `Operation`:

| Operation | Words |
|---|---|
| `add-hole <id>` / `replace-hole <id>` | `<name> <face> <u> <v> <diameter> <shape>` |
| `remove-hole <id>` | |
| `set-params` | the eight parameters in the fixed order, each as `<name>=<nm>` |
| `set-edging` | three treatments: the vertical edges, the edges of the lid, the edges of the floor |
| `add-post <id>` / `replace-post <id>` | `<name> <board hole id or -> <x> <y> <outer diameter> <height> <insert or self-tapping> <bore diameter> <bore depth> <none or foot:<diameter>:<height>>` |
| `remove-post <id>` | |
| `set-lid none` | takes the fixing away |
| `set-lid` | `<fixing> <screw> <hole diameter> <shape> <clearance> <nolip or lip:<thickness>:<depth>>` with the fixing `posts`, `corners:<inset>` or `auto:<inset>` |
| `extrude <id>` (version 2) | `<sketch> <height>`: the outline of the sketch raised outwards from its face |
| `pocket <id>` (version 2) | `<sketch> <depth or through>`: the outline of the sketch cut into its face to the depth (a whole number of nanometres) or through the whole thickness |
| `remove-feature <id>` (version 2) | takes away the extrusion or the pocket of that id |

A **shape** is `plain`, `countersunk:<head>`, `counterbored:<head>:<depth>`, `rectangular:<height>` or `oval:<length>:<u or v>`. A **treatment** is `sharp`, `round:<size>` or `chamfer:<size>`. A **face** is `front`, `back`, `left`, `right`, `top` or `bottom`. Positions of holes are measured on the face as seen from outside (`u` to the right, `v` up); the position of a post is its centre in the inner space, measured from the inner face of the left wall (`x`) and of the front wall (`y`).

A **sketch** word is the name of a sketch of the project (letters, digits, `_` and `-`, up to 64 bytes), so nothing in it is escaped. The outline is **not** in the step: it is taken from the sketch, in the domain `sketch`, every time the recipe is built (`SKETCH.md`, *Extrusions and pockets*). The face, the origin of the plane and the position of the outline come from the sketch too; a step that names a sketch the project does not have, or whose outline does not close or does not fit the face, is failed with `PHOENIX-CAD-036` or `PHOENIX-CAD-037` like any other step the box refuses.

A step is data, not a promise: a step that the box would refuse is kept in the recipe and reported as failed (with its code) when the recipe is built, and the steps after it still run. Nothing about the validity of a step is stored.

### `[board]`

Written by the board tool, read by the mechanical tool; the mechanical tool keeps it as it found it (it writes it back unchanged when it saves the domain).

* The coordinate frame is the board's: `x` to the right and `y` up as seen from the component side, origin anywhere; only differences matter.
* `thickness` is the thickness of the board and is positive. `tallest` is the height of the tallest part above the top face of the board, when known.
* `point` lines are the corners of the outline in order (the last joins the first); there are at least three. The rectangle that holds them is what is placed in the box.
* `hole` lines are the mounting holes: the id the board tool gives them, a name, the centre and the diameter. Ids are unique. The post made for a hole has the id `derived_id(hole id)` (the same time and counter, the last byte changed), so the same hole always gives the same post.
* `place` is where the lower left corner of the rectangle of the outline lies in the inner space of the box. Without it the board is centred.

The order of the lines is fixed: `thickness`, `tallest`, `place`, the points, the holes.

## Who writes what

| Part | Written by | Read by |
|---|---|---|
| `[parameters]`, `[steps]` | the mechanical tool; the board tool when it writes the box (see below) | both |
| `[board]` | the board tool | the mechanical tool |
| the domain `sketch` | the mechanical tool | the mechanical tool (the board tool keeps it as it is) |

The board tool writes the box by writing steps with the grammar above (for example one `add-post` for each mounting hole, or all of them through `mount::posts_for_board`, and a `set-lid`), and saves the whole domain in the canonical form. It must keep `[parameters]`, the steps it does not understand and the tip as it found them. The easiest way is to use `eh3d_phoenix_cad::mechanical` and not to write the text by hand.

## The domain `sketch`

The sketches of the project are in a domain of their own, **optional** (a program that does not know it opens the project and keeps it byte for byte), with its own version in its first line:

```text
sketch 1
[sketch]
<the text of one sketch, as in the sketch file: phoenix-sketch 1, name, plane, next, e1.., c1..>
[sketch]
<the next one>
```

* The sketches come in the order of their names, which are unique; the text is in one canonical form (the same sketches always give the same bytes) and a text that is not in it is refused with `PHOENIX-CAD-035`, which names the sketch and the line. The grammar of one sketch is in the header of `sketch::file`.
* A project **without** the domain has no sketch. Writing a project without sketches writes no domain, and takes away one that is there.
* A domain of a version higher than 1, or one that cannot be read, is **not** read or written: the project opens read-only with the error, the steps that need a sketch are failed, and the bytes stay as they were. The version is read by `Document::sketch_version`, the sketches by `Document::sketches` and written by `Document::set_sketches`.
* The sketch file `.phoenixsketch` next to a project is a way in and a way out, not a part of the format: see `SKETCH.md`.
* The limits are keys of the sketch settings (`phoenix-cad-sketch.default.cfg`, section `[sketch]`): `max_sketches` (64), `max_file_bytes` (1 MiB for each sketch written, and that times `max_sketches` for the whole domain read), `max_elements` and `max_constraints`.

### Compatibility and migration

| The file | A program of this version | A program that only knows version 1 of `mechanical` |
|---|---|---|
| written before the sketches existed (no `sketch`, `mechanical 1` or `mechanical-parameters`) | opens it as it always did; writing it again gives `mechanical 1` | the same |
| with sketches but no extrusion or pocket (`mechanical 1`, `sketch 1`) | opens it, edits it | opens it, edits it and keeps `sketch` as it found it |
| with extrusions or pockets (`mechanical 2`, `sketch 1`) | opens it, edits it | meets a domain of a newer version: does not read it and does not write over it |
| with a `sketch` domain of a version it does not know, or damaged | opens it read-only | (not applicable) |

A sketch that was kept in a `.phoenixsketch` file next to the project is migrated by opening it and keeping it in the project; nothing is converted behind the person's back.

## Limits

The limits are keys of the settings file, section `[mechanical]` (hard caps in brackets): `max_bytes` (the whole domain; 16 MiB by default, cap 256 MiB), `max_steps` (10 000, cap 1 000 000), `max_name_bytes` (256, cap 4096, counted before escaping), `max_outline_points` (10 000, cap 1 000 000) and `max_board_holes` (1 000, cap 100 000). A domain over a limit is refused when it is read **and** when it is written. The expressions are read under the limits of `[expr]`.

## Errors

Every defect of the text is `PHOENIX-CAD-018` (project file not valid). The message says what is wrong in words; it never copies a name, an id or a value from the file. A program must treat a domain that fails to read as read-only data and must not overwrite it.

## What the recipe builds

The geometry of the posts and the lid is written in `mount.rs` and is described here only as far as the contract needs it:

* A post is a cylinder on the floor with a bore for a threaded insert or a self-tapping screw (the sizes come from the `[fixing.<screw>]` tables of the settings), and an optional conical foot.
* The lid is fixed with screws into the posts: the lid has a through hole with a recess for the head over each post, or over the four corners when the box has no posts (`auto`), and optionally a lip on its underside that centres it with the clearance of the printer profile. The holes of the lid are derived from the posts, so they follow a post that moves.
* An extrusion or a pocket is the outline of a sketch of the project raised from a face or cut into it; it is checked when a step is applied (the sketch, its plane, the outline, the face, the margin of a pocket and the thickness) and gives `PHOENIX-CAD-036` or `PHOENIX-CAD-037`. Its volume goes to the material of the status bar. The STEP export and the model for fabrication refuse such a box.
* Posts and lid are checked when a step is applied (collision with a wall, another post or a hole of the floor, the height against the board and its tallest part, the lip against the posts); the codes are `PHOENIX-CAD-027` to `PHOENIX-CAD-030`.

## Conformance

The tests of `mechanical.rs` hold the golden texts of this page: the smallest recipe, every kind of step, and the board section; the round trip of a whole recipe and of random recipes through the text and through the project file; every cut of a good domain refused; the non-canonical spellings refused; the limits on both sides; the migration from `mechanical-parameters`; a domain of a newer version kept byte for byte, the steps of version 2 (their golden text, the version that goes with them and the two ways a version and the steps can disagree), and, for the domain `sketch`, the round trip with other domains around it, the order of the names, the non-canonical spellings, a domain of a newer version and a damaged one kept as they are, and a project written before the sketches existed.

## Not decided yet

* How the board tool is told that a recipe step failed after it moved a mounting hole (the build reports the state of each step; the way it reaches the board tool is not written).
* Whether the posts of a board are regenerated when the board moves; today they are made once by `posts_for_board` and edited as steps.
* Several boards in one box.
