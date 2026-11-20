use super::*;
use eh3d_dock::core::DockCore;
use eh3d_dock::menu::MenuContext;
use eh3d_dock::sectors::workspace_title;

fn core() -> DockCore {
    DockCore::new(defs(), workspaces())
}

fn entry_ids(bar: &MenuBar<Cmd>) -> Vec<String> {
    fn walk(items: &[Item<Cmd>], out: &mut Vec<String>) {
        for i in items {
            match i {
                Item::Entry(e) => out.push(e.id.clone()),
                Item::Submenu(m) => walk(&m.items, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for m in &bar.menus {
        walk(&m.items, &mut out);
    }
    out
}

#[test]
fn the_panels_have_names_that_hold_and_titles_in_the_eight_languages() {
    let d = defs();
    assert_eq!(d.len(), 11);
    let names: Vec<&str> = d.iter().map(|p| p.id.as_str()).collect();
    // the six of the sector first, then the five of this program
    assert_eq!(
        names[..6],
        [
            "view3d",
            "model_tree",
            "properties",
            "tasks",
            "report",
            "console"
        ]
    );
    let mut sorted = names.clone();
    sorted.sort_unstable();
    let mut all = id::ALL.to_vec();
    all.sort_unstable();
    assert_eq!(sorted, all);
    for p in &d {
        assert!(
            p.id.len() <= 48
                && p.id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{}",
            p.id
        );
        for t in p.title.0 {
            assert!(!t.trim().is_empty(), "{}", p.id);
        }
    }
    // the titles of this program's panels are its own messages, in the order the frame wants
    let vars = d.iter().find(|p| p.id == id::VARIABLES).unwrap();
    assert_eq!(vars.title.0[0], "Sheet of variables");
    assert_eq!(vars.title.0[1], "Hoja de variables");
    assert_eq!(vars.title.0[5], "Folha de variáveis");
    assert_eq!(vars.title.0[7], "変数シート");
    let sketch = d.iter().find(|p| p.id == id::SKETCH).unwrap();
    assert_eq!(sketch.title.0[2], "Schizzo");
    assert_eq!(sketch.title.0[3], "Skizze");
}

#[test]
fn the_factory_arrangement_is_sound_and_every_workspace_has_a_name_in_eight_languages() {
    let c = core();
    assert_eq!(c.workspaces().names(), vec!["Model", "Review", "Compact"]);
    assert!(c.warnings().is_empty(), "{:?}", c.warnings());
    assert!(c.layout_is_sound());
    for name in c.workspaces().names() {
        assert!(workspace_title(name).is_some(), "{name}");
    }
    // every panel is in some workspace, so the View menu can bring any of them back from a factory layout
    for p in id::ALL {
        let shown = c
            .workspaces()
            .factory()
            .iter()
            .any(|w| w.layout.is_shown(p));
        assert!(shown, "{p} is in no workspace");
    }
    // the model workspace has the 3D view and the sketch as tabs of one group, the view first
    let model = &c.workspaces().factory()[0].layout;
    assert_eq!(
        model.group_of(id::VIEW3D).map(|g| g.panels.clone()),
        Some(vec![id::VIEW3D.to_string(), id::SKETCH.to_string()])
    );
    // the compact one folds the tree and the properties into the edges
    let compact = &c.workspaces().factory()[2].layout;
    assert!(compact.is_shown(id::VIEW3D) && !compact.is_shown(id::MODEL_TREE));
}

#[test]
fn a_word_of_the_window_option_names_a_workspace() {
    assert_eq!(workspace_of_word("model"), Some("Model"));
    assert_eq!(workspace_of_word(" Review "), Some("Review"));
    assert_eq!(workspace_of_word("COMPACT"), Some("Compact"));
    assert_eq!(workspace_of_word("sketch"), None);
    assert_eq!(workspace_of_word(""), None);
    for w in WINDOW_WORDS {
        assert!(core()
            .workspaces()
            .names()
            .contains(&workspace_of_word(w).unwrap()));
    }
}

#[test]
fn the_mode_follows_the_panel_that_is_on_show() {
    use StartView::{Model, Sketch};
    for current in [Model, Sketch] {
        assert_eq!(mode_after_frame(current, true, false), Model);
        assert_eq!(mode_after_frame(current, false, true), Sketch);
        // side by side, or both hidden: it stays
        assert_eq!(mode_after_frame(current, true, true), current);
        assert_eq!(mode_after_frame(current, false, false), current);
    }
}

#[test]
fn the_menu_bar_is_well_made_and_has_a_text_in_every_language() {
    let bar = menu_bar();
    assert!(bar.validate().is_empty(), "{:?}", bar.validate());
    let titles: Vec<&str> = bar.menus.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(titles, ["file", "edit", "view", "tools", "window", "help"]);
    let ids = entry_ids(&bar);
    assert!(!ids.contains(&"file.export".to_string()));
    for f in Format::ALL {
        assert!(ids.contains(&export_id(f)), "{f:?}");
    }
    for p in Panel::ALL {
        assert!(ids.contains(&report_id(p)), "{p:?}");
    }
    for l in Lang::ALL {
        assert!(ids.contains(&language_id(l)), "{l:?}");
    }
    for l in Lang::ALL {
        let ctx = MenuContext::empty(view_lang(l));
        let resolved = bar.resolve(&ctx);
        assert_eq!(resolved.len(), 6, "{l:?}");
        for m in &resolved {
            assert!(!m.title.trim().is_empty(), "{l:?} {}", m.id);
        }
        assert!(
            bar.conflicts(&ctx).is_empty(),
            "{l:?}: {:?}",
            bar.conflicts(&ctx)
        );
    }
    // the standard entries are still there
    for id in [
        "file.new",
        "file.open",
        "file.save",
        "file.save_as",
        "edit.undo",
        "edit.redo",
        "help.about",
        "help.shortcuts",
    ] {
        assert!(ids.contains(&id.to_string()), "{id}");
    }
}

#[test]
fn the_entries_that_cannot_be_chosen_follow_the_mode_and_the_history() {
    let model = MenuFacts {
        sketch_mode: false,
        can_undo: true,
        can_redo: false,
        can_save: true,
    };
    let off = disabled_entries(&model);
    assert!(off.contains(&"edit.cut".to_string()) && off.contains(&"edit.preferences".to_string()));
    assert!(off.contains(&"edit.redo".to_string()) && !off.contains(&"edit.undo".to_string()));
    assert!(!off.contains(&"file.save".to_string()));
    assert!(off.contains(&"tools.keep_sketch".to_string()));
    assert!(!off.contains(&export_id(Format::Stl)));
    assert!(!off.contains(&report_id(Panel::ALL[0])));
    // a read-only project cannot be saved
    let ro = disabled_entries(&MenuFacts {
        can_save: false,
        ..model
    });
    assert!(ro.contains(&"file.save".to_string()) && ro.contains(&"file.save_as".to_string()));
    // in the sketch the exports and the reports of the model are off, the sketch can be kept
    let sketch = disabled_entries(&MenuFacts {
        sketch_mode: true,
        can_undo: false,
        can_redo: false,
        can_save: true,
    });
    assert!(
        sketch.contains(&export_id(Format::Step))
            && sketch.contains(&"file.fit_report".to_string())
    );
    assert!(sketch.contains(&report_id(Panel::ALL[1])));
    assert!(!sketch.contains(&"tools.keep_sketch".to_string()));
    assert!(
        sketch.contains(&"edit.undo".to_string()) && !sketch.contains(&"file.save".to_string())
    );
    let sketch_ro = disabled_entries(&MenuFacts {
        sketch_mode: true,
        can_save: false,
        ..MenuFacts::default()
    });
    assert!(sketch_ro.contains(&"tools.keep_sketch".to_string()));
    // sorted and without repeats, and every id is an entry of the bar
    let bar_ids = entry_ids(&menu_bar());
    for set in [off, ro, sketch, sketch_ro] {
        let mut sorted = set.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(set, sorted);
        for id in &set {
            assert!(bar_ids.contains(id), "{id} is not an entry");
        }
    }
}

#[test]
fn the_language_in_use_is_the_one_that_is_ticked() {
    for l in Lang::ALL {
        let c = checks(l);
        assert_eq!(c.len(), 8);
        assert_eq!(c.iter().filter(|(_, on)| *on).count(), 1);
        assert!(c.contains(&(language_id(l), true)));
    }
}

#[test]
fn the_frame_is_kept_in_its_own_section_of_the_settings_file_and_the_rest_is_left_alone() {
    let c = core();
    let before = c.config_pairs();
    let user = "[window]\nwidth = 1000  # my screen\n\n[sketch]\nmax_iterations = 7\n";
    let text = with_dock_section(user, &before);
    assert!(text.starts_with(user), "{text}");
    assert!(
        text.contains("\n[dock]\n")
            && text.contains("version = 1\n")
            && text.contains("ws.1.tree = "),
        "{text}"
    );
    // the frame reads its own lines back
    let mut again = core();
    again.load_config(&dock_text_of(&text));
    assert!(again.warnings().is_empty(), "{:?}", again.warnings());
    assert_eq!(again.config_pairs(), before);
    // the file is still a settings file for every part of the program
    assert!(crate::config::Settings::with_user_config(&text).is_ok());
    assert!(crate::uiconfig::UiSettings::with_user_config(&text).is_ok());
    assert!(crate::sketch::settings::SketchSettings::with_user_config(&text).is_ok());
    assert!(crate::featuredag::HistorySettings::with_user_config(&text).is_ok());
    let ui = crate::uiconfig::UiSettings::with_user_config(&text).unwrap();
    assert_eq!(ui.window_size.0, 1000);
    // writing it twice gives the same file, and the section is replaced, not added
    assert_eq!(with_dock_section(&text, &before), text);
    let mut moved = core();
    moved.workspaces();
    assert!(moved
        .apply(&eh3d_dock::menu::DockAction::HidePanel(
            id::TASKS.to_string()
        ))
        .is_ok());
    let text2 = with_dock_section(&text, &moved.config_pairs());
    assert_eq!(text2.matches("[dock]").count(), 1);
    assert_ne!(text2, text);
    assert!(text2.starts_with(user));
    // a section after the frame's is kept
    let tail = format!("{text}\n[history]\nmax_nodes = 9\n");
    let swapped = with_dock_section(&tail, &moved.config_pairs());
    assert!(swapped.contains("[history]\nmax_nodes = 9\n"), "{swapped}");
    assert_eq!(swapped.matches("[dock]").count(), 1);
}

#[test]
fn the_line_ending_of_the_file_is_kept_and_a_file_without_the_section_has_no_lines_of_the_frame() {
    let pairs = vec![("dock.version".to_string(), "1".to_string())];
    let crlf = with_dock_section("[window]\r\nwidth = 900\r\n", &pairs);
    assert!(crlf.replace("\r\n", "").find('\n').is_none(), "{crlf:?}");
    assert!(crlf.ends_with("[dock]\r\nversion = 1\r\n"));
    assert_eq!(dock_text_of("[window]\nwidth = 900\n"), "");
    assert_eq!(dock_text_of(""), "");
    assert_eq!(with_dock_section("", &pairs), "[dock]\nversion = 1\n");
    // a value never breaks a line
    let odd = with_dock_section("", &[("dock.a".to_string(), "x\ny".to_string())]);
    assert_eq!(odd, "[dock]\na = xy\n");
    // comments and other sections are not the frame's
    assert_eq!(
        dock_text_of("# c\n[dock]\n# note\nversion = 1\n[other]\ncurrent = 5\n"),
        "dock.version=1\n"
    );
}

#[test]
fn a_damaged_section_never_stops_the_window_and_the_factory_layout_comes_back() {
    let mut c = core();
    c.load_config(&dock_text_of(
        "[dock]\nversion = 1\nws.1.tree = H200(((\nws.1.name = Model\n",
    ));
    assert!(c.layout_is_sound());
    let mut d = core();
    d.load_config(&dock_text_of("[dock]\nversion = 9\n"));
    assert!(d.layout_is_sound());
    let mut e = core();
    e.load_config(&dock_text_of(
        "[dock]\nws.1.tree = T0(nothing,view3d)\nws.1.name = Model\n",
    ));
    assert!(e.layout_is_sound());
}

#[test]
fn the_layout_is_written_only_when_it_changed() {
    let mut c = core();
    let pairs = c.config_pairs();
    let mut watch = LayoutWatch::new(&pairs);
    assert!(!watch.changed(&pairs));
    assert!(!watch.changed(&pairs));
    c.apply(&eh3d_dock::menu::DockAction::HidePanel(
        id::CONSOLE.to_string(),
    ))
    .unwrap();
    let moved = c.config_pairs();
    assert!(watch.changed(&moved));
    assert!(!watch.changed(&moved));
    c.apply(&eh3d_dock::menu::DockAction::ShowPanel(
        id::CONSOLE.to_string(),
    ))
    .unwrap();
    assert!(watch.changed(&c.config_pairs()));
}

#[test]
fn the_language_of_the_frame_is_the_one_of_the_window() {
    for l in Lang::ALL {
        assert_eq!(view_lang(l).code(), l.code());
    }
    assert_eq!(t8(Msg::SheetTitle).0[0], Msg::SheetTitle.tr(Lang::En));
    assert_eq!(t8(Msg::SheetTitle).0[2], Msg::SheetTitle.tr(Lang::It));
    assert_eq!(t8(Msg::SheetTitle).0[7], Msg::SheetTitle.tr(Lang::Ja));
}

/// A generator of numbers with a fixed seed, the same on every system.
struct Seed(u64);

impl Seed {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

#[test]
fn a_damaged_settings_file_never_stops_the_frame_and_the_layout_is_always_sound_with_a_fixed_seed()
{
    let mut seed = Seed(0x5eed_00f1);
    let mut c = core();
    c.apply(&eh3d_dock::menu::DockAction::HidePanel(
        id::TASKS.to_string(),
    ))
    .unwrap();
    let good = with_dock_section("[window]\nwidth = 1000\n", &c.config_pairs());
    for round in 0..3000 {
        let mut bytes = good.clone().into_bytes();
        for _ in 0..1 + seed.next() % 4 {
            if bytes.is_empty() {
                break;
            }
            let at = (seed.next() % bytes.len() as u64) as usize;
            match seed.next() % 4 {
                0 => bytes[at] ^= 1 << (seed.next() % 7),
                1 => bytes.truncate(at),
                2 => {
                    bytes.remove(at);
                }
                _ => bytes.insert(at, b"()[],;=.0123 TVHx_\n"[(seed.next() % 19) as usize]),
            }
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let mut dock = core();
        dock.load_config(&dock_text_of(&text));
        assert!(dock.layout_is_sound(), "round {round}: {text:?}");
        // whatever came out is written again in a form that reads back the same
        let again = with_dock_section("", &dock.config_pairs());
        let mut third = core();
        third.load_config(&dock_text_of(&again));
        assert_eq!(third.config_pairs(), dock.config_pairs(), "round {round}");
    }
}
