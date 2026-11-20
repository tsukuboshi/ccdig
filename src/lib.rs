//! ccdig digs prompts, replies, tool calls, and tool results out of Claude
//! Code session transcripts.

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the commands that report these come in a later stage"
    )
)]
mod failure;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the collector that calls this comes in a later stage"
    )
)]
mod redact;
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
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the collector that calls this comes in a later stage"
    )
)]
mod transcript;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the collector that calls this comes in a later stage"
    )
)]
mod when;
