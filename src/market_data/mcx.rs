//! MCX's spot market price archive.
//!
//! The page's Excel export is an HTML table saved as `.xls`: commodity, unit,
//! location, the spot price in rupees, an up/down mark, and a date and time.
//! MCX polls the spot price several times a day, so a day has several rows,
//! and every one is kept with its time. Choosing one reading per day is a
//! decision about the data, not about reading the file.

use std::collections::BTreeSet;

use chrono::NaiveTime;

use super::common::{self, column_index, price, Meta, SeriesBuilder};
use crate::decode::html_table;
use crate::decode::Decoded;
use crate::detect::{Claim, Format};
use crate::error::XfinaError;
use crate::models::request::ParseRequest;
use crate::models::series::{Field, InstrumentKind, PricePoint, PriceSeries};
use crate::models::validation::ParseResult;

const PRICE_HEADING: &str = "Spot Price(Rs.)";

/// Reads an MCX spot market price export.
pub fn parse_mcx_spot(input: ParseRequest<'_>) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

pub(crate) fn probe(decoded: &Decoded<'_>) -> Claim {
    let head = decoded.probe_text().to_ascii_uppercase();
    if head.contains("<TABLE")
        && head.contains("SPOT PRICE(RS.)")
        && head.contains(">COMMODITY<")
        && head.contains(">LOCATION<")
    {
        Claim::strong("mcx-spot-table")
    } else {
        Claim::NO
    }
}

pub(crate) fn parse_decoded(
    decoded: &Decoded<'_>,
    _input: &ParseRequest<'_>,
) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let rows = html_table::rows(decoded.text()?);
    let header_at = rows
        .iter()
        .position(|r| column_index(r, PRICE_HEADING).is_some())
        .ok_or_else(|| {
            XfinaError::InvalidFormat("Not an MCX spot price table: no price column".to_string())
        })?;
    let head = &rows[header_at];
    let col = |name: &str| column_index(head, name);
    let (commodity, unit, location, spot, date, time) = (
        col("Commodity"),
        col("Unit"),
        col("Location"),
        col(PRICE_HEADING),
        col("Date"),
        col("Time"),
    );
    let date_col = date.ok_or_else(|| {
        XfinaError::ParseError("MCX spot price table has no Date column".to_string())
    })?;

    let mut builder = SeriesBuilder::new();
    let mut instruments: BTreeSet<(String, String)> = BTreeSet::new();
    let mut units: BTreeSet<String> = BTreeSet::new();
    for row in &rows[header_at + 1..] {
        let cell = |i: Option<usize>| i.and_then(|i| row.get(i)).map(String::as_str).unwrap_or("");
        let Some(day) = common::date(cell(Some(date_col)), &["%d %b %Y", "%d-%b-%Y"]) else {
            continue;
        };
        instruments.insert((cell(commodity).to_string(), cell(location).to_string()));
        if !cell(unit).is_empty() {
            units.insert(cell(unit).to_string());
        }
        let mut p = PricePoint::new(day);
        p.time = NaiveTime::parse_from_str(cell(time), "%H:%M")
            .or_else(|_| NaiveTime::parse_from_str(cell(time), "%H:%M:%S"))
            .ok();
        p.close = price(cell(spot));
        builder.push(p);
    }

    // One query is one commodity at one location. A table mixing them is not
    // one series, and reading it as one would interleave two prices.
    if instruments.len() > 1 {
        return Err(XfinaError::ParseError(
            "MCX spot price table holds more than one commodity or location".to_string(),
        ));
    }

    let mut meta = Meta::new(Format::MarketMcxSpot, Field::Close);
    meta.code = instruments
        .into_iter()
        .next()
        .map(|(commodity, location)| format!("{} {}", commodity, location).trim().to_string())
        .filter(|c| !c.is_empty());
    meta.kind = Some(InstrumentKind::CommoditySpot);
    meta.currency = Some("INR".to_string());
    meta.unit = (units.len() == 1)
        .then(|| units.into_iter().next())
        .flatten();
    Ok(builder.finish(meta, Vec::new()))
}
