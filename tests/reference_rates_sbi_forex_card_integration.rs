#![cfg(feature = "rt-sbi-forex-card")]

//! Snapshot tests for the SBI forex card rate sheet.
//!
//! Fixtures live in `../xfina-test-data/` with every other parser's, and these
//! are skipped when it is not checked out. No input file belongs in this
//! repository whatever it holds, so a rate sheet is kept beside the statements
//! rather than made the one exception.
//!
//! The four fixtures are one per layout era, chosen for what each one breaks:
//!
//! - `2020-01-06` -- two tables on two pages, `TC BUY`/`TC SELL` columns, and
//!   a slashed date that is a real day either way round.
//! - `2023-06-23` -- each row's label set on a different baseline from its
//!   figures, a `FOREX TRAVEL CARD` heading spread over two printed lines,
//!   and a `PC BUY` column that later sheets drop.
//! - `2024-07-02` -- set in a font whose glyph widths this reader cannot
//!   measure, so its headings come out drawn across each other. Refused, and
//!   the negative test says so.
//! - `2026-02-07` -- one page, `FOREX TRAVEL CARD` columns, a footnote that
//!   wraps mid-phrase, and a currency quoted per hundred that earlier sheets
//!   did not list.
//! - `2021-05-25` -- the table collapsed into a flow, where no two rows begin
//!   at the same place and nothing lines up under a heading. Read by the order
//!   the figures are printed in, and says so.

use std::fs;
use std::path::{Path, PathBuf};

use xfina::models::request::ParseRequest;
use xfina::models::Schema;
use xfina::reference_rates::sbi_forex_card::parse_sbi_forex_card_rates;

fn fixtures() -> Option<PathBuf> {
    if std::env::var("GITHUB_ACTIONS").is_ok() {
        return None;
    }
    let dir = Path::new("../xfina-test-data/reference-rates/sbi-forex-card");
    dir.is_dir().then(|| dir.to_path_buf())
}

/// Runs `body` against the fixture directory, or reports the skip and stops.
fn with_fixtures(body: impl FnOnce(&Path)) {
    match fixtures() {
        Some(dir) => body(&dir),
        None => println!("../xfina-test-data not present; skipping"),
    }
}

fn read(dir: &Path, name: &str) -> Vec<u8> {
    fs::read(dir.join("raw").join(name))
        .unwrap_or_else(|e| panic!("fixture {name} is missing: {e}"))
}

#[test]
fn reads_every_layout_era() {
    with_fixtures(|dir| {
        let expected_dir = dir.join("expected/xfina");
        fs::create_dir_all(&expected_dir).unwrap();
        let update = std::env::var("UPDATE_EXPECTED").unwrap_or_else(|_| "0".to_string()) == "1";

        let mut checked = 0;
        for name in [
            "2020-01-06.pdf",
            "2021-05-25.pdf",
            "2023-06-23.pdf",
            "2026-02-07.pdf",
        ] {
            let bytes = read(dir, name);
            let result = parse_sbi_forex_card_rates(ParseRequest::new(&bytes))
                .unwrap_or_else(|e| panic!("{name} must parse: {e}"));

            let json =
                serde_json::to_string_pretty(&result.data.to_json(Schema::Xfina).unwrap()).unwrap();
            let path = expected_dir.join(name.replace(".pdf", ".json"));
            if update {
                fs::write(&path, &json).unwrap();
            } else {
                let want = fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("snapshot for {name} is missing: {e}"));
                assert_eq!(want, json, "rate sheet JSON mismatch for {name}");
            }
            checked += 1;
        }
        assert_eq!(checked, 4, "every era fixture must be checked");
    });
}

#[test]
fn takes_the_date_from_the_sheet_and_not_from_the_name() {
    with_fixtures(|dir| {
        // The 2020 sheet prints "1/6/2020", which is a real day read either way
        // round. Reading it as the sixth of January rather than the first of June
        // is the whole reason the parser consults the file's own creation stamp.
        let bytes = read(dir, "2020-01-06.pdf");
        let sheet = parse_sbi_forex_card_rates(ParseRequest::new(&bytes))
            .unwrap()
            .data;
        assert_eq!(sheet.date.to_string(), "2020-01-06");
    });
}

#[test]
fn quotes_every_currency_the_sheet_prints() {
    with_fixtures(|dir| {
        let bytes = read(dir, "2026-02-07.pdf");
        let sheet = parse_sbi_forex_card_rates(ParseRequest::new(&bytes))
            .unwrap()
            .data;

        // Not just the dollar: the sheet quotes around twenty currencies and
        // choosing between them is the caller's business, not the parser's.
        assert!(
            sheet.currencies.len() > 15,
            "expected the whole sheet, got {} currencies",
            sheet.currencies.len()
        );
        assert!(sheet.currency("USD").is_some(), "the dollar must be quoted");

        // The yen is quoted per hundred units. A caller that assumed one would be
        // out by two orders of magnitude with nothing in the data to warn it.
        let yen = sheet.currency("JPY").expect("the yen must be quoted");
        assert_eq!(yen.unit, 100);
        assert_eq!(sheet.currency("USD").unwrap().unit, 1);
    });
}

#[test]
fn an_unquoted_rate_is_absent_rather_than_zero() {
    with_fixtures(|dir| {
        let bytes = read(dir, "2026-02-07.pdf");
        let sheet = parse_sbi_forex_card_rates(ParseRequest::new(&bytes))
            .unwrap()
            .data;

        // The sheet writes "not quoted" as a zero. Carrying that through as a
        // price is how a rate of 0.00 reaches a published series and is read as a
        // real quote -- so a column the sheet zeroed simply is not there.
        let zeroed = sheet
            .currencies
            .iter()
            .find(|c| c.rates.len() < sheet.columns.len())
            .expect("some currency is not quoted on every column");
        assert!(
            zeroed.rates.values().all(|v| !v.is_zero()),
            "no rate may be published as zero"
        );
    });
}

#[test]
fn every_column_the_sheet_prints_is_carried() {
    with_fixtures(|dir| {
        // The columns are named as printed, era by era, rather than folded into
        // one invented set: the sheet has called the same column TC, FTC and
        // FOREX TRAVEL CARD, and which it was is part of what that day quoted.
        for (name, want) in [
            (
                "2020-01-06.pdf",
                vec![
                    "tt_buy",
                    "tt_sell",
                    "bill_buy",
                    "bill_sell",
                    "tc_buy",
                    "tc_sell",
                    "cn_buy",
                    "cn_sell",
                    "pc_buy",
                ],
            ),
            (
                "2023-06-23.pdf",
                vec![
                    "tt_buy",
                    "tt_sell",
                    "bill_buy",
                    "bill_sell",
                    "forex_travel_card_buy",
                    "forex_travel_card_sell",
                    "cn_buy",
                    "cn_sell",
                    "pc_buy",
                ],
            ),
            (
                "2026-02-07.pdf",
                vec![
                    "tt_buy",
                    "tt_sell",
                    "bill_buy",
                    "bill_sell",
                    "forex_travel_card_buy",
                    "forex_travel_card_sell",
                    "cn_buy",
                    "cn_sell",
                ],
            ),
        ] {
            let bytes = read(dir, name);
            let sheet = parse_sbi_forex_card_rates(ParseRequest::new(&bytes))
                .unwrap()
                .data;
            assert_eq!(sheet.columns, want, "columns for {name}");
        }
    });
}

#[test]
fn reads_a_collapsed_table_by_order_and_says_so() {
    with_fixtures(|dir| {
        // No two rows of this sheet begin at the same place, so no figure sits
        // under a heading. The figures are all there and in order, and every
        // row accounts for every heading exactly once, which is the only thing
        // that makes reading them by order safe rather than a guess.
        let bytes = read(dir, "2021-05-25.pdf");
        let sheet = parse_sbi_forex_card_rates(ParseRequest::new(&bytes))
            .expect("a collapsed table whose figures all account for is still readable")
            .data;

        assert!(
            sheet.figures_matched_by_order,
            "a sheet read by order must not be reported as one the page placed"
        );
        assert_eq!(sheet.currencies.len(), 27);
        assert_eq!(sheet.currency("USD").unwrap().rates.len(), 9);

        // Every other fixture was placed by the page, and must not be flagged.
        for name in ["2020-01-06.pdf", "2023-06-23.pdf", "2026-02-07.pdf"] {
            let bytes = read(dir, name);
            let other = parse_sbi_forex_card_rates(ParseRequest::new(&bytes))
                .unwrap()
                .data;
            assert!(
                !other.figures_matched_by_order,
                "{name} lines up under its headings and must be read that way"
            );
        }
    });
}

#[test]
fn refuses_a_sheet_whose_columns_cannot_be_placed() {
    with_fixtures(|dir| {
        // This sheet is sound -- other readers extract it cleanly -- but it is
        // set in a font whose glyph widths this one cannot measure. Every glyph
        // is advanced by the same amount instead, so the positions drift until
        // separate headings land on top of each other and nothing on the page
        // says which rate a figure is. Counting along the row would answer
        // anyway, and wrongly: the column order has changed twice in six years.
        let bytes = read(dir, "2024-07-02.pdf");
        let err = parse_sbi_forex_card_rates(ParseRequest::new(&bytes))
            .expect_err("a sheet whose columns cannot be placed must not be read");
        assert_eq!(err.kind(), "parse_error");
        assert!(
            err.to_string()
                .contains("Could not place the column headings"),
            "the refusal must say what could not be done, got: {err}"
        );
    });
}

#[test]
fn refuses_a_download_that_was_cut_short() {
    with_fixtures(|dir| {
        // What an interrupted fetch leaves behind: a PDF header and nothing that
        // can be laid out. Half a rate sheet is not a rate sheet.
        let bytes = read(dir, "truncated.pdf");
        let err = parse_sbi_forex_card_rates(ParseRequest::new(&bytes))
            .expect_err("a truncated sheet must not be read");
        assert!(
            matches!(err.kind(), "invalid_format" | "parse_error"),
            "unexpected failure kind: {}",
            err.kind()
        );
    });
}

#[test]
fn a_rate_sheet_is_detected_and_parsed_through_the_front_door() {
    with_fixtures(|dir| {
        let bytes = read(dir, "2026-02-07.pdf");
        let statement =
            xfina::parse(ParseRequest::new(&bytes).with_filename(Some("FOREX_CARD_RATES.pdf")))
                .expect("a rate sheet must be recognised without being named");

        assert_eq!(statement.format.id(), "rt-sbi-forex-card");
        assert_eq!(statement.category().as_str(), "reference_rates");
        assert_eq!(statement.institution(), "State Bank of India");
        assert!(
            statement.data.rates().is_some(),
            "a rate sheet, not an account"
        );
        assert!(statement.data.account().is_none());
    });
}

#[test]
fn rebit_refuses_a_rate_sheet_by_name() {
    with_fixtures(|dir| {
        // ReBIT describes accounts a person holds and has no term for a price an
        // institution published. The envelope has to be an error rather than a
        // shell with an empty `data`, which would read as a sheet that quoted
        // nothing rather than a question with no answer.
        let bytes = read(dir, "2026-02-07.pdf");
        let statement = xfina::parse(ParseRequest::new(&bytes)).unwrap();

        assert!(statement.to_json(Schema::Xfina).is_ok());

        let err = statement
            .to_json(Schema::Rebit)
            .expect_err("ReBIT cannot express a rate sheet");
        assert_eq!(err.kind(), "schema_unsupported");
        assert!(
            err.to_string().contains("rebit"),
            "the refusal must name the schema, got: {err}"
        );
    });
}
