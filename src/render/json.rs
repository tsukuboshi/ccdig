//! JSON Lines and the JSON envelope.

use serde::Serialize;

use crate::failure::Failure;

/// The version of the JSON envelope and the record keys.
pub const SCHEMA_VERSION: u32 = 1;

/// The JSON document: the records with the schema and filter versions.
#[derive(Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
struct Envelope<'items, T> {
    /// `SCHEMA_VERSION`.
    schema: u32,
    /// The name of the rules that selected the prompts.
    filter: &'items str,
    /// The records; an empty list is `[]`, not `null`.
    items: &'items [T],
}

/// One compact JSON object per line.
///
/// # Errors
///
/// When a record cannot be encoded.
pub fn lines<T>(items: &[T]) -> Result<String, Failure>
where
    T: Serialize,
{
    let mut out = String::new();
    for item in items {
        out.push_str(&escape_separators(
            &serde_json::to_string(item).map_err(Failure::Json)?,
        ));
        out.push('\n');
    }
    Ok(out)
}

/// One indented document of `items`, wrapped with the schema version and
/// `filter`.
///
/// # Errors
///
/// When a record cannot be encoded.
pub fn document<T>(filter: &str, items: &[T]) -> Result<String, Failure>
where
    T: Serialize,
{
    let json = serde_json::to_string_pretty(&Envelope {
        schema: SCHEMA_VERSION,
        filter,
        items,
    })
    .map_err(Failure::Json)?;
    Ok(format!("{}\n", escape_separators(&json)))
}

/// Escapes U+2028 and U+2029 so the output can be pasted into JavaScript. They can only occur inside strings.
fn escape_separators(json: &str) -> String {
    json.replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}
