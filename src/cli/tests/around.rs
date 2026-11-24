//! Tests for `--around`: the records next to a record that matches
//! `--grep`, marked as context.

use core::error::Error;

use serde_json::Value;

use super::{History, fixture_time};
use crate::testing::{TestResult, check_eq};

/// A history with a session `ffff` in which a prompt asks about the cloud
/// edition and the replies go on, a prompt marked WIP mentions it too, and
/// a session `gggg`, written earlier, that ends with an unrelated prompt.
fn history_with_a_thread() -> Result<History, Box<dyn Error>> {
    let history = History::new("root")?;
    let prompt = |uuid: &str, session: &str, at: &str, text: &str| {
        format!(
            r#"{{"type":"user","uuid":"{uuid}","sessionId":"{session}","timestamp":"{at}","message":{{"role":"user","content":"{text}"}}}}"#
        )
    };
    let reply = |uuid: &str, at: &str, text: &str| {
        format!(
            r#"{{"type":"assistant","uuid":"{uuid}","sessionId":"ffff","timestamp":"{at}","message":{{"content":[{{"type":"text","text":"{text}"}}]}}}}"#
        )
    };
    history.write(
        "-Users-alice-blog/gggg.jsonl",
        fixture_time()?,
        &[&prompt(
            "g-1",
            "gggg",
            "2026-10-14T23:00:00Z",
            "an unrelated tail",
        )],
    )?;
    history.write(
        "-Users-alice-blog/ffff.jsonl",
        fixture_time()?,
        &[
            &prompt(
                "f-1",
                "ffff",
                "2026-10-15T00:00:00Z",
                "the cloud edition first?",
            ),
            &reply("f-2", "2026-10-15T00:00:10Z", "It was left out."),
            &prompt("f-3", "ffff", "2026-10-15T00:01:00Z", "and the desktop?"),
            &reply("f-4", "2026-10-15T00:01:10Z", "The desktop has it."),
            &prompt("f-5", "ffff", "2026-10-15T00:02:00Z", "WIP: cloud again"),
            &reply("f-6", "2026-10-15T00:02:10Z", "The cloud edition gets zip."),
            &prompt("f-7", "ffff", "2026-10-15T00:03:00Z", "thanks"),
        ],
    )?;
    Ok(history)
}

/// Each record's line `uuid` and whether it matched, for `args` in the
/// week of the history.
fn listed(history: &History, args: &[&str]) -> Result<Vec<(String, bool)>, Box<dyn Error>> {
    let ran = history.run_week(args);
    if ran.code != 0 {
        return Err(ran.err.into());
    }
    ran.out
        .lines()
        .map(|line| {
            let record: Value = serde_json::from_str(line)?;
            let uuid = record
                .get("source")
                .and_then(|source| source.get("uuid"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let matched = record
                .get("matched")
                .and_then(Value::as_bool)
                .ok_or("no matched key")?;
            Ok((uuid, matched))
        })
        .collect()
}

/// The pairs `listed` returns, from string slices.
fn pairs(want: &[(&str, bool)]) -> Vec<(String, bool)> {
    want.iter()
        .map(|&(uuid, matched)| (uuid.to_owned(), matched))
        .collect()
}

/// The flags that search the two sessions for the cloud edition, leaving
/// out what is marked WIP, with `extra` after them.
fn search<'flag>(extra: &[&'flag str]) -> Vec<&'flag str> {
    let mut args = vec![
        "records",
        "--session",
        "ffff",
        "--session",
        "gggg",
        "--type",
        "prompt,reply",
        "--igrep",
        "cloud edition",
        "--grep",
        "!WIP",
    ];
    args.extend_from_slice(extra);
    args
}

#[test]
fn around_adds_the_records_next_to_a_match_marked_as_context() -> TestResult {
    let history = history_with_a_thread()?;
    for (around, want) in [
        (None, pairs(&[("f-1", true), ("f-6", true)])),
        (
            Some("1"),
            pairs(&[
                ("f-1", true),
                ("f-2", false),
                ("f-4", false),
                ("f-6", true),
                ("f-7", false),
            ]),
        ),
    ] {
        let extra: Vec<&str> = around.map_or_else(Vec::new, |n| vec!["--around", n]);
        check_eq(
            &listed(&history, &search(&extra))?,
            &want,
            &format!("{around:?}"),
        )?;
    }
    Ok(())
}

#[test]
fn around_does_not_reach_into_another_file_or_bring_back_an_excluded_record() -> TestResult {
    let history = history_with_a_thread()?;
    let got = listed(&history, &search(&["--around", "2"]))?;
    for uuid in ["g-1", "f-5"] {
        check_eq(
            &got.iter().any(|pair| pair.0 == uuid),
            &false,
            &format!("{uuid} in {got:?}"),
        )?;
    }
    Ok(())
}

#[test]
fn limit_counts_the_matches_and_keeps_the_context_after_the_last() -> TestResult {
    let history = history_with_a_thread()?;
    check_eq(
        &listed(&history, &search(&["--around", "1", "--limit", "1"]))?,
        &pairs(&[("f-1", true), ("f-2", false)]),
        "--limit 1",
    )?;
    Ok(())
}

#[test]
fn around_without_a_pattern_to_find_is_an_error() -> TestResult {
    let history = history_with_a_thread()?;
    let ran = history.run_week(&["records", "--around", "1", "--grep", "!WIP"]);
    check_eq(&ran.code, &1, "exit")?;
    check_eq(
        ran.err.as_str(),
        "ccdig: --around needs --grep or --igrep with a pattern that does not start with !\n",
        "error",
    )?;
    Ok(())
}

#[test]
fn markdown_marks_a_record_of_context() -> TestResult {
    let history = history_with_a_thread()?;
    let ran = history.run_week(&search(&["--around", "1", "--format", "md"]));
    check_eq(&ran.code, &0, &ran.err)?;
    let headings: Vec<&str> = ran
        .out
        .lines()
        .filter(|line| line.starts_with("### "))
        .map(|line| line.split_once(" - ").map_or(line, |pair| pair.1))
        .collect();
    check_eq(
        &headings,
        &vec![
            "Prompt",
            "Reply (context)",
            "Reply (context)",
            "Reply",
            "Prompt (context)",
        ],
        &ran.out,
    )?;
    Ok(())
}
