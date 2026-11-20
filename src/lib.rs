//! ccdig pulls the logs you want to share out of Claude Code session
//! transcripts, formatted and masked.

extern crate alloc;

mod classify;
mod cli;
mod collect;
mod failure;
mod redact;
mod render;
mod select;
#[cfg(test)]
mod testing;
mod text;
mod transcript;
mod when;

use std::ffi::OsString;
use std::io::Write;

/// Runs ccdig with `args`, the program name first, and returns the exit
/// status. Errors are one line on `stderr` with status 1; help goes to
/// `stdout` with status 0, as does a run with no subcommand.
pub fn run<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    cli::run(args, stdout, stderr)
}
