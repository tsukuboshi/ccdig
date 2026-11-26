//! Tests for the writers, against golden files.

mod properties;

use core::error::Error;
use core::slice::from_ref;

use jiff::tz::TimeZone;
use serde::Serialize;
use serde_json::json;

use super::digest::{Limits, digest};
use super::json::{Fields, Run, document, lines};
use super::markdown;
use crate::record::{
    AGENT, Group, Head, Kind, Outcome, Prompt, RECORD_KEYS, Reply, SESSION_KEYS, STATS_KEYS,
    Search, Session, Source, Summary, ToolCall, ToolResult, Turn,
};
use crate::redact::Counts;
use crate::stats::{By, Grouping, columns, tally};
use crate::testing::{TestResult, check_eq, golden, zone};
use crate::transcript::entry::Usage;
use crate::when::Stamp;

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
    })
}

/// A plain prompt with only the fields the digest and grouping look at.
fn bare_prompt(head: Head, file: &str, text: &str) -> Prompt {
    Prompt {
        permission_mode: String::new(),
        superseded: false,
        parent_uuid: String::new(),
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
    golden(
        "render/prompts.jsonl.golden",
        &lines(&prompt_turns(sample_prompts()?), &Fields::default())?,
    )
}

#[test]
fn prompts_as_a_json_document() -> TestResult {
    let prompts = prompt_turns(sample_prompts()?);
    golden(
        "render/prompts.json.golden",
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
    golden(
        "render/empty.json.golden",
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
    golden(
        "render/prompts.md.golden",
        &markdown::records(&prompt_turns(prompts), &tokyo()?)?,
    )
}

#[test]
fn sessions_as_json_lines() -> TestResult {
    golden(
        "render/sessions.jsonl.golden",
        &lines(&[sample_session()?], &Fields::default())?,
    )
}

#[test]
fn sessions_as_a_markdown_table() -> TestResult {
    golden(
        "render/sessions.md.golden",
        &markdown::sessions(&[sample_session()?], &tokyo()?, false)?,
    )
}

#[test]
fn stats_as_json_lines_and_a_markdown_table() -> TestResult {
    let groups = sample_groups()?;
    golden(
        "render/stats.jsonl.golden",
        &lines(&groups, &Fields::default())?,
    )?;
    golden(
        "render/stats.md.golden",
        &markdown::stats(&groups, &columns(&[By::Session]), &tokyo()?)?,
    )
}

#[test]
fn stats_by_a_key_without_a_value_leaves_the_cell_empty_and_counts_sessions() -> TestResult {
    let group = Group {
        keys: serde_json::from_value(json!({"tool": null}))?,
        sessions: Some(2),
        ..sample_groups()?.swap_remove(1)
    };
    golden(
        "render/stats-tool.md.golden",
        &markdown::stats(&[group], &columns(&[By::Tool]), &TimeZone::UTC)?,
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
    )?;
    check_eq(&out.matches("\n## ").count(), &2, "session headings")?;
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
    golden(
        "render/digest.md.golden",
        &digest(&prompts, &tokyo()?, limits)?,
    )
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
        result: Some(Outcome {
            is_error: true,
            denial: String::new(),
            text: "fatal: not a git repository".to_owned(),
        }),
        skill: "weekly-report".to_owned(),
        subagent: "agent-1".to_owned(),
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
        is_error: true,
        denial: String::new(),
        text: "fatal: not a git repository".to_owned(),
        subagent: "agent-1".to_owned(),
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
        is_error: false,
        text: "ok".to_owned(),
        subagent: String::new(),
        source: source(BLOG_FILE, 7, "u-4"),
        search: Search::default(),
        ..result.clone()
    };
    let zone = tokyo()?;
    let replies = [Turn::Reply(reply)];
    golden(
        "kinds/replies.jsonl.golden",
        &lines(&replies, &Fields::default())?,
    )?;
    golden(
        "kinds/replies.md.golden",
        &markdown::records(&replies, &zone)?,
    )?;
    let calls = [Turn::Call(call)];
    golden(
        "kinds/tools.jsonl.golden",
        &lines(&calls, &Fields::default())?,
    )?;
    golden("kinds/tools.md.golden", &markdown::records(&calls, &zone)?)?;
    let results = [Turn::Result(result), Turn::Result(orphan)];
    golden(
        "kinds/results.jsonl.golden",
        &lines(&results, &Fields::default())?,
    )?;
    golden(
        "kinds/results.md.golden",
        &markdown::records(&results, &zone)?,
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
    let reply = Reply {
        head: head.clone(),
        text: "hello".to_owned(),
        model: String::new(),
        skill: String::new(),
        subagent: String::new(),
        usage: None,
        redacted: false,
        source: source(BLOG_FILE, 2, "a-1"),
        search: Search::default(),
        masked: Counts::default(),
    };
    let call = ToolCall {
        model: String::new(),
        head: head.clone(),
        tool: "Bash".to_owned(),
        input: json!({}),
        tool_use_id: String::new(),
        result: None,
        skill: String::new(),
        subagent: String::new(),
        usage: None,
        redacted: false,
        source: source(BLOG_FILE, 3, "a-2"),
        search: Search::default(),
        masked: Counts::default(),
    };
    let result = ToolResult {
        head: head.clone(),
        tool: "Bash".to_owned(),
        input: json!({}),
        tool_use_id: String::new(),
        is_error: false,
        denial: String::new(),
        text: String::new(),
        subagent: String::new(),
        redacted: false,
        source: source(BLOG_FILE, 4, "u-1"),
        search: Search::default(),
        skill: String::new(),
        masked: Counts::default(),
    };
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
        By::Day,
        By::Week,
        By::Month,
    ];
    let every_but_sessions: Vec<&str> = STATS_KEYS
        .into_iter()
        .filter(|key| *key != "sessions")
        .collect();
    for (keys, want) in [
        (&every[..], every_but_sessions),
        (
            &[][..],
            vec!["count", "chars", "sessions", "first", "last", "usage"],
        ),
    ] {
        let grouping = Grouping {
            by: keys.to_vec(),
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
        &markdown::records(&[], &zone)?,
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
    let out = markdown::records(&turns, &zone)?;
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
    let plain = markdown::sessions(from_ref(&session), &zone, false)?;
    check_eq(
        &plain.starts_with("# Sessions\n\n- Sessions: 1\n\n| Started |"),
        &true,
        &plain,
    )?;
    let searched = markdown::sessions(&[session.clone(), session], &zone, true)?;
    check_eq(
        &searched.starts_with("# Sessions\n\n- Sessions: 2\n- Matches: 6\n\n| Started |"),
        &true,
        &searched,
    )?;
    let none = markdown::sessions(&[], &zone, true)?;
    check_eq(
        &none.starts_with("# Sessions\n\n- Sessions: 0\n- Matches: 0\n\n| Started |"),
        &true,
        &none,
    )?;
    let stats = markdown::stats(&sample_groups()?, &[], &zone)?;
    check_eq(
        &stats.starts_with("# Stats\n\n- Groups: 2\n\n| Count |"),
        &true,
        &stats,
    )?;
    Ok(())
}
