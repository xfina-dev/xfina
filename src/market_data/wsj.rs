//! The Wall Street Journal's historical prices CSV.
//!
//! Price only, newest first, two-digit years, and a space after every comma --
//! which is what tells it apart from the many other `Date,Open,High,Low,Close`
//! files about. WSJ names every download `HistoricalPrices.csv`, so the
//! ticker comes from a filename someone else chose, if anyone did.

use super::common::{self, csv_rows, number, price, Meta, SeriesBuilder};
use crate::decode::Decoded;
use crate::detect::{Claim, Format};
use crate::error::XfinaError;
use crate::models::request::ParseRequest;
use crate::models::series::{Field, PricePoint, PriceSeries};
use crate::models::validation::ParseResult;

/// WSJ's heading row, spaces and all.
const HEADER: &str = "Date, Open, High, Low, Close, Volume";

/// Reads a WSJ historical prices CSV.
pub fn parse_wsj(input: ParseRequest<'_>) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

pub(crate) fn probe(decoded: &Decoded<'_>) -> Claim {
    let first = decoded
        .probe_text()
        .trim_start_matches('\u{FEFF}')
        .lines()
        .next()
        .unwrap_or("")
        .trim_end();
    if first == HEADER {
        Claim::strong("wsj-header")
    } else {
        Claim::NO
    }
}

pub(crate) fn parse_decoded(
    decoded: &Decoded<'_>,
    input: &ParseRequest<'_>,
) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let rows = csv_rows(decoded.text()?)?;
    let Some((head, body)) = rows.split_first() else {
        return Err(XfinaError::InvalidFormat("Empty file".to_string()));
    };
    if head.join(", ") != HEADER {
        return Err(XfinaError::InvalidFormat(
            "Not a WSJ historical prices CSV: heading row differs".to_string(),
        ));
    }

    let mut builder = SeriesBuilder::new();
    for row in body {
        let cell = |i: usize| row.get(i).map(String::as_str).unwrap_or("");
        let Some(date) = common::date(cell(0), &["%m/%d/%y", "%m/%d/%Y"]) else {
            continue;
        };
        let mut p = PricePoint::new(date);
        p.open = price(cell(1));
        p.high = price(cell(2));
        p.low = price(cell(3));
        p.close = price(cell(4));
        p.volume = number(cell(5));
        builder.push(p);
    }

    let mut meta = Meta::new(Format::MarketWsj, Field::Close);
    meta.code = common::ticker(input.filename);
    meta.declared_range = common::range_around_to(input.filename, &["%Y-%m-%d"]);
    Ok(builder.finish(meta, Vec::new()))
}
