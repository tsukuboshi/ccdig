//! Tests for the HTML page of the records.

use core::error::Error;

use serde_json::json;

use super::{
    AGENT_FILE, BLOG_FILE, bare_prompt, blog_head, call_at, every_part, fields, found, outcome,
    reply_at, result_at, tokyo,
};
use crate::record::{Head, Prompt, Reply, Search, Spawned, Summary, ToolCall, ToolResult, Turn};
use crate::redact::Counts;
use crate::render::html::{Masking, records};
use crate::render::json::Fields;
use crate::testing::{TestResult, check_eq, snapshot, without_style};
use crate::text::Lang;

/// The page of `turns` with every part, in English, masked with no
/// counts.
fn page(turns: &[Turn]) -> Result<String, Box<dyn Error>> {
    Ok(records(
        turns,
        &tokyo()?,
        &Fields::default(),
        Lang::En,
        &Masking::On(Vec::new()),
    )?)
}

#[test]
fn a_prompt_shows_as_typed_and_a_reply_as_rendered_markdown() -> TestResult {
    let turns = [
        Turn::Prompt(bare_prompt(
            blog_head("2026-10-13T01:23:00Z")?,
            BLOG_FILE,
            "**not bold** <b>x</b>",
        )),
        Turn::Reply(reply_at(
            "2026-10-13T01:24:00Z",
            BLOG_FILE,
            2,
            "**bold** <script>alert(1)</script>",
        )?),
    ];
    let out = page(&turns)?;
    // (what, the markup)
    for (what, markup) in [
        (
            "the prompt as typed",
            "<pre>\n**not bold** &lt;b&gt;x&lt;/b&gt;</pre>",
        ),
        ("the reply's Markdown", "<strong>bold</strong>"),
        (
            "the reply's HTML as text",
            "&lt;script&gt;alert(1)&lt;/script&gt;",
        ),
    ] {
        check_eq(&out.contains(markup), &true, &format!("{what}:\n{out}"))?;
    }
    check_eq(&out.contains("<script"), &false, &out)?;
    Ok(())
}

#[test]
fn a_call_folds_under_its_tool_its_input_and_how_it_ended() -> TestResult {
    let input = json!({"command": "cargo test\nsecond line", "timeout": 60_u32});
    // (name, what the call came to, the badge, what else shows)
    for (name, result, badge, shown) in [
        (
            "ran",
            Some(outcome(false, "", "done")),
            "<span class=\"badge ok\">ok</span>",
            "<pre>\ndone</pre>",
        ),
        (
            "failed",
            Some(outcome(true, "", "exit 1")),
            "<span class=\"badge error\">error</span>",
            "<pre>\nexit 1</pre>",
        ),
        (
            "turned down",
            Some(outcome(true, "user-rejected", "The user said no.")),
            "<span class=\"badge denied\">denied</span>",
            "<code>user-rejected</code>",
        ),
        (
            "no result",
            None,
            "<span class=\"badge missing\">no result</span>",
            "</summary>",
        ),
    ] {
        let out = page(&[Turn::Call(call_at(
            5,
            "Bash",
            input.clone(),
            "toolu_a",
            result,
        )?)])?;
        // (what, the markup)
        for (what, markup) in [
            ("the fold", "class=\"call\">\n<details>\n<summary>"),
            ("the tool", "<code class=\"tool\">Bash</code>"),
            (
                "the first line of the command",
                "<span class=\"gist\">cargo test</span>",
            ),
            ("the badge", badge),
            ("the input", "&quot;timeout&quot;: 60"),
            ("the rest", shown),
        ] {
            check_eq(
                &out.contains(markup),
                &true,
                &format!("{name}, {what}:\n{out}"),
            )?;
        }
        check_eq(
            &out.contains("<span class=\"gist\">cargo test\nsecond"),
            &false,
            name,
        )?;
    }
    Ok(())
}

#[test]
fn the_gist_of_a_call_is_its_first_key_by_rank_cut_to_one_line() -> TestResult {
    let long = "x".repeat(100);
    // (name, the input, the gist, or `None` when there is none)
    for (name, input, gist) in [
        (
            "a command",
            json!({"command": "ls -la", "description": "list"}),
            Some("ls -la"),
        ),
        (
            "a file",
            json!({"file_path": "/a/b.rs", "content": "x"}),
            Some("/a/b.rs"),
        ),
        (
            "a pattern before a path",
            json!({"path": "src", "pattern": "fn main"}),
            Some("fn main"),
        ),
        (
            "a url",
            json!({"url": "https://example.com/", "prompt": "read"}),
            Some("https://example.com/"),
        ),
        ("a query", json!({"query": "rust html"}), Some("rust html")),
        (
            "a description",
            json!({"description": "Find the files", "prompt": "..."}),
            Some("Find the files"),
        ),
        ("a skill", json!({"skill": "tdd"}), Some("tdd")),
        ("a path", json!({"path": "src"}), Some("src")),
        (
            "leading blank lines",
            json!({"command": "\n\n  make\nmore"}),
            Some("make"),
        ),
        (
            "cut",
            json!({"command": long}),
            Some(&*format!("{}\u{2026}", "x".repeat(80))),
        ),
        ("none of the keys", json!({"id": "1209876543210"}), None),
        ("not a string", json!({"command": ["ls"]}), None),
        ("blank", json!({"command": " \n "}), None),
    ] {
        let out = page(&[Turn::Call(call_at(5, "mcp__x", input, "toolu_a", None)?)])?;
        let want = gist.map(|text| format!("<span class=\"gist\">{text}</span>"));
        let got = out
            .split_once("<span class=\"gist\">")
            .and_then(|(_, rest)| rest.split_once("</span>"))
            .map(|(text, _)| format!("<span class=\"gist\">{text}</span>"));
        check_eq(&got, &want, name)?;
    }
    Ok(())
}

#[test]
fn a_result_on_its_own_says_its_tool_and_how_it_ended() -> TestResult {
    let denied = outcome(true, "user-rejected", "The user said no.");
    let out = page(&[Turn::Result(result_at(
        8,
        "ExitPlanMode",
        "toolu_b",
        &denied,
    )?)])?;
    for (what, markup) in [
        ("the article", "class=\"result\">\n<h3>"),
        (
            "the tool",
            "<dt>Tool</dt><dd><code>ExitPlanMode</code></dd>",
        ),
        ("the error", "<dt>Error</dt><dd>yes</dd>"),
        (
            "the denial",
            "<dt>Denial</dt><dd><code>user-rejected</code></dd>",
        ),
        ("the output", "<pre>\nThe user said no.</pre>"),
    ] {
        check_eq(&out.contains(markup), &true, &format!("{what}:\n{out}"))?;
    }
    Ok(())
}

/// The `id` of each article of `page`, in order.
fn ids(page: &str) -> Vec<&str> {
    page.split("<article id=\"")
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(id, _)| id))
        .collect()
}

#[test]
fn each_record_has_an_id_from_its_line_or_its_place() -> TestResult {
    let uuid = "3f2a9c1e-0000-4000-8000-000000000001";
    let reply = |line: usize, line_id: &str, block: usize| -> Result<Turn, String> {
        let mut reply = reply_at("2026-10-13T01:24:00Z", BLOG_FILE, line, "x")?;
        reply.source.uuid = line_id.to_owned();
        reply.source.block = block;
        Ok(Turn::Reply(reply))
    };
    let turns = [
        reply(1, uuid, 0)?,
        reply(1, uuid, 1)?,
        reply(2, "", 0)?,
        reply(3, "\"><b>", 0)?,
        Turn::Thinking(reply_at("2026-10-13T01:24:00Z", BLOG_FILE, 4, "x")?),
    ];
    let want = [
        format!("reply-{uuid}-0"),
        format!("reply-{uuid}-1"),
        "r3".to_owned(),
        "r4".to_owned(),
        "r5".to_owned(),
    ];
    check_eq(
        &ids(&page(&turns)?),
        &want.iter().map(String::as_str).collect(),
        "ids",
    )?;
    let hidden = records(
        &turns,
        &tokyo()?,
        &fields(&[], &["source"]),
        Lang::En,
        &Masking::On(Vec::new()),
    )?;
    check_eq(
        &ids(&hidden),
        &vec!["r1", "r2", "r3", "r4", "r5"],
        "without source",
    )?;
    check_eq(
        &page(&turns)?.contains(&format!(
            "<a class=\"anchor\" href=\"#reply-{uuid}-0\">#</a>"
        )),
        &true,
        "the link to itself",
    )?;
    Ok(())
}

#[test]
fn a_result_on_its_own_and_its_call_link_to_each_other() -> TestResult {
    let denied = outcome(true, "user-rejected", "The user said no.");
    let mut call = call_at(
        7,
        "ExitPlanMode",
        json!({"plan": "one\ntwo"}),
        "toolu_b",
        Some(denied.clone()),
    )?;
    call.source.uuid = "aaaaaaaa-0000-4000-8000-000000000001".to_owned();
    let mut result = ToolResult {
        search: found(&[("text", 1, "The user said no.", true)]),
        ..result_at(8, "ExitPlanMode", "toolu_b", &denied)?
    };
    result.source.uuid = "bbbbbbbb-0000-4000-8000-000000000002".to_owned();
    let out = page(&[Turn::Call(call), Turn::Result(result)])?;
    for (what, markup) in [
        (
            "from the call",
            "<a href=\"#result-bbbbbbbb-0000-4000-8000-000000000002-0\">",
        ),
        (
            "from the result",
            "<a href=\"#call-aaaaaaaa-0000-4000-8000-000000000001-0\">",
        ),
    ] {
        check_eq(&out.contains(markup), &true, &format!("{what}:\n{out}"))?;
    }
    Ok(())
}

#[test]
fn a_section_names_its_branch_and_the_span_of_the_records_shown() -> TestResult {
    let turns = [
        Turn::Reply(reply_at("2026-10-13T01:24:00Z", BLOG_FILE, 2, "later")?),
        Turn::Prompt(bare_prompt(
            blog_head("2026-10-13T01:23:00Z")?,
            BLOG_FILE,
            "first",
        )),
    ];
    let span = "<dt>Records shown</dt><dd><time datetime=\"2026-10-13T01:23:00Z\">2026-10-13 10:23:00 JST</time> \u{2013} <time datetime=\"2026-10-13T01:24:00Z\">2026-10-13 10:24:00 JST</time></dd>";
    let branch = "<dt>Branch</dt><dd><code>main</code></dd>";
    let all = page(&turns)?;
    check_eq(&all.contains(branch), &true, &all)?;
    check_eq(&all.contains(span), &true, &all)?;
    let one = page(&turns[..1])?;
    check_eq(
        &one.contains("<dt>Records shown</dt><dd><time datetime=\"2026-10-13T01:24:00Z\">2026-10-13 10:24:00 JST</time></dd>"),
        &true,
        &one,
    )?;
    let hidden = records(
        &turns,
        &tokyo()?,
        &fields(&[], &["git_branch", "timestamp"]),
        Lang::En,
        &Masking::On(Vec::new()),
    )?;
    for (what, markup) in [
        ("the branch", "<dt>Branch"),
        ("the span", "<dt>Records shown"),
    ] {
        check_eq(
            &hidden.contains(markup),
            &false,
            &format!("{what}:\n{hidden}"),
        )?;
    }
    Ok(())
}

#[test]
fn a_section_lists_the_models_modes_and_versions_as_code() -> TestResult {
    let turns = [
        Turn::Prompt(Prompt {
            permission_mode: "plan".to_owned(),
            ..bare_prompt(
                Head {
                    agent_version: "2.1.326".to_owned(),
                    ..blog_head("2026-10-13T01:23:00Z")?
                },
                BLOG_FILE,
                "first",
            )
        }),
        Turn::Reply(Reply {
            model: "claude-<b>".to_owned(),
            ..reply_at("2026-10-13T01:24:00Z", BLOG_FILE, 2, "later")?
        }),
        Turn::Reply(Reply {
            model: "claude-opus-5-5".to_owned(),
            ..reply_at("2026-10-13T01:25:00Z", BLOG_FILE, 3, "done")?
        }),
    ];
    let all = page(&turns)?;
    for want in [
        "<dt>Model</dt><dd><code>claude-&lt;b&gt;</code>, <code>claude-opus-5-5</code></dd>",
        "<dt>Permission mode</dt><dd><code>plan</code></dd>",
        "<dt>Claude Code</dt><dd><code>2.1.326</code></dd>",
    ] {
        check_eq(&all.contains(want), &true, &format!("{want}:\n{all}"))?;
    }
    Ok(())
}

/// A reply of the sub-agent that the call `parent` started.
fn sub_agent_reply(parent: &str) -> Result<Turn, String> {
    Ok(Turn::Reply(Reply {
        subagent: "agent-7".to_owned(),
        spawned: Spawned {
            subagent_type: "Explore".to_owned(),
            subagent_description: "Look around".to_owned(),
            parent_tool_use_id: parent.to_owned(),
        },
        ..reply_at("2026-10-13T01:25:30Z", AGENT_FILE, 1, "Looked.")?
    }))
}

#[test]
fn a_sub_agent_shows_in_the_call_that_started_it_when_the_page_has_it() -> TestResult {
    let call = Turn::Call(call_at(
        5,
        "Agent",
        json!({"description": "Look around"}),
        "toolu_a",
        Some(outcome(false, "", "done")),
    )?);
    let prompt = Turn::Prompt(bare_prompt(
        blog_head("2026-10-13T01:23:00Z")?,
        BLOG_FILE,
        "go",
    ));
    // (name, the records, whether the sub-agent is in the call)
    for (name, turns, nested) in [
        (
            "with its call",
            vec![call.clone(), sub_agent_reply("toolu_a")?],
            true,
        ),
        (
            "without its call",
            vec![prompt.clone(), sub_agent_reply("toolu_a")?],
            false,
        ),
        (
            "with another call",
            vec![call, sub_agent_reply("toolu_z")?],
            false,
        ),
        (
            "its own parent",
            vec![prompt, sub_agent_reply("toolu_a")?, {
                let mut own = call_at(2, "Agent", json!({}), "toolu_a", None)?;
                own.source.file = AGENT_FILE.to_owned();
                own.subagent = "agent-7".to_owned();
                own.spawned.parent_tool_use_id = "toolu_a".to_owned();
                Turn::Call(own)
            }],
            false,
        ),
    ] {
        let out = page(&turns)?;
        check_eq(
            &out.matches("<section").count(),
            &2,
            &format!("{name}, each once:\n{out}"),
        )?;
        let sub_agent = out
            .find("<section class=\"subagent\">")
            .ok_or_else(|| format!("{name}, no sub-agent:\n{out}"))?;
        let inside = out.find("class=\"call\"").is_some_and(|start| {
            let end = out
                .get(start..)
                .and_then(|rest| rest.find("</article>"))
                .map(|offset| start.saturating_add(offset));
            start < sub_agent && end.is_some_and(|close| sub_agent < close)
        });
        check_eq(&inside, &nested, &format!("{name}:\n{out}"))?;
    }
    Ok(())
}

#[test]
fn the_page_shows_each_part_fields_keeps() -> TestResult {
    let turns = every_part()?;
    let zone = tokyo()?;
    let masking = Masking::On(vec![
        ("aws-credential".to_owned(), 2),
        ("email".to_owned(), 1),
    ]);
    for (name, kept) in [
        ("parts", Fields::default()),
        ("parts-no-tool-input", fields(&[], &["tool", "input"])),
        (
            "parts-no-result",
            fields(&[], &["result", "is_error", "denial"]),
        ),
        ("parts-no-text", fields(&[], &["text"])),
        ("parts-no-matches", fields(&[], &["matches"])),
        (
            "parts-no-heading-facts",
            fields(
                &[],
                &[
                    "timestamp",
                    "turn",
                    "project",
                    "session_id",
                    "git_branch",
                    "subagent",
                    "subagent_type",
                    "subagent_description",
                ],
            ),
        ),
        ("parts-only-type", fields(&["type"], &[])),
    ] {
        snapshot(
            &format!("render/{name}.html"),
            &without_style(&records(&turns, &zone, &kept, Lang::En, &masking)?)?,
        )?;
    }
    Ok(())
}

#[test]
fn the_page_in_japanese() -> TestResult {
    snapshot(
        "render/parts-ja.html",
        &without_style(&records(
            &every_part()?,
            &tokyo()?,
            &Fields::default(),
            Lang::Ja,
            &Masking::Off,
        )?)?,
    )
}

#[test]
fn a_summary_marks_where_the_context_was_compacted_and_folds_its_text() -> TestResult {
    let summary = Turn::Summary(Summary {
        head: blog_head("2026-10-13T01:26:00Z")?,
        text: "**Compacted.**".to_owned(),
        redacted: false,
        source: super::source(BLOG_FILE, 12, ""),
        search: Search::default(),
        masked: Counts::default(),
    });
    let out = page(&[summary])?;
    let fold = "<article id=\"r1\" class=\"summary\">\n<details>\n<summary><time datetime=\"2026-10-13T01:26:00Z\">2026-10-13 10:26:00 JST</time> - Summary <span class=\"note\">The context was compacted here</span> <a class=\"anchor\" href=\"#r1\">#</a></summary>\n<div class=\"prose\">\n<p><strong>Compacted.</strong></p>\n</div>\n</details>\n</article>";
    check_eq(&out.contains(fold), &true, &out)?;
    Ok(())
}

#[test]
fn a_block_keeps_a_first_line_break() -> TestResult {
    let turns = [Turn::Prompt(bare_prompt(
        blog_head("2026-10-13T01:23:00Z")?,
        BLOG_FILE,
        "\nafter a blank line",
    ))];
    let out = page(&turns)?;
    // A parser drops one line break right after <pre>\n, so one is written.
    check_eq(
        &out.contains("<pre>\n\nafter a blank line</pre>"),
        &true,
        &out,
    )?;
    Ok(())
}

#[test]
fn a_call_and_its_result_on_its_own_link_within_their_file() -> TestResult {
    let denied = outcome(true, "user-rejected", "No.");
    let pair = |file: &str, line: usize| -> Result<[Turn; 2], String> {
        let mut call = call_at(line, "Bash", json!({}), "toolu_same", Some(denied.clone()))?;
        call.source.file = file.to_owned();
        let mut result = ToolResult {
            search: found(&[("text", 1, "No.", true)]),
            ..result_at(line.saturating_add(1), "Bash", "toolu_same", &denied)?
        };
        result.source.file = file.to_owned();
        Ok([Turn::Call(call), Turn::Result(result)])
    };
    let [first_call, first_result] = pair(BLOG_FILE, 1)?;
    let [second_call, second_result] = pair("/sessions/copy.jsonl", 5)?;
    let out = page(&[first_call, first_result, second_call, second_result])?;
    for (what, markup) in [
        ("the first call", "<a href=\"#r2\">Tool result</a>"),
        ("the first result", "<a href=\"#r1\">Tool call</a>"),
        ("the second call", "<a href=\"#r4\">Tool result</a>"),
        ("the second result", "<a href=\"#r3\">Tool call</a>"),
    ] {
        check_eq(&out.contains(markup), &true, &format!("{what}:\n{out}"))?;
    }
    Ok(())
}

#[test]
fn a_call_shown_by_its_matched_lines_has_no_gist() -> TestResult {
    let call = ToolCall {
        search: found(&[("input.command", 2, "git push", true)]),
        ..call_at(
            5,
            "Bash",
            json!({"command": "secret first line\ngit push"}),
            "toolu_a",
            None,
        )?
    };
    let out = page(&[Turn::Call(call)])?;
    check_eq(&out.contains("class=\"gist\""), &false, &out)?;
    check_eq(&out.contains("secret first line"), &false, &out)?;
    Ok(())
}
