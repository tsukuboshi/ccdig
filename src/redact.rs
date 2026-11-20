//! Masking of secrets that people paste into prompts and tools print.
//!
//! The patterns are deliberately conservative: each one matches a format
//! with a distinctive prefix or an explicit variable name, so ordinary prose
//! about authentication is left alone.
//!
//! They are promptdig's Go (RE2) patterns, where `\s`, `\w`, `\d`, and `\b`
//! are ASCII-only. Rust's are Unicode by default and would, for example, read
//! a Japanese character right after a key as part of it, so the patterns
//! spell out the ASCII classes: `[\t\n\x0C\r ]`, `[0-9A-Za-z_]`, `[0-9]`, and
//! `(?-u:\b)`.

use regex::{Captures, Regex};
use serde_json::Value;

/// Starts every prefix rule. It is used instead of `\b` because `_` is a word
/// character: `\b` misses `TF_VAR_aws_...`, `MY_ghp_...`, and `MY_AKIA...`.
/// The boundary character is captured as `${1}` and put back.
macro_rules! boundary {
    () => {
        r"(^|[^A-Za-z0-9])"
    };
}

/// Replaces each redacted secret.
pub const MARKER: &str = "***REDACTED***";

/// The rules in the order they apply.
///
/// Token formats without a source comment follow the default rules of
/// gitleaks: <https://github.com/gitleaks/gitleaks/blob/master/config/gitleaks.toml>.
const RULES: [(&str, Replacement); 15] = [
    // AWS credentials by variable name: export lines from the SSO portal,
    // YAML, credentials files, and JSON with snake_case keys. The value must
    // be 16+ characters so that "AWS_SESSION_TOKEN: is it required?" keeps
    // its prose.
    (
        r#"(?i)((?:^|[^A-Za-z0-9])aws_(?:secret_access_key|session_token|access_key_id)["']?[\t\n\x0C\r ]*[:=][\t\n\x0C\r ]*["']?)[A-Za-z0-9/+=_-]{16,}"#,
        Replacement::KeepPrefix,
    ),
    // AWS credentials as printed by `aws sts` and
    // `aws configure export-credentials`.
    (
        r#"("(?:AccessKeyId|SecretAccessKey|SessionToken)"[\t\n\x0C\r ]*:[\t\n\x0C\r ]*")[^"]{16,}"#,
        Replacement::KeepPrefix,
    ),
    // Access key IDs on their own: AKIA for long-term keys, ASIA for
    // temporary STS keys.
    // https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_identifiers.html#identifiers-prefixes
    (
        concat!(boundary!(), r"(?:AKIA|ASIA)[0-9A-Z]{16}(?-u:\b)"),
        Replacement::KeepPrefix,
    ),
    // GitHub tokens: ghp_ (personal), gho_ (OAuth), ghu_ (user-to-server),
    // ghs_ (server-to-server), ghr_ (refresh), and fine-grained github_pat_.
    // https://github.blog/engineering/platform-security/behind-githubs-new-authentication-token-formats/
    (
        concat!(boundary!(), r"(?:gh[pousr]|github_pat)_[A-Za-z0-9_]{20,}"),
        Replacement::KeepPrefix,
    ),
    // GitLab personal access tokens.
    (
        concat!(boundary!(), r"glpat-[0-9A-Za-z_-]{20,}"),
        Replacement::KeepPrefix,
    ),
    // npm access tokens.
    (
        concat!("(?i)", boundary!(), r"npm_[a-z0-9]{36}"),
        Replacement::KeepPrefix,
    ),
    // Google API keys.
    (
        concat!(boundary!(), r"AIza[0-9A-Za-z_-]{35}"),
        Replacement::KeepPrefix,
    ),
    // Anthropic (sk-ant-) and OpenAI (sk-, sk-proj-) API keys. The length
    // floor keeps short words such as "sk-1" out.
    (
        concat!(boundary!(), r"sk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}"),
        Replacement::KeepPrefix,
    ),
    // Stripe secret and restricted keys.
    (
        concat!(
            boundary!(),
            r"(?:sk|rk)_(?:test|live|prod)_[A-Za-z0-9]{10,99}"
        ),
        Replacement::KeepPrefix,
    ),
    // Slack tokens, app-level tokens, and incoming webhook URLs.
    (
        concat!(boundary!(), r"xox[abposr]-[A-Za-z0-9-]{10,}"),
        Replacement::KeepPrefix,
    ),
    (
        concat!(
            "(?i)",
            boundary!(),
            r"xapp-[0-9]-[A-Z0-9]+-[0-9]+-[a-z0-9]+"
        ),
        Replacement::KeepPrefix,
    ),
    (
        r"hooks\.slack\.com/(?:services|workflows|triggers)/[A-Za-z0-9+/]{43,56}",
        Replacement::Marker,
    ),
    // JSON Web Tokens: two base64url "ey..." segments and a signature.
    (
        concat!(
            boundary!(),
            r"ey[A-Za-z0-9]{17,}\.ey[A-Za-z0-9/\\_-]{17,}\.(?:[A-Za-z0-9/\\_-]{10,}={0,2})?"
        ),
        Replacement::KeepPrefix,
    ),
    // The password in a URL's userinfo, such as postgres://app:secret@host or
    // redis://:secret@host. It runs to the last '@' before the path, so a
    // password containing '@', '?', or '#' is masked whole; a '/' in the
    // path stops it, so ?email=a@b.com after the path does not match. The
    // user may not contain '?' or '#', and a port followed by a query or
    // fragment, as in host:8443?email=a@b.com, is left alone; promptdig
    // masked both. The user and host stay so the prompt still says what it
    // connects to.
    (
        r"(://[^/\t\n\x0C\r :@?#]*:)([^\t\n\x0C\r /]*)@",
        Replacement::UrlPassword,
    ),
    // PEM and PGP private keys, through the END line or, when the paste was
    // cut off, to the end of the text.
    (
        r"(?s)-----BEGIN [A-Z0-9 _-]*PRIVATE KEY(?: BLOCK)?-----.*?(?:-----END [A-Z0-9 _-]*PRIVATE KEY(?: BLOCK)?-----|\z)",
        Replacement::Marker,
    ),
];

/// What a rule puts in place of its match.
#[derive(Clone, Copy)]
enum Replacement {
    /// The first capture group, then the marker.
    KeepPrefix,
    /// The marker alone.
    Marker,
    /// The first capture group, the marker, then `@`, unless the second
    /// capture group is a port followed by a query or fragment.
    UrlPassword,
}

/// One pattern and its replacement.
struct Rule {
    /// The secret to find.
    pattern: Regex,
    /// Whether a match that is a port and a query stays as it is.
    port_guard: bool,
    /// The replacement text, in `regex` replacement syntax.
    replacement: String,
}

/// The compiled masking rules, applied in order.
pub struct Redactor {
    /// Each later rule sees the output of the earlier ones.
    rules: Vec<Rule>,
}

impl Redactor {
    /// Compiles the rules.
    ///
    /// The patterns are fixed, so an error here is a bug in this file; a
    /// test compiles them so it surfaces before release.
    pub fn new() -> Result<Self, regex::Error> {
        let rules = RULES
            .iter()
            .map(|&(pattern, replacement)| {
                Ok(Rule {
                    pattern: Regex::new(pattern)?,
                    port_guard: matches!(replacement, Replacement::UrlPassword),
                    replacement: match replacement {
                        Replacement::Marker => MARKER.to_owned(),
                        Replacement::KeepPrefix => format!("${{1}}{MARKER}"),
                        Replacement::UrlPassword => {
                            format!("${{1}}{MARKER}@")
                        }
                    },
                })
            })
            .collect::<Result<Vec<_>, regex::Error>>()?;
        Ok(Self { rules })
    }

    /// Masks secrets in `text` and reports whether it changed anything.
    pub fn redact(&self, text: &str) -> (String, bool) {
        let mut masked = text.to_owned();
        for rule in &self.rules {
            masked = if rule.port_guard {
                rule.pattern
                    .replace_all(&masked, |captures: &Captures<'_>| {
                        let group = |index| captures.get(index).map_or("", |found| found.as_str());
                        if is_port_then_query(group(2)) {
                            group(0).to_owned()
                        } else {
                            format!("{}{MARKER}@", group(1))
                        }
                    })
                    .into_owned()
            } else {
                rule.pattern
                    .replace_all(&masked, rule.replacement.as_str())
                    .into_owned()
            };
        }
        let changed = masked != text;
        (masked, changed)
    }

    /// Masks secrets in every string inside `value`, such as a tool's input,
    /// and reports whether it changed anything. Object keys stay as they are.
    pub fn redact_json(&self, value: &mut Value) -> bool {
        match *value {
            Value::String(ref mut text) => {
                let (masked, changed) = self.redact(text);
                if changed {
                    *text = masked;
                }
                changed
            }
            Value::Array(ref mut items) => {
                let mut changed = false;
                for item in items {
                    changed |= self.redact_json(item);
                }
                changed
            }
            Value::Object(ref mut fields) => {
                let mut changed = false;
                for field in fields.values_mut() {
                    changed |= self.redact_json(field);
                }
                changed
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => false,
        }
    }
}

/// Whether `text`, read as a URL password, is a port number followed by a
/// query or fragment instead.
fn is_port_then_query(text: &str) -> bool {
    let rest = text.trim_start_matches(|ch: char| ch.is_ascii_digit());
    rest.len() < text.len() && rest.starts_with(['?', '#'])
}

#[cfg(test)]
mod tests;
