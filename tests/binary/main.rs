//! Runs the built binary end to end. The flags and formats are covered by
//! the unit tests; this checks the wiring in `main`: exit codes, stderr,
//! and that `--tz` works from the bundled time zone database.

#[cfg(test)]
mod cases;
