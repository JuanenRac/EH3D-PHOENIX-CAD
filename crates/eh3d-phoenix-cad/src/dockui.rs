// SPDX-License-Identifier: GPL-3.0-or-later
//! The panels, the workspaces and the menus of the window as data, with no window in them: which
//! panels there are and what each is called in the eight languages, the factory arrangement of each
//! workspace, the menu bar and which of its entries can be chosen at a given moment, how the
//! arrangement is kept in the settings file, and the rule that makes the mode of the window (the
//! model or the sketch) follow the panel that is on show. The window (`gui`) draws these with the
//! frame of the suite (`eh3d-dock`) and only does what they say.
//!
//! The panels of the sector of mechanical design are the ones the suite proposes (`view3d`,
//! `model_tree`, `properties`, `tasks`, `report`, `console`); this program adds the sheet of
//! variables, the four parts of the sketch (the canvas, its tools, its constraints and its
//! dimensions) and the drawing sheets (views, dimensions, balloons, layers and the bill). The workspaces are the three of the sector: `Model` (the tree and the tasks on the
//! left, the 3D view and the sketch as tabs in the middle, the console and the report below, the
//! properties, variables, constraints and dimensions on the right), `Review` (the view with the
//! report and the properties) and `Compact` (the view alone, the tree and the properties folded).

use crate::export::Format;
use crate::lang::{Lang, Msg};
use crate::panels::Panel;
use crate::shot::StartView;
use crate::window::format_msg;
use eh3d_dock::about::{AboutBrand, Phoenix};
use eh3d_dock::core::PanelDef;
use eh3d_dock::geom::Side;
use eh3d_dock::menu::{DockAction, Entry, Gen, Item, Label, Menu, MenuBar, StdAction};
use eh3d_dock::model::{Layout, Node};
use eh3d_dock::sectors::Sector;
use eh3d_dock::stdmenus::{edit_menu, file_menu, help_menu, view_menu, window_menu, HelpOptions};
use eh3d_dock::texts::Text8;
use eh3d_dock::workspaces::Workspace;

/// The look of the window «About»: which program of the family this is and its own picture, drawn in a ring of its colour.
pub fn about_brand() -> AboutBrand {
    AboutBrand {
        program: Phoenix::Cad,
        logo_png: include_bytes!("../../../images/brand/icon-256.png"),
    }
}

/// The names of the panels (what the settings file stores, so they never change).
pub mod id {
    /// The 3D view and the plan of the model.
    pub const VIEW3D: &str = "view3d";
    /// The tree of the model and the buttons that add and remove steps.
    pub const MODEL_TREE: &str = "model_tree";
    /// The properties of what is selected.
    pub const PROPERTIES: &str = "properties";
    /// The measurements and the reports.
    pub const TASKS: &str = "tasks";
    /// The text of the fit report.
    pub const REPORT: &str = "report";
    /// What the program has to say: the state, the sizes, the fit.
    pub const CONSOLE: &str = "console";
    /// The sheet of variables.
    pub const VARIABLES: &str = "variables";
    /// The canvas of the sketch.
    pub const SKETCH: &str = "sketch";
    /// The tools of the sketch.
    pub const SKETCH_TOOLS: &str = "sketch_tools";
    /// The constraints of the sketch.
    pub const CONSTRAINTS: &str = "constraints";
    /// The dimensions of the sketch.
    pub const DIMENSIONS: &str = "dimensions";
    /// The 2D drawing sheets: views, dimensions, balloons, layers and the bill.
    pub const DRAWING: &str = "drawing";
    /// The attachments of the project of the suite: data sheets, manuals and other files, with their viewer.
    pub const ATTACHMENTS: &str = eh3d_attachpanel::PANEL_ID;

    /// Every panel, in the order of the View menu.
    pub const ALL: [&str; 13] = [
        VIEW3D,
        MODEL_TREE,
        PROPERTIES,
        TASKS,
        REPORT,
        CONSOLE,
        VARIABLES,
        SKETCH,
        SKETCH_TOOLS,
        CONSTRAINTS,
        DIMENSIONS,
        DRAWING,
        ATTACHMENTS,
    ];
}

/// The workspace of the model.
pub const WS_MODEL: &str = "Model";
/// The workspace of the review.
pub const WS_REVIEW: &str = "Review";
/// The workspace of the small screen.
pub const WS_COMPACT: &str = "Compact";

/// The workspaces the window can be asked to open in, as the words of `--window`.
pub const WINDOW_WORDS: [&str; 3] = ["model", "review", "compact"];

/// The workspace a word of `--window` names (in any case), or `None`.
pub fn workspace_of_word(word: &str) -> Option<&'static str> {
    let w = word.trim().to_ascii_lowercase();
    WINDOW_WORDS
        .iter()
        .position(|x| *x == w)
        .map(|i| [WS_MODEL, WS_REVIEW, WS_COMPACT][i])
}

/// A message as the text of the frame: the eight languages in the order English, Spanish, Italian,
/// German, French, Portuguese (of Portugal), Chinese and Japanese.
pub fn t8(m: Msg) -> Text8 {
    Text8([
        m.tr(Lang::En),
        m.tr(Lang::Es),
        m.tr(Lang::It),
        m.tr(Lang::De),
        m.tr(Lang::Fr),
        m.tr(Lang::Pt),
        m.tr(Lang::Zh),
        m.tr(Lang::Ja),
    ])
}

/// The language of the frame for a language of the window.
pub fn view_lang(lang: Lang) -> eh3d_view::texts::Lang {
    eh3d_view::texts::Lang::ALL
        .into_iter()
        .find(|l| l.code() == lang.code())
        .unwrap_or(eh3d_view::texts::Lang::En)
}

/// The panels: the ones the suite proposes for mechanical design, the six that this program adds
/// (the sheet of variables, the four parts of the sketch and the drawing sheets) and the shared
/// panel of the attachments.
pub fn defs() -> Vec<PanelDef> {
    let mut v = Sector::Cad.panels();
    v.push(
        PanelDef::new(id::VARIABLES, t8(Msg::SheetTitle))
            .icon("V")
            .min(240.0, 160.0),
    );
    v.push(
        PanelDef::new(id::SKETCH, t8(Msg::WorkspaceSketch))
            .icon("S")
            .min(320.0, 240.0),
    );
    v.push(
        PanelDef::new(id::SKETCH_TOOLS, t8(Msg::ToolsTitle))
            .icon("Ts")
            .min(200.0, 80.0),
    );
    v.push(
        PanelDef::new(id::CONSTRAINTS, t8(Msg::ConstraintsTitle))
            .icon("C")
            .min(240.0, 160.0),
    );
    v.push(
        PanelDef::new(id::DIMENSIONS, t8(Msg::DimensionsTitle))
            .icon("D")
            .min(240.0, 160.0),
    );
    v.push(
        PanelDef::new(id::DRAWING, t8(Msg::PlTitle))
            .icon("Pl")
            .min(280.0, 200.0),
    );
    v.push(eh3d_attachpanel::panel_def());
    v
}

fn ws(name: &str, root: Node) -> Workspace {
    Workspace::new(name, Layout::with_root(root))
}

/// The factory workspaces: Model, Review and Compact.
pub fn workspaces() -> Vec<Workspace> {
    let t = |ids: &[&str]| Node::tabs(ids);
    vec![
        ws(
            WS_MODEL,
            Node::row(
                0.2,
                t(&[id::MODEL_TREE, id::SKETCH_TOOLS, id::TASKS]),
                Node::row(
                    0.76,
                    Node::column(
                        0.76,
                        t(&[id::VIEW3D, id::SKETCH]),
                        t(&[id::CONSOLE, id::REPORT]),
                    ),
                    t(&[
                        id::PROPERTIES,
                        id::VARIABLES,
                        id::CONSTRAINTS,
                        id::DIMENSIONS,
                        id::DRAWING,
                        id::ATTACHMENTS,
                    ]),
                ),
            ),
        ),
        ws(
            WS_REVIEW,
            Node::row(
                0.74,
                t(&[id::VIEW3D, id::DRAWING]),
                Node::column(
                    0.5,
                    t(&[id::PROPERTIES, id::VARIABLES, id::ATTACHMENTS]),
                    t(&[id::REPORT, id::CONSOLE]),
                ),
            ),
        ),
        Workspace::new(
            WS_COMPACT,
            Layout::with_root(t(&[id::VIEW3D, id::SKETCH]))
                .with_auto_hidden(id::MODEL_TREE, Side::Left)
                .with_auto_hidden(id::PROPERTIES, Side::Right),
        ),
    ]
}

/// The mode the window is in after a frame, from what was drawn in it: when only the 3D view was
/// on show the window is working on the model, when only the sketch was it is working on the
/// sketch, and when both or none were (they are side by side, or both hidden) it stays as it was.
pub fn mode_after_frame(current: StartView, drew_view3d: bool, drew_sketch: bool) -> StartView {
    match (drew_view3d, drew_sketch) {
        (true, false) => StartView::Model,
        (false, true) => StartView::Sketch,
        _ => current,
    }
}

/// What the program does when an entry of its menus is chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    /// An action every program has (New, Open, Save, Quit...).
    Std(StdAction),
    /// Open a library of parts.
    OpenLibrary,
    /// Open a DXF drawing as a sketch.
    ImportDxf,
    /// Open a project file of the suite (`.eh3d`).
    SuiteOpen,
    /// Save into the project file of the suite.
    SuiteSave,
    /// Save the project as a file of the suite with another name.
    SuiteSaveAs,
    /// Keep the original of a file of another program in the suite file.
    SuiteImport,
    /// Show the branches of the suite file.
    SuiteBranches,
    /// Edit the title and the description of the project of the suite.
    SuiteTexts,
    /// Sign the project of the suite.
    SuiteSign,
    /// Look at a 3D file or a flat drawing of another program.
    ViewFile,
    /// Take the board of the branch `pcb` into the box.
    SuiteBoard,
    /// Check the box against the board and the room of its parts.
    SuiteCheck,
    /// Cut the openings of the connectors.
    SuiteOpenings,
    /// Hand the box back to the other programs.
    SuiteHandBack,
    /// Check the sketch.
    CheckSketch,
    /// Open the window of the variants of the project.
    Variants,
    /// Write the model in a format.
    Export(Format),
    /// Open the window with the fit report.
    FitReport,
    /// Work on the model.
    EditModel,
    /// Work on the sketch.
    EditSketch,
    /// Keep the sketch in the project.
    KeepSketch,
    /// Fit the views to what is drawn.
    FitView,
    /// Show the panel of the drawing sheets.
    ShowDrawings,
    /// Open one of the panels of the reports.
    Report(Panel),
    /// Change the language of the window.
    Language(Lang),
}

impl From<StdAction> for Cmd {
    fn from(a: StdAction) -> Cmd {
        Cmd::Std(a)
    }
}

fn own(id: &str, m: Msg, cmd: Cmd) -> Entry<Cmd> {
    Entry::app(id, Label::Own(t8(m)), cmd)
}

/// The entries of the submenu of the project file of the suite, in order: their ids, their words
/// and what they do.
pub const SUITE_ENTRIES: [(&str, Msg, Cmd); 11] = [
    ("file.suite.open", Msg::SuiteMenuOpen, Cmd::SuiteOpen),
    ("file.suite.save", Msg::SuiteMenuSave, Cmd::SuiteSave),
    ("file.suite.save_as", Msg::SuiteMenuSaveAs, Cmd::SuiteSaveAs),
    ("file.suite.import", Msg::SuiteMenuImport, Cmd::SuiteImport),
    (
        "file.suite.branches",
        Msg::SuiteMenuBranches,
        Cmd::SuiteBranches,
    ),
    ("file.suite.texts", Msg::SuiteTextsMenu, Cmd::SuiteTexts),
    ("file.suite.sign", Msg::SignMenu, Cmd::SuiteSign),
    ("file.suite.board", Msg::SuiteMenuBoard, Cmd::SuiteBoard),
    ("file.suite.check", Msg::SuiteMenuCheck, Cmd::SuiteCheck),
    (
        "file.suite.openings",
        Msg::SuiteMenuOpenings,
        Cmd::SuiteOpenings,
    ),
    (
        "file.suite.hand_back",
        Msg::SuiteMenuHandBack,
        Cmd::SuiteHandBack,
    ),
];

/// The id of the entry that exports a format.
pub fn export_id(f: Format) -> String {
    format!(
        "file.export.{}",
        Format::ALL.iter().position(|x| *x == f).unwrap_or(0)
    )
}

/// The id of the entry that opens a panel of the reports.
pub fn report_id(p: Panel) -> String {
    format!(
        "tools.report.{}",
        Panel::ALL.iter().position(|x| *x == p).unwrap_or(0)
    )
}

/// The id of the entry that chooses a language.
pub fn language_id(l: Lang) -> String {
    format!("tools.language.{}", l.code())
}

/// The menu bar: File, Edit, View, Tools, Window and Help. File has the project file of the suite
/// in a submenu after the recent files and the exports of the model in a submenu in the place of
/// the standard Export entry; View has the fit of the views after the
/// panels and the layout; Tools has the two modes, the sketch kept in the project, the panels of
/// the reports and the language.
pub fn menu_bar() -> MenuBar<Cmd> {
    let mut file: Menu<Cmd> = file_menu(true);
    let edit: Menu<Cmd> = edit_menu();
    let mut view: Menu<Cmd> = view_menu();
    let window: Menu<Cmd> = window_menu();
    let help: Menu<Cmd> = help_menu(HelpOptions {
        online_help: false,
        check_updates: false,
    });
    // File: the standard Export entry gives way to the submenu of the formats
    let mut export = Menu::new("file.export_as", Label::Own(t8(Msg::ExportMenu)));
    for f in Format::ALL {
        export = export.entry(own(&export_id(f), format_msg(f), Cmd::Export(f)));
    }
    let at = file
        .items
        .iter()
        .position(|i| matches!(i, Item::Entry(e) if e.id == "file.export"));
    match at {
        Some(i) => file.items[i] = Item::Submenu(export),
        None => file = file.submenu(export),
    }
    let mut suite = Menu::new("file.suite", Label::Own(t8(Msg::SuiteMenu)));
    for (id, words, cmd) in SUITE_ENTRIES {
        suite = suite.entry(own(id, words, cmd));
    }
    // the shared panel of the attachments of that project
    suite = suite.entry(Entry::dock(
        "file.suite.attachments",
        Label::Own(eh3d_attachpanel::panel_title()),
        DockAction::ShowPanel(id::ATTACHMENTS.to_string()),
    ));
    let after_recent = file
        .items
        .iter()
        .position(|i| matches!(i, Item::Generated(Gen::Recent)))
        .map_or(file.items.len(), |i| i + 1);
    file.items.insert(after_recent, Item::Submenu(suite));
    file = file
        .sep()
        .entry(own("file.open_library", Msg::OpenLibrary, Cmd::OpenLibrary))
        .entry(own("file.import_dxf", Msg::PanelImportDxf, Cmd::ImportDxf))
        .entry(own("file.view_file", Msg::MvMenu, Cmd::ViewFile))
        .entry(own("file.fit_report", Msg::ReportShow, Cmd::FitReport));
    view = view
        .sep()
        .entry(own("view.fit", Msg::FitView, Cmd::FitView));
    let mut reports = Menu::new("tools.reports", Label::Own(t8(Msg::ReportsMenu)));
    for p in Panel::ALL {
        reports = reports.entry(own(&report_id(p), p.title(), Cmd::Report(p)));
    }
    let mut languages = Menu::new("tools.languages", Label::Own(t8(Msg::Language)));
    for l in Lang::ALL {
        languages = languages.entry(Entry::app(
            &language_id(l),
            Label::Own(Text8([l.name(); 8])),
            Cmd::Language(l),
        ));
    }
    let tools = Menu::new("tools", Label::Std(eh3d_dock::texts::DockText::MenuTools))
        .entry(own("tools.edit_model", Msg::ModelTitle, Cmd::EditModel))
        .entry(own(
            "tools.edit_sketch",
            Msg::WorkspaceSketch,
            Cmd::EditSketch,
        ))
        .entry(own("tools.keep_sketch", Msg::KeepSketch, Cmd::KeepSketch))
        .entry(own("tools.drawings", Msg::PlTitle, Cmd::ShowDrawings))
        .entry(own("tools.check_sketch", Msg::CheckMenu, Cmd::CheckSketch))
        .entry(own("tools.variants", Msg::VariantsMenu, Cmd::Variants))
        .sep()
        .submenu(reports)
        .sep()
        .submenu(languages);
    MenuBar::new(vec![file, edit, view, tools, window, help])
}

/// What the window knows when it decides which entries of the menus can be chosen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MenuFacts {
    /// The window is working on the sketch.
    pub sketch_mode: bool,
    /// There is a change to undo.
    pub can_undo: bool,
    /// There is a change to redo.
    pub can_redo: bool,
    /// The project may be written (it is not open read-only).
    pub can_save: bool,
    /// The project of the window is a file of the suite.
    pub suite_file: bool,
}

/// The ids of the entries that cannot be chosen now. The ones for things the program does not have
/// (cut, copy, paste, preferences) are always off.
pub fn disabled_entries(f: &MenuFacts) -> Vec<String> {
    let mut off: Vec<String> = ["edit.cut", "edit.copy", "edit.paste", "edit.preferences"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    if !f.can_undo {
        off.push("edit.undo".to_string());
    }
    if !f.can_redo {
        off.push("edit.redo".to_string());
    }
    if !f.can_save && !f.sketch_mode {
        off.push("file.save".to_string());
        off.push("file.save_as".to_string());
    }
    if f.sketch_mode {
        // what is done to the model is not done from the sketch
        off.extend(Format::ALL.iter().map(|x| export_id(*x)));
        off.push("file.open_library".to_string());
        off.push("file.fit_report".to_string());
        off.push("tools.variants".to_string());
        off.extend(Panel::ALL.iter().map(|p| report_id(*p)));
    }
    for (id, _, cmd) in SUITE_ENTRIES {
        // the suite file is the model's; some entries need one, and writing needs leave to write
        let needs_file = !matches!(cmd, Cmd::SuiteOpen | Cmd::SuiteSave | Cmd::SuiteSaveAs);
        let writes = !matches!(cmd, Cmd::SuiteOpen | Cmd::SuiteBranches | Cmd::SuiteCheck);
        if f.sketch_mode || (needs_file && !f.suite_file) || (writes && !f.can_save) {
            off.push(id.to_string());
        }
    }
    if !f.sketch_mode {
        off.push("tools.check_sketch".to_string());
    }
    // the sketch is kept in the project from the sketch, and only when the project may be written
    if !f.sketch_mode || !f.can_save {
        off.push("tools.keep_sketch".to_string());
    }
    off.sort();
    off.dedup();
    off
}

/// The entries that are ticked: the language in use.
pub fn checks(lang: Lang) -> Vec<(String, bool)> {
    Lang::ALL
        .iter()
        .map(|l| (language_id(*l), *l == lang))
        .collect()
}

// ----- the arrangement in the settings file -----

/// The lines of the frame that a settings file holds, as `dock.key=value` text for the frame to
/// read. The settings file is the one of the program: sections with `key = value` lines, and the
/// frame's lines are the section `[dock]` (the key `dock.ws.1.tree` is the key `ws.1.tree` of that
/// section).
pub fn dock_text_of(settings: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for raw in settings.lines() {
        let line = raw.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = name.trim() == "dock";
            continue;
        }
        if !inside || line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.push_str(&format!("dock.{}={}\n", k.trim(), v.trim()));
        }
    }
    out
}

/// A settings text with the section `[dock]` replaced by the lines of the frame: every other line
/// stays as it was, in the line ending the file uses, and the section goes at the end.
pub fn with_dock_section(existing: &str, pairs: &[(String, String)]) -> String {
    let eol = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = String::new();
    let mut skipping = false;
    for line in existing.lines() {
        let t = line.trim();
        if let Some(name) = t.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            skipping = name.trim() == "dock";
        }
        if skipping {
            continue;
        }
        out.push_str(line);
        out.push_str(eol);
    }
    if !out.is_empty() && !out.ends_with(&format!("{eol}{eol}")) {
        out.push_str(eol);
    }
    out.push_str(&format!("[dock]{eol}"));
    for (k, v) in pairs {
        let key = k.strip_prefix("dock.").unwrap_or(k);
        let v: String = v.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        out.push_str(&format!("{key} = {v}{eol}"));
    }
    out
}

/// Decides when the arrangement of the panels is written to the settings file: when the lines of
/// the frame are not the ones last written (or read). It writes nothing when nothing changed, so
/// dragging a panel writes once and a quiet window writes never.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayoutWatch {
    saved: String,
}

impl LayoutWatch {
    /// A watch that knows the lines of the frame as they were read.
    pub fn new(pairs: &[(String, String)]) -> LayoutWatch {
        LayoutWatch {
            saved: LayoutWatch::flat(pairs),
        }
    }

    fn flat(pairs: &[(String, String)]) -> String {
        pairs.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
    }

    /// Whether the lines changed since the last time this answered yes (or since the start); a yes
    /// counts them as written.
    pub fn changed(&mut self, pairs: &[(String, String)]) -> bool {
        let now = LayoutWatch::flat(pairs);
        if now == self.saved {
            return false;
        }
        self.saved = now;
        true
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod about_tests {
    use super::*;

    #[test]
    fn the_window_about_has_the_name_the_repository_and_the_picture_of_this_program() {
        let brand = about_brand();
        assert_eq!(brand.program, Phoenix::Cad);
        assert_eq!(brand.program.name(), "EH3D-PHOENIX-CAD");
        assert_eq!(
            brand.program.repository(),
            "https://github.com/JuanenRac/EH3D-PHOENIX-CAD"
        );
        let png = brand.logo_png;
        assert_eq!(png.get(1..4), Some(&b"PNG"[..]));
        // the width and the height in the header of the picture
        assert_eq!(png.get(16..24), Some(&[0, 0, 1, 0, 0, 0, 1, 0][..]));
    }
}
