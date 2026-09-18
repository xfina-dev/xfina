use crate::models::deposit::{
    DepositAccount, FiType, Holder, Holders, HoldersType, Profile, Summary, Transaction,
    TransactionMode, TransactionType, Transactions, XfinaDepositAccount, XfinaSummary,
    XfinaTransactions,
};
use crate::models::mask_account_number;
use crate::models::txn_order::reorder_same_day_transactions;
use crate::models::validation::{check_row_balances, ParseResult, SummaryCheck, ValidationReport};
use chrono::NaiveDate;
use regex::Regex;
use rust_decimal::Decimal;

use crate::decode::Decoded;
use crate::models::request::ParseRequest;

pub fn parse_icici_xls(
    input: ParseRequest<'_>,
) -> Result<ParseResult<DepositAccount>, crate::error::XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

/// Parses from an already-decoded input, so detection can probe and
/// parse against one read of the file.
pub(crate) fn parse_decoded(
    decoded: &Decoded<'_>,
    input: &ParseRequest<'_>,
) -> Result<ParseResult<DepositAccount>, crate::error::XfinaError> {
    let filename = input.filename;
    let range = decoded.sheets()?.first()?;

    let mut statement = DepositAccount {
        r#type: FiType::Deposit,
        version: 1.1,
        ..Default::default()
    };

    let mut xfina_account = XfinaDepositAccount {
        institution_name: Some("ICICI Bank".to_string()),
        ..Default::default()
    };

    let mut date_only_paths = Vec::new();

    if let Some(fname) = filename {
        // e.g. OpTransactionHistory05-07-2026.xls
        let re = Regex::new(r"(\d{2}-\d{2}-\d{4})").unwrap();
        if let Some(caps) = re.captures(fname) {
            if let Some(m) = caps.get(1) {
                if let Ok(d) = NaiveDate::parse_from_str(m.as_str(), "%d-%m-%Y") {
                    xfina_account.generated_date = Some(crate::models::date_utils::ist_midnight(d));
                    date_only_paths.push("xfina.generatedDate".to_string());
                    // From the filename, not the statement.
                    xfina_account.generated_date_derived = Some(true);
                }
            }
        }
    }

    let mut in_transactions = false;
    let mut parsed_transactions: Vec<Transaction> = Vec::new();
    let mut holders = Vec::new();

    for row in range.rows() {
        let row_vec: Vec<String> = row
            .iter()
            .map(|c| c.to_string().trim().to_string())
            .collect();
        if row_vec.is_empty() {
            continue;
        }

        // Check for Metadata
        if row_vec[0] == "Account Number" && row_vec.len() >= 3 {
            let account_str = &row_vec[2];
            // Format: "000011112222 ( INR )  - <ACCOUNT HOLDER>"
            if let Some(parts) = account_str.split_once(" - ") {
                let left_part = parts.0;
                let acc_no = left_part.split(' ').next().unwrap_or(left_part);
                statement.masked_acc_number = mask_account_number(acc_no.trim());

                let holder = Holder {
                    name: parts.1.trim().to_string(),
                    ..Default::default()
                };
                holders.push(holder);
            }
        }

        if row_vec[0] == "S No." {
            in_transactions = true;
            continue;
        }

        if in_transactions {
            if row_vec[0].starts_with("Legends Used in Account Statement") {
                break;
            }

            if row_vec[0].is_empty() {
                let has_data = row_vec.iter().any(|c| !c.is_empty());
                if !has_data {
                    continue;
                }

                // Handle multi-line narration
                if row_vec.len() >= 5 && !row_vec[4].is_empty() {
                    if let Some(last_tx) = parsed_transactions.last_mut() {
                        last_tx.narration.push_str(row_vec[4].trim());
                    }
                }
                continue;
            }

            // Parse a transaction line
            // ["1", "05-Jun-2026", "05-Jun-2026", "", "NEFT-...", "0.00", "1000.00", "5000.00"]
            if row_vec.len() >= 8 {
                if row_vec[1].is_empty() {
                    continue;
                }

                let _value_date_str = &row_vec[1];
                let date_str = &row_vec[2];
                let ref_num = &row_vec[3];
                let desc = &row_vec[4];
                let withdrawal_str = row_vec[5].replace(",", "");
                let deposit_str = row_vec[6].replace(",", "");
                let balance_str = row_vec[7].replace(",", "");

                if date_str.is_empty() && desc.is_empty() {
                    continue; // Skip empty rows
                }

                // Parse dates using the robust parse_indian_date which handles multiple formats
                let iso_date = crate::models::parse_indian_date(date_str);
                let parsed_date = NaiveDate::parse_from_str(&iso_date, "%Y-%m-%d")
                    .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());

                let withdrawal: Decimal = withdrawal_str.parse().unwrap_or(Decimal::from(0));
                let deposit: Decimal = deposit_str.parse().unwrap_or(Decimal::from(0));
                let balance: Decimal = balance_str.parse().unwrap_or(Decimal::from(0));

                let (tx_type, amount) = if deposit > Decimal::from(0) {
                    (TransactionType::Credit, deposit)
                } else if withdrawal > Decimal::from(0) {
                    (TransactionType::Debit, withdrawal)
                } else {
                    continue; // Zero amount transaction? Skip.
                };

                let narration_upper = desc.to_uppercase();
                let mode = if desc.starts_with("UPI/") {
                    Some(TransactionMode::Upi)
                } else if narration_upper.contains("IMPS") || narration_upper.contains("NEFT") {
                    Some(TransactionMode::Ft)
                } else if desc.contains("ATM") || desc.starts_with("CASH") {
                    Some(TransactionMode::Cash)
                } else {
                    None
                };

                parsed_transactions.push(Transaction {
                    transaction_timestamp: Some(crate::models::date_utils::ist_midnight(
                        parsed_date,
                    )),
                    value_date: Some(parsed_date),
                    narration: desc.to_string(),
                    reference: if ref_num.is_empty() {
                        None
                    } else {
                        Some(ref_num.to_string())
                    },
                    r#type: tx_type,
                    amount,
                    mode,
                    current_balance: balance,
                    txn_id: None,
                });

                if !date_only_paths
                    .contains(&"transactions.transaction.transactionTimestamp".to_string())
                {
                    date_only_paths
                        .push("transactions.transaction.transactionTimestamp".to_string());
                }
            }
        }
    }

    // ICICI shuffles a day's rows relative to the balance column, so put each
    // day back into the order its printed balances describe before anything
    // downstream derives a balance from row order. See xfina-dev/xfina#53.
    let reordered = reorder_same_day_transactions(&mut parsed_transactions);

    let mut summary = Summary::default();

    // Set opening and closing balance
    if let Some(first) = parsed_transactions.first() {
        if first.r#type == TransactionType::Credit {
            let ob = first.current_balance - first.amount;
            summary.xfina = Some(XfinaSummary {
                opening_balance: Some(ob),
                ..Default::default()
            });
        } else {
            let ob = first.current_balance + first.amount;
            summary.xfina = Some(XfinaSummary {
                opening_balance: Some(ob),
                ..Default::default()
            });
        }
    }

    if let Some(last) = parsed_transactions.last() {
        summary.current_balance = last.current_balance;
    }

    let mut transactions_obj = Transactions::default();

    // Set statement period from transactions if available
    if let Some(first) = parsed_transactions.first() {
        transactions_obj.start_date = first.value_date;
    }
    if let Some(last) = parsed_transactions.last() {
        transactions_obj.end_date = last.value_date;
    }

    transactions_obj.transaction = parsed_transactions;
    if reordered {
        transactions_obj.xfina = Some(XfinaTransactions {
            reordered: Some(true),
        });
    }

    let profile = Profile {
        holders: Holders {
            r#type: if holders.len() > 1 {
                HoldersType::Joint
            } else {
                HoldersType::Single
            },
            holder: holders,
        },
    };

    let mut validation = ValidationReport::empty();

    if let Some(ob) = summary.xfina.as_ref().and_then(|x| x.opening_balance) {
        let row_tuples: Vec<(bool, Decimal, Decimal, String)> = transactions_obj
            .transaction
            .iter()
            .map(|t| {
                (
                    t.r#type == TransactionType::Credit,
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
        summary.xfina.as_ref().and_then(|x| x.opening_balance),
        summary.current_balance,
    ) {
        let credits: Decimal = transactions_obj
            .transaction
            .iter()
            .filter(|t| t.r#type == TransactionType::Credit)
            .map(|t| t.amount)
            .sum();
        let debits: Decimal = transactions_obj
            .transaction
            .iter()
            .filter(|t| t.r#type == TransactionType::Debit)
            .map(|t| t.amount)
            .sum();

        validation.summary_level.checks.push(SummaryCheck::derived(
            "computed_closing_balance",
            cb,
            ob + credits - debits,
            None,
        ));
    }

    validation.summary_level.passed = validation.summary_level.checks.iter().all(|c| c.passed);
    validation.finalize();

    statement.profile = Some(profile);
    statement.summary = Some(summary);
    statement.transactions = Some(transactions_obj);
    if !date_only_paths.is_empty() {
        xfina_account.date_only_paths = Some(date_only_paths);
    }
    statement.xfina = Some(xfina_account);

    Ok(ParseResult {
        data: statement,
        validation,
    })
}

pub fn parse_icici_bank_statement<'a>(
    input: ParseRequest<'a>,
) -> Result<ParseResult<DepositAccount>, crate::error::XfinaError> {
    parse_icici_xls(input)
}

/// ICICI titles its export and names its own amount columns.
pub(crate) fn probe(dec: &crate::decode::Decoded<'_>) -> crate::detect::Claim {
    use crate::detect::{probe::any_marker, Claim};
    let Ok(sheets) = dec.sheets() else {
        return Claim::NO;
    };
    let head = sheets.head_text(40);
    if any_marker(
        &head,
        &["DETAILED STATEMENT", "LEGENDS USED IN ACCOUNT STATEMENT"],
    ) {
        return Claim::strong("icici-bank-title");
    }
    if any_marker(&head, &["DEPOSIT AMOUNT", "WITHDRAWAL AMOUNT"]) && head.contains("S NO.") {
        return Claim::weak("icici-bank-columns");
    }
    Claim::NO
}
