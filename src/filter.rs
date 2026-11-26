//! The filters every subcommand shares, and matching a record against them.

use jiff::Timestamp;
use regex::{Regex, RegexBuilder};

use crate::failure::Failure;
use crate::record::Touch;
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
    /// Why a tool call did not run, empty when it ran.
    pub denial: Option<&'record str>,
    /// Whether the tool reported a failure.
    pub is_error: Option<bool>,
    /// How a prompt was typed.
    pub kind: Option<&'record str>,
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
    /// What a tool call changed.
    pub touched: Option<&'record Touch>,
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
    /// `--denial`: exact reasons a call did not run.
    pub denials: Filter<String>,
    /// `--grep` and `--igrep`, compiled by `patterns`.
    pub grep: Filter<Regex>,
    /// `--status`: only failed tool results when `true`, only the others
    /// when `false`.
    pub is_error: Option<bool>,
    /// `--kind`: exact kinds of prompt.
    pub kinds: Filter<String>,
    /// `--project`: substrings of the project directory or the cwd.
    pub projects: Filter<String>,
    /// `--since` and `--until`.
    pub range: DayRange,
    /// `--session`: session ID prefixes.
    pub sessions: Filter<String>,
    /// `--skill`: exact skill names.
    pub skills: Filter<String>,
    /// `--superseded`: only the prompts sent again in place of when
    /// `true`, only the others when `false`. A file tells it only once it
    /// is read to the end, so `admits` does not see it.
    pub superseded: Option<bool>,
    /// `--tool`: exact tool names.
    pub tools: Filter<String>,
    /// `--touched`: paths, matched part by part by `touches`.
    pub touched: Filter<String>,
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
        self.included(&test) && !self.excluded(test)
    }

    /// Whether a record for which `test` tells which values it matches
    /// matches a value given with a leading `!`.
    fn excluded<F>(&self, test: F) -> bool
    where
        F: Fn(&T) -> bool,
    {
        self.exclude.iter().any(test)
    }

    /// Whether a record for which `test` tells which values it matches
    /// matches a value given without a leading `!`, or none was given.
    fn included<F>(&self, test: F) -> bool
    where
        F: Fn(&T) -> bool,
    {
        self.include.is_empty() || self.include.iter().any(test)
    }

    /// Whether no value was given.
    pub const fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }
}

impl Filter<String> {
    /// Whether a record with `name` passes, matched exactly. A kind
    /// without the name (`None`) passes, so the kinds can be mixed.
    pub fn allows_name(&self, name: Option<&str>) -> bool {
        name.is_none_or(|have| self.allows(|want| want == have))
    }

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
        self.excluded(|pattern| pattern.is_match(text))
    }

    /// Whether `text` matches a pattern given without a leading `!`, or
    /// none was given.
    pub fn includes(&self, text: &str) -> bool {
        self.included(|pattern| pattern.is_match(text))
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
            && self.skills.allows_name(facts.skill)
            && self.tools.allows_name(facts.tool)
            && self.denials.allows_name(facts.denial)
            && self.kinds.allows_name(facts.kind)
            && facts.touched.is_none_or(|touch| self.admits_touch(touch))
            && self
                .is_error
                .is_none_or(|want| facts.is_error.is_none_or(|failed| failed == want))
    }

    /// Whether a call that `touch` tells of passes `--touched`: one that
    /// changed nothing passes only without it, so a leading `!` leaves a
    /// path out of the calls that change one.
    fn admits_touch(&self, touch: &Touch) -> bool {
        self.touched.is_empty()
            || (*touch != Touch::Nothing && self.touched.allows(|want| touch_matches(touch, want)))
    }

    /// Whether the project directory's name or the cwd contains a
    /// `--project` value. The directory name is how Claude Code encodes
    /// the cwd (`/` and `.` become `-`), so both spellings work.
    pub fn matches_project(&self, project: &str, cwd: &str) -> bool {
        self.projects
            .allows(|filter| project.contains(filter.as_str()) || cwd.contains(filter.as_str()))
    }

    /// Whether a transcript of the session `id` can hold a record
    /// `--session` lets through, so a file of another session need not be
    /// opened. Claude Code names a transcript after its session and writes
    /// that ID on each of its lines, including the lines it copies into a
    /// resumed session. A value with a leading `!` cannot rule a file out,
    /// as other files are still read.
    pub fn may_hold_session(&self, id: &str) -> bool {
        self.sessions.include.is_empty()
            || self
                .sessions
                .include
                .iter()
                .any(|want| id.starts_with(want.as_str()))
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

/// The parts of `path` between its slashes, or the backslashes of a
/// Windows path, without the empty ones and `.`.
fn path_parts(path: &str) -> Vec<&str> {
    path.split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .collect()
}

/// Whether `value` has a part `touches` can match, one other than `/` and
/// `.`.
pub fn names_a_path(value: &str) -> bool {
    !path_parts(value).is_empty()
}

/// Whether `path` is or is under `want`, a `--touched` value: the parts of
/// `want` appear in a row among the parts of `path`, from the root when
/// `want` is absolute. A part is matched whole, so `src/cli` is not in
/// `src/client`.
pub fn touches(want: &str, path: &str) -> bool {
    let wanted = path_parts(want);
    let parts = path_parts(path);
    if wanted.is_empty() {
        return false;
    }
    if want.starts_with('/') {
        return path.starts_with('/') && parts.starts_with(&wanted);
    }
    parts
        .windows(wanted.len())
        .any(|run| run == wanted.as_slice())
}

/// Whether a word of the shell command `command` is a path `touches`
/// `want`. The words are split at spaces, quotes, and the characters that
/// end a word in a command line, such as `;`, `|`, `>`, and the `=` of an
/// option, so a redirect or a `--out=FILE` is read too, and at `{`, `}`,
/// `,`, and `:`, for a brace expansion, a list, or a `FILE:LINE`.
pub fn command_touches(want: &str, command: &str) -> bool {
    command
        .split(|character: char| {
            character.is_whitespace() || "'\";|&<>()=`{},:".contains(character)
        })
        .any(|word| touches(want, word))
}

/// Whether `touch` changed `want`, or names it in a shell command.
fn touch_matches(touch: &Touch, want: &str) -> bool {
    match *touch {
        Touch::Command(ref command) => command_touches(want, command),
        Touch::Nothing => false,
        Touch::Path(ref path) => touches(want, path),
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
