# ccdig

[![CI](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml/badge.svg)](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml)

Claude Code の会話履歴から、依頼文・返答・ツールの呼び出し・ツールの結果を
掘り出す CLI です。

[English](README.md)

## 何をするか

Claude Code はすべてのセッションを `~/.claude/projects/` 配下に JSONL で
保存します。そこには、人間が打った依頼文と、ハーネスが書いた行が混ざって
います。
ツールの呼び出しとその結果、注入されたスキル本文、`<system-reminder>`
ブロック、タスク通知、コンパクションの要約、サブエージェントのやり取りです。

ccdig はこの履歴を読み、1 種類ずつ取り出して出力します。

- **依頼文**: 人間が打ったもの。ハーネスのノイズを除き、スラッシュコマンドは
  `/command args` の形に戻す
- **返答**: assistant のテキスト
- **ツールの呼び出し**: 入力の JSON 付き
- **ツールの結果**: それぞれが答える呼び出しと組にして出す

どの種類も、期間・プロジェクト・正規表現・セッション・ブランチの同じ
絞り込みが使え、よくある秘密情報は既定でマスクされます。

## 何が便利か

セッションを丸ごと読むのは、ビューアや変換ツールが得意です。
ccdig は全セッションから一部を抜き出すための道具です。今週頼んだこと、
失敗した `Bash` の呼び出しすべて、あるキーワードに触れた返答などを
取り出せます。どの user 行を人間が打ったかを判定し、既定で秘密情報を
マスクし、`jq` につなげる JSON Lines を出力し、単一のバイナリで配布されます。

## 始め方

Rust 1.99.1 以降が必要です。

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

```sh
# 1 週間分の依頼文を、1 行に 1 つの JSON オブジェクトで出す
ccdig prompts --since 2026-10-05 --until 2026-10-11

# blog プロジェクトで "deploy" に触れた返答を、大文字小文字を区別せずに出す
ccdig replies --project blog --grep deploy -i

# 失敗した Bash の呼び出しを、サブエージェントも含めて Markdown で出す
ccdig results --tool Bash --errors --subagents --format md

# LLM に貼るための、サイズ上限付きの依頼文の Markdown ダイジェスト
ccdig digest --since 2026-10-05 --until 2026-10-11

# 人に渡すために 1 セッションを会話の流れで出し、メールアドレス・IP
# アドレス・ユーザー名・ファイルに書いた値もマスクする
ccdig conversation --session 1a2b --format md --mask email,ip,user --mask-file secrets.txt
```

### コマンド

| コマンド | 出力 |
| --- | --- |
| `prompts` | 人間が打った依頼文 1 件につき 1 レコード |
| `replies` | assistant のテキストブロック 1 つにつき 1 レコード |
| `tools` | ツールの呼び出し 1 件につき 1 レコード |
| `results` | ツールの結果 1 件につき 1 レコード。呼び出しのツールと入力付き |
| `conversation` | 依頼文・返答・ツールの呼び出しを、書かれた順に並べたもの |
| `sessions` | 選ばれた依頼文を含むセッション 1 つにつき 1 レコード |
| `digest` | 依頼文の Markdown ダイジェスト |
| `version` | インストールされているバージョン |

`prompts`・`replies`・`tools`・`results`・`conversation`・`sessions` は
`--format` を取り、値は `jsonl`（既定）・`json`・`md` のいずれかです。`digest` は
`--max-prompt-chars`（既定 300）と `--max-session-chars`（既定 2000）を
取ります。

### 絞り込み

| フラグ | コマンド | 残すもの |
| --- | --- | --- |
| `--since`, `--until` | すべて | 開始日から終了日までのレコード。`YYYY-MM-DD` |
| `--tz` | すべて | 日の境界と Markdown の時刻をこの IANA タイムゾーンで扱う |
| `--project` | すべて | ディレクトリ名か cwd にこの値を含むプロジェクト |
| `--grep`, `-i` | すべて | テキストが正規表現に一致するレコード |
| `--session` | すべて | ID がこの値で始まるセッション |
| `--branch` | すべて | この git ブランチ上のレコード |
| `--limit` | すべて | 先頭 N 件のレコード |
| `--skill` | replies, tools, results, conversation | そのスキルの実行中に書かれたレコード |
| `--subagents` | replies, tools, results, conversation | サブエージェントの履歴も含める |
| `--tool` | tools, results | このツールの呼び出し |
| `--errors` | results | ツールが失敗と報告した結果 |

`--project`・`--session`・`--branch`・`--tool` は繰り返し指定でき、いずれかの
値に一致するレコードを残します。共通のフラグはコマンドの前にも後にも
置けます。`--tz` の既定はシステムのタイムゾーン（`TZ`、なければ
`/etc/localtime`）です。`--root` で履歴のルート（既定は
`~/.claude/projects`）を変えます。`--mask` と `--mask-file` でマスクを足し、
`--no-mask` ですべてのマスクを切ります（後述の「マスク」の節を参照）。

`--grep` は、出力されるときのテキスト、つまりマスク後のテキストを検索します。
そのため、秘密情報をその値で探し当てることはできません。対象は、依頼文や
返答のテキスト、呼び出しの入力を詰めた JSON、結果のテキストです。
`sessions` と `digest` では、絞り込みは依頼文に適用されます。

### 出力

`ccdig prompts` は 1 行に 1 つのオブジェクトを書きます。

```json
{"agent":"claude-code","session_id":"<uuid>","session_title":"Weekly report draft",
 "project":"-Users-alice-blog","cwd":"/Users/alice/blog","git_branch":"main",
 "timestamp":"2026-10-13T01:00:01Z",
 "text":"/weekly-report last week","kind":"slash","command":"/weekly-report",
 "redacted":false,
 "source":{"file":"/Users/alice/.claude/projects/-Users-alice-blog/<uuid>.jsonl",
           "line":3,"uuid":"<entry uuid>"}}
```

ほかの種類も、`agent` から `timestamp` までの同じキーで始まり、`redacted` と
`source` で終わります。

| コマンド | 間に入るキー |
| --- | --- |
| `replies` | `text`, `model`, `skill`, `subagent` |
| `tools` | `tool`, `input`, `tool_use_id`, `skill`, `subagent` |
| `results` | `tool`, `input`, `tool_use_id`, `is_error`, `text`, `subagent` |

どのキーも常に出力されます。`subagent` は、セッション本体では空で、
サブエージェントでは `agent-1a2b` のようなファイル名です。サブエージェントの
レコードは、そのセッションの `session_id` と `session_title` を持ちます。
セッション本体の transcript の最終書き込みが `--since` より前なら、タイトルは
空です。`input` は、
Claude Code が行の `wireToolInputs` に保存していればそこにある切り詰められて
いない入力で、なければ `tool_use` ブロックの入力です。呼び出しが見つからな
かった結果は、`tool` が空で `input` が `null` です。`conversation` の
レコードは、依頼文・返答・ツールの呼び出しのレコードの先頭に、`prompt`・
`reply`・`tool` のいずれかを値とする `type` キーを付けたものです。
`--format json` はレコードを `{"schema":1,"filter":"strict-v2","items":[...]}` で包みます。
`filter` は次の節のルールセットの名前で、ルールの変更で残る依頼文が変わり
うるときは必ず変わります。

## 依頼文とみなすもの

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

読むのは通常のファイルだけです。`<root>/<project>/<session>.jsonl` と、
`--subagents` を付けたときは `<root>/<project>/<session>/subagents/*.jsonl`
も読みます。シンボリックリンクはたどりません。再開したセッションは以前の
エントリを繰り返しますが、各エントリ（`uuid` と行内の位置で識別）は元の
セッションで 1 回だけ出力されます。読めないディレクトリやファイル、
デコードできない行、タイムスタンプのない行、64 MiB を超える行は飛ばして
stderr に報告し、残りの出力はそのまま書きます。

## マスク

`--no-mask` を付けない限り常に有効です。一致した部分は
`***REDACTED***` に置き換えます。

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
- 認証情報らしい名前に代入された値: `name=value`、`name: value`、
  `name => value`、`--name=value`、`?name=value`、エスケープされた JSON 内の
  同じ形（`\"password\":\"...\"`）、パスワードとトークンを表す日本語。
  名前は `_`・`.`・`-` と camelCase で分割します。`token`・`secret`・
  `password` のような強い語は、単独でもパートの末尾でも該当し
  （`PGPASSWORD`）、`key` や `auth` のような弱い語は、別のパートと並んだ
  ときだけ該当します（`api_key` は該当し、`key` は該当しない）。引用符で
  囲まれた値は閉じ引用符までマスクするため、空白を含むパスフレーズも丸ごと
  隠れます。パスや URL、データベースのキーを表す名前（`token_path`、
  `primary_key`）、数量を表す名前の後の数字（`token_limit: 100000`）、
  参照（`$VAR`、`ENV[...]`、`<placeholder>`、`%s`、`\u0000`）、コード
  （`self.token`、`config[`、`last_token`）、型名（`token: string`）である
  値はそのまま残します。引用符のない値は印字可能な ASCII に限るため、
  区切りの後に続く文章は残ります
- ツールの入力では、認証情報を表すキーの下の文字列（`{"password": "..."}`
  など）を、同じ基準でマスクします

セッションのタイトル、ツールの入力に含まれるすべての文字列、ツールの結果も
マスクします。

次のマスクは既定では切ってあり、どのコマンドにも足せます。

| フラグ | マスクするもの |
| --- | --- |
| `--mask email` | メールアドレス。`git@github.com:` のような SSH のリモートは除く |
| `--mask ip` | `127.x.x.x`・`0.0.0.0`・`255.255.255.255` 以外の IPv4 アドレスと、3 グループ以上の IPv6 アドレス |
| `--mask user` | ユーザー名（`$HOME` の最後の部分）。大文字・小文字を問わず、区切りが `.`・`_`・`-` のどれでも。そのため `-Users-alice-b-blog` のようなプロジェクトのディレクトリ名の中でも |
| `--mask-file FILE` | ファイルの空でない各行を前後の空白を除いて。どこに現れても、書いたとおりに（大文字と小文字を区別） |

`--mask` はカンマ区切りで並べられ、`--mask email,user` のように繰り返しも
指定できます。`--mask-file` も繰り返し指定できます。これらのマスクは、本文
でないフィールド、つまり `project`・`cwd`・`git_branch`・ファイルのパス・
`tool`・`model`・`subagent`・`command` と、stderr の警告にもかかります。
ツールの入力のキーはそのまま残ります。IPv4 のルールは `1.2.3.4` のような単独のバージョン番号と
アドレスを区別できないため、それもマスクします。`v1.2.3.4` のように英字や
ドットの後に続くものは残ります。名字だけのような、ユーザー名の一部だけの
形は残ります。伏せたいときはマスク用のファイルに書いてください。

`--no-mask` は、秘密情報も含めて書かれたとおりに出します。自分の端末で
マスクの結果を確かめるためのものです。`--mask` や `--mask-file` とは一緒に
使えません。

パターンは特徴的なプレフィックスか認証情報らしい名前を手がかりにするため、
それを持たない秘密情報は見逃します。たとえば空白で区切った
`--password value` や、`snake_case` の識別子のように見える値です。
ツールの結果には、ツールが読んだもの、つまりファイルの中身・環境変数・
コマンドの出力がそのまま入ります。`conversation` が出すツールの入力には、
`Write` の呼び出しの中身のように、書いたものが入ります。マスクは安全網として扱い、出力を共有する
前に自分で中身を確かめてください。

Markdown とダイジェストの出力は、履歴のフィールドを信頼できない入力として
扱います。端末の制御文字と双方向テキストの上書き文字は U+FFFD に置き換え、
メタデータは 1 行にまとめ、タイトルはインラインの Markdown と HTML を
エスケープし、ダイジェストの依頼文はコードスパンに入れます。

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
