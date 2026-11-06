<p align="center">
  <img src="images/EH3D_PHOENIX_CAD_BANNER.svg" alt="EH3D-PHOENIX-CAD banner" width="100%">
</p>

# <img src="images/brand/icon-64.png" alt="" width="40" height="40" align="absmiddle"> EH3D-PHOENIX-CAD
<p align="center">🇺🇸 <b>English</b> | <a href="README_spa.md">🇪🇸 Español</a> | <a href="README_fra.md">🇫🇷 Français</a> | <a href="README_ita.md">🇮🇹 Italiano</a> | <a href="README_deu.md">🇩🇪 Deutsch</a> | <a href="README_zho.md">🇨🇳 中文</a> | <a href="README_jpn.md">🇯🇵 日本語</a> | <a href="README_por.md">🇵🇹 Português</a></p>

### EH3D-PHOENIX-CAD: planned as a parametric mechanical CAD focused on the design of enclosures, chassis and heat sinks. A first model of enclosures exists as a library with a window that has never been opened on a screen; nothing has been run on a machine.

<p align="center">
  <img src="https://img.shields.io/badge/License-GPL%203.0--or--later-blue.svg" alt="GPL 3.0 or later">
  <img src="https://img.shields.io/badge/Stack-Markdown%20%2B%20Python-blue.svg" alt="Markdown and Python">
  <img src="https://img.shields.io/badge/Languages-8-00E5FF.svg" alt="Languages">
  <img src="https://img.shields.io/badge/Tests-343-2EA043.svg" alt="Tests">
  <img src="https://img.shields.io/badge/Version-0.0.3-0f6cbd.svg" alt="Version">
  <img src="https://img.shields.io/badge/Maturity-scaffolding-FFB020.svg" alt="Maturity">
</p>

---

**Honesty check - what actually runs today:** a Rust library with a parametric enclosure: lengths in integer nanometres with their own range, holes in the faces (round, countersunk, counterbored, rectangular and oval), rounded and chamfered edges, parameters that are formulas with units over variables, constants and other parameters (exact fractions, checked overflow), structured change operations that report conflicts and leave the model unchanged, posts under the mounting holes of a board with a lid screwed onto them, the recipe of the box kept in the mechanical domain of the project file, an OpenSCAD-style script and a fit check against the envelope of a board. A window (egui, built with `--features gui`) shows the tree of the model, the properties of each item, a 3D view and a plan, and a `check` command reads a project and prints its measures, its fit and the state of every step; the logic of the window is tested without a screen. It has 340 tests of its own and they pass on a computer. It has only been tried on models made by its own tests; the window has never been opened on a screen and nothing has been run on a machine, and the STEP file it writes has only been checked for its structure, and heat sinks, sketches and assemblies are only planned.

---

> **Safety and security status:** this repository is at its first steps and is not a safety system. Nothing written here replaces the limits, interlocks and emergency stop of the machine and its controller.

## 📋 At a glance

| | |
|---|---|
| Version | `0.0.3` |
| Maturity | Scaffolding |
| Tests | 343 |
| Stack | Markdown, Python 3.10+ |
| On real hardware | Not run on hardware yet |

**Jump to:** [Overview](#-overview) · [Architecture](#-architecture) · [Quick start](#-quick-start) · [Testing](#-testing) · [Repository Structure](#-repository-structure) · [Related Projects](#-related-projects)

## 🚦 Project status

Each line is one part of this repository and the light says how far it is today. It is written from what the repository can show (tests, builds, runs), not from what is planned.

🟢 Done and verified · 🟡 Works, with known limits · 🟠 Partly done or not verified yet · 🔴 Not done or not working

| Area | State | What it means here |
|---|---|---|
| Repository checks | 🟡 | The repository check, the check of the pictures and the 3 tests of the base run in CI and pass. |
| Tests | 🟡 | The 340 tests of the Rust library and the 3 of the base pass on a computer. They use only models made by the tests themselves. |
| Real hardware | 🟠 | Not run on any machine or board: nothing here has been tried on hardware. |
| Planned function | 🟠 | Partly done: a parametric enclosure with holes, posts and a screwed lid, an OpenSCAD-style script, a fit check and the recipe of the box in the project file exist, and so does a first STEP writer checked only for its structure; heat sinks, sketches and assemblies are not started. |
| Requirements | 🟠 | 4 of 6 requirements are at prototype (the enclosure, the script, the change operations and the mechanical range), 2 are only planned and none is validated. |

## 🎯 Overview

* **Planned:** EH3D-PHOENIX-CAD is meant to be a parametric mechanical CAD focused on the design of enclosures, chassis and heat sinks, as a library with a window, a command-line interface and, when it applies, a kind of server task. The library exists with a first parametric enclosure, a window that shows its tree, its properties, a 3D view and a plan (never opened on a screen yet) and a `check` command; the server task, the STEP export and the heat sinks do not exist yet.

## 🧩 Role in EH3D

EH3D-PHOENIX-CAD is part of the PHOENIX family of the EH3D Fabrication Suite. What it will do, what it will not do yet, its requirements and its relations with other repositories are written in the `docs/` folder.

## 🏛️ Architecture

<p align="center">
  <img src="images/architecture.svg" alt="EH3D-PHOENIX-CAD architecture: who uses it, what it holds and what it depends on" width="100%">
</p>

How the parts of this repository fit together: who uses it, what it holds and what it depends on. The picture is drawn from `docs/architecture.json` and checked against the code.

## 🚀 Quick start

You need Python 3.10 or newer; there is nothing to install.

```bash
python -I tools/ci_validate.py
python -I tools/make_architecture.py --check
python -I tools/make_structure.py --check --readmes
python -I -m unittest discover -s tools -p "test_*.py"
```

To try the program (it needs Rust; the window needs `--features gui`):

```bash
cargo run -p eh3d-phoenix-cad -- check crates/eh3d-phoenix-cad/examples/box.phoenixproj
cargo run --release -p eh3d-phoenix-cad --features gui -- crates/eh3d-phoenix-cad/examples/box.phoenixproj
cargo run --release -p eh3d-phoenix-cad --features gui -- gui crates/eh3d-phoenix-cad/examples/box.phoenixproj --shot box.png
```

## 🧪 Testing

The Rust library has 340 tests (`cargo test`) and the base has 3 (`python -I -m unittest discover -s tools -p "test_*.py"`); all pass on a computer. The CI runs the repository check, the check of the pictures, the tests of the base and, for the Rust code, the format check, clippy with warnings as errors and the tests on Linux and Windows, and it also lints the window with clippy (`--features gui`).

## 🏗️ Build & Release

The version lives in the `VERSION` file and in `eh3d.project.json`, and each version has its entry in [CHANGELOG.md](CHANGELOG.md). There is nothing to build; releases are published on GitHub.

## 📂 Repository Structure

<p align="center">
  <img src="images/repository-structure.svg" alt="Repository structure of EH3D-PHOENIX-CAD" width="100%">
</p>

<details>
<summary>Text version of the tree</summary>

```text
EH3D-PHOENIX-CAD/
├── docs/
│   ├── architecture.json structure.json         the data of the two pictures
│   ├── SCOPE.md REQUIREMENTS.md                 what is planned and its requirements
│   ├── DEPENDENCIES.md THREAT_MODEL.md          relations with other repositories and the threat model
│   ├── DECISIONS.md STATUS.md                   decisions that apply here and the honest status
│   ├── NOT_YET.md RULES.md                      ideas left for later and rules for writing code
│   ├── ERROR_CODES.md                           the diagnostic codes PHOENIX-<AREA>-<NNN>
│   ├── EXPRESSIONS.md                           the language of the formulas of the parameters
│   ├── PROJECT_DOMAIN.md                        the recipe of the box in the project file
│   └── adr/                                     the accepted decision records
├── Cargo.toml Cargo.lock                        the Rust workspace and its locked dependencies
├── crates/                                      the mechanical model: enclosure, operations, script, fit check, window
├── AGENTS.md                                    rules for anyone who changes the repository
├── tools/                                       the repository check, its tests and the picture tools
├── images/                                      banner and the Architecture and Repository Structure pictures
├── .github/                                     CI and dashboard workflows, issue and pull-request templates
├── eh3d.project.json                            manifest: version, role, maturity and build command
├── VERSION                                      the version of the repository
├── CHANGELOG.md                                 what changed in each version
├── SECURITY.md SUPPORT.md                       how to report a vulnerability and where to ask
└── CONTRIBUTING.md CODE_OF_CONDUCT.md LICENSE   how to contribute, the code of conduct and the licence
```

</details>

## 🗺️ Roadmap & Known limits

**Known limits today**

* Early code: only the model of an enclosure exists, tried on models made by its own tests; nothing has been run on a machine and no requirement is validated.

**Next**

* Write the mechanical domain of the project format with the board tool, then the change operations that come from it; after that the STEP export, the heat sinks and the command line, each with its tests.

## 🌍 Languages

README and texts exist in English, Spanish, French, Italian, German, Chinese, Japanese and European Portuguese.

## 🔗 Related Projects

* [EH3D-DOCS](https://github.com/JuanenRac/EH3D-DOCS): the map of the suite, its architecture, the conventions and the shared checks
* [EH3D-COMMON](https://github.com/JuanenRac/EH3D-COMMON): the object identity (UUID v7) that the holes use; later the project container and the diagnostic crate
* Planned relations, not built yet (EH3D-PHOENIX-PCB, EH3D-THERMAL-LAB): see [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md)

## 📚 Documentation

* [The conventions of the suite](https://github.com/JuanenRac/EH3D-DOCS/blob/main/docs/CONVENTIONS.md)
* [Changelog](CHANGELOG.md)
* [Scope, requirements, dependencies, threat model and decisions](docs/SCOPE.md)

## 🤝 Contributing & Community

* Before a pull request run `python -I tools/ci_validate.py` and the tests of the base: all must pass. See [CONTRIBUTING.md](CONTRIBUTING.md) and the [code of conduct](CODE_OF_CONDUCT.md).
* The README exists in eight languages: a change to the text is made in all eight files, and the two pictures are drawn from `docs/architecture.json` and `docs/structure.json` by `tools/make_architecture.py` and `tools/make_structure.py`.
* Report a vulnerability privately: [SECURITY.md](SECURITY.md). Questions, ideas and reports: electrohobby3d@gmail.com ([SUPPORT.md](SUPPORT.md)).

## 👤 AUTHOR

**JuanenRac (Electro Hobby 3D)** · electrohobby3d@gmail.com

## 📜 LICENSE

Software: GPL-3.0-or-later. See [LICENSE](LICENSE). Hardware designs of the suite use CERN OHL-S.
