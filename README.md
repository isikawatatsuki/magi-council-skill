<img width="1086" height="350" alt="ChatGPT Image 2026年7月31日 10_10_49" src="https://github.com/user-attachments/assets/28856785-59ae-48fc-b629-b69da7e66636" />



# MAGI Council Agent Skill

3つの独立したCustom Agentが、互いの判断を観測せずに投票し、Hookが回答を封印し、Node.jsスクリプトが決定論的に採決するAgent Skillテンプレートです。

## なぜこのSkillが必要か

AIへ「3つの人格で考えて」と依頼するだけでも、複数の観点を含む回答は生成できます。
しかし、一般的な1セッション内のロールプレイには、意思決定システムとして次の不便がありました。

- 1つの会話コンテキスト内で3人格を演じるため、後から回答する人格が先の判断に影響されやすい
- 各人格が同じ結論へ寄ったのか、本当に独立して判断したのか確認できない
- AI自身に多数決や最終判定を任せると、票と異なる結論へ要約される可能性がある
- 少数意見や条件付き賛成が、最終回答の要約で消えやすい
- 過去の判断基準がチャット履歴へ埋もれ、別のセッションや別の開発者へ継承しにくい
- 生の実行履歴と、今後も使うべき判断原則が混ざり、人格が無秩序に変化しやすい
- 判断結果の生成過程や投票内容を後から監査・再検証しにくい
- Copilot、Claude Codeなど、利用するAIアプリごとに同じ手順を作り直す必要がある

このSkillは、人格定義、共有コンテキスト、秘密投票、採決、長期記憶を分離し、
**「複数人格を演じた回答」ではなく「再現・監査できる合議プロセス」**として扱えるようにします。

## 導入すると良いこと

| これまでの課題 | このSkillによる改善 |
| --- | --- |
| ほかの人格の回答に引っ張られる | 対応ランタイムでは各人格を独立Subagentとして起動し、投票完了まで内容を封印する |
| AIが最終結果を都合よくまとめる | JSON Schemaで票を検証し、Node.jsスクリプトがルールどおりに採決する |
| 反対意見が要約で消える | 少数意見、条件、リスクを`decision.json`と`decision.md`へ残す |
| 開発者との対話で決まった基準が消える | 人間が承認した原則だけを人格別メモリへ昇格し、Gitで共有・レビューできる |
| 判断の根拠を後から追えない | 投票ファイルのハッシュとManifestを保存し、改ざんや欠落を検査できる |
| AIツールごとに仕組みを作り直す | Agent Skills、JSON Protocol、Node.jsスクリプトを共通コアとして再利用できる |

特に、次のような**正解が一つではなく、複数の利害やリスクを比較する判断**に向いています。

- アーキテクチャや技術選定
- Pull Requestのマージ・リリース可否
- セキュリティと利便性のトレードオフ
- 後方互換性を壊す変更
- 納期、品質、保守性の優先順位
- 開発チーム独自の判断基準を継承したい場面

一方、命名変更、Formatterで判定できる修正、明確なテスト失敗など、判断余地の小さい作業では通常の単一Agentの方が速く、低コストです。

## 目的

- 同じモデルでも人格ごとに隔離されたコンテキストで判断する
- 全人格の投票が終わるまで、親エージェントへ内容を返さない
- AIに票数計算や最終判定を書き換えさせない
- 開発者が承認した判断原則だけを人格メモリへ昇格させる
- 投票ファイルのハッシュを保存し、後から改ざん検知できるようにする

## 構成

```text
.agents/skills/magi-council/   Agent Skill本体
.github/agents/                Orchestratorと3人格のCustom Agent
.github/hooks/                 秘密投票・アクセス制御Hook
.magi/                         Constitution、設定、承認済みメモリ
```

## 必要環境

- Node.js 20以上
- Agent Skills対応クライアント
- 秘密投票には、Custom Agent/SubagentとHooksを利用できるホストが必要

## 対応ホストと実行モード

| ホスト | 実行モード | 投票の扱い |
| --- | --- | --- |
| GitHub Copilot CLI / cloud agent | `sealed-subagents` | GitHub側のCustom AgentとHookを使い、3人格を独立実行して投票を封印する |
| Claude Code | `sealed-subagents` | Claude側のCustom AgentとHookを使い、各人格が投票を封印してreceiptだけを親へ返す |
| GitHub Copilot VS Code Agent mode | `inline` | 1つのコンテキストで3観点を評価する。人格の独立性とHookによる封印は保証されない |

`sealed-subagents`が既定です。SubagentまたはHooksを利用できない場合だけ`inline`を指定し、独立実行ではないことを明示します。どちらのモードでも、最終結果は同じNode.js採決スクリプトが計算します。

## 初期化

既存リポジトリへこのテンプレートの各ディレクトリをコピーし、リポジトリ直下で実行します。

```bash
npm run magi:init
npm run magi:test
```

`magi:init`は、プロジェクト状態ディレクトリ内に設定、Constitution、人格別メモリ、実行用ディレクトリを作成します。既存の設定やポリシーは上書きしません。

### Claude CodeでHooksを有効にする

Claude Codeでは、`.claude/`内のCustom Agentに加えて、`settings.json.authoring-off`の`hooks`設定を有効な`settings.json`へ反映してください。既存の設定ファイルがある場合は上書きせず、`hooks`をマージします。

Hookは、保護対象へのアクセス制御とSubagent終了時のreceipt検証を担当します。GitHub Copilotでは保護対象を含むツール結果を置換できますが、Claude CodeではPostToolUseで結果本文を書き換えられないため、PreToolUseの事前ガードが主な防御です。

## 利用例

Custom Agentから `magi-orchestrator` を選択し、次のように依頼します。

```text
この認証方式を本番採用すべきか、MAGIで採決してください。
根拠として src/auth と tests/auth を確認してください。
```

Orchestratorは次の順序で処理します。

1. 質問と、全人格へ同一に渡す共有コンテキストを作る
2. `create-run.mjs`でランを作成する
3. `magi-melchior`、`magi-balthasar`、`magi-casper`を独立Subagentとして起動する
4. `subagentStop` Hookが各回答を検証・封印し、親には`VOTE_SEALED`だけを返す
5. 3票が揃った後、`tally-votes.mjs`で採決する
6. `decision.json`と`decision.md`だけを人間へ提示する

## 3つの人格

| 人格 | 主な判断軸 |
| --- | --- |
| MELCHIOR | 正確性、技術的実現性、アーキテクチャ、テスト、根拠の品質 |
| BALTHASAR | 利用者への影響、安全性、プライバシー、運用、アクセシビリティ、長期的影響 |
| CASPER | 価値、タイミング、コスト、採用可能性、組織上の現実性 |

全人格へ渡す質問、根拠、制約、未知事項は同一です。人格ごとに異なるのは、役割定義と人間が承認した専用メモリだけです。

## 設計上の不変条件

- 3人格を1つのプロンプトへまとめず、他人格の票や途中集計を見せない
- 全人格へ同じ質問と共有コンテキストを渡す
- 各人格はVote Schemaに一致するJSONを1票だけ提出する
- 封印済み投票、Manifest、人格専用メモリをモデルへ公開しない
- 最終結果は必ず`tally-votes.mjs`で計算し、AIが書き換えない
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
npm run magi:audit -- <runId>
```

監査はrequest、3票、decision、Manifestの整合性を検証し、欠落またはハッシュ不一致があれば終了コード1を返します。

## メモリ運用

人格が提案した`memoryCandidates`は、採決後も自動では保存されません。人間が原則、適用範囲、適用条件、非適用条件、根拠を確認し、候補単位で承認します。

```text
approve-memory.mjs <runId> <candidateId> --approved-by "<承認者>"
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

| コマンド / スクリプト | 用途 |
| --- | --- |
| `npm run magi:init` | プロジェクト状態を既存ポリシーを上書きせず初期化 |
| `npm run magi:test` | 検証、封印、採決、監査、アクセス制御をセルフテスト |
| `npm run magi:audit -- <runId>` | 完了済みrunの完全性を監査 |
| `create-run.mjs` | requestとランダムなrun IDを作成 |
| `run-status.mjs` | 投票内容を公開せず、収集状態だけを表示 |
| `tally-votes.mjs` | 3票を検証し、最終結果を生成 |
| `import-inline-votes.mjs` | `inline`モードの3票を警告付きで取り込み |
| `load-persona.mjs` / `seal-vote.mjs` | Claude Codeで人格ポリシーを読み込み、投票を封印 |
| `approve-memory.mjs` | 人間が承認した候補を人格メモリへ昇格 |

## セキュリティ境界

このテンプレートは、通常のAgent操作やプロンプトインジェクションによる偶発的な相互参照をかなり抑えます。ただし、同一OSユーザー権限で任意コマンド実行が許可されたホスト上では、Skills/Hooksだけで暗号学的・OSレベルの隔離は保証できません。

敵対的なAgentや信頼できないコードを想定する場合は、各人格を別プロセス・別コンテナ・専用MCPツールへ分離してください。

## 詳細仕様

- [Council protocol](.agents/skills/magi-council/references/protocol.md): 状態遷移、投票、確信度、完全性
- [Security model](.agents/skills/magi-council/references/security-model.md): 保護対象、防御層、限界
- [Memory policy](.agents/skills/magi-council/references/memory-policy.md): 候補の要件、優先順位、保守方針
- [Vote schema](.agents/skills/magi-council/schemas/vote.schema.json): 投票JSONの完全な制約
- [Request schema](.agents/skills/magi-council/schemas/request.schema.json): request JSONの完全な制約
- [Security policy](SECURITY.md): 脆弱性報告とサポートするセキュリティ境界

