//! Reading every transcript under the root into the records a picker picks:
//! list, read, pick, mask, filter, and drop resumed copies.

pub mod kinds;

use alloc::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::failure::Failure;
use crate::redact::{ExtraMasks, Redactor};
use crate::render::record::{AGENT, Head, Record, Session};
use crate::select::{Facts, Query};
use crate::text::clean_text;
use crate::transcript::entry::{Entry, Role};
use crate::transcript::reader::{self, MAX_LINE_BYTES, Roles};
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

/// A file's selected records with its modification time, each with its
/// block's index in the line.
type FileRecords<'file, T> = (&'file SessionFile, Stamp, Vec<(usize, T)>);

/// The masking applied to what is printed: the built-in rules and the masks
/// `--mask` and `--mask-file` ask for, or none under `--no-mask`.
pub struct Mask(Option<Redactor>);

/// One kind of record and how it is picked out of transcript lines. A
/// picker is made for each file, so it can remember what it saw earlier in
/// the file.
pub trait Pick {
    /// The record it makes.
    type Record: Select;

    /// Makes the records of an entry that `wants` accepted, each with its
    /// block's index in the line.
    fn pick(&self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, Self::Record)>;

    /// The entries the reader should deliver.
    fn roles(&self) -> Roles;

    /// Whether `entry` holds records of this kind. It sees every delivered
    /// entry in file order, so it can also take notes.
    fn wants(&mut self, entry: &Entry) -> bool;
}

/// What a record offers to the collector besides its shared keys.
pub trait Select: Record {
    /// The kind's own facts for the filters.
    fn extra(&self) -> Extra<'_>;

    /// What `--grep` searches, built only when `--grep` is given.
    fn grep_text(&self) -> Cow<'_, str>;

    /// The shared keys, to fill in the session title.
    fn head_mut(&mut self) -> &mut Head;

    /// The kind's own fields that are not text, such as the tool's name,
    /// for the masks `--mask-file` asks for.
    fn labels_mut(&mut self) -> Vec<&mut String>;
}

/// The facts the filters see that only some kinds have, `None` for a kind
/// without them.
pub struct Extra<'record> {
    /// Whether a tool reported a failure.
    pub is_error: Option<bool>,
    /// The skill running at the time.
    pub skill: Option<&'record str>,
    /// The tool's name.
    pub tool: Option<&'record str>,
}

/// Where an entry is, for building its records.
pub struct Spot<'file> {
    /// The transcript file.
    pub file: &'file SessionFile,
    /// The 1-based line number.
    pub line: usize,
    /// The masking to apply.
    pub mask: &'file Mask,
    /// When the line was written.
    pub timestamp: Stamp,
}

/// What a transcript file yielded.
struct Read<T> {
    /// The selected records, in file order.
    records: Vec<(usize, T)>,
    /// The lines that were skipped.
    skipped: Skipped,
    /// The session title, masked; empty when the file has none.
    title: String,
}

/// Lines skipped across files, for the summary warning.
#[derive(Clone, Copy, Debug, Default)]
struct Skipped {
    /// Lines over the limit.
    oversized: usize,
    /// Lines that did not decode.
    undecodable: usize,
    /// Wanted entries without a timestamp, which cannot be placed in a
    /// range or ordered.
    untimed: usize,
}

impl Mask {
    /// Masks every string in `value`, reporting whether anything changed.
    pub fn json(&self, value: &mut Value) -> bool {
        self.0
            .as_ref()
            .is_some_and(|rules| rules.redact_json(value))
    }

    /// Applies the masks `--mask` and `--mask-file` ask for to `field`, a
    /// path, a branch, a tool's name, or another field that is not text.
    pub fn meta(&self, field: &str) -> String {
        self.0
            .as_ref()
            .map_or_else(|| field.to_owned(), |rules| rules.meta(field))
    }

    /// The built-in rules and the masks in `extra`, or none when `off`.
    ///
    /// # Errors
    ///
    /// When the rules do not compile, which is a bug.
    pub fn new(off: bool, extra: ExtraMasks) -> Result<Self, Failure> {
        if off {
            return Ok(Self(None));
        }
        Redactor::with(extra)
            .map(|rules| Self(Some(rules)))
            .map_err(Failure::Rules)
    }

    /// Masks `text`, reporting whether anything changed.
    pub fn text(&self, text: &str) -> (String, bool) {
        self.0
            .as_ref()
            .map_or_else(|| (text.to_owned(), false), |rules| rules.redact(text))
    }
}

impl Skipped {
    /// Whether nothing was skipped.
    const fn is_empty(&self) -> bool {
        self.oversized == 0 && self.untimed == 0 && self.undecodable == 0
    }
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
            project: self.file.project.clone(),
            cwd: entry.cwd.clone(),
            git_branch: entry.git_branch.clone(),
            timestamp: self.timestamp,
        }
    }

    /// The sub-agent file name, or empty for the session itself.
    pub fn subagent(&self) -> String {
        self.file.subagent.clone().unwrap_or_default()
    }
}

/// Reads every transcript under `root` and keeps the records that
/// `new_picker` picks and `query` lets through. A file that cannot be read
/// is skipped with a warning, so one bad file does not hide the rest.
///
/// # Errors
///
/// When `root` cannot be listed. A file whose modification time cannot be
/// shown is skipped with a warning.
pub fn collect<P, F>(
    root: &Path,
    subagents: bool,
    query: &Query,
    mask: &Mask,
    new_picker: F,
) -> Result<Corpus<P::Record>, Failure>
where
    P: Pick,
    F: Fn() -> P,
{
    let found = walk::find_sessions(root, subagents).map_err(Failure::ListProjects)?;
    let mut warnings: Vec<String> = found
        .warnings
        .iter()
        .map(|warning| warning_line(&warning.path, &warning.error.to_string()))
        .collect();
    let mut skipped = Skipped::default();
    let mut troubled_files = 0_usize;
    let mut sessions = Vec::new();
    // The masked title of each session's own transcript, for its sub-agents.
    let mut titles: HashMap<PathBuf, String> = HashMap::new();
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
        match read_file(file, query, mask, new_picker()) {
            Ok(read) => {
                if subagents && file.parent.is_none() && !read.title.is_empty() {
                    titles.insert(file.path.clone(), read.title);
                }
                if !read.skipped.is_empty() {
                    skipped.oversized = skipped.oversized.saturating_add(read.skipped.oversized);
                    skipped.untimed = skipped.untimed.saturating_add(read.skipped.untimed);
                    skipped.undecodable =
                        skipped.undecodable.saturating_add(read.skipped.undecodable);
                    troubled_files = troubled_files.saturating_add(1);
                }
                if !read.records.is_empty() {
                    sessions.push((file, modified, read.records));
                }
            }
            Err(error) => warnings.push(warning_line(&file.path, &error)),
        }
    }
    if troubled_files > 0 {
        warnings.push(format!(
            "ccdig: warning: skipped {} undecodable, {} timestamp-less, and {} over-{}-MiB line(s) in {troubled_files} file(s)",
            skipped.undecodable,
            skipped.untimed,
            skipped.oversized,
            MAX_LINE_BYTES >> 20_u32,
        ));
    }
    for &mut (file, _, ref mut records) in &mut sessions {
        inherit_title(file, records, &titles);
    }
    // Order sessions by their first record. A resumed copy starts with the
    // original's timestamps, so ties go to the older file, the original,
    // and only then to the path, keeping the output deterministic.
    sessions.sort_by(|&(left_file, _, ref left), &(right_file, _, ref right)| {
        let first = |records: &[(usize, P::Record)]| {
            records
                .first()
                .map(|pair| pair.1.head().timestamp.instant())
        };
        first(left)
            .cmp(&first(right))
            .then_with(|| left_file.modified.cmp(&right_file.modified))
            .then_with(|| walk::bytes(&left_file.path).cmp(walk::bytes(&right_file.path)))
    });
    // A warning quotes a path, which can hold a masked value.
    let shown = warnings.iter().map(|warning| mask.meta(warning)).collect();
    Ok(gather(sessions, shown, mask))
}

/// Keeps each record once across sessions, in session order, and builds
/// the session records. A resumed or forked session starts with a copy of
/// earlier entries, IDs included; the first session in order wins.
fn gather<T>(sessions: Vec<FileRecords<'_, T>>, warnings: Vec<String>, mask: &Mask) -> Corpus<T>
where
    T: Select,
{
    let mut seen = HashSet::new();
    let mut corpus = Corpus {
        records: Vec::new(),
        sessions: Vec::new(),
        warnings,
    };
    for (file, modified, records) in sessions {
        let kept: Vec<T> = records
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
        let head = first.head();
        corpus.sessions.push(Session {
            agent: AGENT,
            id: head.session_id.clone(),
            title: head.session_title.clone(),
            project: head.project.clone(),
            cwd: head.cwd.clone(),
            git_branch: head.git_branch.clone(),
            file: mask.meta(&file.path.to_string_lossy()),
            // UTC keeps the output the same on every machine.
            mtime: modified,
            started_at: head.timestamp,
            ended_at: last.head().timestamp,
            prompt_count: kept.len(),
        });
        corpus.records.extend(kept);
    }
    corpus
}

/// Gives the records of a sub-agent's transcript the title of its session,
/// as a sub-agent's transcript has no `ai-title` line of its own. The
/// session's transcript must have been read in the same run, so one last
/// written before `--since` leaves the title empty.
fn inherit_title<T>(
    file: &SessionFile,
    records: &mut [(usize, T)],
    titles: &HashMap<PathBuf, String>,
) where
    T: Select,
{
    let Some(title) = file.parent.as_ref().and_then(|parent| titles.get(parent)) else {
        return;
    };
    for pair in records {
        pair.1.head_mut().session_title.clone_from(title);
    }
}

/// Reads one transcript and returns its selected records in file order.
fn read_file<P>(
    file: &SessionFile,
    query: &Query,
    mask: &Mask,
    mut picker: P,
) -> Result<Read<P::Record>, String>
where
    P: Pick,
{
    let mut lines = reader::open(&file.path, MAX_LINE_BYTES, picker.roles())
        .map_err(|error| format!("open transcript: {error}"))?;
    let mut title = String::new();
    let mut records = Vec::new();
    let mut untimed = 0_usize;
    for item in &mut lines {
        let (line, entry) = item.map_err(|error| format!("read transcript: {error}"))?;
        if entry.role == Role::AiTitle {
            // The title is regenerated as a session goes on; the last wins.
            if !entry.title.is_empty() {
                title = entry.title;
            }
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
        records.extend(
            picker
                .pick(&spot, &entry)
                .into_iter()
                .filter(|pair| passes(query, &pair.1)),
        );
    }
    // Titles are generated from the conversation, so a pasted secret can
    // end up in one.
    let (masked_title, _) = mask.text(&title);
    for pair in &mut records {
        let head = pair.1.head_mut();
        head.session_title.clone_from(&masked_title);
        // After the filters, so --project still matches the real paths.
        head.cwd = mask.meta(&head.cwd);
        head.git_branch = mask.meta(&head.git_branch);
        head.project = mask.meta(&head.project);
        for label in pair.1.labels_mut() {
            *label = mask.meta(label);
        }
    }
    let stats = lines.stats();
    Ok(Read {
        records,
        skipped: Skipped {
            oversized: stats.oversized,
            undecodable: stats.undecodable,
            untimed,
        },
        title: masked_title,
    })
}

/// Whether `record` passes the filters in `query`.
fn passes<T>(query: &Query, record: &T) -> bool
where
    T: Select,
{
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

/// A warning about `path`. Control characters are neutralized because the
/// message can quote a file name from someone else's archive.
fn warning_line(path: &Path, error: &str) -> String {
    clean_text(&format!(
        "ccdig: warning: skipping {}: {error}",
        path.display()
    ))
}
