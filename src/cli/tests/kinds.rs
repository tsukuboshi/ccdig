//! Tests for the replies, tools, and results subcommands, against the
//! history in `testdata/kinds/root`.

use core::error::Error;

use serde_json::Value;

use super::History;
use crate::testing::{TestResult, check_eq};

/// The JSON Lines `args` print, parsed.
fn records(history: &History, args: &[&str]) -> Result<Vec<Value>, Box<dyn Error>> {
    let ran = history.run(args);
    if ran.code != 0 {
        return Err(ran.err.into());
    }
    Ok(ran
        .out
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?)
}

/// The string at `key` of each record.
fn strings<'records>(records: &'records [Value], key: &str) -> Vec<&'records str> {
    records
        .iter()
        .map(|record| {
            record
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or("<missing>")
        })
        .collect()
}

#[test]
fn replies_are_the_assistant_text_blocks() -> TestResult {
    let history = History::new("kinds/root")?;
    let main = records(&history, &["replies"])?;
    check_eq(
        &strings(&main, "text"),
        &vec!["I will run the tests.", "Done."],
        "texts",
    )?;
    check_eq(&strings(&main, "skill"), &vec!["tdd", ""], "skills")?;
    check_eq(
        &strings(&main, "session_title"),
        &vec!["Shopping cart", "Shopping cart"],
        "titles",
    )?;
    let all = records(&history, &["replies", "--subagents"])?;
    check_eq(
        &strings(&all, "subagent"),
        &vec!["", "", "agent-7"],
        "with sub-agents",
    )?;
    Ok(())
}

#[test]
fn tools_are_the_calls_with_their_input() -> TestResult {
    let history = History::new("kinds/root")?;
    let calls = records(&history, &["tools"])?;
    check_eq(&strings(&calls, "tool"), &vec!["Bash", "Read"], "tools")?;
    check_eq(
        &calls.first().and_then(|call| call.get("input")).cloned(),
        &Some(serde_json::json!({"command": "cargo test", "description": "Run tests"})),
        "input",
    )?;
    for (args, want) in [
        (&["tools", "--tool", "Bash"][..], vec!["t-1"]),
        (&["tools", "--skill", "tdd"][..], vec!["t-1"]),
        (&["tools", "--grep", r#""cargo test""#][..], vec!["t-1"]),
        // Grouped by file: the session, then its sub-agent.
        (&["tools", "--subagents"][..], vec!["t-1", "t-2", "t-3"]),
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
fn results_carry_the_call_they_answer() -> TestResult {
    let history = History::new("kinds/root")?;
    let results = records(&history, &["results"])?;
    check_eq(
        &strings(&results, "tool_use_id"),
        &vec!["t-1", "t-2", "t-9"],
        "results",
    )?;
    check_eq(
        &strings(&results, "tool"),
        &vec!["Bash", "Read", ""],
        "paired tools",
    )?;
    check_eq(
        &results
            .get(2)
            .and_then(|result| result.get("input"))
            .cloned(),
        &Some(Value::Null),
        "orphan input",
    )?;
    for (args, want) in [
        (&["results", "--errors"][..], vec!["t-1"]),
        (&["results", "--tool", "Read"][..], vec!["t-2"]),
        (&["results", "--skill", "tdd"][..], vec!["t-1"]),
        (
            &["results", "--subagents", "--tool", "Grep"][..],
            vec!["t-3"],
        ),
        // The day range must not cut the call off from its result.
        (
            &[
                "results",
                "--since",
                "2026-10-13",
                "--tz",
                "UTC",
                "--tool",
                "Bash",
            ][..],
            vec!["t-1"],
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
fn grep_sees_the_text_after_masking() -> TestResult {
    let history = History::new("kinds/root")?;
    let masked = records(&history, &["results", "--grep", "EXAMPLEEXAMPLE"])?;
    check_eq(&masked.len(), &0, "masked")?;
    let read = records(&history, &["results", "--tool", "Read"])?;
    check_eq(
        &read
            .first()
            .and_then(|result| result.get("redacted"))
            .cloned(),
        &Some(Value::Bool(true)),
        "redacted",
    )?;
    let plain = records(
        &history,
        &["results", "--no-redact", "--grep", "EXAMPLEEXAMPLE"],
    )?;
    check_eq(&strings(&plain, "tool_use_id"), &vec!["t-2"], "unmasked")?;
    let insensitive = records(&history, &["replies", "-i", "--grep", "^done"])?;
    check_eq(
        &strings(&insensitive, "text"),
        &vec!["Done."],
        "ignore case",
    )?;
    Ok(())
}

#[test]
fn place_and_time_filters_apply_to_every_kind() -> TestResult {
    let history = History::new("kinds/root")?;
    for (args, want) in [
        (&["replies", "--branch", "main"][..], 1_usize),
        (&["replies", "--session", "dddd"][..], 2),
        (&["replies", "--session", "eeee"][..], 0),
        (&["replies", "--project", "shop"][..], 2),
        (&["replies", "--since", "2026-10-14", "--tz", "UTC"][..], 1),
        (&["tools", "--limit", "1"][..], 1),
        (&["prompts"][..], 1),
    ] {
        check_eq(&records(&history, args)?.len(), &want, &args.join(" "))?;
    }
    Ok(())
}

#[test]
fn the_new_kinds_write_markdown() -> TestResult {
    let history = History::new("kinds/root")?;
    let out = history
        .run(&["results", "--format", "md", "--tz", "UTC"])
        .out;
    check_eq(&out.starts_with("# Tool results\n"), &true, "heading")?;
    check_eq(
        &out.contains("- Tool: `Bash`\n- Error: yes\n"),
        &true,
        "failed call",
    )?;
    check_eq(&out.contains("- Tool: (unknown)\n"), &true, "orphan")?;
    Ok(())
}

#[test]
fn a_mistyped_sibling_block_hides_only_itself_from_results() -> TestResult {
    let history = History::new("kinds/root")?;
    history.write(
        "-Users-alice-shop/eeee.jsonl",
        super::fixture_time()?,
        &[concat!(
            r#"{"type":"user","uuid":"e-1","timestamp":"2026-10-13T03:00:00Z","message":{"content":["#,
            r#"{"type":"tool_result","tool_use_id":"t-5","content":"kept"},{"type":"text","text":5}]}}"#,
        )],
    )?;
    let results = records(&history, &["results", "--session", "eeee"])?;
    check_eq(&strings(&results, "text"), &vec!["kept"], "results")?;
    Ok(())
}
