//! The filters every subcommand shares, and matching a record against them.

use jiff::Timestamp;
use regex::{Regex, RegexBuilder};

use crate::failure::Failure;
use crate::when::DayRange;

/// What a record offers to the filters. A kind without a skill, a tool, or
/// an error flag has `None` there, and the filter on it lets the record
/// through, so the kinds can be mixed.
#[derive(Clone, Copy, Debug)]
pub struct Facts<'record> {
    /// The git branch checked out at the time.
    pub branch: &'record str,
    /// The working directory.
    pub cwd: &'record str,
    /// Whether the tool reported a failure.
    pub is_error: Option<bool>,
    /// The project directory's name.
    pub project: &'record str,
    /// The session ID.
    pub session_id: &'record str,
    /// The skill running at the time.
    pub skill: Option<&'record str>,
    /// What `--grep` searches: the text as it will be printed, so after
    /// masking unless masking is off.
    pub text: &'record str,
    /// When the record was written.
    pub timestamp: Timestamp,
    /// The tool's name.
    pub tool: Option<&'record str>,
}

/// The values of one filter flag. A record passes when it matches one of
/// `include`, or `include` is empty, and none of `exclude`.
#[derive(Clone, Debug)]
pub struct Filter<T> {
    /// The values given with a leading `!`, without it.
    pub exclude: Vec<T>,
    /// The other values.
    pub include: Vec<T>,
}

/// What the filters make of a record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    /// It passes every filter: `--grep` or `--igrep` matched it, or none
    /// was given without a leading `!`.
    Hit,
    /// It passes every filter but that no including pattern matched it,
    /// so `--around` can keep it next to a match.
    Near,
    /// A filter other than the including patterns left it out.
    Out,
}

/// The filters given on the command line. An empty filter or `None` lets
/// everything through.
#[derive(Clone, Debug, Default)]
pub struct Query {
    /// `--branch`: exact branch names.
    pub branches: Filter<String>,
    /// `--grep` and `--igrep`, compiled by `patterns`.
    pub grep: Filter<Regex>,
    /// `--status`: only failed tool results when `true`, only the others
    /// when `false`.
    pub is_error: Option<bool>,
    /// `--project`: substrings of the project directory or the cwd.
    pub projects: Filter<String>,
    /// `--since` and `--until`.
    pub range: DayRange,
    /// `--session`: session ID prefixes.
    pub sessions: Filter<String>,
    /// `--skill`: exact skill names.
    pub skills: Filter<String>,
    /// `--tool`: exact tool names.
    pub tools: Filter<String>,
}

impl<T> Default for Filter<T> {
    fn default() -> Self {
        Self {
            exclude: Vec::new(),
            include: Vec::new(),
        }
    }
}

impl<T> Filter<T> {
    /// Whether a record for which `test` tells which values it matches
    /// passes.
    pub fn allows<F>(&self, test: F) -> bool
    where
        F: Fn(&T) -> bool,
    {
        (self.include.is_empty() || self.include.iter().any(&test))
            && !self.exclude.iter().any(test)
    }

    /// Whether no value was given.
    pub const fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }
}

impl Filter<String> {
    /// The values of a name flag: one with a leading `!` excludes, and a
    /// leading `\!` stands for a `!` that does not.
    pub fn names(values: Vec<String>) -> Self {
        let mut filter = Self::default();
        for value in values {
            if let Some(name) = value.strip_prefix('!') {
                filter.exclude.push(name.to_owned());
            } else if let Some(name) = value.strip_prefix("\\!") {
                filter.include.push(format!("!{name}"));
            } else {
                filter.include.push(value);
            }
        }
        filter
    }
}

impl Filter<Regex> {
    /// Whether `text` matches a pattern given with a leading `!`.
    pub fn excludes(&self, text: &str) -> bool {
        self.exclude.iter().any(|pattern| pattern.is_match(text))
    }

    /// Whether `text` matches a pattern given without a leading `!`, or
    /// none was given.
    pub fn includes(&self, text: &str) -> bool {
        self.include.is_empty() || self.include.iter().any(|pattern| pattern.is_match(text))
    }
}

impl Query {
    /// Whether a record with `facts` passes every filter but `--grep` and
    /// `--igrep`.
    pub fn admits(&self, facts: &Facts<'_>) -> bool {
        self.range.contains(facts.timestamp)
            && self.matches_project(facts.project, facts.cwd)
            && self
                .sessions
                .allows(|id| facts.session_id.starts_with(id.as_str()))
            && self.branches.allows(|branch| branch == facts.branch)
            && facts
                .skill
                .is_none_or(|skill| self.skills.allows(|want| want == skill))
            && facts
                .tool
                .is_none_or(|tool| self.tools.allows(|want| want == tool))
            && self
                .is_error
                .is_none_or(|want| facts.is_error.is_none_or(|failed| failed == want))
    }

    /// Whether the project directory's name or the cwd contains a
    /// `--project` value. The directory name is how Claude Code encodes
    /// the cwd (`/` and `.` become `-`), so both spellings work.
    pub fn matches_project(&self, project: &str, cwd: &str) -> bool {
        self.projects
            .allows(|filter| project.contains(filter.as_str()) || cwd.contains(filter.as_str()))
    }

    /// What the filters make of a record with `facts`.
    pub fn verdict(&self, facts: &Facts<'_>) -> Verdict {
        if !self.admits(facts) || self.grep.excludes(facts.text) {
            Verdict::Out
        } else if self.grep.includes(facts.text) {
            Verdict::Hit
        } else {
            Verdict::Near
        }
    }
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

/// Compiles the values of `--grep` and of `--igrep`, which ignores case.
/// One with a leading `!` excludes; a pattern that starts with `\!`
/// matches a `!`, as the regex crate reads `\!` as `!`.
///
/// # Errors
///
/// When a pattern is not a valid regular expression.
pub fn patterns<S>(grep: &[S], igrep: &[S]) -> Result<Filter<Regex>, Failure>
where
    S: AsRef<str>,
{
    let mut filter = Filter::default();
    for (values, ignore_case) in [(grep, false), (igrep, true)] {
        for value in values.iter().map(AsRef::as_ref) {
            match value.strip_prefix('!') {
                Some(pattern) => filter.exclude.push(compile_grep(pattern, ignore_case)?),
                None => filter.include.push(compile_grep(value, ignore_case)?),
            }
        }
    }
    Ok(filter)
}

#[cfg(test)]
mod tests;
