//! The filters every subcommand shares, and matching a record against them.

use jiff::Timestamp;
use regex::{Regex, RegexBuilder};

use crate::failure::Failure;
use crate::when::DayRange;

/// What a record offers to the filters. Kinds without a tool or an error
/// flag leave them empty; the subcommands that read those kinds are the
/// only ones with the matching flags.
#[derive(Clone, Copy, Debug)]
pub struct Facts<'record> {
    /// The git branch checked out at the time.
    pub branch: &'record str,
    /// The working directory.
    pub cwd: &'record str,
    /// Whether the tool reported a failure.
    pub is_error: bool,
    /// The project directory's name.
    pub project: &'record str,
    /// The session ID.
    pub session_id: &'record str,
    /// The skill running at the time.
    pub skill: &'record str,
    /// What `--grep` searches: the text as it will be printed, so after
    /// masking unless masking is off.
    pub text: &'record str,
    /// When the record was written.
    pub timestamp: Timestamp,
    /// The tool's name.
    pub tool: &'record str,
}

/// The filters given on the command line. An empty list or `None` lets
/// everything through.
#[derive(Clone, Debug, Default)]
pub struct Query {
    /// `--branch`: exact branch names.
    pub branches: Vec<String>,
    /// `--errors`: only failed tool calls.
    pub errors: bool,
    /// `--grep`, compiled by `compile_grep`.
    pub grep: Option<Regex>,
    /// `--project`: substrings of the project directory or the cwd.
    pub projects: Vec<String>,
    /// `--since` and `--until`.
    pub range: DayRange,
    /// `--session`: session ID prefixes.
    pub sessions: Vec<String>,
    /// `--skill`: the exact skill name.
    pub skill: Option<String>,
    /// `--tool`: exact tool names.
    pub tools: Vec<String>,
}

impl Query {
    /// Whether a record with `facts` passes every filter.
    pub fn matches(&self, facts: &Facts<'_>) -> bool {
        self.range.contains(facts.timestamp)
            && self.matches_project(facts.project, facts.cwd)
            && any_or_none(&self.sessions, |id| facts.session_id.starts_with(id))
            && any_or_none(&self.branches, |branch| branch == facts.branch)
            && self
                .skill
                .as_deref()
                .is_none_or(|skill| skill == facts.skill)
            && any_or_none(&self.tools, |tool| tool == facts.tool)
            && (!self.errors || facts.is_error)
            && self
                .grep
                .as_ref()
                .is_none_or(|pattern| pattern.is_match(facts.text))
    }

    /// Whether the project directory's name or the cwd contains any of the
    /// `--project` filters. The directory name is how Claude Code encodes
    /// the cwd (`/` and `.` become `-`), so both spellings work.
    pub fn matches_project(&self, project: &str, cwd: &str) -> bool {
        any_or_none(&self.projects, |filter| {
            project.contains(filter.as_str()) || cwd.contains(filter.as_str())
        })
    }
}

/// Whether `filters` is empty or any of them passes `test`.
fn any_or_none(filters: &[String], test: impl Fn(&String) -> bool) -> bool {
    filters.is_empty() || filters.iter().any(test)
}

/// Compiles the `--grep` pattern, ignoring case when `ignore_case` is set.
/// The regex crate runs in linear time, so no pattern can stall a search.
///
/// # Errors
///
/// When `pattern` is not a valid regular expression.
pub fn compile_grep(pattern: &str, ignore_case: bool) -> Result<Regex, Failure> {
    RegexBuilder::new(pattern)
        .case_insensitive(ignore_case)
        .build()
        .map_err(Failure::Grep)
}

#[cfg(test)]
mod tests;
