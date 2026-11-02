# EH3D-PHOENIX-CAD: requirements

This register is the single source of the state of each capability. Status values: planned, prototype, implemented, validated. Every requirement is `planned` today: nothing is implemented or verified. High test coverage is not proof of geometric or electrical correctness, so each requirement says how it will be checked and what result is expected.

| Requirement | Description | Status | How it will be checked | Expected result | Known limits |
|---|---|---|---|---|---|
| FCAD-001 | Generate a simple parametric enclosure from named parameters. | planned | automatic: dimensional reference cases | A regenerated model with the same parameters is identical. | Not started: no limits known yet. |
| FCAD-002 | Export the model to STEP. | planned | automatic: re-read the export and compare dimensions | Exported solids keep their dimensions within a stated tolerance. | Not started: no limits known yet. |
| FCAD-003 | Export the model as an OpenSCAD-style script. | planned | automatic: golden files | Same parameters give the same script. | Not started: no limits known yet. |
| FCAD-004 | Apply structured change operations coming from the board tool and report conflicts. | planned | automatic: operation tests, including conflicting ones | A rejected operation leaves the model unchanged. | Not started: no limits known yet. |
| FCAD-005 | Heat-sink model with declared dissipation assumptions for the thermal lab. | planned | automatic plus review | Every assumption is stored with the model. | Not started: no limits known yet. |
| FCAD-006 | Mechanical coordinates follow their own range rule, not capped by the board limit. | planned | automatic: property tests | A chassis larger than a board is representable. | Not started: no limits known yet. |

Rules: a claim of correctness names the corpus, the tolerance and the tool version it holds for. A phase is complete when the measured rate does not drop, every failure is classified and fixed or documented, and the measured figure is reported.
