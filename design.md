# Apiary — Claude Code Multi-Session Manager 設計ドキュメント

## 1. 解く問題

### 現状の痛み

開発者が複数のIssueを並列で作業するとき、以下のワークフローが一般的になりつつある：

- Issueごとにgit worktreeを作成
- 各worktreeで独立したClaude Codeセッションを起動
- Ghostty / iTerm2 / tmux のタブ・ペインで切り替え

この方式の問題：

1. **許可待ちの見逃し** — 別タブでClaude Codeがブロックされていても気づかない
2. **状態の不可視性** — どのセッションが作業中で、どれが完了したか一覧できない
3. **コンテキストスイッチのコスト** — タブを巡回して「今何やってたっけ」を思い出す負荷
4. **Agent Teams登場後の複雑化** — 1セッション内にteammateが複数いる二層構造が生まれ、管理対象が爆発する

### 既存ツールが解かない問題

| ツール | 対応範囲 | 非対応 |
|--------|----------|--------|
| Agent of Empires | 複数セッション管理 | Agent Teams内のteammate構造 |
| claude-code-monitor | セッション状態監視 | 二層構造、macOS限定 |
| Clux | セッション永続化 + API | teammate可視化 |
| claude-sessions | 軽量監視 | 状態制御、Agent Teams |

**空いているポジション**: 複数のClaude Codeセッションを俯瞰し、対話的に管理する統合インターフェース。

---

## 2. コンセプト

### 基本思想

**Claude Codeを「使う場所」ではなく「眺めて指揮する場所」を作る。**

開発者のメイン画面がこのTUIになる。
個々のClaude Codeセッションはバックグラウンドで動き、
このTUIが全体の状態を可視化し、必要なときだけ介入する。

比喩: 複数のモニターが並ぶ監視室。
各モニター（Pod）にはClaude Codeが映っている。
監視員（開発者）は椅子に座って全体を見渡し、
注意が必要なモニターがあればそこにフォーカスする。

### Pod モデル

**すべてをPodとして扱う。** tmuxのセッションもペインも、Podという単一の抽象に写像する。

- **solo Pod**: 1 member。単独のClaude Codeセッション
- **team Pod**: N members。Agent Teamsが生成したLead + teammates
- どちらも同じPodインターフェース。memberの数が違うだけ

#### Pod の状態ロールアップ

各memberの状態から、Pod全体の状態を導出する。優先度順：

```
permission > error > working > idle > done
```

| Pod内の状態 | Pod状態 | 意味 |
|-------------|---------|------|
| 1つでも permission | ⚠ permission | 人間の判断が必要 |
| 1つでも error | ❌ error | 要確認 |
| 1つでも working | 🔄 working | 作業中、放置OK |
| 全員 idle | ⏸ idle | 入力待ち |
| 全員 done | ✅ done | 完了 |

#### Podの生成

| パターン | コマンド | 動作 |
|----------|---------|------|
| 新規作成 | `pod create issue-1234 [--worktree ./path]` | tmuxセッション作成 + Claude Code起動 + Pod登録 |
| 既存の取り込み | `pod adopt <tmux-session> [--name issue-1234]` | 既存tmuxセッションをPodとして認識 |
| teammate自動検出 | （自動） | Pod内でAgent Teamsが起動→ 新しいペインを自動的にmemberとして登録 |

---

## 3. インターフェース設計

### レイアウト

TUIアプリケーション。左右分割。

```
┌─ Context Panel (35%) ──┬─ Pods Grid (65%) ──────────────────────┐
│                         │                                        │
│  （左ペイン）             │  ┌─ issue-1234 ⚠ ─┐  ┌─ issue-5678 🔄┐│
│                         │  │ Lead     🔄 12m │  │ Lead    🔄 8m ││
│  モードに応じて           │  │ impl     ⚠  3m │  │ research🔄 8m ││
│  内容が切り替わる         │  │ reviewer ⏸     │  │               ││
│                         │  └────────────────┘  └───────────────┘│
│                         │                                        │
│                         │  ┌─ hotfix ✅ ────┐  ┌─ feature-42 🔄┐│
│                         │  │ claude   ✅ 3m  │  │ Lead    🔄 1m ││
│                         │  │                 │  │ impl    ✏️ 1m ││
│                         │  └────────────────┘  └───────────────┘│
│                         │                                        │
│                         │          5 pods / 1 ⚠ / 8 members     │
└─────────────────────────┴────────────────────────────────────────┘
```

### 右ペイン: Pods Grid

全Podを一覧するグリッド。常時表示。

各Podカード:
```
┌─ issue-1234 ⚠ ──┐
│ Lead     🔄 12m  │ ← member名 + 状態 + 経過時間
│ impl     ⚠  3m  │
│ reviewer ⏸      │
└──────────────────┘
```

- Podカードのボーダー色 = ロールアップ済みPod状態
- カーソルキーでPod間を移動
- Enterで左ペインにそのPodの詳細を表示
- Podの追加・削除はリアルタイムで反映

solo Pod:
```
┌─ hotfix ✅ ─────┐
│ claude   ✅ 3m   │
└──────────────────┘
```

team Pod（多member）:
```
┌─ big-refactor 🔄 ┐
│ Lead      🔄 25m  │
│ frontend  ✏️ 20m  │
│ backend   🔄 18m  │
│ tests     ⏸       │
│ +2 more           │ ← 表示上限を超えたら折り畳み
└───────────────────┘
```

### 左ペイン: Context Panel

フォーカスに応じて内容が切り替わるマルチロールパネル。

#### モード一覧

| モード | トリガー | 表示内容 |
|--------|---------|---------|
| **Home** | 起動時 / Esc | コマンド入力。Pod作成、全体操作 |
| **Pod Detail** | PodをEnterで選択 | 選択Podのmember一覧 + ストリーム出力 |
| **Chat** | Pod Detail内で `c` | そのPodのLead / soloとの対話 |
| **Permission** | ⚠Podを選択 | 許可リクエストの詳細 + approve/deny |

#### Home モード

```
┌─ Home ─────────────────┐
│                         │
│ Commands:               │
│                         │
│ > pod create issue-9999 │
│   --worktree ~/dev/app  │
│                         │
│ Pod "issue-9999" を     │
│ 作成しました。Claude     │
│ Code を起動中...         │
│                         │
│ ❯ _                     │
└─────────────────────────┘
```

#### Pod Detail モード

Podを選択すると、そのPodの中身を展開。
capture-paneの出力を要約 or 直接ストリームで表示。

```
┌─ issue-1234 (3 members)┐
│                         │
│ ▸ Lead     🔄 12m       │ ← 選択するとその出力を表示
│   Delegating tasks...   │
│                         │
│ ▸ impl     ⚠  3m       │
│   Wants to run:         │
│   `rm -rf ./build`      │
│   [A]pprove [D]eny      │
│                         │
│ ▸ reviewer ⏸            │
│   Waiting for code...   │
│                         │
└─────────────────────────┘
```

#### Chat モード

Pod Detail から `c` で対話モードに入る。
選択したPodのClaude Code（Lead or solo）にメッセージを送信。

```
┌─ Chat: issue-1234 ─────┐
│                         │
│ Claude: issue-1234の    │
│ 実装が完了しました。テスト│
│ は全件パスしています。    │
│                         │
│ あなた: レビューして      │
│ 問題なければPR作って      │
│                         │
│ Claude: PRを作成しま     │
│ す...                    │
│                         │
│ ❯ _                     │
└─────────────────────────┘
```

実装: Podが管理するtmuxペインに対して `tmux send-keys` でテキストを送信。
応答は capture-pane で読み取り、表示。

#### Permission モード

⚠状態のPodを選択すると自動的にPermissionモードに入る。

```
┌─ Permission ───────────┐
│                         │
│ Pod: issue-1234         │
│ Member: impl            │
│                         │
│ Tool: bash              │
│ Command:                │
│ ┌─────────────────────┐ │
│ │ rm -rf ./build      │ │
│ │                     │ │
│ └─────────────────────┘ │
│                         │
│ [A]pprove  [D]eny       │
│ [S]kip (次の⚠へ)        │
│                         │
└─────────────────────────┘
```

実装: 許可プロンプトが出ているペインに `tmux send-keys y` or `n` を送信。

### グローバルキーバインド

| キー | 動作 |
|------|------|
| `←` `→` `↑` `↓` | Pods Grid 上でPodカーソル移動 |
| `Enter` | 選択PodのDetailを左ペインに表示 |
| `Esc` | 左ペインをHomeモードに戻る |
| `c` | Pod Detail → Chat モードへ |
| `n` | 次の⚠Podにジャンプ |
| `a` | Permission モードで Approve |
| `d` | Permission モードで Deny |
| `q` | 終了 |
| `/` | Homeモードでコマンド入力にフォーカス |
| `?` | ヘルプ表示 |

---

## 4. アーキテクチャ

### コンポーネント

```
┌──────────────────────────────────────────────────────┐
│                    TUI Application                    │
│                                                       │
│  ┌─ UI Layer (ratatui) ────────────────────────────┐ │
│  │  Context Panel  │  Pods Grid                     │ │
│  │  (左ペイン)      │  (右ペイン)                    │ │
│  └──────────────────────────────────────────────────┘ │
│         ↑ 描画                    ↑ 描画              │
│  ┌──────┴──────────────────────────┴────────────────┐ │
│  │              App State                            │ │
│  │  - pods: Vec<Pod>                                 │ │
│  │  - focus: PodId?                                  │ │
│  │  - mode: Home | Detail | Chat | Permission        │ │
│  └──────────────────────┬───────────────────────────┘ │
│                          │ 更新                        │
│  ┌───────────────────────┴──────────────────────────┐ │
│  │              Pod Manager                          │ │
│  │                                                   │ │
│  │  ┌─ Discovery ──┐  ┌─ Detector ──┐              │ │
│  │  │ tmux監視      │  │ capture-pane│              │ │
│  │  │ pane増減検出  │  │ hooks (opt) │              │ │
│  │  └──────────────┘  └─────────────┘              │ │
│  │                                                   │ │
│  │  ┌─ Pod Store ──┐  ┌─ Executor ──┐              │ │
│  │  │ Pod⟷pane対応 │  │ send-keys   │              │ │
│  │  │ 永続化(JSON) │  │ (Chat/Perm) │              │ │
│  │  └──────────────┘  └─────────────┘              │ │
│  └──────────────────────────────────────────────────┘ │
│         │                                              │
└─────────┼──────────────────────────────────────────────┘
          │ tmux API
┌─────────┴──────────────────────────────────────────────┐
│                       tmux                              │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  (hidden)      │
│  │ session1 │  │ session2 │  │ session3 │  バックグラウンド│
│  │ pane×3   │  │ pane×2   │  │ pane×1   │  で動作        │
│  └─────────┘  └─────────┘  └─────────┘                 │
└────────────────────────────────────────────────────────┘
```

### 前の設計との違い

| 項目 | 旧設計 | 新設計 |
|------|--------|--------|
| 主体 | tmuxの装飾（pane-border, status-bar） | 独立したTUIアプリケーション |
| Claude Codeのセッション | フォアグラウンド（ユーザーが直接見る） | バックグラウンド（TUIが間接表示） |
| 操作 | tmuxキーバインド + CLI | TUI内のモード遷移 + キーバインド |
| 介入度 | 観察のみ | 観察 + 対話（Chat）+ 判断（Permission） |
| ユーザーの居場所 | 各Claude Codeのペインを行き来 | **このTUIに常駐** |

### 重要な設計判断: Rendererの削除

旧設計のRenderer（tmux pane-border / status-bar の更新）は不要になる。
TUI自身が描画を担う。tmuxは純粋にClaude Codeのプロセスをホストする「コンテナランタイム」になる。

ユーザーはtmuxのセッションを直接見ることを想定しない。
必要なら `tmux attach -t <session>` で直接アクセスできるが、
通常の操作はすべてTUI経由。

---

## 5. データモデル

### Pod

```
Pod {
  name:         string          // "issue-1234", "hotfix-9999"
  type:         "solo" | "team"
  members:      Member[]
  status:       PodStatus       // ロールアップ済み
  tmux_session: string          // tmuxセッションID
  worktree:     string?         // git worktreeパス（あれば）
  created_at:   timestamp
}
```

### Member

```
Member {
  role:         string          // "Lead", "implementer", "reviewer", "claude"
  status:       MemberStatus    // 個別の状態
  tmux_pane:    string          // tmuxペインID（%3 等）
  last_change:  timestamp       // 状態が最後に変わった時刻
  last_output:  string          // capture-paneの最新出力（要約用）
}
```

### MemberStatus (enum)

```
idle | working | permission | error | done
```

### PodStatus

memberの状態から優先度順にロールアップ:
`permission > error > working > idle > done`

### AppState

```
AppState {
  pods:           Vec<Pod>
  focus:          Option<PodId>    // 右ペインで選択中のPod
  mode:           Mode             // Home | Detail | Chat | Permission
  command_input:  String           // Homeモードの入力バッファ
  chat_input:     String           // Chatモードの入力バッファ
  chat_history:   Vec<Message>     // 現在フォーカス中のPodとの対話履歴
}
```

### Pod Store（永続化）

`~/.config/apiary/pods.json` に保存。起動時に読み込み、tmuxの実態と照合する。
tmux上に存在しないペインを持つPodは `stale` とマークし、ユーザーに通知。

---

## 6. 状態検出

### member の状態検出

tmux `capture-pane -t {pane_id} -p` で最新の出力を取得し、パターンマッチで各memberの状態を判定する。

| 状態 | 検出パターン（候補） | 信頼度 |
|------|----------------------|--------|
| **入力待ち（idle）** | プロンプト `❯` が最終行付近 | 高 |
| **作業中（working）** | スピナー、ツール実行出力 | 中 |
| **許可待ち（permission）** | `Allow? (y/n)` 系のパターン | 高 |
| **完了（done）** | セッション終了 or プロセスなし | 高 |
| **エラー（error）** | エラーメッセージパターン | 中 |

member状態はPod Storeに記録され、Pod単位のロールアップに使われる。

### 補助方式: hooks（オプション）

Claude Codeのhooks（`preToolUse`, `postToolUse`等）で状態変化イベントをファイル or UNIXソケットに出力。

```jsonc
// ~/.claude/settings.json に追加（オプション）
{
  "hooks": {
    "preToolUse": [{
      "type": "command",
      "command": "echo '{\"event\":\"tool_start\",\"tool\":\"$TOOL_NAME\"}' >> /tmp/claude-monitor.sock"
    }]
  }
}
```

hooks がなくても capture-pane だけで動作する。hooks があれば状態遷移の検出が即時になり精度が上がる。

### ポーリング戦略

全memberを均等にポーリングするのではなく、優先度に応じた適応的ポーリング：

| 条件 | ポーリング間隔 |
|------|--------------|
| フォーカス中のPodのmembers | 1秒 |
| ⚠ permission状態のmember | 1秒 |
| 🔄 working状態のmember | 3秒 |
| ⏸ idle / ✅ done のmember | 10秒 |

### teammate の自動検出

Pod内で Agent Teams が起動されると、tmuxに新しいペインが出現する。
Discovery がこれを検出し、既存Podのmemberとして自動登録する。

判定ロジック:
1. 新しいペインが既存PodのtmuxセッションIDに属しているか
2. capture-paneの出力にClaude Codeの特徴的なUIが含まれるか
3. 両方を満たす → そのPodのmemberとして追加

member の役割名（Lead, implementer等）はcapture-pane出力から推定。
推定できない場合は `member-0`, `member-1` のような自動命名でフォールバック。

> ⚠ Agent Teamsのtmux構造は未ドキュメント。experimentalフェーズでの変更リスクあり。
> 対策: 検出ロジックをプラガブルにし、構造変更に追従しやすくする。

---

## 7. Chat / Permission の実装

### Chat モード

左ペインでのテキスト入力を、対象Podのtmuxペインに送信する。

```
入力 → tmux send-keys -t {pane_id} "テキスト" Enter
応答 ← tmux capture-pane -t {pane_id} -p（差分検出）
```

#### 差分検出

1. Chat入力送信前の capture-pane 出力を記録（スナップショット）
2. 送信後、定期的に capture-pane を実行
3. スナップショットとの差分 = Claude Codeの新しい出力
4. 差分を左ペインに表示

#### 制約

- Claude Codeのリッチなterminal出力（色、レイアウト等）は失われる
- テキストの差分のみ。完璧なチャット体験ではない
- 複雑な対話が必要なら `tmux attach -t <session>` で直接接続を推奨

### Permission モード

⚠状態のmemberが検出されたら:

1. capture-pane出力から許可リクエストの内容をパース
   - ツール名（bash, write, etc.）
   - 実行しようとしているコマンド/内容
2. 左ペインにフォーマットして表示
3. `a` (approve) → `tmux send-keys -t {pane_id} y Enter`
4. `d` (deny) → `tmux send-keys -t {pane_id} n Enter`
5. `s` (skip) → 次の⚠memberにフォーカス移動

---

## 8. 技術選定

### 言語: Rust

TUIの品質が差別化要因であるため、Rustを選択。

| 観点 | 選択理由 |
|------|---------|
| TUIフレームワーク | ratatui — Rust TUIエコシステムの標準。高品質な描画 |
| tmux操作 | `std::process::Command` で tmux CLI をラップ |
| バイナリ配布 | 単一バイナリ、依存なし |
| パフォーマンス | 多数ペインのポーリング + TUI描画の両立に必要 |
| エコシステム | Agent of Empires（Rust）と同じ言語。参考にできる |
| 非同期 | tokio — Discovery, Detector のポーリングを非同期で並行実行 |

### 主要クレート

| 用途 | クレート |
|------|---------|
| TUI | `ratatui` + `crossterm` |
| 非同期ランタイム | `tokio` |
| シリアライズ | `serde` + `serde_json` |
| CLI引数 | `clap` |
| ログ | `tracing` |
| 正規表現 | `regex` |

### 依存

- **必須**: tmux (>= 3.2)
- **必須**: Claude Code
- **オプション**: Claude Code hooks 設定（精度向上用）
- **オプション**: git（worktree連携のとき）

### 配布

- `cargo install apiary` で単一バイナリ
- Homebrew tap
- GitHub Releases（クロスコンパイル済みバイナリ）

---

## 9. MVP スコープ

### Phase 1: solo Pod + 可視化（v0.1）

- [ ] TUI起動（ratatui + crossterm）
- [ ] 左右分割レイアウト
- [ ] `pod create` / `pod adopt` / `pod list` / `pod drop`
- [ ] Pods Grid: Podカードの表示、カーソル移動
- [ ] capture-pane による member 状態検出（ポーリング）
- [ ] Pod Detail モード（左ペイン）
- [ ] Pod Store 永続化（JSON）

このフェーズでは全Podがsolo（1 member）。
Agent Teams非依存で、「全Podが見渡せる + 状態がわかる」を最速で出す。

### Phase 2: Chat + Permission（v0.2）

- [ ] Chat モード（send-keys + capture-pane差分検出）
- [ ] Permission モード（許可リクエストのパース + approve/deny）
- [ ] ⚠発生時の通知（TUI内 + オプションでデスクトップ通知）
- [ ] `n` キーで次の⚠Podにジャンプ

### Phase 3: team Pod（v0.3）

- [ ] Agent Teamsのペイン自動検出 → 既存Podへのmember追加
- [ ] member役割名の推定（Lead / teammate名）
- [ ] team PodのPod Detailで全member展開
- [ ] 適応的ポーリング（フォーカスPodは高頻度、他は低頻度）

### Phase 4: 便利機能（v0.4）

- [ ] `pod create` に `--worktree` 統合（git worktree作成も一括）
- [ ] hooks連携（オプション、状態検出の精度向上）
- [ ] Podごとの経過時間・コスト推定
- [ ] tmux-resurrect 連携（Pod情報の復元）

---

## 10. リスクと対策

| リスク | 影響 | 対策 |
|--------|------|------|
| Agent Teams の tmux 構造変更 | teammate自動検出の破壊 | 検出ロジックをプラガブルに。team Podが壊れてもsolo Podにフォールバック |
| Claude Code の出力フォーマット変更 | member状態 / 許可パースの誤判定 | パターンを設定ファイルで外部化。コミュニティで更新 |
| Agent Teams が正式化せず廃止 | team Podの前提崩壊 | Phase 1-2 は Agent Teams 非依存。solo Pod管理だけでも価値がある |
| tmux 非ユーザーへのリーチ不足 | ユーザーベース限定 | tmux は Agent Teams の split-pane に必須。ターゲット層と一致 |
| capture-pane のパフォーマンス | 多数memberで負荷 | 適応的ポーリング（フォーカスPodは1秒、他は10秒） |
| Pod Store の永続化 | 再起動時にPod情報が消失 | JSONファイルで永続化。tmux復元時にPod情報も復元 |
| Chat の品質 | terminal出力の情報欠落 | 割り切る。複雑な対話は `tmux attach` を案内 |
| send-keys によるClaude Code操作 | 入力タイミングの競合 | idle状態のときのみsend-keysを許可。working中は送信をブロック |

---

## 11. 非スコープ

以下は意図的にスコープ外とする：

- **Claude Codeの完全な代替**: terminal出力の完全再現は目指さない（直接attachで対応）
- **Web UI**: ブラウザベースのダッシュボード
- **マルチマシン対応**: リモートセッションの管理
- **Claude Code以外のエージェント**: 汎用AIエージェント管理（Agent of Empiresの領域）
- **チャット履歴の永続化**: セッション間で対話履歴は保持しない（Claude Code自体が保持）

「Claude Code × tmux × Agent Teams」に特化する。
汎用性を捨てることで、この組み合わせでの体験を最高にする。

---

## 12. ポジショニング

```
                    制御する ←──────────────→ 観察するだけ
                        │                        │
      Agent of Empires  │                        │  claude-sessions
      Clux              │                        │  claude-code-monitor
      claude-pilot      │                        │
                        │                        │
                        │                        │
                        │                        │
  Agent Teams非対応 ────┼────────────────────────┼──── Agent Teams対応
                        │                        │
                        │   ┌──────────┐         │
                        │   │ 本ツール  │         │
                        │   └──────────┘         │
                        │                        │
```

「Agent Teams対応 × 観察と制御のバランス」という独自の象限。

**既存ツールとの決定的な違い**:
ユーザーの居場所が「各Claude Codeセッション」ではなく「このTUI」になること。
他のツールはClaude Codeを使う体験の補助。
これはClaude Codeを使う体験そのものを再定義する。

---

## 13. 未決事項（調査が必要）

1. **Agent Teamsのtmux構造の詳細調査**: experimentalを有効にして実際の構造を確認する必要あり。ペインのsplit方式、ヘッダー有無、teammate名の表示位置など。
2. **capture-paneのパターン精度**: Claude Codeの各状態の出力パターンを網羅的に収集する必要あり。バージョン間の差異も確認。

---

## 14. 決定済み事項

| 項目 | 決定 | 根拠 |
|------|------|------|
| **名前** | Apiary | `hive`はApache Hive / GraphQL Hiveと衝突。養蜂場（Hiveの管理場所）としてメタファーが正確。CLI: `apiary`, エイリアス: `ap` |
| **ライセンス** | MIT | 最大限の採用しやすさ。OSSエコシステムの標準 |
| **Chat差分検出** | 行ベースdiff | capture-paneは行単位テキスト。送信前スナップショットと行数比較で新規行を抽出。Phase 2で実装し、必要なら改善 |
| **Podカードレイアウト** | 固定幅カード + 自動折り返し | カード幅20文字固定。画面幅 ÷ (カード幅 + gap) でカラム数を自動算出。ratatuiのLayout constraintで実装 |
| **言語** | Rust | TUI品質が差別化要因。ratatui + tokio |
| **Podモデル** | 全セッションをPodとして統一 | solo/team問わず同一インターフェース。memberの数が違うだけ |
| **TUI構成** | 左右分割（Context Panel 35% / Pods Grid 65%） | 左ペインはモード切替式（Home / Detail / Chat / Permission） |
| **tmuxの役割** | コンテナランタイム | ユーザーはtmuxを直接操作しない。Apiaryが全面的にUIを担う |
| **設定ファイル** | `~/.config/apiary/` | `pods.json`（Pod Store）, `config.toml`（パターン定義等） |
