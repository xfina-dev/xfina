use serde::Serialize;
use serde_json::{json, Value};

use crate::detect::registry::{Area, Category, Format};
use crate::detect::{Container, Strength};
use crate::models::parsed::Parsed;
use crate::models::request::ParseRequest;
use crate::models::schema::Schema;
use crate::models::validation::ValidationReport;

/// What the caller handed over, echoed back.
///
/// A caller that renders a result -- the web app in particular -- should not
/// have to hold on to the file it uploaded to display its name and size. Both
/// come back with the parse.
#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    pub name: Option<String>,
    pub size: usize,
    pub modified_timestamp: Option<i64>,
}

impl FileInfo {
    pub fn of(req: &ParseRequest<'_>) -> Self {
        FileInfo {
            name: req.filename.map(str::to_string),
            size: req.content.len(),
            modified_timestamp: req.modified_timestamp,
        }
    }
}

/// How confident detection is about the format it picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// The caller named the format; nothing was inferred.
    Stated,
    /// A marker only this format prints was found in the content.
    Strong,
    /// Nothing conclusive; this was the best of the plausible candidates.
    Weak,
}

/// A format that was considered, and how strongly it claimed the file.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub format: Format,
    pub strength: Strength,
    pub reason: &'static str,
}

/// Why detection landed where it did.
#[derive(Debug, Clone, Serialize)]
pub struct Detection {
    /// The format detection settled on.
    pub format: Format,
    pub confidence: Confidence,
    pub container: Container,
    /// Reason code from the winning probe.
    pub reason: &'static str,
    /// What the filename suggested, if anything. Never authoritative.
    pub filename_hint: Option<Format>,
    /// Other formats that also claimed the file, strongest first.
    pub candidates: Vec<Candidate>,
    pub file: FileInfo,
}

/// A parsed document: what it is, what it says, and how we know.
#[derive(Debug, Clone)]
pub struct Statement {
    pub format: Format,
    /// What the parser produced -- an account for most formats, a rate sheet
    /// for a published reference document, a price series for market data.
    pub data: Parsed,
    pub validation: ValidationReport,
    pub detection: Detection,
}

impl Statement {
    pub fn category(&self) -> Category {
        self.format.category()
    }

    /// Personal statement or public data.
    pub fn area(&self) -> Area {
        self.format.category().area()
    }

    pub fn institution(&self) -> &'static str {
        self.format.institution()
    }

    /// The envelope every surface speaks.
    ///
    /// `data` and `validation` keep the exact shape the per-parser functions
    /// have always produced, so this is additive for anything already reading
    /// them; `format`, `area`, `category`, `institution` and `detection` are
    /// what a caller no longer has to work out for itself.
    /// # Errors
    ///
    /// [`crate::error::XfinaError::SchemaUnsupported`] when the schema cannot
    /// express this kind of document. The envelope is either the document or
    /// an error, never a shell with the parts the schema had no word for
    /// quietly missing.
    pub fn to_json(&self, schema: Schema) -> Result<Value, crate::error::XfinaError> {
        Ok(json!({
            "schema": schema.as_str(),
            "format": self.format.id(),
            "area": self.area().as_str(),
            "category": self.format.category().as_str(),
            "institution": self.format.institution(),
            "detection": self.detection,
            "validation": self.validation,
            "data": self.data.to_json(schema)?,
        }))
    }

    pub fn to_json_string(
        &self,
        schema: Schema,
        pretty: bool,
    ) -> Result<String, crate::error::XfinaError> {
        let value = self.to_json(schema)?;
        let rendered = if pretty {
            serde_json::to_string_pretty(&value)
        } else {
            serde_json::to_string(&value)
        };
        rendered.map_err(|e| crate::error::XfinaError::ParseError(e.to_string()))
    }
}
