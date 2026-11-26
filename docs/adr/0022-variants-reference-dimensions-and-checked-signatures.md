# 0022: variants, reference dimensions and checked signatures

Status: accepted.

## Context

A box is often one recipe in a few sizes, and a drawing often needs figures that are read and must not hold it. A project of the suite can also carry signatures, which this program only reported as present.

## Decision

1. **Variants are values of variables.** A variant holds a value for some variables of the project and nothing else, so everything that follows a variable follows the variant. They are kept in an optional domain `variants` with its own versioned text; an older program keeps it byte for byte, and one that cannot be read leaves the project read-only.
2. **An export of the variants is all or nothing.** A variant with a refused step would give a file that is not the box asked for, so it stops the export.
3. **A reference dimension is a state of a dimension,** not another kind of constraint. The sketch file writes it as `ref` or `refoff` and takes version 3 only when it has one.
4. **Signatures are checked with public keys the person gives,** through the shared crate `eh3d-project`. Nothing is trusted by default. This program does not sign and does not handle secret keys.

## Consequences

* A program that reads only versions 1 and 2 of the sketch file refuses a sketch with a reference dimension and keeps the project read-only.
* The comma separated values of `eh3d-info` have a new last column.
