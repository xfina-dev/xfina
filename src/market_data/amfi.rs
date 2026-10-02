//! AMFI's NAV history export, from amfiindia.com.
//!
//! One scheme per workbook, at most five years per download. The sheet opens
//! with the period it covers, the fund house, the scheme group and the scheme
//! with its plan and option, then a row per day: net asset value, repurchase
//! and sale price (blank for most schemes now), date, plan and option.
//!
//! AMFI prints no scheme code or ISIN in the file, so the scheme's full name
//! is what names the series.

use calamine::{Data, Range};

use super::common::{self, cell_date, cell_price, cell_text, Meta, SeriesBuilder};
use crate::decode::Decoded;
use crate::detect::{Claim, Format};
use crate::error::XfinaError;
use crate::models::request::ParseRequest;
use crate::models::series::{
    DateRange, Field, InstrumentKind, PricePoint, PriceSeries, RangeSource,
};
use crate::models::validation::ParseResult;

const SHEET: &str = "Historical NAV Data";
const PERIOD_PREFIX: &str = "Historical NAV Data for From";
const DATE_FORMATS: &[&str] = &["%d-%b-%Y", "%d-%m-%Y"];

/// Reads an AMFI NAV history workbook.
pub fn parse_amfi_nav(input: ParseRequest<'_>) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

pub(crate) fn probe(decoded: &Decoded<'_>) -> Claim {
    let Ok(sheets) = decoded.sheets() else {
        return Claim::NO;
    };
    let Some(range) = sheets.get(SHEET) else {
        return Claim::NO;
    };
    let head = head_text(range, 8);
    if head.contains(&PERIOD_PREFIX.to_uppercase()) && head.contains("NET ASSET VALUE") {
        Claim::strong("amfi-nav-history")
    } else {
        Claim::NO
    }
}

fn head_text(range: &Range<Data>, rows: usize) -> String {
    range
        .rows()
        .take(rows)
        .flatten()
        .map(cell_text)
        .collect::<Vec<_>>()
        .join("\n")
        .to_uppercase()
}

pub(crate) fn parse_decoded(
    decoded: &Decoded<'_>,
    _input: &ParseRequest<'_>,
) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let sheets = decoded.sheets()?;
    let range = sheets.get(SHEET).ok_or_else(|| {
        XfinaError::InvalidFormat(
            "Not an AMFI NAV history: no Historical NAV Data sheet".to_string(),
        )
    })?;
    let rows: Vec<Vec<String>> = range
        .rows()
        .map(|r| r.iter().map(cell_text).collect())
        .collect();

    let header_at = rows
        .iter()
        .position(|r| common::column_index(r, "Net Asset Value").is_some())
        .ok_or_else(|| {
            XfinaError::ParseError("AMFI NAV history has no Net Asset Value column".to_string())
        })?;
    let head = &rows[header_at];
    let col = |name: &str| common::column_index(head, name);
    let (nav, repurchase, sale, date) = (
        col("Net Asset Value"),
        col("Repurchase Price"),
        col("Sale Price"),
        col("Date"),
    );
    let date_col = date
        .ok_or_else(|| XfinaError::ParseError("AMFI NAV history has no Date column".to_string()))?;

    // Above the heading: the period line, then fund house, scheme group and
    // the scheme itself, the last non-empty line before the table.
    let preamble: Vec<&str> = rows[..header_at]
        .iter()
        .filter_map(|r| r.iter().find(|c| !c.is_empty()).map(String::as_str))
        .collect();
    let declared_range = preamble.first().and_then(|line| period(line));
    let scheme = preamble.iter().skip(1).last().map(|s| s.to_string());

    let mut builder = SeriesBuilder::new();
    builder.extras(&["Repurchase Price", "Sale Price"]);
    for row in range.rows().skip(header_at + 1) {
        let cell = |i: Option<usize>| i.and_then(|i| row.get(i)).unwrap_or(&Data::Empty);
        let Some(day) = cell_date(cell(Some(date_col)), DATE_FORMATS) else {
            continue;
        };
        let mut p = PricePoint::new(day);
        p.nav = cell_price(cell(nav));
        for (name, i) in [("Repurchase Price", repurchase), ("Sale Price", sale)] {
            if let Some(v) = cell_price(cell(i)) {
                p.extra.insert(name.to_string(), v);
            }
        }
        builder.push(p);
    }

    let mut meta = Meta::new(Format::MarketAmfiNav, Field::Nav);
    meta.code = scheme;
    meta.kind = Some(InstrumentKind::MutualFund);
    meta.currency = Some("INR".to_string());
    meta.declared_range = declared_range;
    Ok(builder.finish(meta, Vec::new()))
}

/// "Historical NAV Data for From 26-Sep-2026 to 29-Sep-2026".
fn period(line: &str) -> Option<DateRange> {
    let rest = line.get(PERIOD_PREFIX.len()..)?.trim();
    let (from, to) = rest.split_once(" to ")?;
    let from = common::date(from, DATE_FORMATS)?;
    let to = common::date(to, DATE_FORMATS)?;
    (from <= to).then_some(DateRange {
        from,
        to,
        source: RangeSource::Content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_printed_period() {
        let r = period("Historical NAV Data for From 02-Apr-2006 to 31-Mar-2011").unwrap();
        assert_eq!(r.from.to_string(), "2006-04-02");
        assert_eq!(r.to.to_string(), "2011-03-31");
        assert_eq!(r.source, RangeSource::Content);
    }
}
