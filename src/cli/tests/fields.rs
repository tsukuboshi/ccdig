//! Tests for `--fields`: the keys `records` and `sessions` write in JSON
//! and JSON Lines, picked by name or, with a leading `!`, left out.

use core::error::Error;

use serde_json::Value;

use super::History;
use crate::testing::{TestResult, check_eq};

/// The keys of each JSON line `args` print in the week of the history.
fn keys(history: &History, args: &[&str]) -> Result<Vec<Vec<String>>, Box<dyn Error>> {
    let ran = history.run_week(args);
    if ran.code != 0 {
        return Err(ran.err.into());
    }
    ran.out
        .lines()
        .map(|line| {
            let record: Value = serde_json::from_str(line)?;
            Ok(record
                .as_object()
                .ok_or("not an object")?
                .keys()
                .cloned()
                .collect())
        })
        .collect()
}

/// `names` as owned strings.
fn owned(names: &[&str]) -> Vec<String> {
    names.iter().map(|&name| name.to_owned()).collect()
}

#[test]
fn fields_keeps_the_keys_named_in_the_order_they_are_written() -> TestResult {
    let history = History::new("root")?;
    let got = keys(
        &history,
        &[
            "records",
            "--session",
            "aaaa",
            "--type",
            "prompt",
            "--fields",
            "text,type",
            "--fields",
            "timestamp",
        ],
    )?;
    check_eq(
        &got.first(),
        &Some(&owned(&["type", "timestamp", "text"])),
        "keys",
    )?;
    Ok(())
}

#[test]
fn fields_with_a_leading_bang_leaves_the_keys_out() -> TestResult {
    let history = History::new("root")?;
    let got = keys(
        &history,
        &[
            "records",
            "--session",
            "aaaa",
            "--type",
            "prompt",
            "--fields",
            "!agent,!session_title,!project,!cwd,!git_branch,!source",
        ],
    )?;
    check_eq(
        &got.first(),
        &Some(&owned(&[
            "type",
            "session_id",
            "session_title_then",
            "timestamp",
            "turn",
            "text",
            "kind",
            "command",
            "permission_mode",
            "superseded",
            "redacted",
            "matched",
            "matches",
        ])),
        "keys",
    )?;
    let mixed = keys(
        &history,
        &[
            "records",
            "--session",
            "aaaa",
            "--type",
            "prompt",
            "--fields",
            "type,text,!text",
        ],
    )?;
    check_eq(&mixed.first(), &Some(&owned(&["type"])), "mixed")?;
    Ok(())
}

#[test]
fn sessions_fields_can_leave_the_tokens_out() -> TestResult {
    let history = History::new("root")?;
    let got = keys(
        &history,
        &["sessions", "--session", "aaaa", "--fields", "!usage"],
    )?;
    let first = got.first().ok_or("no session")?;
    check_eq(&first.contains(&"usage".to_owned()), &false, "usage")?;
    check_eq(&first.contains(&"ended_at".to_owned()), &true, "ended_at")?;
    Ok(())
}

#[test]
fn fields_applies_to_the_items_of_a_json_document_only() -> TestResult {
    let history = History::new("root")?;
    let ran = history.run_week(&[
        "sessions",
        "--session",
        "aaaa",
        "--format",
        "json",
        "--fields",
        "session_id",
    ]);
    check_eq(&ran.code, &0, &ran.err)?;
    let document: Value = serde_json::from_str(&ran.out)?;
    check_eq(&document.get("schema").is_some(), &true, "the envelope")?;
    check_eq(
        &document
            .pointer("/items/0")
            .and_then(Value::as_object)
            .map(|item| item.keys().cloned().collect::<Vec<_>>()),
        &Some(owned(&["session_id"])),
        "items",
    )?;
    Ok(())
}

#[test]
fn fields_names_an_unknown_key() -> TestResult {
    let history = History::new("root")?;
    for (args, want) in [
        (
            &["sessions", "--fields", "txt"][..],
            "ccdig: --fields: unknown key \"txt\": use agent, session_id, title, project, cwd, git_branch, file, mtime, started_at, ended_at, prompt_count, match_count, first_match, usage, resume, or resume_unavailable\n",
        ),
        (
            &["sessions", "--fields", "!title,!txt"][..],
            "ccdig: --fields: unknown key \"txt\": use agent, session_id, title, project, cwd, git_branch, file, mtime, started_at, ended_at, prompt_count, match_count, first_match, usage, resume, or resume_unavailable\n",
        ),
        (
            &["records", "--fields", "txt", "--format", "md"][..],
            "ccdig: --fields: unknown key \"txt\": use type, agent, session_id, session_title, session_title_then, project, cwd, git_branch, timestamp, turn, tool, input, tool_use_id, result, is_error, denial, text, kind, command, permission_mode, superseded, model, skill, subagent, subagent_type, subagent_description, parent_tool_use_id, usage, redacted, source, matched, or matches\n",
        ),
    ] {
        let ran = history.run(args);
        let name = args.join(" ");
        check_eq(&ran.code, &1, &name)?;
        check_eq(ran.out.as_str(), "", &name)?;
        check_eq(ran.err.as_str(), want, &name)?;
    }
    let ran = history.run(&["records", "--fields", "permision_mode"]);
    check_eq(
        &ran.err
            .starts_with("ccdig: --fields: unknown key \"permision_mode\": use type, agent, "),
        &true,
        &ran.err,
    )?;
    Ok(())
}

#[test]
fn an_empty_key_in_fields_is_skipped() -> TestResult {
    let history = History::new("root")?;
    for value in ["text,", ",text", "text,!"] {
        let got = keys(
            &history,
            &[
                "records",
                "--session",
                "aaaa",
                "--type",
                "prompt",
                "--fields",
                value,
            ],
        )?;
        check_eq(&got.first(), &Some(&owned(&["text"])), value)?;
    }
    Ok(())
}

#[test]
fn fields_picks_what_markdown_shows_by_the_same_keys() -> TestResult {
    let history = History::new("root")?;
    let base = [
        "records",
        "--session",
        "aaaa",
        "--type",
        "prompt",
        "--format",
        "md",
        "--tz",
        "Asia/Tokyo",
    ];
    let only_text = history.run(&[&base[..], &["--fields", "text"]].concat());
    check_eq(&only_text.code, &0, &only_text.err)?;
    check_eq(
        &only_text
            .out
            .contains("\n### Prompt\n\n```text\n/weekly-report last week\n```\n"),
        &true,
        &only_text.out,
    )?;
    for gone in ["- Project:", "- Session:", "JST"] {
        check_eq(&only_text.out.contains(gone), &false, gone)?;
    }
    let no_text = history
        .run(&[&base[..], &["--fields", "!text,!turn"]].concat())
        .out;
    check_eq(
        &no_text.contains("### 2026-10-13 10:00:01 JST - Prompt\n"),
        &true,
        &no_text,
    )?;
    check_eq(&no_text.contains("```text"), &false, &no_text)?;
    let sessions = history.run(&[
        "sessions",
        "--session",
        "aaaa",
        "--format",
        "md",
        "--fields",
        "session_id,title",
    ]);
    check_eq(&sessions.code, &0, &sessions.err)?;
    check_eq(
        &sessions
            .out
            .contains("\n| Session | Title |\n| --- | --- |\n| `aaaaaaaa-"),
        &true,
        &sessions.out,
    )?;
    let stats = history.run(&[
        "stats",
        "--session",
        "aaaa",
        "--by",
        "type",
        "--format",
        "md",
        "--fields",
        "type,count",
    ]);
    check_eq(&stats.code, &0, &stats.err)?;
    check_eq(
        &stats
            .out
            .contains("\n| Type | Count |\n| --- | ---: |\n| `prompt` | "),
        &true,
        &stats.out,
    )?;
    Ok(())
}

#[test]
fn markdown_leaves_out_a_table_with_no_column_and_shows_matches_on_their_own() -> TestResult {
    let history = History::new("root")?;
    for args in [
        &["sessions", "--fields", "usage", "--format", "md"][..],
        &[
            "stats", "--by", "type", "--fields", "file", "--format", "md",
        ][..],
    ] {
        let ran = history.run(args);
        check_eq(&ran.code, &0, &ran.err)?;
        check_eq(&ran.out.contains('|'), &false, &ran.out)?;
    }
    let base = [
        "records",
        "--session",
        "aaaa",
        "--type",
        "prompt",
        "--igrep",
        "weekly",
        "--context",
        "0",
        "--format",
        "md",
    ];
    let matches = history
        .run(&[&base[..], &["--fields", "matches"]].concat())
        .out;
    check_eq(
        &matches.contains("- Line 1\n\n```text\n/weekly-report last week\n```\n"),
        &true,
        &matches,
    )?;
    let text = history
        .run(&[&base[..], &["--fields", "text"]].concat())
        .out;
    check_eq(
        &(text.contains("```text\n/weekly-report last week\n```\n") && !text.contains("- Line 1")),
        &true,
        &text,
    )?;
    Ok(())
}
