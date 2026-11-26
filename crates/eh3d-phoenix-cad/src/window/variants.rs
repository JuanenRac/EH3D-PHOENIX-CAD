// SPDX-License-Identifier: GPL-3.0-or-later
//! The variants of the project in the window and in the command `variants`: the current values of
//! the variables kept under a name, a variant applied, checked or exported, all without a screen.
//!
//! Keeping, changing, applying and removing a variant are changes of the project like any other:
//! one entry of the history each, saved with the project. Checking and exporting look at each
//! variant on a copy of the project, so neither changes it.

use std::path::PathBuf;

use super::*;

/// A variant as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantRow {
    /// Its name.
    pub name: String,
    /// Its values in the unit of the project, `name = value` separated by commas.
    pub values: String,
    /// The variables it names that the project does not have.
    pub missing: Vec<String>,
}

/// What a variant gives when the box is built with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantCheck {
    /// The name of the variant.
    pub name: String,
    /// The sizes, the steps refused and the fit with the board, or why it cannot be built.
    pub text: String,
    /// False when the variant cannot be built, a step is refused or the board does not fit.
    pub ok: bool,
}

/// A file written for a variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantFile {
    /// The name of the variant.
    pub name: String,
    /// Where it was written.
    pub path: PathBuf,
    /// Its bytes.
    pub bytes: usize,
}

impl Window {
    /// The variants of the project, in the order of their names.
    pub fn variant_rows(&self) -> Vec<VariantRow> {
        let set = &self.mech.variants;
        set.names()
            .into_iter()
            .filter_map(|name| {
                let values = set.values(name).ok()?;
                let text: Vec<String> = values
                    .iter()
                    .map(|(variable, value)| format!("{variable} = {}", self.unit_text(*value)))
                    .collect();
                Some(VariantRow {
                    name: name.to_string(),
                    values: text.join(", "),
                    missing: values
                        .keys()
                        .filter(|v| self.mech.vars.get(v).is_none())
                        .cloned()
                        .collect(),
                })
            })
            .collect()
    }

    /// Keeps the current values of the variables as the variant `name`, replacing the one of that
    /// name. `only` names the variables it holds, separated by commas or spaces; empty holds them
    /// all. A name that is not a plain word, a variable the project does not have, a project with
    /// no variable and a limit of `[variants]` are refused, and nothing changes.
    pub fn keep_variant(&mut self, name: &str, only: &str) -> bool {
        if self.read_only_refusal() {
            return false;
        }
        let result = self.variant_keeping(name.trim(), only);
        self.variant_done(result)
    }

    fn variant_keeping(&mut self, name: &str, only: &str) -> Result<(), CadError> {
        let mut wanted: Vec<String> = only
            .split([',', ' ', ';'])
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect();
        if wanted.is_empty() {
            wanted = self
                .mech
                .vars
                .names()
                .into_iter()
                .map(str::to_string)
                .collect();
        }
        let mut values = std::collections::BTreeMap::new();
        for variable in wanted {
            let value = self.mech.vars.require(&variable)?;
            values.insert(variable, value);
        }
        let count = values.len().to_string();
        let mut next = self.mech.clone();
        next.variants.put(name, values, &self.settings.variants)?;
        let title = fill(Msg::VariantKept.tr(self.lang), &[name, &count]);
        let done = Status::Done(Msg::VariantKept, vec![name.to_string(), count]);
        self.variant_store(&title, done, next)
    }

    /// Records the change and says it even when the project already was like that.
    fn variant_store(
        &mut self,
        title: &str,
        done: Status,
        next: Mechanical,
    ) -> Result<(), CadError> {
        self.change(title, done.clone(), next, None)?;
        self.status = done;
        Ok(())
    }

    /// Changes one value of a variant (a length or a formula, as in the sheet of variables), or
    /// adds the variable to the variant. The variable must be one of the project.
    pub fn set_variant_value(&mut self, name: &str, variable: &str, text: &str) -> bool {
        if self.read_only_refusal() {
            return false;
        }
        let result = self.variant_value_change(name.trim(), variable.trim(), text);
        self.variant_done(result)
    }

    fn variant_value_change(
        &mut self,
        name: &str,
        variable: &str,
        text: &str,
    ) -> Result<(), CadError> {
        self.mech.vars.require(variable)?;
        let value = self.variable_value(text)?;
        let mut values = self.mech.variants.values(name)?.clone();
        values.insert(variable.to_string(), value);
        let mut next = self.mech.clone();
        next.variants.put(name, values, &self.settings.variants)?;
        let shown = vec![
            name.to_string(),
            variable.to_string(),
            self.unit_text(value),
        ];
        let title = fill(
            Msg::VariantValueSet.tr(self.lang),
            &[&shown[0], &shown[1], &shown[2]],
        );
        self.variant_store(&title, Status::Done(Msg::VariantValueSet, shown), next)
    }

    /// Takes a variant away. The variables keep the values they have.
    pub fn remove_variant(&mut self, name: &str) -> bool {
        if self.read_only_refusal() {
            return false;
        }
        let result = self.variant_removal(name);
        self.variant_done(result)
    }

    fn variant_removal(&mut self, name: &str) -> Result<(), CadError> {
        let mut next = self.mech.clone();
        next.variants.remove(name)?;
        let title = fill(Msg::VariantRemoved.tr(self.lang), &[name]);
        let done = Status::Done(Msg::VariantRemoved, vec![name.to_string()]);
        self.variant_store(&title, done, next)
    }

    /// Writes the values of a variant into the sheet of variables, as one change of the history.
    /// A variant that names a variable the project does not have, or whose values the model does
    /// not accept, is refused and nothing changes.
    pub fn apply_variant(&mut self, name: &str) -> bool {
        if self.read_only_refusal() {
            return false;
        }
        let result = self.variant_application(name);
        self.variant_done(result)
    }

    fn variant_application(&mut self, name: &str) -> Result<(), CadError> {
        let mut next = self.mech.clone();
        next.vars = self.mech.variants.variables_for(name, &self.mech.vars)?;
        let title = fill(Msg::VariantApplied.tr(self.lang), &[name]);
        let done = Status::Done(Msg::VariantApplied, vec![name.to_string()]);
        self.variant_store(&title, done, next)
    }

    fn variant_done(&mut self, result: Result<(), CadError>) -> bool {
        let ok = result.is_ok();
        self.finish(result);
        ok
    }

    /// A copy of the window with the values of a variant, built. The copy has no history and
    /// names the file `<project>-<variant>`, which is what the titles of its drawings say.
    pub fn variant_window(&self, name: &str) -> Result<Window, CadError> {
        let vars = self.mech.variants.variables_for(name, &self.mech.vars)?;
        let mut copy = self.clone();
        copy.mech.vars = vars;
        copy.built = copy.rebuild(&copy.mech)?;
        copy.history.clear();
        copy.drag = None;
        copy.path = self.path.as_ref().map(|p| {
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            p.with_file_name(format!("{stem}-{name}.phoenixproj"))
        });
        copy.refresh();
        Ok(copy)
    }

    /// Builds the box with each variant and says its sizes, the steps the model refused and, when
    /// the project has a board, whether it fits. The project does not change.
    pub fn check_variants(&self) -> Vec<VariantCheck> {
        let lang = self.lang;
        self.mech
            .variants
            .names()
            .into_iter()
            .map(|name| match self.variant_window(name) {
                Err(e) => VariantCheck {
                    name: name.to_string(),
                    text: error_text(&e, lang),
                    ok: false,
                },
                Ok(copy) => {
                    let info = &copy.info;
                    let size: Vec<String> = info.outer.iter().map(|l| mm_text(l.get())).collect();
                    let mut words = vec![fill(
                        Msg::VariantSize.tr(lang),
                        &[&size[0], &size[1], &size[2]],
                    )];
                    words.extend(copy.failed_text());
                    let mut ok = info.failed == 0;
                    if info.fit_from_board {
                        words.push(copy.fit_text());
                        ok &= info.fit.fits || info.fit.fits_turned;
                    }
                    VariantCheck {
                        name: name.to_string(),
                        text: words.join("; "),
                        ok,
                    }
                }
            })
            .collect()
    }

    /// The name of the file of a variant: the one suggested for the project with the name of the
    /// variant after its stem (`box-small.stl`).
    pub fn variant_file_name(&self, name: &str, format: Format) -> String {
        let suggested = export::suggested_name(self.path.as_deref(), format);
        match suggested.split_once('.') {
            Some((stem, extension)) => format!("{stem}-{name}.{extension}"),
            None => format!("{suggested}-{name}"),
        }
    }

    /// Writes every variant in a format into `folder`, one file each, all or none: every variant
    /// is built and rendered and every target is checked before the first file is written. A
    /// variant that cannot be built, has a refused step (its file would not be the box asked for)
    /// or has a shape the format does not draw stops the whole export, and so does a file that
    /// exists when `overwrite` is false. The error names the variant. The project does not change.
    pub fn export_variants(
        &self,
        format: Format,
        folder: &Path,
        overwrite: bool,
    ) -> Result<Vec<VariantFile>, (String, CadError)> {
        let mut ready = Vec::new();
        for name in self.mech.variants.names() {
            let fail = |e: CadError| (name.to_string(), e);
            let copy = self.variant_window(name).map_err(fail)?;
            if copy.info.failed > 0 {
                return Err(fail(CadError::Export(format!(
                    "the variant {name} has {} refused steps",
                    copy.info.failed
                ))));
            }
            let bytes = copy.export_bytes(format).map_err(fail)?;
            if bytes.len() as u64 > u64::from(self.ui.export.max_kib) * 1024 {
                return Err(fail(CadError::Export(format!(
                    "the file has {} bytes, more than the {} KiB allowed",
                    bytes.len(),
                    self.ui.export.max_kib
                ))));
            }
            let path = folder.join(self.variant_file_name(name, format));
            export::check_target(&path, overwrite, self.path.as_deref()).map_err(fail)?;
            ready.push((name.to_string(), path, bytes));
        }
        ready
            .into_iter()
            .map(
                |(name, path, bytes)| -> Result<VariantFile, (String, CadError)> {
                    let written = export::write_bytes(
                        &path,
                        &bytes,
                        overwrite,
                        self.path.as_deref(),
                        self.ui.export.max_kib,
                    )
                    .map_err(|e| (name.clone(), e))?;
                    Ok(VariantFile {
                        name,
                        path,
                        bytes: written,
                    })
                },
            )
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Folder {
            let dir = std::env::temp_dir().join(format!(
                "phoenix-cad-variants-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Folder(dir)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A new project whose inner width follows the variable `width`, with a small and a large
    /// variant.
    fn family() -> Window {
        let mut w = Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap();
        w.add_variable("width", "60 mm");
        w.mech
            .model
            .set(Param::InnerWidth, Parameter::Variable("width".to_string()))
            .unwrap();
        w.built = w.rebuild(&w.mech).unwrap();
        w.refresh();
        assert!(w.keep_variant("small", ""), "{:?}", w.status);
        assert!(w.keep_variant("large", "width"), "{:?}", w.status);
        assert!(
            w.set_variant_value("large", "width", "90 mm"),
            "{:?}",
            w.status
        );
        w
    }

    #[test]
    fn a_variant_is_kept_changed_applied_and_undone() {
        let mut w = family();
        let rows = w.variant_rows();
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["large", "small"]
        );
        assert!(
            rows[0].values.starts_with("width = 90"),
            "{}",
            rows[0].values
        );
        assert!(rows.iter().all(|r| r.missing.is_empty()));
        assert_eq!(w.params().inner_width, mm(60));
        assert!(w.apply_variant("large"));
        assert_eq!(w.params().inner_width, mm(90));
        assert_eq!(w.mech.vars.get("width"), Some(mm(90)));
        w.undo();
        assert_eq!(w.params().inner_width, mm(60));
        w.redo();
        assert_eq!(w.params().inner_width, mm(90));
        // applying the variant that is already there changes nothing and still says so
        let entries = w.undo_names().len();
        assert!(w.apply_variant("large"));
        assert_eq!(w.undo_names().len(), entries);
        assert!(w.remove_variant("small"));
        assert_eq!(w.variant_rows().len(), 1);
        assert!(!w.remove_variant("small"));
        assert!(matches!(w.status, Status::Problem(..)));
    }

    #[test]
    fn what_a_variant_cannot_hold_is_refused_and_changes_nothing() {
        let mut w = family();
        let before = w.mech.clone();
        assert!(!w.keep_variant("two words", ""));
        assert!(!w.keep_variant("x", "depth"));
        assert!(!w.set_variant_value("large", "depth", "1 mm"));
        assert!(!w.set_variant_value("none", "width", "1 mm"));
        assert!(!w.set_variant_value("large", "width", "wide"));
        assert!(!w.apply_variant("none"));
        assert_eq!(w.mech, before);
        // a variant whose values the model does not accept is not applied
        assert!(w.set_variant_value("large", "width", "-5 mm"));
        let before = w.mech.clone();
        assert!(!w.apply_variant("large"));
        assert_eq!(w.mech, before);
        let mut empty =
            Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap();
        assert!(!empty.keep_variant("small", ""));
        empty.read_only = true;
        assert!(!empty.keep_variant("small", ""));
        assert!(!empty.apply_variant("small"));
        assert!(!empty.remove_variant("small"));
    }

    #[test]
    fn the_check_says_each_variant_without_changing_the_project() {
        let mut w = family();
        let before = w.mech.clone();
        let checks = w.check_variants();
        assert_eq!(checks.len(), 2);
        assert!(checks.iter().all(|c| c.ok), "{checks:?}");
        assert_ne!(checks[0].text, checks[1].text);
        assert_eq!(w.mech, before);
        assert!(w.set_variant_value("large", "width", "-5 mm"));
        let checks = w.check_variants();
        assert!(!checks[0].ok);
        assert!(
            checks[0].text.contains("PHOENIX-CAD-"),
            "{}",
            checks[0].text
        );
        assert!(checks[1].ok);
    }

    #[test]
    fn a_variant_that_names_a_variable_that_is_gone_is_told() {
        let mut w = family();
        w.add_variable("spare", "1 mm");
        assert!(w.set_variant_value("small", "spare", "2 mm"));
        w.remove_variable("spare");
        let rows = w.variant_rows();
        assert_eq!(rows[1].missing, vec!["spare".to_string()]);
        assert!(!w.apply_variant("small"));
        assert!(!w.check_variants()[1].ok);
    }

    #[test]
    fn every_variant_is_exported_or_none_is() {
        let w = family();
        let folder = Folder::new("export");
        let files = w.export_variants(Format::Stl, &folder.0, false).unwrap();
        assert_eq!(
            files
                .iter()
                .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["box-large.stl", "box-small.stl"]
        );
        let large = std::fs::read(&files[0].path).unwrap();
        let small = std::fs::read(&files[1].path).unwrap();
        assert_eq!(large.len(), files[0].bytes);
        assert_ne!(large, small);
        // the model of a variant is the model of the project with its values applied
        let mut applied = family();
        assert!(applied.apply_variant("large"));
        assert_eq!(applied.export_bytes(Format::Stl).unwrap(), large);
        // a file that exists stops the whole export before anything is written
        std::fs::remove_file(&files[0].path).unwrap();
        let (name, e) = w
            .export_variants(Format::Stl, &folder.0, false)
            .unwrap_err();
        assert_eq!((name.as_str(), e.code()), ("small", "PHOENIX-CAD-040"));
        assert!(!files[0].path.exists());
        assert_eq!(
            w.export_variants(Format::Stl, &folder.0, true)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn a_variant_that_cannot_be_built_stops_the_export() {
        let mut w = family();
        assert!(w.set_variant_value("small", "width", "-5 mm"));
        let folder = Folder::new("refused");
        let (name, _) = w
            .export_variants(Format::Script, &folder.0, false)
            .unwrap_err();
        assert_eq!(name, "small");
        assert_eq!(std::fs::read_dir(&folder.0).unwrap().count(), 0);
    }

    #[test]
    fn the_variants_are_saved_with_the_project_and_read_back() {
        let folder = Folder::new("save");
        for file in ["family.phoenixproj", "family.eh3d"] {
            let path = folder.0.join(file);
            let mut w = family();
            assert!(w.save_to(&path), "{:?}", w.status);
            let mut again =
                Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap();
            assert!(again.open(&path), "{:?}", again.status);
            assert!(!again.read_only);
            assert_eq!(again.mech.variants, w.mech.variants);
            // a project with no variant keeps no domain for them
            assert!(again.remove_variant("small") && again.remove_variant("large"));
            assert!(again.save());
            assert!(w.open(&path));
            assert!(w.mech.variants.is_empty());
        }
    }

    #[test]
    fn variants_that_cannot_be_read_leave_the_project_read_only() {
        let folder = Folder::new("damaged");
        let path = folder.0.join("family.phoenixproj");
        let mut w = family();
        assert!(w.save_to(&path));
        let mut document =
            crate::project::Document::from_bytes(&std::fs::read(&path).unwrap(), 1 << 24).unwrap();
        document
            .set(
                crate::variants::VARIANTS_DOMAIN,
                false,
                b"PHOENIX-CAD-VARIANTS 9\n".to_vec(),
            )
            .unwrap();
        std::fs::write(&path, document.to_bytes()).unwrap();
        assert!(w.open(&path), "{:?}", w.status);
        assert!(w.read_only);
        assert!(w.mech.variants.is_empty());
    }
}
