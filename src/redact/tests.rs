//! Tests for secret masking.

use std::time::Instant;

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
