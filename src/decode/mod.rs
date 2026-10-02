//! Reading a file's bytes once, whatever ends up parsing it.
//!
//! Detection asks several parsers whether a file is theirs, and each of them
//! used to open the file for itself -- three PDF loads to answer one question.
//! Everything here is decoded lazily and at most once per input, so probing
//! costs one read no matter how many candidates are considered.

use std::cell::OnceCell;

use crate::detect::{sniff, Container};
use crate::error::XfinaError;
use crate::models::request::ParseRequest;

#[cfg(feature = "_html")]
pub mod html_table;
#[cfg(feature = "_pdf")]
pub mod pdf;
#[cfg(feature = "_spreadsheet")]
pub mod sheets;
#[cfg(feature = "_xmlss")]
pub mod xmlss;

#[cfg(feature = "_pdf")]
pub use pdf::{CharItem, PdfDoc};
#[cfg(feature = "_spreadsheet")]
pub use sheets::Sheets;
#[cfg(feature = "_xmlss")]
pub use xmlss::XmlSheets;

/// Why a file could not be decoded.
///
/// Separate from [`XfinaError`] because it must be `Clone`: a cached decode
/// result is handed out repeatedly, and `XfinaError` wraps `std::io::Error`,
/// which is not cloneable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The bytes are not this kind of container at all.
    NotThisContainer(String),
    /// The right kind of container, but unreadable.
    Damaged(String),
    PasswordRequired,
    IncorrectPassword,
}

impl From<DecodeError> for XfinaError {
    fn from(e: DecodeError) -> Self {
        match e {
            DecodeError::NotThisContainer(msg) => XfinaError::InvalidFormat(msg),
            DecodeError::Damaged(msg) => XfinaError::ParseError(msg),
            DecodeError::PasswordRequired => XfinaError::PasswordRequired,
            DecodeError::IncorrectPassword => XfinaError::IncorrectPassword,
        }
    }
}

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// One input file, decoded on demand and remembered.
pub struct Decoded<'a> {
    container: Container,
    bytes: &'a [u8],
    password: Option<&'a str>,
    probe_text: OnceCell<String>,
    #[cfg(feature = "_spreadsheet")]
    sheets: OnceCell<Result<Sheets, DecodeError>>,
    #[cfg(feature = "_pdf")]
    pdf: OnceCell<Result<PdfDoc, DecodeError>>,
    #[cfg(feature = "_xmlss")]
    xml_sheets: OnceCell<Result<XmlSheets, DecodeError>>,
}

impl<'a> Decoded<'a> {
    /// Sniffs the container. Nothing is decoded until it is asked for.
    pub fn new(req: &ParseRequest<'a>) -> Self {
        Self {
            container: sniff(req.content),
            bytes: req.content,
            password: req.password,
            probe_text: OnceCell::new(),
            #[cfg(feature = "_spreadsheet")]
            sheets: OnceCell::new(),
            #[cfg(feature = "_pdf")]
            pdf: OnceCell::new(),
            #[cfg(feature = "_xmlss")]
            xml_sheets: OnceCell::new(),
        }
    }

    pub fn container(&self) -> Container {
        self.container
    }

    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn password(&self) -> Option<&'a str> {
        self.password
    }

    /// The file as UTF-8, exactly as it is on disk.
    ///
    /// The BOM is deliberately left in place: IBKR CSVs carry one, and its
    /// presence is part of what those parsers see today. Probes that want it
    /// gone use [`Decoded::probe_text`].
    pub fn text(&self) -> Result<&'a str, DecodeError> {
        std::str::from_utf8(self.bytes)
            .map_err(|e| DecodeError::NotThisContainer(format!("Invalid UTF-8: {}", e)))
    }

    /// A forgiving view of the content for probes: lossy, BOM-stripped, and
    /// capped at the head of the file, where institutions identify themselves.
    ///
    /// For a PDF this is the text of page 1; for anything else, the raw bytes
    /// read as text. Never fails -- a probe that cannot read the file simply
    /// does not match.
    pub fn probe_text(&self) -> &str {
        self.probe_text.get_or_init(|| {
            #[cfg(feature = "_pdf")]
            if self.container == Container::Pdf {
                return match self.pdf() {
                    Ok(doc) => doc.page1_text().to_string(),
                    Err(_) => String::new(),
                };
            }
            const HEAD: usize = 16_384;
            let body = self.bytes.strip_prefix(&UTF8_BOM).unwrap_or(self.bytes);
            let head = &body[..body.len().min(HEAD)];
            String::from_utf8_lossy(head).into_owned()
        })
    }

    #[cfg(feature = "_spreadsheet")]
    pub fn sheets(&self) -> Result<&Sheets, DecodeError> {
        self.sheets
            .get_or_init(|| Sheets::open(self.bytes))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The file as an XML Spreadsheet 2003 workbook, read once.
    #[cfg(feature = "_xmlss")]
    pub fn xml_sheets(&self) -> Result<&XmlSheets, DecodeError> {
        self.xml_sheets
            .get_or_init(|| {
                if self.container != Container::Text {
                    return Err(DecodeError::NotThisContainer(
                        "XML Spreadsheet is a text container".to_string(),
                    ));
                }
                XmlSheets::open(self.text()?)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    #[cfg(feature = "_pdf")]
    pub fn pdf(&self) -> Result<&PdfDoc, DecodeError> {
        self.pdf
            .get_or_init(|| PdfDoc::open(self.bytes, self.password))
            .as_ref()
            .map_err(Clone::clone)
    }
}
