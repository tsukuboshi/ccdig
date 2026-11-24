//! Helpers shared by the unit tests.
//!
//! `panic_in_result_fn` forbids `assert_eq!` in tests that return `Result`,
//! so comparisons return an error instead of panicking.

#![expect(
    clippy::disallowed_methods,
    reason = "golden files are read and written on disk"
)]

use core::error::Error;
use core::fmt::{self, Debug, Display, Formatter};
use core::hash::BuildHasher as _;
use std::fs::File;
use std::hash::RandomState;
use std::path::Path;
use std::time::SystemTime;
use std::{env, fs};

use jiff::Timestamp;
use jiff::tz::{TimeZone, TimeZoneDatabase};
use proptest::arbitrary::any;
use proptest::char::range;
use proptest::collection::vec;
use proptest::option;
use proptest::prop_oneof;
use proptest::sample::select;
use proptest::strategy::{Just, Strategy};
use proptest::test_runner::{Config, RngSeed, TestCaseError, TestError, TestRunner};
use serde_json::Value;

use crate::when::Stamp;

/// Characters that sanitizers and parsers treat specially: line breaks,
/// terminal controls, bidi overrides, joiners, odd whitespace, inline
/// Markdown, and the line separators JavaScript does not allow in strings.
const HOSTILE: &[char] = &[
    '\r', '\n', '\t', ' ', '\u{1B}', '\u{7}', '\u{9B}', '\u{202E}', '\u{2066}', '\u{200B}',
    '\u{200C}', '\u{200D}', '\u{FEFF}', '\u{A0}', '\u{3000}', '\\', '`', '*', '_', '[', ']', '<',
    '>', '!', '&', '|', '#', '"', '{', '}', ':', '=', '\u{2028}', '\u{2029}',
];

/// The error a test returns, so `?` works on any error type.
pub type TestResult = Result<(), Box<dyn Error>>;

/// Two values a test expected to be equal.
pub struct Mismatch {
    /// What the comparison was checking.
    context: String,
    /// The value the code produced, in `Debug` form.
    got: String,
    /// The value the test expected, in `Debug` form.
    want: String,
}

impl Display for Mismatch {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}\n  got:  {}\n  want: {}",
            self.context, self.got, self.want
        )
    }
}

/// A failing test prints its error with `Debug`, so show the readable form.
impl Debug for Mismatch {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

impl Error for Mismatch {}

/// Returns an error describing both values unless `got` equals `want`.
pub fn check_eq<T>(got: &T, want: &T, context: &str) -> Result<(), Mismatch>
where
    T: PartialEq + Debug + ?Sized,
{
    if got == want {
        return Ok(());
    }
    Err(Mismatch {
        context: context.to_owned(),
        got: format!("{got:?}"),
        want: format!("{want:?}"),
    })
}

/// Compares `got` with `testdata/<name>`, after rewriting the file from
/// `got` when `UPDATE_GOLDEN` is set. Review the diff before committing.
pub fn golden(name: &str, got: &str) -> TestResult {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join(name);
    if env::var_os("UPDATE_GOLDEN").is_some() {
        fs::write(&path, got)?;
    }
    let want = fs::read_to_string(&path)?;
    check_eq(got, want.as_str(), name)?;
    Ok(())
}

/// Runs `property` on values drawn from `strategy`, and on failure returns
/// the smallest input that still fails, with the property's error.
///
/// `PROPTEST_CASES` sets how many values to try and `PROPTEST_RNG_SEED`
/// replays a run. Failing inputs are not saved to disk, since they can look
/// like secrets; add the reported input to a table-driven test instead.
pub fn for_all<S, P>(strategy: &S, property: P) -> TestResult
where
    S: Strategy,
    P: Fn(S::Value) -> TestResult,
{
    for_all_cases(Config::default().cases, strategy, property)
}

/// `for_all` on `cases` values, whatever `PROPTEST_CASES` says, for a
/// property that builds something costly for each value.
pub fn for_all_cases<S, P>(cases: u32, strategy: &S, property: P) -> TestResult
where
    S: Strategy,
    P: Fn(S::Value) -> TestResult,
{
    let config = Config::default();
    // Pick the seed here, unless PROPTEST_RNG_SEED did, so that a failure
    // can say which one replays it.
    let seed = match config.rng_seed {
        RngSeed::Fixed(seed) => seed,
        RngSeed::Random => RandomState::new().hash_one("seed"),
    };
    let mut runner = TestRunner::new(Config {
        cases,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(seed),
        ..config
    });
    let outcome = runner.run(strategy, |value| {
        property(value).map_err(|error| TestCaseError::fail(error.to_string()))
    });
    match outcome {
        Ok(()) => Ok(()),
        Err(TestError::Fail(reason, value)) => Err(format!(
            "{reason}\n  input: {value:?}\n  replay with PROPTEST_RNG_SEED={seed}"
        )
        .into()),
        Err(TestError::Abort(reason)) => Err(format!("property aborted: {reason}").into()),
    }
}

/// Whether `ch` could drive a terminal or make text read differently from
/// what it is: a control character other than newline and tab, or a bidi
/// override or isolate.
pub fn drives_the_terminal(ch: char) -> bool {
    let control = ch.is_control() && ch != '\n' && ch != '\t';
    let bidi = matches!(ch, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    control || bidi
}

/// Text of up to 64 characters, any of Unicode but mostly the ones in
/// [`HOSTILE`] and ASCII, so that the inputs a sanitizer must handle come up
/// often.
pub fn hostile_text() -> impl Strategy<Value = String> {
    let ch = prop_oneof![
        2 => select(HOSTILE),
        2 => range('a', 'z'),
        1 => any::<char>(),
    ];
    vec(ch, 0..=64).prop_map(String::from_iter)
}

/// A strict RFC 3339 time as Claude Code writes it, with up to nine fraction
/// digits and an offset that is `Z` or `±HH:MM`. Days stop at 28, so every
/// date exists.
pub fn rfc3339_text() -> impl Strategy<Value = String> {
    let dates = (0_u16..=9999, 1_u8..=12, 1_u8..=28);
    let times = (0_u8..=23, 0_u8..=59, 0_u8..=59);
    let fractions = option::of("[0-9]{1,9}");
    let offsets = prop_oneof![
        Just("Z".to_owned()),
        ("[+-]", 0_u8..=23, 0_u8..=59)
            .prop_map(|(sign, hour, minute)| format!("{sign}{hour:02}:{minute:02}")),
    ];
    (dates, times, fractions, offsets).prop_map(
        |((year, month, day), (hour, minute, second), digits, offset)| {
            let fraction = digits.map_or_default(|text| format!(".{text}"));
            format!(
                "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{fraction}{offset}"
            )
        },
    )
}

/// Object keys: names that hold credentials, names that do not, and any
/// text.
pub fn json_key() -> impl Strategy<Value = String> {
    prop_oneof![
        select(&["password", "api_key", "token", "note", "path", "count"][..])
            .prop_map(str::to_owned),
        hostile_text(),
    ]
}

/// A JSON value up to three levels deep, with hostile strings and keys.
pub fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        hostile_text().prop_map(Value::String),
    ];
    leaf.prop_recursive(3, 32, 4, |inner| {
        prop_oneof![
            vec(inner.clone(), 0..4).prop_map(Value::Array),
            vec((json_key(), inner), 0..4)
                .prop_map(|fields| Value::Object(fields.into_iter().collect())),
        ]
    })
}

#[cfg(test)]
mod tests;

/// Formats a fixed instant in `zone`, to tell zones apart.
///
/// # Errors
///
/// When the instant cannot be shown in `zone`.
pub fn label_in(zone: &TimeZone) -> Result<String, Box<dyn Error>> {
    let stamp = Stamp::parse_rfc3339("2026-10-13T01:00:00Z").ok_or("bad fixture")?;
    Ok(stamp.minute_label(zone)?)
}

/// The zone `name` from the database bundled into the binary, so the tests
/// do not depend on the host's.
///
/// # Errors
///
/// When the bundled database has no such zone.
pub fn zone(name: &str) -> Result<TimeZone, jiff::Error> {
    TimeZoneDatabase::bundled().get(name)
}

/// The RFC 3339 instant `text`, as a file time.
///
/// # Errors
///
/// When `text` is not an instant a file time can hold.
pub fn time(text: &str) -> Result<SystemTime, jiff::Error> {
    let instant: Timestamp = text.parse()?;
    SystemTime::try_from(instant)
}

/// Writes a transcript of `lines` at `path`, with its directory, and sets
/// its modification time to `mtime`, since the code reads it.
///
/// # Errors
///
/// When the file cannot be written.
pub fn write_transcript(path: &Path, mtime: SystemTime, lines: &[&str]) -> TestResult {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut text = lines.join("\n");
    text.push('\n');
    fs::write(path, text)?;
    File::options()
        .write(true)
        .open(path)?
        .set_modified(mtime)?;
    Ok(())
}
