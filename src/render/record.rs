//! The records the subcommands print. Every key is always written, in
//! declaration order, so consumers can rely on the keys.

use serde::Serialize;
use serde_json::Value;

use crate::when::Stamp;

/// The `agent` of every record.
pub const AGENT: &str = "claude-code";

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
    /// The project directory's name.
    pub project: String,
    /// The working directory.
    pub cwd: String,
    /// The git branch checked out at the time.
    pub git_branch: String,
    /// When the line was written.
    pub timestamp: Stamp,
}

/// How a prompt was typed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A slash command, folded back into what was typed.
    Slash,
    /// Plain text.
    Text,
}

/// A prompt a person typed: a line of `ccdig prompts`.
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
    /// Plain text or a slash command.
    pub kind: Kind,
    /// The slash command's name, or empty for plain text.
    pub command: String,
    /// Whether masking changed the text.
    pub redacted: bool,
    /// Where the line is.
    pub source: Source,
}

/// What the Markdown writers need of every kind of record.
pub trait Record {
    /// The shared keys.
    fn head(&self) -> &Head;
    /// Where the line is.
    fn source(&self) -> &Source;
    /// The sub-agent file name, or empty for the session itself.
    fn subagent(&self) -> &str;
}

/// The assistant's text: a line of `ccdig replies`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct Reply {
    /// The shared keys.
    #[serde(flatten)]
    pub head: Head,
    /// The text block, masked.
    pub text: String,
    /// The model that wrote it.
    pub model: String,
    /// The skill running at the time, or empty.
    pub skill: String,
    /// The sub-agent file name, or empty for the session itself.
    pub subagent: String,
    /// Whether masking changed the text.
    pub redacted: bool,
    /// Where the line is.
    pub source: Source,
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
    /// The file's modification time, in UTC, for tools that assign
    /// sessions to weeks by it.
    pub mtime: Stamp,
    /// The first prompt's time.
    pub started_at: Stamp,
    /// The last prompt's time.
    pub ended_at: Stamp,
    /// How many prompts were selected.
    pub prompt_count: usize,
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

/// A tool call: a line of `ccdig tools`.
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
    /// The skill running at the time, or empty.
    pub skill: String,
    /// The sub-agent file name, or empty for the session itself.
    pub subagent: String,
    /// Whether masking changed the input.
    pub redacted: bool,
    /// Where the line is.
    pub source: Source,
}

/// A tool's output, with the call it answers: a line of `ccdig results`.
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
    /// The output text, masked.
    pub text: String,
    /// The sub-agent file name, or empty for the session itself.
    pub subagent: String,
    /// Whether masking changed the text or the input.
    pub redacted: bool,
    /// Where the line is.
    pub source: Source,
    /// The skill running when the call was made, for `--skill`; not
    /// written.
    #[serde(skip)]
    pub skill: String,
}

impl Record for Prompt {
    fn head(&self) -> &Head {
        &self.head
    }

    fn source(&self) -> &Source {
        &self.source
    }

    fn subagent(&self) -> &'static str {
        ""
    }
}

impl Record for Reply {
    fn head(&self) -> &Head {
        &self.head
    }

    fn source(&self) -> &Source {
        &self.source
    }

    fn subagent(&self) -> &str {
        &self.subagent
    }
}

impl Record for ToolCall {
    fn head(&self) -> &Head {
        &self.head
    }

    fn source(&self) -> &Source {
        &self.source
    }

    fn subagent(&self) -> &str {
        &self.subagent
    }
}

impl Record for ToolResult {
    fn head(&self) -> &Head {
        &self.head
    }

    fn source(&self) -> &Source {
        &self.source
    }

    fn subagent(&self) -> &str {
        &self.subagent
    }
}
