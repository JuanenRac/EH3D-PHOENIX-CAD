// SPDX-License-Identifier: GPL-3.0-or-later
//! The panel of the settings of the server that every program of the suite shows (`ServerPanel` of
//! `eh3d-view`), for the window of this program: the boxes of the link, the state of the link, the
//! login and the logout, with the words in the language of the window. The logic is the one of the
//! shared crate; this only keeps the client the panel talks through and hands the settings the
//! person applied back to the window, which keeps them for the next link it opens.
//!
//! The login (the token the server gave) is kept in memory for the time the window is open and is
//! never written to a file by this program: the token of `[server]` in the settings file is the
//! way to keep one.

use std::sync::{Arc, Mutex};

use eh3d_serverclient as sc;
use eh3d_view::server_panel::{Action, ServerPanel};

use crate::lang::Lang;
use crate::server::APP;

/// The token of a login, kept in memory only.
#[derive(Clone, Default)]
struct LoginStore(Arc<Mutex<Option<sc::Secret>>>);

impl sc::TokenStore for LoginStore {
    fn load(&self) -> Option<sc::Secret> {
        self.0.lock().ok().and_then(|g| g.clone())
    }

    fn save(&self, token: &sc::Secret) -> Result<(), String> {
        let mut g = self.0.lock().map_err(|_| "the login store is broken")?;
        *g = Some(token.clone());
        Ok(())
    }

    fn clear(&self) {
        if let Ok(mut g) = self.0.lock() {
            *g = None;
        }
    }
}

/// The panel of the server and the client it works through.
pub struct LinkPane {
    /// The panel of the shared crate, which the window draws with `eh3d_view::server_gui::show`.
    pub panel: ServerPanel,
    conn: sc::Client,
    store: LoginStore,
}

impl LinkPane {
    /// A panel over the link of the settings.
    pub fn new(link: &sc::Settings) -> LinkPane {
        let store = LoginStore::default();
        let conn = sc::Client::connect(link.clone()).with_store(Box::new(store.clone()));
        LinkPane {
            panel: ServerPanel::new(link, APP),
            conn,
            store,
        }
    }

    /// Asks the client how the link is, for the state the panel shows.
    pub fn refresh(&mut self) {
        self.panel.refresh(&self.conn);
    }

    /// Carries out what the person chose in the panel. True when the settings were applied: they
    /// are written to `link`, which the window keeps, and the panel works through a new client.
    pub fn handle(&mut self, action: Action, link: &mut sc::Settings) -> bool {
        match self.panel.handle(action, &mut self.conn) {
            Some(applied) => {
                *link = applied;
                self.conn =
                    sc::Client::connect(link.clone()).with_store(Box::new(self.store.clone()));
                self.panel.refresh(&self.conn);
                true
            }
            None => false,
        }
    }
}

/// The language of the shared crate for the language of the window.
pub fn view_lang(lang: Lang) -> eh3d_view::texts::Lang {
    eh3d_view::texts::Lang::ALL
        .into_iter()
        .find(|l| l.code() == lang.code())
        .unwrap_or(eh3d_view::texts::Lang::En)
}
