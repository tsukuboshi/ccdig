//! Reading one transcript: the lines a picker wants become records with
//! their text masked, the filters see them, and the records they keep are
//! sealed with the file's title once the whole file is read.

use alloc::borrow::Cow;
use std::collections::HashSet;

use super::excerpt::excerpt;
use super::mask::{Draft, Mask, Title};
use super::{Pick, Reading, Select, Spot};
use crate::filter::{Facts, Query};
use crate::redact::Counts;
use crate::transcript::entry::{Role, Usage};
use crate::transcript::reader::{self, MAX_LINE_BYTES, Roles};
use crate::transcript::walk::SessionFile;

/// What a transcript file yielded.
pub struct Read<T> {
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
struct Response {
    /// The response's ID, or for a line without one, which is a response
    /// of its own, the line's number after a NUL, which no ID has.
    key: String,
    /// The tokens.
    tokens: Usage,
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

/// Whether `draft` passes the filters in `query`.
fn passes<T>(query: &Query, draft: &Draft<T>) -> bool
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
    query.matches(&Facts {
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
    let usage = reading.usage;
    let wanted = picker.roles();
    let roles = Roles {
        assistant: wanted.assistant || usage,
        user: wanted.user,
    };
    let mut lines = reader::open(&file.path, MAX_LINE_BYTES, roles)
        .map_err(|error| format!("open transcript: {error}"))?;
    let mut title = String::new();
    let mut kept = Kept {
        drafts: Vec::new(),
        priced: HashSet::new(),
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
        if usage
            && entry.role == Role::Assistant
            && let (Some(tokens), Some(timestamp)) = (entry.usage, entry.timestamp)
            && query.range.contains(timestamp.instant())
            && (entry.message_id.is_empty() || responses.insert(entry.message_id.clone()))
        {
            used.push((entry.message_id.clone(), tokens));
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
        let response = entry.usage.map(|tokens| Response {
            key: if entry.message_id.is_empty() {
                format!("\0{line}")
            } else {
                entry.message_id.clone()
            },
            tokens,
        });
        for (block, mut draft) in picker.pick(&spot, &entry) {
            if !passes(query, &draft) {
                continue;
            }
            if let Some(context) = reading.excerpt {
                let found = excerpt(&draft.record().grep_fields(), &query.grep, context);
                *draft.matches_mut() = found;
            }
            kept.push(block, draft, response.as_ref());
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
