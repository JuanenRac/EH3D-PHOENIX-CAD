// EH3D-PHOENIX-CAD - command line and window.
// Copyright (C) 2026 JuanenRac (Electro Hobby 3D). GPL-3.0-or-later.
#![forbid(unsafe_code)]

use eh3d_phoenix_cad::catalog;
use eh3d_phoenix_cad::config::{Settings, DEFAULT_CONFIG};
use eh3d_phoenix_cad::drawing::View;
use eh3d_phoenix_cad::export::Format;
use eh3d_phoenix_cad::lang::{fill, Lang, Msg};
use eh3d_phoenix_cad::shot;
use eh3d_phoenix_cad::sketch::settings::SketchSettings;
#[cfg(feature = "gui")]
use eh3d_phoenix_cad::sketch_ui::SketchUiSettings;
use eh3d_phoenix_cad::uiconfig::{default_file, UiSettings};
use eh3d_phoenix_cad::window::{error_text, Window};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "eh3d-phoenix-cad: parametric enclosures: the model tree, the properties, a 3D view and a plan, and the fit check of a board

  eh3d-phoenix-cad [gui] [PROJECT.phoenixproj | SKETCH.phoenixsketch] [--view model|sketch] [--window model|review|compact] [--viewer FILE] [--lang CODE] [--config FILE]   the window (a build with --features gui): with no arguments it opens with a new project; the sketches of a project are kept in the project file, and a .phoenixsketch file is opened to edit it and put in the project, or written from the sketch as a copy
  eh3d-phoenix-cad gui [PROJECT | SKETCH] [--view model|sketch] [--window model|review|compact] --shot OUT.png [--size WxH] [--theme forge|dark|light] [--wait N] [--viewer FILE] [--lang CODE] [--config FILE] [--replace]   a picture of the window (a build with --features gui)
  eh3d-phoenix-cad check PROJECT.phoenixproj [--lang CODE] [--config FILE]       reads a project, builds the box (with the extrusions and pockets of its sketches) and prints its measures, the fit of the board and the state of every step
  eh3d-phoenix-cad interference PROJECT.phoenixproj [--lang CODE] [--config FILE]  reads a project, puts the parts where their mates place them and lists the pairs of parts that share volume (touching is not sharing; a screw through the hole of a bracket is allowed)
  eh3d-phoenix-cad bill PROJECT.phoenixproj [--config FILE]                       the bill of materials of the parts placed in a project (profiles cut to a length, screws and brackets) as comma separated values, with the volume and the mass for the density of each material
  eh3d-phoenix-cad cutplan PROJECT.phoenixproj [--text] [--lang CODE] [--config FILE]            the cutting plan of the profiles placed in a project: the pieces of the cut list fitted into bars (the longest cut each profile comes in) with the kerf of the saw and the trimmed ends, as comma separated values or, with --text, as lines for a person
  eh3d-phoenix-cad clearance PROJECT.phoenixproj [--text] [--lang CODE] [--config FILE]                  lists, as comma separated values, the pairs of placed parts that do not touch and are closer than clearance.min_gap (exit 1 when there is one)
  eh3d-phoenix-cad placements PROJECT.phoenixproj [--explode PERCENT] [--text] [--lang CODE] [--config FILE]   where each placed part is (the corner and the size in millimetres and the turn) and the size of the whole assembly, as comma separated values; with --explode the parts are first pulled apart from the middle of the assembly by that percentage
  eh3d-phoenix-cad bom PROJECT.phoenixproj [--text] [--group material|kind|name | --by-assembly] [--lang CODE] [--config FILE]                  the bill of materials with cost: the parts and bodies grouped into lines with quantity, material, mass, the price of the material (price_per_kg of the settings) and the cost, with the subtotal, the markup and the total, as comma separated values for a spreadsheet; --text prints lines for a person in the language of --lang (any of the eight); --by-assembly gives a bill for each assembly (the parts that mates join, each set priced alone, and the parts and bodies that no mate holds last, as (no assembly)) instead of one bill for the model
  eh3d-phoenix-cad partlist PROJECT.phoenixproj [--text] [--lang CODE] [--config FILE]           the placed parts with their properties (name, kind, part, material, density, volume, mass, corner and size of the box) and the totals, as comma separated values; --text prints lines for a person in the language of --lang (any of the eight)
  eh3d-phoenix-cad holes PROJECT.phoenixproj [--tools] [--text] [--lang CODE] [--config FILE]            the schedule of the holes of the box for the person who drills (a tag, the face, the position, the diameter and the shape of each) as comma separated values, or with --tools the drills and cutters they need grouped by diameter and shape
  eh3d-phoenix-cad sketch-export PROJECT.phoenixproj|SKETCH.phoenixsketch --format dxf|svg|step [--height LENGTH] [--sketch NAME] [--out FILE] [--overwrite] [--lang CODE] [--config FILE]   writes a sketch (the only one of the project, or the one named) as a flat drawing of its closed outlines for a laser or a router, or with --format step (which needs --height) as a STEP file of the prism that its outlines make when raised by that height, with the holes through it and a circle or an arc as the polygon of the profile; never over a file that exists unless --overwrite is given
  eh3d-phoenix-cad balance PROJECT.phoenixproj [--lang CODE] [--config FILE]                     the mass of the placed parts and where their centre of mass is (exact for profiles, an estimate for screws and brackets, and the number of estimated parts is said)
  eh3d-phoenix-cad revolve PROJECT.phoenixproj|SKETCH.phoenixsketch --axis x|y [--angle DEGREES] [--sketch NAME] [--material NAME] [--out FILE.stl|.obj|.3mf [--overwrite]] [--lang CODE] [--config FILE]   turns a sketch (the only one of the project, or the one named) about its first or second axis, by a full turn unless --angle says less, and prints the volume, the surface and the distances from the axis of the solid made and, with a material of the settings, its mass; the profile must not cross the axis; with --out the triangles of the solid are written to a file of the format its extension says (never over a file that exists unless --overwrite is given)
  eh3d-phoenix-cad sweep PROFILE --path PATH [--profile-sketch NAME] [--path-sketch NAME] [--material NAME] [--out FILE.stl|.obj|.3mf [--overwrite]] [--lang CODE] [--config FILE]   sweeps the profile (a project or a sketch file, and the sketch of it) along the path (the lines and arcs of another sketch, or one circle) and prints the length of the path, the volume and the surface of the solid made and, with a material of the settings, its mass; the profile starts on the path, its first axis to the left of the way the path is walked from the free end of its first element; with --out the triangles of the solid are written as for revolve
  eh3d-phoenix-cad sketch-import DRAWING.dxf|.svg [--out FILE.phoenixsketch] [--name NAME] [--face NAME] [--unit mm|cm|m|in|auto] [--layers NAME,NAME] [--list-layers] [--overwrite] [--lang CODE] [--config FILE]   reads the lines, circles, arcs and polylines of a text DXF drawing into a new sketch (the ends that meet become one point, so closed outlines can be raised from a face or cut into it) and writes it as a sketch file; what is not read (text, dimensions, splines, ...) is counted and told; --layers reads only the entities of the layers named and counts the others, --list-layers prints the layers of the drawing with their entities and writes nothing; the unit is that of the drawing (mm without --unit; auto reads it from the header of the drawing and tells where it came from), the sketch lies on the face given (front without --face), and its name is the one of the file without --name
  eh3d-phoenix-cad section PROJECT.phoenixproj --axis x|y|z --at LENGTH [--format dxf|svg] [--out FILE] [--overwrite] [--lang CODE] [--config FILE]   cuts the solids of a project (the box, the parts and the turned and swept bodies) with the plane across an axis at a position (a length: 15, 15 mm, 0.5 in) and prints the outlines and the area of the material; with --out the section is written as a DXF or SVG drawing (the format is --format or the extension of the file; never over a file that exists unless --overwrite is given)
  eh3d-phoenix-cad library FILE.phoenixlib [--config FILE]                        reads a library of parts, checks it and lists its parts
  eh3d-phoenix-cad mesh-info FILE.stl|.obj|.3mf|.step|.stp|.ply|.off|.dxf [--density KG_M3 | --material NAME] [--config FILE]   reads a file of triangles (the ones this program writes or another program's stored 3MF) or a STEP file of solids made of flat faces (the limits are mesh.step_max_kib, mesh.step_max_entities and mesh.step_max_faces; a face on a cylinder, a surface of revolution or a curved edge is refused with the entity and its name) and prints, for each solid in it, its vertices, triangles, whether its surface is closed, its volume, its surface and its size, and, for a prism along an axis, its height and outlines; with a density (or a material of the settings) also its mass, centre of mass and tensor of inertia; it exits with 0 when every solid is closed and with 1 when one is not
  eh3d-phoenix-cad export PROJECT.phoenixproj --format stl|obj|3mf|dxf|svg|pdf|step|script|report|bill|order [--view top|front|side|sheet] [--out FILE] [--overwrite] [--ascii|--binary] [--title-field KEY=TEXT]... [--cut-line AXIS=POSITION[:LETTERS]] [--lang CODE] [--config FILE]   writes the model to a file: the solid as triangles (stl), as a Wavefront file (obj) or as a 3MF package (3mf), the plan, front or side, or all three on one sheet, as a drawing (dxf, svg), all three on one sheet as a PDF file of one page (pdf, which is always the sheet), the STEP file, the OpenSCAD-style script, the fit report, the bill of materials or the order sheet (an HTML page with the bars to buy, the holes to drill and the parts with their cost, in the language of --lang)
  eh3d-phoenix-cad faces PROJECT.phoenixproj --from FACE[:inner|:outer] --to FACE[:inner|:outer] [--lang CODE] [--config FILE]   measures between two faces of the box (front, back, left, right, top or bottom, the outer face of the wall unless :inner is given): the distance in millimetres between parallel faces (a wall, the inner height, the outer width) and the angle, 0 for parallel faces and 90 for faces that meet; the same face on the same side is refused
  eh3d-phoenix-cad sketch-edit SKETCH.phoenixsketch --op chamfer|trim|extend|linear|circular|offset|reference|driving [--out FILE] [--overwrite] [--config FILE] ...   edits a sketch file the way the buttons of the sketch window do (elements are named as in the file, like e5; lengths are in mm or with a unit): chamfer --lines e5,e6 --distance 3; trim --line e5 --cutter e6 [--drop e1]; extend --line e5 --to e6; linear --items e1,e2 --step DX,DY --count N; circular --items e1 --center e3 --count N [--step DEGREES]; offset --items e5,e6,e7,e8 --distance D (negative goes inwards); reference --constraint c3 makes a dimension a reference (it reads the drawing and holds nothing, with the value of now and no formula) and driving --constraint c3 makes it hold the drawing again; the edited sketch is written to --out, or over the file itself with --overwrite, and a refused edit writes nothing
  eh3d-phoenix-cad server-status [--metrics] [--lang CODE] [--config FILE]        asks the EH3D server named in the [server] section of the settings (server_address, server_token, server_machine, server_timeout_s, server_retries, server_mdns, server_max_answer_mib, server_token_fixed, server_error_body_bytes) how it is: its name, version and state; --metrics adds what the link has done (requests, tries, retries, answers, losses and the latency of the answers, never a token or an address); with no address nothing is sent anywhere
  eh3d-phoenix-cad server-list [TEXT] [--user NAME] [--lang CODE] [--config FILE]        lists the projects this program keeps on the server (those whose name has TEXT, in any letter case); without a token in the settings, --user logs in with the password in the variable PHOENIX_CAD_SERVER_PASSWORD
  eh3d-phoenix-cad server-save PROJECT.phoenixproj [--name NAME] [--comment TEXT] [--user NAME] [--lang CODE] [--config FILE]   saves a project file on the server as a new version of the file of the project NAME (the name of the file without --name; the project is made when the server has none of that name, the write says which version it is made from, and the same bytes make no version)
  eh3d-phoenix-cad server-load NAME --out FILE [--overwrite] [--user NAME] [--lang CODE] [--config FILE]   reads the project NAME from the server, checks it is a project file and writes it to FILE, never over a file that exists unless --overwrite is given
  eh3d-phoenix-cad server-report NAME --from FILE [--report NAME] [--comment TEXT] [--user NAME] [--lang CODE] [--config FILE]   sends a report (a text file such as the output of bom, partlist or holes) to the folder of reports of the project NAME on the server, as a new version of the file REPORT (the name of the file without --report)
  eh3d-phoenix-cad server-check PROJECT [--name NAME] [--kind check|report] [--user NAME] [--lang CODE] [--config FILE]   asks the station cad of the server (the machine of server_machine) to check the structure of a project (sketches, features and parts) and tells what it found in the language of --lang; --kind report adds the counts; --stored NAME takes the project.json that the last server-save wrote instead of a file
  eh3d-phoenix-cad server-contract PROJECT [--name NAME] [--config FILE]   prints the project as the station cad of the server reads it (the project.json that server-save writes: sketches, features and parts, and with the section [contract] of the settings the holes, posts, fins, patterns, edging and lid of the enclosure and the title block), without asking anything of the network, so it works with no address and no server
  eh3d-phoenix-cad project-units PROJECT.phoenixproj [--unit nm|um|mm|cm|m|in|mil] [--decimals N] [--clear] [--out FILE] [--overwrite] [--lang CODE] [--config FILE]   tells the unit and the decimals the project keeps (in its own domain, so a project of inches stays in inches whoever opens it; a project without them follows the settings); with --unit and --decimals (0 to 12) it sets them, with --clear it takes them away, and writes the project to --out or over the file itself, never over a file that exists unless --overwrite is given
  eh3d-phoenix-cad compare BEFORE.phoenixproj AFTER.phoenixproj [--text] [--lang CODE] [--config FILE]   what changed between two versions of an assembly: the parts added, taken away, replaced, cut again, moved or turned (the positions after the mates) and the mates added, taken away or changed, as comma separated values (change,part,from,to) or, with --text, as lines in the language of --lang; it exits with 0 when the two versions are the same and with 1 when they differ
  eh3d-phoenix-cad mesh-check FILE.stl|.obj|.3mf|.step|.stp|.ply|.off|.dxf [--repair OUT.stl|.obj|.3mf [--overwrite]] [--lang CODE] [--config FILE]   checks each solid of a file of triangles for a slicer: the shells, the Euler characteristic and the genus, the open and the shared edges, the triangles with no area, the repeated vertices, a surface turned inside out, the shortest and the longest edge, the worst triangle and the radii of gyration about the principal axes (exit 1 when a solid has a problem); --repair turns the solids that are inside out and writes them all to the file
  eh3d-phoenix-cad convert VALUE [--to nm|um|mm|cm|m|in|mil|deg|dms] [--decimals N] [--fraction N] [--lang CODE]   reads a length (1 1/2 in, 25.4 mm) or an angle (12.5, 12d30m15s or 12 degrees 30 minutes 15 seconds written with the signs of degrees, minutes and seconds) and writes it in another unit; --fraction N writes inches as a mixed number of N-ths (1, 2, 4 ... 128); dms writes degrees, minutes and seconds
  eh3d-phoenix-cad bolt-circle --count N --diameter LENGTH [--start DEGREES] [--center X:Y] [--text] [--lang CODE]   the holes of a bolt circle for the drill: the angle and the place of each hole as comma separated values, or with --text the angle and the distance between neighbours too
  eh3d-phoenix-cad bom-costs PROJECT.phoenixproj [--tax PERCENT] [--top N] [--text] [--lang CODE] [--config FILE]   where the money of the bill of materials goes: the lines from the dearest to the cheapest with their share of the subtotal and of the running sum, the first N of them, and with --tax the tax on the total
  eh3d-phoenix-cad variants PROJECT [--keep NAME [--only V,V] | --set NAME --variable V --value TEXT | --set NAME --step WHICH --state off|on|keep | --remove NAME | --apply NAME | --check | --steps | --figures | --grid | --import TABLE.csv | --export FOLDER [--format stl|step|script|...] [--overwrite]] [--text] [--lang CODE] [--config FILE]   the variants of a project (named values of its variables and steps switched off or on): without an action they are listed as comma separated values; --keep saves the current values under a name, --set changes one value or what the variant does with one step (named by its number, key or name), --apply writes a variant into the project (which is saved), --check builds the box with each variant (exit 1 when one cannot be built, has a refused step or does not fit its board), --steps lists the steps with their keys, --figures prints the sizes and the volume of material of each variant, --grid prints the values as a table for a spreadsheet and --import reads such a table back (all of it or nothing), and --export writes one file for each variant, named after the project and the variant, all or none, never over a file that exists unless --overwrite is given
  eh3d-phoenix-cad variables PROJECT [--follow NAME --sketch S --constraint c3 | --stop NAME] [--text] [--lang CODE] [--config FILE]   the variables of a project with the drawing they follow; --follow makes a variable (made when there is none) follow a reference dimension of a sketch of the project, and it follows again each time the sketch is kept; --stop takes the link away; exit 1 when a link is broken
  eh3d-phoenix-cad templates [--lang CODE]                                       lists the projects a new one can start from (box, plate, frame and assembly)
  eh3d-phoenix-cad new --template NAME --out FILE.phoenixproj [--overwrite] [--lang CODE]   writes a new project from a template, never over a file that exists unless --overwrite is given
  eh3d-phoenix-cad measure distance|angle POINT... [--lang CODE]                  measures between points written X:Y:Z (a length with its unit or bare millimetres): the distance between two points, from a point to an edge (three points) or between two edges (four), the angle at a vertex between three points or between two edges
  eh3d-phoenix-cad config-diff FILE [--text] [--lang CODE]                       puts every key of a settings file next to its default (changed, same or extra) as comma separated values or text; exit 1 when a key is not one of the defaults
  eh3d-phoenix-cad sketch-info SKETCH.phoenixsketch [--lang CODE] [--config FILE]   what a sketch holds: the points, lines, circles and arcs, the construction geometry, the constraints and the size and drawn length of what is drawn
  eh3d-phoenix-cad sketch-check SKETCH.phoenixsketch|PROJECT [--sketch NAME] [--text] [--lang CODE] [--config FILE]   checks a sketch file, or the sketches kept in a project (.phoenixproj or .eh3d), before one is raised: constraints that repeat or contradict others and a solver that does not converge, two points that look like one and are not joined, lines almost horizontal or vertical and open ends of the profile (kind,subject,other,value,code, or lines with --text; the limits are sketch.check_near and sketch.check_angle); it exits with 0 when there is nothing to report, with 2 when there is and with 1 when it could not run
  eh3d-phoenix-cad mesh-convert FILE OUT [--ascii] [--schema ap203|ap214] [--overwrite] [--lang CODE] [--config FILE]   reads an STL, OBJ, 3MF (also compressed ones made by other programs), STEP, PLY, OFF or DXF (3DFACE) file and writes its solids as .stl, .obj, .3mf, .ply, .off, .dxf or .step/.stp (a faceted solid for each body); every warning of the reader is printed with its name
  eh3d-phoenix-cad mesh-transform FILE OUT [--scale F|FX:FY:FZ] [--rotate AXIS:DEGREES] [--about X:Y:Z] [--mirror AXIS[:POSITION]] [--move X:Y:Z] [--overwrite] [--lang CODE]   scales, turns, mirrors and moves the solids of a file, in this order, and writes them (a mirror keeps the solid outside in)
  eh3d-phoenix-cad mesh-pattern FILE OUT --linear COUNT --step X:Y:Z | --circular COUNT --axis x|y|z [--center X:Y:Z] [--angle DEGREES] [--overwrite] [--lang CODE]   repeats every solid of a file in a line or round an axis and writes the result
  eh3d-phoenix-cad mesh-slice FILE --axis x|y|z --at LENGTH [--svg OUT.svg] [--csv] [--overwrite] [--lang CODE]   the section a plane makes in each solid: the outlines, the holes, the area and the length (exit 1 when a section does not close); --svg draws it
  eh3d-phoenix-cad mesh-clip FILE OUT --axis x|y|z --at LENGTH [--keep above|below] | --box LOW..HIGH [--overwrite] [--lang CODE]   cuts each solid by a plane or keeps what is inside a box, closing the cut with a cap so that the part left is a solid again
  eh3d-phoenix-cad iso-fit SIZE CLASS|HOLE/SHAFT [--table FILE] [--csv] [--lang CODE]   the limits of a class of tolerance (H7, g6) or of a fit (H7/g6) at a nominal size up to 500 mm, from a table of data that says whether it is verified (the shipped one is not)
  eh3d-phoenix-cad history-check PROJECT.phoenixproj [--text] [--glb OUT.glb] [--stl OUT.stl] [--export OUT.stl|obj|3mf|ply|off] [--overwrite] [--lang CODE] [--config FILE]   builds the history of features of a project (parameters with expressions and operations that depend on each other: boxes, moves, arrays and the union, difference and intersection of aligned prisms) and reports every node as comma separated values (node,name,state,code,detail) or, with --text, as lines in the language of --lang; with --glb the solid of the history is written as a binary glTF file (the profile phoenix-cad-glb/1: metres, one mesh, flat normals) and with --stl as a binary STL file of the faces on its surface (millimetres) and with --export as the same surface in the format of the extension (STL, OBJ, 3MF, PLY or OFF, welded vertices, two triangles to a rectangle), never over a file that exists unless --overwrite is given; it exits with 0 when every node was built, with 2 when the verification did not pass (a node failed or depends on one that failed) and with 1 when it could not run
  eh3d-phoenix-cad history-graph PROJECT.phoenixproj [--lang CODE] [--config FILE]   the graph of dependencies of the history of a project as a file for Graphviz
  eh3d-phoenix-cad history-measure PROJECT.phoenixproj [--text] [--min-wall LENGTH] [--section AXIS=POSITION] [--lang CODE] [--config FILE]   the volume, the surface, the centre of mass, the box, the thinnest wall and the shadow along each axis of the solid of the history, exact in whole nanometres, the tensor of inertia about the centre (floating point, density 1) and, with --section z=5, the area of the section by that plane (quantity,axis,value,unit, or lines in the language of --lang); with --min-wall it lists every wall thinner than the length (a bare number is in millimetres) and exits with 2 when there is one, with 1 when it could not run
  eh3d-phoenix-cad sketch-points [--start POINT] [--text] [--lang CODE] POINT...   places points typed as text one after another: 10,5 (from the origin), @3,-2 (from the last point), 20<45 (distance and angle from the origin) and @20<45 (from the last point); a number without a unit is in millimetres; prints point,x_nm,y_nm or lines in the language of --lang
  eh3d-phoenix-cad drawing-check PROJECT.phoenixproj [--text] [--lang CODE] [--config FILE]   builds the history and the sketches of a project and works its drawing sheet out against them: the views of the solids (front, top, right and isometric, with the visible and the hidden edges), the dimensions tied to the nodes of the history and the elements of the sketches (a dimension whose node or element is gone is an orphan, PHOENIX-CAD-055), the balloons tied to the bill of materials of the history; it reports every view, dimension and balloon as comma separated values (record,name,state,code,detail) or, with --text, as lines in the language of --lang; it exits with 0 when the sheet agrees with the model, with 2 when the verification did not pass and with 1 when it could not run
  eh3d-phoenix-cad drawing-export PROJECT.phoenixproj OUT.svg|OUT.pdf|OUT.dxf|OUT.csv [--format svg|pdf|dxf|bom] [--create] [--sheet NAME] [--portrait] [--scale auto|N:D] [--force] [--overwrite] [--lang CODE] [--config FILE]   writes the drawing sheet of a project as an SVG, PDF or DXF file, or its bill of materials as comma separated values, the same bytes for the same project; it writes nothing while the check does not pass (exit 2) unless --force is given, never over a file that exists unless --overwrite is given, and --create makes the four views of the result of the history when the project has no sheet
  eh3d-phoenix-cad eh3d-info FILE.eh3d [--text] [--key PUBLIC.pub] [--lang CODE] [--config FILE]   the branches of a project file of the suite, the ones derived from a branch that has changed since, what its checks find and what is known about each signature: with the public key of --key or of suite.trusted_keys a signature is checked (valid, wrong or made by a key that is not trusted), without one it is only seen; exit 2 when the checks find anything or a signature is wrong
  eh3d-phoenix-cad eh3d-save PROJECT FILE.eh3d [--time T] [--audit T] [--seal NAME] [--backup] [--lang CODE]   puts a project in the branch cad of the file of the suite (made when it is not there); the other branches keep their bytes
  eh3d-phoenix-cad eh3d-open FILE.eh3d --out PROJECT.phoenixproj [--overwrite] [--lang CODE]   writes the project of the branch cad to a file of its own
  eh3d-phoenix-cad eh3d-new FILE.eh3d [--gap L] [--headroom L] [--post-height L] [--screw M3] [--kind insert|self-tapping] [--overwrite] [--time T] [--lang CODE] [--config FILE]   makes the box round the board of the branch pcb with a post under every mounting hole (what is not given comes from the section [suite] of the settings)
  eh3d-phoenix-cad eh3d-board FILE.eh3d [--time T] [--lang CODE] [--config FILE]   takes the board of the branch pcb as it is now, moves the posts with their holes and says what changed; exit 2 when something could not be followed
  eh3d-phoenix-cad eh3d-check FILE.eh3d [--clearance L] [--lang CODE] [--config FILE]   whether the board changed since the box took it and every part of it has room; exit 2 when not
  eh3d-phoenix-cad eh3d-openings FILE.eh3d [--clearance L] [--apply] [--time T] [--lang CODE] [--config FILE]   the openings that the connectors of the board need in the walls, written into the box with --apply
  eh3d-phoenix-cad eh3d-publish FILE.eh3d [--sheet-width L] [--gap L] [--time T] [--lang CODE] [--config FILE]   hands the box to the other programs: the STL, the flat faces and the posts in cad, the report and the bill in docs/cad
  eh3d-phoenix-cad eh3d-import FILE.eh3d OTHER.stl|obj|3mf|step|ply|off|dxf [--time T] [--lang CODE]   keeps a file of another program in cad/origin with the report of what was read
  eh3d-phoenix-cad eh3d-meta FILE.eh3d [--title TEXT] [--description TEXT] [--time T] --lang CODE   the title and the description of the project in a language
  eh3d-phoenix-cad eh3d-sign FILE.eh3d --key SECRET_KEY [--branch NAMES] [--project] [--lang CODE]   signs with Ed25519 the branch cad (or the branches named, comma separated) and with --project the whole project as it is now; with --project and no --branch only the project; nothing is written when something cannot be signed; the key is only read
  eh3d-phoenix-cad eh3d-keygen --out SECRET_KEY [--lang CODE]   makes a new signing key: the secret file (never over a file that exists, readable by its owner only) and SECRET_KEY.pub, which is what those who check need
  eh3d-phoenix-cad select PROJECT SELECTOR [--lang CODE]   the stable names that a selector picks (>Z, |X, #Y, edges|Z, holes@>Z[d<3mm]); exit 2 when it picks nothing
  eh3d-phoenix-cad params PROJECT [--limits FILE] [--text] [--lang CODE]   the parameters of the box, their definitions and the ones outside their limits; exit 2 when there are any
  eh3d-phoenix-cad panels PROJECT OUT.dxf|OUT.svg [--sheet-width L] [--gap L] [--overwrite] [--lang CODE]   the six faces as flat templates with their holes, packed on a sheet
  eh3d-phoenix-cad config                                                         every setting of the library, of the window, of the sketch, of the history and of the drawing sheets with its default value, as a file for --config

--open FILE is the same as giving the file by itself; a sketch file opens the window on the sketch. --lang takes en, es, de, fr, it, ja, zh or pt; without it the language of the system is used (the reports cutplan, clearance, placements, holes and balance and the answer of sketch-export are in English unless --lang is given; with --text the first four are lines for a person in that language instead of comma separated values).
--shot writes a PNG file of the window (the exit code is 0 when it was written, 1 when it was not and 2 when the arguments are wrong); the window keeps no settings in that mode.
--config FILE reads a settings file over the defaults: it only needs the keys it changes (see the output of config).
check exits with 0 when every step was applied and the board fits, and with 1 when a step was refused or the board does not fit.
interference exits with 0 when every step was applied and no two parts share volume, and with 1 when one does.
export never writes over a file that exists unless --overwrite is given; --title-field (organisation, title, number, revision or author, as often as needed) turns the title block of a drawing on with those texts and --cut-line (x=40 mm, z=1.5 in:B) draws the cutting line of a plane in a dxf or svg view, over the settings title_block and drawing.cut_line; --out defaults to the name of the project with the extension of the format, in the current folder; --view is for dxf and svg (the plan, top, is the default); --ascii or --binary overrides the setting mesh.stl_binary for stl. It exits with 0 when the file was written and with 1 when it was not (the code of the reason is printed).";

/// The settings of the library and of the window, with the keys of a user file put over the defaults.
fn load_settings(config: Option<&Path>) -> Result<(Settings, UiSettings), String> {
    let Some(path) = config else {
        return Ok((Settings::defaults(), UiSettings::defaults()));
    };
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let settings =
        Settings::with_user_config(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let ui = UiSettings::with_user_config(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((settings, ui))
}

/// The settings of the sketch (the solver, the profiles and the file), with the keys of a user
/// file put over the defaults.
fn load_solver_settings(config: Option<&Path>) -> Result<SketchSettings, String> {
    let Some(path) = config else {
        return Ok(SketchSettings::defaults());
    };
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    SketchSettings::with_user_config(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The settings of the sketch and of its window, with the keys of a user file put over the
/// defaults.
#[cfg(feature = "gui")]
fn load_sketch_settings(
    config: Option<&Path>,
) -> Result<(SketchSettings, SketchUiSettings), String> {
    let solver = load_solver_settings(config)?;
    let Some(path) = config else {
        return Ok((solver, SketchUiSettings::defaults()));
    };
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ui = SketchUiSettings::with_user_config(&text)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((solver, ui))
}

/// Opens the project that `check` and `interference` are given, with the language and the settings
/// file of their options.
fn open_project(args: &[String], command: &str) -> Result<(Window, Lang), String> {
    let mut lang = Lang::from_env();
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--lang" => {
                let code = iter.next().ok_or("--lang needs a language code")?;
                lang = Lang::from_code(code).ok_or("unknown language code")?;
            }
            "--config" => {
                let path = iter.next().ok_or("--config needs a settings file")?;
                config = Some(PathBuf::from(path));
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err("only one project can be checked".to_owned());
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or_else(|| format!("{command} needs a project file"))?;
    let (settings, ui) = load_settings(config.as_deref())?;
    let mut win = Window::new(lang, settings, ui).map_err(|e| error_text(&e, lang))?;
    win.sketch_settings = load_solver_settings(config.as_deref())?;
    if !win.open(&file) {
        return Err(format!("{}: {}", file.display(), win.status.text(lang)));
    }
    if win.status.is_problem() {
        eprintln!("note: {}", win.status.text(lang));
    }
    Ok((win, lang))
}

/// `check`: reads a project and says what the model makes of it. The answer is whether every step
/// was applied and the board fits.
fn run_check(args: &[String]) -> Result<bool, String> {
    let (win, _) = open_project(args, "check")?;
    println!("{}", win.report_text());
    Ok(win.info.failed == 0 && win.info.fit.fits)
}

/// `interference`: reads a project, puts its parts where their mates place them and says which
/// pairs of parts share volume. The answer is whether every step was applied and none does.
fn run_interference(args: &[String]) -> Result<bool, String> {
    let (win, _) = open_project(args, "interference")?;
    println!("{}", win.assembly_text());
    Ok(win.info.failed == 0 && win.assembly_clear())
}

/// The project and the settings file of the commands `bill` and `library`.
fn file_and_config(args: &[String], what: &str) -> Result<(PathBuf, Option<PathBuf>), String> {
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" => {
                let path = iter.next().ok_or("--config needs a settings file")?;
                config = Some(PathBuf::from(path));
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err(format!("only one {what} can be given"));
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or_else(|| format!("this command needs a {what}"))?;
    Ok((file, config))
}

/// Takes `--lang CODE` out of the arguments of a command that prints a report. English without
/// it, so that a report is the same text on every computer.
fn take_lang(args: &[String]) -> Result<(Lang, Vec<String>), String> {
    let mut lang = Lang::En;
    let mut rest = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--lang" {
            let code = iter.next().ok_or("--lang needs a language code")?;
            lang = Lang::from_code(code).ok_or("unknown language code")?;
        } else {
            rest.push(arg.clone());
        }
    }
    Ok((lang, rest))
}

/// Opens the project that a report is asked for, with the settings file and the language.
fn open_for_report(file: &Path, config: Option<&Path>, lang: Lang) -> Result<Window, String> {
    let (settings, ui) = load_settings(config)?;
    let mut win = Window::new(lang, settings, ui).map_err(|e| error_text(&e, lang))?;
    if !win.open(file) {
        return Err(format!("{}: {}", file.display(), win.status.text(lang)));
    }
    Ok(win)
}

/// `bill`: reads a project and prints the bill of materials of the parts placed in it.
fn run_bill(args: &[String]) -> Result<bool, String> {
    let (file, config) = file_and_config(args, "project file")?;
    let (settings, ui) = load_settings(config.as_deref())?;
    let lang = Lang::En;
    let mut win = Window::new(lang, settings, ui).map_err(|e| error_text(&e, lang))?;
    if !win.open(&file) {
        return Err(format!("{}: {}", file.display(), win.status.text(lang)));
    }
    let csv = win.bill_csv_text().map_err(|e| error_text(&e, lang))?;
    print!("{csv}");
    Ok(win.info.failed == 0)
}

/// `cutplan`: reads a project and prints the cutting plan of the profiles placed in it.
fn run_cutplan(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let text = args.iter().any(|a| a == "--text");
    let rest: Vec<String> = args.iter().filter(|a| *a != "--text").cloned().collect();
    let (file, config) = file_and_config(&rest, "project file")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    let out = if text {
        win.stock_text()
    } else {
        win.stock_csv_text()
    }
    .map_err(|e| error_text(&e, lang))?;
    print!("{out}");
    Ok(win.info.failed == 0)
}

/// `clearance`: reads a project and prints the pairs of placed parts that do not touch and are
/// closer than the minimum of the settings. The answer is whether there are none.
fn run_clearance(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let text = args.iter().any(|a| a == "--text");
    let rest: Vec<String> = args.iter().filter(|a| *a != "--text").cloned().collect();
    let (file, config) = file_and_config(&rest, "project file")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    let out = if text {
        win.clearance_text()
    } else {
        win.clearance_csv_text()
    }
    .map_err(|e| error_text(&e, lang))?;
    print!("{out}");
    let none = win
        .clearance_report()
        .map_err(|e| error_text(&e, lang))?
        .pairs
        .is_empty();
    Ok(win.info.failed == 0 && none)
}

/// `placements`: reads a project and prints where each placed part is and the size of the
/// assembly, optionally with the parts pulled apart (`--explode PERCENT`).
fn run_placements(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let mut explode: Option<u32> = None;
    let mut text = false;
    let mut rest: Vec<String> = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--text" {
            text = true;
        } else if arg == "--explode" {
            let value = iter.next().ok_or("--explode needs a percentage")?;
            explode = Some(
                value
                    .parse::<u32>()
                    .map_err(|_| "--explode needs a whole percentage".to_owned())?,
            );
        } else {
            rest.push(arg.clone());
        }
    }
    let (file, config) = file_and_config(&rest, "project file")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    let out = if text {
        win.placement_text(explode)
    } else {
        win.placement_csv_text(explode)
    }
    .map_err(|e| error_text(&e, lang))?;
    print!("{out}");
    Ok(win.info.failed == 0)
}

/// `partlist`: reads a project and prints the placed parts with their properties and the totals.
fn run_partlist(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let text = args.iter().any(|a| a == "--text");
    let rest: Vec<String> = args.iter().filter(|a| *a != "--text").cloned().collect();
    let (file, config) = file_and_config(&rest, "project file")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    let out = if text {
        win.part_list_text()
    } else {
        win.part_list_csv()
    }
    .map_err(|e| error_text(&e, lang))?;
    print!("{out}");
    Ok(win.info.failed == 0)
}

/// `bom`: reads a project and prints its bill of materials with the mass and the cost of each line.
fn run_bom(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let text = args.iter().any(|a| a == "--text");
    // `--group material|kind|name` prints the totals by group instead of the lines
    let mut group = None;
    let mut rest: Vec<String> = Vec::new();
    let by_assembly = args.iter().any(|a| a == "--by-assembly");
    let mut words = args
        .iter()
        .filter(|a| *a != "--text" && *a != "--by-assembly");
    while let Some(word) = words.next() {
        if word == "--group" {
            let by = words
                .next()
                .ok_or_else(|| "--group needs material, kind or name".to_string())?;
            group = Some(
                eh3d_phoenix_cad::bom::GroupBy::from_name(by)
                    .ok_or_else(|| format!("--group {by}: not material, kind or name"))?,
            );
        } else {
            rest.push(word.clone());
        }
    }
    let (file, config) = file_and_config(&rest, "project file")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    if by_assembly {
        if group.is_some() {
            return Err("--by-assembly and --group cannot be given together".to_string());
        }
        let out = if text {
            win.bom_assemblies_text()
        } else {
            win.bom_assemblies_csv()
        }
        .map_err(|e| error_text(&e, lang))?;
        print!("{out}");
        return Ok(win.info.failed == 0);
    }
    let out = match (group, text) {
        (Some(by), true) => win.bom_groups_text(by),
        (Some(by), false) => win.bom_groups_csv(by),
        (None, true) => win.bom_text(),
        (None, false) => win.bom_csv(),
    }
    .map_err(|e| error_text(&e, lang))?;
    print!("{out}");
    Ok(win.info.failed == 0)
}

/// `compare`: reads two versions of a project and prints what changed from the first to the second
/// (see `eh3d_phoenix_cad::compare`). The answer is false (exit code 1) when they differ.
fn run_compare(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let text = args.iter().any(|a| a == "--text");
    let mut files: Vec<String> = Vec::new();
    let mut config: Option<PathBuf> = None;
    let mut words = args.iter().filter(|a| *a != "--text");
    while let Some(word) = words.next() {
        match word.as_str() {
            "--config" => {
                config = Some(PathBuf::from(
                    words.next().ok_or("--config needs a settings file")?,
                ));
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => files.push(other.to_string()),
        }
    }
    let [before, after] = files.as_slice() else {
        return Err(
            "this command needs two project files: the first version and the second".to_string(),
        );
    };
    let first = open_for_report(Path::new(before), config.as_deref(), lang)?;
    let second = open_for_report(Path::new(after), config.as_deref(), lang)?;
    let changes = first
        .changes_to(&second)
        .map_err(|e| error_text(&e, lang))?;
    let shown = if text {
        eh3d_phoenix_cad::compare::text_in(&changes, lang)
    } else {
        eh3d_phoenix_cad::compare::csv(&changes)
    };
    print!("{shown}");
    Ok(changes.is_empty())
}

/// `project-units`: tells the units a project keeps, or sets them (`--unit`, `--decimals`) or takes
/// them away (`--clear`) and writes the project: to `--out`, or over the file itself with
/// `--overwrite`; never over a file that exists unless `--overwrite` is given.
fn run_project_units(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let (mut unit, mut decimals, mut clear) = (None, None, false);
    let (mut out, mut overwrite) = (None::<PathBuf>, false);
    let mut rest: Vec<String> = Vec::new();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--unit" => {
                let w = words.next().ok_or("--unit needs a unit")?;
                unit = Some(
                    eh3d_phoenix_cad::units::Unit::from_suffix(w)
                        .ok_or_else(|| format!("--unit {w}: not nm, um, mm, cm, m, in or mil"))?,
                );
            }
            "--decimals" => {
                let w = words.next().ok_or("--decimals needs a number")?;
                decimals = Some(
                    w.parse::<u32>()
                        .map_err(|_| format!("--decimals {w}: not a whole number"))?,
                );
            }
            "--clear" => clear = true,
            "--out" => out = Some(PathBuf::from(words.next().ok_or("--out needs a file")?)),
            "--overwrite" => overwrite = true,
            _ => rest.push(word.clone()),
        }
    }
    let (file, config) = file_and_config(&rest, "project file")?;
    let mut win = open_for_report(&file, config.as_deref(), lang)?;
    let changes = clear || unit.is_some() || decimals.is_some();
    if !changes {
        if out.is_some() || overwrite {
            return Err("--out and --overwrite need --unit, --decimals or --clear".to_owned());
        }
        match win.project_units() {
            Some(u) => println!(
                "{}",
                fill(
                    Msg::UnitsSet.tr(lang),
                    &[u.unit.suffix(), &u.decimals.to_string()]
                )
            ),
            None => println!(
                "{}",
                fill(
                    Msg::UnitsNone.tr(lang),
                    &[
                        win.settings.unit.suffix(),
                        &win.settings.decimals.to_string()
                    ]
                )
            ),
        }
        return Ok(true);
    }
    if clear && (unit.is_some() || decimals.is_some()) {
        return Err("--clear cannot be given with --unit or --decimals".to_owned());
    }
    let now = win.project_units();
    let units = if clear {
        None
    } else {
        Some(eh3d_phoenix_cad::project::ProjectUnits {
            unit: unit.or(now.map(|u| u.unit)).unwrap_or(win.settings.unit),
            decimals: decimals
                .or(now.map(|u| u.decimals))
                .unwrap_or(win.settings.decimals),
        })
    };
    let target = out.unwrap_or_else(|| file.clone());
    if target.exists() && !overwrite {
        return Err(format!(
            "{} exists: give --overwrite to write over it",
            target.display()
        ));
    }
    if !win.set_project_units(units) {
        return Err(win.status.text(lang));
    }
    let told = win.status.text(lang);
    if !win.save_to(&target) {
        return Err(win.status.text(lang));
    }
    println!("{told}");
    Ok(true)
}

/// `holes`: reads a project and prints the schedule of the holes of the box, or with `--tools`
/// the list of drills and cutters they need.
fn run_holes(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let tools = args.iter().any(|a| a == "--tools");
    let text = args.iter().any(|a| a == "--text");
    let rest: Vec<String> = args
        .iter()
        .filter(|a| *a != "--tools" && *a != "--text")
        .cloned()
        .collect();
    let (file, config) = file_and_config(&rest, "project file")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    if text {
        print!("{}", win.holes_text(tools));
    } else {
        print!("{}", win.holes_csv_text(tools));
    }
    Ok(win.info.failed == 0)
}

/// The sketch that `sketch-export` and `revolve` are given: a sketch file, or the sketch of a
/// project (the only one it has, or the one named), with the project when it is one and the
/// settings.
type ReadSketch = (
    eh3d_phoenix_cad::sketch::Sketch,
    Option<PathBuf>,
    Settings,
    UiSettings,
    SketchSettings,
);

fn read_sketch(
    file: &Path,
    name: Option<&str>,
    config: Option<&Path>,
    lang: Lang,
) -> Result<ReadSketch, String> {
    let (settings, ui) = load_settings(config)?;
    let solver = load_solver_settings(config)?;
    let (sketch, project) = if shot::is_sketch_file(file) {
        let sketch = eh3d_phoenix_cad::sketch::file::read_file(file, &solver)
            .map_err(|e| format!("{}: {}", file.display(), error_text(&e, lang)))?;
        (sketch, None)
    } else {
        let mut win =
            Window::new(lang, settings.clone(), ui.clone()).map_err(|e| error_text(&e, lang))?;
        win.sketch_settings = solver.clone();
        if !win.open(file) {
            return Err(format!("{}: {}", file.display(), win.status.text(lang)));
        }
        let sketch = win.find_sketch(name).map_err(|e| error_text(&e, lang))?;
        (sketch, Some(file.to_path_buf()))
    };
    Ok((sketch, project, settings, ui, solver))
}

/// Writes the triangles of a solid to `out` as the file its extension says (`.stl` binary or text
/// as the setting `mesh.binary` says, `.obj` or `.3mf`) and tells how many bytes it has.
fn write_solid(
    mesh: &eh3d_phoenix_cad::mesh::Mesh,
    name: &str,
    out: &Path,
    overwrite: bool,
    settings: &Settings,
    ui: &UiSettings,
    lang: Lang,
) -> Result<(), String> {
    let extension = out
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let bodies = [(name, mesh)];
    let bytes: Vec<u8> = match extension.as_str() {
        "stl" if settings.mesh.binary => eh3d_phoenix_cad::mesh::stl_binary(&bodies),
        "stl" => eh3d_phoenix_cad::mesh::stl_ascii(&bodies).into_bytes(),
        "obj" => eh3d_phoenix_cad::interchange::obj(&bodies).into_bytes(),
        "3mf" => eh3d_phoenix_cad::interchange::three_mf(&bodies),
        _ => return Err("--out needs a file that ends in .stl, .obj or .3mf".to_owned()),
    };
    let written =
        eh3d_phoenix_cad::export::write_bytes(out, &bytes, overwrite, None, ui.export.max_kib)
            .map_err(|e| error_text(&e, lang))?;
    println!(
        "{}",
        eh3d_phoenix_cad::lang::fill(
            Msg::Exported.tr(lang),
            &[&out.display().to_string(), &written.to_string()]
        )
    );
    Ok(())
}

/// `revolve`: turns a sketch about one of its axes and prints the volume, the surface and, with a
/// material of the settings, the mass of the solid.
fn run_revolve(args: &[String]) -> Result<bool, String> {
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut name: Option<String> = None;
    let mut axis: Option<String> = None;
    let mut turn: Option<String> = None;
    let mut material: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut overwrite = false;
    let mut lang = Lang::En;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" => {
                config = Some(PathBuf::from(
                    iter.next().ok_or("--config needs a settings file")?,
                ));
            }
            "--out" => out = Some(PathBuf::from(iter.next().ok_or("--out needs a file")?)),
            "--overwrite" => overwrite = true,
            "--axis" => axis = Some(iter.next().ok_or("--axis needs x or y")?.clone()),
            "--angle" => turn = Some(iter.next().ok_or("--angle needs degrees")?.clone()),
            "--sketch" => name = Some(iter.next().ok_or("--sketch needs a name")?.clone()),
            "--material" => {
                material = Some(iter.next().ok_or("--material needs a name")?.clone());
            }
            "--lang" => {
                let code = iter.next().ok_or("--lang needs a language code")?;
                lang = Lang::from_code(code).ok_or("unknown language code")?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err("only one project or sketch can be turned".to_owned());
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or("revolve needs a project or a sketch file")?;
    let word = axis.as_deref().ok_or("revolve needs --axis")?;
    let axis = eh3d_phoenix_cad::revolve::Axis::from_name(word)
        .ok_or_else(|| format!("{word}: unknown axis (x or y)"))?;
    let angle = match turn.as_deref() {
        Some(text) => {
            eh3d_phoenix_cad::units::parse_angle(text).map_err(|e| error_text(&e, lang))?
        }
        None => eh3d_phoenix_cad::units::Angle::from_deg(360).map_err(|e| error_text(&e, lang))?,
    };
    let (sketch, _, settings, ui, solver) =
        read_sketch(&file, name.as_deref(), config.as_deref(), lang)?;
    let profile = eh3d_phoenix_cad::profile::Profile::from_sketch(
        &sketch,
        &eh3d_phoenix_cad::profile::ProfileLimits::from_settings(&solver),
    )
    .map_err(|e| error_text(&e, lang))?;
    let turned = eh3d_phoenix_cad::revolve::revolve(&profile, axis, angle)
        .map_err(|e| error_text(&e, lang))?;
    let density = match material.as_deref() {
        Some(word) => Some((
            word,
            *settings
                .materials
                .get(word)
                .ok_or_else(|| format!("{word}: not a material of the settings"))?,
        )),
        None => None,
    };
    print!(
        "{}",
        eh3d_phoenix_cad::revolve::revolve_text(&turned, density, lang)
            .map_err(|e| error_text(&e, lang))?
    );
    if let Some(target) = out {
        let solid = eh3d_phoenix_cad::revolve::mesh(
            &profile,
            axis,
            angle,
            settings.mesh.circle_segments,
            settings.mesh.max_triangles as usize,
        )
        .map_err(|e| error_text(&e, lang))?;
        write_solid(
            &solid,
            sketch.name(),
            &target,
            overwrite,
            &settings,
            &ui,
            lang,
        )?;
    }
    Ok(true)
}

/// `sweep`: sweeps the profile of a sketch along the path of another and prints the length of the
/// path, the volume, the surface and, with a material of the settings, the mass of the solid.
fn run_sweep(args: &[String]) -> Result<bool, String> {
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut path_file: Option<PathBuf> = None;
    let mut profile_name: Option<String> = None;
    let mut path_name: Option<String> = None;
    let mut material: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut overwrite = false;
    let mut lang = Lang::En;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" => {
                config = Some(PathBuf::from(
                    iter.next().ok_or("--config needs a settings file")?,
                ));
            }
            "--out" => out = Some(PathBuf::from(iter.next().ok_or("--out needs a file")?)),
            "--overwrite" => overwrite = true,
            "--path" => {
                path_file = Some(PathBuf::from(iter.next().ok_or("--path needs a file")?));
            }
            "--profile-sketch" => {
                profile_name = Some(iter.next().ok_or("--profile-sketch needs a name")?.clone());
            }
            "--path-sketch" => {
                path_name = Some(iter.next().ok_or("--path-sketch needs a name")?.clone());
            }
            "--material" => {
                material = Some(iter.next().ok_or("--material needs a name")?.clone());
            }
            "--lang" => {
                let code = iter.next().ok_or("--lang needs a language code")?;
                lang = Lang::from_code(code).ok_or("unknown language code")?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err("only one profile can be swept".to_owned());
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or("sweep needs a project or a sketch file for the profile")?;
    let path_file = path_file.ok_or("sweep needs --path")?;
    let (sketch, _, settings, ui, solver) =
        read_sketch(&file, profile_name.as_deref(), config.as_deref(), lang)?;
    let (path_sketch, _, _, _, _) =
        read_sketch(&path_file, path_name.as_deref(), config.as_deref(), lang)?;
    let profile = eh3d_phoenix_cad::profile::Profile::from_sketch(
        &sketch,
        &eh3d_phoenix_cad::profile::ProfileLimits::from_settings(&solver),
    )
    .map_err(|e| error_text(&e, lang))?;
    let path = eh3d_phoenix_cad::sweep::path_of(&path_sketch).map_err(|e| error_text(&e, lang))?;
    let swept =
        eh3d_phoenix_cad::sweep::sweep(&profile, &path).map_err(|e| error_text(&e, lang))?;
    let density = match material.as_deref() {
        Some(word) => Some((
            word,
            *settings
                .materials
                .get(word)
                .ok_or_else(|| format!("{word}: not a material of the settings"))?,
        )),
        None => None,
    };
    print!(
        "{}",
        eh3d_phoenix_cad::sweep::sweep_text(&swept, density, lang)
            .map_err(|e| error_text(&e, lang))?
    );
    if let Some(target) = out {
        let solid = eh3d_phoenix_cad::sweep::mesh(
            &profile,
            &path,
            settings.mesh.circle_segments,
            settings.mesh.max_triangles as usize,
        )
        .map_err(|e| error_text(&e, lang))?;
        write_solid(
            &solid,
            sketch.name(),
            &target,
            overwrite,
            &settings,
            &ui,
            lang,
        )?;
    }
    Ok(true)
}

/// `sketch-export`: writes a sketch of a project, or a sketch file, as a DXF or SVG drawing. The
/// answer is whether the file was written. Nothing is written over a file that exists unless
/// `--overwrite` is given.
fn run_sketch_export(args: &[String]) -> Result<bool, String> {
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut word: Option<String> = None;
    let mut name: Option<String> = None;
    let mut height: Option<String> = None;
    let mut overwrite = false;
    let mut lang = Lang::En;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" => {
                config = Some(PathBuf::from(
                    iter.next().ok_or("--config needs a settings file")?,
                ));
            }
            "--format" => {
                word = Some(
                    iter.next()
                        .ok_or("--format needs dxf, svg or step")?
                        .clone(),
                );
            }
            "--height" => height = Some(iter.next().ok_or("--height needs a length")?.clone()),
            "--sketch" => name = Some(iter.next().ok_or("--sketch needs a name")?.clone()),
            "--out" => out = Some(PathBuf::from(iter.next().ok_or("--out needs a file")?)),
            "--overwrite" => overwrite = true,
            "--lang" => {
                let code = iter.next().ok_or("--lang needs a language code")?;
                lang = Lang::from_code(code).ok_or("unknown language code")?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err("only one project or sketch can be exported".to_owned());
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or("sketch-export needs a project or a sketch file")?;
    let (dxf, step) = match word.as_deref().ok_or("sketch-export needs --format")? {
        "dxf" => (true, false),
        "svg" => (false, false),
        "step" => (false, true),
        other => return Err(format!("{other}: unknown format (dxf, svg or step)")),
    };
    if height.is_some() && !step {
        return Err("--height is for --format step".to_owned());
    }
    let (sketch, project, settings, ui, solver) =
        read_sketch(&file, name.as_deref(), config.as_deref(), lang)?;
    let text = if step {
        let raised = eh3d_phoenix_cad::units::parse_length(
            height.as_deref().ok_or("--format step needs --height")?,
        )
        .map_err(|e| error_text(&e, lang))?;
        let profile = eh3d_phoenix_cad::profile::Profile::from_sketch(
            &sketch,
            &eh3d_phoenix_cad::profile::ProfileLimits::from_settings(&solver),
        )
        .map_err(|e| error_text(&e, lang))?;
        eh3d_phoenix_cad::step::prism_to_step(sketch.name(), &profile, 0, raised.get())
            .map_err(|e| error_text(&e, lang))?
    } else {
        eh3d_phoenix_cad::sketch_export::render_titled(
            &sketch,
            &solver,
            &settings.drawing,
            dxf,
            &settings.title_block,
            lang,
        )
        .map_err(|e| error_text(&e, lang))?
    };
    let target = out.unwrap_or_else(|| {
        PathBuf::from(format!(
            "{}.{}",
            sketch.name(),
            if step {
                "step"
            } else if dxf {
                "dxf"
            } else {
                "svg"
            }
        ))
    });
    let written = eh3d_phoenix_cad::export::write_file(
        &target,
        &text,
        overwrite,
        project.as_deref(),
        ui.export.max_kib,
    )
    .map_err(|e| error_text(&e, lang))?;
    println!(
        "{}",
        eh3d_phoenix_cad::lang::fill(
            Msg::Exported.tr(lang),
            &[&target.display().to_string(), &written.to_string()]
        )
    );
    Ok(true)
}

/// `sketch-import`: reads a text DXF drawing into a sketch and writes the sketch file. Nothing is
/// written over a file that exists unless `--overwrite` is given.
fn run_sketch_import(args: &[String]) -> Result<bool, String> {
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut name: Option<String> = None;
    let mut face = eh3d_phoenix_cad::enclosure::Face::Front;
    let mut unit_nm: Option<i64> = Some(1_000_000);
    let mut overwrite = false;
    let mut chosen: Vec<String> = Vec::new();
    let mut list_layers = false;
    let mut lang = Lang::En;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" => {
                config = Some(PathBuf::from(
                    iter.next().ok_or("--config needs a settings file")?,
                ));
            }
            "--out" => out = Some(PathBuf::from(iter.next().ok_or("--out needs a file")?)),
            "--name" => name = Some(iter.next().ok_or("--name needs a name")?.clone()),
            "--layers" => {
                chosen = iter
                    .next()
                    .ok_or("--layers needs layer names separated by commas")?
                    .split(',')
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
            }
            "--list-layers" => list_layers = true,
            "--face" => {
                let word = iter.next().ok_or("--face needs a face")?;
                face = eh3d_phoenix_cad::enclosure::Face::from_name(word).ok_or_else(|| {
                    format!("{word}: not a face (front, back, left, right, top or bottom)")
                })?;
            }
            "--unit" => {
                let word = iter.next().ok_or("--unit needs mm, cm, m, in or auto")?;
                unit_nm = match word.as_str() {
                    "auto" => None,
                    other => {
                        Some(eh3d_phoenix_cad::dxf_import::unit_nm(other).ok_or_else(|| {
                            format!("{other}: unknown unit (mm, cm, m, in or auto)")
                        })?)
                    }
                };
            }
            "--overwrite" => overwrite = true,
            "--lang" => {
                let code = iter.next().ok_or("--lang needs a language code")?;
                lang = Lang::from_code(code).ok_or("unknown language code")?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err("only one drawing can be read".to_owned());
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or("sketch-import needs a DXF or SVG drawing")?;
    let (settings, ui) = load_settings(config.as_deref())?;
    let solver = load_solver_settings(config.as_deref())?;
    let bytes = eh3d_phoenix_cad::store::load_limited(&file, u64::from(ui.export.max_kib) * 1024)
        .map_err(|e| format!("{}: {}", file.display(), error_text(&e, lang)))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("{}: the drawing is not text", file.display()))?;
    // an SVG drawing is turned into lines and circles in millimetres first
    let is_svg = file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let text = if is_svg {
        let chord_mm = eh3d_phoenix_cad::profile::ProfileLimits::from_settings(&solver).chord_nm
            as f64
            / 1.0e6;
        let (dxf, report) = eh3d_phoenix_cad::svg_import::to_dxf(
            &text,
            chord_mm,
            settings.drawing.max_entities as usize,
        )
        .map_err(|e| format!("{}: {e}", file.display()))?;
        unit_nm = Some(1_000_000);
        for (what, count) in &report.skipped {
            println!(
                "{}",
                eh3d_phoenix_cad::lang::fill(Msg::DxfSkipped.tr(lang), &[what, &count.to_string()])
            );
        }
        dxf
    } else {
        text
    };
    if list_layers {
        let found = eh3d_phoenix_cad::dxf_import::layers(&text)
            .map_err(|e| format!("{}: {}", file.display(), error_text(&e, lang)))?;
        let listed: Vec<String> = found.iter().map(|(k, v)| format!("{k} ({v})")).collect();
        println!(
            "{}",
            eh3d_phoenix_cad::lang::fill(Msg::DxfLayers.tr(lang), &[&listed.join(", ")])
        );
        return Ok(true);
    }
    let name = name.unwrap_or_else(|| {
        file.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let join = eh3d_phoenix_cad::profile::ProfileLimits::from_settings(&solver).join_nm;
    let mut unit_note = None;
    let unit_nm = match unit_nm {
        Some(nm) => nm,
        None => {
            let (nm, note) = eh3d_phoenix_cad::dxf_import::auto_unit(&text, lang)
                .map_err(|e| format!("{}: {}", file.display(), error_text(&e, lang)))?;
            unit_note = Some(note);
            nm
        }
    };
    let imported = eh3d_phoenix_cad::dxf_import::from_dxf_layers(
        &text,
        &name,
        eh3d_phoenix_cad::sketch::SketchPlane::on_face(face),
        unit_nm,
        join,
        settings.drawing.max_entities as usize,
        &chosen,
    )
    .map_err(|e| format!("{}: {}", file.display(), error_text(&e, lang)))?;
    let target = out.unwrap_or_else(|| {
        PathBuf::from(format!(
            "{name}.{}",
            eh3d_phoenix_cad::sketch::file::EXTENSION
        ))
    });
    if target.exists() && !overwrite {
        return Err(error_text(
            &eh3d_phoenix_cad::CadError::ExportExists(target.display().to_string()),
            lang,
        ));
    }
    eh3d_phoenix_cad::sketch::file::write_file(&target, &imported.sketch, settings.backups)
        .map_err(|e| error_text(&e, lang))?;
    println!(
        "{}",
        eh3d_phoenix_cad::lang::fill(
            Msg::DxfRead.tr(lang),
            &[
                &imported.lines.to_string(),
                &imported.circles.to_string(),
                &imported.arcs.to_string()
            ]
        )
    );
    if let Some(note) = unit_note {
        println!("{note}");
    }
    for (what, count) in &imported.skipped {
        println!(
            "{}",
            eh3d_phoenix_cad::lang::fill(Msg::DxfSkipped.tr(lang), &[what, &count.to_string()])
        );
    }
    if imported.off_layers > 0 {
        println!(
            "{}",
            eh3d_phoenix_cad::lang::fill(
                Msg::DxfOffLayers.tr(lang),
                &[&imported.off_layers.to_string()]
            )
        );
    }
    let written = fs::metadata(&target).map_or(0, |m| m.len());
    println!(
        "{}",
        eh3d_phoenix_cad::lang::fill(
            Msg::Exported.tr(lang),
            &[&target.display().to_string(), &written.to_string()]
        )
    );
    Ok(true)
}

/// `section`: reads a project, cuts its solids with a plane and prints the outlines and the area of
/// the material; with `--out` the section is also written as a DXF or SVG drawing.
fn run_section(args: &[String]) -> Result<bool, String> {
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut format: Option<String> = None;
    let mut axis: Option<String> = None;
    let mut at: Option<String> = None;
    let mut overwrite = false;
    let mut lang = Lang::En;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" => {
                config = Some(PathBuf::from(
                    iter.next().ok_or("--config needs a settings file")?,
                ));
            }
            "--axis" => axis = Some(iter.next().ok_or("--axis needs x, y or z")?.clone()),
            "--at" => at = Some(iter.next().ok_or("--at needs a length")?.clone()),
            "--format" => format = Some(iter.next().ok_or("--format needs dxf or svg")?.clone()),
            "--out" => out = Some(PathBuf::from(iter.next().ok_or("--out needs a file")?)),
            "--overwrite" => overwrite = true,
            "--lang" => {
                let code = iter.next().ok_or("--lang needs a language code")?;
                lang = Lang::from_code(code).ok_or("unknown language code")?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err("only one project can be cut".to_owned());
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or("section needs a project file")?;
    let axis_word = axis.as_deref().ok_or("section needs --axis")?;
    let axis = eh3d_phoenix_cad::assembly::Dim::from_name(axis_word)
        .ok_or_else(|| format!("{axis_word}: the axis is x, y or z"))?;
    let at = at.ok_or("section needs --at")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    let section = win.section(axis, &at).map_err(|e| error_text(&e, lang))?;
    print!("{}", eh3d_phoenix_cad::section::text_in(&section, lang));
    if let Some(target) = out {
        let extension = target
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let dxf = match format.as_deref().unwrap_or(extension.as_str()) {
            "dxf" => true,
            "svg" => false,
            _ => {
                return Err(
                    "--out needs a file that ends in .dxf or .svg, or --format dxf|svg".to_owned(),
                )
            }
        };
        let text = eh3d_phoenix_cad::section::render_titled(
            &section,
            &win.settings.drawing,
            dxf,
            &win.settings.title_block,
            &win.model_name(),
            lang,
        )
        .map_err(|e| error_text(&e, lang))?;
        let written = eh3d_phoenix_cad::export::write_file(
            &target,
            &text,
            overwrite,
            win.path.as_deref(),
            win.ui.export.max_kib,
        )
        .map_err(|e| error_text(&e, lang))?;
        println!(
            "{}",
            eh3d_phoenix_cad::lang::fill(
                Msg::Exported.tr(lang),
                &[&target.display().to_string(), &written.to_string()]
            )
        );
    }
    Ok(win.info.failed == 0)
}

/// `balance`: reads a project and prints the mass of its placed parts and their centre of mass.
fn run_balance(args: &[String]) -> Result<bool, String> {
    let (lang, args) = take_lang(args)?;
    let (file, config) = file_and_config(&args, "project file")?;
    let win = open_for_report(&file, config.as_deref(), lang)?;
    print!("{}", win.balance_text().map_err(|e| error_text(&e, lang))?);
    Ok(win.info.failed == 0)
}

/// `library`: reads a library of parts, checks it against the settings and lists its parts.
fn run_library(args: &[String]) -> Result<bool, String> {
    let (file, config) = file_and_config(args, "library file")?;
    let (settings, _) = load_settings(config.as_deref())?;
    let lang = Lang::En;
    let library = catalog::read_file(&file, &settings.design.catalog)
        .and_then(|library| {
            library
                .check_materials(&settings.materials)
                .map(|()| library)
        })
        .map_err(|e| format!("{}: {}", file.display(), error_text(&e, lang)))?;
    println!("library {}: {} parts", library.name, library.parts.len());
    for part in &library.parts {
        println!("  {} {} {}", part.kind().name(), part.name, part.material);
    }
    Ok(true)
}

/// `mesh-info`: reads an STL, OBJ, 3MF or STEP file and, for each solid in it, prints the number
/// of vertices and triangles, whether the surface is closed, the volume, the surface and the size
/// and, when the solid is a prism, its axis, height and outlines; with a density (`--density` or
/// a `--material` of the settings) also its mass, centre of mass and tensor of inertia. The answer
/// is whether every solid is closed.
fn run_mesh_info(args: &[String]) -> Result<bool, String> {
    let mut rest: Vec<String> = Vec::new();
    let mut density_text: Option<String> = None;
    let mut material: Option<String> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--density" => {
                density_text = Some(iter.next().ok_or("--density needs kg/m3")?.clone());
            }
            "--material" => {
                material = Some(iter.next().ok_or("--material needs a name")?.clone());
            }
            other => rest.push(other.to_owned()),
        }
    }
    let (file, config) = file_and_config(&rest, "mesh file")?;
    let (settings, _) = load_settings(config.as_deref())?;
    let density: Option<f64> = match (density_text, material) {
        (Some(_), Some(_)) => {
            return Err("--density and --material cannot be given together".to_owned());
        }
        (Some(text), None) => match text.parse::<u32>() {
            Ok(d) if (1..=100_000).contains(&d) => Some(f64::from(d)),
            _ => {
                return Err(format!(
                    "{text}: --density must be a whole number of kilograms per cubic metre from 1 to 100000"
                ));
            }
        },
        (None, Some(name)) => {
            Some(f64::from(*settings.materials.get(&name).ok_or_else(
                || format!("{name}: not a material of the settings"),
            )?))
        }
        (None, None) => None,
    };
    let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let name = file.to_string_lossy();
    let limits = eh3d_phoenix_cad::step_import::StepLimits::from_settings(&settings.mesh);
    let bodies = eh3d_phoenix_cad::interchange::read_file_bytes_limited(&name, &bytes, &limits)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    println!("{}: {} solids", file.display(), bodies.len());
    let mut all_closed = true;
    for (solid, mesh) in &bodies {
        let closed = mesh.check_closed();
        all_closed &= closed.is_ok();
        let area: f64 = mesh.triangles.iter().map(|t| mesh.area_nm2(t)).sum();
        let size = mesh
            .bounds()
            .map(|(lo, hi)| {
                format!(
                    "{:.3} x {:.3} x {:.3} mm",
                    (hi[0] - lo[0]) / 1.0e6,
                    (hi[1] - lo[1]) / 1.0e6,
                    (hi[2] - lo[2]) / 1.0e6
                )
            })
            .unwrap_or_else(|| "no size".to_owned());
        let state = match closed {
            Ok(()) => "closed".to_owned(),
            Err(why) => format!("not closed ({why})"),
        };
        println!(
            "  {solid}: {} vertices, {} triangles, {state}, volume {:.3} mm3, surface {:.3} mm2, size {size}",
            mesh.vertices.len(),
            mesh.triangles.len(),
            mesh.volume_nm3() / 1.0e18,
            area / 1.0e12
        );
        if let Ok(prism) = mesh.as_prism() {
            let corners: usize = prism.outlines.iter().map(Vec::len).sum();
            println!(
                "    prism along {}: {:.3} mm high from {} = {:.3} mm, {} outlines with {corners} corners, floor {:.3} mm2",
                ["x", "y", "z"][prism.axis],
                prism.height / 1.0e6,
                ["x", "y", "z"][prism.axis],
                prism.low / 1.0e6,
                prism.outlines.len(),
                prism.area_nm2 / 1.0e12
            );
        }
        if let Some(rho) = density {
            match (mesh.centroid_nm(), mesh.inertia_nm5()) {
                (Some(c), Some(t)) => {
                    let volume = mesh.volume_nm3().abs() / 1.0e18;
                    // grams: a cubic millimetre of a material of rho kg/m3 weighs rho * 1e-6 g
                    let grams = volume * rho * 1.0e-6;
                    let k = rho * 1.0e-36;
                    println!(
                        "    mass {grams:.3} g at {rho} kg/m3, centre of mass {:.3} {:.3} {:.3} mm",
                        c[0] / 1.0e6,
                        c[1] / 1.0e6,
                        c[2] / 1.0e6
                    );
                    println!(
                        "    inertia about the centre of mass (g mm2): xx {:.3} yy {:.3} zz {:.3} xy {:.3} xz {:.3} yz {:.3}",
                        t[0][0] * k,
                        t[1][1] * k,
                        t[2][2] * k,
                        t[0][1] * k,
                        t[0][2] * k,
                        t[1][2] * k
                    );
                }
                _ => println!("    no mass: the solid holds no volume"),
            }
        }
    }
    Ok(all_closed)
}

/// A face of the options of `faces`: its name and, after a colon, `inner` or `outer` (the outer
/// face of the wall without it).
fn face_side(
    word: &str,
) -> Result<
    (
        eh3d_phoenix_cad::enclosure::Face,
        eh3d_phoenix_cad::measure::Side,
    ),
    String,
> {
    use eh3d_phoenix_cad::measure::Side;
    let (name, side) = match word.split_once(':') {
        Some((name, "inner")) => (name, Side::Inner),
        Some((name, "outer")) => (name, Side::Outer),
        Some(_) => return Err(format!("{word}: the side is inner or outer")),
        None => (word, Side::Outer),
    };
    let face = eh3d_phoenix_cad::enclosure::Face::from_name(name)
        .ok_or_else(|| format!("{name}: not a face (front, back, left, right, top or bottom)"))?;
    Ok((face, side))
}

/// `faces`: the distance and the angle between two faces of the box of a project. The answer is
/// whether every step of the project was applied.
fn run_faces(args: &[String]) -> Result<bool, String> {
    let mut from: Option<String> = None;
    let mut to: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--from" => from = Some(iter.next().ok_or("--from needs a face")?.clone()),
            "--to" => to = Some(iter.next().ok_or("--to needs a face")?.clone()),
            other => rest.push(other.to_owned()),
        }
    }
    let from = face_side(&from.ok_or("faces needs --from")?)?;
    let to = face_side(&to.ok_or("faces needs --to")?)?;
    let (win, lang) = open_project(&rest, "faces")?;
    let measured = eh3d_phoenix_cad::measure::between_faces(&win.params(), from, to)
        .map_err(|e| error_text(&e, lang))?;
    println!(
        "distance: {} mm",
        eh3d_phoenix_cad::scad::mm_text((measured.distance * 1.0e6).round() as i64)
    );
    println!("angle: {:.0} degrees", measured.angle);
    Ok(win.info.failed == 0)
}

/// An element of a sketch file as the file names it (`e5`).
fn element_word(word: &str) -> Result<eh3d_phoenix_cad::sketch::ElementId, String> {
    word.trim()
        .strip_prefix('e')
        .and_then(|n| n.parse::<u32>().ok())
        .map(eh3d_phoenix_cad::sketch::ElementId)
        .ok_or_else(|| format!("{word}: not an element (they are named like e5)"))
}

fn element_words(text: &str) -> Result<Vec<eh3d_phoenix_cad::sketch::ElementId>, String> {
    text.split(',').map(element_word).collect()
}

fn constraint_word(word: &str) -> Result<eh3d_phoenix_cad::sketch::ConstraintId, String> {
    word.trim()
        .strip_prefix('c')
        .and_then(|n| n.parse::<u32>().ok())
        .map(eh3d_phoenix_cad::sketch::ConstraintId)
        .ok_or_else(|| format!("{word}: not a constraint (they are named like c3)"))
}

/// A length that is a bare number (millimetres) or has a unit.
fn length_word(word: &str) -> Result<eh3d_phoenix_cad::Length, String> {
    let text = if word.trim().parse::<f64>().is_ok() {
        format!("{} mm", word.trim())
    } else {
        word.to_owned()
    };
    eh3d_phoenix_cad::units::parse_length(&text).map_err(|e| format!("{word}: {e}"))
}

/// `sketch-edit`: edits a sketch file with one of the operations of the sketch window and writes
/// it. A refused edit writes nothing; a file that exists is only written over with
/// `--overwrite`.
fn run_sketch_edit(args: &[String]) -> Result<bool, String> {
    use std::collections::BTreeMap;
    let mut options: BTreeMap<String, String> = BTreeMap::new();
    let mut file: Option<PathBuf> = None;
    let mut overwrite = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--overwrite" => overwrite = true,
            "--op" | "--lines" | "--distance" | "--line" | "--cutter" | "--drop" | "--to"
            | "--items" | "--step" | "--count" | "--center" | "--constraint" | "--out"
            | "--config" => {
                let value = iter.next().ok_or_else(|| format!("{arg} needs a value"))?;
                options.insert(arg.clone(), value.clone());
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err("only one sketch can be edited".to_owned());
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or("sketch-edit needs a sketch file")?;
    let need = |key: &str| -> Result<&String, String> {
        options
            .get(key)
            .ok_or_else(|| format!("this operation needs {key}"))
    };
    let op = need("--op")?.clone();
    let config = options.get("--config").map(PathBuf::from);
    let (settings, _) = load_settings(config.as_deref())?;
    let solver = load_solver_settings(config.as_deref())?;
    let mut sketch = eh3d_phoenix_cad::sketch::file::read_file(&file, &solver)
        .map_err(|e| format!("{}: {}", file.display(), error_text(&e, Lang::En)))?;
    let before = sketch.next_ids().0;
    let count = |options: &BTreeMap<String, String>| -> Result<u32, String> {
        options
            .get("--count")
            .ok_or_else(|| "this operation needs --count".to_owned())?
            .parse::<u32>()
            .map_err(|_| "--count is a whole number".to_owned())
    };
    let done: Result<(), eh3d_phoenix_cad::CadError> = match op.as_str() {
        "chamfer" => {
            let lines = element_words(need("--lines")?)?;
            let [a, b] = lines[..] else {
                return Err("--lines names the two lines of the corner".to_owned());
            };
            let distance = length_word(need("--distance")?)?;
            sketch.chamfer_corner(a, b, distance).map(|_| ())
        }
        "trim" => {
            let drop = options.get("--drop").map(|d| element_word(d)).transpose()?;
            sketch
                .trim_line(
                    element_word(need("--line")?)?,
                    element_word(need("--cutter")?)?,
                    drop,
                )
                .map(|_| ())
        }
        "extend" => sketch
            .extend_line(element_word(need("--line")?)?, element_word(need("--to")?)?)
            .map(|_| ()),
        "linear" => {
            let step = need("--step")?;
            let (dx, dy) = step
                .split_once(',')
                .ok_or("--step is DX,DY (millimetres or with a unit)")?;
            sketch
                .linear_pattern(
                    &element_words(need("--items")?)?,
                    length_word(dx)?,
                    length_word(dy)?,
                    count(&options)?,
                )
                .map(|_| ())
        }
        "circular" => {
            let n = count(&options)?;
            let step = match options.get("--step") {
                Some(text) => {
                    let degrees: f64 = text
                        .parse()
                        .map_err(|_| "--step is an angle in degrees".to_owned())?;
                    eh3d_phoenix_cad::units::Angle::new((degrees * 1.0e9).round() as i64)
                        .map_err(|e| error_text(&e, Lang::En))?
                }
                None => eh3d_phoenix_cad::units::Angle::new(360_000_000_000 / i64::from(n.max(1)))
                    .map_err(|e| error_text(&e, Lang::En))?,
            };
            sketch
                .circular_pattern(
                    &element_words(need("--items")?)?,
                    element_word(need("--center")?)?,
                    n,
                    step,
                )
                .map(|_| ())
        }
        "reference" | "driving" => sketch.set_reference(
            constraint_word(need("--constraint")?)?,
            op == "reference",
        ),
        "offset" => sketch
            .offset_outline(
                &element_words(need("--items")?)?,
                length_word(need("--distance")?)?,
            )
            .map(|_| ()),
        other => {
            return Err(format!(
                "{other}: not an operation (chamfer, trim, extend, linear, circular, offset, reference or driving)"
            ))
        }
    };
    done.map_err(|e| error_text(&e, Lang::En))?;
    sketch
        .solve(&solver)
        .map_err(|e| error_text(&e, Lang::En))?;
    let target = options.get("--out").map(PathBuf::from).unwrap_or(file);
    if target.exists() && !overwrite {
        return Err(error_text(
            &eh3d_phoenix_cad::CadError::ExportExists(target.display().to_string()),
            Lang::En,
        ));
    }
    eh3d_phoenix_cad::sketch::file::write_file(&target, &sketch, settings.backups)
        .map_err(|e| error_text(&e, Lang::En))?;
    println!(
        "{op}: {} elements made, {} in all; written to {}",
        sketch.next_ids().0 - before,
        sketch.elements().len(),
        target.display()
    );
    Ok(true)
}

/// The format of the `export` command: a word, and for the drawings the view.
fn export_format(word: &str, view: Option<View>, sheet: bool) -> Result<Format, String> {
    match word {
        "dxf" | "svg" if sheet && view.is_none() => Ok(Format::sheet(word == "dxf")),
        "dxf" | "svg" => Ok(Format::drawing(word == "dxf", view.unwrap_or(View::Top))),
        "pdf" if view.is_none() => Ok(Format::PdfSheet),
        _ if view.is_some() || sheet => {
            Err("--view is only for the formats dxf and svg (pdf is always the sheet)".to_owned())
        }
        "stl" | "step" | "script" | "report" | "bill" | "obj" | "3mf" | "order" => {
            Format::from_name(word).ok_or_else(|| format!("{word}: unknown format"))
        }
        other => Err(format!(
            "{other}: unknown format (stl, obj, 3mf, dxf, svg, pdf, step, script, report, bill or order)"
        )),
    }
}

/// `export`: reads a project and writes the model to a file in a format. The answer is whether the
/// file was written.
fn run_export(args: &[String]) -> Result<bool, String> {
    let mut lang = Lang::from_env();
    let mut config: Option<PathBuf> = None;
    let mut file: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut word: Option<String> = None;
    let mut view: Option<View> = None;
    let mut sheet = false;
    let mut overwrite = false;
    let mut binary: Option<bool> = None;
    let mut title_fields: Vec<(String, String)> = Vec::new();
    let mut cut_line: Option<String> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--lang" => {
                let code = iter.next().ok_or("--lang needs a language code")?;
                lang = Lang::from_code(code).ok_or("unknown language code")?;
            }
            "--config" => {
                let path = iter.next().ok_or("--config needs a settings file")?;
                config = Some(PathBuf::from(path));
            }
            "--title-field" => {
                let pair = iter.next().ok_or("--title-field needs KEY=TEXT")?;
                let (key, value) = pair
                    .split_once('=')
                    .ok_or("--title-field is written KEY=TEXT (title=Front plate)")?;
                title_fields.push((key.trim().to_owned(), value.to_owned()));
            }
            "--cut-line" => {
                cut_line = Some(iter.next().ok_or("--cut-line needs AXIS=POSITION")?.clone());
            }
            "--format" => word = Some(iter.next().ok_or("--format needs a format")?.clone()),
            "--view" => {
                let name = iter
                    .next()
                    .ok_or("--view needs top, front, side or sheet")?;
                if name == "sheet" {
                    sheet = true;
                } else {
                    view =
                        Some(View::from_name(name).ok_or("--view is top, front, side or sheet")?);
                }
            }
            "--out" => out = Some(PathBuf::from(iter.next().ok_or("--out needs a file")?)),
            "--overwrite" => overwrite = true,
            "--ascii" => binary = Some(false),
            "--binary" => binary = Some(true),
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => {
                if file.is_some() {
                    return Err("only one project can be exported".to_owned());
                }
                file = Some(PathBuf::from(other));
            }
        }
    }
    let file = file.ok_or("export needs a project file")?;
    let word = word.ok_or("export needs --format")?;
    let format = export_format(&word, view, sheet)?;
    if binary.is_some() && format != Format::Stl {
        return Err("--ascii and --binary are only for the format stl".to_owned());
    }
    let (settings, ui) = load_settings(config.as_deref())?;
    let mut win = Window::new(lang, settings, ui).map_err(|e| error_text(&e, lang))?;
    win.sketch_settings = load_solver_settings(config.as_deref())?;
    if let Some(b) = binary {
        win.settings.mesh.binary = b;
    }
    let drawing_only = |what: &str| {
        if format.view().is_some() || format.is_sheet() {
            Ok(())
        } else {
            Err(format!("{what} is only for the drawings (dxf and svg)"))
        }
    };
    if !title_fields.is_empty() {
        drawing_only("--title-field")?;
        for (key, value) in &title_fields {
            win.settings
                .title_block
                .set_field(key, value)
                .map_err(|e| error_text(&e, lang))?;
        }
        win.settings.title_block.enabled = true;
    }
    if let Some(text) = &cut_line {
        drawing_only("--cut-line")?;
        win.settings
            .drawing
            .set_cut_line(text)
            .map_err(|e| error_text(&e, lang))?;
    }
    if !win.open(&file) {
        return Err(format!("{}: {}", file.display(), win.status.text(lang)));
    }
    let path = out.unwrap_or_else(|| PathBuf::from(win.export_name(format)));
    if win.export_to(format, &path, overwrite) {
        println!("{}", win.status.text(lang));
        Ok(true)
    } else {
        Err(win.status.text(lang))
    }
}

fn run(args: &[String]) -> Result<bool, String> {
    let Some((command, rest)) = args.split_first() else {
        println!("{HELP}");
        return Ok(true);
    };
    match command.as_str() {
        "help" | "--help" | "-h" => {
            println!("{HELP}");
            Ok(true)
        }
        "config" => {
            if !rest.is_empty() {
                return Err("config takes no arguments".to_owned());
            }
            print!(
                "{DEFAULT_CONFIG}\n{}\n{}\n{}\n{}",
                default_file(),
                eh3d_phoenix_cad::sketch::settings::default_file(),
                eh3d_phoenix_cad::featuredag::default_file(),
                eh3d_phoenix_cad::plans::settings::default_file()
            );
            Ok(true)
        }
        "check" => run_check(rest),
        "interference" => run_interference(rest),
        "bill" => run_bill(rest),
        "cutplan" => run_cutplan(rest),
        "clearance" => run_clearance(rest),
        "placements" => run_placements(rest),
        "holes" => run_holes(rest),
        "partlist" => run_partlist(rest),
        "bom" => run_bom(rest),
        "project-units" => run_project_units(rest),
        "compare" => run_compare(rest),
        "balance" => run_balance(rest),
        "revolve" => run_revolve(rest),
        "sweep" => run_sweep(rest),
        "sketch-export" => run_sketch_export(rest),
        "section" => run_section(rest),
        "sketch-import" => run_sketch_import(rest),
        "library" => run_library(rest),
        "mesh-info" => run_mesh_info(rest),
        "faces" => run_faces(rest),
        "sketch-edit" => run_sketch_edit(rest),
        "export" => run_export(rest),
        c if eh3d_phoenix_cad::tools_cli::COMMANDS.contains(&c) => {
            let (out, ok) = eh3d_phoenix_cad::tools_cli::run(c, rest)?;
            print!("{out}");
            Ok(ok)
        }
        c if eh3d_phoenix_cad::mesh_cli::COMMANDS.contains(&c) => {
            let (out, ok) = eh3d_phoenix_cad::mesh_cli::run(c, rest)?;
            print!("{out}");
            Ok(ok)
        }
        c if eh3d_phoenix_cad::server_cli::COMMANDS.contains(&c) => {
            let out = eh3d_phoenix_cad::server_cli::run(c, rest)?;
            print!("{out}");
            Ok(true)
        }
        "gui" => Err("this build has no window: build it with --features gui".to_owned()),
        other => Err(format!("{other}: unknown command (see help)")),
    }
}

/// The arguments of the window when they ask for it instead of a command: none, `gui`, an option
/// of the window, or a single project file given by the system to open.
#[cfg(feature = "gui")]
fn window_request(args: &[String]) -> Option<Result<shot::WindowArgs, String>> {
    match args.first().map(String::as_str) {
        None | Some("gui") => {}
        Some(
            "help" | "--help" | "-h" | "config" | "check" | "interference" | "bill" | "cutplan"
            | "clearance" | "placements" | "holes" | "partlist" | "bom" | "balance" | "revolve"
            | "sweep" | "sketch-export" | "section" | "sketch-import" | "library" | "mesh-info"
            | "faces" | "sketch-edit" | "export",
        ) => return None,
        Some(first) if first.starts_with("--") => {}
        Some(first) => {
            let path = Path::new(first);
            if path.extension().is_none() || !path.is_file() {
                return None;
            }
        }
    }
    Some(shot::parse_window_args(args, 1))
}

#[cfg(feature = "gui")]
fn start_of(parsed: &shot::WindowArgs) -> Result<eh3d_phoenix_cad::gui::Start, String> {
    let (settings, ui) = load_settings(parsed.config.as_deref())?;
    let (solver, sketch_ui) = load_sketch_settings(parsed.config.as_deref())?;
    let (project, sketch) = match parsed.files.first().cloned() {
        Some(f) if shot::is_sketch_file(&f) => (None, Some(f)),
        other => (other, None),
    };
    let view = parsed.view.unwrap_or(if sketch.is_some() {
        shot::StartView::Sketch
    } else {
        shot::StartView::Model
    });
    Ok(eh3d_phoenix_cad::gui::Start {
        lang: parsed.lang.unwrap_or_else(Lang::from_env),
        settings,
        ui,
        solver,
        sketch_ui,
        project,
        sketch,
        view,
        window: parsed.window,
        config: parsed.config.clone(),
        viewer: parsed.viewer.clone(),
    })
}

#[cfg(feature = "gui")]
fn open_window(parsed: &shot::WindowArgs) -> ExitCode {
    let result = start_of(parsed).and_then(eh3d_phoenix_cad::gui::run);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: the window could not be opened: {e}");
            ExitCode::from(1)
        }
    }
}

/// The picture mode: `--shot FILE.png` opens the window, saves what it draws as a PNG file and
/// closes. The exit code is 0 when the file was written, 1 when it was not and 2 when the
/// arguments are wrong.
#[cfg(feature = "gui")]
fn take_picture(args: &[String]) -> ExitCode {
    let parsed = match shot::parse_window_args(args, 1) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let Some(request) = parsed.shot.clone() else {
        eprintln!("error: --shot needs a file to write");
        return ExitCode::from(2);
    };
    let result =
        start_of(&parsed).and_then(|start| eh3d_phoenix_cad::gui::run_shot(start, request));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: the picture could not be taken: {e}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if shot::wants_picture(&args) {
        #[cfg(feature = "gui")]
        return take_picture(&args);
        #[cfg(not(feature = "gui"))]
        {
            eprintln!("error: this build has no window: build it with --features gui");
            return ExitCode::from(1);
        }
    }
    #[cfg(feature = "gui")]
    if let Some(parsed) = window_request(&args) {
        return match parsed {
            Ok(parsed) => open_window(&parsed),
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(2)
            }
        };
    }
    if let Some(command) = args.first().filter(|c| {
        eh3d_phoenix_cad::history_cli::COMMANDS.contains(&c.as_str())
            || eh3d_phoenix_cad::plans_cli::COMMANDS.contains(&c.as_str())
            || eh3d_phoenix_cad::suite_cli::COMMANDS.contains(&c.as_str())
            || eh3d_phoenix_cad::sketchcheck::COMMANDS.contains(&c.as_str())
    }) {
        // a check that does not pass exits with 2; a command that could not run exits with 1
        let ran = if eh3d_phoenix_cad::plans_cli::COMMANDS.contains(&command.as_str()) {
            eh3d_phoenix_cad::plans_cli::run(command, &args[1..])
        } else if eh3d_phoenix_cad::suite_cli::COMMANDS.contains(&command.as_str()) {
            eh3d_phoenix_cad::suite_cli::run(command, &args[1..])
        } else if eh3d_phoenix_cad::sketchcheck::COMMANDS.contains(&command.as_str()) {
            eh3d_phoenix_cad::sketchcheck::run(command, &args[1..])
        } else {
            eh3d_phoenix_cad::history_cli::run(command, &args[1..])
        };
        return match ran {
            Ok((out, ok)) => {
                print!("{out}");
                if ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(2)
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::from(1)
            }
        };
    }
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
