//! Markdown for reading the records: one section per transcript file, one
//! subsection per record.

use jiff::tz::TimeZone;

use serde_json::Value;

use super::json::Fields;
use super::view::{
    self, Body, CallBody, Count, Ending, Fact, Input, Passage, ResultBody, Run, Tool, View,
    count_types,
};
use super::words::{self, Column, Item, SESSION_COLUMNS};
use crate::failure::Failure;

use crate::record::{Group, Session, Turn};
use crate::text::{Lang, clean_text, code_span, escape_inline, escape_pipes, fenced, one_line};

/// Records of any kind, each under a heading that names its kind. A record
/// with `matches` shows those lines in place of its text. A result shows
/// under its call, and not again on its own unless it lists matched lines
/// or a search matched it and not its call; it then shows under its own
/// heading, and the call says only how it ended. Only the parts `fields`
/// keeps are shown; the type in each heading and the session's title stay.
/// The words around the records are in `lang`.
///
/// # Errors
///
/// When a time cannot be shown in `zone`, or an input cannot be encoded.
pub fn records(
    records: &[Turn],
    zone: &TimeZone,
    fields: &Fields,
    lang: Lang,
) -> Result<String, Failure> {
    write_view(&view::records(records, fields)?, zone, lang)
}

/// `view` under one heading per transcript file, after the count line,
/// with times shown in `zone`. Two files that share a session ID, such as
/// a resumed copy, get two headings.
fn write_view(view: &View<'_>, zone: &TimeZone, lang: Lang) -> Result<String, Failure> {
    let mut out = format!(
        "# {}\n\n{}",
        words::records_title(lang),
        record_count(&view.count, lang)
    );
    for section in &view.sections {
        out.extend(["\n## ", &words::title(lang, section.title), "\n"]);
        if !section.facts.is_empty() {
            out.push('\n');
            out.extend(section.facts.iter().map(|fact| fact_line(fact, lang)));
        }
        for item in &section.items {
            out.push_str(&item_heading(item, zone, lang)?);
            let written = body(&item.body, lang);
            let text = written.trim_start_matches('\n');
            if text.is_empty() {
                out.push('\n');
            } else {
                out.extend(["\n\n", text]);
            }
        }
    }
    Ok(out)
}

/// The line that counts the records: those a search matched, by type, and
/// apart from them, those `--around` added as context.
fn record_count(count: &Count, lang: Lang) -> String {
    format!(
        "- {}\n",
        words::record_count(lang, count.matched, &count.types, count.context)
    )
}

/// The list item that says `fact` under a section's heading.
fn fact_line(fact: &Fact<'_>, lang: Lang) -> String {
    let value = match *fact {
        Fact::Project(text)
        | Fact::Session(text)
        | Fact::Subagent(text)
        | Fact::SubagentType(text) => code_span(text),
        Fact::Description(description) => escape_inline(&one_line(description)),
        Fact::AgentVersions(ref values)
        | Fact::Models(ref values)
        | Fact::PermissionModes(ref values) => values
            .iter()
            .map(|value| code_span(value))
            .collect::<Vec<_>>()
            .join(", "),
    };
    format!("- {}: {value}\n", words::fact(lang, fact))
}

/// The heading of `item`, with its time in `zone`, its turn, and its label,
/// marked when it is context, and followed by its sub-agent, as a reader
/// searching the records may see this heading alone.
fn item_heading(item: &view::Item<'_>, zone: &TimeZone, lang: Lang) -> Result<String, Failure> {
    let mut parts = Vec::new();
    if let Some(stamp) = item.stamp {
        parts.push(stamp.second_label(zone)?);
    }
    if let Some(turn) = item.turn {
        parts.push(words::turn(lang, turn));
    }
    parts.push(words::label(lang, item.label).to_owned());
    let mut out = format!("\n### {}", parts.join(" - "));
    if !item.matched {
        out.extend([" ", words::context(lang)]);
    }
    if let Some(name) = item.subagent {
        out.extend([" - ", &code_span(name)]);
    }
    Ok(out)
}

/// What follows an item's heading.
fn body(body: &Body<'_>, lang: Lang) -> String {
    match *body {
        Body::Call(ref call) => tool_call(call, lang),
        Body::Result(ref result) => tool_result(result, lang),
        Body::Text(ref passage) => passage_text(passage, lang),
    }
}

/// `passage`: its runs of lines, or its text in a text block.
fn passage_text(passage: &Passage<'_>, lang: Lang) -> String {
    match *passage {
        Passage::Hidden => String::new(),
        Passage::Lines(ref runs) => lines(runs, lang),
        Passage::Text(text) => fenced("text", &clean_text(text)),
    }
}

/// Sessions as a table, with how many prompts and replies a search
/// matched in each, and the first of them, when `matches` is set, in the
/// columns whose keys `fields` keeps.
///
/// # Errors
///
/// When a time cannot be shown in `zone`.
pub fn sessions(
    records: &[Session],
    zone: &TimeZone,
    matches: bool,
    fields: &Fields,
    lang: Lang,
) -> Result<String, Failure> {
    let mut out = format!(
        "# {}\n\n{}",
        words::sessions_title(lang),
        words::session_count(lang, records.len())
    );
    if matches {
        let total: usize = records
            .iter()
            .filter_map(|session| session.match_count)
            .sum();
        let types: Vec<&str> = records
            .iter()
            .flat_map(|session| session.matched_types.iter().copied())
            .collect();
        out.push_str(&words::match_count(
            lang,
            total,
            &words::by_type(&count_types(&types)),
        ));
    }
    let shown: Vec<Column> = SESSION_COLUMNS
        .into_iter()
        .filter(|column| {
            fields.keeps(column.key)
                && (matches || !matches!(column.key, "match_count" | "first_match"))
        })
        .collect();
    if shown.is_empty() {
        return Ok(out);
    }
    let names: Vec<&str> = shown.iter().map(|column| column.heading(lang)).collect();
    let aligns: Vec<&str> = shown
        .iter()
        .map(|column| if column.number { "---:" } else { "---" })
        .collect();
    out.extend([
        "\n| ",
        &names.join(" | "),
        " |\n| ",
        &aligns.join(" | "),
        " |\n",
    ]);
    for session in records {
        let mut cells = Vec::new();
        for column in &shown {
            cells.push(match column.key {
                "started_at" => session.started_at.minute_label(zone)?,
                "session_id" => escape_pipes(&code_span(&session.id)),
                "project" => escape_pipes(&code_span(&session.project)),
                "cwd" => escape_pipes(&code_span(&session.cwd)),
                "prompt_count" => session.prompt_count.to_string(),
                "match_count" => session.match_count.unwrap_or_default().to_string(),
                "first_match" => session
                    .first_match
                    .as_ref()
                    .map_or_else(String::new, |found| escape_inline(&one_line(&found.text))),
                _ => words::title(lang, &session.title),
            });
        }
        out.extend(["| ", &cells.join(" | "), " |\n"]);
    }
    Ok(out)
}

/// The lines of `ccdig stats` as a table: a column for each of `columns`,
/// the keys before `count`, then the counts, the times in `zone`, and the
/// tokens, each when `fields` keeps its key, `usage` for the tokens. A key
/// without a value, and tokens that are `null`, leave the cell empty.
///
/// # Errors
///
/// When a time cannot be shown in `zone`.
pub fn stats(
    groups: &[Group],
    columns: &[&str],
    zone: &TimeZone,
    fields: &Fields,
    lang: Lang,
) -> Result<String, Failure> {
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
    names = names
        .into_iter()
        .enumerate()
        .filter_map(|(index, name)| keep(index).then_some(name))
        .collect();
    let mut out = words::stats_head(lang, groups.len());
    if names.is_empty() {
        return Ok(out);
    }
    out.push_str("\n|");
    for name in &names {
        out.extend([" ", &words::stats_column(lang, name), " |"]);
    }
    out.push_str("\n|");
    for name in &names {
        out.push_str(
            if columns.contains(name) || matches!(*name, "first" | "last") {
                " --- |"
            } else {
                " ---: |"
            },
        );
    }
    out.push('\n');
    for group in groups {
        let mut cells: Vec<String> = columns
            .iter()
            .map(|name| {
                let value = group.keys.get(*name).and_then(Value::as_str);
                if *name == "session_title" {
                    return words::title(lang, value.unwrap_or_default());
                }
                value.map_or_else(String::new, |text| escape_pipes(&code_span(text)))
            })
            .collect();
        cells.push(group.count.to_string());
        if empty {
            cells.push(group.empty.unwrap_or_default().to_string());
        }
        cells.push(group.chars.to_string());
        if sessions {
            cells.push(group.sessions.unwrap_or_default().to_string());
        }
        cells.extend([
            group.first.minute_label(zone)?,
            group.last.minute_label(zone)?,
        ]);
        match group.usage {
            Some(tokens) => cells.extend(
                [
                    tokens.input,
                    tokens.output,
                    tokens.cache_read,
                    tokens.cache_creation,
                ]
                .map(|count| count.to_string()),
            ),
            None => cells.extend([const { String::new() }; 4]),
        }
        out.push('|');
        for (_, cell) in cells.iter().enumerate().filter(|&(index, _)| keep(index)) {
            out.extend([" ", cell, " |"]);
        }
        out.push('\n');
    }
    Ok(out)
}

/// A tool result's body: the calling tool's name, whether it failed, why
/// the call did not run when it did not, and the output in a text block, or
/// its matched lines, each when the view keeps it.
fn tool_result(result: &ResultBody<'_>, lang: Lang) -> String {
    let mut head = String::new();
    if let Some(tool) = result.tool {
        head.push_str(&list_item(lang, Item::Tool, &tool_name(tool, lang)));
    }
    if let Some(failed) = result.failed {
        head.push_str(&list_item(lang, Item::Error, words::failed(lang, failed)));
    }
    if let Some(denial) = result.denial {
        head.push_str(&list_item(lang, Item::Denial, &code_span(denial)));
    }
    match result.passage {
        Passage::Hidden => head,
        Passage::Lines(ref runs) => format!("{head}{}", lines(runs, lang)),
        Passage::Text(text) => format!("{head}\n{}", fenced("text", &clean_text(text))),
    }
}

/// `runs`, each a list item that says which lines it holds, and which of
/// them matched when it holds others too, followed by the lines in a text
/// block. The line numbers stay out of the block, so they do not mix with
/// numbers in the text.
fn lines(runs: &[Run<'_>], lang: Lang) -> String {
    let mut out = Vec::new();
    for run in runs {
        let field = run.field.map(code_span);
        let item = format!(
            "- {}",
            words::lines(lang, field.as_deref(), run.first, run.last, &run.hits())
        );
        let text: Vec<&str> = run.lines.iter().map(|line| line.text.as_str()).collect();
        out.push(format!(
            "{item}\n\n{}",
            fenced("text", &clean_text(&text.join("\n")))
        ));
    }
    out.join("\n")
}

/// A tool call's body: the tool's name, its input, or its matched lines,
/// and what it came to, each when the view keeps it.
fn tool_call(call: &CallBody<'_>, lang: Lang) -> String {
    let mut out = String::new();
    if let Some(tool) = call.tool {
        out.push_str(&list_item(lang, Item::Tool, &tool_name(tool, lang)));
    }
    let input = match call.input {
        Input::Hidden => String::new(),
        Input::Lines(ref runs) => lines(runs, lang),
        Input::Shown {
            ref json,
            ref texts,
        } => tool_input(json.as_deref(), texts),
    };
    if !input.is_empty() {
        out.extend([input.as_str(), "\n"]);
    }
    if let Some(ending) = call.ending {
        out.push_str(&outcome(ending, lang));
    }
    out
}

/// A call's input: `json` in a block when there is any, then each of
/// `texts` in a text block named for its key, so that it reads as written.
fn tool_input(json: Option<&str>, texts: &[(&str, &str)]) -> String {
    let mut parts = Vec::new();
    if let Some(indented) = json {
        parts.push(fenced("json", &clean_text(indented)));
    }
    for &(key, text) in texts {
        parts.push(format!(
            "- {}\n\n{}",
            code_span(&format!("input.{key}")),
            fenced("text", &clean_text(text))
        ));
    }
    let body = parts.join("\n");
    if json.is_some() {
        format!("\n{body}")
    } else {
        body
    }
}

/// What a call came to: whether it failed, why it did not run when it did
/// not, and the output in a text block when it carries it; or `(none)` when
/// its file has no result for it.
fn outcome(ending: Ending<'_>, lang: Lang) -> String {
    let Ending::Ran {
        denial,
        failed,
        output,
    } = ending
    else {
        return list_item(lang, Item::Result, words::ending(lang, None));
    };
    let mut out = list_item(lang, Item::Result, words::ending(lang, Some(failed)));
    if let Some(reason) = denial {
        out.push_str(&list_item(lang, Item::Denial, &code_span(reason)));
    }
    if let Some(text) = output {
        out.extend(["\n", &fenced("text", &clean_text(text))]);
    }
    out
}

/// The tool's name as a code span, or `(unknown)` when the call was not
/// found.
fn tool_name(tool: Tool<'_>, lang: Lang) -> String {
    match tool {
        Tool::Named(name) => code_span(name),
        Tool::Unknown => words::unknown_tool(lang).to_owned(),
    }
}

/// The list item `item` of a call or a result, saying `value`.
fn list_item(lang: Lang, item: Item, value: &str) -> String {
    format!("- {}: {value}\n", words::item(lang, item))
}
