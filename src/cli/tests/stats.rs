//! Tests for `stats`: the records `records` would list, grouped by the
//! `--by` keys and counted.

use core::error::Error;

use serde_json::{Value, json};

use super::{History, fixture_time, json_lines};
use crate::testing::{TestResult, check_eq};

/// The first session: a plan the user turns down, then one they take.
const FIRST: &str = "pppp1111-1111-4111-8111-111111111111";

/// The second session: a plan taken at once, then a command, a day later.
const SECOND: &str = "pppp2222-2222-4222-8222-222222222222";

/// The history with the two plan sessions added.
fn plans() -> Result<History, Box<dyn Error>> {
    let history = History::new("root")?;
    history.write(
        &format!("-Users-alice-plans/{FIRST}.jsonl"),
        fixture_time()?,
        &[
            &format!(
                r#"{{"type":"user","uuid":"p-1","sessionId":"{FIRST}","timestamp":"2026-10-15T00:00:00Z","gitBranch":"main","message":{{"role":"user","content":"plan the cart"}}}}"#
            ),
            &format!(
                r#"{{"type":"assistant","uuid":"p-2","sessionId":"{FIRST}","timestamp":"2026-10-15T00:00:10Z","gitBranch":"main","message":{{"id":"msg_p1","model":"opus","content":[{{"type":"text","text":"Here is the plan."}},{{"type":"tool_use","id":"x-1","name":"ExitPlanMode","input":{{"plan":"A"}}}}],"usage":{{"input_tokens":10,"output_tokens":100,"cache_read_input_tokens":1000,"cache_creation_input_tokens":0}}}}}}"#
            ),
            &format!(
                r#"{{"type":"user","uuid":"p-3","sessionId":"{FIRST}","timestamp":"2026-10-15T00:01:00Z","gitBranch":"main","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"x-1","is_error":true,"content":"rejected"}}]}}}}"#
            ),
            &format!(
                r#"{{"type":"assistant","uuid":"p-4","sessionId":"{FIRST}","timestamp":"2026-10-15T00:02:00Z","gitBranch":"main","message":{{"id":"msg_p2","model":"opus","content":[{{"type":"tool_use","id":"x-2","name":"ExitPlanMode","input":{{"plan":"B"}}}}],"usage":{{"input_tokens":1,"output_tokens":50,"cache_read_input_tokens":500,"cache_creation_input_tokens":0}}}}}}"#
            ),
            &format!(
                r#"{{"type":"user","uuid":"p-5","sessionId":"{FIRST}","timestamp":"2026-10-15T00:03:00Z","gitBranch":"main","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"x-2","content":"approved"}}]}}}}"#
            ),
        ],
    )?;
    history.write(
        &format!("-Users-alice-plans/{SECOND}.jsonl"),
        fixture_time()?,
        &[
            &format!(
                r#"{{"type":"user","uuid":"q-1","sessionId":"{SECOND}","timestamp":"2026-10-16T00:00:00Z","gitBranch":"dev","message":{{"role":"user","content":"ship it"}}}}"#
            ),
            &format!(
                r#"{{"type":"assistant","uuid":"q-2","sessionId":"{SECOND}","timestamp":"2026-10-16T00:00:10Z","gitBranch":"dev","message":{{"id":"msg_p3","model":"haiku","content":[{{"type":"tool_use","id":"x-3","name":"ExitPlanMode","input":{{"plan":"C"}}}}],"usage":{{"input_tokens":1,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
            ),
            &format!(
                r#"{{"type":"user","uuid":"q-3","sessionId":"{SECOND}","timestamp":"2026-10-16T00:01:00Z","gitBranch":"dev","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"x-3","content":"approved"}}]}}}}"#
            ),
            &format!(
                r#"{{"type":"assistant","uuid":"q-4","sessionId":"{SECOND}","timestamp":"2026-10-16T00:02:00Z","gitBranch":"dev","message":{{"id":"msg_p4","model":"haiku","content":[{{"type":"tool_use","id":"x-4","name":"Bash","input":{{"command":"ls"}}}}],"usage":{{"input_tokens":1,"output_tokens":7,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
            ),
            &format!(
                r#"{{"type":"user","uuid":"q-5","sessionId":"{SECOND}","timestamp":"2026-10-16T00:03:00Z","gitBranch":"dev","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"x-4","content":"a b"}}]}}}}"#
            ),
        ],
    )?;
    Ok(history)
}

/// The lines `stats` prints for the two plan sessions with `args`.
fn stats(history: &History, args: &[&str]) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut full = vec!["stats", "--session", FIRST, "--session", SECOND];
    full.extend_from_slice(args);
    json_lines(history.run_week(&full))
}

/// The values of `keys` of each line, to compare a few keys of every line.
fn pick(lines: &[Value], keys: &[&str]) -> Vec<Value> {
    lines
        .iter()
        .map(|line| {
            Value::Array(
                keys.iter()
                    .map(|key| line.get(*key).cloned().unwrap_or_default())
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn by_session_finds_the_sessions_with_two_plans_or_more() -> TestResult {
    let history = plans()?;
    let got = stats(
        &history,
        &[
            "--by",
            "session",
            "--type",
            "call",
            "--tool",
            "ExitPlanMode",
            "--min-count",
            "2",
        ],
    )?;
    check_eq(
        &got,
        &vec![json!({
            "session": FIRST,
            "session_title": null,
            "project": "-Users-alice-plans",
            "file": format!("$ROOT/-Users-alice-plans/{FIRST}.jsonl"),
            "count": 2_u64,
            "chars": 24_u64,
            "first": "2026-10-15T09:00:10+09:00",
            "last": "2026-10-15T09:02:00+09:00",
            "usage": {"input": 11_u64, "output": 150_u64, "cache_read": 1500_u64, "cache_creation": 0_u64},
        })],
        "the calls carry the tokens once the replies are left out",
    )?;
    Ok(())
}

#[test]
fn by_tool_counts_the_calls_and_their_size_without_usage() -> TestResult {
    let history = plans()?;
    let got = stats(&history, &["--by", "tool", "--type", "call"])?;
    check_eq(
        &pick(&got, &["tool", "count", "chars", "sessions", "usage"]),
        &vec![
            json!(["ExitPlanMode", 3_u64, 36_u64, 2_u64, null]),
            json!(["Bash", 1_u64, 16_u64, 1_u64, null]),
        ],
        "input as compact JSON",
    )?;
    Ok(())
}

#[test]
fn by_type_counts_every_kind_given() -> TestResult {
    let history = plans()?;
    let got = stats(
        &history,
        &["--by", "type", "--type", "prompt,reply,call,result"],
    )?;
    check_eq(
        &pick(&got, &["type", "count", "chars"]),
        &vec![
            json!(["call", 4_u64, 52_u64]),
            json!(["result", 4_u64, 27_u64]),
            json!(["prompt", 2_u64, 20_u64]),
            json!(["reply", 1_u64, 17_u64]),
        ],
        "the most first, then by name",
    )?;
    Ok(())
}

#[test]
fn by_model_adds_up_each_response_once() -> TestResult {
    let history = plans()?;
    let got = stats(
        &history,
        &["--by", "model", "--type", "reply,call", "--sort", "output"],
    )?;
    check_eq(
        &pick(&got, &["model", "count", "usage"]),
        &vec![
            json!(["opus", 3_u64, {"input": 11_u64, "output": 150_u64, "cache_read": 1500_u64, "cache_creation": 0_u64}]),
            json!(["haiku", 2_u64, {"input": 2_u64, "output": 37_u64, "cache_read": 0_u64, "cache_creation": 0_u64}]),
        ],
        "a reply and a call of one response count its tokens once",
    )?;
    Ok(())
}

#[test]
fn by_week_counts_the_plans_turned_down() -> TestResult {
    let history = plans()?;
    let got = stats(
        &history,
        &[
            "--by",
            "week",
            "--type",
            "result",
            "--tool",
            "ExitPlanMode",
            "--status",
            "error",
        ],
    )?;
    check_eq(
        &pick(&got, &["week", "count"]),
        &vec![json!(["2026-W42", 1_u64])],
        "one plan turned down in the week",
    )?;
    Ok(())
}

#[test]
fn without_by_the_default_types_are_one_line() -> TestResult {
    let history = plans()?;
    let got = stats(&history, &[])?;
    check_eq(
        &pick(&got, &["count", "sessions", "first", "last"]),
        &vec![json!([
            3_u64,
            2_u64,
            "2026-10-15T09:00:00+09:00",
            "2026-10-16T09:00:00+09:00"
        ])],
        "prompts and replies, as records lists them",
    )?;
    Ok(())
}

#[test]
fn by_takes_keys_separated_by_commas_or_repeated_and_limit_cuts_groups() -> TestResult {
    let history = plans()?;
    let got = stats(
        &history,
        &[
            "--by",
            "branch,day",
            "--by",
            "branch",
            "--type",
            "call",
            "--sort",
            "key",
            "--limit",
            "1",
        ],
    )?;
    check_eq(
        &pick(&got, &["branch", "day", "count"]),
        &vec![json!(["dev", "2026-10-16", 2_u64])],
        "each key once, the first group by key",
    )?;
    check_eq(
        &got.first()
            .and_then(Value::as_object)
            .map(|keys| keys.keys().take(2).cloned().collect::<Vec<_>>()),
        &Some(vec!["branch".to_owned(), "day".to_owned()]),
        "the keys first, in the order given",
    )?;
    Ok(())
}

#[test]
fn fields_picks_the_keys_of_a_line() -> TestResult {
    let history = plans()?;
    let got = stats(
        &history,
        &["--by", "tool", "--type", "call", "--fields", "tool,count"],
    )?;
    check_eq(
        &got,
        &vec![
            json!({"tool": "ExitPlanMode", "count": 3_u64}),
            json!({"tool": "Bash", "count": 1_u64}),
        ],
        "only the keys named",
    )?;
    Ok(())
}

#[test]
fn markdown_is_a_table() -> TestResult {
    let history = plans()?;
    let ran = history.run_week(&[
        "stats",
        "--session",
        FIRST,
        "--by",
        "tool",
        "--type",
        "call",
        "--format",
        "md",
    ]);
    check_eq(&ran.code, &0, &ran.err)?;
    check_eq(
        &ran.out.lines().nth(4),
        &Some(
            "| `ExitPlanMode` | 2 | 24 | 1 | 2026-10-15 09:00 JST | 2026-10-15 09:02 JST |  |  |  |  |",
        ),
        &ran.out,
    )?;
    Ok(())
}

#[test]
fn stats_rejects_what_would_change_what_it_counts() -> TestResult {
    let history = plans()?;
    for (args, message) in [
        (
            &["--by", "tool", "--sort", "output"][..],
            "ccdig: --sort output cannot be used with --by tool, which leaves usage null",
        ),
        (
            &["--by", "week,type", "--sort", "cache_read"][..],
            "ccdig: --sort cache_read cannot be used with --by type, which leaves usage null",
        ),
        (
            &["--by", "subagent", "--type", "call"][..],
            "ccdig: --by subagent needs --subagents",
        ),
        (
            &["--fields", "count", "--format", "md"][..],
            "ccdig: --fields needs --format json or jsonl",
        ),
        (
            &["--fields", "text"][..],
            "ccdig: --fields: unknown key \"text\": use ",
        ),
        (
            &["--type", "prompt", "--tool", "Bash"][..],
            "ccdig: --tool needs --type call or result",
        ),
        (
            &["--around", "1"][..],
            "ccdig: unexpected argument '--around'",
        ),
        (
            &["--context", "1"][..],
            "ccdig: unexpected argument '--context'",
        ),
        (
            &["--max-message-chars", "1"][..],
            "ccdig: unexpected argument '--max-message-chars'",
        ),
        (
            &["--max-tool-chars", "1"][..],
            "ccdig: unexpected argument '--max-tool-chars'",
        ),
    ] {
        let mut full = vec!["stats"];
        full.extend_from_slice(args);
        let ran = history.run_week(&full);
        check_eq(&ran.code, &1, &format!("{args:?}"))?;
        check_eq(
            &ran.err.starts_with(message),
            &true,
            &format!("{args:?}: {}", ran.err),
        )?;
        check_eq(&ran.out, &String::new(), "nothing on stdout")?;
    }
    Ok(())
}

#[test]
fn by_subagent_with_subagents_names_each_sub_agent() -> TestResult {
    let history = plans()?;
    history.write(
        &format!("-Users-alice-plans/{FIRST}/subagents/agent-x.jsonl"),
        fixture_time()?,
        &[&format!(
            r#"{{"type":"assistant","uuid":"r-1","sessionId":"{FIRST}","timestamp":"2026-10-15T00:05:00Z","message":{{"id":"msg_r1","content":[{{"type":"tool_use","id":"x-9","name":"Read","input":{{}}}}]}}}}"#
        )],
    )?;
    let got = stats(
        &history,
        &[
            "--by",
            "subagent",
            "--type",
            "call",
            "--subagents",
            "--sort",
            "key",
        ],
    )?;
    check_eq(
        &pick(&got, &["subagent", "count"]),
        &vec![json!(["agent-x", 1_u64]), json!([null, 4_u64])],
        "the session's own calls have no sub-agent",
    )?;
    Ok(())
}
