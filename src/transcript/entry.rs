//! One line of a session transcript.
//!
//! The decoding follows promptdig's Go `encoding/json` where the prompt
//! classification depends on it: `null` reads as empty, unknown fields are
//! ignored, invalid UTF-8 becomes U+FFFD, and one mistyped block empties the
//! prompt text. Tool blocks are read one by one, so a broken block hides only
//! itself.

use serde::Deserialize;
use serde_json::value::RawValue;
use serde_json::{Map, Value};

use crate::when::Stamp;

/// One element of a message's content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Block {
    /// Anything else: thinking, images, and blocks with missing fields.
    Other,
    /// Text written by the user or the assistant.
    Text(String),
    /// The output of a tool call.
    ToolResult(ToolResult),
    /// A tool call made by the assistant.
    ToolUse(ToolUse),
}

/// A message's content, read two ways.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Content {
    /// Every element of array content, in order, so a block's position is
    /// its index in the line.
    pub blocks: Vec<Block>,
    /// Whether array content holds a tool result, as promptdig decided it.
    pub has_tool_result: bool,
    /// The prompt text as promptdig read it: string content as is, or the
    /// text blocks joined by newlines, or empty when any block is mistyped.
    pub text: String,
}

/// One line of a transcript, reduced to the fields ccdig uses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// The message content.
    pub content: Content,
    /// The working directory the session ran in.
    pub cwd: String,
    /// The markers that say the harness, not a person, wrote the line.
    pub flags: Flags,
    /// The git branch checked out at the time.
    pub git_branch: String,
    /// The model that wrote an assistant line.
    pub model: String,
    /// Who produced a user line, such as `human` or `task-notification`.
    /// `None` for sessions recorded before Claude Code wrote the field.
    pub origin: Option<String>,
    /// What kind of line this is.
    pub role: Role,
    /// The session the line belongs to; a sub-agent's lines carry the
    /// parent session's ID.
    pub session_id: String,
    /// The skill running when the line was written (`attributionSkill`).
    pub skill: String,
    /// When the line was written.
    pub timestamp: Option<Stamp>,
    /// The session title, on `ai-title` lines.
    pub title: String,
    /// The line's ID, which a resumed session repeats.
    pub uuid: String,
}

/// The markers on a line that the harness, not a person, wrote.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Flags {
    /// The summary inserted when the context is compacted.
    pub compact_summary: bool,
    /// Content the harness injected, such as a skill body.
    pub meta: bool,
    /// Sub-agent traffic.
    pub sidechain: bool,
}

/// A field with a type promptdig's Go struct could not hold.
#[derive(Clone, Copy, Debug)]
struct Mistyped;

/// The JSON of a line, with every field optional so `null` reads as empty.
#[derive(Deserialize)]
struct RawEntry<'line> {
    /// `aiTitle`.
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
    /// `attributionSkill`.
    #[serde(rename = "attributionSkill")]
    attribution_skill: Option<String>,
    /// `cwd`.
    cwd: Option<String>,
    /// `gitBranch`.
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    /// `isCompactSummary`.
    #[serde(rename = "isCompactSummary")]
    is_compact_summary: Option<bool>,
    /// `isMeta`.
    #[serde(rename = "isMeta")]
    is_meta: Option<bool>,
    /// `isSidechain`.
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    /// `type`.
    #[serde(rename = "type")]
    kind: Option<String>,
    /// `message`.
    #[serde(borrow)]
    message: Option<RawMessage<'line>>,
    /// `origin`.
    origin: Option<RawOrigin>,
    /// `sessionId`.
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    /// `timestamp`, parsed separately so a bad value rejects the line.
    timestamp: Option<String>,
    /// `uuid`.
    uuid: Option<String>,
}

/// The JSON of `message`.
#[derive(Deserialize)]
struct RawMessage<'line> {
    /// A string or an array of blocks, decoded after the line.
    #[serde(borrow)]
    content: Option<&'line RawValue>,
    /// The model that wrote an assistant message.
    model: Option<String>,
}

/// The JSON of `origin`.
#[derive(Deserialize)]
struct RawOrigin {
    /// `human`, `task-notification`, `peer`, and so on.
    kind: Option<String>,
}

/// What kind of line an entry is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    /// A line carrying the session title.
    AiTitle,
    /// The assistant's message.
    Assistant,
    /// Anything else, such as `permission-mode` or `summary`.
    Other,
    /// A user message, typed or injected.
    User,
}

/// The output of a tool call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolResult {
    /// Whether the tool reported a failure.
    pub is_error: bool,
    /// The output text; array output has its text parts joined by newlines.
    pub text: String,
    /// The call this answers.
    pub tool_use_id: String,
}

/// A tool call made by the assistant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolUse {
    /// The call's ID, which its result refers to.
    pub id: String,
    /// The arguments, as the model wrote them.
    pub input: Value,
    /// The tool's name, such as `Bash`.
    pub name: String,
}

/// Decodes one JSONL line, or `None` when it is not a transcript entry.
pub fn decode_line(line: &[u8]) -> Option<Entry> {
    let text = String::from_utf8_lossy(line);
    // serde reads a JSON array into a struct field by field; Go did not.
    if !text.trim_start().starts_with('{') {
        return None;
    }
    let raw: RawEntry<'_> = serde_json::from_str(&text).ok()?;
    let timestamp = match raw.timestamp {
        Some(ref value) => Some(Stamp::parse_rfc3339(value)?),
        None => None,
    };
    let (content, model) = raw.message.map_or_else(
        || (Content::default(), String::new()),
        |message| {
            (
                decode_content(message.content),
                message.model.unwrap_or_default(),
            )
        },
    );
    Some(Entry {
        content,
        cwd: raw.cwd.unwrap_or_default(),
        flags: Flags {
            compact_summary: raw.is_compact_summary.unwrap_or_default(),
            meta: raw.is_meta.unwrap_or_default(),
            sidechain: raw.is_sidechain.unwrap_or_default(),
        },
        git_branch: raw.git_branch.unwrap_or_default(),
        model,
        origin: raw.origin.map(|origin| origin.kind.unwrap_or_default()),
        role: match raw.kind.as_deref() {
            Some("ai-title") => Role::AiTitle,
            Some("assistant") => Role::Assistant,
            Some("user") => Role::User,
            Some(_) | None => Role::Other,
        },
        session_id: raw.session_id.unwrap_or_default(),
        skill: raw.attribution_skill.unwrap_or_default(),
        timestamp,
        title: raw.ai_title.unwrap_or_default(),
        uuid: raw.uuid.unwrap_or_default(),
    })
}

/// Reads one element of array content.
fn block(item: &Value) -> Block {
    let Value::Object(ref fields) = *item else {
        return Block::Other;
    };
    match fields.get("type").and_then(Value::as_str) {
        Some("text") => fields
            .get("text")
            .and_then(Value::as_str)
            .map_or(Block::Other, |text| Block::Text(text.to_owned())),
        Some("tool_result") => tool_result(fields),
        Some("tool_use") => tool_use(fields),
        Some(_) | None => Block::Other,
    }
}

/// Reads a message's content into blocks and promptdig's prompt text.
fn decode_content(raw: Option<&RawValue>) -> Content {
    let value = raw
        .and_then(|content| serde_json::from_str(content.get()).ok())
        .unwrap_or(Value::Null);
    match value {
        Value::String(text) => Content {
            blocks: Vec::new(),
            has_tool_result: false,
            text,
        },
        Value::Array(items) => {
            let (text, has_tool_result) = prompt_text(&items).unwrap_or_default();
            Content {
                blocks: items.iter().map(block).collect(),
                has_tool_result,
                text,
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => Content::default(),
    }
}

/// The string at `key`, or `None` when it is absent or `null`. Another type
/// is `Mistyped`, which made Go reject the whole array.
fn optional_str<'value>(
    fields: &'value Map<String, Value>,
    key: &str,
) -> Result<Option<&'value str>, Mistyped> {
    let Some(value) = fields.get(key) else {
        return Ok(None);
    };
    match *value {
        Value::Null => Ok(None),
        Value::String(ref text) => Ok(Some(text)),
        Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => Err(Mistyped),
    }
}

/// promptdig's text and tool-result check for array content: the text
/// blocks joined by newlines, or `None` when any element would not decode
/// into Go's `{type, text}` struct.
fn prompt_text(items: &[Value]) -> Option<(String, bool)> {
    let mut texts = Vec::new();
    let mut has_tool_result = false;
    for item in items {
        let (kind, text) = match *item {
            Value::Null => (None, None),
            Value::Object(ref fields) => (
                optional_str(fields, "type").ok()?,
                optional_str(fields, "text").ok()?,
            ),
            Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => return None,
        };
        match kind {
            Some("text") => texts.push(text.unwrap_or_default()),
            Some("tool_result") => has_tool_result = true,
            Some(_) | None => {}
        }
    }
    Some((texts.join("\n"), has_tool_result))
}

/// Reads a `tool_result` block; one without the call ID is `Other`.
fn tool_result(fields: &Map<String, Value>) -> Block {
    let Some(tool_use_id) = fields.get("tool_use_id").and_then(Value::as_str) else {
        return Block::Other;
    };
    let text = fields
        .get("content")
        .map_or_else(String::new, |content| match *content {
            Value::String(ref text) => text.clone(),
            Value::Array(ref parts) => parts
                .iter()
                .filter_map(|part| match block(part) {
                    Block::Text(text) => Some(text),
                    Block::Other | Block::ToolResult(_) | Block::ToolUse(_) => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => String::new(),
        });
    Block::ToolResult(ToolResult {
        is_error: fields
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or_default(),
        text,
        tool_use_id: tool_use_id.to_owned(),
    })
}

/// Reads a `tool_use` block; one without an ID or a name is `Other`.
fn tool_use(fields: &Map<String, Value>) -> Block {
    let (Some(id), Some(name)) = (
        fields.get("id").and_then(Value::as_str),
        fields.get("name").and_then(Value::as_str),
    ) else {
        return Block::Other;
    };
    Block::ToolUse(ToolUse {
        id: id.to_owned(),
        input: fields.get("input").cloned().unwrap_or(Value::Null),
        name: name.to_owned(),
    })
}

#[cfg(test)]
mod tests;
