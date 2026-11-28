//! The kinds of record and how each is picked out of a line.

use alloc::borrow::Cow;
use core::mem;
use std::collections::HashMap;

use serde_json::Value;

use super::mask::Draft;
use super::{Extra, Pick, Select, Spot};
use crate::classify::denial::{Answer, comment_in, denial};
use crate::classify::human::{Reason, classify, is_no_response};
use crate::classify::response::{ASK, DIALOGS, PLAN, Response, response, without_repeated_plan};
use crate::classify::slash::fold;
use crate::record::{
    Head, Kind, Outcome, Prompt, Reply, Search, Source, Summary, ToolCall, ToolResult, Touch, Turn,
};
use crate::redact::Counts;
use crate::transcript::entry::{Block, Entry, Role, Usage};
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
    /// Whether the thinking blocks without text are picked too, for
    /// `stats` to count them.
    empty_thinking: bool,
    /// When the calls carry their results: for each call picked so far in
    /// the file, by its ID, what it came to, unmasked, once its result is
    /// seen.
    outcomes: Option<HashMap<String, Option<Outcome>>>,
    /// When the calls carry their results: the plans shown for approval so
    /// far in the file, by the call's ID, so a result does not repeat one.
    plans: HashMap<String, String>,
    /// Notes which prompts were sent again.
    prompts: Prompts,
    /// Pairs the results with their calls.
    results: Results,
    /// The prompts typed so far in the file, which numbers the records.
    turn: usize,
    /// Whether the records are numbered with their turn, which reads the
    /// user lines whatever the kinds.
    turns: bool,
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
    /// Compaction summaries.
    pub summaries: bool,
    /// The assistant's thinking blocks.
    pub thinking: bool,
}

/// Picks the prompts a person typed, and what they answered through a
/// dialog.
#[derive(Default)]
pub struct Prompts {
    /// The first line of the last reply seen since the last prompt picked,
    /// unmasked, when `replies` is set: what a prompt follows when the line
    /// it follows is not known.
    after: String,
    /// When `replies` is set, for each line seen so far in the file, by its
    /// ID, the first line of the last reply on its branch, unmasked, or
    /// empty after a prompt: a prompt sent again in place of another
    /// follows the reply the other followed, not the replies after it.
    branch: HashMap<String, String>,
    /// The calls seen so far in the file that show the user a dialog, by
    /// ID: the tool's name.
    dialogs: HashMap<String, String>,
    /// The last typed prompt seen so far in the file that follows each
    /// line, by the line's ID: a prompt sent again in place of another
    /// follows the same line.
    last: HashMap<String, String>,
    /// Whether each prompt carries the first line of the reply before it,
    /// which reads every assistant line.
    replies: bool,
}

/// Picks the compaction summaries.
pub struct Summaries;

/// Picks the assistant's text blocks.
pub struct Replies;

/// Picks the assistant's thinking blocks: with `empty`, those without text
/// too.
pub struct Thoughts {
    /// Whether the blocks without text are picked too.
    pub empty: bool,
}

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
    /// Prompts and replies: the conversation, which `records` lists and
    /// `sessions` searches by default.
    pub const CONVERSATION: Self = Self {
        calls: false,
        prompts: true,
        replies: true,
        results: false,
        summaries: false,
        thinking: false,
    };

    /// Prompts alone, which `--kind` and `--superseded` pick without
    /// `--type`.
    pub const PROMPTS: Self = Self {
        calls: false,
        prompts: true,
        replies: false,
        results: false,
        summaries: false,
        thinking: false,
    };

    /// Whether any of the kinds is on an assistant line, or needs one, as
    /// a result needs its call.
    pub const fn reads_assistant(self) -> bool {
        self.replies || self.calls || self.results || self.thinking
    }
}

impl Records {
    /// Picks the kinds in `types` as `new` does, and the thinking blocks
    /// without text too, for `stats` to count them.
    pub fn counting_empty_thinking(types: Types) -> Self {
        Self {
            empty_thinking: true,
            ..Self::new(types)
        }
    }

    /// Picks the kinds in `types`, the calls without their results, for
    /// what counts the records and prints none.
    pub fn new(types: Types) -> Self {
        Self {
            empty_thinking: false,
            outcomes: None,
            plans: HashMap::new(),
            prompts: Prompts::default(),
            results: Results::default(),
            turn: 0,
            turns: false,
            types,
        }
    }

    /// Notes what the calls whose results are on `entry` came to.
    fn note_outcomes(&mut self, entry: &Entry) {
        let Some(ref mut outcomes) = self.outcomes else {
            return;
        };
        for block in &entry.content.blocks {
            if let Block::ToolResult(ref result) = *block
                && let Some(slot) = outcomes.get_mut(&result.tool_use_id)
            {
                let plan = self.plans.get(&result.tool_use_id).map(String::as_str);
                *slot = Some(Outcome {
                    is_error: result.is_error,
                    // As for the result itself, in `Results::pick`.
                    denial: denial(Answer::of(entry, result)).kind,
                    text: without_repeated_plan(PLAN, plan, &result.text)
                        .unwrap_or_else(|| result.text.clone()),
                });
            }
        }
    }

    /// Notes the plans the calls on `entry` show for approval.
    fn note_plans(&mut self, entry: &Entry) {
        for block in &entry.content.blocks {
            if let Block::ToolUse(ref call) = *block
                && call.name == PLAN
                && let Some(plan) = plan_of(&call.input)
            {
                self.plans.insert(call.id.clone(), plan.to_owned());
            }
        }
    }

    /// Picks the kinds in `types`, each call with its result and each
    /// record numbered with its turn, for printing them.
    pub fn with_results(types: Types) -> Self {
        Self {
            outcomes: types.calls.then(HashMap::new),
            turns: true,
            ..Self::new(types)
        }
    }
}

impl Pick for Records {
    type Record = Turn;

    fn outcome(&self, tool_use_id: &str) -> Option<&Outcome> {
        self.outcomes.as_ref()?.get(tool_use_id)?.as_ref()
    }

    fn pick(&mut self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, Draft<Turn>)> {
        let mut turns: Vec<(usize, Draft<Turn>)> = Vec::new();
        match entry.role {
            Role::User => {
                // With results picked too, `wants` may have accepted the line
                // for its results only. A typed prompt is never on a line
                // with a result, and a rejection's index comes after the
                // line's blocks, so a prompt's index cannot clash with a
                // result's in the check for resumed copies.
                let reason = classify(entry);
                if self.types.prompts
                    && matches!(
                        reason,
                        Reason::Human | Reason::Rejection | Reason::ToolResult
                    )
                {
                    let prompts = self.prompts.pick(spot, entry).into_iter();
                    turns.extend(prompts.map(|(index, prompt)| (index, prompt.map(Turn::Prompt))));
                }
                if self.types.summaries && reason == Reason::CompactSummary {
                    let summaries = Summaries.pick(spot, entry).into_iter();
                    turns.extend(
                        summaries.map(|(index, summary)| (index, summary.map(Turn::Summary))),
                    );
                }
                if self.types.results {
                    let results = self.results.pick(spot, entry).into_iter();
                    turns.extend(results.map(|(index, result)| (index, result.map(Turn::Result))));
                }
            }
            Role::Assistant => {
                if self.types.replies {
                    let replies = Replies.pick(spot, entry).into_iter();
                    turns.extend(replies.map(|(index, reply)| (index, reply.map(Turn::Reply))));
                }
                if self.types.thinking {
                    let mut thoughts = Thoughts {
                        empty: self.empty_thinking,
                    };
                    let picked = thoughts.pick(spot, entry).into_iter();
                    turns.extend(
                        picked.map(|(index, thought)| (index, thought.map(Turn::Thinking))),
                    );
                }
                if self.types.calls {
                    let calls = Tools.pick(spot, entry);
                    if let Some(ref mut outcomes) = self.outcomes {
                        for pair in &calls {
                            outcomes.insert(pair.1.record().tool_use_id.clone(), None);
                        }
                    }
                    turns.extend(
                        calls
                            .into_iter()
                            .map(|(index, call)| (index, call.map(Turn::Call))),
                    );
                }
            }
            Role::AiTitle | Role::Other => {}
        }
        turns.sort_by_key(|pair| pair.0);
        if self.turns {
            for pair in &mut turns {
                pair.1.set_turn(self.turn);
            }
        }
        turns
    }

    fn roles(&self) -> Roles {
        Roles {
            assistant: self.types.reads_assistant(),
            // A prompt can answer the dialog of a call.
            calls_to: if self.types.prompts { &DIALOGS } else { &[] },
            // A call's result is on a user line, and so is a prompt that
            // starts a turn.
            user: self.turns
                || self.types.prompts
                || self.types.results
                || self.types.summaries
                || self.outcomes.is_some(),
        }
    }

    fn superseded(&self, record: &Turn) -> bool {
        match *record {
            Turn::Prompt(ref prompt) => self.prompts.superseded(prompt),
            Turn::Call(_)
            | Turn::Reply(_)
            | Turn::Result(_)
            | Turn::Summary(_)
            | Turn::Thinking(_) => false,
        }
    }

    fn wants(&mut self, entry: &Entry) -> bool {
        // Always called, so the results see every call before its result,
        // and the prompts every prompt sent again.
        let reason = classify(entry);
        let result = self.types.results && self.results.wants(entry);
        let prompt = self.types.prompts && self.prompts.wants_as(entry, reason);
        match entry.role {
            Role::User => {
                if self.turns && reason == Reason::Human {
                    self.turn = self.turn.saturating_add(1);
                }
                self.note_outcomes(entry);
            }
            Role::Assistant if self.outcomes.is_some() => self.note_plans(entry),
            Role::AiTitle | Role::Assistant | Role::Other => {}
        }
        match entry.role {
            Role::User => {
                result || prompt || (self.types.summaries && reason == Reason::CompactSummary)
            }
            Role::Assistant => self.types.replies || self.types.calls || self.types.thinking,
            Role::AiTitle | Role::Other => false,
        }
    }
}

impl Pick for Prompts {
    type Record = Prompt;

    fn pick(&mut self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, Draft<Prompt>)> {
        let reason = classify(entry);
        let mut picked = Vec::new();
        // Only the first prompt on the line follows the reply.
        let mut after = Some(self.take_after(entry));
        if reason != Reason::ToolResult {
            // A rejection is on the line of its tool result: its index
            // comes after the line's blocks.
            let (index, typed, kind, command) = comment_in(entry).map_or_else(
                || typed_prompt(entry),
                |comment| {
                    (
                        entry.content.blocks.len(),
                        comment,
                        Kind::Rejection,
                        String::new(),
                    )
                },
            );
            let reply = after.take().unwrap_or_default();
            picked.push((index, drafted(spot, entry, &typed, kind, command, &reply)));
        }
        // An answer can be on the line of a comment on another call, when
        // the calls were made together.
        if matches!(reason, Reason::ToolResult | Reason::Rejection) {
            picked.extend(
                self.responses(entry)
                    .into_iter()
                    .map(|(index, typed, kind)| {
                        let reply = after.take().unwrap_or_default();
                        (
                            index,
                            drafted(spot, entry, &typed, kind, String::new(), &reply),
                        )
                    }),
            );
        }
        picked
    }

    fn roles(&self) -> Roles {
        // The calls that show a dialog are on assistant lines, and so are
        // the replies.
        Roles {
            assistant: self.replies,
            calls_to: &DIALOGS,
            ..Roles::USER
        }
    }

    fn superseded(&self, record: &Prompt) -> bool {
        !record.parent_uuid.is_empty()
            && self
                .last
                .get(&record.parent_uuid)
                .is_some_and(|last| *last != record.source.uuid)
    }

    fn wants(&mut self, entry: &Entry) -> bool {
        self.wants_as(entry, classify(entry))
    }
}

impl Prompts {
    /// Notes the first line of the last reply on `entry`, an assistant
    /// line of the session's own, leaving out what the harness wrote in
    /// place of a reply; a line without one carries on the reply of the
    /// line it follows.
    fn note_reply(&mut self, entry: &Entry) {
        if entry.flags.sidechain {
            return;
        }
        let own = entry
            .content
            .blocks
            .iter()
            .rev()
            .find_map(|block| match *block {
                Block::Text(ref written) if !is_no_response(&entry.model, written) => {
                    written.lines().map(str::trim).find(|line| !line.is_empty())
                }
                Block::Other
                | Block::Text(_)
                | Block::Thinking(_)
                | Block::ToolResult(_)
                | Block::ToolUse(_) => None,
            });
        let reply = match own {
            Some(line) => {
                line.clone_into(&mut self.after);
                line.to_owned()
            }
            None => self
                .branch
                .get(&entry.parent_uuid)
                .cloned()
                .unwrap_or_default(),
        };
        self.branch.insert(entry.uuid.clone(), reply);
    }

    /// What the user answered through a dialog on `entry`, a line of tool
    /// results: each answer's index, after the line's blocks and its
    /// rejection's so that none clashes with a result's in the check for
    /// resumed copies, its text, and its kind.
    fn responses(&self, entry: &Entry) -> Vec<(usize, String, Kind)> {
        let after = entry.content.blocks.len().saturating_add(1);
        entry
            .content
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(index, block)| {
                let Block::ToolResult(ref result) = *block else {
                    return None;
                };
                let tool = self.dialogs.get(&result.tool_use_id)?;
                let found = response(tool, Answer::of(entry, result))?;
                let (text, kind) = match found {
                    Response::Answer(text) => (text, Kind::Answer),
                    Response::Approval(text) => (text, Kind::Approval),
                    Response::Rejection(text) => (text, Kind::Rejection),
                    Response::TypedAnswer(text) => (text, Kind::TypedAnswer),
                };
                Some((after.saturating_add(index), text, kind))
            })
            .collect()
    }

    /// The first line of the reply the prompt on `entry` follows, on its
    /// branch, or the last one seen when its branch is not known; the
    /// lines after it follow no reply until another comes.
    fn take_after(&mut self, entry: &Entry) -> String {
        let seen = mem::take(&mut self.after);
        if !self.replies {
            return String::new();
        }
        let after = self.branch.get(&entry.parent_uuid).cloned().unwrap_or(seen);
        self.branch.insert(entry.uuid.clone(), String::new());
        after
    }

    /// What `Pick::wants` says of `entry`, which `classify` says `reason`
    /// of, for a picker that has classified it already.
    fn wants_as(&mut self, entry: &Entry, reason: Reason) -> bool {
        if entry.role == Role::Assistant {
            for block in &entry.content.blocks {
                if let Block::ToolUse(ref call) = *block
                    && matches!(call.name.as_str(), ASK | PLAN)
                {
                    self.dialogs.insert(call.id.clone(), call.name.clone());
                }
            }
            if self.replies {
                self.note_reply(entry);
            }
            return false;
        }
        if self.replies {
            let reply = if reason == Reason::Human {
                String::new()
            } else {
                self.branch
                    .get(&entry.parent_uuid)
                    .cloned()
                    .unwrap_or_default()
            };
            self.branch.insert(entry.uuid.clone(), reply);
        }
        match reason {
            Reason::Human => {
                if !entry.parent_uuid.is_empty() {
                    self.last
                        .insert(entry.parent_uuid.clone(), entry.uuid.clone());
                }
                true
            }
            Reason::Rejection => true,
            Reason::ToolResult => !self.responses(entry).is_empty(),
            Reason::CompactSummary
            | Reason::Empty
            | Reason::HarnessTag
            | Reason::Interrupted
            | Reason::Meta
            | Reason::NonHumanOrigin
            | Reason::NotUser
            | Reason::Sidechain => false,
        }
    }

    /// Picks the prompts, each with the first line of the reply before it,
    /// for the digest.
    pub fn with_replies() -> Self {
        Self {
            replies: true,
            ..Self::default()
        }
    }
}

impl Pick for Summaries {
    type Record = Summary;

    fn pick(&mut self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, Draft<Summary>)> {
        let mut masked = Counts::default();
        let (text, redacted) = spot.mask.text(entry.content.text.trim(), &mut masked);
        let source = source(spot, entry, &mut masked);
        vec![(
            0,
            Draft::new(Summary {
                head: spot.head(entry),
                text,
                redacted,
                source,
                search: Search::default(),
                masked,
            }),
        )]
    }

    fn roles(&self) -> Roles {
        Roles::USER
    }

    fn wants(&mut self, entry: &Entry) -> bool {
        classify(entry) == Reason::CompactSummary
    }
}

impl Pick for Replies {
    type Record = Reply;

    fn pick(&mut self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, Draft<Reply>)> {
        blocks(entry, |block| match *block {
            Block::Text(ref written)
                if !written.trim().is_empty() && !is_no_response(&entry.model, written) =>
            {
                Some(assistant_block(spot, entry, written))
            }
            Block::Text(_)
            | Block::Other
            | Block::Thinking(_)
            | Block::ToolResult(_)
            | Block::ToolUse(_) => None,
        })
    }

    fn roles(&self) -> Roles {
        Roles::ASSISTANT
    }

    fn wants(&mut self, entry: &Entry) -> bool {
        entry.role == Role::Assistant
    }
}

impl Pick for Thoughts {
    type Record = Reply;

    fn pick(&mut self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, Draft<Reply>)> {
        let empty = self.empty;
        blocks(entry, |block| match *block {
            Block::Thinking(ref written) if empty || !written.trim().is_empty() => {
                Some(assistant_block(spot, entry, written))
            }
            Block::Other
            | Block::Text(_)
            | Block::Thinking(_)
            | Block::ToolResult(_)
            | Block::ToolUse(_) => None,
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

    fn pick(&mut self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, Draft<ToolResult>)> {
        blocks(entry, |block| match *block {
            Block::ToolResult(ref result) => {
                let call = self.calls.get(&result.tool_use_id);
                let mut masked = Counts::default();
                let mut input = call.map_or(Value::Null, |found| found.input.clone());
                let input_redacted = spot.mask.json(&mut input, &mut masked);
                let shown = call.and_then(|found| {
                    without_repeated_plan(&found.name, plan_of(&found.input), &result.text)
                });
                let (text, text_redacted) = spot
                    .mask
                    .text(shown.as_deref().unwrap_or(&result.text), &mut masked);
                let source = source(spot, entry, &mut masked);
                Some(ToolResult {
                    head: spot.head(entry),
                    tool: call.map_or_else(String::new, |found| found.name.clone()),
                    input,
                    tool_use_id: result.tool_use_id.clone(),
                    is_error: result.is_error,
                    // The line's keys speak for its one result; see
                    // `comment_in`.
                    denial: denial(Answer::of(entry, result)).kind,
                    text,
                    subagent: spot.subagent(),
                    spawned: spot.spawned(&mut masked),
                    redacted: input_redacted || text_redacted || spot.started.redacted,
                    source,
                    search: Search::default(),
                    skill: call.map_or_else(String::new, |found| found.skill.clone()),
                    touched: call
                        .map_or_else(Touch::default, |found| touch(&found.name, &found.input)),
                    masked,
                })
            }
            Block::Other | Block::Text(_) | Block::Thinking(_) | Block::ToolUse(_) => None,
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

    fn pick(&mut self, spot: &Spot<'_>, entry: &Entry) -> Vec<(usize, Draft<ToolCall>)> {
        blocks(entry, |block| match *block {
            Block::ToolUse(ref call) => {
                let mut masked = Counts::default();
                let mut input = call.input.clone();
                let redacted = spot.mask.json(&mut input, &mut masked);
                let source = source(spot, entry, &mut masked);
                Some(ToolCall {
                    head: spot.head(entry),
                    tool: call.name.clone(),
                    input,
                    tool_use_id: call.id.clone(),
                    result: None,
                    skill: entry.skill.clone(),
                    subagent: spot.subagent(),
                    spawned: spot.spawned(&mut masked),
                    usage: None,
                    redacted: redacted || spot.started.redacted,
                    source,
                    search: Search::default(),
                    model: entry.model.clone(),
                    touched: touch(&call.name, &call.input),
                    masked,
                })
            }
            Block::Other | Block::Text(_) | Block::Thinking(_) | Block::ToolResult(_) => None,
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
            kind: Some(self.kind.name()),
            ..Extra::default()
        }
    }

    fn grep_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }

    fn head_mut(&mut self) -> &mut Head {
        &mut self.head
    }

    fn kind(&self) -> &'static str {
        "prompt"
    }

    fn labels_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.command]
    }

    fn masked_mut(&mut self) -> &mut Counts {
        &mut self.masked
    }

    fn search_mut(&mut self) -> &mut Search {
        &mut self.search
    }

    fn superseded_mut(&mut self) -> Option<&mut bool> {
        Some(&mut self.superseded)
    }
}

impl Select for Reply {
    fn extra(&self) -> Extra<'_> {
        Extra {
            skill: Some(&self.skill),
            ..Extra::default()
        }
    }

    fn grep_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }

    fn head_mut(&mut self) -> &mut Head {
        &mut self.head
    }

    fn kind(&self) -> &'static str {
        "reply"
    }

    fn labels_mut(&mut self) -> Vec<&mut String> {
        vec![
            &mut self.model,
            &mut self.skill,
            &mut self.subagent,
            &mut self.spawned.subagent_type,
        ]
    }

    fn masked_mut(&mut self) -> &mut Counts {
        &mut self.masked
    }

    fn search_mut(&mut self) -> &mut Search {
        &mut self.search
    }

    fn usage_mut(&mut self) -> Option<&mut Option<Usage>> {
        Some(&mut self.usage)
    }
}

impl Select for Summary {
    fn extra(&self) -> Extra<'_> {
        Extra::default()
    }

    fn grep_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }

    fn head_mut(&mut self) -> &mut Head {
        &mut self.head
    }

    fn kind(&self) -> &'static str {
        "summary"
    }

    fn labels_mut(&mut self) -> Vec<&mut String> {
        Vec::new()
    }

    fn masked_mut(&mut self) -> &mut Counts {
        &mut self.masked
    }

    fn search_mut(&mut self) -> &mut Search {
        &mut self.search
    }
}

impl Select for ToolCall {
    fn extra(&self) -> Extra<'_> {
        Extra {
            skill: Some(&self.skill),
            tool: Some(&self.tool),
            touched: Some(&self.touched),
            ..Extra::default()
        }
    }

    /// Each string in the input, named for where it is, as in
    /// `input.edits[0].new_string`.
    fn grep_fields(&self) -> Vec<(String, Cow<'_, str>)> {
        let mut fields = Vec::new();
        strings_in(&self.input, "input".to_owned(), &mut fields);
        fields
    }

    /// The input as compact JSON.
    fn grep_text(&self) -> Cow<'_, str> {
        Cow::Owned(self.input.to_string())
    }

    fn head_mut(&mut self) -> &mut Head {
        &mut self.head
    }

    fn kind(&self) -> &'static str {
        "call"
    }

    fn labels_mut(&mut self) -> Vec<&mut String> {
        let mut labels = vec![
            &mut self.tool,
            &mut self.model,
            &mut self.skill,
            &mut self.subagent,
            &mut self.spawned.subagent_type,
        ];
        labels.extend(self.result.as_mut().map(|result| &mut result.denial));
        labels
    }

    fn masked_mut(&mut self) -> &mut Counts {
        &mut self.masked
    }

    fn outcome_mut(&mut self) -> Option<(&str, &mut Option<Outcome>, &mut bool)> {
        Some((&self.tool_use_id, &mut self.result, &mut self.redacted))
    }

    fn search_mut(&mut self) -> &mut Search {
        &mut self.search
    }

    fn usage_mut(&mut self) -> Option<&mut Option<Usage>> {
        Some(&mut self.usage)
    }
}

impl Select for Turn {
    fn extra(&self) -> Extra<'_> {
        selected(self).extra()
    }

    fn grep_fields(&self) -> Vec<(String, Cow<'_, str>)> {
        selected(self).grep_fields()
    }

    fn grep_text(&self) -> Cow<'_, str> {
        selected(self).grep_text()
    }

    fn head_mut(&mut self) -> &mut Head {
        selected_mut(self).head_mut()
    }

    fn kind(&self) -> &'static str {
        // A thinking block is a `Reply`, whose own kind is `reply`.
        self.type_name()
    }

    fn labels_mut(&mut self) -> Vec<&mut String> {
        selected_mut(self).labels_mut()
    }

    fn masked_mut(&mut self) -> &mut Counts {
        selected_mut(self).masked_mut()
    }

    fn outcome_mut(&mut self) -> Option<(&str, &mut Option<Outcome>, &mut bool)> {
        selected_mut(self).outcome_mut()
    }

    fn search_mut(&mut self) -> &mut Search {
        selected_mut(self).search_mut()
    }

    fn superseded_mut(&mut self) -> Option<&mut bool> {
        selected_mut(self).superseded_mut()
    }

    fn usage_mut(&mut self) -> Option<&mut Option<Usage>> {
        selected_mut(self).usage_mut()
    }
}

impl Select for ToolResult {
    fn extra(&self) -> Extra<'_> {
        Extra {
            denial: Some(&self.denial),
            is_error: Some(self.is_error),
            skill: Some(&self.skill),
            tool: Some(&self.tool),
            touched: Some(&self.touched),
            ..Extra::default()
        }
    }

    fn grep_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }

    fn head_mut(&mut self) -> &mut Head {
        &mut self.head
    }

    fn kind(&self) -> &'static str {
        "result"
    }

    fn labels_mut(&mut self) -> Vec<&mut String> {
        vec![
            &mut self.tool,
            &mut self.denial,
            &mut self.skill,
            &mut self.subagent,
            &mut self.spawned.subagent_type,
        ]
    }

    fn masked_mut(&mut self) -> &mut Counts {
        &mut self.masked
    }

    fn search_mut(&mut self) -> &mut Search {
        &mut self.search
    }
}

/// The prompt `typed` on `entry`, of `kind`, after the reply whose first
/// line is `after`, both masked. Only a typed prompt can be sent again in
/// place of another, so only it keeps the line it follows.
fn drafted(
    spot: &Spot<'_>,
    entry: &Entry,
    typed: &str,
    kind: Kind,
    command: String,
    after: &str,
) -> Draft<Prompt> {
    let mut masked = Counts::default();
    let (text, redacted) = spot.mask.text(typed.trim(), &mut masked);
    let (reply, _) = spot.mask.text(after, &mut masked);
    let source = source(spot, entry, &mut masked);
    Draft::new(Prompt {
        head: spot.head(entry),
        text,
        kind,
        command,
        permission_mode: entry.permission_mode.clone(),
        superseded: false,
        redacted,
        source,
        search: Search::default(),
        masked,
        parent_uuid: if matches!(kind, Kind::Text | Kind::Slash) {
            entry.parent_uuid.clone()
        } else {
            String::new()
        },
        after: reply,
    })
}

/// The record of `written`, a text or thinking block of the assistant line
/// `entry`, trimmed and masked.
fn assistant_block(spot: &Spot<'_>, entry: &Entry, written: &str) -> Reply {
    let mut masked = Counts::default();
    let (text, redacted) = spot.mask.text(written.trim(), &mut masked);
    let source = source(spot, entry, &mut masked);
    Reply {
        head: spot.head(entry),
        text,
        model: entry.model.clone(),
        skill: entry.skill.clone(),
        subagent: spot.subagent(),
        spawned: spot.spawned(&mut masked),
        usage: None,
        redacted: redacted || spot.started.redacted,
        source,
        search: Search::default(),
        masked,
    }
}

/// The records `make` builds from the blocks of `entry`, each with its
/// block's index.
fn blocks<T>(entry: &Entry, make: impl Fn(&Block) -> Option<T>) -> Vec<(usize, Draft<T>)>
where
    T: Select,
{
    entry
        .content
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| make(block).map(|record| (index, Draft::new(record))))
        .collect()
}

/// A typed prompt's block index, text, kind, and command: a slash command
/// folded back into what was typed, or the text as is.
fn typed_prompt(entry: &Entry) -> (usize, String, Kind, String) {
    fold(&entry.content.text).map_or_else(
        || (0, entry.content.text.clone(), Kind::Text, String::new()),
        |folded| (0, folded.text, Kind::Slash, folded.command),
    )
}

/// The record inside `turn`.
fn selected(turn: &Turn) -> &dyn Select {
    match *turn {
        Turn::Prompt(ref prompt) => prompt,
        Turn::Reply(ref reply) | Turn::Thinking(ref reply) => reply,
        Turn::Call(ref call) => call,
        Turn::Result(ref result) => result,
        Turn::Summary(ref summary) => summary,
    }
}

/// Adds each string in `value`, at `path`, to `fields` with its path, in
/// the order the input has them: `.key` for a key of an object, `["key"]`
/// for one that could read as more of a path, and `[n]` for an item of an
/// array.
fn strings_in<'value>(
    value: &'value Value,
    path: String,
    fields: &mut Vec<(String, Cow<'value, str>)>,
) {
    match *value {
        Value::String(ref text) => fields.push((path, Cow::Borrowed(text))),
        Value::Array(ref items) => {
            for (index, item) in items.iter().enumerate() {
                strings_in(item, format!("{path}[{index}]"), fields);
            }
        }
        Value::Object(ref keys) => {
            for (key, item) in keys {
                let plain = !key.is_empty()
                    && key
                        .chars()
                        .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-');
                let step = if plain {
                    format!("{path}.{key}")
                } else {
                    format!("{path}[{}]", Value::String(key.clone()))
                };
                strings_in(item, step, fields);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// The record inside `turn`, to change.
fn selected_mut(turn: &mut Turn) -> &mut dyn Select {
    match *turn {
        Turn::Prompt(ref mut prompt) => prompt,
        Turn::Reply(ref mut reply) | Turn::Thinking(ref mut reply) => reply,
        Turn::Call(ref mut call) => call,
        Turn::Result(ref mut result) => result,
        Turn::Summary(ref mut summary) => summary,
    }
}

/// Where `entry` is, counting what masking the path counted in `masked`.
fn source(spot: &Spot<'_>, entry: &Entry, masked: &mut Counts) -> Source {
    Source {
        file: spot.mask.meta(&spot.file.path.to_string_lossy(), masked),
        line: spot.line,
        uuid: entry.uuid.clone(),
    }
}

/// What a call of `tool` with `input`, before masking, changed: the file
/// an edit or a write went to, or a shell command, whose words may name it.
fn touch(tool: &str, input: &Value) -> Touch {
    let text = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    match tool {
        "Edit" | "MultiEdit" | "Write" => Touch::Path(text("file_path")),
        "NotebookEdit" => Touch::Path(text("notebook_path")),
        "Bash" => Touch::Command(text("command")),
        _ => Touch::Nothing,
    }
}

/// The plan in the input of a call that shows one for approval.
fn plan_of(input: &Value) -> Option<&str> {
    input.get("plan").and_then(Value::as_str)
}
