//! Writing records as JSON Lines, JSON, Markdown, and the digest. Each
//! function returns the whole output; the caller writes it. `view` decides
//! what the records show apart from how a format words and escapes it.

pub mod digest;
pub mod json;
pub mod markdown;
mod view;
mod words;

#[cfg(test)]
mod tests;
