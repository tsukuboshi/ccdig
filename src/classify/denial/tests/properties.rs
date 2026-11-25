//! Properties of reading why a tool call did not run.

use proptest::arbitrary::any;
use proptest::strategy::Strategy as _;

use crate::classify::denial::{
    Answer, CLARIFY_PREFIX, Denial, REJECTED_PREFIX, SAID, USER_REJECTED, denial,
};
use crate::testing::{TestResult, check_eq, for_all, hostile_text};

#[test]
fn a_result_without_a_kind_or_the_rejection_text_ran() -> TestResult {
    let texts = hostile_text().prop_filter("not the rejection text", |text| {
        !text.starts_with(REJECTED_PREFIX)
    });
    for_all(
        &(texts, hostile_text(), any::<bool>()),
        |(text, feedback, is_error)| {
            let answer = Answer {
                denial_kind: "",
                feedback: &feedback,
                is_error,
                text: &text,
            };
            check_eq(&denial(answer), &Denial::default(), &text)?;
            Ok(())
        },
    )
}

#[test]
fn whatever_follows_the_said_text_is_the_comment() -> TestResult {
    let comments = hostile_text().prop_filter("a comment", |comment| {
        let trimmed = comment.trim();
        !trimmed.is_empty() && !trimmed.starts_with(CLARIFY_PREFIX)
    });
    for_all(&comments, |comment| {
        let text = format!("{REJECTED_PREFIX} To tell you how to proceed, {SAID}{comment}");
        let answer = Answer {
            denial_kind: "",
            feedback: "",
            is_error: true,
            text: &text,
        };
        check_eq(
            &denial(answer),
            &Denial {
                comment: Some(comment.trim().to_owned()),
                kind: USER_REJECTED.to_owned(),
            },
            &text,
        )?;
        Ok(())
    })
}

#[test]
fn only_the_user_turning_a_call_down_leaves_a_comment() -> TestResult {
    let kinds = "[a-z-]{1,20}".prop_filter("not user-rejected", |kind| kind != USER_REJECTED);
    for_all(
        &(kinds, hostile_text(), hostile_text()),
        |(kind, feedback, text)| {
            let answer = Answer {
                denial_kind: &kind,
                feedback: &feedback,
                is_error: true,
                text: &text,
            };
            check_eq(
                &denial(answer),
                &Denial {
                    comment: None,
                    kind: kind.clone(),
                },
                &kind,
            )?;
            Ok(())
        },
    )
}

#[test]
fn a_comment_is_never_blank_nor_the_clarify_text() -> TestResult {
    for_all(&(hostile_text(), hostile_text()), |(feedback, text)| {
        let answer = Answer {
            denial_kind: USER_REJECTED,
            feedback: &feedback,
            is_error: true,
            text: &text,
        };
        if let Some(comment) = denial(answer).comment {
            check_eq(&comment.trim().is_empty(), &false, "blank")?;
            check_eq(&comment.starts_with(CLARIFY_PREFIX), &false, "clarify")?;
        }
        Ok(())
    })
}
