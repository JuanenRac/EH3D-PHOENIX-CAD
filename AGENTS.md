# AGENTS.md: rules for anyone who changes EH3D-PHOENIX-CAD

EH3D-PHOENIX-CAD: the mechanical side of the PHOENIX family: parametric parts, assemblies and enclosures that fit the boards of the design tool. Software licence: GPL-3.0-or-later; every source file starts with an SPDX header.

## 1. Before you start

1. Read `docs/STATUS.md`: what exists, what is missing, what the last change decided.
2. Read `docs/REQUIREMENTS.md` (the register of requirements and the state of each) and `docs/DECISIONS.md`.
3. Read the decision records in `docs/adr/` that touch what you change. Do not contradict an accepted record; write a new one instead.

## 2. Build and test

- The repository builds next to its sibling repositories (`EH3D-COMMON`, `EH3D-CORE-LIBS`, ...) as folders at the same level; the versions it needs are listed in `eh3d.project.json`.
- Run before every commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `python -I tools/ci_validate.py` (and, when the repository has them, the two `--check` commands of the README pictures).

## 3. Code rules

- A reader never panics and never guesses. What it cannot understand becomes a diagnostic (crate `eh3d-diagnostic`) with a stable code `PHOENIX-<AREA>-<NNN>` (or `EH3D-...` in the shared crates). A code is added to the registry in the same change as the test that produces it; codes are never reused.
- Every input file is hostile until proven otherwise: sizes, entry names, number formats and nesting depth are checked before use, and everything is bounded.
- No `unwrap` or `expect` in code that handles external data. `unsafe` only inside an isolated crate with its own review note.
- Library errors use `thiserror`; libraries do not print.
- Every public item is documented. Values with a unit carry it in the name (`x_nm`, `feed_mm_min`).
- Writers are deterministic (the same content gives the same bytes), do not read the clock and write atomically (a temporary file, then a rename; on Windows a short, bounded retry when another program holds the file; the old file is never deleted first).
- Output files carry no date, host name, user name or program version unless the caller supplies them.
- Every tuned value (a limit, a tolerance, a time) is a setting in a configuration file, not a constant buried in the code. Every security option has a switch in the configuration, with the conservative value as default.

## 4. Architecture rules

- Layers: `EH3D-COMMON` (contracts and the common diagnostic) -> `EH3D-CORE-LIBS` -> engines (libraries without a window) -> programs -> the server and its clients. A lower layer never depends on a higher one.
- Programs never depend on each other; they exchange work through project files or through the server.
- Contracts change only through an accepted decision record, a new version, updated conformance vectors and consumers that still pass.
- The `.eh3dproj` 1.0.0 behaviour does not change.

## 5. Evidence

- Any claim about a third-party format cites its published specification, an example file or a comparison with a golden file. Formats and programming interfaces are not invented.
- A state in `docs/REQUIREMENTS.md` is `validated` only with evidence; nothing is green that has never run on the real thing.

## 6. Clean room

- Formats are learned from public specifications, from files of the project itself and from golden-file comparisons.
- Executables and libraries of third-party software are never decompiled, disassembled or inspected to derive format details.
- Help texts, scripts, libraries and manufacturing jobs of other vendors are never copied into the repository.

## 7. Privacy

- Real designs are private: never commit them or paste them into issues or logs. Use synthetic cases or minimal anonymised reproducers.
- No telemetry. No network calls except those a feature explicitly needs.
- Runs over a private corpus use the redacted mode: only codes, counts, levels and hashes are reported, never messages, object names, net names, values or paths. A test checks that such a report contains nothing taken from the corpus.

## 8. Physical safety

- Software never decides that a machine program is safe to run: limits, interlocks and the emergency stop belong to the machine and its controller.
- Importing, converting, simulating or validating a design never starts motion, even as a side effect.

## 9. Imported scripts

- Scripts found in an imported project run only when the user turns the option on in the configuration (it is on by default and can be switched off), inside the sandbox, with a log. They never run unannounced.

## 10. Workflow

- One topic per commit, in English. Tests first. Documentation, the changelog, the eight README files and the counters are updated in the same change.
- Update `docs/STATUS.md` at the end of a piece of work. Ideas outside the current work go to `docs/NOT_YET.md`.
- A decision that is not accepted is not implemented; it is recorded as pending with the options.

## 11. Languages

User-facing text exists in eight languages (EN, ES, FR, IT, DE, ZH, JA, PT-PT) with the same structure. Identifiers, field names and diagnostic codes are never translated.
