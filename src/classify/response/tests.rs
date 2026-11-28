//! Tests for reading what the user answered through a dialog.

mod properties;

use super::picked;
use crate::testing::{TestResult, check_eq};

/// A case of `picked`: its name, the answer Claude Code recorded, the
/// labels offered, whether several could be picked, which of them the
/// user picked, and what they typed.
type Case = (
    &'static str,
    &'static str,
    &'static [&'static str],
    bool,
    &'static [bool],
    &'static str,
);

#[test]
fn picked_says_which_options_the_answer_names_and_what_was_typed() -> TestResult {
    let cases: [Case; 9] = [
        (
            "one option",
            "main",
            &["main", "develop"],
            false,
            &[true, false],
            "",
        ),
        (
            "an answer typed in the user's own words",
            "ccdig, of course",
            &["ccdig-cli", "dig"],
            false,
            &[false, false],
            "ccdig, of course",
        ),
        (
            "a typed answer that starts with a label and the separator",
            "main, please",
            &["main", "develop"],
            false,
            &[false, false],
            "main, please",
        ),
        (
            "a typed answer that is a label and more",
            "main branch",
            &["main"],
            false,
            &[false],
            "main branch",
        ),
        (
            "several options, out of order",
            "tests, docs",
            &["docs", "code", "tests"],
            true,
            &[true, false, true],
            "",
        ),
        (
            "a label that holds the separator",
            "a, b",
            &["a", "a, b", "b"],
            true,
            &[false, true, false],
            "",
        ),
        (
            "several options and words of the user's own",
            "docs, and the changelog",
            &["docs", "code"],
            true,
            &[true, false],
            "and the changelog",
        ),
        ("an empty answer", "", &["main"], false, &[false], ""),
        ("no options", "main", &[], false, &[], "main"),
    ];
    for (name, answer, labels, several, want, typed) in cases {
        check_eq(
            &picked(answer, labels, several),
            &(want.to_vec(), typed.to_owned()),
            name,
        )?;
    }
    Ok(())
}
