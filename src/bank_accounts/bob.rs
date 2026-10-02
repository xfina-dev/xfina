use crate::models::date_utils;
use crate::models::deposit::{
    DepositAccount, FiType, Holder, Holders, HoldersType, HoldingNominee, Profile, Summary,
    Transaction, TransactionMode, TransactionType, Transactions, XfinaDepositAccount, XfinaHolder,
    XfinaSummary,
};
use crate::models::mask_account_number;
use crate::models::validation::{check_row_balances, ParseResult, ValidationReport};
use chrono::{NaiveDate, NaiveDateTime};
use regex::Regex;
use rust_decimal::Decimal;

use crate::decode::Decoded;
use crate::models::request::ParseRequest;

pub fn parse_bob_xls(
    input: ParseRequest<'_>,
) -> Result<ParseResult<DepositAccount>, crate::error::XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

/// Parses from an already-decoded input, so detection can probe and
/// parse against one read of the file.
pub(crate) fn parse_decoded(
    decoded: &Decoded<'_>,
    _input: &ParseRequest<'_>,
) -> Result<ParseResult<DepositAccount>, crate::error::XfinaError> {
    let sheet = decoded.sheets()?.first()?;

    let mut statement = DepositAccount {
        r#type: FiType::Deposit,
        version: 1.1,
        ..Default::default()
    };

    let mut xfina_account = XfinaDepositAccount {
        institution_name: Some("Bank of Baroda".to_string()),
        ..Default::default()
    };

    let mut account_number = String::new();
    let mut ifsc_code = String::new();
    let mut micr_code = String::new();
    let mut branch_name = String::new();
    let mut name = String::new();
    let mut address = String::new();
    let mut customer_id = String::new();
    let mut nominee = String::new();
    let mut account_product = String::new();

    let re_prod = Regex::new(r"Statement of transactions in\s+(.*?)\s+\d+").unwrap();

    let mut start_date: Option<NaiveDate> = None;
    let mut end_date: Option<NaiveDate> = None;
    let mut generated_date: Option<NaiveDate> = None;
    let mut generated_date_time: Option<NaiveDateTime> = None;

    let mut parsed_transactions = Vec::new();
    let _transactions_obj = Transactions::default();
    let mut xfina_summary = XfinaSummary::default();

    let mut date_only_paths = Vec::new();

    let mut in_transactions = false;

    for (row_idx, row) in sheet.rows().enumerate() {
        let row_vec: Vec<String> = row
            .iter()
            .map(|c| c.to_string().replace("\u{0}", "").trim().to_string())
            .collect();
        if row_vec.is_empty() {
            continue;
        }

        let first_col = row_vec[0].trim();

        // Extract Metadata
        if first_col.starts_with("Customer Id:") {
            if row_vec.len() > 4 {
                customer_id = row_vec[4].trim().to_string();
            }
            if let Some(idx) = row_vec.iter().position(|s| s.trim() == "Account No:") {
                if row_vec.len() > idx + 6 {
                    account_number = row_vec[idx + 6].trim().to_string();
                }
            }
        } else if first_col.starts_with("Branch Name:") {
            if row_vec.len() > 4 {
                branch_name = row_vec[4].trim().to_string();
            }
            if let Some(idx) = row_vec.iter().position(|s| s.trim() == "MICR Code:") {
                if row_vec.len() > idx + 6 {
                    micr_code = row_vec[idx + 6].trim().to_string();
                }
            }
        } else if first_col.starts_with("IFSC Code:") {
            if row_vec.len() > 4 {
                ifsc_code = row_vec[4].trim().to_string();
            }
            if row_vec.len() > 20 {
                let nom_str = row_vec[20].trim();
                if nom_str.eq_ignore_ascii_case("Yes") || nom_str.eq_ignore_ascii_case("Registered")
                {
                    nominee = "REGISTERED".to_string();
                } else if nom_str.eq_ignore_ascii_case("No")
                    || nom_str.eq_ignore_ascii_case("Not Registered")
                {
                    nominee = "NOT_REGISTERED".to_string();
                }
            }
        } else if first_col.starts_with("Statement of transactions in") {
            if let Some(caps) = re_prod.captures(first_col) {
                account_product = caps[1].trim().to_string();
            }
        } else if first_col.starts_with("Your Account Statement as on") {
            let parts: Vec<&str> = first_col.split("as on").collect();
            if parts.len() > 1 {
                generated_date = NaiveDate::parse_from_str(parts[1].trim(), "%d/%m/%Y").ok();
            }
            if let Some(idx) = row_vec
                .iter()
                .position(|s| s.starts_with("Statement Period from"))
            {
                let period_str = &row_vec[idx];
                let p_parts: Vec<&str> = period_str.split("from").collect();
                if p_parts.len() > 1 {
                    let d_parts: Vec<&str> = p_parts[1].split("to").collect();
                    if d_parts.len() == 2 {
                        start_date = NaiveDate::parse_from_str(d_parts[0].trim(), "%d/%m/%Y").ok();
                        end_date = NaiveDate::parse_from_str(d_parts[1].trim(), "%d/%m/%Y").ok();
                    }
                }
            }
        } else if let Ok(excel_date) = first_col.parse::<f64>() {
            if excel_date > 40000.0 && excel_date < 60000.0 {
                let days = excel_date.trunc() as i64;
                let fraction = excel_date.fract();
                let seconds = (fraction * 86400.0).round() as i64;
                let base_date = NaiveDate::from_ymd_opt(1899, 12, 30).unwrap();
                if let Some(date) = base_date.checked_add_signed(chrono::Duration::days(days)) {
                    if let Some(dt) = date.and_hms_opt(0, 0, 0) {
                        if let Some(dt_with_time) =
                            dt.checked_add_signed(chrono::Duration::seconds(seconds))
                        {
                            generated_date_time = Some(dt_with_time);
                        }
                    }
                }
            }
        }

        // Sometimes Name is at row 12 or 1
        if row_idx == 0 && row_vec.len() > 13 && row_vec[1].contains("Holder Name") {
            let parts: Vec<&str> = row_vec[1].split(':').collect();
            if parts.len() > 1 {
                name = parts[1].trim().to_string();
            }
        }
        if row_idx == 1 && row_vec.len() > 13 && !row_vec[13].is_empty() {
            address = row_vec[13].replace('\n', ", ").trim().to_string();
        }
        if row_idx == 9 && first_col.len() > 5 {
            name = first_col.to_string();
        }

        // Transaction block marker
        if first_col == "TRAN DATE" {
            in_transactions = true;
            continue;
        }

        if in_transactions {
            if first_col.is_empty() {
                continue;
            }

            // Check for end of transactions (e.g. "This is computer-generated statement")
            if first_col.contains("This is computer-generated") || first_col.contains("Page") {
                continue;
            }

            // ReBIT date format: DD/MM/YYYY to ISO (YYYY-MM-DD)
            let date_str = first_col;
            let date_parts: Vec<&str> = date_str.split(' ').collect();
            let date_only = date_parts[0];

            let date = NaiveDate::parse_from_str(date_only, "%d/%m/%Y").ok();

            let val_date_str = if row_vec.len() > 2 {
                row_vec[2].trim()
            } else {
                ""
            };
            let value_date = NaiveDate::parse_from_str(val_date_str, "%d/%m/%Y").ok();

            let narration = if row_vec.len() > 5 {
                row_vec[5].trim().to_string()
            } else {
                String::new()
            };
            if narration.is_empty() && date.is_none() {
                continue; // Skip footer noise
            }

            let debit_str = if row_vec.len() > 11 {
                row_vec[11].trim()
            } else {
                ""
            };
            let credit_str = if row_vec.len() > 17 {
                row_vec[17].trim()
            } else {
                ""
            };
            let balance_str = if row_vec.len() > 24 {
                row_vec[24].trim()
            } else {
                ""
            };

            let parse_amt = |s: &str| -> Option<Decimal> {
                let clean = s.replace(",", "").replace("Cr", "").replace("Dr", "");
                clean.parse().ok()
            };

            let (amount, tx_type) = if let Some(cr) = parse_amt(credit_str) {
                (cr, TransactionType::Credit)
            } else if let Some(dr) = parse_amt(debit_str) {
                (dr, TransactionType::Debit)
            } else {
                continue; // If both empty, maybe a continuation row, but BoB usually fits in one row.
            };

            let desc_upper = narration.to_uppercase();
            let mode = if desc_upper.contains("UPI") {
                Some(TransactionMode::Upi)
            } else if desc_upper.contains("NEFT") || desc_upper.contains("IMPS") {
                Some(TransactionMode::Ft)
            } else if narration.contains("CASH") {
                Some(TransactionMode::Cash)
            } else {
                None
            };

            let mut current_balance = Decimal::from(0);
            if let Some(bal) = parse_amt(balance_str) {
                current_balance = bal;
            }

            if let Some(dt) = date {
                let txn_timestamp = Some(date_utils::ist_midnight(dt));

                let tx = Transaction {
                    txn_id: None,
                    transaction_timestamp: txn_timestamp,
                    value_date,
                    narration,
                    reference: None,
                    r#type: tx_type,
                    amount,
                    current_balance,
                    mode,
                };
                parsed_transactions.push(tx);

                if !date_only_paths
                    .contains(&"transactions.transaction.transactionTimestamp".to_string())
                {
                    date_only_paths
                        .push("transactions.transaction.transactionTimestamp".to_string());
                }
            }
        }
    }

    parsed_transactions.reverse();

    let mut summary = Summary::default();
    if let Some(first) = parsed_transactions.first() {
        let ob = if first.r#type == TransactionType::Credit {
            first.current_balance - first.amount
        } else {
            first.current_balance + first.amount
        };
        xfina_summary.opening_balance = Some(ob);
    }
    if let Some(last) = parsed_transactions.last() {
        summary.current_balance = last.current_balance;
    }

    if !branch_name.is_empty() {
        summary.branch = Some(branch_name);
    }
    if !ifsc_code.is_empty() {
        summary.ifsc_code = Some(ifsc_code);
    }
    if !micr_code.is_empty() {
        summary.micr_code = Some(micr_code);
    }

    if !account_number.is_empty() {
        statement.masked_acc_number = mask_account_number(&account_number);
    }

    if !account_product.is_empty() {
        xfina_summary.account_product = Some(account_product);
    }

    // Always assign xfina_summary back to summary.xfina as it contains at least defaults/opening balance
    summary.xfina = Some(xfina_summary);
    let transactions_obj = Transactions {
        start_date,
        end_date,
        transaction: parsed_transactions,
        ..Default::default()
    };

    if let Some(dt) = generated_date_time {
        xfina_account.generated_date = Some(date_utils::ist_to_utc(dt));
    } else if let Some(d) = generated_date {
        xfina_account.generated_date = Some(date_utils::ist_midnight(d));
        if !date_only_paths.contains(&"xfina.generatedDate".to_string()) {
            date_only_paths.push("xfina.generatedDate".to_string());
        }
    }

    let mut holder = Holder {
        name,
        ..Default::default()
    };
    if !address.is_empty() {
        holder.address = Some(address.replace("  ", " ").replace(" ,", ","));
    }
    if nominee == "REGISTERED" {
        holder.nominee = Some(HoldingNominee::Registered);
    } else if nominee == "NOT_REGISTERED" {
        holder.nominee = Some(HoldingNominee::NotRegistered);
    }

    let mut xfina_holder = XfinaHolder::default();
    if !customer_id.is_empty() {
        xfina_holder.customer_id = Some(customer_id);
    }
    holder.xfina = Some(xfina_holder);

    let holders = vec![holder];

    let profile = Profile {
        holders: Holders {
            r#type: HoldersType::Single,
            holder: holders,
        },
    };

    statement.profile = Some(profile);
    statement.summary = Some(summary);
    statement.transactions = Some(transactions_obj);

    if !date_only_paths.is_empty() {
        xfina_account.date_only_paths = Some(date_only_paths);
    }
    statement.xfina = Some(xfina_account);

    let mut validation = ValidationReport::empty();

    if let Some(ob) = statement
        .summary
        .as_ref()
        .and_then(|s| s.xfina.as_ref())
        .and_then(|x| x.opening_balance)
    {
        let row_tuples: Vec<(bool, rust_decimal::Decimal, rust_decimal::Decimal, String)> =
            statement
                .transactions
                .as_ref()
                .unwrap()
                .transaction
                .iter()
                .map(|t| {
                    (
                        t.r#type == crate::models::deposit::TransactionType::Credit,
                        t.amount,
                        t.current_balance,
                        t.narration.clone(),
                    )
                })
                .collect();
        validation.row_level = check_row_balances(ob, &row_tuples);
    }

    // Derived check: opening + credits - debits = closing
    if let (Some(ob), cb) = (
        statement
            .summary
            .as_ref()
            .and_then(|x| x.xfina.as_ref())
            .and_then(|x| x.opening_balance),
        statement
            .summary
            .as_ref()
            .map(|s| s.current_balance)
            .unwrap_or(rust_decimal::Decimal::ZERO),
    ) {
        let credits: rust_decimal::Decimal = statement
            .transactions
            .as_ref()
            .unwrap()
            .transaction
            .iter()
            .filter(|t| t.r#type == crate::models::deposit::TransactionType::Credit)
            .map(|t| t.amount)
            .sum();
        let debits: rust_decimal::Decimal = statement
            .transactions
            .as_ref()
            .unwrap()
            .transaction
            .iter()
            .filter(|t| t.r#type == crate::models::deposit::TransactionType::Debit)
            .map(|t| t.amount)
            .sum();

        validation
            .summary_level
            .checks
            .push(crate::models::validation::SummaryCheck::derived(
                "computed_closing_balance",
                cb,
                ob + credits - debits,
                None,
            ));
    }

    validation.summary_level.passed = validation.summary_level.checks.iter().all(|c| c.passed);
    validation.finalize();

    Ok(ParseResult {
        data: statement,
        validation,
    })
}

/// Bank of Baroda labels its header fields and heads its table "TRAN DATE".
pub(crate) fn probe(dec: &crate::decode::Decoded<'_>) -> crate::detect::Claim {
    use crate::detect::{probe::any_marker, Claim};
    let Ok(sheets) = dec.sheets() else {
        return Claim::NO;
    };
    let head = sheets.head_text(40);
    if any_marker(&head, &["STATEMENT OF TRANSACTIONS IN", "BANK OF BARODA"]) {
        return Claim::strong("bob-statement-title");
    }
    if head.contains("TRAN DATE") && any_marker(&head, &["HOLDER NAME", "ACCOUNT NO"]) {
        return Claim::strong("bob-header");
    }
    if any_marker(&head, &["THIS IS COMPUTER-GENERATED", "MICR CODE"]) {
        return Claim::weak("bob-fields");
    }
    Claim::NO
}
