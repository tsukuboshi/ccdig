//! Times in transcripts, day ranges for --since and --until, and time zones.

use std::time::SystemTime;

use jiff::civil::{Date, DateTime};
use jiff::fmt::strtime;
use jiff::tz::{Offset, TimeZone};
use jiff::{Timestamp, ZonedDateTime};
use serde::{Serialize, Serializer};

use crate::failure::Failure;

/// The last digit of a fraction that a nanosecond count can hold.
const MAX_FRACTION_DIGITS: usize = 9;

/// A whole-day span in a time zone, both days included. A missing bound is
/// open.
#[derive(Clone, Copy, Debug, Default)]
pub struct DayRange {
    /// The first instant of `--since`, included.
    from: Option<Timestamp>,
    /// The first instant of the day after `--until`, excluded.
    to: Option<Timestamp>,
}

/// A transcript time: the instant and the offset it was written with, so
/// JSON output can repeat it as written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stamp {
    /// When it happened.
    instant: Timestamp,
    /// The offset in the transcript, `Z` being zero.
    offset: Offset,
}

impl DayRange {
    /// Whether `at` falls within the range.
    pub fn contains(&self, at: Timestamp) -> bool {
        self.from.is_none_or(|first| at >= first) && self.to.is_none_or(|end| at < end)
    }

    /// Builds the range from `since` to `until`, both `YYYY-MM-DD` days in
    /// `zone`. An empty string leaves that end open.
    pub fn parse(since: &str, until: &str, zone: &TimeZone) -> Result<Self, Failure> {
        let from = if since.is_empty() {
            None
        } else {
            let day = parse_day(since).ok_or_else(|| Failure::Since(not_a_day(since)))?;
            Some(start_of(day, zone).map_err(|err| Failure::Since(err.to_string()))?)
        };
        let to = if until.is_empty() {
            None
        } else {
            let day = parse_day(until).ok_or_else(|| Failure::Until(not_a_day(until)))?;
            // The day after, not 24 hours later: a day is not 24 hours
            // across a DST change.
            let next = day
                .tomorrow()
                .map_err(|err| Failure::Until(err.to_string()))?;
            Some(start_of(next, zone).map_err(|err| Failure::Until(err.to_string()))?)
        };
        if let (Some(first), Some(end)) = (from, to)
            && first >= end
        {
            return Err(Failure::SinceAfterUntil);
        }
        Ok(Self { from, to })
    }

    /// Whether the whole range lies after `at`. A session file last written
    /// at `at` holds nothing inside such a range, so it need not be read.
    pub fn starts_after(&self, at: Timestamp) -> bool {
        self.from.is_some_and(|first| at < first)
    }
}

impl Stamp {
    /// Formats the date in `zone` as `2006-01-02`, for digest headings.
    pub fn date_label(&self, zone: &TimeZone) -> Result<String, Failure> {
        strtime::format("%Y-%m-%d", &self.in_zone(zone)).map_err(Failure::TimeFormat)
    }

    /// Converts a file's modification time, in UTC.
    pub fn from_system_time(time: SystemTime) -> Result<Self, Failure> {
        let instant = Timestamp::try_from(time).map_err(Failure::TimeFormat)?;
        Ok(Self {
            instant,
            offset: Offset::UTC,
        })
    }

    /// The time as seen in `zone`.
    fn in_zone(&self, zone: &TimeZone) -> ZonedDateTime {
        self.instant.to_zoned_datetime(zone.clone())
    }

    /// When it happened.
    pub const fn instant(&self) -> Timestamp {
        self.instant
    }

    /// Formats the time in `zone` as `2006-01-02 15:04 MST`, for Markdown.
    pub fn minute_label(&self, zone: &TimeZone) -> Result<String, Failure> {
        strtime::format("%Y-%m-%d %H:%M %Z", &self.in_zone(zone)).map_err(Failure::TimeFormat)
    }

    /// Parses the strict RFC 3339 form: `YYYY-MM-DDTHH:MM:SS`, an optional
    /// `.` and digits, then `Z` or `±HH:MM`. Looser forms (a one-digit hour,
    /// a comma before the fraction) are rejected; Claude Code writes
    /// `toISOString()`, which never needs them.
    pub fn parse_rfc3339(text: &str) -> Option<Self> {
        let &[
            y1,
            y2,
            y3,
            y4,
            b'-',
            mo1,
            mo2,
            b'-',
            d1,
            d2,
            b'T',
            h1,
            h2,
            b':',
            mi1,
            mi2,
            b':',
            s1,
            s2,
            ref rest @ ..,
        ] = text.as_bytes()
        else {
            return None;
        };
        let (nanosecond, zone) = match *rest {
            [b'.', ref tail @ ..] => {
                let digits = tail.iter().take_while(|byte| byte.is_ascii_digit()).count();
                if digits == 0 {
                    return None;
                }
                let (fraction, zone) = tail.split_at_checked(digits)?;
                (nanoseconds(fraction)?, zone)
            }
            _ => (0_i32, rest),
        };
        let offset = Offset::from_seconds(offset_seconds(zone)?).ok()?;
        let datetime = DateTime::new(
            i16::try_from(number(&[y1, y2, y3, y4])?).ok()?,
            i8::try_from(number(&[mo1, mo2])?).ok()?,
            i8::try_from(number(&[d1, d2])?).ok()?,
            i8::try_from(number(&[h1, h2])?).ok()?,
            i8::try_from(number(&[mi1, mi2])?).ok()?,
            i8::try_from(number(&[s1, s2])?).ok()?,
            nanosecond,
        )
        .ok()?;
        let instant = offset.to_timestamp(datetime).ok()?;
        Some(Self { instant, offset })
    }

    /// Formats the time as RFC 3339: the offset as written,
    /// `Z` for zero, and the fraction with its trailing zeros dropped.
    pub fn rfc3339(&self) -> String {
        if self.offset == Offset::UTC {
            return self.instant.to_string();
        }
        self.instant.display_with_offset(self.offset).to_string()
    }
}

/// Written as RFC 3339 with the offset it was read with.
impl Serialize for Stamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.rfc3339())
    }
}

/// Reads up to nine digits of a fraction as nanoseconds, dropping the rest
/// as Go did.
fn nanoseconds(fraction: &[u8]) -> Option<i32> {
    let mut nanosecond = 0_i32;
    let mut scale = 100_000_000_i32;
    for &byte in fraction.iter().take(MAX_FRACTION_DIGITS) {
        let digit = i32::try_from(char::from(byte).to_digit(10)?).ok()?;
        nanosecond = nanosecond.checked_add(digit.checked_mul(scale)?)?;
        scale = scale.checked_div(10)?;
    }
    Some(nanosecond)
}

/// The message for a `--since` or `--until` that is not a calendar day.
fn not_a_day(text: &str) -> String {
    format!("{text:?} is not a YYYY-MM-DD date")
}

/// Reads ASCII digits as a number; `None` if any byte is not a digit.
fn number(digits: &[u8]) -> Option<i32> {
    digits.iter().try_fold(0_i32, |value, &byte| {
        let digit = i32::try_from(char::from(byte).to_digit(10)?).ok()?;
        value.checked_mul(10)?.checked_add(digit)
    })
}

/// Reads `Z` or `±HH:MM`, with the hour below 24 and the minute below 60, as
/// seconds east of UTC.
fn offset_seconds(zone: &[u8]) -> Option<i32> {
    match *zone {
        [b'Z'] => Some(0),
        [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2] => {
            let hours = number(&[h1, h2])?;
            let minutes = number(&[m1, m2])?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let seconds = hours
                .checked_mul(60)?
                .checked_add(minutes)?
                .checked_mul(60)?;
            if sign == b'-' {
                return seconds.checked_neg();
            }
            Some(seconds)
        }
        _ => None,
    }
}

/// Reads `YYYY-MM-DD` with four-digit years and zero-padded months and days,
/// rejecting days that do not exist.
fn parse_day(text: &str) -> Option<Date> {
    let &[y1, y2, y3, y4, b'-', m1, m2, b'-', d1, d2] = text.as_bytes() else {
        return None;
    };
    Date::new(
        i16::try_from(number(&[y1, y2, y3, y4])?).ok()?,
        i8::try_from(number(&[m1, m2])?).ok()?,
        i8::try_from(number(&[d1, d2])?).ok()?,
    )
    .ok()
}

/// The first instant of `day` in `zone`. When midnight falls in a DST gap,
/// this is the first instant after the gap.
fn start_of(day: Date, zone: &TimeZone) -> Result<Timestamp, jiff::Error> {
    Ok(day.to_zoned_datetime(zone.clone())?.timestamp())
}

#[cfg(test)]
mod tests;
