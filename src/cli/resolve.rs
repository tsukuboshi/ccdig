//! Turning the command line and the environment into a `Config`: the flags
//! given before and after the subcommand are merged, the environment fills
//! in the defaults, and every value is checked and converted once.

use std::path::{Path, PathBuf};

use super::args::{Activity, Command, Common, MaskKind, RecordType, Status, mask_kinds, on_skip};
use super::environment::Environment;
use super::values::{self, Values};
use crate::collect::Reading;
use crate::collect::kinds::Types;
use crate::collect::mask::Mask;
use crate::config::{Config, Format, Shared};
use crate::failure::Failure;
use crate::filter::{Filter, Query, patterns};
use crate::redact::ExtraMasks;
use crate::render::digest::Limits;
use crate::render::json::Run;
use crate::when::DayRange;

/// The shared flags with the environment's defaults filled in, and what
/// they ask to be looked up.
struct Flags {
    /// The flags.
    common: Common,
    /// The git user name and email, looked up for `--mask git-user`.
    git_users: Vec<String>,
    /// The values `--mask-values` gave.
    values: Values,
}

/// The kind-specific filters, empty for `sessions` and `digest`.
#[derive(Default)]
struct Narrowing {
    /// `--around`: how many records around each match to keep.
    around: Option<usize>,
    /// `--context`: how many lines around each matched line to list.
    context: Option<usize>,
    /// `--status`: whether to keep only failed results, only the others,
    /// or both.
    is_error: Option<bool>,
    /// For `sessions`: whether a search counts the prompts and replies it
    /// matches.
    mentions: bool,
    /// `--skill`.
    skills: Vec<String>,
    /// `--subagents`.
    subagents: bool,
    /// `--tool`.
    tools: Vec<String>,
    /// Whether to add up each session's tokens, for `sessions`.
    usage: bool,
}

impl Flags {
    /// `--root`, or `~/.claude/projects`; `with_env` has already filled in
    /// `CCDIG_ROOT` and `CLAUDE_CONFIG_DIR`.
    fn history_root(&self, environment: &Environment) -> Result<PathBuf, Failure> {
        if let Some(ref root) = self.common.root {
            return Ok(root.clone());
        }
        let home = environment.home.as_ref().ok_or(Failure::Home)?;
        Ok(PathBuf::from(home).join(".claude").join("projects"))
    }

    /// What to read and how to report it, with the filters in `narrowing`.
    /// The checks run in the order the errors are reported.
    fn shared(self, narrowing: Narrowing, environment: &Environment) -> Result<Shared, Failure> {
        let zone = environment.zone(self.common.tz.as_deref().unwrap_or("Local"))?;
        let grep = patterns(&self.common.grep, &self.common.igrep)?;
        let finds = !grep.include.is_empty();
        if narrowing.around.is_some() && !finds {
            return Err(Failure::AroundNeedsGrep);
        }
        if narrowing.context.is_some() && !finds {
            return Err(Failure::ContextNeedsGrep);
        }
        let range = DayRange::parse(
            self.common.since.as_deref().unwrap_or_default(),
            self.common.until.as_deref().unwrap_or_default(),
            &zone,
        )?;
        let root = self.history_root(environment)?;
        let user = self.user(environment)?;
        let sessions = current_session(&self.common.sessions, environment)?;
        let Self {
            common,
            git_users,
            values,
        } = self;
        let has = |kind| common.masks.contains(&kind);
        let mask = Mask::new(
            common.no_mask,
            ExtraMasks {
                aws_accounts: has(MaskKind::AwsAccount),
                aws_profiles: has(MaskKind::AwsProfile),
                emails: has(MaskKind::Email),
                git_users,
                ips: has(MaskKind::Ip),
                user,
                values: values.pairs,
            },
        )?;
        let mut names = Vec::new();
        for name in common.masks.iter().filter_map(|kind| kind.name()) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        Ok(Shared {
            limit: common.limit,
            mask,
            mask_report: common.mask_report.map(Into::into),
            on_skip: common.on_skip.map_or_default(Into::into),
            query: Query {
                branches: Filter::names(common.branches),
                grep,
                is_error: narrowing.is_error,
                projects: Filter::names(common.projects),
                range,
                sessions: Filter::names(sessions),
                skills: Filter::names(narrowing.skills),
                tools: Filter::names(narrowing.tools),
            },
            reading: Reading {
                around: narrowing.around,
                excerpt: narrowing.context,
                mentions: narrowing.mentions && finds,
                subagents: narrowing.subagents,
                usage: narrowing.usage,
            },
            root,
            run: Run {
                since: common.since,
                until: common.until,
                tz: common.tz,
                mask: names,
                mask_values: values.labels,
                no_mask: common.no_mask,
            },
            zone,
        })
    }

    /// The user name `--mask user` asks to mask, after checking that
    /// `--no-mask` is not given with `--mask` or `--mask-values`.
    fn user(&self, environment: &Environment) -> Result<Option<String>, Failure> {
        let common = &self.common;
        if common.no_mask && !(common.masks.is_empty() && common.mask_values.is_empty()) {
            return Err(Failure::MaskWithNoMask);
        }
        if !common.masks.contains(&MaskKind::User) {
            return Ok(None);
        }
        let home = environment
            .home
            .as_deref()
            .and_then(|home| home.to_str())
            .unwrap_or_default();
        user_name(home).map(Some).ok_or(Failure::MaskUser)
    }

    /// `common` with the defaults in `environment` filled in. Masks from
    /// the environment are dropped under `--no-mask`.
    fn with_env(mut common: Common, environment: &Environment) -> Result<Self, Failure> {
        if common.root.is_none() {
            common.root = environment.root.clone().map(PathBuf::from).or_else(|| {
                environment
                    .claude_config_dir
                    .as_ref()
                    .map(|dir| PathBuf::from(dir).join("projects"))
            });
        }
        if common.on_skip.is_none()
            && let Some(ref value) = environment.on_skip
        {
            common.on_skip = Some(on_skip(value)?);
        }
        let mut flags = Self {
            common,
            git_users: Vec::new(),
            values: Values::default(),
        };
        if flags.common.no_mask {
            return Ok(flags);
        }
        if flags.common.masks.is_empty()
            && let Some(ref value) = environment.mask
        {
            flags.common.masks = mask_kinds(value)?;
        }
        if flags.common.mask_values.is_empty()
            && let Some(ref value) = environment.mask_values
        {
            flags.common.mask_values = vec![value.to_string_lossy().into_owned()];
        }
        flags.values = values::parse(&flags.common.mask_values, Environment::read_file)?;
        if flags.common.masks.contains(&MaskKind::GitUser) {
            for key in ["user.name", "user.email"] {
                flags.git_users.push(
                    environment
                        .git_config(key)
                        .ok_or(Failure::MaskGitUser { key })?,
                );
            }
        }
        Ok(flags)
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

/// The `--session` values with `current`, alone or after `!`, replaced by
/// the ID of the session ccdig runs in.
///
/// # Errors
///
/// When `current` is given and the environment names no session.
fn current_session(sessions: &[String], environment: &Environment) -> Result<Vec<String>, Failure> {
    sessions
        .iter()
        .map(|value| {
            let (bang, name) = value
                .strip_prefix('!')
                .map_or(("", value.as_str()), |name| ("!", name));
            if name != "current" {
                return Ok(value.clone());
            }
            let id = environment
                .session_id
                .as_deref()
                .ok_or(Failure::NoCurrentSession)?;
            // `Filter::names` reads a leading `!` as leaving out, so an ID
            // that starts with one is escaped to stay an ID.
            let shown = id.to_string_lossy();
            let escape = if bang.is_empty() && shown.starts_with('!') {
                "\\"
            } else {
                ""
            };
            Ok(format!("{bang}{escape}{shown}"))
        })
        .collect()
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

/// Checks `--max-input-chars`, which applies to Markdown only, and converts
/// it.
fn input_limit(chars: Option<i64>, format: Format) -> Result<Option<usize>, Failure> {
    let Some(given) = chars else {
        return Ok(None);
    };
    if format != Format::Markdown {
        return Err(Failure::InputLimitNeedsMarkdown);
    }
    match usize::try_from(given) {
        Ok(limit) if limit > 0 => Ok(Some(limit)),
        Ok(_) | Err(_) => Err(Failure::InputLimitNotPositive),
    }
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

/// Resolves `command`, with `earlier` the shared flags given before it,
/// into what it runs. The checks run in a fixed order, so the first error
/// reported does not depend on how the code is laid out.
///
/// # Errors
///
/// When a flag or an environment variable holds a value it does not take,
/// a file a flag names cannot be read, or a value to look up is missing.
pub fn resolve(
    earlier: Common,
    command: Command,
    environment: &Environment,
) -> Result<Config, Failure> {
    match command {
        Command::Digest {
            common: later,
            max_prompt_chars,
            max_session_chars,
        } => {
            let flags = Flags::with_env(earlier.merged(later), environment)?;
            let limits = digest_limits(max_prompt_chars, max_session_chars)?;
            Ok(Config::Digest {
                limits,
                shared: Box::new(flags.shared(Narrowing::default(), environment)?),
            })
        }
        Command::Records {
            activity,
            around,
            common: later,
            context,
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
            let format = output.format.into();
            let input_chars = input_limit(max_input_chars, format)?;
            let narrowing = Narrowing {
                around,
                context,
                is_error: status_filter(&statuses),
                tools,
                ..activity.into()
            };
            let flags = Flags::with_env(earlier.merged(later), environment)?;
            Ok(Config::Records {
                format,
                input_chars,
                kinds,
                shared: Box::new(flags.shared(narrowing, environment)?),
            })
        }
        Command::Sessions {
            common: later,
            output,
        } => {
            let flags = Flags::with_env(earlier.merged(later), environment)?;
            let narrowing = Narrowing {
                mentions: true,
                usage: true,
                ..Narrowing::default()
            };
            Ok(Config::Sessions {
                format: output.format.into(),
                shared: Box::new(flags.shared(narrowing, environment)?),
            })
        }
        Command::Version {
            common: _ignored,
            format,
        } => Ok(Config::Version {
            format: format.into(),
        }),
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

/// The user name in `home`: its last part, or `None` for an empty or root
/// directory or a name that is not UTF-8.
pub fn user_name(home: &str) -> Option<String> {
    Path::new(home.trim_end_matches('/'))
        .file_name()?
        .to_str()
        .map(str::to_owned)
}
