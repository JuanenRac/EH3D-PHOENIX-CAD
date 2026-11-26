//! The commands of the project file of the suite (`.eh3d`) and the ones that work with the board
//! of its branch `pcb`:
//!
//! * `eh3d-info`, `eh3d-save`, `eh3d-open`, `eh3d-import`, `eh3d-meta`: the file and the branch
//!   `cad`;
//! * `eh3d-new`, `eh3d-board`, `eh3d-check`, `eh3d-openings`, `eh3d-publish`: the box made from
//!   the board, kept up to date with it and handed back to the other programs;
//! * `select`, `params`, `panels`: selectors over the stable names, the table of the parameters
//!   and the flat templates of the faces (on any project, `.eh3d` included).
//!
//! The exit code is the one of every check of the suite: 0 when everything agrees, 2 when the
//! verification did not pass, 1 when the command could not run. Only `cad`, `docs/cad` and the
//! metadata of `common` are written; the branch of the board is only read. Times are given with
//! `--time`: the program does not read the clock into a file.

use std::collections::BTreeMap;
use std::path::Path;

use eh3d_project::meta::{LocalText, REPORT_FILE};
use eh3d_project::{Dependency, Project, Seal, SignatureStatus};

use crate::boardart::{
    self, BoardArtifact, Change, ARTIFACT, ARTIFACT_VERSION, MOUNTS_ENTRY, SNAPSHOT_ENTRY,
    SOURCE_BRANCH, SOURCE_ENTRY,
};
use crate::boardfit::{self, BoxRequest, Outcome, Trouble};
use crate::config::Settings;
use crate::eh3dfile::{self, SaveSettings, BRANCH, PROJECT_ENTRY};
use crate::export::{write_bytes, Format};
use crate::flatpanels;
use crate::lang::{fill, Lang, Msg};
use crate::mechanical::{Mechanical, MECHANICAL_DOMAIN};
use crate::mount::BoreKind;
use crate::paramreport::{self, Breach};
use crate::profile::ProfileLimits;
use crate::project::{Document, PARAMETERS_DOMAIN};
use crate::scad::mm_text;
use crate::select;
use crate::sketch::settings::SketchSettings;
use crate::suitewin;
use crate::tools_cli::Args;
use crate::uiconfig::UiSettings;
use crate::units::parse_length;
use crate::window::{error_text, Window};
use crate::{CadError, Length};

/// The commands of this module, for the program to know which words are theirs.
pub const COMMANDS: [&str; 13] = [
    "eh3d-info",
    "eh3d-save",
    "eh3d-open",
    "eh3d-new",
    "eh3d-board",
    "eh3d-check",
    "eh3d-openings",
    "eh3d-publish",
    "eh3d-import",
    "eh3d-meta",
    "select",
    "params",
    "panels",
];

/// The largest project or foreign file that these commands read.
const MAX_BYTES: u64 = 256 << 20;

/// What a command answers: the text it prints and whether the check passed.
type Answer = Result<(String, bool), String>;

/// Runs one of the commands of [`COMMANDS`].
pub fn run(command: &str, args: &[String]) -> Answer {
    match command {
        "eh3d-info" => info(args),
        "eh3d-save" => save(args),
        "eh3d-open" => open(args),
        "eh3d-new" => new_box(args),
        "eh3d-board" => take_board(args),
        "eh3d-check" => check(args),
        "eh3d-openings" => openings(args),
        "eh3d-publish" => publish(args),
        "eh3d-import" => import(args),
        "eh3d-meta" => metadata(args),
        "select" => selector(args),
        "params" => parameters(args),
        "panels" => panels(args),
        other => Err(format!("{other}: unknown command (see help)")),
    }
}

pub(crate) struct All {
    pub(crate) lib: Settings,
    pub(crate) ui: UiSettings,
    pub(crate) sketch: SketchSettings,
}

pub(crate) fn settings(config: Option<&str>) -> Result<All, String> {
    let (lib, ui) = crate::tools_cli::load_settings(config)?;
    let sketch = match config {
        None => SketchSettings::defaults(),
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            SketchSettings::with_user_config(&text).map_err(|e| format!("{path}: {e}"))?
        }
    };
    Ok(All { lib, ui, sketch })
}

fn say(lang: Lang) -> impl Fn(CadError) -> String {
    move |e| error_text(&e, lang)
}

fn line(out: &mut String, message: Msg, lang: Lang, values: &[&str]) {
    out.push_str(&fill(message.tr(lang), values));
    out.push('\n');
}

fn csv(text: &str) -> String {
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

fn length(a: &Args, name: &str, default: &str) -> Result<Length, String> {
    let text = a.value(name).unwrap_or(default);
    parse_length(text).map_err(|e| format!("{name}: {}", error_text(&e, a.lang)))
}

/// The file of the suite named by the only word of the command.
fn suite_file(a: &Args) -> Result<&Path, String> {
    let file = Path::new(a.only_word("project file of the suite (.eh3d)")?);
    if eh3dfile::is_eh3d(file) {
        Ok(file)
    } else {
        Err("the file of the suite ends in .eh3d".to_owned())
    }
}

fn two_words<'a>(a: &'a Args, what: &str) -> Result<(&'a str, &'a str), String> {
    match a.words.as_slice() {
        [first, second] => Ok((first, second)),
        _ => Err(format!("this command needs {what}")),
    }
}

fn save_settings(a: &Args) -> SaveSettings {
    SaveSettings {
        modified: a.value("--time").map(str::to_owned),
        audit_at: a.value("--audit").map(str::to_owned),
        keep_backup: a.has("--backup"),
        seal: None,
    }
}

const SAVE_VALUES: [&str; 3] = ["--time", "--audit", "--config"];

fn parse(args: &[String], more: &[&str], switches: &[&str]) -> Result<Args, String> {
    let mut with_value = SAVE_VALUES.to_vec();
    with_value.extend_from_slice(more);
    Args::parse(args, &with_value, switches)
}

/// The word of the comma separated values for what is known about a signature.
fn signature_word(signature: &SignatureStatus) -> &'static str {
    match signature {
        SignatureStatus::Unsigned => "unsigned",
        SignatureStatus::Unchecked => "unchecked",
        SignatureStatus::Valid(_) => "valid",
        SignatureStatus::Invalid => "invalid",
        SignatureStatus::UnknownKey => "unknown-key",
    }
}

fn info(args: &[String]) -> Answer {
    let a = Args::parse(args, &["--key", "--config"], &["--text"])?;
    let file = suite_file(&a)?;
    let mut key_files = settings(a.value("--config"))?.ui.suite.trusted_keys;
    key_files.extend(a.value("--key").map(str::to_owned));
    let (keys, problems) = eh3dfile::keyring(&key_files);
    // a key that was asked for and cannot be used would leave signatures unchecked
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    let found = eh3dfile::summary_with(file, Some(&keys)).map_err(say(a.lang))?;
    let mut out = String::new();
    if a.has("--text") {
        line(
            &mut out,
            Msg::SuiteInfoHead,
            a.lang,
            &[
                &found.name,
                &found.branches.len().to_string(),
                &found.findings.len().to_string(),
            ],
        );
        for b in &found.branches {
            line(
                &mut out,
                Msg::SuiteBranchLine,
                a.lang,
                &[
                    &b.id,
                    &b.app,
                    &b.format,
                    &b.files.to_string(),
                    &b.bytes.to_string(),
                ],
            );
            for source in &b.stale {
                line(&mut out, Msg::SuiteStale, a.lang, &[&b.id, source]);
            }
            if let Some(words) = suitewin::signature_text(&b.signature, a.lang) {
                out.push_str(&format!("{}: {words}\n", b.id));
            }
        }
        if let Some(words) = suitewin::signature_text(&found.project_signature, a.lang) {
            line(&mut out, Msg::SuiteProjectSignature, a.lang, &[&words]);
        }
    } else {
        out.push_str("branch,app,format,files,bytes,state,signature\n");
        for b in &found.branches {
            let state = if !b.stale.is_empty() {
                format!("stale:{}", b.stale.join("+"))
            } else if b.encrypted {
                "encrypted".to_owned()
            } else if b.sealed {
                "sealed".to_owned()
            } else {
                "ok".to_owned()
            };
            out.push_str(&format!(
                "{},{},{},{},{},{state},{}\n",
                csv(&b.id),
                csv(&b.app),
                csv(&b.format),
                b.files,
                b.bytes,
                signature_word(&b.signature)
            ));
        }
        if found.project_signature != SignatureStatus::Unsigned {
            out.push_str(&format!(
                "project-signature,{}\n",
                signature_word(&found.project_signature)
            ));
        }
    }
    for f in &found.findings {
        out.push_str(&format!(
            "finding,{},{},{}\n",
            f.code.code(),
            csv(f.branch.as_deref().unwrap_or("")),
            csv(&f.detail)
        ));
    }
    Ok((
        out,
        found.findings.is_empty() && !found.any_wrong_signature(),
    ))
}

fn save(args: &[String]) -> Answer {
    let a = parse(args, &["--seal"], &["--backup"])?;
    let (project, target) = two_words(&a, "a project and the file of the suite (.eh3d)")?;
    let target = Path::new(target);
    if !eh3dfile::is_eh3d(target) {
        return Err("the file of the suite ends in .eh3d".to_owned());
    }
    let bytes = eh3dfile::load_project_bytes(Path::new(project), MAX_BYTES).map_err(say(a.lang))?;
    // only a project of this program goes into its branch
    Document::from_bytes(&bytes, bytes.len()).map_err(say(a.lang))?;
    let mut how = save_settings(&a);
    if let Some(by) = a.value("--seal") {
        how.seal = Some(Seal {
            by: by.to_owned(),
            at: a
                .value("--time")
                .ok_or("--seal needs the time of --time")?
                .to_owned(),
            reason: None,
        });
    }
    eh3dfile::save_cad_project(target, &bytes, &how).map_err(say(a.lang))?;
    let mut out = String::new();
    line(
        &mut out,
        Msg::SuiteSaved,
        a.lang,
        &[&target.display().to_string()],
    );
    Ok((out, true))
}

fn open(args: &[String]) -> Answer {
    let a = Args::parse(args, &["--out"], &["--overwrite"])?;
    let file = suite_file(&a)?;
    let target = a.value("--out").ok_or("eh3d-open needs --out")?;
    // the one export that writes a project: the writer of exports refuses such a name
    if !target.to_ascii_lowercase().ends_with(".phoenixproj") {
        return Err("--out ends in .phoenixproj".to_owned());
    }
    let bytes = eh3dfile::read_cad_project(file, MAX_BYTES).map_err(say(a.lang))?;
    if !a.has("--overwrite") && Path::new(target).exists() {
        return Err(error_text(
            &CadError::ExportExists(target.to_owned()),
            a.lang,
        ));
    }
    crate::store::save_atomic(Path::new(target), &bytes, 0).map_err(say(a.lang))?;
    let mut out = String::new();
    line(&mut out, Msg::SuiteOpened, a.lang, &[target]);
    Ok((out, true))
}

/// The board as the branch of the board program has it now.
fn board_now(project: &mut Project, lib: &Settings) -> Result<(BoardArtifact, Vec<u8>), CadError> {
    let bytes = eh3dfile::read_file(
        project,
        SOURCE_BRANCH,
        SOURCE_ENTRY,
        lib.mechanical.max_bytes as u64,
    )?
    .ok_or_else(|| {
        CadError::SuiteBranch(format!(
            "EH3D-PKG-018 the branch {SOURCE_BRANCH} has no {SOURCE_ENTRY}"
        ))
    })?;
    let artifact = boardart::parse(&bytes, &lib.mechanical)?;
    let canonical = boardart::to_bytes(&artifact)?;
    Ok((artifact, canonical))
}

fn reference(project: &Project) -> Result<Dependency, CadError> {
    project
        .dependency_on(
            SOURCE_BRANCH,
            ARTIFACT,
            ARTIFACT_VERSION,
            vec![PROJECT_ENTRY.to_owned(), SNAPSHOT_ENTRY.to_owned()],
        )
        .map_err(|e| CadError::SuiteBranch(e.to_string()))
}

/// The recipe kept in the branch `cad`, with its sketches, and the document it came from.
fn recipe(file: &Path, s: &All) -> Result<(Document, Mechanical), CadError> {
    let bytes = eh3dfile::read_cad_project(file, MAX_BYTES)?;
    let opened = Document::open(&bytes, bytes.len(), &[MECHANICAL_DOMAIN, PARAMETERS_DOMAIN])?;
    if opened.read_only {
        return Err(CadError::Format(
            "the project has a part this version does not understand and is not edited".to_owned(),
        ));
    }
    let mut mech = opened
        .document
        .mechanical(&s.lib.expr, &s.lib.mechanical)?
        .ok_or_else(|| CadError::Format("the project has no box".to_owned()))?;
    mech.sketches = opened.document.sketches(&s.sketch)?;
    Ok((opened.document, mech))
}

fn built(mech: &Mechanical, s: &All) -> Result<crate::body::Built, CadError> {
    mech.build_with_limits(
        &s.lib.constants,
        &s.lib.expr,
        Some(ProfileLimits::from_settings(&s.sketch)),
        Some(s.lib.design),
    )
}

fn snapshot(project: &mut Project, lib: &Settings) -> Result<Option<BoardArtifact>, CadError> {
    eh3dfile::read_file(
        project,
        BRANCH,
        SNAPSHOT_ENTRY,
        lib.mechanical.max_bytes as u64,
    )?
    .map(|bytes| boardart::parse(&bytes, &lib.mechanical))
    .transpose()
}

fn new_box(args: &[String]) -> Answer {
    let a = parse(
        args,
        &["--gap", "--headroom", "--post-height", "--screw", "--kind"],
        &["--overwrite", "--backup"],
    )?;
    let file = suite_file(&a)?;
    let s = settings(a.value("--config"))?;
    let has_project = eh3dfile::has_project(file).map_err(say(a.lang))?;
    if has_project && !a.has("--overwrite") {
        return Err(error_text(
            &CadError::ExportExists(format!("{BRANCH}/{PROJECT_ENTRY}")),
            a.lang,
        ));
    }
    let mut request = board_request(&s.ui);
    if a.value("--gap").is_some() {
        request.gap = length(&a, "--gap", "0 mm")?;
    }
    if a.value("--headroom").is_some() {
        request.headroom = length(&a, "--headroom", "0 mm")?;
    }
    if a.value("--post-height").is_some() {
        request.post_height = length(&a, "--post-height", "0 mm")?;
    }
    if let Some(screw) = a.value("--screw") {
        request.screw = screw;
    }
    if let Some(kind) = a.value("--kind") {
        request.kind = BoreKind::from_name(kind).ok_or("--kind is insert or self-tapping")?;
    }
    let out = make_box(file, &s, &request, a.lang, &save_settings(&a))?;
    Ok((out, true))
}

/// The box made round a board as the settings of the window have it (`[suite]`).
pub(crate) fn board_request(ui: &UiSettings) -> BoxRequest<'_> {
    BoxRequest {
        gap: ui.suite.board_gap,
        headroom: ui.suite.board_headroom,
        post_height: ui.suite.post_height,
        screw: &ui.suite.post_screw,
        kind: ui.suite.post_kind,
        shell: ui.new_box,
    }
}

/// Makes the box round the board of the branch `pcb`, in place of the project of the branch
/// `cad`, and answers with what was made.
pub(crate) fn make_box(
    file: &Path,
    s: &All,
    request: &BoxRequest<'_>,
    lang: Lang,
    how: &SaveSettings,
) -> Result<String, String> {
    let mut project = eh3dfile::open_project(file).map_err(say(lang))?;
    let (artifact, canonical) = board_now(&mut project, &s.lib).map_err(say(lang))?;
    let mech = boardfit::box_for_board(&s.lib, &artifact, request).map_err(say(lang))?;
    let made = built(&mech, s).map_err(say(lang))?;
    let mut document = Document::new();
    document
        .set_mechanical(&mech, &s.lib.mechanical)
        .map_err(say(lang))?;
    let depends = vec![reference(&project).map_err(say(lang))?];
    let changes = BTreeMap::from([
        (PROJECT_ENTRY.to_owned(), Some(document.to_bytes())),
        (SNAPSHOT_ENTRY.to_owned(), Some(canonical)),
    ]);
    eh3dfile::write_branch(file, BRANCH, changes, Some(depends), how).map_err(say(lang))?;
    let p = made.enclosure.params();
    let mut out = String::new();
    line(
        &mut out,
        Msg::SuiteBoxMade,
        lang,
        &[
            &mm_text(p.inner_length.get()),
            &mm_text(p.inner_width.get()),
            &mm_text(p.inner_height.get()),
            &made.enclosure.posts().len().to_string(),
        ],
    );
    Ok(out)
}

fn change_line(out: &mut String, change: &Change, lang: Lang) {
    let (message, name) = match change {
        Change::Outline => (Msg::ChangeOutline, ""),
        Change::Thickness => (Msg::ChangeThickness, ""),
        Change::Tallest => (Msg::ChangeTallest, ""),
        Change::HoleAdded(n) => (Msg::ChangeHoleAdded, n.as_str()),
        Change::HoleRemoved(n) => (Msg::ChangeHoleRemoved, n.as_str()),
        Change::HoleMoved(n) => (Msg::ChangeHoleMoved, n.as_str()),
        Change::HoleResized(n) => (Msg::ChangeHoleResized, n.as_str()),
        Change::Parts => (Msg::ChangeParts, ""),
        Change::Connectors => (Msg::ChangeConnectors, ""),
    };
    line(out, message, lang, &[name]);
}

fn take_board(args: &[String]) -> Answer {
    let a = parse(args, &[], &["--backup"])?;
    let file = suite_file(&a)?;
    let s = settings(a.value("--config"))?;
    follow_board(file, &s, a.lang, &save_settings(&a))
}

/// Moves the box of the branch `cad` with the changes of the board of `pcb`, names every change
/// and answers whether nothing is left for the person.
pub(crate) fn follow_board(file: &Path, s: &All, lang: Lang, how: &SaveSettings) -> Answer {
    let (mut document, mut mech) = recipe(file, s).map_err(say(lang))?;
    let mut project = eh3dfile::open_project(file).map_err(say(lang))?;
    let old = snapshot(&mut project, &s.lib).map_err(say(lang))?;
    let (artifact, canonical) = board_now(&mut project, &s.lib).map_err(say(lang))?;
    let mut out = String::new();
    match &old {
        None => line(&mut out, Msg::SuiteBoardImported, lang, &[]),
        Some(old) => {
            let changes = boardart::changes(old, &artifact);
            if changes.is_empty() {
                line(&mut out, Msg::SuiteBoardSame, lang, &[]);
            }
            for change in &changes {
                change_line(&mut out, change, lang);
            }
        }
    }
    let update = boardfit::update_board(&s.lib, &mut mech, &artifact).map_err(say(lang))?;
    for name in &update.moved {
        line(&mut out, Msg::SuitePostMoved, lang, &[name]);
    }
    for name in &update.orphans {
        line(&mut out, Msg::SuitePostOrphan, lang, &[name]);
    }
    for name in &update.bare {
        line(&mut out, Msg::SuiteHoleBare, lang, &[name]);
    }
    for name in &update.failed {
        line(&mut out, Msg::SuiteStepFailed, lang, &[name]);
    }
    document
        .set_mechanical(&mech, &s.lib.mechanical)
        .map_err(say(lang))?;
    let depends = vec![reference(&project).map_err(say(lang))?];
    let changes = BTreeMap::from([
        (PROJECT_ENTRY.to_owned(), Some(document.to_bytes())),
        (SNAPSHOT_ENTRY.to_owned(), Some(canonical)),
    ]);
    eh3dfile::write_branch(file, BRANCH, changes, Some(depends), how).map_err(say(lang))?;
    let ok = update.orphans.is_empty() && update.bare.is_empty() && update.failed.is_empty();
    Ok((out, ok))
}

fn check(args: &[String]) -> Answer {
    let a = Args::parse(args, &["--clearance", "--config"], &[])?;
    let file = suite_file(&a)?;
    let s = settings(a.value("--config"))?;
    let mut clearance = s.ui.suite.part_clearance;
    if a.value("--clearance").is_some() {
        clearance = length(&a, "--clearance", "0 mm")?;
    }
    check_fit(file, &s, clearance, a.lang)
}

/// Says whether the box agrees with the board of `pcb` as it is now and every part has room.
pub(crate) fn check_fit(file: &Path, s: &All, clearance: Length, lang: Lang) -> Answer {
    let (_, mech) = recipe(file, s).map_err(say(lang))?;
    let mut project = eh3dfile::open_project(file).map_err(say(lang))?;
    let mut out = String::new();
    let Some(taken) = snapshot(&mut project, &s.lib).map_err(say(lang))? else {
        line(&mut out, Msg::SuiteBoardNone, lang, &[]);
        return Ok((out, false));
    };
    let (mut now, _) = board_now(&mut project, &s.lib).map_err(say(lang))?;
    let stale = project
        .stale_refs()
        .iter()
        .any(|r| r.branch == BRANCH && r.dependency.branch == SOURCE_BRANCH);
    let changes = boardart::changes(&taken, &now);
    if stale {
        line(&mut out, Msg::SuiteBoardStale, lang, &[]);
    }
    if changes.is_empty() {
        line(&mut out, Msg::SuiteBoardSame, lang, &[]);
    }
    for change in &changes {
        change_line(&mut out, change, lang);
    }
    // the room is checked for the board as it is now, where the box has it
    now.board.placement = mech.board.as_ref().and_then(|b| b.placement);
    let made = built(&mech, s).map_err(say(lang))?;
    let problems = boardfit::check_parts(&made.enclosure, &now, clearance).map_err(say(lang))?;
    if problems.is_empty() {
        line(&mut out, Msg::FitAll, lang, &[]);
    }
    for p in &problems {
        let message = match p.trouble {
            Trouble::Outside => Msg::FitOutside,
            Trouble::Lid => Msg::FitLid,
            Trouble::Floor => Msg::FitFloor,
            Trouble::Post => Msg::FitPost,
        };
        // the sentence of a post has one place only: a second value has nowhere to go
        line(&mut out, message, lang, &[&p.part, &mm_text(p.short)]);
    }
    Ok((out, changes.is_empty() && problems.is_empty()))
}

fn openings(args: &[String]) -> Answer {
    let a = parse(args, &["--clearance"], &["--apply", "--backup"])?;
    let file = suite_file(&a)?;
    let s = settings(a.value("--config"))?;
    let mut clearance = s.ui.suite.opening_clearance;
    if a.value("--clearance").is_some() {
        clearance = length(&a, "--clearance", "0 mm")?;
    }
    let how = a.has("--apply").then(|| save_settings(&a));
    cut_openings(file, &s, clearance, a.lang, how.as_ref())
}

/// The openings of the connectors in the walls of the box; they are written into the recipe only
/// when `write` says how.
pub(crate) fn cut_openings(
    file: &Path,
    s: &All,
    clearance: Length,
    lang: Lang,
    write: Option<&SaveSettings>,
) -> Answer {
    let (mut document, mut mech) = recipe(file, s).map_err(say(lang))?;
    let mut project = eh3dfile::open_project(file).map_err(say(lang))?;
    let mut out = String::new();
    let Some(mut taken) = snapshot(&mut project, &s.lib).map_err(say(lang))? else {
        line(&mut out, Msg::SuiteBoardNone, lang, &[]);
        return Ok((out, false));
    };
    taken.board.placement = mech.board.as_ref().and_then(|b| b.placement);
    let done = boardfit::apply_openings(&s.lib, &mut mech, &taken, clearance).map_err(say(lang))?;
    let mut ok = true;
    for (name, outcome) in &done {
        match outcome {
            Outcome::Added => line(&mut out, Msg::OpenAdded, lang, &[name]),
            Outcome::Moved => line(&mut out, Msg::OpenMoved, lang, &[name]),
            Outcome::Same => line(&mut out, Msg::OpenSame, lang, &[name]),
            Outcome::Refused(e) => {
                ok = false;
                let why = error_text(e, lang);
                line(&mut out, Msg::OpenRefused, lang, &[name, &why]);
            }
        }
    }
    if let Some(how) = write {
        document
            .set_mechanical(&mech, &s.lib.mechanical)
            .map_err(say(lang))?;
        let changes = BTreeMap::from([(PROJECT_ENTRY.to_owned(), Some(document.to_bytes()))]);
        eh3dfile::write_branch(file, BRANCH, changes, None, how).map_err(say(lang))?;
    }
    Ok((out, ok))
}

pub(crate) fn window(file: &Path, lang: Lang, s: &All) -> Result<Window, String> {
    let mut win = Window::new(lang, s.lib.clone(), s.ui.clone()).map_err(say(lang))?;
    win.sketch_settings = s.sketch.clone();
    if !win.open(file) {
        return Err(format!("{}: {}", file.display(), win.status.text(lang)));
    }
    Ok(win)
}

fn titles(message: Msg) -> LocalText {
    Lang::ALL
        .into_iter()
        .map(|l| {
            (
                eh3dfile::language_key(l.code()).to_owned(),
                message.tr(l).to_owned(),
            )
        })
        .collect()
}

fn publish(args: &[String]) -> Answer {
    let a = parse(args, &["--sheet-width", "--gap"], &["--backup"])?;
    let file = suite_file(&a)?;
    let s = settings(a.value("--config"))?;
    let mut width = s.ui.suite.sheet_width;
    if a.value("--sheet-width").is_some() {
        width = length(&a, "--sheet-width", "0 mm")?;
    }
    let mut gap = s.ui.suite.sheet_gap;
    if a.value("--gap").is_some() {
        gap = length(&a, "--gap", "0 mm")?;
    }
    hand_back(file, &s, (width, gap), a.lang, &save_settings(&a))
}

/// Writes into the branch `cad` what the other programs take (the STL of the solid, the flat
/// faces on a sheet of this width and gap, the posts) and the documents of the box.
pub(crate) fn hand_back(
    file: &Path,
    s: &All,
    (width, gap): (Length, Length),
    lang: Lang,
    how: &SaveSettings,
) -> Answer {
    let win = window(file, lang, s)?;
    let mut out = String::new();
    let mut ok = true;
    let mut changes: BTreeMap<String, Option<Vec<u8>>> = BTreeMap::new();
    let mut put = |out: &mut String, name: &str, made: Result<Vec<u8>, CadError>| match made {
        Ok(bytes) => {
            line(out, Msg::SuiteWrote, lang, &[&format!("{BRANCH}/{name}")]);
            changes.insert(name.to_owned(), Some(bytes));
            true
        }
        Err(e) => {
            // an older file of this name would no longer be the box: it is taken away
            let why = error_text(&e, lang);
            line(out, Msg::SuiteSkipped, lang, &[name, &why]);
            changes.insert(name.to_owned(), None);
            false
        }
    };
    ok &= put(&mut out, "export/solid.stl", win.export_bytes(Format::Stl));
    let layout =
        flatpanels::panels(win.enclosure()).and_then(|list| flatpanels::pack(list, width, gap));
    ok &= put(
        &mut out,
        "export/panels.dxf",
        layout.clone().map(|l| flatpanels::to_dxf(&l).into_bytes()),
    );
    ok &= put(
        &mut out,
        "export/panels.svg",
        layout.map(|l| flatpanels::to_svg(&l).into_bytes()),
    );
    if let Some(board) = &win.mech.board {
        ok &= put(
            &mut out,
            MOUNTS_ENTRY,
            boardart::mounts(win.enclosure(), board),
        );
    }
    eh3dfile::write_branch(file, BRANCH, changes, None, how).map_err(say(lang))?;
    let documents = [
        eh3dfile::DocFile {
            name: "report.txt".to_owned(),
            title: titles(Msg::DocReportTitle),
            bytes: win.export_bytes(Format::Report).map_err(say(lang))?,
        },
        eh3dfile::DocFile {
            name: "bill.txt".to_owned(),
            title: titles(Msg::DocBillTitle),
            bytes: win.export_bytes(Format::Bill).map_err(say(lang))?,
        },
    ];
    eh3dfile::write_documents(file, &documents, how).map_err(say(lang))?;
    for d in &documents {
        let name = format!("{}/{}", eh3dfile::DOCS_BRANCH, d.name);
        line(&mut out, Msg::SuiteWrote, lang, &[&name]);
    }
    Ok((out, ok))
}

fn import(args: &[String]) -> Answer {
    let a = parse(args, &[], &["--backup"])?;
    let (target, foreign) = two_words(&a, "the file of the suite and the file to keep")?;
    let target = Path::new(target);
    if !eh3dfile::is_eh3d(target) {
        return Err("the file of the suite ends in .eh3d".to_owned());
    }
    let s = settings(a.value("--config"))?;
    let done =
        suitewin::import_original(target, Path::new(foreign), &s.lib.mesh, &save_settings(&a))
            .map_err(|e| format!("{foreign}: {}", e.text(a.lang)))?;
    let mut out = String::new();
    line(
        &mut out,
        Msg::SuiteImported,
        a.lang,
        &[
            &done.name,
            &done.solids.to_string(),
            &done.triangles.to_string(),
        ],
    );
    out.push_str(&format!("{BRANCH}/{REPORT_FILE}\n"));
    Ok((out, true))
}

fn metadata(args: &[String]) -> Answer {
    let a = parse(args, &["--title", "--description"], &["--backup"])?;
    let file = suite_file(&a)?;
    let title = a.value("--title");
    let description = a.value("--description");
    if title.is_none() && description.is_none() {
        return Err("eh3d-meta needs --title or --description (in the language of --lang)".into());
    }
    eh3dfile::set_metadata(file, a.lang.code(), title, description, &save_settings(&a))
        .map_err(say(a.lang))?;
    let mut out = String::new();
    line(
        &mut out,
        Msg::SuiteMetaSet,
        a.lang,
        &[&file.display().to_string()],
    );
    Ok((out, true))
}

fn selector(args: &[String]) -> Answer {
    let a = Args::parse(args, &["--config"], &[])?;
    let (project, text) = two_words(&a, "a project and a selector")?;
    let s = settings(a.value("--config"))?;
    let win = window(Path::new(project), a.lang, &s)?;
    let names = select::select(win.enclosure(), text).map_err(say(a.lang))?;
    if names.is_empty() {
        return Ok((format!("{}\n", Msg::SelectNone.tr(a.lang)), false));
    }
    Ok((names.iter().map(|n| format!("{n}\n")).collect(), true))
}

fn parameters(args: &[String]) -> Answer {
    let a = Args::parse(args, &["--limits", "--config"], &["--text"])?;
    let project = a.only_word("project file")?;
    let s = settings(a.value("--config"))?;
    let limits = match a.value("--limits") {
        None => BTreeMap::new(),
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            paramreport::parse_limits(&text).map_err(say(a.lang))?
        }
    };
    let win = window(Path::new(project), a.lang, &s)?;
    let table = paramreport::rows(&win.mech, &win.params(), &limits);
    let mut out = String::new();
    if !a.has("--text") {
        out.push_str("parameter,value_nm,kind,definition,breaches\n");
    }
    for row in &table {
        if a.has("--text") {
            let how = match row.kind {
                "variable" => fill(Msg::ParamVariable.tr(a.lang), &[&row.definition]),
                "expression" => fill(Msg::ParamExpression.tr(a.lang), &[&row.definition]),
                _ => Msg::ParamFixed.tr(a.lang).to_owned(),
            };
            line(
                &mut out,
                Msg::ParamLine,
                a.lang,
                &[row.param.name(), &mm_text(row.value), &how],
            );
            for breach in &row.breaches {
                let message = match breach {
                    Breach::Below => Msg::ParamBelow,
                    Breach::Above => Msg::ParamAbove,
                    Breach::Step => Msg::ParamStep,
                };
                line(&mut out, message, a.lang, &[row.param.name()]);
            }
        } else {
            let words: Vec<&str> = row.breaches.iter().map(|b| b.word()).collect();
            out.push_str(&format!(
                "{},{},{},{},{}\n",
                row.param.name(),
                row.value,
                row.kind,
                csv(&row.definition),
                words.join("+")
            ));
        }
    }
    Ok((out, table.iter().all(|r| r.breaches.is_empty())))
}

fn panels(args: &[String]) -> Answer {
    let a = Args::parse(
        args,
        &["--sheet-width", "--gap", "--config"],
        &["--overwrite"],
    )?;
    let (project, target) = two_words(&a, "a project and the drawing to write (.dxf or .svg)")?;
    let s = settings(a.value("--config"))?;
    let width = length(&a, "--sheet-width", "600 mm")?;
    let gap = length(&a, "--gap", "5 mm")?;
    let lower = target.to_ascii_lowercase();
    let dxf = if lower.ends_with(".dxf") {
        true
    } else if lower.ends_with(".svg") {
        false
    } else {
        return Err("the drawing ends in .dxf or .svg".to_owned());
    };
    let win = window(Path::new(project), a.lang, &s)?;
    let layout = flatpanels::panels(win.enclosure())
        .and_then(|list| flatpanels::pack(list, width, gap))
        .map_err(say(a.lang))?;
    let text = if dxf {
        flatpanels::to_dxf(&layout)
    } else {
        flatpanels::to_svg(&layout)
    };
    write_bytes(
        Path::new(target),
        text.as_bytes(),
        a.has("--overwrite"),
        win.path.as_deref(),
        s.ui.export.max_kib,
    )
    .map_err(say(a.lang))?;
    let mut out = String::new();
    line(
        &mut out,
        Msg::PanelsWrote,
        a.lang,
        &[target, &mm_text(layout.width), &mm_text(layout.height)],
    );
    Ok((out, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boardart::tests::{mm, sample};
    use crate::eh3dfile::tests::{other_branch, Scratch, Seeded};

    fn call(words: &[&str]) -> Answer {
        let args: Vec<String> = words.iter().skip(1).map(|w| (*w).to_owned()).collect();
        run(words[0], &args)
    }

    fn passed(words: &[&str]) -> String {
        let (out, ok) = call(words).unwrap_or_else(|e| panic!("{words:?}: {e}"));
        assert!(ok, "{words:?}: {out}");
        out
    }

    fn failed(words: &[&str]) -> String {
        let (out, ok) = call(words).unwrap_or_else(|e| panic!("{words:?}: {e}"));
        assert!(!ok, "{words:?}: {out}");
        out
    }

    fn write_board(file: &Path, board: &BoardArtifact) {
        other_branch(
            file,
            SOURCE_BRANCH,
            SOURCE_ENTRY,
            &boardart::to_bytes(board).unwrap(),
        );
    }

    fn text(path: &Path) -> String {
        path.to_str().unwrap().to_owned()
    }

    #[test]
    fn a_board_of_the_other_branch_becomes_a_box_that_follows_it() {
        let dir = Scratch::new("flow");
        let file = dir.path("job.eh3d");
        let f = text(&file);
        write_board(&file, &sample());
        let pcb_before = {
            let mut p = eh3dfile::open_project(&file).unwrap();
            eh3dfile::branch_files(&mut p, SOURCE_BRANCH).unwrap()
        };
        let made = passed(&["eh3d-new", &f, "--lang", "en"]);
        assert!(made.contains("84 x 54 x 24.6 mm"), "{made}");
        assert!(call(&["eh3d-new", &f]).is_err());
        let first = passed(&["eh3d-check", &f, "--lang", "en"]);
        assert!(first.contains("has not changed"), "{first}");
        assert!(first.contains("has room"), "{first}");
        assert_eq!(
            passed(&["eh3d-openings", &f, "--apply", "--lang", "en"]),
            "J1: opening added\n"
        );
        assert!(passed(&["eh3d-openings", &f, "--lang", "es"]).contains("ya está"));
        let published = passed(&["eh3d-publish", &f, "--lang", "en"]);
        assert!(published.contains("cad/mounts2d.json"), "{published}");
        assert!(published.contains("docs/cad/report.txt"), "{published}");
        passed(&["eh3d-info", &f]);
        // the board program writes only its branch: nothing of it changed
        let pcb_after = {
            let mut p = eh3dfile::open_project(&file).unwrap();
            eh3dfile::branch_files(&mut p, SOURCE_BRANCH).unwrap()
        };
        assert_eq!(pcb_before, pcb_after);

        // the board moves a hole: the box is out of date until it takes the board again
        let mut moved = sample();
        moved.board.holes[0].x = mm(10);
        write_board(&file, &moved);
        let stale = failed(&["eh3d-check", &f, "--lang", "en"]);
        assert!(stale.contains("has changed since"), "{stale}");
        assert!(stale.contains("hole H1 moved"), "{stale}");
        let listed = failed(&["eh3d-info", &f]);
        assert!(listed.contains("stale:pcb"), "{listed}");
        assert!(listed.contains("EH3D-PKG-019"), "{listed}");
        let updated = passed(&["eh3d-board", &f, "--lang", "en"]);
        assert!(
            updated.contains("post H1: moved with its hole"),
            "{updated}"
        );
        passed(&["eh3d-check", &f]);
        passed(&["eh3d-info", &f]);

        // a part too tall for the box is found
        let mut tall = moved.clone();
        tall.parts[0].height = mm(20);
        write_board(&file, &tall);
        let found = failed(&["eh3d-check", &f, "--lang", "en"]);
        assert!(found.contains("C1: reaches the lid, 5 mm short"), "{found}");
    }

    #[test]
    fn a_project_goes_into_the_suite_file_and_comes_back_the_same() {
        let dir = Scratch::new("round");
        let source = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/box.phoenixproj");
        let file = dir.path("p.eh3d");
        let f = text(&file);
        passed(&["eh3d-save", source, &f]);
        let out = dir.path("back.phoenixproj");
        passed(&["eh3d-open", &f, "--out", &text(&out)]);
        assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(source).unwrap());
        assert!(call(&["eh3d-open", &f, "--out", &text(&out)]).is_err());
        // every command that reads a project reads the suite file too
        let names = passed(&["select", &f, "faces"]);
        assert_eq!(names.lines().count(), 6);
        assert_eq!(
            failed(&["select", &f, "holes[d>900mm]", "--lang", "en"]),
            "Nothing is selected\n"
        );
        let table = passed(&["params", &f]);
        assert_eq!(table.lines().count(), 9);
        assert!(table.starts_with("parameter,value_nm,"));
        let limits = dir.path("limits.cfg");
        std::fs::write(&limits, "wall.min = 1 m\n").unwrap();
        let breach = failed(&[
            "params",
            &f,
            "--limits",
            &text(&limits),
            "--text",
            "--lang",
            "en",
        ]);
        assert!(
            breach.contains("wall: below its smallest value"),
            "{breach}"
        );
        let drawing = dir.path("faces.svg");
        passed(&["panels", &f, &text(&drawing)]);
        assert!(std::fs::read_to_string(&drawing)
            .unwrap()
            .contains("<g id=\"top\">"));
        assert!(call(&["panels", &f, &text(&drawing)]).is_err());
        assert!(call(&["panels", &f, "faces.pdf"]).is_err());
        // a sealed branch takes nothing more
        passed(&["eh3d-save", source, &f, "--seal", "tester", "--time", "T1"]);
        let refused = call(&["eh3d-save", source, &f]).unwrap_err();
        assert!(refused.contains("PHOENIX-CAD-064"), "{refused}");
    }

    #[test]
    fn an_original_and_the_metadata_are_kept_in_the_suite_file() {
        let dir = Scratch::new("keepit");
        let file = dir.path("p.eh3d");
        let f = text(&file);
        let stl = dir.path("part.stl");
        std::fs::write(
            &stl,
            "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\n\
             vertex 0 1 0\nendloop\nendfacet\nendsolid t\n",
        )
        .unwrap();
        let out = passed(&["eh3d-import", &f, &text(&stl), "--lang", "en"]);
        assert!(out.contains("1 solids, 1 triangles"), "{out}");
        passed(&["eh3d-meta", &f, "--title", "Box", "--lang", "en"]);
        passed(&["eh3d-meta", &f, "--description", "Caja", "--lang", "en"]);
        assert!(call(&["eh3d-meta", &f]).is_err());
        passed(&["eh3d-info", &f, "--text", "--lang", "ja"]);
        assert!(call(&["eh3d-info", "p.phoenixproj"]).is_err());
        assert!(call(&["eh3d-nothing"]).is_err());
    }

    #[test]
    fn random_boards_go_through_the_commands_and_keep_agreeing() {
        let dir = Scratch::new("random");
        let mut random = Seeded(0x00CA_DB08);
        for round in 0..6 {
            let file = dir.path(&format!("r{round}.eh3d"));
            let f = text(&file);
            let mut board = sample();
            let width = 60 + random.below(100) as i64;
            let depth = 40 + random.below(60) as i64;
            board.board.outline = vec![
                (mm(0), mm(0)),
                (mm(width), mm(0)),
                (mm(width), mm(depth)),
                (mm(0), mm(depth)),
            ];
            let corners = [
                (4, 4),
                (width - 4, 4),
                (width - 4, depth - 4),
                (4, depth - 4),
            ];
            for (hole, (x, y)) in board.board.holes.iter_mut().zip(corners) {
                hole.x = mm(x);
                hole.y = mm(y);
            }
            board.parts.truncate(1);
            board.connectors[0].at = mm(width / 2);
            write_board(&file, &board);
            passed(&["eh3d-new", &f]);
            passed(&["eh3d-check", &f]);
            passed(&["eh3d-board", &f]);
            passed(&["eh3d-openings", &f, "--apply"]);
            passed(&["eh3d-check", &f]);
            passed(&["eh3d-info", &f]);
        }
    }
}
