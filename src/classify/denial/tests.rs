//! Tests for reading why a tool call did not run.

mod properties;

use super::{Answer, Denial, denial};
use crate::testing::{TestResult, check_eq};

/// The text Claude Code returns when the user turns a call down without a
/// comment.
const REJECTED: &str = "The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed.";

/// The text before the comment when the user turns a call down with one.
const SAID: &str = "The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:\n";

/// The text Claude Code puts in the comment when the user picks "clarify"
/// in a question dialog.
const CLARIFY: &str = "The user wants to clarify these questions.\n    This means they may have additional information, context or questions for you.";

/// A denial of `kind` with `comment`.
fn denied(kind: &str, comment: Option<&str>) -> Denial {
    Denial {
        comment: comment.map(str::to_owned),
        kind: kind.to_owned(),
    }
}

/// An error result with `text` and no line keys.
const fn error(text: &str) -> Answer<'_> {
    Answer {
        denial_kind: "",
        feedback: "",
        is_error: true,
        text,
    }
}

#[test]
fn denial_reads_the_kind_and_the_comment() -> TestResult {
    let with_comment = format!("{SAID}One PR, please.");
    let clarified = format!("{SAID}{CLARIFY}");
    let padded = format!("{SAID}  \n  ");
    for (name, answer, want) in [
        (
            "the recorded kind and feedback",
            Answer {
                denial_kind: "user-rejected",
                feedback: "One PR, please.",
                is_error: true,
                text: &with_comment,
            },
            denied("user-rejected", Some("One PR, please.")),
        ),
        (
            "a recorded kind without the feedback key takes the comment from the text",
            Answer {
                denial_kind: "user-rejected",
                ..error(&with_comment)
            },
            denied("user-rejected", Some("One PR, please.")),
        ),
        (
            "before Claude Code recorded the kind, the text tells",
            error(&with_comment),
            denied("user-rejected", Some("One PR, please.")),
        ),
        (
            "a rejection without a comment",
            error(REJECTED),
            denied("user-rejected", None),
        ),
        (
            "a blank comment is none",
            error(&padded),
            denied("user-rejected", None),
        ),
        (
            "the clarify text of a question dialog is not a comment",
            Answer {
                denial_kind: "user-rejected",
                feedback: CLARIFY,
                is_error: true,
                text: &clarified,
            },
            denied("user-rejected", None),
        ),
        (
            "a permission rule has no comment",
            Answer {
                denial_kind: "permission-rule",
                ..error("Permission to use Bash has been denied.")
            },
            denied("permission-rule", None),
        ),
        (
            "an ordinary error",
            error("Error: Exit code 1"),
            Denial::default(),
        ),
        (
            "the rejection text is not a rejection when the tool succeeded",
            Answer {
                is_error: false,
                ..error(REJECTED)
            },
            Denial::default(),
        ),
    ] {
        check_eq(&denial(answer), &want, name)?;
    }
    Ok(())
}
