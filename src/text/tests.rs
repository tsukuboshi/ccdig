//! Tests for the text sanitizers, with hostile inputs.

use super::{clean_text, code_span, escape_inline, escape_pipes, heading_text, one_line};
use crate::testing::{TestResult, check_eq};

#[test]
fn clean_text_replaces_terminal_controls_and_bidi_overrides() -> TestResult {
    let input = concat!(
        "retitle \u{1B}]0;pwned\u{7} and color \u{1B}[31mred\u{1B}[0m\u{9B}\n",
        "next\tline \u{202E} family \u{1F468}\u{200D}\u{1F469}",
    );
    let want = concat!(
        "retitle \u{FFFD}]0;pwned\u{FFFD} and color \u{FFFD}[31mred\u{FFFD}[0m\u{FFFD}\n",
        "next\tline \u{FFFD} family \u{1F468}\u{200D}\u{1F469}",
    );
    check_eq(clean_text(input).as_str(), want, "hostile prompt text")?;
    Ok(())
}

#[test]
fn clean_text_turns_crlf_into_lf_and_a_lone_cr_into_a_replacement() -> TestResult {
    check_eq(
        clean_text("one\r\ntwo\rthree").as_str(),
        "one\ntwo\u{FFFD}three",
        "carriage returns",
    )?;
    Ok(())
}

#[test]
fn clean_text_keeps_zero_width_joiners_and_replaces_other_format_characters() -> TestResult {
    check_eq(
        clean_text("a\u{200C}b\u{200D}c\u{200B}d\u{FEFF}e\u{E0041}f\u{AD}g").as_str(),
        "a\u{200C}b\u{200D}c\u{FFFD}d\u{FFFD}e\u{FFFD}f\u{FFFD}g",
        "format characters",
    )?;
    Ok(())
}

#[test]
fn one_line_collapses_every_kind_of_whitespace() -> TestResult {
    check_eq(
        one_line("  a \n\n b\t c\u{3000}d\u{A0}e  ").as_str(),
        "a b c d e",
        "whitespace runs",
    )?;
    Ok(())
}

#[test]
fn heading_text_escapes_inline_markdown_on_one_line() -> TestResult {
    let title = concat!(
        "title | with pipe\n## fake section <img src=x onerror=alert(1)> ",
        "[x](javascript:alert(1)) \u{202E}evil",
    );
    let want = concat!(
        "title \\| with pipe ## fake section \\<img src=x onerror=alert(1)\\> ",
        "\\[x\\](javascript:alert(1)) \u{FFFD}evil",
    );
    check_eq(heading_text(title).as_str(), want, "hostile title")?;
    Ok(())
}

#[test]
fn heading_text_names_a_blank_title_untitled() -> TestResult {
    check_eq(heading_text("").as_str(), "(untitled)", "empty title")?;
    check_eq(heading_text(" \n\t ").as_str(), "(untitled)", "blank title")?;
    Ok(())
}

#[test]
fn escape_inline_escapes_each_inline_markdown_character_but_not_hash() -> TestResult {
    check_eq(
        escape_inline("\\`*_[]<>!&|#").as_str(),
        "\\\\\\`\\*\\_\\[\\]\\<\\>\\!\\&\\|#",
        "special characters",
    )?;
    Ok(())
}

#[test]
fn code_span_uses_a_single_backtick_when_the_text_has_none() -> TestResult {
    check_eq(code_span("short one").as_str(), "`short one`", "plain text")?;
    Ok(())
}

#[test]
fn code_span_outruns_backticks_inside_and_pads_the_longer_fence() -> TestResult {
    check_eq(
        code_span("s`1\n# injected heading").as_str(),
        "`` s`1 # injected heading ``",
        "one backtick inside",
    )?;
    check_eq(
        code_span("a``b").as_str(),
        "``` a``b ```",
        "two backticks inside",
    )?;
    Ok(())
}

#[test]
fn escape_pipes_keeps_a_code_span_inside_one_table_cell() -> TestResult {
    check_eq(
        escape_pipes(&code_span("p|`x`\nrow")).as_str(),
        "`` p\\|`x` row ``",
        "project cell",
    )?;
    Ok(())
}
