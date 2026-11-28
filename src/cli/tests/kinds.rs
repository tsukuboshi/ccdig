//! Tests for the kinds of record and the flags for them, against the
//! history in `testdata/kinds/root`.

use core::error::Error;

use serde_json::{Value, json};

use super::{History, fixture_time, json_lines, strings};
use crate::testing::{TestResult, check_eq};

/// A session of two turns: a prompt, a reply, a call and its result, then
/// an answer to a question in the second turn.
const TURNS: [&str; 9] = [
    r#"{"type":"assistant","uuid":"n-0","sessionId":"nnnn","timestamp":"2026-10-15T00:00:00Z","message":{"model":"claude-fable-5-2","content":[{"type":"text","text":"Before any prompt"}]}}"#,
    r#"{"type":"user","uuid":"n-1","sessionId":"nnnn","timestamp":"2026-10-15T00:00:01Z","message":{"role":"user","content":"list the files"}}"#,
    r#"{"type":"assistant","uuid":"n-2","sessionId":"nnnn","timestamp":"2026-10-15T00:00:02Z","message":{"model":"claude-fable-5-2","content":[{"type":"text","text":"Listing"},{"type":"tool_use","id":"t-n1","name":"Bash","input":{"command":"ls"}}]}}"#,
    r#"{"type":"user","uuid":"n-3","sessionId":"nnnn","timestamp":"2026-10-15T00:00:03Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-n1","content":"Cargo.toml"}]}}"#,
    r#"{"type":"assistant","uuid":"n-4","sessionId":"nnnn","timestamp":"2026-10-15T00:00:04Z","message":{"model":"claude-fable-5-2","content":[{"type":"text","text":"One file"}]}}"#,
    r#"{"type":"user","uuid":"n-5","sessionId":"nnnn","timestamp":"2026-10-15T00:00:05Z","message":{"role":"user","content":"which one?"}}"#,
    r#"{"type":"assistant","uuid":"n-6","sessionId":"nnnn","timestamp":"2026-10-15T00:00:06Z","message":{"model":"claude-fable-5-2","content":[{"type":"tool_use","id":"t-n2","name":"AskUserQuestion","input":{"questions":[]}}]}}"#,
    r#"{"type":"user","uuid":"n-7","sessionId":"nnnn","timestamp":"2026-10-15T00:00:07Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-n2","content":"Your questions have been answered: \"Which?\"=\"Cargo.toml\". You can now continue with these answers in mind."}]}}"#,
    r#"{"type":"assistant","uuid":"n-8","sessionId":"nnnn","timestamp":"2026-10-15T00:00:08Z","message":{"model":"claude-fable-5-2","content":[{"type":"text","text":"Cargo.toml it is"}]}}"#,
];

/// The JSON Lines `args` print, parsed.
fn records(history: &History, args: &[&str]) -> Result<Vec<Value>, Box<dyn Error>> {
    json_lines(history.run(args))
}

/// The `turn` of each record in `records`.
fn turns(records: &[Value]) -> Vec<Option<u64>> {
    records
        .iter()
        .map(|record| record.get("turn").and_then(Value::as_u64))
        .collect()
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
        // With none of them, prompts and replies; --tool-chars and
        // --subagents pick no kinds of their own.
        (&["records"][..], vec!["prompt", "reply", "reply"]),
        (
            &["records", "--tool-chars", "5"][..],
            vec!["prompt", "reply", "reply"],
        ),
        (
            &["records", "--subagents"][..],
            vec!["prompt", "reply", "reply", "reply"],
        ),
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

#[test]
fn a_call_carries_its_result_masked_without_type_result() -> TestResult {
    let history = History::new("kinds/root")?;
    let calls = records(&history, &["records", "--type", "call"])?;
    let result_of = |index: usize| calls.get(index)?.get("result");
    check_eq(
        &result_of(0),
        &Some(&json!({
            "is_error": true,
            "denial": "",
            "text": "test failed: cart is empty",
        })),
        "the failed test run",
    )?;
    let read = result_of(1)
        .and_then(|result| result.get("text"))
        .and_then(Value::as_str)
        .ok_or("no result text for the read")?;
    check_eq(&read.contains("EXAMPLEEXAMPLE"), &false, read)?;
    let alone = records(&history, &["records", "--type", "result", "--tool", "Read"])?;
    check_eq(
        &strings(&alone, "text"),
        &vec![read],
        "masked as the result alone",
    )?;
    check_eq(
        &calls.get(1).and_then(|call| call.get("redacted")),
        &Some(&json!(true)),
        "a call whose result was masked is redacted",
    )?;
    Ok(())
}

#[test]
fn a_call_without_a_result_in_its_file_has_a_null_result() -> TestResult {
    let history = History::new("kinds/root")?;
    history.write(
        "-Users-alice-shop/eeee.jsonl",
        super::fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"e-1","sessionId":"eeee","timestamp":"2026-10-13T03:00:00Z","message":{"content":[{"type":"tool_use","id":"t-e1","name":"Bash","input":{"command":"sleep 100"}}]}}"#,
        ],
    )?;
    let calls = records(
        &history,
        &["records", "--session", "eeee", "--type", "call"],
    )?;
    check_eq(
        &calls.first().and_then(|call| call.get("result")),
        &Some(&Value::Null),
        "interrupted call",
    )?;
    Ok(())
}

#[test]
fn markdown_shows_a_result_under_its_call_and_not_again() -> TestResult {
    let history = History::new("kinds/root")?;
    let ran = history.run(&["records", "--type", "call,result", "--format", "md"]);
    check_eq(&ran.code, &0, &ran.err)?;
    check_eq(
        &ran.out
            .contains("```\n\n- Result: error\n\n```text\ntest failed: cart is empty\n```\n"),
        &true,
        &ran.out,
    )?;
    check_eq(&ran.out.matches(" - Tool call\n").count(), &2, &ran.out)?;
    // Only the result whose call is not in the file stands alone.
    check_eq(&ran.out.matches(" - Tool result\n").count(), &1, &ran.out)?;
    check_eq(
        &ran.out.contains("- Records: 3 (call 2, result 1)\n"),
        &true,
        &ran.out,
    )?;
    Ok(())
}

#[test]
fn markdown_says_when_a_call_has_no_result() -> TestResult {
    let history = History::new("kinds/root")?;
    history.write(
        "-Users-alice-shop/ffff.jsonl",
        super::fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"f-1","sessionId":"ffff","timestamp":"2026-10-13T03:00:00Z","message":{"content":[{"type":"tool_use","id":"t-f1","name":"Bash","input":{"command":"sleep 100"}}]}}"#,
        ],
    )?;
    let ran = history.run(&[
        "records",
        "--session",
        "ffff",
        "--type",
        "call",
        "--format",
        "md",
    ]);
    check_eq(
        &ran.out.ends_with("```\n\n- Result: (none)\n"),
        &true,
        &ran.out,
    )?;
    Ok(())
}

#[test]
fn tool_chars_cuts_the_result_a_call_carries() -> TestResult {
    let history = History::new("kinds/root")?;
    let calls = records(
        &history,
        &[
            "records",
            "--type",
            "call",
            "--tool",
            "Bash",
            "--tool-chars",
            "5",
        ],
    )?;
    check_eq(
        &calls
            .first()
            .and_then(|call| call.get("result"))
            .and_then(|result| result.get("text")),
        &Some(&json!("test \u{2026} (21 more characters, next: 6-10)")),
        "cut result",
    )?;
    Ok(())
}

#[test]
fn markdown_keeps_a_matched_result_whose_call_is_context() -> TestResult {
    let history = History::new("kinds/root")?;
    let ran = history.run(&[
        "records",
        "--type",
        "call,result",
        "--igrep",
        "cart is empty",
        "--around",
        "1",
        "--format",
        "md",
    ]);
    check_eq(&ran.code, &0, &ran.err)?;
    check_eq(
        &ran.out.contains("- Records: 1 (result 1) + "),
        &true,
        &ran.out,
    )?;
    check_eq(&ran.out.matches(" - Tool result\n").count(), &1, &ran.out)?;
    check_eq(&ran.out.matches("cart is empty").count(), &1, &ran.out)?;
    check_eq(
        &ran.out.contains(" - Tool call (context)\n"),
        &true,
        &ran.out,
    )?;
    Ok(())
}

#[test]
fn a_reply_the_harness_wrote_in_place_of_none_is_dropped() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-shop/ssss.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"s-1","sessionId":"ssss","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"go"}}"#,
            r#"{"type":"assistant","uuid":"s-2","sessionId":"ssss","timestamp":"2026-10-15T00:00:01Z","message":{"model":"<synthetic>","content":[{"type":"text","text":"No response requested."}]}}"#,
            r#"{"type":"assistant","uuid":"s-3","sessionId":"ssss","timestamp":"2026-10-15T00:00:02Z","message":{"model":"<synthetic>","content":[{"type":"text","text":"Partly written before the stop"}]}}"#,
            r#"{"type":"assistant","uuid":"s-4","sessionId":"ssss","timestamp":"2026-10-15T00:00:03Z","message":{"model":"claude-fable-5-2","content":[{"type":"text","text":"No response requested."}]}}"#,
        ],
    )?;
    let replies = records(
        &history,
        &["records", "--session", "ssss", "--type", "reply"],
    )?;
    check_eq(
        &strings(&replies, "text"),
        &vec!["Partly written before the stop", "No response requested."],
        "replies",
    )?;
    Ok(())
}

#[test]
fn every_record_names_the_turn_of_the_prompt_it_follows() -> TestResult {
    let history = History::new("root")?;
    history.write("-Users-alice-shop/nnnn.jsonl", fixture_time()?, &TURNS)?;
    let base = ["records", "--session", "nnnn"];
    let replies = records(&history, &[&base[..], &["--type", "reply"]].concat())?;
    check_eq(
        &turns(&replies),
        &vec![Some(0), Some(1), Some(1), Some(2)],
        "replies alone",
    )?;
    let all = records(
        &history,
        &[&base[..], &["--type", "prompt,reply,call,result"]].concat(),
    )?;
    check_eq(
        &strings(&all, "type"),
        &vec![
            "reply", "prompt", "reply", "call", "result", "reply", "prompt", "call", "result",
            "prompt", "reply",
        ],
        "types",
    )?;
    check_eq(
        &turns(&all),
        &vec![
            Some(0),
            Some(1),
            Some(1),
            Some(1),
            Some(1),
            Some(1),
            Some(2),
            Some(2),
            Some(2),
            Some(2),
            Some(2),
        ],
        "every kind",
    )?;
    let md = history
        .run(&[&base[..], &["--type", "reply", "--format", "md"]].concat())
        .out;
    check_eq(
        &md.contains("### 2026-10-15 09:00:08 JST - turn 2 - Reply\n"),
        &true,
        &md,
    )?;
    check_eq(
        &md.contains("### 2026-10-15 09:00:00 JST - Reply\n"),
        &true,
        &md,
    )?;
    Ok(())
}
