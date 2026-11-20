//! The errors ccdig reports, each with the prefix promptdig used.

/// An error that stops a command. The message is printed after `ccdig: `.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    /// `--grep` is not a valid regular expression.
    #[error("parse grep: {0}")]
    Grep(#[source] regex::Error),
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
