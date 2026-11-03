# EH3D-PHOENIX-CAD: rules for writing code

Rules for anyone who writes code in this repository, human or tool. The code is early; these rules apply to every line.

## Licence
Software is GPL-3.0-or-later. Every source file starts with an SPDX licence header. Hardware designs, where they exist, use CERN-OHL-S-2.0, and a repository that mixes the two states the licence per file or folder.

## Before you start
Read STATUS.md (what exists), the requirement you work on in REQUIREMENTS.md and the decisions in DECISIONS.md. Never contradict an accepted decision: propose a new one. A contract that is not accepted is not implemented or consumed: stop and report it.

## Build and test (once code exists)
Run before every commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo deny check` and `python -I tools/ci_validate.py`. The repository builds next to its sibling repositories (see DEPENDENCIES.md).

## Code
* A reader never panics and never guesses. What it cannot understand becomes a diagnostic with a stable code from the registry of its prefix (`EH3D-` for the common crates, `PHOENIX-` for the PHOENIX family). Never invent a code: add it to the registry in the same change, with a test.
* Every input file is hostile until proven otherwise: check sizes, entry names, number formats and nesting depth first. Everything is bounded.
* No `unwrap` or `expect` in parsers or in code that handles external data. `unsafe` only in an isolated FFI crate with its own note.
* Library errors use `thiserror`; logging uses `tracing`; no `println!` in libraries.
* Every public item is documented. Wire formats carry the unit in the field name (`x_mm`, `feed_mm_min`, `x_nm`).
* Writers are deterministic (same content, same bytes), do not read the clock, and write atomically (temporary file, then rename; on Windows retry briefly on sharing violations and never delete the old file first). Output files carry no date, host name or user name unless the caller supplies them.
* Every value that is tuned lives in a configuration file, not in the code. Every security option has a switch with a conservative default.

## Architecture
Layers: EH3D-COMMON (contracts and the diagnostic crate), EH3D-CORE-LIBS, engines (libraries without a window), programs, then the server and its clients. Programs never depend on each other. Contracts change only through an accepted decision, a new version, updated conformance vectors and passing consumers.

## Evidence
A claim about a third-party format cites its published specification, an example file or a golden-file comparison. Do not invent formats or interfaces. Pin and verify every crate on its documentation page.

## Clean room
Never decompile, disassemble or inspect the executables or libraries of third-party software to derive format details. Work from public documentation, files of your own and golden-file comparisons. Never copy help text, scripts, libraries or output jobs of other vendors into this repository.

## Privacy
The real projects of the author are private: never commit them or paste them into issues or logs. Use synthetic cases. Scripts that run over private data use redacted mode, which reports only codes, counts, levels and hashes. No telemetry and no network calls except those a task explicitly allows.

## Physical safety
Code never decides that a toolpath is safe, and never starts a machine job. Importing, converting, simulating or validating a design never starts motion. Limits, interlocks and the emergency stop belong to the machine and its controller.

## Languages
User-facing text exists in eight languages (English, Spanish, German, French, Italian, Japanese, Chinese and European Portuguese) with identical structure. Identifiers, field names and diagnostic codes are never translated.

## Definition of done
Code, passing tests including negative ones, short documentation, the requirement register updated, and STATUS.md up to date. A button in an interface does not make a feature done.
