// SPDX-License-Identifier: GPL-3.0-or-later
//! The title block of a drawing: the table in the lower right corner of a sheet that says what
//! the drawing is, who drew it and at what scale.
//!
//! The block has four rows and is as wide as the settings say: the organisation, the title, the
//! number of the drawing with its revision, and the author with the scale. The words that name
//! the cells are words of the window in the language asked for; the values are typed in the
//! settings (`[title_block]`) and go in as they are, so a person can write them in any language.
//! Nothing is drawn unless `title_block.enabled` is 1. The block is made of lines and texts in
//! millimetres under the box of the drawing, aligned to its right edge, and
//! [`crate::drawing::Drawing`] writes it into the DXF file (layer `ROTULO`) and the SVG file.
//! Neither file has been opened in a drawing program.

use crate::config::Config;
use crate::lang::{Lang, Msg};
use crate::CadError;
use crate::Length;

/// The layer of the title block in the DXF file.
pub const LAYER: &str = "ROTULO";

/// The most characters of a value of the title block.
pub const MAX_TEXT: usize = 80;

/// The settings of the title block (`[title_block]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleBlockSettings {
    /// Whether the drawings have a title block.
    pub enabled: bool,
    /// The organisation.
    pub organisation: String,
    /// The title; empty means the name of the project (or of the sketch).
    pub title: String,
    /// The number of the drawing.
    pub number: String,
    /// The revision.
    pub revision: String,
    /// The author.
    pub author: String,
    /// The width of the block.
    pub width: Length,
    /// The height of one of the four rows.
    pub row: Length,
}

fn check_text(key: &str, value: &str) -> Result<String, CadError> {
    if value.chars().count() > MAX_TEXT || value.chars().any(char::is_control) {
        return Err(CadError::Config(format!(
            "{key} is at most {MAX_TEXT} characters on one line"
        )));
    }
    Ok(value.to_owned())
}

impl TitleBlockSettings {
    /// The least width of the block, in nanometres (60 mm).
    pub const WIDTH_MIN: i64 = 60_000_000;
    /// The greatest width of the block, in nanometres (400 mm).
    pub const WIDTH_MAX: i64 = 400_000_000;
    /// The least height of a row, in nanometres (4 mm).
    pub const ROW_MIN: i64 = 4_000_000;
    /// The greatest height of a row, in nanometres (20 mm).
    pub const ROW_MAX: i64 = 20_000_000;

    /// The settings of the shipped file: no title block.
    pub fn shipped() -> TitleBlockSettings {
        crate::config::Settings::defaults().title_block
    }

    /// No title block, with the width and the row of the shipped file.
    pub fn off() -> TitleBlockSettings {
        TitleBlockSettings {
            enabled: false,
            ..TitleBlockSettings::shipped()
        }
    }

    /// Reads and checks the keys of `[title_block]`.
    pub fn from_config(c: &Config) -> Result<TitleBlockSettings, CadError> {
        let enabled = match c.number("title_block.enabled")? {
            0 => false,
            1 => true,
            _ => {
                return Err(CadError::Config(
                    "title_block.enabled must be 0 or 1".into(),
                ))
            }
        };
        let text = |key: &str| -> Result<String, CadError> {
            let value = c
                .get(key)
                .ok_or_else(|| CadError::Config(format!("{key} is missing")))?;
            check_text(key, value)
        };
        let width = c.length("title_block.width")?;
        if !(Self::WIDTH_MIN..=Self::WIDTH_MAX).contains(&width.get()) {
            return Err(CadError::Config(
                "title_block.width must be from 60 mm to 400 mm".into(),
            ));
        }
        let row = c.length("title_block.row")?;
        if !(Self::ROW_MIN..=Self::ROW_MAX).contains(&row.get()) {
            return Err(CadError::Config(
                "title_block.row must be from 4 mm to 20 mm".into(),
            ));
        }
        Ok(TitleBlockSettings {
            enabled,
            organisation: text("title_block.organisation")?,
            title: text("title_block.title")?,
            number: text("title_block.number")?,
            revision: text("title_block.revision")?,
            author: text("title_block.author")?,
            width,
            row,
        })
    }
}

/// One thing of the title block, in millimetres.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// A straight line.
    Line((f64, f64), (f64, f64)),
    /// A text whose lower left corner is at `at`.
    Text {
        /// The lower left corner of the text.
        at: (f64, f64),
        /// The height of the letters.
        height: f64,
        /// The text.
        text: String,
    },
}

/// The title block laid out on a sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// The lines and the texts.
    pub items: Vec<Item>,
    /// The lower left corner of the block.
    pub lo: (f64, f64),
    /// The upper right corner of the block.
    pub hi: (f64, f64),
}

fn mm(nm: i64) -> f64 {
    nm as f64 / 1_000_000.0
}

/// The title block with its upper right corner at `top_right`. `fallback` is the title when the
/// settings have none, `scale` the text of the scale (`1:1`) and `lang` the language of the words
/// that name the cells.
pub fn layout(
    settings: &TitleBlockSettings,
    fallback: &str,
    scale: &str,
    top_right: (f64, f64),
    lang: Lang,
) -> Block {
    let (w, r) = (mm(settings.width.get()), mm(settings.row.get()));
    let (x0, y1) = (top_right.0 - w, top_right.1);
    let y0 = y1 - 4.0 * r;
    let mid = x0 + w / 2.0;
    let mut items = Vec::new();
    let rect = [(x0, y0), (top_right.0, y0), (top_right.0, y1), (x0, y1)];
    for i in 0..4 {
        items.push(Item::Line(rect[i], rect[(i + 1) % 4]));
    }
    for k in 1..4 {
        let y = y1 - f64::from(k) * r;
        items.push(Item::Line((x0, y), (top_right.0, y)));
    }
    items.push(Item::Line((mid, y1 - 2.0 * r), (mid, y0)));
    let title = if settings.title.is_empty() {
        fallback
    } else {
        settings.title.as_str()
    };
    // (label, value, left edge of the cell, top of the cell)
    let cells: [(Msg, &str, f64, f64); 6] = [
        (Msg::TbOrganisation, &settings.organisation, x0, y1),
        (Msg::TbTitle, title, x0, y1 - r),
        (Msg::TbNumber, &settings.number, x0, y1 - 2.0 * r),
        (Msg::TbRevision, &settings.revision, mid, y1 - 2.0 * r),
        (Msg::TbAuthor, &settings.author, x0, y1 - 3.0 * r),
        (Msg::TbScale, scale, mid, y1 - 3.0 * r),
    ];
    for (label, value, left, top) in cells {
        items.push(Item::Text {
            at: (left + 0.5, top - 0.32 * r),
            height: 0.24 * r,
            text: label.tr(lang).to_owned(),
        });
        if !value.is_empty() {
            items.push(Item::Text {
                at: (left + 0.5, top - 0.88 * r),
                height: 0.4 * r,
                text: value.to_owned(),
            });
        }
    }
    Block {
        items,
        lo: (x0, y0),
        hi: top_right,
    }
}

/// A text for the DXF file: printable ASCII as it is, any other character as `\U+XXXX` (the
/// escape that the DXF readers understand), and a character that does not fit in four digits as
/// a question mark.
pub fn dxf_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        let code = u32::from(ch);
        if (0x20..0x7f).contains(&code) {
            out.push(ch);
        } else if code <= 0xffff && !ch.is_control() {
            out.push_str(&format!("\\U+{code:04X}"));
        } else {
            out.push('?');
        }
    }
    out
}

/// A text for the SVG file: the five characters that XML keeps for itself are written as
/// entities.
pub fn xml_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    fn mm_len(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn settings() -> TitleBlockSettings {
        TitleBlockSettings {
            enabled: true,
            organisation: "Workshop".into(),
            title: String::new(),
            number: "A-12".into(),
            revision: "B".into(),
            author: "Ana".into(),
            width: mm_len(100),
            row: mm_len(8),
        }
    }

    #[test]
    fn the_shipped_file_has_no_title_block_and_every_key_is_checked() {
        let s = TitleBlockSettings::shipped();
        assert!(!s.enabled);
        assert_eq!(s.width, mm_len(120));
        assert_eq!(s.row, mm_len(7));
        assert_eq!(s.revision, "A");
        let own = Settings::with_user_config(
            "[title_block]
enabled = 1
organisation = Electro Hobby 3D
title = Front panel
number = P-001
revision = C
author = JuanenRac
width = 90 mm
row = 6 mm
",
        )
        .unwrap();
        assert!(own.title_block.enabled);
        assert_eq!(own.title_block.title, "Front panel");
        assert_eq!(own.title_block.width, mm_len(90));
        for bad in [
            "enabled = 2",
            "enabled = yes",
            "width = 59 mm",
            "width = 401 mm",
            "row = 3 mm",
            "row = 21 mm",
        ] {
            let text = format!("[title_block]\n{bad}\n");
            assert!(Settings::with_user_config(&text).is_err(), "{bad}");
        }
        let long = format!("[title_block]\ntitle = {}\n", "x".repeat(MAX_TEXT + 1));
        assert!(Settings::with_user_config(&long).is_err());
        let fits = format!("[title_block]\ntitle = {}\n", "x".repeat(MAX_TEXT));
        assert!(Settings::with_user_config(&fits).is_ok());
    }

    #[test]
    fn the_block_has_four_rows_of_the_height_of_the_settings_under_the_corner_given() {
        let b = layout(&settings(), "box", "1:2", (150.0, -10.0), Lang::En);
        // 100 mm wide and 4 x 8 mm high, with the upper right corner where it was asked
        assert_eq!(b.hi, (150.0, -10.0));
        assert_eq!(b.lo, (50.0, -42.0));
        let lines: Vec<_> = b
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Line(a, b) => Some((*a, *b)),
                Item::Text { .. } => None,
            })
            .collect();
        // the frame (4), three lines between the rows and the divider of the two lower rows
        assert_eq!(lines.len(), 8);
        assert!(lines.contains(&((50.0, -18.0), (150.0, -18.0))));
        assert!(lines.contains(&((50.0, -34.0), (150.0, -34.0))));
        assert!(lines.contains(&((100.0, -26.0), (100.0, -42.0))));
    }

    #[test]
    fn the_cells_say_their_name_in_the_language_and_hold_the_values() {
        let texts = |lang| -> Vec<String> {
            layout(&settings(), "box", "1:2", (150.0, 0.0), lang)
                .items
                .into_iter()
                .filter_map(|i| match i {
                    Item::Text { text, .. } => Some(text),
                    Item::Line(..) => None,
                })
                .collect()
        };
        let en = texts(Lang::En);
        assert_eq!(
            en,
            vec![
                "Organisation",
                "Workshop",
                "Title",
                "box",
                "Drawing number",
                "A-12",
                "Revision",
                "B",
                "Author",
                "Ana",
                "Scale",
                "1:2"
            ]
        );
        let de = texts(Lang::De);
        assert!(de.contains(&"Zeichnungsnummer".to_owned()));
        assert!(de.contains(&"Maßstab".to_owned()));
        assert!(de.contains(&"Workshop".to_owned()));
        // a cell without a value draws only its name
        let mut empty = settings();
        empty.author.clear();
        empty.title = "Front".into();
        let t: Vec<String> = layout(&empty, "box", "1:1", (10.0, 0.0), Lang::Es)
            .items
            .into_iter()
            .filter_map(|i| match i {
                Item::Text { text, .. } => Some(text),
                Item::Line(..) => None,
            })
            .collect();
        assert_eq!(t.len(), 11);
        assert!(t.contains(&"Front".to_owned()) && !t.contains(&"box".to_owned()));
        assert!(t.contains(&"Autor".to_owned()));
    }

    #[test]
    fn the_texts_are_escaped_for_each_file() {
        assert_eq!(dxf_text("Panel 1"), "Panel 1");
        assert_eq!(dxf_text("Ø 8"), "\\U+00D8 8");
        assert_eq!(dxf_text("日本"), "\\U+65E5\\U+672C");
        assert_eq!(dxf_text("a\nb\u{1F600}"), "a?b?");
        assert_eq!(
            xml_text("a & b < c > \"d\" 'e'"),
            "a &amp; b &lt; c &gt; &quot;d&quot; &apos;e&apos;"
        );
        assert_eq!(xml_text("plain"), "plain");
    }
}
