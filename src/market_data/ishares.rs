//! BlackRock iShares fund downloads.
//!
//! Each fund page's "Data Download" is an Excel XML Spreadsheet 2003 file
//! named `<Fund-Name>_fund.xls`, with one sheet for the daily history. The
//! sheet differs by where the fund is domiciled:
//!
//! - **US funds**: `Historical` -- As Of, NAV per Share, Ex-Dividends, Shares
//!   Outstanding -- plus a `Distributions` sheet listing every distribution.
//! - **UCITS funds**: `Historical NAVs` -- a date and the NAV, nothing else.
//! - **Swiss funds**: `Historical` -- As Of, Currency, NAV per Share, Shares
//!   Outstanding, Total Net Assets and the fund's and benchmark's return
//!   series.
//!
//! The dividend on each ex-date is read from the history sheet, the way
//! Tiingo carries one; the Distributions sheet is used to check it.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use rust_decimal::Decimal;

use super::common::{self, column_index, number, price, Meta, SeriesBuilder};
use crate::decode::xmlss::{XmlSheet, XmlSheets, NAMESPACE};
use crate::decode::Decoded;
use crate::detect::{Claim, Format};
use crate::error::XfinaError;
use crate::models::request::ParseRequest;
use crate::models::series::{Field, InstrumentKind, PricePoint, PriceSeries};
use crate::models::validation::{ParseResult, SummaryCheck};

/// "Oct 01, 2026" on US sheets, "28/Sept/2026" on European ones.
const DATE_FORMATS: &[&str] = &["%b %d, %Y", "%d/%b/%Y", "%d-%b-%Y", "%Y-%m-%d"];

/// The history sheet's name in each layout, most specific first.
const HISTORY_SHEETS: &[&str] = &["Historical NAVs", "Historical"];

/// Reads an iShares fund download.
pub fn parse_ishares(input: ParseRequest<'_>) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

pub(crate) fn probe(decoded: &Decoded<'_>) -> Claim {
    // Cheap check first: only an XML Spreadsheet is worth decoding.
    if !decoded.probe_text().contains(NAMESPACE) {
        return Claim::NO;
    }
    let Ok(book) = decoded.xml_sheets() else {
        return Claim::NO;
    };
    let has_history = HISTORY_SHEETS.iter().any(|n| book.sheet(n).is_some());
    if has_history && fund_name(book).is_some() {
        Claim::strong("ishares-fund-download")
    } else {
        Claim::NO
    }
}

pub(crate) fn parse_decoded(
    decoded: &Decoded<'_>,
    _input: &ParseRequest<'_>,
) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let book = decoded.xml_sheets()?;
    let sheet = HISTORY_SHEETS
        .iter()
        .find_map(|n| book.sheet(n))
        .ok_or_else(|| {
            XfinaError::InvalidFormat("Not an iShares fund download: no history sheet".to_string())
        })?;

    let mut builder = SeriesBuilder::new();
    let mut currencies: BTreeMap<String, usize> = BTreeMap::new();
    let mut checks = Vec::new();

    if sheet.name == "Historical NAVs" {
        read_nav_only(sheet, &mut builder);
    } else {
        let ex_dividends = read_history(sheet, &mut builder, &mut currencies)?;
        if let Some(distributions) = book.sheet("Distributions") {
            checks.push(check_distributions(distributions, &ex_dividends));
        }
    }

    let mut meta = Meta::new(Format::MarketIshares, Field::Nav);
    meta.name = fund_name(book);
    meta.code =
        labelled(book, &["ISIN"]).or_else(|| labelled(book, &["Ticker", "Bloomberg Ticker"]));
    meta.kind = Some(InstrumentKind::Etf);
    meta.currency = currencies
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(c, _)| c)
        .or_else(|| labelled(book, &["Share Class Currency", "Fund Base Currency"]))
        // A US fund's sheets never name the currency: they are all dollars.
        .or_else(|| book.sheet("Distributions").map(|_| "USD".to_string()));
    Ok(builder.finish(meta, checks))
}

/// The UCITS layout: a date and a NAV per row, under a single heading.
fn read_nav_only(sheet: &XmlSheet, builder: &mut SeriesBuilder) {
    for row in &sheet.rows {
        let cell = |i: usize| row.get(i).map(String::as_str).unwrap_or("");
        let Some(date) = common::date(cell(0), DATE_FORMATS) else {
            continue;
        };
        let mut p = PricePoint::new(date);
        p.nav = price(cell(1));
        builder.push(p);
    }
}

/// The US and Swiss layouts: a heading row naming every column. Returns the
/// dividend read on each ex-date, for the Distributions check.
fn read_history(
    sheet: &XmlSheet,
    builder: &mut SeriesBuilder,
    currencies: &mut BTreeMap<String, usize>,
) -> Result<BTreeMap<NaiveDate, Decimal>, XfinaError> {
    let header_at = sheet
        .rows
        .iter()
        .position(|r| column_index(r, "As Of").is_some())
        .ok_or_else(|| {
            XfinaError::ParseError("iShares history sheet has no As Of column".to_string())
        })?;
    let head = &sheet.rows[header_at];
    let date_col = column_index(head, "As Of");
    let nav_col = column_index(head, "NAV per Share");
    let currency_col = column_index(head, "Currency");
    let dividend_col = column_index(head, "Ex-Dividends");
    let known = [date_col, nav_col, currency_col, dividend_col];
    let extras: Vec<(usize, String)> = head
        .iter()
        .enumerate()
        .filter(|(i, h)| !known.contains(&Some(*i)) && !h.is_empty())
        .map(|(i, h)| (i, common::heading(h)))
        .collect();
    builder.extras(&extras.iter().map(|(_, h)| h.as_str()).collect::<Vec<_>>());

    let mut dividends = BTreeMap::new();
    for row in &sheet.rows[header_at + 1..] {
        let cell = |i: Option<usize>| i.and_then(|i| row.get(i)).map(String::as_str).unwrap_or("");
        let Some(date) = common::date(cell(date_col), DATE_FORMATS) else {
            continue;
        };
        let currency = cell(currency_col);
        if !currency.is_empty() {
            *currencies.entry(currency.to_string()).or_default() += 1;
        }
        let mut p = PricePoint::new(date);
        p.nav = price(cell(nav_col));
        p.dividend = price(cell(dividend_col));
        if let Some(d) = p.dividend {
            dividends.insert(date, d);
        }
        for (i, name) in &extras {
            if let Some(v) = number(cell(Some(*i))) {
                p.extra.insert(name.clone(), v);
            }
        }
        builder.push(p);
    }
    Ok(dividends)
}

/// Every distribution the Distributions sheet lists should be on the history
/// sheet, on its ex-date, for the same total.
fn check_distributions(
    sheet: &XmlSheet,
    ex_dividends: &BTreeMap<NaiveDate, Decimal>,
) -> SummaryCheck {
    let head_at = sheet
        .rows
        .iter()
        .position(|r| column_index(r, "Ex-Date").is_some());
    let (listed, matched) = match head_at {
        Some(at) => {
            let head = &sheet.rows[at];
            let ex = column_index(head, "Ex-Date");
            let total = column_index(head, "Total Distribution");
            let mut listed = 0usize;
            let mut matched = 0usize;
            for row in &sheet.rows[at + 1..] {
                let cell =
                    |i: Option<usize>| i.and_then(|i| row.get(i)).map(String::as_str).unwrap_or("");
                let (Some(date), Some(amount)) =
                    (common::date(cell(ex), DATE_FORMATS), number(cell(total)))
                else {
                    continue;
                };
                listed += 1;
                if ex_dividends.get(&date) == Some(&amount) {
                    matched += 1;
                }
            }
            (listed, matched)
        }
        None => (0, 0),
    };
    SummaryCheck::declared(
        "distributions_match_ex_dividends",
        Decimal::from(listed as u64),
        Decimal::from(matched as u64),
        Some("distributions on the Distributions sheet found on their ex-date".to_string()),
    )
}

/// The fund's name, from the first rows of whichever sheet prints it.
fn fund_name(book: &XmlSheets) -> Option<String> {
    book.sheets.iter().find_map(|sheet| {
        sheet.rows.iter().take(3).flatten().find_map(|cell| {
            let cell = cell.trim();
            (cell.starts_with("iShares ") && cell.len() < 120).then(|| cell.to_string())
        })
    })
}

/// The value beside a label in a key-facts style sheet ("ISIN", "IE00…").
///
/// Only short rows count: a holdings table's heading row also starts with
/// "Ticker", and the cell beside it is the next heading, not a ticker.
fn labelled(book: &XmlSheets, labels: &[&str]) -> Option<String> {
    labels.iter().find_map(|label| {
        book.sheets.iter().find_map(|sheet| {
            sheet
                .rows
                .iter()
                .filter(|row| row.len() <= 3)
                .find_map(|row| {
                    (row.first().map(|c| c.trim()) == Some(*label))
                        .then(|| row.get(1).map(|v| v.trim().to_string()))
                        .flatten()
                        .filter(|v| !v.is_empty() && v != "-")
                })
        })
    })
}
