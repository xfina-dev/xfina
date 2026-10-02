//! NSE Indices' historical data reports, from niftyindices.com.
//!
//! Two reports share the page:
//!
//! - **Total returns index values** (`<INDEX>_Historical_TR_<from>to<to>.csv`):
//!   the total return level, and the net total return level beside it for
//!   the indices NSE computes one for.
//! - **Historical index data** (`<INDEX>_Historical_PR_<from>to<to>.csv`): the
//!   price index's open, high, low and close -- which is the series for the
//!   debt indices, such as the overnight rate index, that print only a close.
//!
//! The page serves at most a year per request, so a long history is many of
//! these, each read on its own.

use super::common::{self, csv_rows, header_line, heading, price};
use super::common::{Meta, SeriesBuilder};
use crate::decode::Decoded;
use crate::detect::{Claim, Format};
use crate::error::XfinaError;
use crate::models::request::ParseRequest;
use crate::models::series::{Field, InstrumentKind, PricePoint, PriceSeries};
use crate::models::validation::ParseResult;

const TOTAL_RETURN_HEADER: &str = "IndexName,Date,Total Returns Index";
const WITH_NET_HEADER: &str = "IndexName,Date,Total Returns Index,Net Total Return Index";
const PRICE_HEADER: &str = "Index Name,Date,Open,High,Low,Close";

const DATE_FORMATS: &[&str] = &["%d %b %Y", "%d-%b-%Y"];

/// Reads an NSE Indices historical data CSV.
pub fn parse_nse_indices(input: ParseRequest<'_>) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

pub(crate) fn probe(decoded: &Decoded<'_>) -> Claim {
    match header_line(decoded.probe_text()).as_str() {
        TOTAL_RETURN_HEADER | WITH_NET_HEADER => Claim::strong("nse-indices-total-return-header"),
        PRICE_HEADER => Claim::strong("nse-indices-price-header"),
        _ => Claim::NO,
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
    // An exact heading match, so every column below is read by position.
    let headings: Vec<String> = head.iter().map(|h| heading(h)).collect();
    let total_return = match headings.join(",").as_str() {
        TOTAL_RETURN_HEADER | WITH_NET_HEADER => true,
        PRICE_HEADER => false,
        _ => {
            return Err(XfinaError::InvalidFormat(
                "Not an NSE Indices historical data CSV: heading row differs".to_string(),
            ))
        }
    };

    let mut builder = SeriesBuilder::new();
    let mut name: Option<String> = None;
    for row in body {
        let cell = |i: usize| row.get(i).map(String::as_str).unwrap_or("");
        let Some(date) = common::date(cell(1), DATE_FORMATS) else {
            continue;
        };
        if name.is_none() && !cell(0).is_empty() {
            name = Some(cell(0).to_string());
        }
        let mut p = PricePoint::new(date);
        if total_return {
            p.total_return = price(cell(2));
            p.net_total_return = price(cell(3));
        } else {
            // Debt indices print "-" for open, high and low.
            p.open = price(cell(2));
            p.high = price(cell(3));
            p.low = price(cell(4));
            p.close = price(cell(5));
        }
        builder.push(p);
    }

    let mut meta = Meta::new(
        Format::MarketNseIndices,
        if total_return {
            Field::TotalReturn
        } else {
            Field::Close
        },
    );
    meta.code = name.or_else(|| index_from_name(input.filename));
    meta.kind = Some(InstrumentKind::Index);
    meta.currency = Some("INR".to_string());
    meta.declared_range = common::compact_range(input.filename, "%d%m%Y");
    Ok(builder.finish(meta, Vec::new()))
}

/// `NIFTY 50_Historical_TR_…` names the index ahead of the report.
fn index_from_name(filename: Option<&str>) -> Option<String> {
    let stem = common::stem(filename?);
    let at = stem.to_ascii_lowercase().find("_historical_")?;
    Some(stem[..at].to_string()).filter(|s| !s.is_empty())
}
