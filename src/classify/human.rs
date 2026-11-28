//! Typed prompts versus harness traffic recorded under the user role, and
//! the replies the harness wrote in place of the model's.

use super::denial::comment_in;
use crate::transcript::entry::{Entry, Role};

/// Names the rule set `classify` applies. It changes whenever a rule change
/// can change which entries are kept.
pub const FILTER_VERSION: &str = "strict-v5";

/// Tags the harness writes at the start of a body under the user role.
/// `local-command` also covers `<local-command-caveat>` and
/// `<local-command-stdout>`.
const HARNESS_TAGS: [&str; 7] = [
    "task-notification",
    "system-reminder",
    "local-command",
    "bash-input",
    "bash-stdout",
    "bash-stderr",
    "user-prompt-submit-hook",
];

/// The model Claude Code names on an assistant line it wrote itself.
pub const HARNESS_MODEL: &str = "<synthetic>";

/// What Claude Code writes as the reply to a turn that wanted none.
const NO_RESPONSE: &str = "No response requested.";

/// The marker Claude Code writes when the user interrupts a turn.
const INTERRUPTED_PREFIX: &str = "[Request interrupted by user";

/// Why an entry was kept or dropped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    /// The summary inserted when the context is compacted.
    CompactSummary,
    /// The body has no text.
    Empty,
    /// The body starts with a tag the harness writes, such as
    /// `<system-reminder>` or `<bash-input>`.
    HarnessTag,
    /// A prompt a person typed.
    Human,
    /// The marker written when the user interrupts a turn.
    Interrupted,
    /// Content the harness injected, such as a skill body.
    Meta,
    /// Claude Code recorded a non-human origin, such as a task notification
    /// or a message from another session.
    NonHumanOrigin,
    /// Not a user message.
    NotUser,
    /// What the user typed when they turned a tool call down: the tool
    /// result of that call carries it.
    Rejection,
    /// A sub-agent's conversation.
    Sidechain,
    /// Tool output.
    ToolResult,
}

/// Says whether `entry` is a prompt a person typed, or why not, checking the
/// reasons in a fixed order.
pub fn classify(entry: &Entry) -> Reason {
    if entry.role != Role::User {
        return Reason::NotUser;
    }
    if entry.flags.meta {
        return Reason::Meta;
    }
    if entry.flags.sidechain {
        return Reason::Sidechain;
    }
    if entry.flags.compact_summary {
        return Reason::CompactSummary;
    }
    // Sessions recorded before Claude Code wrote origin have none; they fall
    // through to the content checks.
    if entry.origin.as_deref().is_some_and(|kind| kind != "human") {
        return Reason::NonHumanOrigin;
    }
    if entry.content.has_tool_result {
        return if comment_in(entry).is_some() {
            Reason::Rejection
        } else {
            Reason::ToolResult
        };
    }
    // Checked even for origin "human": Claude Code records a ! shell escape
    // (<bash-input>) as human although it is not a prompt.
    if starts_with_harness_tag(&entry.content.text) {
        return Reason::HarnessTag;
    }
    let body = entry.content.text.trim();
    if body.is_empty() {
        return Reason::Empty;
    }
    if body.starts_with(INTERRUPTED_PREFIX) {
        return Reason::Interrupted;
    }
    Reason::Human
}

/// Skips ASCII whitespace: tab, newline, form feed, carriage return, and
/// space, but not Unicode spaces.
pub fn skip_ascii_space(text: &str) -> &str {
    text.trim_start_matches(['\t', '\n', '\u{C}', '\r', ' '])
}

/// Whether `text` starts, after ASCII whitespace, with a harness tag.
fn starts_with_harness_tag(text: &str) -> bool {
    skip_ascii_space(text)
        .strip_prefix('<')
        .is_some_and(|rest| HARNESS_TAGS.iter().any(|tag| rest.starts_with(tag)))
}

#[cfg(test)]
mod tests;

/// Whether `text`, a text block of an assistant line by `model`, is the
/// placeholder Claude Code writes when a turn wants no reply. Other text
/// on its own lines stays: a reply cut short is written that way too.
pub fn is_no_response(model: &str, text: &str) -> bool {
    model == HARNESS_MODEL && text.trim() == NO_RESPONSE
}
