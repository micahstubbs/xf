//! Enhanced statistics and analytics for archive data.
//!
//! This module provides advanced analytics beyond basic counts, including:
//! - Temporal analysis (activity patterns over time)
//! - Engagement metrics (likes, retweets distribution)
//! - Content analysis (media ratios, hashtags, mentions)

use crate::Result;
use crate::storage::Storage;
use chrono::NaiveDate;
use serde::Serialize;

/// Temporal statistics showing activity patterns over time.
#[derive(Debug, Clone, Serialize)]
pub struct TemporalStats {
    /// Tweets per day for the entire archive period
    pub daily_counts: Vec<DailyCount>,
    /// Tweets per hour of day (0-23), aggregated across all days
    pub hourly_distribution: [u64; 24],
    /// Tweets per day of week (0=Sunday, 6=Saturday)
    pub dow_distribution: [u64; 7],
    /// Longest gap between tweets
    pub longest_gap_days: i64,
    /// Start date of the longest gap
    pub longest_gap_start: Option<NaiveDate>,
    /// End date of the longest gap
    pub longest_gap_end: Option<NaiveDate>,
    /// Day with most tweets
    pub most_active_day: Option<NaiveDate>,
    /// Tweet count on most active day
    pub most_active_day_count: u64,
    /// Most active hour (0-23)
    pub most_active_hour: u8,
    /// Tweet count for most active hour
    pub most_active_hour_count: u64,
    /// Average tweets per day (on days with activity)
    pub avg_tweets_per_active_day: f64,
    /// Total days with at least one tweet
    pub active_days_count: u64,
    /// Total days in archive range
    pub total_days_in_range: u64,
}

/// A single day's tweet count.
#[derive(Debug, Clone, Serialize)]
pub struct DailyCount {
    pub date: NaiveDate,
    pub count: u64,
}

impl TemporalStats {
    /// Compute temporal statistics from the storage.
    ///
    /// Uses SQL aggregations for efficiency on large datasets.
    ///
    /// # Errors
    ///
    /// Returns an error if database queries fail.
    #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
    pub fn compute(storage: &Storage) -> Result<Self> {
        // Get daily counts using SQL
        let daily_counts = Self::query_daily_counts(storage)?;

        // Get hourly distribution
        let hourly_distribution = Self::query_hourly_distribution(storage)?;

        // Get day-of-week distribution
        let dow_distribution = Self::query_dow_distribution(storage)?;

        // Compute derived metrics
        let (longest_gap_days, longest_gap_start, longest_gap_end) =
            Self::find_longest_gap(&daily_counts);

        let (most_active_day, most_active_day_count) = daily_counts
            .iter()
            .max_by_key(|d| d.count)
            .map_or((None, 0), |d| (Some(d.date), d.count));

        #[allow(clippy::cast_possible_truncation)]
        let (most_active_hour, most_active_hour_count) = hourly_distribution
            .iter()
            .enumerate()
            .max_by_key(|(_, count)| *count)
            .map_or((0, 0), |(hour, count)| (hour as u8, *count));

        let active_days_count = daily_counts.len() as u64;
        let total_tweets: u64 = daily_counts.iter().map(|d| d.count).sum();
        let avg_tweets_per_active_day = if active_days_count > 0 {
            total_tweets as f64 / active_days_count as f64
        } else {
            0.0
        };

        let total_days_in_range =
            if let (Some(first), Some(last)) = (daily_counts.first(), daily_counts.last()) {
                (last.date - first.date).num_days() as u64 + 1
            } else {
                0
            };

        Ok(Self {
            daily_counts,
            hourly_distribution,
            dow_distribution,
            longest_gap_days,
            longest_gap_start,
            longest_gap_end,
            most_active_day,
            most_active_day_count,
            most_active_hour,
            most_active_hour_count,
            avg_tweets_per_active_day,
            active_days_count,
            total_days_in_range,
        })
    }

    /// Query daily tweet counts from the database.
    #[allow(clippy::cast_sign_loss)]
    fn query_daily_counts(storage: &Storage) -> Result<Vec<DailyCount>> {
        let query = r"
            SELECT DATE(created_at) as day, COUNT(*) as count
            FROM tweets
            WHERE created_at IS NOT NULL
            GROUP BY day
            ORDER BY day
        ";

        let conn = storage.connection();
        let mut stmt = conn.prepare(query)?;
        let rows = stmt.query_map([], |row| {
            let day_str: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((day_str, count as u64))
        })?;

        let mut counts = Vec::new();
        for row in rows {
            let (day_str, count) = row?;
            if let Ok(date) = NaiveDate::parse_from_str(&day_str, "%Y-%m-%d") {
                counts.push(DailyCount { date, count });
            }
        }

        Ok(counts)
    }

    /// Query hourly distribution (tweets per hour of day).
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    fn query_hourly_distribution(storage: &Storage) -> Result<[u64; 24]> {
        let query = r"
            SELECT CAST(strftime('%H', created_at) AS INTEGER) as hour, COUNT(*) as count
            FROM tweets
            WHERE created_at IS NOT NULL
            GROUP BY hour
            ORDER BY hour
        ";

        let conn = storage.connection();
        let mut stmt = conn.prepare(query)?;
        let rows = stmt.query_map([], |row| {
            let hour: i64 = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((hour as usize, count as u64))
        })?;

        let mut distribution = [0u64; 24];
        for row in rows {
            let (hour, count) = row?;
            if hour < 24 {
                distribution[hour] = count;
            }
        }

        Ok(distribution)
    }

    /// Query day-of-week distribution.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    fn query_dow_distribution(storage: &Storage) -> Result<[u64; 7]> {
        // SQLite strftime('%w') returns 0=Sunday, 1=Monday, ..., 6=Saturday
        let query = r"
            SELECT CAST(strftime('%w', created_at) AS INTEGER) as dow, COUNT(*) as count
            FROM tweets
            WHERE created_at IS NOT NULL
            GROUP BY dow
            ORDER BY dow
        ";

        let conn = storage.connection();
        let mut stmt = conn.prepare(query)?;
        let rows = stmt.query_map([], |row| {
            let dow: i64 = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((dow as usize, count as u64))
        })?;

        let mut distribution = [0u64; 7];
        for row in rows {
            let (dow, count) = row?;
            if dow < 7 {
                distribution[dow] = count;
            }
        }

        Ok(distribution)
    }

    /// Find the longest gap between consecutive days with tweets.
    fn find_longest_gap(
        daily_counts: &[DailyCount],
    ) -> (i64, Option<NaiveDate>, Option<NaiveDate>) {
        if daily_counts.len() < 2 {
            return (0, None, None);
        }

        let mut max_gap = 0i64;
        let mut gap_start: Option<NaiveDate> = None;
        let mut gap_end: Option<NaiveDate> = None;

        for window in daily_counts.windows(2) {
            let gap = (window[1].date - window[0].date).num_days();
            if gap > max_gap {
                max_gap = gap;
                gap_start = Some(window[0].date);
                gap_end = Some(window[1].date);
            }
        }

        (max_gap, gap_start, gap_end)
    }
}

/// Generate an ASCII sparkline from a slice of values.
///
/// Uses Unicode block characters: ▁▂▃▄▅▆▇█
///
/// # Arguments
/// * `values` - The values to visualize
/// * `width` - Target width (values will be bucketed if len > width)
///
/// # Returns
/// A string of sparkline characters
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn sparkline(values: &[u64], width: usize) -> String {
    if values.is_empty() || width == 0 {
        return String::new();
    }

    let blocks = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    // Bucket values if we have more than width
    let bucketed: Vec<u64> = if values.len() <= width {
        values.to_vec()
    } else {
        let bucket_size = values.len().div_ceil(width);
        values
            .chunks(bucket_size)
            .map(|chunk| chunk.iter().sum::<u64>() / chunk.len() as u64)
            .collect()
    };

    let max = *bucketed.iter().max().unwrap_or(&1);
    if max == 0 {
        return "▁".repeat(bucketed.len().min(width));
    }

    bucketed
        .iter()
        .take(width)
        .map(|&v| {
            let idx = ((v as f64 / max as f64) * 7.0) as usize;
            blocks[idx.min(7)]
        })
        .collect()
}

/// Generate a sparkline from daily counts.
#[must_use]
pub fn sparkline_from_daily(daily_counts: &[DailyCount], width: usize) -> String {
    let values: Vec<u64> = daily_counts.iter().map(|d| d.count).collect();
    sparkline(&values, width)
}

/// Format day-of-week distribution as a mini-bar chart.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn format_dow_distribution(distribution: &[u64; 7]) -> String {
    let days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let max = *distribution.iter().max().unwrap_or(&1);

    days.iter()
        .zip(distribution.iter())
        .map(|(day, &count)| {
            let bar_len = if max > 0 {
                ((count as f64 / max as f64) * 10.0) as usize
            } else {
                0
            };
            format!("{day}: {}", "█".repeat(bar_len))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format hourly distribution as a 24-hour sparkline.
#[must_use]
pub fn format_hourly_sparkline(distribution: &[u64; 24]) -> String {
    sparkline(distribution, 24)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparkline_empty() {
        assert_eq!(sparkline(&[], 10), "");
    }

    #[test]
    fn test_sparkline_single_value() {
        let result = sparkline(&[5], 1);
        assert_eq!(result.chars().count(), 1);
        assert_eq!(result, "█"); // Single value is max
    }

    #[test]
    fn test_sparkline_values() {
        let values = vec![1, 5, 10, 8, 3, 1];
        let result = sparkline(&values, 6);
        assert_eq!(result.chars().count(), 6);
        // The highest value (10) should produce █
        assert!(result.contains('█'));
        // The lowest values (1) should produce ▁
        assert!(result.contains('▁'));
    }

    #[test]
    fn test_sparkline_all_zeros() {
        let values = vec![0, 0, 0, 0];
        let result = sparkline(&values, 4);
        assert_eq!(result, "▁▁▁▁");
    }

    #[test]
    fn test_sparkline_bucketing() {
        // 12 values bucketed into 6
        let values = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let result = sparkline(&values, 6);
        assert_eq!(result.chars().count(), 6);
    }

    #[test]
    fn test_find_longest_gap_empty() {
        let (gap, start, end) = TemporalStats::find_longest_gap(&[]);
        assert_eq!(gap, 0);
        assert!(start.is_none());
        assert!(end.is_none());
    }

    #[test]
    fn test_find_longest_gap_single() {
        let counts = vec![DailyCount {
            date: NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
            count: 5,
        }];
        let (gap, start, end) = TemporalStats::find_longest_gap(&counts);
        assert_eq!(gap, 0);
        assert!(start.is_none());
        assert!(end.is_none());
    }

    #[test]
    fn test_find_longest_gap_normal() {
        let counts = vec![
            DailyCount {
                date: NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
                count: 5,
            },
            DailyCount {
                date: NaiveDate::from_ymd_opt(2023, 1, 5).unwrap(),
                count: 3,
            }, // 4 day gap
            DailyCount {
                date: NaiveDate::from_ymd_opt(2023, 1, 20).unwrap(),
                count: 2,
            }, // 15 day gap (longest)
            DailyCount {
                date: NaiveDate::from_ymd_opt(2023, 1, 22).unwrap(),
                count: 1,
            }, // 2 day gap
        ];
        let (gap, start, end) = TemporalStats::find_longest_gap(&counts);
        assert_eq!(gap, 15);
        assert_eq!(start, Some(NaiveDate::from_ymd_opt(2023, 1, 5).unwrap()));
        assert_eq!(end, Some(NaiveDate::from_ymd_opt(2023, 1, 20).unwrap()));
    }
}
