//! Markdown for reading the records: one section per transcript file, one
//! subsection per record.

use jiff::tz::TimeZone;

use super::record::{Prompt, Record, Reply, Session, ToolCall, ToolResult, Turn};
use crate::failure::Failure;
use crate::text::{clean_text, code_span, escape_pipes, fenced, heading_text};

/// Prompts, each in a text block.
///
/// # Errors
///
/// When a time cannot be shown in `zone`.
pub fn prompts(records: &[Prompt], zone: &TimeZone) -> Result<String, Failure> {
    sections(
        "Prompts",
        records,
        zone,
        |_| "",
        |prompt| Ok(fenced("text", &clean_text(&prompt.text))),
    )
}

/// Replies, each in a text block.
///
/// # Errors
///
/// When a time cannot be shown in `zone`.
pub fn replies(records: &[Reply], zone: &TimeZone) -> Result<String, Failure> {
    sections(
        "Replies",
        records,
        zone,
        |_| "",
        |reply| Ok(fenced("text", &clean_text(&reply.text))),
    )
}

/// A conversation: each prompt, reply, and tool call under a heading that
/// names which it is.
///
/// # Errors
///
/// When a time cannot be shown in `zone`, or an input cannot be encoded.
pub fn conversation(records: &[Turn], zone: &TimeZone) -> Result<String, Failure> {
    sections(
        "Conversation",
        records,
        zone,
        |turn| match *turn {
            Turn::Prompt(_) => "Prompt",
            Turn::Reply(_) => "Reply",
            Turn::Tool(_) => "Tool call",
        },
        |turn| match *turn {
            Turn::Prompt(ref prompt) => Ok(fenced("text", &clean_text(&prompt.text))),
            Turn::Reply(ref reply) => Ok(fenced("text", &clean_text(&reply.text))),
            Turn::Tool(ref call) => tool_call(call),
        },
    )
}

/// Groups `records` under one heading per transcript file, with times shown
/// in `zone`, each record's heading followed by its `label` when that is
/// not empty, and each record's body written by `body`. Records of a file
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

/// Tool calls, each with the tool's name and its input as indented JSON.
///
/// # Errors
///
/// When a time cannot be shown in `zone`.
pub fn tool_calls(records: &[ToolCall], zone: &TimeZone) -> Result<String, Failure> {
    sections("Tool calls", records, zone, |_| "", tool_call)
}

/// Tool results, each with the calling tool's name, whether it failed, and
/// the output in a text block.
///
/// # Errors
///
/// When a time cannot be shown in `zone`.
pub fn tool_results(records: &[ToolResult], zone: &TimeZone) -> Result<String, Failure> {
    sections(
        "Tool results",
        records,
        zone,
        |_| "",
        |result| {
            Ok(format!(
                "- Tool: {}\n- Error: {}\n\n{}",
                tool_name(&result.tool),
                if result.is_error { "yes" } else { "no" },
                fenced("text", &clean_text(&result.text))
            ))
        },
    )
}

/// A tool call's body: the tool's name and its input as indented JSON.
fn tool_call(call: &ToolCall) -> Result<String, Failure> {
    let input = serde_json::to_string_pretty(&call.input).map_err(Failure::Json)?;
    Ok(format!(
        "- Tool: {}\n\n{}",
        tool_name(&call.tool),
        fenced("json", &clean_text(&input))
    ))
}

/// The tool's name as a code span, or `(unknown)` when the call was not
/// found.
fn tool_name(name: &str) -> String {
    if name.is_empty() {
        return "(unknown)".to_owned();
    }
    code_span(name)
}
