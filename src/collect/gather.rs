//! Joining the files' records: titles for sub-agents, each record once
//! across resumed copies, and one session record per file.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::mask::Mask;
use super::{Corpus, Select};
use crate::record::{AGENT, Record, Session};
use crate::redact::Counts;
use crate::transcript::entry::Usage;
use crate::transcript::walk::SessionFile;
use crate::when::Stamp;

/// A file's selected records, with what the session record needs of it.
pub struct FileRecords<'file, T> {
    /// The transcript file.
    pub file: &'file SessionFile,
    /// With `Reading::mentions`, the prompts and replies the including
    /// patterns matched, by line ID and block.
    pub mentions: Option<Vec<(String, usize)>>,
    /// When the file was last written to.
    pub modified: Stamp,
    /// The records, each with its block's index in the line.
    pub records: Vec<(usize, T)>,
    /// The tokens of the file's responses in the range, by response ID.
    pub usage: Vec<(String, Usage)>,
}

/// Keeps each record once across sessions, in session order, and builds
/// the session records. A resumed or forked session starts with a copy of
/// earlier entries, IDs included; the first session in order wins. A
/// record `--around` kept as context goes too when no match it is within
/// `around` records of is left in its file.
pub fn gather<T>(
    sessions: Vec<FileRecords<'_, T>>,
    warnings: Vec<String>,
    mask: &Mask,
    around: usize,
) -> Corpus<T>
where
    T: Select,
{
    let mut seen = HashSet::new();
    // The responses already added to a session, which a resumed copy
    // repeats.
    let mut priced: HashSet<String> = HashSet::new();
    // The matched prompts and replies already counted, which a resumed copy
    // repeats.
    let mut mentioned: HashSet<(String, usize)> = HashSet::new();
    let mut corpus = Corpus {
        records: Vec::new(),
        sessions: Vec::new(),
        warnings,
    };
    for session in sessions {
        let fresh: Vec<bool> = session
            .records
            .iter()
            .map(|&(block, ref record)| {
                let uuid = &record.source().uuid;
                uuid.is_empty() || seen.insert((uuid.clone(), block))
            })
            .collect();
        let near = near_a_match(&session.records, &fresh, around);
        let kept: Vec<T> = session
            .records
            .into_iter()
            .zip(fresh.iter().zip(near))
            .filter(|&(ref pair, (&is_fresh, is_near))| is_fresh && (pair.1.matched() || is_near))
            .map(|((_, record), _)| record)
            .collect();
        let (Some(first), Some(last)) = (kept.first(), kept.last()) else {
            continue;
        };
        let match_count = match session.mentions {
            Some(found) => {
                let count = found
                    .into_iter()
                    .filter(|key| key.0.is_empty() || mentioned.insert(key.clone()))
                    .count();
                if count == 0 {
                    // A resumed copy repeats these responses, and must not
                    // count them as its own.
                    for response in &session.usage {
                        if !response.0.is_empty() {
                            priced.insert(response.0.clone());
                        }
                    }
                    continue;
                }
                Some(count)
            }
            None => None,
        };
        let mut usage = Usage::default();
        for (id, tokens) in session.usage {
            if id.is_empty() || priced.insert(id) {
                usage.add(tokens);
            }
        }
        let head = first.head();
        let mut masked = head.masked.clone();
        let shown_file = mask.meta(&session.file.path.to_string_lossy(), &mut masked);
        corpus.sessions.push(Session {
            agent: AGENT,
            id: head.session_id.clone(),
            title: head.session_title.clone(),
            project: head.project.clone(),
            cwd: head.cwd.clone(),
            git_branch: head.git_branch.clone(),
            file: shown_file,
            // UTC keeps the output the same on every machine.
            mtime: session.modified,
            started_at: head.timestamp,
            ended_at: last.head().timestamp,
            prompt_count: kept.len(),
            match_count,
            usage,
            masked,
        });
        corpus.records.extend(kept);
    }
    corpus
}

/// For each of `records`, whether a match that is `fresh` is at most
/// `around` records away, looking both ways in one pass each.
fn near_a_match<T>(records: &[(usize, T)], fresh: &[bool], around: usize) -> Vec<bool>
where
    T: Record,
{
    let hits: Vec<bool> = records
        .iter()
        .zip(fresh)
        .map(|(pair, &is_fresh)| is_fresh && pair.1.matched())
        .collect();
    let mut near = vec![false; hits.len()];
    for forward in [true, false] {
        let mut since: Option<usize> = None;
        let order: Vec<usize> = if forward {
            (0..hits.len()).collect()
        } else {
            (0..hits.len()).rev().collect()
        };
        for index in order {
            if hits.get(index).copied().unwrap_or_default() {
                since = Some(0);
            } else {
                since = since.map(|count| count.saturating_add(1));
            }
            if since.is_some_and(|count| count <= around)
                && let Some(slot) = near.get_mut(index)
            {
                *slot = true;
            }
        }
    }
    near
}

/// Gives the records of a sub-agent's transcript the title of its session,
/// as a sub-agent's transcript has no `ai-title` line of its own. The
/// session's transcript must have been read in the same run, so one last
/// written before `--since` leaves the title empty. What masking counted
/// goes with the title the records keep: the session's, or without it the
/// sub-agent's own, from `own_titles`.
pub fn inherit_title<T>(
    file: &SessionFile,
    records: &mut [(usize, T)],
    titles: &HashMap<PathBuf, (String, Counts)>,
    own_titles: &HashMap<PathBuf, Counts>,
) where
    T: Select,
{
    if file.parent.is_none() {
        return;
    }
    let parent = file.parent.as_ref().and_then(|parent| titles.get(parent));
    let Some(counts) = parent
        .map(|found| &found.1)
        .or_else(|| own_titles.get(&file.path))
    else {
        return;
    };
    for pair in records {
        let head = pair.1.head_mut();
        if let Some(found) = parent {
            head.session_title.clone_from(&found.0);
        }
        head.masked.merge(counts);
    }
}
