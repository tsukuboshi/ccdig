//! The words Markdown, HTML, and the digest write around the records, in each
//! language `--lang` takes. Every `match` names each language, and each
//! kind of thing it words, so a new one does not build until it has its
//! words. The headings of the `stats` columns are looked up by key
//! instead, and a test checks that every key has its Japanese one. The
//! notes of a cut are made before the records are rendered, so `text`
//! words them.

#![expect(
    clippy::non_ascii_literal,
    reason = "the Japanese words are written as they read, so the table can be checked by eye"
)]

use super::view::{Fact, Label};
use crate::record::Kind;
use crate::text::{Lang, escape_inline, one_line};

/// The columns of the `sessions` table, in the order they are shown.
pub const SESSION_COLUMNS: [Column; 8] = [
    Column::text("started_at", "Started", "開始"),
    Column::text("session_id", "Session", "セッション"),
    Column::text("project", "Project", "プロジェクト"),
    Column::text("cwd", "Cwd", "作業ディレクトリ"),
    Column::number("prompt_count", "Prompts", "プロンプト数"),
    Column::number("match_count", "Matches", "一致数"),
    Column::text("first_match", "First match", "最初の一致"),
    Column::text("title", "Title", "タイトル"),
];

/// The Japanese headings of the `stats` columns, by key. In English a
/// heading is the key itself, as `Session title` for `session_title`.
const STATS_COLUMNS: [(&str, &str); 26] = [
    ("session", "セッション"),
    ("project", "プロジェクト"),
    ("branch", "ブランチ"),
    ("type", "種別"),
    ("tool", "ツール"),
    ("skill", "スキル"),
    ("subagent", "サブエージェント"),
    ("model", "モデル"),
    ("kind", "プロンプトの種類"),
    ("denial", "却下理由"),
    ("status", "状態"),
    ("day", "日"),
    ("week", "週"),
    ("month", "月"),
    ("session_title", "セッションのタイトル"),
    ("file", "ファイル"),
    ("count", "件数"),
    ("empty", "空"),
    ("chars", "文字数"),
    ("sessions", "セッション数"),
    ("first", "最初"),
    ("last", "最後"),
    ("input", "入力"),
    ("output", "出力"),
    ("cache_read", "キャッシュ読み取り"),
    ("cache_creation", "キャッシュ作成"),
];

/// A column of a table, with its heading in each language.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Column {
    /// The heading in English.
    pub en: &'static str,
    /// The heading in Japanese.
    pub ja: &'static str,
    /// The key `--fields` names it by.
    pub key: &'static str,
    /// Whether the column holds a number, aligned right.
    pub number: bool,
}

/// How a tool call ended, as the badge in its fold says it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Badge {
    /// The user, a hook, or a setting turned it down.
    Denied,
    /// The tool reported a failure.
    Error,
    /// Its file has no result for it.
    Missing,
    /// It ran, and the tool reported no failure.
    Ok,
}

/// A list item of a tool call or a tool result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Item {
    /// Why the call did not run.
    Denial,
    /// Whether the tool reported a failure.
    Error,
    /// What the call came to.
    Result,
    /// The tool's name.
    Tool,
}

impl Column {
    /// The heading of the column in `lang`.
    pub const fn heading(self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => self.en,
            Lang::Ja => self.ja,
        }
    }

    /// A column of numbers.
    const fn number(key: &'static str, en: &'static str, ja: &'static str) -> Self {
        Self {
            en,
            ja,
            key,
            number: true,
        }
    }

    /// A column of text.
    const fn text(key: &'static str, en: &'static str, ja: &'static str) -> Self {
        Self {
            en,
            ja,
            key,
            number: false,
        }
    }
}

/// `title` made safe for a heading, a table cell, or the digest heading:
/// one line, with inline Markdown and HTML escaped; or the words for a
/// session without one, so it is never empty.
pub fn title(lang: Lang, title: &str) -> String {
    escape_inline(&plain_title(lang, title))
}

/// `title` on one line, or the words for a session without one, so it is
/// never empty; for a format that escapes it its own way.
pub fn plain_title(lang: Lang, title: &str) -> String {
    let line = one_line(title);
    if !line.is_empty() {
        return line;
    }
    match lang {
        Lang::En => "(untitled)",
        Lang::Ja => "(無題)",
    }
    .to_owned()
}

/// The title of the Markdown of `sessions`.
pub const fn sessions_title(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Sessions",
        Lang::Ja => "セッション",
    }
}

/// The line under the title of `sessions` that counts them.
pub fn session_count(lang: Lang, sessions: usize) -> String {
    match lang {
        Lang::En => format!("- Sessions: {sessions}\n"),
        Lang::Ja => format!("- セッション: {sessions}\n"),
    }
}

/// The line of `sessions` that counts the matches, `total`, with `by_type`
/// after the number.
pub fn match_count(lang: Lang, total: usize, by_type: &str) -> String {
    match lang {
        Lang::En => format!("- Matches: {total}{by_type}\n"),
        Lang::Ja => format!("- 一致: {total}{by_type}\n"),
    }
}

/// The title of the Markdown of `stats` and how many groups there are.
pub fn stats_head(lang: Lang, groups: usize) -> String {
    match lang {
        Lang::En => format!("# Stats\n\n- Groups: {groups}\n"),
        Lang::Ja => format!("# 集計\n\n- グループ: {groups}\n"),
    }
}

/// The heading of the `stats` column of `key`: in English the key itself,
/// as `Session title` for `session_title`.
pub fn stats_column(lang: Lang, key: &str) -> String {
    match lang {
        Lang::En => column_label(key),
        Lang::Ja => stats_column_ja(key).map_or_else(|| column_label(key), str::to_owned),
    }
}

/// The Japanese heading of the `stats` column of `key`, when it has one.
pub fn stats_column_ja(key: &str) -> Option<&'static str> {
    STATS_COLUMNS
        .iter()
        .find(|&&(name, _)| name == key)
        .map(|&(_, heading)| heading)
}

/// `key` as a heading: `session_title` as `Session title`.
fn column_label(key: &str) -> String {
    let words = key.replace('_', " ");
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// The title of the Markdown and the page of `records`.
pub const fn records_title(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Records",
        Lang::Ja => "記録",
    }
}

/// The count of `records`: how many records a search matched, with how
/// many are each of `types` after the number, and how many `--around`
/// added as context.
pub fn record_count(lang: Lang, matched: usize, types: &[(&str, usize)], context: usize) -> String {
    let by_type = by_type(types);
    let mut out = match lang {
        Lang::En => format!("Records: {matched}{by_type}"),
        Lang::Ja => format!("記録: {matched}{by_type}"),
    };
    if context > 0 {
        let count = context.to_string();
        out.extend(match lang {
            Lang::En => [" + ", &count, " context"],
            Lang::Ja => [" + 前後の記録 ", &count, ""],
        });
    }
    out
}

/// `types` and how many of each there are, as ` (prompt 2, call 1)`;
/// empty when there are none. The type names stay as `--type` takes them.
pub fn by_type(types: &[(&str, usize)]) -> String {
    if types.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = types
        .iter()
        .map(|&(name, count)| format!("{name} {count}"))
        .collect();
    format!(" ({})", parts.join(", "))
}

/// What masking did, as the top of the page says it: how many times each
/// of `counts` masked something, most first, or that nothing was masked.
pub fn masked(lang: Lang, counts: &[(String, usize)]) -> String {
    if counts.is_empty() {
        return match lang {
            Lang::En => "Masked nothing",
            Lang::Ja => "マスクしたものはありません",
        }
        .to_owned();
    }
    let parts: Vec<String> = counts
        .iter()
        .map(|&(ref rule, count)| match lang {
            Lang::En => format!("{count} by {rule}"),
            Lang::Ja => format!("{rule} {count} 件"),
        })
        .collect();
    match lang {
        Lang::En => format!("Masked {}", parts.join(", ")),
        Lang::Ja => format!("マスク: {}", parts.join("、")),
    }
}

/// What the top of the page says when `--no-mask` turned masking off.
pub const fn not_masked(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Not masked (--no-mask)",
        Lang::Ja => "マスクしていません (--no-mask)",
    }
}

/// The name of the list item that says `fact` under a section's heading.
pub const fn fact(lang: Lang, fact: Fact<'_>) -> &'static str {
    match (lang, fact) {
        (Lang::En, Fact::Project(_)) => "Project",
        (Lang::En, Fact::Session(_)) => "Session",
        (Lang::En, Fact::Subagent(_)) => "Subagent",
        (Lang::En, Fact::SubagentType(_)) => "Subagent type",
        (Lang::En, Fact::Description(_)) => "Description",
        (Lang::Ja, Fact::Project(_)) => "プロジェクト",
        (Lang::Ja, Fact::Session(_)) => "セッション",
        (Lang::Ja, Fact::Subagent(_)) => "サブエージェント",
        (Lang::Ja, Fact::SubagentType(_)) => "サブエージェントの種類",
        (Lang::Ja, Fact::Description(_)) => "説明",
    }
}

/// The name of the fact under a section's heading that says the git
/// branch.
pub const fn branch(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Branch",
        Lang::Ja => "ブランチ",
    }
}

/// The name of the fact under a section's heading that says when the
/// earliest and the latest record shown were written, which the filters
/// may make later and earlier than the session's start and end.
pub const fn span(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Records shown",
        Lang::Ja => "表示している記録の期間",
    }
}

/// The turn `turn` in a record's heading.
pub fn turn(lang: Lang, turn: usize) -> String {
    match lang {
        Lang::En => format!("turn {turn}"),
        Lang::Ja => format!("ターン {turn}"),
    }
}

/// The words a heading names `label` with.
pub const fn label(lang: Lang, label: Label) -> &'static str {
    match (lang, label) {
        (Lang::En, Label::Answer) => "Answer",
        (Lang::En, Label::Approval) => "Approval",
        (Lang::En, Label::Call) => "Tool call",
        (Lang::En, Label::Prompt) => "Prompt",
        (Lang::En, Label::Rejection) => "Rejection",
        (Lang::En, Label::Reply) => "Reply",
        (Lang::En, Label::Result) => "Tool result",
        (Lang::En, Label::Summary) => "Summary",
        (Lang::En, Label::Thinking) => "Thinking",
        (Lang::En, Label::TypedAnswer) => "Typed answer",
        (Lang::Ja, Label::Answer) => "回答",
        (Lang::Ja, Label::Approval) => "承認",
        (Lang::Ja, Label::Call) => "ツール呼び出し",
        (Lang::Ja, Label::Prompt) => "プロンプト",
        (Lang::Ja, Label::Rejection) => "却下",
        (Lang::Ja, Label::Reply) => "返答",
        (Lang::Ja, Label::Result) => "ツール結果",
        (Lang::Ja, Label::Summary) => "要約",
        (Lang::Ja, Label::Thinking) => "思考",
        (Lang::Ja, Label::TypedAnswer) => "自由記述の回答",
    }
}

/// What follows the heading of a summary, which Claude Code wrote when it
/// compacted the context.
pub const fn compacted(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "The context was compacted here",
        Lang::Ja => "ここで文脈を圧縮しました",
    }
}

/// What follows the heading of a record `--around` added as context.
pub const fn context(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "(context)",
        Lang::Ja => "(前後の記録)",
    }
}

/// The name of the list item `item`.
pub const fn item(lang: Lang, item: Item) -> &'static str {
    match (lang, item) {
        (Lang::En, Item::Denial) => "Denial",
        (Lang::En, Item::Error) => "Error",
        (Lang::En, Item::Result) => "Result",
        (Lang::En, Item::Tool) => "Tool",
        (Lang::Ja, Item::Denial) => "却下理由",
        (Lang::Ja, Item::Error) => "エラー",
        (Lang::Ja, Item::Result) => "結果",
        (Lang::Ja, Item::Tool) => "ツール",
    }
}

/// Whether the tool reported a failure, as the `Error` item says it.
pub const fn failed(lang: Lang, failed: bool) -> &'static str {
    match (lang, failed) {
        (Lang::En, true) => "yes",
        (Lang::En, false) => "no",
        (Lang::Ja, true) => "あり",
        (Lang::Ja, false) => "なし",
    }
}

/// What a call came to, as the `Result` item says it: whether the tool
/// reported a failure, or `None` when the file has no result for it.
pub const fn ending(lang: Lang, failed: Option<bool>) -> &'static str {
    match (lang, failed) {
        (Lang::En, Some(false)) => "ok",
        (Lang::En, Some(true)) => "error",
        (Lang::En, None) => "(none)",
        (Lang::Ja, Some(false)) => "成功",
        (Lang::Ja, Some(true)) => "エラー",
        (Lang::Ja, None) => "(なし)",
    }
}

/// The words of `badge`.
pub const fn badge(lang: Lang, badge: Badge) -> &'static str {
    match (lang, badge) {
        (Lang::En, Badge::Denied) => "denied",
        (Lang::En, Badge::Error) => "error",
        (Lang::En, Badge::Missing) => "no result",
        (Lang::En, Badge::Ok) => "ok",
        (Lang::Ja, Badge::Denied) => "却下",
        (Lang::Ja, Badge::Error) => "エラー",
        (Lang::Ja, Badge::Missing) => "結果なし",
        (Lang::Ja, Badge::Ok) => "成功",
    }
}

/// What stands for the name of a tool whose call was not found.
pub const fn unknown_tool(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "(unknown)",
        Lang::Ja => "(不明)",
    }
}

/// What names lines `first` to `last` of `field`, written as the format
/// writes a key, or of the record's text when `None`, and when `hits` is
/// not empty, the lines among them a search matched.
pub fn lines(
    lang: Lang,
    field: Option<&str>,
    first: usize,
    last: usize,
    hits: &[String],
) -> String {
    let span = if first == last {
        first.to_string()
    } else {
        format!("{first}-{last}")
    };
    let mut out = match (lang, field) {
        (Lang::En, None) if first == last => format!("Line {span}"),
        (Lang::En, None) => format!("Lines {span}"),
        (Lang::En, Some(name)) if first == last => format!("{name} line {span}"),
        (Lang::En, Some(name)) => format!("{name} lines {span}"),
        (Lang::Ja, None) => format!("{span} 行目"),
        (Lang::Ja, Some(name)) => format!("{name} {span} 行目"),
    };
    if !hits.is_empty() {
        out.push_str(match lang {
            Lang::En => ", matched ",
            Lang::Ja => "、一致 ",
        });
        out.push_str(&hits.join(match lang {
            Lang::En => ", ",
            Lang::Ja => "、",
        }));
    }
    out
}

/// What the digest writes before a prompt of `kind`, with a space after
/// it, or nothing for a prompt typed as plain text or a slash command.
pub const fn prompt_mark(lang: Lang, kind: Kind) -> &'static str {
    match (lang, kind) {
        (_, Kind::Slash | Kind::Text) => "",
        (Lang::En, Kind::Answer) => "(answer) ",
        (Lang::En, Kind::Approval) => "(approval) ",
        (Lang::En, Kind::Rejection) => "(rejection) ",
        (Lang::En, Kind::TypedAnswer) => "(typed answer) ",
        (Lang::Ja, Kind::Answer) => "(回答) ",
        (Lang::Ja, Kind::Approval) => "(承認) ",
        (Lang::Ja, Kind::Rejection) => "(却下) ",
        (Lang::Ja, Kind::TypedAnswer) => "(自由記述の回答) ",
    }
}

/// What the digest writes before the first line of the reply a prompt
/// came after.
pub const fn after(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "  - after: ",
        Lang::Ja => "  - 直前の返答: ",
    }
}

/// The digest's line for the `count` prompts of a session that did not
/// fit.
pub fn omitted(lang: Lang, count: usize) -> String {
    match lang {
        Lang::En => format!("- ({count} more prompts omitted)\n"),
        Lang::Ja => format!("- (ほか {count} 件のプロンプトを省略)\n"),
    }
}

/// What the digest writes in place of a paste of `chars` characters.
pub fn pasted(lang: Lang, chars: usize) -> String {
    match lang {
        Lang::En => format!("[pasted {chars} chars]"),
        Lang::Ja => format!("[貼り付け {chars} 文字]"),
    }
}
