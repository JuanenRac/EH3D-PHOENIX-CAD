<p align="center">
  <img src="images/EH3D_PHOENIX_CAD_BANNER.svg" alt="EH3D-PHOENIX-CAD banner" width="100%">
</p>

# <img src="images/brand/icon-64.png" alt="" width="40" height="40" align="absmiddle"> EH3D-PHOENIX-CAD
<p align="center"><a href="README.md">🇺🇸 English</a> | <a href="README_spa.md">🇪🇸 Español</a> | <a href="README_fra.md">🇫🇷 Français</a> | 🇮🇹 <b>Italiano</b> | <a href="README_deu.md">🇩🇪 Deutsch</a> | <a href="README_zho.md">🇨🇳 中文</a> | <a href="README_jpn.md">🇯🇵 日本語</a> | <a href="README_por.md">🇵🇹 Português</a></p>

### EH3D-PHOENIX-CAD: pianificato come un CAD meccanico parametrico incentrato sulla progettazione di scatole, telai e dissipatori termici. Esiste un primo modello di contenitori come libreria con una finestra che non è mai stata aperta su uno schermo; nulla è stato eseguito su una macchina.

<p align="center">
  <img src="https://img.shields.io/badge/License-GPL%203.0--or--later-blue.svg" alt="GPL 3.0 or later">
  <img src="https://img.shields.io/badge/Stack-Rust%20%2B%20Markdown%20%2B%20Python-blue.svg" alt="Rust, Markdown and Python">
  <img src="https://img.shields.io/badge/Languages-8-00E5FF.svg" alt="Languages">
  <img src="https://img.shields.io/badge/Tests-709-2EA043.svg" alt="Tests">
  <img src="https://img.shields.io/badge/Version-0.0.6-0f6cbd.svg" alt="Version">
  <img src="https://img.shields.io/badge/Maturity-scaffolding-FFB020.svg" alt="Maturity">
</p>

---

**Verifica di onestà - cosa funziona oggi:** una libreria Rust con un contenitore parametrico: lunghezze in nanometri interi con un proprio intervallo, fori nelle facce (rotondi, svasati, con spianatura per la testa, rettangolari e ovali), spigoli arrotondati e smussati, parametri scritti come formule con unità su variabili, costanti e altri parametri (frazioni esatte, overflow controllato), operazioni di modifica strutturate che segnalano i conflitti e lasciano il modello intatto, colonnine sotto i fori di fissaggio di una scheda con un coperchio avvitato su di esse, la ricetta del contenitore conservata nel dominio meccanico del file di progetto, uno script in stile OpenSCAD e un controllo di alloggiamento rispetto all'ingombro di una scheda. Una finestra (egui, con `--features gui`) mostra l'albero del modello, le proprietà di ogni elemento, una vista 3D e una vista dall'alto (dove si trascinano fori e colonnine con il mouse e si misurano distanze e angoli; lo script, il file STEP e il rapporto di adattamento si scrivono in un file da un menu, mai sopra un file esistente a meno che la casella apposita sia spuntata), e un comando `check` legge un progetto e stampa le sue misure, l'alloggiamento e lo stato di ogni passaggio; la logica della finestra è provata senza schermo. Ha 706 test propri e passano su un computer. È stata provata solo su modelli creati dai suoi stessi test; la finestra non è mai stata aperta su uno schermo e nulla è stato eseguito su una macchina, e il file STEP che scrive è stato controllato solo nella struttura, e lo schizzo 2D (geometria, vincoli, un risolutore e una finestra per disegnarlo e quotarlo), che è conservato nel file di progetto e i cui contorni chiusi possono essere estrusi da una faccia o scavati in essa come passi della ricetta, è stato provato solo su schizzi creati dai suoi stessi test e la sua finestra non è ancora stata aperta su uno schermo, mentre gli schemi di fori e colonnine e le alette di raffreddamento (con una stima semplice del calore che cedono all'aria ferma, confrontata con nessuna misura) esistono ma sono stati provati solo su modelli creati dai suoi stessi test, e lo stesso vale per le parti di un catalogo (profili tagliati a una lunghezza, viti e staffe lette da file di libreria e collocate nel modello, con una distinta base, una distinta di taglio, il volume e la massa secondo la densità di ciascun materiale; la libreria degli esempi è inventata), e lo stesso vale per gli assiemi semplici di queste parti (una parte collocata contro un'altra lungo un asse con una distanza, un allineamento o un impilamento, e una verifica di quali parti condividono volume con le forme esatte di profili, staffe e viti, con una vite ammessa attraverso il foro di una staffa; i due assiemi di esempio sono inventati). Il solido del modello può essere scritto come file STL (binario o di testo, con la superficie chiusa verificata rispetto al volume esatto; alcune forme vengono rifiutate) e la pianta, il prospetto e il profilo come disegni DXF e SVG, dal menu Esporta o con il comando `export`; nessuno di questi file è stato aperto in uno slicer, in un programma CAD o di disegno.

---

> **Stato di sicurezza funzionale e informatica:** questo repository è ai primi passi e non è un sistema di sicurezza funzionale. Nulla di quanto scritto qui sostituisce i limiti, gli interblocchi e l'arresto di emergenza della macchina e del suo controllore.

## 📋 In breve

| | |
|---|---|
| Versione | `0.0.6` |
| Maturità | Impalcatura |
| Test | 709 |
| Stack tecnologico | Rust, Markdown, Python 3.10+ |
| Su hardware reale | Non ancora eseguito su hardware |

**Vai a:** [Panoramica](#-panoramica) · [Architettura](#-architettura) · [Avvio rapido](#-avvio-rapido) · [Test](#-test) · [Struttura del repository](#-struttura-del-repository) · [Progetti correlati](#-progetti-correlati)

## 🚦 Stato del progetto

Ogni riga è una parte di questo repository e il semaforo dice a che punto è oggi. È scritta in base a ciò che il repository può mostrare (test, compilazioni, esecuzioni), non a ciò che è previsto.

🟢 Fatto e verificato · 🟡 Funziona, con limiti noti · 🟠 Fatto a metà o non ancora verificato · 🔴 Non fatto o non funzionante

| Area | Stato | Cosa significa qui |
|---|---|---|
| Controlli del repository | 🟡 | Il controllo del repository, quello delle immagini e i 3 test della base girano nella CI e passano. |
| Test | 🟡 | I 706 test della libreria Rust e i 3 della base passano su un computer. Usano solo modelli creati dai test stessi. |
| Hardware reale | 🟠 | Non eseguito su nessuna macchina né scheda: nulla qui è stato provato su hardware. |
| Funzione pianificata | 🟠 | Fatta in parte: esistono un contenitore parametrico con fori, colonnine e coperchio avvitato, uno script in stile OpenSCAD, un controllo di alloggiamento e la ricetta del contenitore nel file di progetto, più un primo scrittore STEP controllato solo nella struttura; esiste uno schizzo 2D con vincoli, un risolutore e una finestra per disegnarlo, conservato nel file di progetto e che può essere estruso da una faccia o scavato in essa, ma la sua finestra non è ancora stata aperta su uno schermo; esistono schemi di fori e colonnine e alette di raffreddamento con una stima del loro calore, provati solo su modelli creati dai test; esistono le parti di un catalogo (profili, viti e staffe da file di libreria) collocate nel modello con distinta base e massa di ciascun materiale, provate solo su parti inventate; esistono assiemi semplici di queste parti (una parte collocata contro un'altra lungo un asse e una verifica di quali parti condividono volume), provati solo su parti inventate. |
| Requisiti | 🟠 | 7 requisiti su 8 sono in prototipo (il contenitore, lo script, le operazioni di modifica, il modello del dissipatore, il catalogo di parti, gli assiemi e l'intervallo meccanico), 1 è solo pianificato e nessuno è validato. |

## 🎯 Panoramica

* **Pianificato:** EH3D-PHOENIX-CAD vuole essere un CAD meccanico parametrico incentrato sulla progettazione di scatole, telai e dissipatori termici, come libreria con finestra, con interfaccia a riga di comando e, quando serve, un tipo di attività del server. La libreria esiste con un primo contenitore parametrico, una finestra che mostra l'albero, le proprietà, una vista 3D e una vista dall'alto, dove si trascinano fori e colonnine con il mouse e si misurano distanze e angoli (mai aperta su uno schermo) e un comando `check`; l'attività del server non esiste ancora, le alette di raffreddamento (con una stima del loro calore) non sono nel file STEP, e il file STEP è stato controllato solo nella struttura.

## 🧩 Ruolo in EH3D

EH3D-PHOENIX-CAD fa parte della famiglia PHOENIX della EH3D Fabrication Suite. Cosa farà, cosa non farà ancora, i suoi requisiti e le sue relazioni con altri repository sono scritti nella cartella `docs/`.

<p align="center">
  <img src="images/screens/it/phoenix-cad-01-box.png" alt="EH3D-PHOENIX-CAD window" width="49%">
  <img src="images/screens/it/phoenix-cad-02-sketch.png" alt="EH3D-PHOENIX-CAD sketch" width="49%">
</p>

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

Per provare il programma (serve Rust; la finestra richiede `--features gui`):

```bash
cargo run -p eh3d-phoenix-cad -- check crates/eh3d-phoenix-cad/examples/box.phoenixproj
cargo run -p eh3d-phoenix-cad -- check crates/eh3d-phoenix-cad/examples/plate.phoenixproj
cargo run -p eh3d-phoenix-cad -- check crates/eh3d-phoenix-cad/examples/frame.phoenixproj
cargo run -p eh3d-phoenix-cad -- bill crates/eh3d-phoenix-cad/examples/frame.phoenixproj
cargo run -p eh3d-phoenix-cad -- library crates/eh3d-phoenix-cad/examples/chassis.phoenixlib
cargo run -p eh3d-phoenix-cad -- check crates/eh3d-phoenix-cad/examples/assembly.phoenixproj
cargo run -p eh3d-phoenix-cad -- interference crates/eh3d-phoenix-cad/examples/assembly.phoenixproj
cargo run -p eh3d-phoenix-cad -- export crates/eh3d-phoenix-cad/examples/box.phoenixproj --format stl --out box.stl
cargo run -p eh3d-phoenix-cad -- export crates/eh3d-phoenix-cad/examples/box.phoenixproj --format svg --view front --out box-front.svg
cargo run --release -p eh3d-phoenix-cad --features gui -- crates/eh3d-phoenix-cad/examples/box.phoenixproj
cargo run --release -p eh3d-phoenix-cad --features gui -- gui crates/eh3d-phoenix-cad/examples/box.phoenixproj --shot box.png
cargo run --release -p eh3d-phoenix-cad --features gui -- gui crates/eh3d-phoenix-cad/examples/box.phoenixsketch --view sketch --shot sketch.png
```

## 🧪 Test

La libreria Rust ha 706 test (`cargo test`) e la base ne ha 3 (`python -I -m unittest discover -s tools -p "test_*.py"`); tutti passano su un computer. La CI esegue il controllo del repository, quello delle immagini, i test della base e, per il codice Rust, il controllo del formato, clippy con gli avvisi come errori e i test su Linux e Windows, e controlla anche la finestra con clippy (`--features gui`).

## 🏗️ Compilazione e rilascio

La versione sta nel file `VERSION` e in `eh3d.project.json`, e ogni versione ha la sua voce in [CHANGELOG.md](CHANGELOG.md). Il programma si compila con `cargo build --release` (la finestra con `--features gui`); le versioni vengono pubblicate su GitHub.

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
│   ├── EXPRESSIONS.md                           il linguaggio delle formule dei parametri
│   ├── PROJECT_DOMAIN.md                        la ricetta del contenitore nel file di progetto
│   ├── SKETCH.md                                lo schizzo 2D: vincoli, risolutore, file di progetto ed estrusioni
│   ├── CATALOG.md                               il catalogo di parti: librerie, distinta base, distinta di taglio
│   ├── ASSEMBLY.md                              assiemi semplici: vincoli e interferenze
│   ├── EXPORT.md                                le esportazioni: solido STL, disegni DXF e SVG
│   └── adr/                                     i registri delle decisioni accettate
├── Cargo.toml Cargo.lock                        il workspace Rust e le sue dipendenze bloccate
├── crates/                                      modello meccanico: contenitore, schemi, alette, parti, assiemi, schizzi, script, finestra
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

* Codice iniziale: il modello di un contenitore con i suoi schizzi, estrusioni e tasche, provato su modelli creati dai suoi stessi test; nulla è stato eseguito su una macchina e nessun requisito è validato.

**Prossimi passi**

* Far leggere e scrivere il dominio meccanico del formato di progetto allo strumento per schede, aprire il file STEP in un programma CAD e poi estendere gli assiemi oltre i vincoli lungo un asse e confrontare la stima delle alette di raffreddamento con una misura, ciascuno con i suoi test.

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
