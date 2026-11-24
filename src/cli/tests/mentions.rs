//! Tests for searching sessions: `sessions --grep` reads the prompts and
//! the replies, and counts the matches of each session.

use core::error::Error;

use serde_json::{Value, json};

use super::{History, fixture_time};
use crate::testing::{TestResult, check_eq};

/// The sessions of the history as `rows` gives them.
const SESSIONS: [&str; 7] = [
    "sessions",
    "--session",
    "kkkk",
    "--session",
    "llll",
    "--session",
    "mmmm",
];

/// A history with a session `kkkk` whose first prompt and one reply
/// mention the cloud, a session `llll` where only a reply does, and a
/// session `mmmm` where nothing does.
fn history_with_mentions() -> Result<History, Box<dyn Error>> {
    let history = History::new("root")?;
    let prompt = |uuid: &str, session: &str, at: &str, text: &str| {
        format!(
            r#"{{"type":"user","uuid":"{uuid}","sessionId":"{session}","timestamp":"{at}","message":{{"role":"user","content":"{text}"}}}}"#
        )
    };
    let reply = |uuid: &str, session: &str, at: &str, text: &str| {
        format!(
            r#"{{"type":"assistant","uuid":"{uuid}","sessionId":"{session}","timestamp":"{at}","message":{{"content":[{{"type":"text","text":"{text}"}}]}}}}"#
        )
    };
    history.write(
        "-Users-alice-blog/kkkk.jsonl",
        fixture_time()?,
        &[
            &prompt(
                "k-1",
                "kkkk",
                "2026-10-15T00:00:00Z",
                "move the blog to the cloud",
            ),
            &reply("k-2", "kkkk", "2026-10-15T00:00:10Z", "Moved."),
            &prompt("k-3", "kkkk", "2026-10-15T00:05:00Z", "and the images?"),
            &reply(
                "k-4",
                "kkkk",
                "2026-10-15T00:05:10Z",
                "They are in the Cloud bucket.",
            ),
        ],
    )?;
    history.write(
        "-Users-alice-blog/llll.jsonl",
        fixture_time()?,
        &[
            &prompt("l-1", "llll", "2026-10-16T00:00:00Z", "is the site up?"),
            &reply("l-2", "llll", "2026-10-16T00:00:10Z", "The cloud says yes."),
        ],
    )?;
    history.write(
        "-Users-alice-blog/mmmm.jsonl",
        fixture_time()?,
        &[
            &prompt("m-1", "mmmm", "2026-10-16T01:00:00Z", "fix the typo"),
            &reply("m-2", "mmmm", "2026-10-16T01:00:10Z", "Fixed."),
        ],
    )?;
    Ok(history)
}

/// The sessions `args` list as JSON lines, in the week of the history,
/// each as its ID, prompt count, match count, start, and end.
fn rows(history: &History, args: &[&str]) -> Result<Vec<Value>, Box<dyn Error>> {
    let ran = history.run_week(args);
    if ran.code != 0 {
        return Err(ran.err.into());
    }
    ran.out
        .lines()
        .map(|line| {
            let session: Value = serde_json::from_str(line)?;
            Ok(json!([
                session.get("session_id"),
                session.get("prompt_count"),
                session.get("match_count").ok_or("no match_count key")?,
                session.get("started_at"),
                session.get("ended_at"),
            ]))
        })
        .collect()
}

#[test]
fn sessions_grep_reads_the_prompts_and_the_replies_and_counts_the_matches() -> TestResult {
    let history = history_with_mentions()?;
    let mut args = SESSIONS.to_vec();
    args.extend(["--igrep", "cloud"]);
    check_eq(
        &rows(&history, &args)?,
        &vec![
            json!([
                "kkkk",
                2_u64,
                2_u64,
                "2026-10-15T00:00:00Z",
                "2026-10-15T00:05:00Z"
            ]),
            json!([
                "llll",
                1_u64,
                1_u64,
                "2026-10-16T00:00:00Z",
                "2026-10-16T00:00:00Z"
            ]),
        ],
        "rows",
    )?;
    Ok(())
}

#[test]
fn sessions_without_grep_have_no_match_count_and_the_same_rows() -> TestResult {
    let history = history_with_mentions()?;
    let plain = rows(&history, &SESSIONS)?;
    check_eq(
        &plain,
        &vec![
            json!([
                "kkkk",
                2_u64,
                null,
                "2026-10-15T00:00:00Z",
                "2026-10-15T00:05:00Z"
            ]),
            json!([
                "llll",
                1_u64,
                null,
                "2026-10-16T00:00:00Z",
                "2026-10-16T00:00:00Z"
            ]),
            json!([
                "mmmm",
                1_u64,
                null,
                "2026-10-16T01:00:00Z",
                "2026-10-16T01:00:00Z"
            ]),
        ],
        "rows",
    )?;
    let mut excluding = SESSIONS.to_vec();
    excluding.extend(["--grep", "!typo"]);
    check_eq(
        &rows(&history, &excluding)?,
        &plain.get(..2).map(<[Value]>::to_vec).unwrap_or_default(),
        "a pattern with a leading ! still leaves out the prompts it matches",
    )?;
    Ok(())
}

#[test]
fn sessions_in_markdown_have_a_matches_column_only_with_grep() -> TestResult {
    let history = history_with_mentions()?;
    let mut plain = SESSIONS.to_vec();
    plain.extend(["--format", "md"]);
    let without = history.run_week(&plain).out;
    check_eq(
        &without.contains("| Started | Project | Prompts | Title |"),
        &true,
        &without,
    )?;
    let mut grep = plain.clone();
    grep.extend(["--igrep", "cloud"]);
    let with = history.run_week(&grep).out;
    check_eq(
        &with.contains("| Started | Project | Prompts | Matches | Title |"),
        &true,
        &with,
    )?;
    check_eq(&with.contains(" | 2 | 2 | "), &true, &with)?;
    Ok(())
}

#[test]
fn the_digest_still_greps_the_prompts_only() -> TestResult {
    let history = history_with_mentions()?;
    let ran = history.run_week(&["digest", "--session", "llll", "--igrep", "cloud"]);
    check_eq(&ran.code, &0, &ran.err)?;
    check_eq(&ran.out.contains("is the site up?"), &false, &ran.out)?;
    Ok(())
}
