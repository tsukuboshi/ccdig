//! Tests for `--context`: the lines of a record that match `--grep`, with
//! the lines around them.

use core::error::Error;

use serde_json::{Value, json};

use super::{History, fixture_time, json_lines};
use crate::testing::{TestResult, check_eq, snapshot, without_style};

/// A history with a session `eeee` that mentions the cloud edition in a
/// reply of many lines, in a tool call's input, and in a tool result read
/// with line numbers of its own.
fn history_with_long_records() -> Result<History, Box<dyn Error>> {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/eeee.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"e-1","sessionId":"eeee","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"why was the export left out?"}}"#,
            r#"{"type":"assistant","uuid":"e-2","sessionId":"eeee","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"text","text":"line one\nline two\nThe Cloud edition was left out.\nline four\nline five\nline six\nThe cloud edition uses zip.\nline eight"}]}}"#,
            r##"{"type":"assistant","uuid":"e-3","sessionId":"eeee","timestamp":"2026-10-15T00:00:20Z","message":{"content":[{"type":"tool_use","id":"toolu_e1","name":"Write","input":{"file_path":"/tmp/notes.md","content":"# Notes\n\nThe cloud edition\nships later.\n"}}]}}"##,
            r#"{"type":"user","uuid":"e-4","sessionId":"eeee","timestamp":"2026-10-15T00:00:30Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_e1","content":"   483\tfn a() {}\r\n   484\tcloud edition\r\n   485\tfn b() {}\r\n   486\tfn c() {}"}]}}"#,
        ],
    )?;
    Ok(history)
}

/// The JSON Lines `args` print in the week of the history, parsed.
fn records(history: &History, args: &[&str]) -> Result<Vec<Value>, Box<dyn Error>> {
    json_lines(history.run_week(args))
}

/// The `matches` of each record.
fn matches(records: &[Value]) -> Vec<Value> {
    records
        .iter()
        .map(|record| record.get("matches").cloned().unwrap_or_default())
        .collect()
}

/// A line of `matches`.
fn line(field: &str, number: usize, text: &str, hit: bool) -> Value {
    json!({"field": field, "line": number, "text": text, "hit": hit})
}

#[test]
fn context_lists_the_matched_lines_of_a_reply_with_the_lines_around_them() -> TestResult {
    let history = history_with_long_records()?;
    for (context, want) in [
        (
            "0",
            vec![
                line("text", 3, "The Cloud edition was left out.", true),
                line("text", 7, "The cloud edition uses zip.", true),
            ],
        ),
        (
            "1",
            vec![
                line("text", 2, "line two", false),
                line("text", 3, "The Cloud edition was left out.", true),
                line("text", 4, "line four", false),
                line("text", 6, "line six", false),
                line("text", 7, "The cloud edition uses zip.", true),
                line("text", 8, "line eight", false),
            ],
        ),
    ] {
        let got = records(
            &history,
            &[
                "records",
                "--session",
                "eeee",
                "--type",
                "reply",
                "--igrep",
                "cloud edition",
                "--context",
                context,
            ],
        )?;
        check_eq(&matches(&got), &vec![Value::Array(want)], context)?;
    }
    Ok(())
}

#[test]
fn a_search_without_context_lists_the_matched_lines_alone() -> TestResult {
    let history = history_with_long_records()?;
    let args = [
        "records",
        "--session",
        "eeee",
        "--type",
        "reply",
        "--igrep",
        "cloud edition",
    ];
    let got = records(&history, &args)?;
    check_eq(
        &matches(&got),
        &vec![Value::Array(vec![
            line("text", 3, "The Cloud edition was left out.", true),
            line("text", 7, "The cloud edition uses zip.", true),
        ])],
        "jsonl",
    )?;
    let mut markdown = args.to_vec();
    markdown.extend(["--format", "md"]);
    let ran = history.run_week(&markdown);
    check_eq(&ran.code, &0, &ran.err)?;
    check_eq(&ran.out.contains("line one\nline two\n"), &true, &ran.out)?;
    check_eq(&ran.out.contains("- Line"), &false, &ran.out)?;
    Ok(())
}

#[test]
fn a_search_that_only_leaves_records_out_lists_no_lines() -> TestResult {
    let history = history_with_long_records()?;
    let got = records(
        &history,
        &[
            "records",
            "--session",
            "eeee",
            "--type",
            "reply",
            "--grep",
            "!WIP",
        ],
    )?;
    check_eq(&matches(&got), &vec![json!([])], "matches")?;
    Ok(())
}

#[test]
fn context_without_a_pattern_to_find_is_an_error() -> TestResult {
    let history = history_with_long_records()?;
    for extra in [&[][..], &["--grep", "!WIP"][..]] {
        let mut args = vec!["records", "--context", "2"];
        args.extend_from_slice(extra);
        let ran = history.run_week(&args);
        let name = args.join(" ");
        check_eq(&ran.code, &1, &name)?;
        check_eq(ran.out.as_str(), "", &name)?;
        check_eq(
            ran.err.as_str(),
            "ccdig: --context needs --grep or --igrep with a pattern that does not start with !\n",
            &name,
        )?;
    }
    Ok(())
}

#[test]
fn context_splits_each_string_of_a_call_s_input_and_a_result_into_lines() -> TestResult {
    let history = history_with_long_records()?;
    let got = records(
        &history,
        &[
            "records",
            "--session",
            "eeee",
            "--type",
            "call,result",
            "--grep",
            "cloud edition",
            "--context",
            "1",
        ],
    )?;
    check_eq(
        &matches(&got),
        &vec![
            Value::Array(vec![
                line("input.content", 2, "", false),
                line("input.content", 3, "The cloud edition", true),
                line("input.content", 4, "ships later.", false),
            ]),
            Value::Array(vec![
                line("text", 1, "   483\tfn a() {}", false),
                line("text", 2, "   484\tcloud edition", true),
                line("text", 3, "   485\tfn b() {}", false),
            ]),
        ],
        "matches",
    )?;
    Ok(())
}

#[test]
fn markdown_prints_the_matched_lines_in_place_of_the_whole_text() -> TestResult {
    let history = history_with_long_records()?;
    let ran = history.run_week(&[
        "records",
        "--session",
        "eeee",
        "--type",
        "reply,call,result",
        "--igrep",
        "cloud edition",
        "--context",
        "1",
        "--format",
        "md",
    ]);
    check_eq(&ran.code, &0, &ran.err)?;
    snapshot("cli/records-context.md", &ran.out)
}

#[test]
fn a_match_no_single_line_holds_lists_no_lines_and_prints_the_whole_text() -> TestResult {
    let history = history_with_long_records()?;
    for (kind, pattern) in [("reply", r"left out\.\nline four"), ("call", "file_path")] {
        let args = [
            "records",
            "--session",
            "eeee",
            "--type",
            kind,
            "--grep",
            pattern,
            "--context",
            "1",
        ];
        let got = records(&history, &args)?;
        check_eq(&matches(&got), &vec![json!([])], kind)?;
        let mut markdown = args.to_vec();
        markdown.extend(["--format", "md"]);
        let ran = history.run_week(&markdown);
        check_eq(&ran.out.contains("- Line"), &false, &ran.out)?;
    }
    let ran = history.run_week(&[
        "records",
        "--session",
        "eeee",
        "--type",
        "reply",
        "--grep",
        r"left out\.\nline four",
        "--context",
        "1",
        "--format",
        "md",
    ]);
    check_eq(&ran.out.contains("line one\nline two\n"), &true, &ran.out)?;
    Ok(())
}

#[test]
fn tool_chars_cuts_the_lines_of_a_call_s_input_too() -> TestResult {
    let history = history_with_long_records()?;
    let ran = history.run_week(&[
        "records",
        "--session",
        "eeee",
        "--type",
        "call",
        "--grep",
        "cloud edition",
        "--context",
        "0",
        "--tool-chars",
        "9",
        "--format",
        "md",
    ]);
    check_eq(&ran.code, &0, &ran.err)?;
    check_eq(
        &ran.out.contains("The cloud\u{2026} (8 more characters)\n"),
        &true,
        &ran.out,
    )?;
    Ok(())
}

#[test]
fn context_reads_only_the_strings_of_a_call_s_input() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/uuuu.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"u-1","sessionId":"uuuu","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"tool_use","id":"toolu_u1","name":"Edit","input":{"replace_all":true,"limit":3,"offset":null,"edits":[{"new_string":"a\ncloud edition"}]}}]}}"#,
        ],
    )?;
    let got = records(
        &history,
        &[
            "records",
            "--session",
            "uuuu",
            "--type",
            "call",
            "--grep",
            "cloud",
            "--context",
            "1",
        ],
    )?;
    check_eq(
        &matches(&got),
        &vec![Value::Array(vec![
            line("input.edits[0].new_string", 1, "a", false),
            line("input.edits[0].new_string", 2, "cloud edition", true),
        ])],
        "matches",
    )?;
    Ok(())
}

#[test]
fn a_secret_in_a_key_of_a_call_s_input_is_masked_in_the_input_and_the_lines() -> TestResult {
    let history = History::new("root")?;
    let token = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    history.write(
        "-Users-alice-blog/wwww.jsonl",
        fixture_time()?,
        &[&format!(
            r#"{{"type":"assistant","uuid":"w-1","sessionId":"wwww","timestamp":"2026-10-15T00:00:10Z","message":{{"content":[{{"type":"tool_use","id":"toolu_w1","name":"Bash","input":{{"{token}":"needle"}}}}]}}}}"#
        )],
    )?;
    for format in ["jsonl", "md"] {
        let ran = history.run_week(&[
            "records",
            "--session",
            "wwww",
            "--type",
            "call",
            "--grep",
            "needle",
            "--context",
            "0",
            "--format",
            format,
        ]);
        check_eq(&ran.code, &0, &ran.err)?;
        check_eq(&ran.out.contains("needle"), &true, &ran.out)?;
        check_eq(&ran.out.contains(token), &false, &ran.out)?;
    }
    Ok(())
}

#[test]
fn a_key_that_could_read_as_a_path_is_quoted_in_the_field() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/xxxx.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"x-1","sessionId":"xxxx","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"tool_use","id":"toolu_x1","name":"Edit","input":{"a.b":{"c[0]":"deploy"},"a":{"b":{"c":["deploy"]}},"new_string":"deploy"}}]}}"#,
        ],
    )?;
    let got = records(
        &history,
        &[
            "records",
            "--session",
            "xxxx",
            "--type",
            "call",
            "--grep",
            "deploy",
            "--context",
            "0",
        ],
    )?;
    check_eq(
        &matches(&got),
        &vec![Value::Array(vec![
            line(r#"input["a.b"]["c[0]"]"#, 1, "deploy", true),
            line("input.a.b.c[0]", 1, "deploy", true),
            line("input.new_string", 1, "deploy", true),
        ])],
        "matches",
    )?;
    Ok(())
}

#[test]
fn the_page_marks_the_matched_lines_in_place_of_the_whole_text() -> TestResult {
    let history = history_with_long_records()?;
    let ran = history.run_week(&[
        "records",
        "--session",
        "eeee",
        "--type",
        "reply,call,result",
        "--igrep",
        "cloud edition",
        "--context",
        "1",
        "--format",
        "html",
    ]);
    check_eq(&ran.code, &0, &ran.err)?;
    snapshot("cli/records-context.html", &without_style(&ran.out)?)
}
