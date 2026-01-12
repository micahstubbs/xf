//! Human-friendly date parsing utilities.
//!
//! Supports natural language expressions via `chrono-english` with
//! targeted helpers for month/year ranges and ISO fallback.

use anyhow::{Result, anyhow};
use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime,
    TimeZone, Utc,
};
use chrono_english::{Dialect, parse_date_string};
use tracing::{debug, trace, warn};

/// Result of parsing a date expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedDate {
    /// A specific point in time.
    Point(DateTime<Utc>),
    /// A date range (inclusive).
    Range {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    },
}

impl ParsedDate {
    /// Get the start of this date (for --from).
    #[must_use]
    pub const fn start(&self) -> DateTime<Utc> {
        match self {
            Self::Point(dt) => *dt,
            Self::Range { start, .. } => *start,
        }
    }

    /// Get the end of this date (for --to).
    #[must_use]
    pub const fn end(&self) -> DateTime<Utc> {
        match self {
            Self::Point(dt) => *dt,
            Self::Range { end, .. } => *end,
        }
    }
}

/// Parse a human-readable date expression.
///
/// # Errors
/// Returns an error if the expression cannot be parsed.
pub fn parse_human_date(input: &str, prefer_end: bool) -> Result<ParsedDate> {
    parse_human_date_with_base(input, prefer_end, Local::now())
}

/// Parse a human-readable date expression using a fixed base time.
fn parse_human_date_with_base(
    input: &str,
    prefer_end: bool,
    base: DateTime<Local>,
) -> Result<ParsedDate> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("Date expression is empty"));
    }

    let normalized = trimmed.to_lowercase();
    let _ = prefer_end;

    if let Some(parsed) = try_parse_quarter(&normalized) {
        debug!(input = trimmed, "Parsed quarter range");
        return Ok(parsed);
    }

    if let Some(parsed) = try_parse_season(&normalized) {
        debug!(input = trimmed, "Parsed season range");
        return Ok(parsed);
    }

    if let Some(parsed) = try_parse_month_year(&normalized) {
        debug!(input = trimmed, "Parsed month/year range");
        return Ok(parsed);
    }

    if let Some(parsed) = try_parse_relative(&normalized, base) {
        debug!(input = trimmed, "Parsed relative date expression");
        return Ok(parsed);
    }

    match parse_date_string(&normalized, base, Dialect::Us) {
        Ok(dt) => {
            debug!(input = trimmed, "Parsed natural language date");
            if !has_explicit_time(&normalized) {
                if let Some(range) = range_for_dates(dt.date_naive(), dt.date_naive()) {
                    return Ok(range);
                }
            }
            Ok(ParsedDate::Point(dt.with_timezone(&Utc)))
        }
        Err(err) => Err(anyhow!(
            "Could not parse date expression: '{trimmed}' ({err})"
        )),
    }
}

/// Unified parser: try natural language, fall back to ISO.
///
/// # Errors
/// Returns an error if parsing fails.
pub fn parse_date_flexible(input: &str, prefer_end: bool) -> Result<DateTime<Utc>> {
    if is_strict_iso_date(input) {
        if let Some(parsed) = try_parse_iso(input, prefer_end) {
            trace!(input = input, "Parsed strict ISO date");
            return Ok(parsed);
        }
    }

    if let Some(parsed) = try_parse_iso_datetime(input) {
        trace!(input = input, "Parsed ISO datetime");
        return Ok(parsed);
    }

    if let Ok(parsed) = parse_human_date(input, prefer_end) {
        return Ok(if prefer_end {
            parsed.end()
        } else {
            parsed.start()
        });
    }

    if let Some(parsed) = try_parse_iso(input, prefer_end) {
        trace!(input = input, "Falling back to ISO date parsing");
        return Ok(parsed);
    }

    warn!(input = input, "Failed to parse date expression");
    Err(anyhow!("Could not parse '{input}' as date"))
}

fn is_strict_iso_date(input: &str) -> bool {
    NaiveDate::parse_from_str(input.trim(), "%Y-%m-%d").is_ok()
}

/// Try parsing as ISO format (YYYY-MM-DD).
#[must_use]
pub fn try_parse_iso(input: &str, prefer_end: bool) -> Option<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(input.trim(), "%Y-%m-%d").ok()?;
    let time = if prefer_end {
        NaiveTime::from_hms_opt(23, 59, 59)?
    } else {
        NaiveTime::from_hms_opt(0, 0, 0)?
    };
    Some(DateTime::from_naive_utc_and_offset(
        date.and_time(time),
        Utc,
    ))
}

fn try_parse_iso_datetime(input: &str) -> Option<DateTime<Utc>> {
    let trimmed = input.trim();

    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.with_timezone(&Utc));
    }

    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return local_naive_datetime_to_utc(naive);
    }

    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M") {
        return local_naive_datetime_to_utc(naive);
    }

    None
}

fn has_explicit_time(input: &str) -> bool {
    if input.contains(':') {
        return true;
    }

    let normalized = input.to_lowercase();
    for token in normalized
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
    {
        if matches!(token, "am" | "pm" | "noon" | "midnight") {
            return true;
        }
        if token.len() > 2 {
            let (num, suffix) = token.split_at(token.len() - 2);
            if matches!(suffix, "am" | "pm") && num.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }

    false
}

fn try_parse_relative(input: &str, base: DateTime<Local>) -> Option<ParsedDate> {
    let tokens: Vec<&str> = input.split_whitespace().collect();
    let base_date = base.date_naive();

    match tokens.as_slice() {
        ["today"] => range_for_dates(base_date, base_date),
        ["yesterday"] => {
            let date = base_date.pred_opt()?;
            range_for_dates(date, date)
        }
        ["weekend"] => weekend_range(base_date),
        ["weekday" | "weekdays"] => weekdays_range(base_date),
        ["this", "month"] => month_to_date_range(base_date),
        ["this", "year"] => year_to_date_range(base_date),
        ["last" | "past", "week"] => rolling_days_range(base_date, 7),
        ["last" | "past", "month"] => previous_month_range(base_date),
        ["last" | "past", "year"] => previous_year_range(base_date),
        ["last" | "past", count, unit] => {
            let count = parse_count(count)?;
            let unit = parse_unit(unit)?;
            parse_last_n(base_date, count, unit)
        }
        [count, unit, "ago"] => {
            let count = parse_count(count)?;
            let unit = parse_unit(unit)?;
            parse_ago(base_date, count, unit)
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum RelativeUnit {
    Day,
    Week,
    Month,
    Year,
}

fn parse_count(token: &str) -> Option<i64> {
    let count = token.parse::<i64>().ok()?;
    if count <= 0 {
        return None;
    }
    Some(count)
}

fn parse_unit(token: &str) -> Option<RelativeUnit> {
    match token.trim_end_matches('s') {
        "day" => Some(RelativeUnit::Day),
        "week" => Some(RelativeUnit::Week),
        "month" => Some(RelativeUnit::Month),
        "year" => Some(RelativeUnit::Year),
        _ => None,
    }
}

fn parse_last_n(base_date: NaiveDate, count: i64, unit: RelativeUnit) -> Option<ParsedDate> {
    match unit {
        RelativeUnit::Day => rolling_days_range(base_date, count),
        RelativeUnit::Week => rolling_days_range(base_date, count.saturating_mul(7)),
        RelativeUnit::Month => {
            let count_i32 = i32::try_from(count).ok()?;
            let start = shift_months(base_date, -count_i32)?;
            range_for_dates(start, base_date)
        }
        RelativeUnit::Year => {
            let count_i32 = i32::try_from(count).ok()?;
            let start = shift_years(base_date, -count_i32)?;
            range_for_dates(start, base_date)
        }
    }
}

fn parse_ago(base_date: NaiveDate, count: i64, unit: RelativeUnit) -> Option<ParsedDate> {
    match unit {
        RelativeUnit::Day => point_for_date(base_date - Duration::days(count)),
        RelativeUnit::Week => point_for_date(base_date - Duration::days(count.saturating_mul(7))),
        RelativeUnit::Month => {
            let count_i32 = i32::try_from(count).ok()?;
            point_for_date(shift_months(base_date, -count_i32)?)
        }
        RelativeUnit::Year => {
            let count_i32 = i32::try_from(count).ok()?;
            point_for_date(shift_years(base_date, -count_i32)?)
        }
    }
}

fn rolling_days_range(base_date: NaiveDate, days: i64) -> Option<ParsedDate> {
    if days <= 0 {
        return None;
    }
    let start = base_date - Duration::days(days - 1);
    range_for_dates(start, base_date)
}

fn month_to_date_range(base_date: NaiveDate) -> Option<ParsedDate> {
    let start = NaiveDate::from_ymd_opt(base_date.year(), base_date.month(), 1)?;
    range_for_dates(start, base_date)
}

fn year_to_date_range(base_date: NaiveDate) -> Option<ParsedDate> {
    let start = NaiveDate::from_ymd_opt(base_date.year(), 1, 1)?;
    range_for_dates(start, base_date)
}

fn previous_month_range(base_date: NaiveDate) -> Option<ParsedDate> {
    let first_current = NaiveDate::from_ymd_opt(base_date.year(), base_date.month(), 1)?;
    let last_prev = first_current - Duration::days(1);
    let first_prev = NaiveDate::from_ymd_opt(last_prev.year(), last_prev.month(), 1)?;
    range_for_dates(first_prev, last_prev)
}

fn previous_year_range(base_date: NaiveDate) -> Option<ParsedDate> {
    let year = base_date.year() - 1;
    let start = NaiveDate::from_ymd_opt(year, 1, 1)?;
    let end = NaiveDate::from_ymd_opt(year, 12, 31)?;
    range_for_dates(start, end)
}

fn weekend_range(base_date: NaiveDate) -> Option<ParsedDate> {
    let weekday = i64::from(base_date.weekday().num_days_from_monday());
    let offset = (weekday + 2) % 7;
    let saturday = base_date - Duration::days(offset);
    let sunday = saturday + Duration::days(1);
    range_for_dates(saturday, sunday)
}

fn weekdays_range(base_date: NaiveDate) -> Option<ParsedDate> {
    let weekday = base_date.weekday().num_days_from_monday();
    let monday = base_date - Duration::days(i64::from(weekday));
    let end = if weekday <= 4 {
        base_date
    } else {
        monday + Duration::days(4)
    };
    range_for_dates(monday, end)
}

fn range_for_dates(start: NaiveDate, end: NaiveDate) -> Option<ParsedDate> {
    let start_dt = local_start_of_day(start)?;
    let end_dt = local_end_of_day(end)?;
    Some(ParsedDate::Range {
        start: start_dt,
        end: end_dt,
    })
}

fn point_for_date(date: NaiveDate) -> Option<ParsedDate> {
    Some(ParsedDate::Point(local_start_of_day(date)?))
}

fn local_start_of_day(date: NaiveDate) -> Option<DateTime<Utc>> {
    let time = NaiveTime::from_hms_opt(0, 0, 0)?;
    local_datetime_to_utc(date, time)
}

fn local_end_of_day(date: NaiveDate) -> Option<DateTime<Utc>> {
    let time = NaiveTime::from_hms_opt(23, 59, 59)?;
    local_datetime_to_utc(date, time)
}

fn local_datetime_to_utc(date: NaiveDate, time: NaiveTime) -> Option<DateTime<Utc>> {
    let naive = date.and_time(time);
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, _) => Some(first.with_timezone(&Utc)),
        LocalResult::None => None,
    }
}

fn local_naive_datetime_to_utc(naive: NaiveDateTime) -> Option<DateTime<Utc>> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, _) => Some(first.with_timezone(&Utc)),
        LocalResult::None => None,
    }
}

fn shift_months(date: NaiveDate, delta_months: i32) -> Option<NaiveDate> {
    let year_months = date.year().checked_mul(12)?;
    let month_i32 = i32::try_from(date.month()).ok()?;
    let total_months = year_months + (month_i32 - 1) + delta_months;
    let new_year = total_months.div_euclid(12);
    let new_month = total_months.rem_euclid(12) + 1;
    let new_month_u32 = u32::try_from(new_month).ok()?;
    let last_day = last_day_of_month(new_year, new_month_u32)?;
    let day = date.day().min(last_day);
    NaiveDate::from_ymd_opt(new_year, new_month_u32, day)
}

fn shift_years(date: NaiveDate, delta_years: i32) -> Option<NaiveDate> {
    shift_months(date, delta_years * 12)
}

fn last_day_of_month(year: i32, month: u32) -> Option<u32> {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_next = NaiveDate::from_ymd_opt(next_year, next_month, 1)?;
    let last = first_next - Duration::days(1);
    Some(last.day())
}

fn try_parse_quarter(input: &str) -> Option<ParsedDate> {
    let cleaned = input.replace('-', " ");
    let mut parts = cleaned.split_whitespace();
    let quarter_token = parts.next()?;
    let year_token = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    let quarter = parse_quarter_token(quarter_token)?;
    let year = year_token.parse::<i32>().ok()?;
    quarter_range(year, quarter)
}

fn parse_quarter_token(token: &str) -> Option<u32> {
    let trimmed = token.trim().trim_start_matches('q');
    let quarter = trimmed.parse::<u32>().ok()?;
    if (1..=4).contains(&quarter) {
        Some(quarter)
    } else {
        None
    }
}

fn quarter_range(year: i32, quarter: u32) -> Option<ParsedDate> {
    let (start_month, end_month) = match quarter {
        1 => (1, 3),
        2 => (4, 6),
        3 => (7, 9),
        4 => (10, 12),
        _ => return None,
    };
    let start = NaiveDate::from_ymd_opt(year, start_month, 1)?;
    let end_day = last_day_of_month(year, end_month)?;
    let end = NaiveDate::from_ymd_opt(year, end_month, end_day)?;
    range_for_dates(start, end)
}

fn try_parse_season(input: &str) -> Option<ParsedDate> {
    let cleaned = input.replace('-', " ");
    let mut parts = cleaned.split_whitespace();
    let season_token = parts.next()?;
    let year_token = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    let year = year_token.parse::<i32>().ok()?;
    season_range(season_token, year)
}

fn season_range(season: &str, year: i32) -> Option<ParsedDate> {
    match season {
        "spring" => season_range_months(year, 3, 5),
        "summer" => season_range_months(year, 6, 8),
        "fall" | "autumn" => season_range_months(year, 9, 11),
        "winter" => {
            let start = NaiveDate::from_ymd_opt(year, 12, 1)?;
            let end_year = year + 1;
            let end_day = last_day_of_month(end_year, 2)?;
            let end = NaiveDate::from_ymd_opt(end_year, 2, end_day)?;
            range_for_dates(start, end)
        }
        _ => None,
    }
}

fn season_range_months(year: i32, start_month: u32, end_month: u32) -> Option<ParsedDate> {
    let start = NaiveDate::from_ymd_opt(year, start_month, 1)?;
    let end_day = last_day_of_month(year, end_month)?;
    let end = NaiveDate::from_ymd_opt(year, end_month, end_day)?;
    range_for_dates(start, end)
}

fn try_parse_month_year(input: &str) -> Option<ParsedDate> {
    if let Some((year, month)) = parse_year_month_numeric(input) {
        return month_range(year, month);
    }

    let mut parts = input.split_whitespace();
    let month_part = parts.next()?;
    let year_part = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    let month = parse_month_name(month_part)?;
    let year = year_part.parse::<i32>().ok()?;
    month_range(year, month)
}

fn parse_year_month_numeric(input: &str) -> Option<(i32, u32)> {
    let trimmed = input.trim();
    let mut parts = trimmed.split(['-', '/']);
    let year_str = parts.next()?;
    let month_str = parts.next()?;
    if parts.next().is_some() {
        return None;
    }

    let year = year_str.parse::<i32>().ok()?;
    let month = month_str.parse::<u32>().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    Some((year, month))
}

fn parse_month_name(input: &str) -> Option<u32> {
    match input.trim().to_lowercase().as_str() {
        "jan" | "january" => Some(1),
        "feb" | "february" => Some(2),
        "mar" | "march" => Some(3),
        "apr" | "april" => Some(4),
        "may" => Some(5),
        "jun" | "june" => Some(6),
        "jul" | "july" => Some(7),
        "aug" | "august" => Some(8),
        "sep" | "sept" | "september" => Some(9),
        "oct" | "october" => Some(10),
        "nov" | "november" => Some(11),
        "dec" | "december" => Some(12),
        _ => None,
    }
}

fn month_range(year: i32, month: u32) -> Option<ParsedDate> {
    let start = Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0).single()?;
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let next_start = Utc
        .with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0)
        .single()?;
    let end = next_start - Duration::seconds(1);
    Some(ParsedDate::Range { start, end })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    fn assert_local_range(parsed: &ParsedDate, start: NaiveDate, end: NaiveDate) {
        match parsed {
            ParsedDate::Range { start: s, end: e } => {
                let start_local = s.with_timezone(&Local);
                let end_local = e.with_timezone(&Local);
                assert_eq!(start_local.date_naive(), start);
                assert_eq!(
                    start_local.time(),
                    NaiveTime::from_hms_opt(0, 0, 0).unwrap()
                );
                assert_eq!(end_local.date_naive(), end);
                assert_eq!(
                    end_local.time(),
                    NaiveTime::from_hms_opt(23, 59, 59).unwrap()
                );
            }
            ParsedDate::Point(_) => panic!("Expected range result"),
        }
    }

    fn assert_local_point(parsed: &ParsedDate, date: NaiveDate) {
        match parsed {
            ParsedDate::Point(dt) => {
                let local = dt.with_timezone(&Local);
                assert_eq!(local.date_naive(), date);
                assert_eq!(local.time(), NaiveTime::from_hms_opt(0, 0, 0).unwrap());
            }
            ParsedDate::Range { .. } => panic!("Expected point result"),
        }
    }

    #[test]
    fn parse_iso_start_end() {
        let start = parse_date_flexible("2024-01-15", false).expect("start parsed");
        let end = parse_date_flexible("2024-01-15", true).expect("end parsed");

        assert_eq!(
            start,
            Utc.with_ymd_and_hms(2024, 1, 15, 0, 0, 0).single().unwrap()
        );
        assert_eq!(
            end,
            Utc.with_ymd_and_hms(2024, 1, 15, 23, 59, 59)
                .single()
                .unwrap()
        );
    }

    #[test]
    fn parse_iso_datetime_rfc3339() {
        let parsed = parse_date_flexible("2024-01-15T12:34:56Z", false).expect("parsed rfc3339");
        let expected = Utc
            .with_ymd_and_hms(2024, 1, 15, 12, 34, 56)
            .single()
            .unwrap();
        assert_eq!(parsed, expected);
    }

    #[test]
    fn parse_iso_datetime_local() {
        let parsed = parse_date_flexible("2024-01-15 12:34", false).expect("parsed local datetime");
        let local = Local
            .with_ymd_and_hms(2024, 1, 15, 12, 34, 0)
            .single()
            .unwrap();
        assert_eq!(parsed, local.with_timezone(&Utc));
    }

    #[test]
    fn parse_month_year_numeric_range() {
        let parsed = parse_human_date("2024-02", false).expect("parsed month range");
        match parsed {
            ParsedDate::Range { start, end } => {
                assert_eq!(
                    start,
                    Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).single().unwrap()
                );
                assert_eq!(
                    end,
                    Utc.with_ymd_and_hms(2024, 2, 29, 23, 59, 59)
                        .single()
                        .unwrap()
                );
            }
            ParsedDate::Point(_) => panic!("Expected range for month/year"),
        }
    }

    #[test]
    fn parse_month_year_text_range() {
        let parsed = parse_human_date("Jan 2023", false).expect("parsed month name");
        match parsed {
            ParsedDate::Range { start, end } => {
                assert_eq!(
                    start,
                    Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).single().unwrap()
                );
                assert_eq!(
                    end,
                    Utc.with_ymd_and_hms(2023, 1, 31, 23, 59, 59)
                        .single()
                        .unwrap()
                );
            }
            ParsedDate::Point(_) => panic!("Expected range for month/year"),
        }
    }

    #[test]
    fn parse_relative_with_base() {
        let base = Local
            .with_ymd_and_hms(2024, 1, 15, 12, 0, 0)
            .single()
            .expect("base time");
        let expected = base.date_naive().pred_opt().expect("prev day");
        let parsed =
            parse_human_date_with_base("yesterday", false, base).expect("parsed yesterday");
        assert_local_range(&parsed, expected, expected);
    }

    #[test]
    fn parse_last_week_range() {
        let base = Local
            .with_ymd_and_hms(2024, 1, 15, 12, 0, 0)
            .single()
            .expect("base time");
        let parsed =
            parse_human_date_with_base("last week", false, base).expect("parsed last week");
        let start = NaiveDate::from_ymd_opt(2024, 1, 9).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        assert_local_range(&parsed, start, end);
    }

    #[test]
    fn parse_days_ago_point() {
        let base = Local
            .with_ymd_and_hms(2024, 1, 15, 12, 0, 0)
            .single()
            .expect("base time");
        let parsed =
            parse_human_date_with_base("3 days ago", false, base).expect("parsed 3 days ago");
        let expected = NaiveDate::from_ymd_opt(2024, 1, 12).unwrap();
        assert_local_point(&parsed, expected);
    }

    #[test]
    fn parse_this_month_range() {
        let base = Local
            .with_ymd_and_hms(2024, 1, 15, 12, 0, 0)
            .single()
            .expect("base time");
        let parsed =
            parse_human_date_with_base("this month", false, base).expect("parsed this month");
        let start = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        assert_local_range(&parsed, start, end);
    }

    #[test]
    fn parse_last_month_range() {
        let base = Local
            .with_ymd_and_hms(2024, 1, 15, 12, 0, 0)
            .single()
            .expect("base time");
        let parsed =
            parse_human_date_with_base("last month", false, base).expect("parsed last month");
        let start = NaiveDate::from_ymd_opt(2023, 12, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2023, 12, 31).unwrap();
        assert_local_range(&parsed, start, end);
    }

    #[test]
    fn parse_quarter_range() {
        let parsed = parse_human_date("Q1 2024", false).expect("parsed quarter");
        let start = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 3, 31).unwrap();
        assert_local_range(&parsed, start, end);
    }

    #[test]
    fn parse_season_range() {
        let parsed = parse_human_date("summer 2023", false).expect("parsed season");
        let start = NaiveDate::from_ymd_opt(2023, 6, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2023, 8, 31).unwrap();
        assert_local_range(&parsed, start, end);
    }

    #[test]
    fn parse_winter_range() {
        let parsed = parse_human_date("winter 2023", false).expect("parsed winter");
        let start = NaiveDate::from_ymd_opt(2023, 12, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        assert_local_range(&parsed, start, end);
    }

    #[test]
    fn parse_weekend_range() {
        let base = Local
            .with_ymd_and_hms(2024, 1, 17, 12, 0, 0)
            .single()
            .expect("base time");
        let parsed = parse_human_date_with_base("weekend", false, base).expect("parsed weekend");
        let start = NaiveDate::from_ymd_opt(2024, 1, 13).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 1, 14).unwrap();
        assert_local_range(&parsed, start, end);
    }

    #[test]
    fn parse_weekdays_range() {
        let base = Local
            .with_ymd_and_hms(2024, 1, 17, 12, 0, 0)
            .single()
            .expect("base time");
        let parsed = parse_human_date_with_base("weekdays", false, base).expect("parsed weekdays");
        let start = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 1, 17).unwrap();
        assert_local_range(&parsed, start, end);
    }

    #[test]
    fn parse_today_range() {
        let base = Local
            .with_ymd_and_hms(2024, 6, 10, 9, 0, 0)
            .single()
            .expect("base time");
        let parsed = parse_human_date_with_base("today", false, base).expect("parsed today");
        let date = NaiveDate::from_ymd_opt(2024, 6, 10).unwrap();
        assert_local_range(&parsed, date, date);
    }

    #[test]
    fn parse_this_year_range() {
        let base = Local
            .with_ymd_and_hms(2024, 6, 10, 9, 0, 0)
            .single()
            .expect("base time");
        let parsed =
            parse_human_date_with_base("this year", false, base).expect("parsed this year");
        let start = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 6, 10).unwrap();
        assert_local_range(&parsed, start, end);
    }

    #[test]
    fn parse_last_year_range() {
        let base = Local
            .with_ymd_and_hms(2024, 6, 10, 9, 0, 0)
            .single()
            .expect("base time");
        let parsed =
            parse_human_date_with_base("last year", false, base).expect("parsed last year");
        let start = NaiveDate::from_ymd_opt(2023, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2023, 12, 31).unwrap();
        assert_local_range(&parsed, start, end);
    }

    #[test]
    fn parse_last_n_days_range() {
        let base = Local
            .with_ymd_and_hms(2024, 1, 15, 12, 0, 0)
            .single()
            .expect("base time");
        let parsed =
            parse_human_date_with_base("last 14 days", false, base).expect("parsed last 14 days");
        let start = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap();
        assert_local_range(&parsed, start, end);
    }

    #[test]
    fn parse_month_prefer_end() {
        let parsed = parse_date_flexible("Jan 2023", true).expect("parsed month");
        let expected = Utc
            .with_ymd_and_hms(2023, 1, 31, 23, 59, 59)
            .single()
            .unwrap();
        assert_eq!(parsed, expected);
    }

    #[test]
    fn parse_named_date_prefer_end() {
        let parsed = parse_date_flexible("January 15, 2024", true).expect("parsed date");
        let local_end = Local
            .with_ymd_and_hms(2024, 1, 15, 23, 59, 59)
            .single()
            .unwrap();
        assert_eq!(parsed, local_end.with_timezone(&Utc));
    }

    #[test]
    fn parse_invalid_expression_error() {
        let err = parse_date_flexible("not-a-real-date", false).expect_err("should fail parsing");
        let message = format!("{err}");
        assert!(message.contains("not-a-real-date"));
    }
}
