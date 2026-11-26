//! Streaming a session transcript one line at a time.

#![expect(
    clippy::disallowed_methods,
    reason = "opening a transcript is the start of the pipeline"
)]

use std::fs::File;
use std::io::{self, BufRead, BufReader, ErrorKind};
use std::path::Path;

use memchr::memchr;
use memchr::memmem;

use super::entry::{Entry, Role, decode_line};

/// The read buffer `open` gives a file. Lines run longer; the buffer only
/// sets how much is read at a time.
const BUFFER_BYTES: usize = 0x0010_0000;

/// The longest line, 64 MiB, that is decoded. Longer lines are skipped and
/// counted in `ReadStats::oversized`.
pub const MAX_LINE_BYTES: usize = 0x0400_0000;

/// What the last line read was.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Line {
    /// The line is in the buffer.
    Fits,
    /// The line was longer than the limit and was dropped.
    Oversized,
}

/// Reads the entries of one transcript, in file order, with their 1-based
/// line numbers. Lines of other kinds, lines over the limit, and lines that
/// do not decode are skipped; the last two are counted in `stats`.
pub struct Lines<R> {
    /// The current line, without its newline.
    buf: Vec<u8>,
    /// The transcript.
    inner: R,
    /// The longest line kept, in bytes.
    max: usize,
    /// The number of the current line.
    number: usize,
    /// Which entries besides `ai-title` are delivered.
    roles: Roles,
    /// What has been skipped so far.
    stats: ReadStats,
}

impl<R: BufRead> Lines<R> {
    /// Reads `inner`, keeping lines of at most `max` bytes.
    pub const fn new(inner: R, max: usize, roles: Roles) -> Self {
        Self {
            buf: Vec::new(),
            inner,
            max,
            number: 0,
            roles,
            stats: ReadStats {
                oversized: 0,
                undecodable: 0,
            },
        }
    }

    /// Reads the next line into `buf`, or drops it when it is over the
    /// limit, holding no more than `max` bytes of it. `None` after the last
    /// line; a last line without a newline still counts.
    fn next_line(&mut self) -> io::Result<Option<Line>> {
        self.buf.clear();
        let mut oversized = false;
        loop {
            let available = match self.inner.fill_buf() {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            if available.is_empty() {
                return Ok(if oversized {
                    Some(Line::Oversized)
                } else if self.buf.is_empty() {
                    None
                } else {
                    Some(Line::Fits)
                });
            }
            let (chunk, used, ended) =
                match memchr(b'\n', available).and_then(|at| available.split_at_checked(at)) {
                    Some((chunk, _)) => (chunk, chunk.len().saturating_add(1), true),
                    None => (available, available.len(), false),
                };
            if !oversized {
                if self.buf.len().saturating_add(chunk.len()) > self.max {
                    oversized = true;
                    self.buf.clear();
                } else {
                    self.buf.extend_from_slice(chunk);
                }
            }
            self.inner.consume(used);
            if ended {
                return Ok(Some(if oversized {
                    Line::Oversized
                } else {
                    Line::Fits
                }));
            }
        }
    }

    /// What has been skipped so far.
    #[must_use]
    pub const fn stats(&self) -> ReadStats {
        self.stats
    }

    /// Whether the current line may hold a wanted entry. A substring check
    /// skips most lines without decoding them. It matches values, not
    /// `"key":"value"` pairs, so spacing after the colon does not matter;
    /// the decoded type still decides.
    fn worth_decoding(&self) -> bool {
        let has = |marker: &str| memmem::find(&self.buf, marker.as_bytes()).is_some();
        has(r#""ai-title""#)
            || (self.roles.user && has(r#""user""#))
            || (self.roles.assistant && has(r#""assistant""#))
            || (has(r#""assistant""#) && self.roles.calls_to.iter().any(|name| has(name)))
    }
}

impl<R: BufRead> Iterator for Lines<R> {
    type Item = io::Result<(usize, Entry)>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let line = match self.next_line().transpose()? {
                Ok(line) => line,
                Err(error) => return Some(Err(error)),
            };
            self.number = self.number.saturating_add(1);
            if line == Line::Oversized {
                self.stats.oversized = self.stats.oversized.saturating_add(1);
                continue;
            }
            if !self.worth_decoding() {
                continue;
            }
            let Some(entry) = decode_line(&self.buf) else {
                self.stats.undecodable = self.stats.undecodable.saturating_add(1);
                continue;
            };
            let wanted = match entry.role {
                Role::AiTitle => true,
                Role::Assistant => self.roles.assistant || !self.roles.calls_to.is_empty(),
                Role::User => self.roles.user,
                Role::Other => false,
            };
            if wanted {
                return Some(Ok((self.number, entry)));
            }
        }
    }
}

/// What a read skipped.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReadStats {
    /// Lines longer than the limit.
    pub oversized: usize,
    /// Lines that looked like wanted entries but did not decode: a partial
    /// last line of a live session, or a format change ccdig does not
    /// understand yet.
    pub undecodable: usize,
}

/// Which entries a read delivers besides `ai-title`, which is always read
/// for the session title. Leaving a role out also leaves its broken lines
/// out of `ReadStats::undecodable`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Roles {
    /// The assistant's messages.
    pub assistant: bool,
    /// Without `assistant`, the assistant's messages that name one of these
    /// tools, as a call to it does, so a reader that needs only those calls
    /// does not decode every assistant line.
    pub calls_to: &'static [&'static str],
    /// User messages, which hold prompts and tool results.
    pub user: bool,
}

impl Roles {
    /// Assistant messages only, for replies and tool calls.
    pub const ASSISTANT: Self = Self {
        assistant: true,
        calls_to: &[],
        user: false,
    };
    /// Both, for tool results and the calls they answer.
    pub const BOTH: Self = Self {
        assistant: true,
        calls_to: &[],
        user: true,
    };
    /// User messages only, for prompts.
    pub const USER: Self = Self {
        assistant: false,
        calls_to: &[],
        user: true,
    };
}

/// Opens the transcript at `path` for reading with `Lines`.
///
/// # Errors
///
/// When the file cannot be opened. Read errors come from the iterator.
pub fn open(path: &Path, max: usize, roles: Roles) -> io::Result<Lines<BufReader<File>>> {
    let file = File::open(path)?;
    Ok(Lines::new(
        BufReader::with_capacity(BUFFER_BYTES, file),
        max,
        roles,
    ))
}

#[cfg(test)]
mod tests;
