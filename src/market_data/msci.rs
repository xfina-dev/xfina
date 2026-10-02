//! MSCI's index level export, from an index page's Performance tab.
//!
//! A header block -- "Index Level:" with the variant (Net, Gross or Price),
//! "Currency:" -- then a table of dates and levels, with MSCI's disclaimer
//! set in the columns beside it. The variant decides the field: a Net level
//! is a net total return, a Gross level a total return, a Price level a
//! price index. The index code is not in the file; MSCI puts it at the start
//! of the download's name.

use calamine::Data;

use super::common::{self, cell_date, cell_price, cell_text, Meta, SeriesBuilder};
use crate::decode::Decoded;
use crate::detect::{Claim, Format};
use crate::error::XfinaError;
use crate::models::request::ParseRequest;
use crate::models::series::{
    DateRange, Field, InstrumentKind, PricePoint, PriceSeries, RangeSource,
};
use crate::models::validation::ParseResult;

const DATE_FORMATS: &[&str] = &["%Y-%m-%d", "%b %d, %Y", "%d/%m/%Y"];

/// Reads an MSCI index level export.
pub fn parse_msci(input: ParseRequest<'_>) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

pub(crate) fn probe(decoded: &Decoded<'_>) -> Claim {
    let Ok(sheets) = decoded.sheets() else {
        return Claim::NO;
    };
    let head = sheets.head_text(6);
    if head.contains("INDEX LEVEL:") && head.contains("CURRENCY:") && head.contains("MSCI") {
        Claim::strong("msci-index-levels")
    } else {
        Claim::NO
    }
}

pub(crate) fn parse_decoded(
    decoded: &Decoded<'_>,
    input: &ParseRequest<'_>,
) -> Result<ParseResult<PriceSeries>, XfinaError> {
    let range = decoded.sheets()?.first()?;
    let text: Vec<Vec<String>> = range
        .rows()
        .map(|r| r.iter().map(cell_text).collect())
        .collect();

    let label = |name: &str| {
        text.iter().find_map(|r| {
            (r.first().map(|c| c.eq_ignore_ascii_case(name)) == Some(true))
                .then(|| r.get(1).cloned())
                .flatten()
        })
    };
    let variant = label("Index Level:").ok_or_else(|| {
        XfinaError::InvalidFormat("Not an MSCI export: no Index Level line".to_string())
    })?;
    let headline = match variant.to_ascii_lowercase().as_str() {
        "net" => Field::NetTotalReturn,
        "gross" => Field::TotalReturn,
        "price" => Field::Close,
        _ => {
            return Err(XfinaError::ParseError(
                "MSCI export names an index level variant this reader does not know".to_string(),
            ))
        }
    };

    let header_at = text
        .iter()
        .position(|r| r.first().map(String::as_str) == Some("Date"))
        .ok_or_else(|| XfinaError::ParseError("MSCI export has no Date heading".to_string()))?;
    let head = &text[header_at];
    // The first column after Date is the index; any more are indices the
    // chart was compared against, kept under their names. The disclaimer
    // starts after an empty heading.
    let columns: Vec<(usize, String)> = head
        .iter()
        .enumerate()
        .skip(1)
        .take_while(|(_, h)| !h.is_empty())
        .map(|(i, h)| (i, h.clone()))
        .collect();
    let Some(((index_col, index_name), compared)) = columns.split_first() else {
        return Err(XfinaError::ParseError(
            "MSCI export has no index column".to_string(),
        ));
    };

    let mut builder = SeriesBuilder::new();
    builder.extras(&compared.iter().map(|(_, h)| h.as_str()).collect::<Vec<_>>());
    for row in range.rows().skip(header_at + 1) {
        let cell = |i: usize| row.get(i).unwrap_or(&Data::Empty);
        let Some(date) = cell_date(cell(0), DATE_FORMATS) else {
            continue;
        };
        let mut p = PricePoint::new(date);
        p.set(headline, cell_price(cell(*index_col)));
        for (i, name) in compared {
            if let Some(v) = cell_price(cell(*i)) {
                p.extra.insert(name.clone(), v);
            }
        }
        builder.push(p);
    }

    let mut meta = Meta::new(Format::MarketMsci, headline);
    meta.name = Some(index_name.clone());
    meta.code = code_from_name(input.filename).or_else(|| Some(index_name.clone()));
    meta.kind = Some(InstrumentKind::Index);
    meta.currency = label("Currency:").filter(|c| !c.is_empty());
    meta.declared_range = range_from_name(input.filename);
    Ok(builder.finish(meta, Vec::new()))
}

/// MSCI names a download "<code> - <index> - <span> - <from> - <to> - <frequency>".
fn code_from_name(filename: Option<&str>) -> Option<String> {
    let stem = common::stem(filename?);
    let code = stem.split(" - ").next()?.trim();
    (!code.is_empty() && code.chars().all(|c| c.is_ascii_digit())).then(|| code.to_string())
}

fn range_from_name(filename: Option<&str>) -> Option<DateRange> {
    let stem = common::stem(filename?);
    let dates: Vec<chrono::NaiveDate> = stem
        .split(" - ")
        .filter_map(|part| common::date(part.trim(), &["%Y-%m-%d"]))
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
    fn the_name_carries_the_code_and_period() {
        let name = "000000 - MSCI Example Index  - FULL - 2000-01-31 - 2026-09-25  - Monthly.xlsx";
        assert_eq!(code_from_name(Some(name)).as_deref(), Some("000000"));
        let r = range_from_name(Some(name)).unwrap();
        assert_eq!(r.from.to_string(), "2000-01-31");
        assert_eq!(r.to.to_string(), "2026-09-25");
    }
}
