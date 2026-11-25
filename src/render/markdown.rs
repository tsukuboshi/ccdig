//! Markdown for reading the records: one section per transcript file, one
//! subsection per record.

use jiff::tz::TimeZone;

use crate::failure::Failure;
use crate::record::{Kind, MatchLine, Record, Session, ToolCall, ToolResult, Turn};
use crate::text::{
    clean_text, code_span, escape_inline, escape_pipes, fenced, heading_text, one_line,
};

/// Records of any kind, each under a heading that names its kind. A record
/// with `matches` shows those lines in place of its text.
///
/// # Errors
///
/// When a time cannot be shown in `zone`, or an input cannot be encoded.
pub fn records(records: &[Turn], zone: &TimeZone) -> Result<String, Failure> {
    sections(
        "Records",
        records,
        zone,
        |turn| match *turn {
            Turn::Prompt(ref prompt) if prompt.kind == Kind::Rejection => "Rejection",
            Turn::Prompt(_) => "Prompt",
            Turn::Reply(_) => "Reply",
            Turn::Call(_) => "Tool call",
            Turn::Result(_) => "Tool result",
            Turn::Summary(_) => "Summary",
        },
        |turn| match *turn {
            Turn::Prompt(ref prompt) => Ok(text_or_lines(&prompt.text, &prompt.search.matches)),
            Turn::Reply(ref reply) => Ok(text_or_lines(&reply.text, &reply.search.matches)),
            Turn::Call(ref call) => tool_call(call),
            Turn::Result(ref result) => Ok(tool_result(result)),
            Turn::Summary(ref summary) => Ok(text_or_lines(&summary.text, &summary.search.matches)),
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
            if !record.matched() {
                out.push_str(" (context)");
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

/// Sessions as a table, with how many prompts and replies a search
/// matched in each, and the first of them, when `matches` is set.
///
/// # Errors
///
/// When a time cannot be shown in `zone`.
pub fn sessions(records: &[Session], zone: &TimeZone, matches: bool) -> Result<String, Failure> {
    let mut out = String::from(if matches {
        "# Sessions\n\n| Started | Project | Prompts | Matches | First match | Title |\n| --- | --- | ---: | ---: | --- | --- |\n"
    } else {
        "# Sessions\n\n| Started | Project | Prompts | Title |\n| --- | --- | ---: | --- |\n"
    });
    for session in records {
        out.extend([
            "| ",
            &session.started_at.minute_label(zone)?,
            " | ",
            &escape_pipes(&code_span(&session.project)),
            " | ",
            &session.prompt_count.to_string(),
            " | ",
        ]);
        if matches {
            let first = session
                .first_match
                .as_ref()
                .map_or_else(String::new, |found| escape_inline(&one_line(&found.text)));
            out.extend([
                &session.match_count.unwrap_or_default().to_string(),
                " | ",
                &first,
                " | ",
            ]);
        }
        out.extend([&heading_text(&session.title), " |\n"]);
    }
    Ok(out)
}

/// A tool result's body: the calling tool's name, whether it failed, why
/// the call did not run when it did not, and the output in a text block, or
/// its matched lines.
fn tool_result(result: &ToolResult) -> String {
    let mut head = format!(
        "- Tool: {}\n- Error: {}\n",
        tool_name(&result.tool),
        if result.is_error { "yes" } else { "no" },
    );
    if !result.denial.is_empty() {
        head.extend(["- Denial: ", &code_span(&result.denial), "\n"]);
    }
    if result.search.matches.is_empty() {
        return format!("{head}\n{}", fenced("text", &clean_text(&result.text)));
    }
    format!("{head}{}", lines(&result.search.matches))
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

/// A tool call's body: the tool's name and its input as indented JSON.
fn tool_call(call: &ToolCall) -> Result<String, Failure> {
    if !call.search.matches.is_empty() {
        return Ok(format!(
            "- Tool: {}\n{}",
            tool_name(&call.tool),
            lines(&call.search.matches)
        ));
    }
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
