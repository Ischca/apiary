#!/bin/bash
# Agent Teams v4: tmux 内で split-pane モードをテスト
set -e

SESSION="apiary-teams-v4-$$"
TIMEOUT=120
LOG="/tmp/apiary-teams-v4.log"

cleanup() {
    echo "Cleaning up session: $SESSION"
    tmux kill-session -t "$SESSION" 2>/dev/null || true
}
trap cleanup EXIT

echo "=== Agent Teams Split-Pane Test ===" | tee "$LOG"
echo "Session: $SESSION" | tee -a "$LOG"

# 1. tmux セッション作成
tmux new-session -d -s "$SESSION" -x 200 -y 50

# 2. 環境変数設定 + Claude 起動
tmux send-keys -t "$SESSION" \
    "export CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1" Enter
sleep 0.5

tmux send-keys -t "$SESSION" \
    "claude --dangerously-skip-permissions --max-turns 10" Enter

# 3. 確認画面を通過: "Yes, I accept" を選択
echo "Waiting for permission confirmation screen..." | tee -a "$LOG"
sleep 3

# 下矢印で "Yes, I accept" に移動して Enter
tmux send-keys -t "$SESSION" Down Enter
echo "Accepted permission bypass." | tee -a "$LOG"

# Claude TUI が起動するまで待機
sleep 5

# 確認: Claude が起動したか
STARTUP_CHECK=$(tmux capture-pane -p -t "$SESSION" 2>/dev/null || echo "")
echo "Startup check (last 5 non-empty lines):" | tee -a "$LOG"
echo "$STARTUP_CHECK" | grep -v '^$' | tail -5 | tee -a "$LOG"
echo "" | tee -a "$LOG"

# 4. Agent Team を作成するプロンプトを送信
tmux send-keys -t "$SESSION" \
    "Create an agent team with 2 teammates to work in parallel. Teammate 1: read and summarize src/main.rs. Teammate 2: read and summarize src/pod/detector.rs." Enter

echo "Prompt sent. Monitoring panes for ${TIMEOUT}s..." | tee -a "$LOG"

# 5. pane 数をポーリング
INITIAL_PANES=$(tmux list-panes -s -t "$SESSION" | wc -l | tr -d ' ')
MAX_PANES=$INITIAL_PANES
PANES_CHANGED=false

for i in $(seq 1 $TIMEOUT); do
    sleep 1
    CURRENT_PANES=$(tmux list-panes -s -t "$SESSION" 2>/dev/null | wc -l | tr -d ' ')

    if [ "$CURRENT_PANES" -gt "$MAX_PANES" ]; then
        MAX_PANES=$CURRENT_PANES
        PANES_CHANGED=true
        echo "[${i}s] *** PANES INCREASED: $INITIAL_PANES -> $CURRENT_PANES ***" | tee -a "$LOG"
        tmux list-panes -s -t "$SESSION" -F "  #{pane_id} pid=#{pane_pid} cmd=#{pane_current_command} #{pane_width}x#{pane_height}" | tee -a "$LOG"
    fi

    if [ $((i % 15)) -eq 0 ]; then
        echo "[${i}s] panes=$CURRENT_PANES (max=$MAX_PANES)" | tee -a "$LOG"
        for pane_id in $(tmux list-panes -s -t "$SESSION" -F "#{pane_id}"); do
            PANE_TAIL=$(tmux capture-pane -p -t "$pane_id" 2>/dev/null | grep -v '^$' | tail -3 || echo "(empty)")
            echo "  $pane_id: $PANE_TAIL" | tee -a "$LOG"
        done
    fi
done

echo "" | tee -a "$LOG"
echo "=== Final Result ===" | tee -a "$LOG"
FINAL_PANES=$(tmux list-panes -s -t "$SESSION" 2>/dev/null | wc -l | tr -d ' ')
echo "Initial panes: $INITIAL_PANES" | tee -a "$LOG"
echo "Max panes:     $MAX_PANES" | tee -a "$LOG"
echo "Final panes:   $FINAL_PANES" | tee -a "$LOG"
echo "Panes changed: $PANES_CHANGED" | tee -a "$LOG"

if [ "$PANES_CHANGED" = "true" ]; then
    echo "" | tee -a "$LOG"
    echo "SUCCESS: Agent Teams created new panes!" | tee -a "$LOG"
    tmux list-panes -s -t "$SESSION" -F "  #{pane_id} pid=#{pane_pid} cmd=#{pane_current_command}" | tee -a "$LOG"
else
    echo "" | tee -a "$LOG"
    echo "No new panes created." | tee -a "$LOG"
    echo "Main pane last 20 lines:" | tee -a "$LOG"
    tmux capture-pane -p -t "$SESSION" 2>/dev/null | grep -v '^$' | tail -20 | tee -a "$LOG"
fi
