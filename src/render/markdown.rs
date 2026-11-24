//! Markdown for reading the records: one section per transcript file, one
//! subsection per record.

use jiff::tz::TimeZone;
use serde_json::Value;

use crate::failure::Failure;
use crate::record::{Record, Session, ToolCall, ToolResult, Turn};
use crate::text::{clean_text, code_span, escape_pipes, fenced, heading_text};

/// Records of any kind, each under a heading that names its kind, with
/// each string in a tool call's input cut to `input_chars` when given.
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
            Turn::Prompt(ref prompt) => Ok(fenced("text", &clean_text(&prompt.text))),
            Turn::Reply(ref reply) => Ok(fenced("text", &clean_text(&reply.text))),
            Turn::Call(ref call) => tool_call(call, input_chars),
            Turn::Result(ref result) => Ok(tool_result(result)),
            Turn::Summary(ref summary) => Ok(fenced("text", &clean_text(&summary.text))),
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
/// the output in a text block.
fn tool_result(result: &ToolResult) -> String {
    format!(
        "- Tool: {}\n- Error: {}\n\n{}",
        tool_name(&result.tool),
        if result.is_error { "yes" } else { "no" },
        fenced("text", &clean_text(&result.text))
    )
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
