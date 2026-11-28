//! Tests for the view of the records, without Markdown.

use core::error::Error;
use core::slice::from_ref;

use serde_json::json;

use super::{
    AGENT_FILE, BLOG_FILE, BLOG_SESSION, at, bare_prompt, blog_head, call_at, context, every_part,
    fields, found, outcome, reply_at, result_at, source,
};
use crate::record::{Head, Prompt, Reply, Search, Spawned, ToolCall, ToolResult, Turn};
use crate::render::json::Fields;
use crate::render::view::{
    Body, CallBody, Count, Ending, Fact, Input, Label, Passage, ResultBody, Run, Tool, View,
    records,
};
use crate::testing::{TestResult, check_eq};
use crate::when::Stamp;

/// A case of `check_bodies`: its name, the record, the `--fields`, and the
/// body the record has.
type BodyCase<'records> = (&'static str, &'records Turn, Fields, Body<'records>);

/// What an item's heading says besides its label: the time, the turn,
/// whether a search matched it, and its sub-agent.
type Heading<'records> = (Option<Stamp>, Option<usize>, bool, Option<&'records str>);

/// An item's label, with what it came to when it is a call.
type Shown<'records> = (Label, Option<Ending<'records>>);

/// Each section's branch, parent, and span, and each item's origin and
/// call ID.
type Origins = (
    Vec<(
        Option<&'static str>,
        Option<&'static str>,
        Option<(Stamp, Stamp)>,
    )>,
    Vec<(&'static str, &'static str, usize, Option<&'static str>)>,
);

/// A section's title and facts.
type Top<'records> = (&'records str, Vec<Fact<'records>>);

/// The label of each item the view shows, with what each call came to.
fn shown(turns: &[Turn]) -> Result<Vec<Shown<'_>>, Box<dyn Error>> {
    let view = records(turns, &Fields::default())?;
    Ok(view
        .sections
        .into_iter()
        .flat_map(|section| section.items)
        .map(|item| {
            let ending = match item.body {
                Body::Call(call) => call.ending,
                Body::Result(_) | Body::Text(_) => None,
            };
            (item.label, ending)
        })
        .collect())
}

#[test]
fn a_result_shows_under_its_call_unless_it_has_to_stand_alone() -> TestResult {
    let ran = outcome(false, "", "done");
    let call = |search| -> Result<Turn, String> {
        Ok(Turn::Call(ToolCall {
            search,
            ..call_at(1, "Bash", json!({}), "toolu_1", Some(ran.clone()))?
        }))
    };
    let result = |search, file: &str| -> Result<Turn, String> {
        Ok(Turn::Result(ToolResult {
            search,
            source: source(file, 2, ""),
            ..result_at(2, "Bash", "toolu_1", &ran)?
        }))
    };
    let with_output = Some(Ending::Ran {
        denial: None,
        failed: false,
        output: Some("done"),
    });
    let without_output = Some(Ending::Ran {
        denial: None,
        failed: false,
        output: None,
    });
    let matched = Search::default;
    for (name, turns, want) in [
        (
            "both matched",
            vec![call(matched())?, result(matched(), BLOG_FILE)?],
            vec![(Label::Call, with_output)],
        ),
        (
            "both context",
            vec![call(context())?, result(context(), BLOG_FILE)?],
            vec![(Label::Call, with_output)],
        ),
        (
            "the result lists matched lines",
            vec![
                call(matched())?,
                result(found(&[("text", 1, "done", true)]), BLOG_FILE)?,
            ],
            vec![(Label::Call, without_output), (Label::Result, None)],
        ),
        (
            "a search matched the result and not its call",
            vec![call(context())?, result(matched(), BLOG_FILE)?],
            vec![(Label::Call, without_output), (Label::Result, None)],
        ),
        (
            "the call is not listed",
            vec![result(matched(), BLOG_FILE)?],
            vec![(Label::Result, None)],
        ),
        (
            "the call is in another file",
            vec![call(matched())?, result(matched(), "/other.jsonl")?],
            vec![(Label::Call, with_output), (Label::Result, None)],
        ),
    ] {
        check_eq(&shown(&turns)?, &want, name)?;
    }
    Ok(())
}

#[test]
fn the_count_splits_the_matched_records_by_type_from_the_context() -> TestResult {
    for (name, turns, want) in [
        (
            "no records",
            Vec::new(),
            Count {
                context: 0,
                matched: 0,
                types: Vec::new(),
            },
        ),
        (
            "every part",
            every_part()?,
            Count {
                context: 2,
                matched: 14,
                types: vec![
                    ("prompt", 5),
                    ("thinking", 1),
                    ("reply", 2),
                    ("call", 3),
                    ("result", 2),
                    ("summary", 1),
                ],
            },
        ),
    ] {
        check_eq(&records(&turns, &Fields::default())?.count, &want, name)?;
    }
    Ok(())
}

/// The title and facts of each section of `view`, and the heading of each
/// item.
fn heads<'records>(view: &View<'records>) -> (Vec<Top<'records>>, Vec<Heading<'records>>) {
    (
        view.sections
            .iter()
            .map(|section| (section.title, section.facts.clone()))
            .collect(),
        view.sections
            .iter()
            .flat_map(|section| &section.items)
            .map(|item| (item.stamp, item.turn, item.matched, item.subagent))
            .collect(),
    )
}

#[test]
fn each_heading_names_the_facts_fields_keeps() -> TestResult {
    let (first, then) = ("2026-10-13T01:23:00Z", "2026-10-13T01:25:30Z");
    let prompt = Turn::Prompt(Prompt {
        head: Head {
            turn: 1,
            ..blog_head(first)?
        },
        ..bare_prompt(blog_head(first)?, BLOG_FILE, "hi")
    });
    let agent = |kind: &str, description: &str| -> Result<Turn, String> {
        Ok(Turn::Reply(Reply {
            subagent: "agent-7".to_owned(),
            spawned: Spawned {
                subagent_type: kind.to_owned(),
                subagent_description: description.to_owned(),
                parent_tool_use_id: String::new(),
            },
            search: context(),
            ..reply_at(then, AGENT_FILE, 1, "Looked.")?
        }))
    };
    let (stamp, later) = (at(first)?, at(then)?);
    let title = "Weekly report draft";
    let project = Fact::Project("-Users-alice-blog");
    let session = Fact::Session(BLOG_SESSION);
    let heading_facts = [
        "project",
        "session_id",
        "subagent",
        "subagent_type",
        "subagent_description",
        "timestamp",
        "turn",
    ];
    for (name, turns, kept, want) in [
        (
            "every key",
            vec![prompt.clone(), agent("Explore", "Look in *src*")?],
            Fields::default(),
            (
                vec![
                    (title, vec![project, session]),
                    (
                        title,
                        vec![
                            project,
                            session,
                            Fact::Subagent("agent-7"),
                            Fact::SubagentType("Explore"),
                            Fact::Description("Look in *src*"),
                        ],
                    ),
                ],
                vec![
                    (Some(stamp), Some(1), true, None),
                    (Some(later), None, false, Some("agent-7")),
                ],
            ),
        ),
        (
            "a blank type and description",
            vec![agent("", " \n ")?],
            Fields::default(),
            (
                vec![(title, vec![project, session, Fact::Subagent("agent-7")])],
                vec![(Some(later), None, false, Some("agent-7"))],
            ),
        ),
        (
            "none of the keys",
            vec![prompt, agent("Explore", "Look in *src*")?],
            fields(&[], &heading_facts),
            (
                vec![(title, Vec::new()), (title, Vec::new())],
                vec![(None, None, true, None), (None, None, false, None)],
            ),
        ),
    ] {
        check_eq(&heads(&records(&turns, &kept)?), &want, name)?;
    }
    Ok(())
}

/// Checks that each of `cases`, a record alone with the parts its
/// `--fields` keeps, has the body it names.
fn check_bodies<const N: usize>(cases: [BodyCase<'_>; N]) -> TestResult {
    for (name, turn, kept, want) in cases {
        let view = records(from_ref(turn), &kept)?;
        let item = view
            .sections
            .into_iter()
            .flat_map(|section| section.items)
            .next()
            .ok_or("no item")?;
        check_eq(&item.body, &want, name)?;
    }
    Ok(())
}

#[test]
fn a_text_shows_its_matched_lines_or_else_itself_as_fields_keeps() -> TestResult {
    let plain = Turn::Reply(reply_at("2026-10-13T01:24:00Z", BLOG_FILE, 1, "hello")?);
    let searched = Turn::Reply(Reply {
        search: found(&[
            ("text", 1, "first hit", true),
            ("text", 3, "before", false),
            ("text", 4, "second hit", true),
            ("input.command", 5, "elsewhere", true),
        ]),
        ..reply_at("2026-10-13T01:24:00Z", BLOG_FILE, 1, "hello")?
    });
    let Turn::Reply(ref reply) = searched else {
        return Err("not a reply".into());
    };
    let lines = &reply.search.matches;
    let runs = vec![
        Run {
            field: None,
            first: 1,
            last: 1,
            lines: lines.get(..1).ok_or("line 1")?,
        },
        Run {
            field: None,
            first: 3,
            last: 4,
            lines: lines.get(1..3).ok_or("lines 3-4")?,
        },
        Run {
            field: Some("input.command"),
            first: 5,
            last: 5,
            lines: lines.get(3..).ok_or("line 5")?,
        },
    ];
    check_bodies([
        (
            "the text",
            &plain,
            Fields::default(),
            Body::Text(Passage::Text("hello")),
        ),
        (
            "no text",
            &plain,
            fields(&[], &["text"]),
            Body::Text(Passage::Hidden),
        ),
        (
            "the matched lines in runs",
            &searched,
            Fields::default(),
            Body::Text(Passage::Lines(runs.clone())),
        ),
        (
            "the matched lines without the text",
            &searched,
            fields(&[], &["text"]),
            Body::Text(Passage::Lines(runs)),
        ),
        (
            "the text without the matched lines",
            &searched,
            fields(&[], &["matches"]),
            Body::Text(Passage::Text("hello")),
        ),
        (
            "neither",
            &searched,
            fields(&[], &["matches", "text"]),
            Body::Text(Passage::Hidden),
        ),
    ])
}

#[test]
fn a_call_shows_its_tool_input_and_ending_as_fields_keeps() -> TestResult {
    let mixed = Turn::Call(call_at(
        1,
        "Bash",
        json!({"command": "ls", "plan": "step 1\nstep 2\n"}),
        "toolu_1",
        Some(outcome(false, "", "done")),
    )?);
    let texts_only = Turn::Call(call_at(
        1,
        "",
        json!({"plan": "one\ntwo"}),
        "toolu_1",
        None,
    )?);
    let searched = Turn::Call(ToolCall {
        search: found(&[("input.command", 1, "git add .", true)]),
        ..call_at(1, "Bash", json!({}), "toolu_1", None)?
    });
    let Turn::Call(ref call) = searched else {
        return Err("not a call".into());
    };
    let shown = Input::Shown {
        json: Some("{\n  \"command\": \"ls\"\n}".to_owned()),
        texts: vec![("plan", "step 1\nstep 2")],
    };
    check_bodies([
        (
            "every part",
            &mixed,
            Fields::default(),
            Body::Call(CallBody {
                ending: Some(Ending::Ran {
                    denial: None,
                    failed: false,
                    output: Some("done"),
                }),
                gist: Some("ls"),
                input: shown,
                tool: Some(Tool::Named("Bash")),
            }),
        ),
        (
            "texts alone, no tool name, and no result",
            &texts_only,
            Fields::default(),
            Body::Call(CallBody {
                ending: Some(Ending::Missing),
                gist: None,
                input: Input::Shown {
                    json: None,
                    texts: vec![("plan", "one\ntwo")],
                },
                tool: Some(Tool::Unknown),
            }),
        ),
        (
            "matched lines for the input",
            &searched,
            Fields::default(),
            Body::Call(CallBody {
                ending: Some(Ending::Missing),
                gist: None,
                input: Input::Lines(vec![Run {
                    field: Some("input.command"),
                    first: 1,
                    last: 1,
                    lines: &call.search.matches,
                }]),
                tool: Some(Tool::Named("Bash")),
            }),
        ),
        (
            "none of the parts",
            &mixed,
            fields(&[], &["tool", "input", "result"]),
            Body::Call(CallBody {
                ending: None,
                gist: None,
                input: Input::Hidden,
                tool: None,
            }),
        ),
    ])
}

#[test]
fn a_result_shows_its_tool_failure_denial_and_text_as_fields_keeps() -> TestResult {
    let denied = outcome(true, "user-rejected", "No.");
    let result = Turn::Result(result_at(1, "Bash", "toolu_1", &denied)?);
    check_bodies([
        (
            "every part",
            &result,
            Fields::default(),
            Body::Result(ResultBody {
                denial: Some("user-rejected"),
                failed: Some(true),
                passage: Passage::Text("No."),
                tool: Some(Tool::Named("Bash")),
            }),
        ),
        (
            "none of the parts",
            &result,
            fields(&[], &["tool", "is_error", "denial", "text"]),
            Body::Result(ResultBody {
                denial: None,
                failed: None,
                passage: Passage::Hidden,
                tool: None,
            }),
        ),
    ])
}

#[test]
fn each_item_and_section_says_where_it_comes_from_as_fields_keeps() -> TestResult {
    let ran = outcome(false, "", "done");
    let mut call = call_at(5, "Agent", json!({}), "toolu_a", Some(ran.clone()))?;
    call.source.uuid = "u-call".to_owned();
    call.source.block = 1;
    let sub_agent = Turn::Reply(Reply {
        subagent: "agent-7".to_owned(),
        spawned: Spawned {
            parent_tool_use_id: "toolu_a".to_owned(),
            ..Spawned::default()
        },
        head: Head {
            git_branch: String::new(),
            ..blog_head("2026-10-13T01:25:30Z")?
        },
        ..reply_at("2026-10-13T01:25:30Z", AGENT_FILE, 1, "Looked.")?
    });
    let turns = [
        Turn::Reply(reply_at("2026-10-13T01:26:00Z", BLOG_FILE, 2, "later")?),
        Turn::Call(call),
        Turn::Result(ToolResult {
            search: found(&[("text", 1, "done", true)]),
            ..result_at(6, "Agent", "toolu_a", &ran)?
        }),
        sub_agent,
    ];
    // (name, the `--fields`, each section's branch, parent, and span, and
    // each item's origin and call ID)
    let session_span = (at("2026-10-13T01:25:00Z")?, at("2026-10-13T01:26:00Z")?);
    let agent_span = (at("2026-10-13T01:25:30Z")?, at("2026-10-13T01:25:30Z")?);
    let cases: [(&str, Fields, Origins); 2] = [
        (
            "every key",
            Fields::default(),
            (
                vec![
                    (Some("main"), None, Some(session_span)),
                    (None, Some("toolu_a"), Some(agent_span)),
                ],
                vec![
                    ("reply", "", 0, None),
                    ("call", "u-call", 1, Some("toolu_a")),
                    ("result", "", 0, Some("toolu_a")),
                    ("reply", "", 0, None),
                ],
            ),
        ),
        (
            "without the branch, the time, and the source",
            fields(&[], &["git_branch", "timestamp", "source"]),
            (
                vec![(None, None, None), (None, Some("toolu_a"), None)],
                vec![
                    ("reply", "", 0, None),
                    ("call", "", 1, Some("toolu_a")),
                    ("result", "", 0, Some("toolu_a")),
                    ("reply", "", 0, None),
                ],
            ),
        ),
    ];
    for (name, kept, (sections, items)) in cases {
        let view = records(&turns, &kept)?;
        let got_sections: Vec<_> = view
            .sections
            .iter()
            .map(|section| (section.branch, section.parent, section.span))
            .collect();
        check_eq(&got_sections, &sections, name)?;
        let got_items: Vec<_> = view
            .sections
            .iter()
            .flat_map(|section| &section.items)
            .map(|item| {
                (
                    item.origin.kind,
                    item.origin.uuid,
                    item.origin.block,
                    item.call_id,
                )
            })
            .collect();
        check_eq(&got_items, &items, name)?;
    }
    Ok(())
}
