//! Tests for matching records against the command-line filters.

use jiff::Timestamp;
use jiff::tz::TimeZone;

use super::{Facts, Query, compile_grep};
use crate::testing::{TestResult, check_eq};
use crate::when::DayRange;

/// A tool result recorded on 2026-10-13 in the blog project.
fn record() -> Result<Facts<'static>, jiff::Error> {
    Ok(Facts {
        branch: "main",
        cwd: "/Users/alice/blog",
        is_error: false,
        project: "-Users-alice-blog",
        session_id: "1f0e9d8c-session",
        skill: "tdd",
        text: "Build FAILED: 3 tests",
        timestamp: "2026-10-13T01:00:00Z".parse::<Timestamp>()?,
        tool: "Bash",
    })
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
    let list = |items: &[&str]| items.iter().map(|&item| item.to_owned()).collect();
    check_all(
        &record()?,
        vec![
            (
                "project by directory name",
                Query {
                    projects: list(&["alice-blog"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "project by cwd",
                Query {
                    projects: list(&["alice/blog"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "any project filter",
                Query {
                    projects: list(&["shop", "blog"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "another project",
                Query {
                    projects: list(&["shop"]),
                    ..Query::default()
                },
                false,
            ),
            (
                "session prefix",
                Query {
                    sessions: list(&["1f0e"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "session text that is not a prefix",
                Query {
                    sessions: list(&["session"]),
                    ..Query::default()
                },
                false,
            ),
            (
                "exact branch",
                Query {
                    branches: list(&["develop", "main"]),
                    ..Query::default()
                },
                true,
            ),
            (
                "branch prefix is not enough",
                Query {
                    branches: list(&["mai"]),
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
                    skill: Some("tdd".to_owned()),
                    ..Query::default()
                },
                true,
            ),
            (
                "another skill",
                Query {
                    skill: Some("td".to_owned()),
                    ..Query::default()
                },
                false,
            ),
            (
                "tool",
                Query {
                    tools: vec!["Read".to_owned(), "Bash".to_owned()],
                    ..Query::default()
                },
                true,
            ),
            (
                "tool names are case sensitive",
                Query {
                    tools: vec!["bash".to_owned()],
                    ..Query::default()
                },
                false,
            ),
            (
                "errors only, on a success",
                Query {
                    errors: true,
                    ..Query::default()
                },
                false,
            ),
        ],
    )?;
    check_eq(
        &Query {
            errors: true,
            ..Query::default()
        }
        .matches(&Facts {
            is_error: true,
            ..facts
        }),
        &true,
        "errors only, on a failure",
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
                    grep: Some(compile_grep(r"FAILED: \d+", false)?),
                    ..Query::default()
                },
                true,
            ),
            (
                "case differs",
                Query {
                    grep: Some(compile_grep("failed", false)?),
                    ..Query::default()
                },
                false,
            ),
            (
                "case ignored",
                Query {
                    grep: Some(compile_grep("failed", true)?),
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
