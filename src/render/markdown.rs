//! Markdown for reading the records: one section per transcript file, one
//! subsection per record.

use jiff::tz::TimeZone;

use super::json::Fields;
use super::table::{self, Cell, Table};
use super::view::{
    self, Body, CallBody, Change, Count, Ending, Fact, Input, Passage, ResultBody, Run, Tool, View,
};
use super::words::{self, Item};
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

/// Each of `values` as inline code, separated by commas.
fn code_list<T>(values: &[T]) -> String
where
    T: AsRef<str>,
{
    values
        .iter()
        .map(|value| code_span(value.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
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
        | Fact::PermissionModes(ref values) => code_list(values),
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
    let shown = table::sessions(records, matches, fields, lang);
    let mut out = format!(
        "# {}\n\n{}",
        words::sessions_title(lang),
        words::session_count(lang, shown.count)
    );
    if let Some((total, ref types)) = shown.matches {
        out.push_str(&words::match_count(lang, total, &words::by_type(types)));
    }
    write_table(&mut out, &shown.table, zone, lang)?;
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
    let shown = table::stats(groups, columns, fields, lang);
    let mut out = words::stats_head(lang, shown.groups);
    write_table(&mut out, &shown.table, zone, lang)?;
    Ok(out)
}

/// `table` after a blank line, with numbers aligned right and times in
/// `zone`; nothing when it has no columns.
fn write_table(
    out: &mut String,
    table: &Table<'_>,
    zone: &TimeZone,
    lang: Lang,
) -> Result<(), Failure> {
    if table.columns.is_empty() {
        return Ok(());
    }
    let names: Vec<&str> = table
        .columns
        .iter()
        .map(|column| column.text.as_str())
        .collect();
    let aligns: Vec<&str> = table
        .columns
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
    for row in &table.rows {
        let cells = row
            .iter()
            .map(|&value| cell(value, zone, lang))
            .collect::<Result<Vec<_>, _>>()?;
        out.extend(["| ", &cells.join(" | "), " |\n"]);
    }
    Ok(())
}

/// `value` as a table cell, with a time in `zone`.
fn cell(value: Cell<'_>, zone: &TimeZone, lang: Lang) -> Result<String, Failure> {
    Ok(match value {
        Cell::Code("") | Cell::Empty => String::new(),
        Cell::Code(text) => escape_pipes(&code_span(text)),
        Cell::Codes(names) => escape_pipes(&code_list(names)),
        Cell::Count(count) => count.to_string(),
        Cell::Text(text) => escape_inline(&one_line(text)),
        Cell::Time(stamp) => stamp.minute_label(zone)?,
        Cell::Title(title) => words::title(lang, title),
        Cell::Tokens(count) => count.to_string(),
    })
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
            change,
            ref json,
            ref texts,
        } => tool_input(json.as_deref(), texts, change),
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
/// `texts` in a text block named for its key, so that it reads as written,
/// then what an `Edit` changed in a diff block.
fn tool_input(json: Option<&str>, texts: &[(&str, &str)], change: Option<Change<'_>>) -> String {
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
    if let Some(edit) = change {
        let lines: Vec<String> = edit
            .lines()
            .map(|(sign, line)| format!("{sign}{line}"))
            .collect();
        parts.push(format!(
            "- {} \u{2192} {}\n\n{}",
            code_span("input.old_string"),
            code_span("input.new_string"),
            fenced("diff", &clean_text(&lines.join("\n")))
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
