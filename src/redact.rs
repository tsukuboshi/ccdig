//! Masking of secrets that people paste into prompts and tools print.
//!
//! The patterns are deliberately conservative: each one matches a format
//! with a distinctive prefix or an explicit variable name, so ordinary prose
//! about authentication is left alone.
//!
//! The `regex` crate's `\s`, `\w`, `\d`, and `\b` are Unicode by default and
//! would, for example, read a Japanese character right after a key as part
//! of it, so the patterns spell out the ASCII classes: `[\t\n\x0C\r ]`,
//! `[0-9A-Za-z_]`, `[0-9]`, and `(?-u:\b)`.

mod name;

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
const RULES: [(&str, Replacement); 24] = [
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
    // SendGrid API keys.
    (
        concat!(boundary!(), r"SG\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}"),
        Replacement::KeepPrefix,
    ),
    // Google OAuth access tokens (ya29.), refresh tokens (1//0), and client
    // secrets (GOCSPX-).
    (
        concat!(boundary!(), r"ya29\.[0-9A-Za-z_-]{20,}"),
        Replacement::KeepPrefix,
    ),
    (
        concat!(boundary!(), r"1//0[0-9A-Za-z_-]{20,}"),
        Replacement::KeepPrefix,
    ),
    (
        concat!(boundary!(), r"GOCSPX-[0-9A-Za-z_-]{10,}"),
        Replacement::KeepPrefix,
    ),
    // Stripe webhook signing secrets.
    (
        concat!(boundary!(), r"whsec_[0-9A-Za-z]{10,}"),
        Replacement::KeepPrefix,
    ),
    // The credentials of an Authorization header. "<token>" and other
    // placeholders do not match the value class and stay.
    (
        r#"(?i)((?:^|[^A-Za-z0-9])authorization["']?[\t ]*[:=][\t ]*["']?(?:bearer|basic|token)[\t ]+)[A-Za-z0-9._~+/=-]{8,}"#,
        Replacement::KeepPrefix,
    ),
    // A bearer token without the header name, in any case. The length
    // floor keeps prose such as "Bearer authentication" out.
    (
        r"((?-u:\b)(?i:bearer) )[A-Za-z0-9._~+/=-]{20,}",
        Replacement::KeepPrefix,
    ),
    // A password glued to -p of the MySQL clients, run as a command: at the
    // start, after a separator, or by a bin/ path, not /var/lib/mysql in an
    // argument. -p followed by a space
    // prompts for the password, and the next word is a database. A variable
    // or a placeholder, such as -p$MYSQL_PWD or -p{}, and prose that only
    // mentions -p stay.
    (
        r#"((?:^|[\t\n ;&|(]|bin/)(?:mysql|mysqldump|mysqladmin|mariadb)(?-u:\b)[^\n|;&]*?[\t ]-p)["']?[A-Za-z0-9!#%*+./=?@^_~][\x21-\x7E]*"#,
        Replacement::KeepPrefix,
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
    // fragment, as in host:8443?email=a@b.com, is left alone. The user and
    // host stay so the prompt still says what it connects to.
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
    // A value assigned to a name: `name = value`, `name: value`,
    // `name => value`, `--name=value`, `?name=value`, the same inside escaped
    // JSON (`\"password\":\"...\"`), and the Japanese words for password,
    // PIN, API key, access key, secret key, secret, and token
    // with a full-width colon. The pattern stops after the opening quote;
    // `Redactor::mask_assignments` finds the value and `name` decides. It
    // runs last, so a value another rule masked starts with `***` and stays.
    (
        r#"(^|[^A-Za-z0-9_.])((?:\\?["'])?([A-Za-z_][A-Za-z0-9_.-]*|\x{30D1}\x{30B9}\x{30EF}\x{30FC}\x{30C9}|\x{6697}\x{8A3C}\x{756A}\x{53F7}|API\x{30AD}\x{30FC}|\x{30A2}\x{30AF}\x{30BB}\x{30B9}\x{30AD}\x{30FC}|\x{30B7}\x{30FC}\x{30AF}\x{30EC}\x{30C3}\x{30C8}\x{30AD}\x{30FC}|\x{30B7}\x{30FC}\x{30AF}\x{30EC}\x{30C3}\x{30C8}|\x{30C8}\x{30FC}\x{30AF}\x{30F3})(?:\\?["'])?\]?[\t ]*(?:=>|[=:\x{FF1A}])[\t ]*(\\?["'])?)"#,
        Replacement::Assignment,
    ),
];

/// The most bytes of a value read before deciding whether to mask it. A
/// value that is masked is then read to its end; one that is not costs at
/// most this much, which keeps the rule linear in the text.
const MAX_VALUE_SCAN: usize = 1024;

/// What a rule puts in place of its match.
#[derive(Clone, Copy)]
enum Replacement {
    /// The value after the match, when `name` says the name and the value
    /// are a credential.
    Assignment,
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
    /// How a match is replaced.
    kind: Replacement,
    /// The secret to find.
    pattern: Regex,
    /// The replacement text, in `regex` replacement syntax, for
    /// `KeepPrefix` and `Marker`; empty for the kinds that build their own.
    replacement: String,
}

/// The compiled masking rules, applied in order.
pub struct Redactor {
    /// Each later rule sees the output of the earlier ones.
    rules: Vec<Rule>,
}

impl Redactor {
    /// Masks the credential values after the matches of `pattern`, the
    /// assignment rule, in `text`.
    ///
    /// A match whose name is not a credential is skipped only up to the end
    /// of its name, without reading its value: in
    /// `https://host/v1?access_token=...` or `--env=DB_PASSWORD=...` the
    /// first name is `https` or `env`, and the credential after it must
    /// still be found.
    fn mask_assignments(pattern: &Regex, text: &str) -> String {
        let mut masked = String::with_capacity(text.len());
        let mut copied = 0;
        let mut from = 0;
        while let Some(captures) = pattern.captures_at(text, from) {
            let (Some(prefix), Some(name)) = (captures.get(2), captures.get(3)) else {
                break;
            };
            from = name.end();
            if !name::is_credential_name(name.as_str()) {
                continue;
            }
            let rest = text.get(prefix.end()..).unwrap_or_default();
            let quote = captures.get(4).map(|found| found.as_str());
            let head = value_of(rest, quote, MAX_VALUE_SCAN);
            if !name::masks(name.as_str(), head) {
                continue;
            }
            let value = if head.len() >= MAX_VALUE_SCAN {
                value_of(rest, quote, usize::MAX)
            } else {
                head
            };
            masked.push_str(text.get(copied..prefix.end()).unwrap_or_default());
            masked.push_str(MARKER);
            copied = prefix.end().saturating_add(value.len());
            from = copied;
        }
        masked.push_str(text.get(copied..).unwrap_or_default());
        masked
    }

    /// Compiles the rules.
    ///
    /// The patterns are fixed, so an error here is a bug in this file; a
    /// test compiles them so it surfaces before release.
    pub fn new() -> Result<Self, regex::Error> {
        let rules = RULES
            .iter()
            .map(|&(pattern, replacement)| {
                Ok(Rule {
                    kind: replacement,
                    pattern: Regex::new(pattern)?,
                    replacement: match replacement {
                        Replacement::Marker => MARKER.to_owned(),
                        Replacement::KeepPrefix => format!("${{1}}{MARKER}"),
                        Replacement::Assignment | Replacement::UrlPassword => String::new(),
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
            masked = match rule.kind {
                Replacement::Assignment => Self::mask_assignments(&rule.pattern, &masked),
                Replacement::UrlPassword => rule
                    .pattern
                    .replace_all(&masked, |captures: &Captures<'_>| {
                        let group = |index| captures.get(index).map_or("", |found| found.as_str());
                        if is_port_then_query(group(2)) {
                            group(0).to_owned()
                        } else {
                            format!("{}{MARKER}@", group(1))
                        }
                    })
                    .into_owned(),
                Replacement::KeepPrefix | Replacement::Marker => rule
                    .pattern
                    .replace_all(&masked, rule.replacement.as_str())
                    .into_owned(),
            };
        }
        let changed = masked != text;
        (masked, changed)
    }

    /// Masks secrets in every string inside `value`, such as a tool's input,
    /// and reports whether it changed anything. A string under a key that
    /// names a credential, as in `{"password": "..."}`, is masked whole by
    /// the same test as an assignment. Object keys stay as they are.
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
                for (key, field) in fields.iter_mut() {
                    if let Value::String(ref mut text) = *field
                        && key.is_ascii()
                        && name::masks(key, text)
                    {
                        MARKER.clone_into(text);
                        changed = true;
                    } else {
                        changed |= self.redact_json(field);
                    }
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

/// The value at the start of `rest`, read through at most `limit` bytes.
///
/// After an opening `quote` it runs to the matching close on the same
/// line, so a passphrase with spaces is masked whole. A quote followed by
/// a space closes something else, as in `grep '^TOKEN=' | cut`, and is not
/// an opening one. Without a quote, or when the close is not found, it
/// runs over printable ASCII up to a quote,
/// a backtick, `,`, `;`, `)`, `}`, `]`, or `&`, so prose after the
/// separator and the next query parameter stay.
fn value_of<'text>(rest: &'text str, quote: Option<&str>, limit: usize) -> &'text str {
    let window = rest.get(..rest.floor_char_boundary(limit)).unwrap_or(rest);
    if let Some(close) = quote {
        let line = window.split('\n').next().unwrap_or_default();
        if !line.starts_with([' ', '\t'])
            && let Some(end) = line.find(close)
        {
            return line.get(..end).unwrap_or_default();
        }
    }
    let end = window
        .bytes()
        .take_while(|&byte| (0x21..=0x7E).contains(&byte) && !b"\"'`,;)}]&".contains(&byte))
        .count();
    window.get(..end).unwrap_or_default()
}

#[cfg(test)]
mod tests;
