use crate::models::credit_card::{
    CcHolder, CcHolders, CcProfile, CcSummary, CcTransaction, CcTransactions, CcXfinaSummary,
    CcXfinaTransaction, CcXfinaTransactions, CreditCardAccount, RewardPointsSummary,
    XfinaCreditCardAccount,
};
use crate::models::date_utils;
use crate::models::deposit::TransactionType;
use crate::models::validation::{ParseResult, SummaryCheck, ValidationReport};
use chrono::{DateTime, NaiveDate, Utc};
use regex::Regex;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;

use crate::decode::Decoded;
use crate::models::request::ParseRequest;

pub fn parse_icici_statement(
    input: ParseRequest<'_>,
) -> Result<ParseResult<CreditCardAccount>, crate::error::XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

/// Parses from an already-decoded input, so detection can probe and
/// parse against one read of the file.
pub(crate) fn parse_decoded(
    decoded: &Decoded<'_>,
    input: &ParseRequest<'_>,
) -> Result<ParseResult<CreditCardAccount>, crate::error::XfinaError> {
    if decoded.container() == crate::detect::Container::Text {
        return parse_csv(decoded, input);
    }
    parse_workbook(decoded, input)
}

/// The current export: a workbook with the statement summary above the
/// transaction table.
fn parse_workbook(
    decoded: &Decoded<'_>,
    input: &ParseRequest<'_>,
) -> Result<ParseResult<CreditCardAccount>, crate::error::XfinaError> {
    let filename = input.filename;
    let range = decoded.sheets()?.first()?;

    let mut stmt = CreditCardAccount {
        r#type: "credit_card".to_string(),
        version: 1.1,
        ..Default::default()
    };
    stmt.version = 1.1;

    let mut holder = CcHolder::default();
    let mut xfina_account = XfinaCreditCardAccount {
        institution_name: Some("ICICI Bank".to_string()),
        ..Default::default()
    };

    let mut date_only_paths = Vec::new();

    if let Some(fname) = filename {
        let re = Regex::new(r"(\d{2}-\d{2}-\d{4})").unwrap();
        if let Some(caps) = re.captures(fname) {
            if let Some(m) = caps.get(1) {
                if let Ok(d) = NaiveDate::parse_from_str(m.as_str(), "%d-%m-%Y") {
                    xfina_account.generated_date = Some(date_utils::ist_midnight(d));
                    date_only_paths.push("xfina.generatedDate".to_string());
                    // From the filename, not the statement.
                    xfina_account.generated_date_derived = Some(true);
                }
            }
        }
    }
    let mut summary = CcSummary::default();
    let mut xfina_summary = CcXfinaSummary::default();

    let mut transactions_list = Vec::new();
    let mut xfina_txns = CcXfinaTransactions::default();

    let mut in_transactions = false;

    let mut card_holder_name = String::new();
    let mut previous_balance = Decimal::new(0, 0);
    let mut purchases = Decimal::new(0, 0);
    let mut payments = Decimal::new(0, 0);

    let mut default_rewards = 0;
    let mut owner_credit_breakdown = HashMap::new();
    let mut owner_debit_breakdown = HashMap::new();

    for row in range.rows() {
        let cells: Vec<String> = row.iter().map(|c| c.to_string()).collect();
        if cells.is_empty() {
            continue;
        }

        let _col0 = cells.first().map(|s| s.trim()).unwrap_or("");

        if !in_transactions {
            for i in [0, 8] {
                if let (Some(key_str), Some(val_str)) = (cells.get(i), cells.get(i + 4)) {
                    let key = key_str.trim();
                    let val = val_str.replace("INR", "").trim().to_string();

                    match key {
                        "Card Holder Name" => {
                            card_holder_name = val.clone();
                            holder.name = card_holder_name.clone();
                        }
                        "Previous Balance" => {
                            previous_balance = parse_decimal(&val).unwrap_or_default();
                        }
                        "Payments and Other Credits" => {
                            payments = parse_decimal(&val).unwrap_or_default();
                        }
                        "Purchases and Other Charges" => {
                            purchases = parse_decimal(&val).unwrap_or_default();
                        }
                        "Total Amount Due" => {
                            let total_due = parse_decimal(&val).unwrap_or_default();
                            summary.total_due_amount = Some(total_due);
                        }
                        "Minimum Amount Due" => {
                            let min_due = parse_decimal(&val).unwrap_or_default();
                            summary.min_due_amount = Some(min_due);
                        }
                        "Available Credit Limit" => {
                            summary.available_credit = parse_decimal(&val);
                        }
                        "Total Credit Limit" => {
                            summary.credit_limit = parse_decimal(&val);
                        }
                        "Available Cash Limit" => {
                            // in some formats, they might map it differently, we will just use cash_limit for total
                        }
                        "Total Cash Limit" => {
                            summary.cash_limit = parse_decimal(&val);
                        }
                        "Statement Date" => {
                            summary.last_statement_date = parse_date(&val);
                        }
                        "Payment Due Date" => {
                            summary.due_date = parse_date(&val);
                        }
                        "Statement Period" => {
                            if let Some((start, end)) = val.split_once(" TO ") {
                                let txns = CcTransactions {
                                    start_date: parse_date(start),
                                    end_date: parse_date(end),
                                    ..Default::default()
                                };
                                xfina_txns.start_date_derived = Some(false);
                                xfina_txns.end_date_derived = Some(false);
                                stmt.transactions = Some(txns);
                            }
                        }
                        "Transaction Date" => {
                            in_transactions = true;
                        }
                        _ => {}
                    }
                }
            }
        } else {
            // Parse Transactions
            // 0=Date, 4=Details, 8=Amount, 12=Reward Points, 16=Ref Number
            let date_str = cells.first().map(|s| s.trim()).unwrap_or("");
            if date_str.is_empty() || date_str == "Transaction Date" {
                continue;
            } // skip empty or header

            let details = cells.get(4).map(|s| s.trim()).unwrap_or("").to_string();
            let amount_str = cells.get(8).map(|s| s.trim()).unwrap_or("").to_string();

            // Parse amount and type
            let is_credit = amount_str.ends_with("Cr.");
            let _is_debit = amount_str.ends_with("Dr.");
            let txn_type = if is_credit {
                TransactionType::Credit
            } else {
                TransactionType::Debit
            };

            let amt_clean = amount_str
                .replace("Dr.", "")
                .replace("Cr.", "")
                .replace("INR", "")
                .trim()
                .to_string();
            let amount = parse_decimal(&amt_clean).unwrap_or_default().abs();

            let reward_points = cells.get(12).and_then(|s| s.trim().parse::<i32>().ok());

            let mut tx_xfina = CcXfinaTransaction {
                owner: Some(card_holder_name.clone()),
                ..Default::default()
            };
            tx_xfina.reward_points = reward_points;

            let mut parsed_date: Option<DateTime<Utc>> = parse_datetime(date_str);
            if parsed_date.is_none() {
                if let Some(stmt_date) = summary.last_statement_date {
                    parsed_date = parse_partial_date(date_str, stmt_date);
                }
            }
            let parsed_naive = parsed_date.map(crate::models::date_utils::ist_date);
            if !date_only_paths.contains(&"transactions.transaction.txnDate".to_string()) {
                date_only_paths.push("transactions.transaction.txnDate".to_string());
                date_only_paths.push("transactions.transaction.valueDate".to_string());
            }
            transactions_list.push(CcTransaction {
                txn_date: parsed_date,
                value_date: parsed_naive,
                narration: details,
                amount,
                txn_type,
                txn_id: None,
                statement_date: None,
                mcc: None,
                masked_card_number: None,
                xfina: Some(tx_xfina),
            });

            // Aggregations
            use rust_decimal::prelude::ToPrimitive;
            let amt_f64 = amount.to_f64().unwrap_or(0.0);

            if let Some(pts) = reward_points {
                default_rewards += pts;
            }
            if txn_type == TransactionType::Credit {
                *owner_credit_breakdown
                    .entry(card_holder_name.clone())
                    .or_insert(0.0) += amt_f64;
            } else if txn_type == TransactionType::Debit {
                *owner_debit_breakdown
                    .entry(card_holder_name.clone())
                    .or_insert(0.0) += amt_f64;
            }
        }
    }

    xfina_summary.opening_balance = Some(previous_balance);
    xfina_summary.payment_credit = Some(payments);
    xfina_summary.purchases_debits = Some(purchases);
    summary.finance_charges = Some(Decimal::new(0, 0)); // not clearly separated in this ICICI extract
    xfina_summary.owner_credit_breakdown = owner_credit_breakdown;
    xfina_summary.owner_debit_breakdown = owner_debit_breakdown;

    xfina_summary.reward_points_summary = Some(RewardPointsSummary {
        default_rewards,
        opening_balance: 0,
        earned: default_rewards,
        disbursed: 0,
        adjusted_lapsed: 0,
        closing_balance: 0,
        expiring_in_30_days: None,
        expiring_in_60_days: None,
        earned_unaccounted: None,
    });

    let stmt_date_opt = summary.last_statement_date;
    summary.xfina = Some(xfina_summary);
    stmt.summary = Some(summary);

    stmt.profile = Some(CcProfile {
        holders: CcHolders {
            holder: vec![holder],
        },
    });

    transactions_list.sort_by_key(|a| a.txn_date);

    let mut txns = stmt.transactions.unwrap_or_default();

    if txns.start_date.is_none() || txns.end_date.is_none() {
        if let Some(stmt_date) = stmt_date_opt {
            let (start, end) = crate::models::date_utils::derive_statement_period(stmt_date);
            txns.start_date = Some(start);
            txns.end_date = Some(end);
            xfina_txns.start_date_derived = Some(true);
            xfina_txns.end_date_derived = Some(true);
        } else {
            if txns.start_date.is_none() {
                if let Some(first) = transactions_list.first() {
                    txns.start_date = first.txn_date.map(crate::models::date_utils::ist_date);
                    xfina_txns.start_date_derived = Some(true);
                }
            }
            if txns.end_date.is_none() {
                if let Some(last) = transactions_list.last() {
                    txns.end_date = last.txn_date.map(crate::models::date_utils::ist_date);
                    xfina_txns.end_date_derived = Some(true);
                }
            }
        }
    }

    txns.transaction = transactions_list;
    txns.xfina = Some(xfina_txns);
    stmt.transactions = Some(txns);

    if !date_only_paths.is_empty() {
        xfina_account.date_only_paths = Some(date_only_paths);
    }
    stmt.xfina = Some(xfina_account);

    let mut validation = ValidationReport::empty();

    // Level 2 - Summary Checks

    // 1. closing_balance_match: total_due == opening_balance + purchases_debits - payment_credit
    if let (Some(total_due), Some(ob), Some(pd), Some(pc)) = (
        stmt.summary.as_ref().and_then(|s| s.total_due_amount),
        stmt.summary
            .as_ref()
            .and_then(|s| s.xfina.as_ref())
            .and_then(|x| x.opening_balance),
        stmt.summary
            .as_ref()
            .and_then(|s| s.xfina.as_ref())
            .and_then(|x| x.purchases_debits),
        stmt.summary
            .as_ref()
            .and_then(|s| s.xfina.as_ref())
            .and_then(|x| x.payment_credit),
    ) {
        validation.summary_level.checks.push(SummaryCheck::declared(
            "closing_balance_match",
            total_due,
            ob + pd - pc,
            None,
        ));
    }

    // 2. txn_credits_match: payment_credit == sum(txn.amount where type == Credit)
    if let Some(pc) = stmt
        .summary
        .as_ref()
        .and_then(|s| s.xfina.as_ref())
        .and_then(|x| x.payment_credit)
    {
        use rust_decimal::prelude::FromPrimitive;
        let sum_credits: Decimal = Decimal::from_f64(
            stmt.transactions
                .as_ref()
                .unwrap()
                .transaction
                .iter()
                .filter(|t| t.txn_type == TransactionType::Credit)
                .map(|t| t.amount.to_f64().unwrap_or(0.0))
                .sum(),
        )
        .unwrap_or(Decimal::from(0));
        validation.summary_level.checks.push(SummaryCheck::declared(
            "txn_credits_match",
            pc,
            sum_credits,
            None,
        ));
    }

    // 3. txn_debits_match: purchases_debits == sum(txn.amount where type == Debit)
    if let Some(pd) = stmt
        .summary
        .as_ref()
        .and_then(|s| s.xfina.as_ref())
        .and_then(|x| x.purchases_debits)
    {
        use rust_decimal::prelude::FromPrimitive;
        let sum_debits: Decimal = Decimal::from_f64(
            stmt.transactions
                .as_ref()
                .unwrap()
                .transaction
                .iter()
                .filter(|t| t.txn_type == TransactionType::Debit)
                .map(|t| t.amount.to_f64().unwrap_or(0.0))
                .sum(),
        )
        .unwrap_or(Decimal::from(0));
        validation.summary_level.checks.push(SummaryCheck::declared(
            "txn_debits_match",
            pd,
            sum_debits,
            None,
        ));
    }

    // 4. owner_credits_match: payment_credit == sum(owner_credit_breakdown.values())
    if let Some(pc) = stmt
        .summary
        .as_ref()
        .and_then(|s| s.xfina.as_ref())
        .and_then(|x| x.payment_credit)
    {
        use rust_decimal::prelude::FromPrimitive;
        if let Some(owner_credits) = stmt
            .summary
            .as_ref()
            .and_then(|s| s.xfina.as_ref())
            .map(|x| &x.owner_credit_breakdown)
        {
            let sum_owner_credits =
                Decimal::from_f64(owner_credits.values().sum()).unwrap_or(Decimal::from(0));
            validation.summary_level.checks.push(SummaryCheck::declared(
                "owner_credits_match",
                pc,
                sum_owner_credits,
                None,
            ));
        }
    }

    // 5. owner_debits_match: purchases_debits == sum(owner_debit_breakdown.values())
    if let Some(pd) = stmt
        .summary
        .as_ref()
        .and_then(|s| s.xfina.as_ref())
        .and_then(|x| x.purchases_debits)
    {
        use rust_decimal::prelude::FromPrimitive;
        if let Some(owner_debits) = stmt
            .summary
            .as_ref()
            .and_then(|s| s.xfina.as_ref())
            .map(|x| &x.owner_debit_breakdown)
        {
            let sum_owner_debits =
                Decimal::from_f64(owner_debits.values().sum()).unwrap_or(Decimal::from(0));
            validation.summary_level.checks.push(SummaryCheck::declared(
                "owner_debits_match",
                pd,
                sum_owner_debits,
                None,
            ));
        }
    }

    validation.summary_level.passed = validation.summary_level.checks.iter().all(|c| c.passed);
    validation.finalize();

    Ok(ParseResult {
        data: stmt,
        validation,
    })
}

fn parse_decimal(val: &str) -> Option<Decimal> {
    let clean = val.replace(",", "");
    clean.parse::<Decimal>().ok()
}

fn parse_date(val: &str) -> Option<NaiveDate> {
    let iso = crate::models::parse_indian_date(val);
    let s = iso.split('T').next().unwrap_or(&iso);
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn parse_datetime(val: &str) -> Option<DateTime<Utc>> {
    let iso = crate::models::parse_indian_date(val);
    let s = iso.split('T').next().unwrap_or(&iso);
    ist_midnight(NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?)
}

fn ist_midnight(naive: NaiveDate) -> Option<DateTime<Utc>> {
    Some(date_utils::ist_midnight(naive))
}

fn parse_partial_date(val: &str, stmt_date: NaiveDate) -> Option<DateTime<Utc>> {
    let val = val.trim();
    let parts: Vec<&str> = if val.contains('/') {
        val.split('/').collect()
    } else {
        val.split('-').collect()
    };
    if parts.len() == 2 {
        let day = parts[0].trim().parse::<u32>().unwrap_or(0);
        let month_str = parts[1].trim();
        let month = if month_str.chars().all(|c| c.is_ascii_digit()) {
            month_str.parse::<u32>().unwrap_or(0)
        } else {
            match month_str.to_lowercase().as_str() {
                "jan" => 1,
                "feb" => 2,
                "mar" => 3,
                "apr" => 4,
                "may" => 5,
                "jun" => 6,
                "jul" => 7,
                "aug" => 8,
                "sep" => 9,
                "oct" => 10,
                "nov" => 11,
                "dec" => 12,
                _ => 0,
            }
        };
        if day > 0 && month > 0 {
            return Some(date_utils::derive_transaction_date(stmt_date, day, month));
        }
    }
    None
}

/// Columns of the transaction table in ICICI's older CSV export.
const CSV_HEADER: [&str; 7] = [
    "Date",
    "Sr.No.",
    "Transaction Details",
    "Reward Point Header",
    "Intl.Amount",
    "Amount(in Rs)",
    "BillingAmountSign",
];

/// Reads a CSV export's records, tolerating their varying widths.
fn csv_records(text: &str) -> Vec<Vec<String>> {
    csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes())
        .records()
        .filter_map(Result::ok)
        .map(|r| r.iter().map(|c| c.trim().to_string()).collect())
        .collect()
}

fn is_csv_header(record: &[String]) -> bool {
    record.len() == CSV_HEADER.len() && record.iter().zip(CSV_HEADER).all(|(c, h)| c == h)
}

/// A card number row, which opens that card's transactions:
/// `"0000XXXXXXXX0000"`, or `"0000 XXXX XXXX 0000"` in the date-range export.
fn csv_card_number(record: &[String]) -> Option<String> {
    let [cell] = record else { return None };
    let number: String = cell.split_whitespace().collect();
    let plausible = number.len() >= 12
        && number.contains('X')
        && number.chars().all(|c| c.is_ascii_digit() || c == 'X');
    plausible.then_some(number)
}

/// How a CSV export's transaction rows are written.
#[derive(Debug, PartialEq, Eq)]
enum CsvRows {
    /// One billing cycle: `dd/mm/yyyy` dates, an unsigned amount and a
    /// `CR`/blank sign column.
    Monthly,
    /// A date range spanning many cycles, from the same download page:
    /// `dd-MON-yy` dates, a serial in place of the reference number and a
    /// signed amount repeated where the sign should be. Nothing in it says
    /// which statement a transaction was billed on, and its range overlaps the
    /// monthly files, so it is not read.
    DateRange,
}

fn csv_row_kind(record: &[String]) -> Option<CsvRows> {
    if record.len() != CSV_HEADER.len() {
        return None;
    }
    let date = record[0].as_str();
    let sign = record[6].as_str();
    if NaiveDate::parse_from_str(date, "%d/%m/%Y").is_ok() && matches!(sign, "" | "CR" | "DR") {
        return Some(CsvRows::Monthly);
    }
    if NaiveDate::parse_from_str(date, "%d-%b-%y").is_ok() {
        return Some(CsvRows::DateRange);
    }
    None
}

fn unsupported_date_range() -> crate::error::XfinaError {
    crate::error::XfinaError::Unsupported(
        "this ICICI card CSV covers a date range rather than one statement; \
         download each monthly statement instead"
            .to_string(),
    )
}

/// ICICI's older card export: a CSV carrying the card holder and one billing
/// cycle's transactions, and nothing else.
///
/// ```text
/// "Accountno:","0000000000000000"
/// "Customer Name:","<TITLE> <CARD HOLDER>"
/// "Address:","<ADDRESS>"
///
/// "Transaction Details:"
/// "Date","Sr.No.","Transaction Details","Reward Point Header","Intl.Amount","Amount(in Rs)","BillingAmountSign"
/// "0000XXXXXXXX0000"
/// "01/01/2026","10000000000","UPI Payment Received","0","0","1000.00","CR"
/// "02/01/2026","10000000001","<MERCHANT>","20","0","400.00",""
/// ```
///
/// No statement date, period, dues or totals are printed, so the period is
/// derived from the transactions and there is nothing declared to reconcile
/// against. The account number is not the card number and is not reported.
fn parse_csv(
    decoded: &Decoded<'_>,
    _input: &ParseRequest<'_>,
) -> Result<ParseResult<CreditCardAccount>, crate::error::XfinaError> {
    use crate::models::credit_card::{CardType, CcCard, CcCards, TypeChoice};

    let text = decoded.text()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let records = csv_records(text);

    if !records.iter().any(|r| is_csv_header(r)) {
        return Err(crate::error::XfinaError::InvalidFormat(
            "not an ICICI credit card CSV: no transaction table".to_string(),
        ));
    }

    let mut holder = CcHolder::default();
    let mut cards: Vec<String> = Vec::new();
    let mut transactions_list = Vec::new();
    let mut in_transactions = false;

    for record in &records {
        if record.iter().all(|c| c.is_empty()) {
            continue;
        }
        if !in_transactions {
            match record.first().map(String::as_str) {
                Some("Customer Name:") => {
                    let name = record.get(1).map(String::as_str).unwrap_or("");
                    holder.name = crate::models::normalize_person_name(name);
                }
                Some("Address:") => {
                    let address = record.get(1).map(String::as_str).unwrap_or("");
                    let address = address.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !address.is_empty() {
                        holder.address = Some(address);
                    }
                }
                _ if is_csv_header(record) => in_transactions = true,
                _ => {}
            }
            continue;
        }

        if let Some(card) = csv_card_number(record) {
            if !cards.contains(&card) {
                cards.push(card);
            }
            continue;
        }
        // The disclaimers that close the date-range export.
        if record.len() < CSV_HEADER.len() {
            break;
        }
        match csv_row_kind(record) {
            Some(CsvRows::Monthly) => {}
            Some(CsvRows::DateRange) => return Err(unsupported_date_range()),
            None => {
                return Err(crate::error::XfinaError::ParseError(
                    "unreadable transaction row in ICICI card CSV".to_string(),
                ))
            }
        }

        let date =
            NaiveDate::parse_from_str(&record[0], "%d/%m/%Y").expect("checked by csv_row_kind");
        let amount = parse_decimal(&record[5])
            .ok_or_else(|| {
                crate::error::XfinaError::ParseError(
                    "unreadable amount in ICICI card CSV".to_string(),
                )
            })?
            .abs();
        let txn_type = if record[6] == "CR" {
            TransactionType::Credit
        } else {
            TransactionType::Debit
        };
        let reference = record[1].clone();

        transactions_list.push(CcTransaction {
            txn_date: ist_midnight(date),
            value_date: Some(date),
            narration: record[2].clone(),
            amount,
            txn_type,
            txn_id: (!reference.is_empty()).then_some(reference),
            statement_date: None,
            mcc: None,
            masked_card_number: cards.last().cloned(),
            xfina: Some(CcXfinaTransaction {
                owner: Some(holder.name.clone()),
                reward_points: record[3].parse::<i32>().ok(),
            }),
        });
    }

    transactions_list.sort_by_key(|t| t.txn_date);

    let mut payments = Decimal::ZERO;
    let mut purchases = Decimal::ZERO;
    let mut default_rewards = 0;
    for txn in &transactions_list {
        if txn.txn_type == TransactionType::Credit {
            payments += txn.amount;
        } else {
            purchases += txn.amount;
        }
        default_rewards += txn
            .xfina
            .as_ref()
            .and_then(|x| x.reward_points)
            .unwrap_or(0);
    }

    // One holder is all the export names, so every row is theirs.
    let mut owner_credit_breakdown = HashMap::new();
    let mut owner_debit_breakdown = HashMap::new();
    if !transactions_list.is_empty() {
        owner_credit_breakdown.insert(holder.name.clone(), payments.to_f64().unwrap_or(0.0));
        owner_debit_breakdown.insert(holder.name.clone(), purchases.to_f64().unwrap_or(0.0));
    }

    let xfina_summary = CcXfinaSummary {
        payment_credit: Some(payments),
        purchases_debits: Some(purchases),
        owner_credit_breakdown,
        owner_debit_breakdown,
        reward_points_summary: Some(RewardPointsSummary {
            default_rewards,
            opening_balance: 0,
            earned: default_rewards,
            disbursed: 0,
            adjusted_lapsed: 0,
            closing_balance: 0,
            expiring_in_30_days: None,
            expiring_in_60_days: None,
            earned_unaccounted: None,
        }),
        ..Default::default()
    };

    let mut stmt = CreditCardAccount {
        r#type: "credit_card".to_string(),
        version: 1.1,
        ..Default::default()
    };
    if let Some(first) = cards.first() {
        stmt.masked_acc_number = first.clone();
        holder.cards = Some(CcCards {
            card: cards
                .iter()
                .enumerate()
                .map(|(i, number)| CcCard {
                    card_type: match number.chars().next() {
                        Some('4') => CardType::Visa,
                        Some('5') => CardType::MasterCard,
                        _ => CardType::Others,
                    },
                    primary: if i == 0 {
                        TypeChoice::Yes
                    } else {
                        TypeChoice::No
                    },
                    masked_card_number: number.clone(),
                    issued_date: None,
                })
                .collect(),
        });
    }

    let xfina_txns = CcXfinaTransactions {
        start_date_derived: Some(true),
        end_date_derived: Some(true),
    };
    stmt.transactions = Some(CcTransactions {
        start_date: transactions_list.first().and_then(|t| t.value_date),
        end_date: transactions_list.last().and_then(|t| t.value_date),
        transaction: transactions_list,
        xfina: Some(xfina_txns),
    });
    stmt.summary = Some(CcSummary {
        xfina: Some(xfina_summary),
        ..Default::default()
    });
    stmt.profile = Some(CcProfile {
        holders: CcHolders {
            holder: vec![holder],
        },
    });
    stmt.xfina = Some(XfinaCreditCardAccount {
        institution_name: Some("ICICI Bank".to_string()),
        date_only_paths: Some(vec![
            "transactions.transaction.txnDate".to_string(),
            "transactions.transaction.valueDate".to_string(),
        ]),
        ..Default::default()
    });

    // The export declares no figures, so there is nothing to reconcile.
    let mut validation = ValidationReport::empty();
    validation.finalize();

    Ok(ParseResult {
        data: stmt,
        validation,
    })
}

/// Claims the older CSV export by its header, but only in its monthly form:
/// the date-range download shares the header and is declined, so detection
/// reports it as unrecognised instead of reading a year as one statement.
fn probe_csv(text: &str) -> crate::detect::Claim {
    use crate::detect::Claim;
    if !text.contains("\"Accountno:\"") {
        return Claim::NO;
    }
    let records = csv_records(text);
    let Some(header) = records.iter().position(|r| is_csv_header(r)) else {
        return Claim::NO;
    };
    let first_row = records[header + 1..].iter().find_map(|r| csv_row_kind(r));
    match first_row {
        Some(CsvRows::Monthly) => Claim::strong("icici-card-csv"),
        Some(CsvRows::DateRange) => Claim::NO,
        // A cycle with no transactions still has the monthly layout.
        None => Claim::weak("icici-card-csv-header"),
    }
}

/// An ICICI card statement names the bank and its own summary fields.
pub(crate) fn probe(dec: &crate::decode::Decoded<'_>) -> crate::detect::Claim {
    use crate::detect::{probe::any_marker, Claim};
    if dec.container() == crate::detect::Container::Text {
        return probe_csv(dec.probe_text());
    }
    let Ok(sheets) = dec.sheets() else {
        return Claim::NO;
    };
    let head = sheets.head_text(60);
    if head.contains("ICICI BANK")
        && any_marker(
            &head,
            &["CARD HOLDER NAME", "TOTAL CREDIT LIMIT", "PAYMENT DUE DATE"],
        )
    {
        return Claim::strong("icici-card-summary");
    }
    if any_marker(&head, &["CARD HOLDER NAME", "TOTAL CREDIT LIMIT"]) {
        return Claim::weak("icici-card-fields");
    }
    Claim::NO
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::Strength;

    // Invented values in the shape of the older CSV export.
    const MONTHLY: &str = "\"Accountno:\",\"0000000000000000\"\r\n\
        \"Customer Name:\",\"Mr  CARD  HOLDER\"\r\n\
        \"Address:\",\"1 SOME STREET  SOME CITY\"\r\n\
        \r\n\
        \r\n\
        \"Transaction Details:\"\r\n\
        \"Date\",\"Sr.No.\",\"Transaction Details\",\"Reward Point Header\",\"Intl.Amount\",\"Amount(in Rs)\",\"BillingAmountSign\"\r\n\
        \"4000XXXXXXXX0000\"\r\n\
        \"03/01/2026\",\"10000000002\",\"UPI Payment Received\",\"0\",\"0\",\"1000.00\",\"CR\"\r\n\
        \"02/01/2026\",\"10000000001\",\"SOME MERCHANT\",\"20\",\"0\",\"1,400.50\",\"\"\r\n\
        \"04/01/2026\",\"10000000003\",\"SOME MERCHANT\",\"-5\",\"0\",\"100.00\",\"CR\"\r\n";

    const DATE_RANGE: &str = "\"Accountno:\",\"0000000000000000\"\r\n\
        \"Customer Name:\",\"Mr CARD HOLDER\"\r\n\
        \"Transaction Details:\"\r\n\
        \"Date\",\"Sr.No.\",\"Transaction Details\",\"Reward Point Header\",\"Intl.Amount\",\"Amount(in Rs)\",\"BillingAmountSign\"\r\n\
        \"4000 XXXX XXXX 0000\"\r\n\
        \"02-JAN-26\",\"1\",\"SOME MERCHANT\",\"20\",\"0.00\",\"1,400.50\",\"1,400.50\"\r\n\
        \r\n\
        \"MESSAGE Details:\"\r\n";

    fn parse(text: &str) -> Result<ParseResult<CreditCardAccount>, crate::error::XfinaError> {
        let req = ParseRequest::new(text.as_bytes());
        parse_decoded(&Decoded::new(&req), &req)
    }

    #[test]
    fn reads_a_monthly_csv() {
        let result = parse(MONTHLY).expect("monthly CSV parses");
        let stmt = result.data;
        assert_eq!(stmt.masked_acc_number, "4000XXXXXXXX0000");

        let holder = &stmt.profile.as_ref().unwrap().holders.holder[0];
        assert_eq!(holder.name, "CARD HOLDER");
        assert_eq!(holder.address.as_deref(), Some("1 SOME STREET SOME CITY"));

        let txns = stmt.transactions.as_ref().unwrap();
        assert_eq!(txns.start_date, NaiveDate::from_ymd_opt(2026, 1, 2));
        assert_eq!(txns.end_date, NaiveDate::from_ymd_opt(2026, 1, 4));
        let ids: Vec<_> = txns
            .transaction
            .iter()
            .map(|t| t.txn_id.as_deref().unwrap())
            .collect();
        assert_eq!(ids, ["10000000001", "10000000002", "10000000003"]);
        assert_eq!(txns.transaction[0].txn_type, TransactionType::Debit);
        assert_eq!(txns.transaction[0].amount, Decimal::new(140050, 2));
        assert_eq!(txns.transaction[2].txn_type, TransactionType::Credit);

        let summary = stmt.summary.as_ref().unwrap().xfina.as_ref().unwrap();
        assert_eq!(summary.payment_credit, Some(Decimal::new(1100, 0)));
        assert_eq!(summary.purchases_debits, Some(Decimal::new(140050, 2)));
        assert_eq!(summary.reward_points_summary.as_ref().unwrap().earned, 15);
    }

    #[test]
    fn refuses_the_date_range_csv() {
        let err = parse(DATE_RANGE).expect_err("a date-range export is not a statement");
        assert_eq!(err.kind(), "unsupported");
    }

    #[test]
    fn probes_only_the_monthly_csv() {
        let probe_text = |text: &str| {
            let req = ParseRequest::new(text.as_bytes());
            probe(&Decoded::new(&req)).strength
        };
        assert_eq!(probe_text(MONTHLY), Strength::Strong);
        assert_eq!(probe_text(DATE_RANGE), Strength::No);
        assert_eq!(probe_text("\"Date\",\"Amount\"\r\n"), Strength::No);
    }
}
