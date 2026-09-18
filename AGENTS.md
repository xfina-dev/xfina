# Xfina - Agent Context & Guidelines

This document serves as a cheat sheet for AI agents working on the Xfina project to quickly understand the architecture, build processes, and specific domain rules.

## TOP PRIORITY: Never publish real statement data

This repository is **public**. The test data it is developed against is real financial
statements belonging to real people, and it lives in a **private** sibling repository
(`../xfina-test-data/`). Nothing derived from those statements may cross into anything
public. This outranks every other instruction here: if following another guideline would
put real data in a public place, do not follow it.

**Never** put any of the following into source, comments, doc examples, test fixtures
committed here, `CHANGELOG.md`, commit messages, PR titles or bodies, PR or issue
comments, or screenshots:

- Account holder names, joint holder names, nominee names — including your own
- Account, card, customer ID, PAN, CKYC, MICR or cheque numbers, masked or not
  (a masked number still identifies an account by the last four digits it keeps)
- Real balances, transaction amounts, credit limits or dues — a real figure is
  identifying even without a name attached
- Email addresses, UPI handles, phone numbers, postal addresses
- Transaction narrations copied from a statement (`UPI/...`, `NEFT-...`, `RTGS-...`),
  which carry counterparty names and reference numbers
- Statement filenames that embed account digits or a holder's name

**Instead**, invent values that cannot belong to anyone. Keep the *shape* — that is what
a parser comment or a bug report needs — and throw away the content:

```rust
// Format: "000011112222 ( INR )  - <ACCOUNT HOLDER>"
// ["1", "05-Jun-2026", "05-Jun-2026", "", "NEFT-...", "0.00", "1000.00", "5000.00"]
```

Describe a bug structurally rather than by quoting the statement: *"a name prefixed `MRS`
lost the honorific and came out as `S <name>`"*, or *"an outward remittance printed ahead
of the credit funding it"*. Report counts and outcomes — "2 of 6 statements went from
warning to passed" — never the amounts that made it fail.

**Before every commit, PR, or comment**, re-read what you are about to publish and check
it for the list above. `git commit --amend` and editing a PR body are cheap; a merged
commit message is permanent, and a published tag is worse.

## Architecture Overview
- **Backend (Rust + WASM):** Financial statements (PDFs, XLS, etc.) are parsed securely entirely within the browser using Rust compiled to WebAssembly. Parsers are split into domain-specific packages (`bank-accounts/`, `credit-cards/`, `mutual-funds/`, `intl-stocks/`).
- **Frontend (Vue 3 + Vite):** The user interface is built with Vue 3, Tailwind CSS, and `shadcn-vue` style components. It takes the JSON output from the WASM parsers and renders standardized views.
- **Models (`xfina-models`):** Shared Rust data structures used to serialize data. They adhere closely to the Sahamati Account Aggregator (AA) schema standards, with our own project-specific extensions nested inside `xfina` objects (e.g. `XfinaCreditCardAccount`).

## Build & Run Instructions
- **WASM Rebuild (CRITICAL):** Anytime a Rust parser or model is modified, the WASM package MUST be rebuilt and copied to the web directory for the frontend to see the changes. 
  ```bash
  cd wasm
  wasm-pack build --target web && cp -r pkg/* ../web/src/wasm/
  ```
  *(Note: Vite aggressively caches WASM files. If UI doesn't update after rebuild, wipe `web/node_modules/.vite` and hard-refresh the browser).*
- **Frontend Dev Server:**
  ```bash
  cd web
  npm run dev
  ```

## Testing Workflow
- **Integration Tests:** Each parser has an `integration.rs` which reads sample raw statements and asserts them against expected JSON snapshots.
- **Test Data Repo:** Test data lives OUTSIDE this repository in a sibling folder at `../xfina-test-data/`.
- **Updating Snapshots:** If a parser modification intentionally changes the JSON output shape, run tests with the `UPDATE_EXPECTED=1` environment variable to overwrite the expected snapshots.

## Technical Rules & Conventions
1. **Timezones & Dates:** 
   - Use `NaiveDate` (from `chrono`) for fields that are strictly dates (e.g. statement start/end dates).
   - Use `DateTime<Utc>` for timestamps (e.g. transaction exact times). When parsing local Indian dates from statements, parse them using `Asia/Kolkata` (IST `+05:30`) timezone offset before converting to `Utc`.
   - **Everything is IST, including dates with no time.** Go through
     `models::date_utils`: `ist_to_utc` for a local timestamp, `ist_midnight`
     for a date that has to become one, and `ist_date` to read the calendar
     date back off an instant. Never build the offset by hand, and never reach
     for `DateTime::date_naive()` on a statement timestamp -- that is the UTC
     day, which is the day before for anything earlier than 05:30 IST.
   - A date-only value serializes as midnight IST in both schemas: the epoch
     of 00:00+05:30 in `xfina`, and the Indian calendar date in `rebit` (which
     is what `date_only_paths` marks). One convention, so dates and timestamps
     in the same document can be compared.
2. **Standardized Naming:**
   - Always use full institution names across parsers and UI: `"HDFC Bank"`, `"ICICI Bank"`, `"State Bank of India"`, `"Bank of Baroda"`.
3. **Transaction Sorting:** Parsed transactions should generally be emitted in chronological (ascending) order.
4. **Data Fallbacks & Safety:** Parsers should fail gracefully. Try to derive information when missing (e.g. deriving missing transaction years from the statement date).

## UI/UX Rules
- **Color & Styling:** Maintain consistency in financial coloring (green for deposits/payments, default colors for neutral balances). Use the predefined Tailwind semantic colors (e.g. `text-muted-foreground`, `text-primary`).
- Ensure UI bindings exactly match the serialized JSON paths (e.g., watch out for `camelCase` conversions like `institution_name` becoming `institutionName` in JS).
