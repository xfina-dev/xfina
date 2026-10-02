#![cfg(all(
    feature = "md-amfi-nav",
    feature = "md-nse-security",
    feature = "md-nse-indices",
    feature = "md-mcx-spot",
    feature = "md-ishares",
    feature = "md-tiingo",
    feature = "md-wsj",
    feature = "md-msci",
    feature = "md-nasdaq",
    feature = "md-yahoo",
    feature = "md-spdr-gold",
))]

//! Every market data format, against files built in this test.
//!
//! The publishers' own downloads carry their own terms, so none is in this
//! repository. Each fixture below copies only a layout -- headings, date
//! style, the rows a publisher pads its file with -- around instruments and
//! prices that are invented. Spreadsheet layouts are written in memory, so no
//! binary file is committed either.
//!
//! Every fixture is detected without its filename first: the content alone
//! has to name the publisher. The filename is then only asked to supply what
//! the content leaves out, such as a ticker.

use std::str::FromStr;

use chrono::{NaiveDate, NaiveTime};
use rust_decimal::Decimal;
use rust_xlsxwriter::{ExcelDateTime, Format as XlsxFormat, Workbook};

use xfina::error::XfinaError;
use xfina::models::series::{Field, Frequency, InstrumentKind, PriceSeries, RangeSource};
use xfina::models::validation::ValidationStatus;
use xfina::models::{ParseRequest, Schema};
use xfina::{Area, Format};

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn day(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

/// Detects without a filename, then parses with one, and checks the two agree.
fn read(bytes: &[u8], filename: Option<&str>, want: Format) -> (PriceSeries, ValidationStatus) {
    let detected = xfina::detect(&ParseRequest::new(bytes))
        .unwrap_or_else(|e| panic!("{want} fixture not recognised without a filename: {e}"));
    assert_eq!(
        detected.format, want,
        "content alone must name the publisher"
    );

    let statement = xfina::parse(ParseRequest::new(bytes).with_filename(filename))
        .unwrap_or_else(|e| panic!("{want} fixture must parse: {e}"));
    assert_eq!(statement.format, want);
    assert_eq!(statement.area(), Area::Public);
    let series = statement
        .data
        .series()
        .expect("market data parses to a series")
        .clone();

    // Rows always come out in date order, whatever order the file used.
    let keys: Vec<_> = series.rows.iter().map(|r| (r.date, r.time)).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "{want} rows must be ascending");

    (series, statement.validation.overall)
}

fn values(series: &PriceSeries, field: Field) -> Vec<Option<Decimal>> {
    series.rows.iter().map(|r| r.get(field)).collect()
}

enum Cell<'a> {
    S(&'a str),
    N(f64),
    D(u16, u8, u8),
    E,
}

/// One workbook of named sheets, written in memory.
fn workbook(sheets: &[(&str, Vec<Vec<Cell>>)]) -> Vec<u8> {
    let mut book = Workbook::new();
    let date_format = XlsxFormat::new().set_num_format("dd-mmm-yyyy");
    for (name, rows) in sheets {
        let ws = book.add_worksheet();
        ws.set_name(*name).unwrap();
        for (r, row) in rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                let (r, c) = (r as u32, c as u16);
                match cell {
                    Cell::S(s) => {
                        ws.write_string(r, c, *s).unwrap();
                    }
                    Cell::N(n) => {
                        ws.write_number(r, c, *n).unwrap();
                    }
                    Cell::D(y, m, d) => {
                        let dt = ExcelDateTime::from_ymd(*y, *m, *d).unwrap();
                        ws.write_datetime_with_format(r, c, &dt, &date_format)
                            .unwrap();
                    }
                    Cell::E => {}
                }
            }
        }
    }
    book.save_to_buffer().unwrap()
}

// -----------------------------------------------------------------------------
// AMFI
// -----------------------------------------------------------------------------

fn amfi_workbook(rows: Vec<Vec<Cell<'static>>>) -> Vec<u8> {
    use Cell::*;
    let mut sheet = vec![
        vec![S("Historical NAV Data for From 01-Sep-2026 to 04-Sep-2026")],
        vec![S("Example Mutual Fund")],
        vec![S("Example Index Fund")],
        vec![S("Example Index Fund - Direct Plan - Growth Option")],
        vec![
            S("Net Asset Value"),
            S("Repurchase Price"),
            S("Sale Price"),
            S("Date"),
            S("Plan"),
            S("Option"),
        ],
    ];
    sheet.extend(rows);
    workbook(&[("Historical NAV Data", sheet)])
}

#[test]
fn amfi_reads_the_nav_and_the_period_it_prints() {
    use Cell::*;
    let bytes = amfi_workbook(vec![
        vec![
            N(10.5),
            E,
            E,
            D(2026, 9, 1),
            S("Direct Plan"),
            S("Growth Option"),
        ],
        vec![
            N(10.75),
            E,
            E,
            D(2026, 9, 2),
            S("Direct Plan"),
            S("Growth Option"),
        ],
        vec![
            N(10.6),
            N(10.5),
            N(10.7),
            D(2026, 9, 3),
            S("Direct Plan"),
            S("Growth Option"),
        ],
    ]);
    let (s, status) = read(&bytes, None, Format::MarketAmfiNav);
    assert_eq!(status, ValidationStatus::Passed);
    assert_eq!(s.headline, Field::Nav);
    assert_eq!(s.kind, Some(InstrumentKind::MutualFund));
    assert_eq!(s.currency.as_deref(), Some("INR"));
    assert_eq!(
        s.code.as_deref(),
        Some("Example Index Fund - Direct Plan - Growth Option")
    );
    assert_eq!(
        values(&s, Field::Nav),
        vec![Some(dec("10.5")), Some(dec("10.75")), Some(dec("10.6"))]
    );
    assert_eq!(s.extra_fields, vec!["Repurchase Price", "Sale Price"]);
    let range = s.declared_range.unwrap();
    assert_eq!(
        (range.from, range.to),
        (day("2026-09-01"), day("2026-09-04"))
    );
    assert_eq!(range.source, RangeSource::Content);
}

#[test]
fn amfi_fails_a_row_outside_the_period_it_declares() {
    use Cell::*;
    let bytes = amfi_workbook(vec![
        vec![
            N(10.5),
            E,
            E,
            D(2026, 9, 1),
            S("Direct Plan"),
            S("Growth Option"),
        ],
        vec![
            N(10.9),
            E,
            E,
            D(2026, 9, 9),
            S("Direct Plan"),
            S("Growth Option"),
        ],
    ]);
    let (_, status) = read(&bytes, None, Format::MarketAmfiNav);
    // The range is printed in the file, so a row outside it is a declared
    // figure the rows contradict.
    assert_eq!(status, ValidationStatus::Failed);
}

// -----------------------------------------------------------------------------
// NSE security-wise
// -----------------------------------------------------------------------------

const NSE_ARCHIVE: &str = concat!(
    "\"Symbol  \",\"Series  \",\"Date  \",\"Prev Close  \",\"Open Price  \",\"High Price  \",",
    "\"Low Price  \",\"Last Price  \",\"Close Price  \",\"Average Price \",\"Total Traded Quantity  \",",
    "\"Turnover \u{20b9}  \",\"No. of Trades  \",\"Deliverable Qty  \",\"% Dly Qt to Traded Qty  \"\n",
    "\"EXMPL\",\"EQ\",\"02-Sep-2026\",\"100.00\",\"100.50\",\"102.00\",\"99.50\",\"101.00\",\"101.20\",",
    "\"100.90\",\"12,34,567\",\"12,45,67,890.12\",\"1,234\",\"6,17,283\",\"50.00\"\n",
    "\"EXMPL\",\"EQ\",\"01-Sep-2026\",\"99.00\",\"99.50\",\"100.50\",\"98.50\",\"100.00\",\"100.00\",",
    "\"99.80\",\"10,00,000\",\"9,98,00,000.00\",\"1,000\",\"-\",\"-\"\n",
);

#[test]
fn nse_archive_reads_prices_and_keeps_the_rest_under_nse_headings() {
    let (s, status) = read(
        NSE_ARCHIVE.as_bytes(),
        Some("01-09-2026-TO-02-09-2026-EXMPL-EQ-N.csv"),
        Format::MarketNseSecurity,
    );
    assert_eq!(status, ValidationStatus::Passed);
    assert_eq!(s.code.as_deref(), Some("EXMPL"));
    assert_eq!(s.kind, Some(InstrumentKind::ListedSecurity));
    assert_eq!(
        values(&s, Field::Close),
        vec![Some(dec("100.00")), Some(dec("101.20"))]
    );
    // Indian digit grouping.
    assert_eq!(s.rows[1].volume, Some(dec("1234567")));
    assert_eq!(s.rows[1].extra["Average Price"], dec("100.90"));
    assert_eq!(s.rows[1].extra["Deliverable Qty"], dec("617283"));
    assert!(
        !s.rows[0].extra.contains_key("Deliverable Qty"),
        "a dash is no value"
    );
    assert_eq!(s.declared_range.unwrap().source, RangeSource::Filename);
}

#[test]
fn nse_quote_download_takes_the_symbol_from_its_name() {
    let csv = concat!(
        "\u{feff}DATE,SERIES,OPEN,HIGH,LOW,PREV. CLOSE,LTP,CLOSE,VWAP,52 WEEK HIGH,52 WEEK LOW,VOLUME,VALUE,NO. OF  TRADES\n",
        "02-Sep-2026,EQ,\"100.50\",\"102.00\",\"99.50\",\"100.00\",\"101.00\",\"101.20\",\"100.90\",\"110.00\",\"90.00\",\"12,34,567\",\"12,45,67,890.12\",\"1,234\"\n",
    );
    let (s, _) = read(
        csv.as_bytes(),
        Some("Quote-Equity-EXMPL-EQ-01-09-2026-02-09-2026.csv"),
        Format::MarketNseSecurity,
    );
    assert_eq!(s.code.as_deref(), Some("EXMPL"));
    assert_eq!(s.rows[0].close, Some(dec("101.20")));
    assert_eq!(s.rows[0].extra["VWAP"], dec("100.90"));
    assert_eq!(s.rows[0].extra["NO. OF TRADES"], dec("1234"));
}

// -----------------------------------------------------------------------------
// NSE Indices
// -----------------------------------------------------------------------------

#[test]
fn nse_indices_total_return_fills_both_levels_on_one_row() {
    let csv = concat!(
        "\"IndexName\",\"Date\",\"Total Returns Index\",\"Net Total Return Index\"\r\n",
        "\"EXAMPLE 50\",\"02 Sep 2026\",\"2010.50\",\"1810.25\"\r\n",
        "\"EXAMPLE 50\",\"01 Sep 2026\",\"2000.00\",\"1800.00\"\r\n",
    );
    let (s, status) = read(
        csv.as_bytes(),
        Some("EXAMPLE 50_Historical_TR_01092026to02092026.csv"),
        Format::MarketNseIndices,
    );
    assert_eq!(status, ValidationStatus::Passed);
    assert_eq!(s.code.as_deref(), Some("EXAMPLE 50"));
    assert_eq!(s.headline, Field::TotalReturn);
    assert_eq!(s.fields, vec![Field::TotalReturn, Field::NetTotalReturn]);
    assert_eq!(s.rows[0].total_return, Some(dec("2000.00")));
    assert_eq!(s.rows[0].net_total_return, Some(dec("1800.00")));
    let range = s.declared_range.unwrap();
    assert_eq!(
        (range.from, range.to),
        (day("2026-09-01"), day("2026-09-02"))
    );
}

#[test]
fn nse_indices_reads_an_index_with_no_net_series() {
    let csv = "\"IndexName\",\"Date\",\"Total Returns Index\"\r\n\"EXAMPLE NEXT\",\"01 Sep 2026\",\"5000.00\"\r\n";
    let (s, _) = read(csv.as_bytes(), None, Format::MarketNseIndices);
    assert_eq!(s.fields, vec![Field::TotalReturn]);
}

#[test]
fn nse_indices_price_report_keeps_a_debt_index_close_without_inventing_the_rest() {
    let csv = concat!(
        "\"Index Name\",\"Date\",\"Open\",\"High\",\"Low\",\"Close\"\n",
        "\"EXAMPLE RATE INDEX\",\"02 Sep 2026\",\"-\",\"-\",\"-\",\"1000.20\"\n",
        "\"EXAMPLE RATE INDEX\",\"01 Sep 2026\",\"-\",\"-\",\"-\",\"1000.10\"\n",
    );
    let (s, _) = read(csv.as_bytes(), None, Format::MarketNseIndices);
    assert_eq!(s.headline, Field::Close);
    assert_eq!(s.fields, vec![Field::Close]);
}

// -----------------------------------------------------------------------------
// MCX
// -----------------------------------------------------------------------------

fn mcx_table(rows: &[(&str, &str, &str)]) -> String {
    let mut html = String::from(
        "\n   <table border=\"1\"><thead><tr><th>Commodity</th><th>Unit</th><th>Location</th>\
         <th>Spot Price(Rs.)</th><th>Up/Down</th><th>Date</th><th>Time</th></tr></thead><tbody>",
    );
    for (price, date, time) in rows {
        html.push_str(&format!(
            "<tr><td style=\"padding: 5px;\">GOLD</td><td>10 GRMS</td><td>EXAMPLETOWN</td>\
             <td style=\"mso-number-format:'0.00';\">{price}</td><td>+ </td><td>{date}</td><td>{time}</td></tr>"
        ));
    }
    html.push_str("</tbody></table>");
    html
}

#[test]
fn mcx_keeps_every_poll_with_its_time() {
    let html = mcx_table(&[
        ("50100.00", "02 Sep 2026", "17:30"),
        ("50000.00", "02 Sep 2026", "13:00"),
        ("50000.00", "02 Sep 2026", "13:00"), // the same reading listed twice
        ("49900.00", "01 Sep 2026", "17:30"),
    ]);
    let (s, status) = read(html.as_bytes(), None, Format::MarketMcxSpot);
    // An identical repeat is kept once and does not fail the file.
    assert_eq!(status, ValidationStatus::Passed);
    assert_eq!(s.frequency, Frequency::Intraday);
    assert_eq!(s.code.as_deref(), Some("GOLD EXAMPLETOWN"));
    assert_eq!(s.unit.as_deref(), Some("10 GRMS"));
    assert_eq!(s.rows.len(), 3);
    assert_eq!(s.rows[1].time, NaiveTime::from_hms_opt(13, 0, 0));
    assert_eq!(s.rows[2].close, Some(dec("50100.00")));
}

#[test]
fn mcx_warns_on_two_different_prices_for_one_poll() {
    let html = mcx_table(&[
        ("50100.00", "02 Sep 2026", "13:00"),
        ("50200.00", "02 Sep 2026", "13:00"),
    ]);
    let (s, status) = read(html.as_bytes(), None, Format::MarketMcxSpot);
    assert_eq!(status, ValidationStatus::Warning);
    assert_eq!(s.rows.len(), 1, "the first printed is kept");
}

// -----------------------------------------------------------------------------
// iShares
// -----------------------------------------------------------------------------

fn xml_sheet(name: &str, rows: &[&[&str]]) -> String {
    let mut out = format!("<ss:Worksheet ss:Name=\"{name}\">\n<ss:Table>\n");
    for row in rows {
        out.push_str("<ss:Row>");
        for cell in *row {
            out.push_str(&format!(
                "<ss:Cell><ss:Data ss:Type=\"String\">{cell}</ss:Data></ss:Cell>"
            ));
        }
        out.push_str("</ss:Row>\n");
    }
    out.push_str("</ss:Table>\n</ss:Worksheet>\n");
    out
}

fn xml_book(sheets: &[String]) -> String {
    let mut out = String::from(
        "\u{feff}<?xml version=\"1.0\"?>\n<ss:Workbook xmlns:ss=\"urn:schemas-microsoft-com:office:spreadsheet\">\n",
    );
    // A link written the way iShares writes them: a bare '&' in the query.
    out.push_str("<ss:Styles><ss:Style ss:ID=\"Default\" ss:HRef=\"https://example.com/?a=1&b=2\"/></ss:Styles>\n");
    for s in sheets {
        out.push_str(s);
    }
    out.push_str("</ss:Workbook>\n");
    out
}

#[test]
fn ishares_us_reads_the_dividend_on_its_ex_date_and_checks_it() {
    let book = xml_book(&[
        xml_sheet(
            "Holdings",
            &[&["09/02/2026"], &["iShares Example Equity ETF"]],
        ),
        xml_sheet(
            "Historical",
            &[
                &[
                    "As Of",
                    "NAV per Share",
                    "Ex-Dividends",
                    "Shares Outstanding",
                ],
                &["Sep 02, 2026", "50.25", "0.40", "1000000"],
                &["Sep 01, 2026", "50.00", "--", "1000000"],
            ],
        ),
        xml_sheet(
            "Distributions",
            &[
                &[
                    "Record Date",
                    "Ex-Date",
                    "Payable Date",
                    "Total Distribution",
                    "Income",
                ],
                &[
                    "Sep 02, 2026",
                    "Sep 02, 2026",
                    "Sep 05, 2026",
                    "0.40",
                    "0.40",
                ],
            ],
        ),
    ]);
    let (s, status) = read(book.as_bytes(), None, Format::MarketIshares);
    assert_eq!(status, ValidationStatus::Passed);
    assert_eq!(s.name.as_deref(), Some("iShares Example Equity ETF"));
    assert_eq!(s.currency.as_deref(), Some("USD"));
    assert_eq!(values(&s, Field::Dividend), vec![None, Some(dec("0.40"))]);
    assert_eq!(s.rows[0].extra["Shares Outstanding"], dec("1000000"));
}

#[test]
fn ishares_fails_when_a_listed_distribution_is_missing_from_the_history() {
    let book = xml_book(&[
        xml_sheet("Holdings", &[&["iShares Example Equity ETF"]]),
        xml_sheet(
            "Historical",
            &[
                &["As Of", "NAV per Share", "Ex-Dividends"],
                &["Sep 01, 2026", "50.00", "--"],
            ],
        ),
        xml_sheet(
            "Distributions",
            &[
                &[
                    "Record Date",
                    "Ex-Date",
                    "Payable Date",
                    "Total Distribution",
                ],
                &["Sep 01, 2026", "Sep 01, 2026", "Sep 05, 2026", "0.40"],
            ],
        ),
    ]);
    let (_, status) = read(book.as_bytes(), None, Format::MarketIshares);
    assert_eq!(status, ValidationStatus::Failed);
}

#[test]
fn ishares_ucits_reads_the_nav_only_sheet_and_its_isin() {
    let book = xml_book(&[
        xml_sheet("Fund Header", &[&["iShares Example UCITS ETF"]]),
        xml_sheet(
            "Key Facts",
            &[&["ISIN", "XX0000000000"], &["Share Class Currency", "USD"]],
        ),
        xml_sheet(
            "Historical NAVs",
            &[
                &["", "Historical NAVs"],
                &["02/Sept/2026", "1,001.50"],
                &["01/Sept/2026", "1,000.00"],
                &["Past performance is not a reliable indicator of future results."],
            ],
        ),
    ]);
    let (s, _) = read(book.as_bytes(), None, Format::MarketIshares);
    assert_eq!(s.code.as_deref(), Some("XX0000000000"));
    assert_eq!(s.currency.as_deref(), Some("USD"));
    assert_eq!(
        values(&s, Field::Nav),
        vec![Some(dec("1000.00")), Some(dec("1001.50"))]
    );
}

// -----------------------------------------------------------------------------
// Tiingo
// -----------------------------------------------------------------------------

const TIINGO_HEADER: &str =
    "date,close,high,low,open,volume,adjClose,adjHigh,adjLow,adjOpen,adjVolume,divCash,splitFactor\n";

#[test]
fn tiingo_traded_line_keeps_prices_adjusted_twins_and_events() {
    let csv = format!(
        "{TIINGO_HEADER}\
         2026-09-01,100.0,101.0,99.0,99.5,1000,50.0,50.5,49.5,49.75,2000,0.0,1.0\n\
         2026-09-02,51.0,51.5,50.5,50.75,2400,51.0,51.5,50.5,50.75,2400,0.25,2.0\n"
    );
    let (s, status) = read(
        csv.as_bytes(),
        Some("EXMPL_2026-09-01_to_2026-09-02.tiingo.csv"),
        Format::MarketTiingo,
    );
    assert_eq!(status, ValidationStatus::Passed);
    assert_eq!(s.code.as_deref(), Some("EXMPL"));
    assert_eq!(s.headline, Field::Close);
    assert_eq!(s.kind, None, "a Tiingo file does not say what it prices");
    assert_eq!(values(&s, Field::Dividend), vec![None, Some(dec("0.25"))]);
    assert_eq!(values(&s, Field::SplitFactor), vec![None, Some(dec("2.0"))]);
    assert_eq!(
        s.split_adjusted,
        Some(false),
        "Tiingo prints prices as traded"
    );
    assert_eq!(s.rows[0].adj_volume, Some(dec("2000")));
    // The CSV comes back in Tiingo's own column order.
    assert!(s.to_csv().starts_with(
        "date,close,high,low,open,volume,adjClose,adjHigh,adjLow,adjOpen,adjVolume,divCash,splitFactor\n"
    ));
}

#[test]
fn tiingo_fund_reads_close_as_its_nav() {
    let csv = format!(
        "{TIINGO_HEADER}\
         2026-09-01,20.0,20.0,20.0,20.0,0,10.0,10.0,10.0,10.0,0,0.0,1.0\n\
         2026-09-02,20.2,20.2,20.2,20.2,0,10.1,10.1,10.1,10.1,0,0.0,1.0\n"
    );
    let (s, _) = read(csv.as_bytes(), Some("EXFND.csv"), Format::MarketTiingo);
    assert_eq!(s.kind, Some(InstrumentKind::MutualFund));
    assert_eq!(s.headline, Field::Nav);
    assert_eq!(s.fields, vec![Field::Nav, Field::AdjNav]);
    assert_eq!(s.rows[1].adj_nav, Some(dec("10.1")));
}

#[test]
fn tiingo_header_alone_is_still_tiingo_and_fails_for_having_no_rows() {
    let (s, status) = read(TIINGO_HEADER.as_bytes(), None, Format::MarketTiingo);
    assert!(s.rows.is_empty());
    assert_eq!(status, ValidationStatus::Failed);
}

// -----------------------------------------------------------------------------
// WSJ
// -----------------------------------------------------------------------------

#[test]
fn wsj_reads_two_digit_years_newest_first() {
    let csv = "Date, Open, High, Low, Close, Volume\n09/02/26, 10.10, 10.30, 10.00, 10.20, 1200\n09/01/26, 10.00, 10.20, 9.90, 10.10, 1000\n";
    let (s, status) = read(csv.as_bytes(), Some("EXMPL.csv"), Format::MarketWsj);
    assert_eq!(status, ValidationStatus::Passed);
    assert_eq!(s.code.as_deref(), Some("EXMPL"));
    assert_eq!(s.rows[0].date, day("2026-09-01"));
    assert_eq!(s.rows[1].close, Some(dec("10.20")));
    assert_eq!(s.split_adjusted, None, "WSJ says nothing about splits");
}

#[test]
fn wsj_default_filename_names_no_ticker() {
    let csv = "Date, Open, High, Low, Close, Volume\n09/01/26, 10.00, 10.20, 9.90, 10.10, 1000\n";
    let (s, _) = read(
        csv.as_bytes(),
        Some("HistoricalPrices.csv"),
        Format::MarketWsj,
    );
    assert_eq!(s.code, None);
}

// -----------------------------------------------------------------------------
// MSCI
// -----------------------------------------------------------------------------

fn msci_workbook(variant: &'static str) -> Vec<u8> {
    use Cell::*;
    workbook(&[(
        "Performance Data",
        vec![
            vec![
                S("Index Level:"),
                S(variant),
                E,
                E,
                S("This information is the property of its publisher."),
            ],
            vec![
                S("Currency:"),
                S("USD"),
                E,
                E,
                S("It is provided for informational purposes only."),
            ],
            vec![],
            vec![
                S("Date"),
                S("MSCI Example Index"),
                E,
                E,
                S("No warranty is given."),
            ],
            vec![S("2026-07-31"), N(100.0)],
            vec![S("2026-08-31"), N(102.5)],
            vec![S("2026-09-30"), N(101.25)],
        ],
    )])
}

#[test]
fn msci_net_level_is_a_net_total_return() {
    let bytes = msci_workbook("Net");
    let (s, status) = read(
        &bytes,
        Some("000000 - MSCI Example Index  - FULL - 2026-07-31 - 2026-09-30  - Monthly.xlsx"),
        Format::MarketMsci,
    );
    assert_eq!(status, ValidationStatus::Passed);
    assert_eq!(s.headline, Field::NetTotalReturn);
    assert_eq!(s.code.as_deref(), Some("000000"));
    assert_eq!(s.name.as_deref(), Some("MSCI Example Index"));
    assert_eq!(s.currency.as_deref(), Some("USD"));
    assert_eq!(s.frequency, Frequency::Monthly);
    assert_eq!(values(&s, Field::NetTotalReturn)[1], Some(dec("102.5")));
}

#[test]
fn msci_gross_level_is_a_total_return() {
    let (s, _) = read(&msci_workbook("Gross"), None, Format::MarketMsci);
    assert_eq!(s.headline, Field::TotalReturn);
    // Without the download's name there is no code, so the index's name stands in.
    assert_eq!(s.code.as_deref(), Some("MSCI Example Index"));
}

// -----------------------------------------------------------------------------
// Nasdaq
// -----------------------------------------------------------------------------

fn nasdaq_workbook() -> Vec<u8> {
    use Cell::*;
    workbook(&[
        (
            "History",
            vec![
                vec![
                    S("Trade Date"),
                    S("Index Value"),
                    S("Net Change"),
                    S("High"),
                    S("Low"),
                ],
                // Today, before the close: every figure zero.
                vec![D(2026, 9, 3), N(0.0), N(0.0), N(0.0), N(0.0)],
                vec![D(2026, 9, 2), N(1010.0), N(10.0), N(1012.0), N(1001.0)],
                vec![D(2026, 9, 1), N(1000.0), N(-5.0), N(0.0), N(0.0)],
            ],
        ),
        ("Chart", vec![]),
    ])
}

#[test]
fn nasdaq_drops_the_zero_row_and_reads_a_price_index() {
    let (s, status) = read(
        &nasdaq_workbook(),
        Some("EODHist_20260901-20260903_EXNDX.xlsx"),
        Format::MarketNasdaq,
    );
    assert_eq!(status, ValidationStatus::Passed);
    assert_eq!(s.code.as_deref(), Some("EXNDX"));
    assert_eq!(s.headline, Field::Close);
    assert_eq!(s.rows.len(), 2);
    assert_eq!(s.rows[0].high, None, "a high of 0 is not a price");
    assert_eq!(s.rows[0].extra["Net Change"], dec("-5"));
}

#[test]
fn nasdaq_total_return_symbol_reads_as_a_total_return() {
    let (s, _) = read(
        &nasdaq_workbook(),
        Some("EODHist_20260901-20260903_XNDX.xlsx"),
        Format::MarketNasdaq,
    );
    assert_eq!(s.headline, Field::TotalReturn);
    assert_eq!(s.fields, vec![Field::TotalReturn]);
    assert_eq!(s.rows[1].extra["High"], dec("1012"));
}

// -----------------------------------------------------------------------------
// Yahoo
// -----------------------------------------------------------------------------

#[test]
fn yahoo_yfinance_reads_events_and_names_a_price_index() {
    let csv = "Date,Open,High,Low,Close,Adj Close,Volume,Dividends,Stock Splits\n\
               2026-09-01,99.0,101.0,98.0,100.0,100.0,500,0.0,0.0\n\
               2026-09-02,100.0,102.0,99.0,101.0,101.0,600,0.0,0.0\n";
    let (s, _) = read(
        csv.as_bytes(),
        Some("GSPC_2026-09-01_to_2026-09-02.yfinance.csv"),
        Format::MarketYahoo,
    );
    assert_eq!(s.code.as_deref(), Some("GSPC"));
    assert_eq!(s.kind, Some(InstrumentKind::Index));
    assert_eq!(s.headline, Field::Close);
    assert!(
        !s.fields.contains(&Field::Dividend),
        "a zero dividend is no dividend"
    );
}

#[test]
fn yahoo_total_return_index_close_is_a_total_return_level() {
    let csv = "Date,Open,High,Low,Close,Adj Close,Volume,Dividends,Stock Splits\n\
               2026-09-01,0.0,0.0,0.0,2000.0,2000.0,0,0.0,0.0\n";
    let (s, _) = read(
        csv.as_bytes(),
        Some("SP500TR_2026-09-01_to_2026-09-01.yfinance.csv"),
        Format::MarketYahoo,
    );
    assert_eq!(s.headline, Field::TotalReturn);
    assert_eq!(s.rows[0].total_return, Some(dec("2000.0")));
    assert_eq!(s.rows[0].close, None);
}

#[test]
fn yahoo_classic_folds_event_lines_into_rows() {
    let csv = concat!(
        "Date,Open,High,Low,Close,Adj Close,Volume\n",
        "\"Sep 1, 2026\",10.00,11.00,9.50,10.50,10.40,\"1,200,000\"\n",
        "\"Aug 20, 2026\",0.25 Dividend     Dividends on any given ex-date include regular and any special dividends\n",
        "\"Aug 1, 2026\",9.00,10.20,8.90,10.00,9.80,\"1,000,000\"\n",
        "\"Jul 15, 2026\",2:1 Stock Splits\n",
        "\"Jul 1, 2026\",17.00,18.20,16.90,18.00,8.80,-\n",
    );
    let (s, status) = read(
        csv.as_bytes(),
        Some("EXMPL_Jul-1-2026_to_Sep-1-2026.csv"),
        Format::MarketYahoo,
    );
    assert_eq!(status, ValidationStatus::Passed);
    assert_eq!(s.code.as_deref(), Some("EXMPL"));
    assert_eq!(s.rows.len(), 5);
    let event = s.rows.iter().find(|r| r.date == day("2026-08-20")).unwrap();
    assert_eq!(event.dividend, Some(dec("0.25")));
    assert!(!event.has_price());
    let split = s.rows.iter().find(|r| r.date == day("2026-07-15")).unwrap();
    assert_eq!(split.split_factor, Some(dec("2")));
    assert_eq!(
        s.split_adjusted,
        Some(true),
        "Yahoo's closes already reflect the split"
    );
    // Event-only rows are not coverage.
    assert_eq!(s.coverage().rows, 3);
    assert_eq!(s.rows[0].volume, None, "a dash is no volume");
    assert_eq!(s.declared_range.unwrap().from, day("2026-07-01"));
}

// -----------------------------------------------------------------------------
// SPDR Gold
// -----------------------------------------------------------------------------

#[test]
fn spdr_skips_holiday_rows_and_reads_close_and_nav() {
    use Cell::*;
    let heading = vec![
        S("Date"),
        S("Closing Price"),
        S("Ounces of Gold per Share"),
        S("NAV/Share at 10:30am NYT"),
        S("Daily Share Volume"),
        S("Total Net Asset Value in the Trust"),
    ];
    let bytes = workbook(&[
        ("Disclaimer", vec![vec![S("EXAMPLE HISTORICAL DATA")]]),
        (
            "US GLD Historical Archive",
            vec![
                heading,
                vec![
                    S("01-Sep-2026"),
                    N(40.0),
                    N(0.1),
                    N(39.9),
                    N(1000.0),
                    N(4000000.0),
                ],
                vec![
                    S("02-Sep-2026"),
                    S("US Holiday"),
                    S("US Holiday"),
                    S("US Holiday"),
                    S("US Holiday"),
                    S("US Holiday"),
                ],
                vec![
                    S("03-Sep-2026"),
                    N(40.5),
                    N(0.1),
                    N(40.4),
                    N(1100.0),
                    N(4050000.0),
                ],
            ],
        ),
    ]);
    let (s, status) = read(&bytes, None, Format::MarketSpdrGold);
    assert_eq!(status, ValidationStatus::Passed);
    assert_eq!(s.rows.len(), 2);
    assert_eq!(
        values(&s, Field::Nav),
        vec![Some(dec("39.9")), Some(dec("40.4"))]
    );
    assert_eq!(s.rows[1].extra["Ounces of Gold per Share"], dec("0.1"));
}

// -----------------------------------------------------------------------------
// Across formats
// -----------------------------------------------------------------------------

#[test]
fn a_series_renders_in_xfina_and_refuses_rebit() {
    let csv = format!(
        "{TIINGO_HEADER}2026-09-01,100.0,101.0,99.0,99.5,1000,100.0,101.0,99.0,99.5,1000,0.0,1.0\n"
    );
    let statement = xfina::parse(ParseRequest::new(csv.as_bytes())).unwrap();
    let json = statement.to_json(Schema::Xfina).unwrap();
    assert_eq!(json["area"], "public");
    assert_eq!(json["category"], "market_data");
    assert_eq!(json["format"], "md-tiingo");
    assert_eq!(json["data"]["headline"], "close");
    assert!(matches!(
        statement.to_json(Schema::Rebit),
        Err(XfinaError::SchemaUnsupported { .. })
    ));
}

#[test]
fn look_alike_price_files_belong_to_nobody() {
    // Headings close to a supported one but published by somebody else, or by
    // a supported publisher in a layout it does not hand out for download.
    let cases = [
        // No spaces after the commas, which is what WSJ prints.
        "Date,Open,High,Low,Close,Volume\n2026-09-01,1,1,1,1,1\n",
        "Date,Close/Last,Open,High,Low\n09/01/2026,1,1,1,1\n",
        "\"Date\",\"Price\",\"Open\",\"High\",\"Low\",\"Vol.\",\"Change %\"\n\"09/01/2026\",\"1\",\"1\",\"1\",\"1\",\"\",\"0%\"\n",
        // A full-history pull assembled from NSE's API rather than its report.
        "Index Name,Date,TotalReturnsIndex,NTR_Value\nExample 50,01 Sep 2026,1,1\n",
        // Tiingo's answer for a ticker it does not carry.
        "Error: Ticker 'EXMPL' not found",
    ];
    for csv in cases {
        let outcome = xfina::detect(&ParseRequest::new(csv.as_bytes()));
        assert!(
            matches!(outcome, Err(XfinaError::UnrecognizedFormat { .. })),
            "claimed a file no parser owns: {:?}",
            outcome.map(|d| d.format)
        );
    }
}

#[test]
fn every_market_data_format_is_public() {
    for info in xfina::formats().iter().filter(|f| f.id.starts_with("md-")) {
        assert_eq!(info.area, Area::Public, "{}", info.id);
    }
}
