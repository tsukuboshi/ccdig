//! The command line: the flags, the subcommands, and how help, output, and
//! errors are written.

use std::env;
use std::ffi::OsString;
use std::io::{ErrorKind, Write};
use std::path::PathBuf;

use clap::{Args, CommandFactory as _, Parser, Subcommand};
use jiff::tz::TimeZone;
use serde::Serialize;

use crate::classify::human::FILTER_VERSION;
use crate::collect::kinds::{Prompts, Replies, Results, Tools};
use crate::collect::{Corpus, Mask, Pick, collect};
use crate::failure::Failure;
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
    long_about = "ccdig reads Claude Code session transcripts and prints one kind of record:\n\
                  the prompts a person typed, the assistant's replies, tool calls, or tool\n\
                  results. Harness noise is dropped, slash commands are folded back, and\n\
                  secrets are masked.",
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

/// The flags of the subcommands that read assistant lines.
#[derive(Args)]
struct Activity {
    /// Keep records written while this skill ran.
    #[arg(long, value_name = "NAME")]
    skill: Option<String>,
    /// Also read sub-agent transcripts.
    #[arg(long)]
    subagents: bool,
}

/// The subcommands.
#[derive(Subcommand)]
enum Command {
    /// Write a size-capped Markdown digest of the prompts for an LLM.
    Digest {
        /// The shared flags.
        #[command(flatten)]
        common: Common,
        /// Truncate each prompt to this many characters.
        #[arg(long, default_value_t = 300)]
        max_prompt_chars: i64,
        /// List at most this many characters per session.
        #[arg(long, default_value_t = 2000)]
        max_session_chars: i64,
    },
    /// List the prompts a person typed.
    Prompts {
        /// The shared flags.
        #[command(flatten)]
        common: Common,
        /// The output format.
        #[command(flatten)]
        output: Output,
    },
    /// List the assistant's text replies.
    Replies {
        /// The flags for assistant lines.
        #[command(flatten)]
        activity: Activity,
        /// The shared flags.
        #[command(flatten)]
        common: Common,
        /// The output format.
        #[command(flatten)]
        output: Output,
    },
    /// List tool results with the calls they answer.
    Results {
        /// The flags for assistant lines.
        #[command(flatten)]
        activity: Activity,
        /// The shared flags.
        #[command(flatten)]
        common: Common,
        /// Keep only results the tool reported as failures.
        #[arg(long)]
        errors: bool,
        /// The output format.
        #[command(flatten)]
        output: Output,
        /// Keep results of calls to this tool (repeatable).
        #[arg(long = "tool", value_name = "NAME")]
        tools: Vec<String>,
    },
    /// List sessions that have at least one selected prompt.
    Sessions {
        /// The shared flags.
        #[command(flatten)]
        common: Common,
        /// The output format.
        #[command(flatten)]
        output: Output,
    },
    /// List tool calls with their input.
    Tools {
        /// The flags for assistant lines.
        #[command(flatten)]
        activity: Activity,
        /// The shared flags.
        #[command(flatten)]
        common: Common,
        /// The output format.
        #[command(flatten)]
        output: Output,
        /// Keep calls to this tool (repeatable).
        #[arg(long = "tool", value_name = "NAME")]
        tools: Vec<String>,
    },
    /// Print the ccdig version.
    // The shared flags are accepted and ignored, as promptdig's were.
    Version(Common),
}

/// The flags every subcommand takes, before or after its name. clap's
/// global flags keep only the last occurrence, so the flags are declared on
/// both levels and merged: lists add up, a later value wins, and a switch
/// given on either level is on.
#[derive(Args)]
struct Common {
    /// Keep records on this git branch (repeatable).
    #[arg(long = "branch", value_name = "NAME")]
    branches: Vec<String>,
    /// Keep records whose text matches this regular expression, after
    /// masking.
    #[arg(long, value_name = "REGEX")]
    grep: Option<String>,
    /// Match --grep regardless of case.
    #[arg(short = 'i', long)]
    ignore_case: bool,
    /// Print at most this many records.
    #[arg(long, value_name = "N")]
    limit: Option<usize>,
    /// Do not mask secrets.
    #[arg(long)]
    no_redact: bool,
    /// Keep projects whose directory name or cwd contains this
    /// (repeatable).
    #[arg(long = "project", value_name = "SUBSTRING")]
    projects: Vec<String>,
    /// Transcript root [default: ~/.claude/projects].
    #[arg(long, value_name = "DIR")]
    root: Option<PathBuf>,
    /// Keep sessions whose ID starts with this (repeatable).
    #[arg(long = "session", value_name = "ID")]
    sessions: Vec<String>,
    /// First day to include, YYYY-MM-DD (record time, in --tz).
    #[arg(long, value_name = "DAY")]
    since: Option<String>,
    /// IANA time zone for --since, --until, and Markdown times [default:
    /// Local, the system zone; set TZ to override].
    #[arg(long, value_name = "ZONE")]
    tz: Option<String>,
    /// Last day to include, YYYY-MM-DD (record time, in --tz).
    #[arg(long, value_name = "DAY")]
    until: Option<String>,
}

/// The output formats of the record subcommands.
#[derive(Clone, Copy)]
enum Format {
    /// One indented JSON document.
    Json,
    /// One JSON object per line.
    Jsonl,
    /// Markdown for reading.
    Markdown,
}

/// The `--format` flag.
#[derive(Args)]
struct Output {
    /// Output format: jsonl, json, or md.
    #[arg(long, default_value = "jsonl")]
    format: String,
}

/// What a subcommand produced.
struct Printed {
    /// What goes to stdout.
    out: String,
    /// The warning lines for stderr.
    warnings: Vec<String>,
}

/// The kind-specific filters, empty for the prompt subcommands.
#[derive(Default)]
struct Narrowing {
    /// `--errors`.
    errors: bool,
    /// `--skill`.
    skill: Option<String>,
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
            errors: narrowing.errors,
            grep: self
                .grep
                .as_deref()
                .map(|pattern| compile_grep(pattern, self.ignore_case))
                .transpose()?,
            projects: self.projects.clone(),
            range: DayRange::parse(
                self.since.as_deref().unwrap_or_default(),
                self.until.as_deref().unwrap_or_default(),
                &zone,
            )?,
            sessions: self.sessions.clone(),
            skill: narrowing.skill,
            tools: narrowing.tools,
        };
        let root = self.history_root()?;
        let mut corpus = collect(
            &root,
            narrowing.subagents,
            &query,
            &Mask::new(self.no_redact)?,
            new_picker,
        )?;
        if let Some(limit) = self.limit {
            corpus.records.truncate(limit);
            corpus.sessions.truncate(limit);
        }
        Ok((corpus, zone))
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
        Self {
            branches: self.branches,
            grep: later.grep.or(self.grep),
            ignore_case: self.ignore_case || later.ignore_case,
            limit: later.limit.or(self.limit),
            no_redact: self.no_redact || later.no_redact,
            projects: self.projects,
            root: later.root.or(self.root),
            sessions: self.sessions,
            since: later.since.or(self.since),
            tz: later.tz.or(self.tz),
            until: later.until.or(self.until),
        }
    }
}

impl Format {
    /// Parses `--format`.
    fn parse(name: &str) -> Result<Self, Failure> {
        match name {
            "json" => Ok(Self::Json),
            "jsonl" => Ok(Self::Jsonl),
            "md" => Ok(Self::Markdown),
            _ => Err(Failure::Format(name.to_owned())),
        }
    }

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
            skill: activity.skill,
            subagents: activity.subagents,
            ..Self::default()
        }
    }
}

/// Runs `command`, with `earlier` the shared flags given before it, and
/// returns its output.
fn execute(earlier: Common, command: Command) -> Result<Printed, Failure> {
    let (out, warnings) = match command {
        Command::Digest {
            common: later,
            max_prompt_chars,
            max_session_chars,
        } => {
            let flags = earlier.merged(later);
            let limits = digest_limits(max_prompt_chars, max_session_chars)?;
            let (corpus, zone) = flags.collect(Narrowing::default(), || Prompts)?;
            (digest(&corpus.records, &zone, limits)?, corpus.warnings)
        }
        Command::Prompts {
            common: later,
            output,
        } => {
            let flags = earlier.merged(later);
            let format = Format::parse(&output.format)?;
            let (corpus, zone) = flags.collect(Narrowing::default(), || Prompts)?;
            let out = format.write(&corpus.records, |records| markdown::prompts(records, &zone))?;
            (out, corpus.warnings)
        }
        Command::Replies {
            activity,
            common: later,
            output,
        } => {
            let flags = earlier.merged(later);
            let format = Format::parse(&output.format)?;
            let (corpus, zone) = flags.collect(activity.into(), || Replies)?;
            let out = format.write(&corpus.records, |records| markdown::replies(records, &zone))?;
            (out, corpus.warnings)
        }
        Command::Results {
            activity,
            common: later,
            errors,
            output,
            tools,
        } => {
            let flags = earlier.merged(later);
            let format = Format::parse(&output.format)?;
            let narrowing = Narrowing {
                errors,
                tools,
                ..activity.into()
            };
            let (corpus, zone) = flags.collect(narrowing, Results::default)?;
            let out = format.write(&corpus.records, |records| {
                markdown::tool_results(records, &zone)
            })?;
            (out, corpus.warnings)
        }
        Command::Sessions {
            common: later,
            output,
        } => {
            let flags = earlier.merged(later);
            let format = Format::parse(&output.format)?;
            let (corpus, zone) = flags.collect(Narrowing::default(), || Prompts)?;
            let out = format.write(&corpus.sessions, |sessions| {
                markdown::sessions(sessions, &zone)
            })?;
            (out, corpus.warnings)
        }
        Command::Tools {
            activity,
            common: later,
            output,
            tools,
        } => {
            let flags = earlier.merged(later);
            let format = Format::parse(&output.format)?;
            let narrowing = Narrowing {
                tools,
                ..activity.into()
            };
            let (corpus, zone) = flags.collect(narrowing, || Tools)?;
            let out = format.write(&corpus.records, |records| {
                markdown::tool_calls(records, &zone)
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
    match execute(cli.common, command) {
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
