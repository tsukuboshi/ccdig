//! Markdown for reading the records: one section per transcript file, one
//! subsection per record.

use jiff::tz::TimeZone;
use serde_json::Value;

use crate::failure::Failure;
use crate::record::{MatchLine, Record, Session, ToolCall, ToolResult, Turn};
use crate::text::{clean_text, code_span, escape_pipes, fenced, heading_text};

/// Records of any kind, each under a heading that names its kind, with
/// each string in a tool call's input cut to `input_chars` when given. A
/// record with `matches` shows those lines in place of its text.
///
/// # Errors
///
/// When a time cannot be shown in `zone`, or an input cannot be encoded.
pub fn records(
    records: &[Turn],
    zone: &TimeZone,
    input_chars: Option<usize>,
) -> Result<String, Failure> {
    sections(
        "Records",
        records,
        zone,
        |turn| match *turn {
            Turn::Prompt(_) => "Prompt",
            Turn::Reply(_) => "Reply",
            Turn::Call(_) => "Tool call",
            Turn::Result(_) => "Tool result",
            Turn::Summary(_) => "Summary",
        },
        |turn| match *turn {
            Turn::Prompt(ref prompt) => Ok(text_or_lines(&prompt.text, &prompt.matches)),
            Turn::Reply(ref reply) => Ok(text_or_lines(&reply.text, &reply.matches)),
            Turn::Call(ref call) => tool_call(call, input_chars),
            Turn::Result(ref result) => Ok(tool_result(result)),
            Turn::Summary(ref summary) => Ok(text_or_lines(&summary.text, &summary.matches)),
        },
    )
}

/// Groups `records` under one heading per transcript file, with times shown
/// in `zone`, each record's heading followed by its `label` when that is
/// not empty and by its sub-agent when it has one, and each record's body
/// written by `body`. Records of a file
/// must be adjacent. Two files that share a session ID, such as a resumed
/// copy, get two headings.
fn sections<T>(
    title: &str,
    records: &[T],
    zone: &TimeZone,
    label: impl Fn(&T) -> &'static str,
    body: impl Fn(&T) -> Result<String, Failure>,
) -> Result<String, Failure>
where
    T: Record,
{
    let mut out = format!("# {title}\n");
    for group in records.chunk_by(|left, right| left.source().file == right.source().file) {
        if let Some(first) = group.first() {
            let head = first.head();
            out.extend([
                "\n## ",
                &heading_text(&head.session_title),
                "\n\n- Project: ",
                &code_span(&head.project),
                "\n- Session: ",
                &code_span(&head.session_id),
                "\n",
            ]);
            if !first.subagent().is_empty() {
                out.extend(["- Subagent: ", &code_span(first.subagent()), "\n"]);
            }
        }
        for record in group {
            out.extend(["\n### ", &record.head().timestamp.minute_label(zone)?]);
            let name = label(record);
            if !name.is_empty() {
                out.extend([" - ", name]);
            }
            // A record of a sub-agent names it in its own heading too, as a
            // reader searching the records may see this heading alone.
            if !record.subagent().is_empty() {
                out.extend([" - ", &code_span(record.subagent())]);
            }
            out.extend(["\n\n", &body(record)?]);
        }
    }
    Ok(out)
}

/// Sessions as a table.
///
/// # Errors
///
/// When a time cannot be shown in `zone`.
pub fn sessions(records: &[Session], zone: &TimeZone) -> Result<String, Failure> {
    let mut out = String::from(
        "# Sessions\n\n| Started | Project | Prompts | Title |\n| --- | --- | ---: | --- |\n",
    );
    for session in records {
        out.extend([
            "| ",
            &session.started_at.minute_label(zone)?,
            " | ",
            &escape_pipes(&code_span(&session.project)),
            " | ",
            &session.prompt_count.to_string(),
            " | ",
            &heading_text(&session.title),
            " |\n",
        ]);
    }
    Ok(out)
}

/// A tool result's body: the calling tool's name, whether it failed, and
/// the output in a text block, or its matched lines.
fn tool_result(result: &ToolResult) -> String {
    let head = format!(
        "- Tool: {}\n- Error: {}\n",
        tool_name(&result.tool),
        if result.is_error { "yes" } else { "no" },
    );
    if result.matches.is_empty() {
        return format!("{head}\n{}", fenced("text", &clean_text(&result.text)));
    }
    format!("{head}{}", lines(&result.matches))
}

/// `text` in a text block, or when `matches` lists lines of it, those.
fn text_or_lines(text: &str, matches: &[MatchLine]) -> String {
    if matches.is_empty() {
        return fenced("text", &clean_text(text));
    }
    lines(matches)
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
    for run in runs {
        let (Some(first), Some(last)) = (run.first(), run.last()) else {
            continue;
        };
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

/// A tool call's body: the tool's name and its input as indented JSON,
/// with each string cut to `input_chars` when given.
fn tool_call(call: &ToolCall, input_chars: Option<usize>) -> Result<String, Failure> {
    let input = input_chars
        .map_or_else(
            || serde_json::to_string_pretty(&call.input),
            |limit| {
                let mut cut = call.input.clone();
                cut_strings(&mut cut, limit);
                serde_json::to_string_pretty(&cut)
            },
        )
        .map_err(Failure::Json)?;
    if !call.matches.is_empty() {
        return Ok(format!(
            "- Tool: {}\n{}",
            tool_name(&call.tool),
            lines(&call.matches)
        ));
    }
    Ok(format!(
        "- Tool: {}\n\n{}",
        tool_name(&call.tool),
        fenced("json", &clean_text(&input))
    ))
}

/// Cuts each string in `value` longer than `limit` characters to `limit`,
/// followed by an ellipsis and how many characters were cut.
fn cut_strings(value: &mut Value, limit: usize) {
    match *value {
        Value::String(ref mut text) => {
            let length = text.chars().count();
            if length > limit {
                let kept: String = text.chars().take(limit).collect();
                *text = format!(
                    "{kept}\u{2026} ({} more characters)",
                    length.saturating_sub(limit)
                );
            }
        }
        Value::Array(ref mut items) => {
            for item in items {
                cut_strings(item, limit);
            }
        }
        Value::Object(ref mut fields) => {
            for field in fields.values_mut() {
                cut_strings(field, limit);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// The tool's name as a code span, or `(unknown)` when the call was not
/// found.
fn tool_name(name: &str) -> String {
    if name.is_empty() {
        return "(unknown)".to_owned();
    }
    code_span(name)
}
