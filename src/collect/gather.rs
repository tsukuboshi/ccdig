//! Joining the files' records: titles for sub-agents, each record once
//! across resumed copies, and one session record per file.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::mask::Mask;
use regex::Regex;

use super::read::{Mention, Spent};
use super::{Corpus, Select};
use crate::classify::human::HARNESS_MODEL;
use crate::filter::Filter;
use crate::record::{AGENT, FIRST_MATCH_CHARS, Record, Session};
use crate::redact::Counts;
use crate::text::{Lang, cut_text, one_line, shell_quote};
use crate::transcript::entry::Usage;
use crate::transcript::walk::{SessionFile, is_dir};
use crate::when::Stamp;

/// A file's selected records, with what the session record needs of it.
pub struct FileRecords<'file, T> {
    /// The working directory the transcript starts in, unmasked.
    pub cwd: String,
    /// The transcript file.
    pub file: &'file SessionFile,
    /// The time of the last record read in the range.
    pub last: Option<Stamp>,
    /// With `Reading::mentions`, the prompts and replies the including
    /// patterns matched, in file order.
    pub mentions: Option<Vec<Mention>>,
    /// When the file was last written to.
    pub modified: Stamp,
    /// The records, each with its block's index in the line.
    pub records: Vec<(usize, T)>,
    /// What the file's responses in the range spent, by response ID.
    pub usage: Vec<Spent>,
}

/// Keeps each record once across sessions, in session order, and builds
/// the session records. A resumed or forked session starts with a copy of
/// earlier entries, IDs included; the first session in order wins. A
/// record `--around` kept as context goes too when no match it is within
/// `around` records of is left in its file. The note of a cut first match
/// is in `lang`.
pub fn gather<T>(
    sessions: Vec<FileRecords<'_, T>>,
    warnings: Vec<String>,
    mask: &Mask,
    around: usize,
    grep: &Filter<Regex>,
    lang: Lang,
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
    // Whether each working directory seen is there, looked up once.
    let mut dirs: HashMap<String, bool> = HashMap::new();
    let mut corpus = Corpus {
        records: Vec::new(),
        sessions: Vec::new(),
        warnings,
    };
    for session in sessions {
        // Whether each record is new, here and in the files before. Only a
        // record kept is marked as seen, so context a copy drops can come
        // back for a later match.
        let mut here = HashSet::new();
        let fresh: Vec<bool> = session
            .records
            .iter()
            .map(|&(block, ref record)| {
                let key = (record.source().uuid.clone(), block);
                key.0.is_empty() || (!seen.contains(&key) && here.insert(key))
            })
            .collect();
        let near = near_a_match(&session.records, &fresh, around);
        let kept: Vec<T> = session
            .records
            .into_iter()
            .zip(fresh.iter().zip(near))
            .filter(|&(ref pair, (&is_fresh, is_near))| is_fresh && (pair.1.matched() || is_near))
            .map(|((block, mut record), _)| {
                let source = record.source_mut();
                source.block = block;
                if !source.uuid.is_empty() {
                    seen.insert((source.uuid.clone(), block));
                }
                record
            })
            .collect();
        let (Some(first), Some(last)) = (kept.first(), kept.last()) else {
            continue;
        };
        let mut matched_types = Vec::new();
        let (match_count, first_match) = match session.mentions {
            Some(found) => {
                let counted: Vec<Mention> = found
                    .into_iter()
                    .filter(|mention| {
                        mention.key().0.is_empty() || mentioned.insert(mention.key().clone())
                    })
                    .collect();
                matched_types = counted.iter().map(Mention::kind).collect();
                let count = counted.len();
                let Some(earliest) = counted.into_iter().next() else {
                    // A resumed copy repeats these responses, and must not
                    // count them as its own.
                    for response in &session.usage {
                        if !response.id.is_empty() {
                            priced.insert(response.id.clone());
                        }
                    }
                    continue;
                };
                (Some(count), Some(earliest))
            }
            None => (None, None),
        };
        let mut models_masked = Counts::default();
        let (usage, models) = unpriced(session.usage, &mut priced, mask, &mut models_masked);
        let head = first.head();
        let last_kept = last.head().timestamp;
        let mut masked = printed(first, first_match.as_ref());
        masked.merge(&models_masked);
        // `printed` counted the path with the first record.
        let shown_file = mask.meta(&session.file.path.to_string_lossy(), &mut Counts::default());
        let (resume, resume_unavailable) = resume(&head.session_id, &session.cwd, mask, &mut dirs);
        corpus.sessions.push(Session {
            agent: AGENT,
            id: head.session_id.clone(),
            title: head.session_title.clone(),
            project: head.project.clone(),
            cwd: head.cwd.clone(),
            git_branch: head.git_branch.clone(),
            file: shown_file,
            mtime: session.modified,
            started_at: head.timestamp,
            ended_at: session.last.map_or(last_kept, |read| {
                if read.instant() > last_kept.instant() {
                    read
                } else {
                    last_kept
                }
            }),
            prompt_count: kept.len(),
            first_prompt: cut_text(&one_line(&first.grep_text()), FIRST_MATCH_CHARS, lang),
            match_count,
            first_match: first_match.map(|earliest| earliest.first_match(grep, lang)),
            usage,
            models,
            resume,
            resume_unavailable,
            masked,
            matched_types,
        });
        corpus.records.extend(kept);
    }
    corpus
}

/// For each of `records`, whether a match that is `fresh` is at most
/// `around` records away, looking both ways in one pass each. The records
/// of a file are its matches and the context `Window` kept around them, so
/// a record of context is within `around` of the match it was kept for.
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
    // How far the last match is, in the direction of the pass.
    let mut pass = |index: usize, since: &mut Option<usize>| {
        if hits.get(index).copied().unwrap_or_default() {
            *since = Some(0);
        } else {
            *since = since.map(|count| count.saturating_add(1));
        }
        if since.is_some_and(|count| count <= around)
            && let Some(slot) = near.get_mut(index)
        {
            *slot = true;
        }
    };
    let mut since = None;
    for index in 0..hits.len() {
        pass(index, &mut since);
    }
    since = None;
    for index in (0..hits.len()).rev() {
        pass(index, &mut since);
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
        // The title then is the same title, so its masks count again.
        head.session_title_then.clone_from(&head.session_title);
        head.masked.merge(counts);
        head.masked.merge(counts);
    }
}

/// What masking counted in what a session's record prints: the shared
/// keys and the text of `first`, its first record, and `earliest`, its
/// first match. The own counts of `first` hold those of its file's path
/// too, and a session's records all come from its file, so they count the
/// session's `file`.
fn printed<T>(first: &T, earliest: Option<&Mention>) -> Counts
where
    T: Select,
{
    let mut masked = first.head().masked.clone();
    if let Some(found) = earliest {
        masked.merge(found.masked());
    }
    masked.merge(first.own_masked());
    masked
}

/// The command that resumes session `id` in `cwd`, the unmasked
/// directory its transcript starts in, or why there is none: `no-cwd` when
/// the transcript gave none, `cwd-masked` when `mask` changes it, so the
/// command would print what the masks hide, and `cwd-missing` when it is
/// not a directory now. `dirs` keeps what was found for each directory, so
/// each is looked up once.
fn resume(
    id: &str,
    cwd: &str,
    mask: &Mask,
    dirs: &mut HashMap<String, bool>,
) -> (Option<String>, Option<&'static str>) {
    if cwd.is_empty() {
        return (None, Some("no-cwd"));
    }
    // Nothing is printed when the masks change it, so nothing is counted.
    if mask.meta(cwd, &mut Counts::default()) != cwd {
        return (None, Some("cwd-masked"));
    }
    let there = if let Some(&known) = dirs.get(cwd) {
        known
    } else {
        let found = is_dir(cwd);
        dirs.insert(cwd.to_owned(), found);
        found
    };
    if !there {
        return (None, Some("cwd-missing"));
    }
    (
        Some(format!(
            "cd {} && claude --resume {}",
            shell_quote(cwd),
            shell_quote(id)
        )),
        None,
    )
}

/// The sum of the tokens of `responses` not in `priced`, each then added
/// to it, and the models that wrote them, masked as a record's `model` is,
/// each once in the order it first wrote one, Claude Code itself left out;
/// a response without an ID is always counted. What masking the models
/// counted, once per model shown, goes in `masked`.
fn unpriced(
    responses: Vec<Spent>,
    priced: &mut HashSet<String>,
    mask: &Mask,
    masked: &mut Counts,
) -> (Usage, Vec<String>) {
    let mut usage = Usage::default();
    let mut models: Vec<String> = Vec::new();
    for response in responses {
        if response.id.is_empty() || priced.insert(response.id) {
            usage.add(response.tokens);
            // Claude Code names itself on a reply it wrote, as when a
            // request failed; no model wrote that.
            if response.model.is_empty() || response.model == HARNESS_MODEL {
                continue;
            }
            let mut counts = Counts::default();
            let shown = mask.meta(&response.model, &mut counts);
            if !models.contains(&shown) {
                models.push(shown);
                masked.merge(&counts);
            }
        }
    }
    (usage, models)
}
