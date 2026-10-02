#![cfg(all(
    feature = "md-amfi-nav",
    feature = "md-nse-security",
    feature = "md-nse-indices",
    feature = "md-mcx-spot",
    feature = "md-ishares",
    feature = "md-tiingo",
    feature = "md-wsj",
    feature = "md-msci",
    feature = "md-nasdaq",
    feature = "md-yahoo",
    feature = "md-spdr-gold",
))]

//! The market data parsers against the publishers' own downloads.
//!
//! The downloads are kept out of this repository -- each publisher's file
//! carries that publisher's terms -- in a local folder, one directory per
//! source. Set `XFINA_MARKET_DATA` to it; the default is `../xfina-labs-data`.
//! Skipped when it is not there, and in CI.
//!
//! Three things are checked:
//!
//! 1. **Detection, both ways.** Every download in a source's folder is
//!    recognised as that source from its content alone, and again with its
//!    name; the files in those folders that are not that source's download,
//!    and every file in the folders of sources not supported, are recognised
//!    as nothing.
//! 2. **Parsing.** Every download parses, and nothing fails validation.
//! 3. **Continuity.** Pieces of one dataset agree with each other and with
//!    the full histories they were cut from. That is a property of the data,
//!    checked here in test code; xfina itself never stitches files together.
//!
//! Failures name a folder and a count, never a file's values.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;

use xfina::models::series::{Field, PriceSeries};
use xfina::models::validation::ValidationStatus;
use xfina::models::ParseRequest;
use xfina::Format;

/// Source folder -> the format every download in it must be read as.
const OWNERS: &[(&str, Format)] = &[
    ("amfi", Format::MarketAmfiNav),
    ("nse", Format::MarketNseSecurity),
    ("nse-indices", Format::MarketNseIndices),
    ("mcx", Format::MarketMcxSpot),
    ("ishares", Format::MarketIshares),
    ("tiingo", Format::MarketTiingo),
    ("wsjm", Format::MarketWsj),
    ("msci", Format::MarketMsci),
    ("nasdaq", Format::MarketNasdaq),
    ("yahoo", Format::MarketYahoo),
    ("spdr", Format::MarketSpdrGold),
];

/// Files inside a source's folder that are not that source's download, by a
/// fragment of their name: nasdaq.com exports filed under iShares, full
/// histories assembled from NSE's API, a saved web page, and an error
/// response saved as a CSV.
const NOT_DOWNLOADS: &[&str] = &[
    "HistoricalData_",
    "nse-indices_",
    "download.html",
    "SGLD.L.csv",
];

/// Folders of publishers that are not sources. Nothing in them is anybody's.
const UNSUPPORTED: &[&str] = &["stooq", "investing.com", "six", "spdji", "ssga", "rbi"];

fn root() -> Option<PathBuf> {
    if std::env::var("GITHUB_ACTIONS").is_ok() {
        return None;
    }
    let root = std::env::var("XFINA_MARKET_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("../xfina-labs-data"));
    root.is_dir().then_some(root)
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            out.extend(files(&path));
        } else {
            out.push(path);
        }
    }
    out.sort();
    out
}

fn name_of(path: &Path) -> &str {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("")
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

fn parse(path: &Path) -> Option<PriceSeries> {
    let bytes = fs::read(path).ok()?;
    let statement =
        xfina::parse(ParseRequest::new(&bytes).with_filename(Some(name_of(path)))).ok()?;
    statement.data.series().cloned()
}

#[test]
fn every_download_is_recognised_and_parses() {
    let Some(root) = root() else {
        println!("market data samples not present; skipping");
        return;
    };

    let mut problems: Vec<String> = Vec::new();
    let mut warnings: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut read, mut refused) = (0usize, 0usize);

    for (folder, owner) in OWNERS {
        for path in files(&root.join(folder)) {
            let ext = ext_of(&path);
            let name = name_of(&path);
            let bytes = fs::read(&path).unwrap();
            let blind = xfina::detect(&ParseRequest::new(&bytes)).map(|d| d.format);
            let named = xfina::detect(&ParseRequest::new(&bytes).with_filename(Some(name)))
                .map(|d| d.format);

            if NOT_DOWNLOADS.iter().any(|n| name.contains(n)) {
                match blind {
                    Err(e) if e.kind() == "unrecognized_format" => refused += 1,
                    other => problems.push(format!(
                        "  {folder}/*.{ext}: a file that is not a download was claimed ({:?})",
                        other.map(|f| f.id())
                    )),
                }
                continue;
            }

            match (&blind, &named) {
                (Ok(b), Ok(n)) if b == owner && n == owner => {}
                _ => {
                    problems.push(format!(
                        "  {folder}/*.{ext}: detected {:?} without its name and {:?} with it, want {owner}",
                        blind.as_ref().map(|f| f.id()).map_err(|e| e.kind()),
                        named.as_ref().map(|f| f.id()).map_err(|e| e.kind()),
                    ));
                    continue;
                }
            }

            match xfina::parse(ParseRequest::new(&bytes).with_filename(Some(name))) {
                Ok(statement) => {
                    read += 1;
                    match statement.validation.overall {
                        ValidationStatus::Passed => {}
                        ValidationStatus::Warning => *warnings.entry(folder).or_default() += 1,
                        ValidationStatus::Failed => problems
                            .push(format!("  {folder}/*.{ext}: parsed but failed validation")),
                    }
                }
                Err(e) => {
                    problems.push(format!("  {folder}/*.{ext}: did not parse ({})", e.kind()))
                }
            }
        }
    }

    for folder in UNSUPPORTED {
        for path in files(&root.join(folder)) {
            let bytes = fs::read(&path).unwrap();
            match xfina::detect(&ParseRequest::new(&bytes).with_filename(Some(name_of(&path)))) {
                Err(e) if matches!(e.kind(), "unrecognized_format") => refused += 1,
                other => problems.push(format!(
                    "  {folder}/*.{}: an unsupported publisher's file was claimed ({:?})",
                    ext_of(&path),
                    other.map(|d| d.format.id())
                )),
            }
        }
    }
    // Loose files at the top of the folder are other publishers' too.
    for path in fs::read_dir(&root).unwrap().flatten().map(|e| e.path()) {
        if !path.is_file() || name_of(&path).starts_with('.') || name_of(&path).ends_with(".md") {
            continue;
        }
        let bytes = fs::read(&path).unwrap();
        match xfina::detect(&ParseRequest::new(&bytes)) {
            Err(e) if e.kind() == "unrecognized_format" => refused += 1,
            other => problems.push(format!(
                "  top-level *.{}: claimed ({:?})",
                ext_of(&path),
                other.map(|d| d.format.id())
            )),
        }
    }

    println!(
        "{read} downloads read, {refused} other files refused, warnings by folder: {warnings:?}"
    );
    assert!(read > 0, "no downloads found under the samples folder");
    assert!(
        problems.is_empty(),
        "market data corpus:\n{}",
        problems.join("\n")
    );
}

/// Values of `field` by date, for a series with one value a day.
fn by_date(series: &PriceSeries, field: Field) -> BTreeMap<NaiveDate, Decimal> {
    series
        .rows
        .iter()
        .filter_map(|r| r.get(field).map(|v| (r.date, v)))
        .collect()
}

/// Folds pieces into one map, counting dates two pieces give different
/// values for.
fn union(pieces: &[PriceSeries], field: Field) -> (BTreeMap<NaiveDate, Decimal>, usize) {
    let mut all = BTreeMap::new();
    let mut disagreements = 0;
    for piece in pieces {
        for (date, value) in by_date(piece, field) {
            if let Some(existing) = all.insert(date, value) {
                if existing != value {
                    disagreements += 1;
                }
            }
        }
    }
    (all, disagreements)
}

/// The full-history pulls beside the NSE yearly pieces:
/// `Index Name,Date,TotalReturnsIndex,NTR_Value`, dates as "25 Sep 2026".
fn nse_full_history(path: &Path) -> (BTreeMap<NaiveDate, Decimal>, BTreeMap<NaiveDate, Decimal>) {
    let text = fs::read_to_string(path).unwrap();
    let (mut tri, mut ntr) = (BTreeMap::new(), BTreeMap::new());
    for line in text.lines().skip(1) {
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        let Some(date) = cells
            .get(1)
            .and_then(|d| NaiveDate::parse_from_str(d, "%d %b %Y").ok())
        else {
            continue;
        };
        if let Some(v) = cells.get(2).and_then(|v| Decimal::from_str(v).ok()) {
            tri.insert(date, v);
        }
        if let Some(v) = cells.get(3).and_then(|v| Decimal::from_str(v).ok()) {
            ntr.insert(date, v);
        }
    }
    (tri, ntr)
}

#[test]
fn nse_index_pieces_rebuild_the_full_history_they_were_cut_from() {
    let Some(root) = root() else {
        println!("market data samples not present; skipping");
        return;
    };
    let mut problems = Vec::new();
    let mut compared = 0usize;

    let Ok(dirs) = fs::read_dir(root.join("nse-indices")) else {
        return;
    };
    for dir in dirs.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let label = name_of(&dir).to_string();
        let all = files(&dir);
        let Some(pull) = all
            .iter()
            .find(|p| name_of(p).starts_with("nse-indices_") && ext_of(p) == "csv")
        else {
            continue;
        };
        let pieces: Vec<PriceSeries> = all
            .iter()
            .filter(|p| name_of(p).contains("_Historical_TR_"))
            .filter_map(|p| parse(p))
            .collect();
        let (full_tri, full_ntr) = nse_full_history(pull);

        for (field, full) in [
            (Field::TotalReturn, &full_tri),
            (Field::NetTotalReturn, &full_ntr),
        ] {
            let (_, overlaps) = union(&pieces, field);
            if overlaps > 0 {
                problems.push(format!(
                    "  {label}: {overlaps} dates where two pieces disagree ({})",
                    field.as_str()
                ));
            }
            // Each piece is a cut of the full history: inside its own window
            // it must hold exactly the full history's dates and values. The
            // pull ends a few days before the newest piece, so a window is
            // clipped to the dates the pull covers.
            let Some(pull_end) = full.keys().next_back().copied() else {
                continue;
            };
            let (mut missing, mut extra, mut different) = (0usize, 0usize, 0usize);
            for piece in &pieces {
                let mine = by_date(piece, field);
                let (Some(first), Some(last)) = (
                    mine.keys().next().copied(),
                    mine.keys().next_back().copied(),
                ) else {
                    continue;
                };
                let last = last.min(pull_end);
                if first > last {
                    continue;
                }
                missing += full
                    .range(first..=last)
                    .filter(|(d, _)| !mine.contains_key(d))
                    .count();
                extra += mine
                    .range(first..=last)
                    .filter(|(d, _)| !full.contains_key(d))
                    .count();
                different += mine
                    .range(first..=last)
                    .filter(|(d, v)| full.get(d).is_some_and(|f| f != *v))
                    .count();
                compared += mine.range(first..=last).count();
            }
            if missing + extra + different > 0 {
                problems.push(format!(
                    "  {label} {}: {missing} dates missing from a piece, {extra} not in the full history, {different} values differ",
                    field.as_str()
                ));
            }
        }

        // Years no piece covers are a hole in the samples, not a defect in
        // the reading: reported, not failed.
        let (stitched, _) = union(&pieces, Field::TotalReturn);
        let dates: Vec<&NaiveDate> = stitched.keys().collect();
        let holes = dates
            .windows(2)
            .filter(|w| (*w[1] - *w[0]).num_days() > 7)
            .count();
        if holes > 0 {
            println!("{label}: {holes} stretches no piece covers");
        }
    }
    println!("{compared} index dates compared against the full histories");
    assert!(
        problems.is_empty(),
        "NSE index continuity:\n{}",
        problems.join("\n")
    );
}

#[test]
fn wsj_and_tiingo_agree_on_the_same_closes() {
    let Some(root) = root() else {
        println!("market data samples not present; skipping");
        return;
    };
    let pick = |folder: &str, suffix: &str| {
        files(&root.join(folder))
            .into_iter()
            .find(|p| name_of(p).starts_with("PHYS_") && name_of(p).ends_with(suffix))
    };
    let (Some(wsj), Some(tiingo)) = (pick("wsjm", ".wsj.csv"), pick("tiingo", ".tiingo.csv"))
    else {
        println!("no PHYS pair to compare; skipping");
        return;
    };
    let wsj = by_date(&parse(&wsj).unwrap(), Field::Close);
    let tiingo = by_date(&parse(&tiingo).unwrap(), Field::Close);

    // WSJ prints two decimals where Tiingo prints up to four, so the two
    // agree to within half a cent. WSJ also lists exchange holidays at the
    // previous close, which Tiingo leaves out; those dates are not compared.
    // The two publishers do disagree on the odd day; a column read wrongly
    // would disagree on nearly all of them, so a handful is allowed.
    let half_cent = Decimal::from_str("0.005").unwrap();
    let shared: Vec<&NaiveDate> = tiingo.keys().filter(|d| wsj.contains_key(*d)).collect();
    let apart = shared
        .iter()
        .filter(|d| (wsj[**d] - tiingo[**d]).abs() > half_cent)
        .count();
    let only_tiingo = tiingo.keys().filter(|d| !wsj.contains_key(*d)).count();
    println!("{} shared dates compared", shared.len());
    assert!(shared.len() > 1000, "too few shared dates to mean anything");
    assert_eq!(only_tiingo, 0, "Tiingo has trading days WSJ does not");
    println!("{apart} shared dates apart by more than rounding");
    assert!(
        apart * 1000 <= shared.len(),
        "WSJ and Tiingo closes disagree beyond rounding on {apart} of {} dates",
        shared.len()
    );
}

#[test]
fn overlapping_pieces_of_one_dataset_agree() {
    let Some(root) = root() else {
        println!("market data samples not present; skipping");
        return;
    };
    let mut problems = Vec::new();
    for (folder, field) in [
        ("amfi", Field::Nav),
        ("nse", Field::Close),
        ("msci", Field::NetTotalReturn),
    ] {
        let mut by_code: BTreeMap<String, Vec<PriceSeries>> = BTreeMap::new();
        for path in files(&root.join(folder)) {
            if let Some(series) = parse(&path) {
                by_code
                    .entry(series.code.clone().unwrap_or_default())
                    .or_default()
                    .push(series);
            }
        }
        let mut overlapping = 0usize;
        for pieces in by_code.values() {
            let (_, disagreements) = union(pieces, field);
            overlapping += pieces.len().saturating_sub(1);
            if disagreements > 0 {
                problems.push(format!(
                    "  {folder}: {disagreements} dates where two pieces disagree"
                ));
            }
        }
        println!(
            "{folder}: {} datasets, {overlapping} extra pieces checked",
            by_code.len()
        );
    }
    assert!(
        problems.is_empty(),
        "pieces of one dataset disagree:\n{}",
        problems.join("\n")
    );
}
