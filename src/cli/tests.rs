//! Tests for the command line, against snapshots and synthetic history.

#![expect(
    clippy::disallowed_methods,
    reason = "the tests build transcript trees on disk"
)]

mod around;
mod context;
mod denials;
mod fields;
mod html;
mod kinds;
mod lang;
mod mentions;
mod overview;
mod properties;
mod responses;
mod resume;
mod stats;
mod subagents;
mod timeline;
mod touched;

use core::error::Error;
use std::fs::{self, File};
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::environment::Environment;
use super::resolve::user_name;
use crate::pipeline::VERSION;
use crate::testing::{TestResult, check_eq, snapshot, time, write_transcript};

/// The home directory the runs see, whatever the test process has.
const HOME: &str = "/home/tester";

/// The flags that pin the sample week, in JST so the snapshots do not
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

    /// Runs ccdig with `args` and `--root` pointing at the copy, and no
    /// `CCDIG_*` variables, whatever the test process has.
    fn run(&self, args: &[&str]) -> Ran {
        self.run_in(&environment(), args)
    }

    /// Runs ccdig with `args` alone, without `--root`, and the variables in
    /// `environment`.
    fn run_bare(&self, environment: &Environment, args: &[&str]) -> Ran {
        let root = self.root.to_string_lossy().into_owned();
        let mut full = vec!["ccdig"];
        full.extend_from_slice(args);
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = super::run_in(full, environment, &mut out, &mut err);
        let shown = |bytes: &[u8]| String::from_utf8_lossy(bytes).replace(&root, "$ROOT");
        Ran {
            code,
            err: shown(&err),
            out: shown(&out),
        }
    }

    /// Runs ccdig with `args`, `--root` pointing at the copy, and the
    /// `CCDIG_*` variables in `environment`.
    fn run_in(&self, environment: &Environment, args: &[&str]) -> Ran {
        let root = self.root.to_string_lossy().into_owned();
        let mut full = args.to_vec();
        full.extend(["--root", &root]);
        self.run_bare(environment, &full)
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
        write_transcript(&path, mtime, lines)?;
        Ok(path)
    }
}

/// The JSON Lines `ran` printed, parsed, or its stderr when it failed.
fn json_lines(ran: Ran) -> Result<Vec<Value>, Box<dyn Error>> {
    if ran.code != 0 {
        return Err(ran.err.into());
    }
    Ok(ran
        .out
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?)
}

/// The one session `sessions --session <id>` lists, run with `extra`
/// flags.
fn the_session(history: &History, id: &str, extra: &[&str]) -> Result<Value, Box<dyn Error>> {
    let mut args = vec!["sessions", "--session", id];
    args.extend_from_slice(extra);
    let sessions = json_lines(history.run_week(&args))?;
    let [ref session] = sessions[..] else {
        return Err(format!("want one session, got {}", sessions.len()).into());
    };
    Ok(session.clone())
}

/// The string at `key` of each record, or `<missing>`.
fn strings<'records>(records: &'records [Value], key: &str) -> Vec<&'records str> {
    records
        .iter()
        .map(|record| {
            record
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or("<missing>")
        })
        .collect()
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

#[test]
fn prompts_as_json_lines_match_the_snapshot() -> TestResult {
    let ran = History::new("root")?.run_week(&["records", "--type", "prompt"]);
    check_eq(&ran.code, &0, "exit")?;
    snapshot("cli/prompts.jsonl", &ran.out)
}

#[test]
fn sessions_as_markdown_and_json_lines_match_the_snapshots() -> TestResult {
    let history = History::new("root")?;
    snapshot(
        "cli/sessions.md",
        &history.run_week(&["sessions", "--format", "md"]).out,
    )?;
    snapshot("cli/sessions.jsonl", &history.run_week(&["sessions"]).out)
}

#[test]
fn the_digest_matches_the_snapshot() -> TestResult {
    snapshot(
        "cli/digest.md",
        &History::new("root")?.run_week(&["digest"]).out,
    )
}

#[test]
fn no_mask_leaves_the_secret_as_typed() -> TestResult {
    let out = History::new("root")?
        .run_week(&["records", "--type", "prompt", "--no-mask"])
        .out;
    check_eq(
        &out.contains("AWS_SECRET_ACCESS_KEY=EXAMPLEEXAMPLEEXAMPLE"),
        &true,
        "secret shown",
    )?;
    check_eq(&out.contains(r#""redacted":true"#), &false, "no redaction")?;
    Ok(())
}

/// An environment with only `HOME` set, to `HOME`.
fn environment() -> Environment {
    Environment {
        home: Some(HOME.into()),
        ..Environment::default()
    }
}

/// A history with one prompt that holds an email address, the home
/// directory, an IP address, and a value for `--mask-values`, in a project
/// under the home directory. Returns it with `@` and the values file's
/// path.
fn history_with_personal_data() -> Result<(History, String), Box<dyn Error>> {
    let history = History::new("root")?;
    let home = HOME;
    // The JSON string of the home directory, without its quotes.
    let escaped = serde_json::to_string(&home)?;
    let quoted = escaped.trim_matches('"');
    let project = encoded(home);
    let prompt = format!(
        r#"{{"type":"user","uuid":"m-1","sessionId":"mmmm","cwd":"{quoted}/blog","timestamp":"2026-10-15T00:00:00Z","message":{{"role":"user","content":"mail bob@example.com about {quoted}/blog on 10.1.2.3 for corp-hidden"}}}}"#
    );
    history.write(
        &format!("{project}-blog/mmmm.jsonl"),
        fixture_time()?,
        &[&prompt],
    )?;
    let values = history.root.with_file_name("mask.json");
    fs::write(&values, r#"{"": ["corp-hidden"]}"#)?;
    Ok((history, format!("@{}", values.to_string_lossy())))
}

/// `path` as Claude Code names a project directory after it.
fn encoded(path: &str) -> String {
    path.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

#[test]
fn mask_hides_emails_the_user_name_and_ips_and_mask_values_hides_its_values() -> TestResult {
    let parent = "/home/";
    let (history, values) = history_with_personal_data()?;
    let ran = history.run_week(&[
        "records",
        "--type",
        "prompt",
        "--session",
        "mmmm",
        "--mask",
        "email,user",
        "--mask",
        "ip",
        "--mask-values",
        &values,
    ]);
    check_eq(&ran.code, &0, "exit")?;
    let cwd = format!("{parent}***REDACTED***/blog");
    check_eq(
        &ran.out.contains(&format!(
            r#""text":"mail ***REDACTED*** about {cwd} on ***REDACTED*** for ***REDACTED***""#
        )),
        &true,
        &ran.out,
    )?;
    check_eq(
        &ran.out.contains(&format!(r#""cwd":"{cwd}""#)),
        &true,
        "cwd",
    )?;
    let project = format!("{}***REDACTED***-blog", encoded(parent));
    check_eq(
        &ran.out.contains(&format!(r#""project":"{project}""#)),
        &true,
        "project",
    )?;
    check_eq(&ran.out.contains(r#""redacted":true"#), &true, "redacted")?;
    let sessions = history
        .run_week(&["sessions", "--session", "mmmm", "--mask", "user"])
        .out;
    check_eq(
        &sessions.contains(&format!(r#""cwd":"{cwd}""#)),
        &true,
        "session cwd",
    )?;
    check_eq(
        &sessions.contains(&format!(r#""project":"{project}""#)),
        &true,
        "session project",
    )?;
    Ok(())
}

#[test]
fn the_user_name_is_the_last_part_of_the_home_directory() -> TestResult {
    check_eq(
        &user_name("/Users/alice.b"),
        &Some("alice.b".to_owned()),
        "plain",
    )?;
    check_eq(
        &user_name("/home/alice/"),
        &Some("alice".to_owned()),
        "trailing slash",
    )?;
    check_eq(&user_name("/"), &None, "root")?;
    check_eq(&user_name(""), &None, "empty")?;
    Ok(())
}

#[test]
fn without_mask_the_personal_data_stays() -> TestResult {
    let (history, _) = history_with_personal_data()?;
    let out = history
        .run_week(&["records", "--type", "prompt", "--session", "mmmm"])
        .out;
    check_eq(&out.contains("bob@example.com"), &true, "email")?;
    check_eq(&out.contains("10.1.2.3"), &true, "ip")?;
    check_eq(&out.contains("corp-hidden"), &true, "value")?;
    Ok(())
}

#[test]
fn mask_values_are_masked_in_the_metadata_too() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-corp-hidden-portal/nnnn.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"n-1","sessionId":"nnnn","cwd":"/work/corp-hidden","gitBranch":"feat/corp-hidden","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"check it"}}"#,
            r#"{"type":"assistant","uuid":"n-2","sessionId":"nnnn","cwd":"/work/corp-hidden","gitBranch":"feat/corp-hidden","timestamp":"2026-10-15T00:00:10Z","message":{"model":"corp-hidden-model","content":[{"type":"text","text":"ok"},{"type":"tool_use","id":"toolu_n1","name":"mcp__corp-hidden__x","input":{}}]}}"#,
        ],
    )?;
    // A BOM, as an editor may save one; and an empty object given after the
    // command, which must not replace the values.
    let values = history.root.with_file_name("values.json");
    fs::write(&values, "\u{feff}{\"\": [\"corp-hidden\"]}\n")?;
    let values_path = format!("@{}", values.to_string_lossy());
    for format in ["jsonl", "md"] {
        let ran = history.run_week(&[
            "--mask-values",
            &values_path,
            "records",
            "--project",
            "corp-hidden",
            "--type",
            "prompt,reply,call",
            "--mask-values",
            "{}",
            "--format",
            format,
        ]);
        check_eq(&ran.code, &0, format)?;
        check_eq(&ran.out.is_empty(), &false, format)?;
        check_eq(
            &ran.out.contains("corp-hidden"),
            &false,
            &format!("{format}: {}", ran.out),
        )?;
    }
    Ok(())
}

#[test]
fn no_mask_with_a_mask_is_an_error() -> TestResult {
    let history = History::new("root")?;
    for args in [
        [
            "records",
            "--type",
            "prompt",
            "--no-mask",
            "--mask",
            "email",
        ],
        [
            "records",
            "--type",
            "prompt",
            "--no-mask",
            "--mask-values",
            "{}",
        ],
    ] {
        let ran = history.run_week(&args);
        check_eq(&(ran.code != 0), &true, args[3])?;
        check_eq(&ran.out.is_empty(), &true, args[3])?;
        check_eq(&ran.err.contains("--no-mask"), &true, &ran.err)?;
    }
    Ok(())
}
#[test]
fn an_unknown_mask_or_a_missing_mask_values_file_is_an_error() -> TestResult {
    let history = History::new("root")?;
    let unknown = history.run_week(&["records", "--type", "prompt", "--mask", "phone"]);
    check_eq(&(unknown.code != 0), &true, "unknown mask")?;
    let missing = history.run_week(&[
        "records",
        "--type",
        "prompt",
        "--mask-values",
        "@/nonexistent/mask.json",
    ]);
    check_eq(&(missing.code != 0), &true, "missing file")?;
    check_eq(
        &missing.err.contains("/nonexistent/mask.json"),
        &true,
        "names the file",
    )?;
    Ok(())
}

/// A history with one session that has two prompts, a reply with a
/// thinking block and a tool call, and the tool's result.
fn history_with_a_conversation() -> Result<History, Box<dyn Error>> {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/cccc.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"c-1","sessionId":"cccc","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"list the posts"}}"#,
            r#"{"type":"assistant","uuid":"c-2","sessionId":"cccc","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"thinking","thinking":"private musing"},{"type":"text","text":"Listing them."},{"type":"tool_use","id":"toolu_c1","name":"Bash","input":{"command":"ls posts"}}]}}"#,
            r#"{"type":"user","uuid":"c-3","sessionId":"cccc","timestamp":"2026-10-15T00:00:20Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_c1","content":"a.md"}]}}"#,
            r#"{"type":"user","uuid":"c-4","sessionId":"cccc","timestamp":"2026-10-15T00:01:00Z","message":{"role":"user","content":"thanks"}}"#,
            r#"{"type":"ai-title","sessionId":"cccc","aiTitle":"List posts"}"#,
        ],
    )?;
    Ok(history)
}

/// The `type` of each JSON line in `out`.
fn types(out: &str) -> Vec<&str> {
    out.lines()
        .filter_map(|line| line.split(r#""type":""#).nth(1))
        .filter_map(|rest| rest.split('"').next())
        .collect()
}

#[test]
fn records_list_prompts_and_replies_in_order_by_default() -> TestResult {
    let ran = history_with_a_conversation()?.run_week(&["records", "--session", "cccc"]);
    check_eq(&ran.code, &0, "exit")?;
    check_eq(
        &types(&ran.out),
        &vec!["prompt", "reply", "prompt"],
        "order",
    )?;
    check_eq(&ran.out.contains("private musing"), &false, "thinking")?;
    check_eq(&ran.out.contains("a.md"), &false, "tool result")?;
    check_eq(
        &ran.out.contains(r#""session_title":"List posts""#),
        &true,
        "title",
    )?;
    Ok(())
}

#[test]
fn type_picks_the_kinds_of_record() -> TestResult {
    let history = history_with_a_conversation()?;
    for (args, want) in [
        (&["--type", "prompt"][..], vec!["prompt", "prompt"]),
        (
            &["--type", "prompt,result"][..],
            vec!["prompt", "result", "prompt"],
        ),
        (
            &["--type", "call", "--type", "reply"][..],
            vec!["reply", "call"],
        ),
        (&["--type", "call,result"][..], vec!["call", "result"]),
        (&["--type", "thinking"][..], vec!["thinking"]),
        (&["--type", "reply,thinking"][..], vec!["thinking", "reply"]),
    ] {
        let mut full = vec!["records", "--session", "cccc"];
        full.extend_from_slice(args);
        let ran = history.run_week(&full);
        check_eq(&ran.code, &0, &ran.err)?;
        check_eq(&types(&ran.out), &want, &args.join(" "))?;
    }
    let unknown = history.run_week(&["records", "--type", "musing"]);
    check_eq(&unknown.code, &1, "unknown type")?;
    check_eq(
        &unknown
            .err
            .starts_with("ccdig: invalid value 'musing' for '--type"),
        &true,
        &unknown.err,
    )?;
    Ok(())
}

#[test]
fn records_in_markdown_read_as_one_thread() -> TestResult {
    let out = history_with_a_conversation()?
        .run_week(&[
            "records",
            "--session",
            "cccc",
            "--type",
            "prompt,reply,call",
            "--format",
            "md",
        ])
        .out;
    let positions: Vec<Option<usize>> = [
        "# Records",
        "## List posts",
        "- Prompt\n\n```text\nlist the posts",
        "- Reply\n\n```text\nListing them.",
        "- Tool call\n\n- Tool: `Bash`",
        "- Prompt\n\n```text\nthanks",
    ]
    .iter()
    .map(|part| out.find(part))
    .collect();
    check_eq(
        &positions.iter().all(Option::is_some),
        &true,
        &format!("parts in {out}"),
    )?;
    check_eq(&positions.is_sorted(), &true, "parts in order")?;
    Ok(())
}

#[test]
fn project_matches_the_directory_name_or_the_cwd() -> TestResult {
    let history = History::new("root")?;
    for project in ["alice-work-notes", "work/notes"] {
        let out = history
            .run_week(&["records", "--type", "prompt", "--project", project])
            .out;
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
        "records",
        "--type",
        "prompt",
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
            &["records", "--type", "prompt", "--format", "xml"][..],
            "ccdig: invalid value 'xml' for '--format <FORMAT>'",
        ),
        (
            &["records", "--type", "prompt", "--since", "2026/10/13"][..],
            "ccdig: parse since:",
        ),
        (
            &["records", "--type", "prompt", "--tz", "Mars/Olympus"][..],
            "ccdig: load time zone:",
        ),
        (
            &["digest", "--max-prompt-chars", "0"][..],
            "ccdig: --max-prompt-chars",
        ),
        (
            &["records", "--type", "prompt", "--grep", "("][..],
            "ccdig: parse grep:",
        ),
        (
            &["sessions", "--format", "digest"][..],
            "ccdig: invalid value 'digest' for '--format <FORMAT>'",
        ),
        (
            &["records", "--type", "prompt", "--bogus"][..],
            "ccdig: unexpected argument",
        ),
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
fn a_flag_named_after_the_kind_key_points_at_type() -> TestResult {
    let history = History::new("root")?;
    let pointer =
        "pick the record types with --type: call, prompt, reply, result, summary, or thinking";
    let elsewhere = "records, stats, and digest pick prompts by how they were typed with --kind";
    for (args, want) in [
        // Where --kind is a flag, a record type is not a kind it takes.
        (
            &["records", "--kind", "prompt"][..],
            format!(
                "ccdig: --kind takes text, slash, rejection, answer, typed-answer, or approval; {pointer}\n"
            ),
        ),
        // Even with a flag that needs another type.
        (
            &["records", "--type", "call", "--kind", "call"][..],
            format!(
                "ccdig: --kind takes text, slash, rejection, answer, typed-answer, or approval; {pointer}\n"
            ),
        ),
        (
            &["records", "--kind=!call"][..],
            format!(
                "ccdig: --kind takes text, slash, rejection, answer, typed-answer, or approval; {pointer}\n"
            ),
        ),
        (
            &["records", "--kinds", "prompt"][..],
            format!("ccdig: unexpected argument '--kinds' found; {elsewhere}; {pointer}\n"),
        ),
        (
            &["--kind", "prompt", "records"][..],
            format!("ccdig: unexpected argument '--kind' found; {elsewhere}; {pointer}\n"),
        ),
        (
            &["sessions", "--kind", "text"][..],
            format!("ccdig: unexpected argument '--kind' found; {elsewhere}; {pointer}\n"),
        ),
    ] {
        let ran = history.run(args);
        let name = args.join(" ");
        check_eq(&ran.code, &1, &name)?;
        check_eq(&ran.err, &want, &name)?;
    }
    let other = history.run(&["records", "--kindly"]);
    check_eq(
        other.err.as_str(),
        "ccdig: unexpected argument '--kindly' found\n",
        "a flag that only starts with --kind",
    )?;
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
    check_eq(&version.out, &format!("ccdig v{VERSION}\n"), "version")?;
    let text = history.run(&["version", "--format", "text"]);
    check_eq(
        &text.out,
        &format!("ccdig v{VERSION}\n"),
        "version --format text",
    )?;
    let json = history.run(&["version", "--format", "json"]);
    check_eq(&json.code, &0, &json.err)?;
    check_eq(
        &json.out,
        &format!("{{\"version\":\"{VERSION}\",\"schema\":12,\"filter\":\"strict-v5\"}}\n"),
        "version --format json",
    )?;
    for args in [&[][..], &["--help"][..], &["records", "--help"][..]] {
        let ran = history.run(args);
        let name = format!("help for {args:?}");
        check_eq(&ran.code, &0, &name)?;
        check_eq(&ran.out.contains("Usage: ccdig"), &true, &name)?;
        check_eq(ran.err.as_str(), "", &name)?;
    }
    Ok(())
}

#[test]
fn help_shows_examples_and_where_the_record_flags_are() -> TestResult {
    let history = History::new("root")?;
    let examples = [
        "Examples:\n",
        "  # Share a log with people\n",
        "  ccdig records --session 1a2b --type prompt,reply,call --format html --mask-report text > session.html\n",
        "  # Search past sessions\n",
        "  ccdig sessions --igrep deploy --sort matches --format md --limit 10\n",
        "  # Review how you use Claude Code\n",
        "  ccdig records --type result --tool Bash --status error --subagents --format md\n",
        "  # Count usage\n",
        "  ccdig stats --by project,month --since 2026-11-01\n",
        "ccdig records --help lists --type, --tool, --status, --skill, --subagents, --around, and --context.",
    ];
    for args in [&[][..], &["--help"][..], &["-h"][..]] {
        let out = history.run(args).out;
        check_eq(&in_order(&out, &examples), &true, &out)?;
    }
    let records = history.run(&["records", "--help"]).out;
    for part in [
        "Without --type or a flag for some kinds, such as --tool or --kind, a search\nreads prompts and replies",
        "add the tool calls with --type prompt,reply,call.\nA tool call's input can be long",
        "ccdig records --type prompt,reply,call --igrep deploy \\\n    --message-chars 500",
        "Also read sub-agent transcripts, which are left out by default",
        "--type call shows each call with its result, so the answers to a\nquestion are not left out",
        "  ccdig records --type prompt,reply,call --igrep deploy --around 2 --format md\n",
        "  ccdig records --type call --tool Bash --igrep error --context 3 --format md\n",
        "  ccdig records --type call --tool-chars 2001-4000\n",
    ] {
        check_eq(&records.contains(part), &true, &records)?;
    }
    let sessions = history.run(&["sessions", "--help"]).out;
    for part in [
        "--grep and --igrep search the prompts and replies, and count the matches\nof each session",
        "  ccdig sessions --igrep deploy --type prompt,reply,call,result --subagents --format md\n",
        "in sessions, they search the records of the types --type picks",
    ] {
        check_eq(&sessions.contains(part), &true, &sessions)?;
    }
    let top = history.run(&["--help"]).out;
    check_eq(
        &top.contains(
            "Your user name and other personal data stay unless you\nadd them, as with --mask user",
        ),
        &true,
        &top,
    )?;
    check_eq(&top.contains("kind of record"), &false, &top)?;
    check_eq(&top.contains("record type"), &true, &top)?;
    check_eq(
        &top.contains("JSON Lines by default, on a terminal and in a pipe alike"),
        &true,
        &top,
    )?;
    let long = history.run(&["records", "--help"]).out;
    for part in [
        "JSON Lines by default, on a terminal and in a pipe alike; md to read",
        "--around lists the records and --context the lines around a match",
    ] {
        check_eq(&long.contains(part), &true, &long)?;
    }
    Ok(())
}

/// Whether each of `parts` is in `text`, in that order.
fn in_order(text: &str, parts: &[&str]) -> bool {
    let positions: Vec<Option<usize>> = parts.iter().map(|part| text.find(part)).collect();
    positions.iter().all(Option::is_some) && positions.is_sorted()
}

#[test]
fn help_lists_the_commands_and_the_flags_by_use() -> TestResult {
    let history = History::new("root")?;
    let top = history.run(&["--help"]).out;
    check_eq(
        &in_order(
            &top,
            &[
                "  records ",
                "  sessions ",
                "  stats ",
                "  digest ",
                "  version ",
            ],
        ),
        &true,
        &top,
    )?;
    let records = history.run(&["records", "-h"]).out;
    check_eq(
        &in_order(
            &records,
            &[
                "Records:\n",
                "--type <TYPE>",
                "Filters:\n",
                "--since <WHEN>",
                "--until <WHEN>",
                "--tz <ZONE>",
                "--project <SUBSTRING>",
                "--session <PREFIX>",
                "--branch <NAME>",
                "--grep <REGEX>",
                "--igrep <REGEX>",
                "--tool <NAME>",
                "--status <STATUS>",
                "--denial <KIND>",
                "--skill <NAME>",
                "--kind <KIND>",
                "--superseded <BOOL>",
                "--touched <PATH>",
                "Input:\n",
                "--root <DIR>",
                "--subagents",
                "Masking:\n",
                "--mask <KIND>",
                "--mask-values <JSON>",
                "--no-mask",
                "Output:\n",
                "--format <FORMAT>",
                "--limit <N>",
                "--message-chars <N|FROM-TO>",
                "--tool-chars <N|FROM-TO>",
                "--fields <KEY>",
                "--lang <LANG>",
            ],
        ),
        &true,
        &records,
    )?;
    let sessions = history.run(&["sessions", "-h"]).out;
    check_eq(
        &in_order(
            &sessions,
            &[
                "Output:\n",
                "--format <FORMAT>",
                "--limit <N>",
                "--fields <KEY>",
                "--sort <SORT>",
                "--lang <LANG>",
            ],
        ),
        &true,
        &sessions,
    )?;
    check_eq(
        &records.contains("[possible values: html, json, jsonl, md]"),
        &true,
        "format values",
    )?;
    let digest = history.run(&["digest", "-h"]).out;
    check_eq(
        &in_order(
            &digest,
            &[
                "Output:\n",
                "--limit <N>",
                "--max-prompt-chars",
                "--max-session-chars",
                "--lang <LANG>",
            ],
        ),
        &true,
        &digest,
    )?;
    check_eq(
        &digest.contains("--type <TYPE>"),
        &false,
        "digest has no --type",
    )?;
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
    let ran = history.run_week(&["records", "--type", "prompt"]);
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
    let ran = history.run(&["records", "--type", "prompt"]);
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
    let ran = history.run_week(&["records", "--type", "prompt"]);
    fs::set_permissions(&old, fs::Permissions::from_mode(0o600))?;
    check_eq(&ran.code, &0, "exit")?;
    check_eq(ran.err.as_str(), "", "stderr")?;
    Ok(())
}

#[test]
fn files_of_other_sessions_are_not_opened() -> TestResult {
    let history = History::new("root")?;
    let other = history.write(
        "-Users-alice-blog/ffff.jsonl",
        fixture_time()?,
        &[r#"{"type":"user","uuid":"f-1","sessionId":"ffff","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"other"}}"#],
    )?;
    history.write(
        "-Users-alice-blog/gggg.jsonl",
        fixture_time()?,
        &[r#"{"type":"user","uuid":"g-1","sessionId":"gggg","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"mine"}}"#],
    )?;
    history.write(
        "-Users-alice-blog/gggg/subagents/agent-1.jsonl",
        fixture_time()?,
        &[r#"{"type":"assistant","uuid":"g-2","sessionId":"gggg","isSidechain":true,"timestamp":"2026-10-15T00:01:00Z","message":{"content":[{"type":"text","text":"from the sub-agent"}]}}"#],
    )?;
    if lock(&other)?.is_none() {
        return Ok(());
    }
    let picked = history.run_week(&[
        "records",
        "--type",
        "prompt,reply",
        "--subagents",
        "--session",
        "gggg",
    ]);
    let excluded = history.run_week(&["records", "--type", "prompt", "--session", "!gggg"]);
    fs::set_permissions(&other, fs::Permissions::from_mode(0o600))?;
    check_eq(&picked.code, &0, "exit")?;
    check_eq(picked.err.as_str(), "", "stderr")?;
    check_eq(
        &(
            picked.out.contains("mine"),
            picked.out.contains("from the sub-agent"),
        ),
        &(true, true),
        "the session and its sub-agent",
    )?;
    // Leaving a session out still reads every other file.
    check_eq(
        &excluded.err.contains("ffff.jsonl"),
        &true,
        "excluding opens the rest",
    )?;
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
    let out = history
        .run_week(&["records", "--type", "prompt", "--project", "blog"])
        .out;
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
        .run_week(&[
            "records",
            "--type",
            "call",
            "--subagents",
            "--session",
            "hhhh",
        ])
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
    check_eq(
        &subagent.contains(r#""session_title_then":"Rotate ***REDACTED*** key""#),
        &true,
        "sub-agent title then",
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
            "records",
            "--type",
            "call",
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
        .run_week(&[
            "records",
            "--type",
            "call",
            "--subagents",
            "--session",
            "jjjj",
        ])
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
    let out = history
        .run_week(&["records", "--type", "prompt", "--project", "notes"])
        .out;
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
    let out = history
        .run_week(&["records", "--type", "prompt", "--project", "alice-r"])
        .out;
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
    let ran = history.run(&["records", "--type", "prompt"]);
    fs::set_permissions(&bad, fs::Permissions::from_mode(0o600))?;
    check_eq(&ran.code, &0, "exit")?;
    check_eq(&ran.err.contains(['\u{1b}', '\u{7}']), &false, &ran.err)?;
    check_eq(&ran.err.contains("PWNED"), &true, &ran.err)?;
    Ok(())
}

#[test]
fn limit_prints_the_first_records() -> TestResult {
    let history = History::new("root")?;
    let out = history
        .run_week(&["records", "--type", "prompt", "--limit", "2"])
        .out;
    check_eq(&line_count(&out), &2, "prompts")?;
    check_eq(&out.contains(r#""uuid":"b-1""#), &false, "third prompt")?;
    Ok(())
}

#[test]
fn ccdig_variables_give_the_masks_and_the_command_line_replaces_them() -> TestResult {
    let (history, _) = history_with_personal_data()?;
    let both = r#"{"": ["corp-hidden", "bob@example.com"]}"#;
    let emails = r#"{"": ["bob@example.com"]}"#;
    let mask = |value: &str| Environment {
        mask: Some(value.into()),
        ..environment()
    };
    let mask_values = |value: &str| Environment {
        mask_values: Some(value.into()),
        ..environment()
    };
    // (environment, extra flags, what must be hidden, what must be shown)
    for (name, environment, extra, hidden, shown) in [
        (
            "mask",
            mask("email,ip"),
            &[][..],
            &["bob@", "10.1.2.3"][..],
            &["corp-hidden"][..],
        ),
        (
            "--mask replaces it",
            mask("email"),
            &["--mask", "ip"][..],
            &["10.1.2.3"][..],
            &["bob@"][..],
        ),
        (
            "--no-mask ignores it",
            mask("email"),
            &["--no-mask"][..],
            &[][..],
            &["bob@", "10.1.2.3"][..],
        ),
        (
            "mask values",
            mask_values(both),
            &[][..],
            &["corp-hidden", "bob@"][..],
            &["10.1.2.3"][..],
        ),
        (
            "--mask-values replaces them",
            mask_values(both),
            &["--mask-values", emails][..],
            &["bob@"][..],
            &["corp-hidden"][..],
        ),
    ] {
        let mut args = vec!["records", "--type", "prompt", "--session", "mmmm"];
        args.extend_from_slice(extra);
        let ran = history.run_in(&environment, &args);
        check_eq(&ran.code, &0, &format!("{name}: {}", ran.err))?;
        for value in hidden {
            check_eq(
                &ran.out.contains(value),
                &false,
                &format!("{name}: {value} hidden"),
            )?;
        }
        for value in shown {
            check_eq(
                &ran.out.contains(value),
                &true,
                &format!("{name}: {value} shown"),
            )?;
        }
    }
    let bad = history.run_in(&mask("email,phone"), &["records"]);
    check_eq(&bad.code, &1, "unknown mask")?;
    check_eq(
        bad.err.as_str(),
        "ccdig: CCDIG_MASK: unknown mask \"phone\": use aws-account, aws-profile, email, git-user, ip, or user\n",
        "unknown mask",
    )?;
    Ok(())
}

#[test]
fn claude_config_dir_gives_the_default_root_after_ccdig_root() -> TestResult {
    let history = History::new("root")?;
    let config = history
        .root
        .parent()
        .ok_or("no parent")?
        .as_os_str()
        .to_owned();
    let mut args = vec!["records", "--type", "prompt"];
    args.extend(WEEK);
    let from_config = history.run_bare(
        &Environment {
            claude_config_dir: Some(config.clone()),
            ..environment()
        },
        &args,
    );
    check_eq(&from_config.code, &0, &from_config.err)?;
    check_eq(&line_count(&from_config.out), &3, "CLAUDE_CONFIG_DIR")?;
    let ccdig_root_wins = history.run_bare(
        &Environment {
            claude_config_dir: Some(config),
            root: Some("/nonexistent/ccdig-root".into()),
            ..environment()
        },
        &args,
    );
    check_eq(&ccdig_root_wins.code, &1, "CCDIG_ROOT wins")?;
    check_eq(
        &ccdig_root_wins.err.starts_with("ccdig: list projects:"),
        &true,
        &ccdig_root_wins.err,
    )?;
    Ok(())
}

#[test]
fn on_skip_fail_ends_with_status_1_after_writing_the_output() -> TestResult {
    let clean = History::new("root")?;
    let broken = History::new("root")?;
    broken.write(
        "-Users-alice-blog/eeee.jsonl",
        fixture_time()?,
        &[r#"{"type":"user", not json"#],
    )?;
    let on_skip = |value: &str| Environment {
        on_skip: Some(value.into()),
        ..environment()
    };
    // (name, history, environment, extra flags, status, warned)
    for (name, history, environment, extra, code, warned) in [
        (
            "broken, fail",
            &broken,
            environment(),
            &["--on-skip", "fail"][..],
            1_u8,
            true,
        ),
        ("broken, default", &broken, environment(), &[][..], 0, true),
        (
            "broken, warn",
            &broken,
            environment(),
            &["--on-skip", "warn"][..],
            0,
            true,
        ),
        (
            "broken, CCDIG_ON_SKIP",
            &broken,
            on_skip("fail"),
            &[][..],
            1,
            true,
        ),
        (
            "broken, --on-skip replaces it",
            &broken,
            on_skip("fail"),
            &["--on-skip", "warn"][..],
            0,
            true,
        ),
        (
            "clean, fail",
            &clean,
            environment(),
            &["--on-skip", "fail"][..],
            0,
            false,
        ),
    ] {
        let mut args = vec!["records", "--type", "prompt"];
        args.extend_from_slice(extra);
        args.extend(WEEK);
        let ran = history.run_in(&environment, &args);
        check_eq(&ran.code, &code, &format!("{name}: {}", ran.err))?;
        check_eq(&line_count(&ran.out), &3, &format!("{name}: output"))?;
        check_eq(
            &ran.err.contains("ccdig: warning: skipped 1 undecodable"),
            &warned,
            &format!("{name}: {}", ran.err),
        )?;
    }
    let bad = clean.run_in(&on_skip("stop"), &["records"]);
    check_eq(&bad.code, &1, &format!("unknown on-skip: {}", bad.err))?;
    check_eq(
        bad.err.as_str(),
        "ccdig: CCDIG_ON_SKIP: unknown value \"stop\": use fail or warn\n",
        "unknown on-skip",
    )?;
    Ok(())
}

#[test]
fn tool_chars_cuts_long_strings_of_a_tool_input_in_markdown() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/wwww.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"w-1","sessionId":"wwww","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"tool_use","id":"toolu_w1","name":"Write","input":{"file_path":"/tmp/a.md","content":"0123456789abcdefghij","lines":["short","abcdefghijklmnop"],"count":12}}]}}"#,
        ],
    )?;
    let cut = history.run_week(&[
        "records",
        "--session",
        "wwww",
        "--type",
        "call",
        "--format",
        "md",
        "--tool-chars",
        "10",
    ]);
    check_eq(&cut.code, &0, &cut.err)?;
    snapshot("cli/records-cut.md", &cut.out)?;
    let whole = history.run_week(&["records", "--session", "wwww", "--type", "call"]);
    check_eq(
        &whole.out.contains(r#""content":"0123456789abcdefghij""#),
        &true,
        &whole.out,
    )?;
    for flag in ["--tool-chars", "--message-chars"] {
        for given in ["0", "-1", "3-2", "0-4", "1-", "x"] {
            let ran = history.run(&["records", flag, given]);
            check_eq(&ran.code, &1, flag)?;
            check_eq(
                ran.err.as_str(),
                format!(
                    "ccdig: {flag} takes a count of characters such as 2000, or a range such as 2001-4000 counted from 1: {given:?}\n"
                )
                .as_str(),
                given,
            )?;
        }
    }
    Ok(())
}

#[test]
fn tool_chars_with_a_range_reads_on_where_the_cut_left_off() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/wwww.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"w-1","sessionId":"wwww","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"tool_use","id":"toolu_w1","name":"Write","input":{"file_path":"/a.md","content":"0123456789abcdefghij"}}]}}"#,
            r#"{"type":"user","uuid":"m-1","sessionId":"wwww","timestamp":"2026-10-15T00:00:20Z","message":{"role":"user","content":"0123456789abcdefghij"}}"#,
        ],
    )?;
    let content = |args: &[&str], key: &str| -> Result<Vec<String>, Box<dyn Error>> {
        let mut full = vec!["records", "--session", "wwww", "--type", "call,prompt"];
        full.extend(args);
        Ok(json_lines(history.run_week(&full))?
            .iter()
            .filter_map(|record| {
                record
                    .pointer(key)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect())
    };
    check_eq(
        &content(&["--tool-chars", "8"], "/input/content")?,
        &vec!["01234567\u{2026} (12 more characters, next: 9-16)".to_owned()],
        "the first part",
    )?;
    check_eq(
        &content(&["--tool-chars", "9-16"], "/input/content")?,
        &vec![
            "(8 characters before) \u{2026}89abcdef\u{2026} (4 more characters, next: 17-20)"
                .to_owned(),
        ],
        "the next part",
    )?;
    check_eq(
        &content(&["--tool-chars", "9-16"], "/input/file_path")?,
        &vec!["/a.md".to_owned()],
        "a string the first part did not cut stays whole",
    )?;
    check_eq(
        &content(&["--tool-chars", "9-16"], "/text")?,
        &vec!["0123456789abcdefghij".to_owned()],
        "the prompt is not a tool's",
    )?;
    check_eq(
        &content(&["--message-chars", "17-40"], "/text")?,
        &vec!["(16 characters before) \u{2026}ghij".to_owned()],
        "the last part",
    )?;
    Ok(())
}

#[test]
fn markdown_indents_a_json_tool_result_and_decodes_its_escapes() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/jjjj.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"j-1","sessionId":"jjjj","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"tool_use","id":"toolu_j1","name":"mcp__notes__find","input":{}},{"type":"tool_use","id":"toolu_j2","name":"Bash","input":{"command":"echo 42"}}]}}"#,
            r#"{"type":"user","uuid":"j-2","sessionId":"jjjj","timestamp":"2026-10-15T00:00:20Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_j1","content":"{\"title\":\"\\u65e5\\u672c\",\"tags\":[\"a\"]}"}]}}"#,
            r#"{"type":"user","uuid":"j-3","sessionId":"jjjj","timestamp":"2026-10-15T00:00:30Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_j2","content":"42"}]}}"#,
        ],
    )?;
    let args = ["records", "--session", "jjjj", "--type", "result"];
    let mut md = args.to_vec();
    md.extend(["--format", "md"]);
    let ran = history.run_week(&md);
    check_eq(&ran.code, &0, &ran.err)?;
    check_eq(
        &ran.out.contains(
            "```text\n{\n  \"title\": \"\u{65e5}\u{672c}\",\n  \"tags\": [\n    \"a\"\n  ]\n}\n```",
        ),
        &true,
        &ran.out,
    )?;
    check_eq(&ran.out.contains("```text\n42\n```"), &true, &ran.out)?;
    let raw = json_lines(history.run_week(&args))?;
    check_eq(
        &strings(&raw, "text"),
        &vec![r#"{"title":"\u65e5\u672c","tags":["a"]}"#, "42"],
        "JSON Lines keep the text",
    )?;
    Ok(())
}

#[test]
fn markdown_leaves_a_json_tool_result_whose_numbers_would_change_as_written() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/nnnn.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"n-1","sessionId":"nnnn","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"tool_use","id":"toolu_n1","name":"mcp__db__count","input":{}},{"type":"tool_use","id":"toolu_n2","name":"mcp__db__ratio","input":{}}]}}"#,
            r#"{"type":"user","uuid":"n-2","sessionId":"nnnn","timestamp":"2026-10-15T00:00:20Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_n1","content":"{\"rows\":12345678901234567890123}"}]}}"#,
            r#"{"type":"user","uuid":"n-3","sessionId":"nnnn","timestamp":"2026-10-15T00:00:30Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_n2","content":"{\"ratio\":1e2,\"half\":0.5,\"n\":-3}"}]}}"#,
        ],
    )?;
    let ran = history.run_week(&[
        "records",
        "--session",
        "nnnn",
        "--type",
        "result",
        "--format",
        "md",
    ]);
    check_eq(&ran.code, &0, &ran.err)?;
    for written in [
        "```text\n{\"rows\":12345678901234567890123}\n```",
        "```text\n{\"ratio\":1e2,\"half\":0.5,\"n\":-3}\n```",
    ] {
        check_eq(&ran.out.contains(written), &true, &ran.out)?;
    }
    Ok(())
}

#[test]
fn markdown_masks_a_secret_the_json_escapes_hid_once_it_is_decoded() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/xxxx.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"assistant","uuid":"x-1","sessionId":"xxxx","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"tool_use","id":"toolu_x1","name":"mcp__keys__get","input":{}}]}}"#,
            r#"{"type":"user","uuid":"x-2","sessionId":"xxxx","timestamp":"2026-10-15T00:00:20Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_x1","content":"{\"key\":\"AKIA\\u0049OSFODNN7EXAMPLE\"}"}]}}"#,
        ],
    )?;
    for kind in ["call", "result"] {
        let ran = history.run_week(&[
            "records",
            "--session",
            "xxxx",
            "--type",
            kind,
            "--format",
            "md",
            "--mask-report",
            "json",
        ]);
        check_eq(&ran.code, &0, &ran.err)?;
        check_eq(&ran.out.contains("\"key\": "), &true, &ran.out)?;
        check_eq(&ran.out.contains("AKIAIOSFODNN7EXAMPLE"), &false, &ran.out)?;
        check_eq(&ran.err.contains("\"masked\":{}"), &false, &ran.err)?;
    }
    Ok(())
}

#[test]
fn chars_cut_the_tool_side_and_the_conversation_apart_in_json_lines() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/vvvv.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"v-1","sessionId":"vvvv","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"please write the long notes file"}}"#,
            r#"{"type":"assistant","uuid":"v-2","sessionId":"vvvv","timestamp":"2026-10-15T00:00:10Z","message":{"content":[{"type":"text","text":"Writing the notes now."},{"type":"tool_use","id":"toolu_v1","name":"Write","input":{"file_path":"/tmp/notes.md","content":"0123456789abcdefghij"}}]}}"#,
            r#"{"type":"user","uuid":"v-3","sessionId":"vvvv","timestamp":"2026-10-15T00:00:20Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_v1","content":"File created successfully"}]}}"#,
        ],
    )?;
    let texts = |extra: &[&str]| -> Result<Vec<Value>, Box<dyn Error>> {
        let mut args = vec![
            "records",
            "--session",
            "vvvv",
            "--type",
            "prompt,reply,call,result",
        ];
        args.extend_from_slice(extra);
        let ran = history.run_week(&args);
        if ran.code != 0 {
            return Err(ran.err.into());
        }
        ran.out
            .lines()
            .map(|line| {
                let record: Value = serde_json::from_str(line)?;
                Ok(json!([
                    record.get("type"),
                    record.get("text"),
                    record.pointer("/input/content"),
                ]))
            })
            .collect()
    };
    check_eq(
        &texts(&["--tool-chars", "4"])?,
        &vec![
            json!(["prompt", "please write the long notes file", null]),
            json!(["reply", "Writing the notes now.", null]),
            json!(["call", null, "0123\u{2026} (16 more characters, next: 5-8)"]),
            json!([
                "result",
                "File\u{2026} (21 more characters, next: 5-8)",
                "0123\u{2026} (16 more characters, next: 5-8)"
            ]),
        ],
        "--tool-chars",
    )?;
    check_eq(
        &texts(&["--message-chars", "6"])?,
        &vec![
            json!([
                "prompt",
                "please\u{2026} (26 more characters, next: 7-12)",
                null
            ]),
            json!([
                "reply",
                "Writin\u{2026} (16 more characters, next: 7-12)",
                null
            ]),
            json!(["call", null, "0123456789abcdefghij"]),
            json!([
                "result",
                "File created successfully",
                "0123456789abcdefghij"
            ]),
        ],
        "--message-chars",
    )?;
    Ok(())
}

#[test]
fn the_old_names_of_the_char_flags_point_at_the_new_ones() -> TestResult {
    let history = History::new("root")?;
    for (old, hint) in [
        (
            "--max-input-chars",
            "it is --tool-chars now, which cuts the tool inputs and outputs in every format",
        ),
        (
            "--max-tool-chars",
            "it is --tool-chars now, which also takes a range such as 2001-4000",
        ),
        (
            "--max-message-chars",
            "it is --message-chars now, which also takes a range such as 2001-4000",
        ),
    ] {
        let ran = history.run(&["records", old, "10"]);
        check_eq(&ran.code, &1, &ran.err)?;
        check_eq(
            ran.err.as_str(),
            format!("ccdig: unexpected argument '{old}' found; {hint}\n").as_str(),
            old,
        )?;
    }
    Ok(())
}

#[test]
fn type_summary_lists_the_compaction_summaries_only_when_asked_for() -> TestResult {
    let history = history_with_a_conversation()?;
    let before = history.run_week(&["records", "--session", "cccc", "--type", "prompt"]);
    history.write(
        "-Users-alice-blog/cccc.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"c-1","sessionId":"cccc","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"list the posts"}}"#,
            r#"{"type":"user","uuid":"c-5","sessionId":"cccc","timestamp":"2026-10-15T00:00:30Z","isCompactSummary":true,"message":{"role":"user","content":"Summary: listed the posts for bob@example.com."}}"#,
            r#"{"type":"user","uuid":"c-4","sessionId":"cccc","timestamp":"2026-10-15T00:01:00Z","message":{"role":"user","content":"thanks"}}"#,
            r#"{"type":"ai-title","sessionId":"cccc","aiTitle":"List posts"}"#,
        ],
    )?;
    for (args, want) in [
        (&[][..], vec!["prompt", "prompt"]),
        (&["--type", "summary"][..], vec!["summary"]),
        (
            &["--type", "prompt,summary"][..],
            vec!["prompt", "summary", "prompt"],
        ),
    ] {
        let mut full = vec!["records", "--session", "cccc", "--mask", "email"];
        full.extend_from_slice(args);
        let ran = history.run_week(&full);
        check_eq(&ran.code, &0, &ran.err)?;
        check_eq(&types(&ran.out), &want, &format!("{args:?}"))?;
    }
    let prompts = history.run_week(&["records", "--session", "cccc", "--type", "prompt"]);
    // The rewritten file has fewer lines, so the line numbers differ.
    let no_line = regex::Regex::new(r#""line":[0-9]+"#)?;
    check_eq(
        &no_line.replace_all(&prompts.out, ""),
        &no_line.replace_all(&before.out, ""),
        "prompts unchanged",
    )?;
    let summary = history.run_week(&[
        "records",
        "--session",
        "cccc",
        "--type",
        "summary",
        "--mask",
        "email",
    ]);
    check_eq(
        &summary
            .out
            .contains(r#""text":"Summary: listed the posts for ***REDACTED***.","redacted":true"#),
        &true,
        &summary.out,
    )?;
    let markdown = history.run_week(&[
        "records",
        "--session",
        "cccc",
        "--type",
        "summary",
        "--format",
        "md",
    ]);
    snapshot("cli/summaries.md", &markdown.out)?;
    Ok(())
}

/// What a git with `Hanako Sato <hanako@example.com>` configured answers.
fn configured_git(key: &str) -> Option<String> {
    match key {
        "user.name" => Some("Hanako Sato".to_owned()),
        "user.email" => Some("hanako@example.com".to_owned()),
        _ => None,
    }
}

/// What a git with only `user.email` configured answers.
fn git_without_a_name(key: &str) -> Option<String> {
    (key == "user.email").then(|| "hanako@example.com".to_owned())
}

#[test]
fn mask_git_user_hides_the_configured_name_and_email() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/gggg.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"g-1","sessionId":"gggg","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"commit as Hanako Sato <hanako@example.com> to arn:aws:iam::123456789012:role/x with --profile org_read"}}"#,
        ],
    )?;
    let git = |lookup: fn(&str) -> Option<String>| Environment {
        git_config: Some(lookup),
        ..environment()
    };
    let mut args = vec![
        "records",
        "--session",
        "gggg",
        "--mask",
        "git-user,aws-account,aws-profile",
    ];
    args.extend(WEEK);
    let ran = history.run_in(&git(configured_git), &args);
    check_eq(&ran.code, &0, &ran.err)?;
    check_eq(
        &ran.out.contains(
            r#""text":"commit as ***REDACTED*** <***REDACTED***> to arn:aws:iam::***REDACTED***:role/x with --profile ***REDACTED***""#,
        ),
        &true,
        &ran.out,
    )?;
    let missing = history.run_in(&git(git_without_a_name), &args);
    check_eq(&missing.code, &1, "no user.name")?;
    check_eq(
        missing.err.as_str(),
        "ccdig: mask git-user: git config has no user.name\n",
        "no user.name",
    )?;
    Ok(())
}

#[test]
fn labeled_mask_values_get_a_number_per_label_kept_across_fields() -> TestResult {
    let history = History::new("root")?;
    for (project, session, content) in [
        (
            "acme-billing",
            "pppp",
            "ask \u{5C71}\u{7530} \u{592A}\u{90CE} and Yamada about acme-infra, token=\\\"acme-billing\\\", secret-word",
        ),
        ("acme-portal", "qqqq", "deploy acme-portal"),
    ] {
        history.write(
            &format!("-Users-alice-{project}/{session}.jsonl"),
            fixture_time()?,
            &[&format!(
                r#"{{"type":"user","uuid":"{session}-1","sessionId":"{session}","cwd":"/Users/alice/{project}","timestamp":"2026-10-15T00:00:00Z","message":{{"role":"user","content":"{content}"}}}}"#
            )],
        )?;
    }
    let values = history.root.with_file_name("values.json");
    fs::write(
        &values,
        "{\"repo\": [\"acme-billing\", \"acme-portal\"], \"person\": [[\"\u{5C71}\u{7530} \u{592A}\u{90CE}\", \"Yamada\"]]}",
    )?;
    let values_path = format!("@{}", values.to_string_lossy());
    let ran = history.run_week(&[
        "records",
        "--type",
        "prompt",
        "--project",
        "acme",
        "--mask-values",
        &values_path,
        "--mask-values",
        r#"{"repo": ["acme-infra", "acme-billing"], "": ["secret-word"]}"#,
    ]);
    check_eq(&ran.code, &0, &ran.err)?;
    for want in [
        "\"project\":\"-Users-alice-\u{FF3B}repo1\u{FF3D}\",\"cwd\":\"/Users/alice/\u{FF3B}repo1\u{FF3D}\"",
        "\"text\":\"ask \u{FF3B}person1\u{FF3D} and \u{FF3B}person1\u{FF3D} about \u{FF3B}repo3\u{FF3D}, token=\\\"***REDACTED***\\\", ***REDACTED***\"",
        "\"project\":\"-Users-alice-\u{FF3B}repo2\u{FF3D}\",\"cwd\":\"/Users/alice/\u{FF3B}repo2\u{FF3D}\"",
        "\"text\":\"deploy \u{FF3B}repo2\u{FF3D}\"",
    ] {
        check_eq(
            &ran.out.contains(want),
            &true,
            &format!("{want} in {}", ran.out),
        )?;
    }
    for (given, want) in [
        (
            "[]",
            "ccdig: --mask-values: expected an object of labels to lists of values\n",
        ),
        (
            r#"{"repo": "a"}"#,
            "ccdig: --mask-values: expected an object of labels to lists of values\n",
        ),
        (
            r#"{"repo": [1]}"#,
            "ccdig: --mask-values: expected an object of labels to lists of values\n",
        ),
        (
            r#"{"repo": [["a", 1]]}"#,
            "ccdig: --mask-values: expected an object of labels to lists of values\n",
        ),
        (
            "{oops",
            "ccdig: --mask-values: not JSON: key must be a string at line 1 column 2\n",
        ),
    ] {
        let bad = history.run_week(&["records", "--type", "prompt", "--mask-values", given]);
        check_eq(&bad.code, &1, given)?;
        check_eq(bad.err.as_str(), want, given)?;
    }
    let bad_file = history.root.with_file_name("bad.json");
    fs::write(&bad_file, "[]")?;
    let bad_path = format!("@{}", bad_file.to_string_lossy());
    let bad = history.run_week(&["records", "--type", "prompt", "--mask-values", &bad_path]);
    check_eq(
        bad.err.as_str(),
        &format!(
            "ccdig: --mask-values {bad_path}: expected an object of labels to lists of values\n"
        ),
        "a file names itself",
    )?;
    Ok(())
}

#[test]
fn mask_report_counts_what_was_masked_in_the_printed_records_by_rule() -> TestResult {
    let history = History::new("root")?;
    // Assembled at run time so that no secret-shaped literal is committed.
    let secret = "a1B2".repeat(5);
    let first = format!(
        r#"{{"type":"user","uuid":"r-1","sessionId":"rrrr","timestamp":"2026-10-15T00:00:00Z","message":{{"role":"user","content":"mail bob@example.com about acme-billing with password={secret}"}}}}"#
    );
    history.write(
        "-Users-alice-blog/rrrr.jsonl",
        fixture_time()?,
        &[
            &first,
            r#"{"type":"user","uuid":"r-2","sessionId":"rrrr","timestamp":"2026-10-15T00:01:00Z","message":{"role":"user","content":"mail carol@example.com"}}"#,
        ],
    )?;
    let base = [
        "records",
        "--type",
        "prompt",
        "--session",
        "rrrr",
        "--mask",
        "email",
        "--mask-values",
        r#"{"repo": ["acme-billing"]}"#,
    ];
    let plain = history.run_week(&base);
    for (extra, want) in [
        (
            &["--mask-report", "text"][..],
            "ccdig: masked 2 by email, 1 by credential-assignment, 1 by values/repo\n",
        ),
        (
            &["--mask-report", "json"][..],
            "{\"masked\":{\"email\":2,\"credential-assignment\":1,\"values/repo\":1}}\n",
        ),
        (
            &["--mask-report", "text", "--grep", "!password"][..],
            "ccdig: masked 1 by email\n",
        ),
        (
            &["--mask-report", "text", "--grep", "nobody"][..],
            "ccdig: masked nothing\n",
        ),
    ] {
        let mut args = base.to_vec();
        args.extend_from_slice(extra);
        let ran = history.run_week(&args);
        check_eq(&ran.code, &0, &ran.err)?;
        check_eq(ran.err.as_str(), want, &format!("{extra:?}"))?;
        check_eq(&ran.err.contains(&secret), &false, "no value")?;
        if extra.len() == 2 {
            check_eq(&ran.out, &plain.out, "stdout unchanged")?;
        }
    }
    let sessions = history.run_week(&[
        "sessions",
        "--session",
        "rrrr",
        "--mask-values",
        r#"{"place": ["blog"]}"#,
        "--mask-report",
        "text",
    ]);
    check_eq(
        sessions.err.as_str(),
        "ccdig: masked 2 by values/place, 1 by credential-assignment\n",
        "sessions count their own fields: here the project and the file, and the first prompt",
    )?;
    Ok(())
}

#[test]
fn a_prompt_carries_the_permission_mode_recorded_on_its_line() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/kkkk.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"permission-mode","permissionMode":"auto","sessionId":"kkkk"}"#,
            r#"{"type":"user","uuid":"k-1","sessionId":"kkkk","timestamp":"2026-10-15T00:00:00Z","permissionMode":"plan","message":{"role":"user","content":"plan the change"}}"#,
            r#"{"type":"user","uuid":"k-2","sessionId":"kkkk","timestamp":"2026-10-15T00:01:00Z","message":{"role":"user","content":"<command-name>/goal</command-name>\n<command-message>goal</command-message>\n<command-args>tests pass</command-args>"}}"#,
        ],
    )?;
    let ran = history.run_week(&["records", "--type", "prompt", "--session", "kkkk"]);
    check_eq(&ran.code, &0, &ran.err)?;
    let modes: Vec<String> = ran
        .out
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).map(|record| {
                record
                    .get("permission_mode")
                    .and_then(Value::as_str)
                    .unwrap_or("(missing)")
                    .to_owned()
            })
        })
        .collect::<Result<_, _>>()?;
    check_eq(&modes, &vec!["plan".to_owned(), String::new()], &ran.out)?;
    check_eq(
        &ran.out.contains(
            r#""command":"","options":[],"permission_mode":"plan","superseded":false,"redacted""#,
        ),
        &true,
        &ran.out,
    )?;
    Ok(())
}

#[test]
fn the_json_envelope_records_how_the_output_was_made() -> TestResult {
    let history = History::new("root")?;
    let mut args = vec![
        "records",
        "--type",
        "prompt",
        "--format",
        "json",
        "--mask",
        "email",
        "--mask",
        "ip,email",
        "--mask-values",
        r#"{"repo": ["secret-repo"], "": ["secret-word"], "person": ["Hanako"]}"#,
    ];
    args.extend(WEEK);
    let ran = history.run(&args);
    check_eq(&ran.code, &0, &ran.err)?;
    let document: serde_json::Value = serde_json::from_str(&ran.out)?;
    let keys: Vec<&str> = document
        .as_object()
        .ok_or("not an object")?
        .keys()
        .map(String::as_str)
        .collect();
    check_eq(
        &keys,
        &vec!["schema", "filter", "ccdig", "run", "items"],
        "keys in order",
    )?;
    check_eq(
        &document.get("schema"),
        &Some(&serde_json::json!(12_i32)),
        "schema",
    )?;
    check_eq(
        &document.get("ccdig"),
        &Some(&serde_json::json!(VERSION)),
        "version",
    )?;
    check_eq(
        &document.get("run"),
        &Some(&serde_json::json!({
            "since": "2026-10-13",
            "until": "2026-10-19",
            "tz": "Asia/Tokyo",
            "mask": ["email", "ip"],
            "mask_values": ["repo", "person"],
            "no_mask": false,
        })),
        "run",
    )?;
    check_eq(&ran.out.contains("secret-"), &false, "no values")?;
    check_eq(
        &document
            .get("items")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        &Some(3),
        "items",
    )?;
    let sessions = history.run(&["sessions", "--format", "json", "--no-mask"]);
    let sessions_document: serde_json::Value = serde_json::from_str(&sessions.out)?;
    check_eq(
        &sessions_document.get("run"),
        &Some(&serde_json::json!({
            "since": null,
            "until": null,
            "tz": null,
            "mask": [],
            "mask_values": [],
            "no_mask": true,
        })),
        "sessions run",
    )?;
    Ok(())
}

#[test]
fn usage_is_on_the_first_record_of_each_response_and_summed_per_session() -> TestResult {
    let history = History::new("root")?;
    history.write(
        "-Users-alice-blog/uuuu.jsonl",
        fixture_time()?,
        &[
            r#"{"type":"user","uuid":"t-1","sessionId":"uuuu","timestamp":"2026-10-15T00:00:00Z","message":{"role":"user","content":"count the posts"}}"#,
            r#"{"type":"assistant","uuid":"t-2","sessionId":"uuuu","timestamp":"2026-10-15T00:00:10Z","message":{"id":"msg_1","content":[{"type":"text","text":"Counting."}],"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":20,"service_tier":"standard"}}}"#,
            r#"{"type":"assistant","uuid":"t-3","sessionId":"uuuu","timestamp":"2026-10-15T00:00:11Z","message":{"id":"msg_1","content":[{"type":"tool_use","id":"toolu_t1","name":"Bash","input":{"command":"ls | wc -l"}}],"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":20}}}"#,
            r#"{"type":"assistant","uuid":"t-4","sessionId":"uuuu","timestamp":"2026-10-15T00:00:20Z","message":{"id":"msg_2","content":[{"type":"text","text":"Three."}],"usage":{"input_tokens":1,"output_tokens":2,"cache_read_input_tokens":3,"cache_creation_input_tokens":4}}}"#,
            r#"{"type":"assistant","uuid":"t-5","sessionId":"uuuu","timestamp":"2026-10-15T00:00:30Z","message":{"content":[{"type":"text","text":"From an older Claude Code."}]}}"#,
        ],
    )?;
    let ran = history.run_week(&["records", "--session", "uuuu", "--type", "reply,call"]);
    check_eq(&ran.code, &0, &ran.err)?;
    let usages: Vec<serde_json::Value> = ran
        .out
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .map(|record| record.get("usage").cloned().unwrap_or_default())
        })
        .collect::<Result<_, _>>()?;
    check_eq(
        &usages,
        &vec![
            serde_json::json!({"input": 10_u64, "output": 5_u64, "cache_read": 100_u64, "cache_creation": 20_u64}),
            serde_json::Value::Null,
            serde_json::json!({"input": 1_u64, "output": 2_u64, "cache_read": 3_u64, "cache_creation": 4_u64}),
            serde_json::Value::Null,
        ],
        &ran.out,
    )?;
    let filtered = history.run_week(&[
        "records",
        "--session",
        "uuuu",
        "--type",
        "reply,call",
        "--tool",
        "Bash",
    ]);
    check_eq(
        &filtered
            .out
            .contains(r#""usage":{"input":10,"output":5,"cache_read":100,"cache_creation":20}"#),
        &true,
        &format!(
            "the call left after the filter carries them: {}",
            filtered.out
        ),
    )?;
    let sessions = history.run_week(&["sessions", "--session", "uuuu"]);
    check_eq(
        &sessions.out.contains(
            r#""prompt_count":1,"first_prompt":"count the posts","match_count":null,"first_match":null,"usage":{"input":11,"output":7,"cache_read":103,"cache_creation":24},"models":[]"#,
        ),
        &true,
        &sessions.out,
    )?;
    Ok(())
}

#[test]
fn the_default_root_is_under_home_in_the_environment() -> TestResult {
    let history = History::new("root")?;
    let home = tempfile::tempdir()?;
    fs::create_dir_all(home.path().join(".claude"))?;
    symlink(&history.root, home.path().join(".claude").join("projects"))?;
    let with_home = Environment {
        home: Some(home.path().into()),
        ..Environment::default()
    };
    let ran = history.run_bare(&with_home, &["records", "--session", "aaaa"]);
    check_eq(&ran.code, &0, &ran.err)?;
    check_eq(
        &ran.out.contains(r#""text":"/weekly-report last week""#),
        &true,
        &ran.out,
    )?;
    let without_home = history.run_bare(&Environment::default(), &["records"]);
    check_eq(&without_home.code, &1, "no HOME")?;
    check_eq(
        without_home.err.as_str(),
        "ccdig: find the default --root: HOME is not set\n",
        "no HOME",
    )?;
    Ok(())
}

/// The session IDs of the sessions `out` lists as JSON lines, in order.
fn session_ids(out: &str) -> Result<Vec<String>, Box<dyn Error>> {
    out.lines()
        .map(|line| {
            let session: serde_json::Value = serde_json::from_str(line)?;
            Ok(session
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned())
        })
        .collect()
}

#[test]
fn session_current_is_the_session_ccdig_runs_in() -> TestResult {
    let history = History::new("root")?;
    let inside = Environment {
        session_id: Some("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into()),
        ..environment()
    };
    for (session, listed) in [
        ("current", "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
        ("!current", "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"),
    ] {
        let mut args = vec!["sessions", "--session", session];
        args.extend(WEEK);
        let ran = history.run_in(&inside, &args);
        check_eq(&ran.code, &0, &ran.err)?;
        check_eq(&session_ids(&ran.out)?, &vec![listed.to_owned()], session)?;
    }
    Ok(())
}

#[test]
fn session_current_takes_the_variable_as_an_id_even_with_a_leading_bang() -> TestResult {
    let history = History::new("root")?;
    let odd = Environment {
        session_id: Some("!aaaaaaaa".into()),
        ..environment()
    };
    for (session, listed) in [
        ("current", vec![]),
        (
            "!current",
            vec![
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".to_owned(),
                "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".to_owned(),
            ],
        ),
    ] {
        let mut args = vec!["sessions", "--session", session];
        args.extend(WEEK);
        let ran = history.run_in(&odd, &args);
        check_eq(&ran.code, &0, &ran.err)?;
        check_eq(&session_ids(&ran.out)?, &listed, session)?;
    }
    Ok(())
}

#[test]
fn session_current_outside_claude_code_is_an_error() -> TestResult {
    let history = History::new("root")?;
    for session in ["current", "!current"] {
        let ran = history.run(&["sessions", "--session", session]);
        check_eq(&ran.code, &1, session)?;
        check_eq(ran.out.as_str(), "", session)?;
        check_eq(
            ran.err.as_str(),
            "ccdig: --session current: CLAUDE_CODE_SESSION_ID is not set; run ccdig inside Claude Code or give a session ID\n",
            session,
        )?;
    }
    Ok(())
}

#[test]
fn the_local_zone_comes_from_tz_in_the_environment() -> TestResult {
    let history = History::new("root")?;
    let tz = |value: &str| Environment {
        tz: Some(value.into()),
        ..environment()
    };
    let args = [
        "records",
        "--session",
        "aaaa",
        "--format",
        "md",
        "--since",
        "2026-10-13",
        "--until",
        "2026-10-19",
    ];
    for (value, want) in [
        (
            "Asia/Tokyo",
            "### 2026-10-13 10:00:01 JST - turn 1 - Prompt",
        ),
        ("", "### 2026-10-13 01:00:01 UTC - turn 1 - Prompt"),
    ] {
        let ran = history.run_in(&tz(value), &args);
        check_eq(&ran.code, &0, &ran.err)?;
        check_eq(&ran.out.contains(want), &true, &ran.out)?;
    }
    Ok(())
}

#[test]
fn json_times_are_written_in_the_zone_of_the_day_range() -> TestResult {
    let history = History::new("root")?;
    let tz = |value: &str| Environment {
        tz: Some(value.into()),
        ..environment()
    };
    for (given, local, want) in [
        (Some("Asia/Tokyo"), "UTC", "2026-10-13T10:00:01+09:00"),
        (Some("UTC"), "Asia/Tokyo", "2026-10-13T01:00:01Z"),
        (None, "Asia/Tokyo", "2026-10-13T10:00:01+09:00"),
    ] {
        let mut args = vec![
            "--session",
            "aaaa",
            "--since",
            "2026-10-13",
            "--until",
            "2026-10-19",
        ];
        if let Some(zone) = given {
            args.extend(["--tz", zone]);
        }
        for command in ["records", "sessions"] {
            let mut full = vec![command];
            full.extend_from_slice(&args);
            let ran = history.run_in(&tz(local), &full);
            check_eq(&ran.code, &0, &ran.err)?;
            let first: Value = serde_json::from_str(ran.out.lines().next().unwrap_or_default())?;
            let key = if command == "records" {
                "timestamp"
            } else {
                "started_at"
            };
            check_eq(
                &first.get(key).and_then(Value::as_str),
                &Some(want),
                &format!("{command} {given:?} with TZ={local}"),
            )?;
        }
    }
    Ok(())
}
