// SPDX-License-Identifier: GPL-3.0-or-later
//! Where the money of a bill of materials goes: the lines of the bill from the dearest to the
//! cheapest, each with its share of the subtotal and the share of everything that costs as much or
//! more, and the total with a tax on it.
//!
//! The figures are the ones of [`crate::bom`] (whole thousandths of the currency, so the sums are
//! exact). A share is a thousandth of the subtotal, rounded half away from zero, written with one
//! decimal as a percentage; the shares of the lines are rounded one by one, so they may not add up
//! to exactly 100, while the cumulative column is the share of the running sum and ends at 100 when
//! every line has a price. The tax is a percentage with up to two decimals (kept in hundredths of a
//! percent), worked out on the total of the bill, markup included, and rounded half away from zero
//! to a thousandth of the currency. Lines whose material has no price cost nothing, as in the bill,
//! and are told.

use std::fmt::Write as _;

use crate::bom::{cell, money_text, Bom};
use crate::lang::{fill, Lang, Msg};
use crate::units::{div_round_half_away, parse_rational};
use crate::CadError;

/// The most that a tax can be, in hundredths of a percent (100 %).
pub const MAX_TAX_BP: u32 = 10_000;

/// One line of the bill with its share of the cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    /// The number of the line in the bill.
    pub item: usize,
    /// The name of the line.
    pub name: String,
    /// The material of the line.
    pub material: String,
    /// How many pieces.
    pub quantity: u32,
    /// What the pieces of the line cost together, in thousandths.
    pub cost_milli: i128,
    /// The share of the subtotal in thousandths of it.
    pub permille: u32,
    /// The share of the lines that cost as much or more, in thousandths of the subtotal.
    pub cumulative_permille: u32,
}

/// The total of a bill with a tax on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taxed {
    /// The tax in hundredths of a percent.
    pub tax_bp: u32,
    /// The total of the bill (subtotal and markup) in thousandths.
    pub net_milli: i128,
    /// The tax in thousandths.
    pub tax_milli: i128,
    /// The total and the tax in thousandths.
    pub gross_milli: i128,
}

fn permille_of(part: i128, whole: i128) -> u32 {
    if whole <= 0 || part <= 0 {
        return 0;
    }
    let value = div_round_half_away(part * 1000, whole).unwrap_or(0);
    u32::try_from(value.clamp(0, 1000)).unwrap_or(1000)
}

/// The lines of the bill from the dearest to the cheapest (the one that comes first in the bill
/// wins a tie), with their shares.
pub fn shares(bom: &Bom) -> Vec<Share> {
    let mut lines: Vec<(usize, &crate::bom::Line, i128)> = bom
        .lines
        .iter()
        .map(|l| (l.item, l, l.total_cost_milli()))
        .collect();
    lines.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    let mut running = 0i128;
    lines
        .into_iter()
        .map(|(item, line, cost)| {
            running += cost;
            Share {
                item,
                name: line.name.clone(),
                material: line.material.clone(),
                quantity: line.quantity,
                cost_milli: cost,
                permille: permille_of(cost, bom.subtotal_milli),
                cumulative_permille: permille_of(running, bom.subtotal_milli),
            }
        })
        .collect()
}

/// The first `count` shares (all of them when there are fewer).
pub fn top(shares: Vec<Share>, count: usize) -> Vec<Share> {
    shares.into_iter().take(count).collect()
}

/// A tax written as a percentage (`21`, `7.5`, `4,25`, `10 %`) in hundredths of a percent. Refused
/// when it is not a number, is negative or is more than 100 %.
///
/// ```
/// use eh3d_phoenix_cad::bomcosts::parse_tax;
/// assert_eq!(parse_tax("21").unwrap(), 2100);
/// assert_eq!(parse_tax("7,25 %").unwrap(), 725);
/// ```
pub fn parse_tax(text: &str) -> Result<u32, CadError> {
    let bad = || CadError::ValueOutOfRange("tax");
    let t = text.trim();
    let number = t.strip_suffix('%').unwrap_or(t).trim();
    if number.starts_with(['-', '+']) {
        return Err(bad());
    }
    let (num, den) = parse_rational(number).ok_or_else(|| CadError::UnitText(text.to_string()))?;
    let bp = div_round_half_away(num.checked_mul(100).ok_or_else(bad)?, den)?;
    u32::try_from(bp)
        .ok()
        .filter(|v| *v <= MAX_TAX_BP)
        .ok_or_else(bad)
}

/// The total of the bill with a tax of `tax_bp` hundredths of a percent.
pub fn with_tax(bom: &Bom, tax_bp: u32) -> Result<Taxed, CadError> {
    if tax_bp > MAX_TAX_BP {
        return Err(CadError::ValueOutOfRange("tax"));
    }
    let tax = div_round_half_away(
        bom.total_milli
            .checked_mul(i128::from(tax_bp))
            .ok_or(CadError::Overflow("tax"))?,
        10_000,
    )?;
    Ok(Taxed {
        tax_bp,
        net_milli: bom.total_milli,
        tax_milli: tax,
        gross_milli: bom.total_milli + tax,
    })
}

/// A share in thousandths as a percentage with one decimal (`12.5`).
pub fn percent_text(permille: u32) -> String {
    format!("{}.{}", permille / 10, permille % 10)
}

/// A tax in hundredths of a percent with the decimals it needs (`21`, `7.5`, `4.25`).
pub fn tax_text(tax_bp: u32) -> String {
    let (whole, hundredths) = (tax_bp / 100, tax_bp % 100);
    match (hundredths % 10, hundredths) {
        (_, 0) => whole.to_string(),
        (0, h) => format!("{whole}.{}", h / 10),
        (_, h) => format!("{whole}.{h:02}"),
    }
}

/// The shares and the totals as comma separated values, the same text in any language: a row for
/// each share, then `subtotal`, `markup` and `total` and, with a tax, `tax` and `total_with_tax`.
pub fn csv(bom: &Bom, shares: &[Share], tax: Option<&Taxed>) -> String {
    let mut out = String::from(
        "item,name,material,quantity,cost,share_percent,cumulative_percent,currency\n",
    );
    for s in shares {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{}",
            s.item,
            cell(&s.name),
            cell(&s.material),
            s.quantity,
            money_text(s.cost_milli),
            percent_text(s.permille),
            percent_text(s.cumulative_permille),
            bom.currency
        );
    }
    let mut row = |name: &str, label: String, value: i128| {
        let _ = writeln!(
            out,
            ",{name},{label},,{},,,{}",
            money_text(value),
            bom.currency
        );
    };
    row("subtotal", String::new(), bom.subtotal_milli);
    row(
        "markup",
        format!("{}%", bom.markup_percent),
        bom.markup_milli,
    );
    row("total", String::new(), bom.total_milli);
    if let Some(t) = tax {
        row("tax", format!("{}%", tax_text(t.tax_bp)), t.tax_milli);
        row("total_with_tax", String::new(), t.gross_milli);
    }
    out
}

/// The same as lines of text in a language.
pub fn text_in(bom: &Bom, shares: &[Share], tax: Option<&Taxed>, lang: Lang) -> String {
    let mut out = String::new();
    out.push_str(&fill(Msg::BomCostsHead.tr(lang), &[&bom.currency]));
    out.push('\n');
    if shares.iter().all(|s| s.cost_milli == 0) {
        out.push_str(Msg::BomCostsNone.tr(lang));
        out.push('\n');
    }
    for s in shares {
        out.push_str(&fill(
            Msg::BomCostsLine.tr(lang),
            &[
                &s.item.to_string(),
                &s.name,
                &s.quantity.to_string(),
                &s.material,
                &money_text(s.cost_milli),
                &bom.currency,
                &percent_text(s.permille),
                &percent_text(s.cumulative_permille),
            ],
        ));
        out.push('\n');
    }
    if bom.unpriced > 0 {
        out.push_str(&fill(
            Msg::BomCostsUnpriced.tr(lang),
            &[&bom.unpriced.to_string()],
        ));
        out.push('\n');
    }
    if let Some(t) = tax {
        out.push_str(&fill(
            Msg::BomCostsTax.tr(lang),
            &[
                &tax_text(t.tax_bp),
                &money_text(t.tax_milli),
                &bom.currency,
                &money_text(t.gross_milli),
            ],
        ));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bom::Line;
    use crate::catalog::Kind;

    fn line(item: usize, name: &str, material: &str, quantity: u32, unit_cost: i128) -> Line {
        Line {
            item,
            name: name.into(),
            kind: Kind::Profile,
            turned: false,
            swept: false,
            material: material.into(),
            density: 2700,
            size: [1, 1, 1],
            quantity,
            unit_volume_nm3: 1,
            unit_mass_ug: 1,
            price_milli_per_kg: Some(1),
            unit_cost_milli: unit_cost,
        }
    }

    fn bill(lines: Vec<Line>, markup_percent: u32) -> Bom {
        let subtotal: i128 = lines.iter().map(Line::total_cost_milli).sum();
        let markup = div_round_half_away(subtotal * i128::from(markup_percent), 100).unwrap();
        Bom {
            pieces: lines.iter().map(|l| l.quantity).sum(),
            lines,
            currency: "EUR".into(),
            markup_percent,
            volume_nm3: 0,
            mass_ug: 0,
            subtotal_milli: subtotal,
            markup_milli: markup,
            total_milli: subtotal + markup,
            unpriced: 0,
        }
    }

    fn sample() -> Bom {
        bill(
            vec![
                line(1, "rail", "aluminium", 4, 2_500), // 10.000
                line(2, "screw", "steel", 20, 100),     // 2.000
                line(3, "bracket", "steel", 2, 5_000),  // 10.000
                line(4, "cap", "plastic", 1, 0),        // 0
            ],
            10,
        )
    }

    #[test]
    fn the_lines_are_ranked_by_cost_and_a_tie_keeps_the_order_of_the_bill() {
        let s = shares(&sample());
        let order: Vec<usize> = s.iter().map(|x| x.item).collect();
        assert_eq!(order, vec![1, 3, 2, 4]);
        assert_eq!(s[0].cost_milli, 10_000);
        // 10 of 22: 454.5 thousandths, rounded away from zero
        assert_eq!(s[0].permille, 455);
        assert_eq!(s[1].permille, 455);
        assert_eq!(s[2].permille, 91);
        assert_eq!(s[3].permille, 0);
        assert_eq!(
            s.iter().map(|x| x.cumulative_permille).collect::<Vec<_>>(),
            vec![455, 909, 1000, 1000]
        );
    }

    #[test]
    fn a_bill_with_no_price_has_no_shares_and_says_so() {
        let bom = bill(vec![line(1, "rail", "unpriced", 3, 0)], 0);
        let s = shares(&bom);
        assert_eq!((s[0].permille, s[0].cumulative_permille), (0, 0));
        let text = text_in(&bom, &s, None, Lang::En);
        assert!(text.contains("The bill has no line with a price"));
        assert!(shares(&bill(Vec::new(), 0)).is_empty());
    }

    #[test]
    fn the_first_lines_are_taken_and_more_than_there_are_gives_them_all() {
        assert_eq!(top(shares(&sample()), 2).len(), 2);
        assert_eq!(top(shares(&sample()), 2)[1].item, 3);
        assert_eq!(top(shares(&sample()), 99).len(), 4);
        assert!(top(shares(&sample()), 0).is_empty());
    }

    #[test]
    fn a_tax_is_read_as_a_percentage_in_hundredths() {
        for (text, bp) in [
            ("21", 2100),
            ("21 %", 2100),
            ("7.5", 750),
            ("7,25%", 725),
            ("0", 0),
            ("100", 10_000),
            ("4.255", 426),
            ("1/4", 25),
        ] {
            assert_eq!(parse_tax(text).unwrap(), bp, "{text}");
        }
        for bad in ["", "-5", "+5", "100.01", "abc", "5 pct", "1e2", "%"] {
            assert!(parse_tax(bad).is_err(), "{bad:?}");
        }
        assert_eq!(parse_tax("101").unwrap_err().code(), "PHOENIX-CAD-007");
    }

    #[test]
    fn the_tax_is_on_the_total_with_the_markup_and_is_rounded_half_away_from_zero() {
        // subtotal 22.000, markup 2.200, total 24.200
        let bom = sample();
        assert_eq!(bom.total_milli, 24_200);
        let t = with_tax(&bom, 2100).unwrap();
        assert_eq!(
            (t.net_milli, t.tax_milli, t.gross_milli),
            (24_200, 5_082, 29_282)
        );
        // 0.05 % of 24.200 is 12.1 thousandths
        assert_eq!(with_tax(&bom, 5).unwrap().tax_milli, 12);
        // 4.25 % of 24.200 is 1028.5 thousandths: up
        assert_eq!(with_tax(&bom, 425).unwrap().tax_milli, 1_029);
        assert_eq!(with_tax(&bom, 0).unwrap().gross_milli, 24_200);
        assert_eq!(
            with_tax(&bom, 10_001).unwrap_err().code(),
            "PHOENIX-CAD-007"
        );
    }

    #[test]
    fn the_percentages_are_written_with_the_decimals_they_need() {
        assert_eq!(percent_text(455), "45.5");
        assert_eq!(percent_text(1000), "100.0");
        assert_eq!(percent_text(0), "0.0");
        assert_eq!(tax_text(2100), "21");
        assert_eq!(tax_text(750), "7.5");
        assert_eq!(tax_text(425), "4.25");
        assert_eq!(tax_text(5), "0.05");
        assert_eq!(tax_text(0), "0");
    }

    #[test]
    fn the_csv_lists_the_shares_and_the_totals_and_guards_the_names() {
        let mut bom = sample();
        bom.lines[1].name = "=cmd,\"x\"".into();
        let s = shares(&bom);
        let t = with_tax(&bom, 2100).unwrap();
        let out = csv(&bom, &s, Some(&t));
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(
            rows[0],
            "item,name,material,quantity,cost,share_percent,cumulative_percent,currency"
        );
        assert_eq!(rows[1], "1,rail,aluminium,4,10.000,45.5,45.5,EUR");
        assert_eq!(rows[2], "3,bracket,steel,2,10.000,45.5,90.9,EUR");
        assert!(rows[3].starts_with("2,\"'=cmd,\"\"x\"\"\",steel,20,2.000,9.1,100.0,"));
        assert_eq!(rows[5], ",subtotal,,,22.000,,,EUR");
        assert_eq!(rows[6], ",markup,10%,,2.200,,,EUR");
        assert_eq!(rows[7], ",total,,,24.200,,,EUR");
        assert_eq!(rows[8], ",tax,21%,,5.082,,,EUR");
        assert_eq!(rows[9], ",total_with_tax,,,29.282,,,EUR");
        assert_eq!(csv(&bom, &s, None).lines().count(), 8);
    }

    #[test]
    fn the_text_is_the_same_figures_in_every_language() {
        let bom = sample();
        let s = shares(&bom);
        let t = with_tax(&bom, 2100).unwrap();
        for lang in Lang::ALL {
            let text = text_in(&bom, &s, Some(&t), lang);
            assert!(!text.contains('{'), "{}", lang.code());
            assert!(text.contains("10.000") && text.contains("45.5") && text.contains("29.282"));
            assert!(text.contains("21"));
        }
        let en = text_in(&bom, &s, Some(&t), Lang::En);
        assert!(en.starts_with("Cost by line, the dearest first (EUR)\n"));
        assert!(en.contains(
            "1. rail x4 (aluminium): 10.000 EUR, 45.5 % of the subtotal, 45.5 % so far\n"
        ));
        assert!(en.ends_with("Tax 21 %: 5.082 EUR; total with tax: 29.282 EUR\n"));
        let mut unpriced = bom.clone();
        unpriced.unpriced = 2;
        assert!(text_in(&unpriced, &s, None, Lang::En)
            .contains("2 line(s) have no price and cost nothing here\n"));
    }

    #[test]
    fn random_bills_have_shares_that_are_ordered_and_a_running_share_that_reaches_the_whole() {
        let mut seed = 0x0b0c_cad3u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            seed >> 33
        };
        for _ in 0..60 {
            let n = 1 + (next() % 12) as usize;
            let lines: Vec<Line> = (0..n)
                .map(|i| {
                    line(
                        i + 1,
                        &format!("part{i}"),
                        "steel",
                        1 + (next() % 20) as u32,
                        i128::from(next() % 9_000) + 1,
                    )
                })
                .collect();
            let bom = bill(lines, (next() % 40) as u32);
            let s = shares(&bom);
            assert_eq!(s.len(), n);
            for pair in s.windows(2) {
                assert!(pair[0].cost_milli >= pair[1].cost_milli);
                assert!(pair[0].cumulative_permille <= pair[1].cumulative_permille);
                if pair[0].cost_milli == pair[1].cost_milli {
                    assert!(pair[0].item < pair[1].item);
                }
            }
            assert_eq!(s.last().unwrap().cumulative_permille, 1000);
            assert_eq!(
                s.iter().map(|x| x.cost_milli).sum::<i128>(),
                bom.subtotal_milli
            );
            // each share is within one thousandth of the exact one
            for x in &s {
                let exact = x.cost_milli * 1000 / bom.subtotal_milli;
                assert!((i128::from(x.permille) - exact).abs() <= 1);
            }
            let tax_bp = (next() % 3_000) as u32;
            let t = with_tax(&bom, tax_bp).unwrap();
            assert_eq!(t.gross_milli, t.net_milli + t.tax_milli);
            assert!(t.tax_milli * 10_000 <= t.net_milli * i128::from(tax_bp) + 5_000);
        }
    }
}
