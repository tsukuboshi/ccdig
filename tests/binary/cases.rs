//! The end-to-end cases.

use core::error::Error;
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

/// The `prompts` arguments for the sample week, plus `extra`.
fn week(root: &str, extra: &[&'static str]) -> Vec<String> {
    let mut args: Vec<String> = [
        "prompts",
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
    let output = ccdig(&["prompts", "--format", "xml"], "UTC")?;
    let stderr = String::from_utf8(output.stderr)?;
    if output.status.code() != Some(1_i32)
        || !stderr.starts_with("ccdig: unknown --format")
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
