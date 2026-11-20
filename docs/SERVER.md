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

Under the application `cad` of the project store (`/api/v1/apps/cad/projects`, see `PROJECT_STORE.md` of EH3D-SERVER):

* a project of the store is named like the project here (1 to 120 characters, no control character) and holds the file `project.phoenixproj`, which is the project file as it is, and the folder `reports/`;
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

## The plugin `cad` of the server (a sheet, not built)

EH3D-SERVER has no plugin `cad` yet. This is the sheet of what the program would ask of it; the first version would be a simulator, like the other plugins, and nothing here has been tried against it.

* Machines: stations that make reports and checks over a project of the store and move nothing (`POST /work/machines`, `"plugin": "cad"`).
* Request kinds: `check` (`{"kind": "check", "project": "<id>"}`: build the box and answer the state of every step and the fit), `report` (`{"kind": "report", "project": "<id>", "report": "bom|partlist|cutplan|holes|balance"}`: write the report into `reports/` of the project) and `export` (`{"kind": "export", "project": "<id>", "format": "stl|step|dxf|svg"}`).
* Role operator, scope `jobs:prepare` and `jobs:start`; the files are written into the project with the rules of the store (tag, lock).
* Settings with a range and a conservative default: the most steps and triangles of a job, the time of the simulated station.

What this program needs from the server today is only the store; the plugin is for running the same checks on the machine next to the server.
