//! One HTML page for reading the records and passing them on: one section
//! per transcript file, one article per record, with tool calls folded.
//! The page runs no script and loads nothing: its policy allows only the
//! styles written in it, and the text of the records goes in escaped, or
//! as Markdown rendered with what could run or load taken out.

mod markup;
mod prose;

use std::collections::{HashMap, HashSet};

use jiff::tz::TimeZone;

use super::json::Fields;
use super::view::{
    self, Body, CallBody, Ending, Fact, Input, Label, Passage, ResultBody, Run, Section, Tool, View,
};
use super::words::{self, Badge, Item};
use crate::failure::Failure;
use crate::record::Turn;
use crate::text::{Lang, cut_short, one_line};
use crate::when::Stamp;
use markup::Page;

/// The policy of the page: nothing loads and nothing runs, but for the
/// styles written in it. `base-uri` and `form-action` are named, as
/// `default-src` does not cover them.
const POLICY: &str =
    "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'";

/// How many characters of what a call is about its fold shows.
const GIST_CHARS: usize = 80;

/// The styles of the page.
const STYLE: &str = include_str!("html/style.css");

/// How the records were masked, as the top of the page says it.
#[derive(Debug, Eq, PartialEq)]
pub enum Masking {
    /// `--no-mask` turned masking off.
    Off,
    /// The rules that masked something and how many times each did, most
    /// first.
    On(Vec<(String, usize)>),
}

/// Where each record is on the page: its `id`, and by a call's ID, where
/// the call and its result on its own are.
struct Places<'view> {
    /// The call with each ID in each section, by section and item, which
    /// its result on its own links to.
    calls: HashMap<(usize, &'view str), (usize, usize)>,
    /// The sections of the sub-agents each call started, by the call's
    /// section and item, in the order of the sections.
    children: HashMap<(usize, usize), Vec<usize>>,
    /// Whether each section shows in the call that started its sub-agent,
    /// rather than at the top of the page.
    hosted: Vec<bool>,
    /// The `id` of each item, by section.
    ids: Vec<Vec<String>>,
    /// The result on its own of the call with each ID in each section, by
    /// section and item.
    results: HashMap<(usize, &'view str), (usize, usize)>,
}

/// What writing the sections needs: the view, where its records are, and
/// the language and the zone to word them in.
struct Writer<'view> {
    /// The language of the words.
    lang: Lang,
    /// Where each record is.
    places: Places<'view>,
    /// What the records show.
    view: &'view View<'view>,
    /// The zone of the times.
    zone: &'view TimeZone,
}

impl<'view> Places<'view> {
    /// The `id` of the item at `at`.
    fn id(&self, at: (usize, usize)) -> &str {
        self.ids
            .get(at.0)
            .and_then(|items| items.get(at.1))
            .map_or("", String::as_str)
    }

    /// Whether section `at` shows in the call that started its sub-agent.
    fn is_hosted(&self, at: usize) -> bool {
        self.hosted.get(at).copied().unwrap_or_default()
    }

    /// The places of the items of `view`. An item's `id` is its type, its
    /// line's ID, and its block's index, so it stays the same whatever the
    /// filters keep; or, when the line has no ID that can go in a fragment,
    /// or its `id` is taken, `r` and its place on the page. A call and its
    /// result on its own link to each other within their file, as the view
    /// pairs them.
    fn of(view: &View<'view>) -> Self {
        let mut taken = HashSet::new();
        let mut calls = HashMap::new();
        let mut results = HashMap::new();
        // A sub-agent's file is not its call's, so its call is looked up by
        // the ID alone, the first on the page.
        let mut starters = HashMap::new();
        let mut ids = Vec::new();
        let mut place: usize = 0;
        for (section_at, section) in view.sections.iter().enumerate() {
            let mut section_ids = Vec::new();
            for (item_at, item) in section.items.iter().enumerate() {
                place = place.saturating_add(1);
                let origin = item.origin;
                let from_line = is_fragment(origin.uuid)
                    .then(|| format!("{}-{}-{}", origin.kind, origin.uuid, origin.block))
                    .filter(|id| taken.insert(id.clone()));
                section_ids.push(from_line.unwrap_or_else(|| format!("r{place}")));
                if let Some(id) = item.call_id {
                    let found = match item.body {
                        Body::Call(_) => {
                            starters.entry(id).or_insert((section_at, item_at));
                            &mut calls
                        }
                        Body::Result(_) | Body::Text(_) => &mut results,
                    };
                    found
                        .entry((section_at, id))
                        .or_insert((section_at, item_at));
                }
            }
            ids.push(section_ids);
        }
        let hosts = hosts(view, &starters);
        let mut children: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (at, host) in hosts.iter().enumerate() {
            if let Some(call) = *host {
                children.entry(call).or_default().push(at);
            }
        }
        Self {
            calls,
            children,
            hosted: hosts.iter().map(Option::is_some).collect(),
            ids,
            results,
        }
    }
}

impl Writer<'_> {
    /// The link of the item at `at` to itself, which ends its heading.
    fn anchor(&self, page: &mut Page, at: (usize, usize)) {
        page.tag(" <a class=\"anchor\" href=\"#");
        page.text(self.places.id(at));
        page.tag("\">#</a>");
    }

    /// What the fold of a call, of `call_id` in section `section`, holds: its input, or its
    /// matched lines, why it did not run when it did not, and its output in
    /// a fold of its own when it carries it, or else a link to its result
    /// on its own.
    fn call_body(
        &self,
        page: &mut Page,
        call: &CallBody<'_>,
        call_id: Option<&str>,
        section: usize,
    ) {
        match call.input {
            Input::Hidden => {}
            Input::Lines(ref runs) => lines(page, runs, self.lang),
            Input::Shown {
                ref json,
                ref texts,
            } => {
                if let Some(ref indented) = *json {
                    page.tag("<pre class=\"json\">\n");
                    page.text(indented);
                    page.tag("</pre>\n");
                }
                for &(key, value) in texts {
                    page.tag("<p class=\"key\"><code>input.");
                    page.text(key);
                    page.tag("</code></p>\n");
                    preformatted(page, value);
                }
            }
        }
        let Some(Ending::Ran {
            denial,
            failed: _badge,
            output,
        }) = call.ending
        else {
            return;
        };
        if let Some(reason) = denial {
            page.tag("<p>");
            page.text(words::item(self.lang, Item::Denial));
            page.tag(": <code>");
            page.text(reason);
            page.tag("</code></p>\n");
        }
        match output {
            Some(text) => {
                page.tag("<details class=\"output\">\n<summary>");
                page.text(words::item(self.lang, Item::Result));
                page.tag("</summary>\n");
                preformatted(page, text);
                page.tag("</details>\n");
            }
            None => {
                if let Some(&at) = call_id.and_then(|id| self.places.results.get(&(section, id))) {
                    self.link(page, at, words::label(self.lang, Label::Result));
                }
            }
        }
    }

    /// What the heading of `section` says of its file: the facts the view
    /// keeps, the branch, and when the records shown were written.
    fn facts(&self, page: &mut Page, section: &Section<'_>) -> Result<(), Failure> {
        if section.facts.is_empty() && section.branch.is_none() && section.span.is_none() {
            return Ok(());
        }
        page.tag("<dl class=\"facts\">\n");
        for &fact in &section.facts {
            fact_start(page, words::fact(self.lang, fact));
            match fact {
                Fact::Project(text)
                | Fact::Session(text)
                | Fact::Subagent(text)
                | Fact::SubagentType(text) => {
                    page.tag("<code>");
                    page.text(text);
                    page.tag("</code>");
                }
                Fact::Description(text) => page.text(&one_line(text)),
            }
            page.tag("</dd>\n");
        }
        if let Some(branch) = section.branch {
            fact_start(page, words::branch(self.lang));
            page.tag("<code>");
            page.text(branch);
            page.tag("</code></dd>\n");
        }
        if let Some((first, last)) = section.span {
            fact_start(page, words::span(self.lang));
            self.time(page, first)?;
            if last != first {
                page.tag(" \u{2013} ");
                self.time(page, last)?;
            }
            page.tag("</dd>\n");
        }
        page.tag("</dl>\n");
        Ok(())
    }

    /// What the heading of `item` says: its time, its turn, its label,
    /// whether it is context, and its sub-agent.
    fn heading(&self, page: &mut Page, item: &view::Item<'_>) -> Result<(), Failure> {
        if let Some(stamp) = item.stamp {
            self.time(page, stamp)?;
            page.tag(" - ");
        }
        if let Some(turn) = item.turn {
            page.text(&words::turn(self.lang, turn));
            page.tag(" - ");
        }
        page.text(words::label(self.lang, item.label));
        if !item.matched {
            page.tag(" <span class=\"note\">");
            page.text(words::context(self.lang));
            page.tag("</span>");
        }
        if let Some(name) = item.subagent {
            page.tag(" - <code>");
            page.text(name);
            page.tag("</code>");
        }
        Ok(())
    }

    /// The heading of `item`, at `at`, as a heading element.
    fn headline(
        &self,
        page: &mut Page,
        item: &view::Item<'_>,
        at: (usize, usize),
    ) -> Result<(), Failure> {
        page.tag("<h3>");
        self.heading(page, item)?;
        self.anchor(page, at);
        page.tag("</h3>\n");
        Ok(())
    }

    /// The record at `at` in an article named by its `id`, under a heading
    /// with its time, its turn, and its label, faded when `--around` added
    /// it as context. A call is folded under its heading.
    fn item(&self, page: &mut Page, at: (usize, usize)) -> Result<(), Failure> {
        let Some(item) = self
            .view
            .sections
            .get(at.0)
            .and_then(|section| section.items.get(at.1))
        else {
            return Ok(());
        };
        page.tag("<article id=\"");
        page.text(self.places.id(at));
        page.tag("\" class=\"");
        page.tag(class(item.label));
        if !item.matched {
            page.tag(" context");
        }
        page.tag("\">\n");
        match item.body {
            Body::Call(ref call) => {
                page.tag("<details>\n<summary>");
                self.heading(page, item)?;
                fold_head(page, call, self.lang);
                self.anchor(page, at);
                page.tag("</summary>\n");
                self.call_body(page, call, item.call_id, at.0);
                for &child in self.places.children.get(&at).into_iter().flatten() {
                    self.section(page, child)?;
                }
                page.tag("</details>\n");
            }
            // Where the context was compacted, with what the summary says
            // folded, as it retells what came before.
            Body::Text(ref passage) if item.label == Label::Summary => {
                page.tag("<details>\n<summary>");
                self.heading(page, item)?;
                page.tag(" <span class=\"note\">");
                page.text(words::compacted(self.lang));
                page.tag("</span>");
                self.anchor(page, at);
                page.tag("</summary>\n");
                text(page, passage, item.label, self.lang);
                page.tag("</details>\n");
            }
            Body::Text(ref passage) => {
                self.headline(page, item, at)?;
                text(page, passage, item.label, self.lang);
            }
            Body::Result(ref result) => {
                self.headline(page, item, at)?;
                self.result_body(page, result, item.call_id, at.0);
            }
        }
        page.tag("</article>\n");
        Ok(())
    }

    /// A paragraph that links to the item at `at` with `words`.
    fn link(&self, page: &mut Page, at: (usize, usize), words: &str) {
        page.tag("<p class=\"link\"><a href=\"#");
        page.text(self.places.id(at));
        page.tag("\">");
        page.text(words);
        page.tag("</a></p>\n");
    }

    /// A result, of `call_id` in section `section`, under its own heading: a link to its call
    /// when the page shows it, the calling tool, whether it failed, and why
    /// the call did not run, each when the view keeps it, then its output
    /// or its matched lines.
    fn result_body(
        &self,
        page: &mut Page,
        result: &ResultBody<'_>,
        call_id: Option<&str>,
        section: usize,
    ) {
        if let Some(&at) = call_id.and_then(|id| self.places.calls.get(&(section, id))) {
            self.link(page, at, words::label(self.lang, Label::Call));
        }
        result_facts(page, result, self.lang);
        match result.passage {
            Passage::Hidden => {}
            Passage::Lines(ref runs) => lines(page, runs, self.lang),
            Passage::Text(text) => preformatted(page, text),
        }
    }

    /// The records of section `at`, one transcript file, under a heading
    /// that names the session and says what `fields` keeps of it.
    fn section(&self, page: &mut Page, at: usize) -> Result<(), Failure> {
        let Some(section) = self.view.sections.get(at) else {
            return Ok(());
        };
        page.tag(if section.parent.is_some() {
            "<section class=\"subagent\">\n<h2>"
        } else {
            "<section>\n<h2>"
        });
        page.text(&words::plain_title(self.lang, section.title));
        page.tag("</h2>\n");
        self.facts(page, section)?;
        for item_at in 0..section.items.len() {
            self.item(page, (at, item_at))?;
        }
        page.tag("</section>\n");
        Ok(())
    }

    /// `stamp` as a time element, which reads in the zone of the page.
    fn time(&self, page: &mut Page, stamp: Stamp) -> Result<(), Failure> {
        page.tag("<time datetime=\"");
        page.text(&stamp.rfc3339());
        page.tag("\">");
        page.text(&stamp.second_label(self.zone)?);
        page.tag("</time>");
        Ok(())
    }
}

/// Records of any kind as one page, in the words of `lang`, with times in
/// `zone`, the parts `fields` keeps, and the top saying `masking`. A
/// result shows under its call unless the view has it stand alone, as in
/// Markdown.
///
/// # Errors
///
/// When a time cannot be shown in `zone`, or an input cannot be encoded.
pub fn records(
    records: &[Turn],
    zone: &TimeZone,
    fields: &Fields,
    lang: Lang,
    masking: &Masking,
) -> Result<String, Failure> {
    let view = view::records(records, fields)?;
    let mut page = Page::new();
    head(&mut page, lang);
    top(&mut page, &view, lang, masking);
    let writer = Writer {
        lang,
        places: Places::of(&view),
        view: &view,
        zone,
    };
    for at in 0..view.sections.len() {
        if !writer.places.is_hosted(at) {
            writer.section(&mut page, at)?;
        }
    }
    page.tag("</body>\n</html>\n");
    Ok(page.finish())
}

/// The call each section of `view` shows in, by section and item: the one
/// in `calls` that started its sub-agent, when the page has it in another
/// section. A section whose calls lead back to it shows at the top
/// instead, so that every section shows once.
fn hosts(view: &View<'_>, calls: &HashMap<&str, (usize, usize)>) -> Vec<Option<(usize, usize)>> {
    let mut hosts: Vec<Option<(usize, usize)>> = view
        .sections
        .iter()
        .enumerate()
        .map(|(at, section)| {
            section
                .parent
                .and_then(|id| calls.get(id).copied())
                .filter(|call| call.0 != at)
        })
        .collect();
    for start in 0..hosts.len() {
        let mut at = start;
        for _ in 0..hosts.len() {
            let Some(call) = hosts.get(at).copied().flatten() else {
                break;
            };
            at = call.0;
            if at == start {
                if let Some(host) = hosts.get_mut(start) {
                    *host = None;
                }
                break;
            }
        }
    }
    hosts
}

/// Whether `uuid` can go in an `id` and a fragment as it is: it is not
/// empty, and it holds only hex digits and hyphens, as a line's ID does.
fn is_fragment(uuid: &str) -> bool {
    !uuid.is_empty() && uuid.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-')
}

/// The start of the page, up to its body: the policy, the language, the
/// title, and the styles.
fn head(page: &mut Page, lang: Lang) {
    page.tag("<!doctype html>\n<html lang=\"");
    page.tag(match lang {
        Lang::En => "en",
        Lang::Ja => "ja",
    });
    page.tag("\">\n<head>\n<meta charset=\"utf-8\">\n");
    page.tag("<meta http-equiv=\"Content-Security-Policy\" content=\"");
    page.tag(POLICY);
    page.tag("\">\n<meta name=\"referrer\" content=\"no-referrer\">\n");
    page.tag("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    page.tag("<title>");
    page.text(words::records_title(lang));
    page.tag("</title>\n<style>\n");
    page.tag(STYLE);
    page.tag("</style>\n</head>\n<body>\n");
}

/// The top of the page: its title, how many records it shows, and how
/// they were masked.
fn top(page: &mut Page, view: &View<'_>, lang: Lang, masking: &Masking) {
    page.tag("<header>\n<h1>");
    page.text(words::records_title(lang));
    page.tag("</h1>\n<p class=\"count\">");
    page.text(&words::record_count(
        lang,
        view.count.matched,
        &view.count.types,
        view.count.context,
    ));
    page.tag("</p>\n<p class=\"masking\">");
    page.text(&match *masking {
        Masking::Off => words::not_masked(lang).to_owned(),
        Masking::On(ref counts) => words::masked(lang, counts),
    });
    page.tag("</p>\n</header>\n");
}

/// What a result on its own says of itself: the calling tool, whether it
/// failed, and why the call did not run, each when the view keeps it.
fn result_facts(page: &mut Page, result: &ResultBody<'_>, lang: Lang) {
    if result.tool.is_none() && result.failed.is_none() && result.denial.is_none() {
        return;
    }
    page.tag("<dl class=\"facts\">\n");
    if let Some(tool) = result.tool {
        fact_start(page, words::item(lang, Item::Tool));
        page.tag("<code>");
        tool_name(page, tool, lang);
        page.tag("</code></dd>\n");
    }
    if let Some(failed) = result.failed {
        fact_start(page, words::item(lang, Item::Error));
        page.text(words::failed(lang, failed));
        page.tag("</dd>\n");
    }
    if let Some(denial) = result.denial {
        fact_start(page, words::item(lang, Item::Denial));
        page.tag("<code>");
        page.text(denial);
        page.tag("</code></dd>\n");
    }
    page.tag("</dl>\n");
}

/// What the fold of a call says after its heading: the tool, what the call
/// is about, and a badge for how it ended, each when the view keeps it.
fn fold_head(page: &mut Page, call: &CallBody<'_>, lang: Lang) {
    if let Some(tool) = call.tool {
        page.tag(" <code class=\"tool\">");
        tool_name(page, tool, lang);
        page.tag("</code>");
    }
    if let Some(gist) = call.gist {
        page.tag(" <span class=\"gist\">");
        page.text(&cut_short(&one_line(gist), GIST_CHARS));
        page.tag("</span>");
    }
    if let Some(ending) = call.ending {
        let shown = badge(ending);
        page.tag(" <span class=\"badge ");
        page.tag(match shown {
            Badge::Denied => "denied",
            Badge::Error => "error",
            Badge::Missing => "missing",
            Badge::Ok => "ok",
        });
        page.tag("\">");
        page.text(words::badge(lang, shown));
        page.tag("</span>");
    }
}

/// How a call that came to `ending` ended: turned down, failed, without a
/// result, or ran.
const fn badge(ending: Ending<'_>) -> Badge {
    let Ending::Ran {
        denial,
        failed,
        output: _fold,
    } = ending
    else {
        return Badge::Missing;
    };
    if denial.is_some() {
        Badge::Denied
    } else if failed {
        Badge::Error
    } else {
        Badge::Ok
    }
}

/// The start of a fact in a list of them: its name, and the start of what
/// it says.
fn fact_start(page: &mut Page, name: &str) {
    page.tag("<dt>");
    page.text(name);
    page.tag("</dt><dd>");
}

/// The name of `tool`, or the words for one whose call was not found.
fn tool_name(page: &mut Page, tool: Tool<'_>, lang: Lang) {
    match tool {
        Tool::Named(name) => page.text(name),
        Tool::Unknown => page.text(words::unknown_tool(lang)),
    }
}

/// The class of an item labeled `label`, which the styles look at.
const fn class(label: Label) -> &'static str {
    match label {
        Label::Answer => "answer",
        Label::Approval => "approval",
        Label::Call => "call",
        Label::Prompt => "prompt",
        Label::Rejection => "rejection",
        Label::Reply => "reply",
        Label::Result => "result",
        Label::Summary => "summary",
        Label::Thinking => "thinking",
        Label::TypedAnswer => "typed-answer",
    }
}

/// The text of a prompt, a reply, a thinking block, or a summary, or the
/// lines of it a search matched. What the assistant wrote is Markdown and
/// is rendered; what the user typed shows as typed.
fn text(page: &mut Page, passage: &Passage<'_>, label: Label, lang: Lang) {
    match *passage {
        Passage::Hidden => {}
        Passage::Lines(ref runs) => lines(page, runs, lang),
        Passage::Text(body) => match label {
            Label::Reply | Label::Summary | Label::Thinking => {
                page.tag("<div class=\"prose\">\n");
                page.prose(&prose::render(body));
                page.tag("</div>\n");
            }
            Label::Answer
            | Label::Approval
            | Label::Call
            | Label::Prompt
            | Label::Rejection
            | Label::Result
            | Label::TypedAnswer => preformatted(page, body),
        },
    }
}

/// `text` as written, in a block that keeps its lines. A line break
/// follows the tag, as a parser drops the first one after it.
fn preformatted(page: &mut Page, text: &str) {
    page.tag("<pre>\n");
    page.text(text);
    page.tag("</pre>\n");
}

/// `runs`, each named by which lines it holds, and which of them matched
/// when it holds others too, followed by the lines, those a search
/// matched marked.
fn lines(page: &mut Page, runs: &[Run<'_>], lang: Lang) {
    for run in runs {
        page.tag("<p class=\"lines\">");
        page.text(&words::lines(
            lang,
            run.field,
            run.first,
            run.last,
            &run.hits(),
        ));
        page.tag("</p>\n<pre>\n");
        for (index, line) in run.lines.iter().enumerate() {
            if index > 0 {
                page.tag("\n");
            }
            if line.hit {
                page.tag("<mark>");
                page.text(&line.text);
                page.tag("</mark>");
            } else {
                page.text(&line.text);
            }
        }
        page.tag("</pre>\n");
    }
}
