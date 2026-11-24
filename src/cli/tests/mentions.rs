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
                "2026-10-15T09:00:00+09:00",
                "move the blog to the cloud",
            ),
            &reply("k-2", "kkkk", "2026-10-15T00:00:10Z", "Moved."),
            &prompt("k-3", "kkkk", "2026-10-15T00:05:00Z", "and the images?"),
            &reply(
                "k-4",
                "kkkk",
                "2026-10-15T09:05:10+09:00",
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
                "2026-10-15T09:00:00+09:00",
                "2026-10-15T09:05:10+09:00"
            ]),
            json!([
                "llll",
                1_u64,
                1_u64,
                "2026-10-16T09:00:00+09:00",
                "2026-10-16T09:00:10+09:00"
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
                "2026-10-15T09:00:00+09:00",
                "2026-10-15T09:05:10+09:00"
            ]),
            json!([
                "llll",
                1_u64,
                null,
                "2026-10-16T09:00:00+09:00",
                "2026-10-16T09:00:10+09:00"
            ]),
            json!([
                "mmmm",
                1_u64,
                null,
                "2026-10-16T10:00:00+09:00",
                "2026-10-16T10:00:10+09:00"
            ]),
        ],
        "rows",
    )?;
    let mut excluding = SESSIONS.to_vec();
    excluding.extend(["--grep", "!typo"]);
    check_eq(
        &rows(&history, &excluding)?,
        &plain.get(..2).map_or_default(<[Value]>::to_vec),
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
fn a_session_ends_at_its_last_record_in_the_range() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/nnnn.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"n-1","sessionId":"nnnn","timestamp":"2026-10-17T00:00:00Z","message":{"role":"user","content":"run the tests"}}"#,
            r#"{"type":"assistant","uuid":"n-2","sessionId":"nnnn","timestamp":"2026-10-17T00:01:00Z","message":{"content":[{"type":"tool_use","id":"toolu_n1","name":"Bash","input":{"command":"cargo test"}}]}}"#,
            r#"{"type":"user","uuid":"n-3","sessionId":"nnnn","timestamp":"2026-10-17T00:23:00Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_n1","content":"ok"}]}}"#,
        ],
    )?;
    history.write(
        "-Users-alice-blog/oooo.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"o-1","sessionId":"oooo","timestamp":"2026-10-19T14:59:00Z","message":{"role":"user","content":"last one this week"}}"#,
            r#"{"type":"assistant","uuid":"o-2","sessionId":"oooo","timestamp":"2026-10-19T15:01:00Z","message":{"content":[{"type":"text","text":"Done next week."}]}}"#,
        ],
    )?;
    check_eq(
        &rows(
            &history,
            &["sessions", "--session", "nnnn", "--session", "oooo"],
        )?,
        &vec![
            json!([
                "nnnn",
                1_u64,
                null,
                "2026-10-17T09:00:00+09:00",
                "2026-10-17T09:23:00+09:00"
            ]),
            json!([
                "oooo",
                1_u64,
                null,
                "2026-10-19T23:59:00+09:00",
                "2026-10-19T23:59:00+09:00"
            ]),
        ],
        "a tool result counts, and a reply after --until does not",
    )?;
    Ok(())
}

#[test]
fn a_line_the_harness_wrote_does_not_move_the_end_of_a_session() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/ssss.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"s-1","sessionId":"ssss","timestamp":"2026-10-17T00:00:00Z","message":{"role":"user","content":"tidy the posts"}}"#,
            r#"{"type":"assistant","uuid":"s-2","sessionId":"ssss","timestamp":"2026-10-17T00:10:00Z","message":{"content":[{"type":"text","text":"Tidied."}]}}"#,
            r#"{"type":"user","uuid":"s-3","isMeta":true,"sessionId":"ssss","timestamp":"2026-10-17T00:30:00Z","message":{"role":"user","content":"Caveat: the messages below were generated by the user while running local commands."}}"#,
            r#"{"type":"user","uuid":"s-4","sessionId":"ssss","timestamp":"2026-10-17T00:31:00Z","message":{"role":"user","content":"<local-command-stdout>ok</local-command-stdout>"}}"#,
        ],
    )?;
    let got = rows(&history, &["sessions", "--session", "ssss"])?;
    check_eq(
        &got.first().and_then(|row| row.get(4)),
        &Some(&json!("2026-10-17T09:10:00+09:00")),
        "the reply ends it",
    )?;
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

#[test]
fn a_session_a_search_skips_leaves_its_tokens_out_of_a_resumed_copy() -> TestResult {
    let history = History::new("root")?;
    let line = |uuid: &str, session: &str, at: &str, role: &str, text: &str, id: &str| {
        if role == "user" {
            format!(
                r#"{{"type":"user","uuid":"{uuid}","sessionId":"{session}","timestamp":"{at}","message":{{"role":"user","content":"{text}"}}}}"#
            )
        } else {
            format!(
                r#"{{"type":"assistant","uuid":"{uuid}","sessionId":"{session}","timestamp":"{at}","message":{{"id":"{id}","content":[{{"type":"text","text":"{text}"}}],"usage":{{"input_tokens":10,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
            )
        }
    };
    let original = [
        line("o-1", "oooo", "2026-10-15T00:00:00Z", "user", "hello", ""),
        line(
            "o-2",
            "oooo",
            "2026-10-15T09:00:10+09:00",
            "assistant",
            "hi",
            "msg_o",
        ),
    ];
    let mut copy = original.to_vec();
    copy.extend([
        line(
            "p-1",
            "pppp",
            "2026-10-15T10:00:00+09:00",
            "user",
            "deploy now",
            "",
        ),
        line(
            "p-2",
            "pppp",
            "2026-10-15T10:00:10+09:00",
            "assistant",
            "ok",
            "msg_p",
        ),
    ]);
    for (file, lines) in [("oooo", original.to_vec()), ("pppp", copy)] {
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        history.write(
            &format!("-Users-alice-blog/{file}.jsonl"),
            fixture_time()?,
            &refs,
        )?;
    }
    let input = |args: &[&str]| -> Result<Option<u64>, Box<dyn Error>> {
        let ran = history.run_week(args);
        for row in ran.out.lines() {
            let session: Value = serde_json::from_str(row)?;
            if session.get("session_id").and_then(Value::as_str) == Some("pppp") {
                return Ok(session
                    .get("usage")
                    .and_then(|usage| usage.get("input"))
                    .and_then(Value::as_u64));
            }
        }
        Ok(None)
    };
    let both = ["sessions", "--session", "oooo", "--session", "pppp"];
    check_eq(&input(&both)?, &Some(10), "without --grep")?;
    let mut search = both.to_vec();
    search.extend(["--igrep", "deploy"]);
    check_eq(&input(&search)?, &Some(10), "with --grep")?;
    Ok(())
}
