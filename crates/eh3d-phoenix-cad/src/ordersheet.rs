// SPDX-License-Identifier: GPL-3.0-or-later
//! The order sheet: what has to be bought and made to build the model, on one page. It brings
//! together the cutting plan of the profiles (the bars to buy and what is sawn from each), the
//! holes to drill, the parts of the catalog with their quantities and the cost with its totals,
//! as one self-contained HTML file: the style is inside the file, there is no script, no picture
//! and no link, and every piece of text that comes from the project (a name, a material) is
//! escaped, so the file can be opened, printed or sent as it is.
//!
//! The words are in the language of the export (any of the eight). The sheet is made from the
//! figures the other reports use (`nesting`, `schedule` and `bom`), so it never disagrees with
//! them. A table with more rows than the limit of the settings (`order.max_rows`) is refused with
//! `PHOENIX-CAD-050` before the page is written.

use std::fmt::Write as _;

use crate::bom::{money_text, Bom};
use crate::catalog::{mass_g_text, volume_mm3_text};
use crate::lang::{fill, Lang, Msg};
use crate::nesting::Plan;
use crate::panels::{face_msg, style_text};
use crate::scad::mm_text;
use crate::schedule::HoleRow;
use crate::window::kind_msg;
use crate::CadError;

/// What goes on the sheet.
pub struct Sheet<'a> {
    /// The title: the name of the project.
    pub title: &'a str,
    /// The language of the words.
    pub lang: Lang,
    /// The bars to buy and the pieces sawn from them.
    pub plan: &'a Plan,
    /// The holes to drill.
    pub holes: &'a [HoleRow],
    /// The parts of the catalog with their cost.
    pub bom: &'a Bom,
    /// The most rows any table may have.
    pub max_rows: usize,
}

/// The text with the characters that mean something in HTML written as entities.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

const STYLE: &str = "body{font-family:sans-serif;margin:2em;color:#222}\
h1{font-size:1.4em}h2{font-size:1.1em;margin-top:1.6em}\
table{border-collapse:collapse;margin:.4em 0}\
th,td{border:1px solid #999;padding:.2em .6em;text-align:left}\
th{background:#eee}td.n{text-align:right}p.total{font-weight:bold}";

fn too_many(rows: usize, max: usize) -> CadError {
    CadError::ExportLimit(format!(
        "the order sheet has a table of {rows} rows, more than the {max} the settings allow"
    ))
}

fn table(out: &mut String, heads: &[&str], rows: &[Vec<(String, bool)>]) {
    out.push_str("<table><thead><tr>");
    for h in heads {
        let _ = write!(out, "<th>{}</th>", escape(h));
    }
    out.push_str("</tr></thead><tbody>");
    for row in rows {
        out.push_str("<tr>");
        for (cell, number) in row {
            let class = if *number { " class=\"n\"" } else { "" };
            let _ = write!(out, "<td{class}>{}</td>", escape(cell));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
}

fn text(s: impl Into<String>) -> (String, bool) {
    (s.into(), false)
}

fn number(s: impl Into<String>) -> (String, bool) {
    (s.into(), true)
}

/// The HTML page of a sheet.
pub fn html(sheet: &Sheet) -> Result<String, CadError> {
    let lang = sheet.lang;
    let tr = |m: Msg| m.tr(lang);
    let mut body = String::new();
    let empty = sheet.plan.lines.is_empty() && sheet.holes.is_empty() && sheet.bom.lines.is_empty();
    if empty {
        let _ = write!(body, "<p>{}</p>", escape(tr(Msg::OrderNone)));
    }

    if !sheet.plan.lines.is_empty() {
        let _ = write!(body, "<h2>{}</h2>", escape(tr(Msg::OrderCuts)));
        for line in &sheet.plan.lines {
            if line.bars.len() > sheet.max_rows {
                return Err(too_many(line.bars.len(), sheet.max_rows));
            }
            let _ = write!(
                body,
                "<h3>{} ({})</h3><p>{}</p>",
                escape(&line.part),
                escape(&line.material),
                escape(&fill(
                    tr(Msg::OrderBars),
                    &[
                        &line.bars.len().to_string(),
                        &mm_text(line.bar_nm),
                        &line.pieces().to_string()
                    ]
                ))
            );
            let rows: Vec<Vec<(String, bool)>> = line
                .bars
                .iter()
                .enumerate()
                .map(|(n, bar)| {
                    let pieces: Vec<String> = bar.pieces.iter().map(|p| mm_text(*p)).collect();
                    vec![
                        number((n + 1).to_string()),
                        number(mm_text(line.bar_nm)),
                        text(pieces.join(" + ")),
                        number(mm_text(bar.offcut_nm)),
                    ]
                })
                .collect();
            table(
                &mut body,
                &[
                    tr(Msg::OrdColBar),
                    tr(Msg::OrdColBarLen),
                    tr(Msg::OrdColPieces),
                    tr(Msg::OrdColOffcut),
                ],
                &rows,
            );
        }
        let _ = write!(
            body,
            "<p class=\"total\">{}</p>",
            escape(&fill(
                tr(Msg::OrderBarsTotal),
                &[&sheet.plan.bars().to_string()]
            ))
        );
    }

    if !sheet.holes.is_empty() {
        if sheet.holes.len() > sheet.max_rows {
            return Err(too_many(sheet.holes.len(), sheet.max_rows));
        }
        let _ = write!(body, "<h2>{}</h2>", escape(tr(Msg::OrderHoles)));
        let rows: Vec<Vec<(String, bool)>> = sheet
            .holes
            .iter()
            .map(|h| {
                let face =
                    face_msg(h.face).map_or_else(|| h.face.to_string(), |m| tr(m).to_string());
                vec![
                    text(h.tag.clone()),
                    text(h.name.clone()),
                    text(face),
                    number(mm_text(h.u)),
                    number(mm_text(h.v)),
                    number(mm_text(h.diameter)),
                    text(style_text(&h.shape, lang)),
                ]
            })
            .collect();
        table(
            &mut body,
            &[
                tr(Msg::OrdColTag),
                tr(Msg::OrdColName),
                tr(Msg::OrdColFace),
                "u (mm)",
                "v (mm)",
                tr(Msg::OrdColDiam),
                tr(Msg::OrdColShape),
            ],
            &rows,
        );
        let _ = write!(
            body,
            "<p class=\"total\">{}</p>",
            escape(&fill(
                tr(Msg::OrderHoleCount),
                &[&sheet.holes.len().to_string()]
            ))
        );
    }

    if !sheet.bom.lines.is_empty() {
        if sheet.bom.lines.len() > sheet.max_rows {
            return Err(too_many(sheet.bom.lines.len(), sheet.max_rows));
        }
        let _ = write!(body, "<h2>{}</h2>", escape(tr(Msg::OrderParts)));
        let rows: Vec<Vec<(String, bool)>> = sheet
            .bom
            .lines
            .iter()
            .map(|l| {
                let kind = if l.turned {
                    Msg::KindTurned
                } else if l.swept {
                    Msg::KindSwept
                } else {
                    kind_msg(l.kind)
                };
                let size = format!(
                    "{} x {} x {}",
                    mm_text(l.size[0]),
                    mm_text(l.size[1]),
                    mm_text(l.size[2])
                );
                vec![
                    number(l.item.to_string()),
                    text(l.name.clone()),
                    text(tr(kind)),
                    text(l.material.clone()),
                    text(size),
                    number(l.quantity.to_string()),
                    number(mass_g_text(l.total_mass_ug())),
                    number(money_text(l.total_cost_milli())),
                ]
            })
            .collect();
        let cost_head = format!("{} ({})", tr(Msg::OrdColCost), sheet.bom.currency);
        table(
            &mut body,
            &[
                tr(Msg::OrdColItem),
                tr(Msg::OrdColName),
                tr(Msg::OrdColKind),
                tr(Msg::OrdColMaterial),
                "mm",
                tr(Msg::OrdColQty),
                tr(Msg::OrdColMass),
                &cost_head,
            ],
            &rows,
        );
        let bom = sheet.bom;
        let _ = write!(body, "<h2>{}</h2>", escape(tr(Msg::OrderTotals)));
        let _ = write!(
            body,
            "<p>{}</p><p class=\"total\">{}</p>",
            escape(&fill(
                tr(Msg::BomTotals),
                &[
                    &bom.pieces.to_string(),
                    &volume_mm3_text(bom.volume_nm3),
                    &mass_g_text(bom.mass_ug)
                ]
            )),
            escape(&fill(
                tr(Msg::BomCost),
                &[
                    &money_text(bom.subtotal_milli),
                    &money_text(bom.markup_milli),
                    &money_text(bom.total_milli),
                    &bom.currency
                ]
            ))
        );
        if bom.unpriced > 0 {
            let _ = write!(
                body,
                "<p>{}</p>",
                escape(&fill(tr(Msg::BomUnpriced), &[&bom.unpriced.to_string()]))
            );
        }
    }

    Ok(format!(
        "<!DOCTYPE html>\n<html lang=\"{}\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>{}</title><style>{STYLE}</style></head><body><h1>{}</h1>{body}</body></html>\n",
        lang.code(),
        escape(&fill(tr(Msg::OrderTitle), &[sheet.title])),
        escape(&fill(tr(Msg::OrderTitle), &[sheet.title])),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nesting::{Bar, StockLine};

    fn mm(v: i64) -> i64 {
        v * 1_000_000
    }

    fn plan() -> Plan {
        Plan {
            lines: vec![StockLine {
                part: "tube<20>".into(),
                material: "alu & co".into(),
                bar_nm: mm(2000),
                bars: vec![
                    Bar {
                        pieces: vec![mm(900), mm(800)],
                        offcut_nm: mm(298),
                    },
                    Bar {
                        pieces: vec![mm(500)],
                        offcut_nm: mm(1498),
                    },
                ],
            }],
        }
    }

    fn bom() -> Bom {
        Bom {
            lines: Vec::new(),
            currency: "EUR".into(),
            markup_percent: 0,
            pieces: 0,
            volume_nm3: 0,
            mass_ug: 0,
            subtotal_milli: 0,
            markup_milli: 0,
            total_milli: 0,
            unpriced: 0,
        }
    }

    fn sheet<'a>(plan: &'a Plan, holes: &'a [HoleRow], bom: &'a Bom) -> Sheet<'a> {
        Sheet {
            title: "my <box>",
            lang: Lang::En,
            plan,
            holes,
            bom,
            max_rows: 100,
        }
    }

    #[test]
    fn the_text_that_comes_from_the_project_is_escaped() {
        assert_eq!(escape("a<b>&\"'"), "a&lt;b&gt;&amp;&quot;&#39;");
        let (p, b) = (plan(), bom());
        let page = html(&sheet(&p, &[], &b)).unwrap();
        assert!(
            page.contains("<title>Order sheet: my &lt;box&gt;</title>"),
            "{page}"
        );
        assert!(
            page.contains("<h3>tube&lt;20&gt; (alu &amp; co)</h3>"),
            "{page}"
        );
        assert!(!page.contains("tube<20>") && !page.contains("my <box>"));
    }

    #[test]
    fn the_page_is_self_contained_with_no_script_picture_or_link() {
        let (p, b) = (plan(), bom());
        let page = html(&sheet(&p, &[], &b)).unwrap();
        assert!(page.starts_with("<!DOCTYPE html>\n<html lang=\"en\">"));
        assert!(page.contains("<meta charset=\"utf-8\">"));
        for banned in [
            "<script", "<img", "<a ", "href=", "src=", "http", "<link", "@import", "url(",
        ] {
            assert!(!page.contains(banned), "{banned}");
        }
        assert!(page.ends_with("</body></html>\n"));
    }

    #[test]
    fn the_cuts_list_the_bars_with_their_pieces_and_offcuts() {
        let (p, b) = (plan(), bom());
        let page = html(&sheet(&p, &[], &b)).unwrap();
        assert!(page.contains("<h2>Profile cuts</h2>"), "{page}");
        assert!(page.contains("2 bars of 2000 mm, 3 pieces"), "{page}");
        assert!(
            page.contains(
                "<tr><td class=\"n\">1</td><td class=\"n\">2000</td><td>900 + 800</td><td class=\"n\">298</td></tr>"
            ),
            "{page}"
        );
        assert!(page.contains("Bars to buy: 2"), "{page}");
        // no holes and no parts: no such section
        assert!(!page.contains("Holes to drill") && !page.contains("Parts of the catalog"));
    }

    #[test]
    fn an_empty_model_says_there_is_nothing_to_order_in_every_language() {
        let (p, b) = (Plan::default(), bom());
        for lang in Lang::ALL {
            let mut s = sheet(&p, &[], &b);
            s.lang = lang;
            let page = html(&s).unwrap();
            assert!(page.contains(&format!("<html lang=\"{}\">", lang.code())));
            assert!(page.contains(&escape(Msg::OrderNone.tr(lang))), "{lang:?}");
        }
    }

    #[test]
    fn a_table_longer_than_the_limit_is_refused() {
        let (p, b) = (plan(), bom());
        let mut s = sheet(&p, &[], &b);
        s.max_rows = 1;
        let e = html(&s).unwrap_err();
        assert_eq!(e.code(), "PHOENIX-CAD-050");
        assert!(e.to_string().contains("2 rows, more than the 1"), "{e}");
        s.max_rows = 2;
        assert!(html(&s).is_ok());
    }
}
