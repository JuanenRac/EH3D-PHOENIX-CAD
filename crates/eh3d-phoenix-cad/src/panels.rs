// SPDX-License-Identifier: GPL-3.0-or-later
//! The reports of the window as text in the language of the person: the cutting plan, the hole
//! schedule and the list of tools, the clearance between parts, the table of placements, the mass
//! and the centre of mass, and the drawing of a sketch.
//!
//! The commands of the program print these reports as comma separated values (the same text in any
//! language, for a program to read); here every label and every sentence is a word of the window
//! in one of the eight languages, and the numbers are the ones of the commands. The state of the
//! panels of the window (which one is open, the choices in it) is kept here too, so that what a
//! panel does is tested without a screen.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::assembly::Dim;
use crate::balance::Balance;
use crate::catalog::{mass_g_text, PartInstance};
use crate::clearance::{self, ClearanceSettings};
use crate::enclosure::{Face, HoleStyle, Id};
use crate::lang::{fill, Lang, Msg};
use crate::nesting::Plan;
use crate::placement::Table;
use crate::revolve::Axis;
use crate::scad::mm_text;
use crate::schedule::{self, HoleRow};
use crate::uiconfig::PanelSettings;
use crate::CadError;

/// The panels of the Reports menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    /// The cutting plan of the profiles.
    CutPlan,
    /// The hole schedule and the list of tools.
    Holes,
    /// The pairs of parts that are too close.
    Clearance,
    /// The table of placements and the exploded view.
    Placements,
    /// The list of the placed parts with their properties.
    PartList,
    /// The mass and the centre of mass.
    Balance,
    /// A sketch written as a DXF or SVG drawing.
    SketchExport,
    /// The figures of a sketch turned about one of its axes.
    Revolve,
    /// The section of the model by a plane, with its drawing.
    Section,
    /// A flat DXF drawing read into a sketch of the project.
    ImportDxf,
    /// The bill of materials with the mass and the cost of each line.
    Bom,
    /// The mates of the model, the conflicts they leave and the editor of a new mate.
    Mates,
    /// The changes that can be undone and redone, and going to any of them.
    History,
}

impl Panel {
    /// Every panel, in the order of the menu.
    pub const ALL: [Panel; 13] = [
        Panel::CutPlan,
        Panel::Holes,
        Panel::Clearance,
        Panel::Placements,
        Panel::PartList,
        Panel::Balance,
        Panel::SketchExport,
        Panel::Revolve,
        Panel::Section,
        Panel::ImportDxf,
        Panel::Bom,
        Panel::Mates,
        Panel::History,
    ];

    /// The title of the panel, which is also the text of its menu entry.
    pub fn title(self) -> Msg {
        match self {
            Panel::CutPlan => Msg::PanelCutPlan,
            Panel::Holes => Msg::PanelHoles,
            Panel::Clearance => Msg::PanelClearance,
            Panel::Placements => Msg::PanelPlacements,
            Panel::PartList => Msg::PanelPartList,
            Panel::Balance => Msg::PanelBalance,
            Panel::SketchExport => Msg::PanelSketchExport,
            Panel::Revolve => Msg::PanelRevolve,
            Panel::Section => Msg::PanelSection,
            Panel::ImportDxf => Msg::PanelImportDxf,
            Panel::Bom => Msg::PanelBom,
            Panel::Mates => Msg::PanelMates,
            Panel::History => Msg::PanelHistory,
        }
    }
}

/// A mate being written in the mates panel: every value as the text the person types or picks, so
/// that a half-typed value is kept until the mate is added. The parts are places in the list that
/// [`crate::window::Window::mate_choices`] gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MateDraft {
    /// The kind of mate: `gap`, `align`, `stack`, `offset`, `symmetric` or `turn`.
    pub kind: String,
    /// The place of the part that stays, in the list of parts.
    pub fixed: usize,
    /// The place of the part that is placed, in the list of parts.
    pub placed: usize,
    /// The side of a `gap`: `+x`, `-x`, `+y`, `-y`, `+z` or `-z`.
    pub direction: String,
    /// The axis of an `align`, an `offset` or a `symmetric`: `x`, `y` or `z`.
    pub axis: String,
    /// The faces an `align` puts on one plane: `min`, `max` or `center`.
    pub edge: String,
    /// The distance of a `gap` or of an `offset`, typed as a length.
    pub distance: String,
    /// The position of the plane of a `symmetric`, typed as a length.
    pub plane: String,
    /// The roll of a `turn` in degrees (0, 90, 180 or 270).
    pub turn: String,
}

impl Default for MateDraft {
    /// A stack of the second part on the first, with the values of the shipped settings.
    fn default() -> MateDraft {
        MateDraft {
            kind: "stack".to_string(),
            fixed: 0,
            placed: 1,
            direction: "+z".to_string(),
            axis: "z".to_string(),
            edge: "min".to_string(),
            distance: "0".to_string(),
            plane: "0".to_string(),
            turn: "90".to_string(),
        }
    }
}

/// The two drawing formats of a sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchFormat {
    /// A DXF drawing.
    Dxf,
    /// An SVG drawing.
    Svg,
}

impl SketchFormat {
    /// The word of the settings file and of the command line.
    pub fn name(self) -> &'static str {
        match self {
            SketchFormat::Dxf => "dxf",
            SketchFormat::Svg => "svg",
        }
    }

    /// The format of a word of the settings file.
    pub fn from_name(name: &str) -> Option<SketchFormat> {
        match name {
            "dxf" => Some(SketchFormat::Dxf),
            "svg" => Some(SketchFormat::Svg),
            _ => None,
        }
    }

    /// Whether the format is DXF.
    pub fn is_dxf(self) -> bool {
        self == SketchFormat::Dxf
    }
}

/// What the person chose in the panels, read by [`crate::window::Window::panel_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PanelOptions {
    /// Show the drills and cutters instead of the holes.
    pub holes_tools: bool,
    /// Pull the parts apart by this percentage first.
    pub explode: Option<u32>,
    /// Add the totals of the bill of materials by material or by kind of piece.
    pub bom_group: Option<crate::bom::GroupBy>,
}

/// The state of the panels of the window: which one is open and what is chosen in it.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelState {
    /// The panel that is open, if any.
    pub open: Option<Panel>,
    /// The choice of the hole panel: tools instead of holes.
    pub holes_tools: bool,
    /// The choice of the placements panel: pull the parts apart.
    pub explode: bool,
    /// The percentage being typed for the exploded view.
    pub explode_text: String,
    /// The totals the bill of materials panel adds to its lines, if any.
    pub bom_group: Option<crate::bom::GroupBy>,
    /// The number of the sketch being drawn, among the sketches of the project.
    pub sketch: usize,
    /// The format of the drawing of a sketch.
    pub format: SketchFormat,
    /// The name of the file being typed; empty means the name suggested for the sketch.
    pub file: String,
    /// Whether a file that exists is written over.
    pub overwrite: bool,
    /// The axis the sketch of the revolve panel is turned about.
    pub revolve_axis: Axis,
    /// The angle being typed for the revolve panel, in degrees.
    pub revolve_angle: String,
    /// The material of the revolve panel; empty means none, and then no mass is given.
    pub revolve_material: String,
    /// The axis across the plane of the section panel.
    pub section_axis: Dim,
    /// The position of the plane being typed for the section panel.
    pub section_at: String,
    /// The other planes typed for the section panel, as `axis=position` separated by commas.
    pub section_extra: String,
    /// The path of the DXF drawing being typed for the import panel.
    pub import_file: String,
    /// The name being typed for the sketch of the import panel; empty means the name of the file.
    pub import_name: String,
    /// The face the sketch of the import panel is made on.
    pub import_face: Face,
    /// The unit of the drawing of the import panel (`mm`, `cm`, `m` or `in`).
    pub import_unit: String,
    /// What the last import read, as text in the language it was made in; empty before one.
    pub import_report: String,
    /// The layers typed for the import panel, separated by commas; empty means all of them.
    pub import_layers: String,
    /// The lines of the picture of the drawing the last preview made of the import panel.
    pub import_lines: Vec<crate::section::PreviewLine>,
    /// The text of that preview (the layers and what the choice would read); empty before one.
    pub import_look: String,
    /// The mate being written in the mates panel.
    pub mate: MateDraft,
    /// The number of the step being typed in the history panel to go to.
    pub history_goto: String,
}

impl PanelState {
    /// The state a window starts with, from the settings (`[panels]` and `export.overwrite`).
    pub fn new(settings: &PanelSettings, overwrite: bool) -> PanelState {
        PanelState {
            open: None,
            holes_tools: settings.holes_tools,
            explode: settings.explode,
            explode_text: settings.explode_percent.to_string(),
            bom_group: None,
            sketch: 0,
            format: settings.sketch_format,
            file: String::new(),
            overwrite,
            revolve_axis: settings.revolve_axis,
            revolve_angle: settings.revolve_angle.to_string(),
            revolve_material: String::new(),
            section_axis: settings.section_axis,
            section_at: crate::scad::mm_text(settings.section_position.get()),
            section_extra: String::new(),
            import_file: String::new(),
            import_name: String::new(),
            import_face: settings.import_face,
            import_unit: settings.import_unit.clone(),
            import_report: String::new(),
            import_layers: String::new(),
            import_lines: Vec::new(),
            import_look: String::new(),
            mate: MateDraft::default(),
            history_goto: String::new(),
        }
    }

    /// What the report of the open panel is asked for.
    pub fn options(&self) -> PanelOptions {
        PanelOptions {
            holes_tools: self.holes_tools,
            explode: if self.explode {
                self.explode_text.trim().parse::<u32>().ok()
            } else {
                None
            },
            bom_group: self.bom_group,
        }
    }
}

/// The name suggested for the file of the drawing of a sketch.
pub fn default_drawing_name(sketch: &str, format: SketchFormat) -> String {
    format!("{sketch}.{}", format.name())
}

/// Where the file of a drawing goes: next to the project that is open (or in the current folder
/// without one). The name is only a name: a name that holds a folder, `..` or nothing is refused
/// with `PHOENIX-CAD-041`, so the panel never writes outside the folder of the project.
pub fn drawing_target(project: Option<&Path>, name: &str) -> Result<PathBuf, CadError> {
    let name = name.trim();
    let bare = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control);
    if !bare {
        return Err(CadError::Export(
            "the name of the file must be a name and not a folder".into(),
        ));
    }
    let folder = project.and_then(Path::parent).map(Path::to_path_buf);
    Ok(match folder {
        Some(f) if !f.as_os_str().is_empty() => f.join(name),
        _ => PathBuf::from(name),
    })
}

/// The word of a face.
pub fn face_msg(face: &str) -> Option<Msg> {
    Some(match Face::from_name(face)? {
        Face::Front => Msg::FaceFront,
        Face::Back => Msg::FaceBack,
        Face::Left => Msg::FaceLeft,
        Face::Right => Msg::FaceRight,
        Face::Top => Msg::FaceTop,
        Face::Bottom => Msg::FaceBottom,
    })
}

/// The shape of a hole in words: its kind and what else makes it, in the language.
pub fn style_text(shape: &HoleStyle, lang: Lang) -> String {
    match shape {
        HoleStyle::Plain => Msg::StylePlain.tr(lang).to_string(),
        HoleStyle::Countersunk { head_diameter } => format!(
            "{} ({})",
            Msg::StyleCountersunk.tr(lang),
            fill(Msg::DetailHead.tr(lang), &[&mm_text(head_diameter.get())])
        ),
        HoleStyle::Counterbored {
            head_diameter,
            depth,
        } => format!(
            "{} ({})",
            Msg::StyleCounterbored.tr(lang),
            fill(
                Msg::DetailHeadDepth.tr(lang),
                &[&mm_text(head_diameter.get()), &mm_text(depth.get())]
            )
        ),
        HoleStyle::Rectangular { height } => format!(
            "{} ({})",
            Msg::StyleRectangular.tr(lang),
            fill(Msg::DetailHeight.tr(lang), &[&mm_text(height.get())])
        ),
        HoleStyle::Oval { length, vertical } => {
            let way = if *vertical {
                Msg::DetailVertical
            } else {
                Msg::DetailHorizontal
            };
            format!(
                "{} ({})",
                Msg::StyleSlot.tr(lang),
                fill(
                    Msg::DetailSlot.tr(lang),
                    &[&mm_text(length.get()), way.tr(lang)]
                )
            )
        }
    }
}

/// The mass and the centre of mass as lines of text in a language.
pub fn balance_text_in(b: &Balance, lang: Lang) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}",
        fill(Msg::BalParts.tr(lang), &[&b.parts.to_string()])
    );
    let _ = writeln!(
        out,
        "{}",
        fill(Msg::BalMass.tr(lang), &[&mass_g_text(b.mass_ug)])
    );
    match b.centre_nm {
        Some(c) => {
            let _ = writeln!(
                out,
                "{}",
                fill(
                    Msg::BalCentre.tr(lang),
                    &[&mm_text(c[0]), &mm_text(c[1]), &mm_text(c[2])]
                )
            );
        }
        None => {
            let _ = writeln!(out, "{}", Msg::BalCentreNone.tr(lang));
        }
    }
    let _ = writeln!(
        out,
        "{}",
        fill(
            Msg::BalEstimated.tr(lang),
            &[&b.estimated.to_string(), &b.parts.to_string()]
        )
    );
    out
}

/// The cutting plan as lines of text in a language.
pub fn cutplan_text_in(plan: &Plan, lang: Lang) -> String {
    let mut out = String::new();
    for line in &plan.lines {
        let ppm = line.yield_ppm();
        let used = format!("{}.{}", ppm / 10_000, (ppm / 1_000) % 10);
        let _ = writeln!(
            out,
            "{}",
            fill(
                Msg::CutLine.tr(lang),
                &[
                    &line.part,
                    &line.material,
                    &line.bars.len().to_string(),
                    &mm_text(line.bar_nm),
                    &line.pieces().to_string(),
                    &used,
                ]
            )
        );
        for (n, bar) in line.bars.iter().enumerate() {
            let pieces: Vec<String> = bar.pieces.iter().map(|p| mm_text(*p)).collect();
            let _ = writeln!(
                out,
                "{}",
                fill(
                    Msg::CutBar.tr(lang),
                    &[
                        &(n + 1).to_string(),
                        &pieces.join(" + "),
                        &mm_text(bar.offcut_nm)
                    ]
                )
            );
        }
    }
    let _ = writeln!(
        out,
        "{}",
        fill(Msg::CutTotal.tr(lang), &[&plan.bars().to_string()])
    );
    out
}

/// The hole schedule as lines of text in a language: a line for each hole and the number of
/// holes.
pub fn holes_text_in(rows: &[HoleRow], lang: Lang) -> String {
    let mut out = String::new();
    for r in rows {
        let face = face_msg(r.face).map_or_else(|| r.face.to_string(), |m| m.tr(lang).to_string());
        let _ = writeln!(
            out,
            "{}",
            fill(
                Msg::HoleLine.tr(lang),
                &[
                    &r.tag,
                    &r.name,
                    &face,
                    &mm_text(r.u),
                    &mm_text(r.v),
                    &mm_text(r.diameter),
                    &style_text(&r.shape, lang),
                ]
            )
        );
    }
    let _ = writeln!(
        out,
        "{}",
        fill(Msg::HoleTotal.tr(lang), &[&rows.len().to_string()])
    );
    out
}

/// The drills and cutters the holes need as lines of text in a language: for each diameter and
/// shape how many holes, the smallest diameter first, and the number of holes.
pub fn tools_text_in(rows: &[HoleRow], lang: Lang) -> String {
    let mut out = String::new();
    for (d, style, detail, n) in schedule::tools(rows) {
        let shape = rows
            .iter()
            .find(|r| r.diameter == d && r.style == style && r.detail == detail)
            .map_or_else(|| style.to_string(), |r| style_text(&r.shape, lang));
        let _ = writeln!(
            out,
            "{}",
            fill(
                Msg::HoleToolLine.tr(lang),
                &[&mm_text(d), &shape, &n.to_string()]
            )
        );
    }
    let _ = writeln!(
        out,
        "{}",
        fill(Msg::HoleTotal.tr(lang), &[&rows.len().to_string()])
    );
    out
}

/// The pairs of parts that are too close as lines of text in a language, the closest first, at
/// most `settings.max_pairs` of them, then how many more there are and the number of pairs.
pub fn clearance_text_in(
    report: &clearance::Report,
    parts: &BTreeMap<Id, PartInstance>,
    settings: &ClearanceSettings,
    lang: Lang,
) -> String {
    let mut out = String::new();
    for p in report.pairs.iter().take(settings.max_pairs) {
        let _ = writeln!(
            out,
            "{}",
            fill(
                Msg::ClrPair.tr(lang),
                &[
                    &clearance::name_of(parts, &p.a),
                    &clearance::name_of(parts, &p.b),
                    &mm_text(p.distance_nm())
                ]
            )
        );
    }
    if report.pairs.len() > settings.max_pairs {
        let more = report.pairs.len() - settings.max_pairs;
        let _ = writeln!(
            out,
            "{}",
            fill(Msg::ReportConflictMore.tr(lang), &[&more.to_string()])
        );
    }
    let _ = writeln!(
        out,
        "{}",
        fill(
            Msg::ClrTotal.tr(lang),
            &[
                &report.pairs.len().to_string(),
                &mm_text(settings.min_gap.get())
            ]
        )
    );
    out
}

/// The table of placements as lines of text in a language: a line for each part and a last line
/// with the box of the assembly.
pub fn placements_text_in(t: &Table, lang: Lang) -> String {
    let mut out = String::new();
    for r in &t.rows {
        let _ = writeln!(
            out,
            "{}",
            fill(
                Msg::PlRow.tr(lang),
                &[
                    &r.name,
                    &r.part,
                    &mm_text(r.min[0]),
                    &mm_text(r.min[1]),
                    &mm_text(r.min[2]),
                    &mm_text(r.size[0]),
                    &mm_text(r.size[1]),
                    &mm_text(r.size[2]),
                    &r.turn,
                ]
            )
        );
    }
    let min = t.min.unwrap_or([0; 3]);
    let _ = writeln!(
        out,
        "{}",
        fill(
            Msg::PlTotal.tr(lang),
            &[
                &t.rows.len().to_string(),
                &mm_text(min[0]),
                &mm_text(min[1]),
                &mm_text(min[2]),
                &mm_text(t.size[0]),
                &mm_text(t.size[1]),
                &mm_text(t.size[2]),
            ]
        )
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nesting::{Bar, StockLine};
    use crate::Length;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn row(tag: &str, face: &'static str, shape: HoleStyle) -> HoleRow {
        HoleRow {
            tag: tag.into(),
            name: "usb".into(),
            face,
            u: 30_000_000,
            v: 15_500_000,
            diameter: 8_000_000,
            style: crate::schedule::style_name(&shape),
            detail: String::new(),
            shape,
        }
    }

    #[test]
    fn the_balance_reads_the_same_in_english_as_the_text_of_the_command() {
        let b = Balance {
            parts: 2,
            estimated: 1,
            mass_ug: 16_000_000,
            centre_nm: Some([55_000_000, 5_000_000, 5_000_000]),
        };
        assert_eq!(
            balance_text_in(&b, Lang::En),
            crate::balance::balance_text(&b)
        );
        let none = Balance::default();
        assert_eq!(
            balance_text_in(&none, Lang::En),
            crate::balance::balance_text(&none)
        );
    }

    #[test]
    fn the_balance_is_written_in_each_language() {
        let b = Balance {
            parts: 2,
            estimated: 1,
            mass_ug: 16_000_000,
            centre_nm: Some([55_000_000, 5_000_000, 5_000_000]),
        };
        let es = balance_text_in(&b, Lang::Es);
        assert!(es.starts_with("piezas: 2\nmasa: 16.000 g\ncentro de masa: 55 5 5 mm\n"));
        assert!(balance_text_in(&b, Lang::Pt).contains("centro de massa: 55 5 5 mm"));
        assert!(balance_text_in(&b, Lang::Ja).contains("重心: 55 5 5 mm"));
        // every language says the same numbers
        for l in Lang::ALL {
            let t = balance_text_in(&b, l);
            assert!(t.contains("16.000"), "{}", l.code());
            assert_eq!(t.lines().count(), 4, "{}", l.code());
        }
    }

    #[test]
    fn the_cutting_plan_reads_the_same_in_english_as_the_text_of_the_command() {
        let plan = Plan {
            lines: vec![StockLine {
                part: "b".into(),
                material: "aluminium".into(),
                bar_nm: 1_000_000_000,
                bars: vec![
                    Bar {
                        pieces: vec![400_000_000, 300_000_000, 200_000_000],
                        offcut_nm: 81_000_000,
                    },
                    Bar {
                        pieces: vec![300_000_000],
                        offcut_nm: 687_000_000,
                    },
                ],
            }],
        };
        assert_eq!(
            cutplan_text_in(&plan, Lang::En),
            crate::nesting::plan_text(&plan)
        );
        let de = cutplan_text_in(&plan, Lang::De);
        assert!(de.starts_with("b (aluminium): 2 Stangen zu 1000 mm, 4 Stücke, 60.0 % genutzt\n"));
        assert!(de.contains("  Stange 1: 400 + 300 + 200 (Rest 81 mm)\n"));
        assert!(de.ends_with("insgesamt 2 Stangen\n"));
    }

    #[test]
    fn a_hole_is_described_by_its_face_and_its_shape_in_the_language() {
        let rows = vec![
            row("H1", "back", HoleStyle::Plain),
            row(
                "H2",
                "top",
                HoleStyle::Oval {
                    length: mm(12),
                    vertical: true,
                },
            ),
        ];
        let en = holes_text_in(&rows, Lang::En);
        assert_eq!(
            en,
            "H1 usb: Back face, at 30 15.5 mm, diameter 8 mm, plain\n\
             H2 usb: Top face, at 30 15.5 mm, diameter 8 mm, slot (length 12 vertical)\n\
             holes: 2\n"
        );
        let fr = holes_text_in(&rows, Lang::Fr);
        assert!(fr.contains("H1 usb : face Arrière, en 30 15.5 mm, diamètre 8 mm, simple\n"));
        assert!(fr.contains("rainure (longueur 12 vertical)"));
        let zh = holes_text_in(&rows, Lang::Zh);
        assert!(zh.contains("长孔 (长度 12 竖直)"));
    }

    #[test]
    fn the_tools_are_grouped_and_counted_in_the_language() {
        let rows = vec![
            row("H1", "back", HoleStyle::Plain),
            row("H2", "front", HoleStyle::Plain),
            row(
                "H3",
                "front",
                HoleStyle::Countersunk {
                    head_diameter: mm(16),
                },
            ),
        ];
        let text = tools_text_in(&rows, Lang::Es);
        assert!(text.contains("8 mm, simple: 2\n"), "{text}");
        assert!(text.contains("8 mm, avellanado (cabeza 16): 1\n"), "{text}");
        assert!(text.ends_with("agujeros: 3\n"));
    }

    #[test]
    fn a_bad_name_for_a_drawing_is_refused_and_a_good_one_goes_next_to_the_project() {
        let project = Path::new("work").join("box.phoenixproj");
        let ok = drawing_target(Some(&project), " plate.dxf ").unwrap();
        assert_eq!(ok, Path::new("work").join("plate.dxf"));
        assert_eq!(
            drawing_target(None, "plate.svg").unwrap(),
            PathBuf::from("plate.svg")
        );
        for bad in [
            "", "  ", ".", "..", "../x.dxf", "a/b.dxf", "a\\b.dxf", "c:x.dxf", "a\nb",
        ] {
            let e = drawing_target(Some(&project), bad).unwrap_err();
            assert_eq!(e.code(), "PHOENIX-CAD-041", "{bad:?}");
        }
    }

    #[test]
    fn the_state_of_the_panels_starts_from_the_settings() {
        let s = PanelSettings::shipped();
        let st = PanelState::new(&s, false);
        assert_eq!(st.open, None);
        assert_eq!(st.options(), PanelOptions::default());
        assert!(!st.overwrite);
        assert_eq!(st.revolve_axis, Axis::X);
        assert_eq!(st.revolve_angle, "360");
        assert!(st.revolve_material.is_empty());
        assert_eq!((st.section_axis, st.section_at.as_str()), (Dim::Z, "0"));
        assert!(st.import_file.is_empty() && st.import_name.is_empty());
        assert_eq!(
            (st.import_face, st.import_unit.as_str()),
            (Face::Front, "mm")
        );
        assert!(st.import_report.is_empty());
        assert!(st.import_layers.is_empty() && st.import_look.is_empty());
        assert!(st.import_lines.is_empty());
        let mut st = PanelState::new(&s, true);
        assert!(st.overwrite);
        st.explode = true;
        st.explode_text = " 40 ".into();
        assert_eq!(st.options().explode, Some(40));
        st.explode_text = "forty".into();
        assert_eq!(st.options().explode, None);
        assert_eq!(
            default_drawing_name("outline", SketchFormat::Svg),
            "outline.svg"
        );
        assert_eq!(SketchFormat::from_name("dxf"), Some(SketchFormat::Dxf));
        assert_eq!(SketchFormat::from_name("pdf"), None);
    }

    #[test]
    fn every_panel_has_a_title_in_each_language() {
        for p in Panel::ALL {
            for l in Lang::ALL {
                assert!(!p.title().tr(l).is_empty());
            }
        }
        for face in ["front", "back", "left", "right", "top", "bottom"] {
            assert!(face_msg(face).is_some());
        }
        assert!(face_msg("middle").is_none());
    }
}
