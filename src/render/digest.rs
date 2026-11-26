//! The digest: compact Markdown of the prompts, meant as LLM input.

use jiff::tz::TimeZone;

use crate::failure::Failure;
use crate::record::{Head, Kind, Prompt};
use crate::text::{code_span, escape_inline, heading_text, one_line};

/// The size limits of a digest. Lengths count characters, not bytes, so
/// multibyte text is cut on character boundaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Each prompt is cut to this many characters, the cut marked with an
    /// ellipsis.
    pub prompt_chars: usize,
    /// At most this many characters are listed per session; the remaining
    /// prompts are summarized as a count.
    pub session_chars: usize,
}

/// Limits that fit a weekly digest.
impl Default for Limits {
    fn default() -> Self {
        Self {
            prompt_chars: 300,
            session_chars: 2000,
        }
    }
}

/// One section per transcript file, one line per prompt, cut to `limits`.
/// Prompts of a file must be adjacent.
///
/// # Errors
///
/// When a date cannot be shown in `zone`.
pub fn digest(prompts: &[Prompt], zone: &TimeZone, limits: Limits) -> Result<String, Failure> {
    let mut out = String::new();
    for (index, group) in prompts
        .chunk_by(|left, right| left.source.file == right.source.file)
        .enumerate()
    {
        if index > 0 {
            out.push('\n');
        }
        session(&mut out, group, zone, limits)?;
    }
    Ok(out)
}

/// Names the project by its cwd with the home directory shown as `~`.
/// Decoding the directory name instead is lossy: Claude Code writes both
/// `/` and `.` as `-`, so `/Users/a.b/x` and `/Users/a/b/x` look the same.
fn project_label(head: &Head) -> String {
    if head.cwd.is_empty() {
        return head.project.clone();
    }
    for base in ["/Users/", "/home/"] {
        if let Some(rest) = head.cwd.strip_prefix(base) {
            let (user, tail) = rest
                .find('/')
                .and_then(|at| rest.split_at_checked(at))
                .unwrap_or((rest, ""));
            if !user.is_empty() {
                return format!("~{tail}");
            }
        }
    }
    head.cwd.clone()
}

/// Writes the section of one transcript file.
fn session(
    out: &mut String,
    prompts: &[Prompt],
    zone: &TimeZone,
    limits: Limits,
) -> Result<(), Failure> {
    let Some(first) = prompts.first() else {
        return Ok(());
    };
    out.extend([
        "## ",
        &first.head.timestamp.date_label(zone)?,
        " | ",
        &escape_inline(&one_line(&project_label(&first.head))),
        " | ",
        &heading_text(&first.head.session_title),
        "\n",
    ]);
    let mut used = 0_usize;
    for (index, prompt) in prompts.iter().enumerate() {
        let line = one_line(&prompt.text);
        let mut text: String = line.chars().take(limits.prompt_chars).collect();
        if line.chars().count() > limits.prompt_chars {
            text.push('\u{2026}');
        }
        let length = text.chars().count();
        if used.saturating_add(length) > limits.session_chars {
            out.extend([
                "- (",
                &prompts.len().saturating_sub(index).to_string(),
                " more prompts omitted)\n",
            ]);
            return Ok(());
        }
        // A code span keeps the prompt literal when the digest is
        // previewed: no rendered HTML, no ![](url) fetched from a
        // transcript.
        let mark = match prompt.kind {
            Kind::Answer => "(answer) ",
            Kind::Approval => "(approval) ",
            Kind::Rejection => "(rejection) ",
            Kind::Slash | Kind::Text => "",
        };
        out.extend(["- ", mark, &code_span(&text), "\n"]);
        used = used.saturating_add(length);
    }
    Ok(())
}
