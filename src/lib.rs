//! # Xfina
//!
//! **Xfina** is a comprehensive suite of financial statement parsers specifically tailored for the Indian financial ecosystem (Bank Accounts, Credit Cards, Mutual Funds) and international brokers (IBKR).
//!
//! This crate acts as the **facade library** that unifies all the individual parser sub-crates under a single, ergonomic namespace. It allows you to import only what you need, while ensuring all output perfectly adheres to the standardized ReBIT JSON schemas via `xfina-models`.
//!
//! ## Example Usage
//!
//! Hand over the bytes and the name they arrived with; xfina works out what
//! the file is.
//!
//! ```no_run
//! use xfina::{parse, ParseRequest, Schema};
//!
//! # fn main() -> Result<(), xfina::error::XfinaError> {
//! let bytes = std::fs::read("statement.xls").unwrap();
//! let statement = parse(ParseRequest::new(&bytes).with_filename(Some("statement.xls")))?;
//!
//! println!("{} statement from {}", statement.format, statement.institution());
//! let json = statement.to_json(Schema::Xfina)?;
//! # Ok(())
//! # }
//! ```
//!
//! To skip detection, put the format on the request with
//! [`ParseRequest::with_format`]. The per-format functions in the modules
//! below remain available for callers that already know what they hold.
//!
//! ## Organization
//!
//! The library is organized by financial instrument category:
//!
//! - [`detect`]: Identifying a statement's format, and the registry of formats
//! - [`models`]: Shared data models (ReBIT / Sahamati AA standard compatible)
//! - [`bank_accounts`]: Parsers for savings and current accounts (HDFC, ICICI, SBI, BOB, Axis)
//! - [`credit_cards`]: Parsers for credit card statements (HDFC, ICICI, Axis)
//! - [`mutual_funds`]: Parsers for mutual fund statements (CAMS CAS)
//! - [`intl_stocks`]: Parsers for international broker statements (IBKR)
//! - [`reference_rates`]: Parsers for published rate sheets (SBI forex card rates)
//! - [`market_data`]: Parsers for published price, NAV and index history
//!
//! Formats fall into two [`Area`]s. **Personal** statements describe an
//! account somebody holds, and render into either schema. **Public** data --
//! a rate sheet, a price history -- does not, so [`Statement::to_json`]
//! returns a `Result`: [`Schema::Rebit`] has no term for a price an
//! institution published and says so rather than returning an empty envelope.
//!
//! Each file parses on its own. A price history split across several
//! downloads comes back as several series; putting them back together is
//! computation over parsed data, not parsing, and is not done here.

pub mod mutual_funds {
    #[cfg(feature = "mf-cams")]
    pub mod cams;
    #[cfg(feature = "mf-cams")]
    pub mod cas;
    #[cfg(feature = "mf-cams")]
    pub mod layout;
}

pub mod intl_stocks {
    #[cfg(feature = "is-ibkr")]
    pub mod ibkr;
}

pub mod reference_rates {
    #[cfg(feature = "rt-sbi-forex-card")]
    pub mod sbi_forex_card;
}

pub mod market_data;

pub mod credit_cards {
    #[cfg(feature = "cc-axis")]
    pub mod axis;
    #[cfg(feature = "cc-hdfc")]
    pub mod hdfc;
    #[cfg(feature = "cc-icici")]
    pub mod icici;
}

pub mod bank_accounts {
    #[cfg(feature = "ba-axis")]
    pub mod axis;
    #[cfg(feature = "ba-bob")]
    pub mod bob;
    #[cfg(feature = "ba-hdfc")]
    pub mod hdfc;
    #[cfg(feature = "ba-icici")]
    pub mod icici;
    #[cfg(feature = "ba-sbi")]
    pub mod sbi;

    #[cfg(feature = "ba-sbi")]
    pub(crate) mod layout;
    #[cfg(feature = "ba-sbi")]
    pub(crate) mod pdf_parser;
}

pub mod decode;
pub mod detect;
pub mod error;
pub mod models;

pub use detect::{detect, detect_format, formats, Area, Category, Format, FormatInfo};
pub use models::{ParseRequest, Parsed, Schema, Statement};

/// This crate's version, so a caller can report which parsers it is running.
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Parses a statement, working out what it is.
///
/// This is the entry point every surface exposes. Hand it bytes and, ideally,
/// the filename they arrived with; it identifies the format and returns the
/// parsed account alongside the evidence for that identification.
///
/// To parse as a known format instead, put it on the request with
/// [`ParseRequest::with_format`] -- the override rides on the input, so there
/// is only ever this one function.
///
/// # Errors
///
/// - [`XfinaError::UnrecognizedFormat`] when the file was read but no
///   compiled-in parser claims it.
/// - [`XfinaError::PasswordRequired`] / [`XfinaError::IncorrectPassword`] when
///   the file cannot be opened to be identified. A failure to open always
///   outranks a failure to recognise, so an encrypted PDF asks for its
///   password rather than being reported as unknown.
/// - Whatever the chosen parser returns, unchanged.
///
/// A statement whose totals do not reconcile is **not** an error: it comes
/// back parsed, labelled with its real format, and with a failing
/// `validation.overall`. Detection never reassigns a file to another
/// institution because the numbers disappointed it.
///
/// [`XfinaError::UnrecognizedFormat`]: error::XfinaError::UnrecognizedFormat
/// [`XfinaError::PasswordRequired`]: error::XfinaError::PasswordRequired
/// [`XfinaError::IncorrectPassword`]: error::XfinaError::IncorrectPassword
///
/// # Example
///
/// ```no_run
/// use xfina::{parse, ParseRequest, Schema};
///
/// let bytes = std::fs::read("statement.xls").unwrap();
/// let statement = parse(ParseRequest::new(&bytes).with_filename(Some("statement.xls")))?;
///
/// println!("{} ({})", statement.institution(), statement.format);
/// let json = statement.to_json(Schema::Xfina)?;
/// # Ok::<(), xfina::error::XfinaError>(())
/// ```
pub fn parse(input: ParseRequest<'_>) -> Result<Statement, error::XfinaError> {
    let decoded = decode::Decoded::new(&input);
    let detection = detect::resolve(&decoded, &input)?;
    let format = detection.format;

    // The decode is warm from probing, so the parser re-reads nothing.
    let (mut data, validation) = detect::registry::dispatch_parse(format, &decoded, &input)?;

    // Last rung of the generated-date chain, after the parser has had its say.
    // Only accounts have one: a rate sheet prints the day it belongs to, and
    // falling back to when the file reached the disk would file a morning's
    // rates under whatever day someone happened to download them.
    if let models::Parsed::Account(account) = &mut data {
        models::generated_date::apply_modified_timestamp_fallback(account, &input);
    }

    Ok(Statement {
        format,
        data,
        validation,
        detection,
    })
}
