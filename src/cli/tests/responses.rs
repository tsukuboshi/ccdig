//! Tests for what the user answered through a dialog, a question's answers
//! and a plan approved or turned down, which are prompts too, against
//! synthetic history.

use core::error::Error;

use serde_json::Value;

use super::{History, fixture_time, json_lines, strings};
use crate::testing::{TestResult, check_eq};

/// The lines of a session where the user answers two questions in both of
/// the texts Claude Code writes, approves a plan in both, turns a plan and
/// a question down without a comment, and a tool runs.
const LINES: [&str; 14] = [
    r#"{"type":"assistant","uuid":"r-1","sessionId":"rrrr","timestamp":"2026-10-15T00:00:00Z","message":{"content":[{"type":"tool_use","id":"t-q1","name":"AskUserQuestion","input":{"questions":[]}}]}}"#,
    r#"{"type":"user","uuid":"r-2","sessionId":"rrrr","timestamp":"2026-10-15T00:00:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-q1","content":"Your questions have been answered: \"Which branch?\"=\"main\", \"How many commits?\"=\"one\". You can now continue with these answers in mind."}]}}"#,
    r#"{"type":"assistant","uuid":"r-3","sessionId":"rrrr","timestamp":"2026-10-15T00:01:00Z","message":{"content":[{"type":"tool_use","id":"t-q2","name":"AskUserQuestion","input":{"questions":[]}}]}}"#,
    r#"{"type":"user","uuid":"r-4","sessionId":"rrrr","timestamp":"2026-10-15T00:01:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-q2","content":"The user answered: \"Which name?\"=\"ccdig\". Read the answers carefully \u2014 they may request clarification, changes, or that you not proceed \u2014 and follow what they actually say."}]}}"#,
    r#"{"type":"assistant","uuid":"r-5","sessionId":"rrrr","timestamp":"2026-10-15T00:02:00Z","message":{"content":[{"type":"tool_use","id":"t-p1","name":"ExitPlanMode","input":{"plan":"Plan"}}]}}"#,
    r#"{"type":"user","uuid":"r-6","sessionId":"rrrr","timestamp":"2026-10-15T00:02:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-p1","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed."}]},"toolDenialKind":"user-rejected"}"#,
    r#"{"type":"assistant","uuid":"r-7","sessionId":"rrrr","timestamp":"2026-10-15T00:03:00Z","message":{"content":[{"type":"tool_use","id":"t-p2","name":"ExitPlanMode","input":{"plan":"Plan"}}]}}"#,
    r#"{"type":"user","uuid":"r-8","sessionId":"rrrr","timestamp":"2026-10-15T00:03:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-p2","content":"User has approved your plan. You can now start coding.\n\nApproved Plan:\nPlan"}]}}"#,
    r#"{"type":"assistant","uuid":"r-9","sessionId":"rrrr","timestamp":"2026-10-15T00:04:00Z","message":{"content":[{"type":"tool_use","id":"t-p3","name":"ExitPlanMode","input":{}}]}}"#,
    r#"{"type":"user","uuid":"r-10","sessionId":"rrrr","timestamp":"2026-10-15T00:04:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-p3","content":"User has approved exiting plan mode. You can now proceed."}]}}"#,
    r#"{"type":"assistant","uuid":"r-11","sessionId":"rrrr","timestamp":"2026-10-15T00:05:00Z","message":{"content":[{"type":"tool_use","id":"t-q3","name":"AskUserQuestion","input":{"questions":[]}},{"type":"tool_use","id":"t-b1","name":"Bash","input":{"command":"ls"}}]}}"#,
    r#"{"type":"user","uuid":"r-12","sessionId":"rrrr","timestamp":"2026-10-15T00:05:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-q3","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed."}]},"toolDenialKind":"user-rejected"}"#,
    r#"{"type":"user","uuid":"r-13","sessionId":"rrrr","timestamp":"2026-10-15T00:05:20Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-b1","content":"Cargo.toml"}]}}"#,
    r#"{"type":"user","uuid":"r-14","sessionId":"rrrr","timestamp":"2026-10-15T00:06:00Z","message":{"role":"user","content":"thanks"}}"#,
];

/// A history with the session `rrrr` holding `LINES`.
fn history() -> Result<History, Box<dyn Error>> {
    let history = History::new("root")?;
    history.write("-Users-alice-plan/rrrr.jsonl", fixture_time()?, &LINES)?;
    Ok(history)
}

#[test]
fn answers_and_plan_approvals_and_rejections_are_prompts() -> TestResult {
    let history = history()?;
    let prompts = json_lines(history.run(&["records", "--session", "rrrr", "--type", "prompt"]))?;
    check_eq(
        &strings(&prompts, "kind"),
        &vec![
            "answer",
            "answer",
            "rejection",
            "approval",
            "approval",
            "text",
        ],
        "kinds",
    )?;
    check_eq(
        &strings(&prompts, "text"),
        &vec![
            r#""Which branch?"="main", "How many commits?"="one""#,
            r#""Which name?"="ccdig""#,
            "The user doesn't want to proceed with this tool use.",
            "User has approved your plan.",
            "User has approved exiting plan mode.",
            "thanks",
        ],
        "texts",
    )?;
    Ok(())
}

#[test]
fn sessions_and_the_digest_count_the_answers_as_prompts() -> TestResult {
    let history = history()?;
    let sessions: Vec<Value> = json_lines(history.run(&["sessions", "--session", "rrrr"]))?;
    check_eq(
        &sessions
            .first()
            .and_then(|session| session.get("prompt_count"))
            .and_then(Value::as_u64),
        &Some(6),
        "prompt count",
    )?;
    let digest = history.run(&["digest", "--session", "rrrr"]).out;
    check_eq(
        &digest.contains("- (answer) `\"Which name?\"=\"ccdig\"`\n"),
        &true,
        &digest,
    )?;
    check_eq(
        &digest.contains("- (approval) `User has approved your plan.`\n"),
        &true,
        &digest,
    )?;
    let md = history
        .run(&[
            "records",
            "--session",
            "rrrr",
            "--type",
            "prompt",
            "--format",
            "md",
        ])
        .out;
    check_eq(&md.matches(" - Answer\n").count(), &2, &md)?;
    check_eq(&md.matches(" - Approval\n").count(), &2, &md)?;
    check_eq(&md.matches(" - Rejection\n").count(), &1, &md)?;
    Ok(())
}

#[test]
fn answers_in_a_text_claude_code_has_not_written_are_kept_whole() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-plan/ssss.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"s-1","sessionId":"ssss","timestamp":"2026-10-15T00:00:00Z","message":{"content":[{"type":"tool_use","id":"t-s1","name":"AskUserQuestion","input":{"questions":[]}}]}}"#,
            r#"{"type":"user","uuid":"s-2","sessionId":"ssss","timestamp":"2026-10-15T00:00:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-s1","content":"  Picked: main  "}]}}"#,
        ],
    )?;
    let prompts = json_lines(history.run(&["records", "--session", "ssss", "--type", "prompt"]))?;
    check_eq(&strings(&prompts, "text"), &vec!["Picked: main"], "texts")?;
    check_eq(&strings(&prompts, "kind"), &vec!["answer"], "kinds")?;
    Ok(())
}

#[test]
fn an_answer_beside_a_comment_on_another_call_is_a_prompt_too() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-plan/pppp.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"p-1","sessionId":"pppp","timestamp":"2026-10-15T00:00:00Z","message":{"content":[{"type":"tool_use","id":"t-a","name":"Bash","input":{"command":"rm -r build"}},{"type":"tool_use","id":"t-q","name":"AskUserQuestion","input":{"questions":[]}}]}}"#,
            r#"{"type":"user","uuid":"p-2","sessionId":"pppp","timestamp":"2026-10-15T00:00:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-a","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:\nkeep the build"},{"type":"tool_result","tool_use_id":"t-q","content":"Your questions have been answered: \"Which?\"=\"this\". You can now continue with these answers in mind."}]}}"#,
        ],
    )?;
    let prompts = json_lines(history.run(&["records", "--session", "pppp", "--type", "prompt"]))?;
    check_eq(
        &strings(&prompts, "kind"),
        &vec!["rejection", "answer"],
        "kinds",
    )?;
    check_eq(
        &strings(&prompts, "text"),
        &vec!["keep the build", r#""Which?"="this""#],
        "texts",
    )?;
    Ok(())
}

#[test]
fn kind_picks_the_prompts_by_how_they_were_typed() -> TestResult {
    let history = history()?;
    for (args, want) in [
        (&["--kind", "rejection"][..], vec!["rejection"]),
        (
            &["--kind", "answer", "--kind", "approval"][..],
            vec!["answer", "answer", "approval", "approval"],
        ),
        (
            &["--kind", "!approval"][..],
            vec!["answer", "answer", "rejection", "text"],
        ),
        // Without --type, --kind picks prompts alone.
        (
            &["--kind", "text", "--type", "prompt,reply"][..],
            vec!["text"],
        ),
    ] {
        let mut full = vec!["records", "--session", "rrrr"];
        full.extend_from_slice(args);
        check_eq(
            &strings(&json_lines(history.run(&full))?, "kind"),
            &want,
            &args.join(" "),
        )?;
    }
    let all = json_lines(history.run(&["records", "--session", "rrrr", "--kind", "text"]))?;
    check_eq(
        &strings(&all, "type"),
        &vec!["prompt"],
        "the type --kind picks",
    )?;
    Ok(())
}

#[test]
fn stats_and_the_digest_take_kind_too() -> TestResult {
    let history = history()?;
    let groups = json_lines(history.run(&[
        "stats",
        "--session",
        "rrrr",
        "--kind",
        "!answer",
        "--by",
        "kind",
    ]))?;
    check_eq(
        &strings(&groups, "kind"),
        &vec!["approval", "rejection", "text"],
        "stats",
    )?;
    let digest = history.run(&["digest", "--session", "rrrr", "--kind", "rejection"]);
    check_eq(&digest.code, &0, &digest.err)?;
    check_eq(
        &(digest.out.contains("(rejection)") && !digest.out.contains("thanks")),
        &true,
        &digest.out,
    )?;
    Ok(())
}

#[test]
fn kind_without_prompts_is_an_error() -> TestResult {
    let history = history()?;
    let ran = history.run(&["records", "--type", "reply", "--kind", "text"]);
    check_eq(
        &ran.err.as_str(),
        &"ccdig: --kind needs --type prompt\n",
        "records",
    )?;
    Ok(())
}
