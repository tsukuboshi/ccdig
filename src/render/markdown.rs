//! Markdown for reading the records: one section per transcript file, one
//! subsection per record.

use std::collections::{HashMap, HashSet};

use jiff::tz::TimeZone;

use serde_json::Value;

use super::json::Fields;
use crate::failure::Failure;

use crate::record::{
    Group, Kind, MatchLine, Outcome, Record, Session, Source, ToolCall, ToolResult, Turn,
};
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
    // The calls that carry their result, by file and ID, and whether a
    // search matched them.
    let mut answered: HashMap<(&str, &str), bool> = HashMap::new();
    for turn in records {
        if let Turn::Call(ref call) = *turn
            && call.result.is_some()
        {
            answered.insert(key_of(&call.source, &call.tool_use_id), call.matched());
        }
    }
    // The results that show under their own heading: those whose call is
    // not listed, those that list matched lines, and those a search matched
    // when it did not match their call, which then shows as context.
    let standing: HashSet<(&str, &str)> = records
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
        .collect();
    let shown: Vec<&Turn> = records
        .iter()
        .filter(|turn| match **turn {
            Turn::Result(ref result) => {
                standing.contains(&key_of(&result.source, &result.tool_use_id))
            }
            Turn::Call(_) | Turn::Prompt(_) | Turn::Reply(_) | Turn::Summary(_) => true,
        })
        .collect();
    sections(
        "Records",
        &record_count(&shown),
        &shown,
        zone,
        fields,
        |turn| match **turn {
            Turn::Prompt(ref prompt) => match prompt.kind {
                Kind::Answer => "Answer",
                Kind::Approval => "Approval",
                Kind::Rejection => "Rejection",
                Kind::Slash | Kind::Text => "Prompt",
                Kind::TypedAnswer => "Typed answer",
            },
            Turn::Reply(_) => "Reply",
            Turn::Call(_) => "Tool call",
            Turn::Result(_) => "Tool result",
            Turn::Summary(_) => "Summary",
        },
        |turn| match **turn {
            Turn::Prompt(ref prompt) => {
                Ok(text_or_lines(&prompt.text, &prompt.search.matches, fields))
            }
            Turn::Reply(ref reply) => Ok(text_or_lines(&reply.text, &reply.search.matches, fields)),
            Turn::Call(ref call) => tool_call(
                call,
                !standing.contains(&key_of(&call.source, &call.tool_use_id)),
                fields,
            ),
            Turn::Result(ref result) => Ok(tool_result(result, fields)),
            Turn::Summary(ref summary) => Ok(text_or_lines(
                &summary.text,
                &summary.search.matches,
                fields,
            )),
        },
    )
}

/// The key that pairs a call and its result: the file and the call's ID.
fn key_of<'record>(source: &'record Source, id: &'record str) -> (&'record str, &'record str) {
    (&source.file, id)
}

/// The line that counts `records`: those a search matched, by type in the
/// order a conversation has them, and apart from them, those `--around`
/// added as context.
fn record_count(records: &[&Turn]) -> String {
    let matched: Vec<&Turn> = records.iter().copied().filter(Record::matched).collect();
    let mut out = format!(
        "- Records: {}{}",
        matched.len(),
        by_type(
            &matched
                .iter()
                .map(|turn| turn.type_name())
                .collect::<Vec<_>>()
        )
    );
    let context = records.len().saturating_sub(matched.len());
    if context > 0 {
        out.extend([" + ", &context.to_string(), " context"]);
    }
    out.push('\n');
    out
}

/// How many of `types` are each type, in the order a conversation has
/// them, as ` (prompt 2, call 1)`; empty when there are none.
fn by_type(types: &[&str]) -> String {
    /// The types in the order the count lists them.
    const ORDER: [&str; 5] = ["prompt", "reply", "call", "result", "summary"];
    let parts: Vec<String> = ORDER
        .iter()
        .filter_map(|name| {
            let count = types.iter().filter(|kind| **kind == *name).count();
            (count > 0).then(|| format!("{name} {count}"))
        })
        .collect();
    if parts.is_empty() {
        return String::new();
    }
    format!(" ({})", parts.join(", "))
}

/// Groups `records` under one heading per transcript file, with times shown
/// in `zone`, each record's heading followed by its `label` when that is
/// not empty and by its sub-agent when it has one, and each record's body
/// written by `body`, after the `count` line. The project, the session, the
/// time, the turn, and the sub-agent show when `fields` keeps their keys.
/// Records of a file must be adjacent. Two files that share a session ID,
/// such as a resumed copy, get two headings.
fn sections<T>(
    title: &str,
    count: &str,
    records: &[T],
    zone: &TimeZone,
    fields: &Fields,
    label: impl Fn(&T) -> &'static str,
    body: impl Fn(&T) -> Result<String, Failure>,
) -> Result<String, Failure>
where
    T: Record,
{
    let subagent = fields.keeps("subagent");
    let mut out = format!("# {title}\n\n{count}");
    for group in records.chunk_by(|left, right| left.source().file == right.source().file) {
        if let Some(first) = group.first() {
            let head = first.head();
            out.extend(["\n## ", &heading_text(&head.session_title), "\n"]);
            let mut facts = Vec::new();
            if fields.keeps("project") {
                facts.push(format!("- Project: {}\n", code_span(&head.project)));
            }
            if fields.keeps("session_id") {
                facts.push(format!("- Session: {}\n", code_span(&head.session_id)));
            }
            if subagent && !first.subagent().is_empty() {
                facts.push(format!("- Subagent: {}\n", code_span(first.subagent())));
            }
            if !facts.is_empty() {
                out.push('\n');
                out.extend(facts);
            }
        }
        for record in group {
            let mut parts = Vec::new();
            if fields.keeps("timestamp") {
                parts.push(record.head().timestamp.second_label(zone)?);
            }
            let turn = record.head().turn;
            if turn > 0 && fields.keeps("turn") {
                parts.push(format!("turn {turn}"));
            }
            let name = label(record);
            if !name.is_empty() {
                parts.push(name.to_owned());
            }
            out.extend(["\n### ", &parts.join(" - ")]);
            if !record.matched() {
                out.push_str(" (context)");
            }
            // A record of a sub-agent names it in its own heading too, as a
            // reader searching the records may see this heading alone.
            if subagent && !record.subagent().is_empty() {
                out.extend([" - ", &code_span(record.subagent())]);
            }
            let written = body(record)?;
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
        out.extend(["- Matches: ", &total.to_string(), &by_type(&types), "\n"]);
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
    let mut names: Vec<&str> = columns.to_vec();
    names.extend(["count", "chars"]);
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
        cells.extend([group.count.to_string(), group.chars.to_string()]);
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
/// its matched lines, each when `fields` keeps its key.
fn tool_result(result: &ToolResult, fields: &Fields) -> String {
    let mut head = String::new();
    if fields.keeps("tool") {
        head.extend(["- Tool: ", &tool_name(&result.tool), "\n"]);
    }
    if fields.keeps("is_error") {
        head.extend([
            "- Error: ",
            if result.is_error { "yes" } else { "no" },
            "\n",
        ]);
    }
    if !result.denial.is_empty() && fields.keeps("denial") {
        head.extend(["- Denial: ", &code_span(&result.denial), "\n"]);
    }
    if !result.search.matches.is_empty() && fields.keeps("matches") {
        return format!("{head}{}", lines(&result.search.matches));
    }
    if !fields.keeps("text") {
        return head;
    }
    format!("{head}\n{}", fenced("text", &clean_text(&result.text)))
}

/// The lines `matches` lists of `text`, when it lists some and `fields`
/// keeps them, or else `text` in a text block when `fields` keeps it.
fn text_or_lines(text: &str, matches: &[MatchLine], fields: &Fields) -> String {
    if !matches.is_empty() && fields.keeps("matches") {
        return lines(matches);
    }
    if !fields.keeps("text") {
        return String::new();
    }
    fenced("text", &clean_text(text))
}

/// `matches` in runs of adjacent lines of one field, each a list item that
/// says which lines it holds, and which of them matched when it holds
/// others too, followed by the lines in a text block. The line numbers stay
/// out of the block, so they do not mix with numbers in the text.
fn lines(matches: &[MatchLine]) -> String {
    let runs = matches.chunk_by(|left, right| {
        left.field == right.field && left.line.checked_add(1) == Some(right.line)
    });
    let mut out = Vec::new();
    // `chunk_by` gives no empty run.
    for (first, last, run) in runs.filter_map(|run| Some((run.first()?, run.last()?, run))) {
        let (single, plural) = if first.field == "text" {
            ("Line".to_owned(), "Lines".to_owned())
        } else {
            let name = code_span(&first.field);
            (format!("{name} line"), format!("{name} lines"))
        };
        let mut item = if first.line == last.line {
            format!("- {single} {}", first.line)
        } else {
            format!("- {plural} {}-{}", first.line, last.line)
        };
        if run.iter().any(|line| !line.hit) {
            let hits: Vec<String> = run
                .iter()
                .filter(|line| line.hit)
                .map(|line| line.line.to_string())
                .collect();
            item.extend([", matched ", &hits.join(", ")]);
        }
        let text: Vec<&str> = run.iter().map(|line| line.text.as_str()).collect();
        out.push(format!(
            "{item}\n\n{}",
            fenced("text", &clean_text(&text.join("\n")))
        ));
    }
    out.join("\n")
}

/// A tool call's body: the tool's name, its input, or its matched lines,
/// and what it came to, with the output when `output`, each when `fields`
/// keeps its key.
fn tool_call(call: &ToolCall, output: bool, fields: &Fields) -> Result<String, Failure> {
    let mut out = String::new();
    if fields.keeps("tool") {
        out.extend(["- Tool: ", &tool_name(&call.tool), "\n"]);
    }
    let input = if !call.search.matches.is_empty() && fields.keeps("matches") {
        lines(&call.search.matches)
    } else if fields.keeps("input") {
        tool_input(&call.input)?
    } else {
        String::new()
    };
    if !input.is_empty() {
        out.extend([input.as_str(), "\n"]);
    }
    if fields.keeps("result") {
        out.push_str(&outcome(call.result.as_ref(), output));
    }
    Ok(out)
}

/// A call's input as indented JSON, but for each string at its top level
/// that runs over lines, such as a plan, which follows in a text block
/// named for its key so that it reads as written. The JSON is left out
/// when those strings are all there is.
fn tool_input(input: &Value) -> Result<String, Failure> {
    let mut rest = input.clone();
    let mut texts: Vec<(String, String)> = Vec::new();
    if let Value::Object(ref mut map) = rest {
        let keys: Vec<String> = map
            .iter()
            .filter(|&(_, value)| value.as_str().is_some_and(|text| text.contains('\n')))
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            if let Some(Value::String(text)) = map.remove(&key) {
                texts.push((key, text));
            }
        }
    }
    let mut parts = Vec::new();
    let only_texts = !texts.is_empty() && rest.as_object().is_some_and(serde_json::Map::is_empty);
    if !only_texts {
        let indented = serde_json::to_string_pretty(&rest).map_err(Failure::Json)?;
        parts.push(fenced("json", &clean_text(&indented)));
    }
    for (key, text) in texts {
        parts.push(format!(
            "- {}\n\n{}",
            code_span(&format!("input.{key}")),
            fenced("text", &clean_text(text.trim_end_matches('\n')))
        ));
    }
    let body = parts.join("\n");
    Ok(if only_texts {
        body
    } else {
        format!("\n{body}")
    })
}

/// What a call came to: whether it failed, why it did not run when it did
/// not, and when `output`, the output in a text block; or `(none)` when its
/// file has no result for it.
fn outcome(result: Option<&Outcome>, output: bool) -> String {
    let Some(found) = result else {
        return "- Result: (none)\n".to_owned();
    };
    let mut out = format!(
        "- Result: {}\n",
        if found.is_error { "error" } else { "ok" }
    );
    if !found.denial.is_empty() {
        out.extend(["- Denial: ", &code_span(&found.denial), "\n"]);
    }
    if output {
        out.extend(["\n", &fenced("text", &clean_text(&found.text))]);
    }
    out
}

/// The tool's name as a code span, or `(unknown)` when the call was not
/// found.
fn tool_name(name: &str) -> String {
    if name.is_empty() {
        return "(unknown)".to_owned();
    }
    code_span(name)
}
