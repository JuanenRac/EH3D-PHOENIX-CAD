// SPDX-License-Identifier: GPL-3.0-or-later
//! The variants of the project in the window and in the command `variants`: the current values of
//! the variables kept under a name, a variant applied, checked or exported, all without a screen.
//!
//! Keeping, changing, applying and removing a variant are changes of the project like any other:
//! one entry of the history each, saved with the project. Checking and exporting look at each
//! variant on a copy of the project, so neither changes it.

use std::path::PathBuf;

use super::*;
use crate::mechanical::step_keys;

/// A variant as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantRow {
    /// Its name.
    pub name: String,
    /// Its values in the unit of the project, `name = value` separated by commas.
    pub values: String,
    /// The steps it switches off and on, in words; empty when it leaves them all as they are.
    pub steps: String,
    /// The variables and the steps it names that the project does not have.
    pub missing: Vec<String>,
    /// Whether the project is in this variant now: every value and every step it holds is the one
    /// of the project.
    pub current: bool,
}

/// A step of the recipe as the variants name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantStepRow {
    /// Its place in the recipe, from 1.
    pub number: usize,
    /// Its key, which stays the same when steps are added or removed.
    pub key: String,
    /// The name shown to people.
    pub label: String,
    /// Whether it is suppressed in the project now.
    pub suppressed: bool,
}

/// What a variant does with a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepChoice {
    /// The step is switched off.
    Off,
    /// The step is switched on.
    On,
    /// The variant leaves the step as it is.
    Keep,
}

impl StepChoice {
    /// The choice with this word (`off`, `on` or `keep`).
    pub fn from_word(word: &str) -> Option<StepChoice> {
        match word {
            "off" => Some(StepChoice::Off),
            "on" => Some(StepChoice::On),
            "keep" => Some(StepChoice::Keep),
            _ => None,
        }
    }
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

/// The cells of one line of a table: separated by `separator`, a cell may be quoted and a quote
/// inside it is written twice.
fn cells(line: &str, separator: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            '"' if quoted => quoted = false,
            '"' if cell.trim().is_empty() => {
                cell.clear();
                quoted = true;
            }
            c if c == separator && !quoted => out.push(std::mem::take(&mut cell)),
            c => cell.push(c),
        }
    }
    out.push(cell);
    out.iter().map(|c| c.trim().to_string()).collect()
}

fn quoted(text: &str) -> String {
    if text.contains([',', ';', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// The largest table of variants that is read, in bytes.
const MAX_TABLE_BYTES: u64 = 1 << 20;

impl Window {
    /// What each variant gives, as comma separated values: the outer and inner sizes in mm, the
    /// volume of the material in mm3, the steps the model refuses and, with a board, whether it
    /// fits. A variant that cannot be built says why in the last column. The project does not
    /// change.
    pub fn variant_figures_csv(&self) -> String {
        let mut out = "variant,outer_x_mm,outer_y_mm,outer_z_mm,inner_x_mm,inner_y_mm,\
                       inner_z_mm,material_mm3,refused_steps,fits_board,error\n"
            .to_string();
        for name in self.mech.variants.names() {
            let row = match self.variant_window(name) {
                Err(e) => format!("{name},,,,,,,,,,{}", quoted(&error_text(&e, self.lang))),
                Ok(copy) => {
                    let info = &copy.info;
                    let sizes: Vec<String> = info
                        .outer
                        .iter()
                        .chain(&info.inner)
                        .map(|l| mm_text(l.get()))
                        .collect();
                    let fits = match (info.fit_from_board, info.fit.fits || info.fit.fits_turned) {
                        (false, _) => "",
                        (true, true) => "yes",
                        (true, false) => "no",
                    };
                    format!(
                        "{name},{},{},{},{fits},",
                        sizes.join(","),
                        catalog::volume_mm3_text(info.material_nm3),
                        info.failed
                    )
                }
            };
            out.push_str(&row);
            out.push('\n');
        }
        out
    }

    /// The variants as a table to be edited in a spreadsheet: a column for each variable that a
    /// variant holds, a line for each variant, each value in mm exactly and an empty cell where
    /// the variant leaves the variable alone. The steps a variant switches are not in the table.
    pub fn variant_grid_csv(&self) -> String {
        let set = &self.mech.variants;
        let columns: std::collections::BTreeSet<&String> = set
            .names()
            .into_iter()
            .filter_map(|n| set.values(n).ok())
            .flat_map(|values| values.keys())
            .collect();
        let mut out = "variant".to_string();
        for column in &columns {
            out.push(',');
            out.push_str(column);
        }
        out.push('\n');
        for name in set.names() {
            out.push_str(name);
            let values = set.values(name).ok();
            for column in &columns {
                out.push(',');
                if let Some(value) = values.and_then(|v| v.get(*column)) {
                    out.push_str(&format!("{} mm", mm_text(value.get())));
                }
            }
            out.push('\n');
        }
        out
    }

    /// Reads a table such as [`Window::variant_grid_csv`] writes (separated by commas or, when
    /// the first line has one, semicolons): each line gives the values of one variant, lengths or
    /// formulas; the variants it does not name stay and the steps a variant switches are kept. All
    /// of it is one change of the history, or none of it: a column that is not a variable of the
    /// project, a repeated variant, a line with nothing to keep or a value that is not understood
    /// refuse the whole table.
    pub fn import_variant_grid(&mut self, text: &str) -> bool {
        if self.read_only_refusal() {
            return false;
        }
        let result = self.variant_grid_change(text);
        self.variant_done(result)
    }

    fn variant_grid_change(&mut self, text: &str) -> Result<(), CadError> {
        let bad = |why: String| CadError::Format(format!("table of variants: {why}"));
        let mut lines = text
            .trim_start_matches('\u{feff}')
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty());
        let (_, header) = lines.next().ok_or_else(|| bad("it is empty".to_string()))?;
        let separator = if header.contains(';') { ';' } else { ',' };
        let columns = cells(header, separator);
        if columns.first().map(String::as_str) != Some("variant") || columns.len() < 2 {
            return Err(bad(
                "the first line is variant and the variables".to_string()
            ));
        }
        for (i, column) in columns.iter().enumerate().skip(1) {
            self.mech.vars.require(column)?;
            if columns[..i].contains(column) {
                return Err(CadError::Duplicate(column.clone()));
            }
        }
        let mut next = self.mech.clone();
        let mut seen = std::collections::BTreeSet::new();
        for (number, line) in lines {
            let row = cells(line, separator);
            if row.len() > columns.len() {
                return Err(bad(format!("line {} has too many cells", number + 1)));
            }
            let name = row.first().cloned().unwrap_or_default();
            if !seen.insert(name.clone()) {
                return Err(CadError::Duplicate(name));
            }
            let mut values = std::collections::BTreeMap::new();
            for (column, cell) in columns.iter().zip(&row).skip(1) {
                if !cell.is_empty() {
                    values.insert(column.clone(), self.variable_value(cell)?);
                }
            }
            next.variants.put(&name, values, &self.settings.variants)?;
        }
        if seen.is_empty() {
            return Err(bad("it has no variant".to_string()));
        }
        let count = seen.len().to_string();
        let title = fill(Msg::VariantsImported.tr(self.lang), &[&count]);
        let done = Status::Done(Msg::VariantsImported, vec![count]);
        self.variant_store(&title, done, next)
    }

    /// [`Window::import_variant_grid`] of a file.
    pub fn read_variant_grid(&mut self, path: &Path) -> bool {
        match store::load_limited(path, MAX_TABLE_BYTES) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => self.import_variant_grid(&text),
                Err(_) => {
                    self.status =
                        Status::Error(CadError::Format("table of variants: not text".to_string()));
                    false
                }
            },
            Err(e) => {
                self.status = Status::Error(e);
                false
            }
        }
    }

    /// Writes [`Window::variant_grid_csv`] to a file, never over one that exists unless
    /// `overwrite` is true.
    pub fn write_variant_grid(&mut self, path: &Path, overwrite: bool) -> bool {
        let written = export::write_bytes(
            path,
            self.variant_grid_csv().as_bytes(),
            overwrite,
            self.path.as_deref(),
            self.ui.export.max_kib,
        );
        match written {
            Ok(_) => {
                self.status =
                    Status::Done(Msg::VariantGridWritten, vec![path.display().to_string()]);
                true
            }
            Err(e) => {
                self.status = Status::Error(e);
                false
            }
        }
    }

    /// The variants of the project, in the order of their names.
    pub fn variant_rows(&self) -> Vec<VariantRow> {
        let set = &self.mech.variants;
        let steps = self.variant_step_rows();
        set.names()
            .into_iter()
            .filter_map(|name| {
                let values = set.values(name).ok()?;
                let text: Vec<String> = values
                    .iter()
                    .map(|(variable, value)| format!("{variable} = {}", self.unit_text(*value)))
                    .collect();
                let mut missing: Vec<String> = values
                    .keys()
                    .filter(|v| self.mech.vars.get(v).is_none())
                    .cloned()
                    .collect();
                let mut off = Vec::new();
                let mut on = Vec::new();
                let mut current = values
                    .iter()
                    .all(|(v, value)| self.mech.vars.get(v) == Some(*value));
                for (key, suppressed) in set.steps_of(name) {
                    let Some(step) = steps.iter().find(|s| s.key == *key) else {
                        missing.push(key.clone());
                        current = false;
                        continue;
                    };
                    current &= step.suppressed == *suppressed;
                    let label = format!("{} {}", step.number, step.label);
                    if *suppressed {
                        off.push(label);
                    } else {
                        on.push(label);
                    }
                }
                let mut words = Vec::new();
                for (msg, list) in [(Msg::VariantStepsOff, off), (Msg::VariantStepsOn, on)] {
                    if !list.is_empty() {
                        words.push(fill(msg.tr(self.lang), &[&list.join(", ")]));
                    }
                }
                Some(VariantRow {
                    name: name.to_string(),
                    values: text.join(", "),
                    steps: words.join("; "),
                    current: current && missing.is_empty(),
                    missing,
                })
            })
            .collect()
    }

    /// The steps of the recipe with the keys the variants use for them.
    pub fn variant_step_rows(&self) -> Vec<VariantStepRow> {
        step_keys(&self.mech.steps, &self.settings.mechanical)
            .into_iter()
            .zip(&self.mech.steps)
            .enumerate()
            .map(|(i, (key, step))| VariantStepRow {
                number: i + 1,
                key,
                label: step_label(step),
                suppressed: step.suppressed,
            })
            .collect()
    }

    /// Says what the variant `name` does with a step, making the variant when there is none.
    /// `which` is the number of the step from 1, its key or its name when only one step has it. A
    /// step that is not there is refused with `PHOENIX-CAD-014` and a name that several steps
    /// have with `PHOENIX-CAD-015`; nothing changes then.
    pub fn set_variant_step(&mut self, name: &str, which: &str, choice: StepChoice) -> bool {
        if self.read_only_refusal() {
            return false;
        }
        let result = self.variant_step_change(name.trim(), which.trim(), choice);
        self.variant_done(result)
    }

    fn variant_step_change(
        &mut self,
        name: &str,
        which: &str,
        choice: StepChoice,
    ) -> Result<(), CadError> {
        let rows = self.variant_step_rows();
        let unknown = || CadError::UnknownName(which.to_string());
        let by_number = which
            .parse::<usize>()
            .ok()
            .and_then(|n| rows.iter().find(|r| r.number == n));
        let (key, label) = match by_number.or_else(|| rows.iter().find(|r| r.key == which)) {
            Some(row) => (row.key.clone(), row.label.clone()),
            None => {
                let named: Vec<&VariantStepRow> =
                    rows.iter().filter(|r| r.label == which).collect();
                match named.as_slice() {
                    [one] => (one.key.clone(), one.label.clone()),
                    // a step that is gone can still be taken out of a variant by its key
                    [] if choice == StepChoice::Keep => (which.to_string(), which.to_string()),
                    [] => return Err(unknown()),
                    _ => return Err(CadError::Duplicate(which.to_string())),
                }
            }
        };
        let off = match choice {
            StepChoice::Off => Some(true),
            StepChoice::On => Some(false),
            StepChoice::Keep => None,
        };
        let mut next = self.mech.clone();
        next.variants
            .set_step(name, &key, off, &self.settings.variants)?;
        let msg = match choice {
            StepChoice::Off => Msg::VariantStepOff,
            StepChoice::On => Msg::VariantStepOn,
            StepChoice::Keep => Msg::VariantStepKeep,
        };
        let shown = vec![name.to_string(), label];
        let title = fill(msg.tr(self.lang), &[&shown[0], &shown[1]]);
        self.variant_store(&title, Status::Done(msg, shown), next)
    }

    /// The recipe of the project with a variant written over it: its values in the variables and
    /// its steps switched off or on. A variable or a step that the project does not have is
    /// refused with `PHOENIX-CAD-014`, and a different value for a variable that follows a drawing
    /// with `PHOENIX-CAD-016`.
    fn variant_recipe(&self, name: &str) -> Result<Mechanical, CadError> {
        let mut next = self.mech.clone();
        next.vars = self.mech.variants.variables_for(name, &self.mech.vars)?;
        if let Some(followed) = self
            .mech
            .links
            .keys()
            .find(|v| next.vars.get(v) != self.mech.vars.get(v))
        {
            return Err(CadError::InUse(followed.clone()));
        }
        let keys = step_keys(&self.mech.steps, &self.settings.mechanical);
        for (key, suppressed) in self.mech.variants.steps_of(name) {
            let step = keys
                .iter()
                .position(|k| k == key)
                .and_then(|i| next.steps.get_mut(i))
                .ok_or_else(|| CadError::UnknownName(key.clone()))?;
            step.suppressed = *suppressed;
        }
        Ok(next)
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
        let next = self.variant_recipe(name)?;
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
        let recipe = self.variant_recipe(name)?;
        let mut copy = self.clone();
        copy.mech = recipe;
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

    /// `family` with two fins on a wall as its only step.
    fn finned() -> Window {
        let mut w = family();
        w.add_step(super::super::NewStep::Fins);
        assert_eq!(w.mech.steps.len(), 1, "{:?}", w.status);
        w
    }

    #[test]
    fn a_variant_switches_steps_off_and_on_and_says_when_it_is_the_current_one() {
        let mut w = finned();
        assert!(w.variant_rows().iter().all(|r| r.steps.is_empty()));
        assert!(
            w.set_variant_step("small", "1", StepChoice::Off),
            "{:?}",
            w.status
        );
        let label = w.variant_step_rows()[0].label.clone();
        let row = |w: &Window, name: &str| {
            w.variant_rows()
                .into_iter()
                .find(|r| r.name == name)
                .unwrap()
        };
        assert!(row(&w, "small").steps.contains(&label));
        assert!(!row(&w, "small").current);
        let with = w.export_bytes(Format::Script).unwrap();
        // the variant built has not the step, and the project still has it
        let small = w.variant_window("small").unwrap();
        assert!(small.mech.steps[0].suppressed);
        assert_ne!(small.export_bytes(Format::Script).unwrap(), with);
        assert!(!w.mech.steps[0].suppressed);
        assert!(w.apply_variant("small"));
        assert!(w.mech.steps[0].suppressed);
        assert!(row(&w, "small").current);
        assert!(!row(&w, "large").current);
        w.undo();
        assert!(!w.mech.steps[0].suppressed);
        // a variant that switches it on brings it back
        assert!(w.set_variant_step("large", &label, StepChoice::On));
        assert!(w.apply_variant("small") && w.apply_variant("large"));
        assert!(!w.mech.steps[0].suppressed);
        // what is not a step is refused, and a step that is gone is told and can be taken out
        let before = w.mech.clone();
        assert!(!w.set_variant_step("small", "7", StepChoice::Off));
        assert!(!w.set_variant_step("small", "no step", StepChoice::On));
        assert_eq!(w.mech, before);
        let key = w.variant_step_rows()[0].key.clone();
        w.mech.steps.clear();
        w.mech.tip = 0;
        assert_eq!(row(&w, "small").missing, vec![key.clone()]);
        assert!(!w.apply_variant("small"));
        assert!(w.status.text(Lang::En).contains("PHOENIX-CAD-014"));
        assert!(w.set_variant_step("small", &key, StepChoice::Keep));
        assert!(row(&w, "small").missing.is_empty());
        assert_eq!(StepChoice::from_word("off"), Some(StepChoice::Off));
        assert_eq!(StepChoice::from_word("maybe"), None);
    }

    #[test]
    fn the_steps_of_the_variants_are_saved_with_the_project() {
        let folder = Folder::new("steps");
        let path = folder.0.join("family.phoenixproj");
        let mut w = finned();
        assert!(w.set_variant_step("bare", "1", StepChoice::Off));
        assert!(w.save_to(&path), "{:?}", w.status);
        let mut again =
            Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap();
        assert!(again.open(&path), "{:?}", again.status);
        assert!(!again.read_only);
        assert_eq!(again.mech.variants, w.mech.variants);
        assert_eq!(
            again.variant_step_rows()[0].key,
            w.variant_step_rows()[0].key
        );
        assert!(again.apply_variant("bare"));
        assert!(again.mech.steps[0].suppressed);
    }

    #[test]
    fn the_figures_say_what_each_variant_gives() {
        let mut w = family();
        let csv = w.variant_figures_csv();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3);
        let column = |line: &str, i: usize| line.split(',').nth(i).unwrap().to_string();
        assert!(lines[0].starts_with("variant,outer_x_mm"));
        assert_eq!(column(lines[1], 0), "large");
        // the inner width is the value of the variant and the larger box takes more material
        assert_eq!(column(lines[1], 5), "90");
        assert_eq!(column(lines[2], 5), "60");
        let volume = |i: usize| column(lines[i], 7).parse::<f64>().unwrap();
        assert!(volume(1) > volume(2), "{csv}");
        assert_eq!(
            (column(lines[1], 8), column(lines[1], 9)),
            ("0".into(), "".into())
        );
        assert!(lines.iter().all(|l| l.split(',').count() == 11), "{csv}");
        assert!(w.set_variant_value("large", "width", "-5 mm"));
        let csv = w.variant_figures_csv();
        assert!(
            csv.lines().nth(1).unwrap().contains("PHOENIX-CAD-"),
            "{csv}"
        );
    }

    #[test]
    fn the_table_of_the_variants_goes_to_a_spreadsheet_and_comes_back() {
        let mut w = family();
        w.add_variable("depth", "2.5 mm");
        assert!(w.set_variant_value("small", "depth", "3.000001 mm"));
        let table = w.variant_grid_csv();
        assert_eq!(
            table,
            "variant,depth,width\nlarge,,90 mm\nsmall,3.000001 mm,60 mm\n"
        );
        let before = w.mech.clone();
        assert!(w.import_variant_grid(&table), "{:?}", w.status);
        assert_eq!(w.mech, before);
        // semicolons, quotes, formulas, a new variant, and one change of the history
        let entries = w.undo_names().len();
        let edited = "\u{feff}variant;width\r\nsmall;\"61 mm\"\r\n\r\nmedium;depth * 30\r\n";
        assert!(w.import_variant_grid(edited), "{:?}", w.status);
        assert_eq!(w.undo_names().len(), entries + 1);
        assert_eq!(w.mech.variants.values("small").unwrap().len(), 1);
        assert_eq!(w.mech.variants.values("small").unwrap()["width"], mm(61));
        assert_eq!(w.mech.variants.values("medium").unwrap()["width"], mm(75));
        assert_eq!(w.mech.variants.values("large").unwrap()["width"], mm(90));
        // all or none
        let before = w.mech.clone();
        for table in [
            "",
            "variant\nsmall\n",
            "name,width\nsmall,1\n",
            "variant,height\nsmall,1\n",
            "variant,width,width\nsmall,1,1\n",
            "variant,width\n",
            "variant,width\nsmall,61\nsmall,62\n",
            "variant,width\nsmall,61\nother,wide\n",
            "variant,width\nnew,\n",
            "variant,width\nsmall,1,2\n",
        ] {
            assert!(!w.import_variant_grid(table), "{table:?}");
            assert_eq!(w.mech, before, "{table:?}");
        }
        let folder = Folder::new("grid");
        let path = folder.0.join("family.csv");
        assert!(w.write_variant_grid(&path, false), "{:?}", w.status);
        assert!(!w.write_variant_grid(&path, false));
        assert!(w.write_variant_grid(&path, true));
        assert!(w.read_variant_grid(&path), "{:?}", w.status);
        assert!(!w.read_variant_grid(&folder.0.join("none.csv")));
        w.read_only = true;
        assert!(!w.import_variant_grid(&table));
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
