# 0001: independent repositories related by contracts

Status: accepted.

## Decision

Every repository of the suite is independent. Repositories relate through versioned contracts and build next to each other as sibling folders with path dependencies. Inside a repository there may be a Cargo workspace of crates.

## Consequences

- The manifest declares the sibling repositories it depends on; the CI checks out the tags named in it.
- A repository does not depend at build time on the satellites that depend on it; it reaches them through their command line or through server tasks.
- A lower layer never depends on a higher one: `EH3D-COMMON` depends on nothing in the suite, so the common diagnostic crate lives there.
- The server links engine crates only (behind cargo features, so a small board can leave them out), never application crates.
