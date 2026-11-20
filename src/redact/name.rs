//! Whether a name holds a credential, and whether the value assigned to it
//! is one, for the rule that masks `name = value` and its relatives and for
//! the keys of a tool's JSON input.
//!
//! A name is split into parts at `_`, `.`, `-`, and camelCase humps. A
//! strong word such as `token` makes any name a credential name, as does a
//! part that ends with one (`PGPASSWORD`); a weak word such as `key` does so
//! only next to another part, so `key: "Escape"` stays while `api_key` is
//! masked. Parts that say the value is a path, a URL, or a code identifier
//! win over both.

use core::mem;

/// Parts that make a name a credential name on their own.
const STRONG_WORDS: &[&str] = &[
    "apikey",
    "credential",
    "dsn",
    "otp",
    "passcode",
    "passphrase",
    "passwd",
    "password",
    "pwd",
    "salt",
    "secret",
    "secretkey",
    "token",
];

/// Strong words that also count at the end of a longer part, as in
/// `PGPASSWORD`, `ACCESSTOKEN`, or `clientsecret`. The short ones are left
/// out so that words such as `cwd` do not count.
const STRONG_SUFFIXES: &[&str] = &[
    "apikey",
    "credential",
    "passcode",
    "passphrase",
    "passwd",
    "password",
    "secret",
    "token",
];

/// Parts that make a name a credential name only next to another part.
/// Plurals such as `secrets` are here because they usually name a
/// container, as in `secrets:`.
const WEAK_WORDS: &[&str] = &[
    "auth",
    "credentials",
    "hook",
    "hooks",
    "key",
    "keys",
    "pass",
    "passwords",
    "private",
    "secrets",
    "tokens",
    "webhook",
];

/// Weak words behind which a value of letters or digits alone is still a
/// password, as in `SMTP_PASS=hunterbunny`. Behind the other weak words,
/// such as `key`, `hook`, and `tokens` (`max_tokens: 4096`), such a value is
/// a word or a count.
const STRICT_WEAK_WORDS: &[&str] = &[
    "auth",
    "credentials",
    "pass",
    "passwords",
    "private",
    "secrets",
];

/// Parts that say the value is not a credential, even next to a strong
/// word: paths, URLs, error and function names, and file extensions.
const NEVER_SECRET_PARTS: &[&str] = &[
    "arn",
    "configure",
    "dir",
    "error",
    "exception",
    "file",
    "files",
    "invalid",
    "js",
    "md",
    "path",
    "paths",
    "py",
    "rb",
    "sh",
    "source",
    "ts",
    "uri",
    "url",
    "validate",
    "yaml",
    "yml",
];

/// Parts that take a weak word out of credentials, as in `primary_key`,
/// `S3_KEY`, or `projectKey`. Next to a strong word they do not.
const WEAK_ONLY_NOT_SECRET_PARTS: &[&str] = &[
    "cache",
    "cert",
    "column",
    "composite",
    "context",
    "count",
    "debug",
    "docs",
    "field",
    "foreign",
    "id",
    "ids",
    "index",
    "issue",
    "length",
    "name",
    "object",
    "oidc",
    "pairs",
    "param",
    "params",
    "partition",
    "prefix",
    "primary",
    "project",
    "provider",
    "s3",
    "service",
    "size",
    "sort",
    "sso",
    "type",
    "unique",
];

/// Parts that make a value of digits alone a quantity, even next to a
/// strong word: `token_limit: 100000`, `AUTH_TIMEOUT_SECONDS = 300`.
const QUANTITY_PARTS: &[&str] = &[
    "count",
    "days",
    "hours",
    "interval",
    "length",
    "limit",
    "max",
    "min",
    "minutes",
    "retries",
    "seconds",
    "size",
    "threshold",
    "timeout",
    "ttl",
];

/// Values that name a type or a scheme, as in `token: string` or
/// `token_type: "Bearer"`, compared without case.
const NON_SECRET_VALUES: &[&str] = &[
    "bearer",
    "bigint",
    "boolean",
    "number",
    "object",
    "string",
    "undefined",
    "unknown",
];

/// Starts of a value that refers to one instead of holding it: shell and
/// language environment lookups, placeholders, and a value another rule
/// already masked. Escape sequences and `%` forms are checked in
/// `is_reference`.
const VALUE_REFS: &[&str] = &[
    "$",
    "***",
    "<",
    "ENV(",
    "ENV.",
    "ENV[",
    "Settings.",
    "os.environ",
    "process.env",
];

/// The characters after `\` that make a value an escape sequence, such as
/// `\u0000`, `\n`, or the `\(` of a jq string.
const ESCAPES: &[char] = &['(', '0', '\\', 'n', 'r', 't', 'u', 'x'];

/// The number of parts a name needs before a weak word counts.
const MIN_COMPOUND_PARTS: usize = 2;

/// The shortest value that is masked; shorter ones are words such as `yes`.
pub const MIN_VALUE_CHARS: usize = 6;

/// The longest segment of a code expression; a longer one is a token.
const MAX_SEGMENT_CHARS: usize = 32;

/// Splits one chunk of a name at camelCase humps, lowercased: `apiKey`
/// gives `api` and `key`, `APIKey` gives `api` and `key`, and `S3Key`
/// gives `s3` and `key`. Digits stay with the letters before them.
fn camel_words(chunk: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut previous: Option<char> = None;
    let mut chars = chunk.chars().peekable();
    while let Some(ch) = chars.next() {
        let next_is_lower = chars.peek().is_some_and(char::is_ascii_lowercase);
        let starts_word = previous.is_some_and(|before| {
            (ch.is_ascii_alphabetic() && before.is_ascii_digit())
                || (ch.is_ascii_uppercase() && before.is_ascii_lowercase())
                || (ch.is_ascii_uppercase() && before.is_ascii_uppercase() && next_is_lower)
        });
        if starts_word && !word.is_empty() {
            words.push(mem::take(&mut word));
        }
        word.push(ch.to_ascii_lowercase());
        previous = Some(ch);
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Whether any of `parts` is one of `words`.
fn has_any(parts: &[String], words: &[&str]) -> bool {
    parts.iter().any(|part| words.contains(&part.as_str()))
}

/// Whether any of `parts` is a strong word or ends with a long one.
fn has_strong(parts: &[String]) -> bool {
    has_any(parts, STRONG_WORDS)
        || parts
            .iter()
            .any(|part| STRONG_SUFFIXES.iter().any(|word| part.ends_with(word)))
}

/// Whether `value` is a code expression, which a credential does not look
/// like: an attribute (`self.api_key`), a call or index (`re.compile(`,
/// `config[`), or a `snake_case` identifier (`last_key`). Each dotted
/// segment is letters and `_` with digits only at the end, and the first
/// starts in lowercase, so `Pass.Word2024` and dotted tokens are not code.
fn is_code(value: &str) -> bool {
    let head = value.split(['(', '[']).next().unwrap_or_default();
    let called = head.len() < value.len();
    let mut segments = head.split('.');
    let first = segments.next().unwrap_or_default();
    let identifier = |segment: &str| {
        let letters = segment.trim_end_matches(|ch: char| ch.is_ascii_digit());
        !letters.is_empty()
            && segment.len() <= MAX_SEGMENT_CHARS
            && letters
                .chars()
                .all(|ch| ch.is_ascii_alphabetic() || ch == '_')
    };
    let starts_lower = first.starts_with(|ch: char| ch.is_ascii_lowercase() || ch == '_');
    let dotted = head.contains('.');
    if called || dotted {
        return starts_lower && identifier(first) && segments.all(identifier);
    }
    is_snake_case(value)
}

/// Whether `name` holds a credential, such as `api_key`,
/// `SLACK_HOOK_ACHIEVE`, or `secretKey`. A Japanese name the rule matched,
/// such as the word for password, always does.
pub fn is_credential_name(name: &str) -> bool {
    if !name.is_ascii() {
        return true;
    }
    let found = parts(name);
    if has_any(&found, NEVER_SECRET_PARTS) {
        return false;
    }
    if has_strong(&found) {
        return true;
    }
    if has_any(&found, WEAK_ONLY_NOT_SECRET_PARTS) {
        return false;
    }
    found.len() >= MIN_COMPOUND_PARTS && has_any(&found, WEAK_WORDS)
}

/// Whether `value` refers to a value instead of holding one: a variable, a
/// lookup, a placeholder, an escape sequence, or a format such as `%s`,
/// `%{key}`, `%(name)s`, or `%TOKEN%`. A percent-encoded value such as
/// `p%40ss` is not a reference.
fn is_reference(value: &str) -> bool {
    if VALUE_REFS.iter().any(|start| value.starts_with(start)) {
        return true;
    }
    if let Some(rest) = value.strip_prefix('\\') {
        return rest.starts_with(ESCAPES);
    }
    let Some(rest) = value.strip_prefix('%') else {
        return false;
    };
    rest.starts_with(['{', '('])
        || rest.split_once('%').is_some_and(|(variable, _)| {
            variable.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_')
                && variable
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        })
}

/// Whether `value` is a lowercase `snake_case` identifier, such as
/// `last_secret`.
fn is_snake_case(value: &str) -> bool {
    value.starts_with(|ch: char| ch.is_ascii_lowercase())
        && value.contains('_')
        && value.split('_').all(|word| {
            !word.is_empty()
                && word
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        })
}

/// Whether `value`, assigned to `name`, is a credential to mask.
///
/// A value that opens a list, a set, a parameter list, or a pipe is code,
/// as in `requireApiKey = (secret: string) =>` or `client_secret={...}`.
///
/// Behind a name whose only credential word is `key` or `hook`, a value of
/// letters or digits alone is a word or a count
/// (`AttributeKey: ProductName`). A value of digits alone behind a name
/// with a quantity part, such as `token_limit`, is a setting.
pub fn masks(name: &str, value: &str) -> bool {
    if value.chars().count() < MIN_VALUE_CHARS
        || is_reference(value)
        || value.starts_with(['[', '{', '(', '|'])
        || NON_SECRET_VALUES.contains(&value.to_ascii_lowercase().as_str())
        || is_code(value)
        || !is_credential_name(name)
    {
        return false;
    }
    let found = parts(name);
    let digits = value.chars().all(|ch| ch.is_ascii_digit());
    if digits && has_any(&found, QUANTITY_PARTS) {
        return false;
    }
    let strict = !name.is_ascii() || has_strong(&found) || has_any(&found, STRICT_WEAK_WORDS);
    strict || !(digits || value.chars().all(char::is_alphabetic))
}

/// The lowercased parts of `name`, split at `_`, `.`, `-`, and humps.
fn parts(name: &str) -> Vec<String> {
    name.split(['_', '.', '-']).flat_map(camel_words).collect()
}
