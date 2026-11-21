# EH3D-PHOENIX-CAD: decisions that apply here

Decisions of the suite that shape this project, written as project documentation. A decision marked proposed is not final: this project must not rely on it silently, and where a number or a limit depends on it, the code will keep it as one named constant.

## Independent repositories related by contracts

State: **accepted**.

Every repository of the suite is independent. Repositories relate through versioned contracts and build next to each other as sibling folders. Programs never depend on each other: they exchange work through project files or through the server. The design tool does not depend at build time on the satellites that depend on it; it reaches them through their command line or through server tasks. A lower layer never depends on a higher one.

## The PHOENIX project keeps its own format

State: **accepted**.

The design tool stores everything in its own project format, with one folder per domain (design, mechanical, simulation, thermal, RF, history, passport, models, exports). The `.eh3dproj` package stays the exchange format for fabrication and is exported from it; the two are not nested. Domains this project does not know are preserved, and domains that change electrical or geometric meaning must be understood or the project is opened read-only.

## Geometry kernel for mechanical work

State: **accepted** (records 0013 and 0014).

No outside geometry kernel is linked. The constraint solver of the 2D sketch is written in this repository, as the crate `eh3d-phoenix-cad-solver` with no dependency. Until a CAD program has opened the plain STEP file this repository writes, the solid operations are the exact booleans between prisms aligned with the axes (`prism.rs`); a mesh evaluation may serve analysis but never the model. A general kernel is decided later, with a record of its own that states its licence, and the geometry that depends on it is limited by a setting until then.

## Drawing sheets

State: **accepted** (record 0016).

A sheet is kept in its own optional domain `drawing`; a dimension keeps references (a face of the box of a node, an element of a sketch) by ids that do not change and its value is worked out from the model every time, an orphan is reported with `PHOENIX-CAD-055`; the bill of materials and the item numbers come from the history and the numbers are never given again; every tuned value is in the section `[sheet]` of the settings. The record is `adr/0016-drawing-sheets-tied-to-the-history.md`. Sections, details, tolerances, ordinate and chain dimensions and revisions are additions to the same records (record 0017): a section and a detail are views with more lines, a tolerance is a line of the dimension read from the table of `iso286`, and the new diagnostics are `PHOENIX-CAD-058` to `060`. A broken view, the notes with a leader and the free attributes are more lines of the same records (record 0018), written as `drawing 2` only when used, with the diagnostics `PHOENIX-CAD-061` to `063`.

## Questions about a solid, mirror and turn, and typed points

State: **accepted** (record 0019).

The questions about a solid of prisms (thickness, thin walls, surface, centre of mass) are answered on the grid that the planes of the prisms cut space into, in whole numbers, with no tolerance and under the limits of the booleans; the history gets `mirror` and `rotate` as exact operations (planes across the axes and quarter turns only); the STL file of a solid is its surface in rectangles with a fixed order; points typed as text are plain logic with no new diagnostic code (`PHOENIX-CAD-008` for text, `PHOENIX-CAD-001` for range). The record is `adr/0019-solid-questions-mirror-turn-and-typed-points.md`.

## Stable diagnostic codes

State: **proposed**.

Failures that a reader or a check cannot hide become diagnostics with a stable code of the form `PHOENIX-<AREA>-<NNN>` (the registry is kept in the board tool). A code is never reused or renumbered, its message may be translated but the code and field names are not, and a repository never keeps a private list: it asks for numbers in the registry in the same change that uses them, together with a test.

## Nothing here moves a machine

State: **accepted**.

Importing, converting, simulating or validating a design never starts motion, even as a side effect. Programs may at most prepare a job in the ready state through the server when a task says so; authorising, starting, confirming and cancelling are actions of a person. Computing tasks run without access to machine connectors.

## Private designs stay private

State: **accepted**.

The real projects of the author are never committed, pasted into issues or sent to a service. Tests use synthetic cases or minimal anonymised reproducers that the owner approved. Scripts that run over a private corpus use redacted mode, which reports only codes, counts, levels and hashes.

## The mechanical domain keeps the recipe

State: **accepted**.

The mechanical domain of the shared project keeps the recipe of the box (parameters, expressions, ordered steps, tip and the interface of the board), not its final shape; the final state is derived from the recipe every time. The text, its version and its limits are in `PROJECT_DOMAIN.md` and the record is `adr/0011-mechanical-domain-recipe.md`.

## The lid is fixed with screws into posts

State: **accepted**.

Posts rise from the floor under the mounting holes of the board, and the lid is screwed into them (or at the four corners when the box has no posts). Every size comes from the table of screws and the table of posts of the settings file, and the gap of the centring lip is the clearance of the printer profile.
