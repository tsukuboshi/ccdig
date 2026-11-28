//! Markdown rendered for the page, with what could run or load taken out:
//! raw HTML shows as text, a link that is not to the web shows as its
//! text, and an image as its description. pulldown-cmark escapes text and
//! attribute values, but writes raw HTML as it is and checks no URL, so
//! the events are rewritten before it writes them.

use pulldown_cmark::{
    CodeBlockKind, CowStr, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd, html,
};

use crate::text::clean_text;

/// Markdown rendered with nothing in it that runs or loads. Only `render`
/// makes one.
pub struct Prose(String);

impl Prose {
    /// The markup.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The events of a parser, rewritten so that the HTML written from them
/// runs and loads nothing.
struct Defused<'text> {
    /// The events as parsed.
    events: Parser<'text>,
    /// Whether each link open now was kept, innermost last.
    links: Vec<bool>,
}

impl<'text> Iterator for Defused<'text> {
    type Item = Event<'text>;

    fn next(&mut self) -> Option<Event<'text>> {
        loop {
            let event = self.events.next()?;
            if let Some(kept) = self.rewrite(event) {
                return Some(kept);
            }
        }
    }
}

impl<'text> Defused<'text> {
    /// The end of a tag as it is written, or `None` to leave it out, as its
    /// start was.
    fn end(&mut self, end: TagEnd) -> Option<Event<'text>> {
        match end {
            TagEnd::HtmlBlock => Some(Event::End(TagEnd::CodeBlock)),
            TagEnd::Link => self
                .links
                .pop()
                .unwrap_or_default()
                .then_some(Event::End(end)),
            TagEnd::Image => None,
            TagEnd::Heading(level) => Some(Event::End(TagEnd::Heading(below_a_record(level)))),
            TagEnd::Paragraph
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::MetadataBlock(_) => Some(Event::End(end)),
        }
    }

    /// `event` as it is written, or `None` to leave it out.
    fn rewrite(&mut self, event: Event<'text>) -> Option<Event<'text>> {
        match event {
            // Raw HTML shows as written; the math and the footnotes the
            // options leave off would too.
            // Each text is cleaned again, as a character reference decodes
            // to what the cleaning before the parse took out, such as a bidi
            // override.
            Event::Html(text)
            | Event::InlineHtml(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text)
            | Event::Text(text) => Some(Event::Text(cleaned(&text))),
            Event::Code(text) => Some(Event::Code(cleaned(&text))),
            Event::Start(tag) => self.start(tag),
            Event::End(end) => self.end(end),
            Event::FootnoteReference(name) => Some(Event::Text(CowStr::from(format!("[^{name}]")))),
            Event::TaskListMarker(done) => Some(Event::Text(CowStr::Borrowed(if done {
                "[x] "
            } else {
                "[ ] "
            }))),
            Event::SoftBreak | Event::HardBreak | Event::Rule => Some(event),
        }
    }

    /// The start of `tag` as it is written, or `None` to leave it out.
    #[expect(
        clippy::rest_pattern_accessible_field,
        reason = "an image and the attributes of a heading are dropped whole, so what they hold is not named"
    )]
    fn start(&mut self, tag: Tag<'text>) -> Option<Event<'text>> {
        match tag {
            // Its HTML shows as written, as code.
            Tag::HtmlBlock => Some(Event::Start(Tag::CodeBlock(CodeBlockKind::Indented))),
            // The language goes in a class attribute.
            Tag::CodeBlock(CodeBlockKind::Fenced(info)) => Some(Event::Start(Tag::CodeBlock(
                CodeBlockKind::Fenced(cleaned(&info)),
            ))),
            Tag::CodeBlock(CodeBlockKind::Indented) => {
                Some(Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)))
            }
            Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            } => {
                let kept = link_type != LinkType::Email && is_web(&dest_url);
                self.links.push(kept);
                kept.then(|| {
                    Event::Start(Tag::Link {
                        link_type,
                        dest_url,
                        title: cleaned(&title),
                        id,
                    })
                })
            }
            // Its description shows as text, and nothing is fetched.
            Tag::Image { .. } => None,
            // pulldown-cmark aligns a cell with a style attribute.
            Tag::Table(alignments) => Some(Event::Start(Tag::Table(
                alignments
                    .iter()
                    .map(|_| pulldown_cmark::Alignment::None)
                    .collect(),
            ))),
            Tag::Heading { level, .. } => Some(Event::Start(Tag::Heading {
                level: below_a_record(level),
                id: None,
                classes: Vec::new(),
                attrs: Vec::new(),
            })),
            Tag::Paragraph
            | Tag::BlockQuote(_)
            | Tag::List(_)
            | Tag::Item
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::MetadataBlock(_) => Some(Event::Start(tag)),
        }
    }
}

/// A heading of the text `level` deep, moved below the heading of the
/// record that holds it, which is the third level.
const fn below_a_record(level: HeadingLevel) -> HeadingLevel {
    match level {
        HeadingLevel::H1 => HeadingLevel::H4,
        HeadingLevel::H2 => HeadingLevel::H5,
        HeadingLevel::H3 | HeadingLevel::H4 | HeadingLevel::H5 | HeadingLevel::H6 => {
            HeadingLevel::H6
        }
    }
}

/// `text` cleaned of what drives a terminal or reorders the text.
fn cleaned(text: &str) -> CowStr<'static> {
    CowStr::from(clean_text(text))
}

/// Whether `url` is a web address, which a link may go to.
fn is_web(url: &str) -> bool {
    ["http://", "https://"].iter().any(|scheme| {
        url.get(..scheme.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    })
}

/// `markdown` rendered, with tables and struck-through text, and nothing
/// in it that runs or loads.
pub fn render(markdown: &str) -> Prose {
    let text = clean_text(markdown);
    let events = Defused {
        events: Parser::new_ext(
            &text,
            Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH,
        ),
        links: Vec::new(),
    };
    let mut out = String::new();
    html::push_html(&mut out, events);
    Prose(out)
}
