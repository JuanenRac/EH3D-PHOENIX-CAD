# EH3D-PHOENIX-CAD: dependencies

One row per relation with another repository. The kind is conceptual, compile-time or run-time. The state of a contract is: not written, drafted, accepted, frozen or published. A repository existing does not mean that its contract is ready: this project does not implement or consume a contract that is not accepted.

| From | To | Kind | Contract | Contract state | Minimum version | Tested version | Compatibility test | Change owner | What it blocks | Migration |
|---|---|---|---|---|---|---|---|---|---|---|
| EH3D-PHOENIX-CAD | EH3D-PHOENIX-PCB | compile + runtime | project format (mechanical domain) | drafted | not set | not set | to be written with the first code | the owner of the contract | all of it | to be defined |
| EH3D-PHOENIX-CAD | EH3D-COMMON | compile | project container and diagnostic crate | drafted | not set | not set | to be written with the first code | the owner of the contract | reading and writing projects | to be defined |
| EH3D-PHOENIX-CAD | EH3D-THERMAL-LAB | conceptual | geometry for thermal coupling | not written | not set | not set | to be written with the first code | the owner of the contract | nothing here is blocked by it; it is blocked by this | to be defined |

Rules: repositories are built next to each other as sibling folders and each declares in its manifest the versions of the others that it needs, once those exist. A lower layer never depends on a higher one. The manifest field for build dependencies stays empty until code exists.
