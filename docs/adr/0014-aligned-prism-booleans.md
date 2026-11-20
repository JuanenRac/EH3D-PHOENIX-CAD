# 0014: booleans between solids are exact for prisms aligned with the axes, with no outside library

Status: accepted.

## Context

The geometry kernel for mechanical work was an open question. A kernel that works on any surface is a large dependency whose licence and results have to be checked, and nothing written here has been opened in a CAD program yet.

## Decision

1. Until a CAD program has opened the plain STEP file this repository writes, the solid operations are the three booleans between **prisms aligned with the axes** (`prism.rs`), done exactly in whole nanometres with no outside library and with a canonical form of the result.
2. A solid that is not made of such boxes has no representation in that module. A mesh evaluation (a solid as triangles) may serve analysis, never the model.
3. The size of an operation is limited by settings (`history.max_cells`, `history.max_prisms`); beyond them the operation is refused with `PHOENIX-CAD-053` and nothing is built.
4. A general kernel is decided later, with a record of its own that states its licence.

## Consequences

* Boxes, plates with rectangular holes, frames and arrays are exact; a cylinder or a rounded edge is not part of the prism solids.
* The volume is an exact integer and two solids can be compared with `==`.
