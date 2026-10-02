use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;

use crate::models::date_utils::{ist, ist_midnight};

/// A date with no time, as the instant it names: midnight IST.
///
/// The same convention as every timestamp a parser builds, so one account's
/// dates are all on the same footing -- a transaction stamped at the start of
/// its day and the statement period that contains it can be compared, and a
/// consumer reading these epochs in IST gets the printed date back for every
/// field. Midnight UTC would put date-only fields 5h30m ahead of the
/// timestamps beside them, on the same calendar day but not the same instant.
pub fn naive_date_to_epoch(d: NaiveDate) -> i64 {
    ist_midnight(d).timestamp()
}

pub fn transform_to_xfina(val: &mut Value) {
    match val {
        Value::Object(map) => {
            for v in map.values_mut() {
                transform_to_xfina(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                transform_to_xfina(v);
            }
        }
        Value::String(s) => {
            if let Ok(ndt) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                *val = Value::Number(serde_json::Number::from(naive_date_to_epoch(ndt)));
            } else if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
                *val = Value::Number(serde_json::Number::from(dt.with_timezone(&Utc).timestamp()));
            }
        }
        _ => {}
    }
}

pub fn transform_to_rebit(val: &mut Value, date_only_paths: &[String], current_path: String) {
    match val {
        Value::Object(map) => {
            map.remove("xfina");
            let keys: Vec<String> = map.keys().cloned().collect();
            for k in keys {
                if let Some(v) = map.get_mut(&k) {
                    let next_path = if current_path.is_empty() {
                        k.clone()
                    } else {
                        format!("{}.{}", current_path, k)
                    };
                    transform_to_rebit(v, date_only_paths, next_path);
                }
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                transform_to_rebit(v, date_only_paths, current_path.clone());
            }
        }
        Value::String(s) => {
            if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
                if date_only_paths.contains(&current_path) {
                    // In IST, the timezone the date was printed in. Formatting
                    // in UTC prints the day before for every timestamp earlier
                    // than 05:30 IST, which is every date-only field, since
                    // those are stamped at midnight.
                    *val = Value::String(dt.with_timezone(&ist()).format("%Y-%m-%d").to_string());
                }
            }
        }
        _ => {}
    }
}

/// Renders any account model into the requested [`Schema`].
///
/// This is the single place the two transforms above are applied; the
/// `to_xfina_json` / `to_rebit_json` methods on each account type and
/// [`crate::models::account::AccountModel`] all funnel through here.
pub fn render<T: serde::Serialize + ?Sized>(
    value: &T,
    schema: crate::models::schema::Schema,
    date_only_paths: &[String],
) -> Value {
    let mut val = serde_json::to_value(value).unwrap();
    match schema {
        crate::models::schema::Schema::Xfina => transform_to_xfina(&mut val),
        crate::models::schema::Schema::Rebit => {
            transform_to_rebit(&mut val, date_only_paths, String::new())
        }
    }
    val
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_date_is_the_epoch_of_midnight_ist() {
        let d = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
        assert_eq!(naive_date_to_epoch(d), 1772389800);
        // Midnight UTC on the same date is 5h30m later, and was what this
        // returned before: two conventions in one document.
        assert_eq!(naive_date_to_epoch(d) + 19800, 1772409600);
    }

    #[test]
    fn a_date_only_field_keeps_its_indian_day_in_rebit() {
        let paths = vec!["transactions.transaction.txnDate".to_string()];
        let mut val = json!({
            "transactions": { "transaction": [{
                // Midnight IST on 2 March, which is 1 March in UTC.
                "txnDate": "2026-03-01T18:30:00+00:00",
                "other": "2026-03-01T18:30:00+00:00"
            }]}
        });
        transform_to_rebit(&mut val, &paths, String::new());
        let txn = &val["transactions"]["transaction"][0];
        assert_eq!(txn["txnDate"], "2026-03-02");
        // A path that is not date-only keeps its full timestamp.
        assert_eq!(txn["other"], "2026-03-01T18:30:00+00:00");
    }
}
