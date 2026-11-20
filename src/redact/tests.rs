//! Tests for secret masking, ported from promptdig's table.

use serde_json::json;

use super::{MARKER, Redactor};
use crate::testing::{TestResult, check_eq};

/// One row of the table: a name, the input, and the expected output.
struct Case {
    /// The text given to `redact`.
    input: String,
    /// What the row checks.
    name: &'static str,
    /// The text `redact` should return.
    want: String,
}

/// A fake secret of `len` characters, assembled at run time so that no
/// token-shaped literal is committed (secret scanners would flag it).
fn fake(len: usize) -> String {
    "a1B2".chars().cycle().take(len).collect()
}

/// A GitHub token with `prefix` and a 36-character body.
fn fake_github(prefix: &str) -> String {
    format!("{prefix}_{}", fake(36))
}

/// A long-term AWS access key ID.
fn key_id() -> String {
    format!("AKIA{}", fake(16).to_ascii_uppercase())
}

/// A temporary AWS access key ID from STS.
fn temp_key_id() -> String {
    format!("ASIA{}", fake(16).to_ascii_uppercase())
}

/// A 40-character AWS secret access key with a slash.
fn secret_key() -> String {
    format!("{}/{}", fake(20), fake(19))
}

/// The first line of a PEM private key, split so scanners do not flag it.
fn pem_header() -> String {
    format!("-----BEGIN {}", "RSA PRIVATE KEY-----")
}

/// The last line of a PEM private key.
fn pem_footer() -> String {
    format!("-----END {}", "RSA PRIVATE KEY-----")
}

/// Builds a row.
fn case(name: &'static str, input: String, want: &str) -> Case {
    Case {
        input,
        name,
        want: want.to_owned(),
    }
}

/// AWS credentials by variable name, in JSON, and as bare key IDs.
fn aws_cases() -> Vec<Case> {
    vec![
        case(
            "pasted SSO export lines",
            format!(
                "export AWS_ACCESS_KEY_ID={}\nexport AWS_SECRET_ACCESS_KEY={}\nexport AWS_SESSION_TOKEN={}+/==\nlog in with these",
                temp_key_id(),
                secret_key(),
                fake(40)
            ),
            "export AWS_ACCESS_KEY_ID=***REDACTED***\nexport AWS_SECRET_ACCESS_KEY=***REDACTED***\nexport AWS_SESSION_TOKEN=***REDACTED***\nlog in with these",
        ),
        case(
            "quoted value",
            format!("AWS_SECRET_ACCESS_KEY=\"{}\"", secret_key()),
            "AWS_SECRET_ACCESS_KEY=\"***REDACTED***\"",
        ),
        case(
            "credentials file",
            format!(
                "[default]\naws_access_key_id = {}\naws_secret_access_key = {}",
                key_id(),
                secret_key()
            ),
            "[default]\naws_access_key_id = ***REDACTED***\naws_secret_access_key = ***REDACTED***",
        ),
        case(
            "bare access key ids",
            format!(
                "the key {} and the temporary key {} leaked",
                key_id(),
                temp_key_id()
            ),
            "the key ***REDACTED*** and the temporary key ***REDACTED*** leaked",
        ),
        case(
            "variable named without a value",
            "why is AWS_SESSION_TOKEN empty?".to_owned(),
            "why is AWS_SESSION_TOKEN empty?",
        ),
        case(
            "aws sts / export-credentials JSON",
            format!(
                "{{\"AccessKeyId\":\"{}\",\"SecretAccessKey\":\"{}\",\"SessionToken\":\"{}\"}}",
                temp_key_id(),
                secret_key(),
                fake(60)
            ),
            "{\"AccessKeyId\":\"***REDACTED***\",\"SecretAccessKey\":\"***REDACTED***\",\"SessionToken\":\"***REDACTED***\"}",
        ),
        case(
            "YAML colon form",
            format!("env:\n  AWS_SECRET_ACCESS_KEY: {}", secret_key()),
            "env:\n  AWS_SECRET_ACCESS_KEY: ***REDACTED***",
        ),
        case(
            "quoted lowercase JSON key",
            format!("{{\"aws_secret_access_key\": \"{}\"}}", secret_key()),
            "{\"aws_secret_access_key\": \"***REDACTED***\"}",
        ),
        case(
            "prefixed variable name",
            format!("TF_VAR_aws_secret_access_key={}", secret_key()),
            "TF_VAR_aws_secret_access_key=***REDACTED***",
        ),
        case(
            "colon followed by prose is not a value",
            "AWS_SESSION_TOKEN: is it required? What is a SessionToken?".to_owned(),
            "AWS_SESSION_TOKEN: is it required? What is a SessionToken?",
        ),
    ]
}

/// Tokens recognized by their prefix.
fn token_cases() -> Vec<Case> {
    vec![
        case(
            "GitHub tokens of every prefix",
            format!(
                "pat {} oauth {} app {} fine-grained github_pat_{}_{}",
                fake_github("ghp"),
                fake_github("gho"),
                fake_github("ghs"),
                fake(22),
                fake(59)
            ),
            "pat ***REDACTED*** oauth ***REDACTED*** app ***REDACTED*** fine-grained ***REDACTED***",
        ),
        case(
            "Anthropic and OpenAI keys",
            format!(
                "claude sk-ant-api03-{} openai sk-proj-{}",
                fake(40),
                fake(40)
            ),
            "claude ***REDACTED*** openai ***REDACTED***",
        ),
        case(
            "Slack token",
            format!("bot token xoxb-{}-{}", fake(12), fake(24)),
            "bot token ***REDACTED***",
        ),
        case(
            "GitHub token glued to a variable name",
            format!("MY_{}", fake_github("ghp")),
            "MY_***REDACTED***",
        ),
        case(
            "GitLab, npm, Google, and Stripe keys",
            format!(
                "glpat-{} npm_{} AIza{} sk_live_{}",
                fake(20),
                fake(36).to_ascii_lowercase(),
                fake(35),
                fake(24)
            ),
            "***REDACTED*** ***REDACTED*** ***REDACTED*** ***REDACTED***",
        ),
        case(
            "Slack app token and webhook",
            format!(
                "xapp-1-A0B1C2-1234567890-{} https://hooks.slack.com/services/{}",
                fake(20).to_ascii_lowercase(),
                fake(44)
            ),
            "***REDACTED*** https://***REDACTED***",
        ),
        case(
            "JWT",
            format!("token eyJ{}.eyJ{}.{} expired", fake(20), fake(20), fake(20)),
            "token ***REDACTED*** expired",
        ),
        case(
            "tokens glued to an underscore",
            format!(
                "MY_{} x_sk-ant-api03-{} MY_glpat-{} X_xoxb-{}",
                key_id(),
                fake(40),
                fake(20),
                fake(12)
            ),
            "MY_***REDACTED*** x_***REDACTED*** MY_***REDACTED*** X_***REDACTED***",
        ),
    ]
}

/// Passwords in URLs and private key blocks.
fn url_cases() -> Vec<Case> {
    vec![
        case(
            "private key block",
            format!(
                "use this key:\n{}\nMIIEowIBAAKCAQEA{}\n{}\nthanks",
                pem_header(),
                fake(40),
                pem_footer()
            ),
            "use this key:\n***REDACTED***\nthanks",
        ),
        case(
            "private key block cut off by the paste",
            format!("key:\n{}\nMIIEowIBAAKCAQEA{}", pem_header(), fake(40)),
            "key:\n***REDACTED***",
        ),
        case(
            "PGP private key block",
            format!(
                "-----BEGIN {}\n{}\n-----END {}\nok",
                "PGP PRIVATE KEY BLOCK-----",
                fake(80),
                "PGP PRIVATE KEY BLOCK-----"
            ),
            "***REDACTED***\nok",
        ),
        case(
            "password in a URL",
            format!(
                "connect to postgres://app:{}@db.example.com:5432/app",
                fake(16)
            ),
            "connect to postgres://app:***REDACTED***@db.example.com:5432/app",
        ),
        case(
            "password-only URL, as Redis uses",
            format!("redis://:{}@cache:6379", fake(16)),
            "redis://:***REDACTED***@cache:6379",
        ),
        case(
            "password containing @",
            format!("postgres://app:{}@{}@db/x", fake(4), fake(10)),
            "postgres://app:***REDACTED***@db/x",
        ),
        case(
            "URLs without a password are left alone",
            "see https://user@example.com and https://example.com:8080/path?email=a@b.com"
                .to_owned(),
            "see https://user@example.com and https://example.com:8080/path?email=a@b.com",
        ),
        case(
            "a query or fragment without a path is not a password",
            "open https://example.com:8443?email=a@b.com or https://example.com:8443#x@y"
                .to_owned(),
            "open https://example.com:8443?email=a@b.com or https://example.com:8443#x@y",
        ),
        case(
            "a password may hold '?' or '#'",
            format!(
                "mysql://root:p#{}@db:3306/app and postgres://app:a?{}@host/db",
                fake(8),
                fake(8)
            ),
            "mysql://root:***REDACTED***@db:3306/app and postgres://app:***REDACTED***@host/db",
        ),
        case(
            "a colon in a query is not a user",
            "at https://example.com?at=10:30@noon or https://github.com/x#L1:2@y".to_owned(),
            "at https://example.com?at=10:30@noon or https://github.com/x#L1:2@y",
        ),
        case(
            "a password before a query is still masked",
            format!("dial redis://:{}@cache:6379?db=0", fake(12)),
            "dial redis://:***REDACTED***@cache:6379?db=0",
        ),
    ]
}

/// Text that must stay as it is, and tokens glued to Japanese.
fn prose_cases() -> Vec<Case> {
    vec![
        case(
            "prose about auth is left alone",
            "the API returns 401 for Authorization: Bearer <token>; is sk-1 the task-notification key?"
                .to_owned(),
            "the API returns 401 for Authorization: Bearer <token>; is sk-1 the task-notification key?",
        ),
        // Go's \b and \w are ASCII-only; Rust's are Unicode by default and
        // would treat a following Japanese character as part of the token.
        case(
            "access key id followed by Japanese",
            format!("\u{30AD}\u{30FC}{}\u{306E}\u{5024}", key_id()),
            "\u{30AD}\u{30FC}***REDACTED***\u{306E}\u{5024}",
        ),
        case(
            "GitLab token followed by Japanese",
            format!("glpat-{}\u{3067}\u{3059}", fake(20)),
            "***REDACTED***\u{3067}\u{3059}",
        ),
    ]
}

/// promptdig's table, plus rows for text glued to Japanese.
fn cases() -> Vec<Case> {
    [aws_cases(), token_cases(), url_cases(), prose_cases()]
        .into_iter()
        .flatten()
        .collect()
}

#[test]
fn redact_masks_each_case_and_reports_whether_it_changed_the_text() -> TestResult {
    let redactor = Redactor::new()?;
    for row in cases() {
        let (got, changed) = redactor.redact(&row.input);
        check_eq(got.as_str(), row.want.as_str(), row.name)?;
        check_eq(&changed, &(row.input != row.want), row.name)?;
    }
    Ok(())
}

#[test]
fn marker_is_the_text_that_replaces_a_secret() -> TestResult {
    check_eq(MARKER, "***REDACTED***", "marker")?;
    Ok(())
}

#[test]
fn redact_json_masks_strings_at_any_depth_and_keeps_the_rest() -> TestResult {
    let redactor = Redactor::new()?;
    let mut value = json!({
        "command": format!("export AWS_SECRET_ACCESS_KEY={}", secret_key()),
        "nested": {"tokens": [fake_github("ghp"), "plain"]},
        "count": 3_i32,
        "flag": true,
    });
    let changed = redactor.redact_json(&mut value);
    let want = json!({
        "command": "export AWS_SECRET_ACCESS_KEY=***REDACTED***",
        "nested": {"tokens": ["***REDACTED***", "plain"]},
        "count": 3_i32,
        "flag": true,
    });
    check_eq(&value, &want, "tool input")?;
    check_eq(&changed, &true, "changed")?;
    Ok(())
}

#[test]
fn redact_json_reports_no_change_when_nothing_matches() -> TestResult {
    let redactor = Redactor::new()?;
    let mut value = json!({"file_path": "/tmp/x", "lines": [1_i32, 2_i32]});
    let before = value.clone();
    check_eq(&redactor.redact_json(&mut value), &false, "changed")?;
    check_eq(&value, &before, "untouched input")?;
    Ok(())
}
