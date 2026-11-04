<p align="center">
  <img src="images/EH3D_PHOENIX_CAD_BANNER.svg" alt="EH3D-PHOENIX-CAD banner" width="100%">
</p>

# <img src="images/brand/icon-64.png" alt="" width="40" height="40" align="absmiddle"> EH3D-PHOENIX-CAD
<p align="center"><a href="README.md">🇺🇸 English</a> | <a href="README_spa.md">🇪🇸 Español</a> | <a href="README_fra.md">🇫🇷 Français</a> | 🇮🇹 <b>Italiano</b> | <a href="README_deu.md">🇩🇪 Deutsch</a> | <a href="README_zho.md">🇨🇳 中文</a> | <a href="README_jpn.md">🇯🇵 日本語</a> | <a href="README_por.md">🇵🇹 Português</a></p>

### EH3D-PHOENIX-CAD: pianificato come un CAD meccanico parametrico incentrato sulla progettazione di scatole, telai e dissipatori termici. Esiste un primo modello di contenitori come libreria; nulla è stato eseguito su una macchina.

<p align="center">
  <img src="https://img.shields.io/badge/License-GPL%203.0--or--later-blue.svg" alt="GPL 3.0 or later">
  <img src="https://img.shields.io/badge/Stack-Markdown%20%2B%20Python-blue.svg" alt="Markdown and Python">
  <img src="https://img.shields.io/badge/Languages-8-00E5FF.svg" alt="Languages">
  <img src="https://img.shields.io/badge/Tests-72-2EA043.svg" alt="Tests">
  <img src="https://img.shields.io/badge/Version-0.0.2-0f6cbd.svg" alt="Version">
  <img src="https://img.shields.io/badge/Maturity-scaffolding-FFB020.svg" alt="Maturity">
</p>

---

**Verifica di onestà - cosa funziona oggi:** una libreria Rust senza finestra con un contenitore parametrico: lunghezze in nanometri interi con un proprio intervallo, fori rotondi nelle facce, operazioni di modifica strutturate che segnalano i conflitti e lasciano il modello intatto, uno script in stile OpenSCAD e un controllo di alloggiamento rispetto all'ingombro di una scheda. Ha 69 test propri e passano su un computer. È stata provata solo su modelli creati dai suoi stessi test; nulla è stato disegnato o eseguito su una macchina, e il file STEP che scrive è stato controllato solo nella struttura, e dissipatori, schizzi e assiemi sono solo pianificati.

---

> **Stato di sicurezza funzionale e informatica:** questo repository è ai primi passi e non è un sistema di sicurezza funzionale. Nulla di quanto scritto qui sostituisce i limiti, gli interblocchi e l'arresto di emergenza della macchina e del suo controllore.

## 📋 In breve

| | |
|---|---|
| Versione | `0.0.2` |
| Maturità | Impalcatura |
| Test | 72 |
| Stack tecnologico | Markdown, Python 3.10+ |
| Su hardware reale | Non ancora eseguito su hardware |

**Vai a:** [Panoramica](#-panoramica) · [Architettura](#-architettura) · [Avvio rapido](#-avvio-rapido) · [Test](#-test) · [Struttura del repository](#-struttura-del-repository) · [Progetti correlati](#-progetti-correlati)

## 🚦 Stato del progetto

Ogni riga è una parte di questo repository e il semaforo dice a che punto è oggi. È scritta in base a ciò che il repository può mostrare (test, compilazioni, esecuzioni), non a ciò che è previsto.

🟢 Fatto e verificato · 🟡 Funziona, con limiti noti · 🟠 Fatto a metà o non ancora verificato · 🔴 Non fatto o non funzionante

| Area | Stato | Cosa significa qui |
|---|---|---|
| Controlli del repository | 🟡 | Il controllo del repository, quello delle immagini e i 3 test della base girano nella CI e passano. |
| Test | 🟡 | I 69 test della libreria Rust e i 3 della base passano su un computer. Usano solo modelli creati dai test stessi. |
| Hardware reale | 🟠 | Non eseguito su nessuna macchina né scheda: nulla qui è stato provato su hardware. |
| Funzione pianificata | 🟠 | Fatta in parte: esistono un contenitore parametrico con fori, uno script in stile OpenSCAD e un controllo di alloggiamento, più un primo scrittore STEP controllato solo nella struttura; i dissipatori, gli schizzi e gli assiemi non sono iniziati. |
| Requisiti | 🟠 | 4 requisiti su 6 sono in prototipo (il contenitore, lo script, le operazioni di modifica e l'intervallo meccanico), 2 sono solo pianificati e nessuno è validato. |

## 🎯 Panoramica

* **Pianificato:** EH3D-PHOENIX-CAD vuole essere un CAD meccanico parametrico incentrato sulla progettazione di scatole, telai e dissipatori termici, come libreria senza finestra, con interfaccia a riga di comando e, quando serve, un tipo di attività del server. La libreria esiste con un primo contenitore parametrico; la riga di comando, l'attività del server, l'esportazione STEP e i dissipatori non esistono ancora.

## 🧩 Ruolo in EH3D

EH3D-PHOENIX-CAD fa parte della famiglia PHOENIX della EH3D Fabrication Suite. Cosa farà, cosa non farà ancora, i suoi requisiti e le sue relazioni con altri repository sono scritti nella cartella `docs/`.

## 🏛️ Architettura

<p align="center">
  <img src="images/architecture.svg" alt="Architettura di EH3D-PHOENIX-CAD: chi lo usa, che cosa contiene e da che cosa dipende" width="100%">
</p>

Come si incastrano le parti di questo repository: chi lo usa, che cosa contiene e da che cosa dipende. L'immagine è disegnata da `docs/architecture.json` e verificata rispetto al codice.

## 🚀 Avvio rapido

Serve Python 3.10 o successivo; non c'è nulla da installare.

```bash
python -I tools/ci_validate.py
python -I tools/make_architecture.py --check
python -I tools/make_structure.py --check --readmes
python -I -m unittest discover -s tools -p "test_*.py"
```

## 🧪 Test

La libreria Rust ha 69 test (`cargo test`) e la base ne ha 3 (`python -I -m unittest discover -s tools -p "test_*.py"`); tutti passano su un computer. La CI esegue il controllo del repository, quello delle immagini, i test della base e, per il codice Rust, il controllo del formato, clippy con gli avvisi come errori e i test su Linux e Windows.

## 🏗️ Compilazione e rilascio

La versione sta nel file `VERSION` e in `eh3d.project.json`, e ogni versione ha la sua voce in [CHANGELOG.md](CHANGELOG.md). Non c'è nulla da compilare; le versioni vengono pubblicate su GitHub.

## 📂 Struttura del repository

<p align="center">
  <img src="images/repository-structure.svg" alt="Struttura del repository EH3D-PHOENIX-CAD" width="100%">
</p>

<details>
<summary>Versione testuale dell'albero</summary>

```text
EH3D-PHOENIX-CAD/
├── docs/
│   ├── architecture.json structure.json         i dati delle due immagini
│   ├── SCOPE.md REQUIREMENTS.md                 ciò che è pianificato e i suoi requisiti
│   ├── DEPENDENCIES.md THREAT_MODEL.md          relazioni con altri repository e modello delle minacce
│   ├── DECISIONS.md STATUS.md                   decisioni applicabili qui e stato onesto
│   ├── NOT_YET.md RULES.md                      idee per dopo e regole per scrivere il codice
│   ├── ERROR_CODES.md                           i codici di diagnostica PHOENIX-<AREA>-<NNN>
│   └── adr/                                     i registri delle decisioni accettate
├── Cargo.toml Cargo.lock                        il workspace Rust e le sue dipendenze bloccate
├── crates/                                      il modello meccanico: contenitore, operazioni, script, controllo di alloggiamento
├── AGENTS.md                                    regole per chi modifica il repository
├── tools/                                       il controllo del repository, i suoi test e gli strumenti delle immagini
├── images/                                      banner e le immagini di Architettura e Struttura del repository
├── .github/                                     workflow di CI e della dashboard, modelli di issue e pull request
├── eh3d.project.json                            manifesto: versione, ruolo, maturità e comando di compilazione
├── VERSION                                      la versione del repository
├── CHANGELOG.md                                 cosa è cambiato in ogni versione
├── SECURITY.md SUPPORT.md                       come segnalare una vulnerabilità e dove chiedere
└── CONTRIBUTING.md CODE_OF_CONDUCT.md LICENSE   come contribuire, codice di condotta e licenza
```

</details>

## 🗺️ Roadmap e limiti noti

**Limiti noti oggi**

* Codice iniziale: esiste solo il modello di un contenitore, provato su modelli creati dai suoi stessi test; nulla è stato eseguito su una macchina e nessun requisito è validato.

**Prossimi passi**

* Scrivere il dominio meccanico del formato di progetto insieme allo strumento per schede, poi le operazioni di modifica che ne derivano; dopo l'esportazione STEP, i dissipatori e la riga di comando, ciascuno con i suoi test.

## 🌍 Lingue

Il README e i testi esistono in inglese, spagnolo, francese, italiano, tedesco, cinese, giapponese e portoghese europeo.

## 🔗 Progetti correlati

* [EH3D-DOCS](https://github.com/JuanenRac/EH3D-DOCS): la mappa della suite, la sua architettura, le convenzioni e i controlli condivisi
* [EH3D-COMMON](https://github.com/JuanenRac/EH3D-COMMON): l'identità di oggetto (UUID v7) che usano i fori; più avanti il contenitore di progetto e il crate di diagnostica
* Relazioni pianificate, non ancora costruite (EH3D-PHOENIX-PCB, EH3D-THERMAL-LAB): vedi [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md)

## 📚 Documentazione

* [Le convenzioni della suite](https://github.com/JuanenRac/EH3D-DOCS/blob/main/docs/CONVENTIONS.md)
* [Registro delle modifiche](CHANGELOG.md)
* [Ambito, requisiti, dipendenze, modello delle minacce e decisioni](docs/SCOPE.md)

## 🤝 Contribuire e comunità

* Prima di una pull request esegui `python -I tools/ci_validate.py` e i test della base: tutto deve passare. Vedi [CONTRIBUTING.md](CONTRIBUTING.md) e il [codice di condotta](CODE_OF_CONDUCT.md).
* Il README esiste in otto lingue: una modifica al testo si fa in tutti e otto i file, e le due immagini sono disegnate da `docs/architecture.json` e `docs/structure.json` con `tools/make_architecture.py` e `tools/make_structure.py`.
* Segnala una vulnerabilità in privato: [SECURITY.md](SECURITY.md). Domande, idee e segnalazioni: electrohobby3d@gmail.com ([SUPPORT.md](SUPPORT.md)).

## 👤 AUTORE

**JuanenRac (Electro Hobby 3D)** · electrohobby3d@gmail.com

## 📜 LICENZA

Software: GPL-3.0-or-later. Vedi [LICENSE](LICENSE). I progetti hardware della suite usano la CERN OHL-S.
