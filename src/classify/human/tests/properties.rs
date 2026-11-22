//! Properties of telling typed prompts from harness traffic.

use proptest::bool::weighted;
use proptest::option;
use proptest::prop_oneof;
use proptest::sample::select;
use proptest::strategy::{Just, Strategy};
use serde_json::{Value, json};

use crate::classify::human::{Reason, classify};
use crate::testing::{TestResult, check_eq, for_all, hostile_text};
use crate::transcript::entry::{Entry, decode_line};

/// The tags the harness writes at the start of a body.
const TAGS: &[&str] = &[
    "<task-notification>",
    "<system-reminder>",
    "<local-command-stdout>",
    "<bash-input>",
    "<bash-stdout>",
    "<bash-stderr>",
    "<user-prompt-submit-hook>",
];

/// A user line with `body` and the flags, origin, and tool result given.
fn user_line(
    body: &str,
    flags: (bool, bool, bool),
    origin: Option<&str>,
    tool_result: bool,
) -> Value {
    let content = if tool_result {
        json!([{"type": "text", "text": body}, {"type": "tool_result", "tool_use_id": "t"}])
    } else {
        Value::String(body.to_owned())
    };
    let mut line = json!({
        "type": "user",
        "isMeta": flags.0,
        "isSidechain": flags.1,
        "isCompactSummary": flags.2,
        "message": {"role": "user", "content": content},
    });
    if let (Some(kind), Some(fields)) = (origin, line.as_object_mut()) {
        fields.insert("origin".to_owned(), json!({"kind": kind}));
    }
    line
}

/// Decodes a line built by the test.
fn entry(line: &Value) -> Result<Entry, String> {
    decode_line(line.to_string().as_bytes()).ok_or_else(|| format!("undecodable: {line}"))
}

/// Any user line: flags, an origin, a tool result, and a body. Each flag
/// and the tool result are set one time in ten and the origin is mostly
/// human, so that typed prompts come up often.
fn any_user_line() -> impl Strategy<Value = (String, (bool, bool, bool), Option<String>, bool)> {
    let origins = option::of(prop_oneof![
        3 => Just("human".to_owned()),
        1 => select(&["task-notification", "peer"][..]).prop_map(str::to_owned),
        1 => hostile_text(),
    ]);
    let flag = || weighted(0.1);
    (hostile_text(), (flag(), flag(), flag()), origins, flag())
}

#[test]
fn a_line_that_is_not_a_user_message_is_never_a_prompt() -> TestResult {
    let kinds = prop_oneof![
        select(&["assistant", "ai-title", "summary"][..]).prop_map(str::to_owned),
        hostile_text()
    ]
    .prop_filter("not user", |kind| kind != "user");
    for_all(&(kinds, hostile_text()), |(kind, body)| {
        let line = json!({"type": kind, "message": {"role": "user", "content": body}});
        check_eq(&classify(&entry(&line)?), &Reason::NotUser, "reason")?;
        Ok(())
    })
}

#[test]
fn leading_whitespace_never_changes_a_prompt_and_a_harness_tag_always_does() -> TestResult {
    let cases = (
        any_user_line(),
        select(&[" ", "\n", "\t", "\r\n", " \n\t "][..]),
        select(TAGS),
    );
    for_all(
        &cases,
        |((body, flags, origin, tool_result), space, tag)| {
            let reason = classify(&entry(&user_line(
                &body,
                flags,
                origin.as_deref(),
                tool_result,
            ))?);
            if reason != Reason::Human {
                return Ok(());
            }
            let spaced = format!("{space}{body}");
            let respaced = classify(&entry(&user_line(
                &spaced,
                flags,
                origin.as_deref(),
                tool_result,
            ))?);
            check_eq(&respaced, &Reason::Human, "after leading whitespace")?;
            let tagged = format!("{space}{tag}{body}");
            let retagged = classify(&entry(&user_line(
                &tagged,
                flags,
                origin.as_deref(),
                tool_result,
            ))?);
            check_eq(&retagged, &Reason::HarnessTag, "after a harness tag")?;
            Ok(())
        },
    )
}
