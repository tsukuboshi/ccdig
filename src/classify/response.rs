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

/// How Claude Code starts the text of the answers when the user picked an
/// option for each question.
const PICKED: &str = "Your questions have been answered: ";

/// How Claude Code starts the text of the answers when the user typed one
/// in their own words. Versions before 2.1.220 start every text as
/// `PICKED` does.
const TYPED: &str = "The user answered: ";

/// What Claude Code writes on the line before the plan the user approved
/// as it was. A plan the user edited has `(edited by user)` in the heading,
/// and is not the one in the call's input.
const APPROVED_PLAN: &str = "\n## Approved Plan:\n";

/// What stands in a result for an approved plan that is the one in the
/// call's input.
const PLAN_IN_INPUT: &str = "(the plan in the call's input)";

/// Where Claude Code's words after the answers start, in the texts it has
/// written.
const AFTER_ANSWERS: [&str; 2] = [
    ". You can now continue with these answers in mind.",
    ". Read the answers carefully",
];

/// What the user answered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Response {
    /// The answers to the questions, each an option picked.
    Answer(String),
    /// A plan approved.
    Approval(String),
    /// A plan turned down without a comment.
    Rejection(String),
    /// The answers to the questions, one typed in the user's own words.
    TypedAnswer(String),
}

/// What the user answered with the result `answer` of a call to `tool`, or
/// `None` when the result is not an answer of theirs. A plan turned down
/// with a comment is left to `comment_in`, which makes the comment the
/// prompt.
pub fn response(tool: &str, answer: Answer<'_>) -> Option<Response> {
    match tool {
        ASK if !answer.is_error => Some(answers(answer.text)),
        PLAN if !answer.is_error => Some(Response::Approval(first_sentence(answer.text))),
        PLAN => {
            let turned_down = denial(answer);
            (turned_down.kind == USER_REJECTED && turned_down.comment.is_none())
                .then(|| Response::Rejection(first_sentence(answer.text)))
        }
        _ => None,
    }
}

/// The result `text` of a call to `tool` with `plan` in its input, with
/// the approved plan in place of a note that it is the plan in the input,
/// or `None` when the result does not repeat that plan, as when the user
/// edited it.
pub fn without_repeated_plan(tool: &str, plan: Option<&str>, text: &str) -> Option<String> {
    let asked = plan.filter(|_| tool == PLAN)?.trim_end();
    let (before, approved) = text.split_once(APPROVED_PLAN)?;
    (!asked.is_empty() && approved.trim_end() == asked)
        .then(|| [before, APPROVED_PLAN, PLAN_IN_INPUT].concat())
}

/// The answers in `text`, without Claude Code's words around them, typed
/// when Claude Code says the user typed one; all of `text`, as picked, when
/// it is not in a form Claude Code has written.
fn answers(text: &str) -> Response {
    let start = text.trim_start();
    let (rest, typed) = if let Some(rest) = start.strip_prefix(TYPED) {
        (rest, true)
    } else if let Some(rest) = start.strip_prefix(PICKED) {
        (rest, false)
    } else {
        return Response::Answer(text.trim().to_owned());
    };
    let end = AFTER_ANSWERS
        .iter()
        .filter_map(|suffix| rest.rfind(suffix))
        .min()
        .unwrap_or(rest.len());
    let found = rest.get(..end).unwrap_or(rest).trim().to_owned();
    if typed {
        Response::TypedAnswer(found)
    } else {
        Response::Answer(found)
    }
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
