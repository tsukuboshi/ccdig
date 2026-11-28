//! The records that flow between the stages: `collect` builds them, and
//! `render` writes them. Every key is written, in declaration order, unless
//! `--fields` leaves it out, so consumers can rely on the keys.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::redact::Counts;
use crate::text::{Lang, Span, cut_span, cut_text};
use crate::transcript::entry::Usage;
use crate::when::Stamp;

/// The `agent` of every record.
pub const AGENT: &str = "claude-code";

/// How many characters `first_match` keeps of the line.
pub const FIRST_MATCH_CHARS: usize = 200;

/// The keys a line of `records` can have, in the order the kinds write
/// them, for `--fields`.
pub const RECORD_KEYS: [&str; 32] = [
    "type",
    "agent",
    "session_id",
    "session_title",
    "session_title_then",
    "project",
    "cwd",
    "git_branch",
    "timestamp",
    "turn",
    "tool",
    "input",
    "tool_use_id",
    "result",
    "is_error",
    "denial",
    "text",
    "kind",
    "command",
    "permission_mode",
    "superseded",
    "model",
    "skill",
    "subagent",
    "subagent_type",
    "subagent_description",
    "parent_tool_use_id",
    "usage",
    "redacted",
    "source",
    "matched",
    "matches",
];

/// The keys of a line of `sessions`, in order, for `--fields`.
pub const SESSION_KEYS: [&str; 16] = [
    "agent",
    "session_id",
    "title",
    "project",
    "cwd",
    "git_branch",
    "file",
    "mtime",
    "started_at",
    "ended_at",
    "prompt_count",
    "match_count",
    "first_match",
    "usage",
    "resume",
    "resume_unavailable",
];

/// The keys a line of `stats` can have, for `--fields`: the `--by` keys,
/// in the order `--by` names them, then the rest in this order.
pub const STATS_KEYS: [&str; 23] = [
    "session",
    "project",
    "branch",
    "type",
    "tool",
    "skill",
    "subagent",
    "model",
    "kind",
    "denial",
    "status",
    "day",
    "week",
    "month",
    "session_title",
    "file",
    "count",
    "empty",
    "chars",
    "sessions",
    "first",
    "last",
    "usage",
];

/// Which characters `--message-chars` and `--tool-chars` keep of each
/// string, `None` for all of them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cuts {
    /// The text of a prompt, a reply, or a summary.
    pub message: Option<Span>,
    /// Each string in a tool's input, and a tool result's text.
    pub tool: Option<Span>,
}

/// The keys every kind of record starts with.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct Head {
    /// Always `AGENT`.
    pub agent: &'static str,
    /// The session ID; a sub-agent's records carry the parent's.
    pub session_id: String,
    /// The session's last title, or empty.
    pub session_title: String,
    /// The title the session had when the line was written: the last one
    /// Claude Code wrote before it, or empty before the first. Claude Code
    /// writes a title after the prompt it comes from, so the prompt that
    /// changes the title carries the one before. A sub-agent's records
    /// carry the session's last title.
    pub session_title_then: String,
    /// The project directory's name.
    pub project: String,
    /// The working directory.
    pub cwd: String,
    /// The git branch checked out at the time.
    pub git_branch: String,
    /// When the line was written.
    pub timestamp: Stamp,
    /// How many prompts of kind `text` or `slash` the transcript file has
    /// up to the line, in the order written, those sent again in place of
    /// another included, so a record shares the number of the prompt it
    /// follows, and an answer to a dialog or a comment on a call turned
    /// down that of the prompt before it: 0 before the first, and in a
    /// sub-agent's transcript, which has none. Only `records` counts them.
    pub turn: usize,
    /// What masking these keys counted, for `--mask-report`; not written.
    #[serde(skip)]
    pub masked: Counts,
}

/// How a prompt was typed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The answers the user picked for Claude Code's questions from the
    /// options it offered.
    Answer,
    /// A plan the user approved.
    Approval,
    /// What the user typed when they turned a tool call down, or a plan
    /// they turned down without a comment.
    Rejection,
    /// A slash command, folded back into what was typed.
    Slash,
    /// Plain text.
    Text,
    /// The answers to Claude Code's questions when the user typed one of
    /// them in their own words rather than picking an option.
    #[serde(rename = "typed-answer")]
    TypedAnswer,
}

/// A line of a record that `--grep` matched, or one around it, for
/// `--context`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct MatchLine {
    /// The text the line is in: `text`, or for a tool call, where the
    /// string is in the input, such as `input.command`.
    pub field: String,
    /// The 1-based number of the line in that text.
    pub line: usize,
    /// The line, masked, without its line break.
    pub text: String,
    /// Whether `--grep` matched the line, rather than it being one around
    /// a matched line.
    pub hit: bool,
}

/// What the search made of a record, the last keys of every kind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Search {
    /// Whether `--grep` matched the record: `false` for one `--around`
    /// adds next to a match, `true` otherwise.
    pub matched: bool,
    /// The lines `--grep` matched and, with `--context`, those around
    /// them, in order: in JSON and JSON Lines whenever a pattern without a
    /// leading `!` is given, in Markdown only with `--context`. Empty
    /// otherwise, or when no single line matched.
    pub matches: Vec<MatchLine>,
}

/// A prompt a person typed: a `prompt` of `ccdig records`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct Prompt {
    /// The shared keys.
    #[serde(flatten)]
    pub head: Head,
    /// The prompt, trimmed and masked.
    pub text: String,
    /// Plain text, a slash command, or a comment on a tool call the user
    /// turned down.
    pub kind: Kind,
    /// The slash command's name, or empty for plain text.
    pub command: String,
    /// The permission mode the line was written in, such as `plan` or
    /// `auto`, or empty when the line does not say, as for slash commands.
    pub permission_mode: String,
    /// Whether the user sent another prompt in its place later in the
    /// file: one that follows the same line, as when they press Esc and
    /// edit the prompt, or interrupt the reply and send it again.
    pub superseded: bool,
    /// Whether masking changed the text.
    pub redacted: bool,
    /// Where the line is.
    pub source: Source,
    /// What the search made of the record.
    #[serde(flatten)]
    pub search: Search,
    /// What masking the kind's own fields counted, for `--mask-report`;
    /// not written.
    #[serde(skip)]
    pub masked: Counts,
    /// The ID of the line the prompt follows, for `superseded`; empty for
    /// a comment on a turned-down call, which no prompt replaces; not
    /// written.
    #[serde(skip)]
    pub parent_uuid: String,
    /// The first line of the last reply before the prompt, masked, which
    /// the digest shows so a short prompt reads in context; empty when no
    /// reply came between it and the prompt before, and for what does not
    /// read the replies; not written.
    #[serde(skip)]
    pub after: String,
}

/// What the Markdown writers need of every kind of record.
pub trait Record {
    /// The shared keys.
    fn head(&self) -> &Head;
    /// What masking the record counted, its shared keys included.
    fn masked(&self) -> Counts {
        let mut all = self.head().masked.clone();
        all.merge(self.own_masked());
        all
    }
    /// Whether `--grep` matched the record, rather than `--around` adding
    /// it as context.
    fn matched(&self) -> bool {
        self.search().matched
    }
    /// What masking the record's own fields counted.
    fn own_masked(&self) -> &Counts;
    /// What the search made of the record.
    fn search(&self) -> &Search;
    /// Where the line is.
    fn source(&self) -> &Source;
    /// How the sub-agent was started, for a kind that says.
    fn spawned(&self) -> Option<&Spawned>;
    /// The sub-agent file name, or empty for the session itself.
    fn subagent(&self) -> &str;
}

/// The assistant's text: a `reply` of `ccdig records`; or its thinking, a
/// `thinking`, whose keys are the same.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct Reply {
    /// The shared keys.
    #[serde(flatten)]
    pub head: Head,
    /// The text or thinking block, masked.
    pub text: String,
    /// The model that wrote it.
    pub model: String,
    /// The skill running at the time, or empty.
    pub skill: String,
    /// The sub-agent file name, or empty for the session itself.
    pub subagent: String,
    /// How the sub-agent was started.
    #[serde(flatten)]
    pub spawned: Spawned,
    /// The tokens of the response, on the first record of the response in
    /// the file; `null` on the others and when the line has none.
    pub usage: Option<Usage>,
    /// Whether masking changed the text or the sub-agent's description.
    pub redacted: bool,
    /// Where the line is.
    pub source: Source,
    /// What the search made of the record.
    #[serde(flatten)]
    pub search: Search,
    /// What masking the kind's own fields counted, for `--mask-report`;
    /// not written.
    #[serde(skip)]
    pub masked: Counts,
}

/// One session with at least one selected prompt: a line of
/// `ccdig sessions`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct Session {
    /// Always `AGENT`.
    pub agent: &'static str,
    /// The session ID.
    #[serde(rename = "session_id")]
    pub id: String,
    /// The session's last title, or empty.
    pub title: String,
    /// The project directory's name.
    pub project: String,
    /// The working directory of the first prompt.
    pub cwd: String,
    /// The git branch of the first prompt.
    pub git_branch: String,
    /// The transcript file.
    pub file: String,
    /// The file's modification time, for tools that assign sessions to
    /// weeks by it.
    pub mtime: Stamp,
    /// The first prompt's time.
    pub started_at: Stamp,
    /// The time of the session's last prompt, reply, tool call, or tool
    /// result in the range, whether the filters kept it or not.
    pub ended_at: Stamp,
    /// How many prompts were selected.
    pub prompt_count: usize,
    /// With `--grep` or `--igrep`, how many of the session's records of
    /// the types `--type` picks they matched; `null` otherwise.
    pub match_count: Option<usize>,
    /// With `--grep` or `--igrep`, the first of those records in the
    /// session; `null` otherwise.
    pub first_match: Option<FirstMatch>,
    /// The tokens of the session's responses in the range, each response
    /// once.
    pub usage: Usage,
    /// The shell command that resumes the session in the directory its
    /// transcript starts in, or `null` when it cannot be built;
    /// `resume_unavailable` says why.
    pub resume: Option<String>,
    /// Why `resume` is `null`, or `null` when it is not.
    pub resume_unavailable: Option<&'static str>,
    /// What masking these keys counted, for `--mask-report`; not written.
    #[serde(skip)]
    pub masked: Counts,
    /// The type of each record `match_count` counts, for Markdown to break
    /// the count down; not written.
    #[serde(skip)]
    pub matched_types: Vec<&'static str>,
}

/// The first record a search matched in a session, so a session can be
/// judged without reading its records.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct FirstMatch {
    /// Its type, as `--type` names it.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// When it was written.
    pub timestamp: Stamp,
    /// The 1-based number of the line a pattern matched in its text, or
    /// `null` when the match spans lines.
    pub line: Option<usize>,
    /// That line, or the whole text when no single line matched, masked,
    /// on one line and cut to `FIRST_MATCH_CHARS` characters.
    pub text: String,
}

/// The records that share the values of the `--by` keys: a line of
/// `ccdig stats`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct Group {
    /// The `--by` keys and their values, in the order `--by` names them,
    /// `null` for records without the key; with `--by session`, then the
    /// session's `session_title`, `project`, and `file`.
    #[serde(flatten)]
    pub keys: Map<String, Value>,
    /// How many records: with `--type thinking`, the thinking blocks with
    /// text alone.
    pub count: usize,
    /// With `--type thinking`, how many thinking blocks kept no text, which
    /// neither `count` nor `chars` counts; not written otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty: Option<usize>,
    /// How many characters of text the records have, masked: a tool call's
    /// input as compact JSON, and the text of the other kinds.
    pub chars: usize,
    /// How many sessions the records are in; not written with
    /// `--by session`, where it is always 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessions: Option<usize>,
    /// The time of the first record.
    pub first: Stamp,
    /// The time of the last record.
    pub last: Stamp,
    /// The tokens of the responses the records carry, each response once;
    /// `null` when none carries any, and with a `--by` key that splits a
    /// response, such as `tool`.
    pub usage: Option<Usage>,
}

/// The summary Claude Code writes when it compacts the context: a
/// `summary` of `ccdig records`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct Summary {
    /// The shared keys.
    #[serde(flatten)]
    pub head: Head,
    /// The summary, trimmed and masked.
    pub text: String,
    /// Whether masking changed the text.
    pub redacted: bool,
    /// Where the line is.
    pub source: Source,
    /// What the search made of the record.
    #[serde(flatten)]
    pub search: Search,
    /// What masking the kind's own fields counted, for `--mask-report`;
    /// not written.
    #[serde(skip)]
    pub masked: Counts,
}

/// Where a record's line is.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Source {
    /// The transcript file.
    pub file: String,
    /// The 1-based line number.
    pub line: usize,
    /// The line's ID, or empty.
    pub uuid: String,
}

/// How the sub-agent a record comes from was started, from the
/// `.meta.json` next to its transcript; empty for the session itself, and
/// when that file is missing or does not say.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct Spawned {
    /// The kind of sub-agent, such as `Explore`.
    pub subagent_type: String,
    /// What the caller wrote the sub-agent was for, masked.
    pub subagent_description: String,
    /// The `tool_use_id` of the call that started the sub-agent, in its
    /// session's transcript or in another sub-agent's.
    pub parent_tool_use_id: String,
}

/// A tool call: a `call` of `ccdig records`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct ToolCall {
    /// The shared keys.
    #[serde(flatten)]
    pub head: Head,
    /// The tool's name.
    pub tool: String,
    /// The arguments, with every string in them masked.
    pub input: Value,
    /// The call's ID, which its result refers to.
    pub tool_use_id: String,
    /// What the call came to, as its result in the same file says, masked;
    /// `null` when the file has no result for it, as when it was
    /// interrupted.
    pub result: Option<Outcome>,
    /// The skill running at the time, or empty.
    pub skill: String,
    /// The sub-agent file name, or empty for the session itself.
    pub subagent: String,
    /// How the sub-agent was started.
    #[serde(flatten)]
    pub spawned: Spawned,
    /// The tokens of the response, as for a reply.
    pub usage: Option<Usage>,
    /// Whether masking changed the input, the result's text, or the
    /// sub-agent's description.
    pub redacted: bool,
    /// Where the line is.
    pub source: Source,
    /// What the search made of the record.
    #[serde(flatten)]
    pub search: Search,
    /// The model that made the call, for `stats --by model`; not written.
    #[serde(skip)]
    pub model: String,
    /// What the call changed, from its input before masking, for
    /// `--touched`; not written.
    #[serde(skip)]
    pub touched: Touch,
    /// What masking the kind's own fields counted, for `--mask-report`;
    /// not written.
    #[serde(skip)]
    pub masked: Counts,
}

/// What a tool call changed, or the shell command that may have, for
/// `--touched`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Touch {
    /// A shell command, whose words may name the paths it changed.
    Command(String),
    /// The call changed nothing `--touched` can tell, as a read.
    #[default]
    Nothing,
    /// The file an edit or a write changed.
    Path(String),
}

/// What a tool call came to: the `result` of a `call`, so a call can be
/// read without asking for the results too.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct Outcome {
    /// Whether the tool reported a failure.
    pub is_error: bool,
    /// Why the call did not run, as for a result; empty when it ran.
    pub denial: String,
    /// The output text.
    pub text: String,
}

/// A tool's output, with the call it answers: a `result` of
/// `ccdig records`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct ToolResult {
    /// The shared keys.
    #[serde(flatten)]
    pub head: Head,
    /// The calling tool's name, or empty when the call was not found.
    pub tool: String,
    /// The call's arguments, masked, or `null` when the call was not found.
    pub input: Value,
    /// The call this answers.
    pub tool_use_id: String,
    /// Whether the tool reported a failure.
    pub is_error: bool,
    /// Claude Code's name for why the call did not run, such as
    /// `user-rejected` or `permission-rule`; empty when it ran.
    pub denial: String,
    /// The output text, masked.
    pub text: String,
    /// The sub-agent file name, or empty for the session itself.
    pub subagent: String,
    /// How the sub-agent was started.
    #[serde(flatten)]
    pub spawned: Spawned,
    /// Whether masking changed the text, the input, or the sub-agent's
    /// description.
    pub redacted: bool,
    /// Where the line is.
    pub source: Source,
    /// What the search made of the record.
    #[serde(flatten)]
    pub search: Search,
    /// The skill running when the call was made, for `--skill`; not
    /// written.
    #[serde(skip)]
    pub skill: String,
    /// What the call changed, from its input before masking, for
    /// `--touched`; not written.
    #[serde(skip)]
    pub touched: Touch,
    /// What masking the kind's own fields counted, for `--mask-report`;
    /// not written.
    #[serde(skip)]
    pub masked: Counts,
}

/// One line of `ccdig records`: a prompt, a reply, a tool call, a tool
/// result, a summary, or the assistant's thinking, written with a `type`
/// key naming which.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Turn {
    /// A tool call.
    Call(ToolCall),
    /// A prompt a person typed.
    Prompt(Prompt),
    /// A text block of the assistant.
    Reply(Reply),
    /// A tool result.
    Result(ToolResult),
    /// A compaction summary.
    Summary(Summary),
    /// A thinking block of the assistant.
    Thinking(Reply),
}

impl Group {
    /// Whether the lines whose keys before `count` are `columns` write
    /// `sessions`: not with `session`, where it is always 1.
    pub fn writes_sessions(columns: &[&str]) -> bool {
        !columns.contains(&"session")
    }
}

impl Kind {
    /// Every kind, in the order `--kind` lists them.
    pub const ALL: [Self; 6] = [
        Self::Text,
        Self::Slash,
        Self::Rejection,
        Self::Answer,
        Self::TypedAnswer,
        Self::Approval,
    ];

    /// The name the `kind` key writes.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Approval => "approval",
            Self::Rejection => "rejection",
            Self::Slash => "slash",
            Self::Text => "text",
            Self::TypedAnswer => "typed-answer",
        }
    }
}

impl Record for Prompt {
    fn head(&self) -> &Head {
        &self.head
    }

    fn own_masked(&self) -> &Counts {
        &self.masked
    }

    fn search(&self) -> &Search {
        &self.search
    }

    fn source(&self) -> &Source {
        &self.source
    }

    fn spawned(&self) -> Option<&Spawned> {
        None
    }

    fn subagent(&self) -> &'static str {
        ""
    }
}

impl Record for Reply {
    fn head(&self) -> &Head {
        &self.head
    }

    fn own_masked(&self) -> &Counts {
        &self.masked
    }

    fn search(&self) -> &Search {
        &self.search
    }

    fn source(&self) -> &Source {
        &self.source
    }

    fn spawned(&self) -> Option<&Spawned> {
        Some(&self.spawned)
    }

    fn subagent(&self) -> &str {
        &self.subagent
    }
}

impl Record for Summary {
    fn head(&self) -> &Head {
        &self.head
    }

    fn own_masked(&self) -> &Counts {
        &self.masked
    }

    fn search(&self) -> &Search {
        &self.search
    }

    fn source(&self) -> &Source {
        &self.source
    }

    fn spawned(&self) -> Option<&Spawned> {
        None
    }

    fn subagent(&self) -> &'static str {
        ""
    }
}

impl Record for ToolCall {
    fn head(&self) -> &Head {
        &self.head
    }

    fn own_masked(&self) -> &Counts {
        &self.masked
    }

    fn search(&self) -> &Search {
        &self.search
    }

    fn source(&self) -> &Source {
        &self.source
    }

    fn spawned(&self) -> Option<&Spawned> {
        Some(&self.spawned)
    }

    fn subagent(&self) -> &str {
        &self.subagent
    }
}

impl Record for ToolResult {
    fn head(&self) -> &Head {
        &self.head
    }

    fn own_masked(&self) -> &Counts {
        &self.masked
    }

    fn search(&self) -> &Search {
        &self.search
    }

    fn source(&self) -> &Source {
        &self.source
    }

    fn spawned(&self) -> Option<&Spawned> {
        Some(&self.spawned)
    }

    fn subagent(&self) -> &str {
        &self.subagent
    }
}

impl Default for Search {
    /// A record the search matched, with no lines listed.
    fn default() -> Self {
        Self {
            matched: true,
            matches: Vec::new(),
        }
    }
}

impl Turn {
    /// Cuts the record's texts, and the lines `matches` lists of them, to
    /// what `cuts` keeps: a prompt's, a reply's, a summary's, or a
    /// thinking block's to `message`, and a tool's input and output to
    /// `tool`, with the notes of each cut in `lang`.
    pub fn cut(&mut self, cuts: Cuts, lang: Lang) {
        let (given, own_text, own_input, search) = match *self {
            Self::Prompt(ref mut prompt) => (
                cuts.message,
                Some(&mut prompt.text),
                None,
                &mut prompt.search,
            ),
            Self::Reply(ref mut reply) | Self::Thinking(ref mut reply) => {
                (cuts.message, Some(&mut reply.text), None, &mut reply.search)
            }
            Self::Summary(ref mut summary) => (
                cuts.message,
                Some(&mut summary.text),
                None,
                &mut summary.search,
            ),
            Self::Call(ref mut call) => (
                cuts.tool,
                call.result.as_mut().map(|result| &mut result.text),
                Some(&mut call.input),
                &mut call.search,
            ),
            Self::Result(ref mut result) => (
                cuts.tool,
                Some(&mut result.text),
                Some(&mut result.input),
                &mut result.search,
            ),
        };
        let Some(span) = given else {
            return;
        };
        if let Some(text) = own_text {
            *text = cut_span(text, span, lang);
        }
        if let Some(input) = own_input {
            cut_strings(input, span, lang);
        }
        // A matched line is shown for where it is, so it keeps its start.
        for line in &mut search.matches {
            line.text = cut_text(&line.text, span.take, lang);
        }
    }

    /// The record inside.
    fn inner(&self) -> &dyn Record {
        match *self {
            Self::Prompt(ref prompt) => prompt,
            Self::Reply(ref reply) | Self::Thinking(ref reply) => reply,
            Self::Call(ref call) => call,
            Self::Result(ref result) => result,
            Self::Summary(ref summary) => summary,
        }
    }

    /// The tool's output the record carries: a result's text, or the one
    /// a call carries; `None` for any other record, and for a call without
    /// a result.
    pub fn tool_output_mut(&mut self) -> Option<&mut String> {
        match *self {
            Self::Result(ref mut result) => Some(&mut result.text),
            Self::Call(ref mut call) => call.result.as_mut().map(|result| &mut result.text),
            Self::Prompt(_) | Self::Reply(_) | Self::Summary(_) | Self::Thinking(_) => None,
        }
    }

    /// The name the `type` key writes.
    pub const fn type_name(&self) -> &'static str {
        match *self {
            Self::Call(_) => "call",
            Self::Prompt(_) => "prompt",
            Self::Reply(_) => "reply",
            Self::Result(_) => "result",
            Self::Summary(_) => "summary",
            Self::Thinking(_) => "thinking",
        }
    }
}

impl<T> Record for &T
where
    T: Record,
{
    fn head(&self) -> &Head {
        (**self).head()
    }

    fn own_masked(&self) -> &Counts {
        (**self).own_masked()
    }

    fn search(&self) -> &Search {
        (**self).search()
    }

    fn source(&self) -> &Source {
        (**self).source()
    }

    fn spawned(&self) -> Option<&Spawned> {
        (**self).spawned()
    }

    fn subagent(&self) -> &str {
        (**self).subagent()
    }
}

impl Record for Turn {
    fn head(&self) -> &Head {
        self.inner().head()
    }

    fn own_masked(&self) -> &Counts {
        self.inner().own_masked()
    }

    fn search(&self) -> &Search {
        self.inner().search()
    }

    fn source(&self) -> &Source {
        self.inner().source()
    }

    fn spawned(&self) -> Option<&Spawned> {
        self.inner().spawned()
    }

    fn subagent(&self) -> &str {
        self.inner().subagent()
    }
}

/// Keeps of each string in `value` the characters `span` keeps, saying
/// what it left out. A string no longer than `span` keeps, which a cut of
/// that many characters leaves whole, stays whole, so the part of an input
/// a range reads on comes with the short strings that say what it is, such
/// as its file's path. The notes are in `lang`.
fn cut_strings(value: &mut Value, span: Span, lang: Lang) {
    match *value {
        Value::String(ref mut text) => {
            if text.chars().nth(span.take).is_some() {
                *text = cut_span(text, span, lang);
            }
        }
        Value::Array(ref mut items) => {
            for item in items {
                cut_strings(item, span, lang);
            }
        }
        Value::Object(ref mut fields) => {
            for field in fields.values_mut() {
                cut_strings(field, span, lang);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
