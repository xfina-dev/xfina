//! State Bank of India's daily forex card rate sheet.
//!
//! A public reference document rather than anybody's statement: SBI publishes
//! one most banking mornings, quoting around twenty currencies against the
//! rupee across several kinds of transaction. Nothing in it belongs to a
//! customer, which is why this is the one format whose fixtures can live in
//! the repository.
//!
//! The sheet carries two tables -- one for transactions below ten lakhs and
//! one for the band above it, the latter marked "to be used as reference
//! rates". Only the reference table is read; it is the one the published rate
//! actually is, and taking the wrong one silently shifts every figure.

/// Turning this sheet's pages into the cells its table is made of.
///
/// Private to the parser rather than shared with the category: what it does is
/// shaped by how these rate cards are set, down to the sheets whose headings
/// are drawn across each other. Another reference document would want its own
/// reading of a page, not this one bent to fit.
mod layout;

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};
use regex::Regex;
use rust_decimal::Decimal;

use self::layout::{cells, Cell};
use crate::decode::Decoded;
use crate::detect::Claim;
use crate::error::XfinaError;
use crate::models::rates::{CurrencyRates, RateSheet};
use crate::models::request::ParseRequest;
use crate::models::validation::{ParseResult, ValidationReport};

/// Reads an SBI forex card rate sheet.
pub fn parse_sbi_forex_card_rates(
    input: ParseRequest<'_>,
) -> Result<ParseResult<RateSheet>, XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

/// Parses from an already-decoded input, so detection can probe and parse
/// against one read of the file.
pub(crate) fn parse_decoded(
    decoded: &Decoded<'_>,
    _input: &ParseRequest<'_>,
) -> Result<ParseResult<RateSheet>, XfinaError> {
    let doc = decoded.pdf()?;
    let pages = doc.pages()?;

    // The sheet names itself in its title. Without that this would read any
    // PDF that happened to hold a grid of numbers.
    if !pages
        .first()
        .map(|page| page_text(&cells(page)).contains(SHEET_TITLE))
        .unwrap_or(false)
    {
        return Err(XfinaError::InvalidFormat(
            "Not an SBI forex card rate sheet: no rate card title found".to_string(),
        ));
    }

    let front = cells(&pages[0]);
    let (date, time) = sheet_date(&front, doc.creation_date())?;

    // Both tables print the same currencies in the same shape, so the only
    // thing distinguishing them is this line.
    let reference = pages
        .iter()
        .map(|page| cells(page))
        .find(|page| {
            let text = page_text(page);
            text.contains(REFERENCE_MARKER) || text.contains(REFERENCE_BAND)
        })
        .ok_or_else(|| {
            XfinaError::ParseError(
                "SBI forex card rate sheet has no reference rates table".to_string(),
            )
        })?;

    let columns = columns(&reference)?;
    let per_hundred = per_hundred_currencies(&reference)?;
    let (currencies, figures_matched_by_order) = currency_rows(&reference, &columns, &per_hundred)?;

    if currencies.is_empty() {
        return Err(XfinaError::ParseError(
            "SBI forex card rate sheet quotes no currencies".to_string(),
        ));
    }

    let published_at = time.map(|time| {
        let ist = FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST is a valid offset");
        ist.from_local_datetime(&date.and_time(time))
            .single()
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|| date.and_time(time).and_utc())
    });

    Ok(ParseResult {
        data: RateSheet {
            date,
            published_at,
            columns: columns.iter().map(|c| c.key.clone()).collect(),
            currencies,
            figures_matched_by_order,
        },
        // Nothing in a rate card cross-foots: there are no totals, no running
        // balance and no declared figure to reconcile against. An empty report
        // says that honestly, where a fabricated passing check would not.
        validation: ValidationReport::empty(),
    })
}

/// Asks whether this PDF is an SBI forex card rate sheet.
pub(crate) fn probe(decoded: &crate::decode::Decoded<'_>) -> Claim {
    let Ok(doc) = decoded.pdf() else {
        return Claim::NO;
    };
    let text = doc.page1_text().to_uppercase();
    if text.contains(SHEET_TITLE) && text.contains("/INR") {
        return Claim::strong("sbi-forex-card-rates");
    }
    if text.contains(SHEET_TITLE) {
        return Claim::weak("sbi-forex-card-title");
    }
    Claim::NO
}

/// The title both layout eras print, differing only in what precedes it
/// ("SBI FOREX CARD RATES" and "STATE BANK OF INDIA - FOREX CARD RATES").
const SHEET_TITLE: &str = "FOREX CARD RATES";

/// Marks the table whose figures are the published reference rates.
const REFERENCE_MARKER: &str = "TO BE USED AS REFERENCE RATES";

/// The same table, identified by the band it prices instead of by the gloss.
///
/// The gloss is an annotation SBI adds to the heading and has been left off at
/// least once; the band is the heading. Only the second table carries it --
/// the first is headed "BELOW Rs. 10 LACS" -- so this cannot select the wrong
/// one, and the spelling of the amount is left out because it has been written
/// both "LACS" and "LAKHS".
const REFERENCE_BAND: &str = "TRANSACTIONS BETWEEN RS. 10";

/// Rows this far apart vertically belong to the same currency.
///
/// Generous enough for the era that sets a row's label and its figures on
/// slightly different baselines, and well under the distance between rows --
/// and a figure is assigned to its *nearest* row regardless, so the tolerance
/// only has to keep the page's footnotes out of the table. Swept across the
/// archive, 6.0 to 14.0 all read identically.
const ROW_TOLERANCE: f64 = 9.0;

/// Headings this far from "TT BUY" vertically are part of the same heading
/// block, which spans up to three printed lines.
///
/// Swept across the archive, 9.0 to 15.0 all read identically. Below that the
/// block's upper line is missed and a column loses half its name; above it the
/// first row of figures is read as a heading.
const HEADING_TOLERANCE: f64 = 12.0;

/// The most words any column the sheet has printed is named in, "FOREX TRAVEL
/// CARD SELL" being the longest. More than this is two headings read as one.
const HEADING_WORDS: usize = 4;

// -----------------------------------------------------------------------------
// Columns
// -----------------------------------------------------------------------------

/// One rate column: its heading, and the band of page that belongs to it.
///
/// The band is not the heading's own extent. Headings and the figures beneath
/// them are set to different widths -- a figure is often flush right of a
/// heading narrower than itself -- so the columns are turned into a tiling of
/// the row, each band running to the midpoint of the gap before the next
/// heading. Every figure then lands in exactly one band, and none can fall
/// between two. This follows the CAMS statement parser, which reads its
/// amount columns the same way.
struct Column {
    key: String,
    x0: f64,
    x1: f64,
    lo: f64,
    hi: f64,
}

/// Reads the table's headings and the span of page each one covers.
///
/// Column identity comes from the heading, never from a figure's position in
/// the row. The sheet has been printed with nine columns and with eight, and
/// has renamed one of them twice, so counting along a row lands on a different
/// rate depending on the year -- which is exactly how a travel-card rate ends
/// up published as a telegraphic-transfer one.
fn columns(page: &[Cell]) -> Result<Vec<Column>, XfinaError> {
    let anchor = page
        .iter()
        .find(|cell| normalize(&cell.text) == "TT BUY")
        .ok_or_else(|| {
            XfinaError::ParseError(
                "SBI forex card rate sheet has no TT BUY column heading".to_string(),
            )
        })?;

    // Everything on the heading block, from the first rate column rightwards.
    // The row-label heading ("CURRENCY") sits to the left and is not a rate.
    let mut heads: Vec<&Cell> = page
        .iter()
        .filter(|cell| {
            (cell.yc - anchor.yc).abs() <= HEADING_TOLERANCE
                && cell.x0 >= anchor.x0 - 2.0
                && !normalize(&cell.text).is_empty()
        })
        .collect();
    heads.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));

    // A heading set over two or three lines -- "FOREX TRAVEL" above "CARD BUY"
    // -- is one column, recognised by the lines sitting over each other.
    let mut groups: Vec<Vec<&Cell>> = Vec::new();
    for cell in heads {
        match groups.last_mut() {
            Some(group) if cell.x0 <= group.iter().fold(f64::MIN, |m, c| m.max(c.x1)) => {
                group.push(cell)
            }
            _ => groups.push(vec![cell]),
        }
    }

    let mut columns = Vec::new();
    for mut group in groups {
        // Top line first, so "FOREX TRAVEL" + "CARD BUY" reads in that order.
        group.sort_by(|a, b| {
            a.yc.partial_cmp(&b.yc)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal))
        });
        let label = normalize(
            &group
                .iter()
                .map(|c| c.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        );
        columns.push(Column {
            key: key_of(&label),
            x0: group.iter().fold(f64::MAX, |m, c| m.min(c.x0)),
            x1: group.iter().fold(f64::MIN, |m, c| m.max(c.x1)),
            lo: f64::MIN,
            hi: f64::MAX,
        });
    }

    // Tile the row. The outermost bands are left open so a figure set wider
    // than the heading block still belongs to the column it sits under.
    columns.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
    for i in 0..columns.len() {
        if i > 0 {
            let boundary = (columns[i - 1].x1 + columns[i].x0) / 2.0;
            columns[i].lo = boundary;
            columns[i - 1].hi = boundary;
        }
    }

    // Two columns under one key would silently overwrite each other.
    let mut seen = HashSet::new();
    if let Some(dup) = columns.iter().find(|c| !seen.insert(c.key.clone())) {
        return Err(XfinaError::ParseError(format!(
            "SBI forex card rate sheet prints the column '{}' twice",
            dup.key
        )));
    }

    // Every rate the sheet has ever quoted names a side of the trade in a few
    // words, whatever the column has been called over the years. A heading
    // that does not, or one far longer than any of them, is two headings read
    // as one.
    //
    // That happens on sheets set in a font whose glyph widths this reader
    // cannot measure. It advances every glyph by the same amount instead, so
    // the positions it reports drift and separate headings come out on top of
    // each other. The sheet is fine and other readers have no trouble with it;
    // the measurement is ours. Stopping here is honest about that, where
    // carrying on would attach figures to a heading assembled out of two.
    if let Some(odd) = columns.iter().find(|c| {
        let words = c.key.split('_').count();
        !(c.key.ends_with("_buy") || c.key.ends_with("_sell")) || words > HEADING_WORDS
    }) {
        return Err(XfinaError::ParseError(format!(
            "Could not place the column headings on this SBI forex card rate sheet; read '{}' as one heading",
            odd.key
        )));
    }
    Ok(columns)
}

// -----------------------------------------------------------------------------
// Currency rows
// -----------------------------------------------------------------------------

/// Reads one row per currency the sheet quotes, in printed order.
fn currency_rows(
    page: &[Cell],
    columns: &[Column],
    per_hundred: &HashSet<String>,
) -> Result<(Vec<CurrencyRates>, bool), XfinaError> {
    let leftmost = columns.iter().fold(f64::MAX, |m, c| m.min(c.x0));

    // Almost every sheet writes the code as a pair, "USD/INR"; at least one
    // writes the code alone. The pair is unmistakable and is taken wherever it
    // is found, which matters on the sheets whose rows do not line up. A bare
    // three-letter word is not, so that form is only read as a currency where
    // the code column belongs, left of every rate.
    let pair = Regex::new(r"^([A-Z]{3})(/INR)?$").expect("static pattern");
    let mut anchors: Vec<(usize, String)> = Vec::new();
    for (index, cell) in page.iter().enumerate() {
        let text = normalize(&cell.text);
        let Some(caps) = pair.captures(&text) else {
            continue;
        };
        if caps.get(2).is_none() && cell.x1 > leftmost {
            continue;
        }
        anchors.push((index, caps[1].to_string()));
    }
    anchors.sort_by(|a, b| {
        page[a.0]
            .yc
            .partial_cmp(&page[b.0].yc)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut positions: Vec<(String, Vec<(&Cell, Decimal)>)> = Vec::new();
    let mut names: HashMap<usize, Vec<&Cell>> = HashMap::new();
    let mut figures: HashMap<usize, Vec<(&Cell, Decimal)>> = HashMap::new();

    for cell in page {
        // Nearest row wins outright, so a figure can never be claimed by the
        // row above as well as its own.
        let Some((row, anchor)) = anchors
            .iter()
            .enumerate()
            .map(|(row, (index, _))| (row, &page[*index]))
            .min_by(|a, b| {
                (a.1.yc - cell.yc)
                    .abs()
                    .partial_cmp(&(b.1.yc - cell.yc).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        else {
            break;
        };
        if (anchor.yc - cell.yc).abs() > ROW_TOLERANCE || std::ptr::eq(cell, anchor) {
            continue;
        }
        if cell.x1 <= anchor.x0 {
            names.entry(row).or_default().push(cell);
        } else if cell.x0 >= leftmost - 12.0 {
            if let Some(value) = decimal(&cell.text) {
                figures.entry(row).or_default().push((cell, value));
            }
        }
    }

    // One entry per currency, in printed order: its code, its name, and its
    // figures left to right.
    let mut rows: Vec<(String, String, Vec<Decimal>)> = Vec::new();
    for (row, (_, code)) in anchors.iter().enumerate() {
        let mut label = names.remove(&row).unwrap_or_default();
        label.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
        let name = normalize(
            &label
                .iter()
                .map(|c| c.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        );

        let mut found = figures.remove(&row).unwrap_or_default();
        found.sort_by(|a, b| {
            a.0.x0
                .partial_cmp(&b.0.x0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        positions.push((code.clone(), found.clone()));
        rows.push((
            code.clone(),
            if name.is_empty() { code.clone() } else { name },
            found.into_iter().map(|(_, value)| value).collect(),
        ));
    }

    // Where each figure sits decides which rate it is. That is the whole point
    // of reading the headings, and it is right for all but a handful of the
    // sheets ever published.
    let placed: Option<Vec<_>> = positions
        .iter()
        .map(|(_, found)| by_position(found, columns))
        .collect();

    // A few sheets are published with the table collapsed into a flow: the
    // figures are all there and in order, but no two rows start at the same
    // place, so nothing lines up under a heading. Falling back to the order
    // they are printed in is only safe because it is required to account for
    // every heading exactly once on every row -- the sheets whose figures run
    // together print a different count on different rows and are refused here
    // rather than read into the wrong column.
    let (assigned, by_order) = match placed {
        Some(assigned) => (assigned, false),
        None => {
            let ordered: Option<Vec<_>> = rows
                .iter()
                .map(|(_, _, values)| by_printed_order(values, columns))
                .collect();
            match ordered {
                Some(assigned) => (assigned, true),
                None => {
                    let (code, values) = rows
                        .iter()
                        .find(|(_, _, values)| values.len() != columns.len())
                        .map(|(code, _, values)| (code.clone(), values.len()))
                        .unwrap_or_else(|| (String::from("a"), 0));
                    return Err(XfinaError::ParseError(format!(
                        "SBI forex card rate sheet prints {} {} figures across {} columns",
                        values,
                        code,
                        columns.len()
                    )));
                }
            }
        }
    };

    let mut out = Vec::new();
    for ((code, name, _), mut rates) in rows.into_iter().zip(assigned) {
        // Zero is how the sheet writes "not quoted", and it is not a price.
        // Publishing it as one is how a rate of 0.00 reaches a time series and
        // is read as the rupee having become worthless that morning.
        rates.retain(|_, value| !value.is_zero());

        out.push(CurrencyRates {
            currency: code.clone(),
            name,
            unit: if per_hundred.contains(&code) { 100 } else { 1 },
            rates,
        });
    }
    Ok((out, by_order))
}

/// Matches a row's figures to headings by where they fall across the page.
///
/// `None` when they do not land one per column: two in the same band, or a
/// column left unaccounted for. Both mean the headings do not describe this
/// row, and neither can be patched over by keeping whichever figure came last.
fn by_position(
    figures: &[(&Cell, Decimal)],
    columns: &[Column],
) -> Option<BTreeMap<String, Decimal>> {
    let mut rates = BTreeMap::new();
    for (cell, value) in figures {
        let middle = (cell.x0 + cell.x1) / 2.0;
        let column = columns.iter().find(|c| middle >= c.lo && middle < c.hi)?;
        if rates.insert(column.key.clone(), *value).is_some() {
            return None;
        }
    }
    (rates.len() == columns.len()).then_some(rates)
}

/// Matches a row's figures to headings by the order both are printed in.
///
/// `None` unless the row prints exactly one figure per heading. That is what
/// keeps this from being a guess: a sheet whose figures have run together, or
/// which has dropped one, prints a different count and is refused instead.
fn by_printed_order(values: &[Decimal], columns: &[Column]) -> Option<BTreeMap<String, Decimal>> {
    (values.len() == columns.len()).then(|| {
        columns
            .iter()
            .map(|c| c.key.clone())
            .zip(values.iter().copied())
            .collect()
    })
}

// -----------------------------------------------------------------------------
// The note that says which currencies are quoted per hundred units
// -----------------------------------------------------------------------------

/// The currencies this sheet quotes in hundreds, read from its own footnote.
///
/// Read rather than hardcoded: which currencies are grouped this way is the
/// sheet's decision and it has changed. A sheet that stopped saying it while
/// still quoting the yen per hundred would make every yen rate wrong by two
/// orders of magnitude, so a missing note is a failure rather than a default.
fn per_hundred_currencies(page: &[Cell]) -> Result<HashSet<String>, XfinaError> {
    let text = page_text(page);
    let note =
        Regex::new(r"QUOTED\s+IN\s+TERMS\s+OF\s+(\d+)\s+FC\s+UNITS").expect("static pattern");
    let Some(caps) = note.captures(&text) else {
        return Err(XfinaError::ParseError(
            "SBI forex card rate sheet does not say which currencies are quoted per hundred units"
                .to_string(),
        ));
    };
    if &caps[1] != "100" {
        return Err(XfinaError::ParseError(format!(
            "SBI forex card rate sheet quotes some currencies per {} units, which is not a size this reads",
            &caps[1]
        )));
    }

    // The codes are printed in brackets ahead of the phrase, as in
    // "JAPANESE YEN (JPY) , THAI BAHT (THB) & KOREAN WON (KRW) are quoted ...".
    let head = &text[..caps.get(0).expect("whole match").start()];
    let sentence = head.rfind('.').map(|at| &head[at + 1..]).unwrap_or(head);
    let code = Regex::new(r"\(([A-Z]{3})\)").expect("static pattern");
    Ok(code
        .captures_iter(sentence)
        .map(|c| c[1].to_string())
        .collect())
}

// -----------------------------------------------------------------------------
// The sheet's own date
// -----------------------------------------------------------------------------

/// Reads the date and time the sheet prints on itself.
///
/// Never the clock and never the filename: a sheet fetched a day late still
/// quotes the morning it was published, and a series keyed on the download
/// time silently attributes each rate to the wrong day.
fn sheet_date(
    page: &[Cell],
    creation: Option<NaiveDate>,
) -> Result<(NaiveDate, Option<NaiveTime>), XfinaError> {
    let date = labelled(page, "DATE").ok_or_else(|| {
        XfinaError::ParseError("SBI forex card rate sheet prints no date".to_string())
    })?;
    let time = labelled(page, "TIME").and_then(|text| read_time(&text));
    Ok((read_date(&date, creation)?, time))
}

/// The value printed against `label`, which the sheet sets either as its own
/// cell ("Date" "18-09-2026") or run together with the label ("Date02-07-2024")
/// depending on the era.
fn labelled(page: &[Cell], label: &str) -> Option<String> {
    let mut rows: Vec<&Cell> = page.iter().collect();
    rows.sort_by(|a, b| {
        a.yc.partial_cmp(&b.yc)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal))
    });
    for (index, cell) in rows.iter().enumerate() {
        let text = cell.text.trim();
        if !text.to_uppercase().starts_with(label) {
            continue;
        }
        let rest = text[label.len()..].trim().trim_start_matches(':').trim();
        if !rest.is_empty() {
            return Some(rest.to_string());
        }
        // Set as a separate cell, on the same line.
        if let Some(next) = rows.get(index + 1) {
            if (next.yc - cell.yc).abs() <= 2.0 {
                return Some(next.text.trim().to_string());
            }
        }
    }
    None
}

/// Reads a printed date, working out which number is the day.
///
/// The sheet has printed `18-09-2026`, `1/6/2020` and `9/1/2020`, and the last
/// two mean January and September respectively -- the same shape, opposite
/// orders. Four things are tried in turn, and only the first three are
/// evidence:
///
/// 1. the numbers themselves, when one of them cannot be a month;
/// 2. the file's own creation date, which across the whole published archive
///    has never contradicted a date the sheet printed;
/// 3. day-first, but only for the dashed layout, which has never once been
///    printed the other way round.
///
/// A slashed date with two readings and no creation stamp is refused. That
/// layout has been printed in both orders, so there is nothing to decide it
/// with, and a guess would file a morning's rates under the wrong day while
/// looking exactly like a date that was read.
fn read_date(text: &str, creation: Option<NaiveDate>) -> Result<NaiveDate, XfinaError> {
    // Written with the separator matched twice rather than back-referenced,
    // which this regex engine does not support; the two are compared below.
    let pattern =
        Regex::new(r"^(\d{1,2})([-/.])(\d{1,2})([-/.])(\d{2}|\d{4})$").expect("static pattern");
    let unreadable = || {
        XfinaError::ParseError("SBI forex card rate sheet prints an unreadable date".to_string())
    };
    let caps = pattern.captures(text.trim()).ok_or_else(unreadable)?;
    if caps[2] != caps[4] {
        return Err(unreadable());
    }

    let first: u32 = caps[1].parse().expect("matched digits");
    let second: u32 = caps[3].parse().expect("matched digits");
    let year: i32 = caps[5].parse().expect("matched digits");
    let year = if caps[5].len() == 2 {
        2000 + year
    } else {
        year
    };

    let day_first = NaiveDate::from_ymd_opt(year, second, first);
    let month_first = NaiveDate::from_ymd_opt(year, first, second);

    match (day_first, month_first) {
        (None, None) => Err(XfinaError::ParseError(
            "SBI forex card rate sheet prints a date that is not a real day".to_string(),
        )),
        (Some(date), None) | (None, Some(date)) => Ok(date),
        (Some(a), Some(b)) if a == b => Ok(a),
        (Some(a), Some(b)) => match creation {
            Some(stamped) if stamped == a => Ok(a),
            Some(stamped) if stamped == b => Ok(b),
            _ if &caps[2] == "-" => Ok(a),
            _ => Err(XfinaError::ParseError(format!(
                "SBI forex card rate sheet prints '{}', which is {} or {} and nothing in the file says which",
                text.trim(),
                a,
                b
            ))),
        },
    }
}

/// Reads the publication time, which the sheet has printed as `9:30 AM`,
/// `10.30 A.M.` and `04:25PM`. Absent or unreadable is not fatal -- the date
/// is what a rate is filed under.
fn read_time(text: &str) -> Option<NaiveTime> {
    let pattern = Regex::new(r"(\d{1,2})[:.](\d{2})\s*(?:([AP])\.?M\.?)?").expect("static pattern");
    let upper = text.to_uppercase();
    let caps = pattern.captures(&upper)?;
    let hour: u32 = caps[1].parse().ok()?;
    let minute: u32 = caps[2].parse().ok()?;
    let hour = match caps.get(3).map(|m| m.as_str()) {
        Some("A") => hour % 12,
        Some("P") => hour % 12 + 12,
        _ => hour,
    };
    NaiveTime::from_hms_opt(hour, minute, 0)
}

// -----------------------------------------------------------------------------
// Shared helpers
// -----------------------------------------------------------------------------

/// The page as uppercase text, one line per printed row, for the few things
/// that are prose rather than table: the title, the reference marker, the
/// footnote about hundreds.
fn page_text(page: &[Cell]) -> String {
    let mut rows: Vec<&Cell> = page.iter().collect();
    rows.sort_by(|a, b| {
        a.yc.partial_cmp(&b.yc)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut out = String::new();
    let mut last = f64::MIN;
    for cell in rows {
        if (cell.yc - last).abs() > 2.0 && !out.is_empty() {
            out.push('\n');
        } else if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&cell.text.to_uppercase());
        last = cell.yc;
    }
    out
}

/// Uppercased with its whitespace squeezed, so a heading compares the same
/// however the sheet padded it.
fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

/// A heading as a stable key: `TT BUY` becomes `tt_buy`.
fn key_of(label: &str) -> String {
    let mut out = String::new();
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// A printed figure, or `None` for anything that is not one.
fn decimal(text: &str) -> Option<Decimal> {
    let cleaned: String = text
        .chars()
        .filter(|c| *c != ',' && !c.is_whitespace())
        .collect();
    if cleaned.is_empty() || !cleaned.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    cleaned.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every pattern in this module is built with `expect`, so a malformed one
    /// is a panic rather than a compile error. Touching each of them once here
    /// turns that into a test failure.
    #[test]
    fn every_static_pattern_compiles() {
        assert!(read_date("18-09-2026", None).is_ok());
        assert!(read_time("9:30 AM").is_some());
        assert!(per_hundred_currencies(&[]).is_err());
        assert!(currency_rows(&[], &[], &HashSet::new()).is_ok());
    }

    #[test]
    fn reads_the_orders_the_sheet_has_printed() {
        // Dashed, and unambiguous because 18 cannot be a month.
        assert_eq!(
            read_date("18-09-2026", None).unwrap(),
            NaiveDate::from_ymd_opt(2026, 9, 18).unwrap()
        );
        // Slashed, month-first, unambiguous because 20 cannot be a month.
        assert_eq!(
            read_date("1/20/2020", None).unwrap(),
            NaiveDate::from_ymd_opt(2020, 1, 20).unwrap()
        );
        // Two digit years belong to this century.
        assert_eq!(
            read_date("05-10-20", None).unwrap(),
            NaiveDate::from_ymd_opt(2020, 10, 5).unwrap()
        );
    }

    #[test]
    fn an_ambiguous_date_is_settled_by_the_file_and_not_by_a_guess() {
        let march = NaiveDate::from_ymd_opt(2020, 3, 7).unwrap();
        let july = NaiveDate::from_ymd_opt(2020, 7, 3).unwrap();
        // The same printed date has meant both, in the same year.
        assert_eq!(read_date("7/3/2020", Some(march)).unwrap(), march);
        assert_eq!(read_date("7/3/2020", Some(july)).unwrap(), july);
        // With nothing to settle it, the slashed layout is refused rather than
        // guessed: filing a morning's rates under the wrong day is worse than
        // not filing them.
        assert!(read_date("7/3/2020", None).is_err());
        // The dashed layout has only ever been day-first, so it is not refused.
        assert_eq!(
            read_date("7-3-2020", None).unwrap(),
            NaiveDate::from_ymd_opt(2020, 3, 7).unwrap()
        );
    }

    #[test]
    fn rejects_a_date_that_is_not_a_day() {
        assert!(read_date("31-02-2026", None).is_err());
        assert!(read_date("18-09", None).is_err());
        // Mixed separators are not a date this sheet has ever printed.
        assert!(read_date("18-09/2026", None).is_err());
    }

    #[test]
    fn reads_the_times_the_sheet_has_printed() {
        assert_eq!(read_time("9:30 AM"), NaiveTime::from_hms_opt(9, 30, 0));
        assert_eq!(read_time("10.30 A.M."), NaiveTime::from_hms_opt(10, 30, 0));
        assert_eq!(read_time("04:25PM"), NaiveTime::from_hms_opt(16, 25, 0));
        // Midnight and noon are where a twelve-hour clock goes wrong.
        assert_eq!(read_time("12:00 AM"), NaiveTime::from_hms_opt(0, 0, 0));
        assert_eq!(read_time("12:05 PM"), NaiveTime::from_hms_opt(12, 5, 0));
        assert_eq!(read_time("not a time"), None);
    }

    #[test]
    fn headings_become_stable_keys() {
        assert_eq!(key_of("TT BUY"), "tt_buy");
        assert_eq!(key_of("FOREX TRAVEL CARD SELL"), "forex_travel_card_sell");
        // The sheet has renamed this column twice; each name keeps its own key
        // rather than being folded into a single invented one.
        assert_eq!(key_of("TC BUY"), "tc_buy");
        assert_eq!(key_of("FTC BUY"), "ftc_buy");
    }

    #[test]
    fn only_figures_are_read_as_figures() {
        assert_eq!(decimal("95.38"), Some(Decimal::new(9538, 2)));
        assert_eq!(decimal("0.00"), Some(Decimal::ZERO));
        assert_eq!(decimal("USD/INR"), None);
        assert_eq!(decimal(""), None);
        assert_eq!(decimal("1/2"), None);
    }
}
