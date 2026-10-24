//! Nasdaq Indexes' end-of-day history, from indexes.nasdaq.com.
//!
//! A `History` sheet of trade date, index value, net change, high and low,
//! newest first and back to the index's own start, in a workbook named
//! `EODHist_<from>-<to>_<SYMBOL>.xlsx`. The sheet does not say which index it
//! is, and so not whether the value is a price or a total return level; the
//! symbol in the name does.

use calamine::Data;

use super::common::{self, cell_date, cell_number, cell_price, cell_text, Meta, SeriesBuilder};
use crate::decode::Decoded;
use crate::detect::{Claim, Format};
use crate::error::XfinaError;
use crate::models::request::ParseRequest;
use crate::models::series::{Field, InstrumentKind, PricePoint, PriceSeries};
use crate::models::validation::ParseResult;

const SHEET: &str = "History";
const HEADINGS: [&str; 5] = ["Trade Date", "Index Value", "Net Change", "High", "Low"];

/// Total return indices among the symbols this reader has been shown. Any
/// other symbol is read as a price index, which is what Nasdaq's plain
/// symbols are.
const TOTAL_RETURN_SYMBOLS: &[&str] = &["XNDX"];

/// Reads a Nasdaq Indexes end-of-day history workbook.
pub fn parse_nasdaq(input: ParseRequest<'_>) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

fn has_headings(row: Option<&[Data]>) -> bool {
    row.is_some_and(|r| {
        r.len() >= HEADINGS.len()
            && HEADINGS
                .iter()
                .zip(r.iter())
                .all(|(h, c)| cell_text(c).eq_ignore_ascii_case(h))
    })
}

pub(crate) fn probe(decoded: &Decoded<'_>) -> Claim {
    let Ok(sheets) = decoded.sheets() else {
        return Claim::NO;
    };
    match sheets.get(SHEET) {
        Some(range) if has_headings(range.rows().next()) => Claim::strong("nasdaq-eod-history"),
        _ => Claim::NO,
    }
}

pub(crate) fn parse_decoded(
    decoded: &Decoded<'_>,
    input: &ParseRequest<'_>,
) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let range = decoded.sheets()?.get(SHEET).ok_or_else(|| {
        XfinaError::InvalidFormat("Not a Nasdaq index history: no History sheet".to_string())
    })?;
    if !has_headings(range.rows().next()) {
        return Err(XfinaError::InvalidFormat(
            "Not a Nasdaq index history: heading row differs".to_string(),
        ));
    }

    let symbol = symbol_from_name(input.filename);
    let total_return = symbol
        .as_deref()
        .is_some_and(|s| TOTAL_RETURN_SYMBOLS.contains(&s));

    let mut builder = SeriesBuilder::new();
    builder.extras(if total_return {
        &["High", "Low", "Net Change"][..]
    } else {
        &["Net Change"][..]
    });
    for row in range.rows().skip(1) {
        let cell = |i: usize| row.get(i).unwrap_or(&Data::Empty);
        let Some(date) = cell_date(cell(0), &["%Y-%m-%d", "%m/%d/%Y"]) else {
            continue;
        };
        let mut p = PricePoint::new(date);
        // The first row is today with every figure zero, and early history
        // prints 0 for a high and low it did not record: none are prices.
        let value = cell_price(cell(1));
        let (high, low) = (cell_price(cell(3)), cell_price(cell(4)));
        if total_return {
            p.total_return = value;
            for (name, v) in [("High", high), ("Low", low)] {
                if let Some(v) = v {
                    p.extra.insert(name.to_string(), v);
                }
            }
        } else {
            p.close = value;
            p.high = high;
            p.low = low;
        }
        // A day with no value has no change either.
        if value.is_some() {
            if let Some(change) = cell_number(cell(2)) {
                p.extra.insert("Net Change".to_string(), change);
            }
        }
        builder.push(p);
    }

    let mut meta = Meta::new(
        Format::MarketNasdaq,
        if total_return {
            Field::TotalReturn
        } else {
            Field::Close
        },
    );
    meta.code = symbol;
    meta.kind = Some(InstrumentKind::Index);
    meta.currency = Some("USD".to_string());
    meta.declared_range = common::compact_range(input.filename, "%Y%m%d");
    Ok(builder.finish(meta, Vec::new()))
}

/// The symbol after the last underscore: `EODHist_19850131-20260926_NDX`.
fn symbol_from_name(filename: Option<&str>) -> Option<String> {
    let stem = common::stem(filename?);
    let (_, symbol) = stem.rsplit_once('_')?;
    (!symbol.is_empty() && symbol.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| symbol.to_ascii_uppercase())
}
