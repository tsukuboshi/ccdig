//! Sanitizers for transcript text shown on a terminal or in Markdown.
//!
//! Fields in a transcript are untrusted text: someone may render a shared
//! archive they did not write. These helpers keep such text from driving the
//! terminal or restructuring the Markdown.

/// The characters that can start emphasis, code, links, images, raw HTML,
/// entities, or a table cell in inline Markdown.
const INLINE_SPECIAL: &str = "\\`*_[]<>!&|";

/// What a heading shows for a session without a title.
const UNTITLED: &str = "(untitled)";

/// Which characters of a text to keep: `take` of them after the first
/// `skip`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    /// How many characters to leave out before.
    pub skip: usize,
    /// How many characters to keep.
    pub take: usize,
}

/// Replaces control characters other than newline and tab, and format
/// characters other than the zero-width joiners, with U+FFFD.
///
/// ESC, BEL, and C1 controls would otherwise reach the terminal as escape
/// sequences (window titles, colors, OSC 52 clipboard writes). Bidi overrides
/// such as U+202E would make the text read differently from what it is
/// (Trojan Source). ZWJ and ZWNJ stay because emoji sequences and some
/// scripts need them. A lone CR could overwrite a line on screen, so CRLF
/// becomes LF and any other CR is replaced.
pub fn clean_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .map(|ch| if is_unsafe(ch) { '\u{FFFD}' } else { ch })
        .collect()
}

/// The characters of `text` that `span` keeps. When it leaves some out
/// before, they are counted before an ellipsis; when it leaves some out
/// after, an ellipsis follows, with how many there are and the range that
/// comes next, counted from 1 as `cut -c` counts.
pub fn cut_span(text: &str, span: Span) -> String {
    let length = text.chars().count();
    let skipped = span.skip.min(length);
    let mut out = String::new();
    if skipped > 0 {
        out.extend(["(", &skipped.to_string(), " characters before) \u{2026}"]);
    }
    out.extend(text.chars().skip(skipped).take(span.take));
    let rest = length.saturating_sub(skipped).saturating_sub(span.take);
    if rest > 0 {
        let from = length.saturating_sub(rest).saturating_add(1);
        let to = from.saturating_add(span.take).saturating_sub(1).min(length);
        out.extend([
            "\u{2026} (",
            &rest.to_string(),
            " more characters, next: ",
            &from.to_string(),
            "-",
            &to.to_string(),
            ")",
        ]);
    }
    out
}

/// `text`, cut to `limit` characters and followed by an ellipsis and how
/// many characters were cut when it is longer.
pub fn cut_text(text: &str, limit: usize) -> String {
    let length = text.chars().count();
    if length <= limit {
        return text.to_owned();
    }
    let kept: String = text.chars().take(limit).collect();
    format!(
        "{kept}\u{2026} ({} more characters)",
        length.saturating_sub(limit)
    )
}

/// `text` cut to `limit` characters, the cut marked with an ellipsis
/// alone, for a line that must stay short, as in the digest.
pub fn cut_short(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let mut kept: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        kept.push('\u{2026}');
    }
    kept
}

/// Cleans `text` and collapses all whitespace, newlines included, to single
/// spaces, so a field cannot start a new Markdown block.
pub fn one_line(text: &str) -> String {
    clean_text(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Makes a session title safe for a heading, a table cell, or the digest
/// heading: one line, with inline Markdown and HTML escaped.
pub fn heading_text(title: &str) -> String {
    let line = one_line(title);
    if line.is_empty() {
        return UNTITLED.to_owned();
    }
    escape_inline(&line)
}

/// Backslash-escapes the inline Markdown characters so that text from a
/// transcript renders literally: no `<img onerror>`, no `[x](javascript:)`,
/// no `![](url)` fetched by a previewer. `CommonMark` allows a backslash before
/// any ASCII punctuation.
pub fn escape_inline(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        if INLINE_SPECIAL.contains(ch) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

/// Wraps `text`, on one line, in a backtick run longer than any inside it,
/// padded with spaces when the run is longer than one. `CommonMark` ends a
/// code span only at a run of the same length.
pub fn code_span(text: &str) -> String {
    let line = one_line(text);
    let fence = "`".repeat(longest_backtick_run(&line).saturating_add(1));
    if fence.len() > 1 {
        return format!("{fence} {line} {fence}");
    }
    format!("{fence}{line}{fence}")
}

/// `text` in a fenced code block with `info` as its info string, fenced by a
/// backtick run longer than any inside it. `text` should be cleaned first.
pub fn fenced(info: &str, text: &str) -> String {
    let fence = "`".repeat(longest_backtick_run(text).saturating_add(1).max(3));
    format!("{fence}{info}\n{text}\n{fence}\n")
}

/// Keeps `|` from splitting a GFM table cell or the `date | project | title`
/// digest heading. GFM needs the escape even inside code spans.
pub fn escape_pipes(text: &str) -> String {
    text.replace('|', "\\|")
}

/// `text` as one word of a POSIX shell command: as it is when it is made
/// only of ASCII letters, digits, and `/._-`, and otherwise in single
/// quotes, with each `'` in it written `'\''`.
pub fn shell_quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-'));
    if plain {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The length of the longest run of backticks in `text`.
fn longest_backtick_run(text: &str) -> usize {
    let (longest, _) = text.chars().fold((0_usize, 0_usize), |(longest, run), ch| {
        if ch == '`' {
            let next = run.saturating_add(1);
            (longest.max(next), next)
        } else {
            (longest, 0)
        }
    });
    longest
}

/// Whether `ch` is a control or format character that `clean_text` replaces.
const fn is_unsafe(ch: char) -> bool {
    match ch {
        '\n' | '\t' | '\u{200C}' | '\u{200D}' => false,
        _ => ch.is_control() || is_format(ch),
    }
}

/// Whether `ch` is in the Unicode general category Cf (format).
///
/// The table covers Unicode 15.0.0 and was generated by testing every code
/// point.
const fn is_format(ch: char) -> bool {
    matches!(
        ch,
        '\u{AD}'
            | '\u{600}'..='\u{605}'
            | '\u{61C}'
            | '\u{6DD}'
            | '\u{70F}'
            | '\u{890}'..='\u{891}'
            | '\u{8E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

#[cfg(test)]
mod tests;
