#!/bin/bash
# Agent Teams が tmux pane を増やすかテスト
# 使用量を抑えるため、最大30秒で強制終了する
set -e

SESSION="apiary-teams-test-$$"
TIMEOUT=30
RESULT_FILE="/tmp/apiary-teams-test-result.txt"

cleanup() {
    echo "Cleaning up session: $SESSION"
    tmux kill-session -t "$SESSION" 2>/dev/null || true
    rm -f "$RESULT_FILE"
}
trap cleanup EXIT

echo "=== Agent Teams Pane Test ==="
echo "Session: $SESSION"
echo "Timeout: ${TIMEOUT}s"
echo ""

# 1. tmux セッションを作成
tmux new-session -d -s "$SESSION" -x 120 -y 40
echo "Initial panes:"
tmux list-panes -s -t "$SESSION" -F "#{pane_id} #{pane_pid} #{pane_current_command}"
INITIAL_PANES=$(tmux list-panes -s -t "$SESSION" | wc -l | tr -d ' ')
echo "Count: $INITIAL_PANES"
echo ""

# 2. Agent Teams 有効で Claude Code を実行
# -p (print mode) では Teams が動かない可能性があるので --no-input で試す
# 短いタスクを与える
echo "Starting Claude Code with CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1..."
tmux send-keys -t "$SESSION" \
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 claude -p 'List the files in the current directory and describe each one briefly. Use multiple agents if possible.' --max-turns 3 2>&1 | tee $RESULT_FILE; echo DONE" \
    Enter

# 3. ポーリングで pane 数を監視
echo "Monitoring panes for ${TIMEOUT}s..."
for i in $(seq 1 $TIMEOUT); do
    sleep 1
    CURRENT_PANES=$(tmux list-panes -s -t "$SESSION" 2>/dev/null | wc -l | tr -d ' ')

    # pane 出力もキャプチャして agent パターンを確認
    PANE_OUTPUT=$(tmux capture-pane -p -t "$SESSION" 2>/dev/null || echo "")
    AGENT_PATTERN=$(echo "$PANE_OUTPUT" | grep -oE '([0-9]+) (agents? running|local agents?|Task agents?)' || echo "none")

    if [ "$CURRENT_PANES" != "$INITIAL_PANES" ]; then
        echo "[${i}s] PANES CHANGED: $INITIAL_PANES -> $CURRENT_PANES (agents: $AGENT_PATTERN)"
        echo "Pane details:"
        tmux list-panes -s -t "$SESSION" -F "  #{pane_id} pid=#{pane_pid} cmd=#{pane_current_command}"
    elif [ "$AGENT_PATTERN" != "none" ]; then
        echo "[${i}s] panes=$CURRENT_PANES, detected: $AGENT_PATTERN"
    elif [ $((i % 5)) -eq 0 ]; then
        echo "[${i}s] panes=$CURRENT_PANES (no agent pattern)"
    fi

    # DONE マーカーが出たら終了
    if echo "$PANE_OUTPUT" | grep -q "^DONE$"; then
        echo "[${i}s] Claude finished."
        break
    fi
done

echo ""
echo "=== Final State ==="
FINAL_PANES=$(tmux list-panes -s -t "$SESSION" 2>/dev/null | wc -l | tr -d ' ')
echo "Initial panes: $INITIAL_PANES"
echo "Final panes:   $FINAL_PANES"
if [ "$FINAL_PANES" -gt "$INITIAL_PANES" ]; then
    echo "RESULT: Agent Teams CREATED new panes"
else
    echo "RESULT: Agent Teams did NOT create new panes"
fi

echo ""
echo "Final pane output (last 20 lines):"
tmux capture-pane -p -t "$SESSION" 2>/dev/null | tail -20

if [ -f "$RESULT_FILE" ]; then
    echo ""
    echo "Claude output (first 30 lines):"
    head -30 "$RESULT_FILE"
fi
