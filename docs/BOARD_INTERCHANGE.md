# EH3D-PHOENIX-CAD: the board and the box in a project of the suite

A project of the suite is one `.eh3d` file with a branch for each program (the format is described in the shared repository, `docs/FORMAT_EH3D.md`). This page says what this program keeps in its branch, what it reads from the branch of the board program and what it hands back.

**State:** the files below are defined and read by this repository only. The board program does not write `outline2d.json` yet, so everything has been tried with boards made up by tests. The contract should move to the shared repository when the board program adopts it.

## What this program writes

| Where | What |
|---|---|
| `cad/project.phoenixproj` | the project of this program, byte for byte |
| `cad/board/outline2d.json` | the copy of the board the box was made from or last updated to |
| `cad/export/solid.stl`, `cad/export/panels.dxf`, `cad/export/panels.svg` | the solid and the flat faces (`eh3d-publish`) |
| `cad/mounts2d.json` | the posts in the frame of the board (`eh3d-publish`) |
| `cad/origin/` | the original of an import and its report (`eh3d-import`) |
| `docs/cad/` | the report and the bill, with the index of documents |
| `common/meta/meta.json` | title and description (`eh3d-meta`) |

No other branch is written. The record of `cad` states the reference `pcb`, artifact `outline2d`, version 1, with the content hash `pcb` had; when `pcb` changes, the reference no longer matches and the shared checks report `EH3D-PKG-019` until the box takes the board again (`eh3d-board`).

## `pcb/outline2d.json` (read)

JSON with every length a whole number of nanometres in the frame of the board, `y` upwards as seen from the component side.

```json
{
  "artifact": "outline2d",
  "version": 1,
  "units": "nm",
  "outline": [[0, 0], [80000000, 0], [80000000, 50000000], [0, 50000000]],
  "thickness": 1600000,
  "tallest": 12000000,
  "holes": [{"id": "00000000-0000-7000-8000-000000000001", "name": "H1", "x": 4000000, "y": 4000000, "diameter": 4000000}],
  "parts": [{"name": "C1", "x": 20000000, "y": 20000000, "width": 10000000, "depth": 10000000, "height": 12000000, "side": "top"}],
  "connectors": [{"name": "J1", "edge": "front", "at": 40000000, "width": 10000000, "height": 5000000, "above": 0}]
}
```

* `artifact`, `version`, `units`, `outline` (three points or more) and `thickness` are required.
* A hole without `id` gets one worked out from its name and position; with an `id` (a UUID v7) the post follows the hole when it moves or is renamed.
* `tallest` may be left out: the tallest part of the top face counts.
* `parts` are the rectangle each part covers (`x`, `y` its lower left corner) and its height above its face; `side` is `top` (the default) or `bottom`.
* `connectors` name the side they look out of (`front` and `back` are the smallest and largest `y`, `left` and `right` the smallest and largest `x`), where their centre is along it, the size of the opening and how far above the top face it starts.
* The limits of the mechanical settings (points, holes, name length, bytes) apply, with at most 10,000 parts and 1,000 connectors. Unknown fields are ignored; another version is refused with `PHOENIX-CAD-066`.

## `cad/mounts2d.json` (written)

`{"artifact": "mounts2d", "version": 1, "units": "nm", "mounts": [...]}` with, for each post, its `id`, `name`, `x`, `y` in the frame of the board, `bore_diameter`, `outer_diameter`, `height` and, for a post made for a mounting hole, `board_hole`. The board program may read it and move its holes in its own branch; this program never writes `pcb`.

## Commands

```
eh3d-save PROJECT FILE.eh3d        eh3d-open FILE.eh3d --out PROJECT     eh3d-info FILE.eh3d
eh3d-new FILE.eh3d                 eh3d-board FILE.eh3d                  eh3d-check FILE.eh3d
eh3d-openings FILE.eh3d [--apply]  eh3d-publish FILE.eh3d                eh3d-import FILE.eh3d OTHER
eh3d-meta FILE.eh3d --title TEXT --lang CODE
```

The exit code is 0 when everything agrees, 2 when the verification did not pass (something out of date, a part without room, a post that could not follow, an export refused) and 1 when the command could not run. Run `eh3d-phoenix-cad help` for every option.

## The window

The File menu has the submenu *Suite project (.eh3d)*, in the eight languages:

| Entry | What it does |
|---|---|
| Open suite project | Opens a `.eh3d` file. A file that holds only the branches of other programs gives a new box that is saved into it; opening writes nothing. |
| Save suite project | Writes the branch `cad` of the file of the window; a project that is not in a `.eh3d` file yet is asked for a name. |
| Save suite project as | Takes every branch of the open file to the new name. A file that holds the box of another project is written over only with the box *Write over an existing file* ticked. |
| Import a file and keep the original | As `eh3d-import`: the original in `cad/origin/` byte for byte with the report; the warnings of the reader are listed as not read. |
| Branches of the project | Each branch with its program, format, files and state (current, out of date and which branch changed, signed, sealed, encrypted), the findings of the checks, the report of the original and the suite files opened last. |
| Take the board from the branch pcb | As `eh3d-new` when there is no box and as `eh3d-board` when there is one; refused while there are changes that are not saved. |

A signature is only shown, never checked. The plain *Save as* of the File menu keeps every branch too. A file that was opened, saved or picked is added to the recent files only when that worked, and a recent file that is gone is taken out of the list; the list is kept between sessions in the settings file given with `--config`. All of this is decided in `suitewin`, which is tested without a screen; the window has never been opened on a screen.

## The box made from a board

`eh3d-new` takes what it is not told, and the window takes everything, from the section `[suite]` of `phoenix-cad-window.default.cfg`:

| Key | Default | Meaning |
|---|---|---|
| `board_gap` | 2 mm | the room between the board and the walls |
| `board_headroom` | 3 mm | the room above the tallest part |
| `post_height` | 8 mm | the height of the post under each mounting hole |
| `post_screw` | M3 | the screw of the posts, from the table of screws |
| `post_kind` | insert | `insert` or `self-tapping` |

## Selectors, parameters and flat faces

`select PROJECT SELECTOR` answers with stable names: `>X <X >Y <Y >Z <Z` (one face), `|X |Y |Z` (the faces along an axis), `#X #Y #Z` (the faces across it), `faces`, `edges|X|Y|Z`, `holes`, `holes@` followed by a face selector and `holes[d<3mm]` with `<`, `<=`, `=`, `>=` or `>`.

`params PROJECT [--limits FILE]` reads limits written one to a line as `parameter.min`, `parameter.max` or `parameter.step` `= length`.

`panels PROJECT OUT.dxf|OUT.svg` draws each face as seen from outside at its outer size, with its holes; extrusions and pockets are not drawn, and there are no joints or kerf allowance.
