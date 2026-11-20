// SPDX-License-Identifier: GPL-3.0-or-later
//! Two versions of a project side by side (see [`crate::compare`]): the project that is open
//! against another one that is open in another window, or read from a file.

use super::*;

impl Window {
    /// What changed from this project to `later`: the placed parts (with the mates applied) and
    /// the mates. A project whose mates cannot be applied is refused with the error of the mates.
    pub fn changes_to(&self, later: &Window) -> Result<Vec<crate::compare::Change>, CadError> {
        let before = self.placed_parts()?;
        let after = later.placed_parts()?;
        Ok(crate::compare::compare(
            &before,
            &after,
            self.built.enclosure.mates(),
            later.built.enclosure.mates(),
        ))
    }

    /// [`Window::changes_to`] as lines of text in the language of the window.
    pub fn changes_text_to(&self, later: &Window) -> Result<String, CadError> {
        self.changes_to(later)
            .map(|c| crate::compare::text_in(&c, self.lang))
    }

    /// What changed from an earlier version of this project, kept in a file, to the one that is
    /// open, as lines of text in the language of the window; or the sentence that says why the
    /// file cannot be read. The file is read with the settings of this window.
    pub fn compare_with_file_text(&self, file: &str) -> String {
        let file = file.trim();
        if file.is_empty() {
            return Msg::CompareNoFile.tr(self.lang).to_string();
        }
        let mut earlier = Window::new(self.lang, self.settings.clone(), self.ui.clone())
            .expect("the settings of an open window are valid");
        if !earlier.open(Path::new(file)) {
            return earlier.status.text(self.lang);
        }
        earlier
            .changes_text_to(self)
            .unwrap_or_else(|e| error_text(&e, self.lang))
    }

    /// The same against the version of the project that the server holds under `name` (the name of
    /// the file without its extension when empty).
    pub fn compare_with_server_text(&self, store: &mut crate::server::Store, name: &str) -> String {
        let Some(name) = self.server_name_for(name) else {
            return Msg::CompareNoFile.tr(self.lang).to_string();
        };
        let bytes = match store.load(&name) {
            Ok(b) => b,
            Err(e) => return e.text,
        };
        let mut earlier = Window::new(self.lang, self.settings.clone(), self.ui.clone())
            .expect("the settings of an open window are valid");
        if let Err(status) = earlier.read_project_bytes(&bytes, None) {
            return status.text(self.lang);
        }
        earlier
            .changes_text_to(self)
            .unwrap_or_else(|e| error_text(&e, self.lang))
    }

    /// [`Window::compare_with_server_text`] over the link of the settings.
    pub fn compare_with_server_now(&self, name: &str) -> String {
        match self.server_store() {
            Ok(mut store) => self.compare_with_server_text(&mut store, name),
            Err(text) => text,
        }
    }

    /// [`Window::changes_to`] as comma separated values.
    pub fn changes_csv_to(&self, later: &Window) -> Result<String, CadError> {
        self.changes_to(later).map(|c| crate::compare::csv(&c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::Change;

    fn frame() -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/frame.phoenixproj"
        ))
    }

    fn open(lang: Lang) -> Window {
        let mut w = Window::new(lang, Settings::defaults(), UiSettings::defaults()).unwrap();
        assert!(w.open(&frame()), "{:?}", w.status);
        w
    }

    #[test]
    fn a_project_against_itself_has_no_change_and_against_an_empty_one_all_of_it_changes() {
        let a = open(Lang::En);
        let same = open(Lang::En);
        assert_eq!(a.changes_to(&same).unwrap(), Vec::<Change>::new());
        assert_eq!(a.changes_csv_to(&same).unwrap(), "change,part,from,to\n");
        assert_eq!(
            a.changes_text_to(&same).unwrap(),
            "The two versions have the same parts and mates\n"
        );
        // against a project with nothing in it every part and mate is taken away, and the other
        // way round every one is added
        let empty = Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap();
        let parts = a.built.enclosure.parts().len();
        let mates = a.built.enclosure.mates().len();
        assert!(parts > 0);
        let gone = a.changes_to(&empty).unwrap();
        assert_eq!(gone.len(), parts + mates);
        assert_eq!(
            gone.iter()
                .filter(|c| matches!(c, Change::PartRemoved { .. }))
                .count(),
            parts
        );
        assert_eq!(
            gone.iter()
                .filter(|c| matches!(c, Change::MateRemoved { .. }))
                .count(),
            mates
        );
        let back = empty.changes_to(&a).unwrap();
        assert_eq!(back.len(), parts + mates);
        assert!(back
            .iter()
            .all(|c| matches!(c, Change::PartAdded { .. } | Change::MateAdded { .. })));
        let text = a.changes_text_to(&empty).unwrap();
        assert!(text.starts_with("Part removed: "), "{text}");
        assert!(
            text.ends_with(&format!("{} changes\n", parts + mates)),
            "{text}"
        );
    }

    #[test]
    fn the_text_is_told_in_the_language_of_the_window() {
        let a = open(Lang::Es);
        let b = open(Lang::Es);
        assert_eq!(
            a.changes_text_to(&b).unwrap(),
            format!("{}\n", Msg::CompareNone.tr(Lang::Es))
        );
    }

    #[test]
    fn the_open_project_is_compared_with_an_earlier_one_kept_in_a_file_or_on_the_server() {
        use crate::serverfake::MemoryStore;
        use eh3d_serverclient as sc;
        let current = open(Lang::En);
        let box_file = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/box.phoenixproj"
        ));
        // the earlier version is the file: against the box with no part everything was added
        let text = current.compare_with_file_text(&box_file.to_string_lossy());
        assert!(text.starts_with("Part added: "), "{text}");
        let parts = current.built.enclosure.parts().len();
        let mates = current.built.enclosure.mates().len();
        assert!(
            text.ends_with(&format!("{} changes\n", parts + mates)),
            "{text}"
        );
        // the same file twice has nothing to tell, and the text is in the language of the window
        let same = current.compare_with_file_text(&frame().to_string_lossy());
        assert_eq!(same, "The two versions have the same parts and mates\n");
        let es = open(Lang::Es);
        assert_eq!(
            es.compare_with_file_text(&frame().to_string_lossy()),
            format!("{}\n", Msg::CompareNone.tr(Lang::Es))
        );
        // no file, a file that is not there and a file that is not a project say so
        assert_eq!(
            current.compare_with_file_text("  "),
            Msg::CompareNoFile.tr(Lang::En)
        );
        let missing = current.compare_with_file_text("no-such-file.phoenixproj");
        assert!(missing.starts_with("PHOENIX-CAD-"), "{missing}");
        let not_project = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
        assert!(!current
            .compare_with_file_text(&not_project.to_string_lossy())
            .starts_with("Part"));
        // the earlier version is on the server
        let mem = MemoryStore::new("t0ken-of-the-tests");
        let mut link = sc::Settings::default();
        link.set("server_address", "memory:1").unwrap();
        link.set("server_token", "t0ken-of-the-tests").unwrap();
        link.set("server_retries", "0").unwrap();
        let mut store = crate::server::Store::over(
            sc::Client::new(link, Box::new(mem.clone())),
            Lang::En,
            1 << 20,
        );
        let mut empty =
            Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap();
        assert!(empty.open(&box_file));
        assert!(
            empty.server_save_with(&mut store, "frame", ""),
            "{:?}",
            empty.status
        );
        let from_server = current.compare_with_server_text(&mut store, "frame");
        assert_eq!(from_server, text);
        // an empty name is the name of the file of the project, which the server does have now
        assert_eq!(current.compare_with_server_text(&mut store, ""), text);
        assert!(current
            .compare_with_server_text(&mut store, "nothing-here")
            .contains("nothing-here"));
        assert_eq!(
            Window::new(Lang::En, Settings::defaults(), UiSettings::defaults())
                .unwrap()
                .compare_with_server_text(&mut store, ""),
            Msg::CompareNoFile.tr(Lang::En)
        );
    }
}
