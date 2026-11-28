//! Markdown for reading the records: one section per transcript file, one
//! subsection per record.

use jiff::tz::TimeZone;

use serde_json::Value;

use super::json::Fields;
use super::view::{
    self, Body, CallBody, Count, Ending, Fact, Input, Item, Label, Passage, ResultBody, Run, Tool,
    View, count_types,
};
use crate::failure::Failure;

use crate::record::{Group, Session, Turn};
use crate::text::{
    clean_text, code_span, escape_inline, escape_pipes, fenced, heading_text, one_line,
};

/// Records of any kind, each under a heading that names its kind. A record
/// with `matches` shows those lines in place of its text. A result shows
/// under its call, and not again on its own unless it lists matched lines
/// or a search matched it and not its call; it then shows under its own
/// heading, and the call says only how it ended. Only the parts `fields`
/// keeps are shown; the type in each heading and the session's title stay.
///
/// # Errors
///
/// When a time cannot be shown in `zone`, or an input cannot be encoded.
pub fn records(records: &[Turn], zone: &TimeZone, fields: &Fields) -> Result<String, Failure> {
    write_view(&view::records(records, fields)?, zone)
}

/// `view` under one heading per transcript file, after the count line,
/// with times shown in `zone`. Two files that share a session ID, such as
/// a resumed copy, get two headings.
fn write_view(view: &View<'_>, zone: &TimeZone) -> Result<String, Failure> {
    let mut out = format!("# Records\n\n{}", record_count(&view.count));
    for section in &view.sections {
        out.extend(["\n## ", &heading_text(section.title), "\n"]);
        if !section.facts.is_empty() {
            out.push('\n');
            out.extend(section.facts.iter().map(|&fact| fact_line(fact)));
        }
        for item in &section.items {
            out.push_str(&item_heading(item, zone)?);
            let written = body(&item.body);
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
fn record_count(count: &Count) -> String {
    let mut out = format!("- Records: {}{}", count.matched, by_type(&count.types));
    if count.context > 0 {
        out.extend([" + ", &count.context.to_string(), " context"]);
    }
    out.push('\n');
    out
}

/// `types` and how many of each there are, as ` (prompt 2, call 1)`;
/// empty when there are none.
fn by_type(types: &[(&str, usize)]) -> String {
    if types.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = types
        .iter()
        .map(|&(name, count)| format!("{name} {count}"))
        .collect();
    format!(" ({})", parts.join(", "))
}

/// The list item that says `fact` under a section's heading.
fn fact_line(fact: Fact<'_>) -> String {
    match fact {
        Fact::Project(project) => format!("- Project: {}\n", code_span(project)),
        Fact::Session(session) => format!("- Session: {}\n", code_span(session)),
        Fact::Subagent(name) => format!("- Subagent: {}\n", code_span(name)),
        Fact::SubagentType(kind) => format!("- Subagent type: {}\n", code_span(kind)),
        Fact::Description(description) => {
            format!("- Description: {}\n", escape_inline(&one_line(description)))
        }
    }
}

/// The heading of `item`, with its time in `zone`, its turn, and its label,
/// marked when it is context, and followed by its sub-agent, as a reader
/// searching the records may see this heading alone.
fn item_heading(item: &Item<'_>, zone: &TimeZone) -> Result<String, Failure> {
    let mut parts = Vec::new();
    if let Some(stamp) = item.stamp {
        parts.push(stamp.second_label(zone)?);
    }
    if let Some(turn) = item.turn {
        parts.push(format!("turn {turn}"));
    }
    parts.push(label(item.label).to_owned());
    let mut out = format!("\n### {}", parts.join(" - "));
    if !item.matched {
        out.push_str(" (context)");
    }
    if let Some(name) = item.subagent {
        out.extend([" - ", &code_span(name)]);
    }
    Ok(out)
}

/// The words a heading names `label` with.
const fn label(label: Label) -> &'static str {
    match label {
        Label::Answer => "Answer",
        Label::Approval => "Approval",
        Label::Call => "Tool call",
        Label::Prompt => "Prompt",
        Label::Rejection => "Rejection",
        Label::Reply => "Reply",
        Label::Result => "Tool result",
        Label::Summary => "Summary",
        Label::Thinking => "Thinking",
        Label::TypedAnswer => "Typed answer",
    }
}

/// What follows an item's heading.
fn body(body: &Body<'_>) -> String {
    match *body {
        Body::Call(ref call) => tool_call(call),
        Body::Result(ref result) => tool_result(result),
        Body::Text(ref passage) => passage_text(passage),
    }
}

/// `passage`: its runs of lines, or its text in a text block.
fn passage_text(passage: &Passage<'_>) -> String {
    match *passage {
        Passage::Hidden => String::new(),
        Passage::Lines(ref runs) => lines(runs),
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
) -> Result<String, Failure> {
    /// The columns: the heading, the key, and whether the column holds a
    /// number, aligned right, in the order they are shown.
    const COLUMNS: [(&str, &str, bool); 8] = [
        ("Started", "started_at", false),
        ("Session", "session_id", false),
        ("Project", "project", false),
        ("Cwd", "cwd", false),
        ("Prompts", "prompt_count", true),
        ("Matches", "match_count", true),
        ("First match", "first_match", false),
        ("Title", "title", false),
    ];
    let mut out = format!("# Sessions\n\n- Sessions: {}\n", records.len());
    if matches {
        let total: usize = records
            .iter()
            .filter_map(|session| session.match_count)
            .sum();
        let types: Vec<&str> = records
            .iter()
            .flat_map(|session| session.matched_types.iter().copied())
            .collect();
        out.extend([
            "- Matches: ",
            &total.to_string(),
            &by_type(&count_types(&types)),
            "\n",
        ]);
    }
    let shown: Vec<(&str, &str, bool)> = COLUMNS
        .into_iter()
        .filter(|&(_, key, _)| {
            fields.keeps(key) && (matches || !matches!(key, "match_count" | "first_match"))
        })
        .collect();
    if shown.is_empty() {
        return Ok(out);
    }
    let names: Vec<&str> = shown.iter().map(|&(name, _, _)| name).collect();
    let aligns: Vec<&str> = shown
        .iter()
        .map(|&(_, _, number)| if number { "---:" } else { "---" })
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
        for &(_, key, _) in &shown {
            cells.push(match key {
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
                _ => heading_text(&session.title),
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
    let mut out = format!("# Stats\n\n- Groups: {}\n", groups.len());
    if names.is_empty() {
        return Ok(out);
    }
    out.push_str("\n|");
    for name in &names {
        out.extend([" ", &column_label(name), " |"]);
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
                    return heading_text(value.unwrap_or_default());
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

/// The heading of the column of `key`: `session_title` as `Session title`.
fn column_label(key: &str) -> String {
    let words = key.replace('_', " ");
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// A tool result's body: the calling tool's name, whether it failed, why
/// the call did not run when it did not, and the output in a text block, or
/// its matched lines, each when the view keeps it.
fn tool_result(result: &ResultBody<'_>) -> String {
    let mut head = String::new();
    if let Some(tool) = result.tool {
        head.extend(["- Tool: ", &tool_name(tool), "\n"]);
    }
    if let Some(failed) = result.failed {
        head.extend(["- Error: ", if failed { "yes" } else { "no" }, "\n"]);
    }
    if let Some(denial) = result.denial {
        head.extend(["- Denial: ", &code_span(denial), "\n"]);
    }
    match result.passage {
        Passage::Hidden => head,
        Passage::Lines(ref runs) => format!("{head}{}", lines(runs)),
        Passage::Text(text) => format!("{head}\n{}", fenced("text", &clean_text(text))),
    }
}

/// `runs`, each a list item that says which lines it holds, and which of
/// them matched when it holds others too, followed by the lines in a text
/// block. The line numbers stay out of the block, so they do not mix with
/// numbers in the text.
fn lines(runs: &[Run<'_>]) -> String {
    let mut out = Vec::new();
    for run in runs {
        let (single, plural) = run.field.map_or_else(
            || ("Line".to_owned(), "Lines".to_owned()),
            |field| {
                let name = code_span(field);
                (format!("{name} line"), format!("{name} lines"))
            },
        );
        let mut item = if run.first == run.last {
            format!("- {single} {}", run.first)
        } else {
            format!("- {plural} {}-{}", run.first, run.last)
        };
        if run.lines.iter().any(|line| !line.hit) {
            let hits: Vec<String> = run
                .lines
                .iter()
                .filter(|line| line.hit)
                .map(|line| line.line.to_string())
                .collect();
            item.extend([", matched ", &hits.join(", ")]);
        }
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
fn tool_call(call: &CallBody<'_>) -> String {
    let mut out = String::new();
    if let Some(tool) = call.tool {
        out.extend(["- Tool: ", &tool_name(tool), "\n"]);
    }
    let input = match call.input {
        Input::Hidden => String::new(),
        Input::Lines(ref runs) => lines(runs),
        Input::Shown {
            ref json,
            ref texts,
        } => tool_input(json.as_deref(), texts),
    };
    if !input.is_empty() {
        out.extend([input.as_str(), "\n"]);
    }
    if let Some(ending) = call.ending {
        out.push_str(&outcome(ending));
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
fn outcome(ending: Ending<'_>) -> String {
    let Ending::Ran {
        denial,
        failed,
        output,
    } = ending
    else {
        return "- Result: (none)\n".to_owned();
    };
    let mut out = format!("- Result: {}\n", if failed { "error" } else { "ok" });
    if let Some(reason) = denial {
        out.extend(["- Denial: ", &code_span(reason), "\n"]);
    }
    if let Some(text) = output {
        out.extend(["\n", &fenced("text", &clean_text(text))]);
    }
    out
}

/// The tool's name as a code span, or `(unknown)` when the call was not
/// found.
fn tool_name(tool: Tool<'_>) -> String {
    match tool {
        Tool::Named(name) => code_span(name),
        Tool::Unknown => "(unknown)".to_owned(),
    }
}
