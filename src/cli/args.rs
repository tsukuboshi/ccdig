//! The command line as clap sees it: the flags, the subcommands, and how
//! they parse. This is the one place that uses clap; the rest of the crate
//! gets plain values from it.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use clap::error::{ContextKind, ErrorKind};
use clap::{Args, CommandFactory as _, Parser, Subcommand, ValueEnum};

use crate::config;
use crate::failure::Failure;

/// The flags people type for `--type`, after the `kind` key of a prompt,
/// which is how it was typed, not its record type.
const KIND_FLAGS: [&str; 2] = ["--kind", "--kinds"];

/// Pull the logs you want to share out of Claude Code session transcripts,
/// formatted and masked.
#[derive(Parser)]
#[command(
    name = "ccdig",
    long_about = "ccdig pulls the logs you want to share out of Claude Code session\n\
                  transcripts, formatted and masked. Filters for time, project, session,\n\
                  branch, text, record type, tool, outcome, and skill narrow the history\n\
                  down, and the result is Markdown to read or JSON Lines to process. Harness\n\
                  noise is dropped, slash commands are folded back, and common secrets are\n\
                  masked by default.",
    after_help = "Examples:\n\
                  \x20 ccdig records --session '!current' --type prompt,reply --igrep deploy --format md --limit 20\n\
                  \x20 ccdig records --type result --tool Bash --status error --subagents --format md\n\
                  \x20 ccdig sessions --igrep deploy --format md\n\
                  \n\
                  ccdig records --help lists --type, --tool, --status, --skill, --subagents, --around, and --context.",
    disable_help_subcommand = true,
    disable_version_flag = true
)]
pub struct Cli {
    /// The subcommand; none prints the help.
    #[command(subcommand)]
    pub command: Option<Command>,
    /// The shared flags given before the subcommand.
    #[command(flatten)]
    pub common: Common,
}

/// The flags of `records` that apply to assistant lines and what answers
/// them.
#[derive(Args)]
pub struct Activity {
    /// Keep replies, tool calls, and tool results written while this skill
    /// ran, or with a leading !, leave them out (repeatable).
    #[arg(
        long = "skill",
        value_name = "NAME",
        help_heading = "Filters",
        display_order = 30
    )]
    pub skills: Vec<String>,
    /// Also read sub-agent transcripts, which are left out by default; the
    /// instruction a sub-agent was given is not listed.
    #[arg(long, help_heading = "Input", display_order = 41)]
    pub subagents: bool,
}

/// The subcommands. Help lists them by `display_order`, most used first.
#[derive(Subcommand)]
pub enum Command {
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
    #[command(
        display_order = 1,
        after_help = "Without --type, a search reads prompts, replies, and tool calls, and a\n\
                      tool call's input can be long: narrow it with --type and --limit, as in\n\
                      \x20 ccdig records --type prompt,reply --igrep deploy --limit 20"
    )]
    Records {
        /// The record types to list (comma-separated, repeatable)
        /// [default: prompt,reply,call, or with --tool, --status, or
        /// --skill, the kinds they all apply to].
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
        /// Also list this many records before and after each record --grep
        /// or --igrep matched, of the types --type picks and in the same
        /// file, with `matched` false.
        #[arg(long, value_name = "N", help_heading = "Records", display_order = 11)]
        around: Option<usize>,
        /// The shared flags.
        #[command(flatten)]
        common: Common,
        /// List the lines --grep or --igrep matched in each record, with
        /// this many lines before and after each, in `matches`; Markdown
        /// prints those lines in place of the whole text.
        #[arg(long, value_name = "N", help_heading = "Output", display_order = 61)]
        context: Option<usize>,
        /// In Markdown, cut each string in a tool call's input to this many
        /// characters [default: no limit].
        #[arg(long, value_name = "N", help_heading = "Output", display_order = 62)]
        max_input_chars: Option<i64>,
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
        /// Keep tool calls and results of this tool, or with a leading !,
        /// leave them out (repeatable).
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
    #[command(display_order = 4)]
    Version {
        /// The shared flags, accepted and ignored.
        #[command(flatten)]
        common: Common,
        /// Output format: the version as text, or with the schema and
        /// filter versions as one JSON object.
        #[arg(
            long,
            default_value = "text",
            help_heading = "Output",
            display_order = 60
        )]
        format: VersionFormat,
    },
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
pub struct Common {
    /// First day to include, YYYY-MM-DD (record time, in --tz).
    #[arg(long, value_name = "DAY", help_heading = "Filters", display_order = 20)]
    pub since: Option<String>,
    /// Last day to include, YYYY-MM-DD (record time, in --tz).
    #[arg(long, value_name = "DAY", help_heading = "Filters", display_order = 21)]
    pub until: Option<String>,
    /// IANA time zone for --since, --until, and Markdown times [default:
    /// Local, the system zone; set TZ to override].
    #[arg(
        long,
        value_name = "ZONE",
        help_heading = "Filters",
        display_order = 22
    )]
    pub tz: Option<String>,
    /// Keep projects whose directory name or cwd contains this, or with a
    /// leading !, leave them out (repeatable).
    #[arg(
        long = "project",
        value_name = "SUBSTRING",
        help_heading = "Filters",
        display_order = 23
    )]
    pub projects: Vec<String>,
    /// Keep sessions whose ID starts with this, or with a leading !, leave
    /// them out (repeatable); `current` is the session Claude Code runs
    /// ccdig in [env: `CLAUDE_CODE_SESSION_ID`].
    #[arg(
        long = "session",
        value_name = "PREFIX",
        help_heading = "Filters",
        display_order = 24
    )]
    pub sessions: Vec<String>,
    /// Keep records on this git branch, or with a leading !, leave them
    /// out (repeatable).
    #[arg(
        long = "branch",
        value_name = "NAME",
        help_heading = "Filters",
        display_order = 25
    )]
    pub branches: Vec<String>,
    /// Keep records whose text matches this regular expression, after
    /// masking, or with a leading !, leave them out (repeatable).
    #[arg(
        long = "grep",
        value_name = "REGEX",
        help_heading = "Filters",
        display_order = 26
    )]
    pub grep: Vec<String>,
    /// Like --grep, ignoring case.
    #[arg(
        long = "igrep",
        value_name = "REGEX",
        help_heading = "Filters",
        display_order = 27
    )]
    pub igrep: Vec<String>,
    /// Transcript root [default: `$CLAUDE_CONFIG_DIR/projects`, or
    /// ~/.claude/projects] [env: `CCDIG_ROOT`].
    #[arg(long, value_name = "DIR", help_heading = "Input", display_order = 40)]
    pub root: Option<PathBuf>,
    /// What to do when a file or line cannot be read: warn and go on, or
    /// also end with status 1 after writing the output [default: warn]
    /// [env: `CCDIG_ON_SKIP`].
    #[arg(
        long,
        value_name = "ACTION",
        help_heading = "Input",
        display_order = 42
    )]
    pub on_skip: Option<OnSkip>,
    /// Also mask these (comma-separated, repeatable) [env: `CCDIG_MASK`].
    #[arg(
        long = "mask",
        value_name = "KIND",
        value_delimiter = ',',
        help_heading = "Masking",
        display_order = 50
    )]
    pub masks: Vec<MaskKind>,
    /// Also mask the values in this JSON object of labels to lists of
    /// values, or with a leading @, in this file (repeatable): a value
    /// under a label becomes the label and a number, as in ［repo1］, and
    /// one under "" becomes ***REDACTED*** [env: `CCDIG_MASK_VALUES`].
    #[arg(
        long = "mask-values",
        value_name = "JSON",
        help_heading = "Masking",
        display_order = 51
    )]
    pub mask_values: Vec<String>,
    /// Do not mask anything, secrets included.
    #[arg(long, help_heading = "Masking", display_order = 52)]
    pub no_mask: bool,
    /// After the output, write on stderr how many times each rule masked
    /// something in the records printed, in all their fields as JSONL holds
    /// them, as text or as one JSON object.
    #[arg(
        long,
        value_name = "FORMAT",
        help_heading = "Masking",
        display_order = 53
    )]
    pub mask_report: Option<MaskReport>,
    /// Print at most this many records.
    #[arg(long, value_name = "N", help_heading = "Output", display_order = 61)]
    pub limit: Option<usize>,
}

/// What `--mask` can ask to mask besides the built-in rules.
#[derive(Clone, Copy, Eq, PartialEq, ValueEnum)]
pub enum MaskKind {
    /// AWS account IDs in ARNs, ECR hosts, and account fields.
    AwsAccount,
    /// AWS profile names after --profile, in `AWS_PROFILE`, and in config
    /// files.
    AwsProfile,
    /// Email addresses.
    Email,
    /// The `user.name` and `user.email` of git config.
    GitUser,
    /// IPv4 and IPv6 addresses.
    Ip,
    /// The user name, the last part of `$HOME`.
    User,
}

/// How `--mask-report` writes the counts.
#[derive(Clone, Copy, ValueEnum)]
pub enum MaskReport {
    /// `{"masked":{"rule":count,...}}` on one line.
    Json,
    /// `ccdig: masked N by rule, ...`.
    Text,
}

/// What `--on-skip` does when a file or line cannot be read.
#[derive(Clone, Copy, Default, Eq, PartialEq, ValueEnum)]
pub enum OnSkip {
    /// Warn, write the output, and end with status 1.
    Fail,
    /// Warn and go on.
    #[default]
    Warn,
}

/// What `--type` can pick.
#[derive(Clone, Copy, Eq, PartialEq, ValueEnum)]
pub enum RecordType {
    /// Tool calls.
    Call,
    /// The prompts a person typed.
    Prompt,
    /// The assistant's text blocks.
    Reply,
    /// Tool results.
    Result,
    /// The summaries Claude Code writes when it compacts the context.
    Summary,
}

/// What `--status` can keep.
#[derive(Clone, Copy, Eq, PartialEq, ValueEnum)]
pub enum Status {
    /// Results the tool reported as failures.
    Error,
    /// Results the tool did not report as failures.
    Ok,
}

/// The output formats of `records` and `sessions`.
#[derive(Clone, Copy, ValueEnum)]
pub enum Format {
    /// One indented JSON document.
    Json,
    /// One JSON object per line.
    Jsonl,
    /// Markdown for reading.
    #[value(name = "md")]
    Markdown,
}

/// The output formats of `version`.
#[derive(Clone, Copy, ValueEnum)]
pub enum VersionFormat {
    /// The version, the schema version, and the filter as one JSON object.
    Json,
    /// `ccdig v` and the version.
    Text,
}

/// The `--format` flag.
#[derive(Args)]
pub struct Output {
    /// Output format.
    #[arg(
        long,
        default_value = "jsonl",
        help_heading = "Output",
        display_order = 60
    )]
    pub format: Format,
}

/// What the command line asks for.
pub enum Parsed {
    /// Text for stdout, such as the help, and exit status 0.
    Print(String),
    /// A subcommand, with the shared flags given before it.
    Run {
        /// The subcommand.
        command: Box<Command>,
        /// The shared flags given before the subcommand.
        earlier: Box<Common>,
    },
    /// The first line of a usage error, without `error: `.
    Usage(String),
}

impl Common {
    /// These flags given before the subcommand, combined with `later`,
    /// given after it.
    pub fn merged(mut self, later: Self) -> Self {
        self.branches.extend(later.branches);
        self.projects.extend(later.projects);
        self.sessions.extend(later.sessions);
        self.masks.extend(later.masks);
        self.mask_values.extend(later.mask_values);
        self.grep.extend(later.grep);
        self.igrep.extend(later.igrep);
        Self {
            branches: self.branches,
            grep: self.grep,
            igrep: self.igrep,
            limit: later.limit.or(self.limit),
            mask_values: self.mask_values,
            masks: self.masks,
            mask_report: later.mask_report.or(self.mask_report),
            no_mask: self.no_mask || later.no_mask,
            on_skip: later.on_skip.or(self.on_skip),
            projects: self.projects,
            root: later.root.or(self.root),
            sessions: self.sessions,
            since: later.since.or(self.since),
            tz: later.tz.or(self.tz),
            until: later.until.or(self.until),
        }
    }
}

impl MaskKind {
    /// The name `--mask` takes for this mask.
    pub fn name(self) -> Option<String> {
        self.to_possible_value()
            .map(|value| value.get_name().to_owned())
    }
}

impl From<Format> for config::Format {
    fn from(format: Format) -> Self {
        match format {
            Format::Json => Self::Json,
            Format::Jsonl => Self::Jsonl,
            Format::Markdown => Self::Markdown,
        }
    }
}

impl From<MaskReport> for config::MaskReport {
    fn from(report: MaskReport) -> Self {
        match report {
            MaskReport::Json => Self::Json,
            MaskReport::Text => Self::Text,
        }
    }
}

impl From<OnSkip> for config::OnSkip {
    fn from(on_skip: OnSkip) -> Self {
        match on_skip {
            OnSkip::Fail => Self::Fail,
            OnSkip::Warn => Self::Warn,
        }
    }
}

impl From<VersionFormat> for config::VersionFormat {
    fn from(format: VersionFormat) -> Self {
        match format {
            VersionFormat::Json => Self::Json,
            VersionFormat::Text => Self::Text,
        }
    }
}

/// The masks `CCDIG_MASK` names, comma-separated as `--mask` takes them.
///
/// # Errors
///
/// When it names a mask `--mask` does not take.
pub fn mask_kinds(value: &OsStr) -> Result<Vec<MaskKind>, Failure> {
    let text = value.to_string_lossy();
    text.split(',')
        .map(|name| {
            MaskKind::from_str(name.trim(), false).map_err(|_unknown| Failure::EnvMask {
                known: value_names::<MaskKind>(),
                name: name.trim().to_owned(),
            })
        })
        .collect()
}

/// What `CCDIG_ON_SKIP` asks for.
///
/// # Errors
///
/// When it holds a value `--on-skip` does not take.
pub fn on_skip(value: &OsStr) -> Result<OnSkip, Failure> {
    let name = value.to_string_lossy();
    OnSkip::from_str(name.trim(), false).map_err(|_unknown| Failure::EnvValue {
        known: value_names::<OnSkip>(),
        name: name.trim().to_owned(),
        variable: "CCDIG_ON_SKIP",
    })
}

/// What to say after an error about an unknown flag that names a flag
/// ccdig has under another name, kept on the error's one line.
fn hint(error: &clap::Error) -> Option<String> {
    if error.kind() != ErrorKind::UnknownArgument {
        return None;
    }
    let arg = error.get(ContextKind::InvalidArg)?.to_string();
    KIND_FLAGS.contains(&arg.as_str()).then(|| {
        format!(
            "pick the record types with --type: {}",
            value_names::<RecordType>()
        )
    })
}

/// Parses `args`, the program name first.
pub fn parse<I, T>(args: I) -> Parsed
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) if error.use_stderr() => {
            let rendered = error.render().to_string();
            let first = rendered.lines().next().unwrap_or_default();
            let line = first.strip_prefix("error: ").unwrap_or(first);
            return Parsed::Usage(
                hint(&error).map_or_else(|| line.to_owned(), |hint| format!("{line}; {hint}")),
            );
        }
        Err(error) => return Parsed::Print(error.render().to_string()),
    };
    match cli.command {
        Some(command) => Parsed::Run {
            command: Box::new(command),
            earlier: Box::new(cli.common),
        },
        None => Parsed::Print(Cli::command().render_help().to_string()),
    }
}

/// The values a flag of `T` takes, as `a, b, or c`.
fn value_names<T>() -> String
where
    T: ValueEnum,
{
    let names: Vec<String> = T::value_variants()
        .iter()
        .filter_map(ValueEnum::to_possible_value)
        .map(|value| value.get_name().to_owned())
        .collect();
    match names.split_last() {
        Some((last, rest)) if rest.len() == 1 => format!("{} or {last}", rest.join(", ")),
        Some((last, rest)) if !rest.is_empty() => format!("{}, or {last}", rest.join(", ")),
        Some((last, _)) => last.clone(),
        None => String::new(),
    }
}
