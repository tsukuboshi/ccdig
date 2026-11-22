//! Properties of decoding one transcript line that hold for any input.

use proptest::arbitrary::any;
use proptest::collection::vec;
use proptest::option;
use proptest::prop_oneof;
use proptest::sample::select;
use proptest::strategy::{Just, Strategy};
use serde_json::{Map, Value, json};

use crate::testing::{TestResult, check_eq, for_all, hostile_text, json_value, rfc3339_text};
use crate::transcript::entry::{Block, Role, Usage, decode_line};
use crate::when::Stamp;

/// One element of array content: a text block, a tool call, a tool result,
/// or something else, with whether it is a tool result and its text.
fn content_item() -> impl Strategy<Value = (Value, Option<String>, bool)> {
    prop_oneof![
        hostile_text().prop_map(|text| (json!({"type": "text", "text": text}), Some(text), false)),
        (hostile_text(), hostile_text(), json_value()).prop_map(|(id, name, input)| {
            (json!({"type": "tool_use", "id": id, "name": name, "input": input}), None, false)
        }),
        (hostile_text(), hostile_text(), any::<bool>()).prop_map(|(id, text, is_error)| {
            (
                json!({"type": "tool_result", "tool_use_id": id, "content": text, "is_error": is_error}),
                None,
                true,
            )
        }),
        Just((json!({"type": "image"}), None, false)),
    ]
}

/// A line's message content, string or array, with the prompt text and
/// tool-result flag it should decode to and the number of blocks.
fn content() -> impl Strategy<Value = (Value, String, bool, usize)> {
    prop_oneof![
        hostile_text().prop_map(|text| (Value::String(text.clone()), text, false, 0)),
        vec(content_item(), 0..5).prop_map(|items| {
            let texts: Vec<String> = items.iter().filter_map(|item| item.1.clone()).collect();
            let has_tool_result = items.iter().any(|item| item.2);
            let count = items.len();
            let values = items.into_iter().map(|item| item.0).collect();
            (
                Value::Array(values),
                texts.join("\n"),
                has_tool_result,
                count,
            )
        }),
    ]
}

#[test]
fn any_bytes_decode_without_panicking() -> TestResult {
    for_all(&vec(any::<u8>(), 0..256), |bytes| {
        drop(decode_line(&bytes));
        Ok(())
    })
}

#[test]
fn only_a_json_object_is_an_entry() -> TestResult {
    for_all(&json_value(), |value| {
        let decoded = decode_line(value.to_string().as_bytes());
        if !value.is_object() {
            check_eq(&decoded.is_none(), &true, "a value that is not an object")?;
        }
        Ok(())
    })
}

#[test]
fn a_line_decodes_to_the_fields_it_was_built_from() -> TestResult {
    let kinds = prop_oneof![
        select(&["user", "assistant", "ai-title", "summary"][..]).prop_map(str::to_owned),
        hostile_text(),
    ];
    let strings = (
        hostile_text(),
        hostile_text(),
        hostile_text(),
        hostile_text(),
    );
    let flags = (
        option::of(any::<bool>()),
        option::of(any::<bool>()),
        option::of(any::<bool>()),
    );
    let cases = (kinds, strings, flags, option::of(rfc3339_text()), content());
    for_all(
        &cases,
        |(kind, (uuid, session, cwd, branch), (meta, sidechain, compact), stamp, body)| {
            let (message, text, has_tool_result, blocks) = body;
            let mut line = Map::new();
            line.insert("type".to_owned(), Value::String(kind.clone()));
            line.insert("uuid".to_owned(), Value::String(uuid.clone()));
            line.insert("sessionId".to_owned(), Value::String(session.clone()));
            line.insert("cwd".to_owned(), Value::String(cwd.clone()));
            line.insert("gitBranch".to_owned(), Value::String(branch.clone()));
            line.insert("isMeta".to_owned(), json!(meta));
            line.insert("isSidechain".to_owned(), json!(sidechain));
            line.insert("isCompactSummary".to_owned(), json!(compact));
            line.insert("timestamp".to_owned(), json!(stamp));
            line.insert(
                "message".to_owned(),
                json!({"role": kind, "content": message}),
            );
            let entry =
                decode_line(Value::Object(line).to_string().as_bytes()).ok_or("rejected")?;
            let role = match kind.as_str() {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "ai-title" => Role::AiTitle,
                _ => Role::Other,
            };
            check_eq(&entry.role, &role, "role")?;
            check_eq(&entry.uuid, &uuid, "uuid")?;
            check_eq(&entry.session_id, &session, "session")?;
            check_eq(&entry.cwd, &cwd, "cwd")?;
            check_eq(&entry.git_branch, &branch, "branch")?;
            check_eq(&entry.flags.meta, &meta.unwrap_or_default(), "meta")?;
            check_eq(
                &entry.flags.sidechain,
                &sidechain.unwrap_or_default(),
                "sidechain",
            )?;
            check_eq(
                &entry.flags.compact_summary,
                &compact.unwrap_or_default(),
                "compact",
            )?;
            check_eq(
                &entry.timestamp,
                &stamp.as_deref().and_then(Stamp::parse_rfc3339),
                "timestamp",
            )?;
            check_eq(&entry.content.text, &text, "text")?;
            check_eq(
                &entry.content.has_tool_result,
                &has_tool_result,
                "tool result",
            )?;
            check_eq(&entry.content.blocks.len(), &blocks, "blocks")?;
            let tool_uses = entry
                .content
                .blocks
                .iter()
                .filter(|block| matches!(**block, Block::ToolUse(_)))
                .count();
            let wanted = message.as_array().map_or(0, |items| {
                items
                    .iter()
                    .filter(|item| item.get("type") == Some(&json!("tool_use")))
                    .count()
            });
            check_eq(&tool_uses, &wanted, "tool calls")?;
            Ok(())
        },
    )
}

#[test]
fn a_line_with_an_unreadable_timestamp_is_not_an_entry() -> TestResult {
    let bad =
        hostile_text().prop_filter("not RFC 3339", |text| Stamp::parse_rfc3339(text).is_none());
    for_all(&bad, |stamp| {
        let line = json!({"type": "user", "timestamp": stamp, "message": {"content": "hi"}});
        check_eq(&decode_line(line.to_string().as_bytes()), &None, "decoded")?;
        Ok(())
    })
}

#[test]
fn usage_adds_in_any_order_and_never_shrinks() -> TestResult {
    let usage = (any::<u64>(), any::<u64>(), any::<u64>(), any::<u64>()).prop_map(
        |(input, output, cache_read, cache_creation)| Usage {
            input,
            output,
            cache_read,
            cache_creation,
        },
    );
    for_all(
        &(usage.clone(), usage.clone(), usage),
        |(first, second, third)| {
            let sum = |parts: &[Usage]| {
                let mut total = Usage::default();
                for &part in parts {
                    total.add(part);
                }
                total
            };
            check_eq(
                &sum(&[first, second]),
                &sum(&[second, first]),
                "commutative",
            )?;
            check_eq(
                &sum(&[first, second, third]),
                &sum(&[third, first, second]),
                "associative",
            )?;
            check_eq(&sum(&[first]), &first, "the default is zero")?;
            let total = sum(&[first, second]);
            let grew = total.input >= first.input
                && total.output >= first.output
                && total.cache_read >= first.cache_read
                && total.cache_creation >= first.cache_creation;
            check_eq(&grew, &true, "never shrinks")?;
            Ok(())
        },
    )
}
