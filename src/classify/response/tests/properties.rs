//! Properties of reading what the user answered through a dialog.

use proptest::collection::vec;
use proptest::sample::subsequence;
use proptest::strategy::{Just, Strategy as _};

use crate::classify::response::picked;
use crate::testing::{TestResult, check_eq, for_all};

#[test]
fn the_options_an_answer_lists_are_the_ones_picked_and_the_rest_was_typed() -> TestResult {
    // Labels that do not start with one another, and hold no separator,
    // so that each is read back one way only; typed words never start
    // with one, as they hold no period.
    let offered = vec("[a-z]{1,8}", 1..=6).prop_map(|mut words| {
        words.sort();
        words.dedup();
        words
            .iter()
            .map(|word| format!("{word}."))
            .collect::<Vec<_>>()
    });
    let cases = (offered, "([a-z]([a-z ]{0,6}[a-z])?)?").prop_flat_map(|(labels, typed)| {
        let count = labels.len();
        (
            subsequence(labels.clone(), 0..=count),
            Just((labels, typed)),
        )
    });
    for_all(&cases, |(chosen, (labels, typed))| {
        let parts: Vec<&str> = chosen
            .iter()
            .map(String::as_str)
            .chain((!typed.is_empty()).then_some(typed.as_str()))
            .collect();
        let answer = parts.join(", ");
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let want: Vec<bool> = labels.iter().map(|label| chosen.contains(label)).collect();
        check_eq(&picked(&answer, &refs, true), &(want, typed), &answer)?;
        Ok(())
    })
}
