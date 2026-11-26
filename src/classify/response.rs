//! What the user answered through a dialog Claude Code showed them: the
//! answers to its questions, and a plan approved or turned down without a
//! comment. These are the user's words as much as a typed prompt is, but
//! they reach the transcript as tool results.

use super::denial::{Answer, USER_REJECTED, denial};

/// The tool that asks the user questions.
pub const ASK: &str = "AskUserQuestion";

/// The tool that shows the user a plan to approve.
pub const PLAN: &str = "ExitPlanMode";

/// The tools that show the user a dialog, whose answers are prompts.
pub const DIALOGS: [&str; 2] = [ASK, PLAN];

/// How Claude Code starts the text of the answers, in the texts it has
/// written.
const ANSWERED: [&str; 2] = ["Your questions have been answered: ", "The user answered: "];

/// Where Claude Code's words after the answers start, in the texts it has
/// written.
const AFTER_ANSWERS: [&str; 2] = [
    ". You can now continue with these answers in mind.",
    ". Read the answers carefully",
];

/// What the user answered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Response {
    /// The answers to the questions.
    Answer(String),
    /// A plan approved.
    Approval(String),
    /// A plan turned down without a comment.
    Rejection(String),
}

/// What the user answered with the result `answer` of a call to `tool`, or
/// `None` when the result is not an answer of theirs. A plan turned down
/// with a comment is left to `comment_in`, which makes the comment the
/// prompt.
pub fn response(tool: &str, answer: Answer<'_>) -> Option<Response> {
    match tool {
        ASK if !answer.is_error => Some(Response::Answer(answers(answer.text))),
        PLAN if !answer.is_error => Some(Response::Approval(first_sentence(answer.text))),
        PLAN => {
            let turned_down = denial(answer);
            (turned_down.kind == USER_REJECTED && turned_down.comment.is_none())
                .then(|| Response::Rejection(first_sentence(answer.text)))
        }
        _ => None,
    }
}

/// The answers in `text`, without Claude Code's words around them; all of
/// `text` when it is not in a form Claude Code has written.
fn answers(text: &str) -> String {
    let Some(rest) = ANSWERED
        .iter()
        .find_map(|prefix| text.trim_start().strip_prefix(prefix))
    else {
        return text.trim().to_owned();
    };
    let end = AFTER_ANSWERS
        .iter()
        .filter_map(|suffix| rest.rfind(suffix))
        .min()
        .unwrap_or(rest.len());
    rest.get(..end).unwrap_or(rest).trim().to_owned()
}

/// The first sentence of `text`, which says what happened: up to the first
/// period followed by a space or a line break, or the first line.
fn first_sentence(text: &str) -> String {
    let line = text.trim_start().lines().next().unwrap_or_default();
    line.find(". ")
        .and_then(|end| line.get(..=end))
        .unwrap_or(line)
        .trim()
        .to_owned()
}
