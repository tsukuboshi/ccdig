//! What a run does, resolved from the command line and the environment:
//! plain data that holds neither clap's types nor anything read later.

use std::path::PathBuf;

use jiff::tz::TimeZone;

use crate::collect::Reading;
use crate::collect::kinds::Types;
use crate::collect::mask::Mask;
use crate::filter::Query;
use crate::record::Cuts;
use crate::render::digest::Limits;
use crate::render::json::{Fields, Run};

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
        /// How many characters to keep of each text.
        cuts: Cuts,
        /// The keys to write in JSON and JSON Lines.
        fields: Fields,
        /// The output format.
        format: Format,
        /// The kinds of record to pick.
        kinds: Types,
        /// What to read and how to report it.
        shared: Box<Shared>,
    },
    /// `sessions`.
    Sessions {
        /// The keys to write in JSON and JSON Lines.
        fields: Fields,
        /// The output format.
        format: Format,
        /// What to read and how to report it.
        shared: Box<Shared>,
        /// The order of the sessions.
        sort: SessionSort,
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

/// The order `sessions --sort` asks for.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SessionSort {
    /// Most matches first, then by time.
    Matches,
    /// By the time of the first prompt.
    #[default]
    Time,
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
    /// How to read the transcripts.
    pub reading: Reading,
    /// The transcript root.
    pub root: PathBuf,
    /// The flags that decide what the records hold, for the JSON envelope.
    pub run: Run,
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
