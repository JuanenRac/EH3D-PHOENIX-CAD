# 0024: seeing the files of other programs and signing from this program

Status: accepted. It replaces the sentence of record 0022 that this program does not sign.

## Context

The attachments of a suite project (data sheets, and the 3D files of the parts) are shown inside the program, but the viewer of the suite does not draw 3D files or flat drawings, so they had to be opened outside. Signatures were checked here and could only be made with the command line tool of the shared repository.

## Decision

1. **A view, not an import.** A 3D file or a flat DXF drawing is read with the readers of the import and shown; nothing is written, extracted or added to the model.
2. **Flat shading painted from far to near,** with the sharp edges. There is no depth buffer and no new dependency; a file with more triangles than `model_view.max_triangles` is measured and not drawn.
3. **The key is given each time.** The secret key file is read to sign and nothing else: no path is kept in the settings, and it is never written into a project.
4. **The branch `cad` by default.** The window signs the branch of this program, the whole project or both; the command can name other branches, as the shared format lets any program sign for the holder of a key.
5. **A save is not stopped by a signature, it tells.** Saving what a signature covers removes it, and the window and the commands of the suite file say so.

## Consequences

* The view cannot judge what two crossing parts look like where they cross, and curved STEP faces are refused as by the import.
* A key file has no password: protecting it is up to its owner.
