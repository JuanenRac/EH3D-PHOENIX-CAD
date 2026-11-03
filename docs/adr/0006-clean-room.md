# 0006: clean-room policy for third-party formats and tools

Status: accepted.

## Decision

- Formats are learned from public specifications, from the project's own files and from golden-file comparisons.
- The executables and libraries of third-party software are never decompiled, disassembled or inspected to derive format details.
- If a person analyses an executable, they write a specification in natural language and whoever implements sees only that specification.

## Notes

- Help texts of other vendors may serve as a language reference but are never copied into a repository.
- Scripts, libraries and manufacturing jobs supplied by other vendors are never redistributed; only the project's own files may be published, and only when its author decides.
