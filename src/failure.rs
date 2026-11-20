//! The errors ccdig reports, each with a prefix naming what failed.

use std::io;

/// An error that stops a command. The message is printed after `ccdig: `.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    /// The digest limits are not both positive.
    #[error("--max-prompt-chars and --max-session-chars must be positive")]
    DigestLimitNotPositive,
    /// The prompt limit leaves no room in the session limit. A cut prompt
    /// is the limit plus an ellipsis, so a limit that is not below the
    /// session limit lets one long prompt use the whole budget.
    #[error("--max-prompt-chars must be less than --max-session-chars")]
    DigestPromptLimitTooLarge,
    /// `--format` is not one of the formats.
    #[error("unknown --format {0:?}: use jsonl, json, or md")]
    Format(String),
    /// `--grep` is not a valid regular expression.
    #[error("parse grep: {0}")]
    Grep(#[source] regex::Error),
    /// `--root` is not given and there is no home directory to default to.
    #[error("find the default --root: HOME is not set")]
    Home,
    /// A record cannot be encoded as JSON.
    #[error("write json: {0}")]
    Json(#[source] serde_json::Error),
    /// The history root cannot be listed.
    #[error("list projects: {0}")]
    ListProjects(#[source] io::Error),
    /// The `--mask-file` cannot be read.
    #[error("read mask file {path}: {source}")]
    MaskFile {
        /// The file given to `--mask-file`.
        path: String,
        /// Why it could not be read.
        #[source]
        source: io::Error,
    },
    /// `--mask home` is given and there is no home directory to mask.
    #[error("mask home: HOME is not set")]
    MaskHome,
    /// The masking rules do not compile, which is a bug.
    #[error("compile masking rules: {0}")]
    Rules(#[source] regex::Error),
    /// `--since` is not a calendar day.
    #[error("parse since: {0}")]
    Since(String),
    /// `--since` and `--until` describe an empty range.
    #[error("since is after until")]
    SinceAfterUntil,
    /// A time cannot be formatted, such as one outside the supported years.
    #[error("format time: {0}")]
    TimeFormat(#[source] jiff::Error),
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
