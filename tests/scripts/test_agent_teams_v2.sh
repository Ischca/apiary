#!/bin/bash
# Agent Teams v2: 十分な待機時間 + permission skip
set -e

SESSION="apiary-teams-v2-$$"
TIMEOUT=90
LOG="/tmp/apiary-teams-v2.txt"

cleanup() {
    echo "Cleaning up..."
    tmux kill-session -t "$SESSION" 2>/dev/null || true
}
trap cleanup EXIT

echo "=== Agent Teams Pane Test v2 ==="
echo "Session: $SESSION"

tmux new-session -d -s "$SESSION" -x 200 -y 50

# Agent Teams 有効 + permission skip + max-turns 制限
tmux send-keys -t "$SESSION" \
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 claude --dangerously-skip-permissions --max-turns 5 -p 'You have access to Task agents. Please use 3 Task agents in parallel to: (1) read src/main.rs and summarize it, (2) read src/tui/app.rs and summarize it, (3) read src/pod/detector.rs and summarize it. Combine the results.' 2>&1; echo '___DONE___'" \
    Enter

echo "Waiting for Claude to process (max ${TIMEOUT}s)..."

PANES_EVER_CHANGED=false
for i in $(seq 1 $TIMEOUT); do
    sleep 1
    CURRENT_PANES=$(tmux list-panes -s -t "$SESSION" 2>/dev/null | wc -l | tr -d ' ')
    PANE_OUTPUT=$(tmux capture-pane -p -t "$SESSION" 2>/dev/null || echo "")

    if [ "$CURRENT_PANES" -gt 1 ]; then
        PANES_EVER_CHANGED=true
        echo "[${i}s] *** PANES: $CURRENT_PANES ***"
        tmux list-panes -s -t "$SESSION" -F "  #{pane_id} pid=#{pane_pid} cmd=#{pane_current_command}"
    fi

    # agent パターン
    AGENTS=$(echo "$PANE_OUTPUT" | grep -oE '([0-9]+ (agents?|Task agents?))' | head -1 || true)
    [ -n "$AGENTS" ] && echo "[${i}s] panes=$CURRENT_PANES | $AGENTS"

    # 完了チェック
    if echo "$PANE_OUTPUT" | grep -q '___DONE___'; then
        echo "[${i}s] Done."
        break
    fi

    [ $((i % 10)) -eq 0 ] && echo "[${i}s] panes=$CURRENT_PANES (waiting...)"
done

echo ""
echo "=== Result ==="
FINAL=$(tmux list-panes -s -t "$SESSION" 2>/dev/null | wc -l | tr -d ' ')
echo "Final panes: $FINAL"
echo "Panes ever changed: $PANES_EVER_CHANGED"

echo ""
echo "Last 30 lines of pane output:"
tmux capture-pane -p -t "$SESSION" -S -30 2>/dev/null
