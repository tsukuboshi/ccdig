//! Properties of the command line that hold for any arguments.

use core::iter;

use proptest::collection::vec;
use proptest::prop_oneof;
use proptest::sample::select;
use proptest::strategy::Strategy;

use super::{History, environment};
use crate::cli::environment::Environment;
use crate::cli::resolve::user_name;
use crate::cli::values::parse;
use crate::testing::{
    TestResult, check_eq, drives_the_terminal, for_all, for_all_cases, hostile_text,
};

/// Command lines tried; each runs the whole pipeline on the sample history.
const RUN_CASES: u32 = 64;

/// One option of a command line, mostly with a value that makes sense
/// and sometimes with any text. `--root` is left out so that no run walks
/// a real directory.
fn option() -> impl Strategy<Value = Vec<String>> {
    let pair = |flag: &'static str, values: &'static [&'static str]| {
        let value = prop_oneof![4 => select(values).prop_map(str::to_owned), 1 => hostile_text()];
        value.prop_map(move |text| vec![flag.to_owned(), text])
    };
    let switch = select(
        &[
            "--no-mask",
            "--mask-report",
            "--subagents",
            "--usage",
            "--help",
        ][..],
    )
    .prop_map(|flag| vec![flag.to_owned()]);
    prop_oneof![
        pair("--format", &["json", "jsonl", "md"]),
        pair("--since", &["2026-10-13", "2026-10-15", ""]),
        pair("--until", &["2026-10-14", "2026-10-20"]),
        pair("--tz", &["Asia/Tokyo", "UTC", "America/New_York"]),
        pair(
            "--mask",
            &["email", "ip,user", "git-user,aws-account,aws-profile"]
        ),
        pair(
            "--mask-values",
            &["{\"repo\":[\"blog\"]}", "{\"\":[\"alice\"]}"]
        ),
        pair("--type", &["prompt", "reply,call", "result,summary"]),
        pair("--grep", &["report", "(?i)WEEK", "!draft"]),
        pair("--igrep", &["REPORT"]),
        pair("--context", &["0", "2", "-1"]),
        pair("--around", &["0", "1", "-1"]),
        pair("--session", &["1", "!2", "current", "!current"]),
        pair("--project", &["blog", "!corp"]),
        pair("--on-skip", &["warn", "fail", "ignore"]),
        pair("--fields", &["text", "!usage,!cwd", "type,!type", "nope"]),
        pair("--max-message-chars", &["1", "100", "0"]),
        pair("--max-tool-chars", &["1", "100", "-1"]),
        switch,
        hostile_text().prop_map(|text| vec![text]),
    ]
}

/// What a git with nothing configured answers.
const fn no_git_config(_: &str) -> Option<String> {
    None
}

#[test]
fn any_command_line_exits_0_or_1_and_writes_no_terminal_controls_to_stderr() -> TestResult {
    let history = History::new("root")?;
    let environment = Environment {
        git_config: Some(no_git_config),
        tz: Some("UTC".into()),
        ..environment()
    };
    let commands = select(&["records", "prompts", "sessions", "digest"][..]);
    for_all_cases(
        RUN_CASES,
        &(commands, vec(option(), 0..5)),
        |(command, options)| {
            let args: Vec<&str> = iter::once(command)
                .chain(options.iter().flatten().map(String::as_str))
                .collect();
            let ran = history.run_in(&environment, &args);
            check_eq(
                &(ran.code <= 1),
                &true,
                &format!("exit status {}", ran.code),
            )?;
            let bad: Vec<char> = ran
                .err
                .chars()
                .filter(|&ch| drives_the_terminal(ch))
                .collect();
            check_eq(&bad, &Vec::new(), &ran.err)?;
            Ok(())
        },
    )
}

#[test]
fn a_broken_values_spec_never_shows_its_values_in_the_error() -> TestResult {
    let shapes = select(
        &[
            "{\"repo\": [\"SECRET\"",
            "{\"repo\": \"SECRET\"}",
            "[\"SECRET\"]",
            "{\"repo\": [[\"SECRET\", 1]]}",
            "{\"repo\": [\"SECRET\"]} trailing",
            "SECRET",
        ][..],
    );
    for_all(&(shapes, "[A-Za-z]{8,16}"), |(shape, secret)| {
        let spec = shape.replace("SECRET", &secret);
        let Err(error) = parse(&[spec], |_| Ok(String::new())) else {
            return Err("a broken spec was read".into());
        };
        let message = error.to_string();
        check_eq(&message.contains(&secret), &false, &message)?;
        Ok(())
    })
}

#[test]
fn the_user_name_is_the_last_part_of_home() -> TestResult {
    let names = "[A-Za-z0-9_-][A-Za-z0-9._-]{0,11}";
    let cases = (
        select(&["/home/", "/Users/", "/"][..]),
        names,
        select(&["", "/", "//"][..]),
    );
    for_all(&cases, |(base, name, slashes)| {
        check_eq(
            &user_name(&format!("{base}{name}{slashes}")),
            &Some(name),
            "name",
        )?;
        Ok(())
    })
}

#[test]
fn a_user_name_never_holds_a_slash() -> TestResult {
    for_all(&hostile_text(), |home| {
        let name = user_name(&home);
        check_eq(
            &name.as_deref().is_some_and(|found| found.contains('/')),
            &false,
            "slash",
        )?;
        Ok(())
    })
}
