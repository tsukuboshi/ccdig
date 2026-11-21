# ccdig

[![CI](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml/badge.svg)](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml)

Claude Code の会話履歴から、共有したいログだけを、整形・マスクして
取り出す CLI です。
期間・プロジェクト・セッション・ブランチ・語句・レコードの種類・ツール・
成否・スキルを組み合わせて必要な部分に絞り込み、読みやすい Markdown か、
加工しやすい JSON Lines で出力します。
よくある秘密情報は既定でマスクし、メールアドレス・IP アドレス・ユーザー名・
指定した値も追加でマスクできます。

[English](README.md)

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
ccdig records --type prompt --since 2026-10-05 --until 2026-10-11

# blog プロジェクトで "deploy" に触れた返答を、大文字小文字を区別せずに出す
ccdig records --type reply --project blog --igrep deploy

# 失敗した Bash の呼び出しを、サブエージェントも含めて Markdown で出す
ccdig records --type result --tool Bash --status error --subagents --format md

# LLM に貼るための、サイズ上限付きの依頼文の Markdown ダイジェスト
ccdig digest --since 2026-10-05 --until 2026-10-11

# 人に渡すために 1 セッションを会話の流れで出し、メールアドレス・IP
# アドレス・ユーザー名・ファイルに書いた値もマスクする
ccdig records --session 1a2b --format md --mask email,ip,user \
  --mask-values @secrets.json
```

### コマンド

| コマンド | 出力 |
| --- | --- |
| `records` | `--type` で選んだ種類の依頼文・返答・ツールの呼び出し・ツールの結果 1 件につき 1 レコード。書かれた順に並ぶ |
| `sessions` | 選ばれた依頼文を含むセッション 1 つにつき 1 レコード |
| `digest` | 依頼文の Markdown ダイジェスト |
| `version` | インストールされているバージョン。`--format json` では `{"version":"0.1.0","schema":2,"filter":"strict-v2"}` |

`--type` は `prompt`（人間が打った依頼文）・`reply`（assistant の
テキストブロック）・`call`（ツールの呼び出し）・`result`（ツールの結果。
呼び出しのツールと入力付き）・`summary`（コンテキストを圧縮したときに
Claude Code が書く要約）を取り、カンマ区切りでも繰り返しでも指定
できます。既定は `prompt,reply,call` です。`records` と `sessions` は
`--format` を取り、値は `jsonl`（既定）・`json`・`md` のいずれかです。
`digest` は `--max-prompt-chars`（既定 300）と `--max-session-chars`
（既定 2000）を取ります。`--format md` の `records` は `--max-input-chars N`
を取り、ツールの呼び出しの入力にある文字列をそれぞれ N 文字で切って
`… (M more characters)` を付けます。ファイル全体を書く `Write` でページが
埋まらずに済みます。JSON と JSON Lines は切りません。

### 絞り込み

| フラグ | コマンド | 残すもの |
| --- | --- | --- |
| `--since`, `--until` | すべて | 開始日から終了日までのレコード。`YYYY-MM-DD` |
| `--tz` | すべて | 日の境界と Markdown の時刻をこの IANA タイムゾーンで扱う |
| `--project` | すべて | ディレクトリ名か cwd にこの値を含むプロジェクト |
| `--session` | すべて | ID がこの値で始まるセッション |
| `--branch` | すべて | この git ブランチ上のレコード |
| `--grep` | すべて | テキストが正規表現に一致するレコード |
| `--igrep` | すべて | 同じく、大文字小文字を区別せずに一致するレコード |
| `--limit` | すべて | 先頭 N 件のレコード |
| `--tool` | records: `call`, `result` | このツールの呼び出しとその結果 |
| `--status` | records: `result` | この状態の結果。`error` か `ok` |
| `--skill` | records: `reply`, `call`, `result` | そのスキルの実行中に書かれたレコード |
| `--subagents` | records: `reply`, `call`, `result` | サブエージェントの履歴も含める |

`--project`・`--session`・`--branch`・`--grep`・`--igrep`・`--tool`・
`--skill` は繰り返し指定でき、いずれかの値に一致するレコードを残します。
先頭に `!` を付けた値は、逆に一致するレコードを除きます。たとえば
`--session '!1a2b'` は 1 つを除くすべてのセッションを、`--grep '!WIP'` は
`WIP` に触れたレコードを除いた残りを出します。`!` で始まる値そのものに
一致させるときは `\!` と書きます。たいていのシェルでは `!` が履歴展開に
なるので、引用符で囲んでください。`--status` は `--type` と同じく
カンマ区切りで並べられます。

`--tool`・`--status`・`--skill` は、調べる値を持つ種類だけを絞り、持たない
種類はそのまま残します。そのため
`ccdig records --type prompt,reply,call --skill tdd` は、すべての依頼文と、
`tdd` の実行中に書かれた返答と呼び出しを残します。`--type` を省略すると、
`records` は付けたフラグがすべて効く種類だけを出します。
`ccdig records --tool Bash` なら、`Bash` の呼び出しとその結果です。表の
最後の 4 つのフラグは、効く種類が `--type` に無いとエラーになります。

共通のフラグはコマンドの前にも後にも置け、繰り返し指定できるフラグの値は
前後で足し合わせます。`--tz` の既定はシステムのタイムゾーン（`TZ`、なければ `/etc/localtime`）
です。`--root` で履歴のルートを変えます。既定は、Claude Code の
`CLAUDE_CONFIG_DIR` が設定されていれば `$CLAUDE_CONFIG_DIR/projects`、
なければ `~/.claude/projects` です。
`--mask` と `--mask-values` でマスクを足し、`--no-mask` ですべてのマスクを
切ります（後述の「マスク」の節を参照）。

`CCDIG_ROOT`・`CCDIG_ON_SKIP`・`CCDIG_MASK`・`CCDIG_MASK_VALUES` で、
`--root`・`--on-skip`・`--mask`（カンマ区切り）・`--mask-values`
（値は 1 つ）の既定値を与えられます。`CCDIG_ROOT` は `CLAUDE_CONFIG_DIR` より優先します。
コマンドラインのフラグは対応する変数を置き換え、
`--no-mask` はマスクの変数を無視し、空の変数は未設定と同じに扱います。

`--grep` は、出力されるときのテキスト、つまりマスク後のテキストを検索します。
そのため、秘密情報をその値で探し当てることはできません。対象は、依頼文や
返答のテキスト、呼び出しの入力を詰めた JSON、結果のテキストです。
`sessions` と `digest` では、絞り込みは依頼文に適用されます。

### 出力

`ccdig records --type prompt` は 1 行に 1 つのオブジェクトを書きます。

```json
{"type":"prompt","agent":"claude-code","session_id":"<uuid>",
 "session_title":"Weekly report draft",
 "project":"-Users-alice-blog","cwd":"/Users/alice/blog","git_branch":"main",
 "timestamp":"2026-10-13T01:00:01Z",
 "text":"/weekly-report last week","kind":"slash","command":"/weekly-report",
 "permission_mode":"","redacted":false,
 "source":{"file":"/Users/alice/.claude/projects/-Users-alice-blog/<uuid>.jsonl",
           "line":3,"uuid":"<entry uuid>"}}
```

どのレコードも、`--type` の名前で種類を表す `type` で始まります。ほかの
種類も、`type` から `timestamp` までの同じキーで始まり、`redacted` と
`source` で終わります。

| 種類 | 間に入るキー |
| --- | --- |
| `reply` | `text`, `model`, `skill`, `subagent`, `usage` |
| `call` | `tool`, `input`, `tool_use_id`, `skill`, `subagent`, `usage` |
| `result` | `tool`, `input`, `tool_use_id`, `is_error`, `text`, `subagent` |
| `summary` | `text` |

`permission_mode` は、Claude Code が依頼文の行に記録したモード
（`default`・`plan`・`auto` など）です。行に記録が無ければ空で、
スラッシュコマンドや古いセッションでは Claude Code が書きません。

`usage` は応答のトークン数で、
`{"input":12,"output":340,"cache_read":51000,"cache_creation":800}` の形です。
1 回の応答は同じトークン数を持つ複数の行に分けて書かれるため、ファイルの中で
その応答の最初の返答か呼び出しだけが値を持ち、残りと、トークン数の無い行は
`null` です。`--type` や絞り込みでその記録が外れると、記録を足しても全体には
なりません。思考だけの応答のように返答も呼び出しも無い応答は、トークン数を
載せる記録がありません。`sessions` のレコードは最後に、範囲内のセッションの
応答を 1 回ずつ足した同じ 4 つのキーを `usage` に持ちます。そのため
`sessions` は assistant の行も読みます。

どのキーも常に出力されます。`subagent` は、セッション本体では空で、
サブエージェントでは `agent-1a2b` のようなファイル名です。サブエージェントの
レコードは、そのセッションの `session_id` と `session_title` を持ちます。
セッション本体の transcript の最終書き込みが `--since` より前なら、タイトルは
空です。`input` は、
Claude Code が行の `wireToolInputs` に保存していればそこにある切り詰められて
いない入力で、なければ `tool_use` ブロックの入力です。呼び出しが見つからな
かった結果は、`tool` が空で `input` が `null` です。
`--format json` は、レコードをバージョンと実行時のフラグで包みます。

```json
{"schema":2,"filter":"strict-v2","ccdig":"0.1.0",
 "run":{"since":"2026-10-05","until":"2026-10-11","tz":"Asia/Tokyo",
        "mask":["email","user"],"mask_values":["repo","person"],
        "no_mask":false},
 "items":[...]}
```

`ccdig` は文書を書いたバージョンです。`run` には、指定されたままの `--since`・
`--until`・`--tz`（指定が無ければ `null`）、`--mask` の名前、値を除いた
`--mask-values` のラベル、`--no-mask` が入ります。そのため、レコードの欠けが
履歴の欠落なのか、狭い条件で実行したためなのかを見分けられます。
`filter` は次の節のルールセットの名前で、ルールの変更で残る依頼文が変わり
うるときは必ず変わります。`schema` は、レコードや包みのキーが増えたり、
減ったり、意味が変わったりするたびに上がります。スクリプトは出力を読む前に、
`ccdig version --format json` で両方を確かめられます。

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
stderr に報告し、残りの出力はそのまま書きます。終了コードは 0 のままですが、
`--on-skip fail`（または `CCDIG_ON_SKIP=fail`）を付けると、出力を書いたうえで
終了コード 1 で終えます。スクリプトは、不完全な出力で前回の結果を置き換えずに済みます。

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
| `--mask aws-account` | 文脈のある 12 桁の AWS アカウント ID。ARN の中、`Account`・`AWS_ACCOUNT_ID`・`OwnerId`・`AWS` のプリンシパルなどの名前への代入、ECR・S3・コンソールのサインインのホスト名、`--account-id` や `--owners` の後。単独の 12 桁は残す |
| `--mask aws-profile` | 文脈のある AWS のプロファイル名。`--profile` の後、`AWS_PROFILE`・`AWS_DEFAULT_PROFILE`・`profile` キーへの代入、`[profile name]`・`[sso-session name]`・認証情報ファイルの見出し、`source_profile`。文中の名前は残す |
| `--mask email` | メールアドレス。`git@github.com:` のような SSH のリモートは除く |
| `--mask git-user` | git config の `user.name` と `user.email`。大文字・小文字を問わない。どちらかが未設定ならコマンドは失敗する |
| `--mask ip` | `127.x.x.x`・`0.0.0.0`・`255.255.255.255` 以外の IPv4 アドレスと、3 グループ以上の IPv6 アドレス |
| `--mask user` | ユーザー名（`$HOME` の最後の部分）。大文字・小文字を問わず、区切りが `.`・`_`・`-` のどれでも。そのため `-Users-alice-b-blog` のようなプロジェクトのディレクトリ名の中でも |
| `--mask-values JSON` | ラベルから値の一覧への JSON オブジェクト（先頭が `@` ならそのファイル）にある値。どこに現れても、書いたとおりに（大文字と小文字を区別） |

`--mask-values` には次のようなオブジェクトを渡します。

```json
{"repo": ["acme-billing", "acme-portal"],
 "person": [["Taro Yamada", "yamada"], "Hanako Sato"],
 "": ["secret-word"]}
```

ラベルの下の値は、ラベルと番号を全角の角かっこで囲んだものに置き換わり、
番号はラベルごとに渡した順に振ります。`acme-billing` は `［repo1］`、
`acme-portal` は `［repo2］` になるので、名前を伏せたまま 2 つのリポジトリの
レコードを区別できます。値の代わりに一覧を書くと、その中の表記はどれも同じ
番号になり、`Taro Yamada` と `yamada` はどちらも `［person1］` です。`""` の
下の値は `***REDACTED***` になります。形の崩れたオブジェクトを渡すと
コマンドは止まります。

`--mask` はカンマ区切りで並べられ、`--mask email,user` のように繰り返しも
指定できます。`--mask-values` も繰り返し指定でき、番号はオブジェクトを
またいで続き、2 回渡した値は最初のラベルのままです。これらのマスクは、本文
でないフィールド、つまり `project`・`cwd`・`git_branch`・ファイルのパス・
`tool`・`model`・`subagent`・`command`・ツールの入力のキーと、stderr の
警告にもかかります。マスクした結果、前のキーと同じになったキー（伏せた
ファイルのパス 2 つなど）には `#2`・`#3` と番号を付け、どちらの値も残します。
組み込みのルールはキーにはかけません。IPv4 のルールは `1.2.3.4` のような
単独のバージョン番号とアドレスを区別できないため、それもマスクします。
`v1.2.3.4` のように英字やドットの後に続くものは残ります。名字だけのような、ユーザー名の一部だけの
形は残ります。伏せたいときは `--mask-values` に書いてください。

`--mask-report text` は、出力の後で stderr に、出力したレコードの中で各
ルールが何回マスクしたかを多い順に書きます。
`ccdig: masked 3 by github-token, 2 by values/repo, 1 by email` のような 1 行
です。`--mask-report json` は同じ内容を `{"masked":{"github-token":3,...}}`
で書きます。絞り込みや `--limit` で出力しなかったレコードは数えず、値そのものは
決して書きません。数えるのは `--format jsonl` が出す各レコードの全フィールドで、
フィールドを省いたり本文を切ったりする Markdown と `digest` では、見えるマスクが
件数より少ないことがあります。組み込みのルールは `aws-key-id` や
`credential-assignment` のように見つけるものの名前で、ツールの入力でキーを理由に丸ごと伏せた文字列は
`credential-key` です。ほかのマスクは `--mask` の名前のとおりで、
`--mask-values` の値は `values`、ラベル付きなら `values/` とラベルです。

`--no-mask` は、秘密情報も含めて書かれたとおりに出します。自分の端末で
マスクの結果を確かめるためのものです。`--mask` や `--mask-values` とは一緒に
使えません。

パターンは特徴的なプレフィックスか認証情報らしい名前を手がかりにするため、
それを持たない秘密情報は見逃します。たとえば空白で区切った
`--password value` や、`snake_case` の識別子のように見える値です。
ツールの結果には、ツールが読んだもの、つまりファイルの中身・環境変数・
コマンドの出力がそのまま入ります。`records` が既定で出すツールの入力には、
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
