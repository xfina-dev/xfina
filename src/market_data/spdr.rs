//! SPDR Gold Shares' historical archive, from spdrgoldshares.com.
//!
//! A workbook whose `US GLD Historical Archive` sheet has a row per day since
//! launch: the closing price, the NAV per share, the gold behind each share
//! and in the trust, and the day's volume. US market holidays are rows too,
//! with "US Holiday" in every column, and are not days with a price.

use calamine::Data;

use super::common::{
    cell_date, cell_number, cell_price, cell_text, column_index, Meta, SeriesBuilder,
};
use crate::decode::Decoded;
use crate::detect::{Claim, Format};
use crate::error::XfinaError;
use crate::models::request::ParseRequest;
use crate::models::series::{Field, InstrumentKind, PricePoint, PriceSeries};
use crate::models::validation::ParseResult;

const SHEET: &str = "US GLD Historical Archive";
const CLOSE: &str = "Closing Price";
const NAV: &str = "NAV/Share at 10:30am NYT";
const VOLUME: &str = "Daily Share Volume";

/// Reads the SPDR Gold Shares historical archive.
pub fn parse_spdr_gold(input: ParseRequest<'_>) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

pub(crate) fn probe(decoded: &Decoded<'_>) -> Claim {
    let Ok(sheets) = decoded.sheets() else {
        return Claim::NO;
    };
    match sheets.get(SHEET) {
        Some(range)
            if range.rows().next().is_some_and(|r| {
                let head: Vec<String> = r.iter().map(cell_text).collect();
                column_index(&head, CLOSE).is_some()
            }) =>
        {
            Claim::strong("spdr-gld-archive")
        }
        _ => Claim::NO,
    }
}

pub(crate) fn parse_decoded(
    decoded: &Decoded<'_>,
    _input: &ParseRequest<'_>,
) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let range = decoded.sheets()?.get(SHEET).ok_or_else(|| {
        XfinaError::InvalidFormat("Not the SPDR Gold archive: no archive sheet".to_string())
    })?;
    let head: Vec<String> = range
        .rows()
        .next()
        .map(|r| r.iter().map(cell_text).collect())
        .unwrap_or_default();
    let col = |name: &str| column_index(&head, name);
    let (date, close, nav, volume) = (col("Date"), col(CLOSE), col(NAV), col(VOLUME));
    let date_col = date.ok_or_else(|| {
        XfinaError::ParseError("SPDR Gold archive has no Date column".to_string())
    })?;
    let known = [date, close, nav, volume];
    let extras: Vec<(usize, String)> = head
        .iter()
        .enumerate()
        .filter(|(i, h)| !known.contains(&Some(*i)) && !h.is_empty())
        .map(|(i, h)| (i, h.clone()))
        .collect();

    let mut builder = SeriesBuilder::new();
    builder.extras(&extras.iter().map(|(_, h)| h.as_str()).collect::<Vec<_>>());
    for row in range.rows().skip(1) {
        let cell = |i: Option<usize>| i.and_then(|i| row.get(i)).unwrap_or(&Data::Empty);
        let Some(day) = cell_date(cell(Some(date_col)), &["%d-%b-%Y", "%d-%b-%y"]) else {
            continue;
        };
        // A holiday row has text where every figure would be, so it reads as
        // no values at all and is dropped when the series is built.
        let mut p = PricePoint::new(day);
        p.close = cell_price(cell(close));
        p.nav = cell_price(cell(nav));
        p.volume = cell_number(cell(volume));
        for (i, name) in &extras {
            if let Some(v) = cell_number(cell(Some(*i))) {
                p.extra.insert(name.clone(), v);
            }
        }
        builder.push(p);
    }

    let mut meta = Meta::new(Format::MarketSpdrGold, Field::Close);
    meta.code = Some("GLD".to_string());
    meta.name = Some("SPDR Gold Shares".to_string());
    meta.kind = Some(InstrumentKind::Etf);
    meta.currency = Some("USD".to_string());
    Ok(builder.finish(meta, Vec::new()))
}
