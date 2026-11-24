//! The stages each subcommand runs, joined: collect the records, render
//! them, and add up what masking counted. Everything comes in a `Config`,
//! and the output goes back as text for the caller to write.

use serde::Serialize;

use crate::classify::human::FILTER_VERSION;
use crate::collect::kinds::{Prompts, Records};
use crate::collect::{Corpus, Pick, Select as _, collect};
use crate::config::{Config, Format, MaskReport, OnSkip, Shared, VersionFormat};
use crate::failure::Failure;
use crate::record::Record;
use crate::redact::Counts;
use crate::render::digest::digest;
use crate::render::json::{Run, document, lines, version};
use crate::render::markdown;

/// The version of this build.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a subcommand produced.
pub struct Printed {
    /// What to do when there are warnings.
    pub on_skip: OnSkip,
    /// What goes to stdout.
    pub out: String,
    /// The `--mask-report` line for stderr.
    pub report: Option<String>,
    /// The warning lines for stderr.
    pub warnings: Vec<String>,
}

/// Runs the subcommand in `config` and returns its output.
///
/// # Errors
///
/// When the root cannot be listed or the output cannot be rendered.
pub fn execute(config: Config) -> Result<Printed, Failure> {
    match config {
        Config::Digest { limits, shared } => {
            let corpus = gather(&shared, || Prompts)?;
            printed(
                &shared,
                digest(&corpus.records, &shared.zone, limits)?,
                corpus.warnings,
                corpus.records.iter().map(Record::masked),
            )
        }
        Config::Records {
            cuts,
            format,
            kinds,
            shared,
        } => {
            let mut corpus = gather(&shared, || Records::new(kinds))?;
            for record in &mut corpus.records {
                record.cut(cuts);
            }
            let out = write(format, &corpus.records, &shared.run, |records| {
                markdown::records(records, &shared.zone)
            })?;
            printed(
                &shared,
                out,
                corpus.warnings,
                corpus.records.iter().map(Record::masked),
            )
        }
        Config::Sessions { format, shared } => {
            let corpus = gather(&shared, || Prompts)?;
            let out = write(format, &corpus.sessions, &shared.run, |sessions| {
                markdown::sessions(sessions, &shared.zone, shared.reading.mentions)
            })?;
            printed(
                &shared,
                out,
                corpus.warnings,
                corpus.sessions.iter().map(|session| session.masked.clone()),
            )
        }
        Config::Version { format } => Ok(Printed {
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

/// Reads every transcript into records of the kind `new_picker` picks,
/// keeping at most `--limit` of them, with their times in the zone the day
/// range was read in.
fn gather<P, F>(shared: &Shared, new_picker: F) -> Result<Corpus<P::Record>, Failure>
where
    P: Pick,
    F: Fn() -> P,
{
    let mut corpus = collect(
        &shared.root,
        shared.reading,
        &shared.query,
        &shared.mask,
        new_picker,
    )?;
    if let Some(limit) = shared.limit {
        let around = shared.reading.around.unwrap_or_default();
        corpus
            .records
            .truncate(limit_end(&corpus.records, limit, around));
        corpus.sessions.truncate(limit);
    }
    // Sorting and joining compared instants; only the offset written
    // changes.
    for record in &mut corpus.records {
        let head = record.head_mut();
        head.timestamp = head.timestamp.shown_in(&shared.zone);
    }
    for session in &mut corpus.sessions {
        session.mtime = session.mtime.shown_in(&shared.zone);
        session.started_at = session.started_at.shown_in(&shared.zone);
        session.ended_at = session.ended_at.shown_in(&shared.zone);
    }
    Ok(corpus)
}

/// How many of `records` `--limit` keeps: those up to the `limit`-th that
/// matched, and after it, up to `around` records of context from its file,
/// which `--around` added after it. Without `--around` every record
/// matched, so this is `limit`.
fn limit_end<T>(records: &[T], limit: usize, around: usize) -> usize
where
    T: Record,
{
    if limit == 0 {
        return 0;
    }
    let mut seen = 0_usize;
    for (index, record) in records.iter().enumerate() {
        if record.matched() {
            seen = seen.saturating_add(1);
            if seen == limit {
                return context_end(records, index, record, around);
            }
        }
    }
    records.len()
}

/// Where the context after `matched`, at `index`, ends: after up to
/// `around` records that did not match, from the same file.
fn context_end<T>(records: &[T], index: usize, matched: &T, around: usize) -> usize
where
    T: Record,
{
    let following = records
        .iter()
        .skip(index.saturating_add(1))
        .take(around)
        .take_while(|record| !record.matched() && record.source().file == matched.source().file)
        .count();
    index.saturating_add(1).saturating_add(following)
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

/// What a command printed: `out`, the `warnings`, and with
/// `--mask-report`, the sum of what masking counted in each record.
fn printed(
    shared: &Shared,
    out: String,
    warnings: Vec<String>,
    masked: impl Iterator<Item = Counts>,
) -> Result<Printed, Failure> {
    let report = match shared.mask_report {
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
        on_skip: shared.on_skip,
        out,
        report,
        warnings,
    })
}

/// Writes `records` in `format`, using `markdown` for Markdown and `run` in
/// the JSON envelope.
fn write<T>(
    format: Format,
    records: &[T],
    run: &Run,
    markdown: impl FnOnce(&[T]) -> Result<String, Failure>,
) -> Result<String, Failure>
where
    T: Serialize,
{
    match format {
        Format::Json => document(FILTER_VERSION, VERSION, run, records),
        Format::Jsonl => lines(records),
        Format::Markdown => markdown(records),
    }
}
