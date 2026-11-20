//! Command-line entry point for ccdig.

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
