use crate::decode::Decoded;
use crate::models::credit_card::{
    CardType, CcCard, CcCards, CcHolder, CcHolders, CcProfile, CcSummary, CcTransaction,
    CcTransactions, CcXfinaSummary, CcXfinaTransaction, CcXfinaTransactions, CreditCardAccount,
    TypeChoice, XfinaCreditCardAccount,
};
use crate::models::deposit::TransactionType;
use crate::models::request::ParseRequest;
use crate::models::validation::{ParseResult, SummaryCheck, ValidationReport};
use chrono::{DateTime, NaiveDate, Utc};
use regex::Regex;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Parses an Axis Bank credit card monthly statement (`.xlsx` export from the
/// Axis mobile app / net banking portal).
///
/// The sheet holds a handful of merged `"Label\nValue"` summary cells followed
/// by a `Date | Transaction Details | | Amount (INR) | Debit/Credit` table,
/// terminated by `** End of Statement **`.
///
/// Axis prints neither a statement date nor a purchases/payments split, so the
/// statement period is derived from the transaction dates and the debit/credit
/// totals are summed from the rows. The declared `Total Payment Due` is still
/// cross-checked against `opening balance + debits - credits`.
pub fn parse_axis_statement(
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
    let range = decoded.sheets()?.first()?;

    let mut stmt = CreditCardAccount {
        r#type: "credit_card".to_string(),
        version: 1.1,
        ..Default::default()
    };

    let mut xfina_account = XfinaCreditCardAccount {
        institution_name: Some("Axis Bank".to_string()),
        ..Default::default()
    };

    let mut date_only_paths = vec![
        "transactions.transaction.txnDate".to_string(),
        "transactions.transaction.valueDate".to_string(),
    ];

    if let Some(generated) = filename_date(input.filename) {
        xfina_account.generated_date = Some(generated);
        // Axis prints no statement date; this comes from the filename.
        xfina_account.generated_date_derived = Some(true);
        date_only_paths.insert(0, "xfina.generatedDate".to_string());
    }

    let mut holder = CcHolder::default();
    let mut summary = CcSummary::default();
    let mut xfina_summary = CcXfinaSummary::default();
    let mut xfina_txns = CcXfinaTransactions::default();

    let mut card_no = String::new();
    let mut card_type = CardType::Others;
    let mut transactions_list: Vec<CcTransaction> = Vec::new();
    let mut in_transactions = false;

    for row in range.rows() {
        let cells: Vec<String> = row.iter().map(|c| c.to_string()).collect();
        if cells.iter().all(|c| c.trim().is_empty()) {
            continue;
        }

        let col0 = cells.first().map(|s| s.trim()).unwrap_or("");
        if col0.starts_with("**") {
            break;
        }

        // The transaction table starts right after its own header row.
        if col0 == "Date" && cells.iter().any(|c| c.trim() == "Debit/Credit") {
            in_transactions = true;
            continue;
        }

        if in_transactions {
            let Some(txn_date) = parse_axis_date(col0) else {
                continue;
            };
            let narration = cells.get(1).map(|s| s.trim()).unwrap_or("").to_string();
            let amount = cells
                .get(3)
                .and_then(|s| parse_amount(s))
                .unwrap_or_default()
                .abs();
            let txn_type = match cells.get(4).map(|s| s.trim().to_lowercase()).as_deref() {
                Some("credit") => TransactionType::Credit,
                _ => TransactionType::Debit,
            };

            transactions_list.push(CcTransaction {
                txn_date: Some(to_ist_utc(txn_date)),
                value_date: Some(txn_date),
                narration,
                amount,
                txn_type,
                txn_id: None,
                statement_date: None,
                mcc: None,
                masked_card_number: None,
                xfina: Some(CcXfinaTransaction::default()),
            });
            continue;
        }

        // Header block: the card title and number sit in the same row as the
        // holder name and address.
        if let Some(title) = cells.iter().find(|c| c.contains("Credit Card Number:")) {
            if let Some(num) = title
                .lines()
                .find_map(|l| l.trim().strip_prefix("Credit Card Number:"))
            {
                card_no = num.trim().to_string();
            }
            card_type = detect_card_type(title);
            xfina_summary.card_product = detect_card_product(title);

            let mut name_block = col0.lines().map(|l| l.trim()).filter(|l| !l.is_empty());
            if let Some(name) = name_block.next() {
                holder.name = name.to_string();
            }
            let address = name_block.collect::<Vec<_>>().join(" ");
            if !address.is_empty() {
                holder.address = Some(normalize_address(&address));
            }
            continue;
        }

        // Summary block: merged cells carrying "Label\nValue" at columns 0, 2, 4.
        for cell in cells.iter() {
            let Some((label, value)) = cell.split_once('\n') else {
                continue;
            };
            let value = value.trim();
            match label.trim() {
                "Total Payment Due" => summary.total_due_amount = parse_amount(value),
                "Minimum Payment Due" => summary.min_due_amount = parse_amount(value),
                "Credit Limit" => summary.credit_limit = parse_amount(value),
                "Opening Balance" => xfina_summary.opening_balance = parse_amount(value),
                "Payment Due Date" => {
                    let d = parse_axis_date(value);
                    summary.due_date = d;
                    if let Some(date) = d {
                        let stmt_date = date - chrono::Duration::days(20);
                        summary.last_statement_date = Some(stmt_date);
                    }
                }
                _ => {}
            }
        }
    }

    transactions_list.sort_by_key(|t| t.txn_date);

    // Layer 1: Estimate statement period based on due date
    let mut est_end = summary.last_statement_date;
    let mut est_start = est_end.and_then(|d| {
        use chrono::Months;
        d.checked_sub_months(Months::new(1))
            .map(|prev| prev + chrono::Duration::days(1))
    });

    // Layer 2: Adjust if any transactions fall outside the estimated period
    if let Some(first) = transactions_list.first().and_then(|t| t.value_date) {
        est_start = match est_start {
            Some(es) if first < es => Some(first),
            Some(es) => Some(es),
            None => Some(first),
        };
    }
    if let Some(last) = transactions_list.last().and_then(|t| t.value_date) {
        est_end = match est_end {
            Some(ee) if last > ee => Some(last),
            Some(ee) => Some(ee),
            None => Some(last),
        };
    }

    let mut txns = CcTransactions {
        start_date: est_start,
        end_date: est_end,
        ..Default::default()
    };
    xfina_txns.start_date_derived = Some(true);
    xfina_txns.end_date_derived = Some(true);

    // Axis prints no purchases/payments split either — sum it from the rows.
    let mut purchases = Decimal::ZERO;
    let mut payments = Decimal::ZERO;
    let mut owner_credit_breakdown: HashMap<String, f64> = HashMap::new();
    let mut owner_debit_breakdown: HashMap<String, f64> = HashMap::new();

    for txn in transactions_list.iter_mut() {
        let amt_f64 = txn.amount.to_f64().unwrap_or(0.0);
        if txn.txn_type == TransactionType::Credit {
            payments += txn.amount;
            *owner_credit_breakdown
                .entry(holder.name.clone())
                .or_insert(0.0) += amt_f64;
        } else {
            purchases += txn.amount;
            *owner_debit_breakdown
                .entry(holder.name.clone())
                .or_insert(0.0) += amt_f64;
        }
        if let Some(x) = txn.xfina.as_mut() {
            x.owner = Some(holder.name.clone());
        }
    }

    xfina_summary.payment_credit = Some(payments);
    xfina_summary.purchases_debits = Some(purchases);
    xfina_summary.owner_credit_breakdown = owner_credit_breakdown;
    xfina_summary.owner_debit_breakdown = owner_debit_breakdown;

    txns.transaction = transactions_list;
    txns.xfina = Some(xfina_txns);

    if !card_no.is_empty() {
        stmt.masked_acc_number = card_no.clone();
        holder.cards = Some(CcCards {
            card: vec![CcCard {
                card_type,
                primary: TypeChoice::Yes,
                masked_card_number: card_no,
                issued_date: None,
            }],
        });
    }

    summary.xfina = Some(xfina_summary);
    stmt.summary = Some(summary);
    stmt.profile = Some(CcProfile {
        holders: CcHolders {
            holder: vec![holder],
        },
    });
    stmt.transactions = Some(txns);
    xfina_account.date_only_paths = Some(date_only_paths);
    stmt.xfina = Some(xfina_account);

    let mut validation = ValidationReport::empty();

    // The one declared figure our rows can reproduce:
    // total due == opening balance + debits - credits.
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

    validation.summary_level.passed = validation.summary_level.checks.iter().all(|c| c.passed);
    validation.finalize();

    Ok(ParseResult {
        data: stmt,
        validation,
    })
}

/// `"Neo Rupay Credit card Monthly Statement"` -> [`CardType::Rupay`].
fn detect_card_type(title: &str) -> CardType {
    let title = title.to_lowercase();
    if title.contains("rupay") {
        CardType::Rupay
    } else if title.contains("visa") {
        CardType::Visa
    } else if title.contains("master") {
        CardType::MasterCard
    } else {
        CardType::Others
    }
}

/// Card variants Axis co-brands with an issuer network or its own name, both of
/// which are noise once the network lands in [`CardType`].
const CARD_TITLE_NOISE: [&str; 6] = ["axis", "bank", "rupay", "visa", "master", "mastercard"];

/// Pulls the card variant out of the statement title, e.g.
/// `"Neo Rupay Credit card Monthly Statement"` -> `"Neo"`.
fn detect_card_product(title: &str) -> Option<String> {
    let heading = title.lines().next()?.trim();
    // `to_ascii_lowercase` keeps byte offsets aligned with `heading`.
    let end = heading
        .to_ascii_lowercase()
        .find("credit card")
        .unwrap_or(heading.len());
    let product = heading[..end]
        .split_whitespace()
        .filter(|word| !CARD_TITLE_NOISE.contains(&word.to_ascii_lowercase().as_str()))
        .collect::<Vec<_>>()
        .join(" ");

    (!product.is_empty()).then_some(product)
}

/// `"₹ 1,23,456.78"` -> `123456.78`. Also normalizes Axis's `"₹ -0.00"` to zero.
fn parse_amount(val: &str) -> Option<Decimal> {
    let clean: String = val
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    let amount = clean.parse::<Decimal>().ok()?;
    if amount.is_zero() {
        return Some(Decimal::ZERO);
    }
    Some(amount)
}

/// Axis writes dates as `25 Aug '26`.
fn parse_axis_date(val: &str) -> Option<NaiveDate> {
    let re = Regex::new(r"^(\d{1,2})\s+([A-Za-z]{3})\s*'?(\d{2}|\d{4})$").ok()?;
    let caps = re.captures(val.trim())?;
    let day = caps.get(1)?.as_str().parse::<u32>().ok()?;
    let month = month_from_abbrev(caps.get(2)?.as_str())?;
    let year_raw = caps.get(3)?.as_str();
    let year = year_raw.parse::<i32>().ok()?;
    let year = if year_raw.len() == 2 {
        2000 + year
    } else {
        year
    };
    NaiveDate::from_ymd_opt(year, month, day)
}

fn month_from_abbrev(abbrev: &str) -> Option<u32> {
    match abbrev.to_lowercase().as_str() {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

/// Axis names its exports `CC_Statement_2026_08_27.xlsx`.
fn filename_date(filename: Option<&str>) -> Option<DateTime<Utc>> {
    let re = Regex::new(r"(\d{4})[_-](\d{2})[_-](\d{2})").ok()?;
    let caps = re.captures(filename?)?;
    let date = NaiveDate::from_ymd_opt(
        caps.get(1)?.as_str().parse().ok()?,
        caps.get(2)?.as_str().parse().ok()?,
        caps.get(3)?.as_str().parse().ok()?,
    )?;
    Some(to_ist_utc(date))
}

fn to_ist_utc(date: NaiveDate) -> DateTime<Utc> {
    crate::models::date_utils::ist_midnight(date)
}

/// Address lines arrive with trailing commas and doubled spaces from the merged cell.
fn normalize_address(address: &str) -> String {
    address
        .split(',')
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The Axis card export titles its own month selector and card number field.
pub(crate) fn probe(dec: &crate::decode::Decoded<'_>) -> crate::detect::Claim {
    use crate::detect::{probe::any_marker, Claim};
    let Ok(sheets) = dec.sheets() else {
        return Claim::NO;
    };
    let head = sheets.head_text(60);
    if any_marker(&head, &["SELECTED STATEMENT MONTH", "CREDIT CARD NUMBER:"]) {
        return Claim::strong("axis-card-header");
    }
    if head.contains("PAYMENT SUMMARY") && head.contains("DEBIT/CREDIT") {
        return Claim::weak("axis-card-columns");
    }
    Claim::NO
}
