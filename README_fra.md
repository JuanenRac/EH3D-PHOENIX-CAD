<p align="center">
  <img src="images/EH3D_PHOENIX_CAD_BANNER.svg" alt="EH3D-PHOENIX-CAD banner" width="100%">
</p>

# <img src="images/brand/icon-64.png" alt="" width="40" height="40" align="absmiddle"> EH3D-PHOENIX-CAD
<p align="center"><a href="README.md">🇺🇸 English</a> | <a href="README_spa.md">🇪🇸 Español</a> | 🇫🇷 <b>Français</b> | <a href="README_ita.md">🇮🇹 Italiano</a> | <a href="README_deu.md">🇩🇪 Deutsch</a> | <a href="README_zho.md">🇨🇳 中文</a> | <a href="README_jpn.md">🇯🇵 日本語</a> | <a href="README_por.md">🇵🇹 Português</a></p>

### EH3D-PHOENIX-CAD : prévu comme un CAD mécanique paramétrique centré sur la conception de boîtiers, de châssis et de dissipateurs thermiques. Un premier modèle de boîtiers existe sous forme de bibliothèque ; rien n'a été exécuté sur une machine.

<p align="center">
  <img src="https://img.shields.io/badge/License-GPL%203.0--or--later-blue.svg" alt="GPL 3.0 or later">
  <img src="https://img.shields.io/badge/Stack-Markdown%20%2B%20Python-blue.svg" alt="Markdown and Python">
  <img src="https://img.shields.io/badge/Languages-8-00E5FF.svg" alt="Languages">
  <img src="https://img.shields.io/badge/Tests-79-2EA043.svg" alt="Tests">
  <img src="https://img.shields.io/badge/Version-0.0.2-0f6cbd.svg" alt="Version">
  <img src="https://img.shields.io/badge/Maturity-scaffolding-FFB020.svg" alt="Maturity">
</p>

---

**Vérification d'honnêteté - ce qui tourne aujourd'hui :** une bibliothèque Rust sans fenêtre avec un boîtier paramétrique : longueurs en nanomètres entiers avec leur propre plage, trous ronds dans les faces, opérations de changement structurées qui signalent les conflits et laissent le modèle intact, un script de style OpenSCAD et un contrôle d'ajustement contre l'enveloppe d'une carte. Elle a 76 tests à elle et ils passent sur un ordinateur. Elle n'a été essayée que sur des modèles faits par ses propres tests ; rien n'a été dessiné ni exécuté sur une machine, et le fichier STEP qu'elle écrit n'a été vérifié que dans sa structure, et les dissipateurs, les esquisses et les assemblages sont seulement prévus.

---

> **État de la sûreté et de la sécurité :** ce dépôt en est à ses premiers pas et n'est pas un système de sûreté. Rien de ce qui est écrit ici ne remplace les limites, les verrouillages et l'arrêt d'urgence de la machine et de son contrôleur.

## 📋 En un coup d'œil

| | |
|---|---|
| Version | `0.0.2` |
| Maturité | Échafaudage |
| Tests | 79 |
| Pile technique | Markdown, Python 3.10+ |
| Sur du matériel réel | Pas encore exécuté sur du matériel |

**Aller à  :** [Présentation](#-présentation) · [Architecture](#-architecture) · [Démarrage rapide](#-démarrage-rapide) · [Tests](#-tests) · [Structure du dépôt](#-structure-du-dépôt) · [Projets liés](#-projets-liés)

## 🚦 État du projet

Chaque ligne est une partie de ce dépôt et le feu dit où elle en est aujourd'hui. Elle est écrite d'après ce que le dépôt peut montrer (tests, compilations, exécutions), pas d'après ce qui est prévu.

🟢 Fait et vérifié · 🟡 Fonctionne, avec des limites connues · 🟠 À moitié fait ou pas encore vérifié · 🔴 Pas fait ou ne fonctionne pas

| Domaine | État | Ce que cela veut dire ici |
|---|---|---|
| Contrôles du dépôt | 🟡 | Le contrôle du dépôt, celui des images et les 3 tests de la base tournent dans la CI et réussissent. |
| Tests | 🟡 | Les 76 tests de la bibliothèque Rust et les 3 de la base passent sur un ordinateur. Ils n'utilisent que des modèles faits par les tests eux-mêmes. |
| Matériel réel | 🟠 | Non exécuté sur une machine ni une carte : rien ici n'a été essayé sur du matériel. |
| Fonction prévue | 🟠 | En partie faite : un boîtier paramétrique avec trous, un script de style OpenSCAD et un contrôle d'ajustement existent, ainsi qu'un premier écrivain STEP vérifié seulement dans sa structure ; les dissipateurs, les esquisses et les assemblages ne sont pas commencés. |
| Exigences | 🟠 | 4 exigences sur 6 sont au stade de prototype (le boîtier, le script, les opérations de changement et la plage mécanique), 2 sont seulement prévues et aucune n'est validée. |

## 🎯 Présentation

* **Prévu :** EH3D-PHOENIX-CAD veut être une CAO mécanique paramétrique centrée sur la conception de boîtiers, de châssis et de dissipateurs, comme bibliothèque sans fenêtre, avec une ligne de commande et, quand il y a lieu, un type de tâche du serveur. La bibliothèque existe avec un premier boîtier paramétrique ; la ligne de commande, la tâche du serveur, l'export STEP et les dissipateurs n'existent pas encore.

## 🧩 Rôle dans EH3D

EH3D-PHOENIX-CAD fait partie de la famille PHOENIX de l'EH3D Fabrication Suite. Ce qu'il fera, ce qu'il ne fera pas encore, ses exigences et ses relations avec d'autres dépôts sont décrits dans le dossier `docs/`.

## 🏛️ Architecture

<p align="center">
  <img src="images/architecture.svg" alt="Architecture de EH3D-PHOENIX-CAD : qui l'utilise, ce qu'il contient et de quoi il dépend" width="100%">
</p>

Comment les parties de ce dépôt s'articulent : qui l'utilise, ce qu'il contient et de quoi il dépend. L'image est dessinée à partir de `docs/architecture.json` et vérifiée par rapport au code.

## 🚀 Démarrage rapide

Il faut Python 3.10 ou plus récent ; il n'y a rien à installer.

```bash
python -I tools/ci_validate.py
python -I tools/make_architecture.py --check
python -I tools/make_structure.py --check --readmes
python -I -m unittest discover -s tools -p "test_*.py"
```

## 🧪 Tests

La bibliothèque Rust a 76 tests (`cargo test`) et la base en a 3 (`python -I -m unittest discover -s tools -p "test_*.py"`) ; tous réussissent sur un ordinateur. La CI exécute le contrôle du dépôt, celui des images, les tests de la base et, pour le code Rust, la vérification du format, clippy avec les avertissements comme erreurs et les tests sous Linux et Windows.

## 🏗️ Compilation et publication

La version se trouve dans le fichier `VERSION` et dans `eh3d.project.json`, et chaque version a son entrée dans [CHANGELOG.md](CHANGELOG.md). Il n'y a rien à compiler ; les versions sont publiées sur GitHub.

## 📂 Structure du dépôt

<p align="center">
  <img src="images/repository-structure.svg" alt="Structure du dépôt EH3D-PHOENIX-CAD" width="100%">
</p>

<details>
<summary>Version texte de l'arborescence</summary>

```text
EH3D-PHOENIX-CAD/
├── docs/
│   ├── architecture.json structure.json         les données des deux images
│   ├── SCOPE.md REQUIREMENTS.md                 ce qui est prévu et ses exigences
│   ├── DEPENDENCIES.md THREAT_MODEL.md          relations avec d'autres dépôts et modèle de menaces
│   ├── DECISIONS.md STATUS.md                   décisions applicables ici et état honnête
│   ├── NOT_YET.md RULES.md                      idées pour plus tard et règles d'écriture du code
│   ├── ERROR_CODES.md                           les codes de diagnostic PHOENIX-<ZONE>-<NNN>
│   └── adr/                                     les décisions acceptées
├── Cargo.toml Cargo.lock                        l'espace de travail Rust et ses dépendances verrouillées
├── crates/                                      le modèle mécanique : boîtier, opérations, script, contrôle d'ajustement
├── AGENTS.md                                    règles pour quiconque modifie le dépôt
├── tools/                                       le contrôle du dépôt, ses tests et les outils des images
├── images/                                      bannière et images d'Architecture et de Structure du dépôt
├── .github/                                     workflows de CI et du tableau de bord, modèles d'issues et de pull requests
├── eh3d.project.json                            manifeste : version, rôle, maturité et commande de compilation
├── VERSION                                      la version du dépôt
├── CHANGELOG.md                                 ce qui a changé dans chaque version
├── SECURITY.md SUPPORT.md                       signaler une vulnérabilité et poser des questions
└── CONTRIBUTING.md CODE_OF_CONDUCT.md LICENSE   contribuer, code de conduite et licence
```

</details>

## 🗺️ Feuille de route et limites connues

**Limites connues aujourd'hui**

* Code de départ : seul le modèle d'un boîtier existe, essayé sur des modèles faits par ses propres tests ; rien n'a été exécuté sur une machine et aucune exigence n'est validée.

**Ensuite**

* Écrire le domaine mécanique du format de projet avec l'outil de cartes, puis les opérations de changement qui en viennent ; ensuite l'export STEP, les dissipateurs et la ligne de commande, chacun avec ses tests.

## 🌍 Langues

Le README et les textes existent en anglais, espagnol, français, italien, allemand, chinois, japonais et portugais européen.

## 🔗 Projets liés

* [EH3D-DOCS](https://github.com/JuanenRac/EH3D-DOCS) : la carte de la suite, son architecture, les conventions et les contrôles partagés
* [EH3D-COMMON](https://github.com/JuanenRac/EH3D-COMMON) : l'identité d'objet (UUID v7) qu'utilisent les trous ; plus tard le conteneur de projet et le crate de diagnostic
* Relations prévues, pas encore construites (EH3D-PHOENIX-PCB, EH3D-THERMAL-LAB) : voir [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md)

## 📚 Documentation

* [Les conventions de la suite](https://github.com/JuanenRac/EH3D-DOCS/blob/main/docs/CONVENTIONS.md)
* [Journal des modifications](CHANGELOG.md)
* [Périmètre, exigences, dépendances, modèle de menaces et décisions](docs/SCOPE.md)

## 🤝 Contribuer et communauté

* Avant une pull request, exécutez `python -I tools/ci_validate.py` et les tests de la base : tout doit réussir. Voir [CONTRIBUTING.md](CONTRIBUTING.md) et le [code de conduite](CODE_OF_CONDUCT.md).
* Le README existe en huit langues : une modification du texte se fait dans les huit fichiers, et les deux images sont dessinées à partir de `docs/architecture.json` et `docs/structure.json` par `tools/make_architecture.py` et `tools/make_structure.py`.
* Signalez une vulnérabilité en privé : [SECURITY.md](SECURITY.md). Questions, idées et rapports : electrohobby3d@gmail.com ([SUPPORT.md](SUPPORT.md)).

## 👤 AUTEUR

**JuanenRac (Electro Hobby 3D)** · electrohobby3d@gmail.com

## 📜 LICENCE

Logiciel : GPL-3.0-or-later. Voir [LICENSE](LICENSE). Les conceptions matérielles de la suite utilisent la CERN OHL-S.
