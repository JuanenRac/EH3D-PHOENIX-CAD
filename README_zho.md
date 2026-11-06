<p align="center">
  <img src="images/EH3D_PHOENIX_CAD_BANNER.svg" alt="EH3D-PHOENIX-CAD banner" width="100%">
</p>

# <img src="images/brand/icon-64.png" alt="" width="40" height="40" align="absmiddle"> EH3D-PHOENIX-CAD
<p align="center"><a href="README.md">🇺🇸 English</a> | <a href="README_spa.md">🇪🇸 Español</a> | <a href="README_fra.md">🇫🇷 Français</a> | <a href="README_ita.md">🇮🇹 Italiano</a> | <a href="README_deu.md">🇩🇪 Deutsch</a> | 🇨🇳 <b>中文</b> | <a href="README_jpn.md">🇯🇵 日本語</a> | <a href="README_por.md">🇵🇹 Português</a></p>

### EH3D-PHOENIX-CAD：计划作为专注于外壳、机箱和散热器设计的参数化机械 CAD。已有第一个外壳模型（以库的形式），并带有一个从未在屏幕上打开过的窗口，尚未在任何机器上运行。

<p align="center">
  <img src="https://img.shields.io/badge/License-GPL%203.0--or--later-blue.svg" alt="GPL 3.0 or later">
  <img src="https://img.shields.io/badge/Stack-Markdown%20%2B%20Python-blue.svg" alt="Markdown and Python">
  <img src="https://img.shields.io/badge/Languages-8-00E5FF.svg" alt="Languages">
  <img src="https://img.shields.io/badge/Tests-375-2EA043.svg" alt="Tests">
  <img src="https://img.shields.io/badge/Version-0.0.3-0f6cbd.svg" alt="Version">
  <img src="https://img.shields.io/badge/Maturity-scaffolding-FFB020.svg" alt="Maturity">
</p>

---

**诚实性核对 - 今天真正能运行的内容：** 一个 Rust 库，带参数化外壳：整数纳米长度（有自己的范围）、面上的孔（圆孔、锥形沉孔、柱形沉孔、矩形孔和长圆孔）、圆角和倒角的边、可写成含单位的公式的参数（基于变量、常量和其他参数，精确分数，溢出受检）、会报告冲突并保持模型不变的结构化修改操作、电路板安装孔下方的立柱及用螺钉固定其上的盖子、保存在项目文件机械域中的外壳配方、OpenSCAD 风格的脚本，以及对电路板外形的装配检查。一个窗口（egui，用 `--features gui` 构建）显示模型树、每一项的属性、3D 视图和俯视图，`check` 命令读取项目并打印其尺寸、装配情况和每一步的状态；窗口的逻辑无需屏幕即可测试。它有 372 个自己的测试，在计算机上通过。它只在自己的测试生成的模型上试过；窗口从未在屏幕上打开过，也没有在机器上运行，写出的 STEP 文件只检查过结构，2D 草图（几何、约束和求解器，尚无窗口）只在自己的测试生成的草图上试过，散热器和装配仍只是计划。

---

> **安全（safety）与信息安全现状：** 本仓库刚刚起步，不是安全系统（safety）。这里写的任何内容都不能取代机床及其控制器的限位、联锁和急停。

## 📋 速览

| | |
|---|---|
| 版本 | `0.0.3` |
| 成熟度 | 脚手架 |
| 测试 | 375 |
| 技术栈 | Markdown, Python 3.10+ |
| 真实硬件 | 尚未在硬件上运行 |

**跳转到：** [概述](#-概述) · [架构](#-架构) · [快速开始](#-快速开始) · [测试](#-测试) · [仓库结构](#-仓库结构) · [相关项目](#-相关项目)

## 🚦 项目状态

每一行是本仓库的一个部分，灯号表示它今天做到了哪一步。内容依据仓库能够展示的证据（测试、构建、运行），而不是计划。

🟢 已完成并验证 · 🟡 可用，有已知限制 · 🟠 部分完成或尚未验证 · 🔴 未完成或不可用

| 领域 | 状态 | 在这里的含义 |
|---|---|---|
| 仓库检查 | 🟡 | 仓库检查、图片检查以及基础部分的 3 个测试都在 CI 中运行并通过。 |
| 测试 | 🟡 | Rust 库的 372 个测试和基础部分的 3 个测试在计算机上通过。它们只使用测试自己生成的模型。 |
| 真实硬件 | 🟠 | 没有在任何机床或电路板上运行过：这里没有任何内容在硬件上试过。 |
| 计划的功能 | 🟠 | 部分完成：已有带孔、立柱和螺钉固定盖子的参数化外壳、OpenSCAD 风格的脚本、装配检查、项目文件中的外壳配方，以及只检查过结构的首个 STEP 写出器；已有带约束和求解器的 2D 草图（尚无窗口）；散热器和装配尚未开始。 |
| 需求 | 🟠 | 6 项需求中有 4 项处于原型阶段（外壳、脚本、修改操作和机械范围），2 项仍是计划，没有任何一项通过验证。 |

## 🎯 概述

* **计划中：** EH3D-PHOENIX-CAD 计划成为专注于外壳、机箱和散热器设计的参数化机械 CAD，形态是带窗口的库、命令行界面，并在适用时提供一种服务器任务。库已带着第一个参数化外壳、显示模型树、属性、3D 视图和俯视图的窗口（从未在屏幕上打开过）以及 `check` 命令存在；服务器任务、STEP 导出和散热器尚不存在。

## 🧩 在 EH3D 中的角色

EH3D-PHOENIX-CAD 是 EH3D Fabrication Suite 的 PHOENIX 系列的一部分。它将做什么、暂时不做什么、需求以及与其他仓库的关系都写在 `docs/` 文件夹中。

## 🏛️ 架构

<p align="center">
  <img src="images/architecture.svg" alt="EH3D-PHOENIX-CAD 的架构：谁在使用、包含什么、依赖什么" width="100%">
</p>

本仓库各部分如何协作：谁在使用它、它包含什么、它依赖什么。图片由 `docs/architecture.json` 绘制，并与代码核对。

## 🚀 快速开始

需要 Python 3.10 或更高版本；无需安装任何东西。

```bash
python -I tools/ci_validate.py
python -I tools/make_architecture.py --check
python -I tools/make_structure.py --check --readmes
python -I -m unittest discover -s tools -p "test_*.py"
```

试用该程序（需要 Rust；窗口需要 `--features gui`）：

```bash
cargo run -p eh3d-phoenix-cad -- check crates/eh3d-phoenix-cad/examples/box.phoenixproj
cargo run --release -p eh3d-phoenix-cad --features gui -- crates/eh3d-phoenix-cad/examples/box.phoenixproj
cargo run --release -p eh3d-phoenix-cad --features gui -- gui crates/eh3d-phoenix-cad/examples/box.phoenixproj --shot box.png
```

## 🧪 测试

Rust 库有 372 个测试（`cargo test`），基础部分有 3 个（`python -I -m unittest discover -s tools -p "test_*.py"`）；全部在计算机上通过。CI 运行仓库检查、图片检查、基础部分的测试，对 Rust 代码还运行格式检查、把警告当作错误的 clippy，以及 Linux 和 Windows 上的测试，并用 clippy（`--features gui`）检查窗口。

## 🏗️ 构建与发布

版本号位于 `VERSION` 文件和 `eh3d.project.json`，每个版本在 [CHANGELOG.md](CHANGELOG.md) 中都有条目。没有需要构建的东西；版本发布在 GitHub 上。

## 📂 仓库结构

<p align="center">
  <img src="images/repository-structure.svg" alt="EH3D-PHOENIX-CAD 的仓库结构" width="100%">
</p>

<details>
<summary>目录树的文本版</summary>

```text
EH3D-PHOENIX-CAD/
├── docs/
│   ├── architecture.json structure.json         两张图的数据
│   ├── SCOPE.md REQUIREMENTS.md                 计划内容及其需求
│   ├── DEPENDENCIES.md THREAT_MODEL.md          与其他仓库的关系和威胁模型
│   ├── DECISIONS.md STATUS.md                   适用的决策和如实的状态
│   ├── NOT_YET.md RULES.md                      留待以后的想法和编写代码的规则
│   ├── ERROR_CODES.md                           诊断代码 PHOENIX-<领域>-<NNN>
│   ├── EXPRESSIONS.md                           参数公式所用的语言
│   ├── PROJECT_DOMAIN.md                        项目文件中的外壳配方
│   ├── SKETCH.md                                2D 草图：约束、求解器和文件
│   └── adr/                                     已接受的决策记录
├── Cargo.toml Cargo.lock                        Rust 工作区及其锁定的依赖
├── crates/                                      机械模型：外壳、操作、脚本、装配检查、窗口
├── AGENTS.md                                    修改仓库的人需遵守的规则
├── tools/                                       仓库检查、其测试以及图片工具
├── images/                                      横幅图以及架构和仓库结构图片
├── .github/                                     CI 与仪表板工作流、Issue 和 PR 模板
├── eh3d.project.json                            清单：版本、角色、成熟度和构建命令
├── VERSION                                      仓库的版本
├── CHANGELOG.md                                 每个版本的变更
├── SECURITY.md SUPPORT.md                       如何报告漏洞及在哪里提问
└── CONTRIBUTING.md CODE_OF_CONDUCT.md LICENSE   参与贡献、行为准则和许可证
```

</details>

## 🗺️ 路线图与已知限制

**目前已知的限制**

* 早期代码：只有外壳的模型，仅在它自己的测试生成的模型上试过；没有在机器上运行过任何东西，也没有经过验证的需求。

**下一步**

* 与电路板工具一起编写项目格式的机械领域，再编写由它而来的修改操作；之后是 STEP 导出、散热器和命令行，各自带测试。

## 🌍 语言

README 和文本提供英语、西班牙语、法语、意大利语、德语、汉语、日语和欧洲葡萄牙语版本。

## 🔗 相关项目

* [EH3D-DOCS](https://github.com/JuanenRac/EH3D-DOCS)：套件地图、架构、约定和共享检查
* [EH3D-COMMON](https://github.com/JuanenRac/EH3D-COMMON)：孔所使用的对象标识（UUID v7）；以后还有项目容器和诊断 crate
* 计划中的关系，尚未构建（EH3D-PHOENIX-PCB, EH3D-THERMAL-LAB）：见 [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md)

## 📚 文档

* [套件的约定](https://github.com/JuanenRac/EH3D-DOCS/blob/main/docs/CONVENTIONS.md)
* [变更日志](CHANGELOG.md)
* [范围、需求、依赖、威胁模型和决策](docs/SCOPE.md)

## 🤝 参与贡献与社区

* 提交 pull request 之前运行 `python -I tools/ci_validate.py` 和基础部分的测试：必须全部通过。见 [CONTRIBUTING.md](CONTRIBUTING.md) 和[行为准则](CODE_OF_CONDUCT.md)。
* README 有八种语言：修改文字时要同步修改全部八个文件，两张图片由 `tools/make_architecture.py` 和 `tools/make_structure.py` 根据 `docs/architecture.json` 和 `docs/structure.json` 绘制。
* 请私下报告漏洞：[SECURITY.md](SECURITY.md)。问题、想法和报告：electrohobby3d@gmail.com（[SUPPORT.md](SUPPORT.md)）。

## 👤 作者

**JuanenRac (Electro Hobby 3D)** · electrohobby3d@gmail.com

## 📜 许可证

软件：GPL-3.0-or-later。见 [LICENSE](LICENSE)。套件的硬件设计使用 CERN OHL-S。
