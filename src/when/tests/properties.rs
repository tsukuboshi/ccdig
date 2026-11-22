//! Properties of transcript times and day ranges that hold for any input.

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use proptest::option;
use proptest::sample::select;
use proptest::strategy::Strategy;

use crate::testing::{TestResult, check_eq, for_all, hostile_text, rfc3339_text, zone};
use crate::when::{DayRange, Stamp};

/// Zones with and without DST, with half-hour and 45-minute offsets, and
/// with midnight inside a DST gap (Santiago) or a 30-minute shift (Lord Howe).
const ZONES: &[&str] = &[
    "UTC",
    "Asia/Tokyo",
    "America/New_York",
    "America/Santiago",
    "Australia/Lord_Howe",
    "Asia/Kathmandu",
    "Pacific/Apia",
];

/// Text shaped like an RFC 3339 time, with any digits in each field, an
/// optional fraction of any length, and any offset or none.
const NEAR_RFC3339: &str = r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]{0,12})?(Z|[+-][0-9]{2}:[0-9]{2}|.)?";

/// A day from 1990 to 2040, as year, month, and day.
fn day() -> impl Strategy<Value = Date> {
    (1990_i16..=2040, 1_i8..=12, 1_i8..=28).prop_filter_map("a real day", |(year, month, day)| {
        Date::new(year, month, day).ok()
    })
}

/// The day `at` falls on in `zone`.
fn local_day(at: Timestamp, zone: &TimeZone) -> Date {
    at.to_zoned_datetime(zone.clone()).date()
}

#[test]
fn parse_rfc3339_reads_the_same_instant_as_jiff_and_reads_back_its_output() -> TestResult {
    for_all(&rfc3339_text(), |text| {
        let stamp = Stamp::parse_rfc3339(&text).ok_or("rejected")?;
        let want: Timestamp = text.parse()?;
        check_eq(&stamp.instant(), &want, "instant")?;
        check_eq(
            &Stamp::parse_rfc3339(&stamp.rfc3339()),
            &Some(stamp),
            "read back",
        )?;
        Ok(())
    })
}

#[test]
fn parse_rfc3339_never_panics_and_what_it_accepts_reads_back() -> TestResult {
    let texts = proptest::prop_oneof![NEAR_RFC3339, hostile_text()];
    for_all(&texts, |text| {
        if let Some(stamp) = Stamp::parse_rfc3339(&text) {
            check_eq(
                &Stamp::parse_rfc3339(&stamp.rfc3339()),
                &Some(stamp),
                "read back",
            )?;
        }
        Ok(())
    })
}

#[test]
fn a_day_range_holds_exactly_the_instants_whose_local_day_is_inside() -> TestResult {
    let cases = (
        option::of(day()),
        option::of(day()),
        select(ZONES),
        -3_i64..=3,
        631_152_000_i64..2_240_611_200,
    );
    for_all(&cases, |(since, until, name, nudge, second)| {
        let zone = zone(name)?;
        let text = |bound: Option<Date>| bound.map(|day| day.to_string()).unwrap_or_default();
        let parsed = DayRange::parse(&text(since), &text(until), &zone);
        let in_order = since.zip(until).is_none_or(|(first, last)| first <= last);
        check_eq(&parsed.is_ok(), &in_order, "accepted")?;
        let Ok(range) = parsed else {
            return Ok(());
        };
        // Instants a few seconds either side of each boundary, and one anywhere.
        let mut instants = vec![Timestamp::from_second(second)?];
        for bound in [since, until.map(Date::tomorrow).transpose()?]
            .into_iter()
            .flatten()
        {
            let midnight = bound.to_zoned_datetime(zone.clone())?.timestamp();
            let near = midnight.as_second().checked_add(nudge).ok_or("overflow")?;
            instants.push(Timestamp::from_second(near)?);
        }
        for at in instants {
            let local = local_day(at, &zone);
            let inside =
                since.is_none_or(|first| first <= local) && until.is_none_or(|last| local <= last);
            check_eq(&range.contains(at), &inside, &format!("contains {at}"))?;
            let before = since.is_some_and(|first| local < first);
            check_eq(
                &range.starts_after(at),
                &before,
                &format!("starts after {at}"),
            )?;
        }
        Ok(())
    })
}

#[test]
fn day_range_parse_never_panics() -> TestResult {
    let texts = proptest::prop_oneof!["[0-9]{4}-[0-9]{2}-[0-9]{2}", hostile_text()];
    for_all(
        &(texts.clone(), texts, select(ZONES)),
        |(since, until, name)| {
            drop(DayRange::parse(&since, &until, &zone(name)?));
            Ok(())
        },
    )
}

#[test]
fn every_time_has_a_minute_label_that_starts_with_its_date_label() -> TestResult {
    for_all(&(rfc3339_text(), select(ZONES)), |(text, name)| {
        let zone = zone(name)?;
        let stamp = Stamp::parse_rfc3339(&text).ok_or("rejected")?;
        let date = stamp.date_label(&zone)?;
        let minute = stamp.minute_label(&zone)?;
        check_eq(&minute.starts_with(&format!("{date} ")), &true, &minute)?;
        Ok(())
    })
}
