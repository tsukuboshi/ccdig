//! Tests for how a sub-agent's records name the call that started it,
//! against the history in `testdata/subagents/root`: a session that starts
//! a sub-agent, which starts another.

use serde_json::Value;

use super::{History, json_lines};
use crate::testing::{TestResult, check_eq, snapshot};

/// What `records` prints for every reply, call, and result, with the
/// sub-agents.
const ARGS: [&str; 4] = ["records", "--subagents", "--type", "reply,call,result"];

/// The value of `key` in `record`, or empty.
fn text<'record>(record: &'record Value, key: &str) -> &'record str {
    record.get(key).and_then(Value::as_str).unwrap_or_default()
}

#[test]
fn the_call_that_started_a_subagent_is_the_call_with_its_parent_tool_use_id() -> TestResult {
    let history = History::new("subagents/root")?;
    let records = json_lines(history.run_week(&ARGS))?;
    // Each sub-agent, the sub-agent its parent call is in (empty for the
    // session itself), and the parent call's tool.
    let mut parents = Vec::new();
    for record in &records {
        let parent = text(record, "parent_tool_use_id");
        if parent.is_empty()
            || parents
                .iter()
                .any(|&(agent, _, _)| agent == text(record, "subagent"))
        {
            continue;
        }
        let call = records
            .iter()
            .find(|other| text(other, "type") == "call" && text(other, "tool_use_id") == parent)
            .ok_or_else(|| format!("no call {parent}"))?;
        parents.push((
            text(record, "subagent"),
            text(call, "subagent"),
            text(call, "tool"),
        ));
    }
    check_eq(
        &parents,
        &vec![("agent-a1", "", "Agent"), ("agent-a2", "agent-a1", "Agent")],
        "parents",
    )?;
    Ok(())
}

#[test]
fn a_subagent_shows_its_type_and_description_in_json_lines_and_markdown() -> TestResult {
    let history = History::new("subagents/root")?;
    let jsonl = history.run_week(&ARGS);
    check_eq(&jsonl.code, &0, &jsonl.err)?;
    snapshot("cli/subagents.jsonl", &jsonl.out)?;
    let mut md_args = ARGS.to_vec();
    md_args.extend(["--format", "md"]);
    let md = history.run_week(&md_args);
    check_eq(&md.code, &0, &md.err)?;
    for line in [
        "- Subagent type: `Explore`\n- Description: Find the cart total\n",
        // One line, with the Markdown escaped.
        "- Subagent type: `general-purpose`\n- Description: Look in \\*src\\* and # lib\n",
    ] {
        check_eq(&md.out.contains(line), &true, line)?;
    }
    check_eq(
        &md.out.contains("toolu_p1"),
        &false,
        "no call ID in Markdown",
    )?;
    snapshot("cli/subagents.md", &md.out)?;
    let without = history.run_week(&["records", "--type", "reply,call,result", "--format", "md"]);
    check_eq(
        &without.out.contains("Subagent type"),
        &false,
        "without --subagents",
    )?;
    Ok(())
}

#[test]
fn fields_can_leave_out_the_type_and_the_description_in_markdown() -> TestResult {
    let history = History::new("subagents/root")?;
    let mut args = ARGS.to_vec();
    args.extend([
        "--format",
        "md",
        "--fields",
        "!subagent_type,!subagent_description",
    ]);
    let md = history.run_week(&args);
    check_eq(&md.code, &0, &md.err)?;
    check_eq(
        &md.out.contains("- Subagent: `agent-a1`"),
        &true,
        "subagent",
    )?;
    check_eq(&md.out.contains("Subagent type"), &false, "type")?;
    check_eq(&md.out.contains("Description"), &false, "description")?;
    Ok(())
}
