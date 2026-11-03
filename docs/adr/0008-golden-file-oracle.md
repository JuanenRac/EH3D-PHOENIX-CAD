# 0008: a reference program as an oracle through golden files

Status: accepted.

## Context

A reference program of the market may stop being available at any moment (a discontinued product, a licence service that goes away), and it cannot be run in a pipeline.

## Decision

Results produced by the reference program (does a file open without warnings, rule-check reports, manufacturing outputs, converted files) are generated while it still works, stored as golden files next to the private projects with redundant, verified copies on more than one medium, and compared by local scripts.

For every golden set the following is recorded: the version and build of the program, the rule and job files used, its settings, the operating system and a `SHA256SUMS` manifest. Golden cases include limit geometries, libraries, rules and hierarchies, not only projects that open. The original files are never modified; work is done on copies with their hashes.

## Consequences

The levels of fidelity are measured only on projects that have golden files. Golden files and private projects never enter a public repository; reports about them are redacted.
