//! The kinds of record and how each is picked out of a line.

use alloc::borrow::Cow;
use std::collections::HashMap;

use serde_json::Value;

use super::{Extra, Pick, Select, Spot};
use crate::classify::human::{Reason, classify};
use crate::classify::slash::fold;
use crate::render::record::{Head, Kind, Prompt, Reply, Source, ToolCall, ToolResult, Turn};
use crate::transcript::entry::{Block, Entry, Role};
use crate::transcript::reader::Roles;

/// What a result needs of the call it answers.
struct Call {
    /// The arguments, as the model wrote them.
    input: Value,
    /// The tool's name.
    name: String,
    /// The skill running when the call was made.
    skill: String,
}

/// Picks the records of the kinds in `types`, in the order they were
/// written.
pub struct Records {
    /// Pairs the results with their calls.
    results: Results,
    /// The kinds to pick.
    types: Types,
}

/// The kinds of record `Records` picks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "--type picks each kind on its own, so each is one switch"
)]
pub struct Types {
    /// Tool calls.
    pub calls: bool,
    /// The prompts a person typed.
    pub prompts: bool,
    /// The assistant's text blocks.
    pub replies: bool,
    /// Tool results.
    pub results: bool,
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

impl Types {
    /// Whether any of the kinds is on an assistant line, or needs one, as
    /// a result needs its call.
    pub const fn reads_assistant(self) -> bool {
        self.replies || self.calls || self.results
    }
}

impl Records {
    /// Picks the kinds in `types`.
    pub fn new(types: Types) -> Self {
        Self {
            results: Results::default(),
            types,
        }
    }
}

impl Pick for Records {
    type Record = Turn;

    fn pick(&self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, Turn)> {
        let mut turns: Vec<(usize, Turn)> = Vec::new();
        match entry.role {
            Role::User => {
                // With results picked too, `wants` may have accepted the line
                // for its results only. A prompt is never on a line with a
                // result, as `classify` turns such a line down, so the
                // prompt's block index 0 cannot clash with a result's in the
                // check for resumed copies.
                if self.types.prompts && (!self.types.results || classify(entry) == Reason::Human) {
                    let prompts = Prompts.pick(spot, entry).into_iter();
                    turns.extend(prompts.map(|(index, prompt)| (index, Turn::Prompt(prompt))));
                }
                if self.types.results {
                    let results = self.results.pick(spot, entry).into_iter();
                    turns.extend(results.map(|(index, result)| (index, Turn::Result(result))));
                }
            }
            Role::Assistant => {
                if self.types.replies {
                    let replies = Replies.pick(spot, entry).into_iter();
                    turns.extend(replies.map(|(index, reply)| (index, Turn::Reply(reply))));
                }
                if self.types.calls {
                    let calls = Tools.pick(spot, entry).into_iter();
                    turns.extend(calls.map(|(index, call)| (index, Turn::Call(call))));
                }
            }
            Role::AiTitle | Role::Other => {}
        }
        turns.sort_by_key(|pair| pair.0);
        turns
    }

    fn roles(&self) -> Roles {
        Roles {
            assistant: self.types.reads_assistant(),
            user: self.types.prompts || self.types.results,
        }
    }

    fn wants(&mut self, entry: &Entry) -> bool {
        // Always called, so the results see every call before its result.
        let result = self.types.results && self.results.wants(entry);
        match entry.role {
            Role::User => result || (self.types.prompts && Prompts.wants(entry)),
            Role::Assistant => self.types.replies || self.types.calls,
            Role::AiTitle | Role::Other => false,
        }
    }
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
        Roles::USER
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
        Roles::ASSISTANT
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
        Roles::BOTH
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
        Roles::ASSISTANT
    }

    fn wants(&mut self, entry: &Entry) -> bool {
        entry.role == Role::Assistant
    }
}

impl Select for Prompt {
    fn extra(&self) -> Extra<'_> {
        Extra {
            is_error: None,
            skill: None,
            tool: None,
        }
    }

    fn grep_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }

    fn head_mut(&mut self) -> &mut Head {
        &mut self.head
    }

    fn labels_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.command]
    }
}

impl Select for Reply {
    fn extra(&self) -> Extra<'_> {
        Extra {
            is_error: None,
            skill: Some(&self.skill),
            tool: None,
        }
    }

    fn grep_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }

    fn head_mut(&mut self) -> &mut Head {
        &mut self.head
    }

    fn labels_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.model, &mut self.subagent]
    }
}

impl Select for ToolCall {
    fn extra(&self) -> Extra<'_> {
        Extra {
            is_error: None,
            skill: Some(&self.skill),
            tool: Some(&self.tool),
        }
    }

    /// The input as compact JSON.
    fn grep_text(&self) -> Cow<'_, str> {
        Cow::Owned(self.input.to_string())
    }

    fn head_mut(&mut self) -> &mut Head {
        &mut self.head
    }

    fn labels_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.tool, &mut self.subagent]
    }
}

impl Select for Turn {
    fn extra(&self) -> Extra<'_> {
        selected(self).extra()
    }

    fn grep_text(&self) -> Cow<'_, str> {
        selected(self).grep_text()
    }

    fn head_mut(&mut self) -> &mut Head {
        selected_mut(self).head_mut()
    }

    fn labels_mut(&mut self) -> Vec<&mut String> {
        selected_mut(self).labels_mut()
    }
}

impl Select for ToolResult {
    fn extra(&self) -> Extra<'_> {
        Extra {
            is_error: Some(self.is_error),
            skill: Some(&self.skill),
            tool: Some(&self.tool),
        }
    }

    fn grep_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }

    fn head_mut(&mut self) -> &mut Head {
        &mut self.head
    }

    fn labels_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.tool, &mut self.subagent]
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

/// The record inside `turn`.
fn selected(turn: &Turn) -> &dyn Select {
    match *turn {
        Turn::Prompt(ref prompt) => prompt,
        Turn::Reply(ref reply) => reply,
        Turn::Call(ref call) => call,
        Turn::Result(ref result) => result,
    }
}

/// The record inside `turn`, to change.
fn selected_mut(turn: &mut Turn) -> &mut dyn Select {
    match *turn {
        Turn::Prompt(ref mut prompt) => prompt,
        Turn::Reply(ref mut reply) => reply,
        Turn::Call(ref mut call) => call,
        Turn::Result(ref mut result) => result,
    }
}

/// Where `entry` is.
fn source(spot: &Spot<'_>, entry: &Entry) -> Source {
    Source {
        file: spot.mask.meta(&spot.file.path.to_string_lossy()),
        line: spot.line,
        uuid: entry.uuid.clone(),
    }
}
