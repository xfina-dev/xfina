//! What every market data parser needs: reading a publisher's numbers and
//! dates, reading a filename for what the content leaves out, and building a
//! series with the checks every series gets.

// A build with only some publishers compiled in uses only some of these.
#![cfg_attr(not(feature = "all"), allow(dead_code))]

use std::collections::BTreeMap;
use std::str::FromStr;

use chrono::{NaiveDate, NaiveTime};
use rust_decimal::Decimal;

use crate::detect::Format;
use crate::models::series::{
    infer_frequency, DateRange, Field, InstrumentKind, PricePoint, PriceSeries, RangeSource,
};
use crate::models::validation::{SummaryCheck, ValidationReport};

/// A number as a publisher prints it, or `None` when it prints none.
///
/// Thousands separators go, whichever a publisher uses: commas in either the
/// Western or the Indian grouping (`12,34,567`), and the right single quote
/// Swiss funds group with. A dash, a double dash, `N/A` or an empty cell is no
/// value. Scientific notation is accepted for floats written by a program.
pub(crate) fn number(text: &str) -> Option<Decimal> {
    let cleaned: String = text
        .trim()
        .trim_matches('"')
        .chars()
        .filter(|c| !matches!(c, ',' | '\u{2019}' | '\'' | ' ' | '\u{a0}'))
        .collect();
    if cleaned.is_empty() || matches!(cleaned.as_str(), "-" | "--" | "N/A" | "NA" | "null") {
        return None;
    }
    Decimal::from_str(&cleaned)
        .or_else(|_| Decimal::from_scientific(&cleaned))
        .ok()
}

/// A price: a number that is not zero.
///
/// Publishers fill a missing price with 0 -- Nasdaq's first row is today's
/// date with every figure zero, and its early history prints 0 for high and
/// low -- and a zero passed on as a price is a 100% loss in somebody's chart.
pub(crate) fn price(text: &str) -> Option<Decimal> {
    number(text).filter(|d| !d.is_zero())
}

/// A float as a spreadsheet stores it, exactly as Excel would display it.
///
/// Going through the shortest round-trip text of the float rather than
/// `Decimal::from_f64` keeps `12345.678901234567` from turning into a longer
/// run of binary noise.
pub(crate) fn from_float(value: f64) -> Option<Decimal> {
    if !value.is_finite() {
        return None;
    }
    number(&value.to_string())
}

/// Parses a date in any of the given `chrono` formats.
///
/// "Sept" is read as "Sep" first: iShares writes the month that way, and
/// `%b` only knows three-letter abbreviations.
pub(crate) fn date(text: &str, formats: &[&str]) -> Option<NaiveDate> {
    let text = text.trim().trim_matches('"');
    let text = if text.contains("September") {
        text.to_string()
    } else {
        text.replace("Sept", "Sep")
    };
    formats
        .iter()
        .find_map(|f| NaiveDate::parse_from_str(&text, f).ok())
}

/// A filename with any directory and every extension removed.
///
/// `SPY_1993-01-29_to_2026-10-01.tiingo.csv` becomes
/// `SPY_1993-01-29_to_2026-10-01`. A dot inside a ticker survives when it is
/// not followed by a known extension, so `SGLD.L.csv` keeps `SGLD.L`.
pub(crate) fn stem(filename: &str) -> &str {
    let base = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    let mut stem = base;
    loop {
        let lower = stem.to_ascii_lowercase();
        let Some(dot) = lower.rfind('.') else {
            break;
        };
        let ext = &lower[dot + 1..];
        if matches!(
            ext,
            "csv" | "xls" | "xlsx" | "tiingo" | "wsj" | "yfinance" | "txt"
        ) {
            stem = &stem[..dot];
        } else {
            break;
        }
    }
    stem
}

/// The ticker a file was named after: everything before the first `_`, as the
/// bookmarklets and a user saving one file per ticker both name them.
///
/// A leading `^` (Yahoo's index prefix) is dropped. Names that are the
/// publisher's generic default rather than a ticker give `None`.
#[cfg(any(feature = "md-tiingo", feature = "md-wsj", feature = "md-yahoo"))]
pub(crate) fn ticker(filename: Option<&str>) -> Option<String> {
    let stem = stem(filename?);
    let head = stem.split('_').next().unwrap_or(stem).trim();
    let head = head.trim_start_matches('^');
    let generic = ["historicalprices", "download", "data", "prices"];
    if head.is_empty()
        || head.contains(char::is_whitespace)
        || generic.contains(&head.to_ascii_lowercase().as_str())
    {
        return None;
    }
    Some(head.to_ascii_uppercase())
}

/// The range in a `<anything>_<from>_to_<to>` name, in any of `formats`.
#[cfg(any(feature = "md-tiingo", feature = "md-wsj", feature = "md-yahoo"))]
pub(crate) fn range_around_to(filename: Option<&str>, formats: &[&str]) -> Option<DateRange> {
    let stem = stem(filename?);
    let (left, right) = stem.split_once("_to_")?;
    let from = left.rsplit('_').next()?;
    let to = right.split('_').next()?;
    let from = date(from, formats)?;
    let to = date(to, formats)?;
    (from <= to).then_some(DateRange {
        from,
        to,
        source: RangeSource::Filename,
    })
}

/// The first two runs of exactly `digits` digits in a name, as dates.
///
/// For the names publishers build out of compact dates: NSE Indices'
/// `…_TR_01092026to02102026` (`%d%m%Y`) and Nasdaq's
/// `EODHist_19850131-20260926_NDX` (`%Y%m%d`).
#[cfg(any(feature = "md-nse-indices", feature = "md-nasdaq"))]
pub(crate) fn compact_range(filename: Option<&str>, format: &str) -> Option<DateRange> {
    let stem = stem(filename?);
    let runs: Vec<&str> = stem
        .split(|c: char| !c.is_ascii_digit())
        .filter(|run| run.len() == 8)
        .collect();
    let (from, to) = match runs.as_slice() {
        [from, to, ..] => (
            NaiveDate::parse_from_str(from, format).ok()?,
            NaiveDate::parse_from_str(to, format).ok()?,
        ),
        _ => return None,
    };
    (from <= to).then_some(DateRange {
        from,
        to,
        source: RangeSource::Filename,
    })
}

/// A CSV file as rows of trimmed cells, headers included.
///
/// Flexible about row length, because Yahoo's classic export puts a
/// two-column dividend line between its seven-column price rows.
#[cfg(any(
    feature = "md-nse-security",
    feature = "md-nse-indices",
    feature = "md-tiingo",
    feature = "md-wsj",
    feature = "md-yahoo",
))]
pub(crate) fn csv_rows(text: &str) -> Result<Vec<Vec<String>>, crate::error::XfinaError> {
    let text = text.trim_start_matches('\u{FEFF}');
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());
    let mut rows = Vec::new();
    for record in reader.records() {
        let record =
            record.map_err(|e| crate::error::XfinaError::ParseError(format!("CSV: {}", e)))?;
        rows.push(record.iter().map(|c| c.trim().to_string()).collect());
    }
    Ok(rows)
}

/// The first line of a text file, BOM and quotes removed, cells trimmed and
/// rejoined with bare commas, internal whitespace collapsed: the form CSV
/// probes compare headings in.
#[cfg(any(
    feature = "md-nse-security",
    feature = "md-nse-indices",
    feature = "md-tiingo",
    feature = "md-wsj",
    feature = "md-yahoo",
))]
pub(crate) fn header_line(probe_text: &str) -> String {
    let line = probe_text
        .trim_start_matches('\u{FEFF}')
        .lines()
        .next()
        .unwrap_or("");
    line.split(',')
        .map(|cell| {
            cell.trim()
                .trim_matches('"')
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// A heading with its padding collapsed, for matching and as an `extra` key.
pub(crate) fn heading(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Where each named column sits in a heading row, matched case-insensitively
/// after collapsing whitespace.
pub(crate) fn column_index(headings: &[String], name: &str) -> Option<usize> {
    headings
        .iter()
        .position(|h| heading(h).eq_ignore_ascii_case(name))
}

/// A calamine cell as text, the way the parsers compare headings.
#[cfg(any(
    feature = "md-amfi-nav",
    feature = "md-msci",
    feature = "md-nasdaq",
    feature = "md-spdr-gold",
))]
pub(crate) fn cell_text(cell: &calamine::Data) -> String {
    heading(&cell.to_string().replace('\u{0}', ""))
}

/// A calamine cell as a number.
#[cfg(any(
    feature = "md-amfi-nav",
    feature = "md-msci",
    feature = "md-nasdaq",
    feature = "md-spdr-gold",
))]
pub(crate) fn cell_number(cell: &calamine::Data) -> Option<Decimal> {
    use calamine::Data;
    match cell {
        Data::Float(f) => from_float(*f),
        Data::Int(i) => Some(Decimal::from(*i)),
        Data::String(s) => number(s),
        _ => None,
    }
}

/// A calamine cell as a price: a number that is not zero.
#[cfg(any(
    feature = "md-amfi-nav",
    feature = "md-msci",
    feature = "md-nasdaq",
    feature = "md-spdr-gold",
))]
pub(crate) fn cell_price(cell: &calamine::Data) -> Option<Decimal> {
    cell_number(cell).filter(|d| !d.is_zero())
}

/// A calamine cell as a date: a real date cell, an ISO string, or text in one
/// of the given formats.
#[cfg(any(
    feature = "md-amfi-nav",
    feature = "md-msci",
    feature = "md-nasdaq",
    feature = "md-spdr-gold",
))]
pub(crate) fn cell_date(cell: &calamine::Data, formats: &[&str]) -> Option<NaiveDate> {
    use calamine::Data;
    match cell {
        Data::DateTime(dt) => {
            // Calamine's chrono conversion sits behind a feature; the
            // calendar parts are all a date needs.
            let (y, m, d, ..) = dt.to_ymd_hms_milli();
            NaiveDate::from_ymd_opt(i32::from(y), u32::from(m), u32::from(d))
        }
        Data::DateTimeIso(s) => date(s.get(..10).unwrap_or(s), &["%Y-%m-%d"]),
        Data::String(s) => date(s, formats).or_else(|| date(s, &["%Y-%m-%d"])),
        _ => None,
    }
}

/// What a parser knows about the file apart from its rows.
pub(crate) struct Meta {
    pub source: Format,
    pub code: Option<String>,
    pub name: Option<String>,
    pub kind: Option<InstrumentKind>,
    pub currency: Option<String>,
    pub unit: Option<String>,
    pub headline: Field,
    pub split_adjusted: Option<bool>,
    pub declared_range: Option<DateRange>,
}

impl Meta {
    pub fn new(source: Format, headline: Field) -> Self {
        Meta {
            source,
            code: None,
            name: None,
            kind: None,
            currency: None,
            unit: None,
            headline,
            split_adjusted: None,
            declared_range: None,
        }
    }
}

/// Collects rows in whatever order the file prints them, and hands back a
/// series in date order.
#[derive(Default)]
pub(crate) struct SeriesBuilder {
    rows: BTreeMap<(NaiveDate, Option<NaiveTime>), PricePoint>,
    /// `extra` headings in the order the publisher prints them.
    extra_order: Vec<String>,
    /// Rows the file printed again, identically, for the same day and time.
    repeats: usize,
    /// Rows the file printed again for the same day and time with different
    /// figures.
    conflicts: usize,
}

impl SeriesBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares the `extra` headings this file prints, in its own order.
    pub fn extras<S: AsRef<str>>(&mut self, names: &[S]) {
        for name in names {
            let name = name.as_ref().to_string();
            if !self.extra_order.contains(&name) {
                self.extra_order.push(name);
            }
        }
    }

    /// A row the file printed.
    ///
    /// A second row for the same day and time keeps the first. An identical
    /// one is the same reading listed twice -- MCX's archive does this for
    /// years at a stretch -- and is only counted; one with different figures
    /// is a conflict the report fails on.
    pub fn push(&mut self, point: PricePoint) {
        let key = (point.date, point.time);
        match self.rows.get(&key) {
            Some(existing) if *existing == point => self.repeats += 1,
            Some(_) => self.conflicts += 1,
            None => {
                self.rows.insert(key, point);
            }
        }
    }

    /// The row for a day, created if the file has not printed one: for a
    /// figure the file prints apart from the day's prices, such as Yahoo's
    /// dividend lines.
    pub fn row(&mut self, date: NaiveDate, time: Option<NaiveTime>) -> &mut PricePoint {
        self.rows.entry((date, time)).or_insert_with(|| {
            let mut p = PricePoint::new(date);
            p.time = time;
            p
        })
    }

    pub fn finish(
        self,
        meta: Meta,
        mut checks: Vec<SummaryCheck>,
    ) -> crate::models::validation::ParseResult<PriceSeries> {
        // A row with neither a price nor an event is a row the publisher
        // printed with nothing in it (a holiday, a zero row).
        let rows: Vec<PricePoint> = self
            .rows
            .into_values()
            .filter(|r| r.has_price() || r.dividend.is_some() || r.split_factor.is_some())
            .collect();

        let fields: Vec<Field> = Field::ALL
            .iter()
            .copied()
            .filter(|f| rows.iter().any(|r| r.get(*f).is_some()))
            .collect();
        let mut extra_fields: Vec<String> = self
            .extra_order
            .into_iter()
            .filter(|name| rows.iter().any(|r| r.extra.contains_key(name)))
            .collect();
        for row in &rows {
            for key in row.extra.keys() {
                if !extra_fields.contains(key) {
                    extra_fields.push(key.clone());
                }
            }
        }

        let series = PriceSeries {
            source: meta.source,
            code: meta.code,
            name: meta.name,
            kind: meta.kind,
            currency: meta.currency,
            unit: meta.unit,
            frequency: infer_frequency(&rows, meta.headline),
            fields,
            extra_fields,
            headline: meta.headline,
            split_adjusted: meta.split_adjusted,
            rows,
            declared_range: meta.declared_range,
        };

        checks.extend(standard_checks(&series, self.conflicts, self.repeats));
        let mut validation = ValidationReport::empty();
        validation.summary_level.checks = checks;
        validation.finalize();
        crate::models::validation::ParseResult {
            data: series,
            validation,
        }
    }
}

/// The checks every series gets, whoever published it.
fn standard_checks(series: &PriceSeries, conflicts: usize, repeats: usize) -> Vec<SummaryCheck> {
    let count = |n: usize| Decimal::from(n as u64);
    let valued = series.valued_rows().count();
    let mut checks = vec![
        // A file with no values in it is still that publisher's file -- an
        // empty answer for a ticker it does not carry -- but it is not a
        // price history, and says so.
        SummaryCheck::declared(
            "has_rows",
            Decimal::ONE,
            Decimal::from(u64::from(valued > 0)),
            Some(format!(
                "{} rows with a {} value",
                valued,
                series.headline.as_str()
            )),
        ),
        SummaryCheck::derived(
            "conflicting_rows",
            Decimal::ZERO,
            count(conflicts),
            (repeats > 0).then(|| format!("{} rows repeated exactly, kept once", repeats)),
        ),
    ];

    if let Some(range) = series.declared_range {
        let outside = series
            .valued_rows()
            .filter(|r| !range.contains(r.date))
            .count();
        let note = Some(format!("{} to {}", range.from, range.to));
        checks.push(match range.source {
            RangeSource::Content => SummaryCheck::declared(
                "rows_outside_declared_range",
                Decimal::ZERO,
                count(outside),
                note,
            ),
            RangeSource::Filename => SummaryCheck::derived(
                "rows_outside_declared_range",
                Decimal::ZERO,
                count(outside),
                note,
            ),
        });
    }

    // Volume may be zero (a day with no trades); nothing else may be zero or
    // below. Zeros are already dropped on the way in, so this catches the
    // negatives.
    let non_positive = series
        .rows
        .iter()
        .flat_map(|r| {
            Field::ALL.iter().filter_map(move |f| match f {
                Field::Volume | Field::AdjVolume => r.get(*f).filter(|v| v.is_sign_negative()),
                _ => r.get(*f).filter(|v| *v <= Decimal::ZERO),
            })
        })
        .count();
    checks.push(SummaryCheck::derived(
        "non_positive_values",
        Decimal::ZERO,
        count(non_positive),
        None,
    ));
    checks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_lose_every_grouping() {
        assert_eq!(number("12,34,567"), Some(Decimal::from(1_234_567)));
        assert_eq!(number("\"1,758.57\""), Decimal::from_str("1758.57").ok());
        assert_eq!(
            number("1\u{2019}234\u{2019}567"),
            Some(Decimal::from(1_234_567))
        );
        assert_eq!(number("-"), None);
        assert_eq!(number("--"), None);
        assert_eq!(number(""), None);
        assert_eq!(number("-1.5"), Decimal::from_str("-1.5").ok());
        assert_eq!(number("1e-3"), Decimal::from_str("0.001").ok());
    }

    #[test]
    fn a_zero_is_not_a_price() {
        assert_eq!(price("0"), None);
        assert_eq!(price("0.00"), None);
        assert_eq!(price("12.5"), Decimal::from_str("12.5").ok());
    }

    #[test]
    fn floats_keep_the_digits_excel_shows() {
        assert_eq!(
            from_float(12345.678901234567).unwrap().to_string(),
            "12345.678901234567"
        );
        assert_eq!(from_float(0.1).unwrap().to_string(), "0.1");
    }

    #[test]
    fn sept_is_september() {
        assert_eq!(
            date("28/Sept/2026", &["%d/%b/%Y"]),
            NaiveDate::from_ymd_opt(2026, 9, 28)
        );
    }

    #[test]
    fn stems_keep_dots_inside_tickers() {
        assert_eq!(stem("dir/SGLD.L.csv"), "SGLD.L");
        assert_eq!(
            stem("SPY_1993-01-29_to_2026-10-01.tiingo.csv"),
            "SPY_1993-01-29_to_2026-10-01"
        );
    }
}
