//! Tests for the system zone.

use core::error::Error;

use jiff::tz::TimeZone;

use super::local_zone;
use crate::testing::{TestResult, check_eq};
use crate::when::Stamp;

/// A fixed instant's minute label in `zone`.
fn label_in(zone: &TimeZone) -> Result<String, Box<dyn Error>> {
    let stamp = Stamp::parse_rfc3339("2026-10-13T01:00:00Z").ok_or("bad fixture")?;
    Ok(stamp.minute_label(zone)?)
}

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
