//! The page as it is written, built so that no text from a transcript
//! reaches it unescaped. Text goes in only through `Page::text`, which
//! escapes it, or as `Prose`, which only `prose::render` makes; markup
//! goes in only as a string written in this crate.

use super::prose::Prose;
use crate::text::{clean_text, escape_html};

/// The page being written.
pub struct Page(String);

impl Page {
    /// The page as written.
    pub fn finish(self) -> String {
        self.0
    }

    /// An empty page.
    pub const fn new() -> Self {
        Self(String::new())
    }

    /// Rendered Markdown.
    pub fn prose(&mut self, prose: &Prose) {
        self.0.push_str(prose.as_str());
    }

    /// Markup of ccdig's own. Only a string written in the source lives
    /// for `'static` here, so no text from a transcript can come in as
    /// markup.
    pub fn tag(&mut self, markup: &'static str) {
        self.0.push_str(markup);
    }

    /// `text` as it reads: cleaned of the characters that drive a terminal
    /// or reorder the text, and escaped, so it may go in an element or in a
    /// quoted attribute value.
    pub fn text(&mut self, text: &str) {
        self.0.push_str(&escape_html(&clean_text(text)));
    }
}
