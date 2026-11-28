//! What the records show for a reader, apart from how a format writes it:
//! which records stand under their own heading, what each heading says,
//! and which parts of each record `--fields` keeps. The Markdown and the
//! HTML of `records` write it. The texts stay as the records hold them, masked
//! but not escaped, and the words stay out: a format escapes and words
//! them its own way.

use std::collections::{HashMap, HashSet};

use super::json::Fields;
use crate::failure::Failure;
use serde_json::{Map, Value};

use crate::record::{Kind, MatchLine, Outcome, Record, Source, ToolCall, ToolResult, Turn};
use crate::text::one_line;
use crate::when::Stamp;

/// The keys of a call's input that say what the call is about, in the order
/// they are looked for: a command, a file, a pattern, a page, a search, what
/// a sub-agent is for, a skill, and a directory. The tools of Claude Code
/// and many others name them so.
const GIST_KEYS: [&str; 8] = [
    "command",
    "file_path",
    "pattern",
    "url",
    "query",
    "description",
    "skill",
    "path",
];

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
    /// The git branch of the first record, when it has one and `--fields`
    /// keeps it.
    pub branch: Option<&'records str>,
    /// What the heading says of the file, in the order shown.
    pub facts: Vec<Fact<'records>>,
    /// The records, each under its own heading.
    pub items: Vec<Item<'records>>,
    /// The ID of the call that started the sub-agent of the file, or
    /// `None` for a session's own file.
    pub parent: Option<&'records str>,
    /// The times of the earliest and the latest record shown, when
    /// `--fields` keeps them: not when the session started or ended, as the
    /// filters may leave out records at either end.
    pub span: Option<(Stamp, Stamp)>,
    /// The session's title, or empty.
    pub title: &'records str,
}

/// Something the heading of a section says of its file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Fact<'records> {
    /// The versions of Claude Code that wrote the records shown, in the
    /// order they first appear.
    AgentVersions(Vec<&'records str>),
    /// What the caller wrote the sub-agent was for.
    Description(&'records str),
    /// The models that wrote the records shown, in the order they first
    /// appear.
    Models(Vec<&'records str>),
    /// The permission modes the prompts shown were written in, in the
    /// order they first appear.
    PermissionModes(Vec<&'records str>),
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
    /// The ID of the call a call or a result is of, which pairs them, or
    /// `None` for the other records.
    pub call_id: Option<&'records str>,
    /// What kind of record it is.
    pub label: Label,
    /// Whether a search matched the record, rather than `--around` adding
    /// it as context.
    pub matched: bool,
    /// Where the record's line is, which a format may name it by.
    pub origin: Origin<'records>,
    /// When the record was written.
    pub stamp: Option<Stamp>,
    /// The sub-agent the record comes from.
    pub subagent: Option<&'records str>,
    /// The record's turn, when it follows a prompt.
    pub turn: Option<usize>,
}

/// Where a record's line is: what tells it apart from every other record,
/// whatever the filters keep.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Origin<'records> {
    /// The block's index in the line.
    pub block: usize,
    /// The record's type, as `--type` names it.
    pub kind: &'static str,
    /// The line's ID, or empty when the line has none or `--fields` leaves
    /// `source` out.
    pub uuid: &'records str,
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

impl Run<'_> {
    /// The numbers of the lines a search matched, when the run holds others
    /// too; empty when it matched them all, which needs no saying.
    pub fn hits(&self) -> Vec<String> {
        if self.lines.iter().all(|line| line.hit) {
            return Vec::new();
        }
        self.lines
            .iter()
            .filter(|line| line.hit)
            .map(|line| line.line.to_string())
            .collect()
    }
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
    /// The first line of the first key of the input that says what the
    /// call is about, such as a command or a file, or `None` when it has
    /// none, or the input does not show whole.
    pub gist: Option<&'records str>,
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
        /// What an `Edit` call changed, or `None` for the other calls.
        change: Option<Change<'records>>,
        /// The input as indented JSON, but for the strings in `texts` and
        /// `change`; `None` when those are all there is.
        json: Option<String>,
        /// Each string at the top level of the input that runs over lines,
        /// such as a plan, with its key, without its final line breaks.
        texts: Vec<(&'records str, &'records str)>,
    },
}

/// What an `Edit` call changed: the text it replaced and the text it put
/// in its place, as the call wrote them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Change<'records> {
    /// The text put in place, `new_string`.
    pub new: &'records str,
    /// The text replaced, `old_string`.
    pub old: &'records str,
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

impl<'records> Change<'records> {
    /// The lines the change takes out, each marked `-`, then the lines it
    /// puts in, each marked `+`, as a diff shows them. Nothing is compared:
    /// an `Edit` holds only the text it replaces. A line break at the end
    /// shows as an empty line, so a change of it alone shows too.
    pub fn lines(&self) -> impl Iterator<Item = (char, &'records str)> {
        split_lines(self.old)
            .map(|line| ('-', line))
            .chain(split_lines(self.new).map(|line| ('+', line)))
    }
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
        let head = first.head();
        sections.push(Section {
            branch: (fields.keeps("git_branch") && !head.git_branch.is_empty())
                .then_some(head.git_branch.as_str()),
            facts: facts(group, fields),
            items: group
                .iter()
                .map(|turn| item(turn, &standing, fields))
                .collect::<Result<_, _>>()?,
            parent: first
                .spawned()
                .map(|spawned| spawned.parent_tool_use_id.as_str())
                .filter(|id| !id.is_empty()),
            span: fields.keeps("timestamp").then(|| span(group)).flatten(),
            title: &head.session_title,
        });
    }
    Ok(View {
        count: count(&shown),
        sections,
    })
}

/// The times of the earliest and the latest of `group`.
fn span(group: &[&Turn]) -> Option<(Stamp, Stamp)> {
    let times = || group.iter().map(|turn| turn.head().timestamp);
    Some((
        times().min_by_key(Stamp::instant)?,
        times().max_by_key(Stamp::instant)?,
    ))
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

/// What the heading of the section of `group` says of its file, of the
/// keys `fields` keeps: the project, the session, the models, permission
/// modes, and versions of Claude Code its records show, and the
/// sub-agent, its kind, and what it was for, when it has them.
fn facts<'records>(group: &[&'records Turn], fields: &Fields) -> Vec<Fact<'records>> {
    let mut facts = Vec::new();
    // `chunk_by` gives no empty group.
    let Some(&first) = group.first() else {
        return facts;
    };
    let head = first.head();
    if fields.keeps("project") {
        facts.push(Fact::Project(&head.project));
    }
    if fields.keeps("session_id") {
        facts.push(Fact::Session(&head.session_id));
    }
    let listed = |key: &str, value: fn(&'records Turn) -> &'records str| {
        fields
            .keeps(key)
            .then(|| distinct(group.iter().map(|turn| value(turn))))
            .filter(|values| !values.is_empty())
    };
    facts.extend(listed("model", Turn::model).map(Fact::Models));
    facts.extend(listed("permission_mode", permission_mode).map(Fact::PermissionModes));
    facts.extend(listed("agent_version", agent_version).map(Fact::AgentVersions));
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

/// The lines of `text`, an empty one after a final line break, and none
/// for an empty `text`. A carriage return before a line break is left out,
/// as the other blocks leave it out.
fn split_lines(text: &str) -> impl Iterator<Item = &str> {
    (!text.is_empty())
        .then_some(text)
        .into_iter()
        .flat_map(|whole| whole.split('\n'))
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// The values of `values` that are not empty, each once, in the order they
/// first appear.
fn distinct<'records>(values: impl Iterator<Item = &'records str>) -> Vec<&'records str> {
    let mut seen = Vec::new();
    for value in values {
        if !value.is_empty() && !seen.contains(&value) {
            seen.push(value);
        }
    }
    seen
}

/// The permission mode a prompt was written in, or empty for the other
/// records.
fn permission_mode(turn: &Turn) -> &str {
    match *turn {
        Turn::Prompt(ref prompt) => &prompt.permission_mode,
        Turn::Call(_) | Turn::Reply(_) | Turn::Result(_) | Turn::Summary(_) | Turn::Thinking(_) => {
            ""
        }
    }
}

/// The version of Claude Code that wrote `turn`, or empty.
fn agent_version(turn: &Turn) -> &str {
    &turn.head().agent_version
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
    let source = turn.source();
    Ok(Item {
        body,
        call_id: match *turn {
            Turn::Call(ref call) => Some(call.tool_use_id.as_str()),
            Turn::Result(ref result) => Some(result.tool_use_id.as_str()),
            Turn::Prompt(_) | Turn::Reply(_) | Turn::Summary(_) | Turn::Thinking(_) => None,
        }
        .filter(|id| !id.is_empty()),
        label: label(turn),
        matched: turn.matched(),
        origin: Origin {
            block: source.block,
            kind: turn.type_name(),
            uuid: if fields.keeps("source") {
                &source.uuid
            } else {
                ""
            },
        },
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
    // The gist comes from the input only when the whole of it shows, so it
    // shows no more of the input than the body does.
    let (input, gist) = if !call.search.matches.is_empty() && fields.keeps("matches") {
        (Input::Lines(runs(&call.search.matches)), None)
    } else if fields.keeps("input") {
        (shown_input(&call.tool, &call.input)?, gist(&call.input))
    } else {
        (Input::Hidden, None)
    };
    Ok(CallBody {
        ending: fields
            .keeps("result")
            .then(|| ending(call.result.as_ref(), output)),
        gist,
        input,
        tool: fields.keeps("tool").then(|| tool(&call.tool)),
    })
}

/// The first line that is not blank of the first of `GIST_KEYS` in `input`
/// whose value is a string with one, trimmed.
fn gist(input: &Value) -> Option<&str> {
    GIST_KEYS.iter().find_map(|&key| {
        input
            .get(key)?
            .as_str()?
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
    })
}

/// `input` as indented JSON, but for each string at its top level that
/// runs over lines, such as a plan, which is kept apart so that it reads
/// as written, and for what an `Edit` call of `tool` changed, kept apart
/// as one change. The JSON is left out when those are all there is.
fn shown_input<'records>(tool: &str, input: &'records Value) -> Result<Input<'records>, Failure> {
    let change = change(tool, input);
    let apart = |key: &str| change.is_some() && matches!(key, "old_string" | "new_string");
    let texts: Vec<(&str, &str)> = input
        .as_object()
        .into_iter()
        .flatten()
        .filter(|&(key, _)| !apart(key))
        .filter_map(|(key, value)| {
            let text = value.as_str().filter(|text| text.contains('\n'))?;
            Some((key.as_str(), text.trim_end_matches('\n')))
        })
        .collect();
    if texts.is_empty() && change.is_none() {
        let json = serde_json::to_string_pretty(input).map_err(Failure::Json)?;
        return Ok(Input::Shown {
            change,
            json: Some(json),
            texts,
        });
    }
    let mut rest = input.clone();
    if let Value::Object(ref mut map) = rest {
        map.retain(|key, _| !apart(key) && !texts.iter().any(|&(text_key, _)| text_key == key));
    }
    let json = if rest.as_object().is_some_and(Map::is_empty) {
        None
    } else {
        Some(serde_json::to_string_pretty(&rest).map_err(Failure::Json)?)
    };
    Ok(Input::Shown {
        change,
        json,
        texts,
    })
}

/// What a call of `tool` with `input` changed, when it is an `Edit` whose
/// input has both strings.
fn change<'records>(tool: &str, input: &'records Value) -> Option<Change<'records>> {
    if tool != "Edit" {
        return None;
    }
    Some(Change {
        new: input.get("new_string")?.as_str()?,
        old: input.get("old_string")?.as_str()?,
    })
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
