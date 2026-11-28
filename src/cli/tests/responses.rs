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

/// A session where the user approves one plan as Claude wrote it and one
/// they edited, with the result texts Claude Code writes.
const PLANS: [&str; 4] = [
    r##"{"type":"assistant","uuid":"x-1","sessionId":"xxxx","timestamp":"2026-10-15T00:00:00Z","message":{"content":[{"type":"tool_use","id":"t-x1","name":"ExitPlanMode","input":{"plan":"# Plan\n\n- step one\n- step two\n"}}]}}"##,
    r#"{"type":"user","uuid":"x-2","sessionId":"xxxx","timestamp":"2026-10-15T00:00:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-x1","content":"User has approved your plan. You can now start coding.\n\nYour plan has been saved to: /p.md\nYou can refer back to it if needed during implementation.\n\n## Approved Plan:\n# Plan\n\n- step one\n- step two\n"}]}}"#,
    r##"{"type":"assistant","uuid":"x-3","sessionId":"xxxx","timestamp":"2026-10-15T00:01:00Z","message":{"content":[{"type":"tool_use","id":"t-x2","name":"ExitPlanMode","input":{"plan":"# Plan\n\n- step one\n"}}]}}"##,
    r#"{"type":"user","uuid":"x-4","sessionId":"xxxx","timestamp":"2026-10-15T00:01:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-x2","content":"User has approved your plan. You can now start coding.\n\n## Approved Plan (edited by user):\n# Plan\n\n- step one, edited\n"}]}}"#,
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
            "typed-answer",
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
        &digest.contains("- (typed answer) `\"Which name?\"=\"ccdig\"`\n"),
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
    check_eq(&md.matches(" - Answer\n").count(), &1, &md)?;
    check_eq(&md.matches(" - Typed answer\n").count(), &1, &md)?;
    check_eq(&md.matches(" - Approval\n").count(), &2, &md)?;
    check_eq(&md.matches(" - Rejection\n").count(), &1, &md)?;
    Ok(())
}

#[test]
fn the_digest_drops_previews_and_folds_pastes_that_the_records_keep() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-plan/pppp.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"p-1","sessionId":"pppp","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"see <pasted_content id=\"a1\">\npasted words\n</pasted_content id=\"a1\"> here"}}"#,
            r#"{"type":"assistant","uuid":"p-2","sessionId":"pppp","timestamp":"2026-10-15T00:01:00Z","message":{"content":[{"type":"tool_use","id":"t-v1","name":"AskUserQuestion","input":{"questions":[]}}]}}"#,
            r#"{"type":"user","uuid":"p-3","sessionId":"pppp","timestamp":"2026-10-15T00:01:10Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-v1","content":"Your questions have been answered: \"Which install?\"=\"cargo\" selected preview:\ncargo install ccdig, then run it, \"Which scope?\"=\"digest\". You can now continue with these answers in mind."}]}}"#,
        ],
    )?;
    let digest = history.run(&["digest", "--session", "pppp"]).out;
    check_eq(
        &digest.contains("- `see [pasted 12 chars] here`\n"),
        &true,
        &digest,
    )?;
    check_eq(
        &digest
            .contains("- (answer) `\"Which install?\"=\"cargo\", \"Which scope?\"=\"digest\"`\n"),
        &true,
        &digest,
    )?;
    let prompts = json_lines(history.run(&["records", "--session", "pppp", "--type", "prompt"]))?;
    check_eq(
        &strings(&prompts, "text"),
        &vec![
            "see <pasted_content id=\"a1\">\npasted words\n</pasted_content id=\"a1\"> here",
            "\"Which install?\"=\"cargo\" selected preview:\ncargo install ccdig, then run it, \"Which scope?\"=\"digest\"",
        ],
        "texts",
    )?;
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
            vec!["answer", "approval", "approval"],
        ),
        (&["--kind", "typed-answer"][..], vec!["typed-answer"]),
        (
            &["--kind", "!approval"][..],
            vec!["answer", "typed-answer", "rejection", "text"],
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
        &vec!["approval", "rejection", "text", "typed-answer"],
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

#[test]
fn an_approved_plan_shows_once_unless_the_user_edited_it() -> TestResult {
    let history = History::new("root")?;
    history.write("-Users-alice-plan/xxxx.jsonl", fixture_time()?, &PLANS)?;
    let same = "User has approved your plan. You can now start coding.\n\nYour plan has been saved to: /p.md\nYou can refer back to it if needed during implementation.\n\n## Approved Plan:\n(the plan in the call's input)";
    let edited = "User has approved your plan. You can now start coding.\n\n## Approved Plan (edited by user):\n# Plan\n\n- step one, edited\n";
    let results = json_lines(history.run(&["records", "--session", "xxxx", "--type", "result"]))?;
    check_eq(&strings(&results, "text"), &vec![same, edited], "results")?;
    let calls = json_lines(history.run(&["records", "--session", "xxxx", "--type", "call"]))?;
    let carried: Vec<&str> = calls
        .iter()
        .filter_map(|call| call.pointer("/result/text").and_then(Value::as_str))
        .collect();
    check_eq(&carried, &vec![same, edited], "calls")?;
    Ok(())
}

#[test]
fn markdown_shows_a_plan_in_a_call_s_input_as_text() -> TestResult {
    let history = History::new("root")?;
    history.write("-Users-alice-plan/xxxx.jsonl", fixture_time()?, &PLANS)?;
    let md = history
        .run(&[
            "records",
            "--session",
            "xxxx",
            "--type",
            "call",
            "--format",
            "md",
        ])
        .out;
    check_eq(
        &md.contains(
            "- Tool: `ExitPlanMode`\n- `input.plan`\n\n```text\n# Plan\n\n- step one\n- step two\n```\n",
        ),
        &true,
        &md,
    )?;
    check_eq(&md.contains("\\n"), &false, &md)?;
    Ok(())
}

#[test]
fn the_digest_shows_the_first_line_of_the_reply_each_prompt_follows() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-plan/dddd.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"d-1","sessionId":"dddd","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"pick some"}}"#,
            r#"{"type":"assistant","uuid":"d-2","sessionId":"dddd","timestamp":"2026-10-15T00:00:01Z","message":{"model":"claude-fable-5-2","content":[{"type":"text","text":"Thinking"}]}}"#,
            r#"{"type":"assistant","uuid":"d-3","sessionId":"dddd","timestamp":"2026-10-15T00:00:02Z","message":{"model":"claude-fable-5-2","content":[{"type":"text","text":"\n\nHere are six for AKIAIOSFODNN7EXAMPLE:\n- one\n- two"}]}}"#,
            r#"{"type":"user","uuid":"d-4","sessionId":"dddd","timestamp":"2026-10-15T00:00:03Z","message":{"role":"user","content":"four of them"}}"#,
            r#"{"type":"assistant","uuid":"d-5","sessionId":"dddd","timestamp":"2026-10-15T00:00:04Z","message":{"model":"claude-fable-5-2","content":[{"type":"text","text":"Let me ask."},{"type":"tool_use","id":"t-d1","name":"AskUserQuestion","input":{"questions":[]}}]}}"#,
            r#"{"type":"user","uuid":"d-6","sessionId":"dddd","timestamp":"2026-10-15T00:00:05Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t-d1","content":"Your questions have been answered: \"Which?\"=\"one\". You can now continue with these answers in mind."}]}}"#,
            r#"{"type":"assistant","uuid":"d-7","sessionId":"dddd","timestamp":"2026-10-15T00:00:06Z","message":{"model":"<synthetic>","content":[{"type":"text","text":"No response requested."}]}}"#,
            r#"{"type":"user","uuid":"d-8","sessionId":"dddd","timestamp":"2026-10-15T00:00:07Z","message":{"role":"user","content":"thanks"}}"#,
        ],
    )?;
    let digest = history.run(&["digest", "--session", "dddd"]);
    check_eq(&digest.code, &0, &digest.err)?;
    let lines: Vec<&str> = digest.out.lines().skip(1).collect();
    check_eq(
        &lines,
        &vec![
            "- `pick some`",
            "- `four of them`",
            "  - after: `Here are six for ***REDACTED***:`",
            "- (answer) `\"Which?\"=\"one\"`",
            "  - after: `Let me ask.`",
            "- `thanks`",
        ],
        &digest.out,
    )?;
    Ok(())
}

#[test]
fn the_reply_a_prompt_follows_is_the_one_on_its_branch_and_not_a_sub_agent_s() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-plan/qqqq.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"q-1","sessionId":"qqqq","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"first"}}"#,
            r#"{"type":"assistant","uuid":"q-2","parentUuid":"q-1","sessionId":"qqqq","timestamp":"2026-10-15T00:00:01Z","message":{"model":"claude-fable-5-2","content":[{"type":"text","text":"Reply one"}]}}"#,
            r#"{"type":"user","uuid":"q-3","parentUuid":"q-2","sessionId":"qqqq","timestamp":"2026-10-15T00:00:02Z","message":{"role":"user","content":"second"}}"#,
            r#"{"type":"assistant","uuid":"q-4","parentUuid":"q-3","sessionId":"qqqq","timestamp":"2026-10-15T00:00:03Z","message":{"model":"claude-fable-5-2","content":[{"type":"text","text":"Reply two"}]}}"#,
            r#"{"type":"assistant","uuid":"q-5","parentUuid":"q-4","isSidechain":true,"sessionId":"qqqq","timestamp":"2026-10-15T00:00:04Z","message":{"model":"claude-fable-5-2","content":[{"type":"text","text":"A sub-agent's words"}]}}"#,
            r#"{"type":"user","uuid":"q-6","parentUuid":"q-4","sessionId":"qqqq","timestamp":"2026-10-15T00:00:05Z","message":{"role":"user","content":"third"}}"#,
            r#"{"type":"user","uuid":"q-7","parentUuid":"q-2","sessionId":"qqqq","timestamp":"2026-10-15T00:00:06Z","message":{"role":"user","content":"second, again"}}"#,
        ],
    )?;
    let digest = history.run(&["digest", "--session", "qqqq"]);
    check_eq(&digest.code, &0, &digest.err)?;
    let lines: Vec<&str> = digest.out.lines().skip(1).collect();
    check_eq(
        &lines,
        &vec![
            "- `first`",
            "- `second`",
            "  - after: `Reply one`",
            "- `third`",
            "  - after: `Reply two`",
            "- `second, again`",
            "  - after: `Reply one`",
        ],
        &digest.out,
    )?;
    Ok(())
}
