//! Grouping records into the lines of `ccdig stats`: the records that
//! share the values of the `--by` keys, counted and added up.

use alloc::collections::BTreeMap;
use core::cmp::Reverse;
use std::collections::HashSet;
use std::io;

use jiff::tz::TimeZone;
use serde_json::Value;

use crate::failure::Failure;
use crate::record::{Group, Record as _, Turn};
use crate::transcript::entry::Usage;
use crate::when::Stamp;

/// A key `--by` groups records by.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum By {
    /// The git branch.
    Branch,
    /// The day the record was written, in the output's zone.
    Day,
    /// Why a tool call did not run, from its result.
    Denial,
    /// How a prompt was typed.
    Kind,
    /// The model of a reply or a tool call.
    Model,
    /// The month the record was written, in the output's zone.
    Month,
    /// The project directory's name.
    Project,
    /// The session ID.
    Session,
    /// The skill running at the time.
    Skill,
    /// Whether a tool result is `ok` or an `error`, the words `--status`
    /// takes.
    Status,
    /// The sub-agent file name.
    Subagent,
    /// The tool of a call or a result.
    Tool,
    /// The kind of record: `prompt`, `reply`, `call`, `result`, or
    /// `summary`.
    Type,
    /// The ISO 8601 week the record was written, in the output's zone.
    Week,
}

/// How to group records and which groups to keep.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Grouping {
    /// The keys, in the order the lines write them; none for one line of
    /// every record.
    pub by: Vec<By>,
    /// Whether the lines write `empty`: with `--type thinking`.
    pub empty: bool,
    /// How many groups to keep after sorting, `None` for all of them.
    pub limit: Option<usize>,
    /// The fewest records a group needs to be kept.
    pub min_count: usize,
    /// The order of the groups.
    pub sort: Sort,
}

/// The order of the groups: the most first, or by key.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Sort {
    /// The most tokens read from the cache first.
    CacheRead,
    /// The most characters first.
    Chars,
    /// The most records first.
    #[default]
    Count,
    /// By the values of the keys, in order, `null` last.
    Key,
    /// The most output tokens first.
    Output,
}

impl Sort {
    /// The token count the order sorts by, as `--sort` names it, or `None`
    /// for an order that does not need tokens.
    pub const fn tokens(self) -> Option<&'static str> {
        match self {
            Self::CacheRead => Some("cache_read"),
            Self::Output => Some("output"),
            Self::Chars | Self::Count | Self::Key => None,
        }
    }
}

/// Counts the characters of the UTF-8 written to it, to measure JSON
/// without keeping it.
struct CharCount(usize);

/// The values of a group's keys, in order, each `(true, "")` for `null`, so
/// that `null` sorts after every value.
type Key = Vec<(bool, String)>;

/// What one group has added up so far.
struct Tally<'records> {
    /// How many characters.
    chars: usize,
    /// How many records, the thinking blocks without text left out.
    count: usize,
    /// How many thinking blocks without text.
    empty: usize,
    /// The time of the first record.
    first: Stamp,
    /// The time of the last record.
    last: Stamp,
    /// The record whose session `--by session` describes: the first of
    /// the session itself, or the first of a sub-agent when it has none.
    lead: &'records Turn,
    /// The session IDs seen.
    sessions: HashSet<&'records str>,
    /// The tokens the records carry, `None` until one does.
    usage: Option<Usage>,
}

impl By {
    /// The name `--by` takes and the line writes the value under.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Day => "day",
            Self::Denial => "denial",
            Self::Kind => "kind",
            Self::Model => "model",
            Self::Month => "month",
            Self::Project => "project",
            Self::Session => "session",
            Self::Skill => "skill",
            Self::Status => "status",
            Self::Subagent => "subagent",
            Self::Tool => "tool",
            Self::Type => "type",
            Self::Week => "week",
        }
    }

    /// Whether the key can put the records of one response in different
    /// groups, so a response's tokens belong to none of them: a reply and
    /// the tool calls of one response have different types and tools.
    pub const fn splits_responses(self) -> bool {
        matches!(
            self,
            Self::Denial | Self::Kind | Self::Status | Self::Tool | Self::Type
        )
    }

    /// The value of the key for `turn`, `None` when it has none or it is
    /// empty.
    fn value(self, turn: &Turn, zone: &TimeZone) -> Result<Option<String>, Failure> {
        let head = turn.head();
        let text = match self {
            Self::Branch => head.git_branch.clone(),
            Self::Day => head.timestamp.date_label(zone)?,
            Self::Denial => match *turn {
                Turn::Result(ref result) => result.denial.clone(),
                Turn::Call(_)
                | Turn::Prompt(_)
                | Turn::Reply(_)
                | Turn::Summary(_)
                | Turn::Thinking(_) => String::new(),
            },
            Self::Kind => match *turn {
                Turn::Prompt(ref prompt) => prompt.kind.name().to_owned(),
                Turn::Call(_)
                | Turn::Reply(_)
                | Turn::Result(_)
                | Turn::Summary(_)
                | Turn::Thinking(_) => String::new(),
            },
            Self::Model => match *turn {
                Turn::Call(ref call) => call.model.clone(),
                Turn::Reply(ref reply) | Turn::Thinking(ref reply) => reply.model.clone(),
                Turn::Prompt(_) | Turn::Result(_) | Turn::Summary(_) => String::new(),
            },
            Self::Month => head.timestamp.month_label(zone)?,
            Self::Project => head.project.clone(),
            Self::Session => head.session_id.clone(),
            Self::Skill => match *turn {
                Turn::Call(ref call) => call.skill.clone(),
                Turn::Reply(ref reply) | Turn::Thinking(ref reply) => reply.skill.clone(),
                Turn::Result(ref result) => result.skill.clone(),
                Turn::Prompt(_) | Turn::Summary(_) => String::new(),
            },
            Self::Status => match *turn {
                Turn::Result(ref result) => if result.is_error { "error" } else { "ok" }.to_owned(),
                Turn::Call(_)
                | Turn::Prompt(_)
                | Turn::Reply(_)
                | Turn::Summary(_)
                | Turn::Thinking(_) => String::new(),
            },
            Self::Subagent => turn.subagent().to_owned(),
            Self::Tool => match *turn {
                Turn::Call(ref call) => call.tool.clone(),
                Turn::Result(ref result) => result.tool.clone(),
                Turn::Prompt(_) | Turn::Reply(_) | Turn::Summary(_) | Turn::Thinking(_) => {
                    String::new()
                }
            },
            Self::Type => turn.type_name().to_owned(),
            Self::Week => head.timestamp.week_label(zone)?,
        };
        Ok(Some(text).filter(|value| !value.is_empty()))
    }
}

impl<'records> Tally<'records> {
    /// The value of `name`, a key `--by session` adds, for the group's
    /// session, `None` when it is empty.
    fn about_session(&self, name: &str) -> Option<String> {
        let head = self.lead.head();
        let text = match name {
            "file" => self.lead.source().file.clone(),
            "project" => head.project.clone(),
            "session_title" => head.session_title.clone(),
            _ => String::new(),
        };
        Some(text).filter(|value| !value.is_empty())
    }

    /// Adds `turn` to the group.
    fn add(&mut self, turn: &'records Turn) {
        let head = turn.head();
        if is_empty_thinking(turn) {
            self.empty = self.empty.saturating_add(1);
        } else {
            self.count = self.count.saturating_add(1);
            self.chars = self.chars.saturating_add(chars(turn));
        }
        self.sessions.insert(&head.session_id);
        if head.timestamp.instant() < self.first.instant() {
            self.first = head.timestamp;
        }
        if head.timestamp.instant() > self.last.instant() {
            self.last = head.timestamp;
        }
        if let Some(tokens) = usage(turn) {
            self.usage.get_or_insert_default().add(tokens);
        }
        if !self.lead.subagent().is_empty() && turn.subagent().is_empty() {
            self.lead = turn;
        }
    }

    /// The line of the group whose keys have the values in `key`, grouped
    /// as `grouping` says.
    fn group(self, key: Key, grouping: &Grouping) -> Group {
        let by = &grouping.by;
        let names = columns(by);
        let mut values = key.into_iter().map(|(none, text)| (!none).then_some(text));
        let keys = names
            .iter()
            .map(|&name| {
                let value = if by.iter().any(|given| given.name() == name) {
                    values.next().flatten()
                } else {
                    self.about_session(name)
                };
                (name.to_owned(), value.map_or(Value::Null, Value::String))
            })
            .collect();
        Group {
            keys,
            count: self.count,
            empty: grouping.empty.then_some(self.empty),
            chars: self.chars,
            sessions: Group::writes_sessions(&names).then_some(self.sessions.len()),
            first: self.first,
            last: self.last,
            usage: if by.iter().any(|given| given.splits_responses()) {
                None
            } else {
                self.usage
            },
        }
    }

    /// A group of `turn` alone.
    fn new(turn: &'records Turn) -> Self {
        let at = turn.head().timestamp;
        Self {
            chars: 0,
            count: 0,
            empty: 0,
            first: at,
            last: at,
            lead: turn,
            sessions: HashSet::new(),
            usage: None,
        }
    }
}

impl io::Write for CharCount {
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Each character has one byte that does not continue another.
        let starts = buf.iter().filter(|byte| **byte & 0xC0 != 0x80).count();
        self.0 = self.0.saturating_add(starts);
        Ok(buf.len())
    }
}

/// The keys a line of `--by` names before `count`: the keys themselves,
/// then, with `session` among them, `session_title`, `project` unless it
/// is one of them, and `file`.
pub fn columns(by: &[By]) -> Vec<&'static str> {
    let mut names: Vec<&str> = by.iter().map(|key| key.name()).collect();
    if by.contains(&By::Session) {
        names.push("session_title");
        if !by.contains(&By::Project) {
            names.push("project");
        }
        names.push("file");
    }
    names
}

/// Groups `records` by the keys of `grouping`, with the day, the week, and
/// the month in `zone`, and keeps, sorts, and cuts the groups as it says.
///
/// # Errors
///
/// When a time cannot be shown in `zone`.
pub fn tally(
    records: &[Turn],
    grouping: &Grouping,
    zone: &TimeZone,
) -> Result<Vec<Group>, Failure> {
    let mut tallies: BTreeMap<Key, Tally<'_>> = BTreeMap::new();
    for turn in records {
        let key = grouping
            .by
            .iter()
            .map(|given| {
                Ok(given
                    .value(turn, zone)?
                    .map_or((true, String::new()), |text| (false, text)))
            })
            .collect::<Result<Key, Failure>>()?;
        tallies
            .entry(key)
            .or_insert_with(|| Tally::new(turn))
            .add(turn);
    }
    let mut groups: Vec<Group> = tallies
        .into_iter()
        // A group of thinking blocks without text alone still has records.
        .filter(|entry| entry.1.count.saturating_add(entry.1.empty) >= grouping.min_count)
        .map(|(key, tally)| tally.group(key, grouping))
        .collect();
    // A stable sort, so groups that tie stay in the order of their keys.
    match grouping.sort {
        Sort::CacheRead => {
            groups.sort_by_key(|group| Reverse(tokens(group).cache_read));
        }
        Sort::Chars => groups.sort_by_key(|group| Reverse(group.chars)),
        Sort::Count => groups.sort_by_key(|group| Reverse(group.count)),
        Sort::Key => {}
        Sort::Output => groups.sort_by_key(|group| Reverse(tokens(group).output)),
    }
    if let Some(limit) = grouping.limit {
        groups.truncate(limit);
    }
    Ok(groups)
}

/// How many characters of text `turn` has: a tool call's input as compact
/// JSON, the text of the other kinds.
fn chars(turn: &Turn) -> usize {
    match *turn {
        Turn::Call(ref call) => {
            let mut counted = CharCount(0);
            match serde_json::to_writer(&mut counted, &call.input) {
                Ok(()) => counted.0,
                // Writing to `CharCount` does not fail; count the string
                // the slow way if it ever does.
                Err(_) => call.input.to_string().chars().count(),
            }
        }
        Turn::Prompt(ref prompt) => prompt.text.chars().count(),
        Turn::Reply(ref reply) | Turn::Thinking(ref reply) => reply.text.chars().count(),
        Turn::Result(ref result) => result.text.chars().count(),
        Turn::Summary(ref summary) => summary.text.chars().count(),
    }
}

/// Whether `turn` is a thinking block that kept no text.
const fn is_empty_thinking(turn: &Turn) -> bool {
    matches!(*turn, Turn::Thinking(ref thought) if thought.text.is_empty())
}

/// The tokens of a group, none counting as zero, for sorting.
fn tokens(group: &Group) -> Usage {
    group.usage.unwrap_or_default()
}

/// The tokens `turn` carries for its response, if it is the record that
/// carries them.
const fn usage(turn: &Turn) -> Option<Usage> {
    match *turn {
        Turn::Call(ref call) => call.usage,
        Turn::Reply(ref reply) | Turn::Thinking(ref reply) => reply.usage,
        Turn::Prompt(_) | Turn::Result(_) | Turn::Summary(_) => None,
    }
}

#[cfg(test)]
mod tests;
