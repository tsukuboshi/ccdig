//! Tests for the writers, against snapshots.

mod html;
mod properties;
mod view;

use core::error::Error;
use core::slice::from_ref;

use jiff::tz::TimeZone;
use serde::Serialize;
use serde_json::json;

use super::digest::{Limits, digest};
use super::json::{Fields, Run, document, lines};
use super::markdown;
use super::view::Fact;
use super::words;
use crate::record::{
    AGENT, Group, Head, Kind, MatchLine, Outcome, Prompt, RECORD_KEYS, Reply, SESSION_KEYS,
    STATS_KEYS, Search, Session, Source, Spawned, Summary, ToolCall, ToolResult, Touch, Turn,
};
use crate::redact::Counts;
use crate::stats::{By, Grouping, columns, tally};
use crate::testing::{TestResult, check_eq, snapshot, zone};
use crate::text::Lang;
use crate::transcript::entry::Usage;
use crate::when::Stamp;

/// A sub-agent's transcript file in the blog session.
const AGENT_FILE: &str =
    "/home/alice/.claude/projects/-Users-alice-blog/1111/subagents/agent-7.jsonl";

/// The blog session's transcript file.
const BLOG_FILE: &str = "/home/alice/.claude/projects/-Users-alice-blog/1111.jsonl";

/// The blog session's ID.
const BLOG_SESSION: &str = "11111111-1111-4111-8111-111111111111";

/// A time from an RFC 3339 string.
fn at(text: &str) -> Result<Stamp, String> {
    Stamp::parse_rfc3339(text).ok_or_else(|| format!("not RFC 3339: {text}"))
}

/// The shared keys of a record in the blog session at `time`.
fn blog_head(time: &str) -> Result<Head, String> {
    Ok(Head {
        masked: Counts::default(),
        agent: AGENT,
        session_id: BLOG_SESSION.to_owned(),
        session_title: "Weekly report draft".to_owned(),
        session_title_then: "Weekly report draft".to_owned(),
        project: "-Users-alice-blog".to_owned(),
        cwd: "/Users/alice/blog".to_owned(),
        git_branch: "main".to_owned(),
        timestamp: at(time)?,
        turn: 0,
    })
}

/// A plain prompt with only the fields the digest and grouping look at.
fn bare_prompt(head: Head, file: &str, text: &str) -> Prompt {
    Prompt {
        permission_mode: String::new(),
        superseded: false,
        parent_uuid: String::new(),
        after: String::new(),
        masked: Counts::default(),
        head,
        text: text.to_owned(),
        kind: Kind::Text,
        command: String::new(),
        redacted: false,
        source: source(file, 0, ""),
        search: Search::default(),
    }
}

/// A record's location.
fn source(file: &str, line: usize, uuid: &str) -> Source {
    Source {
        block: 0,
        file: file.to_owned(),
        line,
        uuid: uuid.to_owned(),
    }
}

/// Two sample prompts.
fn sample_prompts() -> Result<Vec<Prompt>, String> {
    Ok(vec![
        Prompt {
            permission_mode: String::new(),
            superseded: false,
            parent_uuid: String::new(),
        after: String::new(),
            masked: Counts::default(),
            head: blog_head("2026-10-13T01:23:45.678Z")?,
            text: "/weekly-report last week".to_owned(),
            kind: Kind::Slash,
            command: "/weekly-report".to_owned(),
            redacted: false,
            source: source(BLOG_FILE, 4, "u-1"),
            search: Search::default(),
        },
        Prompt {
            permission_mode: String::new(),
            superseded: false,
            parent_uuid: String::new(),
        after: String::new(),
            masked: Counts::default(),
            head: blog_head("2026-10-13T01:30:00Z")?,
            text: "keep <b> tags & \u{65e5}\u{672c}\u{8a9e} as is\nexport AWS_SECRET_ACCESS_KEY=***REDACTED***".to_owned(),
            kind: Kind::Text,
            command: String::new(),
            redacted: true,
            source: source(BLOG_FILE, 9, "u-2"),
            search: Search::default(),
        },
    ])
}

/// A sample session.
fn sample_session() -> Result<Session, String> {
    Ok(Session {
        usage: Usage::default(),
        masked: Counts::default(),
        agent: AGENT,
        id: BLOG_SESSION.to_owned(),
        title: "Weekly report | draft".to_owned(),
        project: "-Users-alice-blog".to_owned(),
        cwd: "/Users/alice/blog".to_owned(),
        git_branch: "main".to_owned(),
        file: BLOG_FILE.to_owned(),
        mtime: at("2026-10-13T02:00:00Z")?,
        started_at: at("2026-10-13T01:23:45.678Z")?,
        ended_at: at("2026-10-13T01:30:00Z")?,
        prompt_count: 2,
        match_count: None,
        first_match: None,
        resume: Some("cd /Users/alice/blog && claude --resume aaaa".to_owned()),
        resume_unavailable: None,
        matched_types: Vec::new(),
    })
}

/// Two lines of `ccdig stats --by session`: one with tokens, and one
/// without a title or any.
fn sample_groups() -> Result<Vec<Group>, Box<dyn Error>> {
    let keys = |session: &str, title: Option<&str>| {
        serde_json::from_value(json!({
            "session": session,
            "session_title": title,
            "project": "-Users-alice-blog",
            "file": format!("/home/alice/.claude/projects/-Users-alice-blog/{session}.jsonl"),
        }))
    };
    Ok(vec![
        Group {
            keys: keys(BLOG_SESSION, Some("Weekly report | draft"))?,
            count: 12,
            empty: None,
            chars: 3_456,
            sessions: None,
            first: at("2026-10-13T01:23:45.678Z")?,
            last: at("2026-10-13T02:00:00Z")?,
            usage: Some(Usage {
                input: 10,
                output: 2_000,
                cache_read: 30_000,
                cache_creation: 400,
            }),
        },
        Group {
            keys: keys("22222222-2222-4222-8222-222222222222", None)?,
            count: 1,
            empty: None,
            chars: 0,
            sessions: None,
            first: at("2026-10-14T00:05:00Z")?,
            last: at("2026-10-14T00:05:00Z")?,
            usage: None,
        },
    ])
}

/// `prompts` as the records `ccdig records --type prompt` writes.
fn prompt_turns(prompts: Vec<Prompt>) -> Vec<Turn> {
    prompts.into_iter().map(Turn::Prompt).collect()
}

/// Asia/Tokyo, where the Markdown samples show their times.
fn tokyo() -> Result<TimeZone, jiff::Error> {
    zone("Asia/Tokyo")
}

#[test]
fn prompts_as_json_lines() -> TestResult {
    snapshot(
        "render/prompts.jsonl",
        &lines(&prompt_turns(sample_prompts()?), &Fields::default())?,
    )
}

#[test]
fn prompts_as_a_json_document() -> TestResult {
    let prompts = prompt_turns(sample_prompts()?);
    snapshot(
        "render/prompts.json",
        &document(
            "strict-v3",
            "0.1.0",
            &Run::default(),
            prompts.get(..1).ok_or("no prompts")?,
            &Fields::default(),
        )?,
    )
}

#[test]
fn an_empty_json_document_has_an_empty_list() -> TestResult {
    snapshot(
        "render/empty.json",
        &document::<Prompt>(
            "strict-v3",
            "0.1.0",
            &Run::default(),
            &[],
            &Fields::default(),
        )?,
    )
}

#[test]
fn prompts_as_markdown() -> TestResult {
    let mut prompts = sample_prompts()?;
    prompts.push(Prompt {
        head: Head {
            session_id: "22222222-2222-4222-8222-222222222222".to_owned(),
            session_title: String::new(),
            project: "-Users-alice-notes".to_owned(),
            cwd: String::new(),
            git_branch: String::new(),
            ..blog_head("2026-10-14T00:05:00Z")?
        },
        ..bare_prompt(
            blog_head("2026-10-14T00:05:00Z")?,
            "",
            "# not a heading\nfix this:\n```go\nfmt.Println(1)\n```",
        )
    });
    snapshot(
        "render/prompts.md",
        &markdown::records(
            &prompt_turns(prompts),
            &tokyo()?,
            &Fields::default(),
            Lang::En,
        )?,
    )
}

#[test]
fn sessions_as_json_lines() -> TestResult {
    snapshot(
        "render/sessions.jsonl",
        &lines(&[sample_session()?], &Fields::default())?,
    )
}

#[test]
fn sessions_as_a_markdown_table() -> TestResult {
    snapshot(
        "render/sessions.md",
        &markdown::sessions(
            &[sample_session()?],
            &tokyo()?,
            false,
            &Fields::default(),
            Lang::En,
        )?,
    )
}

#[test]
fn stats_as_json_lines_and_a_markdown_table() -> TestResult {
    let groups = sample_groups()?;
    snapshot("render/stats.jsonl", &lines(&groups, &Fields::default())?)?;
    snapshot(
        "render/stats.md",
        &markdown::stats(
            &groups,
            &columns(&[By::Session]),
            &tokyo()?,
            &Fields::default(),
            Lang::En,
        )?,
    )
}

#[test]
fn sessions_and_stats_tables_in_japanese() -> TestResult {
    snapshot(
        "render/sessions-ja.md",
        &markdown::sessions(
            &[sample_session()?],
            &tokyo()?,
            true,
            &Fields::default(),
            Lang::Ja,
        )?,
    )?;
    snapshot(
        "render/stats-ja.md",
        &markdown::stats(
            &sample_groups()?,
            &columns(&[By::Session]),
            &tokyo()?,
            &Fields::default(),
            Lang::Ja,
        )?,
    )
}

#[test]
fn stats_by_a_key_without_a_value_leaves_the_cell_empty_and_counts_sessions() -> TestResult {
    let group = Group {
        keys: serde_json::from_value(json!({"tool": null}))?,
        sessions: Some(2),
        ..sample_groups()?.swap_remove(1)
    };
    snapshot(
        "render/stats-tool.md",
        &markdown::stats(
            &[group],
            &columns(&[By::Tool]),
            &TimeZone::UTC,
            &Fields::default(),
            Lang::En,
        )?,
    )
}

#[test]
fn markdown_groups_by_file_not_by_session_id() -> TestResult {
    let head = blog_head("2026-10-13T01:00:00Z")?;
    let out = markdown::records(
        &prompt_turns(vec![
            bare_prompt(head.clone(), "/a.jsonl", "one"),
            bare_prompt(head, "/b.jsonl", "two"),
        ]),
        &TimeZone::UTC,
        &Fields::default(),
        Lang::En,
    )?;
    check_eq(&out.matches("\n## ").count(), &2, "session headings")?;
    Ok(())
}

#[test]
fn markdown_record_headings_tell_apart_records_within_a_minute() -> TestResult {
    let out = markdown::records(
        &prompt_turns(vec![
            bare_prompt(blog_head("2026-10-13T01:00:05Z")?, BLOG_FILE, "one"),
            bare_prompt(blog_head("2026-10-13T01:00:40Z")?, BLOG_FILE, "two"),
        ]),
        &tokyo()?,
        &Fields::default(),
        Lang::En,
    )?;
    for heading in [
        "### 2026-10-13 10:00:05 JST - Prompt\n",
        "### 2026-10-13 10:00:40 JST - Prompt\n",
    ] {
        check_eq(&out.contains(heading), &true, heading)?;
    }
    Ok(())
}

#[test]
fn line_separators_are_escaped_in_json() -> TestResult {
    let prompt = bare_prompt(
        blog_head("2026-10-13T01:00:00Z")?,
        "/a.jsonl",
        "a\u{2028}b\u{2029}c",
    );
    let out = lines(&[prompt], &Fields::default())?;
    check_eq(
        &out.contains("\"text\":\"a\\u2028b\\u2029c\""),
        &true,
        "escaped",
    )?;
    Ok(())
}

#[test]
fn the_digest_cuts_prompts_and_sessions_to_the_limits() -> TestResult {
    let blog = |time: &str, text: &str| -> Result<Prompt, String> {
        Ok(bare_prompt(
            Head {
                masked: Counts::default(),
                session_title: "Weekly report".to_owned(),
                ..blog_head(time)?
            },
            "/a.jsonl",
            text,
        ))
    };
    let other = bare_prompt(
        Head {
            masked: Counts::default(),
            session_title: String::new(),
            project: "-Users-alice-notes".to_owned(),
            cwd: String::new(),
            ..blog_head("2026-10-14T00:00:00Z")?
        },
        "/b.jsonl",
        "\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{3068}\u{3066}\u{3082}\u{9577}\u{3044}\u{4f9d}\u{983c}\u{6587}\u{3067}\u{3059}",
    );
    let prompts = vec![
        blog("2026-10-13T01:00:00Z", "short one")?,
        blog("2026-10-13T02:00:00Z", "this   is\n a long prompt")?,
        blog("2026-10-13T03:00:00Z", "third")?,
        blog("2026-10-13T04:00:00Z", "fourth")?,
        blog("2026-10-13T05:00:00Z", "fifth")?,
        other,
    ];
    let limits = Limits {
        prompt_chars: 10,
        session_chars: 25,
    };
    snapshot(
        "render/digest.md",
        &digest(&prompts, &tokyo()?, limits, Lang::En)?,
    )
}

#[test]
fn the_digest_in_japanese() -> TestResult {
    let at = |time: &str, kind: Kind, text: &str, after: &str| -> Result<Prompt, String> {
        Ok(Prompt {
            kind,
            after: after.to_owned(),
            ..bare_prompt(
                Head {
                    session_title: String::new(),
                    ..blog_head(time)?
                },
                "/a.jsonl",
                text,
            )
        })
    };
    let prompts = vec![
        at(
            "2026-10-13T01:00:00Z",
            Kind::Text,
            "see <pasted_content id=\"1\">xyz</pasted_content id=\"1\">",
            "Which one?",
        )?,
        at("2026-10-13T01:01:00Z", Kind::Answer, "\"A?\"=\"a\"", "")?,
        at(
            "2026-10-13T01:02:00Z",
            Kind::TypedAnswer,
            "\"B?\"=\"b\"",
            "",
        )?,
        at("2026-10-13T01:03:00Z", Kind::Approval, "the plan", "")?,
        at("2026-10-13T01:04:00Z", Kind::Rejection, "not that", "")?,
        at("2026-10-13T01:05:00Z", Kind::Text, "one more", "")?,
        at("2026-10-13T01:06:00Z", Kind::Text, "and another", "")?,
    ];
    let limits = Limits {
        prompt_chars: 300,
        session_chars: 70,
    };
    snapshot(
        "render/digest-ja.md",
        &digest(&prompts, &tokyo()?, limits, Lang::Ja)?,
    )
}

#[test]
fn every_stats_column_has_a_japanese_heading() -> TestResult {
    let keys = STATS_KEYS.into_iter().filter(|&key| key != "usage").chain([
        "input",
        "output",
        "cache_read",
        "cache_creation",
    ]);
    for key in keys {
        check_eq(&words::stats_column_ja(key).is_some(), &true, key)?;
    }
    Ok(())
}

#[test]
fn a_title_is_one_escaped_line_or_named_untitled() -> TestResult {
    let title = concat!(
        "title | with pipe\n## fake section <img src=x onerror=alert(1)> ",
        "[x](javascript:alert(1)) \u{202E}evil",
    );
    let want = concat!(
        "title \\| with pipe ## fake section \\<img src=x onerror=alert(1)\\> ",
        "\\[x\\](javascript:alert(1)) \u{FFFD}evil",
    );
    check_eq(
        words::title(Lang::En, title).as_str(),
        want,
        "hostile title",
    )?;
    for blank in ["", " \n\t "] {
        check_eq(
            words::title(Lang::En, blank).as_str(),
            "(untitled)",
            "blank title",
        )?;
    }
    Ok(())
}

#[test]
fn the_words_no_snapshot_shows_are_in_japanese() -> TestResult {
    // (name, what it gave, what it should give)
    for (name, got, want) in [
        (
            "an unknown tool",
            words::unknown_tool(Lang::Ja).to_owned(),
            "(\u{4e0d}\u{660e})",
        ),
        (
            "an untitled session, blank",
            words::title(Lang::Ja, " \n "),
            "(\u{7121}\u{984c})",
        ),
        (
            "a description",
            words::fact(Lang::Ja, Fact::Description("")).to_owned(),
            "\u{8aac}\u{660e}",
        ),
        (
            "one line of a field",
            words::lines(Lang::Ja, Some("`input.command`"), 3, 3, &[]),
            // `input.command` 3 行目
            "`input.command` 3 \u{884c}\u{76ee}",
        ),
        (
            "lines with several matched",
            words::lines(Lang::Ja, None, 3, 6, &["4".to_owned(), "5".to_owned()]),
            // 3-6 行目、一致 4、5
            "3-6 \u{884c}\u{76ee}\u{3001}\u{4e00}\u{81f4} 4\u{3001}5",
        ),
        (
            "the masks counted",
            words::masked(Lang::Ja, &[("email".to_owned(), 2)]),
            // マスク: email 2 件
            "\u{30de}\u{30b9}\u{30af}: email 2 \u{4ef6}",
        ),
        (
            "nothing masked",
            words::masked(Lang::Ja, &[]),
            // マスクしたものはありません
            "\u{30de}\u{30b9}\u{30af}\u{3057}\u{305f}\u{3082}\u{306e}\u{306f}\u{3042}\u{308a}\u{307e}\u{305b}\u{3093}",
        ),
        (
            "one line of a field in English",
            words::lines(Lang::En, Some("`input.command`"), 3, 3, &[]),
            "`input.command` line 3",
        ),
    ] {
        check_eq(got.as_str(), want, name)?;
    }
    Ok(())
}

/// The line the digest writes for a lone prompt of `kind` with `text`.
fn digest_line(kind: Kind, text: &str) -> Result<String, Box<dyn Error>> {
    let prompt = Prompt {
        kind,
        ..bare_prompt(blog_head("2026-10-13T01:00:00Z")?, "/a.jsonl", text)
    };
    let out = digest(&[prompt], &tokyo()?, Limits::default(), Lang::En)?;
    Ok(out.lines().last().unwrap_or_default().to_owned())
}

#[test]
fn the_digest_folds_each_paste_into_its_length() -> TestResult {
    for (name, text, want) in [
        (
            "a paste between typed words",
            "review this```\n\n<pasted_content id=\"e002\">\nline one\nline two\n</pasted_content id=\"e002\">\n\n``` please",
            "- ```` review this``` [pasted 17 chars] ``` please ````",
        ),
        (
            "two pastes",
            "a <pasted_content id=\"1\">xyz</pasted_content id=\"1\"> b <pasted_content id=\"2\">\nhello\n</pasted_content id=\"2\"> c",
            "- `a [pasted 3 chars] b [pasted 5 chars] c`",
        ),
        (
            "a paste without its closing tag",
            "a <pasted_content id=\"1\">xyz",
            "- `a <pasted_content id=\"1\">xyz`",
        ),
        (
            "a closing tag of another id",
            "a <pasted_content id=\"1\">xyz</pasted_content id=\"2\"> b",
            "- `a <pasted_content id=\"1\">xyz</pasted_content id=\"2\"> b`",
        ),
    ] {
        check_eq(&digest_line(Kind::Text, text)?, &want.to_owned(), name)?;
    }
    Ok(())
}

#[test]
fn the_digest_drops_the_previews_of_the_chosen_options() -> TestResult {
    for (name, kind, text, want) in [
        (
            "one answer with a preview",
            Kind::Answer,
            "\"Which name?\"=\"ccdig\" selected preview:\ncargo install ccdig\n--locked",
            "- (answer) `\"Which name?\"=\"ccdig\"`",
        ),
        (
            "a preview between answers",
            Kind::Answer,
            "\"A?\"=\"a\" selected preview:\nx, y = 1\nz, \"B?\"=\"b\", \"C?\"=\"c\" selected preview:\nw",
            "- (answer) `\"A?\"=\"a\", \"B?\"=\"b\", \"C?\"=\"c\"`",
        ),
        (
            "an answer without a preview",
            Kind::Answer,
            "\"A?\"=\"a\", \"B?\"=\"b\"",
            "- (answer) `\"A?\"=\"a\", \"B?\"=\"b\"`",
        ),
        (
            "a preview holding a quoted equals sign",
            Kind::Answer,
            "\"A?\"=\"a\" selected preview:\nsep=\"=\"\nfoo, \"B?\"=\"b\"",
            "- (answer) `\"A?\"=\"a\", \"B?\"=\"b\"`",
        ),
        (
            "a next question holding a comma and a quote",
            Kind::Answer,
            "\"A?\"=\"a\" selected preview:\nx, \"Use A, \"B\" or C?\"=\"b\"",
            "- (answer) `\"A?\"=\"a\", \"Use A, \"B\" or C?\"=\"b\"`",
        ),
        (
            "the notes after a preview stay",
            Kind::Answer,
            "\"A?\"=\"a\" selected preview:\ncode user notes: please also do X",
            "- (answer) `\"A?\"=\"a\" user notes: please also do X`",
        ),
        (
            "a typed prompt keeps the words",
            Kind::Text,
            "\"A?\"=\"a\" selected preview:\nx",
            "- `\"A?\"=\"a\" selected preview: x`",
        ),
    ] {
        check_eq(&digest_line(kind, text)?, &want.to_owned(), name)?;
    }
    Ok(())
}

#[test]
fn the_new_kinds_as_json_lines_and_markdown() -> TestResult {
    let reply = Reply {
        usage: None,
        masked: Counts::default(),
        head: blog_head("2026-10-13T01:24:00Z")?,
        text: "Drafted the report.".to_owned(),
        model: "claude-opus-5-5".to_owned(),
        skill: "weekly-report".to_owned(),
        subagent: String::new(),
        spawned: Spawned::default(),
        redacted: false,
        source: source(BLOG_FILE, 5, "a-1"),
        search: Search::default(),
    };
    let call = ToolCall {
        model: String::new(),
        usage: None,
        masked: Counts::default(),
        head: blog_head("2026-10-13T01:25:00Z")?,
        tool: "Bash".to_owned(),
        input: json!({"command": "git log --oneline", "description": "List commits"}),
        tool_use_id: "toolu_1".to_owned(),
        touched: Touch::Nothing,
        result: Some(Outcome {
            is_error: true,
            denial: String::new(),
            text: "fatal: not a git repository".to_owned(),
        }),
        skill: "weekly-report".to_owned(),
        subagent: "agent-1".to_owned(),
        spawned: Spawned::default(),
        redacted: false,
        source: source(
            "/home/alice/.claude/projects/-Users-alice-blog/1111/subagents/agent-1.jsonl",
            2,
            "a-2",
        ),
        search: Search::default(),
    };
    let result = ToolResult {
        masked: Counts::default(),
        head: blog_head("2026-10-13T01:25:01Z")?,
        tool: "Bash".to_owned(),
        input: json!({"command": "git log --oneline", "description": "List commits"}),
        tool_use_id: "toolu_1".to_owned(),
        touched: Touch::Nothing,
        is_error: true,
        denial: String::new(),
        text: "fatal: not a git repository".to_owned(),
        subagent: "agent-1".to_owned(),
        spawned: Spawned::default(),
        redacted: false,
        source: source(
            "/home/alice/.claude/projects/-Users-alice-blog/1111/subagents/agent-1.jsonl",
            3,
            "u-3",
        ),
        search: Search::default(),
        skill: "weekly-report".to_owned(),
    };
    let orphan = ToolResult {
        masked: Counts::default(),
        tool: String::new(),
        input: serde_json::Value::Null,
        tool_use_id: "toolu_9".to_owned(),
        touched: Touch::Nothing,
        is_error: false,
        text: "ok".to_owned(),
        subagent: String::new(),
        source: source(BLOG_FILE, 7, "u-4"),
        search: Search::default(),
        ..result.clone()
    };
    let zone = tokyo()?;
    let replies = [Turn::Reply(reply)];
    snapshot("kinds/replies.jsonl", &lines(&replies, &Fields::default())?)?;
    snapshot(
        "kinds/replies.md",
        &markdown::records(&replies, &zone, &Fields::default(), Lang::En)?,
    )?;
    let calls = [Turn::Call(call)];
    snapshot("kinds/tools.jsonl", &lines(&calls, &Fields::default())?)?;
    snapshot(
        "kinds/tools.md",
        &markdown::records(&calls, &zone, &Fields::default(), Lang::En)?,
    )?;
    let results = [Turn::Result(result), Turn::Result(orphan)];
    snapshot("kinds/results.jsonl", &lines(&results, &Fields::default())?)?;
    snapshot(
        "kinds/results.md",
        &markdown::records(&results, &zone, &Fields::default(), Lang::En)?,
    )
}

/// A record the search matched, listing `lines` as `(field, line, text, hit)`.
fn found(lines: &[(&str, usize, &str, bool)]) -> Search {
    Search {
        matched: true,
        matches: lines
            .iter()
            .map(|&(field, line, text, hit)| MatchLine {
                field: field.to_owned(),
                line,
                text: text.to_owned(),
                hit,
            })
            .collect(),
    }
}

/// A record `--around` added as context.
fn context() -> Search {
    Search {
        matched: false,
        matches: Vec::new(),
    }
}

/// A reply in the blog session at `time`, at line `line` of `file`.
fn reply_at(time: &str, file: &str, line: usize, text: &str) -> Result<Reply, String> {
    Ok(Reply {
        usage: None,
        masked: Counts::default(),
        head: blog_head(time)?,
        text: text.to_owned(),
        model: String::new(),
        skill: String::new(),
        subagent: String::new(),
        spawned: Spawned::default(),
        redacted: false,
        source: source(file, line, ""),
        search: Search::default(),
    })
}

/// A call of `tool` with `input` and ID `id` in the blog file, at line
/// `line`, that came to `result`.
fn call_at(
    line: usize,
    tool: &str,
    input: serde_json::Value,
    id: &str,
    result: Option<Outcome>,
) -> Result<ToolCall, String> {
    Ok(ToolCall {
        model: String::new(),
        usage: None,
        masked: Counts::default(),
        head: Head {
            turn: 2,
            ..blog_head("2026-10-13T01:25:00Z")?
        },
        tool: tool.to_owned(),
        input,
        tool_use_id: id.to_owned(),
        touched: Touch::Nothing,
        result,
        skill: String::new(),
        subagent: String::new(),
        spawned: Spawned::default(),
        redacted: false,
        source: source(BLOG_FILE, line, ""),
        search: Search::default(),
    })
}

/// The result of the call `id` of `tool` in the blog file, at line `line`,
/// as `outcome` says.
fn result_at(line: usize, tool: &str, id: &str, outcome: &Outcome) -> Result<ToolResult, String> {
    Ok(ToolResult {
        masked: Counts::default(),
        head: Head {
            turn: 2,
            ..blog_head("2026-10-13T01:25:01Z")?
        },
        tool: tool.to_owned(),
        input: json!({}),
        tool_use_id: id.to_owned(),
        touched: Touch::Nothing,
        is_error: outcome.is_error,
        denial: outcome.denial.clone(),
        text: outcome.text.clone(),
        subagent: String::new(),
        spawned: Spawned::default(),
        redacted: false,
        source: source(BLOG_FILE, line, ""),
        search: Search::default(),
        skill: String::new(),
    })
}

/// What a call came to.
fn outcome(is_error: bool, denial: &str, text: &str) -> Outcome {
    Outcome {
        is_error,
        denial: denial.to_owned(),
        text: text.to_owned(),
    }
}

/// Records with every part the Markdown of `records` can show: each label,
/// context, matched lines, the inputs a call can have, and the results that
/// show under their call or on their own; then a sub-agent's file whose
/// description is blank.
fn every_part() -> Result<Vec<Turn>, String> {
    let prompt = |kind: Kind, text: &str| -> Result<Turn, String> {
        Ok(Turn::Prompt(Prompt {
            kind,
            head: Head {
                turn: 1,
                ..blog_head("2026-10-13T01:23:00Z")?
            },
            ..bare_prompt(blog_head("2026-10-13T01:23:00Z")?, BLOG_FILE, text)
        }))
    };
    let ran = outcome(false, "", "a.txt\nb.txt");
    let denied = outcome(true, "user-rejected", "The user said no.");
    let failed = outcome(true, "", "exit 1");
    Ok(vec![
        prompt(Kind::Text, "plain")?,
        prompt(Kind::Answer, "\"A?\"=\"a\"")?,
        prompt(Kind::Approval, "the plan")?,
        prompt(Kind::Rejection, "not that")?,
        prompt(Kind::TypedAnswer, "\"B?\"=\"my own\"")?,
        Turn::Thinking(reply_at(
            "2026-10-13T01:24:00Z",
            BLOG_FILE,
            2,
            "Let me think.",
        )?),
        Turn::Reply(Reply {
            search: context(),
            ..reply_at("2026-10-13T01:24:01Z", BLOG_FILE, 3, "Nearby.")?
        }),
        Turn::Reply(Reply {
            search: found(&[
                ("text", 1, "first hit", true),
                ("text", 3, "before", false),
                ("text", 4, "second hit", true),
            ]),
            ..reply_at("2026-10-13T01:24:02Z", BLOG_FILE, 4, "ignored")?
        }),
        // The result shows under its call.
        Turn::Call(call_at(
            5,
            "Bash",
            json!({"command": "ls", "plan": "step 1\nstep 2\n"}),
            "toolu_a",
            Some(ran.clone()),
        )?),
        Turn::Result(result_at(6, "Bash", "toolu_a", &ran)?),
        // The result lists matched lines, so it shows on its own.
        Turn::Call(call_at(
            7,
            "ExitPlanMode",
            json!({"plan": "one\ntwo"}),
            "toolu_b",
            Some(denied.clone()),
        )?),
        Turn::Result(ToolResult {
            search: found(&[("text", 1, "The user said no.", true)]),
            ..result_at(8, "ExitPlanMode", "toolu_b", &denied)?
        }),
        // A search matched the result and not its call.
        Turn::Call(ToolCall {
            search: context(),
            ..call_at(
                9,
                "Bash",
                json!({"command": "false"}),
                "toolu_c",
                Some(failed.clone()),
            )?
        }),
        Turn::Result(result_at(10, "Bash", "toolu_c", &failed)?),
        // Matched lines of the input stand for it.
        Turn::Call(ToolCall {
            search: found(&[
                ("input.command", 1, "git add .", true),
                ("input.command", 2, "git commit", true),
            ]),
            ..call_at(11, "Bash", json!({}), "toolu_d", None)?
        }),
        Turn::Summary(Summary {
            head: blog_head("2026-10-13T01:26:00Z")?,
            text: "Compacted.".to_owned(),
            redacted: false,
            source: source(BLOG_FILE, 12, ""),
            search: Search::default(),
            masked: Counts::default(),
        }),
        Turn::Reply(Reply {
            subagent: "agent-7".to_owned(),
            spawned: Spawned {
                subagent_type: "Explore".to_owned(),
                subagent_description: " \n ".to_owned(),
                parent_tool_use_id: "toolu_a".to_owned(),
            },
            ..reply_at("2026-10-13T01:25:30Z", AGENT_FILE, 1, "Looked.")?
        }),
    ])
}

/// The `--fields` of `include` and `exclude`.
fn fields(include: &[&str], exclude: &[&str]) -> Fields {
    Fields {
        include: include.iter().map(|key| (*key).to_owned()).collect(),
        exclude: exclude.iter().map(|key| (*key).to_owned()).collect(),
    }
}

#[test]
fn record_markdown_shows_each_part_fields_keeps() -> TestResult {
    let turns = every_part()?;
    let zone = tokyo()?;
    for (name, kept) in [
        ("parts", Fields::default()),
        ("parts-no-tool-input", fields(&[], &["tool", "input"])),
        (
            "parts-no-result",
            fields(&[], &["result", "is_error", "denial"]),
        ),
        ("parts-no-text", fields(&[], &["text"])),
        ("parts-no-matches", fields(&[], &["matches"])),
        (
            "parts-no-heading-facts",
            fields(
                &[],
                &[
                    "timestamp",
                    "turn",
                    "project",
                    "session_id",
                    "subagent",
                    "subagent_type",
                    "subagent_description",
                ],
            ),
        ),
        ("parts-only-type", fields(&["type"], &[])),
    ] {
        snapshot(
            &format!("render/{name}.md"),
            &markdown::records(&turns, &zone, &kept, Lang::En)?,
        )?;
    }
    Ok(())
}

#[test]
fn record_markdown_in_japanese() -> TestResult {
    snapshot(
        "render/parts-ja.md",
        &markdown::records(&every_part()?, &tokyo()?, &Fields::default(), Lang::Ja)?,
    )
}

/// The keys of `item` as JSON, in the order it writes them.
fn json_keys<T>(item: &T) -> Result<Vec<String>, Box<dyn Error>>
where
    T: Serialize,
{
    Ok(serde_json::to_value(item)?
        .as_object()
        .ok_or("not an object")?
        .keys()
        .cloned()
        .collect())
}

#[test]
fn the_keys_fields_takes_are_the_keys_the_records_write_in_order() -> TestResult {
    let head = blog_head("2026-10-13T01:00:00Z")?;
    let prompt = bare_prompt(head.clone(), BLOG_FILE, "hi");
    let reply = reply_at("2026-10-13T01:00:00Z", BLOG_FILE, 2, "hello")?;
    let call = call_at(3, "Bash", json!({}), "", None)?;
    let result = result_at(4, "Bash", "", &outcome(false, "", ""))?;
    let summary = Summary {
        head,
        text: String::new(),
        redacted: false,
        source: source(BLOG_FILE, 5, "u-2"),
        search: Search::default(),
        masked: Counts::default(),
    };
    let mut seen: Vec<String> = Vec::new();
    for turn in [
        Turn::Prompt(prompt),
        Turn::Reply(reply),
        Turn::Call(call),
        Turn::Result(result),
        Turn::Summary(summary),
    ] {
        let keys = json_keys(&turn)?;
        let positions: Vec<Option<usize>> = keys
            .iter()
            .map(|key| RECORD_KEYS.iter().position(|known| known == key))
            .collect();
        check_eq(&positions.contains(&None), &false, &format!("{keys:?}"))?;
        check_eq(
            &positions.is_sorted(),
            &true,
            &format!("{keys:?} in the order of RECORD_KEYS"),
        )?;
        for key in keys {
            if !seen.contains(&key) {
                seen.push(key);
            }
        }
    }
    check_eq(&seen.len(), &RECORD_KEYS.len(), "every key is written")?;
    check_eq(
        &json_keys(&sample_session()?)?,
        &SESSION_KEYS.map(str::to_owned).to_vec(),
        "session keys",
    )?;
    Ok(())
}

#[test]
fn the_keys_fields_takes_are_the_keys_stats_writes() -> TestResult {
    let records = prompt_turns(sample_prompts()?);
    let every = [
        By::Session,
        By::Project,
        By::Branch,
        By::Type,
        By::Tool,
        By::Skill,
        By::Subagent,
        By::Model,
        By::Kind,
        By::Denial,
        By::Status,
        By::Day,
        By::Week,
        By::Month,
    ];
    let every_but_sessions: Vec<&str> = STATS_KEYS
        .into_iter()
        .filter(|key| *key != "sessions")
        .collect();
    // With every `--by` key and `--type thinking`, and with neither.
    for (keys, empty, want) in [
        (&every[..], true, every_but_sessions),
        (
            &[][..],
            false,
            vec!["count", "chars", "sessions", "first", "last", "usage"],
        ),
    ] {
        let grouping = Grouping {
            by: keys.to_vec(),
            empty,
            ..Grouping::default()
        };
        let groups = tally(&records, &grouping, &TimeZone::UTC)?;
        let got = json_keys(groups.first().ok_or("no group")?)?;
        check_eq(
            &got,
            &want.iter().map(|key| (*key).to_owned()).collect(),
            &format!("{} keys", keys.len()),
        )?;
        check_eq(
            &columns(keys),
            &want.get(..columns(keys).len()).unwrap_or_default().to_vec(),
            "columns names the keys before count",
        )?;
    }
    Ok(())
}

#[test]
fn markdown_counts_the_records_by_type_with_the_context_apart() -> TestResult {
    let zone = TimeZone::UTC;
    check_eq(
        &markdown::records(&[], &zone, &Fields::default(), Lang::En)?,
        &"# Records\n\n- Records: 0\n".to_owned(),
        "no records",
    )?;
    let head = blog_head("2026-10-13T01:00:00Z")?;
    let summary = Summary {
        head: head.clone(),
        text: "compacted".to_owned(),
        redacted: false,
        source: source(BLOG_FILE, 1, "s-1"),
        search: Search::default(),
        masked: Counts::default(),
    };
    let mut near = bare_prompt(head.clone(), BLOG_FILE, "near");
    near.search.matched = false;
    let turns = [
        Turn::Summary(summary),
        Turn::Prompt(bare_prompt(head.clone(), BLOG_FILE, "one")),
        Turn::Prompt(near),
        Turn::Prompt(bare_prompt(head, BLOG_FILE, "two")),
    ];
    let out = markdown::records(&turns, &zone, &Fields::default(), Lang::En)?;
    check_eq(
        &out.starts_with("# Records\n\n- Records: 3 (prompt 2, summary 1) + 1 context\n\n## "),
        &true,
        &out,
    )?;
    Ok(())
}

#[test]
fn markdown_counts_the_sessions_the_matches_and_the_groups() -> TestResult {
    let zone = tokyo()?;
    let session = Session {
        match_count: Some(3),
        ..sample_session()?
    };
    let plain = markdown::sessions(
        from_ref(&session),
        &zone,
        false,
        &Fields::default(),
        Lang::En,
    )?;
    check_eq(
        &plain.starts_with("# Sessions\n\n- Sessions: 1\n\n| Started |"),
        &true,
        &plain,
    )?;
    let searched = markdown::sessions(
        &[session.clone(), session],
        &zone,
        true,
        &Fields::default(),
        Lang::En,
    )?;
    check_eq(
        &searched.starts_with("# Sessions\n\n- Sessions: 2\n- Matches: 6\n\n| Started |"),
        &true,
        &searched,
    )?;
    let none = markdown::sessions(&[], &zone, true, &Fields::default(), Lang::En)?;
    check_eq(
        &none.starts_with("# Sessions\n\n- Sessions: 0\n- Matches: 0\n\n| Started |"),
        &true,
        &none,
    )?;
    let stats = markdown::stats(&sample_groups()?, &[], &zone, &Fields::default(), Lang::En)?;
    check_eq(
        &stats.starts_with("# Stats\n\n- Groups: 2\n\n| Count |"),
        &true,
        &stats,
    )?;
    Ok(())
}
