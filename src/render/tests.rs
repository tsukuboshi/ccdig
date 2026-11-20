//! Tests for the writers, against golden files.

use core::slice;

use jiff::tz::TimeZone;
use serde_json::json;

use super::digest::{Limits, digest};
use super::json::{document, lines};
use super::markdown;
use super::record::{AGENT, Head, Kind, Prompt, Reply, Session, Source, ToolCall, ToolResult};
use crate::failure::Failure;
use crate::testing::{TestResult, check_eq, golden};
use crate::when::{Stamp, load_zone};

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
        agent: AGENT,
        session_id: BLOG_SESSION.to_owned(),
        session_title: "Weekly report draft".to_owned(),
        project: "-Users-alice-blog".to_owned(),
        cwd: "/Users/alice/blog".to_owned(),
        git_branch: "main".to_owned(),
        timestamp: at(time)?,
    })
}

/// A plain prompt with only the fields the digest and grouping look at.
fn bare_prompt(head: Head, file: &str, text: &str) -> Prompt {
    Prompt {
        head,
        text: text.to_owned(),
        kind: Kind::Text,
        command: String::new(),
        redacted: false,
        source: source(file, 0, ""),
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
            head: blog_head("2026-10-13T01:23:45.678Z")?,
            text: "/weekly-report last week".to_owned(),
            kind: Kind::Slash,
            command: "/weekly-report".to_owned(),
            redacted: false,
            source: source(BLOG_FILE, 4, "u-1"),
        },
        Prompt {
            head: blog_head("2026-10-13T01:30:00Z")?,
            text: "keep <b> tags & \u{65e5}\u{672c}\u{8a9e} as is\nexport AWS_SECRET_ACCESS_KEY=***REDACTED***".to_owned(),
            kind: Kind::Text,
            command: String::new(),
            redacted: true,
            source: source(BLOG_FILE, 9, "u-2"),
        },
    ])
}

/// A sample session.
fn sample_session() -> Result<Session, String> {
    Ok(Session {
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
    })
}

/// Asia/Tokyo, where the Markdown samples show their times.
fn tokyo() -> Result<TimeZone, Failure> {
    load_zone("Asia/Tokyo")
}

#[test]
fn prompts_as_json_lines() -> TestResult {
    golden("render/prompts.jsonl.golden", &lines(&sample_prompts()?)?)
}

#[test]
fn prompts_as_a_json_document() -> TestResult {
    let prompts = sample_prompts()?;
    golden(
        "render/prompts.json.golden",
        &document("strict-v2", prompts.get(..1).ok_or("no prompts")?)?,
    )
}

#[test]
fn an_empty_json_document_has_an_empty_list() -> TestResult {
    golden(
        "render/empty.json.golden",
        &document::<Prompt>("strict-v2", &[])?,
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
        &markdown::prompts(&prompts, &tokyo()?)?,
    )
}

#[test]
fn sessions_as_json_lines() -> TestResult {
    golden(
        "render/sessions.jsonl.golden",
        &lines(&[sample_session()?])?,
    )
}

#[test]
fn sessions_as_a_markdown_table() -> TestResult {
    golden(
        "render/sessions.md.golden",
        &markdown::sessions(&[sample_session()?], &tokyo()?)?,
    )
}

#[test]
fn markdown_groups_by_file_not_by_session_id() -> TestResult {
    let head = blog_head("2026-10-13T01:00:00Z")?;
    let out = markdown::prompts(
        &[
            bare_prompt(head.clone(), "/a.jsonl", "one"),
            bare_prompt(head, "/b.jsonl", "two"),
        ],
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
    let out = lines(&[prompt])?;
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
                session_title: "Weekly report".to_owned(),
                ..blog_head(time)?
            },
            "/a.jsonl",
            text,
        ))
    };
    let other = bare_prompt(
        Head {
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
        head: blog_head("2026-10-13T01:24:00Z")?,
        text: "Drafted the report.".to_owned(),
        model: "claude-opus-5-5".to_owned(),
        skill: "weekly-report".to_owned(),
        subagent: String::new(),
        redacted: false,
        source: source(BLOG_FILE, 5, "a-1"),
    };
    let call = ToolCall {
        head: blog_head("2026-10-13T01:25:00Z")?,
        tool: "Bash".to_owned(),
        input: json!({"command": "git log --oneline", "description": "List commits"}),
        tool_use_id: "toolu_1".to_owned(),
        skill: "weekly-report".to_owned(),
        subagent: "agent-1".to_owned(),
        redacted: false,
        source: source(
            "/home/alice/.claude/projects/-Users-alice-blog/1111/subagents/agent-1.jsonl",
            2,
            "a-2",
        ),
    };
    let result = ToolResult {
        head: blog_head("2026-10-13T01:25:01Z")?,
        tool: "Bash".to_owned(),
        input: json!({"command": "git log --oneline", "description": "List commits"}),
        tool_use_id: "toolu_1".to_owned(),
        is_error: true,
        text: "fatal: not a git repository".to_owned(),
        subagent: "agent-1".to_owned(),
        redacted: false,
        source: source(
            "/home/alice/.claude/projects/-Users-alice-blog/1111/subagents/agent-1.jsonl",
            3,
            "u-3",
        ),
        skill: "weekly-report".to_owned(),
    };
    let orphan = ToolResult {
        tool: String::new(),
        input: serde_json::Value::Null,
        tool_use_id: "toolu_9".to_owned(),
        is_error: false,
        text: "ok".to_owned(),
        subagent: String::new(),
        source: source(BLOG_FILE, 7, "u-4"),
        ..result.clone()
    };
    let zone = tokyo()?;
    golden(
        "kinds/replies.jsonl.golden",
        &lines(slice::from_ref(&reply))?,
    )?;
    golden(
        "kinds/replies.md.golden",
        &markdown::replies(&[reply], &zone)?,
    )?;
    golden("kinds/tools.jsonl.golden", &lines(slice::from_ref(&call))?)?;
    golden(
        "kinds/tools.md.golden",
        &markdown::tool_calls(&[call], &zone)?,
    )?;
    let results = [result, orphan];
    golden("kinds/results.jsonl.golden", &lines(&results)?)?;
    golden(
        "kinds/results.md.golden",
        &markdown::tool_results(&results, &zone)?,
    )
}
