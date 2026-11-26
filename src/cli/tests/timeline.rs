//! Tests for how a session changes as it goes on: prompts typed again in
//! place of earlier ones, and titles that change, against synthetic history.

use core::error::Error;

use serde_json::Value;

use super::{History, fixture_time, json_lines};
use crate::testing::{TestResult, check_eq};

/// The JSON Lines `args` print for the session `eeee`, parsed.
fn records(history: &History, args: &[&str]) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut full = args.to_vec();
    full.extend(["--session", "eeee", "--since", "2026-10-13", "--tz", "UTC"]);
    json_lines(history.run(&full))
}

/// The boolean at `key` of each record.
fn flags(records: &[Value], key: &str) -> Vec<Option<bool>> {
    records
        .iter()
        .map(|record| record.get(key).and_then(Value::as_bool))
        .collect()
}

/// A history where the user sends a prompt again before any reply, and
/// later interrupts a reply and sends the prompt it answered again, the
/// next day.
fn history_with_prompts_sent_again() -> Result<History, Box<dyn Error>> {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-e/eeee.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"p-0","parentUuid":null,"sessionId":"eeee","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"start"}}"#,
            r#"{"type":"assistant","uuid":"a-0","parentUuid":"p-0","sessionId":"eeee","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"text","text":"Ready."}]}}"#,
            r#"{"type":"user","uuid":"p-1","parentUuid":"a-0","sessionId":"eeee","timestamp":"2026-10-15T00:01:00Z","message":{"role":"user","content":"draft one"}}"#,
            r#"{"type":"user","uuid":"p-2","parentUuid":"a-0","sessionId":"eeee","timestamp":"2026-10-15T00:01:05Z","message":{"role":"user","content":"draft two"}}"#,
            r#"{"type":"assistant","uuid":"a-2","parentUuid":"p-2","sessionId":"eeee","timestamp":"2026-10-15T00:01:10Z","message":{"content":[{"type":"text","text":"Drafted."}]}}"#,
            r#"{"type":"user","uuid":"p-3","parentUuid":"a-2","sessionId":"eeee","timestamp":"2026-10-15T00:02:00Z","message":{"role":"user","content":"go"}}"#,
            r#"{"type":"assistant","uuid":"a-3","parentUuid":"p-3","sessionId":"eeee","timestamp":"2026-10-15T00:02:10Z","message":{"content":[{"type":"text","text":"Working."}]}}"#,
            r#"{"type":"user","uuid":"i-1","parentUuid":"a-3","sessionId":"eeee","timestamp":"2026-10-15T00:02:20Z","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#,
            r#"{"type":"user","uuid":"p-4","parentUuid":"a-2","sessionId":"eeee","timestamp":"2026-10-16T00:00:00Z","message":{"role":"user","content":"go, but carefully"}}"#,
        ],
    )?;
    Ok(history)
}

#[test]
fn a_prompt_sent_again_in_its_place_supersedes_it() -> TestResult {
    let history = history_with_prompts_sent_again()?;
    let prompts = records(&history, &["records", "--type", "prompt"])?;
    check_eq(
        &flags(&prompts, "superseded"),
        &vec![
            Some(false),
            Some(true),
            Some(false),
            Some(true),
            Some(false),
        ],
        "all",
    )?;
    // The prompt sent again the next day is out of the range, and still
    // supersedes the one it replaced.
    let first_day = records(
        &history,
        &["records", "--type", "prompt", "--until", "2026-10-15"],
    )?;
    check_eq(
        &flags(&first_day, "superseded"),
        &vec![Some(false), Some(true), Some(false), Some(true)],
        "first day",
    )?;
    Ok(())
}

#[test]
fn a_record_carries_the_title_the_session_had_when_it_was_written() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-e/eeee.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"t-1","sessionId":"eeee","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"fix the build"}}"#,
            r#"{"type":"ai-title","sessionId":"eeee","aiTitle":"Fix the build"}"#,
            r#"{"type":"user","uuid":"t-2","sessionId":"eeee","timestamp":"2026-10-15T00:01:00Z","message":{"role":"user","content":"now the docs"}}"#,
            r#"{"type":"ai-title","sessionId":"eeee","aiTitle":"Fix the build"}"#,
            r#"{"type":"ai-title","sessionId":"eeee","aiTitle":"Update docs for alice@example.com"}"#,
            r#"{"type":"user","uuid":"t-3","sessionId":"eeee","timestamp":"2026-10-15T00:02:00Z","message":{"role":"user","content":"thanks"}}"#,
        ],
    )?;
    let prompts = records(
        &history,
        &["records", "--type", "prompt", "--mask", "email"],
    )?;
    let titles = |key: &str| -> Vec<Option<&str>> {
        prompts
            .iter()
            .map(|prompt| prompt.get(key).and_then(Value::as_str))
            .collect()
    };
    check_eq(
        &titles("session_title_then"),
        &vec![
            Some(""),
            Some("Fix the build"),
            Some("Update docs for ***REDACTED***"),
        ],
        "then",
    )?;
    check_eq(
        &titles("session_title"),
        &vec![Some("Update docs for ***REDACTED***"); 3],
        "last",
    )?;
    Ok(())
}

#[test]
fn the_mask_report_counts_what_masking_the_title_then_hid() -> TestResult {
    let history = History::new("root")?;
    let key_id = format!("AKIA{}", "Q".repeat(16));
    let secret_title =
        format!(r#"{{"type":"ai-title","sessionId":"eeee","aiTitle":"Rotate {key_id} key"}}"#);
    history.write(
        "-Users-alice-e/eeee.jsonl",
        fixture_time()?,
        &[
            &secret_title,
            r#"{"type":"user","uuid":"t-1","sessionId":"eeee","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"rotate it"}}"#,
            r#"{"type":"ai-title","sessionId":"eeee","aiTitle":"Plain title"}"#,
        ],
    )?;
    let ran = history.run(&[
        "records",
        "--type",
        "prompt",
        "--session",
        "eeee",
        "--since",
        "2026-10-13",
        "--tz",
        "UTC",
        "--mask-report",
        "text",
    ]);
    check_eq(&ran.code, &0, &ran.err)?;
    check_eq(
        &ran.out
            .contains(r#""session_title_then":"Rotate ***REDACTED*** key""#),
        &true,
        "title then",
    )?;
    check_eq(&ran.err.starts_with("ccdig: masked 1 by "), &true, &ran.err)?;
    Ok(())
}

/// The text of each record.
fn texts(records: &[Value]) -> Vec<&str> {
    records
        .iter()
        .map(|record| {
            record
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
        })
        .collect()
}

#[test]
fn superseded_picks_the_prompts_sent_again_or_the_others() -> TestResult {
    let history = history_with_prompts_sent_again()?;
    for (args, want) in [
        (
            &["--type", "prompt", "--superseded", "false"][..],
            vec!["start", "draft two", "go, but carefully"],
        ),
        (
            &["--type", "prompt", "--superseded", "true"][..],
            vec!["draft one", "go"],
        ),
        // Without --type, --superseded picks prompts alone.
        (&["--superseded", "true"][..], vec!["draft one", "go"]),
    ] {
        let mut full = vec!["records"];
        full.extend_from_slice(args);
        check_eq(&texts(&records(&history, &full)?), &want, &args.join(" "))?;
    }
    Ok(())
}

#[test]
fn the_digest_and_stats_leave_out_the_prompts_sent_again() -> TestResult {
    let history = history_with_prompts_sent_again()?;
    let digest = history.run(&[
        "digest",
        "--session",
        "eeee",
        "--since",
        "2026-10-13",
        "--superseded",
        "false",
    ]);
    check_eq(&digest.code, &0, &digest.err)?;
    check_eq(
        &(digest.out.contains("`draft two`") && !digest.out.contains("`draft one`")),
        &true,
        &digest.out,
    )?;
    let groups = records(&history, &["stats", "--superseded", "true"])?;
    check_eq(
        &groups
            .iter()
            .map(|group| group.get("count").and_then(Value::as_u64))
            .collect::<Vec<_>>(),
        &vec![Some(2)],
        "stats",
    )?;
    Ok(())
}

#[test]
fn superseded_needs_prompts_and_no_around() -> TestResult {
    let history = history_with_prompts_sent_again()?;
    for (args, want) in [
        (
            &["records", "--type", "reply", "--superseded", "true"][..],
            "ccdig: --superseded needs --type prompt\n",
        ),
        // Whether a prompt was sent again is known only at the end of its
        // file, after --around has kept the records next to it.
        (
            &[
                "records",
                "--igrep",
                "go",
                "--around",
                "1",
                "--superseded",
                "false",
            ][..],
            "ccdig: --superseded cannot be used with --around\n",
        ),
    ] {
        check_eq(&history.run(args).err.as_str(), &want, &args.join(" "))?;
    }
    // --around 0 keeps nothing next to a match.
    let none_around = records(
        &history,
        &[
            "records",
            "--igrep",
            "go",
            "--around",
            "0",
            "--superseded",
            "false",
        ],
    )?;
    check_eq(
        &texts(&none_around),
        &vec!["go, but carefully"],
        "--around 0",
    )?;
    Ok(())
}
