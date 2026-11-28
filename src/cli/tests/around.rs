//! Tests for `--around`: the records next to a record that matches
//! `--grep`, marked as context.

use core::error::Error;

use serde_json::Value;

use super::{History, fixture_time};
use crate::testing::{TestResult, check_eq, snapshot, without_style};

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
            "turn 1 - Prompt",
            "turn 1 - Reply (context)",
            "turn 2 - Reply (context)",
            "turn 3 - Reply",
            "turn 4 - Prompt (context)",
        ],
        &ran.out,
    )?;
    Ok(())
}

#[test]
fn a_response_s_tokens_stay_on_the_match_when_around_adds_its_context() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/nnnn.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"n-1","sessionId":"nnnn","timestamp":"2026-10-15T00:00:10Z","message":{"id":"m-1","content":[{"type":"text","text":"let me look"}],"usage":{"input_tokens":7,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
            r#"{"type":"assistant","uuid":"n-2","sessionId":"nnnn","timestamp":"2026-10-15T00:00:11Z","message":{"id":"m-1","content":[{"type":"tool_use","id":"toolu_n1","name":"Bash","input":{"command":"./deploy.sh"}}],"usage":{"input_tokens":7,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#,
        ],
    )?;
    for extra in [&[][..], &["--around", "1"][..]] {
        let mut args = vec![
            "records",
            "--session",
            "nnnn",
            "--type",
            "reply,call",
            "--igrep",
            "deploy",
        ];
        args.extend_from_slice(extra);
        let ran = history.run_week(&args);
        let carriers: Vec<String> = ran
            .out
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .filter(|record| record.get("usage").is_some_and(|usage| !usage.is_null()))
            .filter_map(|record| {
                record
                    .get("type")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect();
        check_eq(&carriers, &vec!["call".to_owned()], &args.join(" "))?;
    }
    Ok(())
}

#[test]
fn a_resumed_copy_lists_no_context_whose_match_its_original_listed() -> TestResult {
    let history = History::new("root")?;
    let shared = [
        r#"{"type":"user","uuid":"r-1","sessionId":"rrrr","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"please ship it"}}"#,
        r#"{"type":"assistant","uuid":"r-2","sessionId":"rrrr","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"text","text":"deploy done"}]}}"#,
    ];
    history.write("-Users-alice-blog/rrrr.jsonl", fixture_time()?, &shared)?;
    let mut resumed = shared.to_vec();
    resumed.extend([
        r#"{"type":"user","uuid":"r-3","sessionId":"ssss","timestamp":"2026-10-15T01:00:00Z","message":{"role":"user","content":"thanks"}}"#,
        r#"{"type":"assistant","uuid":"r-4","sessionId":"ssss","timestamp":"2026-10-15T01:00:10Z","message":{"content":[{"type":"text","text":"you are welcome"}]}}"#,
    ]);
    history.write("-Users-alice-blog/ssss.jsonl", fixture_time()?, &resumed)?;
    check_eq(
        &listed(
            &history,
            &[
                "records",
                "--project",
                "blog",
                "--type",
                "prompt,reply",
                "--igrep",
                "deploy",
                "--around",
                "1",
            ],
        )?,
        &pairs(&[("r-1", false), ("r-2", true)]),
        "records",
    )?;
    Ok(())
}

#[test]
fn limit_zero_lists_nothing_and_a_limit_above_the_matches_lists_them_all() -> TestResult {
    let history = history_with_a_thread()?;
    check_eq(
        &listed(&history, &search(&["--around", "1", "--limit", "0"]))?,
        &Vec::new(),
        "--limit 0",
    )?;
    check_eq(
        &listed(&history, &search(&["--around", "1", "--limit", "9"]))?,
        &listed(&history, &search(&["--around", "1"]))?,
        "--limit 9",
    )?;
    Ok(())
}

#[test]
fn calls_results_and_summaries_can_be_context_too() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/tttt.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"t-1","sessionId":"tttt","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"deploy it"}}"#,
            r#"{"type":"assistant","uuid":"t-2","sessionId":"tttt","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"tool_use","id":"toolu_t1","name":"Bash","input":{"command":"ls"}}]}}"#,
            r#"{"type":"user","uuid":"t-3","sessionId":"tttt","timestamp":"2026-10-15T00:00:20Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_t1","content":"a.md"}]}}"#,
            r#"{"type":"user","uuid":"t-4","sessionId":"tttt","isCompactSummary":true,"timestamp":"2026-10-15T00:00:30Z","message":{"role":"user","content":"The session so far."}}"#,
        ],
    )?;
    check_eq(
        &listed(
            &history,
            &[
                "records",
                "--session",
                "tttt",
                "--type",
                "prompt,call,result,summary",
                "--grep",
                "deploy",
                "--around",
                "3",
            ],
        )?,
        &pairs(&[
            ("t-1", true),
            ("t-2", false),
            ("t-3", false),
            ("t-4", false),
        ]),
        "records",
    )?;
    Ok(())
}

#[test]
fn context_a_copy_dropped_still_comes_back_for_a_later_match() -> TestResult {
    let history = History::new("root")?;
    // Claude Code writes the ID of the session a line is copied into, as
    // the file is named.
    let prompt = |session: &str, uuid: &str, at: &str, text: &str| {
        format!(
            r#"{{"type":"user","uuid":"{uuid}","sessionId":"{session}","timestamp":"{at}","message":{{"role":"user","content":"{text}"}}}}"#
        )
    };
    let first = |session| prompt(session, "q1", "2026-10-15T00:00:00Z", "deploy one");
    let second = |session| prompt(session, "q2", "2026-10-15T01:00:00Z", "plain");
    let third = |session| prompt(session, "q3", "2026-10-15T02:00:00Z", "deploy two");
    for (file, lines) in [
        ("cdf-c", vec![first("cdf-c")]),
        ("cdf-d", vec![first("cdf-d"), second("cdf-d")]),
        (
            "cdf-f",
            vec![first("cdf-f"), second("cdf-f"), third("cdf-f")],
        ),
    ] {
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        history.write(
            &format!("-Users-alice-blog/{file}.jsonl"),
            fixture_time()?,
            &refs,
        )?;
    }
    check_eq(
        &listed(
            &history,
            &[
                "records",
                "--session",
                "cdf",
                "--type",
                "prompt",
                "--grep",
                "deploy",
                "--around",
                "1",
            ],
        )?,
        &pairs(&[("q1", true), ("q2", false), ("q3", true)]),
        "records",
    )?;
    Ok(())
}

#[test]
fn the_page_fades_a_record_of_context() -> TestResult {
    let history = history_with_a_thread()?;
    let ran = history.run_week(&search(&["--around", "1", "--format", "html"]));
    check_eq(&ran.code, &0, &ran.err)?;
    snapshot("cli/records-around.html", &without_style(&ran.out)?)
}
