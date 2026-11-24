//! Tests for collecting records from transcripts: the order of masking and
//! filtering, titles for sub-agents, resumed copies, tokens, and the lines
//! `--context` lists.

mod properties;

use core::error::Error;
use std::path::Path;

use tempfile::TempDir;

use super::excerpt::excerpt;
use super::kinds::{Prompts, Records, Types};
use super::mask::Mask;
use super::{Reading, collect};
use crate::filter::{Filter, Query, patterns};
use crate::record::{Record as _, Turn};
use crate::redact::ExtraMasks;
use crate::testing::{TestResult, check_eq, time, write_transcript};
use crate::transcript::entry::Usage;

/// Every kind of record `records` can pick.
const ALL: Types = Types {
    calls: true,
    prompts: true,
    replies: true,
    results: true,
    summaries: true,
};

/// A transcript root in a temp dir.
struct Root {
    /// Keeps the root until the test ends.
    dir: TempDir,
}

impl Root {
    /// An empty root.
    fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            dir: tempfile::tempdir()?,
        })
    }

    /// The root's path.
    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Adds a transcript at `relative` with `lines`, last written at
    /// `mtime`.
    fn write(&self, relative: &str, mtime: &str, lines: &[&str]) -> TestResult {
        write_transcript(&self.path().join(relative), time(mtime)?, lines)
    }
}

/// The masking with the built-in rules and `values`, as `--mask-values`
/// gives them: each value with its replacement and label.
fn masking(values: &[(&str, &str, &str)]) -> Result<Mask, Box<dyn Error>> {
    Ok(Mask::new(
        false,
        ExtraMasks {
            values: values
                .iter()
                .map(|&(value, replacement, label)| {
                    (value.to_owned(), replacement.to_owned(), label.to_owned())
                })
                .collect(),
            ..ExtraMasks::default()
        },
    )?)
}

/// The text of a prompt record, or empty for another kind.
fn text(turn: &Turn) -> &str {
    match *turn {
        Turn::Prompt(ref prompt) => &prompt.text,
        Turn::Call(_) | Turn::Reply(_) | Turn::Result(_) | Turn::Summary(_) => "",
    }
}

/// The tokens a reply or a tool call carries, or `None`.
fn usage(turn: &Turn) -> Option<Usage> {
    match *turn {
        Turn::Reply(ref reply) => reply.usage,
        Turn::Call(ref call) => call.usage,
        Turn::Prompt(_) | Turn::Result(_) | Turn::Summary(_) => None,
    }
}

#[test]
fn grep_sees_the_masked_text_and_project_sees_the_real_path() -> TestResult {
    let root = Root::new()?;
    root.write(
        "-Users-alice-corp-hidden/aaaa.jsonl",
        "2026-10-15T01:00:00Z",
        &[
            r#"{"type":"user","uuid":"a-1","sessionId":"aaaa","cwd":"/Users/alice/corp-hidden","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"deploy corp-hidden"}}"#,
        ],
    )?;
    let mask = masking(&[("corp-hidden", "\u{FF3B}repo1\u{FF3D}", "repo")])?;
    let run = |grep: &str| {
        let query = Query {
            grep: patterns(&[grep], &[])?,
            projects: Filter::names(vec!["corp-hidden".to_owned()]),
            ..Query::default()
        };
        collect(root.path(), Reading::default(), &query, &mask, || {
            Records::new(ALL)
        })
    };
    let masked = run("repo1")?;
    let record = masked
        .records
        .first()
        .ok_or("--grep did not see the masked text")?;
    check_eq(text(record), "deploy \u{FF3B}repo1\u{FF3D}", "text")?;
    check_eq(
        record.head().project.as_str(),
        "-Users-alice-\u{FF3B}repo1\u{FF3D}",
        "project",
    )?;
    check_eq(
        record.head().cwd.as_str(),
        "/Users/alice/\u{FF3B}repo1\u{FF3D}",
        "cwd",
    )?;
    check_eq(&run("corp-hidden")?.records.len(), &0, "the real text")?;
    Ok(())
}

#[test]
fn a_subagent_takes_the_title_of_its_session() -> TestResult {
    let root = Root::new()?;
    root.write(
        "-Users-alice-blog/hhhh.jsonl",
        "2026-10-15T01:00:00Z",
        &[
            r#"{"type":"assistant","uuid":"h-1","sessionId":"hhhh","timestamp":"2026-10-15T00:00:00Z","message":{"content":[{"type":"tool_use","id":"toolu_h1","name":"Bash","input":{"command":"ls"}}]}}"#,
            r#"{"type":"ai-title","sessionId":"hhhh","aiTitle":"Tidy the notes"}"#,
        ],
    )?;
    root.write(
        "-Users-alice-blog/hhhh/subagents/agent-9.jsonl",
        "2026-10-15T01:00:00Z",
        &[
            r#"{"type":"assistant","uuid":"h-2","sessionId":"hhhh","isSidechain":true,"timestamp":"2026-10-15T00:01:00Z","message":{"content":[{"type":"tool_use","id":"toolu_h2","name":"Bash","input":{"command":"pwd"}}]}}"#,
        ],
    )?;
    let corpus = collect(
        root.path(),
        Reading {
            excerpt: None,
            subagents: true,
            usage: false,
        },
        &Query::default(),
        &masking(&[])?,
        || Records::new(ALL),
    )?;
    check_eq(&corpus.records.len(), &2, "records")?;
    for record in &corpus.records {
        check_eq(
            record.head().session_title.as_str(),
            "Tidy the notes",
            record.subagent(),
        )?;
    }
    Ok(())
}

#[test]
fn a_record_copied_into_a_resumed_session_is_kept_once_from_the_original() -> TestResult {
    let root = Root::new()?;
    let shared = r#"{"type":"user","uuid":"r-1","timestamp":"2026-10-15T00:00:00Z","cwd":"/Users/alice/r","message":{"role":"user","content":"shared"}}"#;
    root.write(
        "-Users-alice-r/zz-original.jsonl",
        "2026-10-15T01:00:00Z",
        &[shared],
    )?;
    root.write(
        "-Users-alice-r/aa-resumed.jsonl",
        "2026-10-16T01:00:00Z",
        &[
            shared,
            r#"{"type":"user","uuid":"r-2","timestamp":"2026-10-16T00:00:00Z","cwd":"/Users/alice/r","message":{"role":"user","content":"new"}}"#,
        ],
    )?;
    let corpus = collect(
        root.path(),
        Reading::default(),
        &Query::default(),
        &masking(&[])?,
        || Prompts,
    )?;
    let kept: Vec<(&str, bool)> = corpus
        .records
        .iter()
        .map(|prompt| {
            (
                prompt.source.uuid.as_str(),
                prompt.source.file.ends_with("zz-original.jsonl"),
            )
        })
        .collect();
    check_eq(&kept, &vec![("r-1", true), ("r-2", false)], "prompts")?;
    check_eq(&corpus.sessions.len(), &2, "sessions")?;
    Ok(())
}

#[test]
fn a_response_s_tokens_go_to_its_first_record_and_count_once_per_session() -> TestResult {
    let root = Root::new()?;
    root.write(
        "-Users-alice-blog/uuuu.jsonl",
        "2026-10-15T01:00:00Z",
        &[
            r#"{"type":"user","uuid":"t-1","sessionId":"uuuu","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"count the files"}}"#,
            r#"{"type":"assistant","uuid":"t-2","sessionId":"uuuu","timestamp":"2026-10-15T00:00:10Z","message":{"id":"msg_1","content":[{"type":"text","text":"Counting."}],"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":20}}}"#,
            r#"{"type":"assistant","uuid":"t-3","sessionId":"uuuu","timestamp":"2026-10-15T00:00:11Z","message":{"id":"msg_1","content":[{"type":"tool_use","id":"toolu_t1","name":"Bash","input":{"command":"ls | wc -l"}}],"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":20}}}"#,
            r#"{"type":"assistant","uuid":"t-4","sessionId":"uuuu","timestamp":"2026-10-15T00:00:20Z","message":{"id":"msg_2","content":[{"type":"text","text":"Three."}],"usage":{"input_tokens":1,"output_tokens":2,"cache_read_input_tokens":3,"cache_creation_input_tokens":4}}}"#,
        ],
    )?;
    let corpus = collect(
        root.path(),
        Reading {
            excerpt: None,
            subagents: false,
            usage: true,
        },
        &Query::default(),
        &masking(&[])?,
        || Records::new(ALL),
    )?;
    let first = Usage {
        input: 10,
        output: 5,
        cache_read: 100,
        cache_creation: 20,
    };
    let second = Usage {
        input: 1,
        output: 2,
        cache_read: 3,
        cache_creation: 4,
    };
    let tokens: Vec<Option<Usage>> = corpus.records.iter().map(usage).collect();
    check_eq(
        &tokens,
        &vec![None, Some(first), None, Some(second)],
        "per record",
    )?;
    let session = corpus.sessions.first().ok_or("no session")?;
    check_eq(
        &session.usage,
        &Usage {
            input: 11,
            output: 7,
            cache_read: 103,
            cache_creation: 24,
        },
        "per session",
    )?;
    Ok(())
}

#[test]
fn context_joins_the_windows_around_matches_and_stops_at_the_ends() -> TestResult {
    let text = "a\nhit 2\nc\nd\ne\nhit 6\ng";
    let grep = patterns(&["hit"], &[])?;
    for (context, want) in [
        (0, vec![(2, true), (6, true)]),
        (
            1,
            vec![
                (1, false),
                (2, true),
                (3, false),
                (5, false),
                (6, true),
                (7, false),
            ],
        ),
        (
            2,
            vec![
                (1, false),
                (2, true),
                (3, false),
                (4, false),
                (5, false),
                (6, true),
                (7, false),
            ],
        ),
        (
            usize::MAX,
            vec![
                (1, false),
                (2, true),
                (3, false),
                (4, false),
                (5, false),
                (6, true),
                (7, false),
            ],
        ),
    ] {
        let got: Vec<(usize, bool)> = excerpt(&[("text".to_owned(), text.into())], &grep, context)
            .iter()
            .map(|line| (line.line, line.hit))
            .collect();
        check_eq(&got, &want, &context.to_string())?;
    }
    Ok(())
}

#[test]
fn context_keeps_each_field_apart_and_lists_nothing_without_a_pattern_to_find() -> TestResult {
    let fields = [
        ("input.old".to_owned(), "x\nhit".into()),
        ("input.new".to_owned(), "hit\r\ny".into()),
    ];
    let got: Vec<(String, usize, String)> = excerpt(&fields, &patterns(&["hit"], &[])?, 1)
        .into_iter()
        .map(|line| (line.field, line.line, line.text))
        .collect();
    check_eq(
        &got,
        &vec![
            ("input.old".to_owned(), 1, "x".to_owned()),
            ("input.old".to_owned(), 2, "hit".to_owned()),
            ("input.new".to_owned(), 1, "hit".to_owned()),
            ("input.new".to_owned(), 2, "y".to_owned()),
        ],
        "fields",
    )?;
    check_eq(
        &excerpt(&fields, &patterns(&["!hit"], &[])?, 1).len(),
        &0,
        "only an excluding pattern",
    )?;
    Ok(())
}

#[test]
fn a_line_without_a_response_id_is_a_response_of_its_own() -> TestResult {
    let root = Root::new()?;
    root.write(
        "-Users-alice-blog/vvvv.jsonl",
        "2026-10-15T01:00:00Z",
        &[
            r#"{"type":"assistant","uuid":"v-1","sessionId":"vvvv","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"text","text":"Counting."},{"type":"tool_use","id":"toolu_v1","name":"Bash","input":{"command":"ls"}}],"usage":{"input_tokens":1,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
            r#"{"type":"assistant","uuid":"v-2","sessionId":"vvvv","timestamp":"2026-10-15T00:00:20Z","message":{"content":[{"type":"text","text":"Done."}],"usage":{"input_tokens":2,"output_tokens":2,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
        ],
    )?;
    let corpus = collect(
        root.path(),
        Reading::default(),
        &Query::default(),
        &masking(&[])?,
        || Records::new(ALL),
    )?;
    let inputs: Vec<Option<u64>> = corpus
        .records
        .iter()
        .map(|record| usage(record).map(|tokens| tokens.input))
        .collect();
    check_eq(&inputs, &vec![Some(1), None, Some(2)], "tokens")?;
    Ok(())
}
