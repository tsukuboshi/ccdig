//! Properties of the writers that hold for any records.

use proptest::bool::ANY;
use proptest::collection::vec;
use proptest::strategy::Strategy;
use serde_json::Value;

use super::{BLOG_FILE, at, bare_prompt, blog_head, sample_session, source, tokyo};
use crate::record::{AGENT, Head, MatchLine, Prompt, Reply, Session, Turn};
use crate::redact::Counts;
use crate::render::digest::{Limits, digest};
use crate::render::json::{Run, document, lines};
use crate::render::markdown;
use crate::testing::{TestResult, check_eq, for_all, hostile_text, json_value};
use crate::text::one_line;

/// A session of hostile prompts: a title, a project, a session ID, and
/// the prompts.
type Group = (String, String, String, Vec<String>);

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

/// The prompts of `groups`, those of a group adjacent and in one file.
fn prompts(groups: &[Group]) -> Result<Vec<Prompt>, String> {
    let mut all = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        let (title, project, session, texts) = (&group.0, &group.1, &group.2, &group.3);
        let head = Head {
            masked: Counts::default(),
            agent: AGENT,
            session_id: session.clone(),
            session_title: title.clone(),
            project: project.clone(),
            cwd: project.clone(),
            git_branch: String::new(),
            timestamp: at("2026-10-13T01:00:00Z")?,
        };
        let file = format!("/sessions/{index}.jsonl");
        all.extend(
            texts
                .iter()
                .map(|text| bare_prompt(head.clone(), &file, text)),
        );
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
        let out = lines(&items)?;
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
            let out = document(&filter, "0.0.1", &Run::default(), &items)?;
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
        let out = digest(&prompts(&groups)?, &tokyo()?, limits)?;
        let sections: Vec<&str> = out.split("\n\n").collect();
        check_eq(&sections.len(), &groups.len(), "sections")?;
        for (section, group) in sections.iter().zip(&groups) {
            let mut listed = 0_usize;
            let mut omitted = 0_usize;
            let mut used = 0_usize;
            for line in section.lines().skip(1) {
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
        let out = markdown::records(&turns, &tokyo()?, None)?;
        let headings = outside_fences(&out)
            .into_iter()
            .filter(|line| line.trim_start().starts_with('#'))
            .count();
        check_eq(
            &headings,
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
                blog_head("2026-10-13T01:24:00Z").map(|head| {
                    Turn::Reply(Reply {
                        usage: None,
                        masked: Counts::default(),
                        head,
                        text: "Drafted the report.".to_owned(),
                        model: String::new(),
                        skill: String::new(),
                        subagent: name.clone(),
                        redacted: false,
                        source: source(BLOG_FILE, 1, "a-1"),
                        matched: true,
                        matches: Vec::new(),
                    })
                })
            })
            .collect::<Result<_, _>>()?;
        let out = markdown::records(&turns, &tokyo()?, None)?;
        let headings: Vec<&str> = outside_fences(&out)
            .into_iter()
            .filter(|line| line.trim_start().starts_with('#'))
            .collect();
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
                blog_head("2026-10-13T01:24:00Z").map(|head| {
                    Turn::Reply(Reply {
                        usage: None,
                        masked: Counts::default(),
                        head,
                        text: String::new(),
                        model: String::new(),
                        skill: String::new(),
                        subagent: String::new(),
                        redacted: false,
                        source: source(BLOG_FILE, 1, "a-1"),
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
                    })
                })
            })
            .collect::<Result<_, _>>()?;
        let out = markdown::records(&turns, &tokyo()?, None)?;
        let headings = outside_fences(&out)
            .into_iter()
            .filter(|line| line.trim_start().starts_with('#'))
            .count();
        check_eq(&headings, &records.len().saturating_add(2), &out)?;
        Ok(())
    })
}

#[test]
fn every_session_row_has_four_cells() -> TestResult {
    for_all(&vec((hostile_text(), hostile_text()), 0..4), |rows| {
        let sessions: Vec<Session> = rows
            .iter()
            .map(|row| {
                sample_session().map(|session| Session {
                    title: row.0.clone(),
                    project: row.1.clone(),
                    ..session
                })
            })
            .collect::<Result<_, _>>()?;
        let out = markdown::sessions(&sessions, &tokyo()?)?;
        let table: Vec<&str> = out.lines().filter(|line| line.starts_with('|')).collect();
        check_eq(&table.len(), &rows.len().saturating_add(2), &out)?;
        for row in table {
            let cells = row.replace("\\|", "").matches('|').count();
            check_eq(&cells, &5, row)?;
        }
        Ok(())
    })
}
