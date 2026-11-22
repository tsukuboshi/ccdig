//! The shell around the pipeline: parse the command line, read the
//! environment, resolve them into a `Config`, run it, and write what it
//! printed, its warnings, and its exit status.

mod args;
mod environment;
mod resolve;
mod values;

use std::ffi::OsString;
use std::io::{ErrorKind, Write};

use self::args::{Parsed, parse};
use self::environment::Environment;
use self::resolve::resolve;
use crate::config::OnSkip;
use crate::pipeline::execute;
use crate::text::one_line;

/// Writes `message` as one `ccdig: ` line on stderr and returns the exit
/// status 1. A failed write is ignored: there is nowhere left to report it.
fn fail(stderr: &mut dyn Write, message: &str) -> u8 {
    drop(writeln!(stderr, "ccdig: {}", one_line(message)));
    1
}

/// Runs ccdig with `args`; see `crate::run`.
pub fn run<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    run_in(args, &Environment::read(), stdout, stderr)
}

/// Runs ccdig with `args` in `environment`.
fn run_in<I, T>(
    args: I,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let (earlier, command) = match parse(args) {
        Parsed::Print(text) => return write_out(stdout, stderr, &text),
        Parsed::Run { command, earlier } => (*earlier, *command),
        Parsed::Usage(line) => return fail(stderr, &line),
    };
    match resolve(earlier, command, environment).and_then(execute) {
        Ok(printed) => {
            for warning in &printed.warnings {
                drop(writeln!(stderr, "{warning}"));
            }
            let code = write_out(stdout, stderr, &printed.out);
            if let Some(ref report) = printed.report {
                drop(writeln!(stderr, "{report}"));
            }
            if code == 0 && printed.on_skip == OnSkip::Fail && !printed.warnings.is_empty() {
                return fail(stderr, "failing as --on-skip is fail: input was skipped");
            }
            code
        }
        Err(failure) => fail(stderr, &failure.to_string()),
    }
}

/// Writes `out` to `stdout`. A closed pipe, as under `| head`, ends the run
/// quietly with status 0.
fn write_out(stdout: &mut dyn Write, stderr: &mut dyn Write, out: &str) -> u8 {
    match stdout
        .write_all(out.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => 0,
        Err(error) if error.kind() == ErrorKind::BrokenPipe => 0,
        Err(error) => fail(stderr, &format!("write output: {error}")),
    }
}

#[cfg(test)]
mod tests;
