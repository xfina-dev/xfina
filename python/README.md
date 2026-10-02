<p align="center">
  <a href="https://github.com/xfina-dev/xfina" target="_blank">
    <img src="https://github.com/xfina-dev/xfina/raw/main/web/public/favicon.svg" width="120" height="120" alt="Xfina Logo"/>
  </a>
</p>

# [Xfina (Python Bindings)](https://github.com/xfina-dev/xfina)

[![Crates.io](https://img.shields.io/crates/v/xfina.svg?color=orange)](https://crates.io/crates/xfina)
[![PyPI](https://img.shields.io/pypi/v/xfina.svg?color=blue)](https://pypi.org/project/xfina/)
[![npm](https://img.shields.io/npm/v/xfina-wasm.svg?color=yellow)](https://www.npmjs.com/package/xfina-wasm)

**Xfina** is a blazingly fast library for parsing Indian financial statements (Bank Accounts, Credit Cards, Mutual Funds) and international brokers (IBKR), written in Rust and exposed to Python via PyO3.

It converts raw PDFs, Excel files, and CSVs into structured, ReBIT-compliant JSON dictionaries in milliseconds.

## Installation

```bash
pip install xfina
```

## Quick Start

The Python bindings expose the same entry point as the Rust core. To keep the core parsing logic as pure functions without side-effects (like file I/O), the library requires raw bytes to be passed in rather than file paths.

### Parsing a Credit Card Statement

```python
import xfina

with open("statement.xls", "rb") as fh:
    data = fh.read()

# Hand over the bytes; the format is worked out from the content.
statement = xfina.parse(data, filename="statement.xls")

statement["format"]       # "ba-hdfc"
statement["area"]         # "personal", or "public" for rate sheets and price histories
statement["category"]     # "bank_account"
statement["institution"]  # "HDFC Bank"
statement["detection"]    # how it was identified, plus the file's metadata
statement["validation"]   # the two-level validation report
statement["data"]         # the account, in the requested schema

# ReBIT output, or a pinned format that skips detection:
xfina.parse(data, schema="rebit")
xfina.parse(data, **{"as": "ba-hdfc"})
```

Failures raise `xfina.XfinaParseError`, which is where this differs from the
JS binding: exceptions are the idiom here, so the same information arrives as
attributes rather than in an error envelope.

```python
try:
    xfina.parse(data, filename=name)
except xfina.XfinaParseError as e:
    if e.kind == "password_required":
        prompt_for_password(e.format)   # the format the filename suggested
```

## The functions

| Function | Purpose |
|---|---|
| `parse(bytes, password=None, filename=None, modified_timestamp=None, **{"as": None}, schema=None)` | Identify and parse a statement. |
| `detect(bytes, password=None, filename=None, modified_timestamp=None)` | Identify a statement without parsing it. |
| `formats()` | Every format this build knows, with `id`, `area`, `category`, `institution`, `containers` and `enabled`. |
| `version()` | The version of the parsers actually running. |
| `series_coverage(data)` | For the `data` of a price history parse: first and last date, rows with a value, and gaps. Of that one file. |
| `series_csv(data)` | The same `data` as CSV, in Tiingo's column layout. |

