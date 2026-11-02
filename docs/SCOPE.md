# EH3D-PHOENIX-CAD: scope

Status: **planned**. EH3D-PHOENIX-CAD has no code. This page says what it is meant to do, what it will not do, and where it sits. Nothing here has been built or run.

## Purpose

EH3D-PHOENIX-CAD is planned as a parametric mechanical CAD focused on the design of enclosures, chassis and heat sinks.

Mechanical design for the PHOENIX family: parametric enclosures, chassis and heat sinks that live in the same project as the board.

## What it is planned to do

* Parametric models of enclosures, chassis and heat sinks, regenerated from named parameters.
* A mechanical space shared with the board tool inside the same project: changes travel as structured, reviewable operations (holes, connectors, component height, heat dissipation, envelope) and never as silent edits.
* Export to STEP and to OpenSCAD-style scripts.
* First step: a generator of simple parametric enclosures. Later steps: sketches with constraints, solid operations, assemblies and a dependency graph.

## What it will not do

* Full parametric CAD (sketch solver, assemblies, dependency graph) is a later, separate effort.
* No toolpaths and no G-code: the CAM programs of the suite read the project and do that.
* No machine control and no motion of any kind.
* The 3D view of the board and the fit check against an existing enclosure may live in the board tool; where they live is still open.

## Planned shape

A library without a window, written in Rust (planned), with a command-line interface and, where it applies, a kind of task for the server. Programs of the suite exchange work through project files or through the server, never by depending on each other.

Crates, names and file layout will be decided together with the first code; no crate exists yet.

## Where it sits

It belongs to the PHOENIX family of the EH3D Fabrication Suite: independent repositories that relate through versioned contracts. The design tool (EH3D-PHOENIX-PCB) does not depend at build time on the repositories that depend on it.
See DEPENDENCIES.md for the relations, REQUIREMENTS.md for the requirements and STATUS.md for the honest state.
