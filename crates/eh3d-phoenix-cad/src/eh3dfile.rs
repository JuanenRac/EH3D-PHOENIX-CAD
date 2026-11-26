//! The branch `cad/` of a `.eh3d` project file of the suite, over the shared crate
//! `eh3d-project` (the format is described in the shared repository, `docs/FORMAT_EH3D.md`).
//!
//! This program writes only its own branches: `cad/` (the project file of this program, the
//! snapshot of the board it was derived from, the files it hands to other programs and the
//! originals of imports) and `docs/cad/` (documents about the box), besides the metadata of the
//! shared branch `common/`. Every other branch is read and kept byte for byte. Saving a branch
//! keeps the files of it that were not written again. A project, a branch or a file that cannot
//! be used is refused with `PHOENIX-CAD-064` (the file) or `PHOENIX-CAD-065` (the branch).
//! Nothing here reads the clock: times are given by the caller.

use std::collections::BTreeMap;
use std::path::Path;

use eh3d_container::{sha256_hex, to_canonical_file_bytes, AtomicOptions, ContainerError, PkgCode};
use eh3d_project::meta::{
    ConversionReport, Converter, Counted, DocIndex, DocKind, Document as DocEntry, LocalText,
    Metadata, SourceFile, DOC_INDEX_FILE, META_FILE, REPORT_FILE,
};
use eh3d_project::{
    BranchUpdate, Checks, Dependency, Finding, NewProject, OpenOptions, Project, SaveOptions, Seal,
    Writer,
};

use crate::{store, CadError};

/// The branch of this program.
pub const BRANCH: &str = "cad";

/// The branch of the documents of this program.
pub const DOCS_BRANCH: &str = "docs/cad";

/// The branch of the project as a whole.
pub const COMMON_BRANCH: &str = "common";

/// The name this program writes in the records of its branches.
pub const APP: &str = "cad";

/// The format of the branch `cad` that this program writes, as `major.minor`.
pub const FORMAT: &str = "1.0";

/// The project file of this program inside the branch `cad`.
pub const PROJECT_ENTRY: &str = "project.phoenixproj";

/// The extension of a project file of the suite.
pub const EXTENSION: &str = "eh3d";

/// The largest file of a branch that is read to be written back, in bytes.
const MAX_CARRIED: u64 = 1 << 30;

/// Whether a path names a project file of the suite (by its extension, in any letter case).
pub fn is_eh3d(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

fn suite(error: ContainerError) -> CadError {
    match error {
        ContainerError::Rejected(v) => {
            CadError::SuiteFile(format!("{} {}", v.code.code(), v.detail))
        }
        ContainerError::Io(e) => CadError::Io(format!("the project file of the suite: {e}")),
    }
}

fn branch_error(code: PkgCode, why: &str) -> CadError {
    CadError::SuiteBranch(format!("{} {why}", code.code()))
}

/// How a branch is written.
#[derive(Debug, Clone, Default)]
pub struct SaveSettings {
    /// The time of this write, as the caller writes it; `None` keeps the one of the file.
    pub modified: Option<String>,
    /// When given, a line with this time is added to the audit log.
    pub audit_at: Option<String>,
    /// Keep the file that is replaced as `<name>.bak`.
    pub keep_backup: bool,
    /// Seals the branch read-only with this write.
    pub seal: Option<Seal>,
}

impl SaveSettings {
    fn atomic(&self) -> AtomicOptions {
        AtomicOptions {
            keep_backup: self.keep_backup,
            ..AtomicOptions::default()
        }
    }

    fn options(&self) -> SaveOptions<'static> {
        SaveOptions {
            modified: self.modified.clone(),
            audit_at: self.audit_at.clone(),
            atomic: self.atomic(),
            signer: None,
        }
    }
}

/// Opens a project file of the suite, with every entry checked against its integrity record.
pub fn open_project(path: &Path) -> Result<Project, CadError> {
    Project::open(path, &OpenOptions::default()).map_err(suite)
}

/// The `major.minor` of a text, when it is one.
fn major_minor(text: &str) -> Option<(u64, u64)> {
    let (major, minor) = text.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// Refuses a branch that this version cannot read or write back: encrypted, or (for the branches
/// of this program) of a newer format.
fn usable(project: &Project, id: &str) -> Result<(), CadError> {
    let Some(branch) = project.branch(id) else {
        return Ok(());
    };
    if branch.is_encrypted() {
        return Err(branch_error(
            PkgCode::EncryptedBranch,
            &format!("the branch {id} is encrypted"),
        ));
    }
    if id != BRANCH && id != DOCS_BRANCH {
        return Ok(());
    }
    let Some(record) = &branch.record else {
        return Err(branch_error(
            PkgCode::BranchRecord,
            &format!("the branch {id} has no record that can be read"),
        ));
    };
    match major_minor(&record.format) {
        Some((1, 0)) => Ok(()),
        _ => Err(branch_error(
            PkgCode::BranchVersion,
            &format!(
                "the branch {id} has the format {}, newer than {FORMAT}",
                record.format
            ),
        )),
    }
}

/// The bytes of one file of any branch, which every program may read, or `None` when the branch
/// or the file is not there. A file larger than `max_bytes` is refused.
pub fn read_file(
    project: &mut Project,
    branch: &str,
    entry: &str,
    max_bytes: u64,
) -> Result<Option<Vec<u8>>, CadError> {
    let Some(size) = project
        .branch(branch)
        .and_then(|b| b.files.get(entry))
        .map(|f| f.size)
    else {
        return Ok(None);
    };
    if size > max_bytes {
        return Err(CadError::Io(format!(
            "{branch}/{entry} has {size} bytes, more than the {max_bytes} allowed"
        )));
    }
    project.read_entry(branch, entry).map(Some).map_err(suite)
}

/// The project file of this program that a project of the suite holds.
pub fn read_cad_project(path: &Path, max_bytes: u64) -> Result<Vec<u8>, CadError> {
    let mut project = open_project(path)?;
    usable(&project, BRANCH)?;
    read_file(&mut project, BRANCH, PROJECT_ENTRY, max_bytes)?.ok_or_else(|| {
        branch_error(
            PkgCode::BranchRef,
            "the file has no project in the branch cad (save one with eh3d-save)",
        )
    })
}

/// The bytes of a project: of the branch `cad` of a `.eh3d` file, or of the file itself.
pub fn load_project_bytes(path: &Path, max_bytes: u64) -> Result<Vec<u8>, CadError> {
    if is_eh3d(path) {
        read_cad_project(path, max_bytes)
    } else {
        store::load_limited(path, max_bytes)
    }
}

/// Every file of a branch but its record, with its bytes. A branch that is not there is empty.
pub fn branch_files(
    project: &mut Project,
    id: &str,
) -> Result<BTreeMap<String, Vec<u8>>, CadError> {
    usable(project, id)?;
    let names: Vec<String> = match project.branch(id) {
        Some(branch) => branch
            .files
            .keys()
            .filter(|n| n.as_str() != "branch.json")
            .cloned()
            .collect(),
        None => Vec::new(),
    };
    let mut out = BTreeMap::new();
    for name in names {
        if let Some(bytes) = read_file(project, id, &name, MAX_CARRIED)? {
            out.insert(name, bytes);
        }
    }
    Ok(out)
}

/// Writes changes into a branch of this program (or the shared one): `Some` writes a file, `None`
/// takes it away, and every file that is not named stays. `depends` replaces the references of
/// the branch; `None` keeps the ones it has. The project file must exist.
pub fn write_branch(
    path: &Path,
    id: &str,
    changes: BTreeMap<String, Option<Vec<u8>>>,
    depends: Option<Vec<Dependency>>,
    settings: &SaveSettings,
) -> Result<(), CadError> {
    let mut project = open_project(path)?;
    let mut entries = branch_files(&mut project, id)?;
    for (name, change) in changes {
        match change {
            Some(bytes) => {
                entries.insert(name, bytes);
            }
            None => {
                entries.remove(&name);
            }
        }
    }
    let old = project.branch(id).and_then(|b| b.record.clone());
    let format = old
        .as_ref()
        .map_or_else(|| FORMAT.to_owned(), |r| r.format.clone());
    let mut update = BranchUpdate::new(id, &format, env!("CARGO_PKG_VERSION"));
    update.entries = entries;
    update.depends = depends.unwrap_or_else(|| {
        old.as_ref()
            .map_or_else(Vec::new, |record| record.depends.clone())
    });
    update.seal = settings.seal.clone();
    update.extra = old.map_or_else(BTreeMap::new, |record| record.extra);
    project
        .save_branch(&Writer::for_app(APP), update, &settings.options())
        .map_err(suite)?;
    Ok(())
}

/// Makes the project file of the suite when it does not exist. Its identifier comes from the
/// first bytes it is given and its name from the file name, so the same input makes the same
/// file.
pub fn ensure_project(path: &Path, seed: &[u8], settings: &SaveSettings) -> Result<(), CadError> {
    if path.exists() {
        return Ok(());
    }
    let name = path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let digest = sha256_hex(seed);
    let new = NewProject {
        id: format!("cad-{}", digest.get(..32).unwrap_or(&digest)),
        name: if name.trim().is_empty() {
            "project".to_owned()
        } else {
            name
        },
        units: "millimetres".to_owned(),
        created: settings.modified.clone(),
    };
    Project::create(path, new, &OpenOptions::default(), &settings.atomic()).map_err(suite)?;
    Ok(())
}

/// Saves the project file of this program in the branch `cad`, making the file of the suite when
/// it is not there. The other branches keep their bytes, and so do the other files of `cad`.
pub fn save_cad_project(
    path: &Path,
    document: &[u8],
    settings: &SaveSettings,
) -> Result<(), CadError> {
    ensure_project(path, document, settings)?;
    let changes = BTreeMap::from([(PROJECT_ENTRY.to_owned(), Some(document.to_vec()))]);
    write_branch(path, BRANCH, changes, None, settings)
}

/// One branch of a project, as `eh3d-info` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchLine {
    /// The name of the branch.
    pub id: String,
    /// The program that wrote it.
    pub app: String,
    /// The format of the branch.
    pub format: String,
    /// How many files it holds, without its record.
    pub files: usize,
    /// How many bytes they are.
    pub bytes: u64,
    /// Whether it is sealed read-only.
    pub sealed: bool,
    /// Whether it is encrypted.
    pub encrypted: bool,
    /// The branches it was derived from that have changed since.
    pub stale: Vec<String>,
}

/// What a project of the suite holds.
#[derive(Debug, Clone)]
pub struct Summary {
    /// The name of the project.
    pub name: String,
    /// The branches, by name.
    pub branches: Vec<BranchLine>,
    /// What the checks of the format found (errors and warnings).
    pub findings: Vec<Finding>,
}

/// Lists the branches of a project and checks it.
pub fn summary(path: &Path) -> Result<Summary, CadError> {
    let mut project = open_project(path)?;
    let stale = project.stale_refs();
    let branches = project
        .branches()
        .map(|b| BranchLine {
            id: b.id.clone(),
            app: b
                .record
                .as_ref()
                .map_or_else(String::new, |r| r.app.clone()),
            format: b
                .record
                .as_ref()
                .map_or_else(String::new, |r| r.format.clone()),
            files: b
                .files
                .keys()
                .filter(|n| n.as_str() != "branch.json")
                .count(),
            bytes: b
                .files
                .iter()
                .filter(|(n, _)| n.as_str() != "branch.json")
                .map(|(_, f)| f.size)
                .sum(),
            sealed: b.is_sealed(),
            encrypted: b.is_encrypted(),
            stale: stale
                .iter()
                .filter(|s| s.branch == b.id)
                .map(|s| s.dependency.branch.clone())
                .collect(),
        })
        .collect();
    let name = project.record().name.clone();
    let findings = project.diagnose(&Checks::default());
    Ok(Summary {
        name,
        branches,
        findings,
    })
}

/// The code of the language of a text of the project (`pt` is written `pt-PT`).
pub fn language_key(code: &str) -> &str {
    if code == "pt" {
        "pt-PT"
    } else {
        code
    }
}

fn record_bytes<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, CadError> {
    to_canonical_file_bytes(value).map_err(|e| CadError::SuiteFile(e.to_string()))
}

/// Sets the title and the description of the project in one language, keeping the other
/// languages and the other metadata. A text needs its English version, so a project without one
/// is given English first.
pub fn set_metadata(
    path: &Path,
    language: &str,
    title: Option<&str>,
    description: Option<&str>,
    settings: &SaveSettings,
) -> Result<Metadata, CadError> {
    let mut project = open_project(path)?;
    let mut metadata = match read_file(&mut project, COMMON_BRANCH, META_FILE, 1 << 24)? {
        Some(bytes) => serde_json::from_slice::<Metadata>(&bytes)
            .map_err(|e| CadError::SuiteFile(format!("{META_FILE}: {e}")))?,
        None => Metadata {
            schema: 1,
            ..Metadata::default()
        },
    };
    let key = language_key(language).to_owned();
    if let Some(text) = title {
        metadata.title.insert(key.clone(), text.to_owned());
    }
    if let Some(text) = description {
        metadata.description.insert(key, text.to_owned());
    }
    metadata.validate().map_err(CadError::SuiteFile)?;
    let changes = BTreeMap::from([(META_FILE.to_owned(), Some(record_bytes(&metadata)?))]);
    write_branch(path, COMMON_BRANCH, changes, None, settings)?;
    Ok(metadata)
}

/// A document about the box to keep in `docs/cad`.
#[derive(Debug, Clone)]
pub struct DocFile {
    /// The name inside the branch; its extension says the kind.
    pub name: String,
    /// The title in the languages of the project.
    pub title: LocalText,
    /// The content.
    pub bytes: Vec<u8>,
}

/// Writes documents into `docs/cad` and their index. A document of the same name is replaced and
/// the others stay listed.
pub fn write_documents(
    path: &Path,
    documents: &[DocFile],
    settings: &SaveSettings,
) -> Result<(), CadError> {
    let mut project = open_project(path)?;
    usable(&project, DOCS_BRANCH)?;
    let mut index = match read_file(&mut project, DOCS_BRANCH, DOC_INDEX_FILE, 1 << 24)? {
        Some(bytes) => serde_json::from_slice::<DocIndex>(&bytes)
            .map_err(|e| CadError::SuiteFile(format!("{DOC_INDEX_FILE}: {e}")))?,
        None => DocIndex {
            schema: 1,
            ..DocIndex::default()
        },
    };
    let mut changes: BTreeMap<String, Option<Vec<u8>>> = BTreeMap::new();
    for document in documents {
        let kind = DocKind::of_name(&document.name).ok_or_else(|| {
            CadError::SuiteFile(format!("{}: not a kind of document", document.name))
        })?;
        index.documents.retain(|d| d.path != document.name);
        index.documents.push(DocEntry {
            path: document.name.clone(),
            kind,
            title: document.title.clone(),
            lang: None,
            translation_of: None,
            extra: BTreeMap::new(),
        });
        changes.insert(document.name.clone(), Some(document.bytes.clone()));
    }
    index.documents.sort_by(|a, b| a.path.cmp(&b.path));
    changes.insert(DOC_INDEX_FILE.to_owned(), Some(record_bytes(&index)?));
    write_branch(path, DOCS_BRANCH, changes, None, settings)
}

/// What an import read and did not read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportCount {
    /// What was read, with how many.
    pub read: Vec<(String, u64)>,
    /// What was left out, with how many.
    pub not_read: Vec<(String, u64)>,
}

fn counted(list: &[(String, u64)], reason: Option<&str>) -> Vec<Counted> {
    list.iter()
        .map(|(item, count)| Counted {
            item: item.clone(),
            count: *count,
            reason: reason.map(str::to_owned),
        })
        .collect()
}

/// Keeps the original file of another program in `cad/origin/` with the report of what was read
/// of it. The branch holds one original: an older one is taken away.
pub fn keep_origin(
    path: &Path,
    format: &str,
    file_name: &str,
    bytes: &[u8],
    count: &ImportCount,
    settings: &SaveSettings,
) -> Result<ConversionReport, CadError> {
    ensure_project(path, bytes, settings)?;
    let mut project = open_project(path)?;
    usable(&project, BRANCH)?;
    let mut changes: BTreeMap<String, Option<Vec<u8>>> = BTreeMap::new();
    if let Some(old) = read_file(&mut project, BRANCH, REPORT_FILE, 1 << 24)? {
        if let Some(original) = serde_json::from_slice::<ConversionReport>(&old)
            .ok()
            .and_then(|r| r.original)
        {
            changes.insert(original, None);
        }
    }
    let original = format!("origin/{file_name}");
    let report = ConversionReport {
        schema: 1,
        source: SourceFile {
            format: format.to_owned(),
            name: file_name.to_owned(),
            sha256: sha256_hex(bytes),
            size: bytes.len() as u64,
        },
        converter: Converter {
            name: "eh3d-phoenix-cad".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        original: Some(original.clone()),
        read: counted(&count.read, None),
        not_read: counted(&count.not_read, Some("not understood by this program")),
        export_losses: Vec::new(),
        extra: BTreeMap::new(),
    };
    changes.insert(original, Some(bytes.to_vec()));
    changes.insert(REPORT_FILE.to_owned(), Some(record_bytes(&report)?));
    write_branch(path, BRANCH, changes, None, settings)?;
    Ok(report)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    pub(crate) struct Scratch(pub(crate) PathBuf);

    impl Scratch {
        pub(crate) fn new(tag: &str) -> Scratch {
            static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "phoenix-cad-suite-{tag}-{}-{n}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        pub(crate) fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A small generator with a fixed seed.
    pub(crate) struct Seeded(pub(crate) u64);

    impl Seeded {
        pub(crate) fn below(&mut self, n: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) % n.max(1)
        }

        pub(crate) fn bytes(&mut self, len: usize) -> Vec<u8> {
            (0..len).map(|_| self.below(256) as u8).collect()
        }
    }

    /// Writes a branch of another program, as that program would.
    pub(crate) fn other_branch(path: &Path, id: &str, name: &str, bytes: &[u8]) {
        ensure_project(path, b"seed", &SaveSettings::default()).unwrap();
        let project = open_project(path).unwrap();
        let update = BranchUpdate::new(id, "1.0", "0.1.0").with_entry(name, bytes);
        project
            .save_branch(&Writer::for_app(id), update, &SaveOptions::default())
            .unwrap();
    }

    fn snapshot(path: &Path, id: &str) -> BTreeMap<String, Vec<u8>> {
        let mut project = open_project(path).unwrap();
        branch_files(&mut project, id).unwrap()
    }

    #[test]
    fn the_extension_is_seen_in_any_letter_case() {
        assert!(is_eh3d(Path::new("a.eh3d")));
        assert!(is_eh3d(Path::new("dir/a.EH3D")));
        assert!(!is_eh3d(Path::new("a.phoenixproj")));
        assert!(!is_eh3d(Path::new("eh3d")));
        assert_eq!(language_key("pt"), "pt-PT");
        assert_eq!(language_key("es"), "es");
    }

    #[test]
    fn saving_the_project_of_this_program_keeps_every_other_branch_byte_for_byte() {
        let dir = Scratch::new("keep");
        let file = dir.path("p.eh3d");
        let mut random = Seeded(0x00CA_DB01);
        for id in ["pcb", "laser", "3dp"] {
            other_branch(&file, id, "data.bin", &random.bytes(200));
        }
        let before: Vec<_> = ["pcb", "laser", "3dp"]
            .iter()
            .map(|id| snapshot(&file, id))
            .collect();
        for round in 0..12 {
            let document = random.bytes(1 + round * 37);
            save_cad_project(&file, &document, &SaveSettings::default()).unwrap();
            assert_eq!(read_cad_project(&file, 1 << 20).unwrap(), document);
            let extra = BTreeMap::from([(format!("export/{round}.bin"), Some(random.bytes(9)))]);
            write_branch(&file, BRANCH, extra, None, &SaveSettings::default()).unwrap();
        }
        let after: Vec<_> = ["pcb", "laser", "3dp"]
            .iter()
            .map(|id| snapshot(&file, id))
            .collect();
        assert_eq!(before, after);
        // the project and the twelve files handed to other programs are all still there
        assert_eq!(snapshot(&file, BRANCH).len(), 13);
        let found = summary(&file).unwrap();
        assert!(found.findings.is_empty(), "{:?}", found.findings);
        assert_eq!(found.branches.len(), 4);
    }

    #[test]
    fn the_same_content_makes_the_same_file() {
        let dir = Scratch::new("same");
        let (a, b) = (dir.path("a"), dir.path("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        for folder in [&a, &b] {
            save_cad_project(&folder.join("p.eh3d"), b"text", &SaveSettings::default()).unwrap();
        }
        assert_eq!(
            fs::read(a.join("p.eh3d")).unwrap(),
            fs::read(b.join("p.eh3d")).unwrap()
        );
    }

    #[test]
    fn a_file_without_the_branch_or_not_of_the_suite_is_refused_with_its_code() {
        let dir = Scratch::new("refuse");
        let file = dir.path("p.eh3d");
        other_branch(&file, "pcb", "a.txt", b"a");
        let error = read_cad_project(&file, 100).unwrap_err();
        assert_eq!(error.code(), "PHOENIX-CAD-065", "{error}");
        save_cad_project(&file, b"0123456789", &SaveSettings::default()).unwrap();
        assert_eq!(
            read_cad_project(&file, 9).unwrap_err().code(),
            "PHOENIX-CAD-012"
        );
        let junk = dir.path("junk.eh3d");
        fs::write(&junk, b"not a zip").unwrap();
        assert_eq!(
            read_cad_project(&junk, 100).unwrap_err().code(),
            "PHOENIX-CAD-064"
        );
    }

    #[test]
    fn damaged_project_files_never_panic() {
        let dir = Scratch::new("damaged");
        let file = dir.path("p.eh3d");
        save_cad_project(&file, b"some project", &SaveSettings::default()).unwrap();
        other_branch(&file, "pcb", "outline2d.json", b"{}");
        let good = fs::read(&file).unwrap();
        let mut random = Seeded(0x00CA_DB02);
        let broken = dir.path("broken.eh3d");
        for _ in 0..250 {
            let mut bytes = good.clone();
            for _ in 0..=random.below(4) {
                let at = random.below(bytes.len() as u64) as usize;
                bytes[at] ^= 1 << random.below(8);
            }
            if random.below(4) == 0 {
                bytes.truncate(random.below(bytes.len() as u64) as usize);
            }
            fs::write(&broken, &bytes).unwrap();
            // an answer or a refusal, never a panic; what is read is what was saved
            if let Ok(read) = read_cad_project(&broken, 1 << 20) {
                assert_eq!(read, b"some project");
            }
            let _ = summary(&broken);
        }
    }

    #[test]
    fn a_sealed_branch_is_not_written_again() {
        let dir = Scratch::new("seal");
        let file = dir.path("p.eh3d");
        let sealed = SaveSettings {
            seal: Some(Seal {
                by: "tester".to_owned(),
                at: "T1".to_owned(),
                reason: None,
            }),
            ..SaveSettings::default()
        };
        save_cad_project(&file, b"final", &sealed).unwrap();
        let again = save_cad_project(&file, b"later", &SaveSettings::default()).unwrap_err();
        assert_eq!(again.code(), "PHOENIX-CAD-064", "{again}");
        assert!(again.to_string().contains("EH3D-PKG-020"), "{again}");
        assert_eq!(read_cad_project(&file, 100).unwrap(), b"final");
        assert!(summary(&file).unwrap().branches[0].sealed);
    }

    #[test]
    fn the_audit_log_and_the_backup_are_written_only_when_asked() {
        let dir = Scratch::new("audit");
        let file = dir.path("p.eh3d");
        save_cad_project(&file, b"one", &SaveSettings::default()).unwrap();
        assert!(open_project(&file).unwrap().audit_entries().is_empty());
        assert!(!dir.path("p.eh3d.bak").exists());
        let asked = SaveSettings {
            audit_at: Some("T2".to_owned()),
            keep_backup: true,
            ..SaveSettings::default()
        };
        save_cad_project(&file, b"two", &asked).unwrap();
        assert_eq!(open_project(&file).unwrap().audit_entries().len(), 1);
        assert!(dir.path("p.eh3d.bak").exists());
    }

    #[test]
    fn metadata_keeps_the_other_languages_and_needs_english() {
        let dir = Scratch::new("meta");
        let file = dir.path("p.eh3d");
        save_cad_project(&file, b"one", &SaveSettings::default()).unwrap();
        let none = SaveSettings::default();
        assert!(set_metadata(&file, "es", Some("Caja"), None, &none).is_err());
        set_metadata(&file, "en", Some("Box"), Some("A box"), &none).unwrap();
        set_metadata(&file, "es", Some("Caja"), None, &none).unwrap();
        let all = set_metadata(&file, "pt", Some("Caixa"), None, &none).unwrap();
        assert_eq!(all.title.get("en").map(String::as_str), Some("Box"));
        assert_eq!(all.title.get("es").map(String::as_str), Some("Caja"));
        assert_eq!(all.title.get("pt-PT").map(String::as_str), Some("Caixa"));
        assert_eq!(all.description.get("en").map(String::as_str), Some("A box"));
        assert!(summary(&file).unwrap().findings.is_empty());
    }

    #[test]
    fn documents_are_listed_once_and_the_origin_of_an_import_is_replaced() {
        let dir = Scratch::new("docs");
        let file = dir.path("p.eh3d");
        let none = SaveSettings::default();
        save_cad_project(&file, b"one", &none).unwrap();
        let title: LocalText = BTreeMap::from([("en".to_owned(), "Bill".to_owned())]);
        let document = |bytes: &[u8]| DocFile {
            name: "bill.txt".to_owned(),
            title: title.clone(),
            bytes: bytes.to_vec(),
        };
        write_documents(&file, &[document(b"1")], &none).unwrap();
        write_documents(&file, &[document(b"2")], &none).unwrap();
        let docs = snapshot(&file, DOCS_BRANCH);
        assert_eq!(docs.get("bill.txt").map(Vec::as_slice), Some(&b"2"[..]));
        assert_eq!(docs.len(), 2);
        let count = ImportCount {
            read: vec![("triangles".to_owned(), 12)],
            not_read: Vec::new(),
        };
        keep_origin(&file, "stl", "a.stl", b"first", &count, &none).unwrap();
        let report = keep_origin(&file, "stl", "b.stl", b"second", &count, &none).unwrap();
        assert_eq!(report.source.size, 6);
        let cad = snapshot(&file, BRANCH);
        assert!(cad.contains_key("origin/b.stl") && !cad.contains_key("origin/a.stl"));
        assert!(cad.contains_key(PROJECT_ENTRY));
        let found = summary(&file).unwrap();
        assert!(found.findings.is_empty(), "{:?}", found.findings);
    }
}
