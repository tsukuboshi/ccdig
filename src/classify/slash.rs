//! Slash command expansions.

use super::human::skip_ascii_space;

/// What a slash command expansion folds back into.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Folded {
    /// The command as recorded, such as `/weekly-report`.
    pub command: String,
    /// What the user typed: the command, then its arguments if any.
    pub text: String,
}

/// Turns the expansion Claude Code records for a slash command back into
/// what the user typed, such as `/weekly-report last week`. `None` when the
/// body is not an expansion.
///
/// The name and args tags are found separately because Claude Code does not
/// always write them in the same order, and args run to their closing tag so
/// newlines and `<` inside them survive.
pub fn fold(body: &str) -> Option<Folded> {
    let start = skip_ascii_space(body);
    if !start.starts_with("<command-message>") && !start.starts_with("<command-name>") {
        return None;
    }
    let command = between(body, "<command-name>", "</command-name>")?.trim();
    let text = match between(body, "<command-args>", "</command-args>").map(str::trim) {
        Some(args) if !args.is_empty() => format!("{command} {args}"),
        Some(_) | None => command.to_owned(),
    };
    Some(Folded {
        command: command.to_owned(),
        text,
    })
}

/// The text between the first `open` and the first `close` after it.
fn between<'body>(body: &'body str, open: &str, close: &str) -> Option<&'body str> {
    let (_, after_open) = body.split_once(open)?;
    let (inside, _) = after_open.split_once(close)?;
    Some(inside)
}

#[cfg(test)]
mod tests;
