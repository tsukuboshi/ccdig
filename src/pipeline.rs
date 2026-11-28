//! The stages each subcommand runs, joined: collect the records, render
//! them, and add up what masking counted. Everything comes in a `Config`,
//! and the output goes back as text for the caller to write.

use core::cmp::Reverse;

use serde::Serialize;

use crate::classify::human::FILTER_VERSION;
use crate::collect::kinds::{Prompts, Records, Types};
use crate::collect::{Corpus, Pick, Select as _, collect};
use crate::config::{Config, Format, MaskReport, OnSkip, SessionSort, Shared, VersionFormat};
use crate::failure::Failure;
use crate::record::{Cuts, Group, Record, Session};
use crate::redact::Counts;
use crate::render::digest::digest;
use crate::render::html::{self, Masking};
use crate::render::json::{Fields, Run, document, lines, version};
use crate::render::markdown;
use crate::stats::{Grouping, columns, tally};

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

/// How a subcommand writes its records in the formats a person reads.
struct Writers<H, M> {
    /// The page.
    html: H,
    /// The Markdown.
    markdown: M,
}

/// Runs the subcommand in `config` and returns its output.
///
/// # Errors
///
/// When the root cannot be listed or the output cannot be rendered.
pub fn execute(config: Config) -> Result<Printed, Failure> {
    match config {
        Config::Digest { limits, shared } => {
            let corpus = gather(&shared, SessionSort::Time, Prompts::with_replies)?;
            printed(
                &shared,
                digest(&corpus.records, &shared.zone, limits, shared.lang)?,
                corpus.warnings,
                corpus.records.iter().map(Record::masked),
            )
        }
        Config::Records {
            cuts,
            fields,
            format,
            kinds,
            shared,
        } => records(cuts, &fields, format, kinds, &shared),
        Config::Sessions {
            fields,
            format,
            shared,
            sort,
        } => sessions(&fields, format, &shared, sort),
        Config::Stats {
            fields,
            format,
            grouping,
            kinds,
            shared,
        } => stats(&fields, format, &grouping, kinds, &shared),
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

/// Runs `sessions`: lists the sessions in the order `sort` asks for, and
/// writes the keys `fields` keeps in `format`.
fn sessions(
    fields: &Fields,
    format: Format,
    shared: &Shared,
    sort: SessionSort,
) -> Result<Printed, Failure> {
    let corpus = gather(shared, sort, Prompts::default)?;
    let matches = shared.reading.mentions.is_some();
    let out = write(
        format,
        fields,
        &corpus.sessions,
        &shared.run,
        Writers {
            html: |sessions: &[Session]| {
                let masked = sessions.iter().map(|session| session.masked.clone());
                html::sessions(
                    sessions,
                    &shared.zone,
                    matches,
                    fields,
                    shared.lang,
                    &masking(shared, masked),
                )
            },
            markdown: |sessions: &[Session]| {
                markdown::sessions(sessions, &shared.zone, matches, fields, shared.lang)
            },
        },
    )?;
    printed(
        shared,
        out,
        corpus.warnings,
        corpus.sessions.iter().map(|session| session.masked.clone()),
    )
}

/// Runs `stats`: counts the records of `kinds` in the groups `grouping`
/// makes, and writes the keys `fields` keeps in `format`.
fn stats(
    fields: &Fields,
    format: Format,
    grouping: &Grouping,
    kinds: Types,
    shared: &Shared,
) -> Result<Printed, Failure> {
    let corpus = gather(shared, SessionSort::Time, || {
        Records::counting_empty_thinking(kinds)
    })?;
    let groups = tally(&corpus.records, grouping, &shared.zone)?;
    let keys = columns(&grouping.by);
    let out = write(
        format,
        fields,
        &groups,
        &shared.run,
        Writers {
            html: |lines: &[Group]| {
                html::stats(
                    lines,
                    &keys,
                    &shared.zone,
                    fields,
                    shared.lang,
                    &masking(shared, corpus.records.iter().map(Record::masked)),
                )
            },
            markdown: |lines: &[Group]| {
                markdown::stats(lines, &keys, &shared.zone, fields, shared.lang)
            },
        },
    )?;
    printed(
        shared,
        out,
        corpus.warnings,
        corpus.records.iter().map(Record::masked),
    )
}

/// Runs `records`: picks the records of `kinds`, cuts their texts to
/// `cuts`, and writes the parts `fields` keeps in `format`.
fn records(
    cuts: Cuts,
    fields: &Fields,
    format: Format,
    kinds: Types,
    shared: &Shared,
) -> Result<Printed, Failure> {
    let mut corpus = gather(shared, SessionSort::Time, || Records::with_results(kinds))?;
    for record in &mut corpus.records {
        // Before the cut, which would leave the JSON unreadable.
        if format.is_read() {
            shared.mask.indent_json(record);
        }
        record.cut(cuts, shared.lang);
    }
    let out = write(
        format,
        fields,
        &corpus.records,
        &shared.run,
        Writers {
            html: |records: &[_]| {
                let masked = records.iter().map(Record::masked);
                html::records(
                    records,
                    &shared.zone,
                    fields,
                    shared.lang,
                    &masking(shared, masked),
                )
            },
            markdown: |records: &[_]| markdown::records(records, &shared.zone, fields, shared.lang),
        },
    )?;
    printed(
        shared,
        out,
        corpus.warnings,
        corpus.records.iter().map(Record::masked),
    )
}

/// Reads every transcript into records of the kind `new_picker` picks, with
/// the sessions in the order `sort` asks for, keeping at most `--limit` of
/// them, with their times in the zone the day range was read in.
fn gather<P, F>(
    shared: &Shared,
    sort: SessionSort,
    new_picker: F,
) -> Result<Corpus<P::Record>, Failure>
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
    if sort == SessionSort::Matches {
        // A stable sort keeps sessions with as many matches in time order.
        corpus
            .sessions
            .sort_by_key(|session| Reverse(session.match_count));
    }
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
        if let Some(ref mut first) = session.first_match {
            first.timestamp = first.timestamp.shown_in(&shared.zone);
        }
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

/// How the records were masked, as the top of a page says it: off with
/// `--no-mask`, or else what masking counted in all of `masked`, most
/// first, which the `--mask-report` line adds up the same way.
fn masking(shared: &Shared, masked: impl Iterator<Item = Counts>) -> Masking {
    if shared.run.no_mask {
        Masking::Off
    } else {
        Masking::On(total(masked).ranked())
    }
}

/// What masking counted in all of `masked`, added up.
fn total(masked: impl Iterator<Item = Counts>) -> Counts {
    let mut total = Counts::default();
    for counts in masked {
        total.merge(&counts);
    }
    total
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
        Some(format) => Some(mask_report(format, &total(masked))?),
        None => None,
    };
    Ok(Printed {
        on_skip: shared.on_skip,
        out,
        report,
        warnings,
    })
}

/// Writes `records` in `format`, with the keys `fields` keeps, using
/// `writers` for HTML and Markdown and `run` in the JSON envelope.
fn write<T, H, M>(
    format: Format,
    fields: &Fields,
    records: &[T],
    run: &Run,
    writers: Writers<H, M>,
) -> Result<String, Failure>
where
    T: Serialize,
    H: FnOnce(&[T]) -> Result<String, Failure>,
    M: FnOnce(&[T]) -> Result<String, Failure>,
{
    match format {
        Format::Html => (writers.html)(records),
        Format::Json => document(FILTER_VERSION, VERSION, run, records, fields),
        Format::Jsonl => lines(records, fields),
        Format::Markdown => (writers.markdown)(records),
    }
}
