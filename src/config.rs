//! What a run does, resolved from the command line and the environment:
//! plain data that holds neither clap's types nor anything read later.

use std::path::PathBuf;

use jiff::tz::TimeZone;

use crate::collect::kinds::Types;
use crate::collect::mask::Mask;
use crate::render::digest::Limits;
use crate::render::json::Run;
use crate::select::Query;

/// A subcommand with everything it needs.
pub enum Config {
    /// `digest`.
    Digest {
        /// The size limits.
        limits: Limits,
        /// What to read and how to report it.
        shared: Box<Shared>,
    },
    /// `records`.
    Records {
        /// The output format.
        format: Format,
        /// In Markdown, how many characters to keep of each string in a
        /// tool call's input, or `None` for all of them.
        input_chars: Option<usize>,
        /// The kinds of record to pick.
        kinds: Types,
        /// What to read and how to report it.
        shared: Box<Shared>,
    },
    /// `sessions`.
    Sessions {
        /// The output format.
        format: Format,
        /// What to read and how to report it.
        shared: Box<Shared>,
    },
    /// `version`.
    Version {
        /// The output format.
        format: VersionFormat,
    },
}

/// The output formats of `records` and `sessions`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// One indented JSON document.
    Json,
    /// One JSON object per line.
    Jsonl,
    /// Markdown for reading.
    Markdown,
}

/// How `--mask-report` writes the counts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaskReport {
    /// `{"masked":{"rule":count,...}}` on one line.
    Json,
    /// `ccdig: masked N by rule, ...`.
    Text,
}

/// What to do when a file or line cannot be read.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OnSkip {
    /// Warn, write the output, and end with status 1.
    Fail,
    /// Warn and go on.
    #[default]
    Warn,
}

/// What the subcommands that read transcripts share: where to read, what
/// to keep, how to mask it, and how to report the run.
pub struct Shared {
    /// How many records to print at most, or `None` for all of them.
    pub limit: Option<usize>,
    /// The masking to apply.
    pub mask: Mask,
    /// How to report what masking counted, or `None` not to.
    pub mask_report: Option<MaskReport>,
    /// What to do when input is skipped.
    pub on_skip: OnSkip,
    /// The filters.
    pub query: Query,
    /// The transcript root.
    pub root: PathBuf,
    /// The flags that decide what the records hold, for the JSON envelope.
    pub run: Run,
    /// Whether to read sub-agent transcripts too.
    pub subagents: bool,
    /// Whether to add up each session's tokens.
    pub usage: bool,
    /// The zone for days and Markdown times.
    pub zone: TimeZone,
}

/// The output formats of `version`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersionFormat {
    /// The version, the schema version, and the filter as one JSON object.
    Json,
    /// `ccdig v` and the version.
    Text,
}
