//! Tests for tool calls the user turned down and the plans they were shown,
//! against synthetic history.

use core::error::Error;

use serde_json::{Value, json};

use super::{History, fixture_time};
use crate::testing::{TestResult, check_eq};

/// The JSON Lines `args` print for the session `dddd`, parsed.
fn records(history: &History, args: &[&str]) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut full = args.to_vec();
    full.extend(["--session", "dddd"]);
    let ran = history.run(&full);
    if ran.code != 0 {
        return Err(ran.err.into());
    }
    Ok(ran
        .out
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?)
}

/// A history with the session `dddd` holding `lines`.
fn history(lines: &[&str]) -> Result<History, Box<dyn Error>> {
    let history = History::new("root")?;
    history.write("-Users-alice-plan/dddd.jsonl", fixture_time()?, lines)?;
    Ok(history)
}

#[test]
fn a_call_keeps_the_recorded_input_the_wire_input_leaves_out() -> TestResult {
    // Claude Code records the plan in the input, while the model sent none.
    let history = history(&[
        r##"{"type":"assistant","uuid":"d-1","sessionId":"dddd","timestamp":"2026-10-15T00:00:00Z","wireToolInputs":{"toolu_p":{},"toolu_b":{"command":"cargo test --all"}},"message":{"content":[{"type":"tool_use","id":"toolu_p","name":"ExitPlanMode","input":{"plan":"# Plan","planFilePath":"/p.md"}},{"type":"tool_use","id":"toolu_b","name":"Bash","input":{"command":"cargo te..."}}]}}"##,
    ])?;
    let calls = records(&history, &["records", "--type", "call"])?;
    let inputs: Vec<Option<&Value>> = calls.iter().map(|call| call.get("input")).collect();
    check_eq(
        &inputs,
        &vec![
            Some(&json!({"plan": "# Plan", "planFilePath": "/p.md"})),
            Some(&json!({"command": "cargo test --all"})),
        ],
        "inputs",
    )?;
    Ok(())
}
