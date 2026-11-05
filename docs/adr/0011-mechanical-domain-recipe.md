# 0011: the mechanical domain keeps the recipe of the enclosure; the lid is screwed into posts

Status: accepted.

## Context

The design tool and the mechanical tool share one project. The mechanical part needs to be edited again later (a hole moves, a board is replaced, a wall gets thicker), so a stored final shape is not enough: what is stored must let the box be rebuilt from its intent. The older domain `mechanical-parameters` kept only the parameters of the box.

The box also needs a way to fix its lid. The options were a lip, tabs, or screws. The boards of the suite already have mounting holes, and a printed box is usually screwed.

## Decision

1. The domain `mechanical` of the project file keeps the **recipe**: the parameters with their variables and expressions, the ordered steps with their names and suppressed flags, the tip, and the interface of the board (outline, mounting holes, thickness, tallest part). The final state is **derived** from the recipe every time and is never stored. The text, its version, its limits and the rules for programs that do not know it are in `docs/PROJECT_DOMAIN.md`.
2. The domain is critical, has its own version in its first line, is written in one canonical form (a text that is not in that form is refused), and has hard limits that are settings, checked when it is read and when it is written. A domain of a newer version, or a domain this program does not know, is kept byte for byte and never rewritten. The older domain `mechanical-parameters` is still read and is replaced the first time the new domain is written.
3. The board tool writes the interface of the board (`[board]`) and can write the box by adding steps with the same grammar; the mechanical tool owns `[parameters]` and `[steps]`.
4. The lid is fixed with **screws into posts**. Posts rise from the floor under the mounting holes of the board, with a bore for a threaded insert or a self-tapping screw taken from the table of screws of the settings file and an optional conical foot. The lid has a through hole with a recess for the head over each post (or over the four corners when the box has no posts) and an optional centring lip whose gap is the clearance of the printer profile. The holes of the lid are derived from the posts, not stored.
5. The new failures have codes `PHOENIX-CAD-027` to `PHOENIX-CAD-030` (post shape, collision, height and lid fixing), with their text in the eight languages. A defect of the text of the domain is `PHOENIX-CAD-018`.

## Consequences

* The recipe can be edited, reordered and suppressed, and the same recipe always gives the same box; undo returns the original recipe.
* A step the box refuses stays in the recipe and is reported as failed when it is built; the following steps still run. The board tool decides what to do with a failed step.
* A program with an older domain version opens the project read-only; it never loses a step by saving.
* Two spellings of one recipe do not exist, so two programs that write the same recipe write the same bytes.
* The export to STEP writes posts with a plain bore and the plain holes of the lid; the conical foot of a post, the centring lip and a countersunk or counterbored recess are refused with `PHOENIX-CAD-022` until the file draws them. The script of OpenSCAD draws everything.
* The mass and the moments of inertia do not include posts, lip or the holes of the lid yet, and say so.
* The repository `EH3D-PHOENIX-PCB` has to read `[board]` and write steps through this contract; until it does, the mechanical tool is the only writer.
