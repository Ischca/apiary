#!/bin/bash
# Agent Teams v3: stdout をファイルに記録 + pane 監視
set -e

SESSION="apiary-teams-v3-$$"
TIMEOUT=90
OUTPUT="/tmp/apiary-teams-v3-output.txt"
MARKER="/tmp/apiary-teams-v3-done"

cleanup() {
    echo "Cleaning up..."
    tmux kill-session -t "$SESSION" 2>/dev/null || true
    rm -f "$MARKER"
}
trap cleanup EXIT
rm -f "$OUTPUT" "$MARKER"

echo "=== Agent Teams Test v3 ==="
tmux new-session -d -s "$SESSION" -x 200 -y 50

# Claude の出力をファイルに記録、完了マーカーをファイルで管理
tmux send-keys -t "$SESSION" \
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 claude --dangerously-skip-permissions --max-turns 5 -p 'Use multiple Task agents in parallel: agent 1 reads src/main.rs, agent 2 reads src/tui/app.rs, agent 3 reads src/pod/detector.rs. Summarize each.' > $OUTPUT 2>&1 && touch $MARKER" \
    Enter

# コマンド送信後2秒待ってからモニタリング開始
sleep 2

echo "Monitoring panes..."
for i in $(seq 1 $TIMEOUT); do
    sleep 1
    PANES=$(tmux list-panes -s -t "$SESSION" 2>/dev/null | wc -l | tr -d ' ')

    if [ "$PANES" -gt 1 ]; then
        echo "[${i}s] *** PANES: $PANES ***"
        tmux list-panes -s -t "$SESSION" -F "  #{pane_id} pid=#{pane_pid} cmd=#{pane_current_command}"
    fi

    # 完了チェック（マーカーファイルの存在）
    if [ -f "$MARKER" ]; then
        echo "[${i}s] Claude finished."
        break
    fi

    [ $((i % 10)) -eq 0 ] && echo "[${i}s] panes=$PANES (waiting...)"
done

echo ""
echo "=== Pane Result ==="
FINAL=$(tmux list-panes -s -t "$SESSION" 2>/dev/null | wc -l | tr -d ' ')
echo "Final panes: $FINAL"

echo ""
echo "=== Claude Output ==="
if [ -f "$OUTPUT" ]; then
    echo "Output size: $(wc -c < "$OUTPUT") bytes"
    echo "--- first 60 lines ---"
    head -60 "$OUTPUT"
    echo ""
    echo "--- Checking for agent patterns ---"
    grep -iE '(Running [0-9]+ Task|[0-9]+ agents? running|[0-9]+ local agents?|Task agent|subagent|team)' "$OUTPUT" || echo "(no agent patterns found in output)"
else
    echo "(no output file yet)"
fi
