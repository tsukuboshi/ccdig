//! Reading one transcript: the lines a picker wants become records with
//! their text masked, the filters see them, and the records they keep are
//! sealed with the file's title once the whole file is read.

use alloc::borrow::Cow;
use alloc::collections::VecDeque;
use core::mem;
use std::collections::HashSet;

use super::excerpt::excerpt;
use super::kinds::Replies;
use super::mask::{Draft, Mask, Title};
use super::{Pick, Reading, Select, Spot};
use regex::Regex;

use crate::classify::human::{Reason, classify};
use crate::filter::{Facts, Filter, Query, Verdict};
use crate::record::{FIRST_MATCH_CHARS, FirstMatch};
use crate::redact::Counts;
use crate::text::{cut_text, one_line};
use crate::transcript::entry::{Entry, Role, Usage};
use crate::transcript::reader::{self, MAX_LINE_BYTES, Roles};
use crate::transcript::walk::SessionFile;
use crate::when::Stamp;

/// What a transcript file yielded.
pub struct Read<T> {
    /// The time of the last prompt, reply, tool call, or tool result read
    /// in the range, whether the filters kept it or not.
    pub last: Option<Stamp>,
    /// With `Reading::mentions`, the prompts and replies the including
    /// patterns matched, in file order.
    pub mentions: Option<Vec<Mention>>,
    /// The selected records, in file order, each with its block's index in
    /// the line.
    pub records: Vec<(usize, T)>,
    /// The lines that were skipped.
    pub skipped: Skipped,
    /// The session title, masked; empty when the file has none.
    pub title: String,
    /// What masking the title counted.
    pub title_masked: Counts,
    /// The tokens of each response in the range, by response ID.
    pub usage: Vec<(String, Usage)>,
}

/// A prompt or reply the including patterns matched, for `sessions`. Only
/// the first one a session counts is shown, so the line it matched is
/// found when `first_match` asks for it, not for every match.
pub struct Mention {
    /// The texts a search reads, masked, each named for its field.
    fields: Vec<(String, String)>,
    /// Its line ID and block, which a resumed copy repeats.
    key: (String, usize),
    /// `prompt` or `reply`.
    kind: &'static str,
    /// What masking its text counted.
    masked: Counts,
    /// When it was written.
    timestamp: Stamp,
}

impl Mention {
    /// What it shows of the match: the first line a pattern of `grep`
    /// matched, or the start of its text when no single line did, on one
    /// line and cut to `FIRST_MATCH_CHARS` characters.
    pub fn first_match(&self, grep: &Filter<Regex>) -> FirstMatch {
        let fields: Vec<(String, Cow<'_, str>)> = self
            .fields
            .iter()
            .map(|named| (named.0.clone(), Cow::Borrowed(named.1.as_str())))
            .collect();
        let (line, text) = excerpt(&fields, grep, 0)
            .into_iter()
            .find(|listed| listed.hit)
            .map_or_else(
                || {
                    (
                        None,
                        self.fields
                            .first()
                            .map_or_else(String::new, |named| named.1.clone()),
                    )
                },
                |listed| (Some(listed.line), listed.text),
            );
        FirstMatch {
            kind: self.kind,
            timestamp: self.timestamp,
            line,
            text: cut_text(&one_line(&text), FIRST_MATCH_CHARS),
        }
    }

    /// Its line ID and block, which a resumed copy repeats.
    pub const fn key(&self) -> &(String, usize) {
        &self.key
    }

    /// What masking its text counted.
    pub const fn masked(&self) -> &Counts {
        &self.masked
    }

    /// The mention of `record`, its block `block`, with its masked text.
    fn of<T>(record: &T, block: usize) -> Self
    where
        T: Select,
    {
        Self {
            fields: record
                .grep_fields()
                .into_iter()
                .map(|(field, text)| (field, text.into_owned()))
                .collect(),
            key: (record.source().uuid.clone(), block),
            kind: record.kind(),
            masked: record.masked(),
            timestamp: record.head().timestamp,
        }
    }
}

/// The records kept from a file so far, in file order, and the responses
/// whose tokens one of them carries.
struct Kept<T> {
    /// The records, each with its block's index in the line.
    drafts: Vec<(usize, Draft<T>)>,
    /// The keys of the responses whose tokens a kept record carries.
    priced: HashSet<String>,
}

/// The tokens of the response a line is part of, and what tells that
/// response from the others in the file.
#[derive(Clone)]
struct Response {
    /// The response's ID, or for a line without one, which is a response
    /// of its own, the line's number after a NUL, which no ID has.
    key: String,
    /// The tokens.
    tokens: Usage,
}

impl Response {
    /// The response the line `entry`, at `line`, is part of, or `None`
    /// when the line has no tokens.
    fn of(entry: &Entry, line: usize) -> Option<Self> {
        entry.usage.map(|tokens| Self {
            key: if entry.message_id.is_empty() {
                format!("\0{line}")
            } else {
                entry.message_id.clone()
            },
            tokens,
        })
    }
}

/// With `--around`, the records waiting to be kept as context before the
/// next match, and how many more to keep after the last one.
struct Window<T> {
    /// How many more records to keep after the last match.
    after: usize,
    /// How many records to keep on each side of a match.
    around: usize,
    /// Up to `around` records since the last one kept, the latest last.
    before: VecDeque<(usize, Draft<T>, Option<Response>)>,
}

/// How a file's records are kept once the filters have seen them.
enum Keeper<T> {
    /// For `sessions --grep`: every record the other filters let through,
    /// and each the including patterns matched.
    Tally(Vec<Mention>),
    /// Each match, and with `--around`, the records next to it.
    Window(Window<T>),
}

/// Lines skipped across files, for the summary warning.
#[derive(Clone, Copy, Debug, Default)]
pub struct Skipped {
    /// Lines over the limit.
    pub oversized: usize,
    /// Lines that did not decode.
    pub undecodable: usize,
    /// Wanted entries without a timestamp, which cannot be placed in a
    /// range or ordered.
    pub untimed: usize,
}

impl Skipped {
    /// Adds `other` to these counts.
    pub const fn add(&mut self, other: Self) {
        self.oversized = self.oversized.saturating_add(other.oversized);
        self.untimed = self.untimed.saturating_add(other.untimed);
        self.undecodable = self.undecodable.saturating_add(other.undecodable);
    }

    /// Whether nothing was skipped.
    pub const fn is_empty(&self) -> bool {
        self.oversized == 0 && self.untimed == 0 && self.undecodable == 0
    }
}

/// The session title as a file is read: the last one seen, and the same
/// masked for the records of the lines after it.
#[derive(Default)]
struct Titles {
    /// The last title seen, as written.
    last: String,
    /// `last` masked, or empty in a sub-agent's transcript, whose records
    /// take their session's title.
    then: String,
    /// What masking `then` counted.
    then_masked: Counts,
}

impl Titles {
    /// Notes the title of an `ai-title` line. The title is regenerated as a
    /// session goes on and the last wins; Claude Code writes the same title
    /// again and again, so it is masked only when it changes, and only in
    /// the session's `own` transcript.
    fn see(&mut self, title: String, own: bool, mask: &Mask) {
        if title.is_empty() || title == self.last {
            return;
        }
        self.last = title;
        if own {
            self.then_masked = Counts::default();
            self.then = mask.text(&self.last, &mut self.then_masked).0;
        }
    }

    /// Where line `line` of `file` is, written at `timestamp`, with the
    /// title then.
    fn spot<'file>(
        &'file self,
        file: &'file SessionFile,
        line: usize,
        mask: &'file Mask,
        timestamp: Stamp,
    ) -> Spot<'file> {
        Spot {
            file,
            line,
            mask,
            timestamp,
            title: &self.then,
            title_masked: &self.then_masked,
        }
    }
}

impl<T> Kept<T>
where
    T: Select,
{
    /// Keeps `draft` at `at` in file order, the end when `None`. A
    /// response's lines repeat its tokens, so only the first record kept
    /// from it that carries tokens gets them; a match is kept before the
    /// context `--around` puts ahead of it, so it gets them first.
    fn keep(
        &mut self,
        at: Option<usize>,
        (block, mut draft): (usize, Draft<T>),
        response: Option<&Response>,
    ) {
        if let Some(of) = response
            && let Some(slot) = draft.usage_mut()
            && self.priced.insert(of.key.clone())
        {
            *slot = Some(of.tokens);
        }
        match at {
            Some(index) => self.drafts.insert(index, (block, draft)),
            None => self.drafts.push((block, draft)),
        }
    }
}

impl<T> Keeper<T>
where
    T: Select,
{
    /// What tallied the matches, or `None` for a window.
    fn mentions(self) -> Option<Vec<Mention>> {
        match self {
            Self::Tally(found) => Some(found),
            Self::Window(_) => None,
        }
    }

    /// The keeper `reading` asks for.
    fn new(reading: Reading) -> Self {
        if reading.mentions {
            return Self::Tally(Vec::new());
        }
        Self::Window(Window {
            after: 0,
            around: reading.around.unwrap_or_default(),
            before: VecDeque::new(),
        })
    }

    /// Keeps `drafted`, of `verdict`, in `kept` or for later, as the
    /// keeper does.
    fn offer(
        &mut self,
        kept: &mut Kept<T>,
        drafted: (usize, Draft<T>),
        verdict: Verdict,
        response: Option<&Response>,
        query: &Query,
        lines: Option<usize>,
    ) {
        match *self {
            Self::Tally(ref mut found) => {
                if verdict == Verdict::Hit {
                    found.push(Mention::of(drafted.1.record(), drafted.0));
                }
                if verdict != Verdict::Out {
                    kept.keep(None, drafted, response);
                }
            }
            Self::Window(ref mut window) => {
                window.offer(kept, drafted, verdict, response, query, lines);
            }
        }
    }
}

impl<T> Window<T>
where
    T: Select,
{
    /// Keeps `draft`, of `verdict`, in `kept` or for later. A match takes
    /// the records waiting before it, its lines `--grep` matched when
    /// `lines` asks for them, and opens the window after it. Another record
    /// in the window is kept as context; one before the window waits.
    fn offer(
        &mut self,
        kept: &mut Kept<T>,
        (block, mut draft): (usize, Draft<T>),
        verdict: Verdict,
        response: Option<&Response>,
        query: &Query,
        lines: Option<usize>,
    ) {
        match verdict {
            Verdict::Hit => {
                if let Some(context) = lines {
                    let found = excerpt(&draft.record().grep_fields(), &query.grep, context);
                    *draft.matches_mut() = found;
                }
                let at = kept.drafts.len();
                kept.keep(None, (block, draft), response);
                for (offset, (earlier, context, of)) in
                    mem::take(&mut self.before).into_iter().enumerate()
                {
                    kept.keep(
                        Some(at.saturating_add(offset)),
                        (earlier, context),
                        of.as_ref(),
                    );
                }
                self.after = self.around;
            }
            Verdict::Near if self.after > 0 => {
                self.after = self.after.saturating_sub(1);
                draft.mark_context();
                kept.keep(None, (block, draft), response);
            }
            Verdict::Near if self.around > 0 => {
                draft.mark_context();
                self.before.push_back((block, draft, response.cloned()));
                if self.before.len() > self.around {
                    self.before.pop_front();
                }
            }
            Verdict::Near | Verdict::Out => {}
        }
    }
}

/// The replies of the assistant line `entry` that the including patterns
/// of `query` match and the other filters let through, by line ID and
/// block. They are counted, not kept, and a line out of the range is not
/// masked only to be dropped.
fn mentioned_replies(spot: &Spot<'_>, entry: &Entry, query: &Query) -> Vec<Mention> {
    if !query.range.contains(spot.timestamp.instant()) {
        return Vec::new();
    }
    Replies
        .pick(spot, entry)
        .into_iter()
        .filter(|pair| verdict(query, &pair.1) == Verdict::Hit)
        .map(|(block, draft)| Mention::of(draft.record(), block))
        .collect()
}

/// Adds the tokens of the assistant line `entry` to `used` when it is in
/// the range of `query` and the first line of its response, by
/// `responses`; a line without a response ID is a response of its own.
fn tally(
    entry: &Entry,
    query: &Query,
    responses: &mut HashSet<String>,
    used: &mut Vec<(String, Usage)>,
) {
    if entry.role == Role::Assistant
        && let (Some(tokens), Some(timestamp)) = (entry.usage, entry.timestamp)
        && query.range.contains(timestamp.instant())
        && (entry.message_id.is_empty() || responses.insert(entry.message_id.clone()))
    {
        used.push((entry.message_id.clone(), tokens));
    }
}

/// The later of `last` and the time of `entry` when that is in the range
/// of `query` and the line holds a record: a reply or tool call, a prompt,
/// a tool result, or a summary, not a line the harness wrote.
fn later(last: Option<Stamp>, entry: &Entry, query: &Query) -> Option<Stamp> {
    let holds_a_record = entry.role == Role::Assistant
        || entry.content.has_tool_result
        || matches!(classify(entry), Reason::Human | Reason::CompactSummary);
    match entry.timestamp {
        Some(timestamp)
            if holds_a_record
                && query.range.contains(timestamp.instant())
                && last.is_none_or(|seen| seen.instant() < timestamp.instant()) =>
        {
            Some(timestamp)
        }
        Some(_) | None => last,
    }
}

/// The records of `drafts`, marked superseded where `picker`, which has now
/// seen the whole file, says so, then masked and given the title.
fn seal_all<P>(
    drafts: Vec<(usize, Draft<P::Record>)>,
    picker: &P,
    mask: &Mask,
    title: &Title<'_>,
) -> Vec<(usize, P::Record)>
where
    P: Pick,
{
    drafts
        .into_iter()
        .map(|(block, mut draft)| {
            if picker.superseded(draft.record()) {
                draft.mark_superseded();
            }
            (block, mask.seal(draft, title))
        })
        .collect()
}

/// What the filters in `query` make of `draft`.
fn verdict<T>(query: &Query, draft: &Draft<T>) -> Verdict
where
    T: Select,
{
    let record = draft.record();
    let head = record.head();
    let extra = record.extra();
    let text = if query.grep.is_empty() {
        Cow::Borrowed("")
    } else {
        record.grep_text()
    };
    query.verdict(&Facts {
        branch: &head.git_branch,
        cwd: &head.cwd,
        denial: extra.denial,
        is_error: extra.is_error,
        project: &head.project,
        session_id: &head.session_id,
        skill: extra.skill,
        text: &text,
        timestamp: head.timestamp.instant(),
        tool: extra.tool,
    })
}

/// Reads one transcript and returns its selected records in file order,
/// with the lines `--grep` matched when `reading` asks for an excerpt, and
/// when it asks for usage, the tokens of each response in the range, once.
///
/// # Errors
///
/// When the file cannot be opened or read, as a message for the warning.
pub fn read_file<P>(
    file: &SessionFile,
    query: &Query,
    mask: &Mask,
    mut picker: P,
    reading: Reading,
) -> Result<Read<P::Record>, String>
where
    P: Pick,
{
    let wanted = picker.roles();
    let roles = Roles {
        assistant: wanted.assistant || reading.usage || reading.mentions,
        user: wanted.user,
    };
    let mut lines = reader::open(&file.path, MAX_LINE_BYTES, roles)
        .map_err(|error| format!("open transcript: {error}"))?;
    let mut titles = Titles::default();
    let mut kept = Kept {
        drafts: Vec::new(),
        priced: HashSet::new(),
    };
    let mut keeper = Keeper::new(reading);
    let mut untimed = 0_usize;
    let mut used = Vec::new();
    let mut responses = HashSet::new();
    let mut last: Option<Stamp> = None;
    for item in &mut lines {
        let (line, entry) = item.map_err(|error| format!("read transcript: {error}"))?;
        if entry.role == Role::AiTitle {
            titles.see(entry.title, file.parent.is_none(), mask);
            continue;
        }
        last = later(last, &entry, query);
        if reading.usage {
            tally(&entry, query, &mut responses, &mut used);
        }
        if let Keeper::Tally(ref mut found) = keeper
            && entry.role == Role::Assistant
            && let Some(timestamp) = entry.timestamp
        {
            found.extend(mentioned_replies(
                &titles.spot(file, line, mask, timestamp),
                &entry,
                query,
            ));
        }
        if !wanted.assistant && entry.role == Role::Assistant {
            continue;
        }
        if !picker.wants(&entry) {
            continue;
        }
        let Some(timestamp) = entry.timestamp else {
            untimed = untimed.saturating_add(1);
            continue;
        };
        let spot = titles.spot(file, line, mask, timestamp);
        let response = Response::of(&entry, line);
        for drafted in picker.pick(&spot, &entry) {
            let found = verdict(query, &drafted.1);
            keeper.offer(
                &mut kept,
                drafted,
                found,
                response.as_ref(),
                query,
                reading.excerpt,
            );
        }
    }
    // Titles are generated from the conversation, so a pasted secret can
    // end up in one.
    let mut title_masked = Counts::default();
    let (masked_title, _) = mask.text(&titles.last, &mut title_masked);
    let sealing = Title {
        masked: file.parent.is_none().then_some(&title_masked),
        text: &masked_title,
    };
    let records = seal_all(kept.drafts, &picker, mask, &sealing);
    let stats = lines.stats();
    Ok(Read {
        last,
        mentions: keeper.mentions(),
        records,
        skipped: Skipped {
            oversized: stats.oversized,
            undecodable: stats.undecodable,
            untimed,
        },
        title: masked_title,
        title_masked,
        usage: used,
    })
}
