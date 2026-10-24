//! Tiingo's end-of-day price CSV, as its API returns it.
//!
//! The most complete free layout there is: the price as traded, the same
//! price adjusted for every split and dividend, and each dividend and split
//! on its own day. The file names nothing -- not the ticker, not whether it
//! is a fund -- so the ticker comes from the filename, and a mutual fund is
//! recognised by its rows.

use rust_decimal::Decimal;

use super::common::{self, csv_rows, header_line, number, price, Meta, SeriesBuilder};
use crate::decode::Decoded;
use crate::detect::{Claim, Format};
use crate::error::XfinaError;
use crate::models::request::ParseRequest;
use crate::models::series::{Field, InstrumentKind, PricePoint, PriceSeries};
use crate::models::validation::ParseResult;

/// Tiingo's heading row, exactly. Nothing else prints this one.
const HEADER: &str =
    "date,close,high,low,open,volume,adjClose,adjHigh,adjLow,adjOpen,adjVolume,divCash,splitFactor";

/// Reads a Tiingo price CSV.
pub fn parse_tiingo(input: ParseRequest<'_>) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

pub(crate) fn probe(decoded: &Decoded<'_>) -> Claim {
    if header_line(decoded.probe_text()) == HEADER {
        Claim::strong("tiingo-header")
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
    if head.join(",") != HEADER {
        return Err(XfinaError::InvalidFormat(
            "Not a Tiingo price CSV: heading row differs".to_string(),
        ));
    }

    // Columns are fixed by the heading check above.
    fn cell(row: &[String], i: usize) -> &str {
        row.get(i).map(String::as_str).unwrap_or("")
    }

    // A mutual fund has one price a day, its NAV: Tiingo prints it as open,
    // high, low and close alike, with no volume. No exchange-traded line in
    // the samples looks like that on every day, and every fund does.
    let is_fund = !body.is_empty()
        && body.iter().all(|r| {
            let close = cell(r, 1);
            cell(r, 2) == close
                && cell(r, 3) == close
                && cell(r, 4) == close
                && number(cell(r, 5)).is_some_and(|v| v.is_zero())
        });

    let mut builder = SeriesBuilder::new();
    for row in body {
        let Some(date) = common::date(cell(row, 0).get(..10).unwrap_or(""), &["%Y-%m-%d"]) else {
            continue;
        };
        let mut p = PricePoint::new(date);
        if is_fund {
            p.nav = price(cell(row, 1));
            p.adj_nav = price(cell(row, 6));
        } else {
            p.close = price(cell(row, 1));
            p.high = price(cell(row, 2));
            p.low = price(cell(row, 3));
            p.open = price(cell(row, 4));
            p.volume = number(cell(row, 5));
            p.adj_close = price(cell(row, 6));
            p.adj_high = price(cell(row, 7));
            p.adj_low = price(cell(row, 8));
            p.adj_open = price(cell(row, 9));
            p.adj_volume = number(cell(row, 10));
        }
        // Tiingo writes 0.0 and 1.0 on days with nothing to report.
        p.dividend = price(cell(row, 11));
        p.split_factor = number(cell(row, 12)).filter(|f| *f != Decimal::ONE && !f.is_zero());
        builder.push(p);
    }

    let mut meta = Meta::new(
        Format::MarketTiingo,
        if is_fund { Field::Nav } else { Field::Close },
    );
    meta.code = common::ticker(input.filename);
    meta.kind = is_fund.then_some(InstrumentKind::MutualFund);
    // Tiingo's close is the price as traded; adjClose is the adjusted one.
    meta.split_adjusted = Some(false);
    meta.declared_range = common::range_around_to(input.filename, &["%Y-%m-%d"]);
    Ok(builder.finish(meta, Vec::new()))
}
