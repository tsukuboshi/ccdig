//! Tests for matching records against the command-line filters.

mod properties;

use jiff::Timestamp;
use jiff::tz::TimeZone;

use super::{Facts, Filter, Query, Verdict, command_touches, compile_grep, patterns, touches};
use crate::testing::{TestResult, check_eq};
use crate::when::DayRange;

/// A tool result recorded on 2026-10-13 in the blog project.
fn record() -> Result<Facts<'static>, jiff::Error> {
    Ok(Facts {
        branch: "main",
        cwd: "/Users/alice/blog",
        denial: Some(""),
        is_error: Some(false),
        kind: None,
        project: "-Users-alice-blog",
        session_id: "1f0e9d8c-session",
        skill: Some("tdd"),
        text: "Build FAILED: 3 tests",
        timestamp: "2026-10-13T01:00:00Z".parse::<Timestamp>()?,
        tool: Some("Bash"),
        touched: None,
    })
}

/// `items` as the values of a name flag.
fn names(items: &[&str]) -> Filter<String> {
    Filter::names(items.iter().map(|&item| item.to_owned()).collect())
}

/// Checks each query in `cases` against `facts`.
fn check_all(facts: &Facts<'_>, cases: Vec<(&str, Query, bool)>) -> TestResult {
    for (name, query, want) in cases {
        check_eq(&(query.verdict(facts) == Verdict::Hit), &want, name)?;
    }
    Ok(())
}

#[test]
fn an_empty_query_lets_everything_through() -> TestResult {
    check_eq(
        &(Query::default().verdict(&record()?) == Verdict::Hit),
        &true,
        "empty query",
    )?;
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
        &(Query {
            is_error: Some(true),
            skills: names(&["grilling"]),
            tools: names(&["Read"]),
            ..Query::default()
        }
        .verdict(&prompt)
            == Verdict::Hit),
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

#[test]
fn a_path_is_touched_where_its_parts_line_up() -> TestResult {
    for (want, path, touched) in [
        ("src/cli/args.rs", "/home/a/ccdig/src/cli/args.rs", true),
        ("src/cli", "/home/a/ccdig/src/cli/args.rs", true),
        ("src/cli/", "/home/a/ccdig/src/cli/args.rs", true),
        ("./src/cli", "/home/a/ccdig/src/cli/args.rs", true),
        ("ccdig//src", "/home/a/ccdig/src/cli/args.rs", true),
        ("README.md", "README.md", true),
        ("src/main.rs", "C:\\Users\\me\\blog\\src\\main.rs", true),
        // Not across a part's boundary.
        ("src/cli", "/home/a/ccdig/src/client/x.rs", false),
        ("args.rs", "/home/a/ccdig/src/cli/args.rs.bak", false),
        ("cli/args", "/home/a/ccdig/src/cli/args.rs", false),
        // An absolute path matches from the root.
        ("/home/a/ccdig/README.md", "/home/a/ccdig/README.md", true),
        ("/home/a/ccdig", "/home/a/ccdig/README.md", true),
        (
            "/home/a/ccdig/README.md",
            "/tmp/home/a/ccdig/README.md",
            false,
        ),
        // Nothing to match.
        ("", "/home/a/ccdig/README.md", false),
        ("/", "/home/a/ccdig/README.md", false),
        ("README.md", "", false),
    ] {
        check_eq(&touches(want, path), &touched, &format!("{want} in {path}"))?;
    }
    Ok(())
}

#[test]
fn a_command_touches_a_path_one_of_its_words_is() -> TestResult {
    for (want, command, touched) in [
        ("README.md", "mv README.md docs/README.md", true),
        ("docs", "mv README.md docs/README.md", true),
        ("src/cli", "sed -i '' 's/a/b/' src/cli/args.rs", true),
        ("README.md", "echo hi >README.md", true),
        ("README.md", "echo hi >> \"README.md\"", true),
        ("a.rs", "git add a.rs&&git commit -m x", true),
        ("out.txt", "cargo run --out=out.txt", true),
        ("x.rs", "(cd src; rm x.rs)", true),
        ("args.rs", "cat src/cli/args.rs.bak", false),
        ("x.rs", "cargo test", false),
        ("README", "grep README.md", false),
        ("src/a.rs", "cp src/a.rs{,.bak}", true),
        ("docs", "tar czf x.tgz src,docs", true),
        ("README.md", "grep -n x README.md:3", true),
    ] {
        check_eq(
            &command_touches(want, command),
            &touched,
            &format!("{want} in {command}"),
        )?;
    }
    Ok(())
}
