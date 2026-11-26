# 0021: the suite file and the board of the other branch

Status: accepted.

## Context

The suite has one project file, `.eh3d`, with a branch for each program (format and reader in the shared repository). The box is designed for a board that another program draws, and until now the only way in was a part of this program's own project that nobody wrote. The branches must stay independent: a program writes its own and reads the others, and a derived design must notice when what it came from has changed.

## Decision

1. **The project stays one thing inside the branch.** `cad/project.phoenixproj` is the project of this program byte for byte; every command and the window read it from a `.eh3d` file and the window saves it back. Nothing of the project format changes, so a project taken out of the suite file is the one that went in.
2. **Only `cad`, `docs/cad` and the metadata of `common` are written,** and a file of `cad` that is not written again stays. The shared crate checks the owner, the seal and the integrity.
3. **The board comes as a small artifact, not as the board program's project.** `pcb/outline2d.json` (version 1) holds what a box needs: the outline, the mounting holes, the sizes, the footprints of the parts and the connectors. It is read under the limits of the mechanical settings. The definition lives here until the board program writes it, and then moves to the shared repository.
4. **The reference carries the hash.** `cad` records that it was derived from `pcb` at that content hash, and keeps a copy of the board it took, so that what changed can be named hole by hole.
5. **Nothing is guessed.** A post follows its hole by identity. A hole that is new gets no post, a post whose hole is gone stays and is told, and an opening the box refuses is not written.
6. **Room is rectangles and heights.** A part is checked as its rectangle and height against the walls, the lid, the floor under the board and the posts: exact in whole nanometres, and no more than that.

## Consequences

* The program depends on the shared crate `eh3d-project`.
* The state is `prototype` and `evidence=simulator`: the board program writes no artifact yet, so every board was made up by a test, and no box has been printed or assembled.
