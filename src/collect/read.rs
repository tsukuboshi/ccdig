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
use crate::filter::{Facts, Query, Verdict};
use crate::redact::Counts;
use crate::transcript::entry::{Entry, Role, Usage};
use crate::transcript::reader::{self, MAX_LINE_BYTES, Roles};
use crate::transcript::walk::SessionFile;

/// What a transcript file yielded.
pub struct Read<T> {
    /// With `Reading::mentions`, the prompts and replies the including
    /// patterns matched, by line ID and block.
    pub mentions: Option<Vec<(String, usize)>>,
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
    /// With `Reading::mentions`, the records the including patterns
    /// matched, by line ID and block; every record the other filters let
    /// through is then kept.
    mentions: Option<Vec<(String, usize)>>,
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

impl<T> Kept<T>
where
    T: Select,
{
    /// Keeps `draft`. A response's lines repeat its tokens, so only the
    /// first record kept from it that carries tokens gets them.
    fn push(&mut self, block: usize, mut draft: Draft<T>, response: Option<&Response>) {
        if let Some(of) = response
            && let Some(slot) = draft.usage_mut()
            && self.priced.insert(of.key.clone())
        {
            *slot = Some(of.tokens);
        }
        self.drafts.push((block, draft));
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
        if let Some(ref mut found) = self.mentions {
            if verdict == Verdict::Hit {
                found.push((draft.record().source().uuid.clone(), block));
            }
            if verdict != Verdict::Out {
                kept.push(block, draft, response);
            }
            return;
        }
        match verdict {
            Verdict::Hit => {
                for (earlier, context, of) in mem::take(&mut self.before) {
                    kept.push(earlier, context, of.as_ref());
                }
                if let Some(context) = lines {
                    let found = excerpt(&draft.record().grep_fields(), &query.grep, context);
                    *draft.matches_mut() = found;
                }
                kept.push(block, draft, response);
                self.after = self.around;
            }
            Verdict::Near if self.after > 0 => {
                self.after = self.after.saturating_sub(1);
                draft.mark_context();
                kept.push(block, draft, response);
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
/// block. They are counted, not kept.
fn mentioned_replies(spot: &Spot<'_>, entry: &Entry, query: &Query) -> Vec<(String, usize)> {
    Replies
        .pick(spot, entry)
        .into_iter()
        .filter(|pair| verdict(query, &pair.1) == Verdict::Hit)
        .map(|(block, _)| (entry.uuid.clone(), block))
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
    let mut title = String::new();
    let mut kept = Kept {
        drafts: Vec::new(),
        priced: HashSet::new(),
    };
    let mut window = Window {
        after: 0,
        around: reading.around.unwrap_or_default(),
        before: VecDeque::new(),
        mentions: reading.mentions.then(Vec::new),
    };
    let mut untimed = 0_usize;
    let mut used = Vec::new();
    let mut responses = HashSet::new();
    for item in &mut lines {
        let (line, entry) = item.map_err(|error| format!("read transcript: {error}"))?;
        if entry.role == Role::AiTitle {
            // The title is regenerated as a session goes on; the last wins.
            if !entry.title.is_empty() {
                title = entry.title;
            }
            continue;
        }
        if reading.usage {
            tally(&entry, query, &mut responses, &mut used);
        }
        if let Some(ref mut found) = window.mentions
            && entry.role == Role::Assistant
            && let Some(timestamp) = entry.timestamp
        {
            found.extend(mentioned_replies(
                &Spot {
                    file,
                    line,
                    mask,
                    timestamp,
                },
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
        let spot = Spot {
            file,
            line,
            mask,
            timestamp,
        };
        let response = Response::of(&entry, line);
        for drafted in picker.pick(&spot, &entry) {
            let found = verdict(query, &drafted.1);
            window.offer(
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
    let (masked_title, _) = mask.text(&title, &mut title_masked);
    let sealing = Title {
        masked: file.parent.is_none().then_some(&title_masked),
        text: &masked_title,
    };
    let records = kept
        .drafts
        .into_iter()
        .map(|(block, draft)| (block, mask.seal(draft, &sealing)))
        .collect();
    let stats = lines.stats();
    Ok(Read {
        mentions: window.mentions,
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
