//! Tests for the HTML page of the records.

use core::error::Error;

use serde_json::json;

use super::{
    AGENT_FILE, BLOG_FILE, BLOG_SESSION, bare_prompt, blog_head, call_at, every_part, fields,
    found, outcome, reply_at, result_at, sample_groups, sample_session, tokyo,
};
use crate::record::{
    FirstMatch, Group, Head, Prompt, Reply, Search, Session, Spawned, Summary, ToolCall,
    ToolResult, Turn,
};
use crate::redact::Counts;
use crate::render::html::{Masking, records, sessions, stats};
use crate::render::json::Fields;
use crate::stats::{By, columns};
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

#[test]
fn an_edit_shows_as_lines_taken_out_and_put_in() -> TestResult {
    let turns = [Turn::Call(call_at(
        1,
        "Edit",
        json!({
            "file_path": "src/a.rs",
            "old_string": "if a < b {",
            "new_string": "if a <= b {\n    swap();",
        }),
        "toolu_1",
        None,
    )?)];
    let all = page(&turns)?;
    let want = concat!(
        "<p class=\"key\"><code>input.old_string</code> \u{2192} <code>input.new_string</code></p>\n",
        "<pre class=\"diff\">\n<del>-if a &lt; b {</del>\n<ins>+if a &lt;= b {</ins>\n<ins>+    swap();</ins></pre>\n",
    );
    check_eq(&all.contains(want), &true, &all)?;
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

#[test]
fn sessions_show_as_one_table_without_the_match_columns_when_nothing_was_searched() -> TestResult {
    let out = sessions(
        &[sample_session()?],
        &tokyo()?,
        false,
        &Fields::default(),
        Lang::En,
        &Masking::Off,
    )?;
    let head = "<thead>\n<tr><th>Started</th><th>Ended</th><th>Session</th><th>Project</th><th>Cwd</th><th>Branch</th><th class=\"number\">Prompts</th><th>Models</th><th>First prompt</th><th>Title</th></tr>\n</thead>";
    let row = format!(
        "<tr><td><time datetime=\"2026-10-13T01:23:45.678Z\">2026-10-13 10:23 JST</time></td><td><time datetime=\"2026-10-13T01:30:00Z\">2026-10-13 10:30 JST</time></td><td><code>{BLOG_SESSION}</code></td><td><code>-Users-alice-blog</code></td><td><code>/Users/alice/blog</code></td><td><code>main</code></td><td class=\"number\">2</td><td><code>claude-fable-5-2</code>, <code>claude-haiku-5-5</code></td><td>Draft the weekly report from these notes</td><td>Weekly report | draft</td></tr>"
    );
    // (what, the markup)
    for (what, markup) in [
        ("the title", "<title>Sessions</title>"),
        ("the heading", "<h1>Sessions</h1>"),
        ("the count", "<p class=\"count\">Sessions: 1</p>"),
        (
            "the masking",
            "<p class=\"masking\">Not masked (--no-mask)</p>",
        ),
        ("the head of the table", head),
        ("the row", row.as_str()),
    ] {
        check_eq(&out.contains(markup), &true, &format!("{what}:\n{out}"))?;
    }
    check_eq(
        &out.matches("<div class=\"table\">\n<table>").count(),
        &1,
        &out,
    )?;
    check_eq(&out.contains("Matches"), &false, &out)?;
    check_eq(
        &out.ends_with("</table>\n</div>\n</body>\n</html>\n"),
        &true,
        &out,
    )?;
    Ok(())
}

#[test]
fn stats_show_as_one_table_with_the_numbers_on_the_right_and_empty_cells_left_empty() -> TestResult
{
    let out = stats(
        &sample_groups()?,
        &columns(&[By::Session]),
        &tokyo()?,
        &Fields::default(),
        Lang::En,
        &Masking::On(vec![("aws-credential".to_owned(), 2)]),
    )?;
    let head = "<thead>\n<tr><th>Session</th><th>Session title</th><th>Project</th><th>File</th><th class=\"number\">Count</th><th class=\"number\">Chars</th><th>First</th><th>Last</th><th class=\"number\">Input</th><th class=\"number\">Output</th><th class=\"number\">Cache read</th><th class=\"number\">Cache creation</th></tr>\n</thead>";
    let untitled = "<tr><td><code>22222222-2222-4222-8222-222222222222</code></td><td>(untitled)</td><td><code>-Users-alice-blog</code></td><td><code>/home/alice/.claude/projects/-Users-alice-blog/22222222-2222-4222-8222-222222222222.jsonl</code></td><td class=\"number\">1</td><td class=\"number\">0</td><td><time datetime=\"2026-10-14T00:05:00Z\">2026-10-14 09:05 JST</time></td><td><time datetime=\"2026-10-14T00:05:00Z\">2026-10-14 09:05 JST</time></td><td class=\"number\"></td><td class=\"number\"></td><td class=\"number\"></td><td class=\"number\"></td></tr>";
    // (what, the markup)
    for (what, markup) in [
        ("the title", "<title>Stats</title>"),
        ("the count", "<p class=\"count\">Groups: 2</p>"),
        (
            "the masking",
            "<p class=\"masking\">Masked 2 by aws-credential</p>",
        ),
        ("the head of the table", head),
        ("the tokens", "<td class=\"number\">30000</td>"),
        ("the row without a title or tokens", untitled),
    ] {
        check_eq(&out.contains(markup), &true, &format!("{what}:\n{out}"))?;
    }
    check_eq(
        &out.matches("<div class=\"table\">\n<table>").count(),
        &1,
        &out,
    )?;
    Ok(())
}

#[test]
fn stats_by_a_key_without_a_value_leave_its_cell_empty() -> TestResult {
    let group = Group {
        keys: serde_json::from_value(json!({"tool": null}))?,
        sessions: Some(2),
        ..sample_groups()?.swap_remove(1)
    };
    let out = stats(
        &[group],
        &columns(&[By::Tool]),
        &tokyo()?,
        &Fields::default(),
        Lang::En,
        &Masking::Off,
    )?;
    check_eq(
        &out.contains("<tr><td></td><td class=\"number\">1</td><td class=\"number\">0</td><td class=\"number\">2</td>"),
        &true,
        &out,
    )?;
    Ok(())
}

/// The sample session, as a search found it: two matches, the first a
/// reply with markup in it.
fn searched_session() -> Result<Session, Box<dyn Error>> {
    let first = sample_session()?;
    Ok(Session {
        match_count: Some(2),
        matched_types: vec!["reply", "reply"],
        first_match: Some(FirstMatch {
            kind: "reply",
            timestamp: first.started_at,
            line: Some(3),
            text: "use <b>bold</b>\nsecond line".to_owned(),
        }),
        title: "<script>x</script>".to_owned(),
        ..first
    })
}

#[test]
fn a_search_adds_the_match_columns_and_counts_the_matches() -> TestResult {
    let out = sessions(
        &[searched_session()?],
        &tokyo()?,
        true,
        &Fields::default(),
        Lang::En,
        &Masking::Off,
    )?;
    // (what, the markup)
    for (what, markup) in [
        (
            "the counts",
            "<p class=\"count\">Sessions: 1</p>\n<p class=\"count\">Matches: 2 (reply 2)</p>",
        ),
        (
            "the match columns",
            "<th class=\"number\">Prompts</th><th class=\"number\">Matches</th><th>First match</th><th>Models</th><th>First prompt</th><th>Title</th>",
        ),
        (
            "the first match on one line, escaped",
            "<td class=\"number\">2</td><td>use &lt;b&gt;bold&lt;/b&gt; second line</td>",
        ),
        (
            "the title escaped",
            "<td>&lt;script&gt;x&lt;/script&gt;</td>",
        ),
    ] {
        check_eq(&out.contains(markup), &true, &format!("{what}:\n{out}"))?;
    }
    check_eq(&out.contains("<script"), &false, &out)?;
    Ok(())
}

#[test]
fn fields_keep_only_the_columns_they_name() -> TestResult {
    let zone = tokyo()?;
    let shown = sessions(
        &[searched_session()?],
        &zone,
        true,
        &fields(&["session_id", "match_count"], &[]),
        Lang::En,
        &Masking::Off,
    )?;
    check_eq(
        &shown.contains("<tr><th>Session</th><th class=\"number\">Matches</th></tr>"),
        &true,
        &shown,
    )?;
    let without = stats(
        &sample_groups()?,
        &columns(&[By::Session]),
        &zone,
        &fields(&[], &["usage", "file", "session_title"]),
        Lang::En,
        &Masking::Off,
    )?;
    check_eq(
        &without.contains("<tr><th>Session</th><th>Project</th><th class=\"number\">Count</th><th class=\"number\">Chars</th><th>First</th><th>Last</th></tr>"),
        &true,
        &without,
    )?;
    let none = sessions(
        &[sample_session()?],
        &zone,
        false,
        &fields(&["match_count"], &[]),
        Lang::En,
        &Masking::Off,
    )?;
    check_eq(&none.contains("<table"), &false, &none)?;
    check_eq(
        &none.contains("<p class=\"count\">Sessions: 1</p>"),
        &true,
        &none,
    )?;
    Ok(())
}

#[test]
fn the_tables_in_japanese() -> TestResult {
    let zone = tokyo()?;
    let listed = sessions(
        &[sample_session()?],
        &zone,
        false,
        &Fields::default(),
        Lang::Ja,
        &Masking::On(Vec::new()),
    )?;
    let counted = stats(
        &sample_groups()?,
        &columns(&[By::Session]),
        &zone,
        &Fields::default(),
        Lang::Ja,
        &Masking::On(Vec::new()),
    )?;
    // (what, the page, the markup): the language; the title セッション;
    // セッション: 1; the heading プロンプト数; the title 集計; グループ: 2;
    // the heading キャッシュ読み取り; and (無題) for a session without a title.
    for (what, out, markup) in [
        ("the language", &listed, "<html lang=\"ja\">"),
        (
            "the sessions title",
            &listed,
            "<title>\u{30bb}\u{30c3}\u{30b7}\u{30e7}\u{30f3}</title>",
        ),
        (
            "the sessions count",
            &listed,
            "<p class=\"count\">\u{30bb}\u{30c3}\u{30b7}\u{30e7}\u{30f3}: 1</p>",
        ),
        (
            "a sessions heading",
            &listed,
            "<th class=\"number\">\u{30d7}\u{30ed}\u{30f3}\u{30d7}\u{30c8}\u{6570}</th>",
        ),
        ("the stats title", &counted, "<h1>\u{96c6}\u{8a08}</h1>"),
        (
            "the stats count",
            &counted,
            "<p class=\"count\">\u{30b0}\u{30eb}\u{30fc}\u{30d7}: 2</p>",
        ),
        (
            "a stats heading",
            &counted,
            "<th class=\"number\">\u{30ad}\u{30e3}\u{30c3}\u{30b7}\u{30e5}\u{8aad}\u{307f}\u{53d6}\u{308a}</th>",
        ),
        (
            "an untitled session",
            &counted,
            "<td>(\u{7121}\u{984c})</td>",
        ),
    ] {
        check_eq(&out.contains(markup), &true, &format!("{what}:\n{out}"))?;
    }
    Ok(())
}

#[test]
fn the_tables_as_snapshots() -> TestResult {
    let zone = tokyo()?;
    snapshot(
        "render/sessions.html",
        &without_style(&sessions(
            &[sample_session()?, searched_session()?],
            &zone,
            true,
            &Fields::default(),
            Lang::En,
            &Masking::On(Vec::new()),
        )?)?,
    )?;
    snapshot(
        "render/stats.html",
        &without_style(&stats(
            &sample_groups()?,
            &columns(&[By::Session]),
            &zone,
            &Fields::default(),
            Lang::En,
            &Masking::On(Vec::new()),
        )?)?,
    )
}
