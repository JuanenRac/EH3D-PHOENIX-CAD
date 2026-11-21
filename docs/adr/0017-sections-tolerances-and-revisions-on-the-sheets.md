# 0017: sections, details, tolerances and revisions are lines of the records of the drawing

Status: accepted.

## Context

The sheets of record 0016 have views, dimensions and balloons but no sections, details or hatching, no tolerances on a dimension, no dimensions from a common origin or in a chain, and no table of revisions. They have to be added without a second file format and without breaking the sheets already written, and without reading any code of another program: only the documentation and the behaviour a person can see.

## Decision

1. **A section and a detail are views.** The record `view` takes the optional lines `section PARENT LETTERS POSITION`, `detail PARENT LETTER circle|rect ...` and `hatch MATERIAL`. A section is a front, top or right view of the solid cut by a plane across the axis the view looks along; a detail is the part of an earlier view of the same kind inside a circle or a rectangle. They are not views of the projection and are not aligned with the others. The version of the text stays `drawing 1`: a sheet without the new lines is written byte for byte as before.
2. **The cut is done on the prisms.** The prisms are cut by the plane and the half that lies away from the viewer stays; the faces on the plane are the cut faces. The hatching is parallel lines on the sheet, numbered from its origin, in rectangles of the cut faces, so the same part is hatched alike in every view; its angle and distance come from the settings by material.
3. **A tolerance is a line of the dimension.** `tol class H7/g6` or `tol dev UPPER LOWER`. The class is worked out from the table of `iso286` at the value measured, so it follows an edit of a parameter. A combination that does not hold is `PHOENIX-CAD-058`; the table that ships is not verified and every sheet that uses it says so.
4. **Chain and ordinate are kinds of dimension** that share the references of the cumulative one. A dimension without an offset is placed by the sheet on the first line where it does not meet another along its line, with a room taken from the settings; the pointer sets the offset on a multiple of a snap from the settings. The logic is plain functions tested without a window.
5. **Revisions are numbered once.** A revision has a number that is never given again, a free date and a free author (the program reads no clock), and a description. The title block can be chosen from several in the settings; the groups of lines are the ISO series and the layers follow the group of the sheet.
6. **Diagnostics.** `PHOENIX-CAD-058` tolerance, `PHOENIX-CAD-059` section or detail, `PHOENIX-CAD-060` revision, each with its text in the eight languages. The check keeps the exit codes of record 0016.

## Consequences

* A program that knows record 0016 only refuses a sheet with the new lines (the text is in one canonical form); sheets without them are the same bytes.
* A broken view, a section along a broken plane and stacked tolerances need records of their own.
* Nothing has been compared with a drawing program: the state stays `prototype` and `evidence=simulator`.
