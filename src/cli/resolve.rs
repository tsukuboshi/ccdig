//! Turning the command line and the environment into a `Config`: the flags
//! given before and after the subcommand are merged, the environment fills
//! in the defaults, and every value is checked and converted once.

use std::path::{Path, PathBuf};

use super::args::{
    Activity, Command, Common, MaskKind, Output, Prompted, RecordType, Selection, Status,
    env_value, listed, mask_kinds, value_names,
};
use super::environment::Environment;
use super::values::{self, Values};
use crate::collect::Reading;
use crate::collect::kinds::Types;
use crate::collect::mask::Mask;
use crate::config::{Config, Format, SessionSort, Shared};
use crate::failure::Failure;
use crate::filter::{Filter, Query, names_a_path, patterns};
use crate::record::{Cuts, Kind, RECORD_KEYS, SESSION_KEYS, STATS_KEYS};
use crate::redact::ExtraMasks;
use crate::render::digest::Limits;
use crate::render::json::{Fields, Run};
use crate::stats::{By, Grouping, Sort};
use crate::text::{Lang, Span};
use crate::when::DayRange;

/// The shared flags with the environment's defaults filled in, and what
/// they ask to be looked up.
struct Flags {
    /// The flags.
    common: Common,
    /// The git user name and email, looked up for `--mask git-user`.
    git_users: Vec<String>,
    /// The language `CCDIG_LANG` names, kept apart from `--lang`, which
    /// only the formats it applies to take.
    lang: Option<Lang>,
    /// The values `--mask-values` gave.
    values: Values,
}

/// Which of the flags that apply to tool results alone are given.
#[derive(Clone, Copy, Debug)]
struct ResultFlags {
    /// `--denial`.
    denial: bool,
    /// `--status`.
    status: bool,
}

/// Which of the flags for calls and results are given.
#[derive(Clone, Copy)]
struct CallFlags {
    /// `--tool`.
    tool: bool,
    /// `--touched`.
    touched: bool,
}

/// The kind-specific filters, empty for `sessions`.
#[derive(Default)]
struct Narrowing {
    /// `--around`: how many records around each match to keep.
    around: Option<usize>,
    /// `--context`: how many lines around each matched line to list.
    context: Option<usize>,
    /// How many lines around each matched line a search lists without
    /// `--context`: none, but the lines themselves, in JSON and JSON Lines.
    /// Markdown prints those lines in place of the whole text, so it lists
    /// them only when asked.
    context_by_default: Option<usize>,
    /// `--denial`.
    denials: Vec<String>,
    /// `--status`: whether to keep only failed results, only the others,
    /// or both.
    is_error: Option<bool>,
    /// `--kind`.
    kinds: Vec<String>,
    /// For `sessions`: the kinds of record whose matches a search counts.
    mentions: Option<Types>,
    /// `--skill`.
    skills: Vec<String>,
    /// Whether to read how each sub-agent was started, for `records`,
    /// whose records say it.
    spawns: bool,
    /// `--subagents`.
    subagents: bool,
    /// `--superseded`.
    superseded: Option<bool>,
    /// `--tool`.
    tools: Vec<String>,
    /// `--touched`.
    touched: Vec<String>,
    /// Whether to add up each session's tokens, for `sessions`.
    usage: bool,
}

/// What `ccdig sessions` was given, its shared flags merged.
struct SessionsFlags {
    /// The shared flags.
    common: Common,
    /// `--fields` and `--format`.
    output: Output,
    /// `--sort`.
    sort: SessionSort,
    /// `--subagents`.
    subagents: bool,
    /// `--type`.
    types: Vec<RecordType>,
}

/// What `ccdig stats` was given, its shared flags merged.
struct StatsFlags {
    /// `--skill` and `--subagents`.
    activity: Activity,
    /// `--by`.
    by: Vec<By>,
    /// The shared flags, given before and after the subcommand.
    common: Common,
    /// `--min-count`.
    min_count: Option<usize>,
    /// `--format` and `--fields`.
    output: Output,
    /// `--kind` and `--superseded`.
    prompted: Prompted,
    /// `--type`, `--tool`, `--status`, and `--denial`.
    selection: Selection,
    /// `--sort`.
    sort: Sort,
}

impl ResultFlags {
    /// Whether any of them is given.
    const fn any(self) -> bool {
        self.denial || self.status
    }
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

    /// What to read and how to report it, with the filters in `narrowing`,
    /// and the language of the words when `worded`, as in Markdown, HTML,
    /// and the digest. The checks run in the order the errors are reported.
    fn shared(
        self,
        narrowing: Narrowing,
        worded: bool,
        environment: &Environment,
    ) -> Result<Shared, Failure> {
        // JSON and JSON Lines get English whatever the variable says: the
        // notes of a cut are written into the records' text before they are
        // rendered, and the JSON values must not change with the language.
        let lang = match (self.common.lang, worded) {
            (Some(_), false) => return Err(Failure::LangNeedsWords),
            (Some(given), true) => given.into(),
            (None, true) => self.lang.unwrap_or_default(),
            (None, false) => Lang::default(),
        };
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
            lang: _env_lang,
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
            lang,
            limit: common.limit,
            mask,
            mask_report: common.mask_report.map(Into::into),
            on_skip: common.on_skip.map_or_default(Into::into),
            query: Query {
                branches: Filter::names(common.branches),
                denials: Filter::names(narrowing.denials),
                grep,
                is_error: narrowing.is_error,
                kinds: Filter::names(narrowing.kinds),
                projects: Filter::names(common.projects),
                range,
                sessions: Filter::names(sessions),
                skills: Filter::names(narrowing.skills),
                superseded: narrowing.superseded,
                tools: Filter::names(narrowing.tools),
                touched: Filter::names(narrowing.touched),
            },
            reading: Reading {
                around: narrowing.around,
                excerpt: narrowing
                    .context
                    .or_else(|| narrowing.context_by_default.filter(|_| finds)),
                lang,
                mentions: narrowing.mentions.filter(|_| finds),
                spawns: narrowing.spawns,
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
            common.on_skip = Some(env_value("CCDIG_ON_SKIP", value)?);
        }
        // As with CCDIG_ON_SKIP, the flag replaces the variable, so a bad
        // value is reported only when the variable is read.
        let lang = environment
            .lang
            .as_deref()
            .filter(|_| common.lang.is_none())
            .map(|value| env_value::<super::args::Lang>("CCDIG_LANG", value))
            .transpose()?
            .map(Into::into);
        let mut flags = Self {
            common,
            git_users: Vec::new(),
            lang,
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
/// applies to: `tool` says which of the flags for calls and results are
/// given, and `results` which of the flags for results alone are.
fn check_kinds(
    kinds: Types,
    activity: &Activity,
    prompted: &Prompted,
    tool: CallFlags,
    results: ResultFlags,
) -> Result<(), Failure> {
    let assistant = kinds.reads_assistant();
    for (given, applies, flag, needed) in [
        (
            tool.touched,
            kinds.calls || kinds.results,
            "--touched",
            "call or result",
        ),
        (
            !prompted.kinds.is_empty(),
            kinds.prompts,
            "--kind",
            "prompt",
        ),
        (
            prompted.superseded.is_some(),
            kinds.prompts,
            "--superseded",
            "prompt",
        ),
        (
            tool.tool,
            kinds.calls || kinds.results,
            "--tool",
            "call or result",
        ),
        (results.status, kinds.results, "--status", "result"),
        (results.denial, kinds.results, "--denial", "result"),
        (
            !activity.skills.is_empty(),
            assistant,
            "--skill",
            "reply, thinking, call, or result",
        ),
        (
            activity.subagents,
            assistant,
            "--subagents",
            "reply, thinking, call, or result",
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

/// Checks that each `--touched` value, a leading `!` aside, has a part to
/// match: `.` or `/` alone would match nothing.
fn check_touched(values: &[String]) -> Result<(), Failure> {
    values
        .iter()
        .find(|value| !names_a_path(value.strip_prefix('!').unwrap_or(value)))
        .map_or(Ok(()), |value| {
            Err(Failure::TouchedPath {
                value: value.clone(),
            })
        })
}

/// Checks that each `--kind` value, a leading `!` aside, is a kind of
/// prompt: a record type there is a mix-up with `--type`.
fn check_prompt_kinds(values: &[String]) -> Result<(), Failure> {
    for value in values {
        let name = value.strip_prefix('!').unwrap_or(value);
        if !Kind::ALL.iter().any(|kind| kind.name() == name) {
            let kinds: Vec<&str> = Kind::ALL.iter().map(|kind| kind.name()).collect();
            return Err(Failure::KindValue {
                kinds: listed(&kinds),
                types: value_names::<RecordType>(),
            });
        }
    }
    Ok(())
}

/// The kinds `selection`, `activity`, and `prompted` pick, after checking
/// that each kind-specific flag given applies to one of them, and the
/// filters they ask for.
fn picked(
    selection: Selection,
    activity: Activity,
    prompted: Prompted,
) -> Result<(Types, Narrowing), Failure> {
    let Selection {
        denials,
        statuses,
        tools,
        touched,
        types,
    } = selection;
    // A record type given to --kind is a mix-up with --type, whatever else
    // is given; so is a path with nothing to match.
    check_prompt_kinds(&prompted.kinds)?;
    check_touched(&touched)?;
    let results_only = ResultFlags {
        denial: !denials.is_empty(),
        status: !statuses.is_empty(),
    };
    // No other kind-specific flag applies to prompts, so one given with
    // --kind or --superseded is reported as needing its own --type.
    let kinds = if types.is_empty() && (!prompted.kinds.is_empty() || prompted.superseded.is_some())
    {
        Types::PROMPTS
    } else {
        let mut picks = record_types(
            &types,
            !tools.is_empty() || !touched.is_empty(),
            results_only.any(),
            !activity.skills.is_empty(),
        );
        // A result would list each call --touched picks a second time.
        if types.is_empty() && !touched.is_empty() && !results_only.any() {
            picks.results = false;
        }
        picks
    };
    check_kinds(
        kinds,
        &activity,
        &prompted,
        CallFlags {
            tool: !tools.is_empty(),
            touched: !touched.is_empty(),
        },
        results_only,
    )?;
    let narrowing = Narrowing {
        is_error: status_filter(&statuses),
        denials,
        kinds: prompted.kinds,
        superseded: prompted.superseded,
        tools,
        touched,
        ..activity.into()
    };
    Ok((kinds, narrowing))
}

/// The keys `--by` names, each once, in the order first given, after
/// checking that `sort` has tokens to sort by with them.
fn grouping_keys(given: Vec<By>, sort: Sort) -> Result<Vec<By>, Failure> {
    let mut keys: Vec<By> = Vec::new();
    for key in given {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    if let Some(tokens) = sort.tokens()
        && let Some(key) = keys.iter().find(|key| key.splits_responses())
    {
        return Err(Failure::SortWithoutUsage {
            by: key.name(),
            sort: tokens,
        });
    }
    Ok(keys)
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

/// Checks the keys `--fields` names against those the command writes,
/// `known`, and converts them.
fn fields(names: Vec<String>, known: &[&str]) -> Result<Fields, Failure> {
    if names.is_empty() {
        return Ok(Fields::default());
    }
    let mut picked = Filter::names(names);
    // A trailing comma, as in `text,`, names no key.
    picked.include.retain(|name| !name.is_empty());
    picked.exclude.retain(|name| !name.is_empty());
    if let Some(unknown) = picked
        .include
        .iter()
        .chain(&picked.exclude)
        .find(|name| !known.contains(&name.as_str()))
    {
        return Err(Failure::UnknownField {
            known: listed(known),
            name: unknown.clone(),
        });
    }
    Ok(Fields {
        exclude: picked.exclude,
        include: picked.include,
    })
}

/// The characters `flag` keeps: `N`, the first N, or `FROM-TO`, those
/// from FROM to TO, both counted from 1 as `cut -c` counts.
fn char_span(chars: Option<String>, flag: &'static str) -> Result<Option<Span>, Failure> {
    let Some(given) = chars else {
        return Ok(None);
    };
    let ends = match given.split_once('-') {
        Some((from, to)) => from.parse::<usize>().ok().zip(to.parse::<usize>().ok()),
        None => given.parse::<usize>().ok().map(|to| (1, to)),
    };
    match ends {
        Some((from, to)) if from >= 1 && to >= from => Ok(Some(Span {
            skip: from.saturating_sub(1),
            take: to.saturating_sub(from).saturating_add(1),
        })),
        Some(_) | None => Err(Failure::CharRange { flag, given }),
    }
}

/// The kinds `--type` picks. Without it: the kinds every one of `--tool`,
/// `--status` or `--denial` (`status`), and `--skill` given applies to, or
/// prompts and replies when none is.
fn record_types(types: &[RecordType], tool: bool, status: bool, skill: bool) -> Types {
    if types.is_empty() {
        if !(tool || status || skill) {
            return Types::CONVERSATION;
        }
        // Every one of them applies to results.
        return Types {
            calls: !status,
            prompts: false,
            replies: !(tool || status),
            results: true,
            summaries: false,
            thinking: false,
        };
    }
    Types {
        calls: types.contains(&RecordType::Call),
        prompts: types.contains(&RecordType::Prompt),
        replies: types.contains(&RecordType::Reply),
        results: types.contains(&RecordType::Result),
        summaries: types.contains(&RecordType::Summary),
        thinking: types.contains(&RecordType::Thinking),
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
            prompted,
        } => digest_config(
            earlier.merged(later),
            prompted,
            (max_prompt_chars, max_session_chars),
            environment,
        ),
        Command::Records {
            activity,
            around,
            common: later,
            context,
            message_chars,
            output,
            prompted,
            selection,
            tool_chars,
        } => {
            if around.is_some_and(|records| records > 0) && prompted.superseded.is_some() {
                return Err(Failure::SupersededWithAround);
            }
            let (kinds, picks) = picked(selection, activity, prompted)?;
            let format: Format = output.format.into();
            let fields = fields(output.fields, &RECORD_KEYS)?;
            let cuts = Cuts {
                message: char_span(message_chars, "--message-chars")?,
                tool: char_span(tool_chars, "--tool-chars")?,
            };
            let narrowing = Narrowing {
                around,
                context,
                context_by_default: (!format.is_read()).then_some(0),
                spawns: true,
                ..picks
            };
            let flags = Flags::with_env(earlier.merged(later), environment)?;
            Ok(Config::Records {
                cuts,
                fields,
                format,
                kinds,
                shared: Box::new(flags.shared(narrowing, format.is_read(), environment)?),
            })
        }
        Command::Sessions {
            common: later,
            output,
            sort,
            subagents,
            types,
        } => sessions_config(
            SessionsFlags {
                common: earlier.merged(later),
                output,
                sort: sort.into(),
                subagents,
                types,
            },
            environment,
        ),
        Command::Stats {
            activity,
            by,
            common: later,
            min_count,
            output,
            prompted,
            selection,
            sort,
        } => stats_config(
            StatsFlags {
                activity,
                by: by.into_iter().map(Into::into).collect(),
                common: earlier.merged(later),
                min_count,
                output,
                prompted,
                selection,
                sort: sort.into(),
            },
            environment,
        ),
        Command::Version {
            common: _ignored,
            format,
        } => Ok(Config::Version {
            format: format.into(),
        }),
    }
}

/// Resolves `digest`, with `given` the prompt and session limits, after
/// checking the `--kind` values.
fn digest_config(
    common: Common,
    prompted: Prompted,
    given: (i64, i64),
    environment: &Environment,
) -> Result<Config, Failure> {
    check_prompt_kinds(&prompted.kinds)?;
    let narrowing = Narrowing {
        kinds: prompted.kinds,
        superseded: prompted.superseded,
        ..Narrowing::default()
    };
    let flags = Flags::with_env(common, environment)?;
    let limits = digest_limits(given.0, given.1)?;
    Ok(Config::Digest {
        limits,
        shared: Box::new(flags.shared(narrowing, true, environment)?),
    })
}

/// Resolves `sessions`, after checking that the flags that change which
/// matches are counted have a pattern to count.
fn sessions_config(given: SessionsFlags, environment: &Environment) -> Result<Config, Failure> {
    let flags = Flags::with_env(given.common, environment)?;
    let narrowing = Narrowing {
        mentions: Some(record_types(&given.types, false, false, false)),
        subagents: given.subagents,
        usage: true,
        ..Narrowing::default()
    };
    let format: Format = given.output.format.into();
    let picked = fields(given.output.fields, &SESSION_KEYS)?;
    let shared = flags.shared(narrowing, format.is_read(), environment)?;
    let counts = shared.reading.mentions.is_some();
    for (flagged, flag) in [
        (!given.types.is_empty(), "--type"),
        (given.subagents, "--subagents"),
    ] {
        if flagged && !counts {
            return Err(Failure::SessionFlagNeedsGrep { flag });
        }
    }
    if given.sort == SessionSort::Matches && !counts {
        return Err(Failure::SortNeedsGrep);
    }
    Ok(Config::Sessions {
        fields: picked,
        format,
        shared: Box::new(shared),
        sort: given.sort,
    })
}

/// Resolves `stats`, with `--limit` taken out of the shared flags to count
/// groups, so every record is read.
fn stats_config(given: StatsFlags, environment: &Environment) -> Result<Config, Failure> {
    let subagents = given.activity.subagents;
    let (kinds, narrowing) = picked(given.selection, given.activity, given.prompted)?;
    let format: Format = given.output.format.into();
    let picked_fields = fields(given.output.fields, &STATS_KEYS)?;
    let keys = grouping_keys(given.by, given.sort)?;
    // Only a result has a denial and a status, so without one every
    // record would fall in the group whose value is null.
    for (key, flag) in [(By::Denial, "--by denial"), (By::Status, "--by status")] {
        if keys.contains(&key) && !kinds.results {
            return Err(Failure::TypeForFlag {
                flag,
                kinds: "result",
            });
        }
    }
    // Without the sub-agent transcripts, every record has no sub-agent.
    if keys.contains(&By::Subagent) && !subagents {
        return Err(Failure::ByNeedsSubagents);
    }
    let flags = Flags::with_env(given.common, environment)?;
    let mut shared = flags.shared(narrowing, format.is_read(), environment)?;
    let limit = shared.limit.take();
    Ok(Config::Stats {
        fields: picked_fields,
        format,
        grouping: Grouping {
            by: keys,
            empty: kinds.thinking,
            limit,
            min_count: given.min_count.unwrap_or_default(),
            sort: given.sort,
        },
        kinds,
        shared: Box::new(shared),
    })
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
