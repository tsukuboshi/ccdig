//! Tests for the text sanitizers, with hostile inputs.

mod properties;

use super::{
    Lang, Span, clean_text, code_span, cut_span, cut_text, escape_inline, escape_pipes, one_line,
    shell_quote,
};
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

#[test]
fn cut_span_keeps_a_range_and_says_what_is_before_and_after() -> TestResult {
    let text = "0123456789abcdefghij";
    for (skip, take, want) in [
        (0, 30, "0123456789abcdefghij"),
        (0, 20, "0123456789abcdefghij"),
        (0, 5, "01234\u{2026} (15 more characters, next: 6-10)"),
        (
            5,
            5,
            "(5 characters before) \u{2026}56789\u{2026} (10 more characters, next: 11-15)",
        ),
        (15, 10, "(15 characters before) \u{2026}fghij"),
        (
            12,
            5,
            "(12 characters before) \u{2026}cdefg\u{2026} (3 more characters, next: 18-20)",
        ),
        (20, 5, "(20 characters before) \u{2026}"),
        (40, 5, "(20 characters before) \u{2026}"),
    ] {
        check_eq(
            &cut_span(text, Span { skip, take }, Lang::En),
            &want.to_owned(),
            &format!("{skip} {take}"),
        )?;
    }
    check_eq(
        &cut_span(
            "\u{65e5}\u{672c}\u{8a9e}",
            Span { skip: 1, take: 1 },
            Lang::En,
        ),
        &"(1 characters before) \u{2026}\u{672c}\u{2026} (1 more characters, next: 3-3)".to_owned(),
        "characters, not bytes",
    )?;
    Ok(())
}

#[test]
fn the_notes_of_a_cut_are_in_the_language_asked_for() -> TestResult {
    let text = "0123456789abcdefghij";
    // (name, what it gave, what it should give)
    for (name, got, want) in [
        (
            "a span with text before and after",
            cut_span(text, Span { skip: 5, take: 5 }, Lang::Ja),
            // (手前に 5 文字) …56789… (残り 10 文字、次: 11-15)
            "(\u{624b}\u{524d}\u{306b} 5 \u{6587}\u{5b57}) \u{2026}56789\u{2026} (\u{6b8b}\u{308a} 10 \u{6587}\u{5b57}\u{3001}\u{6b21}: 11-15)",
        ),
        (
            "a span in English",
            cut_span(text, Span { skip: 5, take: 5 }, Lang::En),
            "(5 characters before) \u{2026}56789\u{2026} (10 more characters, next: 11-15)",
        ),
        (
            "a cut text",
            cut_text(text, 15, Lang::Ja),
            // 0123456789abcde… (残り 5 文字)
            "0123456789abcde\u{2026} (\u{6b8b}\u{308a} 5 \u{6587}\u{5b57})",
        ),
        (
            "a cut text in English",
            cut_text(text, 15, Lang::En),
            "0123456789abcde\u{2026} (5 more characters)",
        ),
    ] {
        check_eq(got.as_str(), want, name)?;
    }
    Ok(())
}

#[test]
fn shell_quote_leaves_plain_words_and_single_quotes_the_rest() -> TestResult {
    for (text, want) in [
        ("b951b9fe-0c1d-4e2f", "b951b9fe-0c1d-4e2f"),
        (
            "/Users/alice/work/ccdig_v2.1",
            "/Users/alice/work/ccdig_v2.1",
        ),
        ("", "''"),
        ("/Users/alice/my work", "'/Users/alice/my work'"),
        ("it's", r"'it'\''s'"),
        ("$(rm -rf ~)", "'$(rm -rf ~)'"),
        ("a;b|c&d", "'a;b|c&d'"),
        (
            "/Users/\u{30A2}\u{30EA}\u{30B9}",
            "'/Users/\u{30A2}\u{30EA}\u{30B9}'",
        ),
        ("line\nbreak", "'line\nbreak'"),
    ] {
        check_eq(shell_quote(text).as_str(), want, text)?;
    }
    Ok(())
}
