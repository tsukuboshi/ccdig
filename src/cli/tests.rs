//! Tests for the command line, against golden files and synthetic history.

mod kinds;

use core::error::Error;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jiff::Timestamp;
use tempfile::TempDir;

use crate::testing::{TestResult, check_eq, golden};

/// The flags that pin the sample week, in JST so the goldens do not
/// depend on the host's zone.
const WEEK: [&str; 6] = [
    "--since",
    "2026-10-13",
    "--until",
    "2026-10-19",
    "--tz",
    "Asia/Tokyo",
];

/// What a run printed and returned.
struct Ran {
    /// The exit status.
    code: u8,
    /// stderr, with the root shown as `$ROOT`.
    err: String,
    /// stdout, with the root shown as `$ROOT`.
    out: String,
}

/// A copy of a synthetic history under a temp dir.
struct History {
    /// Keeps the copy until the test ends.
    _dir: TempDir,
    /// The copied root.
    root: PathBuf,
}

impl History {
    /// Copies `testdata/<fixture>` and pins every file's mtime to
    /// 2026-10-14T01:00:00Z, since git does not keep mtimes.
    fn new(fixture: &str) -> Result<Self, Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().join("projects");
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("testdata")
                .join(fixture),
            &root,
            fixture_time()?,
        )?;
        Ok(Self { _dir: dir, root })
    }

    /// Runs ccdig with `args` and `--root` pointing at the copy.
    fn run(&self, args: &[&str]) -> Ran {
        let root = self.root.to_string_lossy().into_owned();
        let mut full = vec!["ccdig"];
        full.extend_from_slice(args);
        full.extend(["--root", &root]);
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = super::run(full, &mut out, &mut err);
        let shown = |bytes: &[u8]| String::from_utf8_lossy(bytes).replace(&root, "$ROOT");
        Ran {
            code,
            err: shown(&err),
            out: shown(&out),
        }
    }

    /// Runs ccdig with `args` followed by the sample week.
    fn run_week(&self, args: &[&str]) -> Ran {
        let mut full = args.to_vec();
        full.extend(WEEK);
        self.run(&full)
    }

    /// Adds a transcript at `relative` with `lines` and `mtime`, returning
    /// its path.
    fn write(
        &self,
        relative: &str,
        mtime: SystemTime,
        lines: &[&str],
    ) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.root.join(relative);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut text = lines.join("\n");
        text.push('\n');
        fs::write(&path, text)?;
        File::options()
            .write(true)
            .open(&path)?
            .set_modified(mtime)?;
        Ok(path)
    }
}

/// Copies the tree at `from` to `to`, setting every file's mtime to `mtime`.
fn copy_tree(from: &Path, to: &Path, mtime: SystemTime) -> TestResult {
    fs::create_dir_all(to)?;
    for item in fs::read_dir(from)? {
        let entry = item?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target, mtime)?;
        } else {
            fs::copy(entry.path(), &target)?;
            File::options()
                .write(true)
                .open(&target)?
                .set_modified(mtime)?;
        }
    }
    Ok(())
}

/// The mtime of every fixture file.
fn fixture_time() -> Result<SystemTime, jiff::Error> {
    time("2026-10-14T01:00:00Z")
}

/// Locks `path` against reading. `None` when permissions are not enforced
/// for this user, such as root, and the test should not run.
fn lock(path: &Path) -> Result<Option<()>, Box<dyn Error>> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o000))?;
    Ok(File::open(path).is_err().then_some(()))
}

/// The number of lines in `text`.
fn line_count(text: &str) -> usize {
    text.lines().count()
}

/// A time from an RFC 3339 string.
fn time(text: &str) -> Result<SystemTime, jiff::Error> {
    let instant: Timestamp = text.parse()?;
    SystemTime::try_from(instant)
}

#[test]
fn prompts_as_json_lines_match_the_golden() -> TestResult {
    let ran = History::new("root")?.run_week(&["prompts"]);
    check_eq(&ran.code, &0, "exit")?;
    golden("cli/prompts.jsonl.golden", &ran.out)
}

#[test]
fn sessions_as_markdown_and_json_lines_match_the_goldens() -> TestResult {
    let history = History::new("root")?;
    golden(
        "cli/sessions.md.golden",
        &history.run_week(&["sessions", "--format", "md"]).out,
    )?;
    golden(
        "cli/sessions.jsonl.golden",
        &history.run_week(&["sessions"]).out,
    )
}

#[test]
fn the_digest_matches_the_golden() -> TestResult {
    golden(
        "cli/digest.md.golden",
        &History::new("root")?.run_week(&["digest"]).out,
    )
}

#[test]
fn no_redact_leaves_the_secret_as_typed() -> TestResult {
    let out = History::new("root")?
        .run_week(&["prompts", "--no-redact"])
        .out;
    check_eq(
        &out.contains("AWS_SECRET_ACCESS_KEY=EXAMPLEEXAMPLEEXAMPLE"),
        &true,
        "secret shown",
    )?;
    check_eq(&out.contains(r#""redacted":true"#), &false, "no redaction")?;
    Ok(())
}

#[test]
fn project_matches_the_directory_name_or_the_cwd() -> TestResult {
    let history = History::new("root")?;
    for project in ["alice-work-notes", "work/notes"] {
        let out = history.run_week(&["prompts", "--project", project]).out;
        check_eq(&line_count(&out), &1, project)?;
        check_eq(&out.contains("64 KiB"), &true, project)?;
    }
    Ok(())
}

#[test]
fn shared_flags_go_before_or_after_the_subcommand_and_add_up() -> TestResult {
    let ran = History::new("root")?.run_week(&[
        "--project",
        "alice-blog",
        "prompts",
        "--project",
        "notes",
    ]);
    check_eq(&line_count(&ran.out), &3, "both projects")?;
    Ok(())
}

#[test]
fn bad_flags_fail_with_one_line() -> TestResult {
    let history = History::new("root")?;
    for (args, prefix) in [
        (
            &["prompts", "--format", "xml"][..],
            "ccdig: unknown --format",
        ),
        (
            &["prompts", "--since", "2026/10/13"][..],
            "ccdig: parse since:",
        ),
        (
            &["prompts", "--tz", "Mars/Olympus"][..],
            "ccdig: load time zone:",
        ),
        (
            &["digest", "--max-prompt-chars", "0"][..],
            "ccdig: --max-prompt-chars",
        ),
        (&["prompts", "--grep", "("][..], "ccdig: parse grep:"),
        (&["prompts", "--bogus"][..], "ccdig: unexpected argument"),
    ] {
        let ran = history.run(args);
        let name = args.join(" ");
        check_eq(&ran.code, &1, &name)?;
        check_eq(
            &ran.err.starts_with(prefix),
            &true,
            &format!("{name}: {}", ran.err),
        )?;
        check_eq(&line_count(&ran.err), &1, &name)?;
        check_eq(ran.out.as_str(), "", &name)?;
    }
    Ok(())
}

#[test]
fn the_digest_rejects_a_prompt_limit_not_below_the_session_limit() -> TestResult {
    let history = History::new("root")?;
    for (prompt, session) in [("300", "100"), ("100", "100")] {
        let ran = history.run(&[
            "digest",
            "--max-prompt-chars",
            prompt,
            "--max-session-chars",
            session,
        ]);
        check_eq(&ran.code, &1, &format!("{prompt} {session}"))?;
    }
    Ok(())
}

#[test]
fn version_and_help() -> TestResult {
    let history = History::new("root")?;
    let version = history.run(&["version"]);
    check_eq(version.out.as_str(), "ccdig v0.1.0\n", "version")?;
    for args in [&[][..], &["--help"][..], &["prompts", "--help"][..]] {
        let ran = history.run(args);
        let name = format!("help for {args:?}");
        check_eq(&ran.code, &0, &name)?;
        check_eq(&ran.out.contains("Usage: ccdig"), &true, &name)?;
        check_eq(ran.err.as_str(), "", &name)?;
    }
    Ok(())
}

#[test]
fn an_unreadable_file_is_skipped_with_a_warning() -> TestResult {
    let history = History::new("root")?;
    let bad = history.write(
        "-Users-alice-blog/dddd.jsonl",
        fixture_time()?,
        &[r#"{"type":"user","uuid":"d-1","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"unreadable"}}"#],
    )?;
    if lock(&bad)?.is_none() {
        return Ok(());
    }
    let ran = history.run_week(&["prompts"]);
    fs::set_permissions(&bad, fs::Permissions::from_mode(0o600))?;
    check_eq(&ran.code, &0, "exit")?;
    check_eq(&line_count(&ran.out), &3, "readable prompts")?;
    check_eq(
        &ran.err
            .contains("ccdig: warning: skipping $ROOT/-Users-alice-blog/dddd.jsonl"),
        &true,
        &ran.err,
    )?;
    Ok(())
}

#[test]
fn undecodable_and_untimed_lines_are_counted_in_a_warning() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/eeee.jsonl",
        fixture_time()?,
        &[r#"{"type":"user","uuid":"e-1","timestamp":"2026-10-15 00:00:00","message":{"role":"user","content":"odd timestamp"}}"#],
    )?;
    history.write(
        "-Users-alice-blog/hhhh.jsonl",
        fixture_time()?,
        &[r#"{"type":"user","uuid":"h-1","message":{"role":"user","content":"when was this?"}}"#],
    )?;
    let ran = history.run(&["prompts"]);
    check_eq(
        &ran.out.contains("when was this?"),
        &false,
        "untimed prompt",
    )?;
    check_eq(
        &ran.err.contains("ccdig: warning: skipped 1 undecodable, 1 timestamp-less, and 0 over-64-MiB line(s) in 2 file(s)"),
        &true,
        &ran.err,
    )?;
    Ok(())
}

#[test]
fn files_last_written_before_the_range_are_not_opened() -> TestResult {
    let history = History::new("root")?;
    let old = history.write(
        "-Users-alice-blog/ffff.jsonl",
        time("2026-09-01T00:00:00Z")?,
        &[r#"{"type":"user","uuid":"f-1","timestamp":"2026-09-01T00:00:00Z","message":{"role":"user","content":"old"}}"#],
    )?;
    if lock(&old)?.is_none() {
        return Ok(());
    }
    let ran = history.run_week(&["prompts"]);
    fs::set_permissions(&old, fs::Permissions::from_mode(0o600))?;
    check_eq(&ran.code, &0, "exit")?;
    check_eq(ran.err.as_str(), "", "stderr")?;
    Ok(())
}

#[test]
fn session_titles_are_masked() -> TestResult {
    let history = History::new("root")?;
    let key_id = format!("AKIA{}", "Q".repeat(16));
    let title = format!(r#"{{"type":"ai-title","sessionId":"g","aiTitle":"Rotate {key_id} key"}}"#);
    history.write(
        "-Users-alice-blog/gggg.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"g-1","sessionId":"g","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"rotate it"}}"#,
            &title,
        ],
    )?;
    let out = history.run_week(&["prompts", "--project", "blog"]).out;
    check_eq(&out.contains(&key_id), &false, "key shown")?;
    check_eq(
        &out.contains(r#""session_title":"Rotate ***REDACTED*** key""#),
        &true,
        "masked title",
    )?;
    Ok(())
}

#[test]
fn a_subagent_record_carries_the_masked_title_of_its_session() -> TestResult {
    let history = History::new("root")?;
    let key_id = format!("AKIA{}", "Q".repeat(16));
    let title =
        format!(r#"{{"type":"ai-title","sessionId":"hhhh","aiTitle":"Rotate {key_id} key"}}"#);
    history.write(
        "-Users-alice-blog/hhhh.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"h-1","sessionId":"hhhh","timestamp":"2026-10-15T00:00:00Z","message":{"content":[{"type":"tool_use","id":"toolu_h1","name":"Bash","input":{"command":"ls"}}]}}"#,
            &title,
        ],
    )?;
    history.write(
        "-Users-alice-blog/hhhh/subagents/agent-9.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"h-2","sessionId":"hhhh","isSidechain":true,"timestamp":"2026-10-15T00:01:00Z","message":{"content":[{"type":"tool_use","id":"toolu_h2","name":"Bash","input":{"command":"pwd"}}]}}"#,
        ],
    )?;
    let out = history
        .run_week(&["tools", "--subagents", "--session", "hhhh"])
        .out;
    let subagent = out
        .lines()
        .find(|line| line.contains(r#""subagent":"agent-9""#))
        .ok_or("no sub-agent record")?;
    check_eq(
        &subagent.contains(r#""session_title":"Rotate ***REDACTED*** key""#),
        &true,
        "sub-agent title",
    )?;
    check_eq(&out.contains(&key_id), &false, "key shown")?;
    Ok(())
}

#[test]
fn a_subagent_takes_the_title_even_when_its_session_has_no_selected_record() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/iiii.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"i-1","sessionId":"iiii","timestamp":"2026-10-15T00:00:00Z","message":{"content":[{"type":"tool_use","id":"toolu_i1","name":"Read","input":{"file_path":"a.md"}}]}}"#,
            r#"{"type":"ai-title","sessionId":"iiii","aiTitle":"Tidy the notes"}"#,
        ],
    )?;
    history.write(
        "-Users-alice-blog/iiii/subagents/agent-7.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"i-2","sessionId":"iiii","isSidechain":true,"timestamp":"2026-10-15T00:01:00Z","message":{"content":[{"type":"tool_use","id":"toolu_i2","name":"Bash","input":{"command":"pwd"}}]}}"#,
        ],
    )?;
    let out = history
        .run_week(&[
            "tools",
            "--subagents",
            "--tool",
            "Bash",
            "--session",
            "iiii",
        ])
        .out;
    check_eq(&out.lines().count(), &1, "records")?;
    check_eq(
        &out.contains(r#""session_title":"Tidy the notes""#),
        &true,
        "sub-agent title",
    )?;
    Ok(())
}

#[test]
fn a_subagent_of_a_session_without_a_title_has_an_empty_title() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/jjjj.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"j-1","sessionId":"jjjj","timestamp":"2026-10-15T00:00:00Z","message":{"content":[{"type":"tool_use","id":"toolu_j1","name":"Bash","input":{"command":"ls"}}]}}"#,
        ],
    )?;
    history.write(
        "-Users-alice-blog/jjjj/subagents/agent-8.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"j-2","sessionId":"jjjj","isSidechain":true,"timestamp":"2026-10-15T00:01:00Z","message":{"content":[{"type":"tool_use","id":"toolu_j2","name":"Bash","input":{"command":"pwd"}}]}}"#,
        ],
    )?;
    let out = history
        .run_week(&["tools", "--subagents", "--session", "jjjj"])
        .out;
    check_eq(&out.lines().count(), &2, "records")?;
    check_eq(
        &out.lines()
            .all(|line| line.contains(r#""session_title":"""#)),
        &true,
        "empty titles",
    )?;
    Ok(())
}

#[test]
fn prompts_copied_into_a_resumed_session_appear_once() -> TestResult {
    let history = History::new("root")?;
    let original = fs::read_to_string(
        history
            .root
            .join("-Users-alice-work-notes/bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb.jsonl"),
    )?;
    let first_line = original.lines().next().ok_or("empty fixture")?;
    history.write(
        "-Users-alice-work-notes/zzzz.jsonl",
        fixture_time()?,
        &[
            first_line,
            r#"{"type":"user","uuid":"z-1","sessionId":"z","timestamp":"2026-10-15T00:00:00Z","cwd":"/Users/alice/work/notes","message":{"role":"user","content":"continue"}}"#,
        ],
    )?;
    let out = history.run_week(&["prompts", "--project", "notes"]).out;
    check_eq(&out.matches(r#""uuid":"b-1""#).count(), &1, "b-1")?;
    check_eq(&out.contains(r#""uuid":"z-1""#), &true, "z-1")?;
    Ok(())
}

#[test]
fn a_resumed_copy_loses_ties_to_the_original() -> TestResult {
    // The older file is the original and keeps the shared entries, however
    // the random file names sort.
    let history = History::new("root")?;
    let shared = r#"{"type":"user","uuid":"r-1","timestamp":"2026-10-15T00:00:00Z","cwd":"/Users/alice/r","message":{"role":"user","content":"shared"}}"#;
    history.write(
        "-Users-alice-r/zz-original.jsonl",
        time("2026-10-15T01:00:00Z")?,
        &[shared],
    )?;
    history.write(
        "-Users-alice-r/aa-resumed.jsonl",
        time("2026-10-16T01:00:00Z")?,
        &[
            shared,
            r#"{"type":"user","uuid":"r-2","timestamp":"2026-10-16T00:00:00Z","cwd":"/Users/alice/r","message":{"role":"user","content":"new"}}"#,
        ],
    )?;
    let out = history.run_week(&["prompts", "--project", "alice-r"]).out;
    let lines: Vec<&str> = out.lines().collect();
    check_eq(&lines.len(), &2, "prompts")?;
    check_eq(
        &lines
            .first()
            .is_some_and(|line| line.contains("zz-original.jsonl")),
        &true,
        "r-1 from the original",
    )?;
    check_eq(
        &lines
            .get(1)
            .is_some_and(|line| line.contains(r#""uuid":"r-2""#)),
        &true,
        "r-2 from the copy",
    )?;
    Ok(())
}

#[test]
fn warnings_carry_no_control_characters() -> TestResult {
    let history = History::new("root")?;
    let bad = history.write(
        "-Users-alice-blog/x\u{1b}]0;PWNED\u{7}.jsonl",
        fixture_time()?,
        &[r#"{"type":"user"}"#],
    )?;
    if lock(&bad)?.is_none() {
        return Ok(());
    }
    let ran = history.run(&["prompts"]);
    fs::set_permissions(&bad, fs::Permissions::from_mode(0o600))?;
    check_eq(&ran.code, &0, "exit")?;
    check_eq(&ran.err.contains(['\u{1b}', '\u{7}']), &false, &ran.err)?;
    check_eq(&ran.err.contains("PWNED"), &true, &ran.err)?;
    Ok(())
}

#[test]
fn limit_prints_the_first_records() -> TestResult {
    let history = History::new("root")?;
    let out = history.run_week(&["prompts", "--limit", "2"]).out;
    check_eq(&line_count(&out), &2, "prompts")?;
    check_eq(&out.contains(r#""uuid":"b-1""#), &false, "third prompt")?;
    Ok(())
}
