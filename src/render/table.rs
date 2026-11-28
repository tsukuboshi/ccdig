//! What the tables of `sessions` and `stats` show, apart from how a format
//! words and escapes it: which columns `--fields` keeps, in which order,
//! which of them hold numbers, and what each cell holds.

use serde_json::Value;

use super::json::Fields;
use super::view::count_types;
use super::words::{self, SESSION_COLUMNS};
use crate::record::{Group, Session};
use crate::text::Lang;
use crate::when::Stamp;

/// A table: its columns, and its rows, each with a cell per column.
pub struct Table<'records> {
    /// The columns, in order.
    pub columns: Vec<Heading>,
    /// The rows, each with a cell per column.
    pub rows: Vec<Vec<Cell<'records>>>,
}

/// A column of a table.
pub struct Heading {
    /// Whether the column holds numbers, aligned right.
    pub number: bool,
    /// The heading, in the language of the table.
    pub text: String,
}

/// What a cell holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cell<'records> {
    /// A value read as written, such as an ID or a path.
    Code(&'records str),
    /// A count.
    Count(usize),
    /// Nothing: a key without a value, or tokens that are `null`.
    Empty,
    /// Text from a transcript, such as the first match.
    Text(&'records str),
    /// A time.
    Time(Stamp),
    /// A session's title, or the words for one without one.
    Title(&'records str),
    /// A number of tokens.
    Tokens(u64),
}

/// What the table of `sessions` shows: how many sessions there are, with
/// a search how many matches and of which types, and the table.
pub struct Sessions<'records> {
    /// How many sessions there are.
    pub count: usize,
    /// With a search, how many prompts and replies it matched in all, and
    /// how many of each type.
    pub matches: Option<(usize, Vec<(&'static str, usize)>)>,
    /// The table.
    pub table: Table<'records>,
}

/// What the table of `stats` shows: how many groups there are, and the
/// table.
pub struct Stats<'records> {
    /// How many groups there are.
    pub groups: usize,
    /// The table.
    pub table: Table<'records>,
}

/// The table of `sessions`, one row per record, with how many prompts and replies a search
/// matched in each, and the first of them, when `matches` is set, in the
/// columns whose keys `fields` keeps, headed in `lang`.
pub fn sessions<'records>(
    records: &'records [Session],
    matches: bool,
    fields: &Fields,
    lang: Lang,
) -> Sessions<'records> {
    let counted = matches.then(|| {
        let total: usize = records
            .iter()
            .filter_map(|session| session.match_count)
            .sum();
        let types: Vec<&str> = records
            .iter()
            .flat_map(|session| session.matched_types.iter().copied())
            .collect();
        (total, count_types(&types))
    });
    let shown: Vec<_> = SESSION_COLUMNS
        .into_iter()
        .filter(|column| {
            fields.keeps(column.key)
                && (matches || !matches!(column.key, "match_count" | "first_match"))
        })
        .collect();
    let rows = records
        .iter()
        .map(|session| {
            shown
                .iter()
                .map(|column| match column.key {
                    "started_at" => Cell::Time(session.started_at),
                    "session_id" => Cell::Code(&session.id),
                    "project" => Cell::Code(&session.project),
                    "cwd" => Cell::Code(&session.cwd),
                    "prompt_count" => Cell::Count(session.prompt_count),
                    "match_count" => Cell::Count(session.match_count.unwrap_or_default()),
                    "first_match" => session
                        .first_match
                        .as_ref()
                        .map_or(Cell::Empty, |found| Cell::Text(&found.text)),
                    _ => Cell::Title(&session.title),
                })
                .collect()
        })
        .collect();
    Sessions {
        count: records.len(),
        matches: counted,
        table: Table {
            columns: shown
                .iter()
                .map(|column| Heading {
                    number: column.number,
                    text: column.heading(lang).to_owned(),
                })
                .collect(),
            rows,
        },
    }
}

/// The table of `ccdig stats`: a column for each of `columns`, the keys
/// before `count`, then the counts, the times, and the tokens, each when
/// `fields` keeps its key, `usage` for the tokens, headed in `lang`. A key
/// without a value, and tokens that are `null`, leave the cell empty.
pub fn stats<'records>(
    groups: &'records [Group],
    columns: &[&str],
    fields: &Fields,
    lang: Lang,
) -> Stats<'records> {
    let sessions = Group::writes_sessions(columns);
    let empty = groups.iter().any(|group| group.empty.is_some());
    let mut names: Vec<&str> = columns.to_vec();
    names.push("count");
    if empty {
        names.push("empty");
    }
    names.push("chars");
    if sessions {
        names.push("sessions");
    }
    names.extend([
        "first",
        "last",
        "input",
        "output",
        "cache_read",
        "cache_creation",
    ]);
    let kept: Vec<bool> = names
        .iter()
        .map(|name| {
            fields.keeps(match *name {
                "input" | "output" | "cache_read" | "cache_creation" => "usage",
                key => key,
            })
        })
        .collect();
    let keep = |index: usize| kept.get(index).copied().unwrap_or_default();
    let headings = names
        .iter()
        .enumerate()
        .filter(|&(index, _)| keep(index))
        .map(|(_, name)| Heading {
            number: !columns.contains(name) && !matches!(*name, "first" | "last"),
            text: words::stats_column(lang, name),
        })
        .collect();
    let rows = groups
        .iter()
        .map(|group| {
            let mut cells: Vec<Cell<'records>> = Vec::with_capacity(names.len());
            cells.extend(columns.iter().map(|name| {
                let value = group.keys.get(*name).and_then(Value::as_str);
                if *name == "session_title" {
                    return Cell::Title(value.unwrap_or_default());
                }
                value.map_or(Cell::Empty, Cell::Code)
            }));
            cells.push(Cell::Count(group.count));
            if empty {
                cells.push(Cell::Count(group.empty.unwrap_or_default()));
            }
            cells.push(Cell::Count(group.chars));
            if sessions {
                cells.push(Cell::Count(group.sessions.unwrap_or_default()));
            }
            cells.extend([Cell::Time(group.first), Cell::Time(group.last)]);
            match group.usage {
                Some(tokens) => cells.extend(
                    [
                        tokens.input,
                        tokens.output,
                        tokens.cache_read,
                        tokens.cache_creation,
                    ]
                    .map(Cell::Tokens),
                ),
                None => cells.extend([Cell::Empty; 4]),
            }
            cells
                .into_iter()
                .enumerate()
                .filter(|&(index, _)| keep(index))
                .map(|(_, cell)| cell)
                .collect()
        })
        .collect();
    Stats {
        groups: groups.len(),
        table: Table {
            columns: headings,
            rows,
        },
    }
}
