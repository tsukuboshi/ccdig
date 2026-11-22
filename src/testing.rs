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
use std::fs::File;
use std::path::Path;
use std::time::SystemTime;
use std::{env, fs};

use jiff::Timestamp;
use jiff::tz::{TimeZone, TimeZoneDatabase};

use crate::when::Stamp;

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
