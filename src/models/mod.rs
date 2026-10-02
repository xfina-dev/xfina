pub mod credit_card;
pub use credit_card::*;

pub mod deposit;
pub use deposit::*;

pub mod equity;
pub use equity::*;

pub mod mutual_funds;
pub use mutual_funds::*;

pub fn parse_indian_date(input: &str) -> String {
    let input = input.trim();

    // Extract date and optional time component like "09:41:11"
    let mut ws_parts = input.split_whitespace();
    let date_str = ws_parts.next().unwrap_or("");
    let time_str = ws_parts.next().unwrap_or("");

    // try dd/mm/yyyy or dd-mm-yyyy or dd-Mmm-yyyy
    let parts: Vec<&str> = if date_str.contains('/') {
        date_str.split('/').collect()
    } else {
        date_str.split('-').collect()
    };

    if parts.len() == 3 {
        let day = parts[0].trim();
        let month_str = parts[1].trim();
        let year = parts[2].trim();

        let month = if month_str.chars().all(|c| c.is_ascii_digit()) {
            format!("{:02}", month_str.parse::<u32>().unwrap_or(0))
        } else {
            match month_str.to_lowercase().as_str() {
                "jan" => "01".to_string(),
                "feb" => "02".to_string(),
                "mar" => "03".to_string(),
                "apr" => "04".to_string(),
                "may" => "05".to_string(),
                "jun" => "06".to_string(),
                "jul" => "07".to_string(),
                "aug" => "08".to_string(),
                "sep" => "09".to_string(),
                "oct" => "10".to_string(),
                "nov" => "11".to_string(),
                "dec" => "12".to_string(),
                _ => month_str.to_string(),
            }
        };

        let formatted_day = format!("{:02}", day.parse::<u32>().unwrap_or(0));

        let formatted_year = if year.len() == 2 {
            let year_num = year.parse::<u32>().unwrap_or(0);
            if year_num > 50 {
                format!("19{:02}", year_num)
            } else {
                format!("20{:02}", year_num)
            }
        } else {
            year.to_string()
        };

        let mut iso_date = format!("{}-{}-{}", formatted_year, month, formatted_day);
        if !time_str.is_empty() {
            iso_date.push('T');
            iso_date.push_str(time_str);
        }
        return iso_date;
    }

    input.to_string()
}

pub fn mask_account_number(acc: &str) -> String {
    let acc = acc.trim();
    let len = acc.len();
    if len <= 6 {
        return acc.to_string();
    }
    let first = &acc[0..2];
    let last = &acc[len - 4..len];
    let middle = "X".repeat(len - 6);
    format!("{}{}{}", first, middle, last)
}

/// Honorifics that Indian statements print ahead of an account holder's name.
const HONORIFICS: [&str; 9] = [
    "MR", "MRS", "MS", "MISS", "M/S", "DR", "SHRI", "SMT", "PROF",
];

/// Strips a leading honorific ("MR", "Mrs.", "M/S" ...) off a holder name and
/// collapses the whitespace it leaves behind. Returns `None` when the text does
/// not start with one, so callers can also use this to spot the name line in a
/// statement header.
pub fn strip_honorific(name: &str) -> Option<String> {
    let name = name.trim();
    // `to_ascii_uppercase` keeps byte offsets aligned with `name`.
    let upper = name.to_ascii_uppercase();
    for honorific in HONORIFICS {
        let Some(rest) = upper.strip_prefix(honorific) else {
            continue;
        };
        let rest = rest.strip_prefix('.').unwrap_or(rest);
        // Only a whole token counts as a title: "MRIDULA" is a name, "MR." is not.
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            continue;
        }
        return Some(collapse_whitespace(&name[name.len() - rest.len()..]));
    }
    None
}

/// Cleans up a holder name: drops any leading honorific and squeezes the
/// padding statements use to align the header columns.
pub fn normalize_person_name(name: &str) -> String {
    strip_honorific(name).unwrap_or_else(|| collapse_whitespace(name))
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub mod account;
pub use account::{Account, AccountModel};

pub mod decimal_map;

pub mod rates;
pub use rates::{CurrencyRates, RateSheet};

pub mod series;
pub use series::{
    Coverage, DateRange, Field, Frequency, Gap, InstrumentKind, PricePoint, PriceSeries,
    RangeSource,
};

pub mod parsed;
pub use parsed::Parsed;

pub mod generated_date;
pub use generated_date::HasGeneratedDate;

pub mod schema;
pub use schema::Schema;

pub mod statement;
pub use statement::{Candidate, Confidence, Detection, FileInfo, Statement};

pub mod date_utils;
pub mod serializer;
pub mod txn_order;
pub mod validation;
pub use validation::{
    check_row_balances, ParseResult, RowCheckFailure, RowValidation, SummaryCheck, SummarySource,
    SummaryValidation, ValidationReport, ValidationStatus,
};

pub mod request;
pub use request::ParseRequest;
