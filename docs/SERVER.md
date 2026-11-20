# The link to the EH3D server

EH3D-PHOENIX-CAD can keep its projects on the EH3D server and send it the reports of a project. It is a second place for the same files: the local files, the window and every command that works on a file are as they were, and with no address in the settings nothing is sent anywhere.

The code is `src/server.rs` (the store of the server, the transport and the settings), `src/server_cli.rs` (the commands) and `src/serverfake.rs` (a store in memory for the tests). The link is the client of the suite, `eh3d-serverclient` of EH3D-COMMON (the same keys, ranges, login, retries, token that cannot be printed and errors by code in eight languages as every other program of the suite).

## The settings

The section `[server]` of the settings file (`eh3d-phoenix-cad config` prints it with the defaults):

| Key | Range | Default | What it is |
|---|---|---|---|
| `server_address` | `host:port`, or empty | empty | where the server is; empty means no server |
| `server_token` | secret text | empty | the token of this program; it is never printed, logged or shown by `Debug` |
| `server_machine` | text | empty | the machine of the server this program works with (kept for the jobs of the server; the store does not use it) |
| `server_timeout_s` | 1 to 120 | 15 | how long a request may take |
| `server_retries` | 0 to 5 | 2 | how many more times a request that failed on the network is tried; a request that creates something is never sent twice |
| `server_mdns` | `on` or `off` | `off` | look for the server on the local network when the address is empty |
| `server_max_answer_mib` | 1 to 256 | 8 | the most MiB read from one answer |

A value out of its range is refused with its key (`server.server_retries`). The token travels in the clear over plain HTTP: a warning is printed when the address is not this computer or a private network; use a network you trust or a tunnel.

## What is kept on the server

Under the application `phoenix-cad` of the project store (`/api/v1/apps/phoenix-cad/projects`, see `PROJECT_STORE.md` of EH3D-SERVER):

* a project of the store is named like the project here (1 to 120 characters, no control character) and holds the file `project.phoenixproj`, which is the project file as it is, the file `project.json`, which is the same project as the station of the server reads it (see below), and the folder `reports/`;
* the server keeps every version of a file. A new version is written with `If-Match`, the tag of the version it is made from (read just before), so two people never overwrite each other: a change made in the meantime is refused with the text of `precondition_failed` in the language of the person, and the next save goes over it knowingly. The same bytes as the current version make no version;
* the client of the suite sends text, so a project is sent as text (the project file of this program is text); a file that holds other bytes is refused before anything is sent, and what comes back is checked to be a project file before it is given back;
* the transport of this program (`server::HttpTransport`) is the plain HTTP of the suite with the header `If-Match` and the type `application/octet-stream` for the bytes of a file.

## The commands

All of them take `--config FILE` and `--lang CODE` (English without it). Without a token in the settings, `--user NAME` logs in with the password in the variable `PHOENIX_CAD_SERVER_PASSWORD` (a password is never taken from the command line).

| Command | What it does |
|---|---|
| `server-status` | the name, version and state the server says of itself |
| `server-list [TEXT]` | the projects whose name has the text, in any letter case, the last changed first |
| `server-save PROJECT [--name NAME] [--comment TEXT]` | saves the project file as a new version of the file of the project NAME (the name of the file without `--name`) |
| `server-load NAME --out FILE [--overwrite]` | reads the project, checks it is a project file and writes it to FILE, never over a file that exists unless `--overwrite` is given |
| `server-report NAME --from FILE [--report NAME] [--comment TEXT]` | sends a text file (the output of `bom`, `partlist`, `holes`...) to `reports/` of the project |

## In the window

Three entries of the Reports menu belong to this chapter. **Project on the server** has the name the project gets (empty means the name of its file without the extension), a comment for the version, a button to **save on the server** (a new version of `project.phoenixproj`; nothing is written when it is the version already there), a button to **open from the server** (the project comes with no file, so saving asks for one, and the unsaved mark is off) and a text to look for with a button to **list the projects**. Saving on the server is one more place to keep the project, not a save: the file, the unsaved mark and the history of the window stay as they were. The address and the token are those of `[server]`; with no address the panel says so, and a refusal of the server (a token that is gone, a name that cannot be used, a project it does not have) is shown in the status bar in the language of the window. **Units of the project** shows the unit and the decimals the project keeps, with two texts (an empty one keeps what is shown now) and the buttons to keep them in the project or to follow the settings again; it is `project-units` of the command line. The **bill of materials** panel can add the bill of each assembly (the parts that mates join, and the parts and bodies that no mate holds) after the bill of all, as `bom --by-assembly` does. The logic is in `window::Window` (`server_save_with`, `server_open_with`, `server_list_with`, `apply_units_text`) and is tested without a screen, over the store in memory; the buttons have never been opened on a screen.

## The station `cad` of the server

EH3D-SERVER has a plugin `cad`: stations that read the *structure* of a project (its sketches, the features made from them or working on one another, and its parts) and count and check that. It has no geometry kernel and says `evidence: simulator` everywhere; nothing of it has run on a real machine, and it does not make a volume, a mass or a box. The jobs are made with `POST /api/v1/work/jobs` on the machine of `server_machine`, started with `POST /api/v1/work/jobs/{id}/start` and read with `GET /api/v1/work/jobs/{id}` until they end; the answer is in `view.result` of the job.

**The project as the station reads it** (`eh3d.cad.project/1`, `servercad::project_json`) is written next to the file of the program every time a project is saved on the server, as `project.json` of the same project of the store (the application is `phoenix-cad`). Only the steps made from a sketch are features: an extrusion (`extrude`), a pocket (`pocket`), a turned body (`revolve`) and a swept body (`sweep`, which the station does not know and says so with a warning). The holes, posts, lid, edgings, fins and patterns are parameters of the enclosure, not features of the contract. The sketches are the sketches of the project, a placed part is a part with no feature (the station warns about it) and the enclosure is a part named like the project that holds its extrusions and pockets. A suppressed step and the steps after the tip are not sent. The units are `mm`, or `in` for a project of inches or mils.

| Command | What it does |
|---|---|
| `server-check PROJECT [--name NAME] [--kind check\|report]` | makes the contract of a project file, asks the station for a `check` (or, with `--kind report`, a `report` that adds the counts of parts, sketches and features) and tells the verdict, one line for each thing it found and, for a report, the counts, in the language of `--lang` |
| `server-check --stored NAME [--kind ...]` | the same with the `project.json` that the last save of the project NAME wrote, read from the store |

The codes of the station are told in the eight languages: `bad_schema`, `unknown_units`, `missing_id`, `duplicate_id`, `missing_ref`, `unknown_kind` and `empty_part`; a code this program does not know keeps the sentence the station gave. The check can also be made here before anything is sent: `servercad::check_json` follows the rules of the station and the store in memory answers a job with the same document (`servercad::station_answer`). A station that is not set (`server_machine` empty), that ends its job `failed` or that does not answer within `server_timeout_s` is told in the language of the person.

In the window, the **Project on the server** panel has **Check on the station** and **Report from the station** (`Window::server_station_with`), which send the project as it is now, saved or not, and show the answer; below them is the panel of the settings of the server that every program of the suite shows (`eh3d-view`, only in the build with the window): the address, the token, the login and the logout, and the state of the link. The buttons and that panel have never been opened on a screen.

**What the shared client does not do yet.** The request of `eh3d-serverclient` (EH3D-COMMON 0.2.5) has no field for headers and its body is text, so the header `If-Match` and the type `application/octet-stream` of a file still go through the plain HTTP transport of this program (`server::HttpTransport`). When the client takes headers and bytes, that transport goes away; until then this stays here.
