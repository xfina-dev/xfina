use crate::models::credit_card::{
    CardType, CcCard, CcCards, CcHolder, CcHolders, CcProfile, CcSummary, CcTransaction,
    CcTransactions, CcXfinaSummary, CcXfinaTransaction, CcXfinaTransactions, CreditCardAccount,
    PastDues, RewardPointsSummary, RewardProgram, TypeChoice, XfinaCreditCardAccount,
};
use crate::models::deposit::TransactionType;
use crate::models::validation::{ParseResult, SummaryCheck, ValidationReport};
use calamine::Data;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use regex::Regex;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;

use crate::decode::Decoded;
use crate::models::request::ParseRequest;

pub fn parse_hdfc_statement(
    input: ParseRequest<'_>,
) -> Result<ParseResult<CreditCardAccount>, crate::error::XfinaError> {
    let decoded = Decoded::new(&input);
    parse_decoded(&decoded, &input)
}

/// A row's non-empty cells, each with the column it sits in.
///
/// HDFC lays the workbook out in merged cells, so the same row can carry two
/// tables side by side: the Credit Limit row also holds the past dues figures.
/// A value's column only means something against the header above it.
type Cells = Vec<(usize, String)>;

/// Parses from an already-decoded input, so detection can probe and
/// parse against one read of the file.
pub(crate) fn parse_decoded(
    decoded: &Decoded<'_>,
    input: &ParseRequest<'_>,
) -> Result<ParseResult<CreditCardAccount>, crate::error::XfinaError> {
    let sheet = decoded.sheets()?.first()?;
    // Some other workbook: parsing on would yield an account with no card
    // number and no transactions rather than an honest refusal.
    if !probe(decoded).is_match() {
        return Err(crate::error::XfinaError::InvalidFormat(
            "Not an HDFC credit card statement: no HDFC card headings".to_string(),
        ));
    }

    let filename = input.filename;
    let mut stmt = CreditCardAccount {
        r#type: "credit_card".to_string(),
        ..Default::default()
    };
    stmt.version = 1.1;

    let mut address_parts = Vec::new();
    let mut holder = CcHolder::default();
    let mut xfina_account = XfinaCreditCardAccount {
        institution_name: Some("HDFC Bank".to_string()),
        ..Default::default()
    };

    let mut date_only_paths = Vec::new();

    if let Some(fname) = filename {
        let re = Regex::new(r"(\d{2}-\d{2}-\d{4})").unwrap();
        if let Some(caps) = re.captures(fname) {
            if let Some(m) = caps.get(1) {
                if let Ok(d) = NaiveDate::parse_from_str(m.as_str(), "%d-%m-%Y") {
                    xfina_account.generated_date = Some(crate::models::date_utils::ist_midnight(d));
                    date_only_paths.push("xfina.generatedDate".to_string());
                    // From the filename; cleared below if the statement prints
                    // its own Statement Date.
                    xfina_account.generated_date_derived = Some(true);
                }
            }
        }
    }
    let mut summary = CcSummary::default();
    let mut xfina_summary = CcXfinaSummary::default();

    let mut transactions_list = Vec::new();
    let mut xfina_txns = CcXfinaTransactions::default();

    let mut card_no = String::new();

    let rows: Vec<Cells> = sheet
        .rows()
        .map(cells_of)
        .filter(|r| !r.is_empty())
        .collect();

    enum Section {
        Top,
        AccountCcSummary,
        CcTransactions,
        RewardCcSummary,
        RewardProgram,
        None,
    }
    let mut current_section = Section::Top;

    // The column the header block's values sit in, learned from the Name row.
    let mut value_col: Option<usize> = None;
    let mut txn_cols: Option<TxnColumns> = None;

    for (idx, row) in rows.iter().enumerate() {
        // Section headings lead their row; which column that is differs
        // between templates.
        let first = row[0].1.as_str();
        match first {
            // The table's own header row opens it: some downloads print a
            // "Domestic/ International Transactions" heading above it, and
            // others go straight from the account summary to this row.
            "Transaction type" => {
                current_section = Section::CcTransactions;
                txn_cols = TxnColumns::from_header(row);
                continue;
            }
            "Account Summary" => {
                current_section = Section::AccountCcSummary;
                continue;
            }
            "Reward Points Summary" => {
                current_section = Section::RewardCcSummary;
                continue;
            }
            "Rewards Program Points Summary" => {
                current_section = Section::RewardProgram;
                continue;
            }
            "Cashback Summary" | "GST Summary" => {
                current_section = Section::None;
                continue;
            }
            _ if first.starts_with("State account branch GSTN") => {
                current_section = Section::None;
            }
            _ => {}
        }

        match current_section {
            Section::Top => {
                // The right-hand side of the header block: card number and AAN
                // as "Label: value" in a single cell, and the past dues table.
                // Older templates label the card "Card No" and print no AAN.
                for (_, text) in row {
                    if let Some(v) =
                        labelled(text, "Credit Card No.").or_else(|| labelled(text, "Card No"))
                    {
                        card_no = v.to_string();
                        stmt.masked_acc_number = card_no.clone();
                    } else if let Some(v) = labelled(text, "Alternate Account Number") {
                        xfina_account.aan = Some(v.to_string());
                    }
                }
                if let Some(&(overlimit_col, _)) = row.iter().find(|(_, t)| t == "Overlimit") {
                    // The figures are in the next row reaching this column;
                    // older templates put an Address row in between.
                    if let Some(values) = rows[idx + 1..]
                        .iter()
                        .find(|r| r.iter().any(|(c, _)| *c == overlimit_col))
                    {
                        let due = |label: &str| {
                            row.iter()
                                .find(|(c, t)| *c >= overlimit_col && t.starts_with(label))
                                .and_then(|(c, _)| parse_decimal(at(values, *c)))
                        };
                        xfina_summary.past_dues = Some(PastDues {
                            overlimit: due("Overlimit").unwrap_or_default(),
                            three_months: due("3 Months").unwrap_or_default(),
                            two_months: due("2 Months").unwrap_or_default(),
                            one_month: due("1 Month").unwrap_or_default(),
                        });
                        summary.current_due = due("Current Dues");
                    }
                }

                if first == "Name" && value_col.is_none() {
                    value_col = row.get(1).map(|(c, _)| *c);
                }
                let val = value_col.map(|c| at(row, c)).unwrap_or("");
                // Label case varies between templates: "Credit limit" in
                // older ones, "Credit Limit" in newer.
                match first.to_ascii_lowercase().as_str() {
                    "name" => holder.name = val.to_string(),
                    "address" if !val.is_empty() => address_parts.push(val.to_string()),
                    "payment due date" => summary.due_date = parse_date(val),
                    "statement date" => {
                        let d = parse_date(val);
                        summary.last_statement_date = d;
                        if let Some(date) = d {
                            xfina_account.generated_date =
                                Some(crate::models::date_utils::ist_midnight(date));
                            // The statement printed its own date, so this
                            // is no longer an estimate.
                            xfina_account.generated_date_derived = None;
                            if !date_only_paths.contains(&"xfina.generatedDate".to_string()) {
                                date_only_paths.push("xfina.generatedDate".to_string());
                            }
                        }
                    }
                    "total amount due" => summary.total_due_amount = parse_decimal(val),
                    "minimum amount due" => summary.min_due_amount = parse_decimal(val),
                    "credit limit" => summary.credit_limit = parse_decimal(val),
                    "available limit" => summary.available_credit = parse_decimal(val),
                    "available cash limit" => summary.cash_limit = parse_decimal(val),
                    _ => {}
                }
            }
            Section::AccountCcSummary => {
                // Headings interleave "-", "+" and "=" between the figures;
                // the row beneath carries only the figures, in heading order.
                // ["12,345.67", "2,345.67", "3,456.78", "0.00", "13,456.78"]
                if first == "Opening Bal" {
                    if let Some(values) = rows.get(idx + 1) {
                        if values.len() >= 5 {
                            xfina_summary.opening_balance = parse_decimal(&values[0].1);
                            xfina_summary.payment_credit = parse_decimal(&values[1].1);
                            xfina_summary.purchases_debits = parse_decimal(&values[2].1);
                            summary.finance_charges = parse_decimal(&values[3].1);
                        }
                    }
                }
            }
            Section::CcTransactions => {
                let Some(cols) = &txn_cols else { continue };
                let Some(amount) = parse_decimal(at(row, cols.amount)) else {
                    continue;
                };
                let amount = amount.abs();
                // Add-on holders carry their CKYC ID after the name:
                // "<ADDON HOLDER>       [CKYC ID : 00000000000000 ]"
                let owner = at(row, cols.owner)
                    .split('[')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let txn_dt = parse_datetime(at(row, cols.date));
                let txn_date_naive = txn_dt.map(crate::models::date_utils::ist_date);
                let desc = at(row, cols.description).to_string();
                let ty = cols.debit_credit.map(|c| at(row, c)).unwrap_or("");
                let txn_type = if ty == "Cr" {
                    TransactionType::Credit
                } else {
                    TransactionType::Debit
                };
                let reward_points = cols.rewards.and_then(|c| parse_reward_points(at(row, c)));

                let mut tx_xfina = CcXfinaTransaction {
                    owner: Some(owner),
                    ..Default::default()
                };
                tx_xfina.reward_points = reward_points;

                transactions_list.push(CcTransaction {
                    txn_date: txn_dt,
                    value_date: txn_date_naive,
                    narration: desc,
                    amount,
                    txn_type,
                    txn_id: None,
                    statement_date: None,
                    mcc: None,
                    masked_card_number: None,
                    xfina: Some(tx_xfina),
                });
            }
            Section::RewardCcSummary => {
                // The headings wrap over two rows and do not line up with the
                // figures, so the figures are read in order from the one row
                // that is entirely numbers.
                let values: Vec<i32> = row.iter().filter_map(|(_, t)| parse_i32(t)).collect();
                if values.len() == row.len()
                    && values.len() >= 5
                    && xfina_summary.reward_points_summary.is_none()
                {
                    xfina_summary.reward_points_summary = Some(RewardPointsSummary {
                        opening_balance: values[0],
                        earned: values[1],
                        disbursed: values[2],
                        adjusted_lapsed: values[3],
                        closing_balance: values[4],
                        expiring_in_30_days: values.get(5).copied(),
                        expiring_in_60_days: values.get(6).copied(),
                        // Filled in once every transaction has been read.
                        default_rewards: 0,
                        earned_unaccounted: None,
                    });
                }
            }
            Section::RewardProgram => {
                if row.len() >= 2 && first != "Programs" && first != "Total" {
                    xfina_summary.reward_programs.push(RewardProgram {
                        program: first.to_string(),
                        bonus_points: parse_i32(&row[1].1).unwrap_or(0),
                    });
                }
            }
            Section::None => {}
        }
    }

    if !address_parts.is_empty() {
        holder.address = Some(address_parts.join(", "));
    }

    if !card_no.is_empty() {
        holder.cards = Some(CcCards {
            card: vec![CcCard {
                card_type: CardType::Others,
                primary: TypeChoice::Yes,
                masked_card_number: card_no,
                issued_date: None,
            }],
        });
    }

    stmt.profile = Some(CcProfile {
        holders: CcHolders {
            holder: vec![holder],
        },
    });

    // Compute aggregations
    //
    // Only earned points count: HDFC books a reversal under Adjusted/Lapsed,
    // not against Earned, so netting it in here would stop Earned reconciling.
    let mut default_rewards = 0;
    let mut owner_credit_breakdown = HashMap::new();
    let mut owner_debit_breakdown = HashMap::new();

    for txn in &transactions_list {
        if let Some(ref xfina) = txn.xfina {
            if let Some(pts) = xfina.reward_points.filter(|p| *p > 0) {
                default_rewards += pts;
            }
            let owner = if let Some(o) = &xfina.owner {
                if o.is_empty() {
                    "Unknown".to_string()
                } else {
                    o.clone()
                }
            } else {
                "Unknown".to_string()
            };

            use rust_decimal::prelude::ToPrimitive;
            let amt = txn.amount.to_f64().unwrap_or(0.0);

            if txn.txn_type == TransactionType::Credit {
                *owner_credit_breakdown.entry(owner).or_insert(0.0) += amt;
            } else {
                *owner_debit_breakdown.entry(owner).or_insert(0.0) += amt;
            }
        }
    }

    // Only the current template prints points per transaction. Without them
    // there is nothing to account for Earned with, and a
    // residual of the whole figure would claim a gap that is not there.
    let has_rewards_column = txn_cols.as_ref().is_some_and(|c| c.rewards.is_some());
    let bonus_points: i32 = xfina_summary
        .reward_programs
        .iter()
        .map(|p| p.bonus_points)
        .sum();
    if let Some(ref mut rs) = xfina_summary.reward_points_summary {
        rs.default_rewards = default_rewards;
        if has_rewards_column {
            rs.earned_unaccounted = Some(rs.earned - default_rewards - bonus_points);
        }
    }
    xfina_summary.owner_credit_breakdown = owner_credit_breakdown;
    xfina_summary.owner_debit_breakdown = owner_debit_breakdown;

    let stmt_date_opt = summary.last_statement_date;
    summary.xfina = Some(xfina_summary);
    stmt.summary = Some(summary);

    transactions_list.sort_by_key(|a| a.txn_date);

    let mut txns = CcTransactions::default();

    if let Some(stmt_date) = stmt_date_opt {
        let (start, end) = crate::models::date_utils::derive_statement_period(stmt_date);
        txns.start_date = Some(start);
        txns.end_date = Some(end);
        xfina_txns.start_date_derived = Some(true);
        xfina_txns.end_date_derived = Some(true);
    } else {
        if let Some(first) = transactions_list.first() {
            txns.start_date = first.txn_date.map(crate::models::date_utils::ist_date);
            xfina_txns.start_date_derived = Some(true);
        }
        if let Some(last) = transactions_list.last() {
            txns.end_date = last.txn_date.map(crate::models::date_utils::ist_date);
            xfina_txns.end_date_derived = Some(true);
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

    if let Some(rs) = stmt
        .summary
        .as_ref()
        .and_then(|s| s.xfina.as_ref())
        .and_then(|x| x.reward_points_summary.as_ref())
    {
        // 6. reward_points_closing_match: closing == opening + earned - disbursed - adjusted/lapsed
        validation.summary_level.checks.push(SummaryCheck::declared(
            "reward_points_closing_match",
            Decimal::from(rs.closing_balance),
            Decimal::from(rs.opening_balance + rs.earned - rs.disbursed - rs.adjusted_lapsed),
            None,
        ));

        // 7. reward_points_earned_match: earned == transaction points + bonus points
        //
        // Derived rather than declared: a statement can credit points it does
        // not itemise -- a balance carried over from a replaced card -- and
        // still reconcile, so a gap is a warning. It is still raised, because
        // points the parser failed to read would land in the same gap.
        if let Some(unaccounted) = rs.earned_unaccounted {
            validation.summary_level.checks.push(SummaryCheck::derived(
                "reward_points_earned_match",
                Decimal::from(rs.earned),
                Decimal::from(rs.earned - unaccounted),
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

/// Where each field sits in the transaction table, read from its header row:
/// ["Transaction type", "Primary / Addon Customer Name", "Date & Time",
///  "Description", "REWARDS", "AMT", "Debit / Credit"]
/// Older templates head the date column "DATE" and have no REWARDS column.
struct TxnColumns {
    owner: usize,
    date: usize,
    description: usize,
    amount: usize,
    rewards: Option<usize>,
    debit_credit: Option<usize>,
}

impl TxnColumns {
    fn from_header(header: &Cells) -> Option<Self> {
        let col = |label: &str| {
            header
                .iter()
                .find(|(_, t)| t.to_ascii_lowercase().starts_with(label))
                .map(|(c, _)| *c)
        };
        Some(TxnColumns {
            owner: col("primary / addon")?,
            date: col("date")?,
            description: col("description")?,
            amount: col("amt")?,
            rewards: col("rewards"),
            debit_credit: col("debit"),
        })
    }
}

fn cells_of(row: &[Data]) -> Cells {
    row.iter()
        .enumerate()
        .map(|(col, c)| (col, c.to_string().replace('\u{0}', "").trim().to_string()))
        .filter(|(_, text)| !text.is_empty())
        .collect()
}

/// The text in column `col`, or "" when that cell is empty.
fn at(cells: &Cells, col: usize) -> &str {
    cells
        .iter()
        .find(|(c, _)| *c == col)
        .map(|(_, text)| text.as_str())
        .unwrap_or("")
}

/// The value of a "Label: value" cell, if it carries this label.
fn labelled<'a>(text: &'a str, label: &str) -> Option<&'a str> {
    text.strip_prefix(label)?
        .trim_start()
        .strip_prefix(':')
        .map(str::trim)
}

fn parse_decimal(val: &str) -> Option<Decimal> {
    let clean = val.replace(",", "");
    clean.parse::<Decimal>().ok()
}

fn parse_i32(val: &str) -> Option<i32> {
    let clean = val.replace(",", "");
    clean.parse::<i32>().ok()
}

/// A transaction's reward points: "+ 12" earned, "- 12" reversed.
///
/// HDFC sets the sign apart from the digits, so it has to be closed up before
/// parsing; left in, "- 12" does not parse and the reversal silently vanishes.
fn parse_reward_points(val: &str) -> Option<i32> {
    let compact: String = val
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '+' && *c != ',')
        .collect();
    compact.parse().ok()
}

/// Header dates: "dd Mon, yyyy", or "dd/mm/yyyy" in older templates.
fn parse_date(val: &str) -> Option<NaiveDate> {
    let val = val.trim();
    NaiveDate::parse_from_str(val, "%d %b, %Y")
        .or_else(|_| NaiveDate::parse_from_str(val, "%d/%m/%Y"))
        .ok()
}

/// Transaction times, in IST: "dd/mm/yyyy / hh:mm". Older templates print
/// the date alone.
fn parse_datetime(val: &str) -> Option<DateTime<Utc>> {
    let val = val.trim();
    let ndt = NaiveDateTime::parse_from_str(val, "%d/%m/%Y / %H:%M")
        .ok()
        // Fall back to date-only → midnight IST
        .or_else(|| {
            NaiveDate::parse_from_str(val, "%d/%m/%Y")
                .ok()
                .map(|d| d.and_hms_opt(0, 0, 0).unwrap())
        })?;
    Some(crate::models::date_utils::ist_to_utc(ndt))
}

/// HDFC heads its transaction table with a primary / add-on holder column, and
/// prints an alternate account number beside the card number.
pub(crate) fn probe(dec: &crate::decode::Decoded<'_>) -> crate::detect::Claim {
    use crate::detect::{probe::any_marker, Claim};
    let Ok(sheets) = dec.sheets() else {
        return Claim::NO;
    };
    let head = sheets.head_text(60);
    if any_marker(&head, &["PRIMARY / ADDON CUSTOMER NAME"]) {
        return Claim::strong("hdfc-card-columns");
    }
    if any_marker(&head, &["ALTERNATE ACCOUNT NUMBER:"]) {
        return Claim::weak("hdfc-card-aan");
    }
    Claim::NO
}

#[cfg(test)]
mod tests {
    use super::parse_reward_points;

    #[test]
    fn a_reversal_keeps_its_sign() {
        assert_eq!(parse_reward_points("- 12"), Some(-12));
        assert_eq!(parse_reward_points("-12"), Some(-12));
    }

    #[test]
    fn earned_points_parse_with_or_without_a_plus() {
        assert_eq!(parse_reward_points("+ 12"), Some(12));
        assert_eq!(parse_reward_points("12"), Some(12));
        assert_eq!(parse_reward_points("+ 1,234"), Some(1234));
    }

    #[test]
    fn an_empty_cell_has_no_points() {
        assert_eq!(parse_reward_points(""), None);
        assert_eq!(parse_reward_points("   "), None);
    }
}
