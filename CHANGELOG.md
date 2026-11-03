# Changelog

All notable changes to this project are documented here.

## [Unreleased]

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
