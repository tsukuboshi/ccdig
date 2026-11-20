//! The end-to-end cases.

use core::error::Error;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Output};

/// Runs ccdig with `args` and `TZ` set to `tz`.
fn ccdig(args: &[&str], tz: &str) -> io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_ccdig"))
        .args(args)
        .env("TZ", tz)
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
