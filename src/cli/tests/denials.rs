//! Tests for tool calls the user turned down and the plans they were shown,
//! against synthetic history.

use core::error::Error;

use serde_json::{Value, json};

use super::{History, fixture_time, json_lines, strings};
use crate::testing::{TestResult, check_eq};

/// The text Claude Code returns when the user turns a call down with a
/// comment, as a JSON string body.
const SAID: &str = r"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:\nOne PR, please.";

/// The JSON Lines `args` print for the session `dddd`, parsed.
fn records(history: &History, args: &[&str]) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut full = args.to_vec();
    full.extend(["--session", "dddd"]);
    json_lines(history.run(&full))
}

/// A history with the session `dddd` holding `lines`.
fn history(lines: &[&str]) -> Result<History, Box<dyn Error>> {
    let history = History::new("root")?;
    history.write("-Users-alice-plan/dddd.jsonl", fixture_time()?, lines)?;
    Ok(history)
}

#[test]
fn a_call_keeps_the_recorded_input_the_wire_input_leaves_out() -> TestResult {
    // Claude Code records the plan in the input, while the model sent none.
    let history = history(&[
        r##"{"type":"assistant","uuid":"d-1","sessionId":"dddd","timestamp":"2026-10-15T00:00:00Z","wireToolInputs":{"toolu_p":{},"toolu_b":{"command":"cargo test --all"}},"message":{"content":[{"type":"tool_use","id":"toolu_p","name":"ExitPlanMode","input":{"plan":"# Plan","planFilePath":"/p.md"}},{"type":"tool_use","id":"toolu_b","name":"Bash","input":{"command":"cargo te..."}}]}}"##,
    ])?;
    let calls = records(&history, &["records", "--type", "call"])?;
    let inputs: Vec<Option<&Value>> = calls.iter().map(|call| call.get("input")).collect();
    check_eq(
        &inputs,
        &vec![
            Some(&json!({"plan": "# Plan", "planFilePath": "/p.md"})),
            Some(&json!({"command": "cargo test --all"})),
        ],
        "inputs",
    )?;
    Ok(())
}

/// A history where the user turns a plan down with a comment, a command
/// fails, a permission rule blocks another, and a read succeeds.
fn history_with_denials() -> Result<History, Box<dyn Error>> {
    let rejected = format!(
        r#"{{"type":"user","uuid":"d-2","parentUuid":"d-1","sessionId":"dddd","timestamp":"2026-10-15T00:00:10Z","toolDenialKind":"user-rejected","userFeedback":"One PR, please.","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_p","is_error":true,"content":"{SAID}"}}]}}}}"#
    );
    history(&[
        r##"{"type":"assistant","uuid":"d-1","sessionId":"dddd","timestamp":"2026-10-15T00:00:00Z","message":{"content":[{"type":"tool_use","id":"toolu_p","name":"ExitPlanMode","input":{"plan":"# Plan"}}]}}"##,
        &rejected,
        r#"{"type":"assistant","uuid":"d-3","sessionId":"dddd","timestamp":"2026-10-15T00:01:00Z","message":{"content":[{"type":"tool_use","id":"toolu_f","name":"Bash","input":{"command":"false"}},{"type":"tool_use","id":"toolu_r","name":"Bash","input":{"command":"rm x"}},{"type":"tool_use","id":"toolu_ok","name":"Read","input":{"file_path":"/a"}}]}}"#,
        r#"{"type":"user","uuid":"d-4","sessionId":"dddd","timestamp":"2026-10-15T00:01:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_f","is_error":true,"content":"Error: Exit code 1"}]}}"#,
        r#"{"type":"user","uuid":"d-5","sessionId":"dddd","timestamp":"2026-10-15T00:01:11Z","toolDenialKind":"permission-rule","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_r","is_error":true,"content":"Permission to use Bash has been denied."}]}}"#,
        r#"{"type":"user","uuid":"d-6","sessionId":"dddd","timestamp":"2026-10-15T00:01:12Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_ok","content":"a"}]}}"#,
    ])
}

#[test]
fn a_result_names_why_its_call_did_not_run() -> TestResult {
    let history = history_with_denials()?;
    let results = records(&history, &["records", "--type", "result"])?;
    check_eq(
        &strings(&results, "denial"),
        &vec!["user-rejected", "", "permission-rule", ""],
        "denials",
    )?;
    check_eq(
        &results
            .iter()
            .map(|result| result.get("is_error").and_then(Value::as_bool))
            .collect::<Vec<_>>(),
        &vec![Some(true), Some(true), Some(true), Some(false)],
        "is_error stays",
    )?;
    Ok(())
}

#[test]
fn denial_picks_the_results_by_why_the_call_did_not_run() -> TestResult {
    let history = history_with_denials()?;
    for (args, want) in [
        (&["--denial", "user-rejected"][..], vec!["toolu_p"]),
        (
            &["--denial", "user-rejected", "--denial", "permission-rule"][..],
            vec!["toolu_p", "toolu_r"],
        ),
        (
            &["--denial", "!user-rejected", "--status", "error"][..],
            vec!["toolu_f", "toolu_r"],
        ),
        // Without --type, a result flag picks results alone.
        (
            &["--denial", "user-rejected", "--tool", "ExitPlanMode"][..],
            vec!["toolu_p"],
        ),
    ] {
        let mut full = vec!["records"];
        full.extend_from_slice(args);
        check_eq(
            &strings(&records(&history, &full)?, "tool_use_id"),
            &want,
            &args.join(" "),
        )?;
    }
    Ok(())
}

#[test]
fn markdown_names_why_a_call_did_not_run() -> TestResult {
    let ran = history_with_denials()?.run(&[
        "records",
        "--type",
        "result",
        "--session",
        "dddd",
        "--format",
        "md",
    ]);
    check_eq(&ran.code, &0, "exit")?;
    check_eq(
        &ran.out.matches("- Denial: ").count(),
        &2,
        "one line per denied call",
    )?;
    check_eq(
        &ran.out.contains("- Denial: `user-rejected`\n"),
        &true,
        "the kind",
    )?;
    Ok(())
}

#[test]
fn a_comment_on_a_turned_down_call_is_a_prompt() -> TestResult {
    let history = history_with_denials()?;
    let prompts = records(&history, &["records", "--type", "prompt"])?;
    check_eq(&strings(&prompts, "text"), &vec!["One PR, please."], "text")?;
    check_eq(&strings(&prompts, "kind"), &vec!["rejection"], "kind")?;
    check_eq(&strings(&prompts, "command"), &vec![""], "command")?;
    // The result of the same line stays.
    let both = records(&history, &["records", "--type", "prompt,result"])?;
    check_eq(
        &strings(&both, "type"),
        &vec!["result", "prompt", "result", "result", "result"],
        "prompt and result",
    )?;
    check_eq(
        &both
            .iter()
            .take(2)
            .map(|record| record.pointer("/source/uuid").and_then(Value::as_str))
            .collect::<Vec<_>>(),
        &vec![Some("d-2"), Some("d-2")],
        "one line",
    )?;
    Ok(())
}

#[test]
fn a_comment_recorded_before_claude_code_named_the_denial_is_a_prompt() -> TestResult {
    let line = format!(
        r#"{{"type":"user","uuid":"d-2","sessionId":"dddd","timestamp":"2026-10-15T00:00:10Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_p","is_error":true,"content":"{SAID}"}}]}}}}"#
    );
    let history = history(&[&line])?;
    let prompts = records(&history, &["records", "--type", "prompt"])?;
    check_eq(&strings(&prompts, "text"), &vec!["One PR, please."], "text")?;
    Ok(())
}

#[test]
fn markdown_and_the_digest_tell_a_comment_on_a_turned_down_call() -> TestResult {
    let history = history_with_denials()?;
    let markdown = history.run(&[
        "records",
        "--type",
        "prompt",
        "--session",
        "dddd",
        "--format",
        "md",
    ]);
    check_eq(&markdown.code, &0, "markdown exit")?;
    check_eq(
        &(
            markdown.out.contains(" Rejection"),
            markdown.out.contains(" Prompt"),
        ),
        &(true, false),
        "markdown heading",
    )?;
    let digest = history.run_week(&["digest", "--session", "dddd"]);
    check_eq(&digest.code, &0, "digest exit")?;
    check_eq(
        &digest.out.contains("- (rejection) `One PR, please.`\n"),
        &true,
        "digest line",
    )?;
    Ok(())
}

#[test]
fn mask_values_mask_the_denial_too() -> TestResult {
    let history = history_with_denials()?;
    let results = records(
        &history,
        &[
            "records",
            "--denial",
            "permission-rule",
            "--mask-values",
            r#"{"rule": ["permission-rule"]}"#,
        ],
    )?;
    check_eq(
        &strings(&results, "denial"),
        &vec!["\u{FF3B}rule1\u{FF3D}"],
        "denial",
    )?;
    Ok(())
}
