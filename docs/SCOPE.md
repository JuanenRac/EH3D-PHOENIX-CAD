# EH3D-PHOENIX-CAD: scope

Status: **early code**. EH3D-PHOENIX-CAD has a first library, `eh3d-phoenix-cad`: a parametric enclosure with round holes, structured change operations, an OpenSCAD-style script and a fit check against the envelope of a board. This page says what it is meant to do, what it will not do, and where it sits. Nothing here has been run on a machine.

## Purpose

EH3D-PHOENIX-CAD is planned as a parametric mechanical CAD focused on the design of enclosures, chassis and heat sinks.

Mechanical design for the PHOENIX family: parametric enclosures, chassis and heat sinks that live in the same project as the board.

## What it is planned to do

* Parametric models of enclosures, chassis and heat sinks, regenerated from named parameters.
* A mechanical space shared with the board tool inside the same project: changes travel as structured, reviewable operations (holes, connectors, component height, heat dissipation, envelope) and never as silent edits.
* Export to STEP and to OpenSCAD-style scripts.
* First step: a generator of simple parametric enclosures. Later steps: sketches with constraints, solid operations, assemblies and a dependency graph.

## What it will not do

* Full parametric CAD (solid operations other than raising a profile from a face or cutting it into one, assemblies) is a later, separate effort; the sketch with its constraints, its solver and its window exists, and so do the extrusions and pockets of its closed outlines (`docs/SKETCH.md`) and the dependency graph.
* No toolpaths and no G-code: the CAM programs of the suite read the project and do that.
* No machine control and no motion of any kind.
* The 3D view of the board and the fit check against an existing enclosure may live in the board tool; where they live is still open.

## Planned shape

A library written in Rust with a window and a command-line interface and, where it applies, a kind of task for the server. Programs of the suite exchange work through project files or through the server, never by depending on each other.

The first crate is `eh3d-phoenix-cad` (lengths with their own range, the enclosure, the operations, the script and the fit check). The program of the same crate has a window (the model tree, the properties, a 3D view and a plan where holes and posts are dragged and distances and angles are measured, and a menu that writes the script, the STEP file and the fit report to a file, built with `--features gui`) and the commands `check` and `config`. The server task is not written (the cooling fins and the estimate of their heat are, in the library, the window and the script, but not in the STEP file); the other crates will be decided together with their code.

## Where it sits

It belongs to the PHOENIX family of the EH3D Fabrication Suite: independent repositories that relate through versioned contracts. The design tool (EH3D-PHOENIX-PCB) does not depend at build time on the repositories that depend on it.
See DEPENDENCIES.md for the relations, REQUIREMENTS.md for the requirements and STATUS.md for the honest state.
