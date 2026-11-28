# ccdig

[![CI](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml/badge.svg)](https://github.com/tsukuboshi/ccdig/actions/workflows/ci.yml)

Claude Code の会話履歴から、必要なログだけを整形・マスクして取り出す
CLI です。
人や LLM に渡す、過去のやり取りを探す、集計して使い方を振り返る、
といった用途に使えます。
期間・プロジェクト・セッション・ブランチ・語句・レコードの種類・ツール・
成否・スキルを組み合わせて必要な部分に絞り込み、`stats` で件数や
トークン数を数えられます。`records`・`sessions`・`stats` は、
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

## ユースケース

### ログを人に共有する

1 セッションを会話の流れのまま Markdown にし、秘密情報を既定でマスクして渡せます。
顧客名やリポジトリ名もファイルに書いておけば `［repo1］` のような表記に置き換わり、
`--mask-report` で何をいくつマスクしたかを渡す前に確かめられるので、社外や別チームに見せるログもそのまま作れます。
PR のレビュアーや週報には、ブランチや期間で絞った依頼文のダイジェストを作業の経緯として添えられ、別のタイムゾーンの相手には `--tz` で時刻を現地の表記にそろえて渡せます。

```sh
# 1 セッションを、メールアドレス・IP アドレス・ユーザー名・ファイルに書いた値もマスクして出す
ccdig records --session 1a2b --format md --mask email,ip,user \
  --mask-values @secrets.json --mask-report text

# ブランチ feature/login での依頼文を、PR に添えるダイジェストにする
ccdig digest --branch feature/login

# 1 週間分の依頼文を、1 件 300 字・1 セッション 2000 字までの週報の材料にまとめる
ccdig digest --since 2026-10-05 --until 2026-10-11 \
  --max-prompt-chars 300 --max-session-chars 2000

# 米国西海岸のメンバーに渡すログを、現地時刻で出す
ccdig records --session 1a2b --format md --tz America/Los_Angeles
```

### 過去のやり取りを Claude Code 自身に探させる

Claude Code の中から ccdig を呼ぶと、ほかのセッションで決めたことや試したことを探せます。
いま動いているセッションは `current` で除け、どのセッションでも同じキーを省いて本文を切り詰めれば、結果を読むトークンも抑えられます。
まず `sessions` でよく一致したセッションに当たりを付け、`records --around` で一致の前後を読むと、
そのときの依頼と返答を流れごと追えます。

```sh
# ほかのセッションの依頼文・返答・ツールの呼び出しを 20 件まで検索する
ccdig records --session '!current' --type prompt,reply,call --igrep deploy \
  --format md --limit 20

# 同じキーを省き、本文とツールの入出力を切り詰めて検索する
ccdig records --session '!current' --type prompt,reply,call --igrep deploy \
  --message-chars 500 --tool-chars 200 \
  --fields '!agent,!session_title,!project,!cwd,!git_branch'

# "deploy" に最も多く触れたセッションを、最初に一致した行付きで出す
ccdig sessions --igrep deploy --sort matches --fields '!usage' --limit 10

# 当たりを付けたセッションで、一致したレコードと前後 2 件を読む
ccdig records --session b799 --igrep deploy --around 2 --format md

# 長い返答は、一致した行と前後 1 行だけを読む
ccdig records --type reply --igrep deploy --context 1 --format md
```

### どのセッションがファイルを変えたかを辿る

ファイルの編集や書き込みをパスで検索すると、そのファイルを変えたセッションと時刻が分かります。
いつの、どの依頼による変更だったかを後から確かめられます。

```sh
# src/main.rs を変えた呼び出しを、セッションと時刻とツールだけで出す
ccdig records --touched src/main.rs --fields session_id,timestamp,tool
```

### うまくいかなかった呼び出しを見直す

権限ルールで止まった呼び出しや、自分で断った呼び出しを、ツールごとに数えられます。
`settings.json` の allow / deny を調整するときの材料になります。
失敗した呼び出しもサブエージェントの分まで拾えるので繰り返し失敗しているコマンドが見つかり、
スキルで絞ればそのスキルの手順のどこで失敗しているかも分かります。

```sh
# 権限ルールで止まった呼び出しを、ツールごとに数える
ccdig stats --denial permission-rule --by tool --since 2026-10-05 --until 2026-10-11

# 自分で断った呼び出し
ccdig records --denial user-rejected --format md

# 失敗した Bash の呼び出しを、サブエージェントも含めて出す
ccdig records --type result --tool Bash --status error --subagents --format md

# スキル weekly-report の実行中に失敗した呼び出し
ccdig records --type result --skill weekly-report --status error --format md
```

### 頼み方を振り返る

プランの差し戻しや出し直しが多いセッションは、最初の依頼で条件が足りていなかった印です。
差し戻したときに打ったコメントは `kind` が `rejection` の依頼文として残るので、
後から足した条件をそのまま読めます。

```sh
# プランの承認を 2 回以上求めたセッション
ccdig stats --by session --type call --tool ExitPlanMode --min-count 2

# プランやツールの実行を断ったときに打ったコメント
ccdig records --kind rejection --fields session_title,text
```

### 利用量を集計する

プロジェクト・月・スキルごとに、件数・セッション数・トークン数を集計できます。
案件ごとの AI の利用量を把握したり、よく使うスキルを見つけたりするのに使えます。

```sh
# プロジェクトごと・月ごとの件数とトークン数
ccdig stats --by project,month --since 2026-10-01 --until 2026-10-31

# 1 つの案件（blog プロジェクト）の利用量を月ごとに追う
ccdig stats --project blog --by month

# スキルごとの出力トークン数を、多い順に出す
ccdig stats --by skill --type reply,call --since 2026-10-01 --sort output
```

## コマンド

| コマンド | 出すもの | 専用のオプション |
| --- | --- | --- |
| `records` | `--type` で選んだ種類の依頼文・返答・ツールの呼び出し・ツールの結果・要約・思考ブロック 1 件につき 1 レコード。書かれた順に並ぶ | `--type`, `--tool`, `--touched`, `--status`, `--denial`, `--skill`, `--kind`, `--superseded`, `--subagents`, `--around`, `--context`, `--format`, `--fields`, `--message-chars`, `--tool-chars` |
| `sessions` | 選ばれた依頼文を含むセッション 1 つにつき 1 レコード | `--type`, `--subagents`, `--format`, `--fields`, `--sort` |
| `stats` | `records` が出すレコードを `--by` のキーでまとめたグループ 1 つにつき 1 レコード | `--by`, `--min-count`, `--type`, `--tool`, `--touched`, `--status`, `--denial`, `--skill`, `--kind`, `--superseded`, `--subagents`, `--format`, `--fields`, `--sort` |
| `digest` | サイズ上限付きの、依頼文の Markdown ダイジェスト | `--kind`, `--superseded`, `--max-prompt-chars`, `--max-session-chars` |
| `version` | インストールされているバージョン | `--format` |

どのコマンドも、「共通オプション」の節のオプションも取ります。

### records

`--type` はレコードの種類を選び、カンマ区切りでも繰り返しでも指定できます。
既定は `prompt,reply` です。

| 種類 | レコード |
| --- | --- |
| `prompt` | 人間が打った依頼文と、ダイアログで答えた内容 |
| `reply` | assistant のテキストブロック |
| `call` | ツールの呼び出しと、その結果 |
| `result` | ツールの結果。呼び出しのツールと入力付き |
| `summary` | コンテキストを圧縮したときに Claude Code が書く要約 |
| `thinking` | assistant の思考ブロックのうち、本文が残っているもの |

`thinking` は assistant の思考ブロックを、ほかの種類と同じくマスクして出します。
Claude Code は多くの思考ブロックで署名だけを残し、本文を残しません。
そうしたブロックは出さず、`stats` が `empty` に数えます。
本文が無いので、`!` で始まらないパターンには一致しません。

次の 8 つのオプションは、一部の種類にだけ効きます。

| フラグ | 効く種類 | 残すもの |
| --- | --- | --- |
| `--tool` | `call`, `result` | このツールの呼び出しとその結果 |
| `--touched` | `call`, `result` | このファイル、またはこのディレクトリ以下のファイルの編集・書き込みと、それを名指す Bash のコマンド |
| `--status` | `result` | この状態の結果。`error` か `ok` |
| `--denial` | `result` | この理由で実行されなかった呼び出しの結果。`user-rejected` など |
| `--skill` | `reply`, `thinking`, `call`, `result` | そのスキルの実行中に書かれたレコード |
| `--kind` | `prompt` | この打ち方の依頼文。`text`・`slash`・`rejection`・`answer`・`typed-answer`・`approval` のいずれか |
| `--superseded` | `prompt` | `true` なら、代わりに別の依頼文を送り直された依頼文。`false` ならそれ以外 |
| `--subagents` | `reply`, `thinking`, `call`, `result` | サブエージェントの履歴も含める |

- `--tool`・`--touched`・`--denial`・`--skill`・`--kind` は、共通の絞り込みと同じく
  繰り返し指定でき、先頭の `!` も使えます。`--status` は `--type` と同じく
  カンマ区切りで並べられ、`--superseded` は `true` か `false` を取ります。
- `--tool`・`--touched`・`--status`・`--denial`・`--skill`・`--kind`・
  `--superseded` は、調べる値を持つ種類だけを絞り、持たない
  種類はそのまま残します。そのため
  `ccdig records --type prompt,reply,call --skill tdd` は、すべての依頼文と、
  `tdd` の実行中に書かれた返答と呼び出しを残します。
- `--type` を省略すると、`records` は付けた `--tool`・`--status`・`--denial`・`--skill` が
  すべて効く種類だけを出します。`ccdig records --tool Bash` なら、`Bash` の
  呼び出しとその結果です。
  `--touched` は呼び出しだけを出します。結果も出すと、同じ呼び出しが
  2 回並ぶためです。`--kind` と `--superseded` は依頼文を出します。
- この 8 つのフラグは、効く種類が `--type` に無いとエラーになります。
  `--kind` は上の種類しか取らないので、`--kind call` は `--type` を
  案内するエラーになります。
- `--type` を付けず、上のフラグも `--subagents` 以外を付けずに
  検索すると、依頼文と返答だけが対象になります。
  ツールの呼び出しも読むなら `--type prompt,reply,call` を付けます。
  呼び出しの入力は長くなりがちなので、`--limit` で出力を小さく保てます。
  `--message-chars` と `--tool-chars` で本文を切り、`--fields` で
  キーを省くこともできます。
- 呼び出しは結果を持つので、`--type call` だけでも、質問への回答のような
  ツールの返したものが抜けません。`--type result` は結果を単独で並べます。
  中断したときのように、同じファイルに結果が無い呼び出しの結果は `null` です。
- サブエージェントの履歴は、`--subagents` を付けない限り読みません。
  サブエージェントに渡された指示は人間が打った依頼文ではないので、出力
  されません。出力されるのは、その返答・呼び出し・結果のうち `--type` で
  選んだ種類です。`--subagents` だけなら返答が出ます。呼び出しも読むなら
  `--type prompt,reply,call` を付けます。
- `--touched` は、`Edit`・`MultiEdit`・`Write` の `file_path`、
  `NotebookEdit` の `notebook_path`、`Bash` のコマンドの各語を読みます。
  語は、空白・引用符と、`;`・`>`・`=`・`,`・`:` などの文字で区切ります。
  値は、スラッシュかバックスラッシュで区切った要素がパスの要素の中に続けて
  現れれば一致します。そのため `src/cli` は `/repo/src/cli/args.rs` に一致し、
  `/repo/src/client.rs` には一致しません。`/` で始まる値はルートから照合します。
  先頭に `!` を付けた値は、パスを変える呼び出しの中からそのパスを除くので、
  読むだけの呼び出しは残りません。`/` と `.` 以外の要素が無い値はエラーです。
  コマンドは書かれたまま照合するので、コマンド中の相対パスは相対パスの値に
  しか一致せず、`cat` のように読むだけのコマンドも一致します。そうした
  コマンドは `--tool '!Bash'` で除けます。パスは、`--project` と同じく
  マスクする前の値で照合します。
- `--superseded` はファイルを最後まで読まないと決まらないので、0 より大きい
  `--around` とは併用できません。
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
| `--message-chars N` | 依頼文・返答・要約の本文の先頭 N 文字を残し、`… (M more characters, next: N+1-2N)` を付ける。`--message-chars FROM-TO` は、`cut -c` と同じく 1 から数えて FROM から TO までの文字を `(K characters before) …` の後に残すので、切ったときに示された範囲で続きを読める |
| `--tool-chars N` | ツールの入力にある文字列と、ツールの結果の出力（呼び出しが持つ結果も含む）に同じことをする。ファイル全体を書く `Write` で出力が埋まらずに済む。`FROM-TO` も取る。その場合、入力の文字列のうち TO - FROM + 1 文字以下のもの（ファイルのパスのように、その文字数で切ったときに切られなかったもの）はそのまま残す |

### sessions

選ばれた依頼文を含むセッション 1 つにつき 1 レコードを出します。絞り込みは
依頼文に適用されます。`--format` は `jsonl`（既定）・`json`・`md` を取ります。

- `--grep` と `--igrep` は、依頼文か返答が一致したセッションを選び、
  `match_count` にそれぞれで一致した件数を入れます。ある語に最も多く触れた
  セッションから読めます。`--type` を付けると、`records --type` と同じく
  ほかの種類のレコードも検索します（例: `--type prompt,reply,call,result`）。
  `--subagents` を付けると、各セッションのサブエージェントの履歴にある
  返答・呼び出し・結果での一致も、そのセッションに数えます。サブエージェントに
  渡された指示は依頼文ではないので、検索しません。どちらも `!` で始まらない
  パターンが必要です。
  ほかのキーは、`--grep` が無いときと同じく、
  ほかの絞り込みで残ったすべての依頼文から作ります。ただし、`!` 付きの
  パターンで除いた依頼文は数えません。Markdown には
  `Matches` 列が加わります。`!` で始まらないパターンが無ければ、
  `match_count` は `null` です。
- `prompt_count` と `started_at` は、選ばれた依頼文の件数と最初の時刻です。
  `ended_at` は、範囲内でそのセッションの最後の依頼文・返答・ツール呼び出し・
  ツール結果の時刻なので、依頼文が 1 件のセッションでも作業の終わりが分かります。
  ツールの呼び出しを却下したときに打ったコメントも、依頼文として数えます。
  質問への回答と、承認・却下したプランも同じです。
- `first_match` は、そのセッションで検索が最初に一致したレコードです。
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
- `resume` は、そのセッションを transcript が始まったディレクトリで再開する
  コマンドです。範囲や絞り込みで何が残っても、そのディレクトリを使います。例えば
  `cd /Users/alice/blog && claude --resume <session id>` の形で、POSIX シェルで
  クォートが要る部分はクォートします。Claude Code はセッションをそのディレクトリ
  ごとに保存するため、そこで実行する形にしています。コマンドを
  作れないときは `resume` が `null` になり、`resume_unavailable` が理由を示します。
  transcript に `cwd` が無ければ `no-cwd`、マスクで書き換わるなら `cwd-masked`、
  worktree を消した後のように今はディレクトリが無ければ `cwd-missing` です。
  Markdown にはどちらも出しません。

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
| `resume`, `resume_unavailable` | 上のとおり |

### stats

`records` が出すレコードを `--by` で指定したキーでまとめ、グループ 1 つに
つき 1 レコードを出します。`--type`・`--tool`・`--touched`・`--status`・
`--denial`・`--skill`・`--kind`・`--superseded`・`--subagents` と絞り込みは
`records` と同じくレコードを選ぶので、
ここでも `--type` の既定は `prompt,reply` です。`--format` は
`jsonl`（既定）・`json`・表になる `md` を取ります。

```sh
# 1 週間のツールの呼び出しを、ツールごとに入力の長さと合わせて数える
ccdig stats --by tool --type call --since 2026-11-16 --until 2026-11-22

# スキルごとの出力トークン数を、多い順に出す
ccdig stats --by skill --type reply,call --since 2026-11-01 --sort output

# ユーザーが差し戻したプランの数を、週ごとに出す
ccdig stats --by week --type result --tool ExitPlanMode --status error
```

| フラグ | 働き |
| --- | --- |
| `--by KEY` | これらのキーで、指定した順にまとめる（カンマ区切りか繰り返しで指定）。キーは `session`・`project`・`branch`・`type`・`tool`・`skill`・`subagent`・`model`・`kind`・`denial`・`status`・`day`・`week`・`month`。省くと全レコードで 1 グループで、レコードが無ければ何も出さない |
| `--min-count N` | レコードが N 件未満のグループを出さない。`--type thinking` のときは、本文が残っていない思考ブロックも数える |
| `--sort SORT` | `count`（既定）・`chars`・`output`・`cache_read` は多い順、`key` はキーの値の順。同じ値のグループはキーの順に並ぶ |
| `--limit N` | 並べ替えた後の先頭 N グループを残す |

- `day` は `2026-11-25`、`week` は月曜始まりの ISO 8601 週で `2026-W48`、
  `month` は `2026-11` の形で書き、どれも `--tz` のタイムゾーンで決めます。
- `kind` は `kind` キーと同じく依頼文の打ち方で、`model` は返答・思考ブロック・
  ツールの呼び出しのモデルです。`denial` と `status` はツールの結果のもので、
  `--denial` と `--status` が取る語で書きます。そのため
  `--type result --by denial,tool` で、実行されなかった呼び出しを理由と
  ツールごとに数えられます。`--type result` か、`--denial` のように結果を
  選ぶフラグが必要です。`--by tool` での依頼文のようにキーを持たない
  レコードと、`--by skill` でのスキル外の返答のようにキーが空のレコードは、
  値が `null` のグループに入り、`--sort key` ではほかのグループの後に並びます。
- キーはマスク後の値なので、`--mask-values` を使うとプロジェクトはラベルで
  まとまります。
- `--around`・`--context`・`--message-chars`・`--tool-chars` は、
  数える対象が変わってしまうのでエラーです。`--subagents` の無い
  `--by subagent` も、サブエージェントのレコードがすべて外れるのでエラーです。

レコードは次のキーをこの順に持ちます。

| キー | 中身 |
| --- | --- |
| `--by` のキー | その値。`--by` で指定した順に並ぶ。値が無ければ `null` |
| `session_title`, `project`, `file` | `--by session` のときだけ、セッションの最後のタイトル・プロジェクト・transcript のファイル。空なら `null`。`records --session` にそのまま渡せる |
| `count` | レコードの件数。`--type thinking` のときは、本文が残っている思考ブロックだけを数える |
| `empty` | `--type thinking` のときだけ、本文が残っていない思考ブロックの件数。`count` と `chars` には入らない。ほかのときは書かない |
| `chars` | マスク後の本文の文字数。ツールの呼び出しは入力の compact JSON、ほかの種類は本文を数える。トークン数はモデルに依存するので、`usage` が持つ。`--tool-chars` は入力の文字列を 1 つずつ切るので、測るものが違う |
| `sessions` | レコードを含むセッションの数。`--by session` のときは書かない |
| `first`, `last` | 最初と最後のレコードの時刻。`timestamp` と同じ形で書く |
| `usage` | トークン数。「トークン数」の節を参照 |

### digest

依頼文の Markdown ダイジェストを、サイズの上限付きで出します。絞り込みは
依頼文に適用され、`records` と同じく `--kind` と `--superseded` も効きます。
`--superseded false` は、打ち直された依頼文を除きます。ツールの呼び出しを却下したときに打ったコメントは、
前に `(rejection)` を付けて並べます。質問への回答には `(answer)`、
自分の言葉で打った回答を含むなら `(typed answer)`、承認した
プランには `(approval)` を付けます。貼り付けは `<pasted_content>` タグごと
`[pasted N chars]` に縮め、打った文が見えるようにします。回答からは、選んだ
選択肢のプレビュー（`selected preview:`）を除きます。`records` はどちらも残します。
返答の後に続く依頼文の下には、その返答の最初の 1 行を `- after:` で添えます。
「4つに絞りましょう」のような短い依頼文も、何への返事か分かります。依頼文と同じく
切り詰め、`--max-session-chars` の文字数にも数えます。

| フラグ | 既定 | 働き |
| --- | --- | --- |
| `--max-prompt-chars N` | 300 | 依頼文をそれぞれ N 文字で切る |
| `--max-session-chars N` | 2000 | セッションごとに最大 N 文字まで並べる |

### version

`ccdig version` はインストールされているバージョンを出します。
`--format json` では、バージョンと一緒に `schema` と `filter` を出します。
この 2 つの意味は「JSON の包み」の節にあります。

```json
{"version":"0.0.6","schema":9,"filter":"strict-v5"}
```

## 共通オプション

どのコマンドも、次のオプションをコマンド名の前にも後にも置けます。繰り返し
指定できるフラグの値は、前後で足し合わせます。

### 絞り込み

| フラグ | 残すもの |
| --- | --- |
| `--since`, `--until` | 開始から終了までのレコード。日付の `YYYY-MM-DD` か、分か秒までの時刻の `YYYY-MM-DDTHH:MM[:SS]` で指定し、`T` は空白でもよい |
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
- 1 つのセッションに絞るときは ID 全体を渡してください。ID が別の ID の
  先頭と同じなら、そちらにも一致します。Claude Code は transcript を
  セッションの ID で名付けるので、`--session` は名前が値で始まる
  transcript とそのサブエージェントの transcript だけを開き、1 セッションを
  速く読めます。行を残すのは、その transcript の名前と行の `sessionId` が
  どちらも値で始まるときだけです。`!` で始まる値だけなら、すべての
  transcript を読みます。
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
`stats` では先頭 N グループを残します。
`--format` はコマンドごとに違います。「コマンド」の節を参照してください。

`records`・`sessions`・`stats` は `--fields` も受け付けます。JSON と JSON Lines
で、指定したキーだけを、レコードが書く順に出します（カンマ区切りか
繰り返しで指定）。Markdown では、そのキーの部分と列だけを出します。
絞り込みと同じく先頭に `!` を付けたキーは逆に除くので、
`--fields '!agent,!session_title,!project,!cwd,!git_branch'` は同じ
セッションのどのレコードにも繰り返し入るキーを省き、
`sessions --fields '!usage'` はトークン数を省きます。コマンドが書かない
キーを指定するとエラーです。Markdown でも、各見出しの種類とセッションの
タイトルは常に出し、`records` の `usage` のように Markdown に出ないキーは
指定しても何も変わりません。

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
 "session_title_then":"Weekly report draft",
 "project":"-Users-alice-blog","cwd":"/Users/alice/blog","git_branch":"main",
 "timestamp":"2026-10-13T10:00:01+09:00","turn":1,
 "text":"/weekly-report last week","kind":"slash","command":"/weekly-report",
 "permission_mode":"","superseded":false,"redacted":false,
 "source":{"file":"/Users/alice/.claude/projects/-Users-alice-blog/<uuid>.jsonl",
           "line":3,"uuid":"<entry uuid>"},
 "matched":true,"matches":[]}
```

どのレコードも、`--type` の名前で種類を表す `type` で始まります。ほかの
種類も、`type` から `turn` までの同じキーで始まり、`redacted`・
`source`・`matched`・`matches` で終わります。

| 種類 | 間に入るキー |
| --- | --- |
| `reply`, `thinking` | `text`, `model`, `skill`, `subagent`, `subagent_type`, `subagent_description`, `parent_tool_use_id`, `usage` |
| `call` | `tool`, `input`, `tool_use_id`, `result`, `skill`, `subagent`, `subagent_type`, `subagent_description`, `parent_tool_use_id`, `usage` |
| `result` | `tool`, `input`, `tool_use_id`, `is_error`, `denial`, `text`, `subagent`, `subagent_type`, `subagent_description`, `parent_tool_use_id` |
| `summary` | `text` |

`--fields` で除かない限り、どのキーも常に出力されます。依頼文の `kind` は打ち方（`slash` か `text`）で、
ツールの呼び出しを却下したときに打ったコメントなら `rejection`、質問への
回答なら `answer`、自分の言葉で打った回答を含むなら `typed-answer`、
承認したプランなら `approval` です。
レコードの種類は `--type` で選ぶ `type` です。

- 呼び出しの `result` は、同じファイルにあるその結果の内容（マスク後）で、
  結果と同じ `is_error`・`denial`・`text` を持ちます。ファイルに結果が無ければ
  `null` です。
- `ExitPlanMode` の呼び出しの結果は、プランが呼び出しの入力と同じなら
  プラン本文を省き、`## Approved Plan:` の下に `(the plan in the call's input)`
  と書きます。`result` でも結果の `text` でも同じです。ユーザーが編集した
  プランは入力と違うので、そのまま残します。
- `turn` は、履歴ファイルでそのレコードまでにある打ち方が `text` か `slash` の
  依頼文の数で、書かれた順に、打ち直された依頼文も含めて数えます。レコードは
  直前の依頼文と同じ番号になり、2 つ目の依頼文への返答なら `turn` は 2 です。
  ダイアログへの回答と、却下した呼び出しへのコメントは、その前の依頼文と同じ
  番号です。最初の依頼文より前と、サブエージェントの履歴では 0 です。
- ターンが返答を求めないときに Claude Code がモデル `<synthetic>` として書く
  `No response requested.` だけの返答は除きます。
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
- `session_title_then` は、その行が書かれた時点のセッションのタイトルで、
  Claude Code が最初のタイトルを書く前は空です。Claude Code はタイトルを
  元になった依頼文の後に書くので、タイトルを変えた依頼文には変わる前の
  タイトルが付きます。`session_title` は最後のタイトルで、セッションの
  すべてのレコードに付きます。
- 打ち方が `rejection` の依頼文は、ユーザーがツールの呼び出しを却下した
  ときに打ったコメントです。`ExitPlanMode` で差し戻したプランへの
  コメントなどです。呼び出しの結果と同じ行にあるので、`source.uuid` は
  結果と同じで、ツール名は結果の `tool` で分かります。質問のダイアログで
  "clarify" を選んだときに Claude Code が書く定型文は、コメントに含めません。
  コメントなしで差し戻したプランも `rejection` で、本文は Claude Code の
  定型文の最初の 1 文です。
- 打ち方が `answer` の依頼文は、`AskUserQuestion` のダイアログでユーザーが
  選んだ回答です。打ち方が `typed-answer` の依頼文は、1 つでも自分の言葉で
  打った回答を含むものです。どちらも前後の Claude Code の定型文を除きます。
  Claude Code がこの 2 つを書き分けるのはバージョン 2.1.220 からで、それより
  前の履歴では回答はすべて `answer` です。打ち方が `approval` の依頼文は `ExitPlanMode` で承認したプランで、本文は
  Claude Code の定型文の最初の 1 文です。どちらも `rejection` と同じく、
  呼び出しの結果と同じ行にあります。
- `superseded` は、ユーザーがファイル内で後から別の依頼文を代わりに
  送った依頼文で `true` です。同じ行の後に続く依頼文のことで、Esc で
  依頼文を書き直したときや、返答を中断して送り直したときに生じます。
  `rejection` に代わりの依頼文が付くことはありません。
- `denial` は、呼び出しが実行されなかった理由を表す Claude Code の名前で、
  `user-rejected`・`permission-rule`・`automode-blocked`・`cancelled`
  などです。実行されたときは空です。Claude Code がこれを記録する前の
  セッションでは、結果のテキストから `user-rejected` だけを読みます。
  どの理由でも `is_error` は `true` です。
- 依頼文は、貼り付けたテキストを Claude Code が書いた位置に
  `<pasted_content id="...">` タグで囲んだまま持ちます。縮めるのは
  `digest` だけです。打った文だけを読むときは、jq でタグごと除きます。

  ```sh
  ccdig records --type prompt | jq '.text |= gsub(
    "<pasted_content id=\"[0-9a-f]+\">(.|\n)*?</pasted_content id=\"[0-9a-f]+\">"; "")'
  ```

- `permission_mode` は、Claude Code が依頼文の行に記録したモード
  （`default`・`plan`・`auto` など）です。行に記録が無ければ空で、
  スラッシュコマンドや古いセッションでは Claude Code が書きません。
- `subagent` は、セッション本体では空で、サブエージェントでは
  `agent-1a2b` のようなファイル名です。サブエージェントのレコードは、
  そのセッションの `session_id` と `session_title` を持ちます。セッション
  本体の transcript の最終書き込みが `--since` より前なら、タイトルは空です。
- `subagent_type`・`subagent_description`・`parent_tool_use_id` は、
  サブエージェントがどう起動されたかを表します。順に、`Explore` のような種別、
  呼び出し側が書いた目的（本文と同じくマスクします）、起動した呼び出しの
  `tool_use_id` です。その呼び出しはセッション本体の transcript にあり、
  別のサブエージェントが起動したサブエージェントなら、そのサブエージェントの
  transcript にあります。つまり、サブエージェント自身のファイルの外でその
  `tool_use_id` を持つレコードが親です。値はサブエージェントの transcript の隣の
  `.meta.json` から取ります（Claude Code が書く形式を観測したもの）。
  セッション本体、そのファイルが無いとき、書かれていないとき（呼び出しから
  起動されない、フォークしたサブエージェントで動くスキルなど）は空です。
- `input` は `tool_use` ブロックの入力に、Claude Code が行の
  `wireToolInputs` に保存した切り詰められていない値を重ねたものです。
  そのため、`ExitPlanMode` の `plan` のようにモデルが送らなかったキーも
  残ります。
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
  中で、その応答のうち絞り込みで残った最初の返答・思考ブロック・呼び出しだけが
  値を持ちます。残りと、
  トークン数の無い行は `null` です。
- 記録を足しても全体になるとは限りません。思考だけの応答のように返答も
  呼び出しも無い応答は、`--type thinking` でそのブロックを出さない限り、
  トークン数を載せる記録がありません。`--type` や
  絞り込みで記録がすべて外れた応答も同じです。
- `sessions` のレコードは最後に、範囲内のセッションの応答を 1 回ずつ足した
  同じ 4 つのキーを `usage` に持ちます。そのため `sessions` は assistant の
  行も読みます。
- `stats` のレコードの `usage` は、そのレコードが運ぶトークン数の合計です。
  1 回の応答は 1 回だけ数え、記録が 1 件でも残った応答だけが入るので、
  `sessions` の合計とは一致しません。`--by tool`・`type`・`kind`・`denial`・
  `status` では 1 回の
  応答の記録が別々のグループに入るので `usage` は `null` で、
  `--sort output` と `--sort cache_read` はエラーです。`--type prompt` の
  ようにトークン数を運ぶ記録が無いときも `null` です。

### JSON の包み

`--format json` は、レコードをバージョンと実行時のフラグで包みます。

```json
{"schema":9,"filter":"strict-v5","ccdig":"0.0.6",
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

`records --format md` は、出すレコードの件数を種類別に、`--around` が
前後のレコードとして足した件数と合わせて最初に示します
（例: `- Records: 7 (prompt 4, call 3) + 2 context`）。0 件なら
`- Records: 0` です。`sessions` と `stats` は、出すセッションかグループの
件数を最初に示し、`sessions` の検索では一致の件数も種類別に示します。
セッションの表には、`records --session` と `--project` に渡せる
`Session` 列と `Cwd` 列があります。

`records --format md` は、履歴ファイルごとのレコードをプロジェクトと
セッションを記した 1 つの見出しの下にまとめ、各レコードを秒までの時刻、
依頼文の後ならターン番号、種類の見出しの下に置きます。
`ExitPlanMode` のプランのように、呼び出しの入力のうち複数行にわたる文字列は、
残りの入力の後に `input.plan` のようなキー名を付けたテキストブロックで出し、
書かれたとおりに読めるようにします。呼び出しは入力の下に結果を出し、その結果は、
`--context` が一致した行を並べるときと、検索がその結果に一致して呼び出しには
一致しなかったとき（呼び出しは前後のレコードとして出ます）を除いて、単独では
出しません。`--limit` は JSON と同じく呼び出しの下に出した結果も数えるので、
Markdown には上限より少ないレコードが出ることがあります。
ツールの出力が JSON のオブジェクトか配列なら字下げし、`\u65e5` のような
エスケープを戻します。JSON と JSON Lines は、ツールが書いたままにします。サブエージェントのレコードは、自分の見出しの末尾にも
サブエージェント名を付けます（例: ``### 2026-10-13 10:25:01 JST - Tool call -
`agent-1` ``）。セッション本体のレコードと見分けられます。
サブエージェントの transcript の見出しには、種別と、1 行にまとめた目的も
出します（例: ``- Subagent type: `Explore` ``）。

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
3. `tool_result` ブロックを持つ。ただし、ユーザーがコメントを添えて
   呼び出しを却下した行なら、コメントを打ち方が `rejection` の依頼文として残す。
   `AskUserQuestion` か `ExitPlanMode` の呼び出しに答えるブロックなら、
   答えを打ち方が `answer`・`typed-answer`・`approval`・`rejection` の依頼文として残す
4. `<system-reminder>`、`<task-notification>`、`<local-command-…>`、
   `<bash-input>`、`<bash-stdout>`、`<user-prompt-submit-hook>` などの
   ハーネスのタグで始まる
5. 空であるか、`[Request interrupted by user` の印である

ルール 4 は `origin.kind` が `human` でも適用されます。Claude Code は `!` に
よるシェルエスケープを人間の入力として記録するためです。Claude Code が
`origin` を書くようになる前に記録されたセッションでは、ルール 2 を飛ばします。
思考ブロックは `--type thinking` のときだけ、本文が残っているものに限って出力します。

### ファイル

- 読むのは通常のファイルだけです。`<root>/<project>/<session>.jsonl` と、
  `--subagents` を付けたときは `<root>/<project>/<session>/subagents/*.jsonl`
  も読みます。`records` は、読むサブエージェントの transcript ごとに、隣にある
  同じ名前の `.meta.json`（1 MiB まで）も読みます。無いものと通常のファイルで
  ないものは何も表さず、読めないものやデコードできないものは警告します
  （`--on-skip fail` はこの警告も数えます）。
- シンボリックリンクはたどりません。
- 再開したセッションは以前のエントリを繰り返しますが、両方の transcript を
  同じ実行で読めば、各エントリ（`uuid` と行内の位置で識別）は元の
  セッションで 1 回だけ出力されます。Claude Code はコピーに新しい
  セッションの ID を書くので、新しい ID の `--session` ではコピーも出ます。

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
| `aws-account` | 文脈のある 12 桁の AWS アカウント ID。ARN の中、`Account`・`AWS_ACCOUNT_ID`・`OwnerId`・`AWS` のプリンシパルなどの名前への代入、ECR・S3・コンソールのサインインのホスト名、`--account-id` や `--owners` の後、`PS1` や `PROMPT` の括弧の中、「アカウント」の直後（カンマ区切りの並びも含む）と、同じ行か、見出しならその次の行の括弧の中。括弧の中の ID の隣にあるプロファイル名やアカウント名も伏せる。単独の 12 桁は残す |
| `aws-profile` | 文脈のある AWS のプロファイル名。`--profile` の後、`AWS_PROFILE`・`AWS_DEFAULT_PROFILE`・`profile` キーへの代入、`[profile name]`・`[sso-session name]`・認証情報ファイルの見出し、`source_profile`。文中の名前は残す |
| `email` | メールアドレス。`git@github.com:` のような SSH のリモートは除く |
| `git-user` | git config の `user.name` と `user.email`。大文字・小文字を問わない。どちらかが未設定ならコマンドは失敗する |
| `ip` | `127.x.x.x`・`0.0.0.0`・`255.255.255.255` 以外の IPv4 アドレスと、3 グループ以上の IPv6 アドレス |
| `user` | ユーザー名（`$HOME` の最後の部分）。大文字・小文字を問わず、区切りが `.`・`_`・`-` のどれでも。そのため `-Users-alice-b-blog` のようなプロジェクトのディレクトリ名の中でも |

これらのマスクと `--mask-values` は、本文でないフィールド、つまり
`project`・`cwd`・`git_branch`・ファイルのパス・`tool`・`model`・
`skill`・`subagent`・`subagent_type`・`command`・ツールの入力のキーと、stderr の警告にもかかります。
ツールの入力のキーには組み込みのルールもかけるので、トークンのような秘密が
キーになっていてもマスクされます。

- マスクした結果、前のキーと同じになったキー（伏せたファイルのパス 2 つ
  など）には `#2`・`#3` と番号を付け、どちらの値も残します。
- IPv4 のルールは `1.2.3.4` のような単独のバージョン番号とアドレスを区別
  できないため、それもマスクします。`v1.2.3.4` のように英字やドットの後に
  続くものは残ります。
- 下の名前だけのような、ユーザー名の一部だけの形は残ります。伏せたいときは
  `--mask-values` に書いてください。
- ID から離れて単独で出る AWS のアカウント名は残ります。伏せたいときは
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
- `records` が呼び出しと結果に出すツールの入力には、`Write` の呼び出しの中身のように、
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
