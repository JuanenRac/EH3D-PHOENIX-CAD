# 0018: broken views, notes and attributes are lines of the records of the drawing

Status: accepted.

## Context

The sheets of records 0016 and 0017 draw a long part at its full length, have no free text with a leader, and keep nothing about a part but its material and its description. A long bar does not fit a small sheet at a readable scale, a drawing needs words that the model cannot give, and the bill and the title block need data (a part number, a supplier) that belongs to the part. The additions must not break the sheets already written, must not read any code of another program (only documentation and behaviour that can be seen), and every limit must be a setting.

## Decision

1. **A broken view is a line of the view.** `break u|v FROM TO` takes a range out of the picture of a front, top or right view of a solid. The squeeze is a plain function (`plans::brk::Warp`) that the frame of the view applies to every point, so the dimensions, the lines of the cuts and the anchors of the notes land where the picture is, while the **values** are still worked out from the model: a dimension across the break says the real length. The range is in the coordinates of the picture and not of the part, so it needs no new reference; a range outside the picture is reported when the sheet is worked out, not when the file is read. The gap and the marks are settings.
2. **A note is a record of its own** (`note ID TEXT`, `anchor`, `at`, `centre`, `layer`) on its own layer. Its anchor is a point of a view or a face of a node, the same kind of reference a dimension keeps. There is no symbol of roughness or of welding: a note is text, a leader and a centre mark only.
3. **An attribute is a line of the part record** (`attr KEY VALUE`), in the drawing and keyed by the id of the node. This keeps the history file, which other programs read, untouched; moving the attributes to the history later is a change of that file and needs its own record. The number, the lengths and the characters are settings; the bill and the title block take them from settings too (`sheet.bom.attr_columns`, `sheet.titleblock.from_attr`).
4. **The version of the text is 2 only when needed.** A sheet with a break, a note or an attribute starts with `drawing 2`; any other sheet is written as `drawing 1`, byte for byte as before, so a program that knows version 1 still reads it. The reader accepts both and, because a text must be in the one canonical form, a `drawing 1` that uses the new lines or a `drawing 2` that does not is refused.
5. **Diagnostics.** `PHOENIX-CAD-061` broken view, `PHOENIX-CAD-062` note, `PHOENIX-CAD-063` attribute, with their texts in the eight languages. A note whose face is gone keeps `PHOENIX-CAD-055`.
6. **The panel has one set of buttons.** There is no beginner or expert mode: revisions, overall dimensions, sections, notes and breaks are added from the same place, and the cut line, the balloons, the notes and the dimension lines are dragged. What each button and each drag does is plain code (`plans::panel`, `plans::drag`) tested without a screen; the window only passes the pointer on.
7. **The PDF keeps its font.** No font is embedded: the characters of the Japanese and Chinese words are carried by the SVG and the DXF, and the way to get them in a PDF is to print the SVG. Embedding a font needs a font file or a dependency and is a decision for later.

## Consequences

* A program that knows records 0016 and 0017 only refuses a sheet that uses the new lines; the other sheets are the same bytes.
* The table of ISO 286 is still marked as not verified: the standard itself has not been at hand, and the sheets that use it say so.
* Nothing has been compared with a drawing program: the state stays `prototype` and `evidence=simulator`.
