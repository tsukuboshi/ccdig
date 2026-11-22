//! Tests for day ranges, transcript timestamps, and time zones.

use core::error::Error;
use core::time::Duration;
use std::time::SystemTime;

use jiff::Timestamp;
use jiff::tz::{TimeZone, TimeZoneDatabase};

use super::{DayRange, Stamp, load_zone};
use crate::testing::{TestResult, check_eq};

/// The zone the tests use, so day boundaries fall mid-UTC-day.
fn tokyo() -> Result<TimeZone, jiff::Error> {
    TimeZone::get("Asia/Tokyo")
}

/// Parses an RFC 3339 instant for the test inputs.
fn at(text: &str) -> Result<Timestamp, jiff::Error> {
    text.parse()
}

/// Formats a fixed instant in `zone`, to tell zones apart.
fn label_in(zone: &TimeZone) -> Result<String, Box<dyn Error>> {
    let stamp = Stamp::parse_rfc3339("2026-10-13T01:00:00Z").ok_or("bad fixture")?;
    Ok(stamp.minute_label(zone)?)
}

#[test]
fn contains_uses_local_day_boundaries() -> TestResult {
    let range = DayRange::parse("2026-10-05", "2026-10-11", &tokyo()?)?;
    for (instant, want) in [
        ("2026-10-04T14:59:59.999Z", false),
        ("2026-10-04T15:00:00Z", true),
        ("2026-10-11T14:59:59.999Z", true),
        ("2026-10-11T15:00:00Z", false),
    ] {
        check_eq(&range.contains(at(instant)?), &want, instant)?;
    }
    Ok(())
}

#[test]
fn an_empty_bound_leaves_that_end_open() -> TestResult {
    let zone = tokyo()?;
    let early = at("2000-01-01T00:00:00Z")?;
    let late = at("2099-12-31T00:00:00Z")?;
    let inside = at("2026-10-08T03:00:00Z")?;
    for (name, since, until, want) in [
        ("no bounds", "", "", [true, true, true]),
        ("since only", "2026-10-05", "", [false, true, true]),
        ("until only", "", "2026-10-11", [true, false, true]),
    ] {
        let range = DayRange::parse(since, until, &zone)?;
        let got = [
            range.contains(early),
            range.contains(late),
            range.contains(inside),
        ];
        check_eq(&got, &want, name)?;
    }
    Ok(())
}

#[test]
fn parse_rejects_anything_but_a_real_yyyy_mm_dd_day_in_order() -> TestResult {
    let zone = tokyo()?;
    for (name, since, until, message) in [
        (
            "slashes",
            "2026/10/05",
            "",
            "parse since: \"2026/10/05\" is not a YYYY-MM-DD date",
        ),
        (
            "not a date",
            "",
            "last week",
            "parse until: \"last week\" is not a YYYY-MM-DD date",
        ),
        (
            "impossible day",
            "2026-02-30",
            "",
            "parse since: \"2026-02-30\" is not a YYYY-MM-DD date",
        ),
        (
            "no zero padding",
            "2026-1-05",
            "",
            "parse since: \"2026-1-05\" is not a YYYY-MM-DD date",
        ),
        (
            "trailing text",
            "",
            "2026-10-05x",
            "parse until: \"2026-10-05x\" is not a YYYY-MM-DD date",
        ),
        (
            "since after until",
            "2026-10-12",
            "2026-10-11",
            "since is after until",
        ),
    ] {
        let Err(failure) = DayRange::parse(since, until, &zone) else {
            return Err(format!("{name}: parsed").into());
        };
        check_eq(failure.to_string().as_str(), message, name)?;
    }
    Ok(())
}

#[test]
fn the_same_day_twice_is_a_one_day_range() -> TestResult {
    let range = DayRange::parse("2026-10-05", "2026-10-05", &tokyo()?)?;
    check_eq(&range.contains(at("2026-10-05T03:00:00Z")?), &true, "noon")?;
    Ok(())
}

#[test]
fn starts_after_is_true_only_before_a_closed_start() -> TestResult {
    let zone = tokyo()?;
    let closed = DayRange::parse("2026-10-05", "", &zone)?;
    let open = DayRange::parse("", "2026-10-11", &zone)?;
    for (name, range, instant, want) in [
        (
            "just before the first day",
            &closed,
            "2026-10-04T14:59:59Z",
            true,
        ),
        (
            "at the first instant",
            &closed,
            "2026-10-04T15:00:00Z",
            false,
        ),
        ("open start", &open, "2000-01-01T00:00:00Z", false),
    ] {
        check_eq(&range.starts_after(at(instant)?), &want, name)?;
    }
    Ok(())
}

#[test]
fn parse_rfc3339_accepts_the_strict_form_only() -> TestResult {
    for (text, accepted) in [
        ("2026-10-13T01:02:03Z", true),
        ("2026-10-13T01:02:03.456Z", true),
        ("2026-10-13T10:02:03+09:00", true),
        ("2026-10-13T01:02:03-03:30", true),
        ("2026-10-13 01:02:03Z", false),
        ("2026-10-13t01:02:03Z", false),
        ("2026-10-13T01:02Z", false),
        ("2026-10-13T01:02:03", false),
        ("2026-10-13T01:02:03.Z", false),
        ("2026-02-30T00:00:00Z", false),
        ("2026-10-13T24:00:00Z", false),
        ("2026-10-13T00:00:60Z", false),
        ("2026-10-13T00:00:00+24:00", false),
        ("2026-10-13T00:00:00+0900", false),
        ("2026-10-13T00:00:00Z[UTC]", false),
    ] {
        check_eq(&Stamp::parse_rfc3339(text).is_some(), &accepted, text)?;
    }
    Ok(())
}

#[test]
fn rfc3339_keeps_the_offset_and_trims_the_fraction_like_go() -> TestResult {
    for (input, want) in [
        ("2026-10-13T01:02:03.456Z", "2026-10-13T01:02:03.456Z"),
        ("2026-10-13T10:02:03+09:00", "2026-10-13T10:02:03+09:00"),
        ("2026-10-13T01:02:03.100Z", "2026-10-13T01:02:03.1Z"),
        ("2026-10-13T01:02:03.000Z", "2026-10-13T01:02:03Z"),
        ("2026-10-13T01:02:03+00:00", "2026-10-13T01:02:03Z"),
        (
            "2026-10-13T01:02:03.1234567891Z",
            "2026-10-13T01:02:03.123456789Z",
        ),
    ] {
        let stamp = Stamp::parse_rfc3339(input).ok_or(input)?;
        check_eq(stamp.rfc3339().as_str(), want, input)?;
    }
    Ok(())
}

#[test]
fn an_offset_and_utc_spelling_of_one_instant_compare_by_instant() -> TestResult {
    let tokyo_spelling = Stamp::parse_rfc3339("2026-10-13T10:00:00+09:00").ok_or("tokyo")?;
    let utc_spelling = Stamp::parse_rfc3339("2026-10-13T01:00:00Z").ok_or("utc")?;
    check_eq(
        &tokyo_spelling.instant(),
        &utc_spelling.instant(),
        "instant",
    )?;
    Ok(())
}

#[test]
fn minute_label_shows_the_zone_abbreviation() -> TestResult {
    let stamp = Stamp::parse_rfc3339("2026-10-13T01:00:00Z").ok_or("fixture")?;
    for (zone, want) in [
        ("Asia/Tokyo", "2026-10-13 10:00 JST"),
        ("UTC", "2026-10-13 01:00 UTC"),
        ("America/Sao_Paulo", "2026-10-12 22:00 -03"),
    ] {
        check_eq(stamp.minute_label(&load_zone(zone)?)?.as_str(), want, zone)?;
    }
    Ok(())
}

#[test]
fn date_label_is_the_local_day() -> TestResult {
    let stamp = Stamp::parse_rfc3339("2026-10-12T15:00:00Z").ok_or("fixture")?;
    check_eq(stamp.date_label(&tokyo()?)?.as_str(), "2026-10-13", "Tokyo")?;
    check_eq(
        stamp.date_label(&TimeZone::UTC)?.as_str(),
        "2026-10-12",
        "UTC",
    )?;
    Ok(())
}

#[test]
fn from_system_time_is_utc_with_nanoseconds() -> TestResult {
    let time = SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_millis(1_500))
        .ok_or("overflow")?;
    check_eq(
        Stamp::from_system_time(time)?.rfc3339().as_str(),
        "1970-01-01T00:00:01.5Z",
        "epoch plus 1.5s",
    )?;
    Ok(())
}

#[test]
fn load_zone_reads_iana_names_and_treats_empty_as_utc() -> TestResult {
    check_eq(
        label_in(&load_zone("Asia/Tokyo")?)?.as_str(),
        "2026-10-13 10:00 JST",
        "Tokyo",
    )?;
    check_eq(
        label_in(&load_zone("")?)?.as_str(),
        "2026-10-13 01:00 UTC",
        "empty",
    )?;
    check_eq(
        label_in(&load_zone("UTC")?)?.as_str(),
        "2026-10-13 01:00 UTC",
        "UTC",
    )?;
    Ok(())
}

#[test]
fn load_zone_names_an_unknown_zone() -> TestResult {
    let Err(failure) = load_zone("Mars/Olympus") else {
        return Err("Mars/Olympus loaded".into());
    };
    check_eq(
        failure.to_string().as_str(),
        "load time zone: unknown time zone Mars/Olympus",
        "message",
    )?;
    Ok(())
}

#[test]
fn the_time_zone_database_is_bundled_into_the_binary() -> TestResult {
    let zone = TimeZoneDatabase::bundled().get("Asia/Tokyo")?;
    check_eq(
        label_in(&zone)?.as_str(),
        "2026-10-13 10:00 JST",
        "bundled Tokyo",
    )?;
    Ok(())
}
