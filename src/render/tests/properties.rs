//! Properties of the writers that hold for any records.

use core::error::Error;
use std::collections::HashSet;

use proptest::bool::ANY;
use proptest::collection::vec;
use proptest::prop_oneof;
use proptest::sample::select;
use proptest::strategy::Strategy;
use serde_json::{Value, json};

use super::{
    AGENT_FILE, BLOG_FILE, at, bare_prompt, blog_head, call_at, found, outcome, reply_at,
    result_at, sample_session, source, tokyo,
};
use crate::record::{
    AGENT, FirstMatch, Head, MatchLine, Prompt, Reply, Search, Session, Spawned, Summary, ToolCall,
    ToolResult, Turn,
};
use crate::redact::Counts;
use crate::render::digest::{Limits, digest};
use crate::render::html::{self, Masking};
use crate::render::json::{Fields, Run, document, lines};
use crate::render::markdown;
use crate::render::words;
use crate::testing::{TestResult, check_eq, for_all, hostile_text, json_value};
use crate::text::{Lang, clean_text, one_line};

/// Pieces of Markdown and HTML that would run a script, load something,
/// or break out of where they stand, if written as they are.
const ATTACKS: &[&str] = &[
    "<script>alert(1)</script>",
    "<img src=x onerror=alert(1)>",
    "<a href=\"javascript:alert(1)\">x</a>",
    "<iframe src=\"https://evil.example/\"></iframe>",
    "<style>body { display: none }</style>",
    "<!-- x -->",
    "[x](javascript:alert(1))",
    "[x](JaVaScRiPt:alert(1) \"t\\\"onmouseover=\")",
    "[x](data:text/html,<script>alert(1)</script>)",
    "[x][r]\n\n[r]: javascript:alert(1)",
    "<javascript:alert(1)>",
    "<mail@evil.example>",
    "![x](https://evil.example/x.png)",
    "![x **y**](javascript:alert(1))",
    "[ok](https://ok.example/?a=\"b\" 'c')",
    "<https://ok.example/>",
    "| a | b |\n|:--|--:|\n| c | d |",
    "# heading {#id .class onclick=x}",
    "```\"><script>\nx\n```",
    "&lt;script&gt; &#60;b&#62;",
    "\"' onmouseover=alert(1) x='",
    "run &#x202E;txt.exe&#x202C; [a&#x1b;](https://ok.example/ \"&#x202E;\")",
];

/// The elements the page may hold.
const ELEMENTS: &[&str] = &[
    "a",
    "article",
    "blockquote",
    "body",
    "br",
    "code",
    "dd",
    "del",
    "details",
    "div",
    "dl",
    "dt",
    "em",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "li",
    "mark",
    "meta",
    "ol",
    "p",
    "pre",
    "section",
    "span",
    "strong",
    "style",
    "summary",
    "table",
    "tbody",
    "td",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "ul",
];

/// The attributes the page may hold.
const ATTRIBUTES: &[&str] = &[
    "charset",
    "class",
    "content",
    "datetime",
    "href",
    "http-equiv",
    "id",
    "lang",
    "name",
    "start",
    "title",
];

/// A session of hostile prompts: a title, a project, a session ID, and
/// the prompts.
type Group = (String, String, String, Vec<String>);

/// A tag of the page: its element and its attributes with their values.
type Tag = (String, Vec<(String, String)>);

/// Up to three sessions of up to five prompts, each in its own file. A
/// prompt has text: `classify` drops a blank one.
fn sessions_of_prompts() -> impl Strategy<Value = Vec<Group>> {
    let prompt = hostile_text().prop_filter("a prompt has text", |text| !one_line(text).is_empty());
    vec(
        (
            hostile_text(),
            hostile_text(),
            hostile_text(),
            vec(prompt, 1..6),
        ),
        1..4,
    )
}

/// The prompts of `groups`, those of a group adjacent and in one file,
/// every other one after a reply that is the text of the one before.
fn prompts(groups: &[Group]) -> Result<Vec<Prompt>, String> {
    let mut all = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        let (title, project, session, texts) = (&group.0, &group.1, &group.2, &group.3);
        let head = Head {
            masked: Counts::default(),
            agent: AGENT,
            agent_version: String::new(),
            session_id: session.clone(),
            session_title: title.clone(),
            session_title_then: title.clone(),
            project: project.clone(),
            cwd: project.clone(),
            git_branch: String::new(),
            timestamp: at("2026-10-13T01:00:00Z")?,
            turn: 0,
        };
        let file = format!("/sessions/{index}.jsonl");
        let mut before = "";
        let mut odd = false;
        for text in texts {
            all.push(Prompt {
                after: if odd {
                    before.to_owned()
                } else {
                    String::new()
                },
                ..bare_prompt(head.clone(), &file, text)
            });
            before = text;
            odd = !odd;
        }
    }
    Ok(all)
}

/// The lines of `markdown` outside fenced code blocks.
fn outside_fences(markdown: &str) -> Vec<&str> {
    let mut open: Option<usize> = None;
    let mut outside = Vec::new();
    for line in markdown.lines() {
        let indented = line
            .len()
            .saturating_sub(line.trim_start_matches(' ').len())
            <= 3;
        let bare = line.trim_start_matches(' ');
        let run = bare.chars().take_while(|&ch| ch == '`').count();
        match open {
            Some(width) => {
                let rest = bare.trim_start_matches('`');
                if indented && run >= width && rest.trim().is_empty() {
                    open = None;
                }
            }
            None if indented && run >= 3 => open = Some(run),
            None => outside.push(line),
        }
    }
    outside
}

/// The Markdown `records` writes of `turns`, with the times in Tokyo, and
/// its headings outside fenced code blocks.
fn record_headings(turns: &[Turn]) -> Result<(String, Vec<String>), Box<dyn Error>> {
    let out = markdown::records(turns, &tokyo()?, &Fields::default(), Lang::En)?;
    let headings = outside_fences(&out)
        .into_iter()
        .filter(|line| line.trim_start().starts_with('#'))
        .map(str::to_owned)
        .collect();
    Ok((out, headings))
}

/// The text of a code span, or `None` when `span` is not one.
fn span_text(span: &str) -> Option<&str> {
    let width = span.chars().take_while(|&ch| ch == '`').count();
    let fence = "`".repeat(width);
    let inner = span.strip_prefix(&fence)?.strip_suffix(&fence)?;
    if width > 1 {
        return inner.strip_prefix(' ')?.strip_suffix(' ');
    }
    Some(inner)
}

#[test]
fn json_lines_hold_one_record_per_line_without_raw_separators() -> TestResult {
    for_all(&vec(json_value(), 0..5), |items| {
        let out = lines(&items, &Fields::default())?;
        check_eq(
            &out.contains(['\u{2028}', '\u{2029}']),
            &false,
            "raw separators",
        )?;
        let read: Vec<Value> = out
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        check_eq(&read, &items, "records read back")?;
        Ok(())
    })
}

#[test]
fn a_json_document_reads_back_its_records() -> TestResult {
    for_all(
        &(vec(json_value(), 0..5), hostile_text()),
        |(items, filter)| {
            let out = document(
                &filter,
                "0.0.1",
                &Run::default(),
                &items,
                &Fields::default(),
            )?;
            check_eq(
                &out.contains(['\u{2028}', '\u{2029}']),
                &false,
                "raw separators",
            )?;
            let read: Value = serde_json::from_str(&out)?;
            check_eq(&read.get("items"), &Some(&Value::Array(items)), "items")?;
            check_eq(
                &read.get("filter").and_then(Value::as_str),
                &Some(filter.as_str()),
                "filter",
            )?;
            Ok(())
        },
    )
}

#[test]
fn the_digest_lists_or_counts_every_prompt_within_the_limits() -> TestResult {
    let all_limits = (1_usize..50, 0_usize..200).prop_map(|(prompt_chars, session_chars)| Limits {
        prompt_chars,
        session_chars,
    });
    for_all(&(sessions_of_prompts(), all_limits), |(groups, limits)| {
        let out = digest(&prompts(&groups)?, &tokyo()?, limits, Lang::En)?;
        let sections: Vec<&str> = out.split("\n\n").collect();
        check_eq(&sections.len(), &groups.len(), "sections")?;
        for (section, group) in sections.iter().zip(&groups) {
            let mut listed = 0_usize;
            let mut omitted = 0_usize;
            let mut used = 0_usize;
            for line in section.lines().skip(1) {
                if let Some(after) = line.strip_prefix("  - after: ") {
                    let length = span_text(after).ok_or("not a code span")?.chars().count();
                    check_eq(
                        &(length <= limits.prompt_chars.saturating_add(1)),
                        &true,
                        after,
                    )?;
                    used = used.saturating_add(length);
                    continue;
                }
                let item = line.strip_prefix("- ").ok_or("not a list item")?;
                if let Some(count) = item
                    .strip_prefix('(')
                    .and_then(|rest| rest.strip_suffix(" more prompts omitted)"))
                {
                    omitted = count.parse()?;
                } else {
                    let length = span_text(item).ok_or("not a code span")?.chars().count();
                    check_eq(
                        &(length <= limits.prompt_chars.saturating_add(1)),
                        &true,
                        item,
                    )?;
                    used = used.saturating_add(length);
                    listed = listed.saturating_add(1);
                }
            }
            check_eq(
                &listed.saturating_add(omitted),
                &group.3.len(),
                "listed and omitted",
            )?;
            check_eq(&(used <= limits.session_chars), &true, "characters listed")?;
        }
        Ok(())
    })
}

#[test]
fn record_markdown_has_only_the_headings_the_writer_puts() -> TestResult {
    for_all(&sessions_of_prompts(), |groups| {
        let all = prompts(&groups)?;
        let count = all.len();
        let turns: Vec<Turn> = all.into_iter().map(Turn::Prompt).collect();
        let (out, headings) = record_headings(&turns)?;
        check_eq(
            &headings.len(),
            &(1_usize.saturating_add(groups.len()).saturating_add(count)),
            &out,
        )?;
        Ok(())
    })
}

#[test]
fn a_sub_agent_name_adds_no_heading() -> TestResult {
    for_all(&vec(hostile_text(), 1..5), |names| {
        let turns: Vec<Turn> = names
            .iter()
            .map(|name| {
                reply_at("2026-10-13T01:24:00Z", BLOG_FILE, 1, "Drafted the report.").map(|reply| {
                    Turn::Reply(Reply {
                        subagent: name.clone(),
                        ..reply
                    })
                })
            })
            .collect::<Result<_, _>>()?;
        let (out, headings) = record_headings(&turns)?;
        check_eq(&headings.len(), &names.len().saturating_add(2), &out)?;
        let named = headings
            .iter()
            .filter(|line| line.starts_with("### ") && line.contains(" - Reply"))
            .count();
        check_eq(&named, &names.len(), &out)?;
        Ok(())
    })
}

#[test]
fn matched_lines_add_no_heading() -> TestResult {
    let matched = (hostile_text(), hostile_text(), 1_usize..4, ANY);
    for_all(&vec(vec(matched, 1..6), 1..4), |records| {
        let turns: Vec<Turn> = records
            .iter()
            .map(|lines| {
                reply_at("2026-10-13T01:24:00Z", BLOG_FILE, 1, "").map(|reply| {
                    Turn::Reply(Reply {
                        search: Search {
                            matched: true,
                            matches: lines
                                .iter()
                                .map(|&(ref field, ref text, number, hit)| MatchLine {
                                    field: field.clone(),
                                    line: number,
                                    text: text.clone(),
                                    hit,
                                })
                                .collect(),
                        },
                        ..reply
                    })
                })
            })
            .collect::<Result<_, _>>()?;
        let (out, headings) = record_headings(&turns)?;
        check_eq(&headings.len(), &records.len().saturating_add(2), &out)?;
        Ok(())
    })
}

#[test]
fn every_session_row_has_six_cells_or_eight_with_matches() -> TestResult {
    for_all(
        &(
            vec(
                (
                    hostile_text(),
                    hostile_text(),
                    hostile_text(),
                    hostile_text(),
                    hostile_text(),
                ),
                0..4,
            ),
            ANY,
        ),
        |(rows, matches)| {
            let sessions: Vec<Session> = rows
                .iter()
                .map(|row| {
                    sample_session().map(|session| Session {
                        title: row.0.clone(),
                        project: row.1.clone(),
                        id: row.3.clone(),
                        cwd: row.4.clone(),
                        first_match: Some(FirstMatch {
                            kind: "reply",
                            timestamp: session.started_at,
                            line: Some(1),
                            text: row.2.clone(),
                        }),
                        ..session
                    })
                })
                .collect::<Result<_, _>>()?;
            let out =
                markdown::sessions(&sessions, &tokyo()?, matches, &Fields::default(), Lang::En)?;
            let table: Vec<&str> = out.lines().filter(|line| line.starts_with('|')).collect();
            check_eq(&table.len(), &rows.len().saturating_add(2), &out)?;
            for row in table {
                let cells = row.replace("\\|", "").matches('|').count();
                check_eq(&cells, &(if matches { 9 } else { 7 }), row)?;
            }
            Ok(())
        },
    )
}

/// What `markdown` is built of, line by line, apart from its words: for
/// each line, whether it is outside fenced code blocks, how many `#` start
/// it, whether it is a list item, and how many bare `|` split it.
fn skeleton(markdown: &str) -> Vec<(bool, usize, bool, usize)> {
    let outside = outside_fences(markdown);
    let mut rest = outside.iter().peekable();
    markdown
        .lines()
        .map(|line| {
            let out = rest
                .peek()
                .is_some_and(|next| next.as_ptr() == line.as_ptr());
            if out {
                rest.next();
            }
            (
                out,
                line.chars().take_while(|&ch| ch == '#').count(),
                line.trim_start().starts_with("- "),
                line.replace("\\|", "").matches('|').count(),
            )
        })
        .collect()
}

#[test]
fn japanese_markdown_and_digest_are_built_as_the_english_ones_are() -> TestResult {
    for_all(&sessions_of_prompts(), |groups| {
        let all = prompts(&groups)?;
        let turns: Vec<Turn> = all.iter().cloned().map(Turn::Prompt).collect();
        let zone = tokyo()?;
        let records = |lang| markdown::records(&turns, &zone, &Fields::default(), lang);
        check_eq(
            &skeleton(&records(Lang::Ja)?),
            &skeleton(&records(Lang::En)?),
            &records(Lang::Ja)?,
        )?;
        let limits = Limits {
            prompt_chars: 20,
            session_chars: 60,
        };
        let digests = |lang| digest(&all, &zone, limits, lang);
        check_eq(
            &skeleton(&digests(Lang::Ja)?),
            &skeleton(&digests(Lang::En)?),
            &digests(Lang::Ja)?,
        )?;
        let sessions: Vec<Session> = groups
            .iter()
            .map(|group| {
                sample_session().map(|session| Session {
                    title: group.0.clone(),
                    project: group.1.clone(),
                    id: group.2.clone(),
                    ..session
                })
            })
            .collect::<Result<_, _>>()?;
        let tables = |lang| markdown::sessions(&sessions, &zone, true, &Fields::default(), lang);
        check_eq(
            &skeleton(&tables(Lang::Ja)?),
            &skeleton(&tables(Lang::En)?),
            &tables(Lang::Ja)?,
        )?;
        Ok(())
    })
}

#[test]
fn a_title_is_one_line_and_never_empty() -> TestResult {
    for_all(&hostile_text(), |title| {
        for lang in [Lang::En, Lang::Ja] {
            let shown = words::title(lang, &title);
            check_eq(&shown.contains('\n'), &false, "a line break")?;
            check_eq(&shown.is_empty(), &false, "empty")?;
            if one_line(&title).is_empty() {
                check_eq(&shown, &words::title(lang, ""), "untitled")?;
            }
        }
        Ok(())
    })
}

/// Text that mixes hostile characters with the pieces of `ATTACKS`.
fn hostile_markdown() -> impl Strategy<Value = String> {
    let piece = prop_oneof![hostile_text(), select(ATTACKS).prop_map(str::to_owned)];
    let separators = select(&["", " ", "\n", "\n\n"][..]);
    vec((piece, separators), 0..5).prop_map(|pieces| {
        pieces
            .into_iter()
            .flat_map(|(text, separator)| [text, separator.to_owned()])
            .collect()
    })
}

/// The tags of `page`, but for the doctype, the end tags, and what the
/// style element holds. Every `<` in the page starts a tag, as the text of
/// the records is escaped.
fn tags(page: &str) -> Result<Vec<Tag>, String> {
    let (before, rest) = page
        .split_once("<style>")
        .ok_or_else(|| format!("no style:\n{page}"))?;
    let (_, after) = rest
        .split_once("</style>")
        .ok_or_else(|| format!("no end of style:\n{page}"))?;
    let mut found = Vec::new();
    for chunk in [before, after] {
        for part in chunk.split('<').skip(1) {
            let (inside, _) = part
                .split_once('>')
                .ok_or_else(|| format!("an open tag: <{part}"))?;
            if inside.starts_with('/') || inside.starts_with("!doctype") {
                continue;
            }
            found.push(tag(inside)?);
        }
    }
    Ok(found)
}

/// The element and the attributes of a tag, from what is between its `<`
/// and its `>`.
fn tag(between: &str) -> Result<Tag, String> {
    let inside = between.trim_end_matches('/').trim_end();
    let (element, mut rest) = inside.split_once(' ').unwrap_or((inside, ""));
    let mut attributes = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return Ok((element.to_owned(), attributes));
        }
        let (name, quoted) = rest
            .split_once("=\"")
            .ok_or_else(|| format!("an attribute without a quoted value: <{inside}>"))?;
        let (value, after) = quoted
            .split_once('"')
            .ok_or_else(|| format!("an open value: <{inside}>"))?;
        attributes.push((name.to_owned(), value.to_owned()));
        rest = after;
    }
}

/// Records of each kind, with every text that shows taken from `texts`.
fn hostile_turns(texts: &[String]) -> Result<Vec<Turn>, String> {
    let text = |index: usize| texts.get(index).cloned().unwrap_or_default();
    let head = |time: &str| -> Result<Head, String> {
        Ok(Head {
            session_title: text(0),
            project: text(1),
            session_id: text(2),
            git_branch: text(3),
            turn: 1,
            ..blog_head(time)?
        })
    };
    let ran = outcome(true, &text(12), &text(11));
    let input = json!({
        "command": text(10),
        "file_path": text(13),
        text(11): text(10),
    });
    Ok(vec![
        Turn::Prompt(bare_prompt(
            head("2026-10-13T01:23:00Z")?,
            BLOG_FILE,
            &text(4),
        )),
        Turn::Reply(Reply {
            head: head("2026-10-13T01:24:00Z")?,
            ..reply_at("2026-10-13T01:24:00Z", BLOG_FILE, 2, &text(5))?
        }),
        Turn::Thinking(Reply {
            head: head("2026-10-13T01:24:00Z")?,
            ..reply_at("2026-10-13T01:24:00Z", BLOG_FILE, 3, &text(6))?
        }),
        Turn::Summary(Summary {
            head: head("2026-10-13T01:24:00Z")?,
            text: text(7),
            redacted: false,
            source: source(BLOG_FILE, 4, &text(13)),
            search: Search::default(),
            masked: Counts::default(),
        }),
        Turn::Call(ToolCall {
            head: head("2026-10-13T01:25:00Z")?,
            ..call_at(5, &text(9), input.clone(), "toolu_1", Some(ran.clone()))?
        }),
        Turn::Call(ToolCall {
            head: head("2026-10-13T01:25:00Z")?,
            ..call_at(6, &text(9), input, "toolu_2", None)?
        }),
        Turn::Result(ToolResult {
            head: head("2026-10-13T01:25:00Z")?,
            ..result_at(7, &text(9), "toolu_3", &ran)?
        }),
        Turn::Reply(Reply {
            head: head("2026-10-13T01:26:00Z")?,
            search: found(&[("text", 1, &text(13), true), ("text", 2, &text(5), false)]),
            ..reply_at("2026-10-13T01:26:00Z", BLOG_FILE, 8, &text(5))?
        }),
        Turn::Reply(Reply {
            head: head("2026-10-13T01:27:00Z")?,
            subagent: text(8),
            spawned: Spawned {
                subagent_type: text(8),
                subagent_description: text(13),
                parent_tool_use_id: "toolu_1".to_owned(),
            },
            ..reply_at("2026-10-13T01:27:00Z", AGENT_FILE, 1, &text(5))?
        }),
    ])
}

#[test]
fn the_page_has_only_the_elements_and_attributes_it_writes_and_no_id_twice() -> TestResult {
    for_all(&vec(hostile_markdown(), 14..=14), |texts| {
        let turns = hostile_turns(&texts)?;
        let masking = Masking::On(vec![(texts.first().cloned().unwrap_or_default(), 1)]);
        let page = html::records(&turns, &tokyo()?, &Fields::default(), Lang::En, &masking)?;
        // Nothing that drives a terminal or reorders the text, not even
        // what a character reference in the Markdown stands for.
        check_eq(&clean_text(&page), &page, "a control character")?;
        // The session's file and the sub-agent's, each once.
        check_eq(&page.matches("<section").count(), &2, &page)?;
        let mut ids = HashSet::new();
        for (element, attributes) in tags(&page)? {
            check_eq(
                &ELEMENTS.contains(&element.as_str()),
                &true,
                &format!("<{element}>\n{page}"),
            )?;
            for (name, value) in attributes {
                check_eq(
                    &ATTRIBUTES.contains(&name.as_str()),
                    &true,
                    &format!("{name} on <{element}>\n{page}"),
                )?;
                if name == "id" {
                    check_eq(
                        &ids.insert(value.clone()),
                        &true,
                        &format!("id=\"{value}\" again\n{page}"),
                    )?;
                }
                if name == "href" {
                    let web = ["http://", "https://", "#"]
                        .iter()
                        .any(|start| value.starts_with(start));
                    check_eq(&web, &true, &format!("href=\"{value}\"\n{page}"))?;
                }
            }
        }
        Ok(())
    })
}
