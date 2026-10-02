#![cfg(feature = "rt-sbi-forex-card")]

//! Every rate on every sheet, against an independently written reader.
//!
//! The series in `tests/reference_rates_sbi_parity.rs` covers the dollar's two
//! TT rates and nothing else -- about one part in eighty of what the parser
//! produces. This covers the rest, by diffing against the per-currency CSVs
//! published by `sahilgupta/sbi-fx-ratekeeper`, which reads the same PDFs with
//! an unrelated implementation.
//!
//! Skipped unless that repository is checked out beside this one; point
//! `XFINA_SBI_FX_UPSTREAM` somewhere else to override.
//!
//! ```text
//! git clone https://github.com/sahilgupta/sbi-fx-ratekeeper ../sbi-fx-ratekeeper
//! ```
//!
//! Three kinds of difference are expected rather than tolerated, and all are
//! counted so a change in any is visible:
//!
//! - a zero upstream published, which is how the sheet writes "not quoted".
//!   Those are absent here, never carried through as a price.
//! - rows upstream read with two adjacent figures run together, so every
//!   column after the join shifts by one. The figures are still all ours.
//! - rows upstream read as a single number: `22.1823` followed by seven empty
//!   fields, on the sheets whose font widths cannot be measured. That number
//!   is the row's first figure with the start of the next one glued on.
//!
//! SBI republishes the card some days, and upstream keeps a row per edition
//! where this archive keeps one file per day. A rate is therefore checked
//! against every edition published that day: the archive does not say which of
//! them its file is, and the printed times are not reliable enough on the older
//! rows to tell.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use xfina::models::request::ParseRequest;
use xfina::reference_rates::sbi_forex_card::parse_sbi_forex_card_rates;

fn archive() -> Option<PathBuf> {
    let root = std::env::var("XFINA_SBI_FX_ARCHIVE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("../financial-data/src/sbi-fx-card-rates"));
    root.is_dir().then_some(root)
}

fn upstream() -> Option<PathBuf> {
    let root = std::env::var("XFINA_SBI_FX_UPSTREAM")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("../sbi-fx-ratekeeper/csv_files"));
    root.is_dir().then_some(root)
}

/// Upstream's column headings, against the keys this parser takes from the
/// sheet. It folds the travel-card column's four printed names into one; this
/// keeps them apart, so the mapping is one-to-many in that direction.
const COLUMNS: &[(&str, &[&str])] = &[
    ("TT BUY", &["tt_buy"]),
    ("TT SELL", &["tt_sell"]),
    ("BILL BUY", &["bill_buy"]),
    ("BILL SELL", &["bill_sell"]),
    (
        "FOREX TRAVEL CARD BUY",
        &[
            "forex_travel_card_buy",
            "foreign_travel_card_buy",
            "ftc_buy",
            "tc_buy",
        ],
    ),
    (
        "FOREX TRAVEL CARD SELL",
        &[
            "forex_travel_card_sell",
            "foreign_travel_card_sell",
            "ftc_sell",
            "tc_sell",
        ],
    ),
    ("CN BUY", &["cn_buy"]),
    ("CN SELL", &["cn_sell"]),
];

/// Everything this parser reads out of the archive, by date, currency and key.
fn parsed(root: &Path) -> BTreeMap<String, BTreeMap<(String, String), f64>> {
    use rust_decimal::prelude::ToPrimitive;

    let mut out: BTreeMap<String, BTreeMap<(String, String), f64>> = BTreeMap::new();
    for year in fs::read_dir(root).into_iter().flatten().flatten() {
        for sheet in fs::read_dir(year.path()).into_iter().flatten().flatten() {
            let path = sheet.path();
            if path.extension().and_then(|e| e.to_str()) != Some("pdf") {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else { continue };
            let Ok(result) = parse_sbi_forex_card_rates(ParseRequest::new(&bytes)) else {
                continue;
            };
            let sheet = result.data;
            let row = out.entry(sheet.date.to_string()).or_default();
            for currency in &sheet.currencies {
                for (key, value) in &currency.rates {
                    if let Some(v) = value.to_f64() {
                        row.insert((currency.currency.clone(), key.clone()), v);
                    }
                }
            }
        }
    }
    out
}

/// Upstream's figures for one day, by currency, then by the edition they were
/// published in, then by heading. Kept apart by edition because SBI
/// republishes the card some days and the two differ, while this archive holds
/// only one file per day.
type Upstream = BTreeMap<String, BTreeMap<String, BTreeMap<String, BTreeMap<&'static str, f64>>>>;

fn read_upstream(dir: &Path, key_for: &HashMap<&'static str, &'static [&'static str]>) -> Upstream {
    let mut out: Upstream = BTreeMap::new();
    for file in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = file.path();
        let Some(currency) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|n| n.strip_prefix("SBI_REFERENCE_RATES_"))
        else {
            continue;
        };
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let mut lines = text.lines();
        let headings: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
        for line in lines {
            let fields: Vec<&str> = line.split(',').collect();
            let Some(stamp) = fields.first() else {
                continue;
            };
            let Some(date) = stamp.split(' ').next() else {
                continue;
            };
            let row = out
                .entry(date.to_string())
                .or_default()
                .entry(currency.to_string())
                .or_default()
                .entry(stamp.to_string())
                .or_default();
            for (heading, field) in headings.iter().zip(&fields) {
                let Some(heading) = key_for.get_key_value(*heading).map(|(k, _)| *k) else {
                    continue;
                };
                if let Ok(value) = field.trim().parse::<f64>() {
                    row.insert(heading, value);
                }
            }
        }
    }
    out
}

#[test]
fn every_rate_agrees_with_an_independent_reader() {
    let (Some(archive), Some(upstream)) = (archive(), upstream()) else {
        println!("SBI rate archive or ../sbi-fx-ratekeeper not present; skipping");
        return;
    };
    let ours = parsed(&archive);
    assert!(!ours.is_empty(), "archive present but nothing parsed");

    let key_for: HashMap<&'static str, &'static [&'static str]> = COLUMNS.iter().copied().collect();
    let theirs = read_upstream(&upstream, &key_for);

    let (mut agreed, mut zero_absent, mut not_read) = (0usize, 0usize, 0usize);
    let (mut reattributed, mut fused) = (0usize, 0usize);
    let mut wrong: Vec<String> = Vec::new();

    for (date, currencies) in &theirs {
        // A sheet we refused, or a day this archive does not hold.
        let Some(mine) = ours.get(date) else {
            not_read += currencies.len();
            continue;
        };

        for (currency, editions) in currencies {
            let read = |heading: &str| -> Option<f64> {
                key_for[heading]
                    .iter()
                    .find_map(|k| mine.get(&(currency.clone(), k.to_string())).copied())
            };

            let mut disputed: Vec<String> = Vec::new();
            for (heading, _) in COLUMNS {
                let published: Vec<f64> = editions
                    .values()
                    .filter_map(|row| row.get(heading).copied())
                    .collect();
                if published.is_empty() {
                    continue;
                }
                match read(heading) {
                    Some(g) if published.iter().any(|t| (t - g).abs() < 0.0005) => agreed += 1,
                    // A zero is how the sheet writes "not quoted". It must not
                    // come back as a price.
                    None if published.contains(&0.0) => zero_absent += 1,
                    Some(g) => disputed.push(format!(
                        "  {date} {currency} {heading}: read {g}, upstream {published:?}"
                    )),
                    None => disputed.push(format!(
                        "  {date} {currency} {heading}: dropped, upstream {published:?}"
                    )),
                }
            }
            if disputed.is_empty() {
                continue;
            }

            // Upstream's text extraction runs adjacent figures together where
            // the sheet prints two zeros side by side -- its own data holds
            // cells like "0213.9" -- and every column after the join shifts by
            // one, the last falling off the end. The figures it read are still
            // ours; only the headings it filed them under moved.
            //
            // So a disputed row passes when some edition upstream published is
            // accounted for entirely: every rate in it is one this parser also
            // read for that currency, somewhere in the row. Checked an edition
            // at a time because the archive holds one file per day and cannot
            // say which edition it is -- rates from an edition we do not hold
            // are not ours to account for. A rate that appears nowhere in our
            // row is a real disagreement, and fails.
            let ours_here: Vec<f64> = mine
                .iter()
                .filter(|((c, _), _)| c == currency)
                .map(|(_, v)| *v)
                .collect();
            let accounted = editions.values().any(|row| {
                row.values()
                    .all(|t| *t == 0.0 || ours_here.iter().any(|v| (v - t).abs() < 0.0005))
            });

            // Upstream also reads some rows as one long number with every
            // other column empty: the row's figures run together, the first
            // whole and the next one's digits stuck on behind it. That passes
            // only when the number begins with the figure this parser read
            // under the same heading -- the part upstream did read agrees.
            let run_together = editions.values().any(|row| {
                row.len() == 1
                    && row.iter().all(|(heading, t)| {
                        read(heading).is_some_and(|g| {
                            let (g, t) = (format!("{g:.2}"), t.to_string());
                            t.len() > g.len() && t.starts_with(&g)
                        })
                    })
            });
            if accounted {
                reattributed += 1;
            } else if run_together {
                fused += 1;
            } else {
                wrong.extend(disputed);
            }
        }
    }

    println!(
        "{agreed} rates agree, {zero_absent} upstream zeros correctly absent, \
         {reattributed} rows upstream shifted, {fused} rows upstream read as one \
         number, {not_read} rows skipped"
    );
    assert!(
        wrong.is_empty(),
        "{} rates disagree with the independent reader:\n{}",
        wrong.len(),
        wrong
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        agreed > 200_000,
        "only {agreed} rates were compared; the corpus looks incomplete"
    );
}
