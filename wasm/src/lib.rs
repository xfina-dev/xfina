//! The browser-facing interface: four functions, whatever the statement is,
//! and two for a parsed price series -- what it covers, and it as CSV.
//!
//! Callers used to pick one of ten exports by working out the category and
//! institution themselves. They hand over bytes now, and `parse` says what the
//! file was.

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use xfina::detect::Format;
use xfina::error::XfinaError;
use xfina::models::{ParseRequest, PriceSeries, Schema};

/// Everything optional a caller can attach to a file.
///
/// An options object rather than positional arguments: the old signature had
/// already grown to five, and this way a new field is not a breaking change
/// for every existing call.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Options {
    password: Option<String>,
    filename: Option<String>,
    modified_timestamp: Option<i64>,
    /// Parse as this format instead of detecting one.
    #[serde(rename = "as")]
    as_format: Option<String>,
    /// "xfina" (default) or "rebit".
    schema: Option<String>,
}

fn options_of(value: JsValue) -> Result<Options, JsValue> {
    if value.is_undefined() || value.is_null() {
        return Ok(Options::default());
    }
    serde_wasm_bindgen::from_value(value)
        .map_err(|e| JsValue::from_str(&format!("Invalid options: {}", e)))
}

fn to_js(value: &serde_json::Value) -> Result<JsValue, JsValue> {
    // json_compatible, not the default: serde-wasm-bindgen otherwise turns
    // every map into a JS `Map`, which reads as an empty object to anything
    // expecting plain JSON.
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    value
        .serialize(&serializer)
        .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))
}

/// Failures come back in the same envelope as successes rather than as a
/// thrown value, so a caller branches on `error.kind` -- "password_required"
/// above all -- instead of matching on message text.
fn error_envelope(error: &XfinaError, filename: Option<&str>) -> serde_json::Value {
    let mut body = serde_json::json!({
        "kind": error.kind(),
        "message": error.to_string(),
    });
    if let XfinaError::UnrecognizedFormat { container, .. } = error {
        body["container"] = serde_json::json!(container.as_str());
    }
    // What the name suggested, so a password prompt can say which institution
    // it is asking about rather than just "this file".
    if let Some(format) = xfina::detect::hint::from_filename(filename) {
        body["filename_hint"] = serde_json::json!(format.id());
    }
    serde_json::json!({ "error": body })
}

fn format_of(options: &Options) -> Result<Option<Format>, JsValue> {
    match options.as_format.as_deref() {
        None => Ok(None),
        Some(id) => Format::from_id(id)
            .map(Some)
            .ok_or_else(|| JsValue::from_str(&format!("Unknown format '{}'", id))),
    }
}

/// Parses a statement, working out what it is.
///
/// Returns the envelope: `format`, `category`, `institution`, `detection`,
/// `validation` and `data` on success, or `{ error }` on failure.
#[wasm_bindgen]
pub fn parse(bytes: &[u8], options: JsValue) -> Result<JsValue, JsValue> {
    let options = options_of(options)?;
    let format = format_of(&options)?;
    let schema = Schema::from_name(options.schema.as_deref());

    let request = ParseRequest::new(bytes)
        .with_password(options.password.as_deref())
        .with_filename(options.filename.as_deref())
        .with_modified_timestamp(options.modified_timestamp)
        .with_format(format);

    match xfina::parse(request).and_then(|statement| statement.to_json(schema)) {
        Ok(json) => to_js(&json),
        Err(e) => to_js(&error_envelope(&e, options.filename.as_deref())),
    }
}

/// Reports what a file is without parsing it.
#[wasm_bindgen]
pub fn detect(bytes: &[u8], options: JsValue) -> Result<JsValue, JsValue> {
    let options = options_of(options)?;
    let request = ParseRequest::new(bytes)
        .with_password(options.password.as_deref())
        .with_filename(options.filename.as_deref())
        .with_modified_timestamp(options.modified_timestamp);

    match xfina::detect(&request) {
        Ok(detection) => to_js(&serde_json::json!(detection)),
        Err(e) => to_js(&error_envelope(&e, options.filename.as_deref())),
    }
}

/// A parsed price series as CSV, in Tiingo's column layout.
///
/// Takes the `data` of a market data parse -- the object `parse` returned, as
/// it is -- so the file is not read twice. Every surface writes the same CSV
/// because they all call the same function in the library.
#[wasm_bindgen(js_name = seriesCsv)]
pub fn series_csv(data: JsValue) -> Result<String, JsValue> {
    let value: serde_json::Value = serde_wasm_bindgen::from_value(data)
        .map_err(|e| JsValue::from_str(&format!("Invalid series: {}", e)))?;
    PriceSeries::from_json(value)
        .map(|series| series.to_csv())
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

/// What a parsed price series covers: first and last date, rows with a
/// value, and gaps longer than its frequency explains.
///
/// Of the one file it was parsed from, never of several: putting pieces of a
/// dataset together is not this library's job.
#[wasm_bindgen(js_name = seriesCoverage)]
pub fn series_coverage(data: JsValue) -> Result<JsValue, JsValue> {
    let value: serde_json::Value = serde_wasm_bindgen::from_value(data)
        .map_err(|e| JsValue::from_str(&format!("Invalid series: {}", e)))?;
    let series = PriceSeries::from_json(value).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js(&serde_json::json!(series.coverage()))
}

/// Every format this build knows, with whether it is compiled in.
///
/// A UI builds its picker from this rather than hardcoding a list that drifts
/// out of step with the library.
#[wasm_bindgen]
pub fn formats() -> Result<JsValue, JsValue> {
    to_js(&serde_json::json!(xfina::formats()))
}

/// The version of the parsers actually running, which is not necessarily the
/// version of the page that loaded them.
#[wasm_bindgen]
pub fn version() -> String {
    xfina::version().to_string()
}
