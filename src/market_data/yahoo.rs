//! Yahoo Finance price history, in either of the shapes it reaches people.
//!
//! Yahoo has no download button any more, so a file arrives one of two ways:
//!
//! - **yfinance**, the Python library: ISO dates, then `Open, High, Low,
//!   Close, Adj Close, Volume, Dividends, Stock Splits` (Adj Close is left out
//!   when prices were auto-adjusted).
//! - **classic**, the historical-data table as Yahoo shows it: quoted
//!   "Mon D, YYYY" dates, grouped numbers, and each dividend or split as a
//!   line of its own between the price rows.
//!
//! In both, closes are already adjusted for later splits -- unlike Tiingo's --
//! so the series says `splitAdjusted`, and a split row is a record rather
//! than an adjustment still to make.
//!
//! Neither names the ticker. For the two S&P 500 index series the Portfolio
//! Engine asks for, the filename's ticker says which: `^GSPC` is the price
//! index and `^SP500TR` the total return index, whose Close is a total return
//! level rather than a price.

use rust_decimal::Decimal;

use super::common::{self, csv_rows, header_line, number, price, Meta, SeriesBuilder};
use crate::decode::Decoded;
use crate::detect::{Claim, Format};
use crate::error::XfinaError;
use crate::models::request::ParseRequest;
use crate::models::series::{Field, InstrumentKind, PricePoint, PriceSeries};
use crate::models::validation::ParseResult;

/// Yahoo's own historical-data heading row.
const CLASSIC: &str = "Date,Open,High,Low,Close,Adj Close,Volume";

/// The tickers that are total return indices: their Close is a total return
/// level, not a price.
const TOTAL_RETURN_INDICES: &[&str] = &["SP500TR"];
/// Price indices, so the series can say it is one.
const PRICE_INDICES: &[&str] = &["GSPC"];

/// Reads a Yahoo Finance price history CSV.
pub fn parse_yahoo(input: ParseRequest<'_>) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

fn is_yfinance(header: &str) -> bool {
    header.starts_with("Date,Open,High,Low,Close,") && header.ends_with(",Dividends,Stock Splits")
}

pub(crate) fn probe(decoded: &Decoded<'_>) -> Claim {
    let header = header_line(decoded.probe_text());
    if is_yfinance(&header) {
        Claim::strong("yfinance-header")
    } else if header == CLASSIC {
        Claim::strong("yahoo-classic-header")
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
    let header = head.join(",");
    let yfinance = is_yfinance(&header);
    if !yfinance && header != CLASSIC {
        return Err(XfinaError::InvalidFormat(
            "Not a Yahoo Finance price CSV: heading row differs".to_string(),
        ));
    }

    let code = common::ticker(input.filename);
    let total_return = code
        .as_deref()
        .is_some_and(|c| TOTAL_RETURN_INDICES.contains(&c));
    let index = total_return || code.as_deref().is_some_and(|c| PRICE_INDICES.contains(&c));

    let col = |name: &str| head.iter().position(|h| h == name);
    let (open, high, low, close, adj, volume) = (
        col("Open"),
        col("High"),
        col("Low"),
        col("Close"),
        col("Adj Close"),
        col("Volume"),
    );
    let (dividends, splits) = (col("Dividends"), col("Stock Splits"));

    let mut builder = SeriesBuilder::new();
    if total_return {
        builder.extras(&["Open", "High", "Low", "Adj Close", "Volume"]);
    }
    for row in body {
        let cell = |i: Option<usize>| i.and_then(|i| row.get(i)).map(String::as_str).unwrap_or("");
        // yfinance may carry a time and offset after the date.
        let raw_date = row.first().map(String::as_str).unwrap_or("");
        let Some(date) = common::date(
            raw_date.get(..10).filter(|_| yfinance).unwrap_or(raw_date),
            &["%Y-%m-%d", "%b %d, %Y"],
        ) else {
            continue;
        };

        // A classic event line: the date, then "0.25 Dividend ..." or
        // "2:1 Stock Splits" in the second column.
        if !yfinance && row.len() == 2 {
            apply_event(builder.row(date, None), &row[1]);
            continue;
        }

        let mut p = PricePoint::new(date);
        if total_return {
            p.total_return = price(cell(close));
            for (name, i) in [
                ("Open", open),
                ("High", high),
                ("Low", low),
                ("Adj Close", adj),
            ] {
                if let Some(v) = price(cell(i)) {
                    p.extra.insert(name.to_string(), v);
                }
            }
            if let Some(v) = number(cell(volume)) {
                p.extra.insert("Volume".to_string(), v);
            }
        } else {
            p.open = price(cell(open));
            p.high = price(cell(high));
            p.low = price(cell(low));
            p.close = price(cell(close));
            p.adj_close = price(cell(adj));
            p.volume = number(cell(volume));
        }
        // yfinance writes 0 on days without a dividend or split.
        p.dividend = price(cell(dividends));
        p.split_factor = number(cell(splits)).filter(|f| !f.is_zero() && *f != Decimal::ONE);

        // An event line may have come first for the same day.
        let slot = builder.row(date, None);
        let (dividend, split) = (slot.dividend, slot.split_factor);
        *slot = p;
        slot.dividend = slot.dividend.or(dividend);
        slot.split_factor = slot.split_factor.or(split);
    }

    let mut meta = Meta::new(
        Format::MarketYahoo,
        if total_return {
            Field::TotalReturn
        } else {
            Field::Close
        },
    );
    meta.code = code;
    meta.kind = index.then_some(InstrumentKind::Index);
    // Yahoo divides every close through by the splits after it, in both
    // shapes: a close from before a 2-for-1 split is printed at half what
    // it traded at, and the split line only records the event.
    meta.split_adjusted = Some(true);
    meta.currency = index.then(|| "USD".to_string());
    meta.declared_range = common::range_around_to(input.filename, &["%Y-%m-%d", "%b-%d-%Y"]);
    Ok(builder.finish(meta, Vec::new()))
}

/// Reads a classic event line into the day's row.
fn apply_event(row: &mut PricePoint, text: &str) {
    let lower = text.to_ascii_lowercase();
    let first = text.split_whitespace().next().unwrap_or("");
    if lower.contains("dividend") {
        row.dividend = price(first);
    } else if lower.contains("split") {
        // "2:1" is a 2-for-1 split; "1:10" a 1-for-10 reverse split.
        if let Some((new, old)) = first.split_once(':') {
            if let (Some(new), Some(old)) = (number(new), number(old)) {
                if !old.is_zero() {
                    row.split_factor = Some(new / old);
                }
            }
        }
    }
}
