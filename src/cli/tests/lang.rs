//! Tests for `--lang` and `CCDIG_LANG`: the words of the Markdown and the
//! digest, and which formats they apply to.

use super::{History, WEEK, environment, fixture_time};
use crate::cli::environment::Environment;
use crate::testing::{TestResult, check_eq, snapshot};

/// The first line of `records --format md --lang ja`: `# 記録`.
const JA_TITLE: &str = "# \u{8a18}\u{9332}";

/// The environment of the runs with `CCDIG_LANG` set to `value`.
fn lang(value: &str) -> Environment {
    Environment {
        lang: Some(value.into()),
        ..environment()
    }
}

#[test]
fn lang_words_the_markdown_by_the_flag_then_the_variable() -> TestResult {
    let history = History::new("root")?;
    // (name, environment, the command line up to the subcommand's flags,
    // the first line)
    for (name, environment, args, first) in [
        ("default", environment(), &["records"][..], "# Records"),
        (
            "flag",
            environment(),
            &["records", "--lang", "ja"][..],
            JA_TITLE,
        ),
        (
            "flag en",
            environment(),
            &["records", "--lang", "en"][..],
            "# Records",
        ),
        ("variable", lang("ja"), &["records"][..], JA_TITLE),
        (
            "variable with spaces",
            lang(" ja "),
            &["records"][..],
            JA_TITLE,
        ),
        (
            "the flag wins",
            lang("ja"),
            &["records", "--lang", "en"][..],
            "# Records",
        ),
        (
            "the later flag wins",
            environment(),
            &["--lang", "en", "records", "--lang", "ja"][..],
            JA_TITLE,
        ),
        (
            "the earlier flag stays",
            environment(),
            &["--lang", "ja", "records"][..],
            JA_TITLE,
        ),
    ] {
        let mut full = args.to_vec();
        full.extend(["--type", "prompt", "--format", "md"]);
        full.extend(WEEK);
        let ran = history.run_in(&environment, &full);
        check_eq(&ran.code, &0, &format!("{name}: {}", ran.err))?;
        check_eq(
            &ran.out.lines().next(),
            &Some(first),
            &format!("{name}: {}", ran.out),
        )?;
    }
    Ok(())
}

#[test]
fn lang_is_an_error_for_json_and_the_variable_is_left_unused() -> TestResult {
    let history = History::new("root")?;
    let needs = "ccdig: --lang needs --format md, as json and jsonl write the records alone\n";
    // (name, environment, args, status, stderr)
    for (name, environment, args, code, err) in [
        (
            "records jsonl",
            environment(),
            &["records", "--lang", "ja"][..],
            1_u8,
            needs,
        ),
        (
            "records json, en too",
            environment(),
            &["records", "--format", "json", "--lang", "en"][..],
            1,
            needs,
        ),
        (
            "sessions jsonl",
            environment(),
            &["sessions", "--lang", "ja"][..],
            1,
            needs,
        ),
        (
            "stats jsonl",
            environment(),
            &["stats", "--lang", "ja"][..],
            1,
            needs,
        ),
        (
            "records jsonl, variable",
            lang("ja"),
            &["records"][..],
            0,
            "",
        ),
        (
            "digest",
            environment(),
            &["digest", "--lang", "ja"][..],
            0,
            "",
        ),
        (
            "an unknown variable",
            lang("fr"),
            &["records"][..],
            1,
            "ccdig: CCDIG_LANG: unknown value \"fr\": use en or ja\n",
        ),
        (
            "an unknown variable under --lang",
            lang("fr"),
            &["records", "--format", "md", "--lang", "en"][..],
            0,
            "",
        ),
        (
            "an unknown variable, md",
            lang("fr"),
            &["records", "--format", "md"][..],
            1,
            "ccdig: CCDIG_LANG: unknown value \"fr\": use en or ja\n",
        ),
    ] {
        let ran = history.run_in(&environment, args);
        check_eq(&ran.code, &code, &format!("{name}: {}", ran.err))?;
        check_eq(ran.err.as_str(), err, name)?;
    }
    let flag = history.run(&["records", "--format", "md", "--lang", "fr"]);
    check_eq(&flag.code, &1, &flag.err)?;
    check_eq(
        &flag.err.contains("invalid value 'fr' for '--lang <LANG>'"),
        &true,
        &flag.err,
    )?;
    Ok(())
}

#[test]
fn lang_leaves_json_lines_as_they_are() -> TestResult {
    let history = History::new("root")?;
    let args = ["records", "--type", "prompt,reply", "--message-chars", "5"];
    let plain = history.run(&args);
    let worded = history.run_in(&lang("ja"), &args);
    check_eq(&plain.code, &0, &plain.err)?;
    check_eq(&worded.out, &plain.out, "CCDIG_LANG=ja")?;
    check_eq(
        &plain.out.contains(" more characters, next: "),
        &true,
        &plain.out,
    )?;
    let sessions = ["sessions", "--igrep", "deploy"];
    check_eq(
        &history.run_in(&lang("ja"), &sessions).out,
        &history.run(&sessions).out,
        "sessions",
    )?;
    Ok(())
}

#[test]
fn lang_words_the_notes_of_a_cut_in_markdown() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/wwww.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"w-1","sessionId":"wwww","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"tool_use","id":"toolu_w1","name":"Write","input":{"file_path":"/tmp/a.md","content":"0123456789abcdefghij"}}]}}"#,
            r#"{"type":"user","uuid":"w-2","sessionId":"wwww","timestamp":"2026-10-15T00:00:11Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_w1","content":"line one\nline two matched here\nline three"}]}}"#,
        ],
    )?;
    let mut args = vec![
        "records",
        "--session",
        "wwww",
        "--type",
        "call,result",
        "--format",
        "md",
        "--lang",
        "ja",
        "--tool-chars",
        "6-10",
    ];
    args.extend(WEEK);
    let cut = history.run(&args);
    check_eq(&cut.code, &0, &cut.err)?;
    snapshot("cli/records-cut-ja.md", &cut.out)?;
    let mut grep = vec![
        "records",
        "--session",
        "wwww",
        "--type",
        "result",
        "--format",
        "md",
        "--lang",
        "ja",
        "--tool-chars",
        "8",
        "--grep",
        "matched",
        "--context",
        "0",
    ];
    grep.extend(WEEK);
    let lines = history.run(&grep);
    check_eq(&lines.code, &0, &lines.err)?;
    // line two… (残り 13 文字)
    check_eq(
        &lines
            .out
            .contains("line two\u{2026} (\u{6b8b}\u{308a} 13 \u{6587}\u{5b57})"),
        &true,
        &lines.out,
    )?;
    Ok(())
}

#[test]
fn the_help_names_the_languages_and_the_variable() -> TestResult {
    let help = History::new("root")?.run(&["records", "-h"]).out;
    for part in ["[env: `CCDIG_LANG`]", "[possible values: en, ja]"] {
        check_eq(&help.contains(part), &true, &help)?;
    }
    Ok(())
}

#[test]
fn lang_words_the_note_of_a_cut_first_match() -> TestResult {
    let history = History::new("root")?;
    let long = format!("deploy {}", "x".repeat(250));
    history.write(
        "-Users-alice-blog/ffff.jsonl",
        fixture_time()?,
        &[&format!(
            r#"{{"type":"user","uuid":"f-1","sessionId":"ffff","timestamp":"2026-10-15T00:00:00Z","message":{{"role":"user","content":"{long}"}}}}"#
        )],
    )?;
    let mut args = vec![
        "sessions",
        "--session",
        "ffff",
        "--igrep",
        "deploy",
        "--format",
        "md",
        "--lang",
        "ja",
    ];
    args.extend(WEEK);
    let ran = history.run(&args);
    check_eq(&ran.code, &0, &ran.err)?;
    // … (残り 57 文字)
    check_eq(
        &ran.out
            .contains("\u{2026} (\u{6b8b}\u{308a} 57 \u{6587}\u{5b57})"),
        &true,
        &ran.out,
    )?;
    Ok(())
}
