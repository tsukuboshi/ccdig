//! Writing records as JSON Lines, JSON, Markdown, and the digest. Each
//! function returns the whole output; the caller writes it.

pub mod digest;
pub mod json;
pub mod markdown;

#[cfg(test)]
mod tests;
