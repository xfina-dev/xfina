//! Every parser must reject a file it does not own with `InvalidFormat`, and
//! must never mistake a wrong guess for a damaged file.
//!
//! Detection leans entirely on that distinction: `InvalidFormat` means "try the
//! next candidate", anything else means "stop and report this". A parser that
//! returns `ParseError` for a file in the wrong container makes a wrong guess
//! look like corruption; one that returns `Ok` claims files it cannot read.
//!
//! Every fixture here is generated in this file. Nothing is read from the
//! private corpus and no real statement value appears -- the parsers are being
//! asked about containers, and a container needs no contents to be wrong.

use xfina::error::XfinaError;
use xfina::models::request::ParseRequest;

/// The eight leading bytes of an OLE2 compound file, followed by filler.
/// Enough to be sniffed as a real `.xls` and to get past a reader's magic
/// check, not enough to be a workbook.
fn ole2_bytes() -> Vec<u8> {
    let mut v = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    v.extend_from_slice(&[0u8; 512]);
    v
}

/// A zip with one stored (uncompressed) entry named `a.txt`. Structurally a
/// valid archive, so an xlsx reader opens it and then finds no workbook parts.
fn zip_bytes() -> Vec<u8> {
    let name = b"a.txt";
    let body = b"x";
    let crc: u32 = 0x8C9F_3610; // crc32 of "x"
    let mut v = Vec::new();
    v.extend_from_slice(b"PK\x03\x04\x14\x00\x00\x00\x00\x00");
    v.extend_from_slice(&[0u8; 4]); // time, date
    v.extend_from_slice(&crc.to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(&(name.len() as u16).to_le_bytes());
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(name);
    v.extend_from_slice(body);

    let central = v.len() as u32;
    v.extend_from_slice(b"PK\x01\x02\x14\x00\x14\x00\x00\x00\x00\x00");
    v.extend_from_slice(&[0u8; 4]);
    v.extend_from_slice(&crc.to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(&(name.len() as u16).to_le_bytes());
    v.extend_from_slice(&[0u8; 8]);
    v.extend_from_slice(&[0u8; 4]); // external attrs
    v.extend_from_slice(&0u32.to_le_bytes()); // local header offset
    v.extend_from_slice(name);
    let central_len = v.len() as u32 - central;

    v.extend_from_slice(b"PK\x05\x06\x00\x00\x00\x00");
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&central_len.to_le_bytes());
    v.extend_from_slice(&central.to_le_bytes());
    v.extend_from_slice(&0u16.to_le_bytes());
    v
}

/// A one-page PDF with no text, written out by hand so the test carries no
/// binary blob. Loads as a document; contains nothing any parser wants.
fn pdf_bytes() -> Vec<u8> {
    let objects: [&str; 4] = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ];
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, body));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for off in &offsets {
        out.push_str(&format!("{:010} 00000 n \n", off));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        objects.len() + 1,
        xref
    ));
    out.into_bytes()
}

/// Plain text that is syntactically a CSV but belongs to no institution.
fn text_bytes() -> Vec<u8> {
    b"alpha,beta,gamma\n1,2,3\n4,5,6\n".to_vec()
}

fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("ole2", ole2_bytes()),
        ("zip", zip_bytes()),
        ("pdf", pdf_bytes()),
        ("text", text_bytes()),
    ]
}

type Parser = fn(ParseRequest<'_>) -> Result<(), XfinaError>;

/// Erases what each parser returns so they can all be driven from one table.
// Every use sits behind a feature gate, so a build with no parsers has none.
#[allow(unused_macros)]
macro_rules! erased {
    ($p:path) => {
        (|req| $p(req).map(|_| ())) as Parser
    };
}

fn parsers() -> Vec<(&'static str, Parser)> {
    vec![
        #[cfg(feature = "ba-hdfc")]
        (
            "ba-hdfc",
            erased!(xfina::bank_accounts::hdfc::parse_hdfc_bank_statement),
        ),
        #[cfg(feature = "ba-icici")]
        (
            "ba-icici",
            erased!(xfina::bank_accounts::icici::parse_icici_bank_statement),
        ),
        #[cfg(feature = "ba-sbi")]
        (
            "ba-sbi",
            erased!(xfina::bank_accounts::sbi::parse_sbi_bank_statement),
        ),
        #[cfg(feature = "ba-bob")]
        ("ba-bob", erased!(xfina::bank_accounts::bob::parse_bob_xls)),
        #[cfg(feature = "ba-axis")]
        (
            "ba-axis",
            erased!(xfina::bank_accounts::axis::parse_axis_bank_statement),
        ),
        #[cfg(feature = "cc-hdfc")]
        (
            "cc-hdfc",
            erased!(xfina::credit_cards::hdfc::parse_hdfc_statement),
        ),
        #[cfg(feature = "cc-icici")]
        (
            "cc-icici",
            erased!(xfina::credit_cards::icici::parse_icici_statement),
        ),
        #[cfg(feature = "cc-axis")]
        (
            "cc-axis",
            erased!(xfina::credit_cards::axis::parse_axis_statement),
        ),
        #[cfg(feature = "mf-cams")]
        (
            "mf-cams",
            erased!(xfina::mutual_funds::cams::parse_cams_pdf),
        ),
        #[cfg(feature = "is-ibkr")]
        ("is-ibkr", erased!(xfina::intl_stocks::ibkr::parse_ibkr_csv)),
        #[cfg(feature = "rt-sbi-forex-card")]
        (
            "rt-sbi-forex-card",
            erased!(xfina::reference_rates::sbi_forex_card::parse_sbi_forex_card_rates),
        ),
        #[cfg(feature = "md-amfi-nav")]
        (
            "md-amfi-nav",
            erased!(xfina::market_data::amfi::parse_amfi_nav),
        ),
        #[cfg(feature = "md-nse-security")]
        (
            "md-nse-security",
            erased!(xfina::market_data::nse_security::parse_nse_security),
        ),
        #[cfg(feature = "md-nse-indices")]
        (
            "md-nse-indices",
            erased!(xfina::market_data::nse_indices::parse_nse_indices),
        ),
        #[cfg(feature = "md-mcx-spot")]
        (
            "md-mcx-spot",
            erased!(xfina::market_data::mcx::parse_mcx_spot),
        ),
        #[cfg(feature = "md-ishares")]
        (
            "md-ishares",
            erased!(xfina::market_data::ishares::parse_ishares),
        ),
        #[cfg(feature = "md-tiingo")]
        (
            "md-tiingo",
            erased!(xfina::market_data::tiingo::parse_tiingo),
        ),
        #[cfg(feature = "md-wsj")]
        ("md-wsj", erased!(xfina::market_data::wsj::parse_wsj)),
        #[cfg(feature = "md-msci")]
        ("md-msci", erased!(xfina::market_data::msci::parse_msci)),
        #[cfg(feature = "md-nasdaq")]
        (
            "md-nasdaq",
            erased!(xfina::market_data::nasdaq::parse_nasdaq),
        ),
        #[cfg(feature = "md-yahoo")]
        ("md-yahoo", erased!(xfina::market_data::yahoo::parse_yahoo)),
        #[cfg(feature = "md-spdr-gold")]
        (
            "md-spdr-gold",
            erased!(xfina::market_data::spdr::parse_spdr_gold),
        ),
    ]
}

#[test]
fn no_parser_claims_a_file_it_cannot_read() {
    let mut wrong = Vec::new();
    for (parser_name, parse) in parsers() {
        for (fixture_name, bytes) in fixtures() {
            let outcome = match parse(ParseRequest::new(&bytes)) {
                Ok(()) => Some("returned Ok".to_string()),
                Err(e) if e.is_wrong_format() => None,
                Err(e) => Some(format!("returned {}", e.kind())),
            };
            if let Some(outcome) = outcome {
                wrong.push(format!(
                    "  {parser_name} on a {fixture_name} fixture {outcome}"
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "every parser must answer InvalidFormat for a file it does not own:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn an_empty_input_is_never_claimed() {
    for (parser_name, parse) in parsers() {
        match parse(ParseRequest::new(&[])) {
            Ok(()) => panic!("{parser_name} returned Ok for an empty file"),
            Err(e) => assert!(
                e.is_wrong_format(),
                "{parser_name} answered {} for an empty file, want invalid_format",
                e.kind()
            ),
        }
    }
}

#[cfg(feature = "is-ibkr")]
#[test]
fn ibkr_reads_its_own_sections_and_refuses_other_csv() {
    // Column 0 is a section name and column 1 a row kind: the shape every
    // IBKR activity statement opens with. Values are invented.
    let statement = "Statement,Header,Field Name,Field Value\n\
                     Statement,Data,Period,\"January 1, 2026 - January 31, 2026\"\n\
                     Account Information,Data,Account,U0000000\n\
                     Account Information,Data,Name,Test Holder\n";
    assert!(
        xfina::intl_stocks::ibkr::parse_ibkr_csv(ParseRequest::new(statement.as_bytes())).is_ok(),
        "a statement carrying IBKR sections must still parse"
    );

    // Same CSV shape, no IBKR section anywhere.
    let not_ibkr = "Date,Description,Amount\n2026-01-01,something,10.00\n";
    let err = xfina::intl_stocks::ibkr::parse_ibkr_csv(ParseRequest::new(not_ibkr.as_bytes()))
        .expect_err("arbitrary CSV must not be claimed as an IBKR statement");
    assert_eq!(err.kind(), "invalid_format");
}

// An encrypted PDF must report a password need rather than an unreadable
// format. That cannot be asserted here: a PDF hand-written in a test is not
// genuinely encrypted, so the assertion would pass for the wrong reason. It
// lives in tests/detect_corpus.rs instead, against real encrypted statements.
