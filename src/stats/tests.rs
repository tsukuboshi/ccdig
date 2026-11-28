//! Tests for grouping records into the lines of `ccdig stats`.

mod properties;

use core::slice;

use jiff::tz::TimeZone;
use serde_json::{Value, json};

use super::{By, Grouping, Sort, tally};
use crate::record::{
    AGENT, Group, Head, Kind, Prompt, Reply, Search, Source, ToolCall, ToolResult, Touch, Turn,
};
use crate::redact::Counts;
use crate::testing::{TestResult, check_eq, zone};
use crate::transcript::entry::Usage;
use crate::when::Stamp;

/// The shared keys of a record in `session` at `time`.
fn head(session: &str, time: &str) -> Result<Head, String> {
    Ok(Head {
        agent: AGENT,
        session_id: session.to_owned(),
        session_title: format!("title of {session}"),
        session_title_then: String::new(),
        project: format!("project-{session}"),
        cwd: String::new(),
        git_branch: "main".to_owned(),
        timestamp: Stamp::parse_rfc3339(time).ok_or_else(|| format!("not RFC 3339: {time}"))?,
        turn: 0,
        masked: Counts::default(),
    })
}

/// Where a record of `session` is: its own file, or a sub-agent's.
fn source(session: &str, subagent: &str) -> Source {
    let file = if subagent.is_empty() {
        format!("/p/{session}.jsonl")
    } else {
        format!("/p/{session}/subagents/{subagent}.jsonl")
    };
    Source {
        file,
        line: 1,
        uuid: String::new(),
    }
}

/// A plain prompt.
fn prompt(session: &str, time: &str, text: &str) -> Result<Turn, String> {
    Ok(Turn::Prompt(Prompt {
        head: head(session, time)?,
        text: text.to_owned(),
        kind: Kind::Text,
        command: String::new(),
        permission_mode: String::new(),
        superseded: false,
        redacted: false,
        source: source(session, ""),
        search: Search::default(),
        masked: Counts::default(),
        parent_uuid: String::new(),
        after: String::new(),
    }))
}

/// A reply by `model`, carrying `usage`.
fn reply(
    session: &str,
    time: &str,
    text: &str,
    model: &str,
    usage: Option<Usage>,
) -> Result<Turn, String> {
    Ok(Turn::Reply(Reply {
        head: head(session, time)?,
        text: text.to_owned(),
        model: model.to_owned(),
        skill: String::new(),
        subagent: String::new(),
        usage,
        redacted: false,
        source: source(session, ""),
        search: Search::default(),
        masked: Counts::default(),
    }))
}

/// A call to `tool` with `input`, carrying `usage`.
fn call(
    session: &str,
    time: &str,
    tool: &str,
    input: Value,
    usage: Option<Usage>,
) -> Result<Turn, String> {
    Ok(Turn::Call(ToolCall {
        head: head(session, time)?,
        tool: tool.to_owned(),
        input,
        tool_use_id: String::new(),
        touched: Touch::Nothing,
        result: None,
        skill: String::new(),
        subagent: String::new(),
        usage,
        redacted: false,
        source: source(session, ""),
        search: Search::default(),
        model: "opus".to_owned(),
        masked: Counts::default(),
    }))
}

/// The result of a call to `tool`.
fn result(session: &str, time: &str, tool: &str, text: &str) -> Result<Turn, String> {
    Ok(Turn::Result(ToolResult {
        head: head(session, time)?,
        tool: tool.to_owned(),
        input: Value::Null,
        tool_use_id: String::new(),
        touched: Touch::Nothing,
        is_error: false,
        denial: String::new(),
        text: text.to_owned(),
        subagent: String::new(),
        redacted: false,
        source: source(session, ""),
        search: Search::default(),
        skill: String::new(),
        masked: Counts::default(),
    }))
}

/// Tokens with `output` output tokens and `cache_read` read from the cache.
const fn tokens(output: u64, cache_read: u64) -> Usage {
    Usage {
        input: 1,
        output,
        cache_read,
        cache_creation: 0,
    }
}

/// Grouping by `by`, every group kept, the most records first.
fn by(by: &[By]) -> Grouping {
    Grouping {
        by: by.to_vec(),
        min_count: 0,
        sort: Sort::Count,
        limit: None,
    }
}

/// The groups as JSON, to compare whole lines.
fn as_json(groups: &[Group]) -> Result<Value, serde_json::Error> {
    serde_json::to_value(groups)
}

#[test]
fn without_by_every_record_is_one_group() -> TestResult {
    let records = [
        prompt("s1", "2026-11-24T01:00:00Z", "hi")?,
        reply(
            "s1",
            "2026-11-24T01:00:05Z",
            "hello",
            "opus",
            Some(tokens(10, 100)),
        )?,
        call(
            "s2",
            "2026-11-25T02:00:00Z",
            "Bash",
            json!({"command": "ls"}),
            Some(tokens(20, 200)),
        )?,
    ];
    let groups = tally(&records, &by(&[]), &TimeZone::UTC)?;
    check_eq(
        &as_json(&groups)?,
        &json!([{
            "count": 3_u64,
            "chars": 23_u64,
            "sessions": 2_u64,
            "first": "2026-11-24T01:00:00Z",
            "last": "2026-11-25T02:00:00Z",
            "usage": {"input": 2_u64, "output": 30_u64, "cache_read": 300_u64, "cache_creation": 0_u64},
        }]),
        "one total line",
    )?;
    Ok(())
}

#[test]
fn no_records_make_no_groups() -> TestResult {
    check_eq(&tally(&[], &by(&[]), &TimeZone::UTC)?.len(), &0, "no lines")?;
    Ok(())
}

#[test]
fn by_tool_puts_records_without_a_tool_last_and_writes_no_usage() -> TestResult {
    let records = [
        prompt("s1", "2026-11-24T01:00:00Z", "hi")?,
        call(
            "s1",
            "2026-11-24T01:00:01Z",
            "Read",
            json!({}),
            Some(tokens(5, 0)),
        )?,
        call("s1", "2026-11-24T01:00:02Z", "Bash", json!({}), None)?,
        result("s1", "2026-11-24T01:00:03Z", "Bash", "ok")?,
    ];
    let groups = tally(&records, &by(&[By::Tool]), &TimeZone::UTC)?;
    let summary: Vec<(Value, usize, Option<Usage>)> = groups
        .iter()
        .map(|group| {
            (
                group.keys.get("tool").cloned().unwrap_or_default(),
                group.count,
                group.usage,
            )
        })
        .collect();
    check_eq(
        &summary,
        &vec![
            (json!("Bash"), 2, None),
            (json!("Read"), 1, None),
            (Value::Null, 1, None),
        ],
        "most first, then by key, null last",
    )?;
    Ok(())
}

#[test]
fn by_session_adds_what_records_session_needs_and_drops_sessions() -> TestResult {
    let mut subagent_reply = reply("s1", "2026-11-24T01:00:00Z", "from the agent", "", None)?;
    if let Turn::Reply(ref mut inner) = subagent_reply {
        inner.subagent = "agent-a".to_owned();
        inner.source = source("s1", "agent-a");
    }
    let records = [
        subagent_reply,
        prompt("s1", "2026-11-24T01:00:01Z", "hi")?,
        prompt("s2", "2026-11-24T02:00:00Z", "yo")?,
    ];
    let groups = tally(&records, &by(&[By::Session]), &TimeZone::UTC)?;
    let first = groups.first().ok_or("no groups")?;
    check_eq(
        &as_json(slice::from_ref(first))?,
        &json!([{
            "session": "s1",
            "session_title": "title of s1",
            "project": "project-s1",
            "file": "/p/s1.jsonl",
            "count": 2_u64,
            "chars": 16_u64,
            "first": "2026-11-24T01:00:00Z",
            "last": "2026-11-24T01:00:01Z",
            "usage": null,
        }]),
        "the session's own file, not the sub-agent's",
    )?;
    check_eq(&groups.len(), &2, "one line per session")?;
    Ok(())
}

#[test]
fn by_model_counts_the_tokens_a_tool_call_carries_for_its_response() -> TestResult {
    let records = [
        // The reply was left out, so the call carries the response's tokens.
        call(
            "s1",
            "2026-11-24T01:00:00Z",
            "Bash",
            json!({}),
            Some(tokens(7, 70)),
        )?,
        reply(
            "s1",
            "2026-11-24T01:00:05Z",
            "a",
            "haiku",
            Some(tokens(3, 30)),
        )?,
        prompt("s1", "2026-11-24T01:00:06Z", "hi")?,
    ];
    let groups = tally(&records, &by(&[By::Model]), &TimeZone::UTC)?;
    let summary: Vec<(Value, Option<u64>)> = groups
        .iter()
        .map(|group| {
            (
                group.keys.get("model").cloned().unwrap_or_default(),
                group.usage.map(|tokens| tokens.output),
            )
        })
        .collect();
    check_eq(
        &summary,
        &vec![
            (json!("haiku"), Some(3)),
            (json!("opus"), Some(7)),
            (Value::Null, None),
        ],
        "per model, a prompt carrying none",
    )?;
    Ok(())
}

/// Three sessions: `a` with one long reply, `b` with three short ones and
/// the most cache reads, and `c` with two and the most output.
fn three_sessions() -> Result<Vec<Turn>, String> {
    Ok(vec![
        reply(
            "a",
            "2026-11-24T01:00:00Z",
            "a long reply",
            "",
            Some(tokens(1, 1)),
        )?,
        reply("b", "2026-11-24T01:00:00Z", "b", "", Some(tokens(1, 50)))?,
        reply("b", "2026-11-24T01:00:01Z", "b", "", None)?,
        reply("b", "2026-11-24T01:00:02Z", "b", "", None)?,
        reply("c", "2026-11-24T01:00:00Z", "c", "", Some(tokens(9, 0)))?,
        reply("c", "2026-11-24T01:00:01Z", "c", "", None)?,
    ])
}

/// The sessions of the groups, in order.
fn sessions_of(groups: &[Group]) -> Vec<Value> {
    groups
        .iter()
        .map(|group| group.keys.get("session").cloned().unwrap_or_default())
        .collect()
}

#[test]
fn sort_orders_the_most_first_or_by_key() -> TestResult {
    let records = three_sessions()?;
    for (sort, want) in [
        (Sort::Count, ["b", "c", "a"]),
        (Sort::Chars, ["a", "b", "c"]),
        (Sort::Output, ["c", "a", "b"]),
        (Sort::CacheRead, ["b", "a", "c"]),
        (Sort::Key, ["a", "b", "c"]),
    ] {
        let grouping = Grouping {
            sort,
            ..by(&[By::Session])
        };
        check_eq(
            &sessions_of(&tally(&records, &grouping, &TimeZone::UTC)?),
            &want.map(Value::from).to_vec(),
            &format!("{sort:?}"),
        )?;
    }
    Ok(())
}

#[test]
fn min_count_drops_small_groups_and_limit_keeps_the_first_after_sorting() -> TestResult {
    let records = three_sessions()?;
    for (min_count, limit, want) in [
        (2, None, vec!["b", "c"]),
        (0, Some(1), vec!["b"]),
        (3, Some(5), vec!["b"]),
        (4, None, vec![]),
    ] {
        let grouping = Grouping {
            limit,
            min_count,
            ..by(&[By::Session])
        };
        check_eq(
            &sessions_of(&tally(&records, &grouping, &TimeZone::UTC)?),
            &want.into_iter().map(Value::from).collect::<Vec<_>>(),
            &format!("--min-count {min_count} --limit {limit:?}"),
        )?;
    }
    Ok(())
}

#[test]
fn the_time_keys_follow_the_zone_and_combine_with_others() -> TestResult {
    let records = [
        prompt("s1", "2026-11-29T14:00:00Z", "sunday in utc")?,
        prompt("s1", "2026-11-29T16:00:00Z", "monday in tokyo")?,
    ];
    let zone = zone("Asia/Tokyo")?;
    let groups = tally(
        &records,
        &Grouping {
            sort: Sort::Key,
            ..by(&[By::Week, By::Day, By::Month, By::Type])
        },
        &zone,
    )?;
    let keys: Vec<Value> = groups
        .iter()
        .map(|group| Value::Object(group.keys.clone()))
        .collect();
    check_eq(
        &keys,
        &vec![
            json!({"week": "2026-W48", "day": "2026-11-29", "month": "2026-11", "type": "prompt"}),
            json!({"week": "2026-W49", "day": "2026-11-30", "month": "2026-11", "type": "prompt"}),
        ],
        "the keys in the order --by names them",
    )?;
    Ok(())
}

#[test]
fn by_session_writes_an_empty_project_as_null_as_by_project_does() -> TestResult {
    let mut turn = prompt("s1", "2026-11-24T01:00:00Z", "hi")?;
    if let Turn::Prompt(ref mut inner) = turn {
        inner.head.project = String::new();
    }
    for keys in [&[By::Session][..], &[By::Session, By::Project][..]] {
        let groups = tally(slice::from_ref(&turn), &by(keys), &TimeZone::UTC)?;
        check_eq(
            &groups
                .first()
                .and_then(|group| group.keys.get("project"))
                .cloned(),
            &Some(Value::Null),
            &format!("{keys:?}"),
        )?;
    }
    Ok(())
}

#[test]
fn the_type_and_kind_keys_hold_the_names_records_writes() -> TestResult {
    let mut slash = prompt("s1", "2026-11-24T01:00:00Z", "/x")?;
    if let Turn::Prompt(ref mut inner) = slash {
        inner.kind = Kind::Slash;
    }
    let records = [
        slash,
        reply("s1", "2026-11-24T01:00:01Z", "a", "", None)?,
        call("s1", "2026-11-24T01:00:02Z", "Bash", json!({}), None)?,
        result("s1", "2026-11-24T01:00:03Z", "Bash", "")?,
    ];
    for turn in &records {
        let written = serde_json::to_value(turn)?;
        let groups = tally(
            slice::from_ref(turn),
            &by(&[By::Type, By::Kind]),
            &TimeZone::UTC,
        )?;
        let keys = &groups.first().ok_or("no group")?.keys;
        check_eq(&keys.get("type"), &written.get("type"), "type")?;
        let kind = written.get("kind").cloned().unwrap_or(Value::Null);
        check_eq(&keys.get("kind"), &Some(&kind), "kind")?;
    }
    Ok(())
}
