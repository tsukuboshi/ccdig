//! Tests for the comparison helper.

use super::check_eq;
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
