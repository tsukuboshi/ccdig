//! Tests for resolving time zones.

use super::{Environment, local_zone};
use crate::testing::{TestResult, check_eq, label_in};

#[test]
fn local_zone_follows_go_for_the_tz_variable() -> TestResult {
    for (tz, want) in [
        ("", "2026-10-13 01:00 UTC"),
        (":", "2026-10-13 01:00 UTC"),
        ("UTC", "2026-10-13 01:00 UTC"),
        ("Asia/Tokyo", "2026-10-13 10:00 JST"),
        (":Asia/Tokyo", "2026-10-13 10:00 JST"),
        ("Mars/Olympus", "2026-10-13 01:00 UTC"),
        ("/nonexistent/zoneinfo", "2026-10-13 01:00 UTC"),
    ] {
        check_eq(label_in(&local_zone(Some(tz)))?.as_str(), want, tz)?;
    }
    Ok(())
}

#[test]
fn zone_reads_iana_names_and_treats_empty_as_utc() -> TestResult {
    let environment = Environment::default();
    for (name, want) in [
        ("Asia/Tokyo", "2026-10-13 10:00 JST"),
        ("", "2026-10-13 01:00 UTC"),
        ("UTC", "2026-10-13 01:00 UTC"),
    ] {
        check_eq(label_in(&environment.zone(name)?)?.as_str(), want, name)?;
    }
    Ok(())
}

#[test]
fn zone_names_an_unknown_zone() -> TestResult {
    let Err(failure) = Environment::default().zone("Mars/Olympus") else {
        return Err("Mars/Olympus loaded".into());
    };
    check_eq(
        failure.to_string().as_str(),
        "load time zone: unknown time zone Mars/Olympus",
        "message",
    )?;
    Ok(())
}
