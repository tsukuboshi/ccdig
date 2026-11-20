//! Tests for streaming a transcript, including assistant lines and small
//! read buffers.

use std::io::{BufReader, ErrorKind};

use super::{Lines, MAX_LINE_BYTES, ReadStats, Roles, open};
use crate::testing::{TestResult, check_eq};
use crate::transcript::entry::Role;

/// The roles the prompt subcommands read.
const PROMPTS: Roles = Roles {
    assistant: false,
    user: true,
};

/// What a test sees of a delivered entry: its line, role, and text.
type Seen = (usize, Role, String);

/// Reads every entry of `lines` written as a JSONL file.
fn read(lines: &[&str], max: usize, roles: Roles) -> Result<(Vec<Seen>, ReadStats), String> {
    read_with(&jsonl(lines), 8192, max, roles)
}

/// Reads every entry of `input` through a buffer of `buffer` bytes.
fn read_with(
    input: &str,
    buffer: usize,
    max: usize,
    roles: Roles,
) -> Result<(Vec<Seen>, ReadStats), String> {
    let mut lines = Lines::new(
        BufReader::with_capacity(buffer, input.as_bytes()),
        max,
        roles,
    );
    let mut seen = Vec::new();
    for item in &mut lines {
        let (number, entry) = item.map_err(|error| error.to_string())?;
        let text = if entry.role == Role::AiTitle {
            entry.title
        } else {
            entry.content.text
        };
        seen.push((number, entry.role, text));
    }
    Ok((seen, lines.stats()))
}

/// `lines` as a JSONL file, each line ending in a newline.
fn jsonl(lines: &[&str]) -> String {
    lines.iter().flat_map(|line| [*line, "\n"]).collect()
}

/// A user prompt line with `text` as its content.
fn prompt(text: &str) -> String {
    format!(r#"{{"type":"user","message":{{"role":"user","content":"{text}"}}}}"#)
}

#[test]
fn user_and_title_entries_come_with_their_line_numbers() -> TestResult {
    let (seen, _) = read(
        &[
            r#"{"type":"permission-mode","permissionMode":"auto"}"#,
            &prompt("first prompt"),
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"sure"}]}}"#,
            r#"{"type":"ai-title","aiTitle":"A title"}"#,
            &prompt("second prompt"),
        ],
        MAX_LINE_BYTES,
        PROMPTS,
    )?;
    check_eq(
        &seen,
        &vec![
            (2, Role::User, "first prompt".to_owned()),
            (4, Role::AiTitle, "A title".to_owned()),
            (5, Role::User, "second prompt".to_owned()),
        ],
        "entries",
    )?;
    Ok(())
}

#[test]
fn assistant_lines_come_through_when_asked_for() -> TestResult {
    let (seen, _) = read(
        &[
            &prompt("first prompt"),
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"sure"}]}}"#,
        ],
        MAX_LINE_BYTES,
        Roles {
            assistant: true,
            user: false,
        },
    )?;
    check_eq(
        &seen,
        &vec![(2, Role::Assistant, "sure".to_owned())],
        "entries",
    )?;
    Ok(())
}

#[test]
fn lines_run_past_the_read_buffer() -> TestResult {
    let long = "x".repeat(200 * 1024);
    let (seen, _) = read_with(
        &jsonl(&[&prompt(&long), &prompt("after the long line")]),
        16,
        MAX_LINE_BYTES,
        PROMPTS,
    )?;
    check_eq(
        &seen,
        &vec![
            (1, Role::User, long),
            (2, Role::User, "after the long line".to_owned()),
        ],
        "entries",
    )?;
    Ok(())
}

#[test]
fn undecodable_lines_are_counted_only_for_wanted_roles() -> TestResult {
    let complete = prompt("complete");
    let lines = [
        complete.as_str(),
        r#"{"type":"assistant","message":{"role":"assist"#,
        r#"{"type":"user","timestamp":"2026-10-13 00:00:00","message":{"role":"user","content":"odd timestamp"}}"#,
        r#"{"type":"user","message":{"role":"us"#,
    ];
    let (seen, stats) = read(&lines, MAX_LINE_BYTES, PROMPTS)?;
    check_eq(
        &seen,
        &vec![(1, Role::User, "complete".to_owned())],
        "entries",
    )?;
    check_eq(&stats.undecodable, &2, "undecodable for prompts")?;
    let (_, all) = read(
        &lines,
        MAX_LINE_BYTES,
        Roles {
            assistant: true,
            user: true,
        },
    )?;
    check_eq(&all.undecodable, &3, "undecodable for every role")?;
    Ok(())
}

#[test]
fn a_nested_type_field_does_not_leak_other_entries() -> TestResult {
    let (seen, _) = read(
        &[
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","input":{"type":"user"}}]}}"#,
        ],
        MAX_LINE_BYTES,
        PROMPTS,
    )?;
    check_eq(&seen, &Vec::new(), "entries")?;
    Ok(())
}

#[test]
fn over_limit_lines_are_skipped_and_the_rest_kept() -> TestResult {
    let at_limit = prompt(&"y".repeat(50));
    let max = at_limit.len();
    // The last line is over the limit and has no newline.
    let input = format!(
        "{}{}",
        jsonl(&[
            &prompt("before"),
            &prompt(&"x".repeat(500)),
            &at_limit,
            &prompt("after"),
        ]),
        prompt(&"z".repeat(500)),
    );
    let (seen, stats) = read_with(&input, 16, max, PROMPTS)?;
    check_eq(
        &seen,
        &vec![
            (1, Role::User, "before".to_owned()),
            (3, Role::User, "y".repeat(50)),
            (4, Role::User, "after".to_owned()),
        ],
        "entries",
    )?;
    check_eq(&stats.oversized, &2, "oversized")?;
    Ok(())
}

#[test]
fn spaced_json_passes_the_prefilter() -> TestResult {
    let (seen, _) = read(
        &[
            r#"{"type": "user", "message": {"role": "user", "content": "spaced"}}"#,
            r#"{"type": "ai-title", "aiTitle": "Spaced title"}"#,
        ],
        MAX_LINE_BYTES,
        PROMPTS,
    )?;
    check_eq(
        &seen,
        &vec![
            (1, Role::User, "spaced".to_owned()),
            (2, Role::AiTitle, "Spaced title".to_owned()),
        ],
        "entries",
    )?;
    Ok(())
}

#[test]
fn blank_lines_count_and_a_last_line_needs_no_newline() -> TestResult {
    let input = format!("\n{}\n\n{}", prompt("one"), prompt("two"));
    let (seen, _) = read_with(&input, 8192, MAX_LINE_BYTES, PROMPTS)?;
    check_eq(
        &seen,
        &vec![
            (2, Role::User, "one".to_owned()),
            (4, Role::User, "two".to_owned()),
        ],
        "entries",
    )?;
    Ok(())
}

#[test]
fn open_reports_a_missing_file_and_reading_a_directory_fails() -> TestResult {
    let dir = tempfile::tempdir()?;
    let missing = open(&dir.path().join("missing.jsonl"), MAX_LINE_BYTES, PROMPTS)
        .err()
        .map(|error| error.kind());
    check_eq(&missing, &Some(ErrorKind::NotFound), "missing file")?;
    let first = open(dir.path(), MAX_LINE_BYTES, PROMPTS)?.next();
    check_eq(&first.is_some_and(|item| item.is_err()), &true, "directory")?;
    Ok(())
}
