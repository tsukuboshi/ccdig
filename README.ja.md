# ccdig

[![CI](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml/badge.svg)](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml)

Claude Code の会話履歴から、共有したいログだけを、整形・マスクして
取り出す CLI です。
期間・プロジェクト・セッション・ブランチ・語句・レコードの種類・ツール・
成否・スキルを組み合わせて必要な部分に絞り込みます。`records` と `sessions` は、
端末でもパイプでも既定で加工しやすい JSON Lines を出し、`--format md` を
付けると読みやすい Markdown を出します。
よくある秘密情報は既定でマスクし、メールアドレス・IP アドレス・ユーザー名・
指定した値も追加でマスクできます。

[English](README.md)

## インストール

各[リリース](https://github.com/tsukuboshi/ccdig/releases)に、
Apple silicon の macOS 向けビルド済みバイナリを添付しています。
署名のないバイナリを macOS に隔離されないよう、ブラウザではなく `curl` で
取得してください。
次の手順は `ccdig` を `~/.local/bin` に置きます。
このディレクトリが `PATH` に入っていなければ追加してください。

```sh
base=https://github.com/tsukuboshi/ccdig/releases/latest/download
curl -fsSLO "$base/ccdig-aarch64-apple-darwin.tar.gz"
curl -fsSLO "$base/ccdig-aarch64-apple-darwin.tar.gz.sha256"
shasum -a 256 -c ccdig-aarch64-apple-darwin.tar.gz.sha256
tar -xzf ccdig-aarch64-apple-darwin.tar.gz
install -d ~/.local/bin
install ccdig-aarch64-apple-darwin/ccdig ~/.local/bin/
~/.local/bin/ccdig version
```

ほかのプラットフォームでは、Rust 1.100.0 以降でビルドします。

```sh
cargo install --locked --git https://github.com/tsukuboshi/ccdig
ccdig version
```

クローンからビルドしてテストを実行するには、次のようにします。

```sh
git clone https://github.com/tsukuboshi/ccdig && cd ccdig
cargo test --locked
cargo install --path . --locked
```

## クイックスタート

```sh
# 1 週間分の依頼文を、1 行に 1 つの JSON オブジェクトで出す
ccdig records --type prompt --since 2026-10-05 --until 2026-10-11

# blog プロジェクトで "deploy" に触れた返答を、大文字小文字を区別せずに出す
ccdig records --type reply --project blog --igrep deploy

# Claude Code の中から、検索している当のセッションを除いて、ほかの
# セッションの依頼文と返答を 20 件まで検索する
ccdig records --session '!current' --type prompt,reply --igrep deploy \
  --format md --limit 20

# "deploy" に最も多く触れたセッションを、最初に一致した行付きで、
# トークン数を省いて出す
ccdig sessions --igrep deploy --sort matches --fields '!usage' --limit 10

# 失敗した Bash の呼び出しを、サブエージェントも含めて Markdown で出す
ccdig records --type result --tool Bash --status error --subagents --format md

# LLM に貼るための、サイズ上限付きの依頼文の Markdown ダイジェスト
ccdig digest --since 2026-10-05 --until 2026-10-11

# 人に渡すために 1 セッションを会話の流れで出し、メールアドレス・IP
# アドレス・ユーザー名・ファイルに書いた値もマスクする
ccdig records --session 1a2b --format md --mask email,ip,user \
  --mask-values @secrets.json
```

## コマンド

| コマンド | 出すもの | 専用のオプション |
| --- | --- | --- |
| `records` | `--type` で選んだ種類の依頼文・返答・ツールの呼び出し・ツールの結果・要約 1 件につき 1 レコード。書かれた順に並ぶ | `--type`, `--tool`, `--status`, `--skill`, `--subagents`, `--around`, `--context`, `--format`, `--fields`, `--max-message-chars`, `--max-tool-chars` |
| `sessions` | 選ばれた依頼文を含むセッション 1 つにつき 1 レコード | `--format`, `--fields`, `--sort` |
| `digest` | サイズ上限付きの、依頼文の Markdown ダイジェスト | `--max-prompt-chars`, `--max-session-chars` |
| `version` | インストールされているバージョン | `--format` |

どのコマンドも、「共通オプション」の節のオプションも取ります。

### records

`--type` はレコードの種類を選び、カンマ区切りでも繰り返しでも指定できます。
既定は `prompt,reply,call` です。

| 種類 | レコード |
| --- | --- |
| `prompt` | 人間が打った依頼文 |
| `reply` | assistant のテキストブロック |
| `call` | ツールの呼び出し |
| `result` | ツールの結果。呼び出しのツールと入力付き |
| `summary` | コンテキストを圧縮したときに Claude Code が書く要約 |

次の 4 つのオプションは、一部の種類にだけ効きます。

| フラグ | 効く種類 | 残すもの |
| --- | --- | --- |
| `--tool` | `call`, `result` | このツールの呼び出しとその結果 |
| `--status` | `result` | この状態の結果。`error` か `ok` |
| `--skill` | `reply`, `call`, `result` | そのスキルの実行中に書かれたレコード |
| `--subagents` | `reply`, `call`, `result` | サブエージェントの履歴も含める |

- `--tool` と `--skill` は、共通の絞り込みと同じく繰り返し指定でき、先頭の
  `!` も使えます。`--status` は `--type` と同じくカンマ区切りで並べられます。
- `--tool`・`--status`・`--skill` は、調べる値を持つ種類だけを絞り、持たない
  種類はそのまま残します。そのため
  `ccdig records --type prompt,reply,call --skill tdd` は、すべての依頼文と、
  `tdd` の実行中に書かれた返答と呼び出しを残します。
- `--type` を省略すると、`records` は付けた `--tool`・`--status`・`--skill` が
  すべて効く種類だけを出します。`ccdig records --tool Bash` なら、`Bash` の
  呼び出しとその結果です。
- この 4 つのフラグは、効く種類が `--type` に無いとエラーになります。
- `--type` を付けずに検索すると、ツールの呼び出しも対象になり、呼び出しの
  入力は長くなりがちです。`--type prompt,reply` と `--limit` を付けると、
  出力を小さく保てます。`--max-message-chars` と `--max-tool-chars` で
  本文を切り、`--fields` でキーを省くこともできます。
- サブエージェントの履歴は、`--subagents` を付けない限り読みません。
  サブエージェントに渡された指示は人間が打った依頼文ではないので、出力
  されません。出力されるのは、その返答・呼び出し・結果です。
- `--around N` は、`--grep` か `--igrep` に一致した各レコードの前後 N 件の
  レコードも出します。一致した返答が答えた依頼文などです。対象は、
  `--type` で選んだ種類のうち、ほかの絞り込みをすべて通る、同じファイルの
  レコードです。これらは `matched` が false で、Markdown では種類の後に
  `(context)` が付きます。`!` 付きのパターンで除いたレコードは、前後の
  レコードとしても出ません。`!` で始まらないパターンが必要です。

次の 4 つのオプションは出力の形を決めます。

| フラグ | 働き |
| --- | --- |
| `--context N` | 各レコードで `--grep` か `--igrep` に一致した行を、前後 N 行と一緒に `matches` に並べる。Markdown では、全文の代わりにその行を出す。付けなければ、JSON と JSON Lines は一致した行だけを並べる |
| `--format` | `jsonl`（既定）は 1 行に 1 つの JSON オブジェクト、`json` は 1 つの文書、`md` は読むための Markdown を書く |
| `--max-message-chars N` | 依頼文・返答・要約の本文を N 文字で切って `… (M more characters)` を付ける |
| `--max-tool-chars N` | ツールの入力にある文字列とツールの結果の出力を、それぞれ N 文字で切る。ファイル全体を書く `Write` で出力が埋まらずに済む |

### sessions

選ばれた依頼文を含むセッション 1 つにつき 1 レコードを出します。絞り込みは
依頼文に適用されます。`--format` は `jsonl`（既定）・`json`・`md` を取ります。

- `--grep` と `--igrep` は、依頼文か返答が一致したセッションを選び、
  `match_count` にそれぞれで一致した件数を入れます。ある語に最も多く触れた
  セッションから読めます。ほかのキーは、`--grep` が無いときと同じく、
  ほかの絞り込みで残ったすべての依頼文から作ります。ただし、`!` 付きの
  パターンで除いた依頼文は数えません。Markdown には
  `Matches` 列が加わります。`!` で始まらないパターンが無ければ、
  `match_count` は `null` です。
- `prompt_count` と `started_at` は、選ばれた依頼文の件数と最初の時刻です。
  `ended_at` は、範囲内でそのセッションの最後の依頼文・返答・ツール呼び出し・
  ツール結果の時刻なので、依頼文が 1 件のセッションでも作業の終わりが分かります。
- `first_match` は、そのセッションで検索が最初に一致した依頼文か返答です。
  レコードを読まずにセッションを見極められます。中身は `type`・`timestamp`・
  本文でパターンが一致した行番号 `line`・その行（マスク後、1 行にまとめて
  200 文字で切ったもの）の `text` です。一致が行をまたぐときは、`line` が
  `null` で、`text` は本文全体の先頭です。
  `!` で始まらないパターンが無ければ、`first_match` は
  `null` です。Markdown には `Matches` の隣に `First match` 列が加わります。
- `--sort matches` は一致の多いセッションから並べ、同数なら時刻順にします。
  `--limit` は並べ替えた後の先頭 N 件を残します。`!` で始まらない
  パターンが必要です。既定の `--sort time` は、選ばれた最初の依頼文の
  時刻順に並べます。

レコードは次のキーをこの順に持ちます。

| キー | 中身 |
| --- | --- |
| `agent` | `claude-code` |
| `session_id` | セッション ID |
| `title` | セッションの最後のタイトル。無ければ空 |
| `project` | プロジェクトのディレクトリ名 |
| `cwd`, `git_branch` | 選ばれた最初の依頼文のもの |
| `file` | transcript のファイル |
| `mtime` | ファイルの最終書き込み時刻 |
| `started_at`, `ended_at`, `prompt_count`, `match_count`, `first_match` | 上のとおり |
| `usage` | トークン数。「トークン数」の節を参照 |

### digest

依頼文の Markdown ダイジェストを、サイズの上限付きで出します。絞り込みは
依頼文に適用されます。

| フラグ | 既定 | 働き |
| --- | --- | --- |
| `--max-prompt-chars N` | 300 | 依頼文をそれぞれ N 文字で切る |
| `--max-session-chars N` | 2000 | セッションごとに最大 N 文字まで並べる |

### version

`ccdig version` はインストールされているバージョンを出します。
`--format json` では、バージョンと一緒に `schema` と `filter` を出します。
この 2 つの意味は「JSON の包み」の節にあります。

```json
{"version":"0.0.2","schema":3,"filter":"strict-v2"}
```

## 共通オプション

どのコマンドも、次のオプションをコマンド名の前にも後にも置けます。繰り返し
指定できるフラグの値は、前後で足し合わせます。

### 絞り込み

| フラグ | 残すもの |
| --- | --- |
| `--since`, `--until` | 開始日から終了日までのレコード。`YYYY-MM-DD` |
| `--tz` | 日の境界と出力の時刻（Markdown も JSON も）をこの IANA タイムゾーンで扱う。既定はシステムのタイムゾーン（`TZ`、なければ `/etc/localtime`） |
| `--project` | ディレクトリ名か cwd にこの値を含むプロジェクト |
| `--session` | ID がこの値で始まるセッション。`current` なら ccdig を実行しているセッション |
| `--branch` | この git ブランチ上のレコード |
| `--grep` | テキストが正規表現に一致するレコード |
| `--igrep` | 同じく、大文字小文字を区別せずに一致するレコード |

- `--project`・`--session`・`--branch`・`--grep`・`--igrep` は繰り返し
  指定でき、いずれかの値に一致するレコードを残します。
- 先頭に `!` を付けた値は、逆に一致するレコードを除きます。たとえば
  `--session '!1a2b'` は 1 つを除くすべてのセッションを、`--grep '!WIP'` は
  `WIP` に触れたレコードを除いた残りを出します。`!` で始まる値そのものに
  一致させるときは `\!` と書きます。たいていのシェルでは `!` が履歴展開に
  なるので、引用符で囲んでください。
- `--session current` は、Claude Code が ccdig を実行しているセッションを
  指し、`CLAUDE_CODE_SESSION_ID` から読みます。`--session '!current'` で、
  検索している当のセッションを除けます。Claude Code の外では `current` は
  エラーになります。
- `--grep` は、出力されるときのテキスト、つまりマスク後のテキストを
  検索します。そのため、秘密情報をその値で探し当てることはできません。
  対象は、依頼文や返答のテキスト、呼び出しの入力を詰めた JSON、結果の
  テキストです。

### 入力

| フラグ | 働き |
| --- | --- |
| `--root` | このディレクトリの下の履歴を読む。既定は「環境変数」の節の変数で決まり、なければ `~/.claude/projects` |
| `--on-skip` | ファイルや行を読めなかったときの扱い。`warn`（既定）は報告して続け、`fail` は出力を書いたうえで終了コード 1 で終える。「読み飛ばす行」の節を参照 |

### マスクの指定

よくある秘密情報は、フラグを付けなくてもマスクします。次のフラグでマスクする
ものを変えます。ルールの一覧は「マスク」の節にあります。

| フラグ | 働き |
| --- | --- |
| `--mask` | マスクを足す。カンマ区切りか繰り返しで、`aws-account`・`aws-profile`・`email`・`git-user`・`ip`・`user` |
| `--mask-values` | ラベルから値の一覧への JSON オブジェクト（先頭が `@` ならそのファイル）にある値をマスクする。繰り返し指定できる |
| `--mask-report` | 各ルールが何回マスクしたかを、`text` か `json` で stderr に書く |
| `--no-mask` | マスクをすべて切る。`--mask` や `--mask-values` とは一緒に使えない |

### 出力の指定

`--limit N` は先頭 N 件のレコードを出します。`--around` を付けたときは、
一致したレコードを N 件数え、最後に一致したレコードの後ろの文脈も残します。
`--format` はコマンドごとに違います。「コマンド」の節を参照してください。

`records` と `sessions` は `--fields` も受け付けます。JSON と JSON Lines
で、指定したキーだけを、レコードが書く順に出します（カンマ区切りか
繰り返しで指定）。絞り込みと同じく先頭に `!` を付けたキーは逆に除くので、
`--fields '!agent,!session_title,!project,!cwd,!git_branch'` は同じ
セッションのどのレコードにも繰り返し入るキーを省き、
`sessions --fields '!usage'` はトークン数を省きます。コマンドが書かない
キーを指定するとエラーで、`--fields` と `--format md` の併用もエラーです。

### 環境変数

| 変数 | 既定値を与えるフラグ | 値 |
| --- | --- | --- |
| `CCDIG_ROOT` | `--root` | ディレクトリ。`CLAUDE_CONFIG_DIR` より優先する |
| `CLAUDE_CONFIG_DIR` | `--root` | Claude Code 自身の変数。その下の `projects` ディレクトリをルートにする |
| `CLAUDE_CODE_SESSION_ID` | `--session current` | Claude Code 自身の変数。ccdig を実行しているセッションの ID |
| `CCDIG_ON_SKIP` | `--on-skip` | `warn` か `fail` |
| `CCDIG_MASK` | `--mask` | カンマ区切りの一覧 |
| `CCDIG_MASK_VALUES` | `--mask-values` | JSON オブジェクト 1 つ、または `@` とファイル |

- コマンドラインのフラグは、対応する変数を置き換えます。
- `--no-mask` はマスクの変数を無視します。
- 空の変数は未設定と同じに扱います。

## 出力の形式

### レコードのキー

`ccdig records --type prompt` は 1 行に 1 つのオブジェクトを書きます。

```json
{"type":"prompt","agent":"claude-code","session_id":"<uuid>",
 "session_title":"Weekly report draft",
 "project":"-Users-alice-blog","cwd":"/Users/alice/blog","git_branch":"main",
 "timestamp":"2026-10-13T10:00:01+09:00",
 "text":"/weekly-report last week","kind":"slash","command":"/weekly-report",
 "permission_mode":"","redacted":false,
 "source":{"file":"/Users/alice/.claude/projects/-Users-alice-blog/<uuid>.jsonl",
           "line":3,"uuid":"<entry uuid>"},
 "matched":true,"matches":[]}
```

どのレコードも、`--type` の名前で種類を表す `type` で始まります。ほかの
種類も、`type` から `timestamp` までの同じキーで始まり、`redacted`・
`source`・`matched`・`matches` で終わります。

| 種類 | 間に入るキー |
| --- | --- |
| `reply` | `text`, `model`, `skill`, `subagent`, `usage` |
| `call` | `tool`, `input`, `tool_use_id`, `skill`, `subagent`, `usage` |
| `result` | `tool`, `input`, `tool_use_id`, `is_error`, `text`, `subagent` |
| `summary` | `text` |

`--fields` で除かない限り、どのキーも常に出力されます。依頼文の `kind` は打ち方（`slash` か `text`）で、
レコードの種類は `--type` で選ぶ `type` です。

- `matched` は、`--around` が一致の前後に出したレコードでは `false`、
  それ以外のレコードでは `true` です。
- `matches` は、`--context` を付けたとき、`--grep` か `--igrep` に一致した
  行とその前後の行を順に並べます。`field` はその行がある文字列で、呼び出し
  以外は `text`、呼び出しでは入力の中の場所（`input.command` や
  `input.edits[0].new_string` など）です。英数字・`_`・`-` 以外を含むキーは
  `input["a.b"]` のように引用します。
  `line` はその文字列での 1 始まりの行番号、`text` はマスク後の行、`hit` は
  パターンに一致した行かどうかです。パターンは 1 行ずつ試すので、`^` と
  `$` は各行の先頭と末尾に一致します。また、行をまたぐ一致、呼び出しの
  キー名への一致、`\\` のように呼び出しを詰めた JSON のエスケープが要る
  一致では行が並ばず、レコードを全文で出します。`--context` が無ければ、
  JSON と JSON Lines は一致した行だけを並べ、Markdown は行を並べずに
  レコードを全文で出します。`!` で始まらないパターンが無ければ
  `matches` は空です。
- `permission_mode` は、Claude Code が依頼文の行に記録したモード
  （`default`・`plan`・`auto` など）です。行に記録が無ければ空で、
  スラッシュコマンドや古いセッションでは Claude Code が書きません。
- `subagent` は、セッション本体では空で、サブエージェントでは
  `agent-1a2b` のようなファイル名です。サブエージェントのレコードは、
  そのセッションの `session_id` と `session_title` を持ちます。セッション
  本体の transcript の最終書き込みが `--since` より前なら、タイトルは空です。
- `input` は、Claude Code が行の `wireToolInputs` に保存していればそこに
  ある切り詰められていない入力で、なければ `tool_use` ブロックの入力です。
  呼び出しが見つからなかった結果は、`tool` が空で `input` が `null` です。
- `usage` は応答のトークン数です。「トークン数」の節を参照してください。
- `timestamp` は、`sessions` のレコードの時刻と同じく、`--tz` のゾーンの
  その時点のオフセットを付けた RFC 3339 で、
  `2026-10-13T10:00:01+09:00` のように書きます。日を区切ったゾーンで
  時刻を読めます。UTC は `Z` と書きます。

### トークン数

`usage` は応答のトークン数を持ちます。

```json
{"input":12,"output":340,"cache_read":51000,"cache_creation":800}
```

- 1 回の応答は同じトークン数を持つ複数の行に分けて書かれるため、ファイルの
  中で、その応答のうち絞り込みで残った最初の返答か呼び出しだけが値を
  持ちます。残りと、
  トークン数の無い行は `null` です。
- 記録を足しても全体になるとは限りません。思考だけの応答のように返答も
  呼び出しも無い応答は、トークン数を載せる記録がありません。`--type` や
  絞り込みで記録がすべて外れた応答も同じです。
- `sessions` のレコードは最後に、範囲内のセッションの応答を 1 回ずつ足した
  同じ 4 つのキーを `usage` に持ちます。そのため `sessions` は assistant の
  行も読みます。

### JSON の包み

`--format json` は、レコードをバージョンと実行時のフラグで包みます。

```json
{"schema":3,"filter":"strict-v2","ccdig":"0.0.2",
 "run":{"since":"2026-10-05","until":"2026-10-11","tz":"Asia/Tokyo",
        "mask":["email","user"],"mask_values":["repo","person"],
        "no_mask":false},
 "items":[...]}
```

| キー | 中身 |
| --- | --- |
| `schema` | レコードや包みのキーが増えたり、減ったり、意味が変わったりするたびに上がる番号 |
| `filter` | 「依頼文とみなすもの」の節のルールセットの名前。ルールの変更で残る依頼文が変わりうるときは必ず変わる |
| `ccdig` | 文書を書いたバージョン |
| `run` | 指定されたままの `--since`・`--until`・`--tz`（指定が無ければ `null`）、`--mask` の名前、値を除いた `--mask-values` のラベル、`--no-mask`。レコードの欠けが履歴の欠落なのか、狭い条件で実行したためなのかを見分けられる |
| `items` | レコード |

スクリプトは出力を読む前に、`ccdig version --format json` で `schema` と
`filter` を確かめられます。

### Markdown

`records --format md` は、履歴ファイルごとのレコードをプロジェクトと
セッションを記した 1 つの見出しの下にまとめ、各レコードを時刻と種類の
見出しの下に置きます。サブエージェントのレコードは、自分の見出しの末尾にも
サブエージェント名を付けます（例: ``### 2026-10-13 10:25 JST - Tool call -
`agent-1` ``）。セッション本体のレコードと見分けられます。

`--context` を付けると、レコードは全文の代わりに一致した行を出します。
隣り合う行のかたまりごとに、どの行か、そのうちどれが一致したかを箇条書きで
示し、その後に行をテキストブロックで置きます。行番号はブロックの外に置くので、
ツールが読んだファイルの行番号など、本文の中の数字とは混ざりません。

````markdown
- Lines 41-43, matched 42

```text
...
```
````

Markdown とダイジェストの出力は、履歴のフィールドを信頼できない入力として
扱います。端末の制御文字と双方向テキストの上書き文字は U+FFFD に置き換え、
メタデータは 1 行にまとめ、タイトルはインラインの Markdown と HTML を
エスケープし、ダイジェストの依頼文はコードスパンに入れます。

## 読むもの

### 依頼文とみなすもの

`user` エントリは、次のいずれかに当てはまると、最初に当てはまった理由で
除かれます。

1. `isMeta`・`isSidechain`・`isCompactSummary` のいずれかが付いている
2. `origin.kind` が `human` 以外である
3. `tool_result` ブロックを持つ
4. `<system-reminder>`、`<task-notification>`、`<local-command-…>`、
   `<bash-input>`、`<bash-stdout>`、`<user-prompt-submit-hook>` などの
   ハーネスのタグで始まる
5. 空であるか、`[Request interrupted by user` の印である

ルール 4 は `origin.kind` が `human` でも適用されます。Claude Code は `!` に
よるシェルエスケープを人間の入力として記録するためです。Claude Code が
`origin` を書くようになる前に記録されたセッションでは、ルール 2 を飛ばします。
thinking ブロックは決して出力しません。

### ファイル

- 読むのは通常のファイルだけです。`<root>/<project>/<session>.jsonl` と、
  `--subagents` を付けたときは `<root>/<project>/<session>/subagents/*.jsonl`
  も読みます。
- シンボリックリンクはたどりません。
- 再開したセッションは以前のエントリを繰り返しますが、各エントリ（`uuid` と
  行内の位置で識別）は元のセッションで 1 回だけ出力されます。

### 読み飛ばす行

読めないディレクトリやファイル、デコードできない行、タイムスタンプのない行、
64 MiB を超える行は飛ばして stderr に報告し、残りの出力はそのまま書きます。
終了コードは 0 のままですが、`--on-skip fail`（または `CCDIG_ON_SKIP=fail`）
を付けると、出力を書いたうえで終了コード 1 で終えます。スクリプトは、
不完全な出力で前回の結果を置き換えずに済みます。

## マスク

`--no-mask` を付けない限り常に有効です。セッションのタイトル、ツールの
入力に含まれるすべての文字列、ツールの結果もマスクします。

### 組み込みのルール

次のルールは常に有効で、一致した部分を `***REDACTED***` に置き換えます。

- AWS の認証情報: `AWS_SECRET_ACCESS_KEY` 形式の名前に `=` か `:` が続く
  もの、credentials ファイル、`aws sts` の JSON（`SecretAccessKey`、
  `SessionToken`）、単独の `AKIA` / `ASIA` アクセスキー ID
- GitHub（`ghp_`、`gho_`、`ghu_`、`ghs_`、`ghr_`、`github_pat_`）、GitLab、
  npm のトークン
- Anthropic と OpenAI の `sk-` キー、Google の API キー・OAuth トークン・
  クライアントシークレット、Stripe のキーと Webhook シークレット、SendGrid
  のキー
- Slack のトークンと Webhook URL、JWT
- `Authorization` ヘッダーの認証情報と `Bearer` トークン
- URL 中のパスワード（`postgres://user:…@host`）と、`mysql` などの `-p` に
  続けて書いたパスワード
- PEM と PGP の秘密鍵ブロック
- 認証情報らしい名前に代入された値:
  - 形は `name=value`、`name: value`、`name => value`、`--name=value`、
    `?name=value`、エスケープされた JSON 内の同じ形
    （`\"password\":\"...\"`）、パスワードとトークンを表す日本語です。
    引用符で囲まれた値は閉じ引用符までマスクするため、空白を含む
    パスフレーズも丸ごと隠れます。引用符のない値は印字可能な ASCII に
    限るため、区切りの後に続く文章は残ります
  - 名前は `_`・`.`・`-` と camelCase で分割します。`token`・`secret`・
    `password` のような強い語は、単独でもパートの末尾でも該当し
    （`PGPASSWORD`）、`key` や `auth` のような弱い語は、別のパートと
    並んだときだけ該当します（`api_key` は該当し、`key` は該当しない）
  - パスや URL、データベースのキーを表す名前（`token_path`、
    `primary_key`）、数量を表す名前の後の数字（`token_limit: 100000`）、
    参照（`$VAR`、`ENV[...]`、`<placeholder>`、`%s`、`\u0000`）、コード
    （`self.token`、`config[`、`last_token`）、型名（`token: string`）で
    ある値はそのまま残します
- ツールの入力では、認証情報を表すキーの下の文字列（`{"password": "..."}`
  など）を、同じ基準でマスクします

### 追加のマスク

次のマスクは既定では切ってあります。`--mask` で、どのコマンドにも足せます。
`--mask email,user` のように、カンマ区切りでも繰り返しでも指定できます。

| 値 | マスクするもの |
| --- | --- |
| `aws-account` | 文脈のある 12 桁の AWS アカウント ID。ARN の中、`Account`・`AWS_ACCOUNT_ID`・`OwnerId`・`AWS` のプリンシパルなどの名前への代入、ECR・S3・コンソールのサインインのホスト名、`--account-id` や `--owners` の後。単独の 12 桁は残す |
| `aws-profile` | 文脈のある AWS のプロファイル名。`--profile` の後、`AWS_PROFILE`・`AWS_DEFAULT_PROFILE`・`profile` キーへの代入、`[profile name]`・`[sso-session name]`・認証情報ファイルの見出し、`source_profile`。文中の名前は残す |
| `email` | メールアドレス。`git@github.com:` のような SSH のリモートは除く |
| `git-user` | git config の `user.name` と `user.email`。大文字・小文字を問わない。どちらかが未設定ならコマンドは失敗する |
| `ip` | `127.x.x.x`・`0.0.0.0`・`255.255.255.255` 以外の IPv4 アドレスと、3 グループ以上の IPv6 アドレス |
| `user` | ユーザー名（`$HOME` の最後の部分）。大文字・小文字を問わず、区切りが `.`・`_`・`-` のどれでも。そのため `-Users-alice-b-blog` のようなプロジェクトのディレクトリ名の中でも |

これらのマスクと `--mask-values` は、本文でないフィールド、つまり
`project`・`cwd`・`git_branch`・ファイルのパス・`tool`・`model`・
`subagent`・`command`・ツールの入力のキーと、stderr の警告にもかかります。
ツールの入力のキーには組み込みのルールもかけるので、トークンのような秘密が
キーになっていてもマスクされます。

- マスクした結果、前のキーと同じになったキー（伏せたファイルのパス 2 つ
  など）には `#2`・`#3` と番号を付け、どちらの値も残します。
- IPv4 のルールは `1.2.3.4` のような単独のバージョン番号とアドレスを区別
  できないため、それもマスクします。`v1.2.3.4` のように英字やドットの後に
  続くものは残ります。
- 下の名前だけのような、ユーザー名の一部だけの形は残ります。伏せたいときは
  `--mask-values` に書いてください。

### ラベル付きの値

`--mask-values` は、ラベルから値の一覧への JSON オブジェクトにある値を、
どこに現れても、書いたとおりに（大文字と小文字を区別して）マスクします。

```json
{"repo": ["acme-billing", "acme-portal"],
 "person": [["Taro Yamada", "yamada"], "Hanako Sato"],
 "": ["secret-word"]}
```

- ラベルの下の値は、ラベルと番号を全角の角かっこで囲んだものに置き換わり、
  番号はラベルごとに渡した順に振ります。`acme-billing` は `［repo1］`、
  `acme-portal` は `［repo2］` になるので、名前を伏せたまま 2 つの
  リポジトリのレコードを区別できます。
- 値の代わりに一覧を書くと、その中の表記はどれも同じ番号になり、
  `Taro Yamada` と `yamada` はどちらも `［person1］` です。
- `""` の下の値は `***REDACTED***` になります。
- 先頭が `@` なら、`--mask-values @secrets.json` のようにオブジェクトを
  ファイルから読みます。
- 繰り返し指定でき、番号はオブジェクトをまたいで続き、2 回渡した値は最初の
  ラベルのままです。
- 形の崩れたオブジェクトを渡すとコマンドは止まります。

### 件数の報告

`--mask-report text` は、出力の後で stderr に、出力したレコードの中で各
ルールが何回マスクしたかを多い順に書きます。

```text
ccdig: masked 3 by github-token, 2 by values/repo, 1 by email
```

`--mask-report json` は同じ内容を 1 つのオブジェクトで書きます。

```json
{"masked":{"github-token":3,"values/repo":2,"email":1}}
```

- 絞り込みや `--limit` で出力しなかったレコードは数えず、値そのものは決して
  書きません。
- 数えるのは `--format jsonl` が出す各レコードの全フィールドです。
  フィールドを省いたり本文を切ったりする Markdown と `digest` では、見える
  マスクが件数より少ないことがあります。

| ルールの名前 | 数えるもの |
| --- | --- |
| `aws-key-id`・`credential-assignment` など | 組み込みのルール。見つけるものの名前が付いている |
| `credential-key` | ツールの入力で、キーを理由に丸ごと伏せた文字列 |
| `email`・`ip` など `--mask` の値 | そのマスク |
| `values` | `--mask-values` の `""` の下の値 |
| `values/` とラベル | `--mask-values` のそのラベルの下の値 |

### 制約

- `--no-mask` は、秘密情報も含めて書かれたとおりに出します。自分の端末で
  マスクの結果を確かめるためのものです。
- パターンは特徴的なプレフィックスか認証情報らしい名前を手がかりにするため、
  それを持たない秘密情報は見逃します。たとえば空白で区切った
  `--password value` や、`snake_case` の識別子のように見える値です。
- ツールの結果には、ツールが読んだもの、つまりファイルの中身・環境変数・
  コマンドの出力がそのまま入ります。
- `records` が既定で出すツールの入力には、`Write` の呼び出しの中身のように、
  書いたものが入ります。

マスクは安全網として扱い、出力を共有する前に自分で中身を確かめてください。

## 対象範囲

- Claude Code のみ。

## 困ったときは

<https://github.com/tsukuboshi/ccdig/issues> に Issue を作成してください。
実際の会話履歴は決して添付せず、合成した行で問題を再現してください。
脆弱性の報告は [SECURITY.md](SECURITY.md) を参照してください。

## 開発への参加

[CONTRIBUTING.md](CONTRIBUTING.md) を参照してください。

## メンテナー

[@tsukuboshi](https://github.com/tsukuboshi)

## ライセンス

[MIT](LICENSE)
