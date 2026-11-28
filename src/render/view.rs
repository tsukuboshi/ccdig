//! What the records show for a reader, apart from how a format writes it:
//! which records stand under their own heading, what each heading says,
//! and which parts of each record `--fields` keeps. The Markdown of
//! `records` writes it. The texts stay as the records hold them, masked
//! but not escaped, and the words stay out: a format escapes and words
//! them its own way.

use std::collections::{HashMap, HashSet};

use super::json::Fields;
use crate::failure::Failure;
use serde_json::{Map, Value};

use crate::record::{Kind, MatchLine, Outcome, Record, Source, ToolCall, ToolResult, Turn};
use crate::text::one_line;
use crate::when::Stamp;

/// The records, grouped by transcript file.
#[derive(Debug, Eq, PartialEq)]
pub struct View<'records> {
    /// How many records show.
    pub count: Count,
    /// One section per transcript file, in the order of the records.
    pub sections: Vec<Section<'records>>,
}

/// How many records show: those a search matched, by type, and apart from
/// them, those `--around` added as context.
#[derive(Debug, Eq, PartialEq)]
pub struct Count {
    /// How many records `--around` added as context.
    pub context: usize,
    /// How many records a search matched.
    pub matched: usize,
    /// How many of the matched records are each type, as `--type` names
    /// it, in the order a conversation has them, without the types that
    /// have none.
    pub types: Vec<(&'static str, usize)>,
}

/// The records of one transcript file.
#[derive(Debug, Eq, PartialEq)]
pub struct Section<'records> {
    /// What the heading says of the file, in the order shown.
    pub facts: Vec<Fact<'records>>,
    /// The records, each under its own heading.
    pub items: Vec<Item<'records>>,
    /// The session's title, or empty.
    pub title: &'records str,
}

/// Something the heading of a section says of its file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fact<'records> {
    /// What the caller wrote the sub-agent was for.
    Description(&'records str),
    /// The project directory's name.
    Project(&'records str),
    /// The session ID.
    Session(&'records str),
    /// The sub-agent file name.
    Subagent(&'records str),
    /// The kind of sub-agent.
    SubagentType(&'records str),
}

/// One record under its own heading.
#[derive(Debug, Eq, PartialEq)]
pub struct Item<'records> {
    /// What the record holds.
    pub body: Body<'records>,
    /// What kind of record it is.
    pub label: Label,
    /// Whether a search matched the record, rather than `--around` adding
    /// it as context.
    pub matched: bool,
    /// When the record was written.
    pub stamp: Option<Stamp>,
    /// The sub-agent the record comes from.
    pub subagent: Option<&'records str>,
    /// The record's turn, when it follows a prompt.
    pub turn: Option<usize>,
}

/// What kind of record an item is, as its heading names it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Label {
    /// A prompt that answers Claude Code's questions with the options
    /// offered.
    Answer,
    /// A plan the user approved.
    Approval,
    /// A tool call.
    Call,
    /// A prompt typed as plain text or as a slash command.
    Prompt,
    /// A tool call or a plan the user turned down.
    Rejection,
    /// A text block of the assistant.
    Reply,
    /// A tool result.
    Result,
    /// A compaction summary.
    Summary,
    /// A thinking block of the assistant.
    Thinking,
    /// A prompt that answers Claude Code's questions in the user's own
    /// words.
    TypedAnswer,
}

/// What a record holds.
#[derive(Debug, Eq, PartialEq)]
pub enum Body<'records> {
    /// A tool call.
    Call(CallBody<'records>),
    /// A tool result.
    Result(ResultBody<'records>),
    /// A prompt, a reply, a thinking block, or a summary.
    Text(Passage<'records>),
}

/// A record's text, or the lines of it a search matched.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Passage<'records> {
    /// Neither, as `--fields` leaves both out.
    Hidden,
    /// The lines a search matched and those around them, in runs.
    Lines(Vec<Run<'records>>),
    /// The text.
    Text(&'records str),
}

/// Adjacent lines of one field that a search listed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Run<'records> {
    /// Where the lines are, such as `input.command`, or `None` for the
    /// record's text.
    pub field: Option<&'records str>,
    /// The number of the first line.
    pub first: usize,
    /// The number of the last line.
    pub last: usize,
    /// The lines, each saying whether a search matched it.
    pub lines: &'records [MatchLine],
}

/// A tool's name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tool<'records> {
    /// The tool's name.
    Named(&'records str),
    /// The call was not found, so the name is not known.
    Unknown,
}

/// A tool call.
#[derive(Debug, Eq, PartialEq)]
pub struct CallBody<'records> {
    /// What the call came to, or `None` when `--fields` leaves it out.
    pub ending: Option<Ending<'records>>,
    /// The call's input.
    pub input: Input<'records>,
    /// The tool, or `None` when `--fields` leaves it out.
    pub tool: Option<Tool<'records>>,
}

/// A tool call's input.
#[derive(Debug, Eq, PartialEq)]
pub enum Input<'records> {
    /// Neither the input nor its matched lines, as `--fields` leaves both
    /// out.
    Hidden,
    /// The lines of the input a search matched, in runs.
    Lines(Vec<Run<'records>>),
    /// The input.
    Shown {
        /// The input as indented JSON, but for the strings in `texts`;
        /// `None` when those are all there is.
        json: Option<String>,
        /// Each string at the top level of the input that runs over lines,
        /// such as a plan, with its key, without its final line breaks.
        texts: Vec<(&'records str, &'records str)>,
    },
}

/// A tool result.
#[derive(Debug, Eq, PartialEq)]
pub struct ResultBody<'records> {
    /// Why the call did not run, or `None` when it ran or `--fields`
    /// leaves it out.
    pub denial: Option<&'records str>,
    /// Whether the tool reported a failure, or `None` when `--fields`
    /// leaves it out.
    pub failed: Option<bool>,
    /// The output, or the lines of it a search matched.
    pub passage: Passage<'records>,
    /// The calling tool, or `None` when `--fields` leaves it out.
    pub tool: Option<Tool<'records>>,
}

/// What a tool call came to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ending<'records> {
    /// Its file has no result for it.
    Missing,
    /// It has a result.
    Ran {
        /// Why the call did not run, or `None` when it ran.
        denial: Option<&'records str>,
        /// Whether the tool reported a failure.
        failed: bool,
        /// The output, or `None` when the result stands under its own
        /// heading.
        output: Option<&'records str>,
    },
}

/// The key that pairs a call and its result: the file and the call's ID.
type Key<'records> = (&'records str, &'records str);

/// `turns` as they show, with the parts `fields` keeps. A result shows
/// under its call, and not again on its own unless it lists matched lines
/// or a search matched it and not its call; it then shows under its own
/// heading, and the call says only how it ended.
///
/// # Errors
///
/// When an input cannot be encoded.
pub fn records<'records>(
    turns: &'records [Turn],
    fields: &Fields,
) -> Result<View<'records>, Failure> {
    let standing = standing(turns);
    let shown: Vec<&Turn> = turns
        .iter()
        .filter(|turn| match **turn {
            Turn::Result(ref result) => {
                standing.contains(&key_of(&result.source, &result.tool_use_id))
            }
            Turn::Call(_)
            | Turn::Prompt(_)
            | Turn::Reply(_)
            | Turn::Summary(_)
            | Turn::Thinking(_) => true,
        })
        .collect();
    let mut sections = Vec::new();
    for group in shown.chunk_by(|left, right| left.source().file == right.source().file) {
        // `chunk_by` gives no empty group.
        let Some(&first) = group.first() else {
            continue;
        };
        sections.push(Section {
            facts: facts(first, fields),
            items: group
                .iter()
                .map(|turn| item(turn, &standing, fields))
                .collect::<Result<_, _>>()?,
            title: &first.head().session_title,
        });
    }
    Ok(View {
        count: count(&shown),
        sections,
    })
}

/// How many of `shown` a search matched, by type, and how many are
/// context.
fn count(shown: &[&Turn]) -> Count {
    let matched: Vec<&str> = shown
        .iter()
        .copied()
        .filter(Record::matched)
        .map(Turn::type_name)
        .collect();
    Count {
        context: shown.len().saturating_sub(matched.len()),
        matched: matched.len(),
        types: count_types(&matched),
    }
}

/// How many of `types` are each type, in the order a conversation has
/// them, without the types that have none.
pub fn count_types(types: &[&str]) -> Vec<(&'static str, usize)> {
    /// The types in the order the count lists them.
    const ORDER: [&str; 6] = ["prompt", "thinking", "reply", "call", "result", "summary"];
    ORDER
        .into_iter()
        .filter_map(|name| {
            let count = types.iter().filter(|kind| **kind == name).count();
            (count > 0).then_some((name, count))
        })
        .collect()
}

/// What the heading of the section that starts with `first` says of its
/// file, of the keys `fields` keeps: the project, the session, and the
/// sub-agent, its kind, and what it was for, when it has them.
fn facts<'records>(first: &'records Turn, fields: &Fields) -> Vec<Fact<'records>> {
    let head = first.head();
    let mut facts = Vec::new();
    if fields.keeps("project") {
        facts.push(Fact::Project(&head.project));
    }
    if fields.keeps("session_id") {
        facts.push(Fact::Session(&head.session_id));
    }
    if let Some(name) = subagent(first, fields) {
        facts.push(Fact::Subagent(name));
    }
    if let Some(spawned) = first.spawned() {
        if fields.keeps("subagent_type") && !spawned.subagent_type.is_empty() {
            facts.push(Fact::SubagentType(&spawned.subagent_type));
        }
        if fields.keeps("subagent_description")
            && !one_line(&spawned.subagent_description).is_empty()
        {
            facts.push(Fact::Description(&spawned.subagent_description));
        }
    }
    facts
}

/// The sub-agent `turn` comes from, when it does and `fields` keeps it.
fn subagent<'records>(turn: &'records Turn, fields: &Fields) -> Option<&'records str> {
    let name = turn.subagent();
    (fields.keeps("subagent") && !name.is_empty()).then_some(name)
}

/// The results of `turns` that show under their own heading: those whose
/// call is not listed, those that list matched lines, and those a search
/// matched when it did not match their call, which then shows as context.
fn standing(turns: &[Turn]) -> HashSet<Key<'_>> {
    let answered: HashMap<Key<'_>, bool> = turns
        .iter()
        .filter_map(|turn| match *turn {
            Turn::Call(ref call) if call.result.is_some() => {
                Some((key_of(&call.source, &call.tool_use_id), call.matched()))
            }
            Turn::Call(_)
            | Turn::Prompt(_)
            | Turn::Reply(_)
            | Turn::Result(_)
            | Turn::Summary(_)
            | Turn::Thinking(_) => None,
        })
        .collect();
    turns
        .iter()
        .filter_map(|turn| {
            let Turn::Result(ref result) = *turn else {
                return None;
            };
            let key = key_of(&result.source, &result.tool_use_id);
            let alone = answered.get(&key).is_none_or(|&call_matched| {
                !result.search.matches.is_empty() || (result.matched() && !call_matched)
            });
            alone.then_some(key)
        })
        .collect()
}

/// The key of the call `id` in the file of `source`.
fn key_of<'records>(source: &'records Source, id: &'records str) -> Key<'records> {
    (&source.file, id)
}

/// `turn` under its own heading, with the parts `fields` keeps; a call
/// carries its output unless its result is in `standing`.
fn item<'records>(
    turn: &'records Turn,
    standing: &HashSet<Key<'records>>,
    fields: &Fields,
) -> Result<Item<'records>, Failure> {
    let body = match *turn {
        Turn::Prompt(ref prompt) => {
            Body::Text(passage(&prompt.text, &prompt.search.matches, fields))
        }
        Turn::Reply(ref reply) | Turn::Thinking(ref reply) => {
            Body::Text(passage(&reply.text, &reply.search.matches, fields))
        }
        Turn::Summary(ref summary) => {
            Body::Text(passage(&summary.text, &summary.search.matches, fields))
        }
        Turn::Result(ref result) => Body::Result(result_body(result, fields)),
        Turn::Call(ref call) => Body::Call(call_body(
            call,
            !standing.contains(&key_of(&call.source, &call.tool_use_id)),
            fields,
        )?),
    };
    let head = turn.head();
    Ok(Item {
        body,
        label: label(turn),
        matched: turn.matched(),
        stamp: fields.keeps("timestamp").then_some(head.timestamp),
        subagent: subagent(turn, fields),
        turn: (head.turn > 0 && fields.keeps("turn")).then_some(head.turn),
    })
}

/// What kind of record `turn` is, as its heading names it.
const fn label(turn: &Turn) -> Label {
    match *turn {
        Turn::Prompt(ref prompt) => prompt_label(prompt.kind),
        Turn::Reply(_) => Label::Reply,
        Turn::Thinking(_) => Label::Thinking,
        Turn::Summary(_) => Label::Summary,
        Turn::Result(_) => Label::Result,
        Turn::Call(_) => Label::Call,
    }
}

/// What kind of prompt one of `kind` is, as its heading names it: a slash
/// command is a prompt like plain text.
const fn prompt_label(kind: Kind) -> Label {
    match kind {
        Kind::Answer => Label::Answer,
        Kind::Approval => Label::Approval,
        Kind::Rejection => Label::Rejection,
        Kind::Slash | Kind::Text => Label::Prompt,
        Kind::TypedAnswer => Label::TypedAnswer,
    }
}

/// The lines `matches` lists of `text`, when it lists some and `fields`
/// keeps them, or else `text` when `fields` keeps it.
fn passage<'records>(
    text: &'records str,
    matches: &'records [MatchLine],
    fields: &Fields,
) -> Passage<'records> {
    if !matches.is_empty() && fields.keeps("matches") {
        return Passage::Lines(runs(matches));
    }
    if !fields.keeps("text") {
        return Passage::Hidden;
    }
    Passage::Text(text)
}

/// `matches` in runs of adjacent lines of one field.
fn runs(matches: &[MatchLine]) -> Vec<Run<'_>> {
    matches
        .chunk_by(|left, right| {
            left.field == right.field && left.line.checked_add(1) == Some(right.line)
        })
        // `chunk_by` gives no empty run.
        .filter_map(|lines| {
            let (first, last) = (lines.first()?, lines.last()?);
            Some(Run {
                field: (first.field != "text").then_some(first.field.as_str()),
                first: first.line,
                last: last.line,
                lines,
            })
        })
        .collect()
}

/// `name` as a tool, or unknown when it is empty, as when the call was not
/// found.
const fn tool(name: &str) -> Tool<'_> {
    if name.is_empty() {
        return Tool::Unknown;
    }
    Tool::Named(name)
}

/// The parts of `call` that `fields` keeps, its output when `output`.
fn call_body<'records>(
    call: &'records ToolCall,
    output: bool,
    fields: &Fields,
) -> Result<CallBody<'records>, Failure> {
    let input = if !call.search.matches.is_empty() && fields.keeps("matches") {
        Input::Lines(runs(&call.search.matches))
    } else if fields.keeps("input") {
        shown_input(&call.input)?
    } else {
        Input::Hidden
    };
    Ok(CallBody {
        ending: fields
            .keeps("result")
            .then(|| ending(call.result.as_ref(), output)),
        input,
        tool: fields.keeps("tool").then(|| tool(&call.tool)),
    })
}

/// `input` as indented JSON, but for each string at its top level that
/// runs over lines, such as a plan, which is kept apart so that it reads
/// as written. The JSON is left out when those strings are all there is.
fn shown_input(input: &Value) -> Result<Input<'_>, Failure> {
    let texts: Vec<(&str, &str)> = input
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| {
            let text = value.as_str().filter(|text| text.contains('\n'))?;
            Some((key.as_str(), text.trim_end_matches('\n')))
        })
        .collect();
    if texts.is_empty() {
        let json = serde_json::to_string_pretty(input).map_err(Failure::Json)?;
        return Ok(Input::Shown {
            json: Some(json),
            texts,
        });
    }
    let mut rest = input.clone();
    if let Value::Object(ref mut map) = rest {
        for &(key, _) in &texts {
            map.remove(key);
        }
    }
    let json = if rest.as_object().is_some_and(Map::is_empty) {
        None
    } else {
        Some(serde_json::to_string_pretty(&rest).map_err(Failure::Json)?)
    };
    Ok(Input::Shown { json, texts })
}

/// The parts of `result` that `fields` keeps.
fn result_body<'records>(result: &'records ToolResult, fields: &Fields) -> ResultBody<'records> {
    ResultBody {
        denial: (!result.denial.is_empty() && fields.keeps("denial"))
            .then_some(result.denial.as_str()),
        failed: fields.keeps("is_error").then_some(result.is_error),
        passage: passage(&result.text, &result.search.matches, fields),
        tool: fields.keeps("tool").then(|| tool(&result.tool)),
    }
}

/// What a call came to, with the output when `output`.
fn ending(result: Option<&Outcome>, output: bool) -> Ending<'_> {
    result.map_or(Ending::Missing, |found| Ending::Ran {
        denial: (!found.denial.is_empty()).then_some(found.denial.as_str()),
        failed: found.is_error,
        output: output.then_some(found.text.as_str()),
    })
}
