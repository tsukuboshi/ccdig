//! Command-line entry point for ccdig.

#![expect(
    clippy::disallowed_methods,
    reason = "main hands the real command line, stdout, and stderr to ccdig::run"
)]

use std::env;
use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(ccdig::run(
        env::args_os(),
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    ))
}
