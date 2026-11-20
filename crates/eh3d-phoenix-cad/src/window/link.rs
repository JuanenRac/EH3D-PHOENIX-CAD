// SPDX-License-Identifier: GPL-3.0-or-later
//! The panels of the window that look beyond the file: the units the project keeps and the
//! project on the EH3D server. Both are thin: the units are those of `project-units` and the
//! server is the store of [`crate::server`], the same one the five `server-*` commands use.

use super::*;
use crate::server::Store;

impl Window {
    // -----------------------------------------------------------------------------------------
    // The units of the project
    // -----------------------------------------------------------------------------------------

    /// The text of the units panel in the language of the window: the unit and the decimals the
    /// project keeps, or the ones of the settings that it follows.
    pub fn units_panel_text(&self) -> String {
        let lang = self.lang;
        match self.project_units() {
            Some(u) => fill(
                Msg::UnitsSet.tr(lang),
                &[u.unit.suffix(), &u.decimals.to_string()],
            ),
            None => fill(
                Msg::UnitsNone.tr(lang),
                &[
                    self.settings.unit.suffix(),
                    &self.settings.decimals.to_string(),
                ],
            ),
        }
    }

    /// Sets the units of the project from the two texts of the panel: a unit (`nm`, `um`, `mm`,
    /// `cm`, `m`, `in` or `mil`) and the decimals (0 to 12). An empty text keeps what the project
    /// shows now. A unit that is not one, or decimals that are not a whole number, are refused
    /// and the status says so; nothing changes then.
    pub fn apply_units_text(&mut self, unit: &str, decimals: &str) -> bool {
        let unit = unit.trim();
        let decimals = decimals.trim();
        let now = self.project_units();
        let shown_unit = now.map_or(self.settings.unit, |u| u.unit);
        let shown_decimals = now.map_or(self.settings.decimals, |u| u.decimals);
        let unit = if unit.is_empty() {
            shown_unit
        } else {
            match Unit::from_suffix(unit) {
                Some(u) => u,
                None => {
                    self.status = Status::Error(CadError::UnknownName(format!(
                        "{unit} is not nm, um, mm, cm, m, in or mil"
                    )));
                    return false;
                }
            }
        };
        let decimals = if decimals.is_empty() {
            shown_decimals
        } else {
            match decimals.parse::<u32>() {
                Ok(d) => d,
                Err(_) => {
                    self.status = Status::Error(CadError::ValueOutOfRange("decimals"));
                    return false;
                }
            }
        };
        self.set_project_units(Some(ProjectUnits { unit, decimals }))
    }

    // -----------------------------------------------------------------------------------------
    // The project on the server
    // -----------------------------------------------------------------------------------------

    /// The text of the server panel in the language of the window: where the link points to, or
    /// that no address is set.
    pub fn server_panel_text(&self) -> String {
        let lang = self.lang;
        let link = &self.settings.server;
        if !link.is_configured() {
            return Msg::ServerNotSet.tr(lang).to_string();
        }
        fill(Msg::ServerAddress.tr(lang), &[&link.server_address])
    }

    /// A store over the link of the settings, or the sentence that says why there is none.
    pub fn server_store(&self) -> Result<Store, String> {
        Store::connect(
            &self.settings.server,
            self.lang,
            self.settings.mechanical.max_bytes,
        )
        .map_err(|e| e.text)
    }

    /// The name a project gets on the server: the one typed, or the name of its file without the
    /// extension when nothing is typed.
    pub fn server_name_for(&self, typed: &str) -> Option<String> {
        let typed = typed.trim();
        if !typed.is_empty() {
            return Some(typed.to_string());
        }
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
    }

    fn server_problem(&mut self, text: String) -> bool {
        self.status = Status::Problem(Msg::ServerText, vec![text]);
        false
    }

    /// Saves the project on the server as a new version of its file (nothing is written when it is
    /// the version already there). The project keeps its own file, its unsaved mark and its
    /// history: the server is one more place to keep it, not a save. False, and the status says
    /// why, when there is no name, the link refuses or the project cannot be written.
    pub fn server_save_with(&mut self, store: &mut Store, typed: &str, comment: &str) -> bool {
        let Some(name) = self.server_name_for(typed) else {
            return self.server_problem(Msg::ServerBadName.tr(self.lang).replace("{0}", ""));
        };
        let mut document = self.document.clone();
        let bytes = match document
            .set_mechanical(&self.mech, &self.settings.mechanical)
            .and_then(|()| document.set_sketches(&self.mech.sketches, &self.sketch_settings))
        {
            Ok(()) => document.to_bytes(),
            Err(e) => {
                self.status = Status::Error(e);
                return false;
            }
        };
        match store.save(&name, &bytes, comment) {
            Ok(w) => {
                let msg = if w.unchanged {
                    Msg::ServerUnchanged
                } else {
                    Msg::ServerSaved
                };
                self.status = Status::Done(msg, vec![name, w.version.to_string()]);
                true
            }
            Err(e) => self.server_problem(e.text),
        }
    }

    /// Opens the project of this name from the server, as a project with no file yet (saving asks
    /// for one). False, and the status says why, when the server has none of that name, the link
    /// refuses or what it holds is not a project of this program.
    pub fn server_open_with(&mut self, store: &mut Store, name: &str) -> bool {
        let name = name.trim();
        let bytes = match store.load(name) {
            Ok(b) => b,
            Err(e) => return self.server_problem(e.text),
        };
        match self.read_project_bytes(&bytes, None) {
            Ok(()) => {
                self.status = Status::Done(
                    Msg::ServerLoaded,
                    vec![name.to_string(), bytes.len().to_string()],
                );
                true
            }
            Err(status) => {
                self.status = status;
                false
            }
        }
    }

    /// [`Window::server_save_with`] over the link of the settings (the sentence that says why there
    /// is none is the status when there is no address).
    pub fn server_save_now(&mut self, typed: &str, comment: &str) -> bool {
        match self.server_store() {
            Ok(mut store) => self.server_save_with(&mut store, typed, comment),
            Err(text) => self.server_problem(text),
        }
    }

    /// [`Window::server_open_with`] over the link of the settings.
    pub fn server_open_now(&mut self, name: &str) -> bool {
        match self.server_store() {
            Ok(mut store) => self.server_open_with(&mut store, name),
            Err(text) => self.server_problem(text),
        }
    }

    /// [`Window::server_list_with`] over the link of the settings.
    pub fn server_list_now(&self, query: &str) -> String {
        match self.server_store() {
            Ok(mut store) => self.server_list_with(&mut store, query),
            Err(text) => text,
        }
    }

    /// The projects of the server whose name holds the text, one to a line, in the language of the
    /// window (or the sentence that says why they cannot be listed).
    pub fn server_list_with(&self, store: &mut Store, query: &str) -> String {
        match store.projects(query.trim()) {
            Ok(list) => list
                .iter()
                .map(|p| {
                    fill(
                        Msg::ServerProjectLine.tr(self.lang),
                        &[&p.id, &p.name, &p.files.to_string()],
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Err(e) => e.text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serverfake::MemoryStore;
    use eh3d_serverclient as sc;

    const TOKEN: &str = "t0ken-of-the-tests";

    fn frame() -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/frame.phoenixproj"
        ))
    }

    fn fresh(lang: Lang) -> Window {
        Window::new(lang, Settings::defaults(), UiSettings::defaults()).unwrap()
    }

    fn store_over(mem: &MemoryStore, lang: Lang) -> Store {
        let mut link = sc::Settings::default();
        link.set("server_address", "memory:1").unwrap();
        link.set("server_token", TOKEN).unwrap();
        link.set("server_retries", "0").unwrap();
        Store::over(sc::Client::new(link, Box::new(mem.clone())), lang, 1 << 20)
    }

    #[test]
    fn the_units_panel_says_what_the_project_keeps_and_sets_them_from_two_texts() {
        let mut w = fresh(Lang::En);
        assert!(w.open(&frame()), "{:?}", w.status);
        let follows = w.units_panel_text();
        assert!(
            follows.starts_with("The project has no units of its own"),
            "{follows}"
        );
        // an empty text keeps what is shown now
        assert!(w.apply_units_text("", ""), "{:?}", w.status);
        assert_eq!(
            w.units_panel_text(),
            fill(
                Msg::UnitsSet.tr(Lang::En),
                &[
                    Settings::defaults().unit.suffix(),
                    &Settings::defaults().decimals.to_string()
                ]
            )
        );
        assert!(w.apply_units_text(" in ", "3"), "{:?}", w.status);
        assert_eq!(w.units_panel_text(), "Units of the project: in, 3 decimals");
        assert_eq!(
            w.project_units(),
            Some(ProjectUnits {
                unit: Unit::from_suffix("in").unwrap(),
                decimals: 3
            })
        );
        // only the decimals change when the unit is left empty
        assert!(w.apply_units_text("", "1"));
        assert_eq!(w.units_panel_text(), "Units of the project: in, 1 decimals");
    }

    #[test]
    fn units_that_are_not_units_are_refused_and_nothing_changes() {
        let mut w = fresh(Lang::En);
        assert!(w.open(&frame()));
        let before = w.units_panel_text();
        for (unit, decimals) in [("furlong", "2"), ("mm", "x"), ("mm", "-1"), ("mm", "13")] {
            assert!(!w.apply_units_text(unit, decimals), "{unit} {decimals}");
            assert!(matches!(w.status, Status::Error(_)), "{:?}", w.status);
            assert_eq!(w.units_panel_text(), before);
        }
    }

    #[test]
    fn the_server_panel_tells_the_address_or_that_there_is_none_in_each_language() {
        for lang in Lang::ALL {
            let w = fresh(lang);
            assert_eq!(w.server_panel_text(), Msg::ServerNotSet.tr(lang));
        }
        let mut settings = Settings::defaults();
        settings
            .server
            .set("server_address", "192.168.0.180:18200")
            .unwrap();
        let w = Window::new(Lang::De, settings, UiSettings::defaults()).unwrap();
        assert_eq!(w.server_panel_text(), "Server: 192.168.0.180:18200");
        assert!(w.server_store().is_ok());
        assert!(fresh(Lang::En).server_store().is_err());
    }

    #[test]
    fn a_project_is_saved_on_the_server_and_opened_from_it_without_a_file() {
        let mem = MemoryStore::new(TOKEN);
        let mut w = fresh(Lang::En);
        assert!(w.open(&frame()), "{:?}", w.status);
        let mut store = store_over(&mem, Lang::En);
        // the name of the file is the name on the server unless one is typed
        assert_eq!(w.server_name_for(""), Some("frame".to_string()));
        assert_eq!(w.server_name_for(" mine "), Some("mine".to_string()));
        assert!(
            w.server_save_with(&mut store, "", "first"),
            "{:?}",
            w.status
        );
        assert_eq!(
            w.status,
            Status::Done(Msg::ServerSaved, vec!["frame".into(), "1".into()])
        );
        // the same project again writes nothing
        assert!(w.server_save_with(&mut store, "", "again"));
        assert_eq!(
            w.status,
            Status::Done(Msg::ServerUnchanged, vec!["frame".into(), "1".into()])
        );
        assert_eq!(mem.project_names(), vec!["frame".to_string()]);
        // the server is not a save: the window keeps its file and its mark
        assert_eq!(w.path, Some(frame()));
        assert!(!w.dirty);
        let list = w.server_list_with(&mut store, "fra");
        assert!(list.contains("frame"), "{list}");
        assert_eq!(w.server_list_with(&mut store, "zzz"), "");
        // another window opens it from the server and has no file
        let mut other = fresh(Lang::En);
        assert!(
            other.server_open_with(&mut store, "frame"),
            "{:?}",
            other.status
        );
        assert!(matches!(other.status, Status::Done(Msg::ServerLoaded, _)));
        assert_eq!(other.path, None);
        assert!(!other.dirty);
        assert_eq!(other.part_list_text().unwrap(), w.part_list_text().unwrap());
        assert_eq!(other.server_name_for(""), None);
        assert!(!other.server_save_with(&mut store, "", ""));
        assert!(matches!(other.status, Status::Problem(Msg::ServerText, _)));
    }

    #[test]
    fn what_the_server_cannot_do_is_told_in_the_language_of_the_window() {
        let mem = MemoryStore::new(TOKEN);
        let mut w = fresh(Lang::Es);
        let mut store = store_over(&mem, Lang::Es);
        assert!(!w.server_open_with(&mut store, "nothing-here"));
        match &w.status {
            Status::Problem(Msg::ServerText, v) => {
                assert_eq!(
                    v[0],
                    "El servidor no tiene ningún proyecto llamado nothing-here"
                )
            }
            other => panic!("{other:?}"),
        }
        // a name the server cannot take
        assert!(w.open(&frame()));
        assert!(!w.server_save_with(&mut store, "bad\u{1}name", ""));
        assert!(matches!(w.status, Status::Problem(Msg::ServerText, _)));
        assert!(mem.project_names().is_empty());
    }

    #[test]
    fn random_units_and_names_are_taken_or_refused_without_harm_with_a_fixed_seed() {
        let mut state = 0xA076_1D64_78BD_642Fu64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let units = [
            "", "mm", "cm", "in", "mil", "um", "nm", "m", "km", "x", " mm ",
        ];
        let decimals = ["", "0", "3", "12", "13", "-1", "x", "1.5", " 2 "];
        let mut w = fresh(Lang::En);
        assert!(w.open(&frame()));
        let (mut kept, mut refused) = (0, 0);
        for _ in 0..400 {
            let unit = units[(next() % units.len() as u64) as usize];
            let dec = decimals[(next() % decimals.len() as u64) as usize];
            let before = (w.project_units(), w.dirty);
            if w.apply_units_text(unit, dec) {
                kept += 1;
                let now = w.project_units().unwrap();
                if let Ok(d) = dec.trim().parse::<u32>() {
                    assert_eq!(now.decimals, d, "{unit:?} {dec:?}");
                }
                if !unit.trim().is_empty() {
                    assert_eq!(now.unit.suffix(), unit.trim(), "{unit:?} {dec:?}");
                }
            } else {
                refused += 1;
                assert_eq!((w.project_units(), w.dirty), before, "{unit:?} {dec:?}");
            }
        }
        assert!(kept > 50 && refused > 50, "{kept} {refused}");
        // names: a name the server takes comes back with the project it was saved with, and one it
        // does not take writes nothing
        let mem = MemoryStore::new(TOKEN);
        let mut store = store_over(&mem, Lang::En);
        let letters = b"abcXYZ019 _-.";
        let (mut saved, mut turned_down) = (0usize, 0usize);
        for _ in 0..60 {
            let len = (next() % 14) as usize;
            let name: String = (0..len)
                .map(|_| char::from(letters[(next() % letters.len() as u64) as usize]))
                .collect();
            let before = mem.project_names().len();
            if w.server_save_with(&mut store, &name, "") {
                saved += 1;
                let mut other = fresh(Lang::En);
                let kept_as = w.server_name_for(&name).unwrap();
                assert!(
                    other.server_open_with(&mut store, &kept_as),
                    "{name:?} {:?}",
                    other.status
                );
                assert_eq!(other.part_list_text().unwrap(), w.part_list_text().unwrap());
            } else {
                turned_down += 1;
                assert_eq!(mem.project_names().len(), before, "{name:?}");
            }
        }
        assert!(saved > 20 && turned_down > 0, "{saved} {turned_down}");
    }
}
