# 0015: an embedded scripting language is optional, off by default and sandboxed (design only)

Status: accepted as a design; not implemented.

## Context

Designs are sometimes easier to write as a script than to click. A scripting language is also a way to run code that came from somewhere else.

## Decision

1. The programs are fully usable without any scripting. The language is **optional**: a switch in the settings, **off by default**, and the shipped build does not run a script unless the person turned it on.
2. A script runs in a **sandbox**: no file, network, process, clock or environment access; only the operations of the history (record 0012) and the values of the parameters; a limit of steps, memory and depth that are settings; the same script gives the same result.
3. A script produces nodes of the history; it never writes behind the history's back, so every result is checked as any other node and every failure has its code.
4. Nothing of this is implemented. When it is, it needs a record that names the language and its licence and a threat model entry in `docs/THREAT_MODEL.md`.

## Consequences

* The history format is already enough for a script to write; no change is needed to make room for it.
* The OpenSCAD-style script that the program writes (`scad.rs`) is an export, not this language.
