//! What a filename suggests -- and only suggests.
//!
//! Downloaded statements arrive with names their institution chose, and those
//! names are strongly diagnostic. A hint reorders the candidates so the likely
//! parser is asked first, turning detection from "probe all ten" into "usually
//! probe one". It never decides: content is always the authority, so a renamed
//! file still lands on the right parser, just after a little more work.

use crate::detect::Format;

/// Patterns institutions use for the files they hand out.
///
/// Matched case-insensitively against the filename only. Deliberately loose:
/// a hint that misses costs one extra probe, while a hint that fires wrongly
/// costs nothing at all, because the probe still has to agree.
///
/// The exception is a file that cannot be opened. Nothing can be read out of
/// an encrypted PDF, so for SBI and CAMS the filename is the only thing that
/// can name the institution behind a password prompt -- the difference between
/// "this looks like an SBI statement" and "this file is password protected".
const PATTERNS: &[(&str, Format)] = &[
    ("acct_statement", Format::BankHdfc),
    ("optransactionhistoryux", Format::BankBob),
    ("optransactionhistory", Format::BankIcici),
    ("accountstatement", Format::BankSbi),
    ("billedstatements", Format::CardHdfc),
    ("ccstatement", Format::CardIcici),
    ("cc_statement", Format::CardAxis),
    ("cas_", Format::MutualFundsCams),
    ("cams", Format::MutualFundsCams),
    ("axis bank statement", Format::BankAxis),
    ("forex_card_rates", Format::RatesSbiForexCard),
    // Market data. AMFI's own name is NAV_<from>_to_<to>.xlsx; NSE's archive
    // ends in -EQ-N.csv; the bookmarklets name Tiingo and WSJ files
    // <TICKER>_<from>_to_<to>.<source>.csv after the fact.
    ("nav_", Format::MarketAmfiNav),
    ("-eq-n.csv", Format::MarketNseSecurity),
    ("quote-equity-", Format::MarketNseSecurity),
    ("_historical_tr_", Format::MarketNseIndices),
    ("_historical_pr_", Format::MarketNseIndices),
    ("spotmarket", Format::MarketMcxSpot),
    ("_fund.xls", Format::MarketIshares),
    (".tiingo.", Format::MarketTiingo),
    (".wsj.", Format::MarketWsj),
    ("historicalprices", Format::MarketWsj),
    ("msci", Format::MarketMsci),
    ("eodhist_", Format::MarketNasdaq),
    (".yfinance.", Format::MarketYahoo),
    ("gld_archive", Format::MarketSpdrGold),
];

/// IBKR names its activity statements after the account and the period:
/// `U<digits>_<yyyymmdd>_<yyyymmdd>.csv`. There is no fixed word to look for,
/// so this one is recognised by shape.
fn looks_like_ibkr(name: &str) -> bool {
    let stem = name.split('/').next_back().unwrap_or(name);
    let mut parts = stem.trim_end_matches(".csv").split('_');
    let Some(account) = parts.next() else {
        return false;
    };
    if !account.starts_with('u')
        || account.len() < 2
        || !account[1..].chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }
    let dates: Vec<&str> = parts.collect();
    dates.len() == 2
        && dates
            .iter()
            .all(|d| d.len() == 8 && d.chars().all(|c| c.is_ascii_digit()))
}

/// The format a filename points at, if any.
pub fn from_filename(filename: Option<&str>) -> Option<Format> {
    let name = filename?.to_lowercase();
    if looks_like_ibkr(&name) {
        return Some(Format::EquityIbkr);
    }
    // Longest pattern first, so "optransactionhistoryux" (Bank of Baroda) is
    // not shadowed by the ICICI prefix it happens to start with.
    PATTERNS
        .iter()
        .filter(|(pattern, _)| name.contains(pattern))
        .max_by_key(|(pattern, _)| pattern.len())
        .map(|(_, format)| *format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_names_institutions_hand_out() {
        // Shapes only -- the digits an institution embeds are stand-ins.
        assert_eq!(
            from_filename(Some("Acct_Statement_XXXXXXXX0000_01012026.xls")),
            Some(Format::BankHdfc)
        );
        assert_eq!(
            from_filename(Some("CCStatementCurrent01-01-2026.xls")),
            Some(Format::CardIcici)
        );
        assert_eq!(
            from_filename(Some("CAS_01012026-31012026_CP0000000_1.pdf")),
            Some(Format::MutualFundsCams)
        );
    }

    #[test]
    fn the_longer_pattern_wins_when_one_contains_the_other() {
        // Bank of Baroda's name starts with ICICI's; without the length rule
        // every BOB download would be hinted as ICICI.
        assert_eq!(
            from_filename(Some("OpTransactionHistoryUX501-01-2026.xls")),
            Some(Format::BankBob)
        );
        assert_eq!(
            from_filename(Some("OpTransactionHistory01-01-2026.xls")),
            Some(Format::BankIcici)
        );
    }

    #[test]
    fn recognises_the_two_encrypted_formats() {
        // These are the ones that cannot be probed until a password opens
        // them, so the filename is all a password prompt has to go on. Digits
        // below are a date and a time, invented.
        assert_eq!(
            from_filename(Some("AccountStatement_01012026_120000.pdf")),
            Some(Format::BankSbi)
        );
        assert_eq!(
            from_filename(Some("CAS_01012026-31012026_CP0000000_1.pdf")),
            Some(Format::MutualFundsCams)
        );
    }

    #[test]
    fn the_two_card_statement_patterns_do_not_collide() {
        // "ccstatement" and "cc_statement" differ by one underscore and belong
        // to different issuers.
        assert_eq!(
            from_filename(Some("CC_Statement_2026_01_31.xlsx")),
            Some(Format::CardAxis)
        );
        assert_eq!(
            from_filename(Some("CCStatementCurrent01-01-2026.xls")),
            Some(Format::CardIcici)
        );
    }

    #[test]
    fn recognises_an_ibkr_name_by_its_shape() {
        // Account digits below are invented.
        assert_eq!(
            from_filename(Some("U0000000_20260101_20260131.csv")),
            Some(Format::EquityIbkr)
        );
        // A name of the same shape but without the account prefix is not one.
        assert_eq!(from_filename(Some("X0000000_20260101_20260131.csv")), None);
    }

    #[test]
    fn every_format_can_be_hinted_at() {
        // A format with no pattern is invisible to a password prompt, which is
        // how SBI ended up saying "this file is password protected" where CAMS
        // said "this looks like a CAMS statement". Adding a parser without a
        // filename pattern should be a deliberate choice, not an oversight.
        let hinted: std::collections::HashSet<Format> =
            PATTERNS.iter().map(|(_, format)| *format).collect();
        let missing: Vec<&str> = Format::ALL
            .iter()
            // IBKR names its downloads by shape rather than by any fixed word,
            // so it is recognised by `looks_like_ibkr` instead of a pattern.
            .filter(|f| **f != Format::EquityIbkr && !hinted.contains(f))
            .map(|f| f.id())
            .collect();
        assert!(
            missing.is_empty(),
            "these formats have no filename pattern: {}",
            missing.join(", ")
        );
    }

    #[test]
    fn an_unrecognised_or_absent_name_hints_nothing() {
        assert_eq!(from_filename(Some("statement.xls")), None);
        assert_eq!(from_filename(None), None);
    }
}
