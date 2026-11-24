//! JSON Lines and the JSON envelope.

use serde::Serialize;
use serde_json::Value;

use crate::failure::Failure;

/// The version of the JSON envelope and the record keys.
pub const SCHEMA_VERSION: u32 = 3;

/// The JSON document: the records with the schema and filter versions,
/// the ccdig version, and how the run was asked for.
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
    /// The ccdig version that wrote the document.
    ccdig: &'items str,
    /// The flags that decide what the records hold.
    run: &'items Run,
    /// The records; an empty list is `[]`, not `null`.
    items: &'items [T],
}

/// The flags of a run that decide what its records hold, so that a missing
/// stretch of time can be told apart from a narrower run, and a document
/// says how it was masked. The values masked are left out.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
pub struct Run {
    /// `--since` as given, or `None`.
    pub since: Option<String>,
    /// `--until` as given, or `None`.
    pub until: Option<String>,
    /// `--tz` as given, or `None` for the system zone.
    pub tz: Option<String>,
    /// The names `--mask` turned on, each once, in the order given.
    pub mask: Vec<String>,
    /// The labels of `--mask-values`, without the empty one.
    pub mask_values: Vec<String>,
    /// Whether `--no-mask` turned masking off.
    pub no_mask: bool,
}

/// The keys `--fields` keeps of each record: those it names, or all of
/// them when it names none, less those it names with a leading `!`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Fields {
    /// The keys named with a leading `!`.
    pub exclude: Vec<String>,
    /// The keys named without one.
    pub include: Vec<String>,
}

impl Fields {
    /// Whether `--fields` named no key, so every key is written.
    pub const fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    /// Whether `key` is written.
    fn keeps(&self, key: &str) -> bool {
        (self.include.is_empty() || self.include.iter().any(|name| name == key))
            && !self.exclude.iter().any(|name| name == key)
    }

    /// `item` as JSON with only the keys kept.
    fn pick<T>(&self, item: &T) -> Result<Value, Failure>
    where
        T: Serialize,
    {
        let mut value = serde_json::to_value(item).map_err(Failure::Json)?;
        if let Value::Object(ref mut keys) = value {
            keys.retain(|key, _| self.keeps(key));
        }
        Ok(value)
    }
}

/// What `ccdig version --format json` prints.
#[derive(Serialize)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "the JSON keys follow the declaration order"
)]
struct Build<'text> {
    /// The ccdig version.
    version: &'text str,
    /// `SCHEMA_VERSION`.
    schema: u32,
    /// The name of the rules that select the prompts.
    filter: &'text str,
}

/// One compact JSON object per line, with the keys `fields` keeps.
///
/// # Errors
///
/// When a record cannot be encoded.
pub fn lines<T>(items: &[T], fields: &Fields) -> Result<String, Failure>
where
    T: Serialize,
{
    let mut out = String::new();
    for item in items {
        let json = if fields.is_empty() {
            serde_json::to_string(item)
        } else {
            serde_json::to_string(&fields.pick(item)?)
        }
        .map_err(Failure::Json)?;
        out.push_str(&escape_separators(&json));
        out.push('\n');
    }
    Ok(out)
}

/// `version`, with the schema version and `filter`, as one compact JSON
/// object on a line.
///
/// # Errors
///
/// When the object cannot be encoded.
pub fn version(version: &str, filter: &str) -> Result<String, Failure> {
    lines(
        &[Build {
            version,
            schema: SCHEMA_VERSION,
            filter,
        }],
        &Fields::default(),
    )
}

/// One indented document of `items`, with the keys `fields` keeps,
/// wrapped with the schema version, `filter`, the ccdig `version`, and
/// `run`.
///
/// # Errors
///
/// When a record cannot be encoded.
pub fn document<T>(
    filter: &str,
    version: &str,
    run: &Run,
    items: &[T],
    fields: &Fields,
) -> Result<String, Failure>
where
    T: Serialize,
{
    let json = if fields.is_empty() {
        envelope(filter, version, run, items)
    } else {
        let picked = items
            .iter()
            .map(|item| fields.pick(item))
            .collect::<Result<Vec<_>, _>>()?;
        envelope(filter, version, run, &picked)
    }
    .map_err(Failure::Json)?;
    Ok(format!("{}\n", escape_separators(&json)))
}

/// `items` in the envelope, indented.
fn envelope<T>(filter: &str, version: &str, run: &Run, items: &[T]) -> serde_json::Result<String>
where
    T: Serialize,
{
    serde_json::to_string_pretty(&Envelope {
        schema: SCHEMA_VERSION,
        filter,
        ccdig: version,
        run,
        items,
    })
}

/// Escapes U+2028 and U+2029 so the output can be pasted into JavaScript. They can only occur inside strings.
fn escape_separators(json: &str) -> String {
    json.replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}
