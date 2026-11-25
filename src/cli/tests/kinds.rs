//! Tests for the kinds of record and the flags for them, against the
//! history in `testdata/kinds/root`.

use core::error::Error;

use serde_json::Value;

use super::{History, json_lines, strings};
use crate::testing::{TestResult, check_eq};

/// The JSON Lines `args` print, parsed.
fn records(history: &History, args: &[&str]) -> Result<Vec<Value>, Box<dyn Error>> {
    json_lines(history.run(args))
}

#[test]
fn replies_are_the_assistant_text_blocks() -> TestResult {
    let history = History::new("kinds/root")?;
    let main = records(&history, &["records", "--type", "reply"])?;
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
    let all = records(&history, &["records", "--type", "reply", "--subagents"])?;
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
    let calls = records(&history, &["records", "--type", "call"])?;
    check_eq(&strings(&calls, "tool"), &vec!["Bash", "Read"], "tools")?;
    check_eq(
        &calls.first().and_then(|call| call.get("input")).cloned(),
        &Some(serde_json::json!({"command": "cargo test", "description": "Run tests"})),
        "input",
    )?;
    for (args, want) in [
        (
            &["records", "--type", "call", "--tool", "Bash"][..],
            vec!["t-1"],
        ),
        (
            &["records", "--type", "call", "--skill", "tdd"][..],
            vec!["t-1"],
        ),
        (
            &["records", "--type", "call", "--grep", r#""cargo test""#][..],
            vec!["t-1"],
        ),
        // Grouped by file: the session, then its sub-agent.
        (
            &["records", "--type", "call", "--subagents"][..],
            vec!["t-1", "t-2", "t-3"],
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
fn results_carry_the_call_they_answer() -> TestResult {
    let history = History::new("kinds/root")?;
    let results = records(&history, &["records", "--type", "result"])?;
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
        (
            &["records", "--type", "result", "--status", "error"][..],
            vec!["t-1"],
        ),
        (
            &["records", "--type", "result", "--tool", "Read"][..],
            vec!["t-2"],
        ),
        (
            &["records", "--type", "result", "--skill", "tdd"][..],
            vec!["t-1"],
        ),
        (
            &[
                "records",
                "--type",
                "result",
                "--subagents",
                "--tool",
                "Grep",
            ][..],
            vec!["t-3"],
        ),
        // The day range must not cut the call off from its result.
        (
            &[
                "records",
                "--type",
                "result",
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
    let masked = records(
        &history,
        &["records", "--type", "result", "--grep", "EXAMPLEEXAMPLE"],
    )?;
    check_eq(&masked.len(), &0, "masked")?;
    let read = records(&history, &["records", "--type", "result", "--tool", "Read"])?;
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
        &[
            "records",
            "--type",
            "result",
            "--no-mask",
            "--grep",
            "EXAMPLEEXAMPLE",
        ],
    )?;
    check_eq(&strings(&plain, "tool_use_id"), &vec!["t-2"], "unmasked")?;
    for (args, want) in [
        (
            &["records", "--type", "reply", "--grep", "^done"][..],
            vec![],
        ),
        (
            &["records", "--type", "reply", "--igrep", "^done"][..],
            vec!["Done."],
        ),
        // Patterns add up across the levels; a record needs to match one.
        (
            &[
                "--igrep", "^done", "records", "--type", "reply", "--grep", "^I ",
            ][..],
            vec!["I will run the tests.", "Done."],
        ),
        // A leading ! leaves out what matches.
        (
            &["records", "--type", "reply", "--igrep", "!^done"][..],
            vec!["I will run the tests."],
        ),
        (
            &[
                "records", "--type", "reply", "--grep", "run", "--grep", "!tests",
            ][..],
            vec![],
        ),
    ] {
        check_eq(
            &strings(&records(&history, args)?, "text"),
            &want,
            &args.join(" "),
        )?;
    }
    Ok(())
}

#[test]
fn place_and_time_filters_apply_to_every_kind() -> TestResult {
    let history = History::new("kinds/root")?;
    for (args, want) in [
        (
            &["records", "--type", "reply", "--branch", "main"][..],
            1_usize,
        ),
        (&["records", "--type", "reply", "--session", "dddd"][..], 2),
        (&["records", "--type", "reply", "--session", "eeee"][..], 0),
        (&["records", "--type", "reply", "--project", "shop"][..], 2),
        (
            &[
                "records",
                "--type",
                "reply",
                "--since",
                "2026-10-14",
                "--tz",
                "UTC",
            ][..],
            1,
        ),
        (&["records", "--type", "call", "--limit", "1"][..], 1),
        (&["records", "--type", "prompt"][..], 1),
    ] {
        check_eq(&records(&history, args)?.len(), &want, &args.join(" "))?;
    }
    Ok(())
}

#[test]
fn results_in_markdown_carry_the_tool_and_the_status() -> TestResult {
    let history = History::new("kinds/root")?;
    let out = history
        .run(&[
            "records", "--type", "result", "--format", "md", "--tz", "UTC",
        ])
        .out;
    check_eq(&out.starts_with("# Records\n"), &true, "heading")?;
    check_eq(
        &out.contains(" - Tool result\n\n- Tool: `Bash`\n- Error: yes\n"),
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
    let results = records(
        &history,
        &["records", "--type", "result", "--session", "eeee"],
    )?;
    check_eq(&strings(&results, "text"), &vec!["kept"], "results")?;
    Ok(())
}

#[test]
fn kind_filters_let_the_kinds_without_the_fact_through() -> TestResult {
    let history = History::new("kinds/root")?;
    for (args, want) in [
        (
            &["records", "--type", "prompt,call", "--tool", "Bash"][..],
            vec!["prompt", "call"],
        ),
        (
            &["records", "--type", "prompt,result", "--status", "error"][..],
            vec!["prompt", "result"],
        ),
        (
            &["records", "--type", "reply,result", "--tool", "Bash"][..],
            vec!["reply", "result", "reply"],
        ),
        (
            &["records", "--type", "prompt,reply,call", "--skill", "tdd"][..],
            vec!["prompt", "reply", "call"],
        ),
    ] {
        check_eq(
            &strings(&records(&history, args)?, "type"),
            &want,
            &args.join(" "),
        )?;
    }
    Ok(())
}

#[test]
fn a_kind_filter_without_a_kind_it_applies_to_is_an_error() -> TestResult {
    let history = History::new("kinds/root")?;
    for (args, prefix) in [
        (
            &["records", "--type", "prompt,reply", "--tool", "Bash"][..],
            "ccdig: --tool needs --type call or result",
        ),
        (
            &["records", "--type", "call", "--status", "error"][..],
            "ccdig: --status needs --type result",
        ),
        (
            &["records", "--type", "prompt", "--denial", "user-rejected"][..],
            "ccdig: --denial needs --type result",
        ),
        (
            &["records", "--type", "prompt", "--skill", "tdd"][..],
            "ccdig: --skill needs --type reply, call, or result",
        ),
        (
            &["records", "--type", "prompt", "--subagents"][..],
            "ccdig: --subagents needs --type reply, call, or result",
        ),
    ] {
        let ran = history.run(args);
        let name = args.join(" ");
        check_eq(&ran.code, &1, &name)?;
        check_eq(
            &ran.err.starts_with(prefix),
            &true,
            &format!("{name}: {}", ran.err),
        )?;
        check_eq(&ran.out.as_str(), &"", &name)?;
    }
    Ok(())
}

#[test]
fn kind_filters_without_type_list_the_kinds_they_apply_to() -> TestResult {
    let history = History::new("kinds/root")?;
    for (args, want) in [
        (&["records", "--tool", "Bash"][..], vec!["call", "result"]),
        (&["records", "--status", "error"][..], vec!["result"]),
        (
            &["records", "--skill", "tdd"][..],
            vec!["reply", "call", "result"],
        ),
        (
            &["records", "--skill", "tdd", "--tool", "Bash"][..],
            vec!["call", "result"],
        ),
        (
            &["records", "--tool", "Bash", "--status", "error"][..],
            vec!["result"],
        ),
    ] {
        check_eq(
            &strings(&records(&history, args)?, "type"),
            &want,
            &args.join(" "),
        )?;
    }
    Ok(())
}

#[test]
fn a_leading_bang_leaves_out_a_name() -> TestResult {
    let history = History::new("kinds/root")?;
    for (args, key, want) in [
        (
            &["records", "--type", "reply", "--session", "!dddd"][..],
            "text",
            vec![],
        ),
        (
            &["records", "--type", "reply", "--project", "!blog"][..],
            "text",
            vec!["I will run the tests.", "Done."],
        ),
        (
            &["records", "--type", "reply", "--branch", "!main"][..],
            "text",
            vec!["I will run the tests."],
        ),
        (
            &["records", "--type", "reply", "--skill", "!tdd"][..],
            "text",
            vec!["Done."],
        ),
        // Without --type, the kinds --tool applies to; a result whose call
        // was not found has no tool to leave out.
        (
            &["records", "--tool", "!Bash"][..],
            "tool_use_id",
            vec!["t-2", "t-2", "t-9"],
        ),
    ] {
        check_eq(
            &strings(&records(&history, args)?, key),
            &want,
            &args.join(" "),
        )?;
    }
    Ok(())
}
