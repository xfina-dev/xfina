# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.0] - 2026-10-02

### Added

- **Public market data:** an `md-` family of eleven parsers for the price, NAV
  and index history Xfina Labs' Portfolio Engine sends people to download:
  AMFI NAV history (`md-amfi-nav`), NSE's security-wise archive and quote page
  download (`md-nse-security`), NSE Indices' total return and historical index
  reports (`md-nse-indices`), MCX spot prices (`md-mcx-spot`), iShares fund
  downloads (`md-ishares`), Tiingo (`md-tiingo`), WSJ historical prices
  (`md-wsj`), MSCI index levels (`md-msci`), Nasdaq index history
  (`md-nasdaq`), Yahoo Finance in both the yfinance and classic shapes
  (`md-yahoo`), and the SPDR Gold Shares archive (`md-spdr-gold`). Each is
  recognised from its content alone; the filename only fills in what a
  publisher leaves out of the file, such as a ticker.
- **`PriceSeries`:** what a market data file parses into, as a new
  `Parsed::Series`. One superset row holds every value any of the eleven
  publishes -- traded price and its adjusted twin, NAV and adjusted NAV, total
  return and net total return levels, and Tiingo-style `dividend` and
  `splitFactor` on the day they happen -- and a file fills the fields it
  prints. A file with several values a day fills several fields of one row
  (NSE's total and net total return; SPDR's close and NAV). Everything else a
  publisher prints is kept under its own heading in `extra`. A value that was
  not printed is absent, never zero. The series says which field is its
  `headline`, and `splitAdjusted` says whether its closes already reflect
  later splits, which differs by source: Yahoo's do, Tiingo's do not. Each
  parse is checked for rows outside the range the file declares, two
  different values for one day, values that are not positive, and -- for
  iShares US funds -- every distribution on the Distributions sheet being found
  on its ex-date. ReBIT has no form for a price history (`schema_unsupported`).
- **One file, one series.** A history downloaded in pieces comes back as one
  series per piece; stitching pieces together is computation over parsed data
  and is left to the caller. Against the publishers' own downloads, every one
  of 127 is recognised with and without its name, and every NSE index piece
  matches the full history it was cut from on all 19,409 dates compared.
- **Personal and public areas:** every format, `FormatInfo` and parse envelope
  now says `area` -- `personal` for a statement somebody holds, `public` for a
  rate sheet or a price history.
- **Bindings:** `seriesCoverage` / `series_coverage` (first and last date,
  rows, and gaps of one parsed series) and `seriesCsv` / `series_csv` (the
  series in Tiingo's column layout). Both take the `data` a parse returned, so
  the file is not read twice.
- **CLI:** `xfina parse --csv` writes a series as CSV and prints its coverage;
  `xfina formats --area personal|public` lists one area.
- **Web:** a Public data tab beside Personal statements. One drop zone feeds
  both. Public files are grouped by dataset with each file's coverage, gaps and
  checks, a preview of its newest rows, and CSV or JSON downloads. SBI rate
  sheets move here.

### Changed

- **Breaking — Rust.** `Parsed` gains `Series` and `Category` gains
  `MarketData`, so an exhaustive `match` on either needs a new arm. JSON
  consumers see only additions: a new category value and an `area` field.

## [0.8.1] - 2026-10-02

### Fixed

- **SBI forex card rates:** sheets refused as `Could not place the column
  headings` now read. In the archive that is 18 files, 15 distinct sheets from
  July to October 2024. They are set in a font whose glyph widths this reader
  cannot measure, so every run of text came out about twice its real width,
  headings landed on top of each other and some adjacent figures fused into
  one. Text is now also cut wherever the PDF moves the pen to place a new piece
  of text, and when the headings still cannot be placed by position, headings
  and figures are matched in the order the sheet wrote them, under the same
  rule as before: every row must account for every heading exactly once. These
  sheets carry `figuresMatchedByOrder`. Every sheet that already read is
  unchanged, across all 1,838 archived sheets.
- **SBI forex card rates:** a PDF with no text layer at all is refused as
  `No text layer` rather than as `Not an SBI forex card rate sheet`. One
  archived sheet was published that way, with every letter drawn as a filled
  outline rather than set in a font, and the message says it cannot be read
  without OCR, which xfina does not do.

## [0.8.0] - 2026-10-02

### Fixed

- **Dates are IST everywhere, including dates with no time.** A date-only value
  was serialized at midnight UTC while the timestamps beside it were midnight
  IST, so one document carried two conventions 5h30m apart. Now both are
  midnight IST. Consequences, all corrected: the `rebit` export printed the
  previous day for every date-only field (1,082 values across the test corpus),
  because it formatted them in UTC; the HDFC and ICICI card parsers derived a
  transaction's value date and the statement period from the UTC day, putting
  anything before 05:30 IST a day early (222 values); and the ICICI and SBI
  bank parsers stamped transactions at midnight UTC, i.e. 05:30 IST, where
  every other parser used midnight IST.
- **Web:** dates are formatted in `Asia/Kolkata` rather than UTC, which was
  only correct while the two conventions existed.

### Changed

- **`xfina` schema:** every date-only epoch moves 19,800s earlier (midnight UTC
  to midnight IST) — the same calendar day read in IST, but a different number.
  Consumers comparing these epochs against timestamps no longer need to account
  for the gap.


### Added

- **SBI forex card rates:** a new `rt-` family for published reference documents,
  reading State Bank of India's daily forex card rate sheet (`rt-sbi-forex-card`).
  Every currency the sheet quotes is carried, with every rate column named as the
  sheet prints it — the same column has been headed `TC`, `FTC`, `FOREIGN TRAVEL
  CARD` and `FOREX TRAVEL CARD` over the years, and which it was is part of what
  that day quoted. Columns are located by their headings rather than by counting
  along a row, because the column order has changed twice. A rate the sheet leaves
  unquoted is printed as a zero and is reported as absent, never as a price of
  zero. The sheet's own date is used, never the clock or the filename; where the
  printed date is a real day read either way round, the file's creation stamp
  settles it, and a sheet that nothing can date is refused. Where the rows do not
  line up under any heading, the figures are matched to the headings by the order
  both are printed in, and the sheet carries `figuresMatchedByOrder` so a caller
  can tell it from the rest; that is only reached when every row accounts for
  every heading exactly once. Twenty-one sheets of the 1,828 published so far are
  refused outright: this reader cannot measure the glyph widths of the font they
  are set in, so the positions it computes drift until separate headings land on
  top of each other and the columns cannot be placed at all. Those sheets are
  sound — other readers have no trouble with them — and the limitation is ours.
- **Reference documents:** a `reference_rates` category and a `Parsed` enum, so a
  format can produce something other than an account. The registry stays the one
  table that defines what xfina can read.
- **Web:** a rate sheet gets its own view — the day it was published, and a table
  of every currency against the columns that day quoted, headed as the sheet
  headed them. A column the sheet left unquoted reads as a dash rather than a
  zero and a currency quoted per hundred units says so beside its name. Without
  this a
  rate sheet parsed and then belonged to no heading, so it disappeared from the
  page instead of being shown.
- **Test data:** fixtures for five layout eras in `../xfina-test-data/`, and two
  parity checks over the whole published archive — one against the USD series a
  previous implementation derived, one against every rate of every currency as
  read by an unrelated implementation (`sahilgupta/sbi-fx-ratekeeper`). All skip
  when their inputs are not checked out, as every other parser's tests do.

### Changed

- **Breaking — `Statement::to_json` and `to_json_string` return a `Result`, and
  `Statement::account` is now `Statement::data`.** ReBIT describes accounts a
  person holds and has no term for a price an institution published, so asking
  for a rate sheet in `Schema::Rebit` is a named error (`schema_unsupported`)
  rather than an envelope with an empty `data`. Callers reading `statement.account`
  should use `statement.data.account()`, which returns `None` for a document that
  is not an account.

- The repository moved to the `xfina-dev` GitHub organisation. The crate, Python and npm
  metadata, the docs and the web app's GitHub links now point there.

## [0.7.0] - 2026-09-18

### Added

- **ICICI credit cards:** the older monthly CSV export (`"Date","Sr.No.","Transaction Details",...`) is read alongside the workbook. It prints only the card holder, card number and transactions, so the statement period is derived from the transaction dates, the reference number becomes `txnId`, and there are no declared figures to validate against. The date-range CSV served from the same page is declined: it spans many billing cycles and overlaps the monthly files.
- **Web:** the Summary validation badge is grey, with "Totals not printed" on hover, when a statement ran no summary checks, instead of green as if they had passed.

## [0.6.1] - 2026-09-13

### Added

- **HDFC credit cards:** reward point checks — balance (declared) and earned vs. transaction + bonus points (derived) — and `rewardPointsSummary.earnedUnaccounted` for points the statement does not itemise.
- **Web:** Rewards Summary shows unaccounted points under Earned and reversals under Adjusted / Lapsed.

### Fixed

- **HDFC credit cards:** reward reversals (`- 12`) parse as negative points instead of none.
- **Web:** the validation Summary indicator is amber, not red, when only derived checks fail; Disbursed shows as a plain figure.

## [0.6.0] - 2026-09-12

### Changed

- **Breaking — HDFC credit cards are read from the Excel export, not the CSV.**
  Every other spreadsheet statement xfina reads is Excel, and HDFC hands out the
  same billed statement in either format, so there is now one layout to follow
  instead of two. A `~|~`-separated CSV is no longer recognised; download the
  statement again with Format: Excel. Both workbook templates HDFC serves are
  read: the current one, and the older one earlier statements come in, which
  has no transaction times, reward points column or AAN. The masked card number
  is reported as the workbook prints it.
- **Web:** imported statements are listed oldest first by period, and each card shows its period.

## [0.5.0] - 2026-09-11

### Added

- **One entry point that works out what a file is.** `xfina::parse` takes bytes
  and a filename and returns the parsed account together with the format it
  detected, the category, the institution and the evidence. `detect` answers the
  same question without parsing, and `formats` lists what a build can read.
  Closes #64 and #65.
- **`xfina detect` and `xfina formats`** on the CLI.
- **`generatedDateDerived`** on all four account extensions, set when the
  statement date came from the filename or the file's modification time rather
  than the statement itself. The web UI has rendered this as an "est." badge
  since it was written; nothing had ever set it.
- `modified_timestamp` is now the last resort for a statement's generated date.
  It was accepted by every surface and read by nothing.

### Changed

- **Docs:** `versions.json` is now a single ordered `series` list — released versions newest first, the unreleased build last — instead of a list plus separate `latest` and `unreleased` keys repeating parts of it. Each entry records the `commit` its directory was built from, and the web app reads the badge from there, falling back to the hash Vite baked in for local development. A wrong commit id is now corrected in one place, without rebuilding the site. The unreleased entry is keyed `minor: "unreleased"` — the same string the version dropdown already selects by — rather than carrying a flag that would restate what its key already says.

- **Breaking — bindings.** The ten per-parser exports are gone from both the
  WASM and Python packages, replaced by `parse`, `detect`, `formats` and
  `version`. The WASM functions take an options object and return a plain
  object; failures arrive as `{ error: { kind, ... } }` rather than a thrown
  string, so a caller can tell "needs a password" from "cannot read this".
  Python raises `XfinaParseError`, carrying the same information as attributes.
- **Breaking — web.** The category and institution pickers are gone. One drop
  zone takes any number of statements at once; each is read on its own and
  appears as a card under a heading for its kind of account, saying which
  account it is and whose name is on it. Files that need a password collect in
  an "Action required" section, where each is unlocked individually or
  dropped. The supported formats are listed from `formats()` rather than
  hardcoded, so the list cannot drift from what the build actually reads --
  each with the extension the institution actually uses, whether it arrives
  locked, a link to where it is downloaded from and the trail to reach it.
- **Breaking — CLI.** `xfina parse <category> <institution> <file>` is now
  `xfina parse <FILE> [--as <format>]`, and `-f/--format` is `--schema`, which
  frees "format" to mean the parser rather than the output shape.
- **Breaking — Rust.** `ParseRequest` gains a `format` field. The ten per-format
  functions are unchanged and still public.
- Wrong-format failures return `InvalidFormat` instead of `ParseError`, so a
  file in the wrong container is distinguishable from a damaged one.
- `XfinaError` gains `kind()`, `UnrecognizedFormat` and `FormatNotEnabled`.
- The SBI PDF path reports a missing or wrong password as `PasswordRequired` /
  `IncorrectPassword` rather than a `ParseError` string.
- A statement's generated date now prefers what the institution printed over
  what the filename says. This changes SBI's precedence, which was inverted.

### Fixed

- **Web App / Docs:** The commit badge in the header linked to a commit that does not exist on every published site except `/unreleased/`. The hash was baked in by Vite from whatever `HEAD` the deploy ran on, and for the tagged builds that was a pre-squash branch commit the merge discarded — `/0.4/` and the root mirror pointed at `a8a9d75`. The tagged deploy now resolves the tag itself (`git rev-list -n 1 v<version>`), which names a commit that survives the merge, and passes it to the build.

- **Parsers no longer claim files they cannot read.** IBKR returned a populated
  account with placeholder values for arbitrary text; HDFC credit cards, SBI and
  CAMS did the same for content they did not recognise. Each now checks for its
  own structural marker first.
- Each input is decoded once however many parsers examine it. The two copies of
  the PDF spatial extractor are now one.

### Internal

- `Schema { Xfina, Rebit }` and one serializer replace twelve copies of the same
  envelope-building helper.
- `chrono` and `rust_decimal` are no longer optional; `src/models` always needed
  them, so `--no-default-features` never compiled.
- CI builds the empty feature set and each parser feature alone.

## [0.4.1] - 2026-09-02

### Fixed
- **Parsers (ICICI BA):** ICICI shuffles a day's rows relative to the balance column, so a strict running-balance check failed on rows that were not themselves wrong — an inward remittance printed after the debit it funded, for instance. Each day is now put back into the order its printed balances describe before any balance is derived from row order. Closes #53.

  Every row implies the balance it started from (`printed - delta`), which makes the day a walk that uses each row exactly once — an Eulerian path, found in O(n) with Hierholzer's algorithm rather than by trying permutations. A day is rewritten only when a walk consumes all of its rows, and rows never move across dates, so the outcome is either a day that chains exactly or the statement's own order untouched. When a day is reordered, `transactions.xfina.reordered` says so and the web app labels the transaction list.

  Across the ICICI corpus this turns 2 statements from `warning` into `passed` (2 rows moved in each) and leaves the other 4 byte-identical; in every case the set of transactions and both balances are unchanged.
- **Privacy:** Parser comments carried sample rows copied verbatim from real statements — an account number and holder name in the ICICI bank parser, an account number and balances in the Axis bank parser, and a real payment amount in the Axis credit card parser. All replaced with invented values of the same shape. This repository is public; the statements it is developed against are not.

### Added
- **Docs:** `AGENTS.md` opens with a top-priority rule against putting anything derived from a real statement into this repository or its commits, PRs and issues, with the substitutions to use instead. `CONTRIBUTING.md` points at it from the existing "do not commit test data" note.

## [0.4.0] - 2026-09-02

### Added
- **Parsers (Axis CC):** The card variant printed in the statement title (e.g. `Neo`) is now extracted into `summary.xfina.cardProduct`.
- **Web App:** Credit card statement details now show the card variant as `Product`, mirroring the bank account view.

### Fixed
- **Python wheels:** the PyPI job built a single wheel on `ubuntu-latest` with
  CPython 3.11, so only that exact combination could `pip install xfina`
  without a Rust toolchain — 0.2.4 shipped nothing but a
  `manylinux_2_34_x86_64` CPython 3.11 wheel. Fixed on three fronts, mirroring
  [xfina-dev/xfingine](https://github.com/xfina-dev/xfingine):
  - The extension now builds against the **stable ABI** (`pyo3/abi3-py38`), so
    one wheel per OS/arch covers CPython 3.8+ instead of needing one per
    version — turning a ~25-build matrix into 5.
  - Wheels are built for **linux x86_64 / aarch64, macOS x86_64 / arm64, and
    windows x64** via `PyO3/maturin-action`, which cross-compiles properly. A
    plain `maturin build` only ever targets the runner's own platform, which
    was the root cause. Both macOS wheels build on `macos-latest` (Apple
    Silicon), since GitHub has retired the `macos-13` Intel runner.
  - An **sdist** is built and uploaded as its own job, giving pip a source
    fallback on any platform without a prebuilt wheel.
- **Python metadata:** `pyproject.toml` advertised PyPy support that an abi3
  CPython extension cannot provide. Replaced with explicit CPython 3.8–3.13
  classifiers, so the metadata matches what is actually shipped.
- **CI:** bumped `actions/checkout` and `actions/setup-node` to v7 to move off
  the deprecated Node 20 runtime; the new wheel jobs use `actions/upload-artifact`
  v7 and `actions/download-artifact` v8. The `actions/setup-python` call site is
  gone — `maturin-action` provides the interpreter.
- **Parsers (HDFC BA):** Holder names starting with `MRS` were mangled into `S <name>` because the honorific was stripped by a plain substring replace. Honorifics are now removed as whole leading tokens.
- **Parsers (SBI BA):** The honorific (`Mrs.`, `Mr.` ...) is now stripped from the holder name instead of being kept as part of it, and the column padding statements use is collapsed.

## [0.3.0] - 2026-08-27

### Added
- **Parsers:** Added a new parser for Axis Bank credit card statements (`credit_cards/axis.rs`).
- **Web App:** Integrated the Axis Bank credit card parser in the UI.
- **Python / WASM:** Exposed the `parse_axis_cc` function in Python and WASM bindings.

## [0.2.4] - 2026-08-15

### Fixed
- **Parsers (CAMS):** Fixed summary-level validation false failures on statements where CAMS' vertical document-generation watermark (rotated 90°, stamped along the page margin) coincidentally landed within the y-tolerance of an AMC heading line, fusing a stray character onto it and causing that AMC's holdings to be silently misattributed to the previous AMC. Non-upright glyphs are now dropped during character extraction so they can never fuse onto content lines, regardless of which line they happen to land near.
- **Parsers (CAMS):** Fixed nondeterministic ordering of `summary_level` validation checks (backed by a `HashMap`, whose iteration order is randomized per process) by sorting by AMC name before emitting checks, so serialized output is stable across runs — the same class of issue already fixed for the IBKR parser in 0.2.0.
- **Error Handling:** CAMS PDF decryption failures now surface as the typed `XfinaError::IncorrectPassword` / `XfinaError::PasswordRequired` variants instead of a generic string-wrapped `ParseError`.

### Changed
- **Testing:** The CAMS integration test's `passwords.json` lookup now accepts either a single password or an ordered list of candidate passwords per key (including `default`), trying each in turn until one succeeds. Supports CAMS rotating its PDF password over time without needing per-file or date-based entries.

## [0.2.3] - 2026-08-09

### Added
- **Web App:** Added a privacy-first Analytics Consent modal with 3 tracking levels (Off, Page View, Parser Usage).

### Fixed
- **CI/CD:** Fixed publish workflow skipping jobs on tag push due to missing remote tracking branch in GitHub Actions checkout.

## [0.2.2] - 2026-08-08

### Added
- **xtask:** Split release command into `prepare-release` and `tag-release` stages to support PR-based branch protection workflows.

### Changed
- Add `homepage` to package metadata pointing to `xfina.dev`
- Fix page title to just `Xfina`

## [0.2.1] - 2026-08-08

### Added
- **Parsers:** Added derived `computed_closing_balance` validation logic for Bank of Baroda statements.
- **Parsers:** Added `overall_invested_match` and `overall_value_match` summary checks for Mutual Funds (CAS) by extracting portfolio summaries.

### Changed
- **Web App:** Standardized the UI header components across all statement types and added visual validation badges.

### Fixed
- **Web App:** Corrected UI alignment issues for IBKR transaction badges and properly greyed out non-applicable row-level validations for Credit Cards.

## [0.2.0] - 2026-08-06

### Added
- **Validation Engine:** Added a comprehensive two-level validation engine (`src/models/validation.rs`) to detect parsing discrepancies.
  - Row-level validation checks `opening balance + transaction amount = current balance`.
  - Summary-level validation checks `computed_closing = declared_closing` and verifies total credits/debits against declared summaries in PDFs/XLS files.
- **CI/CD:** Added GitHub Actions test pipeline (`.github/workflows/test.yml`) to automatically run `cargo test` and compile the WASM build on pushes and pull requests to `main`.
- **Documentation:** Created a comprehensive `CONTRIBUTING.md` guide for adding new parsers and managing snapshots.
- **Documentation:** Added an architectural diagram to the main `README.md` and added rich metadata (keywords, categories, readme) to `Cargo.toml`.

### Changed
- **Breaking API Change:** All parsers across all crates (`bank_accounts`, `credit_cards`, `mutual_funds`, `intl_stocks`) now return a wrapped `ParseResult<T>` struct containing the parsed `data: T` alongside a `validation: ValidationReport` object, instead of returning the raw account `T` directly.
- **WASM / Python / CLI Output:** The output JSON schema is now wrapped in `{ "data": { ... }, "validation": { ... } }`.
- **Error Handling:** Completely re-architected error handling across all parsers using the `thiserror` crate. Parsers now return a strongly-typed `XfinaError` enum instead of stringly-typed errors, enabling programmatic error matching.
- **Bindings:** Updated FFI boundaries in `xfina-wasm` (JS) and `xfina-py` (Python) to properly propagate `XfinaError` types.
- **Web App:** Updated the website header to explicitly link to `sakthipriyan.com/building-wealth` instead of linking generically to GitHub.

### Fixed
- **Code Cleanup:** Resolved hundreds of compiler and clippy warnings across the workspace, including unused variables, non-idiomatic default struct reassignments, and dead code.
- **Testing:** Resolved integration test failures in CI by ensuring snapshot write/assertion tests are skipped via a `GITHUB_ACTIONS=true` environment check.
- **Testing:** Replaced `HashMap` with `BTreeMap` and `HashSet` with `BTreeSet` in the IBKR parser to ensure deterministic serialization order for consistent snapshot tests.
- **Code Cleanup:** Removed legacy, unused `f64`-based financial models (`Portfolio`, `Asset`, etc.) from `src/models/mod.rs`.
- **Documentation:** Fixed an inaccuracy in `wasm/README.md` to correctly state that the default parser output format is `"xfina"`, not `"rebit"`.


## [0.1.4] - 2026-08-05

### Added
- **Documentation:** Added package registry badges (Crates.io, PyPI, npm) to the `README.md`.
- **Documentation:** Updated deployment instructions to reflect the new `xtask deploy-site` flow.

### Fixed
- **CI/CD:** Updated the release script in `xtask` to ensure `Cargo.lock` is correctly synced and to safely allow dirty publishing.

## [0.1.3] - 2026-08-04

### Added
- **Deployment:** Re-architected website deployment pipeline via `cargo xtask deploy-site` to generate immutable, permanent archives for all minor releases in `gh-pages`.
- **Infrastructure:** Updated `.github/workflows/publish.yml` to trigger orchestrated GitHub Actions deployments and handle safe concurrency locking.

### Changed
- **Web App:** Configured Vite with `base: './'` for path-agnostic artifact generation.
- **Web App:** Transitioned from fetching version lists via GitHub API to a dynamically generated `versions.json` registry.

### Fixed
- **Web App:** Fixed Issue #23 where the version dropdown failed to properly display or navigate to the latest active version.

## [0.1.2] - 2026-08-03

## [0.1.1] - 2026-08-03

### Added
- **Parsers:** Added a new `format` parameter (`"rebit"` or `"xfina"`) to all WASM, Python, and CLI parsers to allow toggling between strict ReBIT AA schema compliance and extended Xfina schemas.
- **Python:** Fully implemented PyO3 bindings for all statement parsers in the `xfina` PyPI package (the previous release accidentally omitted them).
- **CLI:** Added the `--format` option to the CLI tool.

### Fixed
- **CI/CD:** Upgraded NPM in GitHub Actions to v11+ to fully support passwordless OIDC Trusted Publishing, fixing the `ENEEDAUTH` error.
- **Documentation:** Updated all READMEs (Rust, WASM, Python) with correct function signatures, new format parameters, and accurate code examples.
## [0.1.0] - 2026-08-03

### Added
- **Parsers:** Support for parsing PDF/XLS statements from major Indian financial institutions:
  - Bank Accounts: HDFC, ICICI, SBI, Bank of Baroda, Axis Bank
  - Credit Cards: HDFC, ICICI
  - Mutual Funds: CAMS (CAS)
  - International Stocks: Interactive Brokers (IBKR)
- **Data Models:** Centralized `xfina-models` package standardizing financial schema based on RBI Account Aggregator specifications.
- **CLI Tool:** Unified `xfina-cli` binary for parsing statements directly from the terminal and exporting to JSON.
- **Language Bindings:** 
  - `python`: Python bindings published to PyPI using PyO3/Maturin.
  - `wasm`: WebAssembly module (`xfina-wasm`) published to NPM using `wasm-pack` for browser integration.
- **Web App:** Vue 3 + Tailwind CSS frontend interface demonstrating local, privacy-preserving WASM parsing.
- **CI/CD:** Automated GitHub Actions pipeline for testing and publishing to Crates.io, NPM, PyPI, and GitHub Pages.

