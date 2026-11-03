# EH3D-PHOENIX-CAD: status

## Current state

EH3D-PHOENIX-CAD is early code. Beside the base that every repository of the suite carries (manifest, README files in eight languages, changelog, CI, shared checks) and the written scope, requirements, dependencies, threat model and decisions, it has a Rust library, `eh3d-phoenix-cad`: lengths in integer nanometres with their own range, a parametric enclosure with round holes in its faces, structured change operations that report conflicts and leave the model unchanged (each with an exact inverse), an OpenSCAD-style script, and a fit check of the envelope of a board. It has only been tried on models made by its own tests. It also writes the enclosure as a STEP file (a boundary representation with the inner space as a void and a cylindrical face for each hole), the same text for the same model; that file has only been checked for its structure (header, references, counts) and no CAD program has opened it. There is no command line, no server task, no heat sink, no sketch and no assembly, and nothing has been run on a machine.

## Capabilities

The official state of each capability is in REQUIREMENTS.md: 6 requirements: 4 `prototype` (FCAD-001, FCAD-003, FCAD-004, FCAD-006) and 2 `planned`. This page never states a state that REQUIREMENTS.md does not.

## Open questions

* Which geometry kernel and constraint solver to use. Candidates being looked at: a Rust solid-modelling crate with STEP support, OpenCASCADE through bindings (not verified), the SolveSpace solver through the slvs crate (GPL-3.0) and planegcs (LGPL, not verified). Licences must be compatible with GPL-3.0-or-later.
* Where the 3D board view and the enclosure fit check live: here or in the board tool.

## Needs from other repositories

* EH3D-PHOENIX-PCB: the mechanical domain of the project format
* EH3D-COMMON: the project container

## Known technical debt and omitted tests

The holes carry the shared object identity (UUID v7 of EH3D-COMMON), but the mechanical domain of the project format is not written, so nothing stores or exchanges them yet. The script has not been opened in OpenSCAD and nothing is compared with a reference CAD program.
