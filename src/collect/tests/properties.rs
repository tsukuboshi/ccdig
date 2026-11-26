//! Properties of collecting records that hold for any masked values.

use proptest::bool::ANY;
use proptest::collection::vec;
use proptest::sample::select;
use serde_json::{Value, json};

use super::{ALL, Root, masking};
use crate::collect::kinds::{Prompts, Records, Types};
use crate::collect::{Reading, collect};
use crate::filter::{Query, patterns};
use crate::record::{Record as _, Session, Turn};
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
            // The excerpt lists lines of the masked text, found by the
            // marker that replaced the values.
            let excerpting = Query {
                grep: patterns(&["\u{FF3B}v\u{FF3D}"], &[])?,
                ..Query::default()
            };
            let mut last = None;
            for (excerpt, query) in [(None, Query::default()), (Some(1), excerpting)] {
                let found = collect(
                    root.path(),
                    Reading {
                        around: None,
                        excerpt,
                        mentions: None,
                        subagents: false,
                        usage: true,
                    },
                    &query,
                    &mask,
                    || Records::new(ALL),
                )?;
                let printed = format!(
                    "{}\n{}",
                    serde_json::to_string(&found.records)?,
                    serde_json::to_string(&found.sessions)?
                );
                for value in &values {
                    check_eq(
                        &printed.contains(value.as_str()),
                        &false,
                        &format!("{value} in {printed}"),
                    )?;
                }
                if excerpt.is_some() {
                    check_eq(
                        &printed.contains(r#""matches":[]"#),
                        &false,
                        &format!("every record lists the marker's line: {printed}"),
                    )?;
                }
                last = Some(found);
            }
            let corpus = last.ok_or("no run")?;
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

#[test]
fn around_adds_only_context_and_lists_each_record_once() -> TestResult {
    let prompts = vec(select(&["hit", "miss", "WIP hit"][..]), 1..12);
    for_all_cases(FILE_CASES, &(prompts, 0_usize..4), |(texts, around)| {
        let root = Root::new()?;
        let lines: Vec<String> = texts
            .iter()
            .enumerate()
            .map(|(index, text)| {
                json!({
                    "type": "user",
                    "uuid": format!("u-{index}"),
                    "sessionId": "s-1",
                    "timestamp": format!("2026-10-15T00:00:{index:02}Z"),
                    "message": {"role": "user", "content": text},
                })
                .to_string()
            })
            .collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        root.write("-Users-alice-app/s-1.jsonl", "2026-10-15T01:00:00Z", &refs)?;
        let query = Query {
            grep: patterns(&["hit", "!WIP"], &[])?,
            ..Query::default()
        };
        let mask = masking(&[])?;
        let run = |window| {
            collect(
                root.path(),
                Reading {
                    around: window,
                    ..Reading::default()
                },
                &query,
                &mask,
                || Records::new(ALL),
            )
        };
        let uuids = |records: &[Turn], matched: bool| -> Vec<String> {
            records
                .iter()
                .filter(|record| record.matched() == matched)
                .map(|record| record.source().uuid.clone())
                .collect()
        };
        let plain = run(None)?;
        let wide = run(Some(around))?;
        check_eq(
            &uuids(&wide.records, true),
            &uuids(&plain.records, true),
            "the matches",
        )?;
        let mut all: Vec<String> = wide
            .records
            .iter()
            .map(|record| record.source().uuid.clone())
            .collect();
        let listed = all.len();
        all.sort();
        all.dedup();
        check_eq(&all.len(), &listed, "each record once")?;
        let context = uuids(&wide.records, false);
        check_eq(
            &context.iter().any(|uuid| {
                texts
                    .get(
                        uuid.trim_start_matches("u-")
                            .parse::<usize>()
                            .unwrap_or_default(),
                    )
                    .is_some_and(|text| text.contains("WIP"))
            }),
            &false,
            "an excluded record as context",
        )?;
        if around == 0 {
            check_eq(&context.len(), &0, "no context without a window")?;
        }
        Ok(())
    })
}

#[test]
fn a_search_of_sessions_keeps_their_rows_and_counts_what_mentions_it() -> TestResult {
    let typed_or_not = (ANY, select(&["cloud", "sky", "Cloud nine"][..]));
    let histories = vec(vec(typed_or_not, 1..6), 1..4);
    for_all_cases(FILE_CASES, &histories, |sessions| {
        let root = Root::new()?;
        for (index, turns) in sessions.iter().enumerate() {
            let lines: Vec<String> = turns
                .iter()
                .enumerate()
                .map(|(at, &(typed, text))| {
                    let (kind, message) = if typed {
                        ("user", json!({"role": "user", "content": text}))
                    } else {
                        (
                            "assistant",
                            json!({"content": [{"type": "text", "text": text}]}),
                        )
                    };
                    json!({
                        "type": kind,
                        "uuid": format!("u-{index}-{at}"),
                        "sessionId": format!("s-{index}"),
                        "timestamp": format!("2026-10-15T0{index}:00:{at:02}Z"),
                        "message": message,
                    })
                    .to_string()
                })
                .collect();
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            root.write(
                &format!("-Users-alice-app/s-{index}.jsonl"),
                "2026-10-15T09:00:00Z",
                &refs,
            )?;
        }
        let mask = masking(&[])?;
        // As `sessions` reads them: with the assistant lines, for the tokens
        // and the time of the last record.
        let plain = collect(
            root.path(),
            Reading {
                usage: true,
                ..Reading::default()
            },
            &Query::default(),
            &mask,
            Prompts::default,
        )?;
        let searched = collect(
            root.path(),
            Reading {
                mentions: Some(Types::CONVERSATION),
                usage: true,
                ..Reading::default()
            },
            &Query {
                grep: patterns(&["cloud"], &[])?,
                ..Query::default()
            },
            &mask,
            Prompts::default,
        )?;
        for session in &searched.sessions {
            let index: usize = session.id.trim_start_matches("s-").parse()?;
            let want = sessions.get(index).map_or(0, |turns| {
                turns.iter().filter(|said| said.1.contains("cloud")).count()
            });
            check_eq(&session.match_count, &Some(want), &session.id)?;
            let same = plain
                .sessions
                .iter()
                .find(|row| row.id == session.id)
                .ok_or("a row the plain list lacks")?;
            check_eq(
                &Session {
                    match_count: None,
                    first_match: None,
                    matched_types: Vec::new(),
                    ..session.clone()
                },
                same,
                &session.id,
            )?;
        }
        let listed = sessions
            .iter()
            .filter(|turns| {
                turns.iter().any(|said| said.0) && turns.iter().any(|said| said.1.contains("cloud"))
            })
            .count();
        check_eq(
            &searched.sessions.len(),
            &listed,
            "the sessions that mention it",
        )?;
        Ok(())
    })
}
