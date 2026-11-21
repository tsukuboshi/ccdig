//! The command line: the flags, the subcommands, and how help, output, and
//! errors are written.

mod values;

use std::ffi::{OsStr, OsString};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::{env, process};

use clap::{Args, CommandFactory as _, Parser, Subcommand, ValueEnum};
use jiff::tz::TimeZone;
use serde::Serialize;

use crate::classify::human::FILTER_VERSION;
use crate::collect::kinds::{Prompts, Records, Types};
use crate::collect::{Corpus, Mask, Pick, collect};
use crate::failure::Failure;
use crate::redact::{Counts, ExtraMasks};
use crate::render::digest::{Limits, digest};
use crate::render::json::{Run, document, lines, version};
use crate::render::markdown;
use crate::render::record::Record;
use crate::select::{Filter, Query, patterns};
use crate::text::one_line;
use crate::when::{DayRange, load_zone};

/// The version of this build.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Pull the logs you want to share out of Claude Code session transcripts,
/// formatted and masked.
#[derive(Parser)]
#[command(
    name = "ccdig",
    long_about = "ccdig pulls the logs you want to share out of Claude Code session\n\
                  transcripts, formatted and masked. Filters for time, project, session,\n\
                  branch, text, kind of record, tool, outcome, and skill narrow the history\n\
                  down, and the result is Markdown to read or JSON Lines to process. Harness\n\
                  noise is dropped, slash commands are folded back, and common secrets are\n\
                  masked by default.",
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
    /// ran, or with a leading !, leave them out (repeatable).
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
        /// The shared flags.
        #[command(flatten)]
        common: Common,
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
    /// Keep projects whose directory name or cwd contains this, or with a
    /// leading !, leave them out (repeatable).
    #[arg(
        long = "project",
        value_name = "SUBSTRING",
        help_heading = "Filters",
        display_order = 23
    )]
    projects: Vec<String>,
    /// Keep sessions whose ID starts with this, or with a leading !, leave
    /// them out (repeatable).
    #[arg(
        long = "session",
        value_name = "PREFIX",
        help_heading = "Filters",
        display_order = 24
    )]
    sessions: Vec<String>,
    /// Keep records on this git branch, or with a leading !, leave them
    /// out (repeatable).
    #[arg(
        long = "branch",
        value_name = "NAME",
        help_heading = "Filters",
        display_order = 25
    )]
    branches: Vec<String>,
    /// Keep records whose text matches this regular expression, after
    /// masking, or with a leading !, leave them out (repeatable).
    #[arg(
        long = "grep",
        value_name = "REGEX",
        help_heading = "Filters",
        display_order = 26
    )]
    grep: Vec<String>,
    /// Like --grep, ignoring case.
    #[arg(
        long = "igrep",
        value_name = "REGEX",
        help_heading = "Filters",
        display_order = 27
    )]
    igrep: Vec<String>,
    /// Transcript root [default: `$CLAUDE_CONFIG_DIR/projects`, or
    /// ~/.claude/projects] [env: `CCDIG_ROOT`].
    #[arg(long, value_name = "DIR", help_heading = "Input", display_order = 40)]
    root: Option<PathBuf>,
    /// What to do when a file or line cannot be read: warn and go on, or
    /// also end with status 1 after writing the output [default: warn]
    /// [env: `CCDIG_ON_SKIP`].
    #[arg(
        long,
        value_name = "ACTION",
        help_heading = "Input",
        display_order = 42
    )]
    on_skip: Option<OnSkip>,
    /// Also mask these (comma-separated, repeatable) [env: `CCDIG_MASK`].
    #[arg(
        long = "mask",
        value_name = "KIND",
        value_delimiter = ',',
        help_heading = "Masking",
        display_order = 50
    )]
    masks: Vec<MaskKind>,
    /// The git user name and email, looked up for `--mask git-user`.
    #[arg(skip)]
    git_users: Vec<String>,
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
    mask_values: Vec<String>,
    /// The values `--mask-values` gave, read once by `with_env`.
    #[arg(skip)]
    values: values::Values,
    /// Do not mask anything, secrets included.
    #[arg(long, help_heading = "Masking", display_order = 52)]
    no_mask: bool,
    /// After the output, write on stderr how many times each rule masked
    /// something in the records printed, in all their fields as JSONL holds
    /// them, as text or as one JSON object.
    #[arg(
        long,
        value_name = "FORMAT",
        help_heading = "Masking",
        display_order = 53
    )]
    mask_report: Option<MaskReport>,
    /// Print at most this many records.
    #[arg(long, value_name = "N", help_heading = "Output", display_order = 61)]
    limit: Option<usize>,
}

/// The `CCDIG_*` environment variables and `CLAUDE_CONFIG_DIR`: defaults
/// for the flags they name, used when neither level of the command line
/// gives the flag. An empty variable counts as unset.
#[derive(Default)]
struct Environment {
    /// `CLAUDE_CONFIG_DIR`, Claude Code's config directory, whose
    /// `projects` is the root when neither `--root` nor `CCDIG_ROOT` is
    /// given.
    claude_config_dir: Option<OsString>,
    /// Looks up a git config value for `--mask git-user`; `None` runs
    /// `git config --get`.
    git_config: Option<fn(&str) -> Option<String>>,
    /// `CCDIG_MASK`, for `--mask`.
    mask: Option<OsString>,
    /// `CCDIG_MASK_VALUES`, for `--mask-values`.
    mask_values: Option<OsString>,
    /// `CCDIG_ON_SKIP`, for `--on-skip`.
    on_skip: Option<OsString>,
    /// `CCDIG_ROOT`, for `--root`.
    root: Option<OsString>,
}

/// What `--mask` can ask to mask besides the built-in rules.
#[derive(Clone, Copy, Eq, PartialEq, ValueEnum)]
enum MaskKind {
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
enum MaskReport {
    /// `{"masked":{"rule":count,...}}` on one line.
    Json,
    /// `ccdig: masked N by rule, ...`.
    Text,
}

/// What `--on-skip` does when a file or line cannot be read.
#[derive(Clone, Copy, Default, Eq, PartialEq, ValueEnum)]
enum OnSkip {
    /// Warn, write the output, and end with status 1.
    Fail,
    /// Warn and go on.
    #[default]
    Warn,
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
    /// The summaries Claude Code writes when it compacts the context.
    Summary,
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

/// The output formats of `version`.
#[derive(Clone, Copy, ValueEnum)]
enum VersionFormat {
    /// The version, the schema version, and the filter as one JSON object.
    Json,
    /// `ccdig v` and the version.
    Text,
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
    /// What to do when there are warnings.
    on_skip: OnSkip,
    /// What goes to stdout.
    out: String,
    /// The `--mask-report` line for stderr.
    report: Option<String>,
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
    /// Whether to add up each session's tokens, for `sessions`.
    usage: bool,
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
            branches: Filter::names(self.branches.clone()),
            is_error: narrowing.is_error,
            grep: patterns(&self.grep, &self.igrep)?,
            projects: Filter::names(self.projects.clone()),
            range: DayRange::parse(
                self.since.as_deref().unwrap_or_default(),
                self.until.as_deref().unwrap_or_default(),
                &zone,
            )?,
            sessions: Filter::names(self.sessions.clone()),
            skills: Filter::names(narrowing.skills),
            tools: Filter::names(narrowing.tools),
        };
        let root = self.history_root()?;
        let mut corpus = collect(
            &root,
            narrowing.subagents,
            narrowing.usage,
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

    /// The masks `--mask` and `--mask-values` ask for.
    fn extra_masks(&self) -> Result<ExtraMasks, Failure> {
        if self.no_mask && !(self.masks.is_empty() && self.mask_values.is_empty()) {
            return Err(Failure::MaskWithNoMask);
        }
        let user = if self.masks.contains(&MaskKind::User) {
            let home = env::var("HOME").unwrap_or_default();
            Some(user_name(&home).ok_or(Failure::MaskUser)?)
        } else {
            None
        };
        Ok(ExtraMasks {
            aws_accounts: self.masks.contains(&MaskKind::AwsAccount),
            aws_profiles: self.masks.contains(&MaskKind::AwsProfile),
            emails: self.masks.contains(&MaskKind::Email),
            git_users: self.git_users.clone(),
            ips: self.masks.contains(&MaskKind::Ip),
            user,
            values: self.values.pairs.clone(),
        })
    }

    /// `--root`, or `~/.claude/projects`; `with_env` has already filled in
    /// `CCDIG_ROOT` and `CLAUDE_CONFIG_DIR`.
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
        self.mask_values.extend(later.mask_values);
        self.grep.extend(later.grep);
        self.igrep.extend(later.igrep);
        Self {
            branches: self.branches,
            git_users: self.git_users,
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
            values: self.values,
        }
    }

    /// What a command printed: `out`, the `warnings`, and with
    /// `--mask-report`, the sum of what masking counted in each record.
    fn printed(
        &self,
        out: String,
        warnings: Vec<String>,
        masked: impl Iterator<Item = Counts>,
    ) -> Result<Printed, Failure> {
        let report = match self.mask_report {
            Some(format) => {
                let mut total = Counts::default();
                for counts in masked {
                    total.merge(&counts);
                }
                Some(mask_report(format, &total)?)
            }
            None => None,
        };
        Ok(Printed {
            on_skip: self.on_skip.unwrap_or_default(),
            out,
            report,
            warnings,
        })
    }

    /// The flags that decide what the records hold, for the JSON envelope.
    fn run(&self) -> Run {
        let mut mask = Vec::new();
        for kind in &self.masks {
            if let Some(value) = kind.to_possible_value() {
                let name = value.get_name().to_owned();
                if !mask.contains(&name) {
                    mask.push(name);
                }
            }
        }
        Run {
            since: self.since.clone(),
            until: self.until.clone(),
            tz: self.tz.clone(),
            mask,
            mask_values: self.values.labels.clone(),
            no_mask: self.no_mask,
        }
    }

    /// These flags with the defaults in `environment` filled in. Masks
    /// from the environment are dropped under `--no-mask`.
    fn with_env(mut self, environment: &Environment) -> Result<Self, Failure> {
        if self.root.is_none() {
            self.root = environment.root.clone().map(PathBuf::from).or_else(|| {
                environment
                    .claude_config_dir
                    .as_ref()
                    .map(|dir| PathBuf::from(dir).join("projects"))
            });
        }
        if self.on_skip.is_none()
            && let Some(ref value) = environment.on_skip
        {
            let name = value.to_string_lossy();
            self.on_skip = Some(OnSkip::from_str(name.trim(), false).map_err(|_unknown| {
                Failure::EnvValue {
                    known: value_names::<OnSkip>(),
                    name: name.trim().to_owned(),
                    variable: "CCDIG_ON_SKIP",
                }
            })?);
        }
        if self.no_mask {
            return Ok(self);
        }
        if self.masks.is_empty()
            && let Some(ref value) = environment.mask
        {
            self.masks = mask_kinds(value)?;
        }
        if self.mask_values.is_empty()
            && let Some(ref value) = environment.mask_values
        {
            self.mask_values = vec![value.to_string_lossy().into_owned()];
        }
        self.values = values::parse(&self.mask_values)?;
        if self.masks.contains(&MaskKind::GitUser) {
            let lookup = environment.git_config.unwrap_or(git_config);
            for key in ["user.name", "user.email"] {
                self.git_users
                    .push(lookup(key).ok_or(Failure::MaskGitUser { key })?);
            }
        }
        Ok(self)
    }
}

impl Environment {
    /// Reads the variables, leaving out empty ones.
    fn read() -> Self {
        let var = |name: &str| env::var_os(name).filter(|value| !value.is_empty());
        Self {
            claude_config_dir: var("CLAUDE_CONFIG_DIR"),
            git_config: None,
            mask: var("CCDIG_MASK"),
            mask_values: var("CCDIG_MASK_VALUES"),
            on_skip: var("CCDIG_ON_SKIP"),
            root: var("CCDIG_ROOT"),
        }
    }
}

impl Format {
    /// Writes `records` in this format, using `markdown` for Markdown and
    /// `run` in the JSON envelope.
    fn write<T>(
        self,
        records: &[T],
        run: &Run,
        markdown: impl FnOnce(&[T]) -> Result<String, Failure>,
    ) -> Result<String, Failure>
    where
        T: Serialize,
    {
        match self {
            Self::Json => document(FILTER_VERSION, VERSION, run, records),
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
    match command {
        Command::Digest {
            common: later,
            max_prompt_chars,
            max_session_chars,
        } => {
            let flags = earlier.merged(later).with_env(environment)?;
            let limits = digest_limits(max_prompt_chars, max_session_chars)?;
            let (corpus, zone) = flags.collect(Narrowing::default(), || Prompts)?;
            flags.printed(
                digest(&corpus.records, &zone, limits)?,
                corpus.warnings,
                corpus.records.iter().map(Record::masked),
            )
        }
        Command::Records {
            activity,
            common: later,
            max_input_chars,
            output,
            statuses,
            tools,
            types,
        } => {
            let kinds = record_types(
                &types,
                !tools.is_empty(),
                !statuses.is_empty(),
                !activity.skills.is_empty(),
            );
            check_kinds(kinds, &activity, !tools.is_empty(), !statuses.is_empty())?;
            let input_chars = input_limit(max_input_chars, output.format)?;
            let narrowing = Narrowing {
                is_error: status_filter(&statuses),
                tools,
                ..activity.into()
            };
            let flags = earlier.merged(later).with_env(environment)?;
            let (corpus, zone) = flags.collect(narrowing, || Records::new(kinds))?;
            let out = output
                .format
                .write(&corpus.records, &flags.run(), |records| {
                    markdown::records(records, &zone, input_chars)
                })?;
            flags.printed(
                out,
                corpus.warnings,
                corpus.records.iter().map(Record::masked),
            )
        }
        Command::Sessions {
            common: later,
            output,
        } => {
            let flags = earlier.merged(later).with_env(environment)?;
            let (corpus, zone) = flags.collect(
                Narrowing {
                    usage: true,
                    ..Narrowing::default()
                },
                || Prompts,
            )?;
            let out = output
                .format
                .write(&corpus.sessions, &flags.run(), |sessions| {
                    markdown::sessions(sessions, &zone)
                })?;
            flags.printed(
                out,
                corpus.warnings,
                corpus.sessions.iter().map(|session| session.masked.clone()),
            )
        }
        Command::Version {
            common: _ignored,
            format,
        } => Ok(Printed {
            on_skip: OnSkip::default(),
            out: match format {
                VersionFormat::Json => version(VERSION, FILTER_VERSION)?,
                VersionFormat::Text => format!("ccdig v{VERSION}\n"),
            },
            report: None,
            warnings: Vec::new(),
        }),
    }
}

/// Checks that each kind-specific flag given has a kind in `kinds` it
/// applies to: `tool` and `status` say whether `--tool` and `--status` are
/// given.
fn check_kinds(kinds: Types, activity: &Activity, tool: bool, status: bool) -> Result<(), Failure> {
    let assistant = kinds.reads_assistant();
    for (given, applies, flag, needed) in [
        (
            tool,
            kinds.calls || kinds.results,
            "--tool",
            "call or result",
        ),
        (status, kinds.results, "--status", "result"),
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
    Ok(())
}

/// The value of `key` in git config, or `None` when git is not there or
/// the value is not set or empty.
fn git_config(key: &str) -> Option<String> {
    let output = process::Command::new("git")
        .args(["config", "--get", key])
        .output()
        .ok()?;
    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    (output.status.success() && !trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// The user name in `home`: its last part, or `None` for an empty or root
/// directory or a name that is not UTF-8.
fn user_name(home: &str) -> Option<String> {
    Path::new(home.trim_end_matches('/'))
        .file_name()?
        .to_str()
        .map(str::to_owned)
}

/// The kinds `--type` picks. Without it: the kinds every one of `--tool`,
/// `--status`, and `--skill` given applies to, or prompts, replies, and
/// calls when none is.
fn record_types(types: &[RecordType], tool: bool, status: bool, skill: bool) -> Types {
    if types.is_empty() {
        if !(tool || status || skill) {
            return Types {
                calls: true,
                prompts: true,
                replies: true,
                results: false,
                summaries: false,
            };
        }
        // Every one of them applies to results.
        return Types {
            calls: !status,
            prompts: false,
            replies: !(tool || status),
            results: true,
            summaries: false,
        };
    }
    Types {
        calls: types.contains(&RecordType::Call),
        prompts: types.contains(&RecordType::Prompt),
        replies: types.contains(&RecordType::Reply),
        results: types.contains(&RecordType::Result),
        summaries: types.contains(&RecordType::Summary),
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
                known: value_names::<MaskKind>(),
                name: name.trim().to_owned(),
            })
        })
        .collect()
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

/// The `--mask-report` line for `total` in `format`, without a newline.
fn mask_report(format: MaskReport, total: &Counts) -> Result<String, Failure> {
    let ranked = total.ranked();
    match format {
        MaskReport::Json => {
            let counts: serde_json::Map<String, serde_json::Value> = ranked
                .into_iter()
                .map(|(rule, count)| (rule, count.into()))
                .collect();
            serde_json::to_string(&serde_json::json!({ "masked": counts })).map_err(Failure::Json)
        }
        MaskReport::Text if ranked.is_empty() => Ok("ccdig: masked nothing".to_owned()),
        MaskReport::Text => {
            let parts: Vec<String> = ranked
                .iter()
                .map(|&(ref rule, count)| format!("{count} by {rule}"))
                .collect();
            Ok(format!("ccdig: masked {}", parts.join(", ")))
        }
    }
}

/// Checks `--max-input-chars`, which applies to Markdown only, and converts
/// it.
fn input_limit(chars: Option<i64>, format: Format) -> Result<Option<usize>, Failure> {
    let Some(given) = chars else {
        return Ok(None);
    };
    if !matches!(format, Format::Markdown) {
        return Err(Failure::InputLimitNeedsMarkdown);
    }
    match usize::try_from(given) {
        Ok(limit) if limit > 0 => Ok(Some(limit)),
        Ok(_) | Err(_) => Err(Failure::InputLimitNotPositive),
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
            let code = write_out(stdout, stderr, &printed.out);
            if let Some(ref report) = printed.report {
                drop(writeln!(stderr, "{report}"));
            }
            if code == 0 && printed.on_skip == OnSkip::Fail && !printed.warnings.is_empty() {
                return fail(stderr, "failing as --on-skip is fail: input was skipped");
            }
            code
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
