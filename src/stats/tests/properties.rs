//! Properties of grouping that hold for any records and any keys.

use std::collections::HashSet;

use jiff::tz::TimeZone;
use proptest::collection::vec;
use proptest::option;
use proptest::sample::select;
use proptest::strategy::Strategy;
use serde_json::json;

use super::{by, call, prompt, reply, result, tokens};
use crate::record::Turn;
use crate::stats::{By, tally};
use crate::testing::{TestResult, check_eq, for_all, hostile_text};

/// Every key `--by` takes.
const KEYS: &[By] = &[
    By::Branch,
    By::Day,
    By::Kind,
    By::Model,
    By::Month,
    By::Project,
    By::Session,
    By::Skill,
    By::Subagent,
    By::Tool,
    By::Type,
    By::Week,
];

/// One record of a few sessions, tools, and days, as the kind, the
/// session, the tool, the day, the text, and the output tokens it carries.
type Spec = (
    u8,
    &'static str,
    &'static str,
    &'static str,
    String,
    Option<u64>,
);

/// Builds the record `spec` describes.
fn record(spec: &Spec) -> Result<Turn, String> {
    let &(kind, session, tool, day, ref text, output) = spec;
    let time = format!("{day}T12:00:00Z");
    let usage = output.map(|count| tokens(count, 0));
    match kind {
        0 => prompt(session, &time, text),
        1 => reply(session, &time, text, tool, usage),
        2 => call(session, &time, tool, json!({ "text": text }), usage),
        _ => result(session, &time, tool, text),
    }
}

/// Any records, and any keys to group them by.
fn records_and_keys() -> impl Strategy<Value = (Vec<Spec>, Vec<By>)> {
    let spec = (
        0_u8..4,
        select(&["s1", "s2", "s3"][..]),
        select(&["Bash", "Read", ""][..]),
        select(&["2026-11-29", "2026-11-30", "2027-01-01"][..]),
        hostile_text(),
        option::of(0_u64..1_000),
    );
    (vec(spec, 0..24), vec(select(KEYS), 0..3))
}

#[test]
fn the_groups_split_the_records_without_losing_or_repeating_any() -> TestResult {
    for_all(&records_and_keys(), |(specs, keys)| {
        let records = specs.iter().map(record).collect::<Result<Vec<_>, _>>()?;
        let groups = tally(&records, &by(&keys), &TimeZone::UTC)?;
        let count: usize = groups.iter().map(|group| group.count).sum();
        check_eq(&count, &records.len(), "count adds up to the records")?;
        let chars: usize = groups.iter().map(|group| group.chars).sum();
        let whole = tally(&records, &by(&[]), &TimeZone::UTC)?;
        let want = whole.first().map_or(0, |group| group.chars);
        check_eq(&chars, &want, "chars adds up to that of every record")?;
        let distinct: HashSet<String> = groups
            .iter()
            .map(|group| serde_json::to_string(&group.keys))
            .collect::<Result<_, _>>()?;
        check_eq(
            &distinct.len(),
            &groups.len(),
            "each group has its own keys",
        )?;
        if !keys.iter().any(|key| key.splits_responses()) {
            let output: u64 = groups
                .iter()
                .filter_map(|group| group.usage)
                .map(|usage| usage.output)
                .sum();
            let carried: u64 = specs
                .iter()
                .filter(|spec| matches!(spec.0, 1 | 2))
                .filter_map(|spec| spec.5)
                .sum();
            check_eq(&output, &carried, "each response's tokens once")?;
        }
        Ok(())
    })
}
