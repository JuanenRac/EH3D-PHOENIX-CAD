# Changelog

All notable changes to this project are documented here.

## [Unreleased]
- Add `eh3d_phoenix_cad::mass`: the volume of the material, the volume of the holes (marked as not exact when a hole reaches a corner, the floor or the lid) and the mass for a density, in integers, with a table of densities in the settings file.
- Add `eh3d_phoenix_cad::store`: saving a file through a temporary file that is flushed and renamed into place, with as many numbered copies as the settings say, and loading a file under a size limit; code `PHOENIX-CAD-012`.
- Add `eh3d_phoenix_cad::config` and `eh3d_phoenix_cad::printer`: the settings file with its shipped defaults (copies kept, undo depth, unit shown, printer compensation and a table of metric screws, all outside the code) and the model for fabrication whose holes are widened by the profile of the printer; code `PHOENIX-CAD-011`.
- Add `eh3d_phoenix_cad::body`: the recipe of an enclosure as a body with a tip, steps that can be suppressed, the state of each step (valid, suppressed, failed with its code, after the tip) and an undo and redo history in named transactions that are all or nothing.
- Add `eh3d_phoenix_cad::names`: stable names for the faces, the edges and the walls of the holes of an enclosure, built by rule so that they do not change when other operations are added, removed or reordered, and the code `PHOENIX-CAD-010` for a name that no longer exists.
- Add `eh3d_phoenix_cad::units`: angles in integer nano-degrees with integer sine and cosine, counts and ratios with their own range, rounding half away from zero, an integer square root, and lengths read and shown in mm, um, in and mil (`1.6 mm`, `62 mil`, `1/16 in`); codes `PHOENIX-CAD-006` to `PHOENIX-CAD-009`.
- Add `eh3d_phoenix_cad::step::to_step`: the enclosure as a STEP file (a boundary representation with the inner space as a void and a cylindrical face for each hole), the same text for the same model, tested for its structure (header, references, counts); no CAD program has opened it.
- Correct the test counter of the README files to 27 tests, 24 of the Rust library and 3 of the base.

## [0.0.2] - Parametric box with holes, shared object identifiers, reversible operations and the fit check against a board

- **The application icon and the logos of the program:** `images/brand/` holds the icon (SVG, PNG sizes, `.ico` and `.icns`), the horizontal and vertical logos in a light and a dark version and the social preview image.
- **Shared checks updated:** the validator also compares the badge and the Version row of the README with the manifest, fails when one of its checks cannot run, and the CHANGELOG step of the workflow reads the base branch from the environment.

- Correct the test counter of the badge and of the overview in the README files: 20 tests, 17 of the Rust library and 3 of the base.
- Identify the holes with the object identity of EH3D-COMMON (`ObjectId`, UUID v7) like the objects of the board tool; this repository now depends on EH3D-COMMON (`depends_on` of the manifest and the tag named in `SIBLING_REFS` of the CI).
- Add the Rust workspace and the crate `eh3d-phoenix-cad`: lengths in integer nanometres with their own range (2^40, larger than the board model), a parametric enclosure with round holes in its faces, structured change operations with an exact inverse that report conflicts (a hole outside its face, holes too close, an id used twice) and leave the model unchanged, an OpenSCAD-style script that is the same text for the same model, a fit check of the envelope of a board, and the codes `PHOENIX-CAD-001` to `PHOENIX-CAD-005` in `docs/ERROR_CODES.md`.
- Add the Rust job to the CI: format check, clippy with warnings as errors and the tests on Linux and Windows.
- Add `AGENTS.md` with the rules for anyone who changes the repository, and `docs/adr/` with the four accepted decision records (independent repositories, own project format, clean room, golden-file oracle) written as ordinary documents.

## [0.0.1] - The base of the repository

- **The base that every repository of the suite has:** the manifest, the README files in eight languages, this changelog, the CI workflow, the issue and pull-request templates, and the shared checks of EH3D-DOCS (the repository check and the drawers of the two README pictures).
- **A planned project, described honestly:** the scope, the requirements (all planned), the relations with other repositories, the threat model, the decisions that apply and the rules for writing code are in `docs/`; there is no code yet.
