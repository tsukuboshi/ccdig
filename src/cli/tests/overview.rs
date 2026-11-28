//! Tests for what `sessions` tells of each session at a glance: the first
//! prompt and the models that answered.

use core::error::Error;

use serde_json::Value;

use super::{History, fixture_time, the_session};
use crate::testing::{TestResult, check_eq};

#[test]
fn models_lists_each_model_that_answered_once_in_the_order_it_first_answered() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/oooo.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"o-1","sessionId":"oooo","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"count the posts"}}"#,
            r#"{"type":"assistant","uuid":"o-2","sessionId":"oooo","timestamp":"2026-10-15T00:00:10Z","message":{"id":"msg_1","model":"claude-haiku-5-5","content":[{"type":"text","text":"Counting."}],"usage":{"input_tokens":1,"output_tokens":1}}}"#,
            r#"{"type":"assistant","uuid":"o-3","sessionId":"oooo","timestamp":"2026-10-15T00:00:11Z","message":{"id":"msg_1","model":"claude-haiku-5-5","content":[{"type":"tool_use","id":"toolu_o1","name":"Bash","input":{"command":"ls"}}],"usage":{"input_tokens":1,"output_tokens":1}}}"#,
            r#"{"type":"assistant","uuid":"o-4","sessionId":"oooo","timestamp":"2026-10-15T00:00:20Z","message":{"id":"msg_2","model":"claude-fable-5-2","content":[{"type":"text","text":"Three."}],"usage":{"input_tokens":1,"output_tokens":1}}}"#,
            r#"{"type":"assistant","uuid":"o-5","sessionId":"oooo","timestamp":"2026-10-15T00:00:30Z","message":{"id":"msg_3","model":"claude-haiku-5-5","content":[{"type":"text","text":"Done."}],"usage":{"input_tokens":1,"output_tokens":1}}}"#,
        ],
    )?;
    let session = the_session(&history, "oooo", &[])?;
    check_eq(
        &session.get("models"),
        &Some(&serde_json::json!(["claude-haiku-5-5", "claude-fable-5-2"])),
        &session.to_string(),
    )?;
    Ok(())
}

#[test]
fn a_session_without_a_reply_has_no_models() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/nnnn.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"n-1","sessionId":"nnnn","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"hello"}}"#,
        ],
    )?;
    let session = the_session(&history, "nnnn", &[])?;
    check_eq(
        &session.get("models"),
        &Some(&serde_json::json!([])),
        &session.to_string(),
    )?;
    Ok(())
}

/// Adds session `ffff` to `history`, whose prompts are `prompts` in order,
/// a minute apart.
fn with_prompts(history: &History, prompts: &[&str]) -> Result<(), Box<dyn Error>> {
    let lines: Vec<String> = prompts
        .iter()
        .enumerate()
        .map(|(index, text)| {
            serde_json::json!({
                "type": "user",
                "uuid": format!("f-{index}"),
                "sessionId": "ffff",
                "timestamp": format!("2026-10-15T00:0{index}:00Z"),
                "message": {"role": "user", "content": text},
            })
            .to_string()
        })
        .collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    history.write("-Users-alice-blog/ffff.jsonl", fixture_time()?, &refs)?;
    Ok(())
}

#[test]
fn first_prompt_is_the_first_selected_prompt_on_one_line() -> TestResult {
    let history = History::new("root")?;
    with_prompts(&history, &["fix the login\npage, please", "and the tests"])?;
    let session = the_session(&history, "ffff", &[])?;
    check_eq(
        &session.get("first_prompt"),
        &Some(&Value::from("fix the login page, please")),
        &session.to_string(),
    )?;
    let later = the_session(&history, "ffff", &["--grep", "!login"])?;
    check_eq(
        &later.get("first_prompt"),
        &Some(&Value::from("and the tests")),
        &format!("a pattern with a leading ! leaves out the first: {later}"),
    )?;
    Ok(())
}

#[test]
fn first_prompt_is_masked_and_cut_to_200_characters() -> TestResult {
    let history = History::new("root")?;
    let key_id = format!("AKIA{}", "Q".repeat(16));
    let long = format!("rotate {key_id} {}", "x".repeat(300));
    with_prompts(&history, &[&long])?;
    let session = the_session(&history, "ffff", &[])?;
    let first = session
        .get("first_prompt")
        .and_then(Value::as_str)
        .unwrap_or_default();
    check_eq(&first.contains(&key_id), &false, first)?;
    check_eq(&first.starts_with("rotate ***REDACTED*** x"), &true, first)?;
    check_eq(&first.ends_with(" more characters)"), &true, first)?;
    check_eq(
        &first
            .split('\u{2026}')
            .next()
            .map(|kept| kept.chars().count()),
        &Some(200),
        first,
    )?;
    Ok(())
}

#[test]
fn models_leave_out_the_replies_claude_code_wrote_itself() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/ssss.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"s-1","sessionId":"ssss","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"count the posts"}}"#,
            r#"{"type":"assistant","uuid":"s-2","sessionId":"ssss","timestamp":"2026-10-15T00:00:10Z","message":{"id":"msg_1","model":"<synthetic>","content":[{"type":"text","text":"API Error: overloaded"}],"usage":{"input_tokens":0,"output_tokens":0}}}"#,
            r#"{"type":"assistant","uuid":"s-3","sessionId":"ssss","timestamp":"2026-10-15T00:00:20Z","message":{"id":"msg_2","model":"claude-fable-5-2","content":[{"type":"text","text":"Three."}],"usage":{"input_tokens":1,"output_tokens":1}}}"#,
        ],
    )?;
    let session = the_session(&history, "ssss", &[])?;
    check_eq(
        &session.get("models"),
        &Some(&serde_json::json!(["claude-fable-5-2"])),
        &session.to_string(),
    )?;
    Ok(())
}

#[test]
fn models_are_masked_as_the_model_of_a_record_is_and_counted() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/kkkk.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"k-1","sessionId":"kkkk","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"count the posts"}}"#,
            r#"{"type":"assistant","uuid":"k-2","sessionId":"kkkk","timestamp":"2026-10-15T00:00:10Z","message":{"id":"msg_1","model":"acme-proxy-model","content":[{"type":"text","text":"Three."}],"usage":{"input_tokens":1,"output_tokens":1}}}"#,
        ],
    )?;
    let values = ["--mask-values", r#"{"org": ["acme"]}"#];
    let session = the_session(&history, "kkkk", &values)?;
    let models = session
        .get("models")
        .map_or_else(String::new, Value::to_string);
    check_eq(&models.contains("acme"), &false, &models)?;
    let mut args = vec!["sessions", "--session", "kkkk", "--mask-report", "text"];
    args.extend_from_slice(&values);
    let ran = history.run_week(&args);
    check_eq(
        &ran.err.contains("1 by values/org"),
        &true,
        &format!("the report counts the model: {}", ran.err),
    )?;
    Ok(())
}
