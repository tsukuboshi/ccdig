//! ccdig digs prompts, replies, tool calls, and tool results out of Claude
//! Code session transcripts.

#[cfg(test)]
mod testing;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the renderers that call these come in a later stage"
    )
)]
mod text;
