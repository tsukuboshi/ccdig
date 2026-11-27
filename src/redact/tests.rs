//! Tests for secret masking.

#![expect(clippy::disallowed_methods, reason = "the test times the masking")]

mod properties;

use std::time::Instant;

use serde_json::{Value, json};

use super::{Counts, ExtraMasks, MARKER, Redactor};
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
            "Slack incoming webhook followed by more path",
            format!("https://hooks.slack.com/services/{}/notes", fake(56)),
            "https://***REDACTED***/notes",
        ),
        case(
            "Slack workflow webhook with a long path",
            format!(
                "post to https://hooks.slack.com/workflows/{}/{}/{}/{} now",
                fake(11),
                fake(11),
                fake(18),
                fake(24)
            ),
            "post to https://***REDACTED*** now",
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

/// Headers, command-line passwords, and tokens of other services.
fn format_cases() -> Vec<Case> {
    vec![
        case(
            "Authorization header of each scheme",
            format!(
                "curl -H 'Authorization: Bearer {}' -H \"authorization: basic {}\" -H 'Authorization=Token {}'",
                fake(12),
                fake(12),
                fake(12)
            ),
            "curl -H 'Authorization: Bearer ***REDACTED***' -H \"authorization: basic ***REDACTED***\" -H 'Authorization=Token ***REDACTED***'",
        ),
        case(
            "bearer token without the header name",
            format!("send Bearer {} with it", fake(24)),
            "send Bearer ***REDACTED*** with it",
        ),
        case(
            "bearer token in lowercase",
            format!("using bearer {}", fake(24)),
            "using bearer ***REDACTED***",
        ),
        case(
            "mysql-family password glued to -p",
            format!(
                "mysql -u root -p{} app; mysqldump -h db -p{} app | gzip",
                fake(10),
                fake(10)
            ),
            "mysql -u root -p***REDACTED*** app; mysqldump -h db -p***REDACTED*** app | gzip",
        ),
        case(
            "mysql -p on its own prompts and is left alone",
            "mysql -u root -p app and mysql -P 3306".to_owned(),
            "mysql -u root -p app and mysql -P 3306",
        ),
        case(
            "mysql in a path is not the command",
            "find /var/lib/mysql -name '*.cnf' -print; ls /etc/mysql -perm".to_owned(),
            "find /var/lib/mysql -name '*.cnf' -print; ls /etc/mysql -perm",
        ),
        case(
            "mysql by its full path",
            format!("/usr/local/bin/mysql -u root -p{} app", fake(10)),
            "/usr/local/bin/mysql -u root -p***REDACTED*** app",
        ),
        case(
            "prose that only mentions mysql -p",
            "the mysql-family password glued to -p\", is masked".to_owned(),
            "the mysql-family password glued to -p\", is masked",
        ),
        case(
            "quoted mysql password",
            format!("mysql -u root -p'{}' app", fake(10)),
            "mysql -u root -p***REDACTED*** app",
        ),
        case(
            "mysql -p followed by a variable or a placeholder",
            "mysql -u root -p$MYSQL_PWD app; format!(\"mysql -p{}\"); \"mysql -p\\\", -p\u{30FB}x\u{3068}".to_owned(),
            "mysql -u root -p$MYSQL_PWD app; format!(\"mysql -p{}\"); \"mysql -p\\\", -p\u{30FB}x\u{3068}",
        ),
        case(
            "SendGrid, Google OAuth, and Stripe webhook secrets",
            format!(
                "SG.{}.{} ya29.{} 1//0{} GOCSPX-{} whsec_{}",
                fake(22),
                fake(43),
                fake(30),
                fake(30),
                fake(28),
                fake(32)
            ),
            "***REDACTED*** ***REDACTED*** ***REDACTED*** ***REDACTED*** ***REDACTED***",
        ),
    ]
}

/// Values assigned to names that hold credentials.
fn assignment_cases() -> Vec<Case> {
    vec![
        case(
            "an assignment inside a flag value",
            format!(
                "kubectl create secret generic db --from-literal=password={} && docker run --env=DB_PASSWORD={}",
                fake(12),
                fake(12)
            ),
            "kubectl create secret generic db --from-literal=password=***REDACTED*** && docker run --env=DB_PASSWORD=***REDACTED***",
        ),
        case(
            "escaped JSON inside a string",
            format!(
                "\"SecretString\": \"{{\\\"username\\\":\\\"admin\\\",\\\"password\\\":\\\"{}\\\"}}\"",
                fake(12)
            ),
            "\"SecretString\": \"{\\\"username\\\":\\\"admin\\\",\\\"password\\\":\\\"***REDACTED***\\\"}\"",
        ),
        case(
            "names without separators",
            format!(
                "PGPASSWORD={} ACCESSTOKEN={} clientsecret={} SECRETKEY={}",
                fake(10),
                fake(10),
                fake(10),
                fake(10)
            ),
            "PGPASSWORD=***REDACTED*** ACCESSTOKEN=***REDACTED*** clientsecret=***REDACTED*** SECRETKEY=***REDACTED***",
        ),
        case(
            "percent-encoded and ENV-like values",
            "password=p%40ssw0rd123 access_token=Ab3dE%2FfGh9Jk%2BLm0&x=1 token='ENVx9k2Lm3Qp'"
                .to_owned(),
            "password=***REDACTED*** access_token=***REDACTED***&x=1 token='***REDACTED***'",
        ),
        case(
            "tokens with dots or brackets",
            format!(
                "DISCORD_TOKEN={}.{}.{} api_token=v1.{} password=Pass.Word2024 password=Pa(ssw0rd!x",
                fake(24),
                fake(6),
                fake(27),
                fake(15)
            ),
            "DISCORD_TOKEN=***REDACTED*** api_token=***REDACTED*** password=***REDACTED*** password=***REDACTED***",
        ),
        case(
            "a quoted passphrase is masked whole",
            "password: \"correct horse battery staple\" and passphrase='a b c d e f'".to_owned(),
            "password: \"***REDACTED***\" and passphrase='***REDACTED***'",
        ),
        case(
            "weak password behind a pass name",
            "DB_PASS=12345678 SMTP_PASS=hunterbunny".to_owned(),
            "DB_PASS=***REDACTED*** SMTP_PASS=***REDACTED***",
        ),
        case(
            "shell, flag, YAML, and Ruby forms",
            format!(
                "PASSWORD={} --api-token={} client_secret: \"{}\" :password => '{}'",
                fake(12),
                fake(12),
                fake(12),
                fake(12)
            ),
            "PASSWORD=***REDACTED*** --api-token=***REDACTED*** client_secret: \"***REDACTED***\" :password => '***REDACTED***'",
        ),
        case(
            "query string keeps the other parameters",
            format!(
                "GET https://api.example.com/v1?access_token={}&page=2",
                fake(20)
            ),
            "GET https://api.example.com/v1?access_token=***REDACTED***&page=2",
        ),
        case(
            "JSON with a camelCase name",
            format!("{{\"apiKey\":\"{}\",\"region\":\"us-east-1\"}}", fake(20)),
            "{\"apiKey\":\"***REDACTED***\",\"region\":\"us-east-1\"}",
        ),
        case(
            "Japanese name with a full-width colon",
            format!(
                "\u{30D1}\u{30B9}\u{30EF}\u{30FC}\u{30C9}\u{FF1A}{}",
                fake(10)
            ),
            "\u{30D1}\u{30B9}\u{30EF}\u{30FC}\u{30C9}\u{FF1A}***REDACTED***",
        ),
        case(
            "a strong name masks a password of letters only",
            "password: correcthorse".to_owned(),
            "password: ***REDACTED***",
        ),
    ]
}

/// Japanese names glued to the text before them, which has no ASCII
/// boundary for the name.
fn japanese_name_cases() -> Vec<Case> {
    vec![
        case(
            "Japanese name right after a masked value",
            format!(
                "token={}~\u{30D1}\u{30B9}\u{30EF}\u{30FC}\u{30C9}={}",
                fake(10),
                fake(10)
            ),
            "token=***REDACTED***\u{30D1}\u{30B9}\u{30EF}\u{30FC}\u{30C9}=***REDACTED***",
        ),
        case(
            "API key in Japanese right after a value, which would read its ASCII part",
            format!("token={}API\u{30AD}\u{30FC}={}", fake(10), fake(10)),
            "token=***REDACTED***\u{30AD}\u{30FC}=***REDACTED***",
        ),
        case(
            "Japanese name right after an ASCII word",
            format!(
                "ID\u{FF1A}user1\u{30D1}\u{30B9}\u{30EF}\u{30FC}\u{30C9}\u{FF1A}{}",
                fake(10)
            ),
            "ID\u{FF1A}user1\u{30D1}\u{30B9}\u{30EF}\u{30FC}\u{30C9}\u{FF1A}***REDACTED***",
        ),
    ]
}

/// Assignments that look like credentials but are not.
fn non_credential_cases() -> Vec<Case> {
    vec![
        case(
            "parameters, sets, and counts after a name",
            "const requireApiKey = (secret: string) => secret; f\"client_secret={client_secret}\" recache_tokens_if_cold: 999999"
                .to_owned(),
            "const requireApiKey = (secret: string) => secret; f\"client_secret={client_secret}\" recache_tokens_if_cold: 999999",
        ),
        case(
            "a quote that closes a pattern is not an opening quote",
            "env | grep '^GCP_PRIVATE_KEY=' | cut -d= -f2- | tr -d '\\n'".to_owned(),
            "env | grep '^GCP_PRIVATE_KEY=' | cut -d= -f2- | tr -d '\\n'",
        ),
        case(
            "numeric settings",
            "token_limit: 100000 TOKEN_COUNT=123456 AUTH_TIMEOUT_SECONDS = 300000".to_owned(),
            "token_limit: 100000 TOKEN_COUNT=123456 AUTH_TIMEOUT_SECONDS = 300000",
        ),
        case(
            "format strings and variables",
            "api_key=%{key} password=%(password)s token=%TOKEN% secret=\\u0041bcdefg".to_owned(),
            "api_key=%{key} password=%(password)s token=%TOKEN% secret=\\u0041bcdefg",
        ),
        case(
            "type annotations and the OAuth token type",
            "readonly token: string; refreshToken: number; token_type: \"Bearer\"".to_owned(),
            "readonly token: string; refreshToken: number; token_type: \"Bearer\"",
        ),
        case(
            "escape sequences",
            "const KEY_SEPARATOR = \"\\u0000\"; tokens_above=\\($limit)".to_owned(),
            "const KEY_SEPARATOR = \"\\u0000\"; tokens_above=\\($limit)",
        ),
        case(
            "prose after a name",
            "password= \u{5F62}\u{5F0F}\u{306E}\u{5024}\u{304C}\u{6B8B}\u{308B}".to_owned(),
            "password= \u{5F62}\u{5F0F}\u{306E}\u{5024}\u{304C}\u{6B8B}\u{308B}",
        ),
        case(
            "a weak word alone is not a credential name",
            "key: \"Escape\" and hook: post-commit".to_owned(),
            "key: \"Escape\" and hook: post-commit",
        ),
        case(
            "database and storage terms",
            format!(
                "primary_key: {} S3_KEY={} max_tokens: 4096",
                fake(10),
                fake(10)
            ),
            &format!(
                "primary_key: {} S3_KEY={} max_tokens: 4096",
                fake(10),
                fake(10)
            ),
        ),
        case(
            "paths and URLs",
            "token_path=./secrets/token.txt secret_url: https://vault.example.com".to_owned(),
            "token_path=./secrets/token.txt secret_url: https://vault.example.com",
        ),
        case(
            "references to a value",
            "password=$DB_PASSWORD token: ENV['TOKEN'] secret: <your-secret> api_key=%{key}"
                .to_owned(),
            "password=$DB_PASSWORD token: ENV['TOKEN'] secret: <your-secret> api_key=%{key}",
        ),
        case(
            "code expressions",
            "password = self.password token = config[\"token\"] secret = last_secret".to_owned(),
            "password = self.password token = config[\"token\"] secret = last_secret",
        ),
        case(
            "a weak name with a word or a count",
            "AttributeKey: ProductName and api_key: 123456789".to_owned(),
            "AttributeKey: ProductName and api_key: 123456789",
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
        // Unicode \b and \w would treat a following Japanese character as
        // part of the token.
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

/// Every case, including text glued to Japanese.
fn cases() -> Vec<Case> {
    [
        aws_cases(),
        token_cases(),
        format_cases(),
        assignment_cases(),
        japanese_name_cases(),
        non_credential_cases(),
        url_cases(),
        prose_cases(),
    ]
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
fn redact_json_masks_a_string_under_a_credential_key() -> TestResult {
    let redactor = Redactor::new()?;
    let mut value = json!({
        "connection": {"host": "db", "password": fake(10), "port": 5432_i32},
        "api_key": fake(20),
        "token": "string",
        "description": "rotate the password",
    });
    let changed = redactor.redact_json(&mut value);
    let want = json!({
        "connection": {"host": "db", "password": "***REDACTED***", "port": 5432_i32},
        "api_key": "***REDACTED***",
        "token": "string",
        "description": "rotate the password",
    });
    check_eq(&value, &want, "tool input")?;
    check_eq(&changed, &true, "changed")?;
    Ok(())
}

#[test]
fn redact_runs_in_linear_time_on_many_non_credential_assignments() -> TestResult {
    let redactor = Redactor::new()?;
    let text = "://a:".repeat(40_000);
    let started = Instant::now();
    let (got, changed) = redactor.redact(&text);
    check_eq(got.as_str(), text.as_str(), "text")?;
    check_eq(&changed, &false, "changed")?;
    // The quadratic version took over a minute on this input.
    check_eq(&(started.elapsed().as_secs() < 5), &true, "elapsed")?;
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

#[test]
fn redact_json_masks_keys_with_the_extra_masks() -> TestResult {
    let redactor = extra_redactor()?;
    let mut value = json!({
        "corp-internal": "kept",
        "/home/alice.b/notes": 1_i32,
        "plain": "corp-internal",
    });
    let changed = redactor.redact_json(&mut value);
    let want = json!({
        "***REDACTED***": "kept",
        "/home/***REDACTED***/notes": 1_i32,
        "plain": "***REDACTED***",
    });
    check_eq(&value, &want, "tool input")?;
    check_eq(&changed, &true, "changed")?;
    Ok(())
}

#[test]
fn redact_json_numbers_the_keys_that_masking_makes_equal() -> TestResult {
    let redactor = extra_redactor()?;
    let mut value = json!({
        "corp-internal": 1_i32,
        "RED": 2_i32,
        "corp-internal.example": 3_i32,
    });
    redactor.redact_json(&mut value);
    let got: Vec<(String, Value)> = value
        .as_object()
        .ok_or("not an object")?
        .iter()
        .map(|(key, field)| (key.clone(), field.clone()))
        .collect();
    let want = vec![
        ("***REDACTED***".to_owned(), json!(1_i32)),
        ("***REDACTED***#2".to_owned(), json!(2_i32)),
        ("***REDACTED***#3".to_owned(), json!(3_i32)),
    ];
    check_eq(&got, &want, "keys in order")?;
    Ok(())
}

#[test]
fn redact_json_masks_a_secret_in_a_key_and_leaves_the_other_keys_alone() -> TestResult {
    let redactor = Redactor::new()?;
    let token = fake_github("ghp");
    let mut plain = json!({ "corp-internal": 1_i32, "command": "ls" });
    let before = plain.clone();
    check_eq(
        &redactor.redact_json(&mut plain),
        &false,
        "plain keys changed",
    )?;
    check_eq(&plain, &before, "plain keys")?;
    let mut secret = json!({ token.clone(): 2_i32 });
    let mut counts = Counts::default();
    check_eq(
        &redactor.redact_json_counted(&mut secret, &mut counts),
        &true,
        "a secret key changed",
    )?;
    check_eq(
        &secret.to_string().contains(&token),
        &false,
        &secret.to_string(),
    )?;
    check_eq(&counts.ranked().is_empty(), &false, "counted")?;
    Ok(())
}

/// The extra masks on, with `alice.b` as the user name and three values
/// from a mask file.
fn extra_redactor() -> Result<Redactor, regex::Error> {
    Redactor::with(ExtraMasks {
        emails: true,
        ips: true,
        user: Some("alice.b".to_owned()),
        values: ["corp-internal", "corp-internal.example", "RED"]
            .map(|value| (value.to_owned(), MARKER.to_owned(), String::new()))
            .into(),
        ..ExtraMasks::default()
    })
}

/// The rows for the masks that `--mask` and `--mask-values` turn on.
fn extra_cases() -> Vec<Case> {
    vec![
        case(
            "email address",
            "mail alice.b+ci@mail.example.co.jp now".to_owned(),
            "mail ***REDACTED*** now",
        ),
        case(
            "SSH remote is not an email address",
            "git clone git@github.com:alice/blog.git".to_owned(),
            "git clone git@github.com:alice/blog.git",
        ),
        case(
            "user name in a path and with other separators or case",
            "cd /Users/alice.b/blog by Alice-B in users-alice_b-x".to_owned(),
            "cd /Users/***REDACTED***/blog by ***REDACTED*** in users-***REDACTED***-x",
        ),
        case(
            "a longer name or a part of the user name stays",
            "ls /Users/alice.bx and malice.b and alice".to_owned(),
            "ls /Users/alice.bx and malice.b and alice",
        ),
        case(
            "project directory name",
            "-Users-alice-b-blog".to_owned(),
            "-Users-***REDACTED***-blog",
        ),
        case(
            "IPv4 address",
            "ssh 10.0.12.34 and 192.168.1.255:8080".to_owned(),
            "ssh ***REDACTED*** and ***REDACTED***:8080",
        ),
        case(
            "loopback, unspecified, broadcast, versions, and non-addresses stay",
            "127.0.0.1 0.0.0.0 255.255.255.255 v1.2.3.4 1.2.3.4.5 300.1.1.1".to_owned(),
            "127.0.0.1 0.0.0.0 255.255.255.255 v1.2.3.4 1.2.3.4.5 300.1.1.1",
        ),
        case(
            "IPv6 address",
            "ping 2001:db8::1 and fe80:0:0:0:0:0:0:1a".to_owned(),
            "ping ***REDACTED*** and ***REDACTED***",
        ),
        case(
            "IPv6 loopback, times, and paths stay",
            "::1 at 12:30:45 in std::fs and a::b".to_owned(),
            "::1 at 12:30:45 in std::fs and a::b",
        ),
        case(
            "IPv4 address at the end of a sentence",
            "server is 10.0.0.5.".to_owned(),
            "server is ***REDACTED***.",
        ),
        case(
            "email address followed by a colon in prose",
            "From alice@example.com: ok".to_owned(),
            "From ***REDACTED***: ok",
        ),
        case(
            "an email address that holds a value is masked whole",
            "mail bob@corp-internal.example".to_owned(),
            "mail ***REDACTED***",
        ),
        case(
            "a value inside the marker does not break it",
            "alice@example.com and RED".to_owned(),
            "***REDACTED*** and ***REDACTED***",
        ),
        case(
            "values from the mask file, longest first",
            "host corp-internal.example for corp-internal".to_owned(),
            "host ***REDACTED*** for ***REDACTED***",
        ),
    ]
}

#[test]
fn account_masks_count_each_value_once_and_nothing_masked_again() -> TestResult {
    let redactor = account_redactor()?;
    let mut counts = Counts::default();
    let (masked, _) =
        redactor.redact_counted("PS1='(123456789012 prod)' [profile dev]", &mut counts);
    check_eq(
        &counts.ranked(),
        &vec![
            ("aws-account".to_owned(), 2_usize),
            ("aws-profile".to_owned(), 1),
        ],
        "first",
    )?;
    let mut again = Counts::default();
    let (twice, changed) = redactor.redact_counted(&masked, &mut again);
    check_eq(&twice, &masked, "twice")?;
    check_eq(&changed, &false, "changed")?;
    check_eq(&again.ranked(), &vec![], "again")?;
    Ok(())
}

#[test]
fn counts_name_each_rule_that_masked_something() -> TestResult {
    let redactor = extra_redactor()?;
    let mut counts = Counts::default();
    let (_, changed) = redactor.redact_counted(
        &format!(
            "mail bob@example.com about corp-internal with password={} on 10.0.12.34 and 10.0.12.35",
            fake(20)
        ),
        &mut counts,
    );
    check_eq(&changed, &true, "changed")?;
    check_eq(
        &counts.ranked(),
        &vec![
            ("ip".to_owned(), 2_usize),
            ("credential-assignment".to_owned(), 1),
            ("email".to_owned(), 1),
            ("values".to_owned(), 1),
        ],
        "text",
    )?;
    let mut json_counts = Counts::default();
    let mut value = json!({"password": fake(10), "corp-internal": "kept"});
    redactor.redact_json_counted(&mut value, &mut json_counts);
    check_eq(
        &json_counts.ranked(),
        &vec![
            ("credential-key".to_owned(), 1_usize),
            ("values".to_owned(), 1),
        ],
        "tool input",
    )?;
    let mut none = Counts::default();
    redactor.redact_counted("nothing here", &mut none);
    check_eq(&none.ranked(), &Vec::new(), "nothing")?;
    Ok(())
}

#[test]
fn extra_masks_apply_only_when_asked_for() -> TestResult {
    let extra = extra_redactor()?;
    let plain = Redactor::new()?;
    for row in extra_cases() {
        let (got, changed) = extra.redact(&row.input);
        check_eq(got.as_str(), row.want.as_str(), row.name)?;
        check_eq(&changed, &(row.input != row.want), row.name)?;
        let (untouched, _) = plain.redact(&row.input);
        check_eq(untouched.as_str(), row.input.as_str(), row.name)?;
    }
    Ok(())
}

#[test]
fn labels_and_user_names_do_not_cut_a_secret_short() -> TestResult {
    let redactor = Redactor::with(ExtraMasks {
        user: Some("bob".to_owned()),
        values: vec![
            (
                "acme".to_owned(),
                "\u{FF3B}org1\u{FF3D}".to_owned(),
                "org".to_owned(),
            ),
            (
                "bob-secret-infra".to_owned(),
                "\u{FF3B}repo1\u{FF3D}".to_owned(),
                "repo".to_owned(),
            ),
        ],
        ..ExtraMasks::default()
    })?;
    for (input, want) in [
        (
            format!("password={}acme-{}", fake(9), fake(10)),
            "password=***REDACTED***",
        ),
        (format!("token=xacme-{}", fake(6)), "token=***REDACTED***"),
        (
            format!("DB_PASSWORD=acme-{}!x", fake(10)),
            "DB_PASSWORD=***REDACTED***",
        ),
        (
            "clone acme/bob-secret-infra as bob".to_owned(),
            "clone \u{FF3B}org1\u{FF3D}/\u{FF3B}repo1\u{FF3D} as ***REDACTED***",
        ),
    ] {
        check_eq(&redactor.redact(&input).0, &want.to_owned(), &input)?;
    }
    Ok(())
}

/// The AWS account, AWS profile, and git user masks on, with
/// `Hanako Sato` and `hanako@example.com` from git.
fn account_redactor() -> Result<Redactor, regex::Error> {
    Redactor::with(ExtraMasks {
        aws_accounts: true,
        aws_profiles: true,
        git_users: vec!["Hanako Sato".to_owned(), "hanako@example.com".to_owned()],
        ..ExtraMasks::default()
    })
}

/// The AWS account IDs, AWS profiles, and git user names in context.
fn account_cases() -> Vec<Case> {
    vec![
        case(
            "account in an ARN",
            "arn:aws:iam::123456789012:role/deploy".to_owned(),
            "arn:aws:iam::***REDACTED***:role/deploy",
        ),
        case(
            "an ARN without an account",
            "arn:aws:s3:::my-bucket/key".to_owned(),
            "arn:aws:s3:::my-bucket/key",
        ),
        case(
            "account from aws sts",
            r#"{"Account": "123456789012"}"#.to_owned(),
            r#"{"Account": "***REDACTED***"}"#,
        ),
        case(
            "account assigned to a name",
            "export AWS_ACCOUNT_ID=123456789012 account_id: 210987654321".to_owned(),
            "export AWS_ACCOUNT_ID=***REDACTED*** account_id: ***REDACTED***",
        ),
        case(
            "account in an ECR host",
            "docker push 123456789012.dkr.ecr.ap-northeast-1.amazonaws.com/app:1".to_owned(),
            "docker push ***REDACTED***.dkr.ecr.ap-northeast-1.amazonaws.com/app:1",
        ),
        case(
            "account after a flag",
            "aws organizations describe-account --account-id 123456789012".to_owned(),
            "aws organizations describe-account --account-id ***REDACTED***",
        ),
        case(
            "twelve digits without context and thirteen digits stay",
            "order 123456789012 shipped, account_id: 1234567890123".to_owned(),
            "order 123456789012 shipped, account_id: 1234567890123",
        ),
        case(
            "profile after the flag",
            "aws s3 ls --profile org_read && aws sso login --profile=prod-admin".to_owned(),
            "aws s3 ls --profile ***REDACTED*** && aws sso login --profile=***REDACTED***",
        ),
        case(
            "profile in a variable",
            "export AWS_PROFILE=prod-admin AWS_DEFAULT_PROFILE='org_read'".to_owned(),
            "export AWS_PROFILE=***REDACTED*** AWS_DEFAULT_PROFILE='***REDACTED***'",
        ),
        case(
            "profile in a config file",
            "[profile prod-admin]\nsource_profile = org_read".to_owned(),
            "[profile ***REDACTED***]\nsource_profile = ***REDACTED***",
        ),
        case(
            "account in a console host, a hyphenated key, an owner, and a principal",
            r#"https://123456789012.signin.aws.amazon.com/console account-id: 210987654321 "OwnerId": "111122223333" {"AWS": "222233334444"}"#.to_owned(),
            r#"https://***REDACTED***.signin.aws.amazon.com/console account-id: ***REDACTED*** "OwnerId": "***REDACTED***" {"AWS": "***REDACTED***"}"#,
        ),
        case(
            "account after an owners flag",
            "aws ec2 describe-images --owners 444455556666".to_owned(),
            "aws ec2 describe-images --owners ***REDACTED***",
        ),
        case(
            "profile in a credentials file, an SSO session, and a YAML key",
            "[acme-prod]\naws_access_key_id = x\n[sso-session acme]\nprovider:\n  profile: acme-stg".to_owned(),
            "[***REDACTED***]\naws_access_key_id = x\n[sso-session ***REDACTED***]\nprovider:\n  profile: ***REDACTED***",
        ),
        case(
            "a section of another file stays",
            "[dependencies]\nserde = \"1\"".to_owned(),
            "[dependencies]\nserde = \"1\"",
        ),
        case(
            "a profile name in prose stays",
            "use the org_read profile".to_owned(),
            "use the org_read profile",
        ),
        case(
            "git user name and email, in any case",
            "Author: Hanako Sato <hanako@example.com> and hanako sato".to_owned(),
            "Author: ***REDACTED*** <***REDACTED***> and ***REDACTED***",
        ),
        case(
            "a longer word stays",
            "Hanako Satox".to_owned(),
            "Hanako Satox",
        ),
    ]
}

/// AWS account IDs, and the account or profile name beside one, in a
/// prompt `PS1` and after the Japanese word for account, and twelve digits
/// out of that context that stay.
fn account_context_cases() -> Vec<Case> {
    let account = "\u{30A2}\u{30AB}\u{30A6}\u{30F3}\u{30C8}";
    vec![
        case(
            "account and profile in a PS1",
            r#"export PS1="\n(123456789012 my-profile)\n[\t \u@\h \W]$ ""#.to_owned(),
            r#"export PS1="\n(***REDACTED*** ***REDACTED***)\n[\t \u@\h \W]$ ""#,
        ),
        case(
            "account after the word, with ID or the word for number",
            format!("{account} ID: 123456789012 {account}\u{756A}\u{53F7}\u{FF1A}210987654321\u{3067}\u{3059}"),
            &format!("{account} ID: ***REDACTED*** {account}\u{756A}\u{53F7}\u{FF1A}***REDACTED***\u{3067}\u{3059}"),
        ),
        case(
            "account and its name in full-width brackets after a heading",
            format!("\u{3010}{account}\u{3011} my-account-pd\u{FF08}123456789012\u{FF09}"),
            &format!("\u{3010}{account}\u{3011} ***REDACTED***\u{FF08}***REDACTED***\u{FF09}"),
        ),
        case(
            "account and its name on the line after a heading",
            format!("\u{3010}AWS{account}\u{3011}\nmy-account-pd\u{FF08}123456789012\u{FF09}"),
            &format!("\u{3010}AWS{account}\u{3011}\n***REDACTED***\u{FF08}***REDACTED***\u{FF09}"),
        ),
        case(
            "brackets two lines after a heading stay",
            format!("\u{3010}{account}\u{3011}\nnote\nsee (123456789012)"),
            &format!("\u{3010}{account}\u{3011}\nnote\nsee (123456789012)"),
        ),
        case(
            "every account in brackets on the line and in a list",
            format!("{account}: prod(111111111111)\u{3001}dev(222222222222) {account}ID: 333333333333, 444444444444"),
            &format!("{account}: ***REDACTED***(***REDACTED***)\u{3001}***REDACTED***(***REDACTED***) {account}ID: ***REDACTED***, ***REDACTED***"),
        ),
        case(
            "an account name in a list or an indented line after a heading",
            format!("\u{3010}{account}\u{3011}\n- prod: prod-account\u{FF08}111111111111\u{FF09}\n\u{3010}{account}\u{3011}\n  dev\u{FF08}222222222222\u{FF09}"),
            &format!("\u{3010}{account}\u{3011}\n- prod: ***REDACTED***\u{FF08}***REDACTED***\u{FF09}\n\u{3010}{account}\u{3011}\n  ***REDACTED***\u{FF08}***REDACTED***\u{FF09}"),
        ),
        case(
            "only the name before the brackets after a comma",
            format!("{account}: prod\u{3001}dev\u{FF08}123456789012\u{FF09}"),
            &format!("{account}: prod\u{3001}***REDACTED***\u{FF08}***REDACTED***\u{FF09}"),
        ),
        case(
            "a word before another rule's marker stays",
            format!("{account} token (***REDACTED***)"),
            &format!("{account} token (***REDACTED***)"),
        ),
        case(
            "account and profile in a zsh PROMPT and before a space",
            "PROMPT='(123456789012 prod) %~ $ ' PS1='(210987654321 dev )'".to_owned(),
            "PROMPT='(***REDACTED*** ***REDACTED***) %~ $ ' PS1='(***REDACTED*** ***REDACTED*** )'",
        ),
        case(
            "account and its name in ASCII brackets",
            format!("{account}: dev (123456789012)"),
            &format!("{account}: ***REDACTED*** (***REDACTED***)"),
        ),
        case(
            "a job ID, digits in brackets without the word, and a PS1 without brackets stay",
            "https://github.com/o/r/actions/runs/1/job/123456789012 see (123456789012) PS1=\"123456789012 $ \"".to_owned(),
            "https://github.com/o/r/actions/runs/1/job/123456789012 see (123456789012) PS1=\"123456789012 $ \"",
        ),
        case(
            "thirteen digits in brackets stay",
            format!("\u{3010}{account}\u{3011} dev\u{FF08}1234567890123\u{FF09}"),
            &format!("\u{3010}{account}\u{3011} dev\u{FF08}1234567890123\u{FF09}"),
        ),
        case(
            "brackets far from the word stay",
            format!("{account} {} (123456789012)", "x".repeat(81)),
            &format!("{account} {} (123456789012)", "x".repeat(81)),
        ),
    ]
}

#[test]
fn account_masks_apply_only_when_asked_for() -> TestResult {
    let masked = account_redactor()?;
    let plain = Redactor::new()?;
    for row in account_cases().into_iter().chain(account_context_cases()) {
        let (got, changed) = masked.redact(&row.input);
        check_eq(got.as_str(), row.want.as_str(), row.name)?;
        check_eq(&changed, &(row.input != row.want), row.name)?;
        let (untouched, _) = plain.redact(&row.input);
        check_eq(untouched.as_str(), row.input.as_str(), row.name)?;
    }
    Ok(())
}

#[test]
fn meta_applies_only_the_masks_asked_for() -> TestResult {
    let extra = extra_redactor()?;
    check_eq(
        &extra.meta("/Users/alice.b/.claude/projects/-Users-alice-b-blog/s.jsonl"),
        &"/Users/***REDACTED***/.claude/projects/-Users-***REDACTED***-blog/s.jsonl".to_owned(),
        "path",
    )?;
    let key = key_id();
    check_eq(
        &extra.meta(&format!("feat/corp-internal-{key}")),
        &format!("feat/***REDACTED***-{key}"),
        "branch keeps what only the built-in rules mask",
    )?;
    check_eq(
        &Redactor::new()?.meta("/Users/alice/x"),
        &"/Users/alice/x".to_owned(),
        "nothing asked for",
    )?;
    Ok(())
}
