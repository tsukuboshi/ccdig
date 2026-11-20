//! The command line: the flags, the subcommands, and how help, output, and
//! errors are written.

use std::ffi::{OsStr, OsString};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::{env, fs};

use clap::{Args, CommandFactory as _, Parser, Subcommand, ValueEnum};
use jiff::tz::TimeZone;
use serde::Serialize;

use crate::classify::human::FILTER_VERSION;
use crate::collect::kinds::{Prompts, Records, Types};
use crate::collect::{Corpus, Mask, Pick, collect};
use crate::failure::Failure;
use crate::redact::ExtraMasks;
use crate::render::digest::{Limits, digest};
use crate::render::json::{document, lines};
use crate::render::markdown;
use crate::select::{Query, compile_grep};
use crate::text::one_line;
use crate::when::{DayRange, load_zone};

/// Dig prompts, replies, tool calls, and tool results out of Claude Code
/// session transcripts.
#[derive(Parser)]
#[command(
    name = "ccdig",
    long_about = "ccdig reads Claude Code session transcripts and lists the prompts a person\n\
                  typed, the assistant's replies, tool calls, and tool results, one kind or\n\
                  several in the order they were written. Harness noise is dropped, slash\n\
                  commands are folded back, and secrets are masked.",
    disable_help_subcommand = true,
    disable_version_flag = true
)]
struct Cli {
    /// The subcommand; none prints the help.
    #[command(subcommand)]
    command: Option<Command>,
    /// The shared flags given before the subcommand.
    #[command(flatten)]
    common: Common,
}

/// The flags of `records` that apply to assistant lines and what answers
/// them.
#[derive(Args)]
struct Activity {
    /// Keep replies, tool calls, and tool results written while this skill
    /// ran (repeatable).
    #[arg(
        long = "skill",
        value_name = "NAME",
        help_heading = "Filters",
        display_order = 30
    )]
    skills: Vec<String>,
    /// Also read sub-agent transcripts.
    #[arg(long, help_heading = "Input", display_order = 41)]
    subagents: bool,
}

/// The subcommands. Help lists them by `display_order`, most used first.
#[derive(Subcommand)]
enum Command {
    /// Write a size-capped Markdown digest of the prompts for an LLM.
    #[command(display_order = 3)]
    Digest {
        /// The shared flags.
        #[command(flatten)]
        common: Common,
        /// Truncate each prompt to this many characters.
        #[arg(
            long,
            default_value_t = 300,
            value_name = "N",
            help_heading = "Output",
            display_order = 62
        )]
        max_prompt_chars: i64,
        /// List at most this many characters per session.
        #[arg(
            long,
            default_value_t = 2000,
            value_name = "N",
            help_heading = "Output",
            display_order = 63
        )]
        max_session_chars: i64,
    },
    /// List prompts, replies, tool calls, and tool results in the order
    /// they were written.
    #[command(display_order = 1)]
    Records {
        /// The kinds of record to list (comma-separated, repeatable)
        /// [default: prompt,reply,call].
        #[arg(
            long = "type",
            value_name = "TYPE",
            value_delimiter = ',',
            help_heading = "Records",
            display_order = 10
        )]
        types: Vec<RecordType>,
        /// The flags for assistant lines.
        #[command(flatten)]
        activity: Activity,
        /// The shared flags.
        #[command(flatten)]
        common: Common,
        /// The output format.
        #[command(flatten)]
        output: Output,
        /// Keep tool results with this status (comma-separated,
        /// repeatable).
        #[arg(
            long = "status",
            value_name = "STATUS",
            value_delimiter = ',',
            help_heading = "Filters",
            display_order = 29
        )]
        statuses: Vec<Status>,
        /// Keep tool calls and results of this tool (repeatable).
        #[arg(
            long = "tool",
            value_name = "NAME",
            help_heading = "Filters",
            display_order = 28
        )]
        tools: Vec<String>,
    },
    /// List sessions that have at least one selected prompt.
    #[command(display_order = 2)]
    Sessions {
        /// The shared flags.
        #[command(flatten)]
        common: Common,
        /// The output format.
        #[command(flatten)]
        output: Output,
    },
    /// Print the ccdig version.
    // The shared flags are accepted and ignored.
    #[command(display_order = 4)]
    Version(Common),
}

/// The flags every subcommand takes, before or after its name. clap's
/// global flags keep only the last occurrence, so the flags are declared on
/// both levels and merged: lists add up, a later value wins, and a switch
/// given on either level is on.
#[derive(Args)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "clap lists the help headings in the order their flags are declared"
)]
struct Common {
    /// First day to include, YYYY-MM-DD (record time, in --tz).
    #[arg(long, value_name = "DAY", help_heading = "Filters", display_order = 20)]
    since: Option<String>,
    /// Last day to include, YYYY-MM-DD (record time, in --tz).
    #[arg(long, value_name = "DAY", help_heading = "Filters", display_order = 21)]
    until: Option<String>,
    /// IANA time zone for --since, --until, and Markdown times [default:
    /// Local, the system zone; set TZ to override].
    #[arg(
        long,
        value_name = "ZONE",
        help_heading = "Filters",
        display_order = 22
    )]
    tz: Option<String>,
    /// Keep projects whose directory name or cwd contains this
    /// (repeatable).
    #[arg(
        long = "project",
        value_name = "SUBSTRING",
        help_heading = "Filters",
        display_order = 23
    )]
    projects: Vec<String>,
    /// Keep sessions whose ID starts with this (repeatable).
    #[arg(
        long = "session",
        value_name = "PREFIX",
        help_heading = "Filters",
        display_order = 24
    )]
    sessions: Vec<String>,
    /// Keep records on this git branch (repeatable).
    #[arg(
        long = "branch",
        value_name = "NAME",
        help_heading = "Filters",
        display_order = 25
    )]
    branches: Vec<String>,
    /// Keep records whose text matches this regular expression, after
    /// masking.
    #[arg(
        long,
        value_name = "REGEX",
        help_heading = "Filters",
        display_order = 26
    )]
    grep: Option<String>,
    /// Like --grep, ignoring case.
    #[arg(
        long,
        value_name = "REGEX",
        conflicts_with = "grep",
        help_heading = "Filters",
        display_order = 27
    )]
    igrep: Option<String>,
    /// Transcript root [default: ~/.claude/projects] [env: `CCDIG_ROOT`].
    #[arg(long, value_name = "DIR", help_heading = "Input", display_order = 40)]
    root: Option<PathBuf>,
    /// Also mask these (comma-separated, repeatable) [env: `CCDIG_MASK`].
    #[arg(
        long = "mask",
        value_name = "KIND",
        value_delimiter = ',',
        help_heading = "Masking",
        display_order = 50
    )]
    masks: Vec<MaskKind>,
    /// Also mask each non-empty line of this file wherever it appears
    /// (repeatable) [env: `CCDIG_MASK_FILE`, separated as in
    /// `PATH`].
    #[arg(
        long = "mask-file",
        value_name = "FILE",
        help_heading = "Masking",
        display_order = 51
    )]
    mask_files: Vec<PathBuf>,
    /// Do not mask anything, secrets included.
    #[arg(long, help_heading = "Masking", display_order = 52)]
    no_mask: bool,
    /// Print at most this many records.
    #[arg(long, value_name = "N", help_heading = "Output", display_order = 61)]
    limit: Option<usize>,
}

/// The `CCDIG_*` environment variables: defaults for the flags they name,
/// used when neither level of the command line gives the flag. An empty
/// variable counts as unset.
#[derive(Default)]
struct Environment {
    /// `CCDIG_MASK`, for `--mask`.
    mask: Option<OsString>,
    /// `CCDIG_MASK_FILE`, for `--mask-file`: paths separated as in `PATH`.
    mask_file: Option<OsString>,
    /// `CCDIG_ROOT`, for `--root`.
    root: Option<OsString>,
}

/// What `--mask` can ask to mask besides the built-in rules.
#[derive(Clone, Copy, Eq, PartialEq, ValueEnum)]
enum MaskKind {
    /// Email addresses.
    Email,
    /// IPv4 and IPv6 addresses.
    Ip,
    /// The user name, the last part of `$HOME`.
    User,
}

/// What `--type` can pick.
#[derive(Clone, Copy, Eq, PartialEq, ValueEnum)]
enum RecordType {
    /// Tool calls.
    Call,
    /// The prompts a person typed.
    Prompt,
    /// The assistant's text blocks.
    Reply,
    /// Tool results.
    Result,
}

/// What `--status` can keep.
#[derive(Clone, Copy, Eq, PartialEq, ValueEnum)]
enum Status {
    /// Results the tool reported as failures.
    Error,
    /// Results the tool did not report as failures.
    Ok,
}

/// The output formats of `records` and `sessions`.
#[derive(Clone, Copy, ValueEnum)]
enum Format {
    /// One indented JSON document.
    Json,
    /// One JSON object per line.
    Jsonl,
    /// Markdown for reading.
    #[value(name = "md")]
    Markdown,
}

/// The `--format` flag.
#[derive(Args)]
struct Output {
    /// Output format.
    #[arg(
        long,
        default_value = "jsonl",
        help_heading = "Output",
        display_order = 60
    )]
    format: Format,
}

/// What a subcommand produced.
struct Printed {
    /// What goes to stdout.
    out: String,
    /// The warning lines for stderr.
    warnings: Vec<String>,
}

/// The kind-specific filters, empty for `sessions` and `digest`.
#[derive(Default)]
struct Narrowing {
    /// `--status`: whether to keep only failed results, only the others,
    /// or both.
    is_error: Option<bool>,
    /// `--skill`.
    skills: Vec<String>,
    /// `--subagents`.
    subagents: bool,
    /// `--tool`.
    tools: Vec<String>,
}

impl Common {
    /// Reads every transcript into records of the kind `new_picker` picks.
    fn collect<P>(
        &self,
        narrowing: Narrowing,
        new_picker: impl Fn() -> P,
    ) -> Result<(Corpus<P::Record>, TimeZone), Failure>
    where
        P: Pick,
    {
        let zone = load_zone(self.tz.as_deref().unwrap_or("Local"))?;
        let query = Query {
            branches: self.branches.clone(),
            is_error: narrowing.is_error,
            grep: match (self.grep.as_deref(), self.igrep.as_deref()) {
                (Some(pattern), _) => Some(compile_grep(pattern, false)?),
                (None, Some(pattern)) => Some(compile_grep(pattern, true)?),
                (None, None) => None,
            },
            projects: self.projects.clone(),
            range: DayRange::parse(
                self.since.as_deref().unwrap_or_default(),
                self.until.as_deref().unwrap_or_default(),
                &zone,
            )?,
            sessions: self.sessions.clone(),
            skills: narrowing.skills,
            tools: narrowing.tools,
        };
        let root = self.history_root()?;
        let mut corpus = collect(
            &root,
            narrowing.subagents,
            &query,
            &Mask::new(self.no_mask, self.extra_masks()?)?,
            new_picker,
        )?;
        if let Some(limit) = self.limit {
            corpus.records.truncate(limit);
            corpus.sessions.truncate(limit);
        }
        Ok((corpus, zone))
    }

    /// The masks `--mask` and `--mask-file` ask for.
    fn extra_masks(&self) -> Result<ExtraMasks, Failure> {
        if self.no_mask && !(self.masks.is_empty() && self.mask_files.is_empty()) {
            return Err(Failure::MaskWithNoMask);
        }
        let mut values = Vec::new();
        for path in &self.mask_files {
            let read = fs::read_to_string(path).map_err(|source| Failure::MaskFile {
                path: path.display().to_string(),
                source,
            })?;
            // An editor may start the file with a byte order mark.
            let text = read.strip_prefix('\u{feff}').unwrap_or(&read);
            values.extend(
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned),
            );
        }
        let user = if self.masks.contains(&MaskKind::User) {
            let home = env::var("HOME").unwrap_or_default();
            Some(user_name(&home).ok_or(Failure::MaskUser)?)
        } else {
            None
        };
        Ok(ExtraMasks {
            emails: self.masks.contains(&MaskKind::Email),
            ips: self.masks.contains(&MaskKind::Ip),
            user,
            values,
        })
    }

    /// `--root`, or `~/.claude/projects`.
    fn history_root(&self) -> Result<PathBuf, Failure> {
        if let Some(ref root) = self.root {
            return Ok(root.clone());
        }
        let home = env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .ok_or(Failure::Home)?;
        Ok(PathBuf::from(home).join(".claude").join("projects"))
    }
    /// These flags given before the subcommand, combined with `later`,
    /// given after it.
    fn merged(mut self, later: Self) -> Self {
        self.branches.extend(later.branches);
        self.projects.extend(later.projects);
        self.sessions.extend(later.sessions);
        self.masks.extend(later.masks);
        self.mask_files.extend(later.mask_files);
        // --grep and --igrep are one pattern: the later level's, if any.
        let (grep, igrep) = if later.grep.is_some() || later.igrep.is_some() {
            (later.grep, later.igrep)
        } else {
            (self.grep, self.igrep)
        };
        Self {
            branches: self.branches,
            grep,
            igrep,
            limit: later.limit.or(self.limit),
            mask_files: self.mask_files,
            masks: self.masks,
            no_mask: self.no_mask || later.no_mask,
            projects: self.projects,
            root: later.root.or(self.root),
            sessions: self.sessions,
            since: later.since.or(self.since),
            tz: later.tz.or(self.tz),
            until: later.until.or(self.until),
        }
    }

    /// These flags with the defaults in `environment` filled in. Masks
    /// from the environment are dropped under `--no-mask`.
    fn with_env(mut self, environment: &Environment) -> Result<Self, Failure> {
        if self.root.is_none() {
            self.root = environment.root.clone().map(PathBuf::from);
        }
        if self.no_mask {
            return Ok(self);
        }
        if self.masks.is_empty()
            && let Some(ref value) = environment.mask
        {
            self.masks = mask_kinds(value)?;
        }
        if self.mask_files.is_empty()
            && let Some(ref value) = environment.mask_file
        {
            self.mask_files = env::split_paths(value)
                .filter(|path| !path.as_os_str().is_empty())
                .collect();
        }
        Ok(self)
    }
}

impl Environment {
    /// Reads the variables, leaving out empty ones.
    fn read() -> Self {
        let var = |name: &str| env::var_os(name).filter(|value| !value.is_empty());
        Self {
            mask: var("CCDIG_MASK"),
            mask_file: var("CCDIG_MASK_FILE"),
            root: var("CCDIG_ROOT"),
        }
    }
}

impl Format {
    /// Writes `records` in this format, using `markdown` for Markdown.
    fn write<T>(
        self,
        records: &[T],
        markdown: impl FnOnce(&[T]) -> Result<String, Failure>,
    ) -> Result<String, Failure>
    where
        T: Serialize,
    {
        match self {
            Self::Json => document(FILTER_VERSION, records),
            Self::Jsonl => lines(records),
            Self::Markdown => markdown(records),
        }
    }
}

impl From<Activity> for Narrowing {
    fn from(activity: Activity) -> Self {
        Self {
            skills: activity.skills,
            subagents: activity.subagents,
            ..Self::default()
        }
    }
}

/// Runs `command`, with `earlier` the shared flags given before it, and
/// returns its output.
fn execute(
    earlier: Common,
    command: Command,
    environment: &Environment,
) -> Result<Printed, Failure> {
    let (out, warnings) = match command {
        Command::Digest {
            common: later,
            max_prompt_chars,
            max_session_chars,
        } => {
            let flags = earlier.merged(later).with_env(environment)?;
            let limits = digest_limits(max_prompt_chars, max_session_chars)?;
            let (corpus, zone) = flags.collect(Narrowing::default(), || Prompts)?;
            (digest(&corpus.records, &zone, limits)?, corpus.warnings)
        }
        Command::Records {
            activity,
            common: later,
            output,
            statuses,
            tools,
            types,
        } => {
            let kinds = record_types(&types);
            let assistant = kinds.reads_assistant();
            for (given, applies, flag, needed) in [
                (
                    !tools.is_empty(),
                    kinds.calls || kinds.results,
                    "--tool",
                    "call or result",
                ),
                (!statuses.is_empty(), kinds.results, "--status", "result"),
                (
                    !activity.skills.is_empty(),
                    assistant,
                    "--skill",
                    "reply, call, or result",
                ),
                (
                    activity.subagents,
                    assistant,
                    "--subagents",
                    "reply, call, or result",
                ),
            ] {
                if given && !applies {
                    return Err(Failure::TypeForFlag {
                        flag,
                        kinds: needed,
                    });
                }
            }
            let narrowing = Narrowing {
                is_error: status_filter(&statuses),
                tools,
                ..activity.into()
            };
            list(
                &earlier.merged(later).with_env(environment)?,
                narrowing,
                &output,
                || Records::new(kinds),
                markdown::records,
            )?
        }
        Command::Sessions {
            common: later,
            output,
        } => {
            let flags = earlier.merged(later).with_env(environment)?;
            let (corpus, zone) = flags.collect(Narrowing::default(), || Prompts)?;
            let out = output.format.write(&corpus.sessions, |sessions| {
                markdown::sessions(sessions, &zone)
            })?;
            (out, corpus.warnings)
        }
        Command::Version(_) => (
            concat!("ccdig v", env!("CARGO_PKG_VERSION"), "\n").to_owned(),
            Vec::new(),
        ),
    };
    Ok(Printed { out, warnings })
}

/// Collects the records `new_picker` picks under `flags` and `narrowing`,
/// and writes them in the format of `output`, with `markdown` for Markdown.
/// Returns the output and the warnings.
fn list<P>(
    flags: &Common,
    narrowing: Narrowing,
    output: &Output,
    new_picker: impl Fn() -> P,
    markdown: impl FnOnce(&[P::Record], &TimeZone) -> Result<String, Failure>,
) -> Result<(String, Vec<String>), Failure>
where
    P: Pick,
    P::Record: Serialize,
{
    let (corpus, zone) = flags.collect(narrowing, new_picker)?;
    let out = output
        .format
        .write(&corpus.records, |records| markdown(records, &zone))?;
    Ok((out, corpus.warnings))
}

/// The user name in `home`: its last part, or `None` for an empty or root
/// directory or a name that is not UTF-8.
fn user_name(home: &str) -> Option<String> {
    Path::new(home.trim_end_matches('/'))
        .file_name()?
        .to_str()
        .map(str::to_owned)
}

/// The kinds `--type` picks, or prompts, replies, and calls when it is
/// not given.
fn record_types(types: &[RecordType]) -> Types {
    if types.is_empty() {
        return Types {
            calls: true,
            prompts: true,
            replies: true,
            results: false,
        };
    }
    Types {
        calls: types.contains(&RecordType::Call),
        prompts: types.contains(&RecordType::Prompt),
        replies: types.contains(&RecordType::Reply),
        results: types.contains(&RecordType::Result),
    }
}

/// What `--status` keeps: only failed results, only the others, or
/// `None` for both.
fn status_filter(statuses: &[Status]) -> Option<bool> {
    match (
        statuses.contains(&Status::Error),
        statuses.contains(&Status::Ok),
    ) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        (true, true) | (false, false) => None,
    }
}

/// The masks `CCDIG_MASK` names, comma-separated as `--mask` takes them.
fn mask_kinds(value: &OsStr) -> Result<Vec<MaskKind>, Failure> {
    let text = value.to_string_lossy();
    text.split(',')
        .map(|name| {
            MaskKind::from_str(name.trim(), false).map_err(|_unknown| Failure::EnvMask {
                known: mask_names(),
                name: name.trim().to_owned(),
            })
        })
        .collect()
}

/// The names `--mask` takes, as `a, b, or c`.
fn mask_names() -> String {
    let names: Vec<String> = MaskKind::value_variants()
        .iter()
        .filter_map(ValueEnum::to_possible_value)
        .map(|value| value.get_name().to_owned())
        .collect();
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{}, or {last}", rest.join(", ")),
        Some((last, _)) => last.clone(),
        None => String::new(),
    }
}

/// Checks the digest limits and converts them.
fn digest_limits(prompt_chars: i64, session_chars: i64) -> Result<Limits, Failure> {
    let (Ok(prompt), Ok(session)) = (
        usize::try_from(prompt_chars),
        usize::try_from(session_chars),
    ) else {
        return Err(Failure::DigestLimitNotPositive);
    };
    if prompt == 0 || session == 0 {
        return Err(Failure::DigestLimitNotPositive);
    }
    if prompt >= session {
        return Err(Failure::DigestPromptLimitTooLarge);
    }
    Ok(Limits {
        prompt_chars: prompt,
        session_chars: session,
    })
}

/// Writes `message` as one `ccdig: ` line on stderr and returns the exit
/// status 1. A failed write is ignored: there is nowhere left to report it.
fn fail(stderr: &mut dyn Write, message: &str) -> u8 {
    drop(writeln!(stderr, "ccdig: {}", one_line(message)));
    1
}

/// Runs ccdig with `args`; see `crate::run`.
pub fn run<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    run_in(args, &Environment::read(), stdout, stderr)
}

/// Runs ccdig with `args` and the `CCDIG_*` variables in `environment`.
fn run_in<I, T>(
    args: I,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) if error.use_stderr() => {
            let rendered = error.render().to_string();
            let line = rendered.lines().next().unwrap_or_default();
            return fail(stderr, line.strip_prefix("error: ").unwrap_or(line));
        }
        Err(error) => return write_out(stdout, stderr, &error.render().to_string()),
    };
    let Some(command) = cli.command else {
        return write_out(stdout, stderr, &Cli::command().render_help().to_string());
    };
    match execute(cli.common, command, environment) {
        Ok(printed) => {
            for warning in &printed.warnings {
                drop(writeln!(stderr, "{warning}"));
            }
            write_out(stdout, stderr, &printed.out)
        }
        Err(failure) => fail(stderr, &failure.to_string()),
    }
}

/// Writes `out` to `stdout`. A closed pipe, as under `| head`, ends the run
/// quietly with status 0.
fn write_out(stdout: &mut dyn Write, stderr: &mut dyn Write, out: &str) -> u8 {
    match stdout
        .write_all(out.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => 0,
        Err(error) if error.kind() == ErrorKind::BrokenPipe => 0,
        Err(error) => fail(stderr, &format!("write output: {error}")),
    }
}

#[cfg(test)]
mod tests;
