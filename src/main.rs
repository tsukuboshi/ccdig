//! Command-line entry point for ccdig.

use std::process::ExitCode;

#[cfg_attr(
    test,
    expect(
        clippy::missing_const_for_fn,
        reason = "the test build treats main as a plain function; this goes away once main calls into the library"
    )
)]
fn main() -> ExitCode {
    ExitCode::SUCCESS
}
