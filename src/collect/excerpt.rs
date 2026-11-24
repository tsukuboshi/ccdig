//! The lines of a record that `--grep` matched, with the lines around
//! them, for `--context`.

use alloc::borrow::Cow;

use regex::Regex;

use crate::filter::Filter;
use crate::record::MatchLine;

/// The lines of each of `fields` that a pattern of `grep` without a
/// leading `!` matches on its own, each with up to `context` lines before
/// and after it in the same field, in order and each once. A line ends at
/// `\n`, and a `\r` before it is dropped. Without such a pattern, no line
/// is listed.
pub fn excerpt(
    fields: &[(String, Cow<'_, str>)],
    grep: &Filter<Regex>,
    context: usize,
) -> Vec<MatchLine> {
    let mut listed = Vec::new();
    if grep.include.is_empty() {
        return listed;
    }
    for named in fields {
        let (field, text) = (&named.0, &named.1);
        let lines: Vec<&str> = text
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect();
        let hits: Vec<bool> = lines.iter().map(|line| grep.includes(line)).collect();
        // How many windows each line is in, as a running sum of where the
        // windows start and end, so a long text with many matches and a
        // wide context still takes one pass.
        let mut edges = vec![0_isize; lines.len().saturating_add(1)];
        for (index, _) in hits.iter().enumerate().filter(|pair| *pair.1) {
            let first = index.saturating_sub(context);
            let after = index
                .saturating_add(context)
                .saturating_add(1)
                .min(lines.len());
            if let Some(start) = edges.get_mut(first) {
                *start = start.saturating_add(1);
            }
            if let Some(end) = edges.get_mut(after) {
                *end = end.saturating_sub(1);
            }
        }
        let mut open = 0_isize;
        for (index, (line, hit)) in lines.iter().zip(&hits).enumerate() {
            open = open.saturating_add(edges.get(index).copied().unwrap_or_default());
            if open > 0 {
                listed.push(MatchLine {
                    field: field.clone(),
                    line: index.saturating_add(1),
                    text: (*line).to_owned(),
                    hit: *hit,
                });
            }
        }
    }
    listed
}
