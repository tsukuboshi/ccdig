//! Properties of folding slash command expansions.

use proptest::arbitrary::any;
use proptest::sample::select;
use proptest::strategy::Strategy as _;

use crate::classify::human::skip_ascii_space;
use crate::classify::slash::{Folded, fold};
use crate::testing::{TestResult, check_eq, for_all, hostile_text};

#[test]
fn an_expansion_folds_back_into_the_command_and_its_arguments() -> TestResult {
    let arguments =
        hostile_text().prop_filter("no command tags inside", |text| !text.contains("<command-"));
    let cases = (
        "/[a-z][a-z0-9-]{0,15}",
        arguments,
        any::<bool>(),
        any::<bool>(),
        select(&["", " ", "\n", "\t\n "][..]),
    );
    for_all(
        &cases,
        |(command, args, message_first, args_first, space)| {
            let name = format!("<command-name>{command}</command-name>");
            let message = format!(
                "<command-message>{}</command-message>",
                command.trim_start_matches('/')
            );
            let wrapped = format!("<command-args>{args}</command-args>");
            let (first, second) = if message_first {
                (&message, &name)
            } else {
                (&name, &message)
            };
            let body = if args_first {
                format!("{space}{first}\n{wrapped}\n{second}")
            } else {
                format!("{space}{first}\n{second}\n{wrapped}")
            };
            let trimmed = args.trim();
            let text = if trimmed.is_empty() {
                command.clone()
            } else {
                format!("{command} {trimmed}")
            };
            check_eq(&fold(&body), &Some(Folded { command, text }), &body)?;
            Ok(())
        },
    )
}

#[test]
fn a_body_that_does_not_start_with_a_command_tag_does_not_fold() -> TestResult {
    let bodies = hostile_text().prop_filter("no command tag at the start", |text| {
        let start = skip_ascii_space(text);
        !start.starts_with("<command-message>") && !start.starts_with("<command-name>")
    });
    for_all(&bodies, |body| {
        check_eq(&fold(&body), &None, "folded")?;
        Ok(())
    })
}
