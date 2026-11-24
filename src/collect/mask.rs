//! Masking, in the two steps the filters sit between. A picker masks the
//! text, which `--grep` searches; the record then waits as a `Draft` while
//! the filters see its real metadata, which `--project` matches; and only
//! `Mask::seal` gives it out, with the metadata masked too.

use serde_json::Value;

use super::Select;
use crate::failure::Failure;
use crate::record::MatchLine;
use crate::redact::{Counts, ExtraMasks, Redactor};
use crate::transcript::entry::Usage;

/// A record whose text is masked and whose metadata is not yet. The record
/// cannot be taken out but through `Mask::seal`.
pub struct Draft<T>(T);

/// The masking applied to what is printed: the built-in rules and the masks
/// `--mask` and `--mask-values` ask for, or none under `--no-mask`.
pub struct Mask(Option<Redactor>);

/// The masked title of a record's transcript, and what masking it counted.
pub struct Title<'title> {
    /// What masking the title counted, or `None` for a sub-agent's
    /// transcript, whose title may give way to its session's, so
    /// `inherit_title` counts it.
    pub masked: Option<&'title Counts>,
    /// The masked title, or empty.
    pub text: &'title str,
}

impl<T> Draft<T>
where
    T: Select,
{
    /// The draft of what `wrap` makes of the record, as when a picker puts
    /// a kind's record in a `Turn`. `wrap` only wraps it; it must not keep
    /// the record.
    pub fn map<U, F>(self, wrap: F) -> Draft<U>
    where
        U: Select,
        F: FnOnce(T) -> U,
    {
        Draft(wrap(self.0))
    }

    /// Marks the record as one `--around` keeps as context.
    pub fn mark_context(&mut self) {
        *self.0.matched_mut() = false;
    }

    /// Where the record lists the lines `--grep` matched. They come from
    /// the text, which is already masked.
    pub fn matches_mut(&mut self) -> &mut Vec<MatchLine> {
        self.0.matches_mut()
    }

    /// `record`, whose text a picker masked.
    pub const fn new(record: T) -> Self {
        Self(record)
    }

    /// The record as the filters see it.
    pub const fn record(&self) -> &T {
        &self.0
    }

    /// Where the record carries the tokens of its response, `None` for a
    /// kind that does not.
    pub fn usage_mut(&mut self) -> Option<&mut Option<Usage>> {
        self.0.usage_mut()
    }
}

impl Mask {
    /// Masks every string in `value`, reporting whether anything changed
    /// and counting what it masked in `counts`.
    pub fn json(&self, value: &mut Value, counts: &mut Counts) -> bool {
        self.0
            .as_ref()
            .is_some_and(|rules| rules.redact_json_counted(value, counts))
    }

    /// Applies the masks `--mask` and `--mask-values` ask for to `field`, a
    /// path, a branch, a tool's name, or another field that is not text,
    /// counting what it masked in `counts`.
    pub fn meta(&self, field: &str, counts: &mut Counts) -> String {
        self.0.as_ref().map_or_else(
            || field.to_owned(),
            |rules| rules.meta_counted(field, counts),
        )
    }

    /// The built-in rules and the masks in `extra`, or none when `off`.
    ///
    /// # Errors
    ///
    /// When the rules do not compile, which is a bug.
    pub fn new(off: bool, extra: ExtraMasks) -> Result<Self, Failure> {
        if off {
            return Ok(Self(None));
        }
        Redactor::with(extra)
            .map(|rules| Self(Some(rules)))
            .map_err(Failure::Rules)
    }

    /// The record in `draft`, with `title` as its session title and its
    /// metadata masked. This runs after the filters, so `--project` still
    /// matches the real paths.
    pub fn seal<T>(&self, draft: Draft<T>, title: &Title<'_>) -> T
    where
        T: Select,
    {
        let mut record = draft.0;
        let head = record.head_mut();
        title.text.clone_into(&mut head.session_title);
        if let Some(counts) = title.masked {
            head.masked.merge(counts);
        }
        head.cwd = self.meta(&head.cwd, &mut head.masked);
        head.git_branch = self.meta(&head.git_branch, &mut head.masked);
        head.project = self.meta(&head.project, &mut head.masked);
        let mut labels = Counts::default();
        for label in record.labels_mut() {
            *label = self.meta(label, &mut labels);
        }
        record.masked_mut().merge(&labels);
        record
    }

    /// Masks `text`, reporting whether anything changed and counting what
    /// it masked in `counts`.
    pub fn text(&self, text: &str, counts: &mut Counts) -> (String, bool) {
        self.0.as_ref().map_or_else(
            || (text.to_owned(), false),
            |rules| rules.redact_counted(text, counts),
        )
    }
}
