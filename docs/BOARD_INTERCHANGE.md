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
eh3d-keygen --out SECRET_KEY       eh3d-sign FILE.eh3d --key SECRET_KEY [--branch NAMES] [--project]
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
| Branches of the project | Each branch with its program, format, files and state (current, out of date and which branch changed, signature, sealed, encrypted), the findings of the checks, the report of the original and the suite files opened last; a button chooses a public key that checks the signatures. |
| Title and description | The title and the description of the project in each language (below). |
| Take the board from the branch pcb | As `eh3d-new` when there is no box and as `eh3d-board` when there is one; refused while there are changes that are not saved. |
| Check the fit with the board | As `eh3d-check`; it writes nothing. |
| Cut the openings of the connectors | As `eh3d-openings --apply`. |
| Hand the box to the other programs | As `eh3d-publish`. |

The plain *Save as* of the File menu keeps every branch too. A file that was opened, saved or picked is added to the recent files only when that worked, and a recent file that is gone is taken out of the list; the list is kept between sessions in the settings folder of the user (`%APPDATA%\EH3D\recent-phoenix-cad.txt`, or `~/.config/eh3d/`), with or without `--config`. All of this is decided in `suitewin`, which is tested without a screen; the window has never been opened on a screen.

## The box made from a board

`eh3d-new` takes what it is not told, and the window takes everything, from the section `[suite]` of `phoenix-cad-window.default.cfg`:

| Key | Default | Meaning |
|---|---|---|
| `board_gap` | 2 mm | the room between the board and the walls |
| `board_headroom` | 3 mm | the room above the tallest part |
| `post_height` | 8 mm | the height of the post under each mounting hole |
| `post_screw` | M3 | the screw of the posts, from the table of screws |
| `post_kind` | insert | `insert` or `self-tapping` |
| `part_clearance` | 0 mm | the room every part must have when the fit is checked |
| `opening_clearance` | 0.5 mm | the room round a connector in its opening |
| `sheet_width`, `sheet_gap` | 600 mm, 5 mm | the sheet of the flat faces that are handed back |

## Selectors, parameters and flat faces

`select PROJECT SELECTOR` answers with stable names: `>X <X >Y <Y >Z <Z` (one face), `|X |Y |Z` (the faces along an axis), `#X #Y #Z` (the faces across it), `faces`, `edges|X|Y|Z`, `holes`, `holes@` followed by a face selector and `holes[d<3mm]` with `<`, `<=`, `=`, `>=` or `>`.

`params PROJECT [--limits FILE]` reads limits written one to a line as `parameter.min`, `parameter.max` or `parameter.step` `= length`.

`panels PROJECT OUT.dxf|OUT.svg` draws each face as seen from outside at its outer size, with its holes; extrusions and pockets are not drawn, and there are no joints or kerf allowance.

## The attachments of the project

The panel «Attachments» of the window (View, or *File > Suite project*) is the shared panel `eh3d-attachpanel` of EH3D-COMMON (its guide is `docs/ATTACHMENTS_PANEL.md` there). It works on the `.eh3d` file the window has open; with a project of this program alone (`.phoenixproj`) it says there is no project of the suite. It writes only the branch `attachments/`: `cad/` and the other branches keep their bytes, and the window reads the list of branches again after a write. A write that removes a signature is asked about first. Dangerous files and kinds the suite does not know are extracted or opened only after a confirmation, programs and scripts are never opened, nothing is downloaded and a settings file that holds `server_token` is not attached. A file dropped on the panel is attached there; the window keeps the projects and sketches it opens.

The limits are the sections `[attachments]` (`max_mb`) and `[viewer]` (`max_attachment_mb`, `max_pdf_mb`, `max_image_mb`, `max_text_mb`, `max_office_mb`) of the settings file given with `--config`, each a whole number of megabytes from 1 to 4096. PDF pages need the PDF library beside the program, which the packages carry with its licences; a build from source gets it with `python ../EH3D-COMMON/tools/fetch_pdfium.py --out target/release`.

## The title, the description and the signatures

**Title and description** (File, Suite project) reads and writes the two texts of `common/meta/meta.json` in each of the eight languages; only the branch `common` is written. A text needs its English version before another language, and an empty field takes the language away (`eh3d-meta --title ""` too).

**Signatures** are checked with the public key files that the person trusts: `suite.trusted_keys` (full paths separated by `;`, `none` by default), `eh3d-info --key FILE` or the button of the list of branches, which holds for the time the program runs. Each signature is told as valid with the identifier of its key, wrong, of a key that is not trusted, or only seen when no key is given; `eh3d-info` ends with code 2 on a wrong one and its comma separated values have a last column `signature`. To check, only public keys are read (a secret key is refused), and a valid signature says who signed the branch, not that the design is right.

Opening a file of the suite whose branch `cad` or project record is signed adds to the status line what is known about it with the keys of `suite.trusted_keys` (a wrong signature, or a key that is not trusted, as a problem).

## Signing from this program

*File, Suite project, Sign the suite project* signs the branch `cad`, the record of the whole project or both, with the Ed25519 code of the suite. The secret key is a file that the person chooses each time: it is read to sign and nothing else, its path is not kept after the window is closed, it is never written into a project and what it holds is never shown. The same window makes a new key (the secret file, which never replaces one and is readable by its owner only, and the public one with `.pub` added), and so does `eh3d-keygen`. Signing is refused while there are changes that are not saved, since the signature is of what is in the file. `eh3d-sign` signs the branch `cad`, or the branches of `--branch`, and with `--project` the whole project (alone when no branch is named); every branch is checked before the first signature, so a refusal (a branch that is not there or is sealed) leaves the file as it was.

A signature covers what was there: saving the box removes the signature of the branch `cad` and of the project, and writing any branch removes the one of the project. The window and the commands of the suite file say so when it happens; the panel of the attachments asks before.

## Looking at the files of other programs

A button over the panel of the attachments shows the chosen attachment when it is a STL, OBJ, 3MF, STEP, PLY, OFF or DXF file; a button of the list of branches shows the original that the last import kept in `cad/origin/`; *File, View a 3D or DXF file* and the option `--viewer FILE` of the window take a file of the disk. Nothing is written or taken out of the project: an attachment is read from inside it and checked against its SHA-256 as it is read. The file is read on its own thread with the readers of the import and their limits (`[mesh]`), and those of `[model_view]`: `max_mb` (64), `max_triangles` (150000; a larger file is measured and not drawn) and `edge_angle` (30 degrees).

A file of triangles is told with its solids, triangles, size, volume when every surface is closed and wound one way (otherwise the open edges), the warnings of the reader and whether its bounding box fits inside the box of the project as it is, turned by quarter turns or not at all. It is drawn as flat shaded triangles with the sharp edges, worked out pixel by pixel so that what is behind is hidden (also where two parts pass through each other), with straight views, and the distance between two corners that are seen, clicked on it. A cut along x, y or z hides what is beyond a coordinate chosen with a slider and paints the inside in a colour of its own; the surface is not closed again at the cut. A DXF without faces is drawn as its lines, arcs and circles on a flat sheet. Faces of STEP that are curved are refused by the reader, as in the import. The picture of the window has been looked at on a virtual screen; it has not been tried by hand.
