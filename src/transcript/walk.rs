//! Finding the session transcripts under the history root.

use std::fs::{self, DirEntry};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The transcripts under a root, and what could not be listed.
#[derive(Debug, Default)]
pub struct Found {
    /// The transcripts, sorted by the bytes of their paths.
    pub sessions: Vec<SessionFile>,
    /// Directories and files that were skipped.
    pub warnings: Vec<Warning>,
}

/// One transcript found under the history root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionFile {
    /// When the file was last written to.
    pub modified: SystemTime,
    /// The transcript file.
    pub path: PathBuf,
    /// The project directory's name, such as `-Users-alice-blog`; Claude
    /// Code encodes the cwd this way.
    pub project: String,
    /// The file name without `.jsonl`, such as `agent-1a2b`, for a
    /// sub-agent's transcript; `None` for the session itself.
    pub subagent: Option<String>,
}

/// A directory or file that was skipped, and why.
#[derive(Debug)]
pub struct Warning {
    /// What went wrong.
    pub error: io::Error,
    /// The directory or file.
    pub path: PathBuf,
}

impl Found {
    /// Adds `entry` when it is a regular `.jsonl` file, as a sub-agent's
    /// transcript when `subagent` is set.
    fn add(&mut self, entry: &DirEntry, project: &str, subagent: bool) {
        if !is(entry, fs::FileType::is_file) {
            return;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = file_name.strip_suffix(".jsonl") else {
            return;
        };
        let path = entry.path();
        let modified = match entry.metadata().and_then(|metadata| metadata.modified()) {
            Ok(time) => time,
            Err(error) => {
                self.warn(&path, error);
                return;
            }
        };
        self.sessions.push(SessionFile {
            modified,
            path,
            project: project.to_owned(),
            subagent: subagent.then(|| stem.to_owned()),
        });
    }

    /// Adds the sub-agent transcripts in `dir`, which most sessions do not
    /// have. A `dir` that is a symlink is skipped.
    fn add_subagents(&mut self, dir: &Path, project: &str) {
        // read_dir follows a symlink, which could lead outside the root.
        match fs::symlink_metadata(dir) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => return,
            Err(error) if error.kind() == ErrorKind::NotFound => return,
            Err(error) => {
                self.warn(dir, error);
                return;
            }
        }
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return,
            Err(error) => {
                self.warn(dir, error);
                return;
            }
        };
        for item in entries {
            match item {
                Ok(entry) => self.add(&entry, project, true),
                Err(error) => self.warn(dir, error),
            }
        }
    }

    /// The entries of `dir`, or `None` with a warning when it cannot be
    /// listed.
    fn list(&mut self, dir: &Path) -> Option<Vec<DirEntry>> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => {
                self.warn(dir, error);
                return None;
            }
        };
        let mut listed = Vec::new();
        for item in entries {
            match item {
                Ok(entry) => listed.push(entry),
                Err(error) => self.warn(dir, error),
            }
        }
        Some(listed)
    }

    /// Records that `path` was skipped.
    fn warn(&mut self, path: &Path, error: io::Error) {
        self.warnings.push(Warning {
            error,
            path: path.to_owned(),
        });
    }
}

/// The bytes of `path`, which compare the way Go compares path strings.
pub fn bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
}

/// Lists the transcripts under `root`.
///
/// Claude Code stores a session as `root/<project>/<session>.jsonl` and its
/// sub-agents as `root/<project>/<session>/subagents/<agent>.jsonl`. Other
/// deeper files, such as `tool-results/`, are not transcripts. The
/// sub-agents are listed only when `subagents` is set. Only regular files
/// are listed: a symlink could point outside `root`, and a FIFO would
/// block.
///
/// # Errors
///
/// When `root` cannot be listed. A project directory that cannot be listed,
/// or a file that vanishes while listing (a live session being cleaned
/// up), is a warning instead.
pub fn find_sessions(root: &Path, subagents: bool) -> io::Result<Found> {
    let mut found = Found::default();
    for item in fs::read_dir(root)? {
        let project = match item {
            Ok(project) => project,
            Err(error) => {
                found.warn(root, error);
                continue;
            }
        };
        if !is(&project, fs::FileType::is_dir) {
            continue;
        }
        let name = project.file_name().to_string_lossy().into_owned();
        let dir = project.path();
        let Some(entries) = found.list(&dir) else {
            continue;
        };
        for entry in entries {
            if subagents && is(&entry, fs::FileType::is_dir) {
                found.add_subagents(&entry.path().join("subagents"), &name);
            } else {
                found.add(&entry, &name, false);
            }
        }
    }
    // By the whole path, not per directory: "a-b/x" sorts before "a/x".
    found
        .sessions
        .sort_by(|left, right| bytes(&left.path).cmp(bytes(&right.path)));
    Ok(found)
}

/// Whether `entry`, not following symlinks, has the type `test` checks.
fn is(entry: &DirEntry, test: fn(&fs::FileType) -> bool) -> bool {
    entry.file_type().is_ok_and(|kind| test(&kind))
}

#[cfg(test)]
mod tests;
