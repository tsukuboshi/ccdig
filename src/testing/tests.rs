//! Tests for the comparison and property helpers.

use core::cell::Cell;

use proptest::arbitrary::any;

use super::{check_eq, for_all, for_all_cases};
use crate::testing::TestResult;

#[test]
fn equal_values_pass() -> TestResult {
    check_eq("same", "same", "equal strings")?;
    Ok(())
}

#[test]
fn a_mismatch_shows_both_values_once_quoted() -> TestResult {
    let Err(mismatch) = check_eq("got", "want", "strings") else {
        return Err("different strings compared equal".into());
    };
    check_eq(
        format!("{mismatch:?}").as_str(),
        "strings\n  got:  \"got\"\n  want: \"want\"",
        "failure message",
    )?;
    Ok(())
}

#[test]
fn a_property_that_holds_passes() -> TestResult {
    for_all(&(0_u32..1000), |value| {
        check_eq(&(value < 1000), &true, "in range")?;
        Ok(())
    })
}

#[test]
fn a_broken_property_reports_the_smallest_failing_input() -> TestResult {
    let Err(error) = for_all(&(0_u32..1000), |value| {
        check_eq(&(value < 500), &true, "below 500")?;
        Ok(())
    }) else {
        return Err("a broken property passed".into());
    };
    let message = error.to_string();
    check_eq(&message.contains("input: 500"), &true, &message)?;
    Ok(())
}

#[test]
fn a_failure_names_the_seed_that_replays_it() -> TestResult {
    let Err(error) = for_all(&(0_u32..1000), |value| {
        check_eq(&(value < 500), &true, "below 500")?;
        Ok(())
    }) else {
        return Err("a broken property passed".into());
    };
    let message = error.to_string();
    let seed = message
        .split_once("PROPTEST_RNG_SEED=")
        .map(|(_, rest)| rest.trim());
    check_eq(
        &seed.is_some_and(|digits| digits.parse::<u64>().is_ok()),
        &true,
        &message,
    )?;
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "the test checks that a panic inside a property is caught"
)]
fn a_panic_is_reported_as_a_failure() -> TestResult {
    let result = for_all(&(0_u32..10), |value| {
        assert!(value <= 3, "too large");
        Ok(())
    });
    check_eq(&result.is_err(), &true, "failing property")?;
    Ok(())
}

#[test]
fn for_all_cases_runs_the_property_that_many_times() -> TestResult {
    let calls = Cell::new(0_u32);
    for_all_cases(5, &any::<u8>(), |_| {
        calls.set(calls.get().saturating_add(1));
        Ok(())
    })?;
    check_eq(&calls.get(), &5, "calls")?;
    Ok(())
}
