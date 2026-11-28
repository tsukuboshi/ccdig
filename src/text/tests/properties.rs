//! Properties of the text sanitizers that hold for any input.

use proptest::sample::select;
use proptest::strategy::Strategy as _;

use proptest::arbitrary::any;
use proptest::char::range;
use proptest::collection::vec;
use proptest::prop_oneof;

use crate::testing::{TestResult, check_eq, drives_the_terminal, for_all, hostile_text};
use crate::text::{
    clean_text, code_span, escape_inline, escape_pipes, fenced, heading_text, one_line, shell_quote,
};

/// The characters `CommonMark` gives meaning in inline text, written out
/// here rather than taken from the code under test.
const SPECIAL: &str = "\\`*_[]<>!&|";

/// Reverses an inline escape, or returns `None` when a special character is
/// left unescaped or a backslash escapes an ordinary one.
fn unescape(text: &str) -> Option<String> {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let escaped = chars.next().filter(|&next| SPECIAL.contains(next))?;
            plain.push(escaped);
        } else if SPECIAL.contains(ch) {
            return None;
        } else {
            plain.push(ch);
        }
    }
    Some(plain)
}

/// Reads `word` as one word of a POSIX shell, or returns `None` when it is
/// not one plain word: outside single quotes, only ASCII letters, digits,
/// `/._-`, and `\'` for a quote, which the shell leaves as they are.
fn shell_word(word: &str) -> Option<String> {
    let mut read = String::with_capacity(word.len());
    let mut chars = word.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\'' => loop {
                match chars.next()? {
                    '\'' => break,
                    quoted => read.push(quoted),
                }
            },
            '\\' if chars.next()? == '\'' => read.push('\''),
            plain if plain.is_ascii_alphanumeric() || "/._-".contains(plain) => read.push(plain),
            _ => return None,
        }
    }
    Some(read)
}

#[test]
fn shell_quote_reads_back_as_the_text_in_one_word() -> TestResult {
    let ch = prop_oneof![
        2 => select(vec!['\'', '"', ' ', '$', '`', '\\', '\n', ';', '*', '~']),
        2 => range('a', 'z'),
        1 => any::<char>(),
    ];
    let texts = vec(ch, 0..=32).prop_map(String::from_iter);
    for_all(&texts, |text| {
        let quoted = shell_quote(&text);
        check_eq(&shell_word(&quoted), &Some(text), &quoted)?;
        check_eq(&quoted.is_empty(), &false, "an empty word would vanish")?;
        Ok(())
    })?;
    for_all(&hostile_text(), |text| {
        check_eq(&shell_word(&shell_quote(&text)), &Some(text.clone()), &text)?;
        Ok(())
    })
}

#[test]
fn clean_text_is_idempotent_and_leaves_nothing_that_drives_the_terminal() -> TestResult {
    for_all(&hostile_text(), |text| {
        let cleaned = clean_text(&text);
        check_eq(&clean_text(&cleaned), &cleaned, "cleaning twice")?;
        let bad: Vec<char> = cleaned
            .chars()
            .filter(|&ch| drives_the_terminal(ch))
            .collect();
        check_eq(&bad, &Vec::new(), "controls left")?;
        let shorter = cleaned.chars().count() <= text.chars().count();
        check_eq(&shorter, &true, "no longer than the input")?;
        Ok(())
    })
}

#[test]
fn one_line_is_idempotent_and_single_spaced() -> TestResult {
    for_all(&hostile_text(), |text| {
        let line = one_line(&text);
        check_eq(&one_line(&line), &line, "one_line twice")?;
        let other_space = line.chars().any(|ch| ch.is_whitespace() && ch != ' ');
        check_eq(&other_space, &false, "whitespace other than a space")?;
        check_eq(&line.contains("  "), &false, "a run of spaces")?;
        check_eq(&line.trim(), &line.as_str(), "outer spaces")?;
        Ok(())
    })
}

#[test]
fn escape_inline_escapes_every_special_character_and_nothing_else() -> TestResult {
    for_all(&hostile_text(), |text| {
        check_eq(&unescape(&escape_inline(&text)), &Some(text), "unescaped")?;
        Ok(())
    })
}

#[test]
fn heading_text_is_one_escaped_line_that_is_never_empty() -> TestResult {
    for_all(&hostile_text(), |title| {
        let heading = heading_text(&title);
        let line = one_line(&title);
        let want = if line.is_empty() {
            "(untitled)".to_owned()
        } else {
            line
        };
        check_eq(&heading.contains('\n'), &false, "a line break")?;
        check_eq(&unescape(&heading), &Some(want), "unescaped heading")?;
        Ok(())
    })
}

#[test]
fn escape_pipes_leaves_no_bare_pipe_and_reverses() -> TestResult {
    for_all(&hostile_text(), |text| {
        let escaped = escape_pipes(&text);
        // Every escaped pipe is removed, so any pipe left was bare.
        let bare = escaped.replace("\\|", "").contains('|');
        check_eq(&bare, &false, "a bare pipe")?;
        check_eq(&escaped.replace("\\|", "|"), &text, "reversed")?;
        Ok(())
    })
}

#[test]
fn code_span_fence_outruns_every_backtick_run_inside() -> TestResult {
    let texts =
        hostile_text().prop_filter("a code span needs text", |text| !one_line(text).is_empty());
    for_all(&texts, |text| {
        let span = code_span(&text);
        let width = span.chars().take_while(|&ch| ch == '`').count();
        let fence = "`".repeat(width);
        let fenced_in = span
            .strip_prefix(&fence)
            .and_then(|rest| rest.strip_suffix(&fence))
            .ok_or("unbalanced fence")?;
        let inner = if width > 1 {
            fenced_in
                .strip_prefix(' ')
                .and_then(|rest| rest.strip_suffix(' '))
                .ok_or("unpadded")?
        } else {
            fenced_in
        };
        check_eq(inner, one_line(&text).as_str(), "span content")?;
        check_eq(
            &inner.contains(&fence),
            &false,
            "a run as long as the fence",
        )?;
        Ok(())
    })
}

#[test]
fn fenced_text_cannot_close_its_own_block() -> TestResult {
    for_all(
        &(select(&["text", "json"][..]), hostile_text()),
        |(info, raw)| {
            let text = clean_text(&raw);
            let block = fenced(info, &text);
            let width = block.chars().take_while(|&ch| ch == '`').count();
            let fence = "`".repeat(width);
            check_eq(&(width >= 3), &true, "fence of at least three")?;
            check_eq(
                &block,
                &format!("{fence}{info}\n{text}\n{fence}\n"),
                "block layout",
            )?;
            check_eq(&text.contains(&fence), &false, "a run as long as the fence")?;
            Ok(())
        },
    )
}
