//! Properties of streaming a transcript that hold for any file.

use proptest::arbitrary::any;
use proptest::collection::vec;
use proptest::prop_oneof;
use proptest::sample::select;
use proptest::strategy::{Just, Strategy};
use serde_json::json;

use super::read_with;
use crate::testing::{TestResult, check_eq, for_all, hostile_text};
use crate::transcript::entry::{Role, decode_line};
use crate::transcript::reader::Roles;

/// One line of a transcript: an entry of some type, text that is not JSON,
/// a line cut off after the type, or a blank line.
fn line() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => (select(&["user", "assistant", "ai-title", "summary"][..]), hostile_text()).prop_map(
            |(kind, text)| json!({"type": kind, "aiTitle": text, "message": {"content": text}}).to_string()
        ),
        1 => hostile_text().prop_map(|text| text.replace('\n', " ")),
        1 => hostile_text().prop_map(|text| format!("{{\"type\":\"user\",{}", text.replace('\n', " "))),
        1 => Just(String::new()),
    ]
}

/// A transcript of up to 12 lines, with or without a final newline.
fn file() -> impl Strategy<Value = String> {
    (vec(line(), 0..12), any::<bool>()).prop_map(|(lines, newline)| {
        let mut text = lines.join("\n");
        if newline {
            text.push('\n');
        }
        text
    })
}

/// Whether `roles` asks for entries of `role`; titles are always read.
const fn wanted(roles: Roles, role: Role) -> bool {
    match role {
        Role::AiTitle => true,
        Role::Assistant => roles.assistant,
        Role::User => roles.user,
        Role::Other => false,
    }
}

#[test]
fn any_buffer_reads_what_a_line_by_line_reading_finds() -> TestResult {
    let role_sets = select(&[Roles::USER, Roles::ASSISTANT, Roles::BOTH][..]);
    for_all(
        &(file(), 1_usize..64, 0_usize..300, role_sets),
        |(text, buffer, max, roles)| {
            let (seen, stats) = read_with(&text, buffer, max, roles)?;
            check_eq(
                &read_with(&text, 8192, max, roles)?,
                &(seen.clone(), stats),
                "large buffer",
            )?;
            let mut lines: Vec<&str> = text.split('\n').collect();
            if text.ends_with('\n') || text.is_empty() {
                lines.pop();
            }
            let oversized = lines.iter().filter(|line| line.len() > max).count();
            check_eq(&stats.oversized, &oversized, "oversized")?;
            let delivered: Vec<(usize, Role)> = lines
                .iter()
                .zip(1_usize..)
                .filter(|&(line, _)| line.len() <= max)
                .filter_map(|(line, number)| {
                    decode_line(line.as_bytes()).map(|entry| (number, entry.role))
                })
                .filter(|&(_, role)| wanted(roles, role))
                .collect();
            let got: Vec<(usize, Role)> = seen
                .iter()
                .map(|&(number, role, _)| (number, role))
                .collect();
            check_eq(&got, &delivered, "delivered lines")?;
            Ok(())
        },
    )
}
