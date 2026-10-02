use super::layout::Line;
use crate::models::validation::{
    ParseResult, RowCheckFailure, SummaryCheck, SummarySource, ValidationReport,
};
use crate::models::{
    parse_indian_date, MfHolder, MfHolders, MfHolding, MfHoldings, MfInvestment, MfProfile,
    MfSummary, MfTransaction, MfTransactionType, MfTransactions, MutualFundsAccount,
    XfinaMutualFundsAccount, XfinaMutualFundsHolding, XfinaMutualFundsTransaction,
    XfinaTransactionCategory,
};
use regex::Regex;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Debug, Clone)]
struct ColumnBounds {
    name: &'static str,
    x_lo: f64,
    x_hi: f64,
}

fn detect_columns(lines: &[Line]) -> Option<Vec<ColumnBounds>> {
    for line in lines {
        if line.text.to_lowercase().contains("date")
            && line.text.to_lowercase().contains("transaction")
        {
            let mut words: Vec<(String, f64, f64)> = Vec::new();
            let mut current_word = String::new();
            let mut current_x0 = 0.0;
            let mut current_x1 = 0.0;

            for ch in &line.chars {
                if ch.text.trim().is_empty() {
                    if !current_word.is_empty() {
                        words.push((current_word.clone(), current_x0, current_x1));
                        current_word.clear();
                    }
                    continue;
                }
                if current_word.is_empty() {
                    current_word.push_str(&ch.text);
                    current_x0 = ch.x0;
                    current_x1 = ch.x1;
                } else {
                    if ch.x0 - current_x1 > 3.0 {
                        words.push((current_word.clone(), current_x0, current_x1));
                        current_word = ch.text.clone();
                        current_x0 = ch.x0;
                        current_x1 = ch.x1;
                    } else {
                        current_word.push_str(&ch.text);
                        current_x1 = ch.x1;
                    }
                }
            }
            if !current_word.is_empty() {
                words.push((current_word, current_x0, current_x1));
            }

            let mut cols: HashMap<String, (f64, f64)> = HashMap::new();
            for (w, x0, x1) in words {
                cols.insert(w.to_lowercase(), (x0, x1));
            }

            if cols.contains_key("date")
                && cols.contains_key("transaction")
                && cols.contains_key("amount")
                && cols.contains_key("units")
            {
                let date_x0 = cols["date"].0;
                let txn_x0 = cols["transaction"].0;
                let amt_x1 = cols["amount"].1;
                let units_x1 = cols["units"].1;

                let mut price_x1 = 0.0;
                if let Some(p) = cols.get("price") {
                    price_x1 = p.1;
                } else if let Some(n) = cols.get("nav") {
                    price_x1 = n.1;
                }

                let mut bal_x1 = 0.0;
                if let Some(u) = cols.get("unit") {
                    bal_x1 = u.1;
                } else if let Some(b) = cols.get("balance") {
                    bal_x1 = b.1;
                }

                let mut bounds = vec![
                    ColumnBounds {
                        name: "Date",
                        x_lo: date_x0 - 3.0,
                        x_hi: txn_x0 - 3.0,
                    },
                    ColumnBounds {
                        name: "Transaction",
                        x_lo: txn_x0 - 3.0,
                        x_hi: amt_x1 - 65.0,
                    },
                    ColumnBounds {
                        name: "Amount",
                        x_lo: amt_x1 - 65.0,
                        x_hi: amt_x1 + 3.0,
                    },
                    ColumnBounds {
                        name: "Units",
                        x_lo: amt_x1 + 3.0,
                        x_hi: units_x1 + 3.0,
                    },
                ];
                if price_x1 > 0.0 {
                    bounds.push(ColumnBounds {
                        name: "Price",
                        x_lo: units_x1 + 3.0,
                        x_hi: price_x1 + 3.0,
                    });
                }
                if bal_x1 > 0.0 {
                    let prev_x1 = if price_x1 > 0.0 { price_x1 } else { units_x1 };
                    bounds.push(ColumnBounds {
                        name: "Balance",
                        x_lo: prev_x1 + 3.0,
                        x_hi: 1000.0,
                    });
                }

                return Some(bounds);
            }
        }
    }
    None
}

fn assign_cells(line: &Line, columns: &[ColumnBounds]) -> HashMap<&'static str, String> {
    let mut cells: HashMap<&'static str, Vec<&super::cams::CharItem>> = HashMap::new();
    for col in columns {
        cells.insert(col.name, Vec::new());
    }

    for ch in &line.chars {
        if ch.text.trim().is_empty() {
            continue;
        }
        let x_mid = (ch.x0 + ch.x1) / 2.0;
        for col in columns {
            if x_mid >= col.x_lo && x_mid < col.x_hi {
                cells.get_mut(col.name).unwrap().push(ch);
                break;
            }
        }
    }

    let mut result = HashMap::new();
    for (name, mut chars) in cells {
        if chars.is_empty() {
            continue;
        }
        chars.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap_or(std::cmp::Ordering::Equal));
        let mut parts = Vec::new();
        let mut prev_x1: Option<f64> = None;
        for ch in chars {
            if let Some(px1) = prev_x1 {
                if ch.x0 - px1 > 0.5 {
                    parts.push(" ".to_string());
                }
            }
            parts.push(ch.text.clone());
            prev_x1 = Some(ch.x1);
        }
        result.insert(name, parts.join("").trim().to_string());
    }
    result
}

#[derive(Debug, Clone)]
struct ValidationData {
    pub total_invested: Decimal,
    pub current_value: Decimal,
}

#[derive(Debug)]
enum ParserState {
    OutsideFolio,
    InPortfolioSummary,
    InSchemeHeader {
        folio_no: String,
        buffer: Vec<Line>,
    },
    InSchemeBody {
        holding: Box<MfHolding>,
        buffer: Vec<Line>,
    },
}

fn extract_investor_info(
    pages_lines: &[Vec<Line>],
) -> (Option<String>, Option<String>, Option<String>) {
    if pages_lines.is_empty() {
        return (None, None, None);
    }
    let page_lines = &pages_lines[0]; // CAMS prints this on page 1

    let mut email_seen = false;
    let mut address_lines = Vec::new();
    let mut name = String::new();
    let mut mobile = None;

    let email_re = Regex::new(r"(?i)Email\s*Id\s*:").unwrap();
    let mobile_re = Regex::new(r"(?i)Mobile\s*:\s*([+\d]+)").unwrap();
    let phone_re = Regex::new(r"(?i)^\s*Phone\s+Off\s*:").unwrap();

    for line in page_lines {
        if line.text.trim().is_empty() {
            continue;
        }
        let mut left_text = String::new();
        for ch in &line.chars {
            if ch.x0 < 200.0 {
                left_text.push_str(&ch.text);
            }
        }
        let text = left_text.trim();
        if text.is_empty() {
            continue;
        }
        if email_re.is_match(text) {
            email_seen = true;
            continue;
        }
        if let Some(caps) = mobile_re.captures(text) {
            mobile = Some(caps.get(1).unwrap().as_str().to_string());
            break;
        }
        if !email_seen {
            continue;
        }
        if phone_re.is_match(text) {
            continue;
        }

        if name.is_empty() {
            name = text.to_string();
        } else {
            address_lines.push(text.to_string());
        }
    }

    let address = if !address_lines.is_empty() {
        Some(address_lines.join("\n"))
    } else {
        None
    };
    let parsed_name = if !name.is_empty() { Some(name) } else { None };
    (parsed_name, address, mobile)
}

pub fn parse_cas_lines(
    pages_lines: Vec<Vec<Line>>,
    filename: Option<&str>,
) -> Result<ParseResult<MutualFundsAccount>, crate::error::XfinaError> {
    let mut account = MutualFundsAccount {
        r#type: "mutualfunds".to_string(),
        masked_acc_number: String::new(),
        version: "1.1".to_string(),
        linked_acc_ref: String::new(),
        profile: None,
        summary: None,
        transactions: None,
        xfina: Some(XfinaMutualFundsAccount::default()),
    };

    if let Some(fname) = filename {
        let re = Regex::new(r"_(\d{14})\d*").unwrap();
        if let Some(caps) = re.captures(fname) {
            if let Some(m) = caps.get(1) {
                if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(m.as_str(), "%d%m%Y%H%M%S") {
                    let utc_dt = Some(crate::models::date_utils::ist_to_utc(dt));
                    if let Some(ref mut xfina) = account.xfina {
                        xfina.generated_date = utc_dt;
                        // From the filename, not the statement body.
                        xfina.generated_date_derived = Some(true);
                    }
                }
            }
        }
    }

    let (name_opt, address, mobile) = extract_investor_info(&pages_lines);
    let mut holder = MfHolder {
        name: name_opt.unwrap_or_default(),
        address,
        mobile,
        ..Default::default()
    };

    let mut holdings: Vec<MfHolding> = Vec::new();
    let mut all_transactions: Vec<MfTransaction> = Vec::new();

    let mut validation = ValidationReport::empty();
    let mut statement_start_date = None;
    let mut statement_end_date = None;

    let mut state = ParserState::OutsideFolio;
    let mut portfolio_summary: HashMap<String, ValidationData> = HashMap::new();
    let mut current_amc = String::new();
    let mut current_columns: Option<Vec<ColumnBounds>> = None;

    let date_re = Regex::new(r"^\d{2}-\S{3}-\d{4}").unwrap();
    let folio_re = Regex::new(r"(?i)Folio\s+No\s*:\s*([a-zA-Z0-9/\-]+)").unwrap();
    let pan_re = Regex::new(r"(?i)PAN\s*:\s*([A-Z]{5}\d{4}[A-Z])").unwrap();
    let kyc_re = Regex::new(r"(?i)KYC\s*:\s*(\S+(?:\s+OK)?)").unwrap();
    let pan_kyc_re = Regex::new(r"(?i)PAN\s*:\s*(OK)").unwrap();

    let mut current_kyc: Option<String> = None;
    let mut current_pan_kyc: Option<String> = None;

    // Parse loop over lines
    for lines in pages_lines {
        for line in lines {
            if let Some(cols) = detect_columns(std::slice::from_ref(&line)) {
                current_columns = Some(cols);
            }
            let text = line.text.trim();
            if text.is_empty() {
                continue;
            }
            let lower_text = text.to_lowercase();

            // Statement dates
            if statement_start_date.is_none() && lower_text.contains(" to ") {
                let parts: Vec<&str> = lower_text.split(" to ").collect();
                if parts.len() == 2
                    && date_re.is_match(parts[0].trim())
                    && date_re.is_match(parts[1].trim())
                {
                    let original_parts: Vec<&str> = text.splitn(2, ['T', 't']).collect();
                    if original_parts.len() == 2 {
                        let p1 = original_parts[0].trim().to_string();
                        let p2 = original_parts[1][1..].trim().to_string();
                        if date_re.is_match(&p1) {
                            statement_start_date = Some(parse_indian_date(&p1));
                            statement_end_date = Some(parse_indian_date(&p2));
                        }
                    }
                }
            }

            // Global matches
            if lower_text == "portfolio summary" {
                state = ParserState::InPortfolioSummary;
                continue;
            }

            if (lower_text.ends_with(" mutual fund")
                || lower_text.ends_with(" mf")
                || lower_text.ends_with(" fund house"))
                && !matches!(state, ParserState::InPortfolioSummary)
            {
                current_amc = text.to_string();
            }

            if let Some(caps) = folio_re.captures(text) {
                let folio_no = caps.get(1).unwrap().as_str().trim().to_string();
                if let Some(pan_caps) = pan_re.captures(text) {
                    if holder.pan.is_none() {
                        holder.pan = Some(pan_caps.get(1).unwrap().as_str().to_string());
                    }
                }

                if let Some(kyc_caps) = kyc_re.captures(text) {
                    current_kyc = Some(kyc_caps.get(1).unwrap().as_str().to_string());
                }
                // Check if PAN: OK exists too
                if let Some(pan_kyc_caps) = pan_kyc_re.captures(text) {
                    current_pan_kyc = Some(pan_kyc_caps.get(1).unwrap().as_str().to_string());
                }

                state = ParserState::InSchemeHeader {
                    folio_no,
                    buffer: Vec::new(),
                };
                continue;
            }

            match &mut state {
                ParserState::OutsideFolio => {
                    // Do nothing
                }
                ParserState::InPortfolioSummary => {
                    if lower_text.starts_with("total ") {
                        let parts: Vec<&str> = text.split_whitespace().collect();
                        if parts.len() >= 3 {
                            let len = parts.len();
                            let current_str = parts[len - 1].replace(",", "");
                            let invested_str = parts[len - 2].replace(",", "");
                            if let (Ok(current), Ok(invested)) = (
                                Decimal::from_str(&current_str),
                                Decimal::from_str(&invested_str),
                            ) {
                                portfolio_summary.insert(
                                    "TOTAL_PORTFOLIO".to_string(),
                                    ValidationData {
                                        total_invested: invested,
                                        current_value: current,
                                    },
                                );
                            }
                        }
                        state = ParserState::OutsideFolio;
                    } else {
                        let parts: Vec<&str> = text.split_whitespace().collect();
                        if parts.len() >= 3 {
                            let len = parts.len();
                            let current_str = parts[len - 1].replace(",", "");
                            let invested_str = parts[len - 2].replace(",", "");

                            if let (Ok(current), Ok(invested)) = (
                                Decimal::from_str(&current_str),
                                Decimal::from_str(&invested_str),
                            ) {
                                let fund_name = parts[0..len - 2].join(" ");
                                let fund_name_lower = fund_name.to_lowercase();
                                if fund_name_lower.contains("mutual")
                                    || fund_name_lower.contains("fund")
                                    || fund_name_lower.contains("mf")
                                    || fund_name_lower.contains("amc")
                                {
                                    portfolio_summary.insert(
                                        fund_name,
                                        ValidationData {
                                            total_invested: invested,
                                            current_value: current,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
                ParserState::InSchemeHeader { folio_no, buffer } => {
                    if lower_text.starts_with("opening unit balance:") {
                        let header_strings: Vec<String> =
                            buffer.iter().map(|l| l.text.clone()).collect();
                        let mut holding = parse_scheme_header(
                            folio_no.clone(),
                            current_amc.clone(),
                            &header_strings,
                        );

                        let parts: Vec<&str> = text.split_whitespace().collect();
                        for (i, p) in parts.iter().enumerate() {
                            if p.to_lowercase() == "balance:" && i + 1 < parts.len() {
                                let val_str = parts[i + 1].replace(",", "");
                                if let Ok(val) = Decimal::from_str(&val_str) {
                                    if let Some(x) = holding.xfina.as_mut() {
                                        x.opening_balance = val;
                                    }
                                }
                                break;
                            }
                        }
                        if let Some(ref mut xfina) = holding.xfina {
                            xfina.kyc = current_kyc.clone();
                            xfina.pan_kyc = current_pan_kyc.clone();
                        }

                        state = ParserState::InSchemeBody {
                            holding: Box::new(holding),
                            buffer: vec![line.clone()],
                        };
                    } else {
                        buffer.push(line.clone());
                    }
                }
                ParserState::InSchemeBody { holding, buffer } => {
                    if lower_text.starts_with("closing unit balance:") {
                        let mut scheme_transactions = parse_transactions(
                            holding,
                            buffer,
                            current_columns.as_deref(),
                            &mut validation,
                        );

                        let mut period_buy_units = Decimal::ZERO;
                        let mut period_sell_units = Decimal::ZERO;
                        let mut period_buy_count = 0;
                        let mut period_sell_count = 0;

                        for txn in &scheme_transactions {
                            if let Some(x_txn) = &txn.xfina {
                                let units = x_txn.units;
                                let abs_units = units.abs();

                                if txn.r#type == Some(MfTransactionType::Buy)
                                    || (txn.r#type != Some(MfTransactionType::Sell)
                                        && units > Decimal::ZERO)
                                {
                                    period_buy_units += abs_units;
                                    period_buy_count += 1;
                                } else if txn.r#type == Some(MfTransactionType::Sell)
                                    || (txn.r#type != Some(MfTransactionType::Buy)
                                        && units < Decimal::ZERO)
                                {
                                    period_sell_units += abs_units;
                                    period_sell_count += 1;
                                }
                            }
                        }

                        if let Some(x) = holding.xfina.as_mut() {
                            x.period_buy_units = period_buy_units;
                            x.period_sell_units = period_sell_units;
                            x.period_buy_count = period_buy_count;
                            x.period_sell_count = period_sell_count;
                        }

                        all_transactions.append(&mut scheme_transactions);

                        let parts: Vec<&str> = text.split_whitespace().collect();
                        for (i, p) in parts.iter().enumerate() {
                            let parse_dec = |s: &str| -> Decimal {
                                Decimal::from_str(&s.replace(",", "")).unwrap_or_default()
                            };

                            if p.to_lowercase() == "balance:" && i + 1 < parts.len() {
                                holding.units = parse_dec(parts[i + 1]);
                                holding.closing_units = holding.units;
                            }
                            if p.to_lowercase() == "value:"
                                && i > 0
                                && parts[i - 1].to_lowercase() == "cost"
                                && i + 1 < parts.len()
                            {
                                if let Some(x) = holding.xfina.as_mut() {
                                    x.total_invested = parse_dec(parts[i + 1]);
                                }
                            }
                            if p.to_lowercase() == "nav"
                                && i + 2 < parts.len()
                                && parts[i + 1].to_lowercase() == "on"
                            {
                                if let Some(x) = holding.xfina.as_mut() {
                                    let date_str =
                                        parse_indian_date(&parts[i + 2].replace(":", ""));
                                    x.nav_date = date_str.parse().ok();
                                }
                            }

                            if p.to_lowercase() == "inr" && i + 1 < parts.len() {
                                let val = parse_dec(parts[i + 1]);
                                let mut is_nav = false;
                                let mut is_market = false;
                                for j in 1..=4 {
                                    if i >= j {
                                        if parts[i - j].to_lowercase() == "nav" {
                                            is_nav = true;
                                            break;
                                        }
                                        if parts[i - j].to_lowercase() == "market" {
                                            is_market = true;
                                            break;
                                        }
                                    }
                                }

                                if is_nav {
                                    holding.nav = val;
                                    holding.rate = val;
                                } else if is_market {
                                    if let Some(x) = holding.xfina.as_mut() {
                                        x.current_value = val;
                                        x.unrealized_pl = val - x.total_invested;
                                    }
                                }
                            }
                        }

                        holdings.push(*holding.clone());
                        let folio = holding.folio_no.clone().unwrap_or_default();
                        state = ParserState::InSchemeHeader {
                            folio_no: folio,
                            buffer: Vec::new(),
                        };
                    } else if lower_text.starts_with("folio no") {
                        if let Some(caps) = folio_re.captures(text) {
                            let folio_no = caps.get(1).unwrap().as_str().trim().to_string();
                            if let Some(kyc_caps) = kyc_re.captures(text) {
                                current_kyc = Some(kyc_caps.get(1).unwrap().as_str().to_string());
                            }
                            if let Some(pan_kyc_caps) = pan_kyc_re.captures(text) {
                                current_pan_kyc =
                                    Some(pan_kyc_caps.get(1).unwrap().as_str().to_string());
                            }
                            state = ParserState::InSchemeHeader {
                                folio_no,
                                buffer: Vec::new(),
                            };
                        }
                    } else if let Some(cols) = detect_columns(std::slice::from_ref(&line)) {
                        current_columns = Some(cols);
                    } else {
                        buffer.push(line.clone());
                    }
                }
            }
        }
    }

    if !holder.name.is_empty() {
        account.profile = Some(MfProfile {
            holders: MfHolders {
                r#type: None,
                holder: vec![holder],
            },
        });
    }

    if !holdings.is_empty() {
        account.summary = Some(MfSummary {
            investment_value: holdings
                .iter()
                .map(|h| h.xfina.as_ref().map_or(Decimal::ZERO, |x| x.total_invested))
                .sum(),
            current_value: holdings
                .iter()
                .map(|h| h.xfina.as_ref().map_or(Decimal::ZERO, |x| x.current_value))
                .sum(),
            investment: MfInvestment {
                holdings: MfHoldings {
                    holding: holdings.clone(),
                },
            },
        });
    }

    if !all_transactions.is_empty() {
        account.transactions = Some(MfTransactions {
            start_date: statement_start_date.and_then(|d| d.parse().ok()),
            end_date: statement_end_date.and_then(|d| d.parse().ok()),
            transaction: all_transactions,
        });
    }

    // VALIDATION

    if !portfolio_summary.is_empty() {
        let mut computed_summary: HashMap<String, ValidationData> = HashMap::new();
        for h in &holdings {
            let amc = h.amc.clone().unwrap_or_default();
            let invested = h.xfina.as_ref().map_or(Decimal::ZERO, |x| x.total_invested);
            let current = h.xfina.as_ref().map_or(Decimal::ZERO, |x| x.current_value);

            let e = computed_summary.entry(amc).or_insert(ValidationData {
                total_invested: Decimal::ZERO,
                current_value: Decimal::ZERO,
            });
            e.total_invested += invested;
            e.current_value += current;
        }

        let mut overall_computed = ValidationData {
            total_invested: Decimal::ZERO,
            current_value: Decimal::ZERO,
        };
        // Iterate in a deterministic (sorted) order: HashMap iteration order is
        // randomized per process, which would otherwise make the emitted check
        // order - and therefore the serialized JSON - nondeterministic across runs.
        let mut sorted_summary: Vec<(&String, &ValidationData)> =
            portfolio_summary.iter().collect();
        sorted_summary.sort_by(|a, b| a.0.cmp(b.0));
        for (amc, summary) in sorted_summary {
            if amc == "TOTAL_PORTFOLIO" {
                continue;
            }
            if let Some(computed) = computed_summary.get(amc) {
                overall_computed.total_invested += computed.total_invested;
                overall_computed.current_value += computed.current_value;
                // Invested validation
                validation.summary_level.checks.push(SummaryCheck {
                    name: format!("{}_invested_match", amc)
                        .replace(' ', "_")
                        .to_lowercase(),
                    passed: (summary.total_invested - computed.total_invested).abs()
                        <= Decimal::from(1),
                    source: SummarySource::Declared,
                    declared: Some(summary.total_invested),
                    computed: computed.total_invested,
                    delta: Some(summary.total_invested - computed.total_invested),
                    note: Some(format!("amc: {}", amc)),
                });

                // Current Value validation
                validation.summary_level.checks.push(SummaryCheck {
                    name: format!("{}_value_match", amc)
                        .replace(' ', "_")
                        .to_lowercase(),
                    passed: (summary.current_value - computed.current_value).abs()
                        <= Decimal::from(1),
                    source: SummarySource::Declared,
                    declared: Some(summary.current_value),
                    computed: computed.current_value,
                    delta: Some(summary.current_value - computed.current_value),
                    note: Some(format!("amc: {}", amc)),
                });
            } else {
                // Missing AMC in computed - register a failure
                validation.summary_level.checks.push(SummaryCheck {
                    name: format!("{}_invested_match", amc)
                        .replace(' ', "_")
                        .to_lowercase(),
                    passed: false,
                    source: SummarySource::Declared,
                    declared: Some(summary.total_invested),
                    computed: Decimal::ZERO,
                    delta: Some(summary.total_invested),
                    note: Some(format!("amc: {} (no parsed schemes found)", amc)),
                });
            }
        }

        if let Some(total_decl) = portfolio_summary.get("TOTAL_PORTFOLIO") {
            validation.summary_level.checks.push(SummaryCheck {
                name: "overall_invested_match".to_string(),
                passed: (total_decl.total_invested - overall_computed.total_invested).abs()
                    <= Decimal::from(1),
                source: SummarySource::Declared,
                declared: Some(total_decl.total_invested),
                computed: overall_computed.total_invested,
                delta: Some(total_decl.total_invested - overall_computed.total_invested),
                note: Some("overall portfolio".to_string()),
            });
            validation.summary_level.checks.push(SummaryCheck {
                name: "overall_value_match".to_string(),
                passed: (total_decl.current_value - overall_computed.current_value).abs()
                    <= Decimal::from(1),
                source: SummarySource::Declared,
                declared: Some(total_decl.current_value),
                computed: overall_computed.current_value,
                delta: Some(total_decl.current_value - overall_computed.current_value),
                note: Some("overall portfolio".to_string()),
            });
        }
    }

    validation.summary_level.passed = validation.summary_level.checks.iter().all(|c| c.passed);
    validation.finalize();

    Ok(ParseResult {
        data: account,
        validation,
    })
}

fn parse_scheme_header(folio_no: String, amc: String, buffer: &[String]) -> MfHolding {
    let mut holding = MfHolding {
        folio_no: Some(folio_no),
        amc: if amc.is_empty() { None } else { Some(amc) },
        xfina: Some(XfinaMutualFundsHolding::default()),
        ..Default::default()
    };

    let header_text = buffer.join(" ");

    // 1. Isin extraction
    let isin_re = Regex::new(r"(?i)ISIN\s*:\s*([A-Z0-9]{12})").unwrap();
    if let Some(caps) = isin_re.captures(&header_text) {
        holding.isin = Some(caps.get(1).unwrap().as_str().to_string());
    } else {
        let isin_loose_re = Regex::new(r"(?i)ISIN\s*:\s*([A-Z0-9]+)").unwrap();
        if let Some(caps) = isin_loose_re.captures(&header_text) {
            let partial = caps.get(1).unwrap().as_str();
            if partial.len() < 12 {
                let mut continuation = String::new();
                for (i, line) in buffer.iter().enumerate() {
                    if isin_loose_re.is_match(line) {
                        if let Some(next_line) = buffer.get(i + 1) {
                            continuation = next_line
                                .trim()
                                .chars()
                                .take_while(|c| c.is_alphanumeric())
                                .collect();
                        }
                        break;
                    }
                }
                if continuation.is_empty() {
                    let after = &header_text[caps.get(0).unwrap().end()..];
                    let next_word = after.split_whitespace().next().unwrap_or("");
                    continuation = next_word
                        .chars()
                        .take_while(|c| c.is_alphanumeric())
                        .collect();
                }
                let mut full = partial.to_string();
                full.push_str(&continuation);
                if full.len() == 12 {
                    holding.isin = Some(full);
                }
            }
        }
    }

    // 2. Extract Nominees, Advisor, Registrar
    let mut clean_text = header_text.clone();

    let nom_ext_re =
        Regex::new(r"(?i)Nominee\s+1\s*:\s*([a-zA-Z0-9\s]+?)(?:\s+Nominee\s+2|$)").unwrap();
    if let Some(caps) = nom_ext_re.captures(&clean_text) {
        let mut nominees = Vec::new();
        if let Some(n1) = caps.get(1) {
            nominees.push(n1.as_str().trim().to_string());
        }
        holding.xfina.as_mut().unwrap().nominees = Some(nominees);
    }
    let nom_strip_re = Regex::new(r"(?i)Nominee\s+1\s*:\s*[a-zA-Z0-9\s]+").unwrap();
    clean_text = nom_strip_re.replace(&clean_text, "").to_string();

    let adv_ext_re =
        Regex::new(r"(?i)(?:\()?Advisor\s*:\s*(?:Registrar\s*:\s*)?(.*?)(?:\)|Registrar|$)")
            .unwrap();
    if let Some(caps) = adv_ext_re.captures(&clean_text) {
        let adv_str = caps.get(1).unwrap().as_str().trim();
        holding.xfina.as_mut().unwrap().advisor = Some(adv_str.to_string());

        // Strip out the parsed advisor block from clean_text
        let escaped_adv = regex::escape(adv_str);
        // Be careful not to wipe out "Registrar:" if it wasn't swallowed
        let adv_strip_re = Regex::new(&format!(
            r"(?i)(?:\()?Advisor\s*:\s*(?:Registrar\s*:\s*)?{}\s*(?:\))?",
            escaped_adv
        ))
        .unwrap();
        clean_text = adv_strip_re.replace(&clean_text, "").to_string();
    }

    let mut registrar = None;
    let upper_text = clean_text.to_uppercase();
    if upper_text.contains("KFINTECH") {
        registrar = Some("KFINTECH".to_string());
    } else if upper_text.contains("KARVY") {
        registrar = Some("KARVY".to_string());
    } else if upper_text.contains("CAMS") {
        registrar = Some("CAMS".to_string());
    } else if upper_text.contains("FRANKLIN") {
        registrar = Some("FRANKLIN".to_string());
    }
    holding.registrar = registrar;

    let reg_strip_re = Regex::new(r"(?i)Registrar\s*:\s*[A-Za-z]+").unwrap();
    clean_text = reg_strip_re.replace(&clean_text, "").to_string();

    let isin_strip_re = Regex::new(r"(?i)ISIN\s*:\s*[A-Z0-9]+").unwrap();
    clean_text = isin_strip_re.replace(&clean_text, "").to_string();

    // 3. Scheme Code
    let code_re = Regex::new(r"^\s*([A-Za-z0-9]+)\s*-").unwrap();
    let mut scheme_code = String::new();
    let mut raw_name = clean_text.clone();

    for line in buffer {
        if let Some(caps) = code_re.captures(line) {
            let code = caps.get(1).unwrap().as_str().trim();
            if code.chars().any(|c| c.is_ascii_alphabetic()) {
                scheme_code = code.to_string();
                let strip_code_re =
                    Regex::new(&format!(r"^.*?{}\s*-", regex::escape(code))).unwrap();
                raw_name = strip_code_re.replace(&clean_text, "").to_string();
                break;
            }
        }
    }
    if !scheme_code.is_empty() {
        holding.scheme_code = Some(scheme_code);
    }

    // Clean scheme name
    clean_text = Regex::new(r"(?i)\((formerly|erstwhile).+?\)")
        .unwrap()
        .replace_all(&raw_name, "")
        .to_string();
    clean_text = Regex::new(r"(?i)\((Demat|Non-Demat|Non Demat).*")
        .unwrap()
        .replace_all(&clean_text, "")
        .to_string();
    clean_text = Regex::new(r"\s+")
        .unwrap()
        .replace_all(&clean_text, " ")
        .to_string();
    clean_text = Regex::new(r"[^a-zA-Z0-9_)]+$")
        .unwrap()
        .replace_all(&clean_text, "")
        .to_string();
    // Normalize to Title Case so KFINTECH all-caps names match CAMS style.
    // Preserve known acronyms that should stay uppercase.
    let acronyms: &[&str] = &[
        // Fund types / categories
        "ELSS", "SIP", "NAV", "NFO", "ETF", "FOF", "IDCW", "STP", "SWP", "ULIP", "SEBI", "RBI",
        "NRI", // Indices & exchanges
        "NIFTY", "BSE", "NSE", "NASDAQ", "US", // Institutions (AMCs, banks)
        "HDFC", "ICICI", "ICICI", "SBI", "IDBI", "IDFC", "UTI", "DSP", "JM", "LIC", "ITI", "NJ",
        "PGIM", "BOI", "HSBC",
    ];
    let normalized: String = clean_text
        .split_whitespace()
        .map(|word| {
            // Strip trailing punctuation for acronym check
            let core = word.trim_end_matches(|c: char| !c.is_alphanumeric());
            let upper_core = core.to_uppercase();
            if acronyms.contains(&upper_core.as_str()) {
                // Keep the acronym uppercase, re-attach any trailing punctuation
                let suffix = &word[core.len()..];
                format!("{}{}", upper_core, suffix)
            } else {
                // Title-case: uppercase first char, lowercase the rest
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(c) => {
                        c.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                    }
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    holding.xfina.as_mut().unwrap().scheme_name = Some(normalized);

    holding
}

fn parse_transactions(
    holding: &MfHolding,
    buffer: &[Line],
    columns: Option<&[ColumnBounds]>,
    validation: &mut ValidationReport,
) -> Vec<MfTransaction> {
    let mut transactions: Vec<MfTransaction> = Vec::new();
    let date_re = Regex::new(r"^\d{2}-\S{3}-\d{4}").unwrap();
    let space_fix_re1 = Regex::new(r"([a-z])([A-Z])").unwrap();
    let space_fix_re2 = Regex::new(r"([a-zA-Z])(\()").unwrap();
    let space_fix_re3 = Regex::new(r"([a-z])([0-9])").unwrap();
    let mut running_balance = holding
        .xfina
        .as_ref()
        .map(|x| x.opening_balance)
        .unwrap_or_default();

    for line in buffer {
        let is_fee_row = line.text.contains("*** Stamp Duty ***")
            || line.text.contains("*** STT ***")
            || line.text.contains("*** STT Paid ***");
        if is_fee_row {
            let fee = if let Some(cols) = columns {
                // Use the column layout to read the fee from the Amount column,
                // avoiding stray trailing digits that can appear in line.text.
                let cells = assign_cells(line, cols);
                let amt_str = cells.get("Amount").unwrap_or(&"".to_string()).to_string();
                let first_tok = amt_str.split_whitespace().next().unwrap_or("");
                Decimal::from_str(&first_tok.replace(",", ""))
                    .unwrap_or(Decimal::ZERO)
                    .abs()
            } else {
                // Fallback: find the first decimal-looking token (xx.xx) in the line,
                // skipping single-digit stray characters.
                let parts: Vec<&str> = line.text.split_whitespace().collect();
                let mut found = Decimal::ZERO;
                for part in &parts {
                    let clean = part.replace(",", "");
                    if clean.contains('.') {
                        if let Ok(val) = Decimal::from_str(&clean) {
                            if !val.is_zero() {
                                found = val.abs();
                                break;
                            }
                        }
                    }
                }
                found
            };
            if !fee.is_zero() {
                if let Some(txn) = transactions.last_mut() {
                    if let Some(x) = &mut txn.xfina {
                        x.fees = fee;
                    }
                }
            }
            continue;
        }

        let Some(cols) = columns else {
            continue;
        };
        let cells = assign_cells(line, cols);

        let date_str = cells.get("Date").unwrap_or(&"".to_string()).to_string();
        if !date_re.is_match(&date_str) {
            let mut desc = cells
                .get("Transaction")
                .unwrap_or(&"".to_string())
                .trim()
                .to_string();
            let raw_text_lower = line.text.to_lowercase();

            // Skip CAMS page headers that appear at the top of every page after a page break:
            //   - "Consolidated Account Statement"
            //   - Statement date range: "01-Jan-2016 To 01-Aug-2026"
            //   - Column header row: "Date Transaction Amount Units Price Unit Balance"
            //   - Sub-header "(INR) (INR) Balance"
            let is_page_header = raw_text_lower.contains("consolidated account")
                || raw_text_lower.contains("(inr)")
                || raw_text_lower.starts_with("page ")
                || (raw_text_lower.contains(" to ") && {
                    // Date range line: both sides of " to " look like dates
                    let parts: Vec<&str> = line.text.splitn(2, ['T', 't']).collect();
                    parts.len() == 2 && date_re.is_match(parts[0].trim())
                })
                || (desc.to_lowercase().starts_with("date")
                    && raw_text_lower.contains("transaction")
                    && raw_text_lower.contains("amount"));
            if is_page_header {
                continue;
            }

            // Skip annotation-only lines (e.g. "***Stamp Duty***", "***SIP Registered***",
            // "***Invalid Purchase...(Reversal...***") — they are event notices, not transaction narrations.
            let desc_trimmed = desc.trim();
            let is_annotation = (desc_trimmed.starts_with("***") || desc_trimmed.ends_with("***"))
                && !cells
                    .get("Amount")
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false)
                && !cells
                    .get("Units")
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false);
            if is_annotation {
                continue;
            }

            if !desc.is_empty() {
                desc = space_fix_re1.replace_all(&desc, "$1 $2").to_string();
                desc = space_fix_re2.replace_all(&desc, "$1 $2").to_string();
                desc = space_fix_re3.replace_all(&desc, "$1 $2").to_string();

                if let Some(txn) = transactions.last_mut() {
                    if let Some(existing_desc) = &mut txn.narration {
                        existing_desc.push(' ');
                        existing_desc.push_str(&desc);
                    }
                }
            }
            continue;
        }

        let mut desc = cells
            .get("Transaction")
            .unwrap_or(&"".to_string())
            .to_string();
        if desc.is_empty() {
            continue;
        }

        // CAMS PDFs often omit spaces in transaction descriptions (e.g., "SystematicInvestmentNewPurchasewithSIP(1)").
        // We insert a space between lowercase and uppercase letters, or before a parenthesis/number if they are stuck together.
        desc = space_fix_re1.replace_all(&desc, "$1 $2").to_string();
        desc = space_fix_re2.replace_all(&desc, "$1 $2").to_string();
        desc = space_fix_re3.replace_all(&desc, "$1 $2").to_string();

        let parse_dec = |s: &str| -> Option<Decimal> {
            let first_token = s.split_whitespace().next().unwrap_or("");
            Decimal::from_str(
                &first_token
                    .replace(",", "")
                    .replace("(", "-")
                    .replace(")", ""),
            )
            .ok()
        };

        let amt = parse_dec(cells.get("Amount").unwrap_or(&"".to_string()));
        let units = parse_dec(cells.get("Units").unwrap_or(&"".to_string()));
        let nav = parse_dec(cells.get("Price").unwrap_or(&"".to_string()));
        let bal = parse_dec(cells.get("Balance").unwrap_or(&"".to_string()));

        if amt.is_none() && units.is_none() {
            continue;
        }

        let (xfina_cat, div_rate) = categorize_transaction(&desc, units);

        let tx_type = match xfina_cat {
            XfinaTransactionCategory::Redemption
            | XfinaTransactionCategory::SwitchOut
            | XfinaTransactionCategory::SwitchOutMerger
            | XfinaTransactionCategory::StpOut
            | XfinaTransactionCategory::GiftOut
            | XfinaTransactionCategory::Reversal => MfTransactionType::Sell,
            XfinaTransactionCategory::Purchase
            | XfinaTransactionCategory::PurchaseSip
            | XfinaTransactionCategory::SwitchIn
            | XfinaTransactionCategory::SwitchInMerger
            | XfinaTransactionCategory::StpIn
            | XfinaTransactionCategory::GiftIn
            | XfinaTransactionCategory::DividendReinvest => MfTransactionType::Buy,
            _ => MfTransactionType::Others,
        };

        let date_parsed = parse_indian_date(&date_str);

        let xfina = XfinaMutualFundsTransaction {
            units: units.unwrap_or_default(),
            transaction_category: Some(xfina_cat),
            dividend_rate: div_rate,
            folio_no: holding.folio_no.clone(),
            ..Default::default()
        };
        let t = MfTransaction {
            isin: holding.isin.clone(),
            amount: amt.unwrap_or_default(),
            nav: nav.unwrap_or_default(),
            closing_units: bal.unwrap_or_default(),
            r#type: Some(tx_type),
            narration: Some(desc),
            order_date: date_parsed.parse().ok(),
            execution_date: date_parsed.parse().ok(),
            xfina: Some(xfina),
            ..Default::default()
        };

        if let Some(u) = units {
            running_balance += u;
        }

        if let Some(printed) = bal {
            let passed = (running_balance - printed).abs() <= Decimal::new(5, 3);
            if !passed {
                let s_name = holding
                    .xfina
                    .as_ref()
                    .and_then(|x| x.scheme_name.clone())
                    .unwrap_or_default();
                eprintln!("WARNING: Row-by-row checksum mismatch in {}. Computed {}, Printed {}. Resyncing.", s_name, running_balance, printed);
                running_balance = printed;
            }
            validation.row_level.checked_rows += 1;
            if !passed {
                validation.row_level.passed = false;
                validation.row_level.failed_rows.push(RowCheckFailure {
                    row_index: validation.row_level.checked_rows - 1,
                    narration: t.narration.clone().unwrap_or_default(),
                    expected_balance: running_balance,
                    actual_balance: printed,
                    delta: running_balance - printed,
                });
            }
        }

        transactions.push(t);
    }

    transactions
}

fn categorize_transaction(
    description: &str,
    units: Option<Decimal>,
) -> (XfinaTransactionCategory, Option<Decimal>) {
    let mut dividend_rate = None;
    let desc = description.to_lowercase();

    let div_re = Regex::new(r"(?i)(?:div\.|dividend|idcw).*?@\s*rs\.\s*([\d\.]+)").unwrap();
    let reinvest_re = Regex::new(r"(?i)reinvest").unwrap();
    let stp_re = Regex::new(r"(?i)\bs\s*t\s*p\b|systematic\s+transfer").unwrap();

    if let Some(caps) = div_re.captures(&desc) {
        if let Ok(rate) = Decimal::from_str(caps.get(1).unwrap().as_str()) {
            dividend_rate = Some(rate);
        }
        let cat = if reinvest_re.is_match(&desc) {
            XfinaTransactionCategory::DividendReinvest
        } else {
            XfinaTransactionCategory::DividendPayout
        };
        return (cat, dividend_rate);
    }

    let txn_type = if let Some(u) = units {
        if u > Decimal::ZERO {
            if desc.contains("gift") {
                XfinaTransactionCategory::GiftIn
            } else if desc.contains("switch") || stp_re.is_match(&desc) {
                if desc.contains("merger") {
                    XfinaTransactionCategory::SwitchInMerger
                } else {
                    XfinaTransactionCategory::SwitchIn
                }
            } else if desc.contains("segregat") {
                XfinaTransactionCategory::Segregation
            } else if desc.contains("sip")
                || desc.contains("systematic")
                || desc.contains("instalment")
                || desc.contains("installment")
                || (desc.contains("sys") && desc.contains("invest"))
            {
                XfinaTransactionCategory::PurchaseSip
            } else {
                XfinaTransactionCategory::Purchase
            }
        } else if u < Decimal::ZERO {
            if desc.contains("gift") {
                XfinaTransactionCategory::GiftOut
            } else if desc.contains("reversal")
                || desc.contains("rejection")
                || desc.contains("dishonoured")
                || desc.contains("mismatch")
                || desc.contains("insufficient balance")
                || desc.contains("payment not received")
            {
                XfinaTransactionCategory::Reversal
            } else if desc.contains("switch") || stp_re.is_match(&desc) {
                if desc.contains("merger") {
                    XfinaTransactionCategory::SwitchOutMerger
                } else {
                    XfinaTransactionCategory::SwitchOut
                }
            } else {
                XfinaTransactionCategory::Redemption
            }
        } else {
            XfinaTransactionCategory::Unknown
        }
    } else {
        if desc.contains("stt") {
            XfinaTransactionCategory::SttTax
        } else if desc.contains("stamp") {
            XfinaTransactionCategory::StampDutyTax
        } else if desc.contains("tds") {
            XfinaTransactionCategory::TdsTax
        } else {
            XfinaTransactionCategory::Misc
        }
    };

    (txn_type, dividend_rate)
}
