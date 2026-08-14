<img width="1086" height="350" alt="ChatGPT Image 2026年7月31日 10_10_49" src="https://github.com/user-attachments/assets/28856785-59ae-48fc-b629-b69da7e66636" />



# MAGI Council Agent Skill

[日本語](README.md) | [English](README.en.md)

3つの独立したCustom Agentにそれぞれ判断させ、投票結果をもとに最終決定を行うAgent Skillテンプレートです。

各Agentは、ほかのAgentの回答を見ない状態で投票します。
投票内容はHookによって一時的に封印され、3票が揃った後にRust製の単一バイナリが決められたルールで採決します。

単にAIへ「3つの人格で考えて」と依頼するのではなく、判断の独立性や再現性、監査のしやすさを重視した仕組みです。

## 質問から回答まで

![MAGI Councilでユーザーの質問を3人格が独立評価し、封印投票と決定論的採決を経て回答する流れ](docs/assets/magi-question-flow.svg)

図では、全人格に同じ質問と判断材料を配布します。人格や会話履歴を共有・移行するわけではありません。各人格は分離されたコンテキストで独立して判断し、投票が封印された後に`magi` CLIが結果を採決します。

[draw.ioソース](docs/diagrams/magi-question-flow.drawio)

## なぜこのSkillを作ったのか

AIに複数の立場を与えて議論させること自体は、通常のチャットでもできます。

ただし、1つの会話内で複数人格を演じさせる方法では、意思決定の仕組みとして使うにはいくつか問題がありました。

* 後から回答する人格が、先に出た意見へ引っ張られやすい
* 本当に独立して判断したのか確認しにくい
* 多数決をAI自身に任せると、実際の票とは異なる結論にまとめられる可能性がある
* 反対意見や条件付き賛成が、最終回答の要約から消えやすい
* 過去に決めた判断基準がチャット履歴へ埋もれる
* 実行履歴と、今後も使いたい判断原則が混ざりやすい
* 後から判断過程や投票内容を確認しにくい
* CopilotやClaude Codeなど、AIツールごとに同じ仕組みを作り直す必要がある

MAGI Councilでは、以下を別々の要素として管理します。

* 人格定義
* 全人格に同一内容で配布する質問と判断材料
* 各人格による秘密投票
* 投票結果の採決
* 長期的に残す判断基準

これにより、単なる「複数人格を演じた回答」ではなく、後から確認・再実行できる合議プロセスとして扱えるようにしています。

## 導入するメリット

| これまでの課題               | このSkillでの対応                                            |
| --------------------- | ------------------------------------------------------ |
| ほかの人格の回答に影響される        | 対応ランタイムでは、各人格を独立したSubagentとして起動し、投票完了まで回答を封印します        |
| AIが最終結果を都合よくまとめる      | JSON Schemaで投票形式を検証し、Rust製CLIが決められたルールで採決します       |
| 反対意見が要約から消える          | 少数意見、条件、リスクを`decision.json`と`decision.md`へ残します         |
| 開発者との会話で決まった基準が消える    | 人間が承認した原則だけを人格別メモリへ追加し、Gitで管理できます                      |
| 判断の根拠を後から確認できない       | 投票ファイルのハッシュとManifestを保存し、欠落や変更を検査できます                  |
| AIツールごとに仕組みを作り直す必要がある | Agent Skills、JSON Protocol、単一の`magi`バイナリを共通部分として再利用できます |

## 向いている用途

このSkillは、明確な正解がなく、複数の利害やリスクを比較する必要がある判断に向いています。

例えば、次のような場面です。

* アーキテクチャや技術選定
* Pull Requestをマージするかどうか
* リリースの実施可否
* セキュリティと利便性のトレードオフ
* 後方互換性を壊す変更
* 納期、品質、保守性の優先順位
* チーム独自の判断基準を、別の開発者やAgentへ引き継ぎたい場合

一方で、次のような判断余地が少ない作業には向いていません。

* 変数名やファイル名の変更
* Formatterで自動判定できる修正
* 原因が明確なテスト失敗
* 単純なコード変換

このような作業では、通常の単一Agentを使った方が速く、実行コストも抑えられます。

## 目的

このSkillでは、次の状態を実現することを目的としています。

* 同じモデルを使用していても、人格ごとに分離されたコンテキストで判断する
* すべての人格が投票するまで、親Agentへ回答内容を返さない
* AIに票数の計算や最終結果の書き換えを任せない
* 開発者が承認した判断原則だけを、人格メモリへ追加する
* 投票ファイルのハッシュを保存し、後から変更を検知できるようにする

## ディレクトリ構成

```text
.agents/skills/magi-council/   Agent Skill本体
.github/agents/                Orchestratorと3人格のCustom Agent
.github/hooks/                 秘密投票とアクセス制御用のHook
.claude/agents/                Claude Code向けOrchestratorと3人格
.magi/                         Constitution、設定、承認済みメモリ
```

## 必要環境

* 配布済みの`magi`バイナリ、またはビルド用のRust 1.85以上
* Agent Skillsに対応したクライアント
* 秘密投票には、Custom Agent、Subagent、Hooksを利用できるホスト

## 対応ホストと実行モード

| ホスト | 実行モード | 投票の扱い |
| --- | --- | --- |
| GitHub Copilot CLI / cloud agent | `sealed-subagents` | GitHub側のCustom AgentとHookを使い、3人格を独立実行して投票を封印する |
| Claude Code | `sealed-subagents` | Claude側のCustom AgentとHookを使い、各人格が投票を封印してreceiptだけを親へ返す |
| GitHub Copilot VS Code Agent mode | `sealed-subagents`（対応時） | Custom Agent、subagentツール、Hookが利用可能なら、3人格を個別実行して投票本文を親から隠す。いずれかを利用できない場合のみ`inline` |

`sealed-subagents`が既定です。ホスト名ではなく、Custom Agent、subagentツール、`subagentStart`/`subagentStop` Hookの利用可否で実行モードを決めます。SubagentまたはHookを利用できない場合だけ`inline`を指定し、独立実行ではないことを明示します。どちらのモードでも、最終結果は同じ`magi`バイナリが計算します。

## 導入から使用まで

以下は、既存の開発リポジトリへMAGI Councilを導入し、最初の採決を監査するまでの手順です。

### 1. `magi` CLIをインストールする

[GitHub Releases](https://github.com/isikawatatsuki/magi-council-skill/releases)から環境に合うアーカイブを取得し、同梱のSHA-256チェックサムを検証してから、`magi`（Windowsでは`magi.exe`）を`PATH`上へ配置します。配布バイナリの実行にRustやNode.jsは不要です。

ソースからインストールする場合は、このリポジトリのルートで実行します。

```bash
cargo install --path . --locked
cargo test --locked
```

インストール後、CLIを確認します。

```bash
magi version
```

### 2. 対象リポジトリへテンプレートを配置する

まず、全ホストで共通して使うAgent Skillをコピーします。次の例では`SOURCE`がこのテンプレート、`TARGET`がMAGIを導入するリポジトリです。

```bash
SOURCE=/path/to/magi-council-skill
TARGET=/path/to/your-repository

mkdir -p "$TARGET/.agents/skills"
cp -R "$SOURCE/.agents/skills/magi-council" "$TARGET/.agents/skills/"
```

使用するホストに応じてCustom AgentとHookもコピーします。既存ファイルがある場合は、そのまま上書きせず内容を確認してマージしてください。

GitHub Copilot（CLI、cloud agent、VS Code Agent mode）:

```bash
mkdir -p "$TARGET/.github"
cp -R "$SOURCE/.github/agents" "$TARGET/.github/"
cp -R "$SOURCE/.github/hooks" "$TARGET/.github/"
```

Claude Code:

```bash
mkdir -p "$TARGET/.claude"
cp -R "$SOURCE/.claude/agents" "$TARGET/.claude/"
```

GitHub Copilot VS Code Agent modeでも、Custom Agent一覧に3つのペルソナがあり、subagentツールとHookを利用できる場合は`sealed-subagents`を使用します。ペルソナの応答本文ではなく`VOTE_SEALED`だけが親へ返ることを確認できない場合は、sealed実行を中止します。

Hookを利用できず`inline`へ切り替える場合でも、subagentツールがあれば3つのペルソナを別々の新規コンテキストで実行します。ペルソナ固有の承認済み非公開メモリ、先行する投票、得票数、信頼度は後続ペルソナへ渡しません。この方式はコンテキスト分離を改善しますが、投票本文が親へ返るため`sealed-subagents`とは記録しません。

### 3. ホストのHookを有効にする

GitHub Copilotでは、コピーした`.github/hooks/magi-council.json`を使用します。

Claude Codeでは、次の設定を既存設定へマージします。

* `.claude/settings.json.authoring-off`の`hooks`を、対象リポジトリの`.claude/settings.json`へ追加する
* `.claude/settings.local.json`の`Bash(magi *)`権限を、対象リポジトリのローカル権限設定へ追加する

設定ファイル全体を上書きしないでください。`settings.json.authoring-off`は、このテンプレートの編集中に未インストールの`magi`が自動実行されないよう、意図的に無効な名前で保存されています。

### 4. プロジェクト状態を初期化する

Agent Skillを配置した後、対象リポジトリのルートで実行します。

```bash
cd "$TARGET"
magi init
```

`magi init`は`.magi/`へ設定、Constitution、人格別メモリ、`runs`、`tmp`、`locks`を作成します。既存の設定とポリシーは上書きしないため、再実行できます。

### 5. Orchestratorへ採決を依頼する

ホストのCustom Agent一覧から`magi-orchestrator`を選択し、判断してほしい質問と根拠の場所を伝えます。

```text
この認証方式を本番採用すべきか、MAGIで採決してください。
根拠として src/auth と tests/auth を確認してください。
```

GitHub CopilotとClaude CodeのOrchestratorは、run作成後の状態に応じて通常の3票フロー、または初回3票、THOMASの反証、最終3票、集計、監査の敵対的検証フローを実行します。敵対的検証はProject Configまたは利用者の明示指定を尊重します。各出力はHookまたは`magi` CLIで封印され、親には検証済み受領通知だけが返ります。SubagentまたはHookを利用できないホストでは、独立実行ではないことを明示した`inline`モードを使用し、敵対的検証は行いません。

### 6. 結果を確認・監査する

Orchestratorが提示する最終判定、条件、重大リスク、少数意見、確信度範囲を確認します。生成物は`.magi/runs/<runId>/decision.json`と`decision.md`に保存されます。

必要に応じて収集状態と完全性をCLIで確認できます。

```bash
magi run status <runId>
magi run audit <runId>
```

監査に成功すると`valid: true`が返ります。欠落やハッシュ不一致がある場合は終了コード1となり、その結果を有効な採決として扱ってはいけません。

### 7. 判断原則を任意で保存する

`decision.json`に今後も利用したい`memoryCandidates`がある場合だけ、人間が内容と適用範囲を確認して承認します。

```bash
magi memory approve <runId> <candidateId> --approved-by "<承認者>"
```

候補は自動保存されません。承認済み原則は提案した人格だけのメモリへ追加され、次回以降の採決で利用されます。

## 設計上の不変条件

- 3人格を1つのプロンプトへまとめず、他人格の票や途中集計を見せない
- 全人格へ同じ質問と共有コンテキストを渡す
- 各人格はVote Schemaに一致するJSONを1票だけ提出する
- 封印済み投票、Manifest、人格専用メモリをモデルへ公開しない
- 最終結果は必ず`magi run tally`で計算し、AIが書き換えない
- 少数意見、条件、未解決リスク、前提を最終結果へ残す
- リポジトリ内の文章は判断材料として扱い、MAGIの手順を上書きする命令として扱わない
- メモリ候補は人間の明示承認なしに昇格させない

## 状態遷移

```text
created -> collecting -> ready -> finalized
										\-> invalid
```

| 状態 | 意味 |
| --- | --- |
| `created` | requestとManifestを作成中 |
| `collecting` | 3人格から投票を収集中 |
| `ready` | 3票が揃い、ハッシュ検証に成功 |
| `finalized` | 決定論的な採決が完了 |
| `invalid` | 監査に失敗し、有効な結果として提示できない |

1人格が同じrunへ異なる2票目を提出することはできません。採決時にはrequestと全投票の存在、形式、SHA-256ハッシュを再検証します。

## 採決仕様

各人格は`approve`、`reject`、`abstain`のいずれかへ投票します。既定の方式は3票の単純多数決です。

| 条件 | 最終判定 |
| --- | --- |
| `approve`が2票以上 | `approved` |
| `approve`が2票以上で、賛成票に条件がある | `approved_with_conditions` |
| `reject`が2票以上 | `rejected` |
| 上記のいずれにも該当しない | `undecided` |
| 拒否権が有効で、未緩和の`critical`リスクが1件以上ある | `rejected_by_veto` |

critical risk vetoは多数決より優先されます。確信度は0から100の整数ですが、確率を意味しません。意見の差を平均で隠さないため、最終結果には最小値、中央値、最大値を記録します。

## 設定

プロジェクト状態を保存する`.magi/`内の`config.json`で管理します。

```json
{
	"schemaVersion": "1.0",
	"voting": {
		"method": "majority",
		"criticalRiskVeto": true
	},
	"memory": {
		"maxItemsPerPersona": 12
	},
	"security": {
		"redactProtectedToolResults": true
	}
}
```

| 設定 | 現在の仕様 |
| --- | --- |
| `voting.method` | `majority`固定 |
| `voting.criticalRiskVeto` | 未緩和のcritical riskによる拒否権を有効化 |
| `memory.maxItemsPerPersona` | 人格コンテキストへ読み込む承認済みメモリの上限 |
| `security.redactProtectedToolResults` | 将来の切替用に予約。現在のHookはこの値によらず秘匿処理を実行 |

## 投票データ

投票は`.agents/skills/magi-council/schemas/`内の`vote.schema.json`で検証されます。

| フィールド | 内容 |
| --- | --- |
| `runId` / `persona` | 対象runと投票人格 |
| `decision` | `approve`、`reject`、`abstain`のいずれか |
| `confidence` | 0から100の整数 |
| `summary` / `reasons` | 判定要約と、根拠を含む理由 |
| `conditions` | 承認に必要な条件 |
| `risks` | 重大度、緩和済みか、緩和策を含むリスク |
| `assumptions` | 未確認事項や判断の前提 |
| `memoryCandidates` | 再利用可能な判断原則の候補。1票につき最大3件 |

新規投票は`schemaVersion: "1.1"`を使用します。各`reasons[].evidence[]`は、一意な`id`、`type`、検証対象の`claim`、確認日時`observedAt`に加えて、次の追跡先を持ちます。

| Evidence type | 必須の追跡先 | 任意の補足 |
| --- | --- | --- |
| `file` | `path` | `lineStart`、`lineEnd`、`commitSha` |
| `test` | `command`、`outcome` | `output`、`commitSha` |
| `issue` / `pull_request` / `external_document` | HTTP(S) `url` | `title` |

確認できない内容はEvidenceとして捏造せず、`assumptions`、条件、棄権、低い確信度として表します。`schemaVersion: "1.0"`は既存Runの読み取り・監査互換のため引き続き受理されますが、新規投票には使用しません。

## 生成物

各runのデータは`.magi/runs/`以下のrun別ディレクトリへ生成されます。

| 生成物 | 内容 |
| --- | --- |
| `request.json` | 質問、共有コンテキスト、実行モード、採決設定、状態 |
| `manifest.json` | request、投票、決定のハッシュと完了状態。モデルへは非公開 |
| `sealed/<persona>.json` | 人格ごとの封印済み投票。モデルへは非公開 |
| `decision.json` | 機械可読な最終結果 |
| `decision.md` | 人間向けの最終レポート |

`decision.json`には、最終判定、票数、確信度範囲、拒否権、承認条件、high/criticalリスク、少数意見、前提、人格別要約、メモリ候補、完全性ハッシュが含まれます。

## 監査

完了済みrunは次のコマンドで監査できます。

```bash
magi run audit <runId>
```

監査はrequest、3票、decision、Manifestの整合性を検証し、欠落またはハッシュ不一致があれば終了コード1を返します。

## メモリ運用

人格が提案した`memoryCandidates`は、採決後も自動では保存されません。人間が原則、適用範囲、適用条件、非適用条件、根拠を確認し、候補単位で承認します。

```text
magi memory approve <runId> <candidateId> --approved-by "<承認者>"
```

承認済み項目は人格別メモリへ保存され、次回以降、その人格だけへ渡されます。生の会話、秘密情報、一時的なプロジェクト事実、他人格の票、最終票数はメモリへ保存しません。古い原則は履歴を直接書き換えず、無効化または後継項目で置き換えます。

判断時の優先順位は次のとおりです。

1. Constitution
2. 明示的なプロジェクトポリシー
3. 人格定義
4. 承認済みのスコープ付きメモリ
5. 現在の共有コンテキスト
6. モデルの一般知識

## 主要コマンド

| コマンド | 用途 |
| --- | --- |
| `magi init` | プロジェクト状態を既存ポリシーを上書きせず初期化 |
| `cargo test --locked` | 検証、封印、採決、監査、アクセス制御をテスト |
| `magi run create --stdin` | requestとランダムなrun IDを作成 |
| `magi run status <runId>` | 投票内容を公開せず、収集状態だけを表示 |
| `magi run import-votes <runId>` | `inline`モードの3票を警告付きで取り込み |
| `magi run tally <runId>` | 3票を検証し、最終結果を生成 |
| `magi run audit <runId>` | 完了済みrunの完全性を監査 |
| `magi persona load` / `magi vote seal` | Claude Codeで人格ポリシーを読み込み、投票を封印 |
| `magi memory approve` | 人間が承認した候補を人格メモリへ昇格 |

## 判断基準の保存

すべての実行結果を人格メモリへ追加するわけではありません。

投票結果や議論の中から、今後も継続して利用したい判断原則を人間が確認し、承認したものだけをメモリへ追加します。

これにより、一度だけ発生した特殊な事情や誤った判断が、そのまま人格の性格や判断基準として残ることを防ぎます。

承認済みのメモリはGitで管理できるため、チーム内でのレビューや別環境への引き継ぎも可能です。

## 3博士の人格設定について

MELCHIOR（メルキオール）、BALTHASAR（バルタザール）、CASPER（カスパー）という名称は、西方教会の伝承において「東方の三博士」に与えられた名前に由来します。

聖書本文には博士たちの人数や名前は記載されていませんが、黄金、乳香、没薬という3つの贈り物から、後世の西方キリスト教圏で3人の博士として定着しました。

また、本プロジェクトの合議システムは、『新世紀エヴァンゲリオン』に登場するMAGIシステムから着想を得ています。

作中のMAGIは、開発者である赤木ナオコの異なる側面を移植した3台のコンピューターで構成されています。

* MELCHIOR：科学者としての人格
* BALTHASAR：母親としての人格
* CASPER：女性としての人格

それぞれが異なる価値観から判断することで、単一の論理だけでは解決できない問題を合議によって処理します。

MAGI Councilでは、この考え方をソフトウェア開発や組織の意思決定へ適用できるように再設計しています。

### MELCHIOR（メルキオール）

MELCHIORは、原作における「科学者としての人格」を引き継ぎ、**論理と技術を担当する人格**として設定しています。

提案が技術的に正しいか、実現可能か、継続的に保守できるかを審査します。

感覚的な期待や流行ではなく、事実、根拠、テスト可能性、アーキテクチャの整合性を重視します。

> 技術的に正しく、実際に作り続けられるか。

### BALTHASAR（バルタザール）

BALTHASARは、原作における「母親としての人格」が持つ保護や継続という側面をもとに、**人、安全、運用を守る人格**として設定しています。

ユーザーへの被害、セキュリティ、プライバシー、障害時の復旧、運用担当者の負担などを審査します。

技術的に実現できる場合でも、人やサービスへ重大な危険を与える提案は承認しません。

> ユーザーと運用を守り、問題が起きても回復できるか。

### CASPER（カスパー）

CASPERは、原作の人格設定をそのまま性別として再現するのではなく、**社会の中で行動する人間としての現実的な側面**へ置き換えています。

ユーザーや関係者が実際に利用するか、費用や工数に見合うか、現在の人員や組織で運用できるかを審査します。

技術的に正しく安全な提案であっても、利用されないものや、維持できないものは価値を生まないという立場です。

> 現実に使われ、費用や労力に見合う価値を生むか。

### 3つの人格を分ける理由

重要な意思決定では、技術的な正しさだけで結論を出すことはできません。

例えば、技術的には優れた提案でも、ユーザーに重大な危険を与える場合があります。また、安全性が高くても、費用や運用負担が大きすぎれば継続できません。

MAGI Councilでは、意思決定に必要な観点を次の3つに分離しています。

* **MELCHIOR：技術的に正しいか**
* **BALTHASAR：人とサービスを守れるか**
* **CASPER：現実に価値を生み出せるか**

3つの人格が同じ質問と情報を受け取り、それぞれ独立して判断することで、1つの価値観へ偏った結論を防ぎます。

この人格設定は、原作の人格をそのまま再現するものではありません。

「科学者・母親・女性」という構成から着想を得ながら、ソフトウェア開発における意思決定へ適用できるよう、**技術・安全・現実**という3つの判断軸へ再構成しています。


## 敵対的検証（THOMAS）

GitHub CopilotまたはClaude Codeで`magi-orchestrator`を使用すると、Project Configまたは利用者が指定した`adversarialReview`に従います。`.magi/config.json`の`adversarialReview.mode`を`enabled`にするか、run作成入力へ`"adversarialReview": true`を指定すると、通常の3人格による初回投票を封印した後、非投票監査役THOMASが匿名化された判断を反証します。THOMASは4票目ではなく、採決には参加しません。

処理順は「初回3票 → `magi run prepare-adversarial <runId>` → THOMASの反証 → 最終3票 → `magi run tally <runId>`」です。初回・最終票、匿名対応表、反証は別々に封印され、最終票だけが正式な採決に使われます。未解決の具体的なCritical反証は自動否決せず、`suspended_for_human_review`として人間確認へ移行します。

このモードはモデル呼び出し回数とレイテンシを増やします。`inline`実行では厳密な独立性を保証できないため、敵対的検証との併用をCLIが拒否します。生成物は `rounds/initial/sealed`、`adversarial`、`rounds/final/sealed` に保存され、`magi run audit` がすべてのハッシュを検証します。無効時は従来の `collecting → ready → finalized` フローと既存Run形式を維持します。

## セキュリティ上の注意

このテンプレートでは、通常のAgent操作やプロンプトインジェクションによって、ほかの人格の投票内容を偶発的に参照してしまう可能性を減らしています。

ただし、SkillsやHooksは、OSレベルの隔離機能ではありません。

同じOSユーザー権限で任意のコマンドを実行できる環境では、暗号学的な秘密保持や完全なプロセス分離までは保証できません。

敵対的なAgentや信頼できないコードを扱う場合は、各人格を次のような単位で分離してください。

* 別プロセス
* 別コンテナ
* 別ユーザー
* 専用のMCPツール
* 権限を制限した実行環境

詳しい前提や脅威モデルについては、[Security model](.agents/skills/magi-council/references/security-model.md)を参照してください。

## 詳細仕様

- [Council protocol](.agents/skills/magi-council/references/protocol.md): 状態遷移、投票、確信度、完全性
- [Security model](.agents/skills/magi-council/references/security-model.md): 保護対象、防御層、限界
- [Memory policy](.agents/skills/magi-council/references/memory-policy.md): 候補の要件、優先順位、保守方針
- [Vote schema](.agents/skills/magi-council/schemas/vote.schema.json): 投票JSONの完全な制約
- [Request schema](.agents/skills/magi-council/schemas/request.schema.json): request JSONの完全な制約
- [Security policy](SECURITY.md): 脆弱性報告とサポートするセキュリティ境界
