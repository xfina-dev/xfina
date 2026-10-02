<p align="center">
  <a href="https://github.com/xfina-dev/xfina" target="_blank">
    <img src="https://github.com/xfina-dev/xfina/raw/main/web/public/favicon.svg" width="120" height="120" alt="Xfina Logo"/>
  </a>
</p>

# [Xfina (WASM Bindings)](https://github.com/xfina-dev/xfina)

[![Crates.io](https://img.shields.io/crates/v/xfina.svg?color=orange)](https://crates.io/crates/xfina)
[![PyPI](https://img.shields.io/pypi/v/xfina.svg?color=blue)](https://pypi.org/project/xfina/)
[![npm](https://img.shields.io/npm/v/xfina-wasm.svg?color=yellow)](https://www.npmjs.com/package/xfina-wasm)

**Xfina** is a blazingly fast WebAssembly (WASM) library for parsing Indian financial statements (Bank Accounts, Credit Cards, Mutual Funds) and international brokers (IBKR).

It runs **100% in the browser** or Node.js, extracting structured data from raw PDFs, Excel files, and CSVs into ReBIT-compliant JSON objects without ever sending sensitive financial data to a server.

## Installation

```bash
npm install xfina-wasm
```

## Quick Start (Browser / Vite)

```javascript
import init, { parse, detect, formats, version, seriesCoverage, seriesCsv } from "xfina-wasm";

await init();

// Hand over the bytes; the format is worked out from the content.
const result = parse(fileBytes, {
  filename: file.name,                 // used as an ordering hint, never as proof
  password: null,                      // required for encrypted PDFs
  modifiedTimestamp: 1750000000,       // last-resort statement date
  as: null,                            // pin a format id to skip detection
  schema: "xfina",                     // or "rebit"
});

if (result.error) {
  // Failures arrive in the envelope rather than as a thrown value, so a
  // caller branches on `kind` instead of matching on message text.
  if (result.error.kind === "password_required") {
    promptForPassword(result.error.filename_hint); // e.g. "mf-cams"
  }
} else {
  result.format;       // "ba-hdfc"
  result.area;         // "personal", or "public" for rate sheets and price histories
  result.category;     // "bank_account"
  result.institution;  // "HDFC Bank"
  result.detection;    // how it was identified, plus the file's own metadata
  result.validation;   // the two-level validation report
  result.data;         // the account, in the requested schema
}
```

## The functions

| Function | Purpose |
|---|---|
| `parse(bytes, options)` | Identify and parse a statement. Returns the envelope, or `{ error }`. |
| `detect(bytes, options)` | Identify a statement without parsing it. |
| `formats()` | Every format this build knows, with `id`, `area`, `category`, `institution`, `containers` and `enabled`. Build your UI from this rather than a hardcoded list. |
| `version()` | The version of the parsers actually running. |
| `seriesCoverage(data)` | For the `data` of a price history parse: first and last date, rows with a value, and gaps. Of that one file. |
| `seriesCsv(data)` | The same `data` as CSV, in Tiingo's column layout. |

**Error kinds:** `password_required`, `incorrect_password`, `unrecognized_format`,
`invalid_format`, `parse_error`, `unsupported`, `format_not_enabled`, `io`.

