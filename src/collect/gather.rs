//! Joining the files' records: titles for sub-agents, each record once
//! across resumed copies, and one session record per file.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::mask::Mask;
use super::{Corpus, Select};
use crate::record::{AGENT, Session};
use crate::redact::Counts;
use crate::transcript::entry::Usage;
use crate::transcript::walk::SessionFile;
use crate::when::Stamp;

/// A file's selected records, with what the session record needs of it.
pub struct FileRecords<'file, T> {
    /// The transcript file.
    pub file: &'file SessionFile,
    /// When the file was last written to.
    pub modified: Stamp,
    /// The records, each with its block's index in the line.
    pub records: Vec<(usize, T)>,
    /// The tokens of the file's responses in the range, by response ID.
    pub usage: Vec<(String, Usage)>,
}

/// Keeps each record once across sessions, in session order, and builds
/// the session records. A resumed or forked session starts with a copy of
/// earlier entries, IDs included; the first session in order wins.
pub fn gather<T>(sessions: Vec<FileRecords<'_, T>>, warnings: Vec<String>, mask: &Mask) -> Corpus<T>
where
    T: Select,
{
    let mut seen = HashSet::new();
    // The responses already added to a session, which a resumed copy
    // repeats.
    let mut priced: HashSet<String> = HashSet::new();
    let mut corpus = Corpus {
        records: Vec::new(),
        sessions: Vec::new(),
        warnings,
    };
    for session in sessions {
        let kept: Vec<T> = session
            .records
            .into_iter()
            .filter(|&(block, ref record)| {
                let uuid = &record.source().uuid;
                uuid.is_empty() || seen.insert((uuid.clone(), block))
            })
            .map(|(_, record)| record)
            .collect();
        let (Some(first), Some(last)) = (kept.first(), kept.last()) else {
            continue;
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
            usage,
            masked,
        });
        corpus.records.extend(kept);
    }
    corpus
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
