//! The end-to-end cases.

use core::error::Error;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Output};

/// The variables ccdig reads its defaults from.
const VARIABLES: [&str; 6] = [
    "CCDIG_MASK",
    "CCDIG_MASK_VALUES",
    "CCDIG_ON_SKIP",
    "CCDIG_ROOT",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CONFIG_DIR",
];

/// Runs ccdig with `args` and `TZ` set to `tz`, without the variables in
/// `VARIABLES` that the test process has.
fn ccdig(args: &[&str], tz: &str) -> io::Result<Output> {
    ccdig_with(args, tz, &[])
}

/// Runs ccdig with `args`, `TZ` set to `tz`, and only the variables of
/// `VARIABLES` given in `vars`.
fn ccdig_with(args: &[&str], tz: &str, vars: &[(&str, &str)]) -> io::Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ccdig"));
    for name in VARIABLES {
        command.env_remove(name);
    }
    command
        .args(args)
        .env("TZ", tz)
        .envs(vars.iter().copied())
        .output()
}

/// The arguments listing the prompts of the sample week, plus `extra`.
fn week(root: &str, extra: &[&'static str]) -> Vec<String> {
    let mut args: Vec<String> = [
        "records",
        "--type",
        "prompt",
        "--root",
        root,
        "--since",
        "2026-10-13",
        "--until",
        "2026-10-19",
    ]
    .iter()
    .map(|&arg| arg.to_owned())
    .collect();
    args.extend(extra.iter().map(|&arg| arg.to_owned()));
    args
}

#[test]
fn day_boundaries_follow_the_time_zone() -> Result<(), Box<dyn Error>> {
    // Two prompts at 01:00 UTC on Oct 13 fall on Oct 12 in New York.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/root")
        .to_string_lossy()
        .into_owned();
    for (extra, tz, want) in [
        (&["--tz", "Asia/Tokyo"][..], "America/New_York", 3_usize),
        (&["--tz", "America/New_York"][..], "Asia/Tokyo", 1),
        (&[][..], "Asia/Tokyo", 3),
        (&[][..], "America/New_York", 1),
    ] {
        let args = week(&root, extra);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = ccdig(&refs, tz)?;
        let lines = String::from_utf8(output.stdout)?.lines().count();
        if !output.status.success() || lines != want {
            return Err(format!("{extra:?} under TZ={tz}: {lines} lines, want {want}").into());
        }
    }
    Ok(())
}

#[test]
fn an_error_exits_1_with_one_line_on_stderr() -> Result<(), Box<dyn Error>> {
    let output = ccdig(&["records", "--type", "prompt", "--format", "xml"], "UTC")?;
    let stderr = String::from_utf8(output.stderr)?;
    if output.status.code() != Some(1_i32)
        || !stderr.starts_with("ccdig: invalid value 'xml' for '--format <FORMAT>'")
        || stderr.lines().count() != 1
    {
        return Err(format!("status {:?}, stderr {stderr:?}", output.status).into());
    }
    Ok(())
}

#[test]
fn version_and_bare_help_exit_0() -> Result<(), Box<dyn Error>> {
    let version = ccdig(&["version"], "UTC")?;
    let help = ccdig(&[], "UTC")?;
    if !version.status.success()
        || !String::from_utf8(version.stdout)?.starts_with("ccdig v")
        || !help.status.success()
        || !String::from_utf8(help.stdout)?.contains("Usage: ccdig")
    {
        return Err("version or help failed".into());
    }
    Ok(())
}

#[test]
fn tools_print_the_untruncated_input_with_credentials_masked() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let project = root.path().join("-Users-alice-blog");
    fs::create_dir_all(&project)?;
    fs::write(
        project.join("dddddddd-dddd-4ddd-8ddd-dddddddddddd.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"a-1","sessionId":"dddddddd-dddd-4ddd-8ddd-dddddddddddd","#,
            r#""timestamp":"2026-10-13T01:00:00Z","message":{"content":[{"type":"tool_use","#,
            r#""id":"toolu_1","name":"Bash","input":{"command":"mysql --password=correcthorse"}}]},"#,
            r#""wireToolInputs":{"toolu_1":{"command":"mysql --password=correcthorse -e 'select 1'"}}}"#,
            "\n",
        ),
    )?;
    let output = ccdig(
        &[
            "records",
            "--type",
            "call",
            "--root",
            &root.path().to_string_lossy(),
        ],
        "UTC",
    )?;
    let stdout = String::from_utf8(output.stdout)?;
    let want = r#""input":{"command":"mysql --password=***REDACTED*** -e 'select 1'"}"#;
    if !output.status.success() || !stdout.contains(want) {
        return Err(format!("status {:?}, stdout {stdout:?}", output.status).into());
    }
    Ok(())
}

#[test]
fn ccdig_root_is_the_default_root_and_empty_variables_are_unset() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/root")
        .to_string_lossy()
        .into_owned();
    let week = [
        "records",
        "--type",
        "prompt",
        "--since",
        "2026-10-13",
        "--until",
        "2026-10-19",
    ];
    let from_variable = ccdig_with(
        &week,
        "Asia/Tokyo",
        &[("CCDIG_ROOT", &root), ("CCDIG_MASK", "")],
    )?;
    let lines = String::from_utf8(from_variable.stdout)?.lines().count();
    if !from_variable.status.success() || lines != 3 {
        return Err(format!(
            "CCDIG_ROOT: status {:?}, {lines} lines, stderr {:?}",
            from_variable.status,
            String::from_utf8_lossy(&from_variable.stderr)
        )
        .into());
    }
    let mut args = week.to_vec();
    args.extend(["--root", "/nonexistent/ccdig-root"]);
    let replaced = ccdig_with(&args, "Asia/Tokyo", &[("CCDIG_ROOT", &root)])?;
    let stderr = String::from_utf8(replaced.stderr)?;
    if replaced.status.code() != Some(1_i32) || !stderr.starts_with("ccdig: list projects:") {
        return Err(format!("--root: status {:?}, stderr {stderr:?}", replaced.status).into());
    }
    Ok(())
}

#[test]
fn session_current_reads_claude_code_session_id() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/root")
        .to_string_lossy()
        .into_owned();
    let args = week(&root, &["--session", "current"]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let inside = ccdig_with(
        &refs,
        "Asia/Tokyo",
        &[(
            "CLAUDE_CODE_SESSION_ID",
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        )],
    )?;
    let lines = String::from_utf8(inside.stdout)?.lines().count();
    if !inside.status.success() || lines != 1 {
        return Err(format!("inside: status {:?}, {lines} lines", inside.status).into());
    }
    let outside = ccdig(&refs, "Asia/Tokyo")?;
    if outside.status.code() != Some(1_i32) {
        return Err(format!("outside: status {:?}", outside.status).into());
    }
    Ok(())
}
