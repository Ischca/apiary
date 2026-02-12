#!/bin/bash
# Agent Teams: インタラクティブモードで pane 増加を検証
# 最大60秒で強制終了
set -e

SESSION="apiary-teams-int-$$"
TIMEOUT=60
LOG="/tmp/apiary-teams-int-log.txt"

cleanup() {
    echo "Cleaning up session: $SESSION"
    tmux kill-session -t "$SESSION" 2>/dev/null || true
}
trap cleanup EXIT

echo "=== Agent Teams Interactive Test ===" | tee "$LOG"
echo "Session: $SESSION" | tee -a "$LOG"
echo "" | tee -a "$LOG"

# 1. tmux セッション作成
tmux new-session -d -s "$SESSION" -x 120 -y 40

# 2. Agent Teams 有効で Claude をインタラクティブ起動
tmux send-keys -t "$SESSION" \
    "export CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1" Enter
sleep 1

# claude を起動（--max-turns で自動終了させる）
tmux send-keys -t "$SESSION" \
    "claude --max-turns 5" Enter
sleep 3

# 3. プロンプトを送信（Teams を誘発するタスク）
# 複数ファイルを並行調査するようなタスクを与える
tmux send-keys -t "$SESSION" \
    "Read src/main.rs, src/tui/app.rs, and src/pod/detector.rs in parallel and summarize each file in one sentence." Enter

echo "Prompt sent. Monitoring for ${TIMEOUT}s..." | tee -a "$LOG"

# 4. ポーリング
INITIAL_PANES=1
PANES_CHANGED=false
for i in $(seq 1 $TIMEOUT); do
    sleep 1
    CURRENT_PANES=$(tmux list-panes -s -t "$SESSION" 2>/dev/null | wc -l | tr -d ' ')
    PANE_OUTPUT=$(tmux capture-pane -p -t "$SESSION" 2>/dev/null || echo "")

    # agent パターン検出
    AGENT_BG=$(echo "$PANE_OUTPUT" | grep -oE '[0-9]+ agents? running in the background' || true)
    AGENT_LOCAL=$(echo "$PANE_OUTPUT" | grep -oE '[0-9]+ local agents?' || true)
    AGENT_TASK=$(echo "$PANE_OUTPUT" | grep -oE 'Running [0-9]+ Task agents?' || true)

    AGENTS="${AGENT_BG}${AGENT_LOCAL}${AGENT_TASK}"

    if [ "$CURRENT_PANES" != "$INITIAL_PANES" ] && [ "$PANES_CHANGED" = "false" ]; then
        PANES_CHANGED=true
        MSG="[${i}s] *** PANES CHANGED: $INITIAL_PANES -> $CURRENT_PANES ***"
        echo "$MSG" | tee -a "$LOG"
        tmux list-panes -s -t "$SESSION" -F "  #{pane_id} pid=#{pane_pid} cmd=#{pane_current_command}" | tee -a "$LOG"
    elif [ -n "$AGENTS" ]; then
        echo "[${i}s] panes=$CURRENT_PANES | $AGENTS" | tee -a "$LOG"
    elif [ $((i % 5)) -eq 0 ]; then
        echo "[${i}s] panes=$CURRENT_PANES" | tee -a "$LOG"
    fi

    # Claude の TUI が終了したか検出（プロンプトが戻る or max-turns 到達）
    if echo "$PANE_OUTPUT" | grep -qE '(❯|→|\$)\s*$'; then
        # Claude 終了っぽい → もう少し待つ
        if [ "$i" -gt 10 ]; then
            # Cooked パターンで終了確認
            if echo "$PANE_OUTPUT" | grep -qE '(Cooked|completed|max turns)'; then
                echo "[${i}s] Claude appears to have finished." | tee -a "$LOG"
                break
            fi
        fi
    fi
done

echo "" | tee -a "$LOG"
echo "=== Final State ===" | tee -a "$LOG"
FINAL_PANES=$(tmux list-panes -s -t "$SESSION" 2>/dev/null | wc -l | tr -d ' ')
echo "Initial panes: $INITIAL_PANES" | tee -a "$LOG"
echo "Final panes:   $FINAL_PANES" | tee -a "$LOG"

if [ "$PANES_CHANGED" = "true" ]; then
    echo "RESULT: Agent Teams DID create new panes" | tee -a "$LOG"
else
    echo "RESULT: Agent Teams did NOT create new panes (same as before)" | tee -a "$LOG"
fi

echo "" | tee -a "$LOG"
echo "Final capture-pane output:" | tee -a "$LOG"
tmux capture-pane -p -t "$SESSION" 2>/dev/null | tee -a "$LOG"
