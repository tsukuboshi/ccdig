//! Reading one transcript: the lines a picker wants become records with
//! their text masked, the filters see them, and the records they keep are
//! sealed with the file's title once the whole file is read.

use alloc::borrow::Cow;
use std::collections::HashSet;

use super::mask::{Draft, Mask, Title};
use super::{Pick, Select, Spot};
use crate::redact::Counts;
use crate::select::{Facts, Query};
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
/// and with `usage`, the tokens of each response in the range, once.
///
/// # Errors
///
/// When the file cannot be opened or read, as a message for the warning.
pub fn read_file<P>(
    file: &SessionFile,
    query: &Query,
    mask: &Mask,
    mut picker: P,
    usage: bool,
) -> Result<Read<P::Record>, String>
where
    P: Pick,
{
    let wanted = picker.roles();
    let roles = Roles {
        assistant: wanted.assistant || usage,
        user: wanted.user,
    };
    let mut lines = reader::open(&file.path, MAX_LINE_BYTES, roles)
        .map_err(|error| format!("open transcript: {error}"))?;
    let mut title = String::new();
    let mut drafts: Vec<(usize, Draft<P::Record>)> = Vec::new();
    let mut untimed = 0_usize;
    let mut used = Vec::new();
    let mut responses = HashSet::new();
    let mut priced = HashSet::new();
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
        let first_new = drafts.len();
        drafts.extend(
            picker
                .pick(&spot, &entry)
                .into_iter()
                .map(|(block, record)| (block, Draft::new(record)))
                .filter(|pair| passes(query, &pair.1)),
        );
        // A response's lines repeat its tokens, so only the first record
        // printed from it carries them; a line without an ID is a response
        // of its own.
        if let Some(tokens) = entry.usage
            && let Some(slot) = drafts
                .get_mut(first_new..)
                .and_then(|fresh| fresh.iter_mut().find_map(|pair| pair.1.usage_mut()))
            && (entry.message_id.is_empty() || priced.insert(entry.message_id.clone()))
        {
            *slot = Some(tokens);
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
    let records = drafts
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
