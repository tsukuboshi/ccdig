//! Tool calls that were turned down, and what the user said when they turned
//! one down.

/// How the comment starts when the user picks "clarify" in a question
/// dialog: Claude Code's words, not the user's.
const CLARIFY_PREFIX: &str = "The user wants to clarify these questions.";

/// How the text of a call the user turned down starts.
const REJECTED_PREFIX: &str = "The user doesn't want to proceed with this tool use.";

/// What comes before the comment in the text of a call the user turned
/// down with one.
const SAID: &str = "the user said:\n";

/// The kind Claude Code records when the user turns a call down.
pub const USER_REJECTED: &str = "user-rejected";

/// What Claude Code recorded about a tool result: the line's
/// `toolDenialKind` and `userFeedback`, and the `tool_result` block.
#[derive(Clone, Copy, Debug)]
pub struct Answer<'line> {
    /// `toolDenialKind`, empty when the line has none.
    pub denial_kind: &'line str,
    /// `userFeedback`, empty when the line has none.
    pub feedback: &'line str,
    /// The block's `is_error`.
    pub is_error: bool,
    /// The block's text.
    pub text: &'line str,
}

/// Why a tool call did not run, and what the user said about it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Denial {
    /// What the user typed when they turned the call down, if anything.
    pub comment: Option<String>,
    /// Claude Code's name for why the call did not run, such as
    /// `user-rejected`, or empty when it ran.
    pub kind: String,
}

/// What the user typed when they turned the call down: `userFeedback`, or
/// before Claude Code recorded it, the text after "the user said".
fn comment(answer: Answer<'_>) -> Option<String> {
    let said = if answer.feedback.is_empty() {
        answer.text.split_once(SAID)?.1
    } else {
        answer.feedback
    }
    .trim();
    (!said.is_empty() && !said.starts_with(CLARIFY_PREFIX)).then(|| said.to_owned())
}

/// Reads why the call `answer` answers did not run. Before Claude Code
/// recorded `toolDenialKind`, only the text tells that the user turned it
/// down, and only that kind is recognised.
pub fn denial(answer: Answer<'_>) -> Denial {
    let kind = if !answer.denial_kind.is_empty() {
        answer.denial_kind
    } else if answer.is_error && answer.text.starts_with(REJECTED_PREFIX) {
        USER_REJECTED
    } else {
        return Denial::default();
    };
    let comment = if kind == USER_REJECTED {
        comment(answer)
    } else {
        None
    };
    Denial {
        comment,
        kind: kind.to_owned(),
    }
}

#[cfg(test)]
mod tests;
