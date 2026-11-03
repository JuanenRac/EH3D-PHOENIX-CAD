# 0002: the design tool keeps its own project format; `.eh3dproj` stays for exchange

Status: accepted.

## Decision

The design tool stores everything (schematic, board, mechanical, simulation, thermal, RF, history, passport) in its own `.phoenixproj` project. The `.eh3dproj` package, version 1.0.0, stays unchanged and is the manufacturing and exchange package: the tool exports it and imports it.

A generic container in `EH3D-COMMON` (ZIP, integrity record, limits, deterministic and atomic writer) is shared by both formats without changing the behaviour of `.eh3dproj`.

## Consequences

Two formats, kept in step by the shared container and by one deterministic, tested export path. Exports write `.eh3dproj` 1.0.0 by default; 1.1.0 is written only when asked for.
