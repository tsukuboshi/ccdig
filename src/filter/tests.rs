//! Tests for matching records against the command-line filters.

mod properties;

use jiff::Timestamp;
use jiff::tz::TimeZone;

use super::{Facts, Filter, Query, compile_grep, patterns};
use crate::testing::{TestResult, check_eq};
use crate::when::DayRange;

/// A tool result recorded on 2026-10-13 in the blog project.
fn record() -> Result<Facts<'static>, jiff::Error> {
    Ok(Facts {
        branch: "main",
        cwd: "/Users/alice/blog",
        is_error: Some(false),
        project: "-Users-alice-blog",
        session_id: "1f0e9d8c-session",
        skill: Some("tdd"),
        text: "Build FAILED: 3 tests",
        timestamp: "2026-10-13T01:00:00Z".parse::<Timestamp>()?,
        tool: Some("Bash"),
    })
}

/// `items` as the values of a name flag.
fn names(items: &[&str]) -> Filter<String> {
    Filter::names(items.iter().map(|&item| item.to_owned()).collect())
}

/// Checks each query in `cases` against `facts`.
fn check_all(facts: &Facts<'_>, cases: Vec<(&str, Query, bool)>) -> TestResult {
    for (name, query, want) in cases {
        check_eq(&query.matches(facts), &want, name)?;
    }
    Ok(())
}

#[test]
fn an_empty_query_lets_everything_through() -> TestResult {
    check_eq(&Query::default().matches(&record()?), &true, "empty query")?;
    Ok(())
}

#[test]
fn place_filters_match_the_project_the_session_and_the_branch() -> TestResult {
    check_all(
        &record()?,
        vec![
            (
                "project by directory name",
                Query {
                    projects: names(&["alice-blog"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "project by cwd",
                Query {
                    projects: names(&["alice/blog"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "any project filter",
                Query {
                    projects: names(&["shop", "blog"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "another project",
                Query {
                    projects: names(&["shop"]),
                    ..Query::default()
                },
                false,
            ),
            (
                "session prefix",
                Query {
                    sessions: names(&["1f0e"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "session text that is not a prefix",
                Query {
                    sessions: names(&["session"]),
                    ..Query::default()
                },
                false,
            ),
            (
                "exact branch",
                Query {
                    branches: names(&["develop", "main"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "branch prefix is not enough",
                Query {
                    branches: names(&["mai"]),
                    ..Query::default()
                },
                false,
            ),
        ],
    )
}

#[test]
fn kind_filters_match_the_skill_the_tool_and_the_error_flag() -> TestResult {
    let facts = record()?;
    check_all(
        &facts,
        vec![
            (
                "skill",
                Query {
                    skills: names(&["tdd"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "any skill",
                Query {
                    skills: names(&["grilling", "tdd"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "another skill",
                Query {
                    skills: names(&["td"]),
                    ..Query::default()
                },
                false,
            ),
            (
                "tool",
                Query {
                    tools: names(&["Read", "Bash"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "tool names are case sensitive",
                Query {
                    tools: names(&["bash"]),
                    ..Query::default()
                },
                false,
            ),
            (
                "errors only, on a success",
                Query {
                    is_error: Some(true),
                    ..Query::default()
                },
                false,
            ),
            (
                "successes only, on a success",
                Query {
                    is_error: Some(false),
                    ..Query::default()
                },
                true,
            ),
        ],
    )?;
    check_all(
        &Facts {
            is_error: Some(true),
            ..facts
        },
        vec![
            (
                "errors only, on a failure",
                Query {
                    is_error: Some(true),
                    ..Query::default()
                },
                true,
            ),
            (
                "successes only, on a failure",
                Query {
                    is_error: Some(false),
                    ..Query::default()
                },
                false,
            ),
        ],
    )?;
    Ok(())
}

#[test]
fn grep_searches_the_text_with_or_without_case() -> TestResult {
    check_all(
        &record()?,
        vec![
            (
                "pattern",
                Query {
                    grep: patterns(&[r"FAILED: \d+"], &[])?,
                    ..Query::default()
                },
                true,
            ),
            (
                "case differs",
                Query {
                    grep: patterns(&["failed"], &[])?,
                    ..Query::default()
                },
                false,
            ),
            (
                "case ignored",
                Query {
                    grep: patterns(&[], &["failed"])?,
                    ..Query::default()
                },
                true,
            ),
        ],
    )
}

#[test]
fn the_day_range_applies_to_the_timestamp() -> TestResult {
    let range = |since, until| DayRange::parse(since, until, &TimeZone::UTC);
    check_all(
        &record()?,
        vec![
            (
                "inside",
                Query {
                    range: range("2026-10-13", "2026-10-13")?,
                    ..Query::default()
                },
                true,
            ),
            (
                "before",
                Query {
                    range: range("2026-10-14", "")?,
                    ..Query::default()
                },
                false,
            ),
        ],
    )
}

#[test]
fn an_invalid_pattern_is_a_grep_failure() -> TestResult {
    let message = compile_grep("(", false)
        .err()
        .map(|failure| failure.to_string());
    check_eq(
        &message.is_some_and(|text| text.starts_with("parse grep: ")),
        &true,
        "message",
    )?;
    Ok(())
}

#[test]
fn kind_filters_let_a_record_without_the_fact_through() -> TestResult {
    let prompt = Facts {
        is_error: None,
        skill: None,
        tool: None,
        ..record()?
    };
    check_eq(
        &Query {
            is_error: Some(true),
            skills: names(&["grilling"]),
            tools: names(&["Read"]),
            ..Query::default()
        }
        .matches(&prompt),
        &true,
        "a prompt",
    )?;
    Ok(())
}

#[test]
fn a_leading_bang_excludes_and_a_backslash_keeps_it() -> TestResult {
    let facts = record()?;
    check_all(
        &facts,
        vec![
            (
                "session excluded",
                Query {
                    sessions: names(&["!1f0e"]),
                    ..Query::default()
                },
                false,
            ),
            (
                "another session excluded",
                Query {
                    sessions: names(&["!abcd"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "kept, then excluded",
                Query {
                    sessions: names(&["1f0e", "!1f0e9"]),
                    ..Query::default()
                },
                false,
            ),
            (
                "project excluded by its cwd",
                Query {
                    projects: names(&["!alice/blog"]),
                    ..Query::default()
                },
                false,
            ),
            (
                "branch excluded",
                Query {
                    branches: names(&["!main"]),
                    ..Query::default()
                },
                false,
            ),
            (
                "tool excluded",
                Query {
                    tools: names(&["!Bash"]),
                    ..Query::default()
                },
                false,
            ),
            (
                "skill excluded",
                Query {
                    skills: names(&["!tdd"]),
                    ..Query::default()
                },
                false,
            ),
            (
                "text excluded",
                Query {
                    grep: patterns(&["!FAILED"], &[])?,
                    ..Query::default()
                },
                false,
            ),
            (
                "text excluded, ignoring case",
                Query {
                    grep: patterns(&[], &["!failed"])?,
                    ..Query::default()
                },
                false,
            ),
            (
                "text kept, other text excluded",
                Query {
                    grep: patterns(&["FAILED", "!passed"], &[])?,
                    ..Query::default()
                },
                true,
            ),
        ],
    )
}

#[test]
fn a_leading_backslash_bang_matches_a_bang() -> TestResult {
    check_all(
        &Facts {
            branch: "!odd",
            text: "!important",
            tool: None,
            ..record()?
        },
        vec![
            (
                "a name that starts with a bang",
                Query {
                    branches: names(&[r"\!odd"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "a pattern that starts with a bang",
                Query {
                    grep: patterns(&[r"\!imp"], &[])?,
                    ..Query::default()
                },
                true,
            ),
            (
                "a kind without a tool",
                Query {
                    tools: names(&["!Bash"]),
                    ..Query::default()
                },
                true,
            ),
        ],
    )
}
