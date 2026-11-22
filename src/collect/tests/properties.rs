//! Properties of collecting records that hold for any masked values.

use proptest::collection::vec;
use serde_json::{Value, json};

use super::{ALL, Root, masking};
use crate::collect::collect;
use crate::collect::kinds::Records;
use crate::filter::Query;
use crate::testing::{TestResult, check_eq, for_all_cases, hostile_text};

/// Transcripts tried; each writes files and compiles the masking rules.
const FILE_CASES: u32 = 32;

/// The lines of one session that put `value` in every field that is
/// printed: the cwd, the branch, a prompt, a reply, its model, a tool's
/// name and input, the tool's output, and the title.
fn session_lines(value: &str, noise: &str) -> Vec<String> {
    let head = |kind: &str, uuid: &str, at: &str| {
        json!({
            "type": kind,
            "uuid": uuid,
            "sessionId": "s-1",
            "cwd": format!("/Users/alice/{value}/app"),
            "gitBranch": format!("feat/{value}"),
            "timestamp": at,
        })
    };
    let with = |mut line: Value, message: Value| {
        if let Some(fields) = line.as_object_mut() {
            fields.insert("message".to_owned(), message);
        }
        line.to_string()
    };
    vec![
        with(
            head("user", "u-1", "2026-10-15T00:00:00Z"),
            json!({"role": "user", "content": format!("deploy {value} {noise}")}),
        ),
        with(
            head("assistant", "a-1", "2026-10-15T00:00:01Z"),
            json!({"role": "assistant", "id": "m-1", "model": format!("model-{value}"), "content": [
                {"type": "text", "text": format!("deployed {value}")},
                {"type": "tool_use", "id": "t-1", "name": format!("Tool{value}"),
                 "input": {"path": format!("/srv/{value}/x"), "list": [value]}},
            ]}),
        ),
        with(
            head("user", "r-1", "2026-10-15T00:00:02Z"),
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t-1", "content": format!("saw {value}")},
            ]}),
        ),
        json!({"type": "ai-title", "sessionId": "s-1", "aiTitle": format!("about {value}")})
            .to_string(),
    ]
}

#[test]
fn a_masked_value_reaches_no_record_and_a_resumed_copy_adds_none() -> TestResult {
    let value_lists = vec("[A-Z]{3}[0-9]{3}[a-z]{2}", 1..3);
    for_all_cases(
        FILE_CASES,
        &(value_lists, hostile_text()),
        |(values, noise)| {
            let root = Root::new()?;
            let first = values.first().ok_or("no value")?;
            let lines = session_lines(first, &noise);
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            let project = format!("-Users-alice-{first}-app");
            root.write(
                &format!("{project}/s-1.jsonl"),
                "2026-10-15T01:00:00Z",
                &refs,
            )?;
            root.write(
                &format!("{project}/s-2.jsonl"),
                "2026-10-15T02:00:00Z",
                &refs,
            )?;
            let given: Vec<(&str, &str, &str)> = values
                .iter()
                .map(|value| (value.as_str(), "\u{FF3B}v\u{FF3D}", "repo"))
                .collect();
            let mask = masking(&given)?;
            let corpus = collect(root.path(), false, true, &Query::default(), &mask, || {
                Records::new(ALL)
            })?;
            let printed = format!(
                "{}\n{}",
                serde_json::to_string(&corpus.records)?,
                serde_json::to_string(&corpus.sessions)?
            );
            for value in &values {
                check_eq(
                    &printed.contains(value.as_str()),
                    &false,
                    &format!("{value} in {printed}"),
                )?;
            }
            check_eq(
                &corpus.records.len(),
                &4,
                "a prompt, a reply, a call, and a result, each once",
            )?;
            let prompts: usize = corpus
                .sessions
                .iter()
                .map(|session| session.prompt_count)
                .sum();
            check_eq(
                &prompts,
                &corpus.records.len(),
                "records the sessions count",
            )?;
            Ok(())
        },
    )
}
