//! Tests for `--touched`: the tool calls that changed a path, or a shell
//! command that names it, against synthetic history.

use core::error::Error;

use serde_json::Value;

use super::{History, fixture_time, json_lines, strings};
use crate::testing::{TestResult, check_eq};

/// The session's ID.
const SESSION: &str = "tttt";

/// A history where the session edits a file, writes the README, edits a
/// notebook, moves the README in a shell, reads the README, and edits
/// another file whose old text names the README, under the home directory
/// the runs see.
fn history() -> Result<History, Box<dyn Error>> {
    let history = History::new("root")?;
    let calls = [
        (
            "t-edit",
            "Edit",
            r#"{"file_path":"/home/tester/blog/src/cli/args.rs","old_string":"a","new_string":"b"}"#,
        ),
        (
            "t-write",
            "Write",
            r#"{"file_path":"/home/tester/blog/README.md","content":"hi"}"#,
        ),
        (
            "t-notebook",
            "NotebookEdit",
            r#"{"notebook_path":"/home/tester/blog/nb.ipynb","new_source":"x"}"#,
        ),
        (
            "t-bash",
            "Bash",
            r#"{"command":"mv README.md docs/README.md"}"#,
        ),
        (
            "t-read",
            "Read",
            r#"{"file_path":"/home/tester/blog/README.md"}"#,
        ),
        (
            "t-other",
            "Edit",
            r#"{"file_path":"/home/tester/blog/src/main.rs","old_string":"see README.md","new_string":"x"}"#,
        ),
    ];
    let mut lines = Vec::new();
    for (index, &(id, tool, input)) in calls.iter().enumerate() {
        lines.push(format!(
            r#"{{"type":"assistant","uuid":"c-{index}","sessionId":"{SESSION}","cwd":"/home/tester/blog","timestamp":"2026-10-15T00:0{index}:00Z","message":{{"content":[{{"type":"tool_use","id":"{id}","name":"{tool}","input":{input}}}]}}}}"#
        ));
        lines.push(format!(
            r#"{{"type":"user","uuid":"r-{index}","sessionId":"{SESSION}","cwd":"/home/tester/blog","timestamp":"2026-10-15T00:0{index}:10Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"ok"}}]}}}}"#
        ));
    }
    let borrowed: Vec<&str> = lines.iter().map(String::as_str).collect();
    history.write(
        &format!("-home-tester-blog/{SESSION}.jsonl"),
        fixture_time()?,
        &borrowed,
    )?;
    Ok(history)
}

/// The records `args` print for the session, parsed.
fn records(history: &History, args: &[&str]) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut full = vec!["records", "--session", SESSION];
    full.extend_from_slice(args);
    json_lines(history.run(&full))
}

#[test]
fn touched_picks_the_calls_that_changed_a_path_or_name_it_in_a_shell() -> TestResult {
    let history = history()?;
    for (args, want) in [
        (&["--touched", "README.md"][..], vec!["t-write", "t-bash"]),
        (
            &["--touched", "README.md", "--tool", "!Bash"][..],
            vec!["t-write"],
        ),
        (&["--touched", "src/cli"][..], vec!["t-edit"]),
        (&["--touched", "nb.ipynb"][..], vec!["t-notebook"]),
        (
            &["--touched", "src/cli/args.rs", "--touched", "nb.ipynb"][..],
            vec!["t-edit", "t-notebook"],
        ),
        // The shell's relative path is not joined to the cwd.
        (
            &["--touched", "/home/tester/blog/README.md"][..],
            vec!["t-write"],
        ),
        // A read changes nothing, so a leading ! leaves out a path among
        // the calls that change one; a path in the old text is not where
        // the edit went.
        (
            &[
                "--touched",
                "!README.md",
                "--tool",
                "Edit",
                "--tool",
                "Read",
            ][..],
            vec!["t-edit", "t-other"],
        ),
    ] {
        check_eq(
            &strings(&records(&history, args)?, "tool_use_id"),
            &want,
            &args.join(" "),
        )?;
    }
    Ok(())
}

#[test]
fn touched_lists_the_calls_unless_type_asks_for_the_results() -> TestResult {
    let history = history()?;
    let calls = records(&history, &["--touched", "README.md"])?;
    check_eq(&strings(&calls, "type"), &vec!["call", "call"], "calls")?;
    let results = records(&history, &["--touched", "README.md", "--type", "result"])?;
    check_eq(
        &strings(&results, "tool_use_id"),
        &vec!["t-write", "t-bash"],
        "results",
    )?;
    Ok(())
}

#[test]
fn touched_matches_the_path_before_masking() -> TestResult {
    let history = history()?;
    let masked = records(
        &history,
        &["--touched", "/home/tester/blog/README.md", "--mask", "user"],
    )?;
    check_eq(&strings(&masked, "tool_use_id"), &vec!["t-write"], "ids")?;
    check_eq(
        &masked
            .first()
            .and_then(|call| call.pointer("/input/file_path"))
            .and_then(Value::as_str),
        &Some("/home/***REDACTED***/blog/README.md"),
        "the input is still masked",
    )?;
    Ok(())
}

#[test]
fn stats_counts_the_sessions_that_touched_a_path() -> TestResult {
    let history = history()?;
    let groups = json_lines(history.run(&[
        "stats",
        "--session",
        SESSION,
        "--by",
        "session",
        "--touched",
        "README.md",
    ]))?;
    check_eq(
        &groups
            .iter()
            .map(|group| group.get("count").and_then(Value::as_u64))
            .collect::<Vec<_>>(),
        &vec![Some(2)],
        "count",
    )?;
    Ok(())
}

#[test]
fn touched_without_calls_or_results_or_a_path_is_an_error() -> TestResult {
    let history = history()?;
    for (args, want) in [
        (
            &["records", "--type", "prompt", "--touched", "README.md"][..],
            "ccdig: --touched needs --type call or result\n",
        ),
        (
            &["records", "--touched", "."][..],
            "ccdig: --touched needs a path with a part other than / and ., not \".\"\n",
        ),
        (
            &["records", "--touched", "!/"][..],
            "ccdig: --touched needs a path with a part other than / and ., not \"!/\"\n",
        ),
    ] {
        check_eq(&history.run(args).err.as_str(), &want, &args.join(" "))?;
    }
    Ok(())
}
