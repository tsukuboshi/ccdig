//! Tests for the command `sessions` gives to resume each session.

use core::error::Error;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use super::{History, fixture_time, json_lines};
use crate::testing::{TestResult, check_eq};

/// Adds session `rrrr`, one prompt with `cwd` as its working directory.
fn with_session_in(history: &History, cwd: &str) -> Result<(), Box<dyn Error>> {
    let line = serde_json::json!({
        "type": "user",
        "uuid": "r-1",
        "sessionId": "rrrr",
        "timestamp": "2026-10-15T00:00:00Z",
        "cwd": cwd,
        "message": {"role": "user", "content": "carry on"},
    })
    .to_string();
    history.write("-work/rrrr.jsonl", fixture_time()?, &[&line])?;
    Ok(())
}

/// The `resume` and `resume_unavailable` of session `rrrr`, run with
/// `extra` flags.
fn resume_of(history: &History, extra: &[&str]) -> Result<(Value, Value), Box<dyn Error>> {
    let mut args = vec!["sessions", "--session", "rrrr"];
    args.extend_from_slice(extra);
    let sessions = json_lines(history.run_week(&args))?;
    let [ref session] = sessions[..] else {
        return Err(format!("want one session, got {}", sessions.len()).into());
    };
    Ok((
        session.get("resume").cloned().unwrap_or(Value::Null),
        session
            .get("resume_unavailable")
            .cloned()
            .unwrap_or(Value::Null),
    ))
}

/// A directory with a space in its name, next to the history's root, so
/// it exists while the test runs and is not taken for a project.
fn existing_dir(history: &History) -> Result<PathBuf, Box<dyn Error>> {
    let dir = history
        .root
        .parent()
        .ok_or("the root has a parent")?
        .join("my work");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[test]
fn a_session_whose_directory_exists_resumes_there() -> TestResult {
    let history = History::new("root")?;
    let dir = existing_dir(&history)?;
    let cwd = dir.to_string_lossy();
    with_session_in(&history, &cwd)?;
    let (resume, unavailable) = resume_of(&history, &[])?;
    check_eq(
        &resume,
        &Value::String(format!("cd '{cwd}' && claude --resume rrrr")),
        "resume",
    )?;
    check_eq(&unavailable, &Value::Null, "reason")?;
    Ok(())
}

#[test]
fn a_session_whose_directory_is_masked_has_no_command() -> TestResult {
    let history = History::new("root")?;
    let dir = existing_dir(&history)?;
    with_session_in(&history, &dir.to_string_lossy())?;
    let (resume, unavailable) = resume_of(&history, &["--mask-values", r#"{"dir":["my work"]}"#])?;
    check_eq(&resume, &Value::Null, "resume")?;
    check_eq(&unavailable, &Value::from("cwd-masked"), "reason")?;
    Ok(())
}

#[test]
fn a_session_without_a_directory_has_no_command() -> TestResult {
    let history = History::new("root")?;
    with_session_in(&history, "")?;
    let (resume, unavailable) = resume_of(&history, &[])?;
    check_eq(&resume, &Value::Null, "resume")?;
    check_eq(&unavailable, &Value::from("no-cwd"), "reason")?;
    Ok(())
}

#[test]
fn a_session_whose_directory_is_gone_has_no_command() -> TestResult {
    let history = History::new("root")?;
    let dir = existing_dir(&history)?;
    with_session_in(&history, &dir.to_string_lossy())?;
    fs::remove_dir(&dir)?;
    let (resume, unavailable) = resume_of(&history, &[])?;
    check_eq(&resume, &Value::Null, "resume")?;
    check_eq(&unavailable, &Value::from("cwd-missing"), "reason")?;
    Ok(())
}

#[test]
fn a_session_resumes_where_its_transcript_starts_whatever_the_range() -> TestResult {
    let history = History::new("root")?;
    let dir = existing_dir(&history)?;
    let start = dir.to_string_lossy();
    let later = dir.join("sub");
    let line = |uuid: &str, time: &str, cwd: &str| {
        serde_json::json!({
            "type": "user",
            "uuid": uuid,
            "sessionId": "rrrr",
            "timestamp": time,
            "cwd": cwd,
            "message": {"role": "user", "content": "carry on"},
        })
        .to_string()
    };
    // The first prompt is before the week, so the range keeps the second.
    history.write(
        "-work/rrrr.jsonl",
        fixture_time()?,
        &[
            &line("r-1", "2026-10-01T00:00:00Z", &start),
            &line("r-2", "2026-10-15T00:00:00Z", &later.to_string_lossy()),
        ],
    )?;
    let (resume, unavailable) = resume_of(&history, &[])?;
    check_eq(
        &resume,
        &Value::String(format!("cd '{start}' && claude --resume rrrr")),
        "resume",
    )?;
    check_eq(&unavailable, &Value::Null, "reason")?;
    Ok(())
}
