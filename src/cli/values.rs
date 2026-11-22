//! The values `--mask-values` masks: a JSON object of labels to lists of
//! values, given as is or with `@` and a file to read it from.

use std::collections::{HashMap, HashSet};
use std::io;

use serde_json::{Map, Value};

use crate::failure::Failure;
use crate::redact::MARKER;

/// What a values object must be, for the error message.
const EXPECTED: &str = "expected an object of labels to lists of values";

/// The values to mask, each with what replaces it, and the labels in the
/// order they were first given.
#[derive(Debug, Default)]
pub struct Values {
    /// The labels, without the empty one.
    pub labels: Vec<String>,
    /// Each value with its replacement and label, a value given twice only
    /// once.
    pub pairs: Vec<(String, String, String)>,
}

/// Reads `specs`, each a JSON object or `@` and a file holding one, which
/// `read_file` reads.
///
/// Under a label, each value is replaced with the label and its number in
/// full-width brackets, as in `［repo1］`; a list in place of a value gives
/// every spelling in it the same number. Under the empty label, a value is
/// replaced with the marker. The numbers go on across `specs`, and a value
/// given again keeps its first replacement.
///
/// # Errors
///
/// When a file cannot be read, or a spec is not JSON of that shape.
pub fn parse<R>(specs: &[String], read_file: R) -> Result<Values, Failure>
where
    R: Fn(&str) -> io::Result<String>,
{
    let mut values = Values::default();
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();
    for spec in specs {
        let (given, text) = read(spec, &read_file)?;
        let invalid = |reason: String| Failure::MaskValues {
            given: given.clone(),
            reason,
        };
        let object: Map<String, Value> = match serde_json::from_str(&text) {
            Ok(Value::Object(object)) => object,
            Ok(_) => return Err(invalid(EXPECTED.to_owned())),
            Err(error) => return Err(invalid(format!("not JSON: {error}"))),
        };
        for (label, list) in object {
            let Value::Array(items) = list else {
                return Err(invalid(EXPECTED.to_owned()));
            };
            if !label.is_empty() && !values.labels.contains(&label) {
                values.labels.push(label.clone());
            }
            for item in items {
                let spellings = spellings(item).ok_or_else(|| invalid(EXPECTED.to_owned()))?;
                let fresh: Vec<String> = spellings
                    .into_iter()
                    .filter(|spelling| !spelling.is_empty() && seen.insert(spelling.clone()))
                    .collect();
                if fresh.is_empty() {
                    continue;
                }
                let replacement = if label.is_empty() {
                    MARKER.to_owned()
                } else {
                    let count = counts.entry(label.clone()).or_default();
                    *count = count.saturating_add(1);
                    format!("\u{FF3B}{label}{count}\u{FF3D}")
                };
                values.pairs.extend(
                    fresh
                        .into_iter()
                        .map(|spelling| (spelling, replacement.clone(), label.clone())),
                );
            }
        }
    }
    Ok(values)
}

/// The text of `spec` and how to name it in an error: the spec itself, or
/// the file after its `@`, read by `read_file`, without a byte order mark.
fn read<R>(spec: &str, read_file: &R) -> Result<(String, String), Failure>
where
    R: Fn(&str) -> io::Result<String>,
{
    let Some(path) = spec.strip_prefix('@') else {
        // The JSON itself is not quoted: it holds the values to hide.
        return Ok(("--mask-values".to_owned(), spec.to_owned()));
    };
    let read = read_file(path).map_err(|source| Failure::MaskFile {
        path: path.to_owned(),
        source,
    })?;
    // An editor may start the file with a byte order mark.
    let text = read.strip_prefix('\u{feff}').unwrap_or(&read).to_owned();
    Ok((format!("--mask-values {spec}"), text))
}

/// The spellings of one item of a list: a string, or a list of strings.
fn spellings(item: Value) -> Option<Vec<String>> {
    match item {
        Value::String(text) => Some(vec![text]),
        Value::Array(parts) => parts
            .into_iter()
            .map(|part| match part {
                Value::String(text) => Some(text),
                Value::Null
                | Value::Bool(_)
                | Value::Number(_)
                | Value::Array(_)
                | Value::Object(_) => None,
            })
            .collect(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => None,
    }
}
