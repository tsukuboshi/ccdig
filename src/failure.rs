//! The errors ccdig reports, each with a prefix naming what failed.

use std::io;

/// An error that stops a command. The message is printed after `ccdig: `.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    /// `--around` is given without a pattern for it to find.
    #[error("--around needs --grep or --igrep with a pattern that does not start with !")]
    AroundNeedsGrep,
    /// `stats --by subagent` is given without `--subagents`, so no record
    /// has a sub-agent.
    #[error("--by subagent needs --subagents")]
    ByNeedsSubagents,
    /// `--message-chars` or `--tool-chars` is neither a positive count nor
    /// a range counted from 1.
    #[error(
        "{flag} takes a count of characters such as 2000, or a range such as 2001-4000 counted from 1: {given:?}"
    )]
    CharRange {
        /// The flag.
        flag: &'static str,
        /// What it was given.
        given: String,
    },
    /// `--context` is given without a pattern for it to find.
    #[error("--context needs --grep or --igrep with a pattern that does not start with !")]
    ContextNeedsGrep,
    /// The digest limits are not both positive.
    #[error("--max-prompt-chars and --max-session-chars must be positive")]
    DigestLimitNotPositive,
    /// The prompt limit leaves no room in the session limit. A cut prompt
    /// is the limit plus an ellipsis, so a limit that is not below the
    /// session limit lets one long prompt use the whole budget.
    #[error("--max-prompt-chars must be less than --max-session-chars")]
    DigestPromptLimitTooLarge,
    /// `CCDIG_MASK` names a mask `--mask` does not know.
    #[error("CCDIG_MASK: unknown mask {name:?}: use {known}")]
    EnvMask {
        /// The masks `--mask` takes, listed.
        known: String,
        /// The unknown name.
        name: String,
    },
    /// A variable holds a value its flag does not take.
    #[error("{variable}: unknown value {name:?}: use {known}")]
    EnvValue {
        /// The values the flag takes, listed.
        known: String,
        /// The unknown value.
        name: String,
        /// The variable.
        variable: &'static str,
    },
    /// `--grep` is not a valid regular expression.
    #[error("parse grep: {0}")]
    Grep(#[source] regex::Error),
    /// `--root` is not given and there is no home directory to default to.
    #[error("find the default --root: HOME is not set")]
    Home,
    /// A record cannot be encoded as JSON.
    #[error("write json: {0}")]
    Json(#[source] serde_json::Error),
    /// `--kind` names something other than a kind of prompt, as a record
    /// type.
    #[error("--kind takes {kinds}; pick the record types with --type: {types}")]
    KindValue {
        /// The kinds of prompt.
        kinds: String,
        /// The record types.
        types: String,
    },
    /// The history root cannot be listed.
    #[error("list projects: {0}")]
    ListProjects(#[source] io::Error),
    /// The file after `@` in `--mask-values` cannot be read.
    #[error("read mask values file {path}: {source}")]
    MaskFile {
        /// The file given to `--mask-values`.
        path: String,
        /// Why it could not be read.
        #[source]
        source: io::Error,
    },
    /// `--mask git-user` is given and git config has no `key`.
    #[error("mask git-user: git config has no {key}")]
    MaskGitUser {
        /// `user.name` or `user.email`.
        key: &'static str,
    },
    /// `--mask user` is given and `$HOME` names no user.
    #[error("mask user: HOME is not set or has no last part")]
    MaskUser,
    /// `--mask-values` is not a JSON object of labels to lists of values.
    #[error("{given}: {reason}")]
    MaskValues {
        /// `--mask-values`, with the file when one was given.
        given: String,
        /// What is wrong.
        reason: String,
    },
    /// `--no-mask` is given with `--mask` or `--mask-values`.
    #[error("--no-mask cannot be used with --mask or --mask-values")]
    MaskWithNoMask,
    /// `--session current` is given outside Claude Code, where no variable
    /// names the session ccdig runs in.
    #[error(
        "--session current: CLAUDE_CODE_SESSION_ID is not set; run ccdig inside Claude Code or give a session ID"
    )]
    NoCurrentSession,
    /// The masking rules do not compile, which is a bug.
    #[error("compile masking rules: {0}")]
    Rules(#[source] regex::Error),
    /// A flag of `sessions` that changes which matches are counted is given
    /// without a pattern to count.
    #[error("sessions {flag} needs --grep or --igrep with a pattern that does not start with !")]
    SessionFlagNeedsGrep {
        /// The flag.
        flag: &'static str,
    },
    /// `--since` is not a calendar day.
    #[error("parse since: {0}")]
    Since(String),
    /// `--since` and `--until` describe an empty range.
    #[error("since is after until")]
    SinceAfterUntil,
    /// `sessions --sort matches` is given without a pattern to count.
    #[error("--sort matches needs --grep or --igrep with a pattern that does not start with !")]
    SortNeedsGrep,
    /// `stats --sort` orders by tokens that a `--by` key leaves `null`.
    #[error("--sort {sort} cannot be used with --by {by}, which leaves usage null")]
    SortWithoutUsage {
        /// The `--by` key.
        by: &'static str,
        /// The `--sort` order.
        sort: &'static str,
    },
    /// `--superseded` is given with `--around`, which keeps the records
    /// next to a prompt before its file tells whether it was sent again.
    #[error("--superseded cannot be used with --around")]
    SupersededWithAround,
    /// A time cannot be formatted, such as one outside the supported years.
    #[error("format time: {0}")]
    TimeFormat(#[source] jiff::Error),
    /// A `--touched` value has no part to match, as `.` or `/`.
    #[error("--touched needs a path with a part other than / and ., not {value:?}")]
    TouchedPath {
        /// The value given.
        value: String,
    },
    /// A kind-specific flag is given without a `--type` it applies to.
    #[error("{flag} needs --type {kinds}")]
    TypeForFlag {
        /// The flag.
        flag: &'static str,
        /// The kinds it applies to.
        kinds: &'static str,
    },
    /// `--fields` names a key the command does not write.
    #[error("--fields: unknown key {name:?}: use {known}")]
    UnknownField {
        /// The keys the command writes, listed.
        known: String,
        /// The unknown key.
        name: String,
    },
    /// `--tz` names no known time zone.
    #[error("load time zone: unknown time zone {name}")]
    UnknownZone {
        /// The name given to `--tz`.
        name: String,
        /// Why the time zone database did not have it.
        #[source]
        source: jiff::Error,
    },
    /// `--until` is not a calendar day.
    #[error("parse until: {0}")]
    Until(String),
}
