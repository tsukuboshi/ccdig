//! The four kinds of record and how each is picked out of a line.

use alloc::borrow::Cow;
use std::collections::HashMap;

use serde_json::Value;

use super::{Extra, Pick, Select, Spot};
use crate::classify::human::{Reason, classify};
use crate::classify::slash::fold;
use crate::render::record::{Head, Kind, Prompt, Reply, Source, ToolCall, ToolResult};
use crate::transcript::entry::{Block, Entry, Role};
use crate::transcript::reader::Roles;

/// The roles of the kinds read from assistant lines only.
const ASSISTANT: Roles = Roles {
    assistant: true,
    user: false,
};

/// What a result needs of the call it answers.
struct Call {
    /// The arguments, as the model wrote them.
    input: Value,
    /// The tool's name.
    name: String,
    /// The skill running when the call was made.
    skill: String,
}

/// Picks the prompts a person typed.
pub struct Prompts;

/// Picks the assistant's text blocks.
pub struct Replies;

/// Picks tool calls.
pub struct Tools;

/// Picks tool results, pairing each with the call it answers.
#[derive(Default)]
pub struct Results {
    /// The calls seen so far in the file, by ID. Calls come before their
    /// results, so every result can find its call however the lines are
    /// filtered.
    calls: HashMap<String, Call>,
}

impl Pick for Prompts {
    type Record = Prompt;

    fn pick(&self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, Prompt)> {
        let (typed, kind, command) = match fold(&entry.content.text) {
            Some(folded) => (folded.text, Kind::Slash, folded.command),
            None => (entry.content.text.clone(), Kind::Text, String::new()),
        };
        let (text, redacted) = spot.mask.text(typed.trim());
        vec![(
            0,
            Prompt {
                head: spot.head(entry),
                text,
                kind,
                command,
                redacted,
                source: source(spot, entry),
            },
        )]
    }

    fn roles(&self) -> Roles {
        Roles {
            assistant: false,
            user: true,
        }
    }

    fn wants(&mut self, entry: &Entry) -> bool {
        classify(entry) == Reason::Human
    }
}

impl Pick for Replies {
    type Record = Reply;

    fn pick(&self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, Reply)> {
        blocks(entry, |block| match *block {
            Block::Text(ref written) if !written.trim().is_empty() => {
                let (text, redacted) = spot.mask.text(written.trim());
                Some(Reply {
                    head: spot.head(entry),
                    text,
                    model: entry.model.clone(),
                    skill: entry.skill.clone(),
                    subagent: spot.subagent(),
                    redacted,
                    source: source(spot, entry),
                })
            }
            Block::Text(_) | Block::Other | Block::ToolResult(_) | Block::ToolUse(_) => None,
        })
    }

    fn roles(&self) -> Roles {
        ASSISTANT
    }

    fn wants(&mut self, entry: &Entry) -> bool {
        entry.role == Role::Assistant
    }
}

impl Pick for Results {
    type Record = ToolResult;

    fn pick(&self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, ToolResult)> {
        blocks(entry, |block| match *block {
            Block::ToolResult(ref result) => {
                let call = self.calls.get(&result.tool_use_id);
                let mut input = call.map_or(Value::Null, |found| found.input.clone());
                let input_redacted = spot.mask.json(&mut input);
                let (text, text_redacted) = spot.mask.text(&result.text);
                Some(ToolResult {
                    head: spot.head(entry),
                    tool: call.map_or_else(String::new, |found| found.name.clone()),
                    input,
                    tool_use_id: result.tool_use_id.clone(),
                    is_error: result.is_error,
                    text,
                    subagent: spot.subagent(),
                    redacted: input_redacted || text_redacted,
                    source: source(spot, entry),
                    skill: call.map_or_else(String::new, |found| found.skill.clone()),
                })
            }
            Block::Other | Block::Text(_) | Block::ToolUse(_) => None,
        })
    }

    fn roles(&self) -> Roles {
        Roles {
            assistant: true,
            user: true,
        }
    }

    fn wants(&mut self, entry: &Entry) -> bool {
        match entry.role {
            Role::Assistant => {
                for block in &entry.content.blocks {
                    if let Block::ToolUse(ref call) = *block {
                        self.calls.insert(
                            call.id.clone(),
                            Call {
                                input: call.input.clone(),
                                name: call.name.clone(),
                                skill: entry.skill.clone(),
                            },
                        );
                    }
                }
                false
            }
            // Block by block: unlike the prompt check, a mistyped sibling
            // block must not hide the results.
            Role::User => entry
                .content
                .blocks
                .iter()
                .any(|block| matches!(*block, Block::ToolResult(_))),
            Role::AiTitle | Role::Other => false,
        }
    }
}

impl Pick for Tools {
    type Record = ToolCall;

    fn pick(&self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, ToolCall)> {
        blocks(entry, |block| match *block {
            Block::ToolUse(ref call) => {
                let mut input = call.input.clone();
                let redacted = spot.mask.json(&mut input);
                Some(ToolCall {
                    head: spot.head(entry),
                    tool: call.name.clone(),
                    input,
                    tool_use_id: call.id.clone(),
                    skill: entry.skill.clone(),
                    subagent: spot.subagent(),
                    redacted,
                    source: source(spot, entry),
                })
            }
            Block::Other | Block::Text(_) | Block::ToolResult(_) => None,
        })
    }

    fn roles(&self) -> Roles {
        ASSISTANT
    }

    fn wants(&mut self, entry: &Entry) -> bool {
        entry.role == Role::Assistant
    }
}

impl Select for Prompt {
    fn extra(&self) -> Extra<'_> {
        Extra {
            is_error: false,
            skill: "",
            tool: "",
        }
    }

    fn grep_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }

    fn head_mut(&mut self) -> &mut Head {
        &mut self.head
    }
}

impl Select for Reply {
    fn extra(&self) -> Extra<'_> {
        Extra {
            is_error: false,
            skill: &self.skill,
            tool: "",
        }
    }

    fn grep_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }

    fn head_mut(&mut self) -> &mut Head {
        &mut self.head
    }
}

impl Select for ToolCall {
    fn extra(&self) -> Extra<'_> {
        Extra {
            is_error: false,
            skill: &self.skill,
            tool: &self.tool,
        }
    }

    /// The input as compact JSON.
    fn grep_text(&self) -> Cow<'_, str> {
        Cow::Owned(self.input.to_string())
    }

    fn head_mut(&mut self) -> &mut Head {
        &mut self.head
    }
}

impl Select for ToolResult {
    fn extra(&self) -> Extra<'_> {
        Extra {
            is_error: self.is_error,
            skill: &self.skill,
            tool: &self.tool,
        }
    }

    fn grep_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }

    fn head_mut(&mut self) -> &mut Head {
        &mut self.head
    }
}

/// The records `make` builds from the blocks of `entry`, each with its
/// block's index.
fn blocks<T>(entry: &Entry, make: impl Fn(&Block) -> Option<T>) -> Vec<(usize, T)> {
    entry
        .content
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| make(block).map(|record| (index, record)))
        .collect()
}

/// Where `entry` is.
fn source(spot: &Spot<'_>, entry: &Entry) -> Source {
    Source {
        file: spot.file.path.to_string_lossy().into_owned(),
        line: spot.line,
        uuid: entry.uuid.clone(),
    }
}
