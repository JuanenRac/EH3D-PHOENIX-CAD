# EH3D-PHOENIX-CAD: status

## Current state

EH3D-PHOENIX-CAD is planned. Version 0.0.1 holds only the base that every repository of the suite carries (manifest, README files in eight languages, changelog, CI, shared checks) and the written scope, requirements, dependencies, threat model and decisions. There is no code, nothing has been run on a machine, and no requirement is implemented.

## Capabilities

The official state of each capability is in REQUIREMENTS.md: 6 requirements, all `planned`. This page never states a state that REQUIREMENTS.md does not.

## Open questions

* Which geometry kernel and constraint solver to use. Candidates being looked at: a Rust solid-modelling crate with STEP support, OpenCASCADE through bindings (not verified), the SolveSpace solver through the slvs crate (GPL-3.0) and planegcs (LGPL, not verified). Licences must be compatible with GPL-3.0-or-later.
* Where the 3D board view and the enclosure fit check live: here or in the board tool.

## Needs from other repositories

* EH3D-PHOENIX-PCB: the mechanical domain of the project format
* EH3D-COMMON: the project container

## Known technical debt and omitted tests

None yet. The only tests are those of the base (the repository check and the checks of the two README pictures).
