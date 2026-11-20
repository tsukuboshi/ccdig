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

use aho_corasick::{AhoCorasick, MatchKind};
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
const RULES: [(&str, Replacement); 25] = [
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
    // Slack tokens, app-level tokens, and incoming webhook URLs. The path of
    // a workflow or trigger URL has no upper bound, as it runs longer than
    // the 56 characters gitleaks allows; an incoming webhook's stays at 56 so
    // that the text after it is kept.
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
        r"hooks\.slack\.com/services/[A-Za-z0-9+/]{43,56}",
        Replacement::Marker,
    ),
    (
        r"hooks\.slack\.com/(?:workflows|triggers)/[A-Za-z0-9+/]{43,}",
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

/// An IPv4 address with its boundary character as `${1}`; the character
/// after it is checked in code, as `regex` has no lookahead.
const IPV4: &str = concat!(
    r"(^|[^0-9A-Za-z.])",
    r"((?:25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])",
    r"(?:\.(?:25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])){3})"
);

/// An IPv6 address, full or with `::`, with its boundary as `${1}`.
const IPV6: &str = concat!(
    r"(?i)(^|[^0-9a-z:])",
    r"((?:[0-9a-f]{1,4}:){7}[0-9a-f]{1,4}",
    r"|(?:[0-9a-f]{1,4}:){1,6}(?::[0-9a-f]{1,4}){1,6})"
);

/// An email address.
const EMAIL: &str = r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}";

/// The fewest groups of an IPv6 address that is masked, so that `a::b`
/// and `std::fs` stay.
const MIN_IPV6_GROUPS: usize = 3;

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

/// The masks that are off unless asked for with `--mask` and `--mask-file`.
#[derive(Debug, Default)]
pub struct ExtraMasks {
    /// Mask email addresses.
    pub emails: bool,
    /// The home directory to shorten to `~`, in paths and in project
    /// directory names.
    pub home: Option<String>,
    /// Mask IP addresses.
    pub ips: bool,
    /// Values to mask wherever they appear.
    pub values: Vec<String>,
}

/// The compiled masks that `ExtraMasks` asks for.
struct ExtraRules {
    /// Email addresses; an SSH remote such as `git@github.com:alice/x`
    /// stays.
    email: Option<Regex>,
    /// The home directory and its project directory form, each to `~`.
    home: Option<(String, String)>,
    /// IPv4 and IPv6 addresses.
    ip: Option<(Regex, Regex)>,
    /// The values, matched longest first so one that holds another is
    /// masked whole.
    values: Option<AhoCorasick>,
}

/// The compiled masking rules, applied in order.
pub struct Redactor {
    /// Whether the built-in rules apply, including the credential keys of
    /// a tool's input.
    built_in: bool,
    /// The masks asked for, applied before the rules.
    extra: ExtraRules,
    /// Each later rule sees the output of the earlier ones.
    rules: Vec<Rule>,
}

impl ExtraMasks {
    /// Whether no mask is asked for.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        !self.emails && self.home.is_none() && !self.ips && self.values.is_empty()
    }
}

impl ExtraRules {
    /// Applies the masks to `text`: the home directory first, then email
    /// and IP addresses, so that a value inside one cannot split it, and
    /// the values last, around the markers already placed.
    fn apply(&self, text: &str) -> String {
        let mut masked = self.shorten_home(text.to_owned());
        if let Some(ref pattern) = self.email {
            masked = replace_unless_followed(pattern, &masked, |_, after| is_ssh_path(after));
        }
        if let Some((ref v4, ref v6)) = self.ip {
            masked = replace_unless_followed(v4, &masked, |address, after| {
                let mut next = after.chars();
                let continues = match next.next() {
                    Some('.') => next.next().is_some_and(|ch| ch.is_ascii_digit()),
                    Some(ch) => ch.is_ascii_digit(),
                    None => false,
                };
                continues
                    || address.starts_with("127.")
                    || address == "0.0.0.0"
                    || address == "255.255.255.255"
            });
            masked = replace_unless_followed(v6, &masked, |address, after| {
                after.starts_with(|ch: char| ch.is_ascii_hexdigit() || ch == ':')
                    || address.split(':').filter(|group| !group.is_empty()).count()
                        < MIN_IPV6_GROUPS
            });
        }
        match self.values {
            Some(ref values) => replace_values(values, &masked),
            None => masked,
        }
    }

    /// Compiles the masks `extra` asks for.
    fn new(extra: ExtraMasks) -> Result<Self, regex::Error> {
        let values: Vec<String> = extra
            .values
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect();
        let trimmed = extra
            .home
            .map(|home| home.trim_end_matches('/').to_owned())
            .filter(|home| !home.is_empty());
        Ok(Self {
            email: extra.emails.then(|| Regex::new(EMAIL)).transpose()?,
            home: trimmed.map(|home| {
                let encoded = home
                    .chars()
                    .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
                    .collect();
                (home, encoded)
            }),
            ip: if extra.ips {
                Some((Regex::new(IPV4)?, Regex::new(IPV6)?))
            } else {
                None
            },
            values: if values.is_empty() {
                None
            } else {
                // Only a pattern too large for memory fails, and the values
                // then stay unmasked by none of them: fail the run instead.
                Some(
                    AhoCorasick::builder()
                        .match_kind(MatchKind::LeftmostLongest)
                        .build(&values)
                        .map_err(|error| regex::Error::Syntax(error.to_string()))?,
                )
            },
        })
    }

    /// Shortens the home directory, and its project directory form such as
    /// `-Users-alice`, to `~` where a path separator or the end follows.
    fn shorten_home(&self, text: String) -> String {
        let Some((ref home, ref encoded)) = self.home else {
            return text;
        };
        let once = shorten(&text, home, '/');
        shorten(&once, encoded, '-')
    }
}

impl Redactor {
    /// Applies the built-in rules to `text`.
    fn apply_rules(&self, text: &str) -> String {
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
        masked
    }

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

    /// Applies only the masks asked for to `field`, such as a path, a
    /// branch, or a tool's name, where the built-in rules do not belong.
    pub fn meta(&self, field: &str) -> String {
        self.extra.apply(field)
    }

    /// Compiles the rules.
    ///
    /// The patterns are fixed, so an error here is a bug in this file; a
    /// test compiles them so it surfaces before release.
    #[cfg(test)]
    pub fn new() -> Result<Self, regex::Error> {
        Self::with(true, ExtraMasks::default())
    }

    /// Masks secrets in `text` and reports whether it changed anything.
    pub fn redact(&self, text: &str) -> (String, bool) {
        let masked = self.apply_rules(&self.extra.apply(text));
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
                        && self.built_in
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

    /// Compiles the rules, or none when `built_in` is off, and the masks
    /// `extra` asks for.
    ///
    /// # Errors
    ///
    /// When a fixed pattern does not compile, which is a bug in this file.
    pub fn with(built_in: bool, extra: ExtraMasks) -> Result<Self, regex::Error> {
        let patterns = if built_in { RULES.as_slice() } else { &[] };
        let rules = patterns
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
        Ok(Self {
            built_in,
            extra: ExtraRules::new(extra)?,
            rules,
        })
    }
}

/// Whether `text`, read as a URL password, is a port number followed by a
/// query or fragment instead.
fn is_port_then_query(text: &str) -> bool {
    let rest = text.trim_start_matches(|ch: char| ch.is_ascii_digit());
    rest.len() < text.len() && rest.starts_with(['?', '#'])
}

/// Whether `after`, the text after an email address, makes it an SSH or
/// scp remote such as `git@github.com:alice/blog.git`.
fn is_ssh_path(after: &str) -> bool {
    after.strip_prefix(':').is_some_and(|path| {
        let head = path
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '~')))
            .map_or(path, |end| path.get(..end).unwrap_or_default());
        path.get(head.len()..)
            .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// Replaces each match of `values` in `text` with the marker, except one
/// that overlaps a marker already placed, so a value such as `RED` does not
/// break it.
fn replace_values(values: &AhoCorasick, text: &str) -> String {
    let markers: Vec<(usize, usize)> = text
        .match_indices(MARKER)
        .map(|(start, found)| (start, start.saturating_add(found.len())))
        .collect();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for found in values.find_iter(text) {
        let overlaps = markers
            .iter()
            .any(|&(start, end)| found.start() < end && start < found.end());
        if overlaps {
            continue;
        }
        out.push_str(text.get(copied..found.start()).unwrap_or_default());
        out.push_str(MARKER);
        copied = found.end();
    }
    out.push_str(text.get(copied..).unwrap_or_default());
    out
}

/// Replaces the matches of `pattern` in `text` with the marker, keeping the
/// boundary in `${1}` when the pattern has one, except where `keep` says the
/// match, given with the text after it, is not one to mask.
fn replace_unless_followed(
    pattern: &Regex,
    text: &str,
    keep: impl Fn(&str, &str) -> bool,
) -> String {
    pattern
        .replace_all(text, |captures: &Captures<'_>| {
            let whole = captures.get(0).map_or("", |found| found.as_str());
            let boundary = captures.get(1).map_or("", |found| found.as_str());
            let Some(found) = captures.get(2).or_else(|| captures.get(0)) else {
                return String::new();
            };
            let after = text.get(found.end()..).unwrap_or_default();
            if keep(found.as_str(), after) {
                whole.to_owned()
            } else {
                format!("{boundary}{MARKER}")
            }
        })
        .into_owned()
}

/// Replaces `prefix` in `text` with `~` where `separator` or the end of a
/// path follows it.
fn shorten(text: &str, prefix: &str, separator: char) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(prefix) {
        let after = rest
            .get(at.saturating_add(prefix.len())..)
            .unwrap_or_default();
        let continues =
            |ch: char| ch != separator && (ch.is_alphanumeric() || matches!(ch, '.' | '_' | '-'));
        let ends = !after.chars().next().is_some_and(continues);
        out.push_str(rest.get(..at).unwrap_or_default());
        out.push_str(if ends { "~" } else { prefix });
        rest = after;
    }
    out.push_str(rest);
    out
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
