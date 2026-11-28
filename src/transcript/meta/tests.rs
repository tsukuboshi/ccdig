//! Tests for decoding a sub-agent's `.meta.json`.

use super::{Spawn, decode};
use crate::testing::{TestResult, check_eq};

/// A spawn with these fields.
fn spawn(agent_type: &str, description: &str, tool_use_id: &str) -> Spawn {
    Spawn {
        agent_type: agent_type.to_owned(),
        description: description.to_owned(),
        tool_use_id: tool_use_id.to_owned(),
    }
}

#[test]
fn the_type_the_description_and_the_call_are_read_and_the_rest_ignored() -> TestResult {
    for (name, json, want) in [
        (
            "all three",
            r#"{"agentType":"Explore","description":"Find the cart total","toolUseId":"toolu_1","spawnDepth":1}"#,
            Some(spawn("Explore", "Find the cart total", "toolu_1")),
        ),
        (
            "a forked skill, which names no call",
            r#"{"agentType":"general-purpose","description":"Run the skill","name":"review"}"#,
            Some(spawn("general-purpose", "Run the skill", "")),
        ),
        (
            "null reads as empty",
            r#"{"agentType":null,"description":"d","toolUseId":null}"#,
            Some(spawn("", "d", "")),
        ),
        (
            "only unknown keys",
            r#"{"spawnDepth":2,"parentAgentId":"a1"}"#,
            Some(Spawn::default()),
        ),
        ("an empty object", "{}", Some(Spawn::default())),
        ("an array", r#"["Explore"]"#, None),
        ("a string", r#""Explore""#, None),
        ("broken JSON", r#"{"agentType":"#, None),
        (
            "a mistyped key hides only itself",
            r#"{"agentType":"Explore","description":{"text":"d"},"toolUseId":7}"#,
            Some(spawn("Explore", "", "")),
        ),
        ("empty", "", None),
    ] {
        check_eq(&decode(json.as_bytes()), &want, name)?;
    }
    Ok(())
}

#[test]
fn invalid_utf8_becomes_the_replacement_character() -> TestResult {
    let mut bytes = br#"{"description":"find "#.to_vec();
    bytes.push(0xFF);
    bytes.extend_from_slice(br#""}"#);
    check_eq(
        &decode(&bytes).map(|found| found.description),
        &Some("find \u{FFFD}".to_owned()),
        "description",
    )?;
    Ok(())
}
