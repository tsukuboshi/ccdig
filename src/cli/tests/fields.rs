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
            "timestamp",
            "text",
            "kind",
            "command",
            "permission_mode",
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
fn fields_names_an_unknown_key_and_needs_json() -> TestResult {
    let history = History::new("root")?;
    for (args, want) in [
        (
            &["sessions", "--fields", "txt"][..],
            "ccdig: --fields: unknown key \"txt\": use agent, session_id, title, project, cwd, git_branch, file, mtime, started_at, ended_at, prompt_count, match_count, or usage\n",
        ),
        (
            &["sessions", "--fields", "!title,!txt"][..],
            "ccdig: --fields: unknown key \"txt\": use agent, session_id, title, project, cwd, git_branch, file, mtime, started_at, ended_at, prompt_count, match_count, or usage\n",
        ),
        (
            &["records", "--fields", "text", "--format", "md"][..],
            "ccdig: --fields needs --format json or jsonl\n",
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
