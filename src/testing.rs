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
use std::path::Path;
use std::{env, fs};

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
