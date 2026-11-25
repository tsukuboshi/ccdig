//! Properties of the command-line filters that hold for any record.

use jiff::Timestamp;
use proptest::arbitrary::any;
use proptest::collection::vec;
use proptest::option;
use proptest::prop_oneof;
use proptest::sample::select;
use proptest::strategy::Strategy;

use crate::filter::{Facts, Filter, Query, Verdict, compile_grep, patterns};
use crate::testing::{TestResult, check_eq, for_all, hostile_text};

/// Names a record and a filter share, so that values match often.
const NAMES: &[&str] = &["main", "dev", "Bash", "Read", "tdd", "!", "\\!x", "1f0e"];

/// Facts made of `NAMES`, with optional skill, tool, and error flag.
#[derive(Clone, Debug)]
struct Record {
    /// Branch, cwd, project, session, and text.
    names: [&'static str; 5],
    /// The skill, the tool, and the error flag.
    optional: (Option<&'static str>, Option<&'static str>, Option<bool>),
}

impl Record {
    /// The facts the filters see.
    const fn facts(&self) -> Facts<'_> {
        Facts {
            branch: self.names[0],
            cwd: self.names[1],
            denial: None,
            is_error: self.optional.2,
            project: self.names[2],
            session_id: self.names[3],
            skill: self.optional.0,
            text: self.names[4],
            timestamp: Timestamp::UNIX_EPOCH,
            tool: self.optional.1,
        }
    }
}

/// Any record made of `NAMES`.
fn record() -> impl Strategy<Value = Record> {
    let name = || select(NAMES);
    (
        [name(), name(), name(), name(), name()],
        (
            option::of(name()),
            option::of(name()),
            option::of(any::<bool>()),
        ),
    )
        .prop_map(|(names, optional)| Record { names, optional })
}

/// The values of a name flag, some with a leading `!` or `\!`.
fn flag_values() -> impl Strategy<Value = Vec<String>> {
    let value = prop_oneof![
        select(NAMES).prop_map(str::to_owned),
        select(NAMES).prop_map(|name| format!("!{name}")),
        hostile_text(),
    ];
    vec(value, 0..3)
}

/// Any query on the name flags and the error flag.
fn query() -> impl Strategy<Value = Query> {
    (
        flag_values(),
        flag_values(),
        flag_values(),
        flag_values(),
        flag_values(),
        option::of(any::<bool>()),
    )
        .prop_map(
            |(branches, projects, sessions, skills, tools, is_error)| Query {
                branches: Filter::names(branches),
                is_error,
                projects: Filter::names(projects),
                sessions: Filter::names(sessions),
                skills: Filter::names(skills),
                tools: Filter::names(tools),
                ..Query::default()
            },
        )
}

#[test]
fn name_values_split_into_include_and_exclude_and_spell_back() -> TestResult {
    for_all(&flag_values(), |values| {
        let filter = Filter::names(values.clone());
        let mut spelled: Vec<String> = filter
            .include
            .iter()
            .map(|name| {
                name.strip_prefix('!')
                    .map_or_else(|| name.clone(), |rest| format!("\\!{rest}"))
            })
            .chain(filter.exclude.iter().map(|name| format!("!{name}")))
            .collect();
        let mut given = values;
        spelled.sort();
        given.sort();
        check_eq(&spelled, &given, "spelled back")?;
        Ok(())
    })
}

#[test]
fn an_empty_query_lets_every_record_through() -> TestResult {
    for_all(&record(), |record| {
        check_eq(
            &(Query::default().verdict(&record.facts()) == Verdict::Hit),
            &true,
            "matches",
        )?;
        Ok(())
    })
}

#[test]
fn excluding_more_never_lets_more_through_and_excluding_the_branch_drops_the_record() -> TestResult
{
    for_all(
        &(query(), record(), select(NAMES)),
        |(query, record, extra)| {
            let facts = record.facts();
            let mut narrower = query.clone();
            narrower.branches.exclude.push(extra.to_owned());
            narrower.tools.exclude.push(extra.to_owned());
            if narrower.verdict(&facts) == Verdict::Hit {
                check_eq(
                    &(query.verdict(&facts) == Verdict::Hit),
                    &true,
                    "the wider query",
                )?;
            }
            let mut without_branch = query;
            without_branch
                .branches
                .exclude
                .push(facts.branch.to_owned());
            check_eq(
                &(without_branch.verdict(&facts) == Verdict::Hit),
                &false,
                "branch excluded",
            )?;
            Ok(())
        },
    )
}

/// The `--grep` and `--igrep` values: escaped names, some with a leading
/// `!`.
fn grep_values() -> impl Strategy<Value = Vec<String>> {
    let value = prop_oneof![
        select(NAMES).prop_map(regex::escape),
        select(NAMES).prop_map(|name| format!("!{}", regex::escape(name))),
    ];
    vec(value, 0..3)
}

#[test]
fn the_verdict_is_the_other_filters_then_the_excluding_and_including_patterns() -> TestResult {
    for_all(
        &(query(), grep_values(), grep_values(), record()),
        |(base, grep, igrep, record)| {
            let query = Query {
                grep: patterns(&grep, &igrep)?,
                ..base
            };
            let facts = record.facts();
            let open = query.admits(&facts) && !query.grep.excludes(facts.text);
            let want = match (open, query.grep.includes(facts.text)) {
                (false, _) => Verdict::Out,
                (true, true) => Verdict::Hit,
                (true, false) => Verdict::Near,
            };
            check_eq(&query.verdict(&facts), &want, "verdict")?;
            Ok(())
        },
    )
}

#[test]
fn any_grep_pattern_compiles_or_fails_without_panicking() -> TestResult {
    for_all(
        &(hostile_text(), any::<bool>()),
        |(pattern, ignore_case)| {
            drop(compile_grep(&pattern, ignore_case));
            Ok(())
        },
    )
}

#[test]
fn an_escaped_pattern_finds_its_text_and_igrep_ignores_ascii_case() -> TestResult {
    for_all(&hostile_text(), |text| {
        let exact = compile_grep(&regex::escape(&text), false)?;
        check_eq(&exact.is_match(&text), &true, "grep")?;
        let lower = compile_grep(&regex::escape(&text.to_ascii_lowercase()), true)?;
        check_eq(&lower.is_match(&text.to_ascii_uppercase()), &true, "igrep")?;
        Ok(())
    })
}
