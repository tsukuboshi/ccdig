//! What ccdig reads from outside the command line: the environment
//! variables, git config, the system time zone, and the files flags name.
//! The rest of the crate gets what these return and reads nothing itself.

#![expect(
    clippy::disallowed_methods,
    reason = "this module is where ccdig reads the environment, git config, and the files flags name"
)]

use std::ffi::OsString;
use std::{env, fs, io, process};

use jiff::tz::TimeZone;

use crate::failure::Failure;

/// The environment a run sees. `CCDIG_*` and `CLAUDE_CONFIG_DIR` are
/// defaults for the flags they name, used when neither level of the command
/// line gives the flag; an empty one counts as unset.
#[derive(Default)]
pub struct Environment {
    /// `CLAUDE_CONFIG_DIR`, Claude Code's config directory, whose
    /// `projects` is the root when neither `--root` nor `CCDIG_ROOT` is
    /// given.
    pub claude_config_dir: Option<OsString>,
    /// Looks up a git config value for `--mask git-user`; `None` runs
    /// `git config --get`.
    pub git_config: Option<fn(&str) -> Option<String>>,
    /// `HOME`, for the default root and `--mask user`.
    pub home: Option<OsString>,
    /// `CCDIG_MASK`, for `--mask`.
    pub mask: Option<OsString>,
    /// `CCDIG_MASK_VALUES`, for `--mask-values`.
    pub mask_values: Option<OsString>,
    /// `CCDIG_ON_SKIP`, for `--on-skip`.
    pub on_skip: Option<OsString>,
    /// `CCDIG_ROOT`, for `--root`.
    pub root: Option<OsString>,
    /// `CLAUDE_CODE_SESSION_ID`, the session Claude Code runs ccdig in,
    /// which `--session current` names.
    pub session_id: Option<OsString>,
    /// `TZ`, for the `Local` zone. Unlike the others, an empty one is kept:
    /// it means UTC, while an unset one means the system zone.
    pub tz: Option<OsString>,
}

impl Environment {
    /// The value of `key` in git config, through `git_config` when it is
    /// set.
    pub fn git_config(&self, key: &str) -> Option<String> {
        self.git_config
            .map_or_else(|| git_config(key), |lookup| lookup(key))
    }

    /// The system zone, chosen by `TZ`.
    pub fn local_zone(&self) -> TimeZone {
        local_zone(self.tz.as_deref().map(|tz| {
            // A TZ that is not UTF-8 names no zone, which falls back to UTC.
            tz.to_str().unwrap_or("Mars/Olympus")
        }))
    }

    /// Reads the variables, leaving out empty ones but `TZ`.
    pub fn read() -> Self {
        let var = |name: &str| env::var_os(name).filter(|value| !value.is_empty());
        Self {
            claude_config_dir: var("CLAUDE_CONFIG_DIR"),
            git_config: None,
            home: var("HOME"),
            mask: var("CCDIG_MASK"),
            mask_values: var("CCDIG_MASK_VALUES"),
            on_skip: var("CCDIG_ON_SKIP"),
            root: var("CCDIG_ROOT"),
            session_id: var("CLAUDE_CODE_SESSION_ID"),
            tz: env::var_os("TZ"),
        }
    }

    /// The text of the file at `path`, for a flag that names one.
    ///
    /// # Errors
    ///
    /// When the file cannot be read or is not UTF-8.
    pub fn read_file(path: &str) -> io::Result<String> {
        fs::read_to_string(path)
    }

    /// Resolves `--tz`: empty and `UTC` are UTC, `Local` is the system zone,
    /// and anything else is an IANA name, looked up in the host's zoneinfo
    /// and then in the copy bundled into the binary.
    ///
    /// # Errors
    ///
    /// When no zone has the name.
    pub fn zone(&self, name: &str) -> Result<TimeZone, Failure> {
        match name {
            "" | "UTC" => Ok(TimeZone::UTC),
            "Local" => Ok(self.local_zone()),
            _ => TimeZone::get(name).map_err(|source| Failure::UnknownZone {
                name: name.to_owned(),
                source,
            }),
        }
    }
}

/// The value of `key` in git config, or `None` when git is not there or
/// the value is not set or empty.
fn git_config(key: &str) -> Option<String> {
    let output = process::Command::new("git")
        .args(["config", "--get", key])
        .output()
        .ok()?;
    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    (output.status.success() && !trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// The system zone, chosen from the value of `TZ` the way Go did: unset
/// reads `/etc/localtime`; otherwise one leading `:` is dropped, an absolute
/// path is read as a zoneinfo file, and anything else is an IANA name. Any
/// failure, and an empty value, gives UTC.
fn local_zone(tz: Option<&str>) -> TimeZone {
    let Some(value) = tz else {
        return TimeZone::try_system().unwrap_or(TimeZone::UTC);
    };
    let name = value.strip_prefix(':').unwrap_or(value);
    if name.is_empty() || name == "UTC" {
        return TimeZone::UTC;
    }
    if name.starts_with('/') {
        return fs::read(name)
            .ok()
            .and_then(|data| TimeZone::tzif(name, &data).ok())
            .unwrap_or(TimeZone::UTC);
    }
    TimeZone::get(name).unwrap_or(TimeZone::UTC)
}

#[cfg(test)]
mod tests;
