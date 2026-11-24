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

use alloc::collections::BTreeMap;
use core::cmp::Reverse;
use core::mem;
use std::collections::HashSet;

use aho_corasick::{AhoCorasick, MatchKind};
use regex::{Captures, Regex};

use serde_json::{Map, Value};

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

/// The rules in the order they apply, each with the name `--mask-report`
/// counts it under.
///
/// Token formats without a source comment follow the default rules of
/// gitleaks: <https://github.com/gitleaks/gitleaks/blob/master/config/gitleaks.toml>.
const RULES: [(&str, &str, Replacement); 26] = [
    // AWS credentials by variable name: export lines from the SSO portal,
    // YAML, credentials files, and JSON with snake_case keys. The value must
    // be 16+ characters so that "AWS_SESSION_TOKEN: is it required?" keeps
    // its prose.
    (
        "aws-credential",
        r#"(?i)((?:^|[^A-Za-z0-9])aws_(?:secret_access_key|session_token|access_key_id)["']?[\t\n\x0C\r ]*[:=][\t\n\x0C\r ]*["']?)[A-Za-z0-9/+=_-]{16,}"#,
        Replacement::KeepPrefix,
    ),
    // AWS credentials as printed by `aws sts` and
    // `aws configure export-credentials`.
    (
        "aws-credential",
        r#"("(?:AccessKeyId|SecretAccessKey|SessionToken)"[\t\n\x0C\r ]*:[\t\n\x0C\r ]*")[^"]{16,}"#,
        Replacement::KeepPrefix,
    ),
    // Access key IDs on their own: AKIA for long-term keys, ASIA for
    // temporary STS keys.
    // https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_identifiers.html#identifiers-prefixes
    (
        "aws-key-id",
        concat!(boundary!(), r"(?:AKIA|ASIA)[0-9A-Z]{16}(?-u:\b)"),
        Replacement::KeepPrefix,
    ),
    // GitHub tokens: ghp_ (personal), gho_ (OAuth), ghu_ (user-to-server),
    // ghs_ (server-to-server), ghr_ (refresh), and fine-grained github_pat_.
    // https://github.blog/engineering/platform-security/behind-githubs-new-authentication-token-formats/
    (
        "github-token",
        concat!(boundary!(), r"(?:gh[pousr]|github_pat)_[A-Za-z0-9_]{20,}"),
        Replacement::KeepPrefix,
    ),
    // GitLab personal access tokens.
    (
        "gitlab-token",
        concat!(boundary!(), r"glpat-[0-9A-Za-z_-]{20,}"),
        Replacement::KeepPrefix,
    ),
    // npm access tokens.
    (
        "npm-token",
        concat!("(?i)", boundary!(), r"npm_[a-z0-9]{36}"),
        Replacement::KeepPrefix,
    ),
    // Google API keys.
    (
        "google-api-key",
        concat!(boundary!(), r"AIza[0-9A-Za-z_-]{35}"),
        Replacement::KeepPrefix,
    ),
    // Anthropic (sk-ant-) and OpenAI (sk-, sk-proj-) API keys. The length
    // floor keeps short words such as "sk-1" out.
    (
        "api-key",
        concat!(boundary!(), r"sk-(?:ant-|proj-)?[A-Za-z0-9_-]{20,}"),
        Replacement::KeepPrefix,
    ),
    // Stripe secret and restricted keys.
    (
        "stripe-key",
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
        "slack-token",
        concat!(boundary!(), r"xox[abposr]-[A-Za-z0-9-]{10,}"),
        Replacement::KeepPrefix,
    ),
    (
        "slack-token",
        concat!(
            "(?i)",
            boundary!(),
            r"xapp-[0-9]-[A-Z0-9]+-[0-9]+-[a-z0-9]+"
        ),
        Replacement::KeepPrefix,
    ),
    (
        "slack-webhook",
        r"hooks\.slack\.com/services/[A-Za-z0-9+/]{43,56}",
        Replacement::Marker,
    ),
    (
        "slack-webhook",
        r"hooks\.slack\.com/(?:workflows|triggers)/[A-Za-z0-9+/]{43,}",
        Replacement::Marker,
    ),
    // SendGrid API keys.
    (
        "sendgrid-key",
        concat!(boundary!(), r"SG\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}"),
        Replacement::KeepPrefix,
    ),
    // Google OAuth access tokens (ya29.), refresh tokens (1//0), and client
    // secrets (GOCSPX-).
    (
        "google-oauth",
        concat!(boundary!(), r"ya29\.[0-9A-Za-z_-]{20,}"),
        Replacement::KeepPrefix,
    ),
    (
        "google-oauth",
        concat!(boundary!(), r"1//0[0-9A-Za-z_-]{20,}"),
        Replacement::KeepPrefix,
    ),
    (
        "google-oauth",
        concat!(boundary!(), r"GOCSPX-[0-9A-Za-z_-]{10,}"),
        Replacement::KeepPrefix,
    ),
    // Stripe webhook signing secrets.
    (
        "stripe-webhook-secret",
        concat!(boundary!(), r"whsec_[0-9A-Za-z]{10,}"),
        Replacement::KeepPrefix,
    ),
    // The credentials of an Authorization header. "<token>" and other
    // placeholders do not match the value class and stay.
    (
        "authorization",
        r#"(?i)((?:^|[^A-Za-z0-9])authorization["']?[\t ]*[:=][\t ]*["']?(?:bearer|basic|token)[\t ]+)[A-Za-z0-9._~+/=-]{8,}"#,
        Replacement::KeepPrefix,
    ),
    // A bearer token without the header name, in any case. The length
    // floor keeps prose such as "Bearer authentication" out.
    (
        "bearer",
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
        "mysql-password",
        r#"((?:^|[\t\n ;&|(]|bin/)(?:mysql|mysqldump|mysqladmin|mariadb)(?-u:\b)[^\n|;&]*?[\t ]-p)["']?[A-Za-z0-9!#%*+./=?@^_~][\x21-\x7E]*"#,
        Replacement::KeepPrefix,
    ),
    // JSON Web Tokens: two base64url "ey..." segments and a signature.
    (
        "jwt",
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
        "url-password",
        r"(://[^/\t\n\x0C\r :@?#]*:)([^\t\n\x0C\r /]*)@",
        Replacement::UrlPassword,
    ),
    // PEM and PGP private keys, through the END line or, when the paste was
    // cut off, to the end of the text.
    (
        "private-key",
        r"(?s)-----BEGIN [A-Z0-9 _-]*PRIVATE KEY(?: BLOCK)?-----.*?(?:-----END [A-Z0-9 _-]*PRIVATE KEY(?: BLOCK)?-----|\z)",
        Replacement::Marker,
    ),
    // A value assigned to a name, as below, for the Japanese words for
    // password, PIN, API key, access key, secret key, secret, and token, also
    // with a full-width colon. No boundary is needed before them: Japanese is
    // written without spaces, so the name may follow a value or an ASCII word
    // directly. It runs before the ASCII names, whose unquoted value would
    // otherwise read the ASCII part of a name that follows it, as `API` in
    // the word for API key. The empty first group keeps the groups where the
    // rule below has them.
    (
        "credential-assignment",
        r#"()((?:\\?["'])?(\x{30D1}\x{30B9}\x{30EF}\x{30FC}\x{30C9}|\x{6697}\x{8A3C}\x{756A}\x{53F7}|API\x{30AD}\x{30FC}|\x{30A2}\x{30AF}\x{30BB}\x{30B9}\x{30AD}\x{30FC}|\x{30B7}\x{30FC}\x{30AF}\x{30EC}\x{30C3}\x{30C8}\x{30AD}\x{30FC}|\x{30B7}\x{30FC}\x{30AF}\x{30EC}\x{30C3}\x{30C8}|\x{30C8}\x{30FC}\x{30AF}\x{30F3})(?:\\?["'])?\]?[\t ]*(?:=>|[=:\x{FF1A}])[\t ]*(\\?["'])?)"#,
        Replacement::Assignment,
    ),
    // A value assigned to a name: `name = value`, `name: value`,
    // `name => value`, `--name=value`, `?name=value`, and the same inside
    // escaped JSON (`\"password\":\"...\"`). The pattern stops after the
    // opening quote; `Redactor::mask_assignments` finds the value and `name`
    // decides. The two assignment rules run last, so a value another rule
    // masked starts with `***` and stays.
    (
        "credential-assignment",
        r#"(^|[^A-Za-z0-9_.])((?:\\?["'])?([A-Za-z_][A-Za-z0-9_.-]*)(?:\\?["'])?\]?[\t ]*(?:=>|[=:\x{FF1A}])[\t ]*(\\?["'])?)"#,
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

/// AWS account IDs where the context says they are one: in an ARN, assigned
/// to an account, owner, or `AWS` principal name, in an ECR, S3, or console
/// sign-in host, and after an account or owner flag. The text before the ID
/// is `${1}` and the text after it, when the pattern needs it, is `${3}`; a
/// run of 13 digits is not an ID.
const AWS_ACCOUNTS: [&str; 4] = [
    "(arn:aws[a-z-]*:[a-z0-9-]*:[a-z0-9-]*:)([0-9]{12})(:)",
    r#"(?i)((?:^|[^A-Za-z0-9_])["']?(?:(?:aws_?)?account(?:[-_]?id)?|owner[-_]?id|aws)["']?[\t ]*[:=][\t ]*["']?)([0-9]{12})(?-u:\b)"#,
    r"(^|[^0-9A-Za-z])([0-9]{12})(\.(?:dkr\.ecr|s3-accesspoint|s3-control|signin\.aws)\.)",
    r"((?:^|[\t ])--(?:account-ids?|account|owner-id|owners|source-account)(?:=|[\t ]+))([0-9]{12})(?-u:\b)",
];

/// AWS profile names where the context says they are one: after
/// `--profile`, assigned to `AWS_PROFILE`, `AWS_DEFAULT_PROFILE`, or a
/// `profile` key at the start of a line or in an object, in a
/// `[profile ...]` or `[sso-session ...]` header or a credentials file
/// header followed by an `aws_` key, and as a `source_profile`. The text
/// before the name is `${1}` and after it `${3}`, as in `AWS_ACCOUNTS`.
const AWS_PROFILES: [&str; 6] = [
    r#"((?:^|[\t ])--profile(?:=|[\t ]+)["']?)([A-Za-z0-9_.@+-]+)"#,
    r#"((?:^|[^A-Za-z0-9_])AWS_(?:DEFAULT_)?PROFILE["']?[\t ]*[:=][\t ]*["']?)([A-Za-z0-9_.@+-]+)"#,
    r#"(?m)((?:^|[\t ,{])["']?profile["']?[\t ]*[:=][\t ]*["']?)([A-Za-z0-9_.@+-]+)"#,
    r"(\[(?:profile|sso-session)[\t ]+)([^\]\t\n\r ]+)(\])",
    r"(?m)(^\[)([A-Za-z0-9_.@+-]+)(\][\t ]*\r?\n[\t ]*aws_[a-z_]+[\t ]*=)",
    r"(?i)((?:^|[^A-Za-z0-9_])source_profile[\t ]*=[\t ]*)([A-Za-z0-9_.@+-]+)",
];

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

/// How many times each rule masked something, by the rule's name.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Counts(BTreeMap<String, usize>);

/// One pattern and its replacement.
struct Rule {
    /// How a match is replaced.
    kind: Replacement,
    /// The name it is counted under.
    name: &'static str,
    /// The secret to find.
    pattern: Regex,
    /// The replacement text, in `regex` replacement syntax, for
    /// `KeepPrefix` and `Marker`; empty for the kinds that build their own.
    replacement: String,
}

/// The masks that are off unless asked for with `--mask` and
/// `--mask-values`.
#[derive(Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "--mask turns each kind on on its own, so each is one switch"
)]
pub struct ExtraMasks {
    /// Mask AWS account IDs in context.
    pub aws_accounts: bool,
    /// Mask AWS profile names in context.
    pub aws_profiles: bool,
    /// Mask email addresses.
    pub emails: bool,
    /// The git user name and email to mask wherever they appear, in any
    /// case.
    pub git_users: Vec<String>,
    /// Mask IP addresses.
    pub ips: bool,
    /// The user name to mask, with any of `.`, `_`, and `-` between its
    /// parts and in any case, as in a path or a project directory name.
    pub user: Option<String>,
    /// Values to mask wherever they appear, each with what replaces it and
    /// its label, empty for none.
    pub values: Vec<(String, String, String)>,
}

/// The compiled masks that `ExtraMasks` asks for.
struct ExtraRules {
    /// AWS account IDs and profile names in context, each with its name and
    /// the text around the value as `${1}` and `${3}`.
    aws: Vec<(&'static str, Regex)>,
    /// Email addresses; an SSH remote such as `git@github.com:alice/x`
    /// stays.
    email: Option<Regex>,
    /// The git user name and email, with the boundary as `${1}`.
    git_user: Option<Regex>,
    /// IPv4 and IPv6 addresses.
    ip: Option<(Regex, Regex)>,
    /// The user name, with its boundary as `${1}`.
    user: Option<Regex>,
    /// The values, matched longest first so one that holds another is
    /// masked whole, and what replaces each and the name it is counted
    /// under, by pattern.
    values: Option<(AhoCorasick, Vec<(String, String)>)>,
}

/// The compiled masking rules, applied in order.
pub struct Redactor {
    /// The masks asked for, applied after the rules.
    extra: ExtraRules,
    /// Each later rule sees the output of the earlier ones.
    rules: Vec<Rule>,
}

impl Counts {
    /// Counts one mask by the rule named `rule`.
    pub fn add(&mut self, rule: &str) {
        let count = self.0.entry(rule.to_owned()).or_default();
        *count = count.saturating_add(1);
    }

    /// Adds the counts in `other`.
    pub fn merge(&mut self, other: &Self) {
        for (rule, &count) in &other.0 {
            let total = self.0.entry(rule.clone()).or_default();
            *total = total.saturating_add(count);
        }
    }

    /// The rules and their counts, most first, then by name.
    pub fn ranked(&self) -> Vec<(String, usize)> {
        let mut ranked: Vec<(String, usize)> = self
            .0
            .iter()
            .map(|(rule, &count)| (rule.clone(), count))
            .collect();
        ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        ranked
    }
}

impl ExtraRules {
    /// Applies the masks to `text`: email and IP addresses first, so that
    /// a user name or a value inside one cannot split it, then the AWS
    /// contexts and the values around the markers already placed, and the
    /// user names last, so a value holding one is replaced whole.
    fn apply(&self, text: &str, counts: &mut Counts) -> String {
        let mut masked = text.to_owned();
        if let Some(ref pattern) = self.email {
            masked = replace_unless_followed(
                pattern,
                &masked,
                |_, after| is_ssh_path(after),
                ("email", counts),
            );
        }
        if let Some((ref v4, ref v6)) = self.ip {
            masked = replace_unless_followed(
                v4,
                &masked,
                |address, after| {
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
                },
                ("ip", counts),
            );
            masked = replace_unless_followed(
                v6,
                &masked,
                |address, after| {
                    after.starts_with(|ch: char| ch.is_ascii_hexdigit() || ch == ':')
                        || address.split(':').filter(|group| !group.is_empty()).count()
                            < MIN_IPV6_GROUPS
                },
                ("ip", counts),
            );
        }
        for &(name, ref pattern) in &self.aws {
            masked = pattern
                .replace_all(&masked, |captures: &Captures<'_>| {
                    counts.add(name);
                    let mut out = String::new();
                    captures.expand(&format!("${{1}}{MARKER}${{3}}"), &mut out);
                    out
                })
                .into_owned();
        }
        if let Some((ref values, ref replacements)) = self.values {
            masked = replace_values(values, replacements, &masked, counts);
        }
        for (name, pattern) in [("git-user", &self.git_user), ("user", &self.user)] {
            if let Some(ref found) = *pattern {
                masked = replace_unless_followed(
                    found,
                    &masked,
                    |_, after| after.starts_with(|ch: char| ch.is_ascii_alphanumeric()),
                    (name, counts),
                );
            }
        }
        masked
    }

    /// Compiles the masks `extra` asks for.
    fn new(extra: ExtraMasks) -> Result<Self, regex::Error> {
        let mut seen = HashSet::new();
        let (values, replacements): (Vec<String>, Vec<(String, String)>) = extra
            .values
            .into_iter()
            .filter(|triple| !triple.0.is_empty() && seen.insert(triple.0.clone()))
            .map(|(value, replacement, label)| {
                let name = if label.is_empty() {
                    "values".to_owned()
                } else {
                    format!("values/{label}")
                };
                (value, (replacement, name))
            })
            .unzip();
        let mut aws = Vec::new();
        for (on, name, patterns) in [
            (extra.aws_accounts, "aws-account", AWS_ACCOUNTS.as_slice()),
            (extra.aws_profiles, "aws-profile", AWS_PROFILES.as_slice()),
        ] {
            if on {
                for pattern in patterns {
                    aws.push((name, Regex::new(pattern)?));
                }
            }
        }
        Ok(Self {
            aws,
            email: extra.emails.then(|| Regex::new(EMAIL)).transpose()?,
            git_user: names_pattern(extra.git_users)
                .map(|pattern| Regex::new(&pattern))
                .transpose()?,
            ip: if extra.ips {
                Some((Regex::new(IPV4)?, Regex::new(IPV6)?))
            } else {
                None
            },
            user: extra
                .user
                .filter(|name| !name.is_empty())
                .map(|name| Regex::new(&user_pattern(&name)))
                .transpose()?,
            values: if values.is_empty() {
                None
            } else {
                // Only a pattern too large for memory fails, and the values
                // then stay unmasked by none of them: fail the run instead.
                Some((
                    AhoCorasick::builder()
                        .match_kind(MatchKind::LeftmostLongest)
                        .build(&values)
                        .map_err(|error| regex::Error::Syntax(error.to_string()))?,
                    replacements,
                ))
            },
        })
    }
}

impl Redactor {
    /// Applies the built-in rules to `text`, counting each in `counts`.
    fn apply_rules(&self, text: &str, counts: &mut Counts) -> String {
        let mut masked = text.to_owned();
        for rule in &self.rules {
            masked = match rule.kind {
                Replacement::Assignment => {
                    Self::mask_assignments(&rule.pattern, &masked, (rule.name, counts))
                }
                Replacement::UrlPassword => rule
                    .pattern
                    .replace_all(&masked, |captures: &Captures<'_>| {
                        let group = |index| captures.get(index).map_or("", |found| found.as_str());
                        if is_port_then_query(group(2)) {
                            group(0).to_owned()
                        } else {
                            counts.add(rule.name);
                            format!("{}{MARKER}@", group(1))
                        }
                    })
                    .into_owned(),
                Replacement::KeepPrefix | Replacement::Marker => rule
                    .pattern
                    .replace_all(&masked, |captures: &Captures<'_>| {
                        counts.add(rule.name);
                        let mut out = String::new();
                        captures.expand(&rule.replacement, &mut out);
                        out
                    })
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
    fn mask_assignments(
        pattern: &Regex,
        text: &str,
        (rule, counts): (&str, &mut Counts),
    ) -> String {
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
            counts.add(rule);
            copied = prefix.end().saturating_add(value.len());
            from = copied;
        }
        masked.push_str(text.get(copied..).unwrap_or_default());
        masked
    }

    /// Applies only the masks asked for to `field`, such as a path, a
    /// branch, or a tool's name, where the built-in rules do not belong.
    #[cfg(test)]
    pub fn meta(&self, field: &str) -> String {
        self.meta_counted(field, &mut Counts::default())
    }

    /// `meta`, counting what it masked in `counts`.
    pub fn meta_counted(&self, field: &str, counts: &mut Counts) -> String {
        self.extra.apply(field, counts)
    }

    /// Compiles the rules.
    ///
    /// The patterns are fixed, so an error here is a bug in this file; a
    /// test compiles them so it surfaces before release.
    #[cfg(test)]
    pub fn new() -> Result<Self, regex::Error> {
        Self::with(ExtraMasks::default())
    }

    /// Masks secrets in `text` and reports whether it changed anything.
    #[cfg(test)]
    pub fn redact(&self, text: &str) -> (String, bool) {
        self.redact_counted(text, &mut Counts::default())
    }

    /// `redact`, counting what it masked in `counts`.
    pub fn redact_counted(&self, text: &str, counts: &mut Counts) -> (String, bool) {
        // The rules first, so a label a mask puts inside a secret cannot
        // stop an assignment's value short and leave the rest.
        let masked = self.extra.apply(&self.apply_rules(text, counts), counts);
        let changed = masked != text;
        (masked, changed)
    }

    /// Masks secrets in every string inside `value`, such as a tool's input,
    /// and reports whether it changed anything. A string under a key that
    /// names a credential, as in `{"password": "..."}`, is masked whole by
    /// the same test as an assignment. Object keys are masked as strings
    /// are; a key that masking makes equal to an earlier one gets `#2`,
    /// `#3`, and so on, so no value is lost.
    #[cfg(test)]
    pub fn redact_json(&self, value: &mut Value) -> bool {
        self.redact_json_counted(value, &mut Counts::default())
    }

    /// `redact_json`, counting what it masked in `counts`; a string masked
    /// whole for its key counts as `credential-key`.
    pub fn redact_json_counted(&self, value: &mut Value, counts: &mut Counts) -> bool {
        match *value {
            Value::String(ref mut text) => {
                let (masked, changed) = self.redact_counted(text, counts);
                if changed {
                    *text = masked;
                }
                changed
            }
            Value::Array(ref mut items) => {
                let mut changed = false;
                for item in items {
                    changed |= self.redact_json_counted(item, counts);
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
                        counts.add("credential-key");
                        changed = true;
                    } else {
                        changed |= self.redact_json_counted(field, counts);
                    }
                }
                // A key gets every rule a string does, each key once so
                // nothing is counted twice, and the object is rebuilt only
                // when a key changes.
                let shown: Vec<String> = fields
                    .keys()
                    .map(|key| self.redact_counted(key, counts).0)
                    .collect();
                if shown
                    .iter()
                    .zip(fields.keys())
                    .any(|(masked, key)| masked != key)
                {
                    let mut masked = Map::with_capacity(fields.len());
                    for ((_, field), key) in mem::take(fields).into_iter().zip(shown) {
                        masked.insert(unused_key(&masked, key), field);
                    }
                    *fields = masked;
                    changed = true;
                }
                changed
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => false,
        }
    }

    /// Compiles the rules and the masks `extra` asks for.
    ///
    /// # Errors
    ///
    /// When a fixed pattern does not compile, which is a bug in this file.
    pub fn with(extra: ExtraMasks) -> Result<Self, regex::Error> {
        let rules = RULES
            .iter()
            .map(|&(name, pattern, replacement)| {
                Ok(Rule {
                    kind: replacement,
                    name,
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
            extra: ExtraRules::new(extra)?,
            rules,
        })
    }
}

/// `key`, or when `fields` already has it, `key` followed by the first of
/// `#2`, `#3`, and so on that it does not have.
fn unused_key(fields: &Map<String, Value>, key: String) -> String {
    if !fields.contains_key(&key) {
        return key;
    }
    // Of the numbers up to one past the keys there, one is always free.
    (2..=fields.len().saturating_add(1))
        .map(|number| format!("{key}#{number}"))
        .find(|numbered| !fields.contains_key(numbered))
        .unwrap_or(key)
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

/// Replaces each match of `values` in `text` with its entry in
/// `replacements`, except one that overlaps a marker already placed, so a
/// value such as `RED` does not break it.
fn replace_values(
    values: &AhoCorasick,
    replacements: &[(String, String)],
    text: &str,
    counts: &mut Counts,
) -> String {
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
        let replacement = replacements.get(found.pattern().as_usize());
        out.push_str(replacement.map_or(MARKER, |pair| pair.0.as_str()));
        counts.add(replacement.map_or("values", |pair| pair.1.as_str()));
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
    (rule, counts): (&str, &mut Counts),
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
                counts.add(rule);
                format!("{boundary}{MARKER}")
            }
        })
        .into_owned()
}

/// The pattern for any of `names`, longest first so that one holding
/// another is masked whole: in any case, after the start or a character
/// that is not a letter or a digit, captured as `${1}`. `None` when there
/// are no names.
fn names_pattern(mut names: Vec<String>) -> Option<String> {
    names.retain(|name| !name.is_empty());
    if names.is_empty() {
        return None;
    }
    names.sort_by_key(|name| Reverse(name.len()));
    let body: Vec<String> = names.iter().map(|name| regex::escape(name)).collect();
    Some(format!("(?i)(^|[^0-9A-Za-z])({})", body.join("|")))
}

/// The pattern for the user name `name`: case-insensitive, with any of `.`,
/// `_`, and `-` where it has one, after the start or a character that is
/// not a letter or a digit, captured as `${1}`.
fn user_pattern(name: &str) -> String {
    let body: String = name
        .chars()
        .map(|ch| {
            if matches!(ch, '.' | '_' | '-') {
                "[._-]".to_owned()
            } else {
                regex::escape(ch.encode_utf8(&mut [0; 4]))
            }
        })
        .collect();
    format!("(?i)(^|[^0-9A-Za-z])({body})")
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
