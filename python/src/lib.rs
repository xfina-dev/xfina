//! The Python interface: four functions, whatever the statement is, and two
//! for a parsed price series -- what it covers, and it as CSV.
//!
//! Callers used to pick one of ten functions by working out the category and
//! institution themselves. They hand over bytes now, and `parse` says what the
//! file was.

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pythonize::{depythonize_bound, pythonize};

use ::xfina::detect::Format;
use ::xfina::error::XfinaError;
use ::xfina::models::{ParseRequest, PriceSeries, Schema};

create_exception!(
    xfina,
    XfinaParseError,
    PyException,
    "Raised when a statement cannot be read or identified."
);

fn to_py(py: Python, value: serde_json::Value) -> PyResult<PyObject> {
    pythonize(py, &value).map_err(|e| PyValueError::new_err(format!("Serialization error: {}", e)))
}

/// Failures are raised, not returned -- exceptions are the idiom here, where
/// the JS binding returns an error envelope. The same information is carried
/// either way: `kind` distinguishes a missing password from an unreadable
/// file, and `format` names what the filename suggested.
fn raise(py: Python, error: &XfinaError, filename: Option<&str>) -> PyErr {
    let exception = XfinaParseError::new_err(error.to_string());
    let value = exception.value_bound(py);
    let _ = value.setattr("kind", error.kind());
    // What the name suggested, so a caller prompting for a password can say
    // which institution it is asking about rather than just "this file".
    let hint = ::xfina::detect::hint::from_filename(filename).map(|f| f.id());
    let _ = value.setattr("format", hint);
    exception
}

fn format_of(as_format: Option<&str>) -> PyResult<Option<Format>> {
    match as_format {
        None => Ok(None),
        Some(id) => Format::from_id(id)
            .map(Some)
            .ok_or_else(|| PyValueError::new_err(format!("Unknown format '{}'", id))),
    }
}

/// Parses a statement, working out what it is.
///
/// Returns a dict with `format`, `category`, `institution`, `detection`,
/// `validation` and `data`. Raises `XfinaParseError` if the file cannot be
/// read or is not a statement we recognise.
#[pyfunction]
#[pyo3(signature = (bytes, password=None, filename=None, modified_timestamp=None, r#as=None, schema=None))]
#[allow(clippy::too_many_arguments)]
fn parse(
    py: Python,
    bytes: &[u8],
    password: Option<&str>,
    filename: Option<&str>,
    modified_timestamp: Option<i64>,
    r#as: Option<&str>,
    schema: Option<&str>,
) -> PyResult<PyObject> {
    let request = ParseRequest::new(bytes)
        .with_password(password)
        .with_filename(filename)
        .with_modified_timestamp(modified_timestamp)
        .with_format(format_of(r#as)?);

    match ::xfina::parse(request).and_then(|statement| statement.to_json(Schema::from_name(schema)))
    {
        Ok(json) => to_py(py, json),
        Err(e) => Err(raise(py, &e, filename)),
    }
}

/// Reports what a file is without parsing it.
#[pyfunction]
#[pyo3(signature = (bytes, password=None, filename=None, modified_timestamp=None))]
fn detect(
    py: Python,
    bytes: &[u8],
    password: Option<&str>,
    filename: Option<&str>,
    modified_timestamp: Option<i64>,
) -> PyResult<PyObject> {
    let request = ParseRequest::new(bytes)
        .with_password(password)
        .with_filename(filename)
        .with_modified_timestamp(modified_timestamp);

    match ::xfina::detect(&request) {
        Ok(detection) => to_py(py, serde_json::json!(detection)),
        Err(e) => Err(raise(py, &e, filename)),
    }
}

/// A parsed price series as CSV, in Tiingo's column layout.
///
/// Takes the `data` dict of a market data parse, as `parse` returned it, so
/// the file is not read twice.
#[pyfunction]
fn series_csv(data: &Bound<'_, PyAny>) -> PyResult<String> {
    let value: serde_json::Value = depythonize_bound(data.clone())
        .map_err(|e| PyValueError::new_err(format!("Invalid series: {}", e)))?;
    PriceSeries::from_json(value)
        .map(|series| series.to_csv())
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

/// What a parsed price series covers: first and last date, rows with a
/// value, and gaps longer than its frequency explains. Of that one file only.
#[pyfunction]
fn series_coverage(py: Python, data: &Bound<'_, PyAny>) -> PyResult<PyObject> {
    let value: serde_json::Value = depythonize_bound(data.clone())
        .map_err(|e| PyValueError::new_err(format!("Invalid series: {}", e)))?;
    let series = PriceSeries::from_json(value).map_err(|e| PyValueError::new_err(e.to_string()))?;
    to_py(py, serde_json::json!(series.coverage()))
}

/// Every format this build knows, with whether it is compiled in.
#[pyfunction]
fn formats(py: Python) -> PyResult<PyObject> {
    to_py(py, serde_json::json!(::xfina::formats()))
}

#[pyfunction]
fn version() -> PyResult<String> {
    Ok(::xfina::version().to_string())
}

#[pymodule]
fn xfina(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(detect, m)?)?;
    m.add_function(wrap_pyfunction!(formats, m)?)?;
    m.add_function(wrap_pyfunction!(series_csv, m)?)?;
    m.add_function(wrap_pyfunction!(series_coverage, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add(
        "XfinaParseError",
        m.py().get_type_bound::<XfinaParseError>(),
    )?;
    Ok(())
}
