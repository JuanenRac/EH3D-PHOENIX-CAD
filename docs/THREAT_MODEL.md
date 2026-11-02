# EH3D-PHOENIX-CAD: threat model

Basic model for a project that has no code yet. Every row is a plan, not a result: no mitigation exists and no test has been run. The rows are the threats to design against from the first line of code.

| Threat | Planned mitigation | Planned test | State | Residual risk |
|---|---|---|---|---|
| Hostile or malformed STEP or 3D model from a third party | bounded parsers, size and depth limits, provenance and licence record | fuzzing and hostile samples | planned | not assessed: nothing is implemented |
| Corrupt or malicious project or input files (oversized entries, path tricks, deep nesting, resource exhaustion) | name rules, size and depth limits, bounded readers | fuzzing and hostile samples | planned | not assessed: nothing is implemented |
| Temporary files, logs and reports that reveal private designs | no private data in outputs; redaction | automated pattern search | planned | not assessed: nothing is implemented |
| Compromised dependency or binary | pinned versions, licence and advisory checks, software bill of materials for releases | CI checks | planned | not assessed: nothing is implemented |
| Power loss or forced close during a save | atomic write: temporary file, then rename; old file never deleted first | interruption tests at controlled points | planned | not assessed: nothing is implemented |

Minimum negative tests once there is code: hostile files and resource limits; secrets absent from logs, errors and reports; pinned dependencies and licence review before publishing. Report a vulnerability privately as described in SECURITY.md, without attaching private designs.
