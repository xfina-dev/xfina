//! NSE's security-wise price history, in both shapes NSE hands it out.
//!
//! - The **Security-wise Archives** report (`<from>-TO-<to>-<SYMBOL>-EQ-N.csv`):
//!   symbol, series and date, then prices, volume, turnover, trades and
//!   deliverable position, with headings padded with trailing spaces.
//! - The **quote page** download (`Quote-Equity-<SYMBOL>-EQ-<from>-<to>.csv`):
//!   the same day's prices with VWAP and the 52-week range, but no symbol in
//!   the file at all.
//!
//! Both list newest first and group their numbers the Indian way.

use std::collections::BTreeMap;

use super::common::{self, column_index, csv_rows, header_line, heading, number, price};
use super::common::{Meta, SeriesBuilder};
use crate::decode::Decoded;
use crate::detect::{Claim, Format};
use crate::error::XfinaError;
use crate::models::request::ParseRequest;
use crate::models::series::{
    DateRange, Field, InstrumentKind, PricePoint, PriceSeries, RangeSource,
};
use crate::models::validation::ParseResult;

const ARCHIVE_PREFIX: &str = "Symbol,Series,Date,Prev Close,Open Price,High Price,Low Price";
const QUOTE_PREFIX: &str = "DATE,SERIES,OPEN,HIGH,LOW,PREV. CLOSE,LTP,CLOSE,VWAP";

/// Where each priced column sits in the two layouts: (open, high, low, close,
/// volume) headings, in that order.
const ARCHIVE_PRICES: [&str; 5] = [
    "Open Price",
    "High Price",
    "Low Price",
    "Close Price",
    "Total Traded Quantity",
];
const QUOTE_PRICES: [&str; 5] = ["OPEN", "HIGH", "LOW", "CLOSE", "VOLUME"];

/// Reads an NSE security-wise price history CSV.
pub fn parse_nse_security(input: ParseRequest<'_>) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

pub(crate) fn probe(decoded: &Decoded<'_>) -> Claim {
    let header = header_line(decoded.probe_text());
    if header.starts_with(ARCHIVE_PREFIX) {
        Claim::strong("nse-security-archive-header")
    } else if header.starts_with(QUOTE_PREFIX) {
        Claim::strong("nse-quote-equity-header")
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
    let headings: Vec<String> = head.iter().map(|h| heading(h)).collect();
    let joined = headings.join(",");
    let priced = if joined.starts_with(ARCHIVE_PREFIX) {
        ARCHIVE_PRICES
    } else if joined.starts_with(QUOTE_PREFIX) {
        QUOTE_PRICES
    } else {
        return Err(XfinaError::InvalidFormat(
            "Not an NSE security-wise price CSV: heading row differs".to_string(),
        ));
    };

    let date_col = column_index(&headings, "Date")
        .ok_or_else(|| XfinaError::ParseError("NSE price CSV has no Date column".to_string()))?;
    let symbol_col = column_index(&headings, "Symbol");
    let series_col = column_index(&headings, "Series");
    let [open, high, low, close, volume] = priced.map(|name| column_index(&headings, name));

    // Everything else numeric is kept under NSE's own heading.
    let skip: Vec<Option<usize>> = vec![
        Some(date_col),
        symbol_col,
        series_col,
        open,
        high,
        low,
        close,
        volume,
    ];
    let extras: Vec<(usize, String)> = headings
        .iter()
        .enumerate()
        .filter(|(i, _)| !skip.contains(&Some(*i)))
        .map(|(i, h)| (i, h.clone()))
        .collect();

    let mut builder = SeriesBuilder::new();
    builder.extras(&extras.iter().map(|(_, h)| h.as_str()).collect::<Vec<_>>());
    let mut symbols: BTreeMap<String, usize> = BTreeMap::new();
    for row in body {
        let cell = |i: Option<usize>| i.and_then(|i| row.get(i)).map(String::as_str).unwrap_or("");
        let Some(date) = common::date(cell(Some(date_col)), &["%d-%b-%Y", "%d-%m-%Y"]) else {
            continue;
        };
        if let Some(symbol) = Some(cell(symbol_col)).filter(|s| !s.is_empty()) {
            *symbols.entry(symbol.to_string()).or_default() += 1;
        }
        let mut p = PricePoint::new(date);
        p.open = price(cell(open));
        p.high = price(cell(high));
        p.low = price(cell(low));
        p.close = price(cell(close));
        p.volume = number(cell(volume));
        for (i, name) in &extras {
            if let Some(v) = number(cell(Some(*i))) {
                p.extra.insert(name.clone(), v);
            }
        }
        builder.push(p);
    }

    let mut meta = Meta::new(Format::MarketNseSecurity, Field::Close);
    // The archive prints the symbol on every row; the quote download only in
    // its name.
    meta.code = symbols
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(s, _)| s)
        .or_else(|| symbol_from_name(input.filename));
    meta.kind = Some(InstrumentKind::ListedSecurity);
    meta.currency = Some("INR".to_string());
    meta.declared_range = range_from_name(input.filename);
    Ok(builder.finish(meta, Vec::new()))
}

/// `Quote-Equity-<SYMBOL>-EQ-…` names the symbol; nothing else in the file does.
fn symbol_from_name(filename: Option<&str>) -> Option<String> {
    let stem = common::stem(filename?);
    let rest = stem
        .get(..13)
        .filter(|p| p.eq_ignore_ascii_case("quote-equity-"))
        .map(|_| &stem[13..])?;
    rest.split('-')
        .next()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Both NSE names carry the requested window as two `DD-MM-YYYY` dates; the
/// bookmarklet's fallback name carries it as two ISO dates.
fn range_from_name(filename: Option<&str>) -> Option<DateRange> {
    let stem = common::stem(filename?);
    let parts: Vec<&str> = stem.split('-').collect();
    let dates: Vec<chrono::NaiveDate> = parts
        .windows(3)
        .filter_map(|w| {
            let text = w.join("-");
            let format = match (w[0].len(), w[1].len(), w[2].len()) {
                (2, 2, 4) => "%d-%m-%Y",
                (4, 2, 2) => "%Y-%m-%d",
                _ => return None,
            };
            chrono::NaiveDate::parse_from_str(&text, format).ok()
        })
        .collect();
    match dates.as_slice() {
        [from, to] if from <= to => Some(DateRange {
            from: *from,
            to: *to,
            source: RangeSource::Filename,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_names_carry_the_window() {
        let range = range_from_name(Some("01-09-2026-TO-11-09-2026-ABCDEF-EQ-N.csv")).unwrap();
        assert_eq!(range.from.to_string(), "2026-09-01");
        assert_eq!(range.to.to_string(), "2026-09-11");
        let range =
            range_from_name(Some("Quote-Equity-ABCDEF-EQ-01-01-2026-26-09-2026.csv")).unwrap();
        assert_eq!(range.to.to_string(), "2026-09-26");
        assert_eq!(
            symbol_from_name(Some("Quote-Equity-ABCDEF-EQ-01-01-2026-26-09-2026.csv")).as_deref(),
            Some("ABCDEF")
        );
    }
}
