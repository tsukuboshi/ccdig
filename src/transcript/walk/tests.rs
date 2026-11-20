//! Tests for finding transcripts, ported from promptdig and extended to
//! sub-agent transcripts.

use core::time::Duration;
use std::fs::{self, File};
use std::io::ErrorKind;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::Path;
use std::time::SystemTime;

use super::{Found, find_sessions};
use crate::testing::{TestResult, check_eq};

/// Creates empty files at the slash-separated `paths` under `root`.
fn make_tree(root: &Path, paths: &[&str]) -> TestResult {
    for path in paths {
        let full = root.join(path);
        if let Some(dir) = full.parent() {
            fs::create_dir_all(dir)?;
        }
        File::create(full)?;
    }
    Ok(())
}

/// Each session as `relative/path (project)`, with `[agent]` for a
/// sub-agent, in the order found.
fn listed(root: &Path, found: &Found) -> Result<Vec<String>, String> {
    found
        .sessions
        .iter()
        .map(|session| {
            let relative = session
                .path
                .strip_prefix(root)
                .map_err(|error| error.to_string())?;
            let agent = session
                .subagent
                .as_ref()
                .map_or_else(String::new, |agent| format!(" [{agent}]"));
            Ok(format!(
                "{} ({}){agent}",
                relative.display(),
                session.project
            ))
        })
        .collect()
}

#[test]
fn only_top_level_session_files_are_listed_sorted() -> TestResult {
    let root = tempfile::tempdir()?;
    make_tree(
        root.path(),
        &[
            "-Users-alice-proj/bbbb.jsonl",
            "-Users-alice-proj/aaaa.jsonl",
            "-Users-alice-proj/aaaa/subagents/agent-1.jsonl",
            "-Users-alice-proj/aaaa/tool-results/out.jsonl",
            "-Users-alice-proj/notes.txt",
            "-Users-alice-blog/cccc.jsonl",
            "stray.jsonl",
        ],
    )?;
    let found = find_sessions(root.path(), false)?;
    check_eq(
        &listed(root.path(), &found)?,
        &vec![
            "-Users-alice-blog/cccc.jsonl (-Users-alice-blog)".to_owned(),
            "-Users-alice-proj/aaaa.jsonl (-Users-alice-proj)".to_owned(),
            "-Users-alice-proj/bbbb.jsonl (-Users-alice-proj)".to_owned(),
        ],
        "sessions",
    )?;
    check_eq(&found.warnings.len(), &0, "warnings")?;
    Ok(())
}

#[test]
fn subagent_transcripts_follow_their_session_when_asked_for() -> TestResult {
    let root = tempfile::tempdir()?;
    make_tree(
        root.path(),
        &[
            "p/aaaa.jsonl",
            "p/aaaa/subagents/agent-2.jsonl",
            "p/aaaa/subagents/agent-1.jsonl",
            "p/aaaa/subagents/notes.txt",
            "p/aaaa/tool-results/out.jsonl",
            "p/bbbb.jsonl",
            "p/cccc/tool-results/out.jsonl",
        ],
    )?;
    let found = find_sessions(root.path(), true)?;
    check_eq(
        &listed(root.path(), &found)?,
        &vec![
            "p/aaaa.jsonl (p)".to_owned(),
            "p/aaaa/subagents/agent-1.jsonl (p) [agent-1]".to_owned(),
            "p/aaaa/subagents/agent-2.jsonl (p) [agent-2]".to_owned(),
            "p/bbbb.jsonl (p)".to_owned(),
        ],
        "sessions",
    )?;
    check_eq(&found.warnings.len(), &0, "warnings")?;
    Ok(())
}

#[test]
fn paths_sort_by_their_bytes() -> TestResult {
    // Per directory, "a/" would come first; by bytes, "a-b/" does because
    // '-' < '/'.
    let root = tempfile::tempdir()?;
    make_tree(root.path(), &["a/x.jsonl", "a-b/x.jsonl"])?;
    let found = find_sessions(root.path(), false)?;
    check_eq(
        &listed(root.path(), &found)?,
        &vec!["a-b/x.jsonl (a-b)".to_owned(), "a/x.jsonl (a)".to_owned()],
        "sessions",
    )?;
    Ok(())
}

#[test]
fn symlinks_are_skipped() -> TestResult {
    // A symlink could pull in a file outside --root.
    let outside = tempfile::tempdir()?;
    make_tree(outside.path(), &["o.jsonl"])?;
    let root = tempfile::tempdir()?;
    make_tree(root.path(), &["p/real.jsonl"])?;
    symlink(
        outside.path().join("o.jsonl"),
        root.path().join("p/link.jsonl"),
    )?;
    symlink(outside.path(), root.path().join("linked-project"))?;
    let found = find_sessions(root.path(), true)?;
    check_eq(
        &listed(root.path(), &found)?,
        &vec!["p/real.jsonl (p)".to_owned()],
        "sessions",
    )?;
    Ok(())
}

#[test]
fn the_modification_time_is_reported() -> TestResult {
    let root = tempfile::tempdir()?;
    make_tree(root.path(), &["p/s.jsonl"])?;
    // 2026-10-11T23:30:00Z.
    let time = SystemTime::UNIX_EPOCH + Duration::from_mins(29_862_690);
    File::options()
        .write(true)
        .open(root.path().join("p/s.jsonl"))?
        .set_modified(time)?;
    let found = find_sessions(root.path(), false)?;
    check_eq(
        &found.sessions.first().map(|session| session.modified),
        &Some(time),
        "modified",
    )?;
    Ok(())
}

#[test]
fn a_missing_root_is_an_error() -> TestResult {
    let root = tempfile::tempdir()?;
    let kind = find_sessions(&root.path().join("missing"), false)
        .err()
        .map(|error| error.kind());
    check_eq(&kind, &Some(ErrorKind::NotFound), "error")?;
    Ok(())
}

#[test]
fn an_unreadable_project_is_a_warning_and_the_rest_is_listed() -> TestResult {
    let root = tempfile::tempdir()?;
    make_tree(root.path(), &["a/s.jsonl", "b/s.jsonl"])?;
    let locked = root.path().join("a");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))?;
    let readable = fs::read_dir(&locked).is_ok();
    let result = find_sessions(root.path(), false);
    // A directory needs the x bit for the temp dir to be removed.
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700))?;
    if readable {
        // Directory permissions are not enforced for this user.
        return Ok(());
    }
    let found = result?;
    check_eq(
        &listed(root.path(), &found)?,
        &vec!["b/s.jsonl (b)".to_owned()],
        "sessions",
    )?;
    check_eq(
        &found
            .warnings
            .iter()
            .map(|warning| (warning.path.clone(), warning.error.kind()))
            .collect::<Vec<_>>(),
        &vec![(locked, ErrorKind::PermissionDenied)],
        "warnings",
    )?;
    Ok(())
}
