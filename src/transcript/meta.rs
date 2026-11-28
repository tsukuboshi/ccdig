//! What Claude Code notes about a sub-agent next to its transcript, in
//! `subagents/agent-<id>.meta.json`.
//!
//! The format is not documented; the keys read here are the ones observed.
//! The file holds many more, which vary between versions and are ignored.

use serde::Deserialize;
use serde_json::Value;

/// The JSON of the file. Each field is read as any value, so that `null`
/// or a value of another type empties only that field.
#[derive(Deserialize)]
struct RawMeta {
    /// `agentType`.
    #[serde(rename = "agentType")]
    agent_type: Option<Value>,
    /// `description`.
    description: Option<Value>,
    /// `toolUseId`; a forked skill has none.
    #[serde(rename = "toolUseId")]
    tool_use_id: Option<Value>,
}

/// How a sub-agent was started.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Spawn {
    /// The kind of sub-agent (`agentType`), such as `Explore`.
    pub agent_type: String,
    /// What the sub-agent was started for (`description`), as the caller
    /// wrote it.
    pub description: String,
    /// The tool call that started it (`toolUseId`).
    pub tool_use_id: String,
}

/// Decodes a `.meta.json`, or `None` when it is not a JSON object. A key
/// that is missing, `null`, or not a string reads as empty. Invalid UTF-8
/// becomes U+FFFD, as in a transcript line.
pub fn decode(bytes: &[u8]) -> Option<Spawn> {
    let text = String::from_utf8_lossy(bytes);
    // serde reads a JSON array into a struct field by field.
    if !text.trim_start().starts_with('{') {
        return None;
    }
    let raw: RawMeta = serde_json::from_str(&text).ok()?;
    Some(Spawn {
        agent_type: string(raw.agent_type),
        description: string(raw.description),
        tool_use_id: string(raw.tool_use_id),
    })
}

/// The string in `value`, or empty when it is missing or not a string.
fn string(value: Option<Value>) -> String {
    match value {
        Some(Value::String(text)) => text,
        Some(_) | None => String::new(),
    }
}

#[cfg(test)]
mod tests;
