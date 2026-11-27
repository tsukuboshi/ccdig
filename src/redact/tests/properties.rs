//! Properties of secret masking that hold for any input.
//!
//! Secrets are generated from patterns at run time, so no token-shaped
//! literal is committed.

use core::error::Error;
use core::iter;

use proptest::arbitrary::any;
use proptest::array::uniform4;
use proptest::collection::vec;
use proptest::option;
use proptest::sample::select;
use proptest::strategy::{BoxedStrategy, Strategy, Union};
use proptest::string::string_regex;
use serde_json::Value;

use crate::redact::{Counts, ExtraMasks, MARKER, Redactor};
use crate::testing::{TestResult, check_eq, for_all, for_all_cases, hostile_text, json_value};

/// Values tried by a property that compiles the masking rules for each,
/// which takes milliseconds in a debug build.
const COMPILING_CASES: u32 = 64;

/// Whitespace that ends a secret and starts the text after it.
const SEPARATORS: &[&str] = &[" ", "\n", "\t"];

/// Pieces that put the masking rules near their edges when joined at
/// random: names and operators of assignments, quotes, URL parts, rule
/// prefixes, the contexts of an AWS account ID, and multibyte characters.
const FRAGMENTS: &[&str] = &[
    "PS1=\"(",
    "\u{30A2}\u{30AB}\u{30A6}\u{30F3}\u{30C8}",
    "\u{3010}",
    "\u{3011}",
    "\u{FF08}",
    "\u{FF09}",
    "(",
    ")",
    "123456789012",
    "***REDACTED***",
    "password",
    "api_key",
    "token",
    "\u{30D1}\u{30B9}\u{30EF}\u{30FC}\u{30C9}",
    "API\u{30AD}\u{30FC}",
    "=",
    ":",
    "\u{FF1A}",
    "=>",
    "\"",
    "'",
    "\\\"",
    "`",
    " ",
    "\t",
    "\n",
    "://",
    "@",
    "/",
    "?",
    "#",
    " -p",
    "mysql",
    "Bearer ",
    "Authorization: ",
    "AKIA",
    "ghp_",
    "sk-",
    "ey",
    ".",
    "*",
    "\u{E9}",
    "\u{65E5}\u{672C}",
    "\u{1F600}",
    "aB3!xY9~",
    "0123456789",
    "abcdefghij",
];

/// Text holding a secret, and the secret.
type Secret = BoxedStrategy<(String, String)>;

/// A secret between `before` and `after`, each drawn from a regular
/// expression.
fn secret(before: &str, body: &str, after: &str) -> Result<Secret, Box<dyn Error>> {
    Ok((
        string_regex(before)?,
        string_regex(body)?,
        string_regex(after)?,
    )
        .prop_map(|(head, secret, tail)| (format!("{head}{secret}{tail}"), secret))
        .boxed())
}

/// One secret for each built-in rule, in a form the rule is meant to catch.
fn secrets() -> Result<Union<Secret>, Box<dyn Error>> {
    let value = "[A-Z][a-z0-9]{2,10}[0-9][!#*+/=@^~-][A-Za-z0-9]{2,15}";
    let names = "(password|DB_PASSWORD|api_key|client_secret|accessToken|PGPASSWORD|\u{30D1}\u{30B9}\u{30EF}\u{30FC}\u{30C9})";
    Ok(Union::new([
        secret(
            "(AWS_SECRET_ACCESS_KEY|aws_session_token|AWS_ACCESS_KEY_ID)(=| = |: )",
            "[A-Za-z0-9/+=_-]{16,40}",
            "",
        )?,
        secret(
            "\"(AccessKeyId|SecretAccessKey|SessionToken)\": \"",
            "[A-Za-z0-9/+=]{16,40}",
            "\"",
        )?,
        secret("(AKIA|ASIA)", "[0-9A-Z]{16}", "")?,
        secret(
            "(ghp|gho|ghu|ghs|ghr|github_pat)_",
            "[A-Za-z0-9_]{20,40}",
            "",
        )?,
        secret("glpat-", "[0-9A-Za-z_-]{20,30}", "")?,
        secret("npm_", "[a-z0-9]{36}", "")?,
        secret("AIza", "[0-9A-Za-z_-]{35}", "")?,
        secret("sk-(ant-|proj-)?", "[A-Za-z0-9_-]{20,40}", "")?,
        secret("(sk|rk)_(test|live|prod)_", "[A-Za-z0-9]{10,99}", "")?,
        secret("xox[abposr]-", "[A-Za-z0-9-]{10,40}", "")?,
        secret(
            "xapp-[0-9]-",
            "[A-Z0-9]{4,12}-[0-9]{4,12}-[a-z0-9]{10,40}",
            "",
        )?,
        secret(
            "https://hooks\\.slack\\.com/services/",
            "[A-Za-z0-9+/]{43,56}",
            "",
        )?,
        secret(
            "https://hooks\\.slack\\.com/(workflows|triggers)/",
            "[A-Za-z0-9+/]{43,80}",
            "",
        )?,
        secret("SG\\.", "[A-Za-z0-9_-]{16,30}\\.[A-Za-z0-9_-]{16,40}", "")?,
        secret("ya29\\.", "[0-9A-Za-z_-]{20,40}", "")?,
        secret("1//0", "[0-9A-Za-z_-]{20,40}", "")?,
        secret("GOCSPX-", "[0-9A-Za-z_-]{10,30}", "")?,
        secret("whsec_", "[0-9A-Za-z]{10,40}", "")?,
        secret(
            "Authorization: (Bearer|Basic|Token) ",
            "[A-Za-z0-9._~+/=-]{8,40}",
            "",
        )?,
        secret("(bearer|Bearer|BEARER) ", "[A-Za-z0-9._~+/=-]{20,40}", "")?,
        secret(
            "(mysql|mysqldump|mysqladmin|mariadb) -u[a-z]{1,8} -p",
            "[A-Za-z0-9!#%*+./=?@^_~][\\x21-\\x7E]{7,20}",
            "",
        )?,
        secret(
            "",
            "ey[A-Za-z0-9]{17,30}\\.ey[A-Za-z0-9_-]{17,30}\\.[A-Za-z0-9_-]{10,40}",
            "",
        )?,
        secret(
            "(postgres|redis|https)://[a-z]{1,8}:",
            "[A-Za-z0-9!$*+.=~-]{8,20}",
            "@db\\.example/",
        )?,
        // Split so that secret scanners do not read a key header here.
        secret(
            concat!("-----BEGIN (RSA |EC |OPENSSH )?", "PRIVATE KEY-----\n"),
            "[A-Za-z0-9+/]{20,64}",
            concat!("\n-----END ", "PRIVATE KEY-----"),
        )?,
        secret(&format!("{names}(=| = |: |\u{FF1A})"), value, "")?,
        secret(&format!("{names}(=|: )\""), value, "\"")?,
    ]))
}

/// Text that tends to sit on the edges of the rules.
fn edgy_text() -> impl Strategy<Value = String> {
    let piece = proptest::prop_oneof![
        3 => select(FRAGMENTS).prop_map(str::to_owned),
        1 => hostile_text(),
    ];
    vec(piece, 0..24).prop_map(|pieces| pieces.concat())
}

/// Masks asked for with any user name, git user, and values, as
/// `--mask` and `--mask-values` could give them.
fn any_extra_masks() -> impl Strategy<Value = ExtraMasks> {
    (
        option::of(hostile_text()),
        vec(hostile_text(), 0..3),
        vec((hostile_text(), hostile_text(), hostile_text()), 0..3),
        uniform4(any::<bool>()),
    )
        .prop_map(
            |(user, git_users, values, [aws_accounts, aws_profiles, emails, ips])| ExtraMasks {
                aws_accounts,
                aws_profiles,
                emails,
                git_users,
                ips,
                user,
                values,
            },
        )
}

/// Whether `left` and `right` have the same arrays, the same number of
/// keys in each object, and the same leaves other than strings.
fn same_shape(left: &Value, right: &Value) -> bool {
    if let (Some(items), Some(others)) = (left.as_array(), right.as_array()) {
        return items.len() == others.len()
            && items
                .iter()
                .zip(others)
                .all(|(item, other)| same_shape(item, other));
    }
    if let (Some(fields), Some(others)) = (left.as_object(), right.as_object()) {
        return fields.len() == others.len()
            && fields
                .values()
                .zip(others.values())
                .all(|(field, other)| same_shape(field, other));
    }
    (left.is_string() && right.is_string()) || left == right
}

#[test]
fn every_built_in_secret_is_masked_wherever_it_sits() -> TestResult {
    let redactor = Redactor::new()?;
    let cases = (
        hostile_text(),
        select(SEPARATORS),
        secrets()?,
        select(SEPARATORS),
        hostile_text(),
    );
    for_all(&cases, |(before, left, (secret, body), right, after)| {
        let input = format!("{before}{left}{secret}{right}{after}");
        let mut counts = Counts::default();
        let (masked, changed) = redactor.redact_counted(&input, &mut counts);
        check_eq(
            &masked.contains(&body),
            &false,
            &format!("secret left in {masked:?}"),
        )?;
        check_eq(&changed, &true, "changed")?;
        check_eq(&counts.ranked().is_empty(), &false, "counted")?;
        let mut keyed = serde_json::json!({ input: 1_i32 });
        check_eq(&redactor.redact_json(&mut keyed), &true, "a key changed")?;
        check_eq(
            &keyed.to_string().contains(&body),
            &false,
            &format!("secret left in the key of {keyed}"),
        )?;
        Ok(())
    })
}

#[test]
fn redaction_never_panics_counts_what_it_changes_and_is_idempotent() -> TestResult {
    let redactor = Redactor::new()?;
    for_all(&edgy_text(), |text| {
        let mut counts = Counts::default();
        let (masked, changed) = redactor.redact_counted(&text, &mut counts);
        check_eq(
            &counts.ranked().is_empty(),
            &!changed,
            "counted only when changed",
        )?;
        check_eq(&redactor.redact(&masked).0, &masked, "masking twice")?;
        Ok(())
    })
}

#[test]
fn a_long_value_with_multibyte_characters_never_panics() -> TestResult {
    let redactor = Redactor::new()?;
    let chars = vec(
        select(&["a", "\u{E9}", "\u{65E5}", "\u{1F600}", "\"", " ", "\n"][..]),
        1000..1100,
    );
    let cases = (select(&["password=", "password=\"", "token: '"][..]), chars);
    for_all(&cases, |(head, tail)| {
        drop(redactor.redact(&format!("{head}{}", tail.concat())));
        Ok(())
    })
}

#[test]
fn any_extra_masks_compile_and_never_panic() -> TestResult {
    for_all_cases(
        COMPILING_CASES,
        &(any_extra_masks(), edgy_text()),
        |(extra, text)| {
            let redactor = Redactor::with(extra)?;
            drop(redactor.redact(&text));
            drop(redactor.meta(&text));
            Ok(())
        },
    )
}

#[test]
fn the_user_name_is_masked_in_a_path_in_any_case_and_separator() -> TestResult {
    let names = "[a-z]{2,8}([._-][a-z]{2,8})?";
    let cases = (
        names,
        vec(any::<bool>(), 17),
        select(&[".", "_", "-"][..]),
        hostile_text(),
    );
    for_all_cases(
        COMPILING_CASES,
        &cases,
        |(name, upper, separator, before)| {
            let redactor = Redactor::with(ExtraMasks {
                user: Some(name.clone()),
                ..ExtraMasks::default()
            })?;
            let spelled: String = name
                .chars()
                .zip(upper.iter().chain(iter::repeat(&false)))
                .map(|(ch, &up)| match ch {
                    '.' | '_' | '-' => separator.to_owned(),
                    _ if up => ch.to_uppercase().to_string(),
                    _ => ch.to_string(),
                })
                .collect();
            let segment = format!("/{spelled}/");
            let masked = redactor.meta(&format!("{before} /home{segment}notes"));
            check_eq(&masked.contains(&segment), &false, &masked)?;
            Ok(())
        },
    )
}

#[test]
fn a_value_to_mask_is_left_nowhere() -> TestResult {
    let value_lists = vec("[A-Za-z0-9]{4,12}", 1..4).prop_map(|values| {
        values
            .into_iter()
            .filter(|value| !MARKER.contains(value.as_str()))
            .collect::<Vec<_>>()
    });
    for_all_cases(
        COMPILING_CASES,
        &(value_lists, vec(hostile_text(), 1..5)),
        |(values, pieces)| {
            let redactor = Redactor::with(ExtraMasks {
                values: values
                    .iter()
                    .map(|value| (value.clone(), MARKER.to_owned(), String::new()))
                    .collect(),
                ..ExtraMasks::default()
            })?;
            let text = pieces.join(&values.concat());
            let masked = redactor.meta(&text);
            for value in &values {
                check_eq(
                    &masked.contains(value.as_str()),
                    &false,
                    &format!("{value} in {masked:?}"),
                )?;
            }
            Ok(())
        },
    )
}

#[test]
fn json_masking_keeps_the_shape_and_reports_any_change() -> TestResult {
    let plain = Redactor::new()?;
    let user = Redactor::with(ExtraMasks {
        user: Some("alice".to_owned()),
        ..ExtraMasks::default()
    })?;
    for_all(&json_value(), |value| {
        for redactor in [&plain, &user] {
            let mut masked = value.clone();
            let changed = redactor.redact_json(&mut masked);
            check_eq(&changed, &(masked != value), "changed")?;
            check_eq(
                &same_shape(&masked, &value),
                &true,
                &format!("shape of {masked}"),
            )?;
        }
        Ok(())
    })
}

#[test]
fn counts_merge_in_any_order_and_rank_most_first() -> TestResult {
    let rule_lists = vec(
        select(&["jwt", "api-key", "user", "values/repo"][..]),
        0..12,
    );
    for_all(
        &(rule_lists.clone(), rule_lists.clone(), rule_lists),
        |(first, second, third)| {
            let tally = |names: &[&str]| {
                let mut counts = Counts::default();
                for name in names {
                    counts.add(name);
                }
                counts
            };
            let (one, two, three) = (tally(&first), tally(&second), tally(&third));
            let merged = |parts: [&Counts; 3]| {
                let mut total = Counts::default();
                for part in parts {
                    total.merge(part);
                }
                total
            };
            let forward = merged([&one, &two, &three]);
            check_eq(&merged([&three, &one, &two]), &forward, "order of merging")?;
            let all: Vec<&str> = first.iter().chain(&second).chain(&third).copied().collect();
            check_eq(&forward, &tally(&all), "same as counting all at once")?;
            let ranked = forward.ranked();
            let sorted = ranked
            .windows(2)
            .all(|pair| matches!(pair, [left, right] if left.1 > right.1 || (left.1 == right.1 && left.0 < right.0)));
            check_eq(&sorted, &true, &format!("{ranked:?}"))?;
            Ok(())
        },
    )
}
