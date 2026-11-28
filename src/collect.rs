//! Reading every transcript under the root into the records a picker picks.
//! The stages run in this order: list the files, read each one, pick the
//! records with their text masked, filter them, mask their metadata, and
//! join the files, dropping resumed copies.

mod excerpt;
mod gather;
pub mod kinds;
pub mod mask;
mod read;

use alloc::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use self::gather::{FileRecords, gather, inherit_title};
use self::kinds::Types;
use self::mask::{Draft, Mask};
use self::read::{Mention, Skipped, read_file};
use crate::failure::Failure;
use crate::filter::Query;
use crate::record::{AGENT, Head, Outcome, Record, Search, Session, Spawned, Touch};
use crate::redact::Counts;
use crate::text::{Lang, clean_text};
use crate::transcript::entry::{Entry, Usage};
use crate::transcript::meta::Spawn;
use crate::transcript::reader::{MAX_LINE_BYTES, Roles};
use crate::transcript::walk::{self, SessionFile};
use crate::when::Stamp;

/// The selected records, and the sessions they came from, in output order.
pub struct Corpus<T> {
    /// The records, grouped by transcript file.
    pub records: Vec<T>,
    /// One record per transcript file with any selected records.
    pub sessions: Vec<Session>,
    /// What was skipped, each a line for stderr.
    pub warnings: Vec<String>,
}

/// How to read the transcripts, besides which records to pick and keep.
#[derive(Clone, Copy, Debug, Default)]
pub struct Reading {
    /// With `--around`, how many records before and after a record
    /// `--grep` matched to keep as context.
    pub around: Option<usize>,
    /// With `--context`, how many lines around each line `--grep` matched
    /// to list in a record's `matches`.
    pub excerpt: Option<usize>,
    /// The language of the note that marks a cut, as in `first_match`:
    /// English for JSON, whose values do not change with `--lang`.
    pub lang: Lang,
    /// For `sessions --grep`: the kinds of record whose matches of the
    /// including patterns to count, keeping every prompt the other filters
    /// let through rather than only the prompts the patterns match; `None`
    /// otherwise.
    pub mentions: Option<Types>,
    /// Whether to read how each sub-agent was started, for the records
    /// that say it; the others read none of its meta file.
    pub spawns: bool,
    /// Whether to read sub-agent transcripts too.
    pub subagents: bool,
    /// Whether to add up the tokens of each session's responses in the
    /// range.
    pub usage: bool,
}

/// One kind of record and how it is picked out of transcript lines. A
/// picker is made for each file, so it can remember what it saw earlier in
/// the file.
pub trait Pick {
    /// The record it makes.
    type Record: Select;

    /// What the call `tool_use_id` came to, its text not yet masked, as
    /// only the whole file tells: asked once every line has been seen.
    /// Only calls have one.
    fn outcome(&self, _tool_use_id: &str) -> Option<&Outcome> {
        None
    }

    /// Makes the records of an entry that `wants` accepted, each with its
    /// block's index in the line. A record comes with its text masked and
    /// its metadata not, as a `Draft` that only `Mask::seal` opens.
    fn pick(&mut self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, Draft<Self::Record>)>;

    /// The entries the reader should deliver.
    fn roles(&self) -> Roles;

    /// Whether the user sent another prompt in place of `record`, as only
    /// the whole file tells: asked once every line has been seen. Only
    /// prompts can be.
    fn superseded(&self, _record: &Self::Record) -> bool {
        false
    }

    /// Whether `entry` holds records of this kind. It sees every delivered
    /// entry in file order, so it can also take notes.
    fn wants(&mut self, entry: &Entry) -> bool;
}

/// What a record offers to the collector besides its shared keys.
pub trait Select: Record {
    /// The kind's own facts for the filters.
    fn extra(&self) -> Extra<'_>;

    /// The texts `--context` lists lines of, each named for `field`:
    /// `text` by default.
    fn grep_fields(&self) -> Vec<(String, Cow<'_, str>)> {
        vec![("text".to_owned(), self.grep_text())]
    }

    /// What `--grep` searches, built only when `--grep` is given.
    fn grep_text(&self) -> Cow<'_, str>;

    /// The shared keys, to fill in the session title.
    fn head_mut(&mut self) -> &mut Head;

    /// The record's `type`, as `--type` names it.
    fn kind(&self) -> &'static str;

    /// The kind's own fields that are not text, such as the tool's name,
    /// for the masks `--mask-values` asks for.
    fn labels_mut(&mut self) -> Vec<&mut String>;

    /// What masking the kind's own fields counted.
    fn masked_mut(&mut self) -> &mut Counts;

    /// The call's ID, where the kind carries what the call came to, and
    /// whether masking changed the record, `None` for a kind that is not a
    /// call.
    fn outcome_mut(&mut self) -> Option<(&str, &mut Option<Outcome>, &mut bool)> {
        None
    }

    /// What the search made of the record, which `--around` and
    /// `--context` fill in.
    fn search_mut(&mut self) -> &mut Search;

    /// Where the kind says whether the user sent another prompt in its
    /// place, `None` for a kind that does not.
    fn superseded_mut(&mut self) -> Option<&mut bool> {
        None
    }

    /// Where the kind carries the tokens of its response, `None` for a kind
    /// that does not.
    fn usage_mut(&mut self) -> Option<&mut Option<Usage>> {
        None
    }
}

/// The facts the filters see that only some kinds have, `None` for a kind
/// without them.
#[derive(Default)]
pub struct Extra<'record> {
    /// Why a tool call did not run, empty when it ran.
    pub denial: Option<&'record str>,
    /// Whether a tool reported a failure.
    pub is_error: Option<bool>,
    /// How a prompt was typed.
    pub kind: Option<&'static str>,
    /// The skill running at the time.
    pub skill: Option<&'record str>,
    /// The tool's name.
    pub tool: Option<&'record str>,
    /// What a tool call changed.
    pub touched: Option<&'record Touch>,
}

/// Where an entry is, for building its records.
pub struct Spot<'file> {
    /// The transcript file.
    pub file: &'file SessionFile,
    /// The 1-based line number.
    pub line: usize,
    /// The masking to apply.
    pub mask: &'file Mask,
    /// How the sub-agent whose transcript this is was started.
    pub started: &'file Started,
    /// When the line was written.
    pub timestamp: Stamp,
    /// The session title when the line was written, masked; empty in a
    /// sub-agent's transcript, whose records take their session's title.
    pub title: &'file str,
    /// What masking `title` counted, which each record of the line counts
    /// again, as it does the last title.
    pub title_masked: &'file Counts,
}

impl Spot<'_> {
    /// The shared keys of a record of `entry`. A line without a session ID
    /// takes it from the file name.
    pub fn head(&self, entry: &Entry) -> Head {
        let session_id = if entry.session_id.is_empty() {
            self.file
                .path
                .file_stem()
                .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned())
        } else {
            entry.session_id.clone()
        };
        Head {
            agent: AGENT,
            session_id,
            session_title: String::new(),
            session_title_then: self.title.to_owned(),
            project: self.file.project.clone(),
            cwd: entry.cwd.clone(),
            git_branch: entry.git_branch.clone(),
            timestamp: self.timestamp,
            turn: 0,
            masked: self.title_masked.clone(),
        }
    }

    /// How the sub-agent was started, adding what masking its description
    /// counted to `masked`, as each of its records counts it again.
    pub fn spawned(&self, masked: &mut Counts) -> Spawned {
        masked.merge(&self.started.masked);
        self.started.spawned.clone()
    }

    /// The sub-agent file name, or empty for the session itself.
    pub fn subagent(&self) -> String {
        self.file.subagent.clone().unwrap_or_default()
    }
}

/// How the sub-agent of a file was started, with its description masked
/// once for all its records; empty for a session's own transcript.
#[derive(Default)]
pub struct Started {
    /// What masking the description counted.
    pub masked: Counts,
    /// Whether masking changed the description.
    pub redacted: bool,
    /// The keys its records carry.
    pub spawned: Spawned,
}

impl Started {
    /// `spawn` with its description masked by `mask`. The type is masked
    /// with the other labels when a record is sealed.
    pub fn of(spawn: Spawn, mask: &Mask) -> Self {
        let mut masked = Counts::default();
        let (description, redacted) = mask.text(&spawn.description, &mut masked);
        Self {
            masked,
            redacted,
            spawned: Spawned {
                subagent_type: spawn.agent_type,
                subagent_description: description,
                parent_tool_use_id: spawn.tool_use_id,
            },
        }
    }

    /// How the sub-agent of `file` was started, read from its meta file
    /// when `reading` asks for it, with a warning when that file cannot be
    /// used; empty for a session's own transcript.
    fn read(file: &SessionFile, reading: Reading, mask: &Mask, warnings: &mut Vec<String>) -> Self {
        if !reading.spawns || file.parent.is_none() {
            return Self::default();
        }
        match walk::read_spawn(&file.path) {
            Ok(spawn) => Self::of(spawn, mask),
            Err(warning) => {
                warnings.push(warning_line(&warning.path, &warning.error.to_string()));
                Self::default()
            }
        }
    }
}

/// Reads every transcript under `root` and keeps the records that
/// `new_picker` picks and `query` lets through, reading them as `reading`
/// says. A file that cannot
/// be read is skipped with a warning, so one bad file does not hide the
/// rest.
///
/// # Errors
///
/// When `root` cannot be listed. A file whose modification time cannot be
/// shown is skipped with a warning.
pub fn collect<P, F>(
    root: &Path,
    reading: Reading,
    query: &Query,
    mask: &Mask,
    new_picker: F,
) -> Result<Corpus<P::Record>, Failure>
where
    P: Pick,
    F: Fn() -> P,
{
    let found = walk::find_sessions(root, reading.subagents).map_err(Failure::ListProjects)?;
    let mut warnings: Vec<String> = found
        .warnings
        .iter()
        .map(|warning| warning_line(&warning.path, &warning.error.to_string()))
        .collect();
    let mut skipped = Skipped::default();
    let mut troubled_files = 0_usize;
    let mut sessions = Vec::new();
    // The masked title of each session's own transcript, for its sub-agents.
    let mut titles: HashMap<PathBuf, (String, Counts)> = HashMap::new();
    // What masking a sub-agent's own title counted, kept until it is known
    // whether its session's title replaces it.
    let mut own_titles: HashMap<PathBuf, Counts> = HashMap::new();
    // The matches in each session's sub-agents, by the session's file,
    // which count toward the session.
    let mut delegated: HashMap<PathBuf, Vec<Mention>> = HashMap::new();
    for file in &found.sessions {
        // A time outside the supported years skips this file, not the run.
        let modified = match Stamp::from_system_time(file.modified) {
            Ok(stamp) => stamp,
            Err(failure) => {
                warnings.push(warning_line(&file.path, &failure.to_string()));
                continue;
            }
        };
        if query.range.starts_after(modified.instant()) {
            // Last written before the first day: nothing to find.
            continue;
        }
        // A sub-agent's transcript belongs to its parent's session.
        let session = file.parent.as_deref().unwrap_or(&file.path).file_stem();
        if !session.is_none_or(|id| query.may_hold_session(&id.to_string_lossy())) {
            continue;
        }
        let started = Started::read(file, reading, mask, &mut warnings);
        match read_file(file, &started, query, mask, new_picker(), reading) {
            Ok(mut read) => {
                if let Some(ref parent) = file.parent
                    && let Some(theirs) = read.mentions.take()
                {
                    delegated.entry(parent.clone()).or_default().extend(theirs);
                }
                if file.parent.is_some() {
                    own_titles.insert(file.path.clone(), read.title_masked);
                } else if reading.subagents && !read.title.is_empty() {
                    titles.insert(file.path.clone(), (read.title, read.title_masked));
                } else {
                    // No sub-agent reads this title.
                }
                if !read.skipped.is_empty() {
                    skipped.add(read.skipped);
                    troubled_files = troubled_files.saturating_add(1);
                }
                if !read.records.is_empty() {
                    sessions.push(FileRecords {
                        cwd: read.cwd,
                        file,
                        last: read.last,
                        modified,
                        mentions: read.mentions,
                        records: read.records,
                        usage: read.usage,
                    });
                }
            }
            Err(error) => warnings.push(warning_line(&file.path, &error)),
        }
    }
    warnings.extend(skipped_warning(skipped, troubled_files));
    for session in &mut sessions {
        inherit_title(session.file, &mut session.records, &titles, &own_titles);
        if let Some(theirs) = delegated.remove(&session.file.path)
            && let Some(ref mut mentions) = session.mentions
        {
            mentions.extend(theirs);
            // The first match of the session is the earliest of all.
            mentions.sort_by_key(Mention::instant);
        }
    }
    // Order sessions by their first record. A resumed copy starts with the
    // original's timestamps, so ties go to the older file, the original,
    // and only then to the path, keeping the output deterministic.
    sessions.sort_by(|left, right| {
        let first = |session: &FileRecords<'_, P::Record>| {
            session
                .records
                .first()
                .map(|pair| pair.1.head().timestamp.instant())
        };
        first(left)
            .cmp(&first(right))
            .then_with(|| left.file.modified.cmp(&right.file.modified))
            .then_with(|| walk::bytes(&left.file.path).cmp(walk::bytes(&right.file.path)))
    });
    // A warning quotes a path, which can hold a masked value.
    let shown = warnings
        .iter()
        .map(|warning| mask.meta(warning, &mut Counts::default()))
        .collect();
    Ok(gather(
        sessions,
        shown,
        mask,
        reading.around.unwrap_or_default(),
        &query.grep,
        reading.lang,
    ))
}

/// The warning for the lines `skipped` in `troubled_files` files, or `None`
/// when no file had any.
fn skipped_warning(skipped: Skipped, troubled_files: usize) -> Option<String> {
    (troubled_files > 0).then(|| {
        format!(
            "ccdig: warning: skipped {} undecodable, {} timestamp-less, and {} over-{}-MiB line(s) in {troubled_files} file(s)",
            skipped.undecodable,
            skipped.untimed,
            skipped.oversized,
            MAX_LINE_BYTES >> 20_u32,
        )
    })
}

/// A warning about `path`. Control characters are neutralized because the
/// message can quote a file name from someone else's archive.
fn warning_line(path: &Path, error: &str) -> String {
    clean_text(&format!(
        "ccdig: warning: skipping {}: {error}",
        path.display()
    ))
}

#[cfg(test)]
mod tests;
