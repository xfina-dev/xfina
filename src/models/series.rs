//! A published price history: what a market data file says, one row per day.
//!
//! Eleven publishers print prices, NAVs and index levels eleven different
//! ways. Rather than a model per publisher, or a series per column, there is
//! one superset row: every value any of them publishes has a named field, and
//! a file fills the ones it prints. NSE's total return files fill
//! `totalReturn` and `netTotalReturn` on the same row; SPDR's archive fills
//! `close` and `nav`; a Tiingo file fills the price, its adjusted twin and the
//! day's dividend. Whatever else a publisher prints goes into `extra` under
//! its own heading, so nothing in the file is lost.
//!
//! Like a rate sheet this is a public document, not an account, so ReBIT has
//! nothing to say about it and refuses to render one.
//!
//! A series is exactly one file. Stitching the pieces of a dataset together --
//! the yearly NSE downloads, AMFI's five-year windows, an update fetched after
//! the first pull -- is not reading a file, and belongs in a computation layer
//! that takes parsed series as its input.

use std::collections::BTreeMap;

use chrono::{NaiveDate, NaiveTime};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::detect::Format;
use crate::error::XfinaError;
use crate::models::schema::Schema;

/// A value a row can carry, named as it appears on the wire.
///
/// The declaration order is the order [`PriceSeries::fields`] lists them in
/// and the order CSV columns are written in, so it is part of the format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Field {
    Close,
    High,
    Low,
    Open,
    Volume,
    AdjClose,
    AdjHigh,
    AdjLow,
    AdjOpen,
    AdjVolume,
    Dividend,
    SplitFactor,
    Nav,
    AdjNav,
    TotalReturn,
    NetTotalReturn,
}

impl Field {
    /// Every field, in wire order.
    pub const ALL: [Field; 16] = [
        Field::Close,
        Field::High,
        Field::Low,
        Field::Open,
        Field::Volume,
        Field::AdjClose,
        Field::AdjHigh,
        Field::AdjLow,
        Field::AdjOpen,
        Field::AdjVolume,
        Field::Dividend,
        Field::SplitFactor,
        Field::Nav,
        Field::AdjNav,
        Field::TotalReturn,
        Field::NetTotalReturn,
    ];

    /// The field's name in JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            Field::Close => "close",
            Field::High => "high",
            Field::Low => "low",
            Field::Open => "open",
            Field::Volume => "volume",
            Field::AdjClose => "adjClose",
            Field::AdjHigh => "adjHigh",
            Field::AdjLow => "adjLow",
            Field::AdjOpen => "adjOpen",
            Field::AdjVolume => "adjVolume",
            Field::Dividend => "dividend",
            Field::SplitFactor => "splitFactor",
            Field::Nav => "nav",
            Field::AdjNav => "adjNav",
            Field::TotalReturn => "totalReturn",
            Field::NetTotalReturn => "netTotalReturn",
        }
    }

    /// The field's CSV heading.
    ///
    /// The JSON name, except that the dividend keeps Tiingo's `divCash`: the
    /// CSV is laid out as Tiingo lays its own out, which is also the "own
    /// data" layout Xfina Labs documents, so a column means the same thing
    /// under the same heading in either.
    pub const fn csv_header(self) -> &'static str {
        match self {
            Field::Dividend => "divCash",
            other => other.as_str(),
        }
    }

    /// Whether this is a corporate action rather than a price.
    ///
    /// A row carrying only an action is a day the publisher recorded an event
    /// on without printing a price, and is not counted as coverage.
    pub const fn is_event(self) -> bool {
        matches!(self, Field::Dividend | Field::SplitFactor)
    }
}

/// What kind of instrument a series prices, when the file says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentKind {
    Index,
    Etf,
    MutualFund,
    /// A security listed on an exchange whose file does not say whether it is
    /// a fund or a company.
    ListedSecurity,
    CommoditySpot,
}

/// How often the series has a value, read off the rows themselves.
///
/// Inferred rather than taken from the file: Tiingo's oldest mutual fund rows
/// are monthly before turning daily, and MCX polls several times a day, so
/// the honest answer comes from the dates actually present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frequency {
    /// More than one reading on the same day.
    Intraday,
    Daily,
    Monthly,
    /// Too few rows to tell, or gaps that fit neither daily nor monthly.
    Irregular,
}

/// Where a series' declared range was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeSource {
    /// Printed inside the file, as AMFI's "Historical NAV Data for From … to …".
    Content,
    /// Read from the name the publisher or a bookmarklet gave the file.
    Filename,
}

/// The period a file says it covers, which is not always the period its rows
/// cover: a range that runs into a weekend has no row on its last day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateRange {
    #[serde(deserialize_with = "date_or_epoch::deserialize")]
    pub from: NaiveDate,
    #[serde(deserialize_with = "date_or_epoch::deserialize")]
    pub to: NaiveDate,
    pub source: RangeSource,
}

impl DateRange {
    pub fn contains(&self, date: NaiveDate) -> bool {
        self.from <= date && date <= self.to
    }
}

/// One day of a series -- or one reading, for a publisher that polls.
///
/// Every value is optional, and absent rather than zero when not published:
/// a zero is not a price, and passing one on is how a "0.0" ends up in
/// somebody's return calculation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricePoint {
    #[serde(deserialize_with = "date_or_epoch::deserialize")]
    pub date: NaiveDate,
    /// The reading's time, as printed, for a publisher that polls (MCX). The
    /// clock is the publisher's own, IST for an Indian exchange.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<NaiveTime>,

    /// Traded price, a price index's level, or a spot price.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub open: Option<Decimal>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub high: Option<Decimal>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub low: Option<Decimal>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub close: Option<Decimal>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub volume: Option<Decimal>,

    /// The publisher's split- and dividend-adjusted twin of each of the above,
    /// as printed. Never computed here.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub adj_open: Option<Decimal>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub adj_high: Option<Decimal>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub adj_low: Option<Decimal>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub adj_close: Option<Decimal>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub adj_volume: Option<Decimal>,

    /// Net asset value per unit or share.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub nav: Option<Decimal>,
    /// The publisher's distribution-adjusted NAV, as printed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub adj_nav: Option<Decimal>,

    /// A total return index level: dividends reinvested, gross of tax.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub total_return: Option<Decimal>,
    /// A total return index level net of withholding tax on dividends.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub net_total_return: Option<Decimal>,

    /// Total cash distributed per unit, on its ex-date -- Tiingo's `divCash`.
    ///
    /// One figure, as Tiingo carries it: no breakdown by character and no
    /// record or payable date. Those do not change a total return; the
    /// amount on the ex-date is all that rebuilding one needs.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub dividend: Option<Decimal>,
    /// A split taking effect on this day: 2 for 2-for-1, 0.5 for 1-for-2.
    /// Absent, not 1, on a day without one.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rust_decimal::serde::float_option"
    )]
    pub split_factor: Option<Decimal>,

    /// Every other figure the publisher printed for the day, under its own
    /// heading: VWAP and deliverable quantity on NSE, ounces of gold per share
    /// on SPDR, shares outstanding on iShares.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        with = "crate::models::decimal_map"
    )]
    pub extra: BTreeMap<String, Decimal>,
}

impl PricePoint {
    pub fn new(date: NaiveDate) -> Self {
        PricePoint {
            date,
            ..Default::default()
        }
    }

    pub fn get(&self, field: Field) -> Option<Decimal> {
        *self.slot(field)
    }

    pub fn set(&mut self, field: Field, value: Option<Decimal>) {
        *self.slot_mut(field) = value;
    }

    /// Whether the row prints any price at all, as opposed to only an event.
    pub fn has_price(&self) -> bool {
        Field::ALL
            .iter()
            .any(|f| !f.is_event() && self.get(*f).is_some())
    }

    fn slot(&self, field: Field) -> &Option<Decimal> {
        match field {
            Field::Close => &self.close,
            Field::High => &self.high,
            Field::Low => &self.low,
            Field::Open => &self.open,
            Field::Volume => &self.volume,
            Field::AdjClose => &self.adj_close,
            Field::AdjHigh => &self.adj_high,
            Field::AdjLow => &self.adj_low,
            Field::AdjOpen => &self.adj_open,
            Field::AdjVolume => &self.adj_volume,
            Field::Dividend => &self.dividend,
            Field::SplitFactor => &self.split_factor,
            Field::Nav => &self.nav,
            Field::AdjNav => &self.adj_nav,
            Field::TotalReturn => &self.total_return,
            Field::NetTotalReturn => &self.net_total_return,
        }
    }

    fn slot_mut(&mut self, field: Field) -> &mut Option<Decimal> {
        match field {
            Field::Close => &mut self.close,
            Field::High => &mut self.high,
            Field::Low => &mut self.low,
            Field::Open => &mut self.open,
            Field::Volume => &mut self.volume,
            Field::AdjClose => &mut self.adj_close,
            Field::AdjHigh => &mut self.adj_high,
            Field::AdjLow => &mut self.adj_low,
            Field::AdjOpen => &mut self.adj_open,
            Field::AdjVolume => &mut self.adj_volume,
            Field::Dividend => &mut self.dividend,
            Field::SplitFactor => &mut self.split_factor,
            Field::Nav => &mut self.nav,
            Field::AdjNav => &mut self.adj_nav,
            Field::TotalReturn => &mut self.total_return,
            Field::NetTotalReturn => &mut self.net_total_return,
        }
    }
}

/// Everything one market data file publishes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceSeries {
    /// The publisher's format, which is what a series is named by first.
    pub source: Format,
    /// What the publisher calls the instrument: an exchange symbol, a ticker,
    /// an index name, a fund name or an index code. Read from the content
    /// where the file prints one and from the filename otherwise -- several
    /// publishers put nothing identifying inside the file at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// The instrument's name as printed, where it differs from the code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<InstrumentKind>,
    /// ISO 4217, where the file prints it or the publisher only quotes in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// The quantity a price is for, where it is not one unit ("10 GRMS").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub frequency: Frequency,
    /// The fields any row of this file fills, in wire order.
    pub fields: Vec<Field>,
    /// The `extra` headings any row of this file fills, in the order the
    /// publisher prints them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_fields: Vec<String>,
    /// Whether the prices already reflect splits that came after them.
    ///
    /// Sources differ, and a consumer rebuilding a return has to know:
    /// Tiingo prints prices as traded, so a 2-for-1 split halves the close
    /// and `splitFactor` must be compounded to undo it; Yahoo prints closes
    /// already divided through by every later split, so its split rows are
    /// a record of what happened and must not be applied again. Absent for a
    /// source whose file says nothing about splits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_adjusted: Option<bool>,
    /// The field that is this series' value: `nav` for a fund's NAV history,
    /// `totalReturn` for a total return index, `close` for a traded price.
    pub headline: Field,
    /// Ascending by date, then time. Prices and corporate actions share rows.
    pub rows: Vec<PricePoint>,
    /// The period the file says it covers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_range: Option<DateRange>,
}

/// A stretch with no value, longer than the series' frequency explains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gap {
    /// The last date with a value before the gap.
    pub after: NaiveDate,
    /// The first date with a value after it.
    pub before: NaiveDate,
    /// Calendar days between the two.
    pub days: i64,
}

/// What one file covers, described from its own rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first: Option<NaiveDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<NaiveDate>,
    /// Rows carrying the headline value.
    pub rows: usize,
    pub gaps: Vec<Gap>,
}

impl PriceSeries {
    /// Reads a series back from JSON in either schema form: dates as
    /// `YYYY-MM-DD` or, as the `xfina` schema renders them, as the epoch of
    /// midnight IST. This is what lets a binding take back the `data` a parse
    /// returned -- to write it as CSV, say -- without parsing the file again.
    pub fn from_json(value: serde_json::Value) -> Result<Self, XfinaError> {
        serde_json::from_value(value)
            .map_err(|e| XfinaError::InvalidFormat(format!("Not a price series: {}", e)))
    }

    /// Rows that carry this series' headline value.
    pub fn valued_rows(&self) -> impl Iterator<Item = &PricePoint> {
        let headline = self.headline;
        self.rows.iter().filter(move |r| r.get(headline).is_some())
    }

    /// First and last date, row count and in-file gaps.
    ///
    /// A gap is a stretch between two values longer than the frequency
    /// accounts for: more than a week for a daily series, which no run of
    /// weekends and holidays reaches, and more than 45 days for a monthly one.
    pub fn coverage(&self) -> Coverage {
        let dates: Vec<NaiveDate> = {
            let mut d: Vec<NaiveDate> = self.valued_rows().map(|r| r.date).collect();
            d.dedup();
            d
        };
        let threshold = match self.frequency {
            Frequency::Intraday | Frequency::Daily => Some(7),
            Frequency::Monthly => Some(45),
            Frequency::Irregular => None,
        };
        let gaps = match threshold {
            Some(limit) => dates
                .windows(2)
                .filter_map(|w| {
                    let days = (w[1] - w[0]).num_days();
                    (days > limit).then_some(Gap {
                        after: w[0],
                        before: w[1],
                        days,
                    })
                })
                .collect(),
            None => Vec::new(),
        };
        Coverage {
            first: dates.first().copied(),
            last: dates.last().copied(),
            rows: self.valued_rows().count(),
            gaps,
        }
    }

    /// Renders into the requested [`Schema`].
    ///
    /// # Errors
    ///
    /// [`XfinaError::SchemaUnsupported`] for ReBIT, which describes accounts a
    /// person holds and has no term for a published price.
    pub fn to_json(&self, schema: Schema) -> Result<serde_json::Value, XfinaError> {
        match schema {
            Schema::Xfina => Ok(crate::models::serializer::render(self, schema, &[])),
            Schema::Rebit => Err(XfinaError::SchemaUnsupported {
                schema: schema.as_str(),
                document: "price series",
            }),
        }
    }

    /// The series as CSV, in Tiingo's column layout.
    ///
    /// `date`, then `time` for a polled series, then each filled field under
    /// its [`Field::csv_header`] in wire order -- for a Tiingo file, exactly
    /// Tiingo's own heading row. Dates are ISO 8601. A value the row does not
    /// carry is an empty cell, never a zero. `extra` figures are left out:
    /// they are each publisher's own, and the JSON keeps them.
    pub fn to_csv(&self) -> String {
        let timed = self.rows.iter().any(|r| r.time.is_some());
        let mut out = String::from("date");
        if timed {
            out.push_str(",time");
        }
        for field in &self.fields {
            out.push(',');
            out.push_str(field.csv_header());
        }
        out.push('\n');
        for row in &self.rows {
            out.push_str(&row.date.format("%Y-%m-%d").to_string());
            if timed {
                out.push(',');
                if let Some(t) = row.time {
                    out.push_str(&t.format("%H:%M").to_string());
                }
            }
            for field in &self.fields {
                out.push(',');
                if let Some(v) = row.get(*field) {
                    out.push_str(&v.normalize().to_string());
                }
            }
            out.push('\n');
        }
        out
    }
}

/// A calendar date written either as `YYYY-MM-DD` or as the epoch the
/// `xfina` schema turns one into: midnight IST, read back in IST.
mod date_or_epoch {
    use chrono::{DateTime, NaiveDate};
    use serde::de::{self, Deserializer, Visitor};

    use crate::models::date_utils::ist_date;

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveDate, D::Error> {
        struct DateVisitor;

        impl Visitor<'_> for DateVisitor {
            type Value = NaiveDate;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a YYYY-MM-DD date or a midnight-IST epoch")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<NaiveDate, E> {
                NaiveDate::parse_from_str(v, "%Y-%m-%d").map_err(E::custom)
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<NaiveDate, E> {
                DateTime::from_timestamp(v, 0)
                    .map(ist_date)
                    .ok_or_else(|| E::custom("epoch out of range"))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<NaiveDate, E> {
                self.visit_i64(i64::try_from(v).map_err(E::custom)?)
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<NaiveDate, E> {
                self.visit_i64(v as i64)
            }
        }

        d.deserialize_any(DateVisitor)
    }
}

/// Reads how often a series has a value from the dates it has values on.
///
/// More than one valued row on a date is a polled series. Otherwise the
/// median distance between consecutive dates decides: up to five days is
/// daily (a weekend plus a holiday is four), 25 to 35 is monthly.
pub(crate) fn infer_frequency(rows: &[PricePoint], headline: Field) -> Frequency {
    let mut dates: Vec<NaiveDate> = rows
        .iter()
        .filter(|r| r.get(headline).is_some())
        .map(|r| r.date)
        .collect();
    let valued = dates.len();
    dates.dedup();
    if dates.len() < valued {
        return Frequency::Intraday;
    }
    if dates.len() < 3 {
        // Two dates a day apart could be daily or a two-day window of
        // anything; not enough to say.
        return if dates.len() == 2 && (dates[1] - dates[0]).num_days() <= 5 {
            Frequency::Daily
        } else {
            Frequency::Irregular
        };
    }
    let mut steps: Vec<i64> = dates.windows(2).map(|w| (w[1] - w[0]).num_days()).collect();
    steps.sort_unstable();
    match steps[steps.len() / 2] {
        1..=5 => Frequency::Daily,
        25..=35 => Frequency::Monthly,
        _ => Frequency::Irregular,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn priced(date: &str, close: &str) -> PricePoint {
        let mut p = PricePoint::new(d(date));
        p.close = Some(Decimal::from_str(close).unwrap());
        p
    }

    fn series(rows: Vec<PricePoint>) -> PriceSeries {
        let frequency = infer_frequency(&rows, Field::Close);
        PriceSeries {
            source: Format::MarketTiingo,
            code: Some("TEST".into()),
            name: None,
            kind: None,
            currency: None,
            unit: None,
            frequency,
            fields: vec![Field::Close, Field::Dividend],
            extra_fields: vec![],
            headline: Field::Close,
            split_adjusted: None,
            rows,
            declared_range: None,
        }
    }

    #[test]
    fn frequency_comes_from_the_dates_present() {
        let daily = vec![
            priced("2026-01-01", "1"),
            priced("2026-01-02", "1"),
            priced("2026-01-05", "1"),
            priced("2026-01-06", "1"),
        ];
        assert_eq!(infer_frequency(&daily, Field::Close), Frequency::Daily);

        let monthly = vec![
            priced("2026-01-30", "1"),
            priced("2026-02-27", "1"),
            priced("2026-03-31", "1"),
        ];
        assert_eq!(infer_frequency(&monthly, Field::Close), Frequency::Monthly);

        let mut polled = vec![priced("2026-01-01", "1"), priced("2026-01-01", "2")];
        polled[1].time = NaiveTime::from_hms_opt(17, 0, 0);
        assert_eq!(infer_frequency(&polled, Field::Close), Frequency::Intraday);
    }

    #[test]
    fn an_event_only_row_is_not_coverage() {
        let mut event = PricePoint::new(d("2026-01-03"));
        event.dividend = Some(Decimal::from_str("0.5").unwrap());
        let s = series(vec![
            priced("2026-01-01", "10"),
            event,
            priced("2026-01-02", "11"),
        ]);
        let c = s.coverage();
        assert_eq!(c.rows, 2);
        assert_eq!(c.first, Some(d("2026-01-01")));
        assert_eq!(c.last, Some(d("2026-01-02")));
    }

    #[test]
    fn a_gap_is_longer_than_the_frequency_explains() {
        let s = series(vec![
            priced("2026-01-01", "1"),
            priced("2026-01-02", "1"),
            priced("2026-01-05", "1"),
            priced("2026-01-20", "1"),
            priced("2026-01-21", "1"),
        ]);
        assert_eq!(
            s.coverage().gaps,
            vec![Gap {
                after: d("2026-01-05"),
                before: d("2026-01-20"),
                days: 15
            }]
        );
    }

    #[test]
    fn csv_uses_tiingo_headings_and_leaves_absent_values_empty() {
        let mut event = PricePoint::new(d("2026-01-03"));
        event.dividend = Some(Decimal::from_str("0.50").unwrap());
        let s = series(vec![priced("2026-01-01", "10.10"), event]);
        assert_eq!(
            s.to_csv(),
            "date,close,divCash\n2026-01-01,10.1,\n2026-01-03,,0.5\n"
        );
    }

    #[test]
    fn a_rendered_series_reads_back() {
        let mut event = PricePoint::new(d("2026-01-03"));
        event.dividend = Some(Decimal::from_str("0.5").unwrap());
        event
            .extra
            .insert("VWAP".into(), Decimal::from_str("10.05").unwrap());
        let s = series(vec![priced("2026-01-01", "10.1"), event]);
        let json = s.to_json(Schema::Xfina).unwrap();
        let back = PriceSeries::from_json(json).unwrap();
        assert_eq!(back.rows, s.rows);
        assert_eq!(back.to_csv(), s.to_csv());
    }

    #[test]
    fn rebit_has_no_word_for_a_price_series() {
        let s = series(vec![priced("2026-01-01", "1")]);
        assert!(matches!(
            s.to_json(Schema::Rebit),
            Err(XfinaError::SchemaUnsupported { .. })
        ));
        let json = s.to_json(Schema::Xfina).unwrap();
        // Dates follow the xfina convention: midnight IST as an epoch.
        assert_eq!(json["rows"][0]["date"], 1767205800);
        assert_eq!(json["rows"][0]["close"], 1.0);
        assert_eq!(json["source"], "md-tiingo");
    }
}
