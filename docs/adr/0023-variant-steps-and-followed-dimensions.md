# 0023: variants that switch steps and variables that follow a drawing

Status: accepted.

## Context

Record 0022 made variants of values only and left reference dimensions to be read. A family of boxes often differs in a step (with or without fins), and a size that was measured in a sketch had to be typed again.

## Decision

1. **A variant can switch a step off or on.** The step is named by its operation and the id it works on (`add-fins:ID`), or by the operation alone for the steps without an id, so adding, removing or reordering other steps does not move the choice to another step. A step that a variant does not name stays as it is.
2. **Version 2 of the `variants` text only when it is needed.** A project whose variants hold values only keeps version 1, which older programs read. An older program keeps a version 2 domain byte for byte and opens the project read-only.
3. **A variable follows a reference dimension, never a driving one,** and only lengths. The link is read when it is made and when the sketch is kept, as part of the same change; nothing is watched in the background.
4. **No circles.** A sketch whose own formulas use the variable cannot be followed by it, and a followed variable cannot be set by hand or changed by a variant.
5. **A broken link keeps the value** and is shown; it is never removed on its own.

## Consequences

* The links are kept in the optional domain `variable-links`; an older program keeps it and does not follow the drawing.
* Removing one of two steps that share an operation and an id renumbers the second one.
