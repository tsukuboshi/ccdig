//! Tests for decoding one transcript line, including assistant lines and
//! tool blocks.

use serde_json::json;

use super::{Block, Entry, Flags, Role, ToolResult, ToolUse, decode_line};
use crate::testing::{TestResult, check_eq};
use crate::when::Stamp;

/// Decodes `line`, failing the test when it is not decodable.
fn decode(line: &str) -> Result<Entry, String> {
    decode_line(line.as_bytes()).ok_or_else(|| format!("undecodable: {line}"))
}

#[test]
fn a_user_line_with_string_content_keeps_its_metadata() -> TestResult {
    let entry = decode(concat!(
        r#"{"type":"user","uuid":"u-1","sessionId":"s-1","#,
        r#""timestamp":"2026-10-13T01:23:45.678Z","cwd":"/Users/alice/blog","gitBranch":"main","#,
        r#""message":{"role":"user","content":"draft the weekly report"}}"#,
    ))?;
    check_eq(&entry.role, &Role::User, "role")?;
    check_eq(entry.uuid.as_str(), "u-1", "uuid")?;
    check_eq(entry.session_id.as_str(), "s-1", "session")?;
    check_eq(entry.cwd.as_str(), "/Users/alice/blog", "cwd")?;
    check_eq(entry.git_branch.as_str(), "main", "branch")?;
    check_eq(
        &entry.timestamp,
        &Stamp::parse_rfc3339("2026-10-13T01:23:45.678Z"),
        "timestamp",
    )?;
    check_eq(
        entry.content.text.as_str(),
        "draft the weekly report",
        "text",
    )?;
    Ok(())
}

#[test]
fn array_content_joins_text_blocks_and_skips_images() -> TestResult {
    let entry = decode(concat!(
        r#"{"type":"user","message":{"role":"user","content":["#,
        r#"{"type":"text","text":"what is wrong with this screen?"},"#,
        r#"{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgo="}},"#,
        r#"{"type":"text","text":"it shows a blank page"}]}}"#,
    ))?;
    check_eq(
        entry.content.text.as_str(),
        "what is wrong with this screen?\nit shows a blank page",
        "text",
    )?;
    Ok(())
}

#[test]
fn an_ai_title_line_carries_the_session_title() -> TestResult {
    let entry = decode(r#"{"type":"ai-title","sessionId":"s-1","aiTitle":"Weekly report draft"}"#)?;
    check_eq(&entry.role, &Role::AiTitle, "role")?;
    check_eq(entry.title.as_str(), "Weekly report draft", "title")?;
    Ok(())
}

#[test]
fn an_unknown_type_decodes_with_no_text() -> TestResult {
    let entry = decode(r#"{"type":"permission-mode","permissionMode":"auto","sessionId":"s-1"}"#)?;
    check_eq(&entry.role, &Role::Other, "role")?;
    check_eq(entry.content.text.as_str(), "", "text")?;
    Ok(())
}

#[test]
fn truncated_json_and_a_top_level_array_are_undecodable() -> TestResult {
    check_eq(
        &decode_line(br#"{"type":"user","#).is_none(),
        &true,
        "truncated",
    )?;
    check_eq(&decode_line(br#"["user"]"#).is_none(), &true, "array")?;
    Ok(())
}

#[test]
fn classification_flags_and_origin_are_read() -> TestResult {
    for (name, line, flags, origin) in [
        (
            "plain prompt from an old session without origin",
            r#"{"type":"user","isSidechain":false,"message":{"role":"user","content":"hi"}}"#,
            Flags::default(),
            None,
        ),
        (
            "human origin",
            r#"{"type":"user","origin":{"kind":"human"},"message":{"role":"user","content":"hi"}}"#,
            Flags::default(),
            Some("human"),
        ),
        (
            "task notification origin",
            r#"{"type":"user","origin":{"kind":"task-notification"},"message":{"role":"user","content":"x"}}"#,
            Flags::default(),
            Some("task-notification"),
        ),
        (
            "an origin without a kind",
            r#"{"type":"user","origin":{},"message":{"role":"user","content":"hi"}}"#,
            Flags::default(),
            Some(""),
        ),
        (
            "injected skill body",
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Base directory"}}"#,
            Flags {
                meta: true,
                ..Flags::default()
            },
            None,
        ),
        (
            "sub-agent traffic",
            r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"explore"}}"#,
            Flags {
                sidechain: true,
                ..Flags::default()
            },
            None,
        ),
        (
            "compaction summary",
            r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"continued"}}"#,
            Flags {
                compact_summary: true,
                ..Flags::default()
            },
            None,
        ),
    ] {
        let entry = decode(line)?;
        check_eq(&entry.flags, &flags, name)?;
        check_eq(&entry.origin.as_deref(), &origin, name)?;
    }
    Ok(())
}

#[test]
fn null_fields_read_as_empty() -> TestResult {
    let entry = decode(concat!(
        r#"{"type":"user","uuid":null,"cwd":null,"isMeta":null,"origin":null,"#,
        r#""timestamp":null,"message":{"content":null}}"#,
    ))?;
    check_eq(entry.uuid.as_str(), "", "uuid")?;
    check_eq(&entry.flags, &Flags::default(), "flags")?;
    check_eq(&entry.origin, &None, "origin")?;
    check_eq(&entry.timestamp, &None, "timestamp")?;
    check_eq(entry.content.text.as_str(), "", "text")?;
    Ok(())
}

#[test]
fn a_timestamp_that_is_not_rfc3339_makes_the_line_undecodable() -> TestResult {
    let line = r#"{"type":"user","timestamp":"2026-10-13 00:00:00","message":{"content":"hi"}}"#;
    check_eq(
        &decode_line(line.as_bytes()).is_none(),
        &true,
        "space-separated",
    )?;
    let missing = decode(r#"{"type":"user","message":{"content":"hi"}}"#)?;
    check_eq(&missing.timestamp, &None, "missing")?;
    Ok(())
}

#[test]
fn invalid_utf8_becomes_a_replacement_character() -> TestResult {
    let line = b"{\"type\":\"user\",\"message\":{\"content\":\"a\xFFb\"}}";
    let entry = decode_line(line).ok_or("undecodable")?;
    check_eq(entry.content.text.as_str(), "a\u{FFFD}b", "text")?;
    Ok(())
}

#[test]
fn a_mistyped_block_empties_the_prompt_text() -> TestResult {
    let entry = decode(concat!(
        r#"{"type":"user","message":{"content":["#,
        r#"{"type":"text","text":"kept by itself"},{"type":"text","text":5}]}}"#,
    ))?;
    check_eq(entry.content.text.as_str(), "", "prompt text")?;
    check_eq(
        &entry.content.blocks,
        &vec![Block::Text("kept by itself".to_owned()), Block::Other],
        "blocks",
    )?;
    Ok(())
}

#[test]
fn an_assistant_line_keeps_every_block_in_order() -> TestResult {
    let entry = decode(concat!(
        r#"{"type":"assistant","uuid":"a-1","attributionSkill":"tdd","#,
        r#""message":{"model":"claude-opus-5-5","content":["#,
        r#"{"type":"thinking","thinking":"hmm","signature":"x"},"#,
        r#"{"type":"text","text":"Running the tests."},"#,
        r#"{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"cargo test"}}]}}"#,
    ))?;
    check_eq(&entry.role, &Role::Assistant, "role")?;
    check_eq(entry.model.as_str(), "claude-opus-5-5", "model")?;
    check_eq(entry.skill.as_str(), "tdd", "skill")?;
    check_eq(
        &entry.content.blocks,
        &vec![
            Block::Other,
            Block::Text("Running the tests.".to_owned()),
            Block::ToolUse(ToolUse {
                id: "toolu_1".to_owned(),
                input: json!({"command": "cargo test"}),
                name: "Bash".to_owned(),
            }),
        ],
        "blocks",
    )?;
    Ok(())
}

#[test]
fn tool_results_read_string_and_array_content_and_the_error_flag() -> TestResult {
    let entry = decode(concat!(
        r#"{"type":"user","message":{"content":["#,
        r#"{"type":"tool_result","tool_use_id":"toolu_1","content":"ok","is_error":false},"#,
        r#"{"type":"tool_result","tool_use_id":"toolu_2","is_error":true,"content":["#,
        r#"{"type":"text","text":"line one"},{"type":"image","source":{}},{"type":"text","text":"line two"}]},"#,
        r#"{"type":"tool_result","tool_use_id":"toolu_3"}]}}"#,
    ))?;
    check_eq(&entry.content.has_tool_result, &true, "has tool result")?;
    check_eq(entry.content.text.as_str(), "", "prompt text")?;
    check_eq(
        &entry.content.blocks,
        &vec![
            Block::ToolResult(ToolResult {
                is_error: false,
                text: "ok".to_owned(),
                tool_use_id: "toolu_1".to_owned(),
            }),
            Block::ToolResult(ToolResult {
                is_error: true,
                text: "line one\nline two".to_owned(),
                tool_use_id: "toolu_2".to_owned(),
            }),
            Block::ToolResult(ToolResult {
                is_error: false,
                text: String::new(),
                tool_use_id: "toolu_3".to_owned(),
            }),
        ],
        "blocks",
    )?;
    Ok(())
}

#[test]
fn a_tool_use_without_a_name_is_not_a_tool_call() -> TestResult {
    let entry = decode(
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","input":{}}]}}"#,
    )?;
    check_eq(&entry.content.blocks, &vec![Block::Other], "blocks")?;
    Ok(())
}

#[test]
fn wire_tool_inputs_replace_the_input_of_the_call_with_the_same_id() -> TestResult {
    let entry = decode(concat!(
        r#"{"type":"assistant","message":{"content":["#,
        r#"{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo cut"}},"#,
        r#"{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"echo kept"}},"#,
        r#"{"type":"tool_use","id":"toolu_3","name":"Bash","input":{"command":"echo also kept"}}]},"#,
        r#""wireToolInputs":{"toolu_1":{"command":"echo cut and the rest"},"toolu_3":"not an object","toolu_9":{"command":"echo nothing calls me"}}}"#,
    ))?;
    let inputs: Vec<_> = entry
        .content
        .blocks
        .iter()
        .filter_map(|block| match *block {
            Block::ToolUse(ref call) => Some(call.input.clone()),
            Block::Other | Block::Text(_) | Block::ToolResult(_) => None,
        })
        .collect();
    check_eq(
        &inputs,
        &vec![
            json!({"command": "echo cut and the rest"}),
            json!({"command": "echo kept"}),
            json!({"command": "echo also kept"}),
        ],
        "inputs",
    )?;
    Ok(())
}

#[test]
fn wire_tool_inputs_that_are_not_an_object_leave_the_line_as_it_is() -> TestResult {
    for wire in ["[]", "\"text\"", "3"] {
        let entry = decode(&format!(
            concat!(
                r#"{{"type":"assistant","message":{{"content":["#,
                r#"{{"type":"tool_use","id":"toolu_1","name":"Bash","input":{{"command":"ls"}}}}]}},"#,
                r#""wireToolInputs":{}}}"#,
            ),
            wire
        ))?;
        check_eq(
            &entry.content.blocks,
            &vec![Block::ToolUse(ToolUse {
                id: "toolu_1".to_owned(),
                input: json!({"command": "ls"}),
                name: "Bash".to_owned(),
            })],
            wire,
        )?;
    }
    Ok(())
}
