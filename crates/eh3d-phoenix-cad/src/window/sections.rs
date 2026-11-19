// SPDX-License-Identifier: GPL-3.0-or-later
//! The section panel of the window with several planes at once: the plane the panel is asked for
//! and the others typed as `axis=position`, each with its report, its small picture of the cut
//! outline and its drawing file. The model is cut into triangles once for all the planes.

use super::*;
use crate::section::{self, PreviewLine, Section};

/// The picture of one plane of the section panel: its title and the lines of the outline in the
/// pixels of the pane.
pub type PlanePicture = (String, Vec<PreviewLine>);

impl Window {
    /// The planes of the section panel: the one asked for first and then the ones typed in `extra`
    /// as `axis=position` separated by commas or semicolons (`x=10, z=5 mm`). A word that is not
    /// an axis, a pair with no `=` and more planes than `panels.section_max_planes` are refused.
    /// The positions are kept as the text typed; they are read when the model is cut.
    pub fn section_planes(
        &self,
        axis: Dim,
        position: &str,
        extra: &str,
    ) -> Result<Vec<(Dim, String)>, CadError> {
        let mut planes = vec![(axis, position.trim().to_string())];
        for pair in extra.split([',', ';']) {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let (word, at) = pair
                .split_once('=')
                .ok_or_else(|| CadError::Reference(pair.to_string()))?;
            let axis = Dim::from_name(word.trim())
                .ok_or_else(|| CadError::Reference(word.trim().to_string()))?;
            planes.push((axis, at.trim().to_string()));
        }
        if planes.len() > self.ui.panels.section_max_planes as usize {
            return Err(CadError::ValueOutOfRange("planes of the section"));
        }
        Ok(planes)
    }

    /// The section of the model by each of the planes, the model being cut into triangles once.
    fn cut_planes(&self, planes: &[(Dim, String)]) -> Result<Vec<Section>, CadError> {
        let bodies = crate::solid::bodies(&self.built.enclosure, &self.settings.mesh)?;
        planes
            .iter()
            .map(|(axis, at)| Ok(section::cut(&bodies, *axis, self.parse_text(at)?)))
            .collect()
    }

    /// The reports of the planes as lines of text in the language of the window, one report after
    /// the other with an empty line between them; a refusal is written as the text of its error.
    pub fn sections_panel_text(&self, axis: Dim, position: &str, extra: &str) -> String {
        let lang = self.lang;
        self.section_planes(axis, position, extra)
            .and_then(|planes| self.cut_planes(&planes))
            .map(|all| {
                all.iter()
                    .map(|s| section::text_in(s, lang))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|e| error_text(&e, lang))
    }

    /// The small pictures of the planes, each in a pane of the size of `panels.section_preview_size`
    /// with the margin of `panels.section_preview_margin`. A refusal gives no picture (the report
    /// says why).
    pub fn section_pictures(&self, axis: Dim, position: &str, extra: &str) -> Vec<PlanePicture> {
        let side = f64::from(self.ui.panels.section_preview_size);
        let margin = f64::from(self.ui.panels.section_preview_margin);
        let Ok(planes) = self.section_planes(axis, position, extra) else {
            return Vec::new();
        };
        let Ok(all) = self.cut_planes(&planes) else {
            return Vec::new();
        };
        all.iter()
            .map(|s| {
                let title = fill(
                    Msg::SecPlane.tr(self.lang),
                    &[s.axis.name(), &mm_text(s.position.get())],
                );
                (title, section::preview(s, side, side, margin))
            })
            .collect()
    }

    /// Writes the drawing of every plane next to the project, each in a file named after its plane
    /// (see [`Window::section_file_name`]), as the drawing of one section is written. It stops at
    /// the first refusal, which the status bar tells; otherwise the status bar says how many files
    /// were written. Returns how many were.
    pub fn write_section_set(
        &mut self,
        axis: Dim,
        position: &str,
        extra: &str,
        format: crate::panels::SketchFormat,
        overwrite: bool,
    ) -> usize {
        let planes = match self.section_planes(axis, position, extra) {
            Ok(planes) => planes,
            Err(e) => {
                self.status = Status::Error(e);
                return 0;
            }
        };
        let mut written = 0;
        for (axis, at) in &planes {
            let file = self.section_file_name(*axis, at, format);
            if !self.write_section_drawing(*axis, at, format, &file, overwrite) {
                return written;
            }
            written += 1;
        }
        written
    }
}
