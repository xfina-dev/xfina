use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Months, NaiveDate, NaiveDateTime, TimeZone, Utc,
};

/// India Standard Time, UTC+05:30.
///
/// Every statement xfina reads is printed by an Indian institution in local
/// time, and India keeps one offset all year with no daylight saving, so a
/// fixed offset is exact rather than an approximation of a zone.
pub fn ist() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("+05:30 is a valid offset")
}

/// A local Indian timestamp as the instant it names.
pub fn ist_to_utc(local: NaiveDateTime) -> DateTime<Utc> {
    ist()
        .from_local_datetime(&local)
        .single()
        // A fixed offset maps every local time to exactly one instant, so this
        // is unreachable; subtracting the offset is the same answer anyway.
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|| (local - Duration::minutes(330)).and_utc())
}

/// Midnight IST on `date`, as the instant it names.
///
/// This is how a date with no time is represented once it has to become a
/// timestamp: the start of that day in the timezone the statement was printed
/// in, never midnight UTC, which is 05:30 on the same morning in India.
pub fn ist_midnight(date: NaiveDate) -> DateTime<Utc> {
    ist_to_utc(date.and_hms_opt(0, 0, 0).expect("midnight is a valid time"))
}

/// The Indian calendar date an instant falls on.
///
/// The counterpart of [`ist_midnight`]. Reading the date off a `DateTime<Utc>`
/// directly answers a different question -- which UTC day it fell on -- and
/// for anything before 05:30 IST that is the day before.
pub fn ist_date(dt: DateTime<Utc>) -> NaiveDate {
    dt.with_timezone(&ist()).date_naive()
}

/// Derives the full transaction date (with year) from partial date info.
/// Returns `DateTime<Utc>` at midnight IST, converted to UTC.
pub fn derive_transaction_date(stmt_date: NaiveDate, tx_day: u32, tx_month: u32) -> DateTime<Utc> {
    let stmt_year = stmt_date.year();
    let stmt_month = stmt_date.month();

    // If the transaction month is strictly greater than the statement month,
    // it must have occurred in the previous year.
    let tx_year = if tx_month > stmt_month {
        stmt_year - 1
    } else {
        stmt_year
    };

    let naive = NaiveDate::from_ymd_opt(tx_year, tx_month, tx_day).unwrap_or(stmt_date);
    ist_midnight(naive)
}

/// Derives the statement period (start, end) as `DateTime<Utc>` at midnight IST.
pub fn derive_statement_period(stmt_date: NaiveDate) -> (NaiveDate, NaiveDate) {
    let prev_month = stmt_date
        .checked_sub_months(Months::new(1))
        .unwrap_or(stmt_date);
    let start_date = prev_month
        .checked_add_signed(Duration::days(1))
        .unwrap_or(prev_month);
    (start_date, stmt_date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_becomes_the_start_of_that_day_in_india() {
        let d = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
        // 00:00 IST is 18:30 UTC the previous day.
        assert_eq!(ist_midnight(d).to_rfc3339(), "2026-03-01T18:30:00+00:00");
        assert_eq!(ist_date(ist_midnight(d)), d, "a date must survive the trip");
    }

    #[test]
    fn the_indian_date_is_not_the_utc_date_before_0530() {
        // The bug this exists to prevent: reading the calendar date off the
        // UTC instant moves everything before 05:30 IST to the day before.
        let early = ist_to_utc(
            NaiveDate::from_ymd_opt(2026, 3, 2)
                .unwrap()
                .and_hms_opt(5, 29, 0)
                .unwrap(),
        );
        assert_eq!(
            ist_date(early),
            NaiveDate::from_ymd_opt(2026, 3, 2).unwrap()
        );
        assert_eq!(
            early.date_naive(),
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            "the UTC day is the day before, which is what made this wrong"
        );
    }

    #[test]
    fn a_time_is_read_as_indian_local_time() {
        let local = NaiveDate::from_ymd_opt(2026, 3, 2)
            .unwrap()
            .and_hms_opt(9, 40, 0)
            .unwrap();
        assert_eq!(ist_to_utc(local).to_rfc3339(), "2026-03-02T04:10:00+00:00");
    }
}
