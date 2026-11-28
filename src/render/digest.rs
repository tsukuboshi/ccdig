//! The digest: compact Markdown of the prompts, meant as LLM input.

use alloc::borrow::Cow;

use jiff::tz::TimeZone;

use crate::failure::Failure;
use crate::record::{Head, Kind, Prompt};
use crate::text::{code_span, cut_short, escape_inline, heading_text, one_line};

/// Where Claude Code starts the tag it puts around pasted text, up to the
/// paste's id.
const PASTE_OPEN: &str = "<pasted_content id=\"";

/// Where the tag that ends a paste starts, up to the paste's id.
const PASTE_CLOSE: &str = "</pasted_content id=\"";

/// What Claude Code writes after an answer, from its closing quote, before
/// the preview of the option the user chose.
const PREVIEW: &str = "\" selected preview:\n";

/// What Claude Code writes between one answer and the next question.
const NEXT_ANSWER: &str = ", \"";

/// What Claude Code writes before the notes the user added to an answer.
const NOTES: &str = " user notes: ";

/// The size limits of a digest. Lengths count characters, not bytes, so
/// multibyte text is cut on character boundaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Each prompt is cut to this many characters, the cut marked with an
    /// ellipsis.
    pub prompt_chars: usize,
    /// At most this many characters are listed per session; the remaining
    /// prompts are summarized as a count.
    pub session_chars: usize,
}

/// Limits that fit a weekly digest.
impl Default for Limits {
    fn default() -> Self {
        Self {
            prompt_chars: 300,
            session_chars: 2000,
        }
    }
}

/// One section per transcript file, one line per prompt, cut to `limits`,
/// each followed by the first line of the reply before it when it has one.
/// Prompts of a file must be adjacent.
///
/// # Errors
///
/// When a date cannot be shown in `zone`.
pub fn digest(prompts: &[Prompt], zone: &TimeZone, limits: Limits) -> Result<String, Failure> {
    let mut out = String::new();
    for (index, group) in prompts
        .chunk_by(|left, right| left.source.file == right.source.file)
        .enumerate()
    {
        if index > 0 {
            out.push('\n');
        }
        session(&mut out, group, zone, limits)?;
    }
    Ok(out)
}

/// Names the project by its cwd with the home directory shown as `~`.
/// Decoding the directory name instead is lossy: Claude Code writes both
/// `/` and `.` as `-`, so `/Users/a.b/x` and `/Users/a/b/x` look the same.
fn project_label(head: &Head) -> String {
    if head.cwd.is_empty() {
        return head.project.clone();
    }
    for base in ["/Users/", "/home/"] {
        if let Some(rest) = head.cwd.strip_prefix(base) {
            let (user, tail) = rest
                .find('/')
                .and_then(|at| rest.split_at_checked(at))
                .unwrap_or((rest, ""));
            if !user.is_empty() {
                return format!("~{tail}");
            }
        }
    }
    head.cwd.clone()
}

/// `text` with each paste and its tags in place of a count of what was
/// pasted, so that the words the user typed fit in the digest. A paste
/// without a closing tag of its id stays as it is.
fn fold_pastes(text: &str) -> Cow<'_, str> {
    if !text.contains(PASTE_OPEN) {
        return Cow::Borrowed(text);
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some((before, tag)) = rest.split_once(PASTE_OPEN) {
        out.push_str(before);
        let paste = tag.split_once("\">").and_then(|(id, body)| {
            body.match_indices(PASTE_CLOSE).find_map(|(at, _)| {
                let after = body
                    .get(at.saturating_add(PASTE_CLOSE.len())..)?
                    .strip_prefix(id)?
                    .strip_prefix("\">")?;
                Some((body.get(..at)?, after))
            })
        });
        if let Some((pasted, after)) = paste {
            out.extend([
                "[pasted ",
                &pasted.trim().chars().count().to_string(),
                " chars]",
            ]);
            rest = after;
        } else {
            out.push_str(PASTE_OPEN);
            rest = tag;
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// The answers in `text` without the previews of the options chosen,
/// which can be long and tell less than the answer does.
fn without_previews(text: &str) -> Cow<'_, str> {
    if !text.contains(PREVIEW) {
        return Cow::Borrowed(text);
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some((before, preview)) = rest.split_once(PREVIEW) {
        out.extend([before, "\""]);
        rest = preview.get(preview_end(preview)..).unwrap_or_default();
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// Where the preview at the start of `preview` ends. Nothing marks it, so
/// it is taken to end at the notes the user wrote, or at the first `, "`
/// that starts the next answer: a question on one line up to `"="`. A
/// preview runs over lines, and a question does not.
fn preview_end(preview: &str) -> usize {
    let next = preview.match_indices(NEXT_ANSWER).find_map(|(at, _)| {
        let after = preview.get(at.saturating_add(NEXT_ANSWER.len())..)?;
        let question = after.get(..after.find("\"=\"")?)?;
        (!question.contains('\n')).then_some(at)
    });
    [next, preview.find(NOTES)]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(preview.len())
}

/// Writes the section of one transcript file.
fn session(
    out: &mut String,
    prompts: &[Prompt],
    zone: &TimeZone,
    limits: Limits,
) -> Result<(), Failure> {
    let Some(first) = prompts.first() else {
        return Ok(());
    };
    out.extend([
        "## ",
        &first.head.timestamp.date_label(zone)?,
        " | ",
        &escape_inline(&one_line(&project_label(&first.head))),
        " | ",
        &heading_text(&first.head.session_title),
        "\n",
    ]);
    let mut used = 0_usize;
    for (index, prompt) in prompts.iter().enumerate() {
        let body = match prompt.kind {
            Kind::Answer | Kind::TypedAnswer => without_previews(&prompt.text),
            Kind::Approval | Kind::Rejection | Kind::Slash | Kind::Text => {
                Cow::Borrowed(prompt.text.as_str())
            }
        };
        let text = cut_short(&one_line(&fold_pastes(&body)), limits.prompt_chars);
        let after = cut_short(&one_line(&prompt.after), limits.prompt_chars);
        let length = text.chars().count().saturating_add(after.chars().count());
        if used.saturating_add(length) > limits.session_chars {
            out.extend([
                "- (",
                &prompts.len().saturating_sub(index).to_string(),
                " more prompts omitted)\n",
            ]);
            return Ok(());
        }
        // A code span keeps the prompt literal when the digest is
        // previewed: no rendered HTML, no ![](url) fetched from a
        // transcript.
        let mark = match prompt.kind {
            Kind::Answer => "(answer) ",
            Kind::Approval => "(approval) ",
            Kind::Rejection => "(rejection) ",
            Kind::TypedAnswer => "(typed answer) ",
            Kind::Slash | Kind::Text => "",
        };
        out.extend(["- ", mark, &code_span(&text), "\n"]);
        if !after.is_empty() {
            out.extend(["  - after: ", &code_span(&after), "\n"]);
        }
        used = used.saturating_add(length);
    }
    Ok(())
}
