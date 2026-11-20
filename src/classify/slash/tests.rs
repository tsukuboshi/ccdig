//! Tests for folding slash command expansions.

use super::{Folded, fold};
use crate::testing::{TestResult, check_eq};

/// The fold that `text` and `command` describe.
fn folded(text: &str, command: &str) -> Folded {
    Folded {
        command: command.to_owned(),
        text: text.to_owned(),
    }
}

#[test]
fn fold_turns_an_expansion_back_into_what_was_typed() -> TestResult {
    for (name, body, want) in [
        (
            "message, name, args",
            "<command-message>weekly-report</command-message>\n<command-name>/weekly-report</command-name>\n<command-args>last week</command-args>",
            Some(folded("/weekly-report last week", "/weekly-report")),
        ),
        (
            "name before message keeps the args",
            "<command-name>/tdd</command-name>\n<command-message>tdd</command-message>\n<command-args>step 1</command-args>",
            Some(folded("/tdd step 1", "/tdd")),
        ),
        (
            "no args",
            "<command-message>clear</command-message>\n<command-name>/clear</command-name>",
            Some(folded("/clear", "/clear")),
        ),
        (
            "args with newlines and angle brackets",
            "<command-name>/open-issue</command-name>\n<command-args>  title: a < b\nbody line  </command-args>",
            Some(folded("/open-issue title: a < b\nbody line", "/open-issue")),
        ),
        ("typed prompt", "draft the weekly report", None),
        (
            "typed prompt that mentions the tag",
            "what does <command-name>/x</command-name> mean in the log?",
            None,
        ),
        (
            "an opening tag without its closing tag",
            "<command-name>/x and no end",
            None,
        ),
        (
            "blank args are dropped",
            "<command-name>/clear</command-name><command-args>  \n </command-args>",
            Some(folded("/clear", "/clear")),
        ),
    ] {
        check_eq(&fold(body), &want, name)?;
    }
    Ok(())
}
